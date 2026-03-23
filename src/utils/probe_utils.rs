//! Utilities for working with probe set CSV and FASTA files.
//!
//! Shared between `multiplex-quant` (multiplexed) and `quant` (single-sample)
//! for converting 10x probe set CSVs to FASTA + t2g mapping files.

use anyhow::{Context, bail};
use serde_json::json;
use std::collections::{BTreeMap, HashSet};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use tracing::info;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeT2gMode {
    Gene,
    Usa,
}

#[derive(Debug)]
pub struct ProbeReferenceFiles {
    pub fasta_path: PathBuf,
    pub gene_t2g_path: PathBuf,
    pub usa_t2g_path: Option<PathBuf>,
    pub gene_id_to_name_path: Option<PathBuf>,
    #[allow(dead_code)]
    pub metadata: serde_json::Value,
}

fn get_optional_idx(headers: &csv::StringRecord, names: &[&str]) -> Option<usize> {
    names
        .iter()
        .find_map(|name| headers.iter().position(|h| h == *name))
}

fn parse_probe_region(region: &str) -> anyhow::Result<&'static str> {
    if region.eq_ignore_ascii_case("spliced") {
        Ok("S")
    } else if region.eq_ignore_ascii_case("unspliced") {
        Ok("U")
    } else {
        bail!(
            "Invalid probe `region` value `{}`. Expected `spliced` or `unspliced`.",
            region
        );
    }
}

fn get_required_idx(headers: &csv::StringRecord, name: &str) -> anyhow::Result<usize> {
    headers
        .iter()
        .position(|h| h == name)
        .with_context(|| format!("probe CSV is missing required column `{}`", name))
}

/// Convert a 10x probe set CSV file to a FASTA file suitable for indexing.
///
/// Also generates a collapsed gene-level transcript-to-gene (t2g) map and, when
/// probe `region` annotations are present, a separate USA-mode t2g map.
pub fn convert_probe_csv_to_reference_files(
    csv_path: &Path,
    output_dir: &Path,
) -> anyhow::Result<ProbeReferenceFiles> {
    std::fs::create_dir_all(output_dir)?;

    let meta_reader = BufReader::new(
        std::fs::File::open(csv_path)
            .with_context(|| format!("couldn't open probe CSV: {}", csv_path.display()))?,
    );
    let mut metadata = serde_json::Map::new();
    for line in meta_reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if let Some(stripped) = trimmed.strip_prefix('#')
            && let Some((key, val)) = stripped.split_once('=')
        {
            metadata.insert(key.to_string(), serde_json::Value::String(val.to_string()));
        }
    }

    let mut rdr = csv::ReaderBuilder::new()
        .comment(Some(b'#'))
        .from_path(csv_path)
        .with_context(|| format!("couldn't parse probe CSV: {}", csv_path.display()))?;
    let headers = rdr.headers()?.clone();
    let gene_idx = get_required_idx(&headers, "gene_id")?;
    let seq_idx = get_required_idx(&headers, "probe_seq")?;
    let probe_idx = get_required_idx(&headers, "probe_id")?;
    let included_idx = headers.iter().position(|h| h == "included");
    let region_idx = headers.iter().position(|h| h == "region");
    let gene_name_idx = get_optional_idx(&headers, &["gene_symbol", "gene_name"]);

    let fasta_path = output_dir.join("probes.fa");
    let gene_t2g_path = output_dir.join("probe_t2g.tsv");
    let usa_t2g_path = region_idx.map(|_| output_dir.join("probe_t2g_usa.tsv"));
    let gene_id_to_name_path = gene_name_idx.map(|_| output_dir.join("gene_id_to_name.tsv"));
    let meta_path = output_dir.join("probe_set_info.json");

    let mut fasta_writer = BufWriter::new(std::fs::File::create(&fasta_path)?);
    let mut gene_t2g_writer = BufWriter::new(std::fs::File::create(&gene_t2g_path)?);
    let mut usa_t2g_writer = if let Some(ref path) = usa_t2g_path {
        Some(BufWriter::new(std::fs::File::create(path)?))
    } else {
        None
    };

    let mut num_probes = 0u64;
    let mut num_included = 0u64;
    let mut num_excluded = 0u64;
    let mut genes = HashSet::new();
    let mut gene_id_to_name = BTreeMap::new();

    for record in rdr.records() {
        let record = record?;
        let gene_id = record
            .get(gene_idx)
            .context("probe CSV record missing gene_id value")?;
        let probe_seq = record
            .get(seq_idx)
            .context("probe CSV record missing probe_seq value")?;
        let probe_id = record
            .get(probe_idx)
            .context("probe CSV record missing probe_id value")?;
        let included = included_idx
            .and_then(|i| record.get(i))
            .map(|v| !v.eq_ignore_ascii_case("false"))
            .unwrap_or(true);

        num_probes += 1;
        if !included {
            num_excluded += 1;
            continue;
        }
        num_included += 1;

        writeln!(fasta_writer, ">{}", probe_id)?;
        writeln!(fasta_writer, "{}", probe_seq)?;
        writeln!(gene_t2g_writer, "{}\t{}", probe_id, gene_id)?;

        if let Some(gene_name_i) = gene_name_idx
            && let Some(gene_name) = record.get(gene_name_i).map(str::trim)
            && !gene_name.is_empty()
        {
            if let Some(prev) = gene_id_to_name.insert(gene_id.to_string(), gene_name.to_string())
                && prev != gene_name
            {
                bail!(
                    "probe CSV contains inconsistent gene annotations for `{}`: saw both `{}` and `{}`.",
                    gene_id,
                    prev,
                    gene_name,
                );
            }
        }

        if let Some(region_i) = region_idx {
            let region = record
                .get(region_i)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .with_context(|| {
                    format!(
                        "probe `{}` is missing a `region` value required for USA-mode quantification. \
Expected `spliced` or `unspliced`.",
                        probe_id
                    )
                })?;
            let parsed = parse_probe_region(region)?;
            if let Some(writer) = usa_t2g_writer.as_mut() {
                writeln!(writer, "{}\t{}\t{}", probe_id, gene_id, parsed)?;
            }
        }

        genes.insert(gene_id.to_string());
    }

    fasta_writer.flush()?;
    gene_t2g_writer.flush()?;
    if let Some(writer) = usa_t2g_writer.as_mut() {
        writer.flush()?;
    }
    if let Some(ref path) = gene_id_to_name_path {
        let mut writer = BufWriter::new(std::fs::File::create(path)?);
        for (gene_id, gene_name) in &gene_id_to_name {
            writeln!(writer, "{}\t{}", gene_id, gene_name)?;
        }
        writer.flush()?;
    }

    metadata.insert("num_probes".to_string(), json!(num_probes));
    metadata.insert("num_included".to_string(), json!(num_included));
    metadata.insert("num_excluded".to_string(), json!(num_excluded));
    metadata.insert("num_genes".to_string(), json!(genes.len()));
    metadata.insert("has_region".to_string(), json!(region_idx.is_some()));
    metadata.insert("has_gene_symbol".to_string(), json!(gene_name_idx.is_some()));
    if let Some(idx) = gene_name_idx {
        metadata.insert("gene_symbol_column".to_string(), json!(headers.get(idx)));
    }
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

    Ok(ProbeReferenceFiles {
        fasta_path,
        gene_t2g_path,
        usa_t2g_path,
        gene_id_to_name_path,
        metadata: meta_value,
    })
}

