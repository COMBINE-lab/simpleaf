use anyhow::anyhow;
use std::path::Path;

pub fn construct_json_from_piscem_log<P: AsRef<Path>>(
    log_path: P,
) -> anyhow::Result<serde_json::Value> {
    let file = std::fs::File::open(log_path.as_ref())?;
    let reader = std::io::BufReader::new(file);
    let mut log_val: serde_json::Value = serde_json::from_reader(reader)?;
    let log = log_val
        .as_object_mut()
        .ok_or(anyhow!("piscem mapping log should be a valid JSON object"))?;
    log.insert(
        "mapper".to_string(),
        serde_json::Value::String("piscem".to_string()),
    );
    Ok(log_val)
}

/// The entries of alevin-fry's `quant.json` (in `quant_dir`) that describe the
/// optional quant outputs requested with `--dump-molecules` and `--probe-mtx`,
/// for simpleaf's run information. `None` when neither was requested. Entries
/// alevin-fry did not write are left out, and a `quant.json` that cannot be
/// read only costs these entries, with a warning: quant itself succeeded.
pub fn requested_quant_outputs(
    quant_dir: &Path,
    dump_molecules: bool,
    probe_mtx: bool,
) -> Option<serde_json::Value> {
    let mut keys = Vec::new();
    if dump_molecules {
        keys.push("molecule_table");
    }
    if probe_mtx {
        keys.extend(["probe_mtx", "num_probes", "probe_nnz"]);
    }
    if keys.is_empty() {
        return None;
    }

    let quant_json = quant_dir.join("quant.json");
    // Read in one go: on Flex data quant.json runs to megabytes.
    let parsed = std::fs::read(&quant_json)
        .map_err(anyhow::Error::from)
        .and_then(|bytes| Ok(serde_json::from_slice::<serde_json::Value>(&bytes)?));
    let parsed = match parsed {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                "could not read {} to record the requested quant outputs: {}",
                quant_json.display(),
                e
            );
            return None;
        }
    };
    let outputs: serde_json::Map<String, serde_json::Value> = keys
        .into_iter()
        .filter_map(|key| parsed.get(key).map(|v| (key.to_string(), v.clone())))
        .collect();
    Some(serde_json::Value::Object(outputs))
}

#[cfg(test)]
mod tests {
    use super::requested_quant_outputs;
    use serde_json::json;

    #[test]
    fn requested_quant_outputs_copies_only_what_was_requested() {
        let td = tempfile::tempdir().expect("failed to create tempdir");
        std::fs::write(
            td.path().join("quant.json"),
            json!({
                "num_genes": 3,
                "molecule_table": {"file": "alevin/molecules.parquet", "num_molecules": 7},
                "probe_mtx": true,
                "num_probes": 5,
                "probe_nnz": 9
            })
            .to_string(),
        )
        .expect("failed to write quant.json");

        // Nothing requested: nothing recorded, and quant.json is not read.
        assert_eq!(
            requested_quant_outputs(&td.path().join("absent"), false, false),
            None
        );
        assert_eq!(
            requested_quant_outputs(td.path(), true, false),
            Some(json!({
                "molecule_table": {"file": "alevin/molecules.parquet", "num_molecules": 7}
            }))
        );
        assert_eq!(
            requested_quant_outputs(td.path(), false, true),
            Some(json!({"probe_mtx": true, "num_probes": 5, "probe_nnz": 9}))
        );
        // An unreadable quant.json costs only the entries.
        assert_eq!(
            requested_quant_outputs(&td.path().join("absent"), true, true),
            None
        );
    }
}
