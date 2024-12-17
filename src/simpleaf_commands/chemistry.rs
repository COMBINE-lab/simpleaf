use crate::utils::af_utils::*;

use anyhow::{bail, Context, Result};
use std::io::{Seek, Write};
use std::path::PathBuf;
use tracing::info;
use semver::Version;

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
                name: name.clone(),
                geometry: geometry.clone(),
                expected_ori: Some(ExpectedOri::from_str(&expected_ori)?),
                local_pl_path: local_pl_path.clone(),
                remote_pl_url: remote_pl_url.clone(),
                version: None
            };

            // read in the custom chemistry file
            let custom_chem_p = af_home_path.join("custom_chemistries.json");

            let mut custom_chem_hm = get_custom_chem_hm(&custom_chem_p)?;

            // check if the chemistry already exists and log
            if let Some(cc) = custom_chem_hm.get(&name) {
                info!("chemistry {} already existed, with geometry {}; overwriting geometry specification", name, cc.geometry());
                custom_chem_hm
                    .entry(name.clone())
                    .and_modify(|e| *e = custom_chem);
            } else {
                info!("inserting chemistry {} with geometry {}", name, geometry);
                custom_chem_hm.insert(name.clone(), custom_chem);
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
