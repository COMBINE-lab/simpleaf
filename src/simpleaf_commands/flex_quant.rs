//! Pipeline orchestration for 10x Flex GEX quantification.
//!
//! Handles the complete Flex pipeline:
//! 1. Resource resolution (probe index, cell BC whitelist, probe barcode file)
//! 2. Mapping with piscem
//! 3. Generate-permit-list (multi-barcode aware)
//! 4. Collate (hierarchical, multi-barcode)
//! 5. Quant

use crate::core::{context, exec};
use crate::simpleaf_commands::FlexQuantOpts;
use crate::utils::chem_utils::{CustomChemistry, CustomChemistryMap};
use crate::utils::constants::CHEMISTRIES_PATH;
use crate::utils::prog_utils;

use anyhow::{Context, bail};
use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{info, warn};

/// Convert a 10x probe set CSV file to a FASTA file suitable for indexing.
///
/// Also generates a transcript-to-gene (t2g) map and extracts probe set metadata.
/// Returns (fasta_path, t2g_path, metadata_json).
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
    let mut genes = std::collections::HashSet::new();
    let mut header_parsed = false;

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();

        if let Some(stripped) = trimmed.strip_prefix('#') {
            if let Some((key, val)) = stripped.split_once('=') {
                metadata.insert(key.to_string(), serde_json::Value::String(val.to_string()));
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
        if included {
            num_included += 1;
        } else {
            num_excluded += 1;
        }
        // All probes (included and excluded) go into the FASTA and t2g map.
        // The index contains all probes, so quant needs a t2g entry for every
        // reference. Excluded probes still map to their gene — they simply
        // won't contribute meaningful counts in practice.
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

/// Main entry point for the flex-quant pipeline.
pub fn flex_map_and_quant(af_home: &Path, opts: FlexQuantOpts) -> anyhow::Result<()> {
    let start = Instant::now();
    info!("Starting Flex GEX pipeline");

    // Load runtime context (program paths)
    let rt = context::load_runtime_context(af_home)?;
    let piscem_info = rt
        .progs
        .piscem
        .as_ref()
        .context("piscem is required for Flex mapping; please run `simpleaf set-paths`")?;
    let alevin_fry_info = rt
        .progs
        .alevin_fry
        .as_ref()
        .context("alevin-fry is required; please run `simpleaf set-paths`")?;

    // Load chemistry from registry
    let chem_path = af_home.join(CHEMISTRIES_PATH);
    if !chem_path.exists() {
        bail!(
            "Chemistry registry not found at {}. Run `simpleaf chemistry refresh` first.",
            chem_path.display(),
        );
    }
    let chem_file = std::fs::File::open(&chem_path)?;
    let chem_map: CustomChemistryMap = serde_json::from_reader(chem_file)
        .with_context(|| format!("couldn't parse {}", chem_path.display()))?;

    let chem = chem_map
        .get(&opts.chemistry)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Chemistry '{}' not found in registry. Run `simpleaf chemistry refresh`.",
                opts.chemistry,
            )
        })?;

    if !chem.is_flex_gex() {
        bail!(
            "Chemistry '{}' is not a Flex GEX protocol. Use `simpleaf quant` for standard chemistries.",
            opts.chemistry,
        );
    }

    info!(
        "Chemistry: {} (geometry: {}, organism: {})",
        opts.chemistry,
        chem.geometry(),
        opts.organism,
    );

    // Create output directory structure
    let output_dir = &opts.output;
    std::fs::create_dir_all(output_dir)?;
    let map_output = output_dir.join("af_map");
    let quant_output = output_dir.join("af_quant");
    std::fs::create_dir_all(&map_output)?;
    std::fs::create_dir_all(&quant_output)?;

    // === Step 1: Resolve probe index ===
    let (index_path, t2g_path) = resolve_probe_index(
        af_home, chem, &opts, &piscem_info.exe_path,
    )?;

    // === Step 2: Resolve cell barcode whitelist ===
    // For Flex, the cell BC whitelist is fetched the same way as standard chemistries.
    let cell_bc_path = resolve_cell_bc_whitelist(af_home, chem)?;

    // === Step 3: Resolve probe barcode (sample BC) file ===
    let sample_bc_path = resolve_sample_bc_list(af_home, chem, &opts)?;

    // === Step 4: Map reads with piscem ===
    info!("Mapping reads with piscem...");
    let mut piscem_cmd = std::process::Command::new(&piscem_info.exe_path);
    piscem_cmd
        .arg("map-sc")
        .arg("-i").arg(&index_path)
        .arg("-g").arg(chem.geometry())
        .arg("-o").arg(&map_output)
        .arg("-t").arg(format!("{}", opts.threads));

    if opts.struct_constraints {
        piscem_cmd.arg("--struct-constraints");
    }
    piscem_cmd
        .arg("--skipping-strategy").arg(&opts.skipping_strategy)
        .arg("--max-ec-card").arg(format!("{}", opts.max_ec_card));

    let r1_str: Vec<String> = opts.reads1.iter().map(|p| p.display().to_string()).collect();
    let r2_str: Vec<String> = opts.reads2.iter().map(|p| p.display().to_string()).collect();
    piscem_cmd.arg("-1").arg(r1_str.join(","));
    piscem_cmd.arg("-2").arg(r2_str.join(","));

    let map_cmd_str = prog_utils::get_cmd_line_string(&piscem_cmd);
    info!("piscem map-scrna cmd: {}", map_cmd_str);
    let map_start = Instant::now();
    exec::run_checked(&mut piscem_cmd, "[piscem map-sc]")?;
    let map_duration = map_start.elapsed();
    info!("Mapping complete in {:.1}s", map_duration.as_secs_f64());

    // === Step 5: Generate permit list (multi-barcode) ===
    info!("Generating permit list...");
    let mut gpl_cmd = std::process::Command::new(&alevin_fry_info.exe_path);
    gpl_cmd
        .arg("generate-permit-list")
        .arg("-i").arg(&map_output)
        .arg("-d").arg(chem.expected_ori().as_str())
        .arg("-o").arg(&quant_output)
        .arg("-t").arg(format!("{}", opts.threads.min(8)))
        .arg("--unfiltered-pl").arg(&cell_bc_path)
        .arg("--sample-bc-list").arg(&sample_bc_path)
        .arg("--sample-correction-mode").arg("exact")
        .arg("--min-reads").arg(format!("{}", opts.min_reads));

    let gpl_cmd_str = prog_utils::get_cmd_line_string(&gpl_cmd);
    info!("generate-permit-list cmd: {}", gpl_cmd_str);
    let gpl_start = Instant::now();
    exec::run_checked(&mut gpl_cmd, "[generate permit list]")?;
    let gpl_duration = gpl_start.elapsed();

    // === Step 6: Collate ===
    info!("Collating...");
    let mut collate_cmd = std::process::Command::new(&alevin_fry_info.exe_path);
    collate_cmd
        .arg("collate")
        .arg("-i").arg(&quant_output)
        .arg("-r").arg(&map_output)
        .arg("-t").arg(format!("{}", opts.threads));

    let collate_cmd_str = prog_utils::get_cmd_line_string(&collate_cmd);
    info!("collate cmd: {}", collate_cmd_str);
    let collate_start = Instant::now();
    exec::run_checked(&mut collate_cmd, "[collate]")?;
    let collate_duration = collate_start.elapsed();

    // === Step 7: Quantify ===
    info!("Quantifying...");
    let mut quant_cmd = std::process::Command::new(&alevin_fry_info.exe_path);
    quant_cmd
        .arg("quant")
        .arg("-i").arg(&quant_output)
        .arg("-o").arg(&quant_output)
        .arg("-m").arg(&t2g_path)
        .arg("-t").arg(format!("{}", opts.threads))
        .arg("-r").arg(&opts.resolution)
        .arg("--use-mtx");

    let quant_cmd_str = prog_utils::get_cmd_line_string(&quant_cmd);
    info!("quant cmd: {}", quant_cmd_str);
    let quant_start = Instant::now();
    exec::run_checked(&mut quant_cmd, "[quant]")?;
    let quant_duration = quant_start.elapsed();

    // === Write pipeline metadata ===
    let meta = json!({
        "chemistry": opts.chemistry,
        "organism": opts.organism.to_string(),
        "geometry": chem.geometry(),
        "resolution": opts.resolution,
        "threads": opts.threads,
        "kmer_length": opts.kmer_length,
        "index_path": index_path.display().to_string(),
        "cell_bc_path": cell_bc_path.display().to_string(),
        "sample_bc_path": sample_bc_path.display().to_string(),
        "t2g_path": t2g_path.display().to_string(),
        "map_cmd": map_cmd_str,
        "gpl_cmd": gpl_cmd_str,
        "collate_cmd": collate_cmd_str,
        "quant_cmd": quant_cmd_str,
        "map_duration_secs": map_duration.as_secs_f64(),
        "gpl_duration_secs": gpl_duration.as_secs_f64(),
        "collate_duration_secs": collate_duration.as_secs_f64(),
        "quant_duration_secs": quant_duration.as_secs_f64(),
        "total_duration_secs": start.elapsed().as_secs_f64(),
        "simpleaf_version": env!("CARGO_PKG_VERSION"),
    });
    let meta_path = output_dir.join("simpleaf_flex_quant_info.json");
    let meta_file = std::fs::File::create(&meta_path)?;
    serde_json::to_writer_pretty(meta_file, &meta)?;

    info!(
        "Flex GEX pipeline complete in {:.1}s. Output: {}",
        start.elapsed().as_secs_f64(),
        output_dir.display(),
    );

    Ok(())
}

