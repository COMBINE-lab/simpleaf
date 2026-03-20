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
use crate::utils::probe_utils;
use crate::utils::prog_utils;

use anyhow::{Context, bail};
use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{info, warn};

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

    // === Layered resolution: chemistry provides defaults, CLI flags override ===

    // Load chemistry from registry (optional)
    let chem: Option<CustomChemistry> = if let Some(ref chem_name) = opts.chemistry {
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

        let c = chem_map
            .get(chem_name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Chemistry '{}' not found in registry. Run `simpleaf chemistry refresh`.",
                    chem_name,
                )
            })?
            .clone();

        if !c.is_flex_gex() {
            bail!(
                "Chemistry '{}' is not a Flex GEX protocol. Use `simpleaf quant` for standard chemistries.",
                chem_name,
            );
        }
        Some(c)
    } else {
        None
    };

    // Resolve geometry: CLI override > chemistry default > error
    let geometry = opts.geometry.as_deref()
        .or_else(|| chem.as_ref().map(|c| c.geometry()))
        .ok_or_else(|| anyhow::anyhow!(
            "No geometry specified. Provide --geometry or --chemistry."
        ))?
        .to_string();

    // Resolve expected orientation
    let expected_ori = &opts.expected_ori;

    info!(
        "Geometry: {}, orientation: {}{}",
        geometry,
        expected_ori,
        opts.chemistry.as_ref().map(|c| format!(" (chemistry: {})", c)).unwrap_or_default(),
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
        af_home, chem.as_ref(), &opts, &piscem_info.exe_path,
    )?;

    // === Step 2: Resolve cell barcode whitelist ===
    let cell_bc_path = if let Some(ref user_list) = opts.cell_bc_list {
        info!("Using user-provided cell barcode whitelist: {}", user_list.display());
        user_list.clone()
    } else if let Some(ref c) = chem {
        resolve_cell_bc_whitelist(af_home, c)?
    } else {
        bail!("No cell barcode whitelist specified. Provide --cell-bc-list or --chemistry.");
    };

    // === Step 3: Resolve probe barcode (sample BC) file ===
    let sample_bc_path = resolve_sample_bc_list(af_home, chem.as_ref(), &opts)?;

    // === Step 4: Map reads with piscem ===
    info!("Mapping reads with piscem...");
    let mut piscem_cmd = std::process::Command::new(&piscem_info.exe_path);
    piscem_cmd
        .arg("map-sc")
        .arg("-i").arg(&index_path)
        .arg("-g").arg(&geometry)
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
        .arg("-d").arg(expected_ori)
        .arg("-o").arg(&quant_output)
        .arg("-t").arg(format!("{}", opts.threads.min(8)))
        .arg("--unfiltered-pl").arg(&cell_bc_path)
        .arg("--sample-bc-list").arg(&sample_bc_path)
        .arg("--sample-correction-mode").arg(&opts.sample_correction_mode)
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
        "organism": opts.organism.as_ref().map(|o| o.to_string()),
        "geometry": geometry,
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
    chem: Option<&CustomChemistry>,
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
            let (_, t2g, _) = probe_utils::convert_probe_csv_to_fasta(ps, &conv_dir)?;
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
            "Using user-provided probe set: {}.",
            probe_set.display(),
        );
        return build_index_from_probe_set(probe_set, opts, piscem_path);
    }

    // Auto mode: look up probe set in chemistry registry
    let chem_ref = chem.ok_or_else(|| {
        anyhow::anyhow!("No chemistry specified and no --probe-set or --index provided.")
    })?;
    let organism = opts.organism.as_ref().ok_or_else(|| {
        anyhow::anyhow!("--organism is required when auto-downloading probe sets from the chemistry registry.")
    })?;
    let organism_key = organism.to_string();
    let probe_sets = chem_ref.probe_sets.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "Chemistry has no registered probe sets. Provide --probe-set.",
        )
    })?;
    let probe_info = probe_sets.get(&organism_key).ok_or_else(|| {
        anyhow::anyhow!(
            "No probe set for organism '{}'. Provide --probe-set.",
            organism_key,
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
        let (fa, t2g, _meta) = probe_utils::convert_probe_csv_to_fasta(probe_set, &index_dir)?;
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
    chem: Option<&CustomChemistry>,
    opts: &FlexQuantOpts,
) -> anyhow::Result<PathBuf> {
    if let Some(ref path) = opts.sample_bc_list {
        info!("Using user-provided sample barcode list: {}", path.display());
        return Ok(path.clone());
    }

    let chem_ref = chem.ok_or_else(|| {
        anyhow::anyhow!("No chemistry specified and no --sample-bc-list provided.")
    })?;
    let sbc_info = chem_ref.sample_bc_list.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "Chemistry has no sample barcode list. Provide --sample-bc-list.",
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
        bail!("Chemistry has no sample barcode list URL. Provide --sample-bc-list.");
    }
}
