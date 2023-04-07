use crate::utils::prog_utils::*;

use anyhow::{Context, Result};
use serde_json::Value;
use std::io::BufReader;
use std::path::PathBuf;

pub fn inspect_simpleaf(af_home_path: PathBuf) -> Result<()> {
    // Read the JSON contents of the file as an instance of `User`.
    let v: Value = inspect_af_home(af_home_path.as_path())?;
    println!(
        "\n----- simpleaf info -----\n{}",
        serde_json::to_string_pretty(&v).unwrap()
    );

    // do we have a custom chemistry file
    let custom_chem_p = af_home_path.join("custom_chemistries.json");
    if custom_chem_p.is_file() {
        println!(
            "\nCustom chemistries exist at path : {}\n----- custom chemistries -----\n",
            custom_chem_p.display()
        );
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
        println!("{}", serde_json::to_string_pretty(&v).unwrap());
    }
    Ok(())
}
