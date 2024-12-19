use crate::atac::commands::AtacChemistry;
use crate::utils::constants::CHEMISTRIES_PATH;
use crate::utils::{
    af_utils::RnaChemistry,
    chem_utils::{custom_chem_hm_to_json, get_custom_chem_hm},
    prog_utils::*,
};
use anyhow::Result;
use serde_json::{json, Value};
use std::path::PathBuf;
use strum::IntoEnumIterator;
use tracing::warn;

pub fn inspect_simpleaf(version: &str, af_home_path: PathBuf) -> Result<()> {
    // Read the JSON contents of the file as an instance of `User`.
    let simpleaf_info: Value = inspect_af_home(af_home_path.as_path())?;
    // do we have a custom chemistry file
    let custom_chem_p = af_home_path.join(CHEMISTRIES_PATH);
    let chem_info_value = if custom_chem_p.is_file() {
        // parse the chemistry json file
        let custom_chem_hm = get_custom_chem_hm(&custom_chem_p)?;
        let v = custom_chem_hm_to_json(&custom_chem_hm)?;
        json!({
            "custom_chem_path" : custom_chem_p.display().to_string(),
            "custom_geometries" : v
        })
    } else {
        warn!(
            r#"
            You are missing a "chemistries.json" file from your ALEVIN_FRY_HOME. This 
            likely means you installed a new version of simpleaf and have not yet run 
            simpleaf chem refresh
            please invoke the `chem refresh` command to obtain the relevant chemistries.json
            file from upstream.
            "#
        );
        json!("")
    };

    let rna_chem_list = RnaChemistry::iter()
        .map(|c| format!("{:?}", c))
        .collect::<Vec<String>>();
    let atac_chem_list = AtacChemistry::iter()
        .map(|x| format!("{:?}", x))
        .collect::<Vec<String>>();

    let inspect_v = json!({
        "simpleaf_version" : version,
        "simpleaf_info" : simpleaf_info,
        "custom_chem_info" : chem_info_value,
        "builtin_chemistries" : {
            "rna" : rna_chem_list,
            "atac" : atac_chem_list,
        }
    });
    println!("{}", serde_json::to_string_pretty(&inspect_v)?);
    Ok(())
}
