use anyhow::{anyhow, bail};
use regex;
use serde_json::json;
use std::path::Path;

pub fn construct_json_from_salmon_log<P: AsRef<Path>>(
    log_path: P,
) -> anyhow::Result<serde_json::Value> {
    let log: String = std::fs::read_to_string(log_path.as_ref())?;
    let pat = regex::Regex::new(r"(?<mapped>\d+) total fragments out of (?<total>\d+)[\w\W]+discarded because they are best-mapped to decoys : (?<decoys>\d+)").unwrap();
    if let Some(captures) = pat.captures(&log) {
        let nmapped = captures
            .name("mapped")
            .ok_or(anyhow!("Could not extract mapped reads"))?
            .as_str()
            .parse::<u64>()?;
        let nreads = captures
            .name("total")
            .ok_or(anyhow!("Could not extract total reads"))?
            .as_str()
            .parse::<u64>()?;
        let decoys = captures
            .name("decoys")
            .ok_or(anyhow!("Could not extract decoy mappings"))?
            .as_str()
            .parse::<u64>()?;
        let pmapped = (nmapped as f64) / (nreads as f64) * 100.;
        Ok(json!({
            "mapper": "salmon",
            "num_mapped": nmapped,
            "num_poisoned": decoys,
            "num_reads": nreads,
            "percent_mapped": pmapped
        }))
    } else {
        bail!("Could not capture relevant logging output!");
    }
}

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