/// Resolve the probe index: use provided, build from probe set, or auto-download.
fn resolve_probe_index(
    af_home: &Path,
    chem: &CustomChemistry,
    opts: &FlexQuantOpts,
    piscem_path: &Path,
) -> anyhow::Result<(PathBuf, PathBuf)> {
    // If user provided a pre-built index, use it directly.
    if let Some(ref index) = opts.index {
        info!("Using user-provided probe index: {}", index.display());
        let t2g_candidate = index.join("probe_t2g.tsv");
        let t2g = if t2g_candidate.exists() {
            t2g_candidate
        } else if let Some(ref ps) = opts.probe_set {
            let conv_dir = opts.output.join("probe_conversion");
            let (_, t2g, _) = convert_probe_csv_to_fasta(ps, &conv_dir)?;
            t2g
        } else {
            bail!(
                "When providing --index, a t2g map is also needed. \
                 Place probe_t2g.tsv in the index directory, or provide --probe-set."
            );
        };
        return Ok((index.clone(), t2g));
    }

    // If user provided a probe set file, build index from it.
    if let Some(ref probe_set) = opts.probe_set {
        warn!(
            "Using user-provided probe set: {}. This overrides the default for '{}'.",
            probe_set.display(),
            opts.chemistry,
        );
        return build_index_from_probe_set(probe_set, opts, piscem_path);
    }

    // Auto mode: look up probe set in chemistry registry
    let organism_key = opts.organism.to_string();
    let probe_sets = chem.probe_sets.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "Chemistry '{}' has no registered probe sets. Provide --probe-set.",
            opts.chemistry,
        )
    })?;
    let probe_info = probe_sets.get(&organism_key).ok_or_else(|| {
        anyhow::anyhow!(
            "No probe set for organism '{}' in chemistry '{}'. Provide --probe-set.",
            organism_key,
            opts.chemistry,
        )
    })?;

    // Check cache
    let cache_dir = af_home.join("probe_indices");
    std::fs::create_dir_all(&cache_dir)?;
    let cache_key = probe_info.plist_name.as_deref().unwrap_or("unknown");
    let cached_index = cache_dir.join(format!("{}_{}", cache_key, opts.kmer_length));

    if cached_index.join("index.ssi").exists() {
        let t2g = cached_index.join("probe_t2g.tsv");
        if t2g.exists() {
            info!("Using cached probe index: {}", cached_index.display());
            return Ok((cached_index.join("index"), t2g));
        }
    }

    // Download and build
    if let Some(ref url) = probe_info.remote_url {
        info!("Downloading probe set '{}'...", probe_info.name);
        let download_dir = cache_dir.join("downloads");
        std::fs::create_dir_all(&download_dir)?;
        let csv_path = download_dir.join(format!("{}.csv", probe_info.name));
        if !csv_path.exists() {
            prog_utils::download_to_file(url, &csv_path)?;
            info!("Downloaded probe set to {}", csv_path.display());
        }

        // Build index in the cache directory
        let mut build_opts = opts.clone();
        build_opts.output = cached_index.clone();
        let result = build_index_from_probe_set(&csv_path, &build_opts, piscem_path)?;
        Ok((result.0, result.1))
    } else {
        bail!(
            "No remote URL for probe set '{}'. Provide --probe-set.",
            probe_info.name,
        );
    }
}

