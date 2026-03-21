use std::path::PathBuf;

use anyhow::bail;
use serde_json::Value;

use crate::core::io;
use crate::utils::af_utils::IndexType;

#[derive(Debug)]
pub struct QuantIndexMetadata {
    pub index_type: IndexType,
    pub inferred_t2g: Option<PathBuf>,
    pub inferred_gene_id_to_name: Option<PathBuf>,
}

pub fn resolve_quant_index(index: Option<PathBuf>) -> anyhow::Result<QuantIndexMetadata> {
    let mut inferred_t2g = None;
    let mut inferred_gene_id_to_name = None;
    let index_type;

    if let Some(mut index) = index {
        let removed_piscem_idx_suffix = if !index.is_dir() && index.ends_with("piscem_idx") {
            index.pop();
            true
        } else {
            false
        };

        let index_json_path = index.join("simpleaf_index.json");
        match index_json_path.try_exists() {
            Ok(true) => {
                let v: Value = io::read_json_file(&index_json_path)?;

                let index_type_str: String = serde_json::from_value(v["index_type"].clone())?;
                index_type = match index_type_str.as_ref() {
                    "piscem" => IndexType::Piscem(index.join("piscem_idx")),
                    "salmon" => {
                        bail!(
                            "The index at {} was built for salmon, which is no longer supported. Please rebuild this index with `simpleaf index` to create a piscem index.",
                            index.display()
                        );
                    }
                    _ => {
                        bail!(
                            "unknown index type {} present in simpleaf_index.json",
                            index_type_str
                        );
                    }
                };

                let t2g_opt: Option<PathBuf> = serde_json::from_value(v["t2g_file"].clone())?;
                if let Some(t2g_rel) = t2g_opt {
                    inferred_t2g = Some(index.join(t2g_rel));
                }

                if index.join("gene_id_to_name.tsv").exists() {
                    inferred_gene_id_to_name = Some(index.join("gene_id_to_name.tsv"));
                } else if let Some(index_parent) = index.parent() {
                    let gene_name_path = index_parent.join("ref").join("gene_id_to_name.tsv");
                    if gene_name_path.exists() && gene_name_path.is_file() {
                        inferred_gene_id_to_name = Some(gene_name_path);
                    }
                }
            }
            Ok(false) => {
                if removed_piscem_idx_suffix {
                    index.push("piscem_idx");
                }
                index_type = IndexType::Piscem(index);
            }
            Err(e) => bail!(e),
        }
    } else {
        index_type = IndexType::NoIndex;
    }

    Ok(QuantIndexMetadata {
        index_type,
        inferred_t2g,
        inferred_gene_id_to_name,
    })
}

pub fn resolve_atac_piscem_index_base(mut index: PathBuf) -> anyhow::Result<PathBuf> {
    let removed_piscem_idx_suffix = if !index.is_dir() && index.ends_with("piscem_idx") {
        index.pop();
        true
    } else {
        false
    };

    let index_json_path = index.join("simpleaf_index.json");
    match index_json_path.try_exists() {
        Ok(true) => {
            let v: Value = io::read_json_file(&index_json_path)?;

            let index_type_str: String = serde_json::from_value(v["index_type"].clone())?;
            match index_type_str.as_ref() {
                "piscem" => Ok(index.join("piscem_idx")),
                _ => bail!(
                    "unknown index type {} present in simpleaf_index.json",
                    index_type_str
                ),
            }
        }
        Ok(false) => {
            if removed_piscem_idx_suffix {
                index.push("piscem_idx");
            }
            Ok(index)
        }
        Err(e) => bail!(e),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::tempdir;

    use super::{resolve_atac_piscem_index_base, resolve_quant_index};

    #[test]
    fn resolve_quant_index_reads_simpleaf_index_metadata() {
        let td = tempdir().expect("failed to create tempdir");
        let idx_dir = td.path().join("index");
        fs::create_dir_all(&idx_dir).expect("failed to create index dir");
        fs::write(
            idx_dir.join("simpleaf_index.json"),
            serde_json::to_string_pretty(&json!({
            "index_type":"salmon",
            "t2g_file":"t2g_3col.tsv"
        }))
        .expect("failed to serialize json"),
        )
        .expect("failed to write simpleaf_index.json");
        fs::write(idx_dir.join("gene_id_to_name.tsv"), "g1\tn1\n")
            .expect("failed to write gene_id_to_name.tsv");

        let err = resolve_quant_index(Some(idx_dir.clone()))
            .expect_err("salmon metadata should be rejected");
        assert!(
            format!("{:#}", err).contains("no longer supported"),
            "unexpected error: {:#}",
            err
        );
    }

    #[test]
    fn resolve_atac_index_base_accepts_plain_prefix() {
        let td = tempdir().expect("failed to create tempdir");
        let idx = td.path().join("foo").join("piscem_idx");
        let resolved =
            resolve_atac_piscem_index_base(idx.clone()).expect("failed to resolve atac index");
        assert_eq!(resolved, idx);
    }
}
