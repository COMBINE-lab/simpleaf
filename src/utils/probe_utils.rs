//! Utilities for working with probe set CSV and FASTA files.
//!
//! Shared between `multiplex-quant` (multiplexed) and `quant` (single-sample)
//! for converting 10x probe set CSVs to FASTA + t2g mapping files.

use anyhow::{Context, bail};
use serde_json::json;
use std::collections::{BTreeMap, HashSet};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// After a `piscem build` that was handed decoy sequences (`--decoy-paths`),
/// warn if the resulting poison table came back empty. piscem's poison k-mers
/// are the decoy k-mers *adjacent* to the indexed reference in the compacted de
/// Bruijn graph; when `k` is large relative to any decoy/reference adjacency
/// (e.g. the generic `index` default k=31 on 50 bp probes) none are found, the
/// table is empty, and `--excluded-probes decoy` then silently filters nothing
/// at map time. The probe-quant path defaults to k=23, which does populate it.
/// Best-effort: stays quiet if the poison sidecar is absent or unparseable
/// (e.g. a piscem build that produced no decoys at all).
pub fn warn_if_empty_poison_table(index_prefix: &Path, kmer_length: usize) {
    let mut poison_json = index_prefix.as_os_str().to_owned();
    poison_json.push(".poison.json");
    let poison_json = PathBuf::from(poison_json);

    let Ok(contents) = std::fs::read_to_string(&poison_json) else {
        return;
    };
    let num_poison = serde_json::from_str::<serde_json::Value>(&contents)
        .ok()
        .and_then(|v| v.get("num_poison_kmers").and_then(|x| x.as_u64()));
    if num_poison == Some(0) {
        warn!(
            "Decoy sequences were indexed, but the resulting poison table is empty \
             (0 poison k-mers at k={kmer_length}): no reads will be filtered by the \
             decoys at map time. piscem poison k-mers are decoy k-mers adjacent to the \
             reference in the de Bruijn graph, so an empty table usually means k is too \
             large for any decoy/reference adjacency (the probe-quant default k is 23; \
             the generic `index` default is 31). Rebuild with a smaller k to make the \
             decoys effective."
        );
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeT2gMode {
    Gene,
    Usa,
}

/// What to do with probes the probe set flags as excluded (`included == FALSE`,
/// i.e. 10x's `__EXCLUDED` probes).
///
/// 10x excludes a probe from quantification because it is known to be
/// off-target or otherwise unreliable, but reads from such a probe are still
/// present in the library. Dropping the probe entirely lets those reads find
/// their next-best match among the *retained* probes, which is exactly the
/// spurious mapping the exclusion was meant to prevent. Indexing the excluded
/// sequences as decoys keeps them out of the quantified reference (they never
/// enter the t2g map or the gene set) while still letting piscem recognise —
/// and discard — reads that belong to them.
#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ExcludedProbeMode {
    /// Index excluded probes as decoys (piscem poison k-mers): reads from them
    /// are recognised and discarded rather than mis-assigned to a retained probe.
    #[default]
    Decoy,
    /// Drop excluded probes from the reference entirely (the behavior of earlier releases).
    Ignore,
}

impl ExcludedProbeMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExcludedProbeMode::Decoy => "decoy",
            ExcludedProbeMode::Ignore => "ignore",
        }
    }
}