/// Build a probe index from a CSV or FASTA file.
fn build_index_from_probe_set(
    probe_set: &Path,
    opts: &FlexQuantOpts,
    piscem_path: &Path,
) -> anyhow::Result<(PathBuf, PathBuf)> {
    let index_dir = opts.output.join("probe_index");
    std::fs::create_dir_all(&index_dir)?;

    let ext = probe_set
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let (fasta_path, t2g_path) = if ext.eq_ignore_ascii_case("csv") {
        info!("Converting probe CSV to FASTA: {}", probe_set.display());
        let (fa, t2g, _meta) = convert_probe_csv_to_fasta(probe_set, &index_dir)?;
        (fa, t2g)
    } else {
        // Assume FASTA — generate identity t2g
        let t2g = index_dir.join("probe_t2g.tsv");
        if !t2g.exists() {
            let fa_file = std::fs::File::open(probe_set)?;
            let reader = BufReader::new(fa_file);
            let mut t2g_writer = std::io::BufWriter::new(std::fs::File::create(&t2g)?);
            for line in reader.lines() {
                let line = line?;
                if let Some(name) = line.strip_prefix('>') {
                    let name = name.split_whitespace().next().unwrap_or(name);
                    let gene = name.split('|').next().unwrap_or(name);
                    writeln!(t2g_writer, "{}\t{}", name, gene)?;
                }
            }
        }
        (probe_set.to_path_buf(), t2g)
    };

    // Build piscem index
    info!("Building probe index with k={}...", opts.kmer_length);
    let index_prefix = index_dir.join("index");
    let mut build_cmd = std::process::Command::new(piscem_path);
    build_cmd
        .arg("build")
        .arg("-s").arg(&fasta_path)
        .arg("-o").arg(&index_prefix)
        .arg("-k").arg(format!("{}", opts.kmer_length))
        .arg("-t").arg(format!("{}", opts.threads))
        .arg("--overwrite");

    info!("piscem build cmd: {}", prog_utils::get_cmd_line_string(&build_cmd));
    exec::run_checked(&mut build_cmd, "[piscem build]")?;

    // Copy t2g alongside index
    let t2g_dest = index_dir.join("probe_t2g.tsv");
    if t2g_path != t2g_dest && t2g_path.exists() {
        std::fs::copy(&t2g_path, &t2g_dest)?;
    }

    Ok((index_prefix, t2g_dest))
}

