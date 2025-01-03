use crate::utils::af_utils::{extract_geometry, parse_resource_json_file, validate_geometry};
use crate::utils::constants::*;
use anyhow::{anyhow, bail, Context, Result};
use semver::Version;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use strum::EnumIter;
use strum::IntoEnumIterator;
use tracing::warn;

// TODO: Change to main repo when we are ready

static GEOMETRY_KEY: &str = "geometry";
static EXPECTED_ORI_KEY: &str = "expected_ori";
static VERSION_KEY: &str = "version";
static META_KEY: &str = "meta";

pub(crate) static LOCAL_PL_PATH_KEY: &str = "plist_name";
pub(crate) static REMOTE_PL_URL_KEY: &str = "remote_url";

pub trait QueryInRegistry {
    fn registry_key(&self) -> &str;
}

/// Represents the expected orientation for a chemistry; the
/// orientation in which the fragment is expected to map.
#[derive(Debug, Clone, PartialEq, EnumIter)]
pub enum ExpectedOri {
    Forward,
    Reverse,
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
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub struct CustomChemistry {
    pub name: String,
    pub geometry: String,
    pub expected_ori: ExpectedOri,
    pub version: String,
    pub plist_name: Option<String>,
    pub remote_pl_url: Option<String>,
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
        let mut value = json!({
            GEOMETRY_KEY: cc.geometry
        });
        value[EXPECTED_ORI_KEY] = json!(cc.expected_ori.as_str());
        value[VERSION_KEY] = json!(cc.version);
        value[LOCAL_PL_PATH_KEY] = if let Some(lpp) = cc.plist_name {
            json!(lpp)
        } else {
            json!(null)
        };
        value[REMOTE_PL_URL_KEY] = if let Some(rpu) = cc.remote_pl_url {
            json!(rpu)
        } else {
            json!(null)
        };
        if let Some(meta) = cc.meta {
            value[META_KEY] = meta;
        }
        value
    }
}

// IO
impl CustomChemistry {
    /// Parse the value that corresponds to a key in the top-level custom chemistry JSON object.
    /// The key is ONLY used for error messages and assigning the name field of the CustomChemistry struct.
    /// The value must be an json value object with a valid geometry field that can be parsed into a CustomChemistry struct.
    pub fn from_value(key: &str, value: &Value) -> Result<CustomChemistry> {
        match value {
            // deprecated case. Need to warn and return an error
            Value::String(record_v) => {
                warn!("The geometry entry for {} was a string rather than an object. String values for geometry keys are deprecated and this should not happen!.", key);
                match validate_geometry(record_v) {
                    Ok(_) => Err(anyhow!(
                        "Found string version of custom chemistry {}: {}. This is deprecated. Please add the chemistry again using simpleaf chem add.",
                        key,
                        record_v
                    )),
                    Err(_) => Err(anyhow!(
                        "Found invalid custom chemistry record for {}: {}",
                        key,
                        record_v
                    )),
                }
            }

            Value::Object(obj) => {
                let geometry =
                    try_get_str_from_json(GEOMETRY_KEY, obj, FieldType::Mandatory, None)?;

                let geometry = geometry.unwrap(); // we made this Some, safe to unwrap
                                                  // check if geometry is valid
                validate_geometry(&geometry)?;

                let expected_ori = try_get_str_from_json(
                    EXPECTED_ORI_KEY,
                    obj,
                    FieldType::Optional,
                    Some(ExpectedOri::default().to_string()),
                )?
                .unwrap(); // we made this Some, safe to unwrap

                let expected_ori =
                    ExpectedOri::from_str(&expected_ori)
                    .with_context(|| {
                        format!(
                            "Found invalid {} string for the custom chemistry {}: {}. It should be one of {}",
                            EXPECTED_ORI_KEY,
                            key, &expected_ori,
                            ExpectedOri::all_to_str().join(", ")
                        )
                    })?;

                let version = try_get_str_from_json(
                    VERSION_KEY,
                    obj,
                    FieldType::Optional,
                    Some(CustomChemistry::default_version()),
                )?
                .unwrap(); // we made this Some, safe to unwrap

                Version::parse(&version).with_context(|| {
                    format!(
                        "Found invalid {} string for the custom chemistry {}: {}",
                        VERSION_KEY, key, &version
                    )
                })?;

                let plist_name =
                    try_get_str_from_json(LOCAL_PL_PATH_KEY, obj, FieldType::Optional, None)?;

                let remote_pl_url =
                    try_get_str_from_json(REMOTE_PL_URL_KEY, obj, FieldType::Optional, None)?;

                let meta = obj.get(META_KEY).cloned();

                Ok(CustomChemistry {
                    name: key.to_string(),
                    geometry,
                    expected_ori,
                    version,
                    plist_name,
                    remote_pl_url,
                    meta,
                })
            }
            _ => Err(anyhow!(
                "Found invalid custom chemistry record for {}: {}.",
                key,
                value
            )),
        }
    }
}

