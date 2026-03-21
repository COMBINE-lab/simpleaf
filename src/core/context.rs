use std::path::Path;

use anyhow::Context;
use serde_json::Value;

use crate::utils::prog_utils::{self, ReqProgs};

pub struct RuntimeContext {
    pub progs: ReqProgs,
}

pub fn load_required_programs(af_home_path: &Path) -> anyhow::Result<ReqProgs> {
    let v: Value = prog_utils::inspect_af_home(af_home_path)?;
    serde_json::from_value(v["prog_info"].clone())
        .with_context(|| "Could not deserialize required program metadata from simpleaf_info.json")
}

pub fn load_runtime_context(af_home_path: &Path) -> anyhow::Result<RuntimeContext> {
    Ok(RuntimeContext {
        progs: load_required_programs(af_home_path)?,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::tempdir;

    use super::load_runtime_context;

    #[test]
    fn load_runtime_context_reads_prog_info() {
        let td = tempdir().expect("failed to create tempdir");
        let af_info = json!({
            "prog_info": {
                "piscem": {"exe_path": "/bin/echo", "version": "0.12.2"},
                "alevin_fry": {"exe_path": "/bin/echo", "version": "0.11.2"},
                "macs": null
            }
        });
        fs::write(
            td.path().join("simpleaf_info.json"),
            serde_json::to_string_pretty(&af_info).expect("failed to serialize json"),
        )
        .expect("failed to write simpleaf_info.json");

        let ctx = load_runtime_context(td.path()).expect("failed to load runtime context");
        assert!(ctx.progs.piscem.is_some());
        assert!(ctx.progs.alevin_fry.is_some());
    }
}
