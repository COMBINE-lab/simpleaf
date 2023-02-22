use anyhow::Context;
use serde::{Deserialize};
use std::collections::HashMap;
use std::path::PathBuf;
// use crate::check_version_constraints;

// Each record contains a json
#[derive(Deserialize, Debug)]
pub struct WorkflowJsonRecord {
    pub json_type: String,        // has to be "Simpleaf Workflow"
    pub simpleaf_version: String, // used for version control

    // each index and quant record is the pair of
    // the name of the simpleaf run (String), for example "HTO index",
    // and the run information.
    // As the values can be string, boolean and number,
    // here I will treat them as serde_json values
    pub index: Option<HashMap<String, HashMap<String, serde_json::Value>>>,
    pub quant: Option<HashMap<String, HashMap<String, serde_json::Value>>>,
}

impl WorkflowJsonRecord {
    pub fn validate(&self) {
        assert_eq!(self.json_type, String::from("Simpleaf Workflow"), "Invalid JSON file; Please make sure the json_type field is `Simpleaf Workflow`");

        // TODO: check simpleaf version
    }
}

pub fn read_workflow_json(json_path: &PathBuf) -> anyhow::Result<WorkflowJsonRecord> {
    let json_file = std::fs::File::open(json_path)
        .with_context(|| format!("Could not open JSON file {}.", json_path.display()))?;
    let v: WorkflowJsonRecord = serde_json::from_reader(json_file)?;
    v.validate();
    Ok(v)
}

#[derive(Deserialize, Debug)]
enum SimpleafProgram {
    Quant,
    Index,
}

// impl FromStr for SimpleafProgram {
//     type Err = anyhow::Error;
//     fn from_str(input: &str) -> Result<SimpleafProgram, anyhow::Error> {
//         match input {
//             "index"  => Ok(SimpleafProgram::Quant),
//             "quant"  => Ok(SimpleafProgram::Quant),
//             _      => Err(anyhow!("Found invalid commands; Simpleaf Workflow currently can take only `simpleaf index` and `simpleaf quant` commands")),
//         }
//     }
// }