use crate::utils::af_utils::{extract_geometry, parse_resource_json_file, validate_geometry};
use crate::utils::constants::*;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;
use std::io::Read;
use std::path::Path;
use strum::EnumIter;
use strum::IntoEnumIterator;

// TODO: Change to main repo when we are ready

pub(crate) type CustomChemistryMap = HashMap<String, CustomChemistry>;

static GEOMETRY_KEY: &str = "geometry";
static EXPECTED_ORI_KEY: &str = "expected_ori";

pub(crate) static LOCAL_PL_PATH_KEY: &str = "plist_name";
pub(crate) static REMOTE_PL_URL_KEY: &str = "remote_url";

pub trait QueryInRegistry {
    fn registry_key(&self) -> &str;
}

/// Represents the expected orientation for a chemistry; the
/// orientation in which the fragment is expected to map.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, EnumIter)]
pub enum ExpectedOri {
    #[serde(rename = "fw")]
    Forward,
    #[serde(rename = "rc")]
    Reverse,
    #[serde(rename = "both")]
    Both,
}

impl std::fmt::Display for ExpectedOri {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl ExpectedOri {
    pub fn default() -> ExpectedOri {
        ExpectedOri::Both
    }

    /// convert an `ExpectedOri` to a string
    pub fn as_str(&self) -> &str {
        match self {
            ExpectedOri::Forward => "fw",
            ExpectedOri::Reverse => "rc",
            ExpectedOri::Both => "both",
        }
    }

    // construct the `ExpectedOri` from a str
    pub fn from_str(s: &str) -> Result<ExpectedOri> {
        match s {
            "fw" => Ok(ExpectedOri::Forward),
            "rc" => Ok(ExpectedOri::Reverse),
            "both" => Ok(ExpectedOri::Both),
            _ => Err(anyhow!("Invalid expected_ori value: {}", s)),
        }
    }

    /// Return a vector of all of the string representations of
    /// ExpectedOris
    pub fn all_to_str() -> Vec<String> {
        ExpectedOri::iter()
            .map(|v| v.to_string())
            .collect::<Vec<String>>()
    }
}

/// A CustomChemistry is a description of a chemistry that is not
/// covered under the different built-in chemistries.  It defines the
/// relevant information about how a chemistry should be defined including
/// the name, geometry string, potential permit list etc.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CustomChemistry {
    #[serde(default, skip_serializing)]
    pub name: String,
    pub geometry: String,
    #[serde(default = "ExpectedOri::default")]
    pub expected_ori: ExpectedOri,
    #[serde(default = "CustomChemistry::default_version")]
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plist_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_pl_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// The key to use to query a custom chemistry
/// in the registry.
impl QueryInRegistry for CustomChemistry {
    fn registry_key(&self) -> &str {
        self.name()
    }
}

impl CustomChemistry {
    pub fn simple_custom(geometry: &str) -> Result<CustomChemistry> {
        extract_geometry(geometry)?;
        Ok(CustomChemistry {
            name: geometry.to_string(),
            geometry: geometry.to_string(),
            expected_ori: ExpectedOri::default(),
            version: CustomChemistry::default_version(),
            plist_name: None,
            remote_pl_url: None,
            meta: None,
        })
    }
    pub fn geometry(&self) -> &str {
        self.geometry.as_str()
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn expected_ori(&self) -> &ExpectedOri {
        &self.expected_ori
    }

    pub fn version(&self) -> &String {
        &self.version
    }

    pub fn default_version() -> String {
        String::from("0.0.0")
    }

    pub fn plist_name(&self) -> &Option<String> {
        &self.plist_name
    }

    pub fn remote_pl_url(&self) -> &Option<String> {
        &self.remote_pl_url
    }

    pub fn meta(&self) -> &Option<Value> {
        &self.meta
    }
}

impl fmt::Display for CustomChemistry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "chemistry name\t: {}", self.name())?;
        writeln!(f, "{}\t: {}", GEOMETRY_KEY, self.geometry())?;
        writeln!(f, "{}\t: {}", EXPECTED_ORI_KEY, self.expected_ori())?;
        if let Some(plist_name) = self.plist_name() {
            writeln!(f, "{}\t: {}", LOCAL_PL_PATH_KEY, plist_name)?;
        }
        if let Some(remote_pl_url) = self.remote_pl_url() {
            writeln!(f, "{}\t: {}", REMOTE_PL_URL_KEY, remote_pl_url)?;
        }

        if let Some(serde_json::Value::Object(meta)) = self.meta() {
            if !meta.is_empty() {
                writeln!(f, "meta\t: {{")?;
                for (k, v) in meta.iter() {
                    writeln!(f, "  {}\t: {:#}", k, v)?;
                }
                writeln!(f, "}}")?;
            }
        }
        Ok(())
    }
}

/// Allow obtaining a `serde_json::Value` from a `CustomChemistry`
/// and allow converting a `CustomChemistry` into a `serde_json::Value`.
impl From<CustomChemistry> for Value {
    fn from(cc: CustomChemistry) -> Value {
        serde_json::to_value(cc)
            .expect("Valid chemistry should always be convertible to JSON value")
    }
}

/// This function gets the custom chemistry from the `af_home_path` directory.
/// If the file doesn't exist, it downloads the file from the `url` and saves it
pub fn get_custom_chem_hm(custom_chem_p: &Path) -> Result<HashMap<String, CustomChemistry>> {
    let mut chem_hm: HashMap<String, CustomChemistry> = serde_json::from_value(
        parse_resource_json_file(custom_chem_p, Some(CHEMISTRIES_URL))?,
    )?;
    for (k, v) in chem_hm.iter_mut() {
        validate_geometry(&v.geometry)?;
        v.name = k.clone();
    }
    Ok(chem_hm)
}

/// convert the custom chemistry hashmap into a `serde_json::Value`
pub fn custom_chem_hm_into_json(custom_chem_hm: HashMap<String, CustomChemistry>) -> Result<Value> {
    let v = serde_json::to_value(custom_chem_hm)?;
    Ok(v)
}

/// This function tries to extract the custom chemistry with the specified name from the provided
/// reader
#[allow(dead_code)]
pub fn get_single_custom_chem_from_reader(
    reader: impl Read,
    key: &str,
) -> Result<Option<CustomChemistry>> {
    let chem_hm: HashMap<String, CustomChemistry> = serde_json::from_reader(reader)?;
    if let Some(chem_v) = chem_hm.get(key) {
        let mut custom_chem = chem_v.clone();
        custom_chem.name = key.to_owned();
        Ok(Some(custom_chem))
    } else {
        Ok(None)
    }
}

/// This function tries to extract the custom chemistry with the specified name from the custom_chemistries.json file in the `af_home_path` directory.
pub fn get_single_custom_chem_from_file(
    custom_chem_p: &Path,
    key: &str,
) -> Result<Option<CustomChemistry>> {
    let chem_hm: HashMap<String, CustomChemistry> = serde_json::from_value(
        parse_resource_json_file(custom_chem_p, Some(CHEMISTRIES_URL))?,
    )?;
    if let Some(chem_v) = chem_hm.get(key) {
        let mut custom_chem = chem_v.clone();
        custom_chem.name = key.to_owned();
        Ok(Some(custom_chem))
    } else {
        Ok(None)
    }
}
