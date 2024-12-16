use crate::atac::commands::AtacChemistry;
use crate::utils::{
    af_utils::{custom_chem_hm_to_json, get_custom_chem_hm, RnaChemistry},
    prog_utils::*,
};
use strum::IntoEnumIterator;

use anyhow::Result;
use serde_json::{json, Value};
use std::path::PathBuf;

pub fn inspect_simpleaf(version: &str, af_home_path: PathBuf) -> Result<()> {
    // Read the JSON contents of the file as an instance of `User`.
    let v: Value = inspect_af_home(af_home_path.as_path())?;
    // do we have a custom chemistry file
    let custom_chem_p = af_home_path.join("custom_chemistries.json");
    let chem_info_value = if custom_chem_p.is_file() {
        // parse the custom chemistry json file
        let custom_chem_hm = get_custom_chem_hm(&custom_chem_p)?;
        let v = custom_chem_hm_to_json(&custom_chem_hm)?;
        json!({
            "custom_chem_path" : custom_chem_p.display().to_string(),
            "custom_geometries" : v
        })
    } else {
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
        "simpleaf_info" : v,
        "custom_chem_info" : chem_info_value,
        "builtin_chemistries" : {
            "rna" : rna_chem_list,
            "atac" : atac_chem_list,
        }
    });
    println!("{}", serde_json::to_string_pretty(&inspect_v)?);
    Ok(())
}
