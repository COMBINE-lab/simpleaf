use std::fs;
use std::path::Path;

use anyhow::Context;
use serde::Serialize;
use serde_json::Value;

pub fn read_json_file(path: &Path) -> anyhow::Result<Value> {
    let json_file = std::fs::File::open(path)
        .with_context(|| format!("Could not open JSON file {}.", path.display()))?;
    let v: Value = serde_json::from_reader(json_file)
        .with_context(|| format!("Could not parse JSON file {}.", path.display()))?;
    Ok(v)
}

pub fn write_json_pretty<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    let payload = serde_json::to_string_pretty(value)
        .with_context(|| format!("Could not serialize JSON for {}.", path.display()))?;
    fs::write(path, payload).with_context(|| format!("could not write {}", path.display()))
}

pub fn write_json_pretty_atomic<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    let payload = serde_json::to_string_pretty(value)
        .with_context(|| format!("Could not serialize JSON for {}.", path.display()))?;
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, payload)
        .with_context(|| format!("could not write temporary file {}", tmp_path.display()))?;
    fs::rename(&tmp_path, path).with_context(|| {
        format!(
            "could not atomically rename {} to {}",
            tmp_path.display(),
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::tempdir;

    use super::{read_json_file, write_json_pretty, write_json_pretty_atomic};

    #[test]
    fn write_and_read_json_roundtrip() {
        let td = tempdir().expect("failed to create tempdir");
        let path = td.path().join("a.json");
        let v = json!({"key":"value","n":1});
        write_json_pretty(&path, &v).expect("failed to write json");
        let read_back = read_json_file(&path).expect("failed to read json");
        assert_eq!(read_back, v);
    }

    #[test]
    fn write_json_pretty_atomic_persists_content() {
        let td = tempdir().expect("failed to create tempdir");
        let path = td.path().join("b.json");
        let v = json!({"ok":true});
        write_json_pretty_atomic(&path, &v).expect("failed to write json atomically");
        let read_back = read_json_file(&path).expect("failed to read json");
        assert_eq!(read_back, v);
    }
}