#[derive(Debug)]
pub struct ProbeReferenceFiles {
    pub fasta_path: PathBuf,
    pub gene_t2g_path: PathBuf,
    pub usa_t2g_path: Option<PathBuf>,
    pub gene_id_to_name_path: Option<PathBuf>,
    /// FASTA of excluded probes to index as decoys. `Some` only when
    /// [`ExcludedProbeMode::Decoy`] was requested *and* at least one probe was
    /// excluded, so callers can forward it to `piscem build --decoy-paths`
    /// unconditionally (piscem is never handed an empty decoy file).
    pub decoy_fasta_path: Option<PathBuf>,
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

/// Record a `gene_id -> gene_name` association into `map`, erroring if a *different*
/// name was already recorded for the same `gene_id` (an internally inconsistent probe
/// set). Shared by the probe-set conversion (auto-build) and `simpleaf index --probe-csv`.
pub fn insert_gene_name(
    map: &mut BTreeMap<String, String>,
    gene_id: &str,
    gene_name: &str,
) -> anyhow::Result<()> {
    if let Some(prev) = map.insert(gene_id.to_string(), gene_name.to_string())
        && prev != gene_name
    {
        bail!(
            "probe CSV contains inconsistent gene annotations for `{}`: saw both `{}` and `{}`.",
            gene_id,
            prev,
            gene_name,
        );
    }
    Ok(())
}

/// Write a `gene_id -> gene_name` map as a 2-column TSV (rows sorted by `gene_id`,
/// since the map is a `BTreeMap`). Shared by both probe-index build paths.
pub fn write_gene_id_to_name(map: &BTreeMap<String, String>, path: &Path) -> anyhow::Result<()> {
    let mut writer = BufWriter::new(std::fs::File::create(path)?);
    for (gene_id, gene_name) in map {
        writeln!(writer, "{}\t{}", gene_id, gene_name)?;
    }
    writer.flush()?;
    Ok(())
}

/// Convert a 10x probe set CSV file to a FASTA file suitable for indexing.
///
/// Also generates a collapsed gene-level transcript-to-gene (t2g) map and, when
/// probe `region` annotations are present, a separate USA-mode t2g map.
///
/// Excluded probes (`included == FALSE`) never enter `probes.fa` or either t2g
/// map. Under [`ExcludedProbeMode::Decoy`] their sequences are written to
/// `probe_decoys.fa` instead, for the caller to hand to `piscem build
/// --decoy-paths`; under [`ExcludedProbeMode::Ignore`] they are dropped.
pub fn convert_probe_csv_to_reference_files(
    csv_path: &Path,
    output_dir: &Path,
    excluded_mode: ExcludedProbeMode,
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
    let decoy_fasta_path = output_dir.join("probe_decoys.fa");
    let meta_path = output_dir.join("probe_set_info.json");

    let mut fasta_writer = BufWriter::new(std::fs::File::create(&fasta_path)?);
    let mut gene_t2g_writer = BufWriter::new(std::fs::File::create(&gene_t2g_path)?);
    let mut usa_t2g_writer = if let Some(ref path) = usa_t2g_path {
        Some(BufWriter::new(std::fs::File::create(path)?))
    } else {
        None
    };
    // Opened lazily on the first excluded probe so that a probe set with no
    // exclusions leaves no (empty) decoy file behind.
    let mut decoy_writer: Option<BufWriter<std::fs::File>> = None;

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
            if excluded_mode == ExcludedProbeMode::Decoy {
                if decoy_writer.is_none() {
                    decoy_writer = Some(BufWriter::new(std::fs::File::create(&decoy_fasta_path)?));
                }
                if let Some(writer) = decoy_writer.as_mut() {
                    writeln!(writer, ">{}", probe_id)?;
                    writeln!(writer, "{}", probe_seq)?;
                }
            }
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
            insert_gene_name(&mut gene_id_to_name, gene_id, gene_name)?;
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
    let decoy_fasta_path = match decoy_writer.as_mut() {
        Some(writer) => {
            writer.flush()?;
            Some(decoy_fasta_path)
        }
        None => None,
    };
    let num_decoy = if decoy_fasta_path.is_some() {
        num_excluded
    } else {
        0
    };
    if let Some(ref path) = gene_id_to_name_path {
        write_gene_id_to_name(&gene_id_to_name, path)?;
    }

    metadata.insert("num_probes".to_string(), json!(num_probes));
    metadata.insert("num_included".to_string(), json!(num_included));
    metadata.insert("num_excluded".to_string(), json!(num_excluded));
    metadata.insert("num_decoy".to_string(), json!(num_decoy));
    metadata.insert("excluded_probes".to_string(), json!(excluded_mode.as_str()));
    metadata.insert("num_genes".to_string(), json!(genes.len()));
    metadata.insert("has_region".to_string(), json!(region_idx.is_some()));
    metadata.insert(
        "has_gene_symbol".to_string(),
        json!(gene_name_idx.is_some()),
    );
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
        "Converted probe CSV: {} included probes, {} genes, {} excluded ({} indexed as decoys)",
        num_included,
        genes.len(),
        num_excluded,
        num_decoy,
    );

