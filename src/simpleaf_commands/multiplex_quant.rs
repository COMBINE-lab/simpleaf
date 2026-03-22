//! Pipeline orchestration for multiplexed sample quantification.
//!
//! Handles any multiplexed protocol (10x Flex, custom multi-barcode, etc.):
//! 1. Resource resolution (index, cell BC whitelist, sample barcode file)
//! 2. Mapping with piscem
//! 3. Generate-permit-list (multi-barcode aware)
//! 4. Collate (hierarchical, multi-barcode)
//! 5. Quant with sample-prefixed output

use crate::core::{context, exec, index_meta};
use crate::simpleaf_commands::MultiplexQuantOpts;
use crate::utils::af_utils::IndexType;
use crate::utils::chem_utils::{CustomChemistry, CustomChemistryMap};
use crate::utils::constants::CHEMISTRIES_PATH;
use crate::utils::probe_utils;
use crate::utils::prog_utils;

use anyhow::{Context, bail};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{info, warn};

#[derive(Debug)]
struct ResolvedProbeSetFiles {
    fasta_path: PathBuf,
    gene_t2g_path: PathBuf,
    usa_t2g_path: Option<PathBuf>,
}

fn t2g_mode(opts: &MultiplexQuantOpts) -> probe_utils::ProbeT2gMode {
    if opts.usa {
        probe_utils::ProbeT2gMode::Usa
    } else {
        probe_utils::ProbeT2gMode::Gene
    }
}

fn select_probe_set_t2g(
    files: &ResolvedProbeSetFiles,
    mode: probe_utils::ProbeT2gMode,
) -> anyhow::Result<PathBuf> {
    match mode {
        probe_utils::ProbeT2gMode::Gene => Ok(files.gene_t2g_path.clone()),
        probe_utils::ProbeT2gMode::Usa => files.usa_t2g_path.clone().with_context(|| {
            format!(
                "USA-mode quantification was requested, but the supplied probe set `{}` does not provide splicing annotations. \
For probe CSV inputs, add a `region` column with values `spliced` and `unspliced`. FASTA probe sets do not carry this information. \
Provide a splicing-aware probe CSV or rerun without `--usa`.",
                files.fasta_path.display(),
            )
        }),
    }
}

fn prepare_probe_set_files(
    probe_set: &Path,
    output_dir: &Path,
) -> anyhow::Result<ResolvedProbeSetFiles> {
    let ext = probe_set.extension().and_then(|e| e.to_str()).unwrap_or("");

    if ext.eq_ignore_ascii_case("csv") {
        let converted = probe_utils::convert_probe_csv_to_reference_files(probe_set, output_dir)?;
        return Ok(ResolvedProbeSetFiles {
            fasta_path: converted.fasta_path,
            gene_t2g_path: converted.gene_t2g_path,
            usa_t2g_path: converted.usa_t2g_path,
        });
    }

    std::fs::create_dir_all(output_dir)?;
    let gene_t2g_path = output_dir.join("probe_t2g.tsv");
    if !gene_t2g_path.exists() {
        probe_utils::write_identity_t2g_from_fasta(probe_set, &gene_t2g_path)?;
    }
    Ok(ResolvedProbeSetFiles {
        fasta_path: probe_set.to_path_buf(),
        gene_t2g_path,
        usa_t2g_path: None,
    })
}

fn probe_index_base_exists(index_base: &Path) -> bool {
    prog_utils::check_piscem_index_files(index_base).is_ok()
}

fn existing_path(paths: &[PathBuf]) -> Option<PathBuf> {
    paths.iter().find(|p| p.exists()).cloned()
}

fn resolve_t2g_from_candidates(
    candidates: &[PathBuf],
    output_dir: &Path,
    mode: probe_utils::ProbeT2gMode,
) -> anyhow::Result<Option<PathBuf>> {
    let selected = match mode {
        probe_utils::ProbeT2gMode::Usa => existing_path(candidates),
        probe_utils::ProbeT2gMode::Gene => existing_path(candidates),
    };

    if let Some(candidate) = selected {
        return Ok(Some(probe_utils::ensure_t2g_mode(
            &candidate, output_dir, mode,
        )?));
    }
    Ok(None)
}