/// This function gets the custom chemistry from the `af_home_path` directory.
/// If the file doesn't exist, it downloads the file from the `url` and saves it
pub fn get_custom_chem_hm(custom_chem_p: &Path) -> Result<HashMap<String, CustomChemistry>> {
    let v: Value = parse_resource_json_file(custom_chem_p, Some(CHEMISTRIES_URL))?;
    let chem_hm = get_custom_chem_hm_from_value(v);
    match chem_hm {
        Ok(hm) => Ok(hm),
        Err(e) => {
            bail!(
                "{}; \
                Please consider delete it from {}",
                e,
                custom_chem_p.display()
            );
        }
    }
}

/// This function gets the custom chemistry from the custom_chemistries.json file in the `af_home_path` directory.
/// We need to ensure back compatibility with the old version of the custom_chemistries.json file.
/// In the old version, each key of `v` is associated with a string field recording the geometry.
/// In the new version, each key of `v` is associated with a json object with two fields: `geometry`, `expected_ori`, `version`, `plist_name`, and "remote_pl_url".
pub fn get_custom_chem_hm_from_value(v: Value) -> Result<HashMap<String, CustomChemistry>> {
    // the top-level value should be an object
    let v_obj = v.as_object().with_context(|| {
        format!("Couldn't parse the existing custom chemistry json file: {}. The top-level JSON value should be an object", v)
    })?;

    // Then we go over the keys and values and create a hashmap
    let mut custom_chem_map = HashMap::with_capacity(v_obj.len());

    // we build the hashmap
    for (key, value) in v_obj.iter() {
        let cc: CustomChemistry = CustomChemistry::from_value(key, value)?;
        custom_chem_map.insert(key.clone(), cc);
    }

    Ok(custom_chem_map)
}

/// This function tries to extract a string from a json object
/// if it is a mandatory field, it will return an error if it is missing
/// if it is an optional field, it will return a default value if it is missing
pub fn try_get_str_from_json(
    key: &str,
    obj: &Map<String, Value>,
    mandatory: FieldType,
    default: Option<String>,
) -> Result<Option<String>> {
    match obj.get(key) {
        // if we get a null, if mandatory, return an error, if optional, return the default
        Some(Value::Null) | None => {
            if default.is_none() && mandatory.is_mandatory() {
                Err(anyhow!(
                    "The mandatory field {} is null in the json object {:#?}",
                    key,
                    obj
                ))
            } else {
                Ok(default)
            }
        }
        Some(Value::String(s)) => Ok(Some(s.to_string())),
        v => Err(anyhow!(
            "Couldn't parse the {} field, {:#?}, to a string for the json object {:#?}",
            key,
            v,
            obj
        )),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FieldType {
    Mandatory,
    Optional,
}

impl FieldType {
    pub fn is_mandatory(&self) -> bool {
        match self {
            FieldType::Mandatory => true,
            FieldType::Optional => false,
        }
    }
}

pub fn custom_chem_hm_into_json(custom_chem_hm: HashMap<String, CustomChemistry>) -> Result<Value> {
    // first create the name to geometry mapping
    let v: Value = custom_chem_hm
        .into_iter()
        .map(|(k, v)| {
            let value: Value = v.into();
            (k, value)
        })
        .collect();

    Ok(v)
}

/// This function tries to extract the custom chemistry with the specified name from the custom_chemistries.json file in the `af_home_path` directory.
pub fn get_single_custom_chem_from_file(
    custom_chem_p: &Path,
    key: &str,
) -> Result<Option<CustomChemistry>> {
    let v: Value = parse_resource_json_file(custom_chem_p, Some(CHEMISTRIES_URL))?;
    if let Some(chem_v) = v.get(key) {
        let custom_chem = CustomChemistry::from_value(key, chem_v)?;
        Ok(Some(custom_chem))
    } else {
        Ok(None)
    }
}