    Ok(ProbeReferenceFiles {
        fasta_path,
        gene_t2g_path,
        usa_t2g_path,
        gene_id_to_name_path,
        decoy_fasta_path,
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
        ExcludedProbeMode, ProbeT2gMode, collapse_t2g_to_gene,
        convert_probe_csv_to_reference_files, ensure_t2g_mode, insert_gene_name,
        t2g_has_usa_mapping, warn_if_empty_poison_table,
    };
    use std::collections::BTreeMap;
    use std::fs;
    use tempfile::tempdir;

    // The warning is best-effort and only logs, so these tests exercise the
    // code paths (path building, JSON parsing, absent-file handling) rather than
    // asserting on log output; the point is that none of them panic.
    #[test]
    fn poison_warning_handles_empty_missing_and_populated() {
        let dir = tempdir().unwrap();
        let prefix = dir.path().join("piscem_idx");
        // absent sidecar -> quiet, no panic
        warn_if_empty_poison_table(&prefix, 31);
        // empty poison table -> would warn, must not panic
        fs::write(
            dir.path().join("piscem_idx.poison.json"),
            r#"{"max_poison_occ":0,"num_poison_kmers":0,"num_poison_occs":0}"#,
        )
        .unwrap();
        warn_if_empty_poison_table(&prefix, 31);
        // populated table -> no warning, no panic
        fs::write(
            dir.path().join("piscem_idx.poison.json"),
            r#"{"max_poison_occ":1,"num_poison_kmers":46,"num_poison_occs":46}"#,
        )
        .unwrap();
        warn_if_empty_poison_table(&prefix, 23);
    }

    #[test]
    fn insert_gene_name_dedups_and_detects_conflicts() {
        let mut m = BTreeMap::new();
        insert_gene_name(&mut m, "G1", "GeneOne").expect("first insert ok");
        insert_gene_name(&mut m, "G1", "GeneOne").expect("identical re-insert ok");
        insert_gene_name(&mut m, "G2", "GeneTwo").expect("distinct gene ok");
        assert_eq!(m.len(), 2);
        assert_eq!(m.get("G1").map(String::as_str), Some("GeneOne"));

        let err = insert_gene_name(&mut m, "G1", "Different")
            .expect_err("conflicting name for same gene_id should error");
        assert!(
            format!("{:#}", err).contains("inconsistent gene annotations"),
            "unexpected error: {:#}",
            err
        );
    }

    #[test]
    fn convert_probe_csv_writes_gene_and_usa_t2g_files() {
        let td = tempdir().expect("failed to create tempdir");
        let csv_path = td.path().join("probes.csv");
        fs::write(
            &csv_path,
            "#panel=test\ngene_id,gene_name,probe_seq,probe_id,included,region\nG1,GeneOne,AAAA,P1,TRUE,spliced\nG1,GeneOne,CCCC,P2,FALSE,unspliced\nG2,GeneTwo,GGGG,P3,TRUE,unspliced\n",
        )
        .expect("failed to write probe CSV");

        let converted =
            convert_probe_csv_to_reference_files(&csv_path, td.path(), ExcludedProbeMode::Decoy)
                .expect("failed to convert probe CSV");

        // The excluded probe (P2) is quantified nowhere: not in the reference FASTA
        // and not in either t2g map ...
        assert_eq!(
            fs::read_to_string(&converted.fasta_path).expect("failed to read probes.fa"),
            ">P1\nAAAA\n>P3\nGGGG\n"
        );
        assert_eq!(
            fs::read_to_string(&converted.gene_t2g_path).expect("failed to read gene t2g"),
            "P1\tG1\nP3\tG2\n"
        );
        // ... but its sequence is emitted as a decoy.
        assert_eq!(
            fs::read_to_string(
                converted
                    .decoy_fasta_path
                    .as_ref()
                    .expect("decoy FASTA should be present")
            )
            .expect("failed to read decoy FASTA"),
            ">P2\nCCCC\n"
        );
        assert_eq!(converted.metadata["num_probes"], 3);
        assert_eq!(converted.metadata["num_included"], 2);
        assert_eq!(converted.metadata["num_excluded"], 1);
        assert_eq!(converted.metadata["num_decoy"], 1);
        assert_eq!(converted.metadata["num_genes"], 2);
        assert_eq!(converted.metadata["excluded_probes"], "decoy");
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
    fn convert_probe_csv_ignore_mode_drops_excluded_probes() {
        let td = tempdir().expect("failed to create tempdir");
        let csv_path = td.path().join("probes.csv");
        fs::write(
            &csv_path,
            "gene_id,probe_seq,probe_id,included\nG1,AAAA,P1,TRUE\nG1,CCCC,P2,FALSE\n",
        )
        .expect("failed to write probe CSV");

        let converted =
            convert_probe_csv_to_reference_files(&csv_path, td.path(), ExcludedProbeMode::Ignore)
                .expect("failed to convert probe CSV");

        assert!(converted.decoy_fasta_path.is_none());
        assert!(!td.path().join("probe_decoys.fa").exists());
        assert_eq!(
            fs::read_to_string(&converted.fasta_path).expect("failed to read probes.fa"),
            ">P1\nAAAA\n"
        );
        assert_eq!(converted.metadata["num_excluded"], 1);
        assert_eq!(converted.metadata["num_decoy"], 0);
        assert_eq!(converted.metadata["excluded_probes"], "ignore");
    }

    #[test]
    fn convert_probe_csv_without_exclusions_writes_no_decoy_file() {
        let td = tempdir().expect("failed to create tempdir");
        let csv_path = td.path().join("probes.csv");
        fs::write(
            &csv_path,
            "gene_id,probe_seq,probe_id,included\nG1,AAAA,P1,TRUE\nG2,CCCC,P2,TRUE\n",
        )
        .expect("failed to write probe CSV");

        let converted =
            convert_probe_csv_to_reference_files(&csv_path, td.path(), ExcludedProbeMode::Decoy)
                .expect("failed to convert probe CSV");

        // Nothing to decoy: no file is created, so callers never hand piscem an
        // empty decoy FASTA.
        assert!(converted.decoy_fasta_path.is_none());
        assert!(!td.path().join("probe_decoys.fa").exists());
        assert_eq!(converted.metadata["num_decoy"], 0);
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

        let converted =
            convert_probe_csv_to_reference_files(&csv_path, td.path(), ExcludedProbeMode::Decoy)
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