fn multiplex_t2g_candidates_for(
    t2g_hint: Option<PathBuf>,
    sibling_dir: Option<&Path>,
    mode: probe_utils::ProbeT2gMode,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(dir) = sibling_dir {
        match mode {
            probe_utils::ProbeT2gMode::Usa => {
                candidates.push(dir.join("probe_t2g_usa.tsv"));
                candidates.push(dir.join("t2g_3col.tsv"));
                candidates.push(dir.join("probe_t2g.tsv"));
                candidates.push(dir.join("t2g.tsv"));
            }
            probe_utils::ProbeT2gMode::Gene => {
                candidates.push(dir.join("probe_t2g.tsv"));
                candidates.push(dir.join("t2g.tsv"));
                candidates.push(dir.join("probe_t2g_usa.tsv"));
                candidates.push(dir.join("t2g_3col.tsv"));
            }
        }
    }

    if let Some(t2g) = t2g_hint {
        candidates.push(t2g);
    }

    candidates
}

fn resolve_user_supplied_index(
    index: &Path,
    output_dir: &Path,
    mode: probe_utils::ProbeT2gMode,
) -> anyhow::Result<(PathBuf, Option<PathBuf>)> {
    let simpleaf_index_dir = if index.join("simpleaf_index.json").exists() {
        Some(index.to_path_buf())
    } else if index.join("index").join("simpleaf_index.json").exists() {
        Some(index.join("index"))
    } else if !index.is_dir()
        && index
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n == "piscem_idx")
            .unwrap_or(false)
        && index
            .parent()
            .map(|p| p.join("simpleaf_index.json").exists())
            .unwrap_or(false)
    {
        index.parent().map(Path::to_path_buf)
    } else {
        None
    };

    if let Some(index_dir) = simpleaf_index_dir {
        let meta = index_meta::resolve_quant_index(Some(index_dir.clone()))?;
        let index_path = match meta.index_type {
            IndexType::Piscem(path) => path,
            IndexType::NoIndex => {
                bail!("Could not resolve a piscem index from {}.", index.display())
            }
        };
        let candidates =
            multiplex_t2g_candidates_for(meta.inferred_t2g, Some(index_dir.as_path()), mode);
        let t2g = resolve_t2g_from_candidates(&candidates, output_dir, mode)?;
        return Ok((index_path, t2g));
    }

    if index.is_dir() {
        let prefix = index.join("index");
        if probe_index_base_exists(&prefix) {
            let candidates = multiplex_t2g_candidates_for(None, Some(index), mode);
            let t2g = resolve_t2g_from_candidates(&candidates, output_dir, mode)?;
            return Ok((prefix, t2g));
        }
    }

    if probe_index_base_exists(index) {
        let sibling_dir = index.parent();
        let candidates = multiplex_t2g_candidates_for(None, sibling_dir, mode);
        let t2g = resolve_t2g_from_candidates(&candidates, output_dir, mode)?;
        return Ok((index.to_path_buf(), t2g));
    }

    bail!(
        "Could not resolve a valid piscem index from {}.",
        index.display()
    );
}

