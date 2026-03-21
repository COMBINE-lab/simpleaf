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