pub fn write_identity_t2g_from_fasta(fasta_path: &Path, t2g_path: &Path) -> anyhow::Result<()> {
    let fa_file = std::fs::File::open(fasta_path)?;
    let reader = BufReader::new(fa_file);
    let mut t2g_writer = BufWriter::new(std::fs::File::create(t2g_path)?);
    for line in reader.lines() {
        let line = line?;
        if let Some(name) = line.strip_prefix('>') {
            let name = name.split_whitespace().next().unwrap_or(name);
            let gene = name.split('|').next().unwrap_or(name);
            writeln!(t2g_writer, "{}\t{}", name, gene)?;
        }
    }
    t2g_writer.flush()?;
    Ok(())
}

pub fn t2g_has_usa_mapping(t2g_path: &Path) -> anyhow::Result<bool> {
    let reader = BufReader::new(std::fs::File::open(t2g_path)?);
    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        return Ok(trimmed.split('\t').count() >= 3);
    }
    Ok(false)
}

pub fn collapse_t2g_to_gene(input_t2g: &Path, output_t2g: &Path) -> anyhow::Result<()> {
    let reader = BufReader::new(std::fs::File::open(input_t2g)?);
    let mut writer = BufWriter::new(std::fs::File::create(output_t2g)?);
    let mut seen = HashSet::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let mut cols = trimmed.split('\t');
        let txp = cols
            .next()
            .with_context(|| format!("invalid t2g line in {}", input_t2g.display()))?;
        let gene = cols
            .next()
            .with_context(|| format!("invalid t2g line in {}", input_t2g.display()))?;
        let key = format!("{}\t{}", txp, gene);
        if seen.insert(key.clone()) {
            writeln!(writer, "{}", key)?;
        }
    }

    writer.flush()?;
    Ok(())
}

