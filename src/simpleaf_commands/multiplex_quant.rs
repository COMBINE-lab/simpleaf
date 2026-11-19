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
use crate::utils::chem_utils::{CustomChemistry, CustomChemistryMap, ProbeSetInfo};
use crate::utils::constants::{CHEMISTRIES_PATH, CHEMISTRIES_URL};
use crate::utils::probe_index_cache;
use crate::utils::probe_utils;
use crate::utils::prog_parsing_utils;
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
    gene_id_to_name_path: Option<PathBuf>,
    /// Excluded probes to index as decoys (see `ExcludedProbeMode`); `None`
    /// for FASTA probe sets, `--excluded-probes ignore`, or a CSV with no
    /// excluded probes.
    decoy_fasta_path: Option<PathBuf>,
}

/// A resolved probe index: the index prefix, the t2g, the gene-name file and
/// the original probe-set CSV (copied into `af_quant/` as `probe_set.csv`).
type ResolvedIndex = (PathBuf, Option<PathBuf>, Option<PathBuf>, Option<PathBuf>);

fn t2g_mode(opts: &MultiplexQuantOpts) -> probe_utils::ProbeT2gMode {
    if opts.usa {
        probe_utils::ProbeT2gMode::Usa
    } else {
        probe_utils::ProbeT2gMode::Gene
    }
}

/// Whether the reference is a probe set: a `--probe-set` CSV or FASTA (with or
/// without a pre-built `--index` built from it), the chemistry's registered
/// probe set, or a pre-built `--index` that identifies itself as a probe index
/// (see `index_holds_probe_set`). A `--t2g-map` marks a transcriptome
/// reference, and any other pre-built `--index` could hold either, so neither
/// counts.
fn is_probe_set_reference(opts: &MultiplexQuantOpts) -> bool {
    opts.t2g_map.is_none()
        && (opts.probe_set.is_some() || opts.index.as_deref().is_none_or(index_holds_probe_set))
}

/// Whether a pre-built index was built by simpleaf from a probe CSV, read off
/// the metadata simpleaf writes beside it, for every layout `--index` accepts:
///
/// - a multiplex-quant probe index (a directory, or its `index` prefix) has
///   `probe_set_info.json` next to the index files;
/// - a `simpleaf index --probe-csv` output (its root, its `index/`
///   subdirectory, or the `piscem_idx` prefix inside that) records the CSV as
///   `args.probe-csv` in `index_info.json` at the root.
///
/// Anything else, including a probe index built from a FASTA, which records
/// neither, is not recognised.
fn index_holds_probe_set(index: &Path) -> bool {
    let index_dir = if index.is_dir() {
        Some(index)
    } else {
        index.parent()
    };
    let Some(index_dir) = index_dir else {
        return false;
    };
    if index_dir.join("probe_set_info.json").is_file() {
        return true;
    }
    let records_probe_csv = |root: &Path| {
        std::fs::read_to_string(root.join("index_info.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .is_some_and(|info| info["args"]["probe-csv"].is_string())
    };
    records_probe_csv(index_dir) || index_dir.parent().is_some_and(records_probe_csv)
}

/// Why a run is quantified per gene by alevin-fry's `quant` rather than per
/// probe by `quant-probe`, which resolves UMIs over the probes the reads hit
/// and then sums each gene's probes into its count.
///
/// Per probe is the default for a probe-set reference, which is what such a
/// reference describes, so these are the exceptions. The variant is the single
/// answer to "which axis, and why": the quant step, the log and the
/// `--probe-mtx` refusal all read it, instead of each re-deriving the
/// conditions and drifting apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeneAxis {
    /// `--t2g-map`: a transcriptome, where resolving per target would discard
    /// UMIs shared by the isoforms of one gene.
    Transcriptome,
    /// A pre-built `--index` without `--probe-set` that does not identify
    /// itself as a probe index, so it could hold either kind of reference; the
    /// gene axis is the reading that is never wrong.
    UnknownIndex,
    /// `--usa`: USA mode resolves over a 3-column map, which `quant-probe`
    /// rejects outright.
    Usa,
    /// `--gene-level`: the gene axis was asked for, to reproduce an earlier run.
    GeneLevel,
    /// The configured alevin-fry has no `quant-probe` subcommand.
    NoQuantProbe,
}

impl GeneAxis {
    /// Why `--probe-mtx` cannot be honoured, phrased as the thing to change.
    fn probe_mtx_reason(self, alevin_fry: &Path) -> String {
        match self {
            GeneAxis::Transcriptome => "`--t2g-map` marks a transcriptome reference, which has \
                 no probes; rerun without `--probe-mtx`"
                .to_string(),
            GeneAxis::UnknownIndex => "this run uses a pre-built `--index` without \
                 `--probe-set` that does not say whether it holds a probe set or a \
                 transcriptome; pass the probe set the index was built from with \
                 `--probe-set`, or rerun without `--probe-mtx`"
                .to_string(),
            GeneAxis::Usa => "`--usa` runs `quant` over a 3-column map, which has no probe \
                 axis; rerun without `--usa`"
                .to_string(),
            GeneAxis::GeneLevel => {
                "`--gene-level` runs `quant`; rerun without `--gene-level`".to_string()
            }
            GeneAxis::NoQuantProbe => format!(
                "`{}` has no `quant-probe` subcommand. Install an alevin-fry that has it and \
                 re-run `simpleaf set-paths`, or rerun without `--probe-mtx`",
                alevin_fry.display()
            ),
        }
    }
}

/// `None` when the run is quantified per probe.
///
/// The order is the order the user would have to act in, so that the reason
/// reported is always one they must fix: a reference with no probes cannot be
/// rescued by dropping a flag, a flag that forces the gene axis has to go
/// before the installed alevin-fry matters at all, and only then is the
/// install the one thing left to change.
fn gene_axis_reason(
    opts: &MultiplexQuantOpts,
    af_caps: &prog_utils::AlevinFryCapabilities,
) -> Option<GeneAxis> {
    if !is_probe_set_reference(opts) {
        // Which shape it is decides the advice, not the outcome.
        Some(if opts.t2g_map.is_some() {
            GeneAxis::Transcriptome
        } else {
            GeneAxis::UnknownIndex
        })
    } else if opts.usa {
        Some(GeneAxis::Usa)
    } else if opts.gene_level {
        Some(GeneAxis::GeneLevel)
    } else if !af_caps.quant_probe {
        Some(GeneAxis::NoQuantProbe)
    } else {
        None
    }
}

/// Reject, before any work starts, the options this run cannot honour with the
/// configured alevin-fry. Without these checks each one fails only at the final
/// quant step, after mapping, permit-list generation and collation.
fn validate_alevin_fry_options(
    opts: &MultiplexQuantOpts,
    af_caps: &prog_utils::AlevinFryCapabilities,
    gene_axis: Option<GeneAxis>,
    alevin_fry: &Path,
) -> anyhow::Result<()> {
    let per_probe = gene_axis.is_none();
    if per_probe {
        let gene_level_alternative = match opts.resolution.as_str() {
            "parsimony-gene" => Some("parsimony"),
            "parsimony-gene-em" => Some("parsimony-em"),
            _ => None,
        };
        if let Some(alternative) = gene_level_alternative {
            bail!(
                "`--resolution {}` resolves UMIs over genes, but a probe-set reference is \
                 quantified per probe, so no per-probe assignment would survive to sum. Use \
                 `--resolution {}` instead -- probe counts are still summed into genes in the \
                 output matrix -- or `--gene-level` to quantify per gene as before.",
                opts.resolution,
                alternative
            );
        }
    }
    crate::utils::af_utils::check_umi_edit_dist(
        &opts.resolution,
        opts.umi_edit_dist,
        opts.small_thresh,
        opts.usa.then_some("`--usa` was given"),
    )?;
    if opts.dump_molecules {
        // Checked against the subcommand this run will actually invoke, not
        // against `quant` always: the two need not carry the same options.
        let (subcommand, defined) = if per_probe {
            ("quant-probe", af_caps.quant_probe_dump_molecules)
        } else {
            ("quant", af_caps.quant_dump_molecules)
        };
        if !defined {
            return Err(prog_utils::missing_alevin_fry_option(
                alevin_fry,
                subcommand,
                "--dump-molecules",
            ));
        }
    }
    if opts.probe_mtx {
        // The probe matrix is the one `quant-probe` summed the gene matrix
        // from, so it exists only on a run that goes there. Name the reason
        // this run does not: a bare "needs a probe-set reference" would not
        // say which of the four to change.
        if let Some(reason) = gene_axis {
            bail!(
                "`--probe-mtx` needs a run quantified per probe, but {}.",
                reason.probe_mtx_reason(alevin_fry)
            );
        }
        if !af_caps.probe_mtx {
            return Err(prog_utils::missing_alevin_fry_option(
                alevin_fry,
                "quant-probe",
                "--probe-mtx",
            ));
        }
    }
    Ok(())
}

fn resolved_sample_bc_orientation<'a>(
    opts: &'a MultiplexQuantOpts,
    chemistry: Option<&'a CustomChemistry>,
) -> Option<&'a str> {
    opts.sample_bc_ori.as_deref().or_else(|| {
        chemistry
            .and_then(|chemistry| chemistry.sample_bc_list.as_ref())
            .and_then(|sample_list| sample_list.sample_bc_ori.as_deref())
    })
}

