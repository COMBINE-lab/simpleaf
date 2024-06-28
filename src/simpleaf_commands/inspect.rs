use crate::utils::{af_utils::Chemistry, prog_utils::*};
use strum::IntoEnumIterator;

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::io::BufReader;
use std::path::PathBuf;

pub fn inspect_simpleaf(version: &str, af_home_path: PathBuf) -> Result<()> {
    // Read the JSON contents of the file as an instance of `User`.
    let v: Value = inspect_af_home(af_home_path.as_path())?;
    // do we have a custom chemistry file
    let custom_chem_p = af_home_path.join("custom_chemistries.json");
    let chem_info_value = if custom_chem_p.is_file() {
        // parse the custom chemistry json file
        let custom_chem_file = std::fs::File::open(&custom_chem_p).with_context({
            || {
                format!(
                    "couldn't open the custom chemistry file {}",
                    custom_chem_p.display()
                )
            }
        })?;
        let custom_chem_reader = BufReader::new(custom_chem_file);
        let v: Value = serde_json::from_reader(custom_chem_reader)?;
        json!({
            "custom_chem_path" : custom_chem_p.display().to_string(),
            "custom_geometries" : v
        })
    } else {
        json!("")
    };

    let chem_list = Chemistry::iter()
        .map(|c| format!("{:?}", c))
        .collect::<Vec<String>>()
        .join(", ");

    let inspect_v = json!({
        "simpleaf_version" : version,
        "simpleaf_info" : v,
        "custom_chem_info" : chem_info_value,
        "builtin_chemistries" : chem_list
    });
    println!("{}", serde_json::to_string_pretty(&inspect_v)?);
    Ok(())
}
