use crate::utils::af_utils::{parse_resource_json_file, validate_geometry, ExpectedOri};
use crate::utils::constants::*;
use anyhow::{anyhow, bail, Context, Result};
use semver::Version;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::Path;
use tracing::info;

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

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub struct CustomChemistry {
    pub name: String,
    pub geometry: String,
    pub expected_ori: Option<ExpectedOri>,
    pub version: Option<String>,
    pub plist_name: Option<String>,
    pub remote_pl_url: Option<String>,
    pub meta: Option<Value>,
}

impl QueryInRegistry for CustomChemistry {
    fn registry_key(&self) -> &str {
        self.name()
    }
}

#[allow(dead_code)]
impl CustomChemistry {
    pub fn simple_custom(geometry: &str) -> Result<CustomChemistry> {
        // TODO: once we ensure the geometry must be a valid geometry, we do validation here
        // extract_geometry(geometry)?;
        Ok(CustomChemistry {
            name: geometry.to_string(),
            geometry: geometry.to_string(),
            expected_ori: None,
            version: None,
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

    pub fn expected_ori(&self) -> &Option<ExpectedOri> {
        &self.expected_ori
    }

    pub fn version(&self) -> &Option<String> {
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
                "{}; Please consider delete it from {}",
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
        let cc: CustomChemistry = parse_single_custom_chem_from_value(key, value)?;
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
            if mandatory == FieldType::Mandatory {
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
        _ => Err(anyhow!(
            "Couldn't parse the {} field, {}, to a string for the json object {:#?}",
            key,
            obj.get(key).unwrap(),
            obj
        )),
    }
}

/// Takes a key and value from the top-level custom chemistry JSON object, and returns the
/// CustomChemistry struct corresponding to this key.
/// The value corresponding to this key can be either
///     1. An object having the associated / expected keys
///     2. A string representing the geometry
/// The second case here is legacy from older versions of simpleaf and deprecated, so we should
/// warn by default when we see it.
pub fn parse_single_custom_chem_from_value(key: &str, value: &Value) -> Result<CustomChemistry> {
    match value {
        // deprecated case. Need to warn and return an error
        Value::String(record_v) => {
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
            let geometry = try_get_str_from_json(GEOMETRY_KEY, obj, FieldType::Mandatory, None)?;
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

            let expected_ori = Some(
                ExpectedOri::from_str(&expected_ori)
                .with_context(|| {
                    format!(
                        "Found invalid {} string for the custom chemistry {}: {}. It should be one of {}",
                        EXPECTED_ORI_KEY,
                        key, &expected_ori,
                        ExpectedOri::all_to_str().join(", ")
                    )
                })?
            );

            let version = try_get_str_from_json(
                VERSION_KEY,
                obj,
                FieldType::Optional,
                Some(CustomChemistry::default_version()),
            )?;
            if let Some(version) = &version {
                Version::parse(version).with_context(|| {
                    format!(
                        "Found invalid {} string for the custom chemistry {}: {}",
                        VERSION_KEY,
                        key,
                        &version
                    )
                })?;
            };

            let plist_name =
                try_get_str_from_json(LOCAL_PL_PATH_KEY, obj, FieldType::Optional, None)?;

            let remote_pl_url =
                try_get_str_from_json(REMOTE_PL_URL_KEY, obj, FieldType::Optional, None)?;

            let meta = obj.get(META_KEY).map(|v| v.clone());

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

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FieldType {
    Mandatory,
    Optional,
}

pub fn custom_chem_hm_to_json(custom_chem_hm: &HashMap<String, CustomChemistry>) -> Result<Value> {
    // first create the name to geometry mapping
    let v: Value = custom_chem_hm
        .iter()
        .map(|(k, v)| {
            let mut value = json!({
                GEOMETRY_KEY: v.geometry.clone()
            });
            value[EXPECTED_ORI_KEY] = if let Some(eo) = &v.expected_ori {
                json!(eo.as_str())
            } else {
                info!(
                    "`expected_ori` is missing for custom chemistry {}; Set as {}",
                    k,
                    ExpectedOri::default().as_str()
                );
                json!(ExpectedOri::default().as_str())
            };
            value[VERSION_KEY] = if let Some(ver) = &v.version {
                json!(ver)
            } else {
                info!(
                    "`version` is missing for custom chemistry {}; Set as {}",
                    k,
                    CustomChemistry::default_version()
                );
                json!(CustomChemistry::default_version())
            };
            value[LOCAL_PL_PATH_KEY] = if let Some(lpp) = &v.plist_name {
                json!(lpp)
            } else {
                json!(null)
            };
            value[REMOTE_PL_URL_KEY] = if let Some(rpu) = &v.remote_pl_url {
                json!(rpu)
            } else {
                json!(null)
            };
            (k.clone(), value)
        })
        .collect();

    Ok(v)
}

/// This function tries to extract the custom chemistry with the specified name from the custom_chemistries.json file in the `af_home_path` directory.
pub fn get_single_custom_chem_from_file(
    custom_chem_p: &Path,
    chem_name: &str,
) -> Result<Option<CustomChemistry>> {
    let v: Value = parse_resource_json_file(custom_chem_p, Some(CHEMISTRIES_URL))?;
    if let Some(chem_v) = v.get(chem_name) {
        let custom_chem = parse_single_custom_chem_from_value(chem_name, chem_v)?;
        Ok(Some(custom_chem))
    } else {
        Ok(None)
    }
}