fn write_multiplex_metadata(
    output_dir: &Path,
    quant_output: &Path,
    meta: &serde_json::Value,
) -> anyhow::Result<()> {
    let meta_path = output_dir.join("simpleaf_multiplex_quant_info.json");
    let meta_file = std::fs::File::create(&meta_path)?;
    serde_json::to_writer_pretty(meta_file, meta)?;
    let quant_meta_path = quant_output.join("simpleaf_multiplex_quant_info.json");
    let quant_meta_file = std::fs::File::create(&quant_meta_path)?;
    serde_json::to_writer_pretty(quant_meta_file, meta)?;
    Ok(())
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
    excluded_mode: probe_utils::ExcludedProbeMode,
) -> anyhow::Result<ResolvedProbeSetFiles> {
    if is_probe_csv(probe_set) {
        let converted = probe_utils::convert_probe_csv_to_reference_files(
            probe_set,
            output_dir,
            excluded_mode,
        )?;
        return Ok(ResolvedProbeSetFiles {
            fasta_path: converted.fasta_path,
            gene_t2g_path: converted.gene_t2g_path,
            usa_t2g_path: converted.usa_t2g_path,
            gene_id_to_name_path: converted.gene_id_to_name_path,
            decoy_fasta_path: converted.decoy_fasta_path,
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
        gene_id_to_name_path: None,
        decoy_fasta_path: None,
    })
}

/// Whether a probe set file is a 10x probe-set CSV, not a FASTA.
fn is_probe_csv(probe_set: &Path) -> bool {
    let ext = probe_set.extension().and_then(|e| e.to_str()).unwrap_or("");
    ext.eq_ignore_ascii_case("csv")
}

fn probe_index_base_exists(index_base: &Path) -> bool {
    prog_utils::check_piscem_index_files(index_base).is_ok()
}

fn existing_path(paths: &[PathBuf]) -> Option<PathBuf> {
    paths.iter().find(|p| p.exists()).cloned()
}

fn gene_id_to_name_for_dir(dir: Option<&Path>) -> Option<PathBuf> {
    dir.and_then(|d| {
        let candidate = d.join("gene_id_to_name.tsv");
        candidate.exists().then_some(candidate)
    })
}

/// Check, before mapping, the probe-set CSV that the run copies into
/// `af_quant/` after quant: fail now if it does not exist, and tell the user
/// when a probe-set run has no CSV to copy. The probe h5ad (`probe_h5ad`)
/// needs the CSV to map probes to genes, so without one it is an error;
/// `fasta_probe_set` tells if the user gave a FASTA `--probe-set`.
fn check_probe_set_source(
    probe_set: Option<&Path>,
    probe_reference: bool,
    probe_h5ad: bool,
    fasta_probe_set: bool,
) -> anyhow::Result<()> {
    match probe_set {
        Some(p) if !p.is_file() => bail!("The probe-set CSV {} does not exist.", p.display()),
        Some(_) => {}
        None if probe_h5ad && fasta_probe_set => bail!(
            "--anndata-out with --probe-mtx writes the probe h5ad, but a FASTA probe set has no probe-to-gene map; give the 10x probe-set CSV with --probe-set."
        ),
        None if probe_h5ad => bail!(
            "--anndata-out with --probe-mtx writes the probe h5ad, but this index has no probe_set.csv (built before simpleaf 0.33.0); give the 10x probe-set CSV with --probe-set (it can be used together with --index <index dir>)."
        ),
        None if probe_reference => info!(
            "af_quant/probe_set.csv is not written because the index has no probe-set CSV; give one with --probe-set."
        ),
        None => {}
    }
    Ok(())
}

/// Write the probe matrix of a `--probe-mtx` run as `alevin/probe_quants.h5ad`,
/// with `var` from the probe-set CSV, and return its path.
fn write_probe_h5ad(quant_output: &Path, probe_set: Option<&Path>) -> anyhow::Result<PathBuf> {
    let probe_path = quant_output.join("alevin").join("probe_quants.h5ad");
    af_anndata::convert_probe_csr_to_anndata(
        quant_output,
        probe_path.as_path(),
        probe_set,
        af_anndata::ConvertOpts::default(),
    )?;
    Ok(probe_path)
}

/// Copy the original probe-set CSV, if there is one, into the quant output
/// dir as `probe_set.csv`, as `gene_id_to_name.tsv` is copied.
fn copy_probe_set_into(probe_set: Option<&Path>, quant_output: &Path) -> anyhow::Result<()> {
    if let Some(probe_set_path) = probe_set {
        let target_path = quant_output.join("probe_set.csv");
        // The CSV may already be this copy (`--probe-set af_quant/probe_set.csv`).
        if probe_utils::same_file(probe_set_path, &target_path) {
            return Ok(());
        }
        std::fs::copy(probe_set_path, &target_path).with_context(|| {
            format!(
                "could not copy the probe set from {} to {}",
                probe_set_path.display(),
                target_path.display()
            )
        })?;
    }
    Ok(())
}

/// The copy of the original probe-set CSV kept in a probe index dir.
fn probe_set_for_dir(dir: Option<&Path>) -> Option<PathBuf> {
    dir.and_then(|d| {
        let candidate = d.join("probe_set.csv");
        candidate.exists().then_some(candidate)
    })
}

fn resolve_t2g_from_candidates(
    candidates: &[PathBuf],
    output_dir: &Path,
    mode: probe_utils::ProbeT2gMode,
) -> anyhow::Result<Option<PathBuf>> {
    if let Some(candidate) = existing_path(candidates) {
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
) -> anyhow::Result<ResolvedIndex> {
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
        return Ok((
            index_path,
            t2g,
            meta.inferred_gene_id_to_name,
            meta.inferred_probe_set,
        ));
    }

    if index.is_dir() {
        let prefix = index.join("index");
        if probe_index_base_exists(&prefix) {
            let candidates = multiplex_t2g_candidates_for(None, Some(index), mode);
            let t2g = resolve_t2g_from_candidates(&candidates, output_dir, mode)?;
            return Ok((
                prefix,
                t2g,
                gene_id_to_name_for_dir(Some(index)),
                probe_set_for_dir(Some(index)),
            ));
        }
    }

    if probe_index_base_exists(index) {
        let sibling_dir = index.parent();
        let candidates = multiplex_t2g_candidates_for(None, sibling_dir, mode);
        let t2g = resolve_t2g_from_candidates(&candidates, output_dir, mode)?;
        return Ok((
            index.to_path_buf(),
            t2g,
            gene_id_to_name_for_dir(sibling_dir),
            probe_set_for_dir(sibling_dir),
        ));
    }

    bail!(
        "Could not resolve a valid piscem index from {}.",
        index.display()
    );
}

/// Check the read lists the way `quant` does before it maps: at least one
/// file per read, as many R1 as R2 files, and every file present. piscem pairs
/// `-1`/`-2` files by position and does not compare the counts, so an unpaired
/// file would otherwise be dropped from the run without an error.
fn check_read_pairs(reads1: &[PathBuf], reads2: &[PathBuf]) -> anyhow::Result<()> {
    if reads1.is_empty() || reads2.is_empty() {
        bail!("Both read1 and read2 files must be provided; Cannot proceed!");
    }
    if reads1.len() != reads2.len() {
        bail!(
            "{} read1 files and {} read2 files were given; Cannot proceed!",
            reads1.len(),
            reads2.len()
        );
    }
    let mut read_inputs = Vec::with_capacity(reads1.len() + reads2.len());
    read_inputs.extend_from_slice(reads1);
    read_inputs.extend_from_slice(reads2);
    prog_utils::check_files_exist(&read_inputs)
}

/// Main entry point for the multiplex-quant pipeline.
pub fn multiplex_map_and_quant(af_home: &Path, mut opts: MultiplexQuantOpts) -> anyhow::Result<()> {
    let start = Instant::now();
    info!("Starting multiplex quantification pipeline");
    opts.threads = crate::core::runtime::cap_threads_warned(opts.threads);

    // Before any resource is resolved: building or downloading the probe index
    // can take a while, and a bad read list would otherwise only fail (or, with
    // unequal counts, silently lose reads) once piscem runs.
    check_read_pairs(&opts.reads1, &opts.reads2)?;
    opts.decode.check_thread_policy()?;

    // Load runtime context (program paths)
    let rt = context::load_runtime_context(af_home)?;
    // Fail fast on a too-old registered alevin-fry, before mapping — otherwise a
    // stale registration only surfaces deep in `collate` (e.g. `--compress`
    // needs alevin-fry >= 0.18.3).
    crate::utils::prog_utils::ensure_alevin_fry_version(&rt.progs)?;
    let piscem_info = rt
        .progs
        .piscem
        .as_ref()
        .context("piscem is required for mapping; please run `simpleaf set-paths`")?;
    // Likewise for piscem: one below the floor would build or map with the old
    // index format and without the decoy speedups.
    crate::utils::prog_utils::ensure_piscem_version(&rt.progs)?;
    let alevin_fry_info = rt
        .progs
        .alevin_fry
        .as_ref()
        .context("alevin-fry is required; please run `simpleaf set-paths`")?;
    // The version string cannot tell alevin-fry builds apart, so read what this
    // one can do off its `--help`, then reject anything it cannot do for this run
    // before any work starts.
    let af_caps = prog_utils::AlevinFryCapabilities::detect(&alevin_fry_info.exe_path);
    let gene_axis = gene_axis_reason(&opts, &af_caps);
    let per_probe = gene_axis.is_none();
    // Say when a run that could be quantified per probe is not, but only where
    // the reason is not already in what the user typed: `--gene-level` names
    // its own effect, while `--usa` does not say that it also picks the axis,
    // and neither an index that does not say what it holds nor an alevin-fry
    // without `quant-probe` is visible from the command line. A transcriptome
    // needs no announcement.
    match gene_axis {
        Some(GeneAxis::Usa) => {
            info!("Quantifying per gene with `quant`: USA mode is only available there.");
        }
        Some(GeneAxis::UnknownIndex) => {
            info!(
                "Quantifying per gene with `quant`: the pre-built `--index` does not say \
                 whether it holds a probe set. If it does, pass the probe set it was built \
                 from with `--probe-set` to quantify per probe with `quant-probe`."
            );
        }
        Some(GeneAxis::NoQuantProbe) => {
            info!(
                "Falling back to `quant`: the configured alevin-fry has no `quant-probe` \
                 subcommand, unchanged from earlier releases. Install a newer alevin-fry to \
                 use `quant-probe` for per-probe quantification."
            );
        }
        // Listed rather than matched with `_`, so that a new variant has to
        // decide here too instead of silently saying nothing.
        Some(GeneAxis::Transcriptome | GeneAxis::GeneLevel) | None => {}
    }
    validate_alevin_fry_options(&opts, &af_caps, gene_axis, &alevin_fry_info.exe_path)?;

    // === Layered resolution: chemistry provides defaults, CLI flags override ===

    // Load chemistry from registry (optional)
    let chem: Option<CustomChemistry> = if let Some(ref chem_name) = opts.chemistry {
        let chem_path = af_home.join(CHEMISTRIES_PATH);
        if !chem_path.exists() {
            prog_utils::download_to_file(CHEMISTRIES_URL, &chem_path)?;
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

    // `quant` validates its geometry (via `add_chemistry_to_args_piscem`) but
    // this path handed `-g` straight to piscem, so a typo in `--geometry`
    // surfaced as a mapping failure partway through a run rather than as an
    // error before any work started. The named-chemistry lookup that `quant`
    // also performs does not apply here: presets in this path already store a
    // full geometry string rather than a protocol name.
    crate::utils::af_utils::validate_geometry(&geometry)
        .with_context(|| format!("invalid geometry: {}", geometry))?;

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
    let (index_path, probe_t2g_path_opt, gene_id_to_name_opt, probe_set_opt) =
        resolve_probe_index(af_home, chem.as_ref(), &opts, piscem_info, t2g_mode(&opts))?;
    check_probe_set_source(
        probe_set_opt.as_deref(),
        is_probe_set_reference(&opts),
        opts.anndata_out && opts.probe_mtx,
        opts.probe_set.as_deref().is_some_and(|p| !is_probe_csv(p)),
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
        .arg(format!("{}", opts.max_ec_card))
        .arg("--dict")
        .arg(opts.dict.as_cli());
    // Flex maps cheap probe reads against a tiny index, so decode dominates and
    // the parallel decoder pays much earlier than piscem's cross-workload
    // default assumes. Engage it earlier here unless the user set their own
    // policy. See `FLEX_DECODE_THREAD_POLICY`. `append_to` writes the inline
    // JSON (the Flex default, or a user-supplied inline value) to a file, since
    // piscem accepts only a path for --thread-policy.
    let flex_default = crate::simpleaf_commands::FLEX_DECODE_THREAD_POLICY;
    if opts.decode.thread_policy.is_none() {
        info!("Flex default decoder engagement: {flex_default}");
    }
    opts.decode
        .append_to(&mut piscem_cmd, output_dir, Some(flex_default))?;

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
    info!("piscem map-sc cmd: {}", map_cmd_str);
    let map_start = Instant::now();
    exec::run_checked(&mut piscem_cmd, "[piscem map-sc]")?;
    let map_duration = map_start.elapsed();
    info!("Mapping complete in {:.1}s", map_duration.as_secs_f64());
    let mapping_log =
        prog_parsing_utils::construct_json_from_piscem_log(map_output.join("map_info.json"))?;
    let map_info_path = quant_output.join("simpleaf_map_info.json");
    let map_info_file = std::fs::File::create(&map_info_path)?;
    serde_json::to_writer(map_info_file, &mapping_log)?;

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
        .arg(format!("{}", opts.threads))
        .arg("--unfiltered-pl")
        .arg(&cell_bc_path)
        .arg("--sample-bc-list")
        .arg(&sample_bc_path)
        .arg("--min-reads")
        .arg(format!("{}", opts.min_reads));

    opts.cell_correction.append_to(&mut gpl_cmd);
    opts.sample_correction.append_to(&mut gpl_cmd);
    opts.gpl_resources.append_to(&mut gpl_cmd);

    // Forward the sample-barcode orientation to alevin-fry. Precedence:
    //   1. user-supplied --sample-bc-ori CLI override (`forward` / `reverse`,
    //      matching alevin-fry's vocabulary and the preset JSON).
    //   2. the chemistry preset's declared sample_bc_ori (e.g. 10x Flex v2
    //      where the whitelist is the RC of what appears on the read).
    //   3. omit the flag (alevin-fry default).
    let sbc_ori_override = resolved_sample_bc_orientation(&opts, chem.as_ref());
    if let Some(ori) = sbc_ori_override {
        gpl_cmd.arg("--sample-bc-ori").arg(ori);
    }

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
    opts.collation_resources.append_to(&mut collate_cmd);

    let collate_cmd_str = prog_utils::get_cmd_line_string(&collate_cmd);
    info!("collate cmd: {}", collate_cmd_str);
    let collate_start = Instant::now();
    exec::run_checked(&mut collate_cmd, "[collate]")?;
    let collate_duration = collate_start.elapsed();

    // === Step 7: Quantify ===
    // The feature axis is the subcommand: `quant-probe` resolves UMIs over
    // probes and sums each gene's probes into its count, `quant` resolves over
    // genes. See `gene_axis_reason`.
    info!("Quantifying...");
    let mut quant_cmd = std::process::Command::new(&alevin_fry_info.exe_path);
    quant_cmd
        .arg(if per_probe { "quant-probe" } else { "quant" })
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

    // Only forwarded when the user set it, so alevin-fry's own default stands
    // otherwise.
    if let Some(small_thresh) = opts.small_thresh {
        quant_cmd
            .arg("--small-thresh")
            .arg(small_thresh.to_string());
    }
    if let Some(umi_edit_dist) = opts.umi_edit_dist {
        quant_cmd
            .arg("--umi-edit-dist")
            .arg(umi_edit_dist.to_string());
    }
    // Optional outputs, forwarded only when requested; support was checked
    // before any work started.
    if opts.dump_molecules {
        quant_cmd.arg("--dump-molecules");
    }
    // Only `quant-probe` defines it, and `validate_alevin_fry_options` has
    // already refused the combination where this run is not going there.
    if opts.probe_mtx {
        quant_cmd.arg("--probe-mtx");
    }

    let quant_cmd_str = prog_utils::get_cmd_line_string(&quant_cmd);
    info!("quant cmd: {}", quant_cmd_str);
    let quant_start = Instant::now();
    exec::run_checked(&mut quant_cmd, "[quant]")?;
    let quant_duration = quant_start.elapsed();

    if let Some(gene_id_to_name_path) = &gene_id_to_name_opt {
        let target_path = quant_output.join("gene_id_to_name.tsv");
        std::fs::copy(gene_id_to_name_path, &target_path).with_context(|| {
            format!(
                "could not copy gene_id_to_name.tsv from {} to {}",
                gene_id_to_name_path.display(),
                target_path.display()
            )
        })?;
    }

    copy_probe_set_into(probe_set_opt.as_deref(), &quant_output)?;

    let anndata_path = opts
        .anndata_out
        .then(|| quant_output.join("alevin").join("quants.h5ad"));
    let mut convert_duration_secs = None;

    // Write metadata before conversion so af-anndata can embed it into `uns`.
    let mut meta = json!({
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
        "gene_id_to_name_path": gene_id_to_name_opt
            .as_ref()
            .map(|p| p.display().to_string()),
        "map_cmd": map_cmd_str,
        "gpl_cmd": gpl_cmd_str,
        "collate_cmd": collate_cmd_str,
        "quant_cmd": quant_cmd_str,
        "map_info_path": map_info_path.display().to_string(),
        "map_duration_secs": map_duration.as_secs_f64(),
        "gpl_duration_secs": gpl_duration.as_secs_f64(),
        "collate_duration_secs": collate_duration.as_secs_f64(),
        "quant_duration_secs": quant_duration.as_secs_f64(),
        "anndata_path": anndata_path.as_ref().map(|p| p.display().to_string()),
        "conversion_duration_secs": convert_duration_secs,
        "total_duration_secs": start.elapsed().as_secs_f64(),
        "simpleaf_version": env!("CARGO_PKG_VERSION"),
    });
    if let Some(outputs) = prog_parsing_utils::requested_quant_outputs(
        &quant_output,
        opts.dump_molecules,
        opts.probe_mtx,
    ) {
        meta["quant_outputs"] = outputs;
    }
    write_multiplex_metadata(output_dir, &quant_output, &meta)?;

    if opts.anndata_out {
        let convert_start = Instant::now();
        let opath = anndata_path
            .as_ref()
            .expect("anndata_path must exist when --anndata-out is set");
        af_anndata::convert_csr_to_anndata(&quant_output, opath)?;
        // The gene conversion only, as before the probe h5ad.
        convert_duration_secs = Some(convert_start.elapsed().as_secs_f64());
        if opts.probe_mtx {
            write_probe_h5ad(&quant_output, probe_set_opt.as_deref())?;
        }
    }

    // === Rewrite pipeline metadata with final timing information ===
    meta["conversion_duration_secs"] = json!(convert_duration_secs);
    meta["total_duration_secs"] = json!(start.elapsed().as_secs_f64());
    write_multiplex_metadata(output_dir, &quant_output, &meta)?;

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
    piscem: &prog_utils::ProgInfo,
    mode: probe_utils::ProbeT2gMode,
) -> anyhow::Result<ResolvedIndex> {
    let piscem_path = piscem.exe_path.as_path();
    // If user provided a pre-built index, use it directly.
    if let Some(ref index) = opts.index {
        info!("Using user-provided probe index: {}", index.display());
        let (index_path, inferred_t2g, inferred_gene_id_to_name, inferred_probe_set) =
            resolve_user_supplied_index(index, &opts.output.join("resolved_t2g"), mode)?;
        let t2g = if inferred_t2g.is_some() {
            inferred_t2g
        } else if let Some(ref ps) = opts.probe_set {
            let conv_dir = opts.output.join("probe_conversion");
            // A user-supplied prebuilt index can't gain decoys after the fact, so
            // only derive the t2g/gene-name here — never write a `probe_decoys.fa`
            // (which would be unused and misleadingly counted).
            let probe_set_files =
                prepare_probe_set_files(ps, &conv_dir, probe_utils::ExcludedProbeMode::Ignore)?;
            Some(select_probe_set_t2g(&probe_set_files, mode)?)
        } else {
            None
        };
        let gene_id_to_name = if inferred_gene_id_to_name.is_some() {
            inferred_gene_id_to_name
        } else if let Some(ref ps) = opts.probe_set {
            let conv_dir = opts.output.join("probe_conversion");
            // A user-supplied prebuilt index can't gain decoys after the fact, so
            // only derive the t2g/gene-name here — never write a `probe_decoys.fa`
            // (which would be unused and misleadingly counted).
            let probe_set_files =
                prepare_probe_set_files(ps, &conv_dir, probe_utils::ExcludedProbeMode::Ignore)?;
            probe_set_files.gene_id_to_name_path
        } else {
            None
        };
        // A `--probe-set` CSV is the file the user named for this run, so it
        // wins over the copy in the index.
        let probe_set = match opts.probe_set.as_deref() {
            Some(ps) if is_probe_csv(ps) => Some(ps.to_path_buf()),
            _ => inferred_probe_set,
        };
        return Ok((index_path, t2g, gene_id_to_name, probe_set));
    }

    // If user provided a probe set file, build index from it.
    if let Some(ref probe_set) = opts.probe_set {
        warn!("Using user-provided probe set: {}.", probe_set.display(),);
        let (index_path, t2g, gene_id_to_name, probe_set) =
            build_index_from_probe_set(probe_set, opts, piscem_path, mode)?;
        return Ok((index_path, Some(t2g), gene_id_to_name, probe_set));
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
    probe_index_cache::remove_abandoned(&cache_dir);
    let cache_key = probe_info.plist_name.as_deref().unwrap_or("unknown");
    // Indices built under the two `--excluded-probes` modes differ (one carries
    // a poison table, the other does not), so they must not share a cache slot.
    // The `ignore` slot keeps its historical name; what earlier simpleaf
    // releases cached there carries no build record and is rebuilt once.
    let cached_index = match opts.excluded_probes {
        probe_utils::ExcludedProbeMode::Ignore => {
            cache_dir.join(format!("{}_{}", cache_key, opts.kmer_length))
        }
        probe_utils::ExcludedProbeMode::Decoy => {
            cache_dir.join(format!("{}_{}_decoy", cache_key, opts.kmer_length))
        }
    };
    let cached_probe_index_dir = cached_index.join("probe_index");
    let cached_probe_index = cached_probe_index_dir.join("index");
    // Reuse a cached index only if this piscem built it with these settings:
    // an index from another piscem release may not even load (see
    // `probe_index_cache`).
    let build_record = probe_index_cache::ProbeIndexBuildRecord::for_run(
        piscem,
        opts.kmer_length,
        opts.excluded_probes,
    )?;
    let use_cached = match probe_index_cache::slot_status(
        &cached_index,
        &cached_probe_index,
        &build_record,
    ) {
        probe_index_cache::SlotStatus::Fresh => true,
        probe_index_cache::SlotStatus::Absent => false,
        probe_index_cache::SlotStatus::Stale(reason) => {
            info!(
                "Rebuilding the cached probe index {}: {}",
                cached_index.display(),
                reason
            );
            // A run whose rebuild fails goes on with the index in the slot,
            // if there is one, so keep what a failed build left only if not.
            let keep_failed_build = !probe_index_base_exists(&cached_probe_index);
            match build_cached_probe_index(
                probe_info,
                &cache_dir,
                &cached_index,
                &build_record,
                opts,
                piscem_path,
                mode,
                keep_failed_build,
            ) {
                Ok(resolved) => return Ok(resolved),
                // E.g. a read-only shared ALEVIN_FRY_HOME or cache slot, or no
                // network to download the probe set again: use the index as
                // earlier releases did; piscem reports it if it cannot load it.
                Err(e) if probe_index_base_exists(&cached_probe_index) => {
                    warn!(
                        "The cached probe index {} could not be rebuilt ({:#}); using it as is, although {}.",
                        cached_index.display(),
                        e,
                        reason
                    );
                    true
                }
                Err(e) => return Err(e),
            }
        }
    };
    if use_cached {
        let candidates = multiplex_t2g_candidates_for(None, Some(&cached_probe_index_dir), mode);
        let t2g =
            resolve_t2g_from_candidates(&candidates, &opts.output.join("resolved_t2g"), mode)?;
        let gene_id_to_name = gene_id_to_name_for_dir(Some(&cached_probe_index_dir));
        let probe_set = probe_set_for_dir(Some(&cached_probe_index_dir));
        info!("Using cached probe index: {}", cached_probe_index.display());
        // Re-emit the empty-poison warning on cache hits too: otherwise a user
        // who first built at a k with an empty poison table would only ever see
        // the warning on that first (uncached) build.
        if opts.excluded_probes == probe_utils::ExcludedProbeMode::Decoy {
            probe_utils::warn_if_empty_poison_table(&cached_probe_index, opts.kmer_length);
        }
        return Ok((cached_probe_index, t2g, gene_id_to_name, probe_set));
    }

    build_cached_probe_index(
        probe_info,
        &cache_dir,
        &cached_index,
        &build_record,
        opts,
        piscem_path,
        mode,
        true,
    )
}

/// Build the index of the auto-selected probe set `probe_info`, downloading
/// its CSV into `cache_dir` first if needed, and install it in the cache slot
/// `cached_index` with `build_record`. If the build fails, what it left is
/// kept for inspection when `keep_failed_build` is set, and removed if not.
#[allow(clippy::too_many_arguments)]
fn build_cached_probe_index(
    probe_info: &ProbeSetInfo,
    cache_dir: &Path,
    cached_index: &Path,
    build_record: &probe_index_cache::ProbeIndexBuildRecord,
    opts: &MultiplexQuantOpts,
    piscem_path: &Path,
    mode: probe_utils::ProbeT2gMode,
    keep_failed_build: bool,
) -> anyhow::Result<ResolvedIndex> {
    // The index is built in a private directory next to the cache slot and
    // the finished index moved into place, so no run ever sees a half-built
    // one.
    let staging = probe_index_cache::staging_dir(cached_index)?;

    // Download and build
    if let Some(ref url) = probe_info.remote_url {
        info!("Downloading probe set '{}'...", probe_info.name);
        let download_dir = cache_dir.join("downloads");
        std::fs::create_dir_all(&download_dir)?;
        let csv_path = download_dir.join(format!("{}.csv", probe_info.name));
        if !csv_path.exists() {
            download_registry_file(
                url,
                &csv_path,
                probe_info.plist_name.as_deref(),
                "probe set",
            )?;
            info!("Downloaded probe set to {}", csv_path.display());
        }

        let mut build_opts = opts.clone();
        build_opts.output = staging.path().to_path_buf();
        let result = match build_index_from_probe_set(&csv_path, &build_opts, piscem_path, mode) {
            Ok(result) => result,
            Err(e) => {
                if keep_failed_build {
                    // Keep what the failed build left (any piscem scratch
                    // among it) for inspection, as a build that failed in the
                    // slot itself used to; `remove_abandoned` deletes it once
                    // it is old.
                    let staging = staging.keep();
                    warn!(
                        "The probe index build failed; what it left is kept in {} (a later run removes it once it is an hour old)",
                        staging.display()
                    );
                } else {
                    info!(
                        "The probe index build failed; removing what it left in {}",
                        staging.path().display()
                    );
                }
                return Err(e);
            }
        };
        probe_index_cache::write_build_record(staging.path(), build_record)?;
        let cached_probe_index = cached_index.join("probe_index").join("index");
        if !probe_index_cache::install(
            staging.path(),
            cached_index,
            &cached_probe_index,
            build_record,
        )? {
            info!(
                "Another run cached an equivalent probe index first; using {}",
                cached_probe_index.display()
            );
        }
        let in_cache = |p: PathBuf| probe_index_cache::rebase(p, staging.path(), cached_index);
        Ok((
            in_cache(result.0),
            Some(in_cache(result.1)),
            result.2.map(in_cache),
            result.3.map(in_cache),
        ))
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
) -> anyhow::Result<(PathBuf, PathBuf, Option<PathBuf>, Option<PathBuf>)> {
    let index_dir = opts.output.join("probe_index");
    std::fs::create_dir_all(&index_dir)?;
    let probe_set_files = prepare_probe_set_files(probe_set, &index_dir, opts.excluded_probes)?;
    // Keep the original CSV with the index, as `simpleaf index --probe-csv`
    // does. A FASTA probe set has no CSV to keep.
    let mut probe_set_copy = None;
    if is_probe_csv(probe_set) {
        let index_probe_set_path = index_dir.join("probe_set.csv");
        // The CSV may already be this copy (`--probe-set probe_index/probe_set.csv`).
        if !probe_utils::same_file(probe_set, &index_probe_set_path) {
            std::fs::copy(probe_set, &index_probe_set_path)?;
        }
        probe_set_copy = Some(index_probe_set_path);
    }
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
        .arg("--dict")
        .arg(opts.dict.as_cli())
        .arg("--overwrite");

    // Excluded probes become poison k-mers: piscem records the k-mers at the
    // boundary between decoy sequence and the indexed reference and, at map
    // time, discards any read whose hits include one. `set-paths` already
    // requires a piscem new enough (>= min_versions::PISCEM) to accept
    // `--decoy-paths`, so no version gate is needed here.
    if let Some(ref decoys) = probe_set_files.decoy_fasta_path {
        info!(
            "Indexing excluded probes as decoys from {}",
            decoys.display()
        );
        build_cmd.arg("--decoy-paths").arg(decoys);
    }

    // Without `-w`, piscem puts its scratch in `./workdir.noindex` under the
    // current directory, where cf1-rs uses fixed file names: concurrent builds
    // launched from one directory clobber each other's files and either fail
    // or produce a corrupt index. Give each build its own work directory under
    // the index directory instead, and remove it once the build succeeds.
    let work_dir = tempfile::Builder::new()
        .prefix("workdir.")
        .suffix(".noindex")
        .tempdir_in(&index_dir)
        .with_context(|| {
            format!(
                "could not create a piscem work directory in {}",
                index_dir.display()
            )
        })?;
    build_cmd.arg("-w").arg(work_dir.path());

    info!(
        "piscem build cmd: {}",
        prog_utils::get_cmd_line_string(&build_cmd)
    );
    if let Err(e) = exec::run_checked(&mut build_cmd, "[piscem build]") {
        // Keep the scratch of a failed build for inspection.
        let work_dir = work_dir.keep();
        warn!(
            "piscem build failed; its work directory was left at {}",
            work_dir.display()
        );
        return Err(e);
    }
    let work_dir_path = work_dir.path().to_path_buf();
    if let Err(e) = work_dir.close() {
        warn!(
            "could not remove the piscem work directory {}: {e}",
            work_dir_path.display()
        );
    }

    // If we asked piscem to index decoys, warn when the poison table came back
    // empty (e.g. k too large for any decoy/reference adjacency) so the user is
    // not left thinking the excluded-probe decoys are filtering reads when they
    // are not.
    if probe_set_files.decoy_fasta_path.is_some() {
        probe_utils::warn_if_empty_poison_table(&index_prefix, opts.kmer_length);
    }

    Ok((
        index_prefix,
        t2g_path,
        probe_set_files.gene_id_to_name_path,
        probe_set_copy,
    ))
}

/// Download a file the chemistry registry lists to `dest`. When the registry
/// records the file's blake3 content hash (its `plist_name`), check the
/// download against it, and, as `get_permit_if_absent` does for permit lists,
/// warn on a mismatch rather than fail; the file is still used.
fn download_registry_file(
    url: &str,
    dest: &Path,
    expected_hash: Option<&str>,
    what: &str,
) -> anyhow::Result<()> {
    let Some(expected_hash) = expected_hash else {
        return prog_utils::download_to_file(url, dest);
    };
    let observed_hash = prog_utils::download_to_file_compute_hash(url, dest)?.to_string();
    if observed_hash != expected_hash {
        warn!(
            "The {} obtained from the remote url {} does not match the expected hash {} (the observed hash was {}); it is used as is, and stays cached at {}",
            what,
            url,
            expected_hash,
            observed_hash,
            dest.display()
        );
    }
    Ok(())
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
        download_registry_file(
            url,
            &dest,
            chem.plist_name.as_deref(),
            "cell barcode whitelist",
        )?;
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

    // Resolve the chemistry's registry sample-barcode file (all samples, every
    // spelling), from cache or by download.
    let registry_path = if let Some(cached) = sbc_info
        .plist_name
        .as_ref()
        .map(|hash| plist_dir.join(hash))
        .filter(|p| p.exists())
    {
        info!("Sample barcode list cached: {}", cached.display());
        cached
    } else if let Some(ref url) = sbc_info.remote_url {
        let dest = if let Some(ref hash) = sbc_info.plist_name {
            plist_dir.join(hash)
        } else {
            plist_dir.join("sample_bc_list.txt")
        };
        info!("Downloading sample barcode list...");
        download_registry_file(
            url,
            &dest,
            sbc_info.plist_name.as_deref(),
            "sample barcode list",
        )?;
        info!("Downloaded to {}", dest.display());
        dest
    } else {
        bail!("Chemistry has no sample barcode list URL. Provide --sample-bc-list.");
    };

    // With a sample sheet, select and rename a subset of the samples, taking the
    // barcode sequences (every spelling) from the registry file so the caller
    // never transcribes them.
    match opts.samples {
        Some(ref sheet) => derive_sample_bc_list(&registry_path, sheet, &opts.output),
        None => Ok(registry_path),
    }
}

/// Build a sample-barcode list for a caller-selected, caller-named subset of the
/// chemistry's samples, without hand-transcribing any barcode sequence.
///
/// `registry_path` is the chemistry's 3-column file (`observed  canonical
/// sample_id`) holding every spelling of every sample. `sheet_path` maps a
/// registry `sample_id` to the caller's name (one or two tab-separated columns;
/// a single column keeps the id as the name). The result is the registry rows
/// for the requested ids only, with the sample-name column replaced by the
/// caller's name — every spelling preserved, so no spelling can be dropped.
fn derive_sample_bc_list(
    registry_path: &Path,
    sheet_path: &Path,
    output_dir: &Path,
) -> anyhow::Result<PathBuf> {
    use std::io::{BufRead, BufReader, BufWriter, Write};

    // Parse the sheet: id -> name (name defaults to id), preserving first-seen
    // order for stable, informative errors.
    let sheet = std::fs::File::open(sheet_path)
        .with_context(|| format!("couldn't open sample sheet: {}", sheet_path.display()))?;
    let mut requested_order: Vec<String> = Vec::new();
    let mut name_by_id: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for line in BufReader::new(sheet).lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut cols = trimmed.split('\t');
        let id = cols.next().unwrap().trim().to_string();
        let name = cols
            .next()
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| id.clone());
        if name_by_id.insert(id.clone(), name).is_none() {
            requested_order.push(id);
        }
    }
    if requested_order.is_empty() {
        bail!("sample sheet {} contained no samples", sheet_path.display());
    }

    // Rewrite the registry rows for the requested ids with the caller's names.
    std::fs::create_dir_all(output_dir)?;
    let registry = std::fs::File::open(registry_path).with_context(|| {
        format!(
            "couldn't open registry sample barcode list: {}",
            registry_path.display()
        )
    })?;
    let out_path = output_dir.join("sample_bc_list.derived.tsv");
    let mut out = BufWriter::new(std::fs::File::create(&out_path)?);
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut spellings = 0usize;
    for line in BufReader::new(registry).lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = trimmed.split('\t').collect();
        if parts.len() < 3 {
            bail!(
                "registry sample barcode list {} is not the expected 3-column \
                 (observed, canonical, sample_id) format",
                registry_path.display()
            );
        }
        if let Some(name) = name_by_id.get(parts[2]) {
            writeln!(out, "{}\t{}\t{}", parts[0], parts[1], name)?;
            seen.insert(parts[2].to_string());
            spellings += 1;
        }
    }
    out.flush()?;

    // Every requested id must exist, or a typo would silently drop a sample.
    let missing: Vec<&str> = requested_order
        .iter()
        .filter(|id| !seen.contains(*id))
        .map(String::as_str)
        .collect();
    if !missing.is_empty() {
        bail!(
            "sample sheet requests id(s) absent from the chemistry's registry \
             sample barcode list: {}. Check the ids against the chemistry preset.",
            missing.join(", ")
        );
    }
    info!(
        "Derived sample barcode list for {} sample(s) ({} spellings) at {}",
        seen.len(),
        spellings,
        out_path.display()
    );
    Ok(out_path)
}

#[cfg(test)]
mod tests {
    use super::{GeneAxis, gene_axis_reason, validate_alevin_fry_options};
    use super::{
        derive_sample_bc_list, resolve_user_supplied_index, resolved_sample_bc_orientation,
        t2g_mode,
    };
    use crate::simpleaf_commands::MultiplexQuantOpts;
    use crate::utils::chem_utils::{CustomChemistry, SampleBcListInfo};
    use crate::utils::probe_utils::ProbeT2gMode;
    use crate::utils::prog_utils::AlevinFryCapabilities;
    use clap::Parser;
    use serde_json::json;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    /// A registry file in the shipped 3-column format: two samples, each with
    /// several spellings mapping to one canonical barcode.
    const REGISTRY: &str = "\
ACTTTAGG\tACTTTAGG\tBC001
CTTTAGGC\tACTTTAGG\tBC001
CGAGGGTA\tACTTTAGG\tBC001
TGGACTAT\tTGGACTAT\tBC002
GGACTATC\tTGGACTAT\tBC002
GATCCTCT\tGATCCTCT\tBC003
";

    fn derived_lines(dir: &Path) -> Vec<String> {
        fs::read_to_string(dir.join("sample_bc_list.derived.tsv"))
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn sample_sheet_selects_subset_and_renames_keeping_all_spellings() {
        let dir = tempdir().unwrap();
        let reg = dir.path().join("registry.tsv");
        fs::write(&reg, REGISTRY).unwrap();
        // select BC003 and BC001 (out of order), rename BC001, keep BC003's id.
        let sheet = dir.path().join("samples.tsv");
        fs::write(&sheet, "BC003\tkidney\nBC001\n").unwrap();

        let out = derive_sample_bc_list(&reg, &sheet, dir.path()).unwrap();
        let lines = derived_lines(out.parent().unwrap());
        // BC002 dropped; BC001 keeps all 3 spellings (named by its id); BC003 renamed.
        assert_eq!(
            lines,
            vec![
                "ACTTTAGG\tACTTTAGG\tBC001",
                "CTTTAGGC\tACTTTAGG\tBC001",
                "CGAGGGTA\tACTTTAGG\tBC001",
                "GATCCTCT\tGATCCTCT\tkidney",
            ]
        );
    }

    #[test]
    fn sample_sheet_two_column_renames() {
        let dir = tempdir().unwrap();
        let reg = dir.path().join("registry.tsv");
        fs::write(&reg, REGISTRY).unwrap();
        let sheet = dir.path().join("samples.tsv");
        fs::write(&sheet, "BC002\tlung\n").unwrap();
        derive_sample_bc_list(&reg, &sheet, dir.path()).unwrap();
        assert_eq!(
            derived_lines(dir.path()),
            vec!["TGGACTAT\tTGGACTAT\tlung", "GGACTATC\tTGGACTAT\tlung"]
        );
    }

    #[test]
    fn sample_sheet_unknown_id_is_an_error() {
        let dir = tempdir().unwrap();
        let reg = dir.path().join("registry.tsv");
        fs::write(&reg, REGISTRY).unwrap();
        let sheet = dir.path().join("samples.tsv");
        fs::write(&sheet, "BC001\ta\nBC999\tb\n").unwrap();
        let err = derive_sample_bc_list(&reg, &sheet, dir.path())
            .unwrap_err()
            .to_string();
        assert!(err.contains("BC999"), "{err}");
    }

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

        let (resolved_index, t2g, gene_id_to_name, probe_set) = resolve_user_supplied_index(
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
        assert!(gene_id_to_name.is_none());
        // An index built before simpleaf kept the CSV records no probe set.
        assert!(probe_set.is_none());
    }

    /// A `simpleaf index --probe-csv` output, with its `probe_set.csv` copy
    /// recorded in `simpleaf_index.json`; returns the index dir.
    fn write_simpleaf_probe_index(root: &Path) -> PathBuf {
        let index_dir = root.join("index");
        fs::create_dir_all(&index_dir).expect("failed to create index dir");
        write_piscem_index(&index_dir.join("piscem_idx"));
        fs::write(index_dir.join("t2g_3col.tsv"), "P1\tG1\tS\n").expect("failed to write t2g");
        fs::write(
            index_dir.join("probe_set.csv"),
            "gene_id,probe_seq,probe_id\n",
        )
        .expect("failed to write probe set copy");
        fs::write(
            index_dir.join("simpleaf_index.json"),
            serde_json::to_string_pretty(&json!({
                "index_type": "piscem",
                "t2g_file": "t2g_3col.tsv",
                "probe_set_file": "probe_set.csv"
            }))
            .expect("failed to serialize index json"),
        )
        .expect("failed to write simpleaf index json");
        index_dir
    }

    #[test]
    fn simpleaf_index_supplies_its_probe_set_copy() {
        let td = tempdir().expect("failed to create tempdir");
        let root = td.path().join("simpleaf_output");
        let index_dir = write_simpleaf_probe_index(&root);

        let resolved =
            resolve_user_supplied_index(&root, &td.path().join("resolved"), ProbeT2gMode::Gene)
                .expect("failed to resolve simpleaf index");

        assert_eq!(resolved.3, Some(index_dir.join("probe_set.csv")));
    }

    #[test]
    fn multiplex_probe_index_dir_supplies_its_probe_set_copy() {
        let td = tempdir().expect("failed to create tempdir");
        let probe_index_dir = td.path().join("probe_index");
        fs::create_dir_all(&probe_index_dir).expect("failed to create probe index dir");
        write_piscem_index(&probe_index_dir.join("index"));
        fs::write(probe_index_dir.join("probe_t2g.tsv"), "P1\tG1\n")
            .expect("failed to write gene t2g");
        fs::write(
            probe_index_dir.join("probe_set.csv"),
            "gene_id,probe_seq,probe_id\n",
        )
        .expect("failed to write probe set copy");

        // As a directory and as the bare `index` prefix inside it.
        for index in [probe_index_dir.clone(), probe_index_dir.join("index")] {
            let resolved = resolve_user_supplied_index(
                &index,
                &td.path().join("resolved"),
                ProbeT2gMode::Gene,
            )
            .expect("failed to resolve probe index");
            assert_eq!(
                resolved.3,
                Some(probe_index_dir.join("probe_set.csv")),
                "for --index {}",
                index.display()
            );
        }
    }

    #[test]
    fn probe_set_csv_given_with_an_index_wins_over_the_index_copy() {
        use crate::utils::prog_utils::ProgInfo;
        let td = tempdir().expect("failed to create tempdir");
        let root = td.path().join("simpleaf_output");
        let index_dir = write_simpleaf_probe_index(&root);
        let piscem = ProgInfo {
            exe_path: PathBuf::from("/bin/false"),
            version: "0.24.0".to_string(),
        };
        let out = td.path().join("out");
        let resolve = |probe_set: Option<&str>| {
            let mut args = vec!["-o", out.to_str().unwrap(), "-i", root.to_str().unwrap()];
            if let Some(ps) = probe_set {
                args.extend(["--probe-set", ps]);
            }
            let opts = parse_multiplex_quant_opts(&args);
            super::resolve_probe_index(td.path(), None, &opts, &piscem, ProbeT2gMode::Gene)
                .expect("failed to resolve the index")
                .3
        };

        // The index has no gene names, so the run reads them from the probe set.
        let csv = td.path().join("other_probes.csv");
        fs::write(
            &csv,
            "gene_id,probe_seq,probe_id,included\nG1,ACGTACGT,P1,TRUE\n",
        )
        .expect("failed to write probe csv");
        assert_eq!(resolve(Some(csv.to_str().unwrap())), Some(csv.clone()));
        // A FASTA probe set has no CSV, so the index copy stays.
        let fasta = td.path().join("probes.fa");
        fs::write(&fasta, ">P1\nACGTACGT\n").expect("failed to write probe fasta");
        assert_eq!(
            resolve(Some(fasta.to_str().unwrap())),
            Some(index_dir.join("probe_set.csv"))
        );
        assert_eq!(resolve(None), Some(index_dir.join("probe_set.csv")));
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

        let (resolved_index, t2g, gene_id_to_name, probe_set) = resolve_user_supplied_index(
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
        assert!(gene_id_to_name.is_none());
        assert!(probe_set.is_none());
    }

    /// Build `MultiplexQuantOpts` the way a user would: by parsing a command
    /// line. A struct literal has to name every field, so adding an option to
    /// the CLI breaks this file with an E0063 that only `--all-targets`
    /// surfaces -- `cargo build` does not compile tests. Parsing instead lets
    /// new options pick up their clap defaults, and anything genuinely
    /// required fails loudly at parse time. The required `-1`/`-2` are
    /// supplied here, since none of these tests looks at the reads.
    fn parse_multiplex_quant_opts(args: &[&str]) -> MultiplexQuantOpts {
        let mut cli_args = vec!["simpleaf", "multiplex-quant", "-1", "r1.fq", "-2", "r2.fq"];
        cli_args.extend_from_slice(args);
        match crate::Cli::parse_from(cli_args).command {
            crate::simpleaf_commands::Commands::MultiplexQuant(opts) => opts,
            cmd => panic!("expected multiplex-quant command, found {:?}", cmd),
        }
    }

    /// Parse `multiplex-quant -o . <args>` without supplying any reads.
    fn try_parse_multiplex_quant(args: &[&str]) -> Result<crate::Cli, clap::Error> {
        let mut cli_args = vec!["simpleaf", "multiplex-quant", "-o", "."];
        cli_args.extend_from_slice(args);
        crate::Cli::try_parse_from(cli_args)
    }

    #[test]
    fn compress_flag_parses_to_codec() {
        use crate::simpleaf_commands::CompressCodec;
        let codec = |args: &[&str]| {
            parse_multiplex_quant_opts(args)
                .collation_resources
                .compress
        };
        // Absent => lz4 (compression is ON by default via the CLI default_value).
        assert_eq!(codec(&["-o", "."]), CompressCodec::Lz4);
        // Bare `--compress` => lz4 (default_missing_value).
        assert_eq!(codec(&["-o", ".", "--compress"]), CompressCodec::Lz4);
        // Explicit codecs.
        assert_eq!(codec(&["-o", ".", "--compress", "lz4"]), CompressCodec::Lz4);
        assert_eq!(
            codec(&["-o", ".", "--compress", "zstd"]),
            CompressCodec::Zstd
        );
        assert_eq!(
            codec(&["-o", ".", "--compress", "none"]),
            CompressCodec::None
        );
    }

    /// What each alevin-fry build offers: the 0.18.3 release has none of
    /// these, the RAD v2 integration build writes the molecule table but has
    /// no `quant-probe`, and a build with `quant-probe` has all three.
    /// `--probe-mtx` lives on `quant-probe`, so it cannot be had without it.
    fn caps(quant_probe: bool, quant_outputs: bool) -> AlevinFryCapabilities {
        AlevinFryCapabilities {
            quant_probe,
            quant_dump_molecules: quant_outputs,
            quant_probe_dump_molecules: quant_probe && quant_outputs,
            probe_mtx: quant_probe && quant_outputs,
        }
    }

    /// Every variant, and which one is reported when several apply: the
    /// reference first, since no flag change rescues it, then the flags that
    /// force the gene axis, and only then the installed alevin-fry, which is
    /// the one thing left when everything the user controls is already right.
    #[test]
    fn gene_axis_reason_names_what_to_change_first() {
        let axis = |args: &[&str], af_caps: &AlevinFryCapabilities| {
            let mut cli = vec!["-o", "."];
            cli.extend_from_slice(args);
            gene_axis_reason(&parse_multiplex_quant_opts(&cli), af_caps)
        };
        let modern = caps(true, true);
        let older = caps(false, false);

        assert_eq!(axis(&["--probe-set", "p.csv"], &modern), None);
        assert_eq!(
            axis(&["-i", "idx", "-m", "t2g.tsv"], &modern),
            Some(GeneAxis::Transcriptome)
        );
        assert_eq!(axis(&["-i", "idx"], &modern), Some(GeneAxis::UnknownIndex));
        assert_eq!(
            axis(&["--probe-set", "p.csv", "--usa"], &modern),
            Some(GeneAxis::Usa)
        );
        assert_eq!(
            axis(&["--probe-set", "p.csv", "--gene-level"], &modern),
            Some(GeneAxis::GeneLevel)
        );
        assert_eq!(
            axis(&["--probe-set", "p.csv"], &older),
            Some(GeneAxis::NoQuantProbe)
        );

        // Precedence when several apply at once: "drop `--usa`" on a
        // transcriptome, or "install a newer alevin-fry" on a run that also
        // passed `--gene-level`, would each be advice that does not help.
        assert_eq!(
            axis(
                &["-i", "idx", "-m", "t2g.tsv", "--usa", "--gene-level"],
                &older
            ),
            Some(GeneAxis::Transcriptome)
        );
        assert_eq!(
            axis(&["-i", "idx", "--usa"], &older),
            Some(GeneAxis::UnknownIndex)
        );
        assert_eq!(
            axis(&["--probe-set", "p.csv", "--usa", "--gene-level"], &older),
            Some(GeneAxis::Usa)
        );
        assert_eq!(
            axis(&["--probe-set", "p.csv", "--gene-level"], &older),
            Some(GeneAxis::GeneLevel)
        );
    }

    #[test]
    fn probe_set_references_are_quantified_per_probe_by_default() {
        let per_probe = |args: &[&str], af_caps: &AlevinFryCapabilities| {
            let mut cli = vec!["-o", "."];
            cli.extend_from_slice(args);
            gene_axis_reason(&parse_multiplex_quant_opts(&cli), af_caps).is_none()
        };
        let modern = caps(true, true);
        // The chemistry's registered probe set, a probe CSV/FASTA, or a
        // pre-built index named together with the probe set it was built from.
        assert!(per_probe(&["-c", "10x-flexv2-gex-3p"], &modern));
        assert!(per_probe(&["--probe-set", "p.csv"], &modern));
        assert!(per_probe(&["--probe-set", "p.fa"], &modern));
        assert!(per_probe(&["-i", "idx", "--probe-set", "p.csv"], &modern));
        // A pre-built index alone could hold a transcriptome; a t2g map marks one.
        assert!(!per_probe(&["-i", "idx"], &modern));
        assert!(!per_probe(&["-i", "idx", "-m", "t2g.tsv"], &modern));
        assert!(!per_probe(
            &["--probe-set", "p.csv", "-m", "t2g.tsv"],
            &modern
        ));
        // The two ways to ask for the gene axis on a probe set.
        assert!(!per_probe(&["--probe-set", "p.csv", "--usa"], &modern));
        assert!(!per_probe(
            &["--probe-set", "p.csv", "--gene-level"],
            &modern
        ));
        // Never on an alevin-fry without the subcommand (0.18.3, the RAD v2
        // integration build): those runs are quantified per gene as before.
        for af_caps in [caps(false, false), caps(false, true)] {
            assert!(!per_probe(&["-c", "10x-flexv2-gex-3p"], &af_caps));
            assert!(!per_probe(&["--probe-set", "p.csv"], &af_caps));
        }
    }

    /// A pre-built index counts as a probe set when simpleaf's own metadata
    /// says so, in every layout `--index` accepts; anything else stays unknown.
    #[test]
    fn prebuilt_probe_indices_are_recognised_from_their_metadata() {
        let axis = |index: &Path, extra: &[&str]| {
            let index = index.to_str().expect("utf-8 tempdir path");
            let mut cli = vec!["-o", ".", "-i", index];
            cli.extend_from_slice(extra);
            gene_axis_reason(&parse_multiplex_quant_opts(&cli), &caps(true, true))
        };
        let td = tempdir().expect("failed to create tempdir");

        // multiplex-quant's probe index: `probe_set_info.json` beside the
        // `index` prefix, given as the directory or as the prefix.
        let multiplex = td.path().join("probe_index");
        fs::create_dir_all(&multiplex).unwrap();
        fs::write(multiplex.join("probe_set_info.json"), "{}").unwrap();
        assert_eq!(axis(&multiplex, &[]), None);
        assert_eq!(axis(&multiplex.join("index"), &[]), None);

        // `simpleaf index --probe-csv`: given as the root, its `index/`
        // subdirectory, or the `piscem_idx` prefix inside that.
        let probe_csv = td.path().join("probe_csv_index");
        fs::create_dir_all(probe_csv.join("index")).unwrap();
        fs::write(
            probe_csv.join("index_info.json"),
            json!({"args": {"probe-csv": "probes.csv"}}).to_string(),
        )
        .unwrap();
        assert_eq!(axis(&probe_csv, &[]), None);
        assert_eq!(axis(&probe_csv.join("index"), &[]), None);
        assert_eq!(axis(&probe_csv.join("index").join("piscem_idx"), &[]), None);
        // A t2g map still marks a transcriptome, whatever the index says.
        assert_eq!(
            axis(&probe_csv, &["-m", "t2g.tsv"]),
            Some(GeneAxis::Transcriptome)
        );

        // A transcriptome index built by `simpleaf index`, and a directory
        // with no simpleaf metadata at all, do not say what they hold.
        let txome = td.path().join("txome_index");
        fs::create_dir_all(txome.join("index")).unwrap();
        fs::write(
            txome.join("index_info.json"),
            json!({"args": {"fasta": "genome.fa", "gtf": "genes.gtf"}}).to_string(),
        )
        .unwrap();
        assert_eq!(axis(&txome, &[]), Some(GeneAxis::UnknownIndex));
        assert_eq!(
            axis(&txome.join("index"), &[]),
            Some(GeneAxis::UnknownIndex)
        );
        let bare = td.path().join("bare");
        fs::create_dir_all(&bare).unwrap();
        assert_eq!(axis(&bare, &[]), Some(GeneAxis::UnknownIndex));
    }

    #[test]
    fn per_probe_runs_reject_gene_level_resolutions_up_front() {
        let check = |args: &[&str], af_caps: AlevinFryCapabilities| {
            let mut cli = vec!["-o", ".", "--probe-set", "p.csv"];
            cli.extend_from_slice(args);
            let opts = parse_multiplex_quant_opts(&cli);
            let axis = gene_axis_reason(&opts, &af_caps);
            validate_alevin_fry_options(&opts, &af_caps, axis, Path::new("alevin-fry"))
                .map_err(|e| format!("{e:#}"))
        };
        let modern = caps(true, true);

        let err = check(&["-r", "parsimony-gene"], modern).expect_err("must be rejected");
        assert!(
            err.contains("Use `--resolution parsimony` instead"),
            "{err}"
        );
        let err = check(&["-r", "parsimony-gene-em"], modern).expect_err("must be rejected");
        assert!(
            err.contains("Use `--resolution parsimony-em` instead"),
            "{err}"
        );
        for ok in ["cr-like", "cr-like-em", "parsimony", "parsimony-em"] {
            assert!(
                check(&["-r", ok], modern).is_ok(),
                "{ok} should be accepted"
            );
        }

        // The refusal belongs to the probe axis alone. `--usa`, `--gene-level`
        // and an alevin-fry without `quant-probe` all stay on the gene axis,
        // where these resolutions are what they have always been.
        assert!(check(&["-r", "parsimony-gene", "--usa"], modern).is_ok());
        assert!(check(&["-r", "parsimony-gene", "--gene-level"], modern).is_ok());
        assert!(check(&["-r", "parsimony-gene"], caps(false, true)).is_ok());
    }

    #[test]
    fn optional_quant_outputs_need_an_alevin_fry_that_has_them() {
        let check = |args: &[&str], af_caps: AlevinFryCapabilities| {
            let mut cli = vec!["-o", "."];
            cli.extend_from_slice(args);
            let opts = parse_multiplex_quant_opts(&cli);
            let axis = gene_axis_reason(&opts, &af_caps);
            validate_alevin_fry_options(&opts, &af_caps, axis, Path::new("/opt/af/alevin-fry"))
                .map_err(|e| format!("{e:#}"))
        };

        // Not requested: no requirement on alevin-fry (0.18.3 keeps working).
        assert!(check(&["--probe-set", "p.csv"], caps(false, false)).is_ok());
        assert!(check(&["-i", "idx", "-m", "t2g.tsv"], caps(false, false)).is_ok());

        // Requested from an alevin-fry that lacks them.
        let err = check(
            &["--probe-set", "p.csv", "--dump-molecules"],
            caps(false, false),
        )
        .expect_err("--dump-molecules must need alevin-fry support");
        assert!(
            err.contains("`/opt/af/alevin-fry quant --help` does not list it"),
            "{err}"
        );
        let err = check(&["--probe-set", "p.csv", "--probe-mtx"], caps(false, false))
            .expect_err("--probe-mtx must need the quant-probe subcommand");
        assert!(err.contains("has no `quant-probe` subcommand"), "{err}");

        // Requested from a build that has them.
        let modern = caps(true, true);
        assert!(
            check(
                &["--probe-set", "p.csv", "--dump-molecules", "--probe-mtx"],
                modern
            )
            .is_ok()
        );
        assert!(check(&["-c", "10x-flexv2-gex-3p", "--probe-mtx"], modern).is_ok());
        // The molecule table depends on neither the reference nor the axis.
        for af_caps in [modern, caps(false, true)] {
            assert!(check(&["-i", "idx", "-m", "t2g.tsv", "--dump-molecules"], af_caps).is_ok());
        }

        // `--probe-mtx` says which of the four reasons put this run on `quant`,
        // since three of them are the user's own flags.
        let cases: [(&[&str], &str); 4] = [
            (&["--probe-set", "p.csv", "--usa"], "`--usa` runs `quant`"),
            (
                &["--probe-set", "p.csv", "--gene-level"],
                "`--gene-level` runs `quant`",
            ),
            (
                &["-i", "idx", "-m", "t2g.tsv"],
                "`--t2g-map` marks a transcriptome reference",
            ),
            (
                &["-i", "idx"],
                "pass the probe set the index was built from",
            ),
        ];
        for (args, expected) in cases {
            let mut cli = args.to_vec();
            cli.push("--probe-mtx");
            let err = check(&cli, modern).expect_err("--probe-mtx needs a per-probe run");
            assert!(err.contains(expected), "expected {expected:?} in: {err}");
        }

        // The molecule table is checked against the subcommand the run will
        // invoke: here `quant` lists it and `quant-probe` does not, so the
        // same flag passes on the gene axis and fails on the probe axis.
        let probe_axis_without_molecules = AlevinFryCapabilities {
            quant_probe: true,
            quant_dump_molecules: true,
            quant_probe_dump_molecules: false,
            probe_mtx: true,
        };
        assert!(
            check(
                &["-i", "idx", "-m", "t2g.tsv", "--dump-molecules"],
                probe_axis_without_molecules
            )
            .is_ok()
        );
        let err = check(
            &["--probe-set", "p.csv", "--dump-molecules"],
            probe_axis_without_molecules,
        )
        .expect_err("--dump-molecules must be listed by the subcommand that runs");
        assert!(
            err.contains("`/opt/af/alevin-fry quant-probe --help` does not list it"),
            "{err}"
        );

        // A build whose `quant-probe` does not list `--probe-mtx`.
        let err = check(
            &["--probe-set", "p.csv", "--probe-mtx"],
            AlevinFryCapabilities {
                quant_probe: true,
                quant_dump_molecules: true,
                quant_probe_dump_molecules: true,
                probe_mtx: false,
            },
        )
        .expect_err("--probe-mtx must be listed by quant-probe");
        assert!(
            err.contains("`/opt/af/alevin-fry quant-probe --help` does not list it"),
            "{err}"
        );
    }

    #[test]
    fn expected_ori_accepts_what_alevin_fry_accepts() {
        let ori = |value: &str| {
            parse_multiplex_quant_opts(&["-o", ".", "--expected-ori", value]).expected_ori
        };
        assert_eq!(
            parse_multiplex_quant_opts(&["-o", "."]).expected_ori,
            "both"
        );
        for value in ["fw", "rc", "both", "either", "FW", "Both"] {
            // Passed through as given; alevin-fry matches case-insensitively.
            assert_eq!(ori(value), value);
        }

        let err =
            try_parse_multiplex_quant(&["-1", "r1.fq", "-2", "r2.fq", "--expected-ori", "bogus"])
                .expect_err("an orientation alevin-fry rejects must not parse");
        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
    }

    #[test]
    fn reads_are_required() {
        for args in [&[][..], &["-1", "r1.fq"][..], &["-2", "r2.fq"][..]] {
            let err = try_parse_multiplex_quant(args)
                .expect_err("multiplex-quant must require -1 and -2");
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::MissingRequiredArgument,
                "{args:?}"
            );
        }
    }

    #[test]
    fn read_pairs_must_match_and_exist() {
        let td = tempdir().expect("failed to create tempdir");
        let r1 = vec![td.path().join("r1.fq")];
        let r2 = vec![td.path().join("r2.fq")];
        fs::write(&r1[0], "").expect("failed to write r1");
        fs::write(&r2[0], "").expect("failed to write r2");

        super::check_read_pairs(&r1, &r2).expect("a valid pair");

        // piscem would map only the first pair here, without an error.
        let two_r1 = vec![r1[0].clone(), r1[0].clone()];
        let err = super::check_read_pairs(&two_r1, &r2)
            .expect_err("unequal read counts must be rejected");
        assert!(
            err.to_string()
                .contains("2 read1 files and 1 read2 files were given"),
            "{err:#}"
        );

        let missing = vec![td.path().join("missing_r2.fq")];
        let err = super::check_read_pairs(&r1, &missing)
            .expect_err("a missing read file must be rejected");
        assert!(format!("{err:#}").contains("missing_r2.fq"), "{err:#}");

        assert!(super::check_read_pairs(&[], &[]).is_err());
    }

    /// Write a stand-in for `piscem` that records the `-w` it was given (and
    /// fails if that directory does not exist), then exits with `status`.
    #[cfg(unix)]
    fn fake_piscem(dir: &Path, status: i32) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let record = dir.join(format!("work_dir_{status}.txt"));
        let script = dir.join(format!("fake_piscem_{status}"));
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = \"-w\" ]; then\n    \
                 printf '%s' \"$2\" > '{}'\n    test -d \"$2\" || exit 3\n  fi\n  shift\n\
                 done\nexit {status}\n",
                record.display()
            ),
        )
        .expect("failed to write fake piscem");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755))
            .expect("failed to chmod fake piscem");
        (script, record)
    }

    #[cfg(unix)]
    #[test]
    fn probe_index_build_uses_its_own_work_dir() {
        let td = tempdir().expect("failed to create tempdir");
        let probes = td.path().join("probes.fa");
        fs::write(&probes, ">P1\nACGTACGTACGTACGTACGTACGTA\n").expect("failed to write probes");
        let out = td.path().join("out");
        let out_str = out.display().to_string();
        let opts = parse_multiplex_quant_opts(&["-o", &out_str]);
        let index_dir = out.join("probe_index");

        // Success: the work dir lives under the index dir and is removed.
        let (piscem, record) = fake_piscem(td.path(), 0);
        super::build_index_from_probe_set(&probes, &opts, &piscem, ProbeT2gMode::Gene)
            .expect("fake build should succeed");
        let work_dir = PathBuf::from(fs::read_to_string(&record).unwrap());
        assert_eq!(work_dir.parent(), Some(index_dir.as_path()));
        assert!(
            !work_dir.exists(),
            "work dir should be removed after success"
        );

        // Failure: the work dir is kept for inspection.
        let (piscem, record) = fake_piscem(td.path(), 1);
        super::build_index_from_probe_set(&probes, &opts, &piscem, ProbeT2gMode::Gene)
            .expect_err("fake build should fail");
        let failed_work_dir = PathBuf::from(fs::read_to_string(&record).unwrap());
        assert_eq!(failed_work_dir.parent(), Some(index_dir.as_path()));
        assert_ne!(
            failed_work_dir, work_dir,
            "each build gets its own work dir"
        );
        assert!(
            failed_work_dir.is_dir(),
            "work dir should be kept on failure"
        );
    }

    #[test]
    fn usa_flag_maps_to_usa_mode() {
        let opts = parse_multiplex_quant_opts(&["-o", ".", "--usa"]);
        assert_eq!(t2g_mode(&opts), ProbeT2gMode::Usa);
    }

    #[test]
    fn excluded_probes_default_to_decoy_and_accept_ignore() {
        use crate::utils::probe_utils::ExcludedProbeMode;
        let opts = parse_multiplex_quant_opts(&["-o", "."]);
        assert_eq!(opts.excluded_probes, ExcludedProbeMode::Decoy);
        let opts = parse_multiplex_quant_opts(&["-o", ".", "--excluded-probes", "ignore"]);
        assert_eq!(opts.excluded_probes, ExcludedProbeMode::Ignore);
    }

    #[test]
    fn csv_probe_set_exposes_excluded_probes_as_decoys() {
        use crate::utils::probe_utils::ExcludedProbeMode;
        let td = tempfile::tempdir().expect("failed to create tempdir");
        let csv_path = td.path().join("probes.csv");
        std::fs::write(
            &csv_path,
            "gene_id,probe_seq,probe_id,included\nG1,AAAA,P1,TRUE\nG1,CCCC,P2,FALSE\n",
        )
        .expect("failed to write probe CSV");

        let files = super::prepare_probe_set_files(&csv_path, td.path(), ExcludedProbeMode::Decoy)
            .expect("conversion failed");
        assert_eq!(
            files.decoy_fasta_path.as_deref(),
            Some(td.path().join("probe_decoys.fa").as_path())
        );

        let files = super::prepare_probe_set_files(&csv_path, td.path(), ExcludedProbeMode::Ignore)
            .expect("conversion failed");
        assert!(files.decoy_fasta_path.is_none());
    }

    #[test]
    fn sample_barcode_orientation_uses_cli_then_preset_then_child_default() {
        let mut opts = parse_multiplex_quant_opts(&["-o", "."]);
        let mut chemistry =
            CustomChemistry::simple_custom("1{b[16]u[12]x:}2{r:}").expect("valid test geometry");
        chemistry.sample_bc_list = Some(SampleBcListInfo {
            plist_name: None,
            remote_url: None,
            sample_bc_ori: Some("reverse".to_string()),
        });

        assert_eq!(
            resolved_sample_bc_orientation(&opts, Some(&chemistry)),
            Some("reverse")
        );
        opts.sample_bc_ori = Some("forward".to_string());
        assert_eq!(
            resolved_sample_bc_orientation(&opts, Some(&chemistry)),
            Some("forward")
        );
        opts.sample_bc_ori = None;
        assert_eq!(resolved_sample_bc_orientation(&opts, None), None);
    }

    #[test]
    fn usa_mode_without_probe_regions_returns_helpful_error() {
        let files = super::ResolvedProbeSetFiles {
            fasta_path: Path::new("/tmp/custom_probes.fa").to_path_buf(),
            gene_t2g_path: Path::new("/tmp/probe_t2g.tsv").to_path_buf(),
            usa_t2g_path: None,
            gene_id_to_name_path: None,
            decoy_fasta_path: None,
        };

        let err = super::select_probe_set_t2g(&files, ProbeT2gMode::Usa)
            .expect_err("USA mode should require splicing annotations");
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("region") && msg.contains("rerun without `--usa`"),
            "unexpected error: {msg}",
        );
    }

    #[test]
    fn barcode_and_stage_resource_overrides_are_forwarded() {
        let opts = parse_multiplex_quant_opts(&[
            "-o",
            ".",
            "--cell-bc-correction",
            "frequency",
            "--cell-bc-neighborhood",
            "hamming-1",
            "--cell-bc-confidence",
            "0.975",
            "--sample-bc-correction",
            "unique",
            "--sample-bc-neighborhood",
            "substitution-or-shift-1",
            "--sample-bc-confidence",
            "39/40",
            "--gpl-memory-limit",
            "1GiB",
            "--gpl-tmp-dir",
            "/tmp/gpl",
            "--collate-memory-limit",
            "4GiB",
        ]);

        let mut gpl = std::process::Command::new("alevin-fry");
        opts.cell_correction.append_to(&mut gpl);
        opts.sample_correction.append_to(&mut gpl);
        opts.gpl_resources.append_to(&mut gpl);
        let gpl_args: Vec<_> = gpl
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(
            gpl_args
                .windows(2)
                .any(|pair| pair == ["--cell-bc-correction", "frequency"])
        );
        assert!(
            gpl_args
                .windows(2)
                .any(|pair| pair == ["--sample-bc-correction", "unique"])
        );
        assert!(
            gpl_args
                .windows(2)
                .any(|pair| pair == ["--memory-limit", "1GiB"])
        );
        assert!(
            gpl_args
                .windows(2)
                .any(|pair| pair == ["--tmp-dir", "/tmp/gpl"])
        );

        let mut collate = std::process::Command::new("alevin-fry");
        opts.collation_resources.append_to(&mut collate);
        let collate_args: Vec<_> = collate
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        // Compression is on by default (lz4), so the parsed opts forward it too.
        assert_eq!(collate_args, ["--memory-limit", "4GiB", "--compress=lz4"]);
    }

    #[test]
    fn legacy_sample_mode_is_hidden_but_still_translates() {
        let opts = parse_multiplex_quant_opts(&["-o", ".", "--sample-correction-mode", "1-edit"]);
        let mut command = std::process::Command::new("alevin-fry");
        opts.sample_correction.append_to(&mut command);
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "--sample-bc-correction",
                "unique",
                "--sample-bc-neighborhood",
                "substitution-or-shift-1",
            ]
        );
    }

    /// A failed build of an auto-selected probe index installs nothing. Its
    /// staging directory, with whatever piscem left in it, is kept when asked
    /// (the run fails), and removed otherwise (the run falls back to the
    /// index already in the slot).
    #[cfg(unix)]
    #[test]
    fn failed_cached_probe_index_build_keeps_its_staging_dir_only_if_asked() {
        use crate::utils::chem_utils::ProbeSetInfo;
        use crate::utils::probe_index_cache::ProbeIndexBuildRecord;
        use crate::utils::prog_utils::ProgInfo;
        use std::os::unix::fs::PermissionsExt;

        let td = tempdir().expect("failed to create tempdir");
        // A piscem whose build writes a partial file next to its `-o`, then fails.
        let piscem = td.path().join("piscem");
        fs::write(
            &piscem,
            "#!/bin/sh\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = \"-o\" ]; then\n    \
             echo partial > \"$2.partial\"\n  fi\n  shift\ndone\nexit 1\n",
        )
        .expect("failed to write fake piscem");
        fs::set_permissions(&piscem, fs::Permissions::from_mode(0o755))
            .expect("failed to chmod fake piscem");
        let piscem_info = ProgInfo {
            exe_path: piscem.clone(),
            version: "0.24.0".to_string(),
        };

        let cache_dir = td.path().join("probe_indices");
        let downloads = cache_dir.join("downloads");
        fs::create_dir_all(&downloads).expect("failed to create downloads dir");
        // Already downloaded, so nothing is fetched.
        fs::write(
            downloads.join("probes.csv"),
            "gene_id,probe_seq,probe_id,included\nG1,ACGTACGTACGTACGTACGTACGTA,P1,TRUE\n",
        )
        .expect("failed to write probe set");
        let probe_info = ProbeSetInfo {
            name: "probes".to_string(),
            plist_name: Some("abc".to_string()),
            remote_url: Some("https://example.invalid/probes.csv".to_string()),
        };
        let out = td.path().join("out");
        let out_str = out.display().to_string();
        let opts = parse_multiplex_quant_opts(&["-o", &out_str]);
        let slot = cache_dir.join(format!("abc_{}", opts.kmer_length));
        let record =
            ProbeIndexBuildRecord::for_run(&piscem_info, opts.kmer_length, opts.excluded_probes)
                .expect("failed to build record");

        let build = |keep_failed_build: bool| {
            super::build_cached_probe_index(
                &probe_info,
                &cache_dir,
                &slot,
                &record,
                &opts,
                &piscem,
                ProbeT2gMode::Gene,
                keep_failed_build,
            )
            .expect_err("the fake build should fail");
            assert!(!slot.exists(), "a failed build must not be installed");
        };
        let staging_prefix = format!(".abc_{}.tmp-", opts.kmer_length);
        let staging_dirs = || -> Vec<_> {
            fs::read_dir(&cache_dir)
                .expect("failed to list cache dir")
                .map(|entry| entry.expect("failed to read cache entry").path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with(&staging_prefix))
                })
                .collect()
        };

        build(false);
        let left = staging_dirs();
        assert!(left.is_empty(), "expected no staging dir left: {left:?}");

        build(true);
        let kept = staging_dirs();
        assert_eq!(kept.len(), 1, "expected one kept staging dir: {kept:?}");
        assert!(
            kept[0].join("probe_index").join("index.partial").is_file(),
            "the failed build's files should be kept"
        );
    }

    /// Write a stand-in for `piscem` whose build writes the index files of its
    /// `-o` prefix and exits 0.
    #[cfg(unix)]
    fn fake_piscem_that_builds(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let script = dir.join("fake_piscem_builds");
        fs::write(
            &script,
            "#!/bin/sh\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = \"-o\" ]; then\n    \
             for e in ctab refinfo ssi ssi.mphf; do : > \"$2.$e\"; done\n  fi\n  shift\n\
             done\nexit 0\n",
        )
        .expect("failed to write fake piscem");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755))
            .expect("failed to chmod fake piscem");
        script
    }

    const TOY_PROBE_CSV: &str = "#probe_set_file_format=3.0\n\
        gene_id,probe_seq,probe_id,included,region,gene_name\n\
        G1,ACGTACGTACGTACGTACGTACGTA,G1|A|1,TRUE,spliced,Gene1\n";

    #[cfg(unix)]
    #[test]
    fn probe_index_built_from_a_csv_keeps_the_csv() {
        let td = tempdir().expect("failed to create tempdir");
        let probes = td.path().join("probes.csv");
        fs::write(&probes, TOY_PROBE_CSV).expect("failed to write probes");
        let out = td.path().join("out");
        let opts = parse_multiplex_quant_opts(&["-o", out.to_str().unwrap()]);
        let piscem = fake_piscem_that_builds(td.path());

        let resolved =
            super::build_index_from_probe_set(&probes, &opts, &piscem, ProbeT2gMode::Gene)
                .expect("fake build should succeed");

        let copy = out.join("probe_index").join("probe_set.csv");
        assert_eq!(resolved.3, Some(copy.clone()));
        assert_eq!(
            fs::read_to_string(&copy).expect("missing copy"),
            TOY_PROBE_CSV
        );
    }

    /// `std::fs::copy` onto the same file empties it; a `--probe-set` that
    /// already is the index copy must stay as it is.
    #[cfg(unix)]
    #[test]
    fn probe_set_that_is_the_probe_index_copy_is_not_emptied() {
        let td = tempdir().expect("failed to create tempdir");
        let out = td.path().join("out");
        let probe_index = out.join("probe_index");
        fs::create_dir_all(&probe_index).expect("failed to create probe index dir");
        let probes = probe_index.join("probe_set.csv");
        fs::write(&probes, TOY_PROBE_CSV).expect("failed to write probes");
        let opts = parse_multiplex_quant_opts(&["-o", out.to_str().unwrap()]);
        let piscem = fake_piscem_that_builds(td.path());

        let resolved =
            super::build_index_from_probe_set(&probes, &opts, &piscem, ProbeT2gMode::Gene)
                .expect("fake build should succeed");

        assert_eq!(resolved.3, Some(probes.clone()));
        assert_eq!(
            fs::read_to_string(&probes).expect("missing CSV"),
            TOY_PROBE_CSV
        );
    }

    #[cfg(unix)]
    #[test]
    fn probe_index_built_from_a_fasta_has_no_probe_set() {
        let td = tempdir().expect("failed to create tempdir");
        let probes = td.path().join("probes.fa");
        fs::write(&probes, ">P1\nACGTACGTACGTACGTACGTACGTA\n").expect("failed to write probes");
        let out = td.path().join("out");
        let opts = parse_multiplex_quant_opts(&["-o", out.to_str().unwrap()]);
        let piscem = fake_piscem_that_builds(td.path());

        let resolved =
            super::build_index_from_probe_set(&probes, &opts, &piscem, ProbeT2gMode::Gene)
                .expect("fake build should succeed");

        assert_eq!(resolved.3, None);
        assert!(!out.join("probe_index").join("probe_set.csv").exists());
    }

    /// The auto-selected (chemistry registry) probe index keeps the CSV in the
    /// cache slot: the path a fresh build returns points into the slot, not
    /// into the staging dir it was built in, and a later cache hit finds it.
    #[cfg(unix)]
    #[test]
    fn cached_probe_index_keeps_the_csv_in_its_slot() {
        use crate::utils::chem_utils::CustomChemistry;
        use crate::utils::prog_utils::ProgInfo;

        let td = tempdir().expect("failed to create tempdir");
        let piscem = fake_piscem_that_builds(td.path());
        let piscem_info = ProgInfo {
            exe_path: piscem.clone(),
            version: "0.24.0".to_string(),
        };
        let af_home = td.path().join("af_home");
        let downloads = af_home.join("probe_indices").join("downloads");
        fs::create_dir_all(&downloads).expect("failed to create downloads dir");
        // Already downloaded, so nothing is fetched.
        fs::write(downloads.join("probes.csv"), TOY_PROBE_CSV).expect("failed to write probe set");
        let chem: CustomChemistry = serde_json::from_value(json!({
            "geometry": "1{b[16]u[12]x:}2{r:}",
            "probe_sets": {"human": {
                "name": "probes",
                "plist_name": "abc",
                "remote_url": "https://example.invalid/probes.csv"
            }}
        }))
        .expect("failed to build chemistry");
        let out = td.path().join("out");
        let opts =
            parse_multiplex_quant_opts(&["-o", out.to_str().unwrap(), "--organism", "human"]);
        let slot_dir = af_home
            .join("probe_indices")
            .join(format!("abc_{}_decoy", opts.kmer_length))
            .join("probe_index");

        // First run: build and install. Second run: cache hit.
        for run in ["build", "cache hit"] {
            let resolved = super::resolve_probe_index(
                &af_home,
                Some(&chem),
                &opts,
                &piscem_info,
                ProbeT2gMode::Gene,
            )
            .expect("failed to resolve the cached probe index");
            let copy = slot_dir.join("probe_set.csv");
            assert_eq!(resolved.3, Some(copy.clone()), "{run}");
            assert_eq!(
                fs::read_to_string(&copy).expect("missing copy"),
                TOY_PROBE_CSV,
                "{run}"
            );
        }
    }

    #[test]
    fn probe_set_is_copied_into_the_quant_output_unchanged() {
        let td = tempdir().expect("failed to create tempdir");
        let src = td.path().join("10x_probes.csv");
        fs::write(&src, TOY_PROBE_CSV).expect("failed to write probe csv");
        let quant_output = td.path().join("af_quant");
        fs::create_dir_all(&quant_output).expect("failed to create af_quant");

        super::copy_probe_set_into(Some(&src), &quant_output).expect("copy should succeed");

        assert_eq!(
            fs::read_to_string(quant_output.join("probe_set.csv")).expect("missing copy"),
            TOY_PROBE_CSV,
            "the copy must be the original CSV, header lines included"
        );
    }

    #[test]
    fn probe_set_that_is_the_quant_output_copy_is_not_emptied() {
        let td = tempdir().expect("failed to create tempdir");
        let quant_output = td.path().join("af_quant");
        fs::create_dir_all(&quant_output).expect("failed to create af_quant");
        let csv = quant_output.join("probe_set.csv");
        fs::write(&csv, TOY_PROBE_CSV).expect("failed to write probe csv");

        // The same file, also when it is named another way.
        let other_name = td.path().join("af_quant").join(".").join("probe_set.csv");
        super::copy_probe_set_into(Some(&other_name), &quant_output).expect("no copy needed");

        assert_eq!(
            fs::read_to_string(&csv).expect("missing CSV"),
            TOY_PROBE_CSV
        );
    }

    #[test]
    fn no_probe_set_means_no_copy_in_the_quant_output() {
        let td = tempdir().expect("failed to create tempdir");
        super::copy_probe_set_into(None, td.path()).expect("nothing to copy");
        assert!(!td.path().join("probe_set.csv").exists());
    }

    #[test]
    fn missing_probe_set_fails_the_copy_and_names_the_file() {
        let td = tempdir().expect("failed to create tempdir");
        let missing = td.path().join("missing.csv");
        let err = super::copy_probe_set_into(Some(&missing), td.path())
            .expect_err("a missing source must fail");
        assert!(
            format!("{err:#}").contains("missing.csv"),
            "the error must name the source: {err:#}"
        );
    }

    #[test]
    fn missing_probe_set_source_fails_before_mapping() {
        let td = tempdir().expect("failed to create tempdir");
        let missing = td.path().join("missing.csv");
        let err = super::check_probe_set_source(Some(&missing), true, false, false)
            .expect_err("a missing probe set must fail");
        assert!(
            format!("{err:#}").contains("missing.csv"),
            "the error must name the file: {err:#}"
        );
    }

    #[test]
    fn existing_or_absent_probe_set_source_passes_the_check() {
        let td = tempdir().expect("failed to create tempdir");
        let csv = td.path().join("probes.csv");
        fs::write(&csv, TOY_PROBE_CSV).expect("failed to write probe csv");
        super::check_probe_set_source(Some(&csv), true, true, false)
            .expect("an existing CSV passes");
        // An index without the CSV (e.g. an old cache slot) only logs that
        // nothing is copied.
        super::check_probe_set_source(None, true, false, false).expect("no CSV is not an error");
        super::check_probe_set_source(None, false, false, false).expect("no CSV is not an error");
    }

    /// A `--probe-set` CSV given with `--index` that does not exist fails at
    /// index resolution, before mapping, although the index has all it needs.
    #[test]
    fn missing_probe_set_given_with_a_complete_index_fails_before_mapping() {
        use crate::utils::prog_utils::ProgInfo;
        let td = tempdir().expect("failed to create tempdir");
        let root = td.path().join("simpleaf_output");
        let index_dir = write_simpleaf_probe_index(&root);
        fs::write(index_dir.join("gene_id_to_name.tsv"), "G1\tGene1\n")
            .expect("failed to write gene names");
        let missing = td.path().join("missing.csv");
        let opts = parse_multiplex_quant_opts(&[
            "-o",
            td.path().join("out").to_str().unwrap(),
            "-i",
            root.to_str().unwrap(),
            "--probe-set",
            missing.to_str().unwrap(),
        ]);
        let piscem = ProgInfo {
            exe_path: PathBuf::from("/bin/false"),
            version: "0.24.0".to_string(),
        };

        let (_, t2g, names, probe_set) =
            super::resolve_probe_index(td.path(), None, &opts, &piscem, ProbeT2gMode::Gene)
                .expect("the index alone resolves");
        assert!(t2g.is_some() && names.is_some(), "the index supplies both");
        let err = super::check_probe_set_source(probe_set.as_deref(), true, false, false)
            .expect_err("the missing CSV must fail");
        assert!(format!("{err:#}").contains("missing.csv"), "{err:#}");
    }

    #[test]
    fn probe_h5ad_without_a_probe_set_csv_fails_before_mapping() {
        // An index (or cache slot) without probe_set.csv.
        let err = super::check_probe_set_source(None, true, true, false)
            .expect_err("the probe h5ad needs the CSV");
        let msg = format!("{err:#}");
        assert!(msg.contains("this index has no probe_set.csv"), "{msg}");
        assert!(
            msg.contains("--probe-set") && msg.contains("--index"),
            "{msg}"
        );
        // A FASTA --probe-set.
        let err = super::check_probe_set_source(None, true, true, true)
            .expect_err("the probe h5ad needs the CSV");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("FASTA probe set has no probe-to-gene map"),
            "{msg}"
        );
        assert!(msg.contains("--probe-set"), "{msg}");
    }

    /// What the index error tells the user works: `--index <dir>` of an index
    /// without probe_set.csv, together with `--probe-set <csv>`, gives the CSV
    /// that the probe h5ad needs.
    #[test]
    fn probe_set_csv_with_an_index_that_has_none_passes_the_probe_h5ad_check() {
        use crate::utils::prog_utils::ProgInfo;
        let td = tempdir().expect("failed to create tempdir");
        // A multiplex-quant probe index (like a cache slot) built before
        // simpleaf kept the CSV.
        let probe_index_dir = td.path().join("probe_index");
        fs::create_dir_all(&probe_index_dir).expect("failed to create probe index dir");
        write_piscem_index(&probe_index_dir.join("index"));
        fs::write(probe_index_dir.join("probe_t2g.tsv"), "P1\tG1\n")
            .expect("failed to write gene t2g");
        fs::write(probe_index_dir.join("gene_id_to_name.tsv"), "G1\tGene1\n")
            .expect("failed to write gene names");
        let csv = td.path().join("probes.csv");
        fs::write(&csv, TOY_PROBE_CSV).expect("failed to write probe csv");
        let piscem = ProgInfo {
            exe_path: PathBuf::from("/bin/false"),
            version: "0.24.0".to_string(),
        };
        let out = td.path().join("out");

        for (with_csv, ok) in [(false, false), (true, true)] {
            let mut args = vec![
                "-o",
                out.to_str().unwrap(),
                "-i",
                probe_index_dir.to_str().unwrap(),
                "--anndata-out",
                "--probe-mtx",
            ];
            if with_csv {
                args.extend(["--probe-set", csv.to_str().unwrap()]);
            }
            let opts = parse_multiplex_quant_opts(&args);
            let (_, _, _, probe_set) =
                super::resolve_probe_index(td.path(), None, &opts, &piscem, ProbeT2gMode::Gene)
                    .expect("the index resolves");
            let check = super::check_probe_set_source(
                probe_set.as_deref(),
                super::is_probe_set_reference(&opts),
                opts.anndata_out && opts.probe_mtx,
                false,
            );
            assert_eq!(check.is_ok(), ok, "with --probe-set: {with_csv}: {check:?}");
        }
    }

    /// The smallest `af_quant` of a `quant-probe --probe-mtx` run: 2 cells x 2
    /// probes of one gene, and the files af-anndata reads for `uns`.
    fn write_probe_af_quant(root: &Path) {
        let alevin = root.join("alevin");
        fs::create_dir_all(&alevin).expect("failed to create alevin dir");
        fs::write(
            alevin.join("probe_quants_mat.mtx"),
            "%%MatrixMarket matrix coordinate real general\n2 2 2\n1 1 3\n2 2 1\n",
        )
        .expect("failed to write mtx");
        // The probe rows file is a byte copy of the gene rows file, which
        // featureDump.txt follows; af-anndata checks this.
        for rows in ["quants_mat_rows.txt", "probe_quants_mat_rows.txt"] {
            fs::write(alevin.join(rows), "S1_AAAA\nS1_CCCC\n").expect("failed to write rows");
        }
        fs::write(alevin.join("probe_quants_mat_cols.txt"), "G1|A|1\nG1|A|2\n")
            .expect("failed to write cols");
        fs::write(
            root.join("featureDump.txt"),
            "CB\tsample_name\tCorrectedReads\tMappedReads\tDeduplicatedReads\tMappingRate\tDedupRate\tMeanByMax\tNumGenesExpressed\tNumGenesOverMean\n\
             AAAA\tS1\t3\t3\t3\t1\t0\t1\t1\t1\n\
             CCCC\tS1\t1\t1\t1\t1\t0\t1\t1\t1\n",
        )
        .expect("failed to write featureDump");
        fs::write(
            root.join("quant.json"),
            r#"{"usa_mode": false, "num_genes": 1}"#,
        )
        .expect("failed to write quant.json");
        fs::write(root.join("collate.json"), r#"{"multi_barcode": true}"#)
            .expect("failed to write collate.json");
        fs::write(root.join("generate_permit_list.json"), "{}").expect("failed to write gpl json");
    }

    #[test]
    fn probe_h5ad_is_written_next_to_the_gene_h5ad() {
        let td = tempdir().expect("failed to create tempdir");
        let quant_output = td.path().join("af_quant");
        write_probe_af_quant(&quant_output);
        let csv = td.path().join("probes.csv");
        fs::write(
            &csv,
            "#probe_set_file_format=3.0\ngene_id,probe_seq,probe_id,included,region,gene_name\n\
             G1,ACGT,G1|A|1,TRUE,spliced,Gene1\nG1,TTGC,G1|A|2,TRUE,unspliced,Gene1\n",
        )
        .expect("failed to write probe csv");

        let path = super::write_probe_h5ad(&quant_output, Some(&csv))
            .expect("the probe h5ad must be written");

        assert_eq!(path, quant_output.join("alevin").join("probe_quants.h5ad"));
        assert!(path.is_file(), "missing {}", path.display());
    }

    #[test]
    fn umi_edit_dist_is_unset_unless_given() {
        let ued = |args: &[&str]| {
            let mut cli = vec!["-o", "."];
            cli.extend_from_slice(args);
            parse_multiplex_quant_opts(&cli).umi_edit_dist
        };
        // Unset: nothing is forwarded, so alevin-fry's own default applies.
        assert_eq!(ued(&[]), None);
        assert_eq!(ued(&["--umi-edit-dist", "1"]), Some(1));
        assert_eq!(ued(&["--umi-edit-dist", "0"]), Some(0));
        for bad in ["x", "2"] {
            assert!(
                try_parse_multiplex_quant(&["-1", "r1.fq", "-2", "r2.fq", "--umi-edit-dist", bad])
                    .is_err()
            );
        }
    }

    #[test]
    fn umi_edit_dist_with_usa_is_rejected_up_front() {
        let check = |args: &[&str]| {
            let mut cli = vec!["-o", ".", "--probe-set", "p.csv"];
            cli.extend_from_slice(args);
            let opts = parse_multiplex_quant_opts(&cli);
            let af_caps = caps(true, true);
            let axis = gene_axis_reason(&opts, &af_caps);
            validate_alevin_fry_options(&opts, &af_caps, axis, Path::new("alevin-fry"))
                .map_err(|e| format!("{e:#}"))
        };
        for r in ["cr-like", "cr-like-em"] {
            let err = check(&["--usa", "-r", r, "--umi-edit-dist", "1"]).expect_err("rejected");
            assert!(err.contains("`--usa` was given"), "{err}");
            assert!(check(&["--usa", "-r", r, "--umi-edit-dist", "0"]).is_ok());
            assert!(check(&["-r", r, "--umi-edit-dist", "1"]).is_ok());
            assert!(check(&["--gene-level", "-r", r, "--umi-edit-dist", "1"]).is_ok());
        }
        assert!(check(&["--usa", "-r", "parsimony", "--umi-edit-dist", "1"]).is_ok());
    }
}