/// Main entry point for the multiplex-quant pipeline.
pub fn multiplex_map_and_quant(af_home: &Path, opts: MultiplexQuantOpts) -> anyhow::Result<()> {
    let start = Instant::now();
    info!("Starting multiplex quantification pipeline");

    // Load runtime context (program paths)
    let rt = context::load_runtime_context(af_home)?;
    let piscem_info = rt
        .progs
        .piscem
        .as_ref()
        .context("piscem is required for mapping; please run `simpleaf set-paths`")?;
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

        Some(c)
    } else {
        None
    };

    // Resolve geometry: CLI override > chemistry default > error
    let geometry = opts
        .geometry
        .as_deref()
        .or_else(|| chem.as_ref().map(|c| c.geometry()))
        .ok_or_else(|| {
            anyhow::anyhow!("No geometry specified. Provide --geometry or --chemistry.")
        })?
        .to_string();

    // Resolve expected orientation
    let expected_ori = &opts.expected_ori;

    info!(
        "Geometry: {}, orientation: {}{}",
        geometry,
        expected_ori,
        opts.chemistry
            .as_ref()
            .map(|c| format!(" (chemistry: {})", c))
            .unwrap_or_default(),
    );

    // Create output directory structure
    let output_dir = &opts.output;
    std::fs::create_dir_all(output_dir)?;
    let map_output = output_dir.join("af_map");
    let quant_output = output_dir.join("af_quant");
    std::fs::create_dir_all(&map_output)?;
    std::fs::create_dir_all(&quant_output)?;

    // === Step 1: Resolve index and t2g ===
    let (index_path, probe_t2g_path_opt) = resolve_probe_index(
        af_home,
        chem.as_ref(),
        &opts,
        &piscem_info.exe_path,
        t2g_mode(&opts),
    )?;
    // --t2g-map overrides any probe-derived or inferred t2g.
    let t2g_path = if let Some(t2g_map) = opts.t2g_map.clone() {
        t2g_map
    } else {
        probe_t2g_path_opt.context(
            "A transcript-to-gene map could not be inferred from the supplied multiplex reference. Provide --t2g-map, or provide a probe set / index layout with an adjacent t2g file.",
        )?
    };

    // === Step 2: Resolve cell barcode whitelist ===
    let cell_bc_path = if let Some(ref user_list) = opts.cell_bc_list {
        info!(
            "Using user-provided cell barcode whitelist: {}",
            user_list.display()
        );
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
        .arg("-i")
        .arg(&index_path)
        .arg("-g")
        .arg(&geometry)
        .arg("-o")
        .arg(&map_output)
        .arg("-t")
        .arg(format!("{}", opts.threads));

    if opts.struct_constraints {
        piscem_cmd.arg("--struct-constraints");
    }
    piscem_cmd
        .arg("--skipping-strategy")
        .arg(&opts.skipping_strategy)
        .arg("--max-ec-card")
        .arg(format!("{}", opts.max_ec_card));

    let r1_str: Vec<String> = opts
        .reads1
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    let r2_str: Vec<String> = opts
        .reads2
        .iter()
        .map(|p| p.display().to_string())
        .collect();
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
        .arg("-i")
        .arg(&map_output)
        .arg("-d")
        .arg(expected_ori)
        .arg("-o")
        .arg(&quant_output)
        .arg("-t")
        .arg(format!("{}", opts.threads.min(8)))
        .arg("--unfiltered-pl")
        .arg(&cell_bc_path)
        .arg("--sample-bc-list")
        .arg(&sample_bc_path)
        .arg("--sample-correction-mode")
        .arg(&opts.sample_correction_mode)
        .arg("--min-reads")
        .arg(format!("{}", opts.min_reads));

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
        .arg("-i")
        .arg(&quant_output)
        .arg("-r")
        .arg(&map_output)
        .arg("-t")
        .arg(format!("{}", opts.threads));

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
        .arg("-i")
        .arg(&quant_output)
        .arg("-o")
        .arg(&quant_output)
        .arg("-m")
        .arg(&t2g_path)
        .arg("-t")
        .arg(format!("{}", opts.threads))
        .arg("-r")
        .arg(&opts.resolution)
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
    let meta_path = output_dir.join("simpleaf_multiplex_quant_info.json");
    let meta_file = std::fs::File::create(&meta_path)?;
    serde_json::to_writer_pretty(meta_file, &meta)?;

    info!(
        "Multiplex pipeline complete in {:.1}s. Output: {}",
        start.elapsed().as_secs_f64(),
        output_dir.display(),
    );

    Ok(())
}

