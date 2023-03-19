use crate::utils::af_utils::*;

use anyhow::{bail, Context, Result};
use serde_json::json;
use serde_json::Value;
use std::io::{BufReader, Seek, Write};
use std::path::PathBuf;
use tracing::info;

use super::Commands;

pub fn add_chemistry(af_home_path: PathBuf, add_chem_cmd: Commands) -> Result<()> {
    match add_chem_cmd {
        Commands::AddChemistry { name, geometry } => {
            // check geometry string, if no good then
            // propagate error.
            let _cg = extract_geometry(&geometry)?;

            // do we have a custom chemistry file
            let custom_chem_p = af_home_path.join("custom_chemistries.json");

            let mut custom_chem_file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(&custom_chem_p)
                .with_context({
                    || {
                        format!(
                            "couldn't open the custom chemistry file {}",
                            custom_chem_p.display()
                        )
                    }
                })?;

            let custom_chem_reader = BufReader::new(&custom_chem_file);
            let mut v: Value = match serde_json::from_reader(custom_chem_reader) {
                Ok(sv) => sv,
                Err(_) => {
                    // the file was empty so here return an empty json object
                    json!({})
                }
            };

            if let Some(g) = v.get_mut(&name) {
                let gs = g.as_str().unwrap();
                info!("chemistry {} already existed, with geometry {}; overwriting geometry specification", name, gs);
                *g = json!(geometry);
            } else {
                info!("inserting chemistry {} with geometry {}", name, geometry);
                v[name] = json!(geometry);
            }

            custom_chem_file.set_len(0)?;
            // custom_chem_file.seek(SeekFrom::Start(0))?;
            // suggested by cargo clippy
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
