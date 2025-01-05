use crate::utils::prog_utils::*;

use anyhow::Context;
use serde_json::{json, Value};
use std::path::PathBuf;

pub fn refresh_prog_info(af_home_path: PathBuf) -> anyhow::Result<()> {
    // Read the JSON contents of the file as an instance of `User`.
    let v: Value = inspect_af_home(af_home_path.as_path())?;
    let current_rp: ReqProgs = serde_json::from_value(v["prog_info"].clone())?;

    let new_rp = get_required_progs_from_paths(
        current_rp.salmon.map(|p| p.exe_path),
        current_rp.piscem.map(|p| p.exe_path),
        current_rp.alevin_fry.map(|p| p.exe_path),
        current_rp.macs.map(|p| p.exe_path),
    )?;

    let simpleaf_info_file = af_home_path.join("simpleaf_info.json");
    let simpleaf_info = json!({ "prog_info": new_rp });

    std::fs::write(
        &simpleaf_info_file,
        serde_json::to_string_pretty(&simpleaf_info).unwrap(),
    )
    .with_context(|| format!("could not write {}", simpleaf_info_file.display()))?;
    Ok(())
}