/// Resolve cell barcode whitelist from the chemistry's plist_name/remote_url.
fn resolve_cell_bc_whitelist(af_home: &Path, chem: &CustomChemistry) -> anyhow::Result<PathBuf> {
    let plist_dir = af_home.join("plist");
    std::fs::create_dir_all(&plist_dir)?;

    if let Some(ref hash) = chem.plist_name {
        let cached = plist_dir.join(hash);
        if cached.exists() {
            info!("Cell barcode whitelist cached: {}", cached.display());
            return Ok(cached);
        }
    }

    if let Some(ref url) = chem.remote_pl_url {
        let dest = if let Some(ref hash) = chem.plist_name {
            plist_dir.join(hash)
        } else {
            plist_dir.join("cell_bc_whitelist.txt")
        };
        info!("Downloading cell barcode whitelist...");
        prog_utils::download_to_file(url, &dest)?;
        info!("Downloaded to {}", dest.display());
        Ok(dest)
    } else {
        bail!("Chemistry '{}' has no cell barcode whitelist URL.", chem.name());
    }
}

/// Resolve probe barcode (sample BC) file: use provided or auto-fetch.
fn resolve_sample_bc_list(
    af_home: &Path,
    chem: &CustomChemistry,
    opts: &FlexQuantOpts,
) -> anyhow::Result<PathBuf> {
    if let Some(ref path) = opts.sample_bc_list {
        info!("Using user-provided sample barcode list: {}", path.display());
        return Ok(path.clone());
    }

    let sbc_info = chem.sample_bc_list.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "Chemistry '{}' has no sample barcode list. Provide --sample-bc-list.",
            opts.chemistry,
        )
    })?;

    let plist_dir = af_home.join("plist");
    std::fs::create_dir_all(&plist_dir)?;

    if let Some(ref hash) = sbc_info.plist_name {
        let cached = plist_dir.join(hash);
        if cached.exists() {
            info!("Sample barcode list cached: {}", cached.display());
            return Ok(cached);
        }
    }

    if let Some(ref url) = sbc_info.remote_url {
        let dest = if let Some(ref hash) = sbc_info.plist_name {
            plist_dir.join(hash)
        } else {
            plist_dir.join("sample_bc_list.txt")
        };
        info!("Downloading sample barcode list...");
        prog_utils::download_to_file(url, &dest)?;
        info!("Downloaded to {}", dest.display());
        Ok(dest)
    } else {
        bail!("Chemistry '{}' has no sample barcode list URL. Provide --sample-bc-list.", opts.chemistry);
    }
}
