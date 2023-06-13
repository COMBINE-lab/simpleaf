use crate::utils::prog_utils::*;

use anyhow::{bail, Context};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use tracing::info;

use super::Commands;

pub fn set_paths(af_home_path: PathBuf, set_path_args: Commands) -> anyhow::Result<()> {
    const AF_HOME: &str = "ALEVIN_FRY_HOME";
    match set_path_args {
        Commands::SetPaths {
            salmon,
            piscem,
            alevin_fry,
        } => {
            // create AF_HOME if needed
            if !af_home_path.as_path().is_dir() {
                info!(
                    "The {} directory, {}, doesn't exist, creating...",
                    AF_HOME,
                    af_home_path.display()
                );
                fs::create_dir_all(af_home_path.as_path())?;
            }

            let rp = get_required_progs_from_paths(salmon, piscem, alevin_fry)?;

            let have_mapper = rp.salmon.is_some() || rp.piscem.is_some();
            if !have_mapper {
                bail!("Suitable executable for piscem or salmon not found — at least one of these must be available.");
            }
            if rp.alevin_fry.is_none() {
                bail!("Suitable alevin_fry executable not found.");
            }

            let simpleaf_info_file = af_home_path.join("simpleaf_info.json");
            let simpleaf_info = json!({ "prog_info": rp });

            std::fs::write(
                &simpleaf_info_file,
                serde_json::to_string_pretty(&simpleaf_info).unwrap(),
            )
            .with_context(|| format!("could not write {}", simpleaf_info_file.display()))?;
        }
        _ => {
            bail!("unexpected command")
        }
    }
    Ok(())
}
