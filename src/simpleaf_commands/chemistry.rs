use crate::utils::af_utils::*;

use anyhow::{bail, Context, Result};
use std::io::{Seek, Write};
use std::path::PathBuf;
use tracing::info;

use super::Commands;

pub fn add_chemistry(af_home_path: PathBuf, add_chem_cmd: Commands) -> Result<()> {
    match add_chem_cmd {
        Commands::AddChemistry {
            name,
            geometry,
            expected_ori,
        } => {
            // check geometry string, if no good then
            // propagate error.
            let _cg = extract_geometry(&geometry)?;

            // cannot use expected_ori as the name
            if &name == "expected_ori" {
                bail!("The name 'expected_ori' is reserved for the expected orientation of the molecule; Please choose another name");
            }

            // init the custom chemistry struct
            let custom_chem = CustomChemistry {
                name: name.clone(),
                geometry: geometry.clone(),
                expected_ori: ExpectedOri::from_str(&expected_ori)?,
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
