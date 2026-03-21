//! Utilities for working with probe set CSV files.
//!
//! Shared between `multiplex-quant` (multiplexed) and `quant` (single-sample)
//! for converting 10x probe set CSVs to FASTA + t2g mapping files.

use anyhow::Context;
use serde_json::json;
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use tracing::info;

/// Convert a 10x probe set CSV file to a FASTA file suitable for indexing.
///
/// Also generates a transcript-to-gene (t2g) map and extracts probe set metadata.
/// Returns (fasta_path, t2g_path, metadata_json).
///
/// All probes (included and excluded) are written to both FASTA and t2g.
/// The index needs entries for all probes; excluded probes still map to their
/// gene but won't contribute meaningful counts in practice.
pub fn convert_probe_csv_to_fasta(
    csv_path: &Path,
    output_dir: &Path,
) -> anyhow::Result<(PathBuf, PathBuf, serde_json::Value)> {
    std::fs::create_dir_all(output_dir)?;

    let file = std::fs::File::open(csv_path)
        .with_context(|| format!("couldn't open probe CSV: {}", csv_path.display()))?;
    let reader = BufReader::new(file);

    let fasta_path = output_dir.join("probes.fa");
    let t2g_path = output_dir.join("probe_t2g.tsv");
    let meta_path = output_dir.join("probe_set_info.json");

    let mut fasta_writer = std::io::BufWriter::new(std::fs::File::create(&fasta_path)?);
    let mut t2g_writer = std::io::BufWriter::new(std::fs::File::create(&t2g_path)?);

    let mut metadata = serde_json::Map::new();
    let mut num_probes = 0u64;
    let mut num_included = 0u64;
    let mut num_excluded = 0u64;
    let mut genes = HashSet::new();
    let mut header_parsed = false;

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();

        if let Some(stripped) = trimmed.strip_prefix('#') {
            if let Some((key, val)) = stripped.split_once('=') {
                metadata.insert(
                    key.to_string(),
                    serde_json::Value::String(val.to_string()),
                );
            }
            continue;
        }

        if !header_parsed {
            header_parsed = true;
            continue;
        }

        let cols: Vec<&str> = trimmed.split(',').collect();
        if cols.len() < 4 {
            continue;
        }
        let gene_id = cols[0];
        let probe_seq = cols[1];
        let probe_id = cols[2];
        let included = cols[3].eq_ignore_ascii_case("true");

        num_probes += 1;
        if !included {
            num_excluded += 1;
            continue; // skip excluded probes — they are omitted from index and t2g
        }
        num_included += 1;

        writeln!(fasta_writer, ">{}", probe_id)?;
        writeln!(fasta_writer, "{}", probe_seq)?;
        writeln!(t2g_writer, "{}\t{}", probe_id, gene_id)?;
        genes.insert(gene_id.to_string());
    }

    fasta_writer.flush()?;
    t2g_writer.flush()?;

    metadata.insert("num_probes".to_string(), json!(num_probes));
    metadata.insert("num_included".to_string(), json!(num_included));
    metadata.insert("num_excluded".to_string(), json!(num_excluded));
    metadata.insert("num_genes".to_string(), json!(genes.len()));
    metadata.insert(
        "source_file".to_string(),
        json!(csv_path.file_name().unwrap_or_default().to_string_lossy()),
    );

    let meta_value = serde_json::Value::Object(metadata);
    let meta_file = std::fs::File::create(&meta_path)?;
    serde_json::to_writer_pretty(meta_file, &meta_value)?;

    info!(
        "Converted probe CSV: {} included probes, {} genes, {} excluded",
        num_included,
        genes.len(),
        num_excluded,
    );

    Ok((fasta_path, t2g_path, meta_value))
}