/// Resolve the probe index: use provided, build from probe set, or auto-download.
fn resolve_probe_index(
    af_home: &Path,
    chem: Option<&CustomChemistry>,
    opts: &MultiplexQuantOpts,
    piscem_path: &Path,
    mode: probe_utils::ProbeT2gMode,
) -> anyhow::Result<(PathBuf, Option<PathBuf>)> {
    // If user provided a pre-built index, use it directly.
    if let Some(ref index) = opts.index {
        info!("Using user-provided probe index: {}", index.display());
        let (index_path, inferred_t2g) =
            resolve_user_supplied_index(index, &opts.output.join("resolved_t2g"), mode)?;
        let t2g = if inferred_t2g.is_some() {
            inferred_t2g
        } else if let Some(ref ps) = opts.probe_set {
            let conv_dir = opts.output.join("probe_conversion");
            let probe_set_files = prepare_probe_set_files(ps, &conv_dir)?;
            Some(select_probe_set_t2g(&probe_set_files, mode)?)
        } else {
            None
        };
        return Ok((index_path, t2g));
    }

    // If user provided a probe set file, build index from it.
    if let Some(ref probe_set) = opts.probe_set {
        warn!("Using user-provided probe set: {}.", probe_set.display(),);
        let (index_path, t2g) = build_index_from_probe_set(probe_set, opts, piscem_path, mode)?;
        return Ok((index_path, Some(t2g)));
    }

    // Auto mode: look up probe set in chemistry registry
    let chem_ref = chem.ok_or_else(|| {
        anyhow::anyhow!("No chemistry specified and no --probe-set or --index provided.")
    })?;
    let organism = opts.organism.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "--organism is required when auto-downloading probe sets from the chemistry registry."
        )
    })?;
    let organism_key = organism.to_string();
    let probe_sets = chem_ref.probe_sets.as_ref().ok_or_else(|| {
        anyhow::anyhow!("Chemistry has no registered probe sets. Provide --probe-set.",)
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
    let cached_probe_index_dir = cached_index.join("probe_index");
    let cached_probe_index = cached_probe_index_dir.join("index");
    if probe_index_base_exists(&cached_probe_index) {
        let candidates = multiplex_t2g_candidates_for(None, Some(&cached_probe_index_dir), mode);
        let t2g =
            resolve_t2g_from_candidates(&candidates, &opts.output.join("resolved_t2g"), mode)?;
        info!("Using cached probe index: {}", cached_probe_index.display());
        return Ok((cached_probe_index, t2g));
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
        let result = build_index_from_probe_set(&csv_path, &build_opts, piscem_path, mode)?;
        Ok((result.0, Some(result.1)))
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
    opts: &MultiplexQuantOpts,
    piscem_path: &Path,
    mode: probe_utils::ProbeT2gMode,
) -> anyhow::Result<(PathBuf, PathBuf)> {
    let index_dir = opts.output.join("probe_index");
    std::fs::create_dir_all(&index_dir)?;
    let probe_set_files = prepare_probe_set_files(probe_set, &index_dir)?;
    let fasta_path = probe_set_files.fasta_path.clone();
    let t2g_path = select_probe_set_t2g(&probe_set_files, mode)?;

    // Build piscem index
    info!("Building probe index with k={}...", opts.kmer_length);
    let index_prefix = index_dir.join("index");
    let mut build_cmd = std::process::Command::new(piscem_path);
    build_cmd
        .arg("build")
        .arg("-s")
        .arg(&fasta_path)
        .arg("-o")
        .arg(&index_prefix)
        .arg("-k")
        .arg(format!("{}", opts.kmer_length))
        .arg("-t")
        .arg(format!("{}", opts.threads))
        .arg("--overwrite");

    info!(
        "piscem build cmd: {}",
        prog_utils::get_cmd_line_string(&build_cmd)
    );
    exec::run_checked(&mut build_cmd, "[piscem build]")?;

    Ok((index_prefix, t2g_path))
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
        bail!(
            "Chemistry '{}' has no cell barcode whitelist URL.",
            chem.name()
        );
    }
}

/// Resolve probe barcode (sample BC) file: use provided or auto-fetch.
fn resolve_sample_bc_list(
    af_home: &Path,
    chem: Option<&CustomChemistry>,
    opts: &MultiplexQuantOpts,
) -> anyhow::Result<PathBuf> {
    if let Some(ref path) = opts.sample_bc_list {
        info!(
            "Using user-provided sample barcode list: {}",
            path.display()
        );
        return Ok(path.clone());
    }

    let chem_ref = chem.ok_or_else(|| {
        anyhow::anyhow!("No chemistry specified and no --sample-bc-list provided.")
    })?;
    let sbc_info = chem_ref.sample_bc_list.as_ref().ok_or_else(|| {
        anyhow::anyhow!("Chemistry has no sample barcode list. Provide --sample-bc-list.",)
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

#[cfg(test)]
mod tests {
    use super::{resolve_user_supplied_index, t2g_mode};
    use crate::simpleaf_commands::MultiplexQuantOpts;
    use crate::utils::probe_utils::ProbeT2gMode;
    use serde_json::json;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn write_piscem_index(prefix: &Path) {
        fs::write(prefix.with_extension("ctab"), "").expect("failed to write ctab");
        fs::write(prefix.with_extension("refinfo"), "").expect("failed to write refinfo");
        fs::write(prefix.with_extension("ssi"), "").expect("failed to write ssi");
        fs::write(prefix.with_extension("ssi.mphf"), "").expect("failed to write ssi.mphf");
    }

    #[test]
    fn resolves_simpleaf_index_directory_and_collapses_t2g_by_default() {
        let td = tempdir().expect("failed to create tempdir");
        let output_root = td.path().join("simpleaf_output");
        let index_dir = output_root.join("index");
        fs::create_dir_all(&index_dir).expect("failed to create index dir");
        let index_prefix = index_dir.join("piscem_idx");
        write_piscem_index(&index_prefix);
        fs::write(index_dir.join("t2g_3col.tsv"), "P1\tG1\tS\n").expect("failed to write t2g");
        fs::write(
            index_dir.join("simpleaf_index.json"),
            serde_json::to_string_pretty(&json!({
                "index_type": "piscem",
                "t2g_file": "t2g_3col.tsv"
            }))
            .expect("failed to serialize index json"),
        )
        .expect("failed to write simpleaf index json");

        let (resolved_index, t2g) = resolve_user_supplied_index(
            &output_root,
            &td.path().join("resolved"),
            ProbeT2gMode::Gene,
        )
        .expect("failed to resolve simpleaf index");

        assert_eq!(resolved_index, index_prefix);
        let t2g = t2g.expect("t2g should have been inferred");
        assert_eq!(
            fs::read_to_string(t2g).expect("failed to read inferred t2g"),
            "P1\tG1\n"
        );
    }

    #[test]
    fn resolves_probe_index_directory_and_preserves_usa_t2g() {
        let td = tempdir().expect("failed to create tempdir");
        let probe_index_dir = td.path().join("probe_index");
        fs::create_dir_all(&probe_index_dir).expect("failed to create probe index dir");
        let index_prefix = probe_index_dir.join("index");
        write_piscem_index(&index_prefix);
        fs::write(probe_index_dir.join("probe_t2g.tsv"), "P1\tG1\n")
            .expect("failed to write gene t2g");
        fs::write(probe_index_dir.join("probe_t2g_usa.tsv"), "P1\tG1\tS\n")
            .expect("failed to write USA t2g");

        let (resolved_index, t2g) = resolve_user_supplied_index(
            &probe_index_dir,
            &td.path().join("resolved"),
            ProbeT2gMode::Usa,
        )
        .expect("failed to resolve probe index dir");

        assert_eq!(resolved_index, index_prefix);
        assert_eq!(
            fs::read_to_string(t2g.expect("missing USA t2g")).expect("failed to read USA t2g"),
            "P1\tG1\tS\n"
        );
    }

    #[test]
    fn usa_flag_maps_to_usa_mode() {
        let opts = MultiplexQuantOpts {
            chemistry: None,
            geometry: None,
            organism: None,
            cell_bc_list: None,
            expected_ori: String::from("both"),
            sample_correction_mode: String::from("exact"),
            output: Path::new(".").to_path_buf(),
            threads: 1,
            index: None,
            probe_set: None,
            t2g_map: None,
            usa: true,
            sample_bc_list: None,
            reads1: Vec::new(),
            reads2: Vec::new(),
            resolution: String::from("cr-like"),
            kmer_length: 23,
            skipping_strategy: String::from("permissive"),
            struct_constraints: false,
            max_ec_card: 4096,
            min_reads: 10,
        };

        assert_eq!(t2g_mode(&opts), ProbeT2gMode::Usa);
    }

    #[test]
    fn usa_mode_without_probe_regions_returns_helpful_error() {
        let files = super::ResolvedProbeSetFiles {
            fasta_path: Path::new("/tmp/custom_probes.fa").to_path_buf(),
            gene_t2g_path: Path::new("/tmp/probe_t2g.tsv").to_path_buf(),
            usa_t2g_path: None,
        };

        let err = super::select_probe_set_t2g(&files, ProbeT2gMode::Usa)
            .expect_err("USA mode should require splicing annotations");
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("region") && msg.contains("rerun without `--usa`"),
            "unexpected error: {msg}",
        );
    }
}