pub fn ensure_t2g_mode(
    input_t2g: &Path,
    output_dir: &Path,
    mode: ProbeT2gMode,
) -> anyhow::Result<PathBuf> {
    match mode {
        ProbeT2gMode::Usa => {
            if t2g_has_usa_mapping(input_t2g)? {
                Ok(input_t2g.to_path_buf())
            } else {
                bail!(
                    "USA-mode quantification was requested, but `{}` does not contain a splicing-aware 3-column t2g map. \
Provide a probe CSV with a `region` column (`spliced` / `unspliced`), or a pre-built index with an adjacent `t2g_3col.tsv` or `probe_t2g_usa.tsv`, or rerun without `--usa`.",
                    input_t2g.display(),
                );
            }
        }
        ProbeT2gMode::Gene => {
            if !t2g_has_usa_mapping(input_t2g)? {
                return Ok(input_t2g.to_path_buf());
            }

            std::fs::create_dir_all(output_dir)?;
            let collapsed = output_dir.join("gene_t2g.tsv");
            collapse_t2g_to_gene(input_t2g, &collapsed)?;
            Ok(collapsed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ProbeT2gMode, collapse_t2g_to_gene, convert_probe_csv_to_reference_files, ensure_t2g_mode,
        t2g_has_usa_mapping,
    };
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn convert_probe_csv_writes_gene_and_usa_t2g_files() {
        let td = tempdir().expect("failed to create tempdir");
        let csv_path = td.path().join("probes.csv");
        fs::write(
            &csv_path,
            "#panel=test\ngene_id,gene_name,probe_seq,probe_id,included,region\nG1,GeneOne,AAAA,P1,TRUE,spliced\nG1,GeneOne,CCCC,P2,FALSE,unspliced\nG2,GeneTwo,GGGG,P3,TRUE,unspliced\n",
        )
        .expect("failed to write probe CSV");

        let converted = convert_probe_csv_to_reference_files(&csv_path, td.path())
            .expect("failed to convert probe CSV");

        assert_eq!(
            fs::read_to_string(&converted.gene_t2g_path).expect("failed to read gene t2g"),
            "P1\tG1\nP3\tG2\n"
        );
        assert_eq!(
            fs::read_to_string(
                converted
                    .usa_t2g_path
                    .as_ref()
                    .expect("USA t2g should be present")
            )
            .expect("failed to read USA t2g"),
            "P1\tG1\tS\nP3\tG2\tU\n"
        );
        assert!(
            converted.metadata["has_region"]
                .as_bool()
                .expect("has_region must be a bool")
        );
        assert_eq!(
            fs::read_to_string(
                converted
                    .gene_id_to_name_path
                    .as_ref()
                    .expect("gene_id_to_name should be present")
            )
            .expect("failed to read gene_id_to_name"),
            "G1\tGeneOne\nG2\tGeneTwo\n"
        );
    }

    #[test]
    fn convert_probe_csv_without_gene_name_skips_gene_id_to_name() {
        let td = tempdir().expect("failed to create tempdir");
        let csv_path = td.path().join("probes.csv");
        fs::write(
            &csv_path,
            "gene_id,probe_seq,probe_id\nG1,AAAA,P1\nG2,CCCC,P2\n",
        )
        .expect("failed to write probe CSV");

        let converted = convert_probe_csv_to_reference_files(&csv_path, td.path())
            .expect("failed to convert probe CSV");

        assert!(converted.gene_id_to_name_path.is_none());
        assert!(
            !converted.metadata["has_gene_symbol"]
                .as_bool()
                .expect("has_gene_symbol must be a bool")
        );
    }

    #[test]
    fn ensure_t2g_mode_collapses_usa_mappings() {
        let td = tempdir().expect("failed to create tempdir");
        let input = td.path().join("t2g_3col.tsv");
        fs::write(&input, "P1\tG1\tS\nP2\tG1\tU\n").expect("failed to write input t2g");

        let collapsed =
            ensure_t2g_mode(&input, td.path(), ProbeT2gMode::Gene).expect("collapse failed");
        assert_eq!(
            fs::read_to_string(collapsed).expect("failed to read collapsed t2g"),
            "P1\tG1\nP2\tG1\n"
        );
    }

    #[test]
    fn ensure_t2g_mode_rejects_gene_only_mapping_for_usa() {
        let td = tempdir().expect("failed to create tempdir");
        let input = td.path().join("probe_t2g.tsv");
        fs::write(&input, "P1\tG1\n").expect("failed to write input t2g");

        let err = ensure_t2g_mode(&input, td.path(), ProbeT2gMode::Usa)
            .expect_err("gene-only t2g should be rejected for USA");
        assert!(
            format!("{:#}", err).contains("rerun without `--usa`"),
            "unexpected error: {:#}",
            err
        );
    }

    #[test]
    fn t2g_helpers_detect_and_collapse_usa_maps() {
        let td = tempdir().expect("failed to create tempdir");
        let input = td.path().join("in.tsv");
        let output = td.path().join("out.tsv");
        fs::write(&input, "P1\tG1\tS\nP1\tG1\tU\n").expect("failed to write t2g");

        assert!(t2g_has_usa_mapping(&input).expect("failed to inspect t2g"));
        collapse_t2g_to_gene(&input, &output).expect("failed to collapse t2g");
        assert_eq!(
            fs::read_to_string(output).expect("failed to read collapsed t2g"),
            "P1\tG1\n"
        );
    }
}
