use crate::utils::af_utils::*;
use crate::utils::chem_utils::{custom_chem_hm_to_json, get_custom_chem_hm, CustomChemistry};
use crate::utils::constants::*;
use crate::utils::prog_utils;

use anyhow::{anyhow, bail, Context, Result};
use semver::Version;
use serde_json::json;
use std::io::{Seek, Write};
use std::path::PathBuf;
use tracing::{info, warn};

use super::Commands;

pub fn add_chemistry(af_home_path: PathBuf, add_chem_cmd: Commands) -> Result<()> {
    match add_chem_cmd {
        Commands::AddChemistry {
            name,
            geometry,
            expected_ori,
            local_pl_path,
            remote_pl_url,
            version,
        } => {
            // check geometry string, if no good then
            // propagate error.
            extract_geometry(&geometry)?;
            Version::parse(version.as_ref()).with_context(|| format!("could not parse version {}. Please follow https://semver.org/. A valid example is 0.1.0", version))?;

            // init the custom chemistry struct
            let custom_chem = CustomChemistry {
                name,
                geometry,
                expected_ori: Some(ExpectedOri::from_str(&expected_ori)?),
                local_pl_path,
                remote_pl_url,
                version: None,
            };

            // read in the custom chemistry file
            let custom_chem_p = af_home_path.join("custom_chemistries.json");

            let mut custom_chem_hm = get_custom_chem_hm(&custom_chem_p)?;

            // check if the chemistry already exists and log
            if let Some(cc) = custom_chem_hm.get(custom_chem.name()) {
                info!("chemistry {} already existed, with geometry {} the one recorded: {}; overwriting geometry specification", custom_chem.name(), if cc.geometry() == custom_chem.geometry() {"same as"} else {"different with"}, cc.geometry());
                custom_chem_hm
                    .entry(custom_chem.name().to_string())
                    .and_modify(|e| *e = custom_chem);
            } else {
                info!(
                    "inserting chemistry {} with geometry {}",
                    custom_chem.name(),
                    custom_chem.geometry()
                );
                custom_chem_hm.insert(custom_chem.name().to_string(), custom_chem);
            }

            // convert the custom chemistry hashmap to json
            let v = custom_chem_hm_to_json(&custom_chem_hm)?;

            // write out the new custom chemistry file
            let mut custom_chem_file = std::fs::File::create(&custom_chem_p)
                .with_context(|| format!("could not create {}", custom_chem_p.display()))?;
            custom_chem_file.rewind()?;

            custom_chem_file
                .write_all(serde_json::to_string_pretty(&v).unwrap().as_bytes())
                .with_context(|| format!("could not write {}", custom_chem_p.display()))?;
        }
        _ => {
            bail!("unknown command");
        }
    }
    Ok(())
}

pub fn refresh_chemistries(af_home: PathBuf) -> Result<()> {
    // if the old custom chem file exists, then warn the user about it
    // but read it in and attempt to populate.
    let custom_chem_file = af_home.join(CUSTOM_CHEMISTRIES_PATH);
    let merge_custom_chem = if custom_chem_file.exists() {
        warn!("The \"custom_chemistries.json\" file is deprecated, and in the future, these chemistries should be 
        regustered in the \"chemistries.json\" file instead. We will attempt to automatically migrate over the old 
        chemistries into the new file");
        true
    } else {
        false
    };

    // check if the chemistry file is absent altogether
    // if so, then download it
    let chem_path = af_home.join(CHEMISTRIES_PATH);
    if !chem_path.is_file() {
        prog_utils::download_to_file(CHEMISTRIES_URL, &chem_path)?;
    } else {
        let tmp_chem_path = af_home.join(CHEMISTRIES_PATH).with_extension("tmp.json");
        prog_utils::download_to_file(CHEMISTRIES_URL, &tmp_chem_path)?;
        if let Some(existing_chem) = parse_resource_json_file(&chem_path, None)?.as_object_mut() {
            if let Some(new_chem) = parse_resource_json_file(&tmp_chem_path, None)?.as_object() {
                for (k, v) in new_chem.iter() {
                    match existing_chem.get_mut(k) {
                        None => {
                            existing_chem.insert(k.clone(), v.clone());
                        }
                        Some(ev) => {
                            let curr_ver = Version::parse(
                                ev.get("version")
                                    .expect("chemistry should have a version field")
                                    .as_str()
                                    .expect("version should be a string"),
                            )?;
                            let new_ver = Version::parse(
                                v.get("version")
                                    .expect("chemistry should have a version field")
                                    .as_str()
                                    .expect("version should be a string"),
                            )?;
                            if new_ver > curr_ver {
                                existing_chem.insert(k.clone(), v.clone());
                            }
                        }
                    }
                }

                // write out the merged chemistry file
                let mut chem_file = std::fs::File::create(&chem_path)
                    .with_context(|| format!("could not create {}", chem_path.display()))?;
                chem_file.rewind()?;

                chem_file
                    .write_all(serde_json::to_string_pretty(&new_chem).unwrap().as_bytes())
                    .with_context(|| format!("could not write {}", chem_path.display()))?;

                // remove the temp file
                std::fs::remove_file(tmp_chem_path)?;
            } else {
                bail!("Could not parse newly downloaded \"chemistries.json\" file as a JSON object, something is wrong. Please report this on GitHub.");
            }
        } else {
            bail!("Could not parse existing \"chemistries.json\" file as a JSON object, something is wrong. Please report this on GitHub.");
        }
    }

    if merge_custom_chem {
        if let Some(new_chem) = parse_resource_json_file(&chem_path, None)?.as_object_mut() {
            if let Some(old_custom_chem) =
                parse_resource_json_file(&custom_chem_file, None)?.as_object()
            {
                for (k, v) in old_custom_chem.iter() {
                    if new_chem.contains_key(k) {
                        warn!("The newly downloaded \"chemistries.json\" file already contained the key {}, skipping entry from the existing \"custom_chemistries.json\" file.", k);
                    } else {
                        let new_ent = json!({
                            "geometry": v,
                            "expected_ori": "both",
                            "version" : "0.1.0"
                        });
                        new_chem.insert(k.to_owned(), new_ent);
                        info!("successfully inserted {} from old custom chemistries file into the new chemistries registry", k);
                    }
                }

                // write out the merged chemistry file
                let mut chem_file = std::fs::File::create(&chem_path)
                    .with_context(|| format!("could not create {}", chem_path.display()))?;
                chem_file.rewind()?;

                chem_file
                    .write_all(serde_json::to_string_pretty(&new_chem).unwrap().as_bytes())
                    .with_context(|| format!("could not write {}", chem_path.display()))?;

                let backup = custom_chem_file.with_extension("json.bak");
                std::fs::rename(custom_chem_file, backup)?;
            } else {
                bail!("Could not parse existing \"custom_chemistries.json\" file as a JSON object; it may be corrupted. Consider deleting this file.");
            }
        } else {
            bail!("Could not parse newly downloaded \"chemistries.json\" file as a JSON object, something is wrong. Please report this on GitHub.");
        }
    }
    Ok(())
}
