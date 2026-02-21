use crate::atac::commands::ProcessOpts;
use crate::core::{context, exec, index_meta, io, runtime};
use crate::utils::chem_utils::ExpectedOri;
use crate::utils::chem_utils::QueryInRegistry;
use crate::utils::chem_utils::get_single_custom_chem_from_file;
use crate::utils::constants::CHEMISTRIES_PATH;
use crate::utils::{prog_utils, prog_utils::ReqProgs};
use anyhow;
use anyhow::{Context, bail};
use serde_json::json;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{info, warn};

pub(crate) struct MapStageOutput {
    pub map_output: PathBuf,
    pub map_duration_secs: f64,
    pub map_cmd: String,
}

struct GplStageOutput {
    gpl_duration_secs: f64,
    gpl_cmd: String,
}

struct SortStageOutput {
    sort_duration_secs: f64,
    sort_cmd: String,
}

struct MacsStageOutput {
    macs_duration_secs: f64,
    macs_cmd: String,
}

fn push_advanced_piscem_options(
    piscem_map_cmd: &mut std::process::Command,
    opts: &ProcessOpts,
) -> anyhow::Result<()> {
    if opts.ignore_ambig_hits {
        piscem_map_cmd.arg("--ignore-ambig-hits");
    } else {
        piscem_map_cmd
            .arg("--max-ec-card")
            .arg(opts.max_ec_card.to_string());
    }

    if opts.no_poison {
        piscem_map_cmd.arg("--no-poison");
    }

    if opts.no_tn5_shift {
        piscem_map_cmd.arg("--no-tn5-shift");
    }

    if opts.check_kmer_orphan {
        piscem_map_cmd.arg("--check-kmer-orphan");
    }

    if opts.use_chr {
        piscem_map_cmd.arg("--use-chr");
    }

    piscem_map_cmd
        .arg("--max-hit-occ")
        .arg(opts.max_hit_occ.to_string());

    piscem_map_cmd
        .arg("--max-hit-occ-recover")
        .arg(opts.max_hit_occ_recover.to_string());

    piscem_map_cmd
        .arg("--max-read-occ")
        .arg(opts.max_read_occ.to_string());

    Ok(())
}

fn add_read_args(map_cmd: &mut std::process::Command, opts: &ProcessOpts) -> anyhow::Result<()> {
    if let Some(ref reads1) = opts.reads1 {
        let reads2 = opts
            .reads2
            .as_ref()
            .context("read2 files must be provided when read1 files are provided.")?;
        let barcode_reads: &Vec<PathBuf> = &opts.barcode_reads;
        if reads1.len() != reads2.len() || reads1.len() != barcode_reads.len() {
            bail!(
                "{} read1 files, {} read2 files, and {} barcode read files were given; Cannot proceed!",
                reads1.len(),
                reads2.len(),
                barcode_reads.len()
            );
        }

        prog_utils::check_files_exist(reads1)?;
        prog_utils::check_files_exist(reads2)?;
        prog_utils::check_files_exist(barcode_reads)?;

        let reads1_str = reads1
            .iter()
            .map(|x| x.to_string_lossy().into_owned())
            .collect::<Vec<String>>()
            .join(",");
        map_cmd.arg("-1").arg(reads1_str);

        let reads2_str = reads2
            .iter()
            .map(|x| x.to_string_lossy().into_owned())
            .collect::<Vec<String>>()
            .join(",");
        map_cmd.arg("-2").arg(reads2_str);

        let bc_str = barcode_reads
            .iter()
            .map(|x| x.to_string_lossy().into_owned())
            .collect::<Vec<String>>()
            .join(",");
        map_cmd.arg("--barcode").arg(bc_str);
    } else {
        let reads = opts.reads.as_ref().context(
            "single-end reads must be provided when read1/read2 files are not provided.",
        )?;
        let barcode_reads: &Vec<PathBuf> = &opts.barcode_reads;
        if reads.len() != barcode_reads.len() {
            bail!(
                "{} read files and {} barcode read files were given; Cannot proceed!",
                reads.len(),
                barcode_reads.len()
            );
        }

        prog_utils::check_files_exist(reads)?;
        prog_utils::check_files_exist(barcode_reads)?;

        let reads_str = reads
            .iter()
            .map(|x| x.to_string_lossy().into_owned())
            .collect::<Vec<String>>()
            .join(",");
        map_cmd.arg("-r").arg(reads_str);

        let bc_str = barcode_reads
            .iter()
            .map(|x| x.to_string_lossy().into_owned())
            .collect::<Vec<String>>()
            .join(",");
        map_cmd.arg("--barcode").arg(bc_str);
    }
    Ok(())
}

pub(crate) fn check_progs<P: AsRef<Path>>(
    af_home_path: P,
    opts: &ProcessOpts,
) -> anyhow::Result<()> {
    let af_home_path = af_home_path.as_ref();
    let rp: ReqProgs = context::load_required_programs(af_home_path)?;

    let af_prog_info = rp
        .alevin_fry
        .as_ref()
        .context("alevin-fry program info is missing; please run `simpleaf set-paths`.")?;

    match prog_utils::check_version_constraints(
        "alevin-fry",
        ">=0.11.2, <1.0.0",
        &af_prog_info.version,
    ) {
        Ok(af_ver) => info!("found alevin-fry version {:#}, proceeding", af_ver),
        Err(e) => return Err(e),
    }

    let piscem_prog_info = rp
        .piscem
        .as_ref()
        .context("piscem program info is missing; please run `simpleaf set-paths`.")?;

    match prog_utils::check_version_constraints(
        "piscem",
        ">=0.11.0, <1.0.0",
        &piscem_prog_info.version,
    ) {
        Ok(piscem_ver) => info!("found piscem version {:#}, proceeding", piscem_ver),
        Err(e) => return Err(e),
    }

    if opts.call_peaks {
        let macs_prog_info = rp
            .macs
            .as_ref()
            .context(
                "macs3 program info is missing; please run `simpleaf set-paths` before using `--call-peaks`.",
            )?;
        match prog_utils::check_version_constraints(
            "macs3",
            ">=3.0.2, <4.0.0",
            &macs_prog_info.version,
        ) {
            Ok(macs_ver) => info!("found macs3 version {:#}, proceeding", macs_ver),
            Err(e) => return Err(e),
        }
    }

    Ok(())
}

// NOTE: we assume that check_progs has already been called and so version constraints have
// already been checked.
pub(crate) fn map_reads(af_home_path: &Path, opts: &ProcessOpts) -> anyhow::Result<MapStageOutput> {
    let rp: ReqProgs = context::load_required_programs(af_home_path)?;

    let piscem_prog_info = rp
        .piscem
        .as_ref()
        .context("piscem program info is missing; please run `simpleaf set-paths`.")?;

    let index_base = index_meta::resolve_atac_piscem_index_base(opts.index.clone())?;
    prog_utils::check_piscem_index_files(index_base.as_path())?;

    // using a piscem index
    let mut piscem_map_cmd =
        std::process::Command::new(format!("{}", &piscem_prog_info.exe_path.display()));
    let index_path = format!("{}", index_base.display());
    piscem_map_cmd
        .arg("map-sc-atac")
        .arg("--index")
        .arg(index_path);

    piscem_map_cmd
        .arg("--bin-size")
        .arg(opts.bin_size.to_string())
        .arg("--bin-overlap")
        .arg(opts.bin_overlap.to_string());

    // if the user requested more threads than can be used
    let (threads, capped_at) = runtime::cap_threads(opts.threads);
    if let Some(max_threads) = capped_at {
        warn!(
            "The maximum available parallelism is {}, but {} threads were requested.",
            max_threads, opts.threads
        );
        warn!("setting number of threads to {}", max_threads);
    }

    // location of output directory, number of threads
    let map_output = opts.output.join("af_map");
    piscem_map_cmd
        .arg("--threads")
        .arg(threads.to_string())
        .arg("-o")
        .arg(&map_output);

    // add either the paired-end or single-end read arguments
    add_read_args(&mut piscem_map_cmd, opts)?;

    // if the user is requesting a mapping option that required
    // piscem version >= 0.7.0, ensure we have that
    match prog_utils::check_version_constraints(
        "piscem",
        ">=0.11.0, <1.0.0",
        &piscem_prog_info.version,
    ) {
        Ok(_piscem_ver) => {
            push_advanced_piscem_options(&mut piscem_map_cmd, opts)?;
        }
        Err(_) => {
            info!(
                r#"
Simpleaf is currently using piscem version {}, but you must be using version >= 0.11.0 in order to use the 
mapping options specific to this, or later versions. If you wish to use these options, please upgrade your 
piscem version or, if you believe you have a sufficiently new version installed, update the executable 
being used by simpleaf"#,
                &piscem_prog_info.version
            );
        }
    }

    let map_cmd_string = prog_utils::get_cmd_line_string(&piscem_map_cmd);
    info!("map command : {}", map_cmd_string);

    let map_start = Instant::now();
    exec::run_checked(&mut piscem_map_cmd, "[atac::map]")?;
    let map_duration = map_start.elapsed();
    info!("mapping completed successfully in {:#?}", map_duration);

    let af_process_info_file = opts.output.join("simpleaf_process_log.json");
    let af_process_info = json!({
        "time_info" : {
        "map_time" : map_duration.as_secs_f64(),
    },
        "cmd_info" : {
        "map_cmd" : map_cmd_string,
    },
        "map_info" : {
        "mapper" : "piscem",
        "map_cmd" : map_cmd_string,
        "map_outdir": map_output
    }
    });

    // write the relevant info about
    // our run to file.
    io::write_json_pretty_atomic(&af_process_info_file, &af_process_info)?;

    info!("successfully mapped reads and generated output RAD file.");
    Ok(MapStageOutput {
        map_output,
        map_duration_secs: map_duration.as_secs_f64(),
        map_cmd: map_cmd_string,
    })
}

fn macs_call_peaks(af_home_path: &Path, opts: &ProcessOpts) -> anyhow::Result<MacsStageOutput> {
    let rp: ReqProgs = context::load_required_programs(af_home_path)?;

    let macs_prog_info = rp
        .macs
        .as_ref()
        .context("macs program info is missing; please run `simpleaf set-paths`.")?;

    let gpl_dir = opts.output.join("af_process");
    let bedsuf = if opts.compress { ".bed.gz" } else { ".bed" };
    let bed_input = gpl_dir.join(format!("map{}", bedsuf));
    let peaks_output = gpl_dir.join("macs");
    let mut macs_cmd =
        std::process::Command::new(format!("{}", &macs_prog_info.exe_path.display()));
    macs_cmd
        .arg("callpeak")
        .arg("-f")
        .arg("BEDPE")
        .arg("--nomodel")
        .arg("--extsize")
        .arg(opts.extsize.to_string())
        .arg("--keep-dup")
        .arg("all")
        .arg("-q")
        .arg(opts.qvalue.to_string())
        .arg("-g")
        .arg(opts.gsize.as_arg_str())
        .arg("-t")
        .arg(bed_input)
        .arg("-n")
        .arg(peaks_output);

    let macs_cmd_string = prog_utils::get_cmd_line_string(&macs_cmd);
    info!("macs3 command : {}", macs_cmd_string);

    let macs_start = Instant::now();
    exec::run_checked(&mut macs_cmd, "[atac::macs]")?;
    let macs_duration = macs_start.elapsed();
    info!("macs completed successfully in {:#?}", macs_duration);

    let af_process_info_file = opts.output.join("simpleaf_process_log.json");
    let json_file = std::fs::File::open(af_process_info_file.clone())
        .with_context(|| format!("couldn't open file {}", af_process_info_file.display()))?;
    let json_reader = BufReader::new(json_file);
    let mut af_process_info: serde_json::Value = serde_json::from_reader(json_reader)
        .with_context(|| {
            format!(
                "couldn't parse JSON content from {}",
                af_process_info_file.display()
            )
        })?;

    af_process_info["time_info"]["macs_time"] = json!(macs_duration.as_secs_f64());
    af_process_info["cmd_info"]["macs_cmd"] = json!(macs_cmd_string);

    // write the relevant info about
    // our run to file.
    io::write_json_pretty_atomic(&af_process_info_file, &af_process_info)?;

    info!("successfully called peaks using macs3.");

    Ok(MacsStageOutput {
        macs_duration_secs: macs_duration.as_secs_f64(),
        macs_cmd: macs_cmd_string,
    })
}

pub(crate) fn gen_bed(af_home_path: &Path, opts: &ProcessOpts) -> anyhow::Result<()> {
    let gpl = af_gpl(af_home_path, opts)?;
    let sort = af_sort(af_home_path, opts)?;
    let macs = macs_call_peaks(af_home_path, opts)?;
    info!(
        "ATAC downstream stages completed (gpl: {:.2}s, sort: {:.2}s, macs: {:.2}s).",
        gpl.gpl_duration_secs, sort.sort_duration_secs, macs.macs_duration_secs
    );
    info!(
        "ATAC commands: gpl=`{}`, sort=`{}`, macs=`{}`",
        gpl.gpl_cmd, sort.sort_cmd, macs.macs_cmd
    );
    Ok(())
}

// NOTE: we assume that check_progs has already been called and so version constraints have
// already been checked.
fn af_sort(af_home_path: &Path, opts: &ProcessOpts) -> anyhow::Result<SortStageOutput> {
    let rp: ReqProgs = context::load_required_programs(af_home_path)?;

    let af_prog_info = rp
        .alevin_fry
        .as_ref()
        .context("alevin-fry program info is missing; please run `simpleaf set-paths`.")?;

    let gpl_dir = opts.output.join("af_process");
    let rad_dir = opts.output.join("af_map");
    let mut af_sort = std::process::Command::new(format!("{}", &af_prog_info.exe_path.display()));
    af_sort
        .arg("atac")
        .arg("sort")
        .arg("--input-dir")
        .arg(gpl_dir)
        .arg("--rad-dir")
        .arg(rad_dir);

    // if the user requested more threads than can be used
    let (threads, capped_at) = runtime::cap_threads(opts.threads);
    if let Some(max_threads) = capped_at {
        warn!(
            "The maximum available parallelism is {}, but {} threads were requested.",
            max_threads, opts.threads
        );
        warn!("setting number of threads to {}", max_threads);
    }
    af_sort.arg("--threads").arg(threads.to_string());

    if opts.compress {
        af_sort.arg("--compress");
    }

    let sort_cmd_string = prog_utils::get_cmd_line_string(&af_sort);
    info!("sort command : {}", sort_cmd_string);

    let af_sort_start = Instant::now();
    exec::run_checked(&mut af_sort, "[atac::af_sort]")?;
    let af_sort_duration = af_sort_start.elapsed();
    info!("sort completed successfully in {:#?}", af_sort_duration);

    let af_process_info_file = opts.output.join("simpleaf_process_log.json");
    let json_file = std::fs::File::open(af_process_info_file.clone())
        .with_context(|| format!("couldn't open file {}", af_process_info_file.display()))?;
    let json_reader = BufReader::new(json_file);
    let mut af_process_info: serde_json::Value = serde_json::from_reader(json_reader)
        .with_context(|| {
            format!(
                "couldn't parse JSON content from {}",
                af_process_info_file.display()
            )
        })?;

    af_process_info["time_info"]["sort_time"] = json!(af_sort_duration.as_secs_f64());
    af_process_info["cmd_info"]["sort_cmd"] = json!(sort_cmd_string);

    // write the relevant info about
    // our run to file.
    io::write_json_pretty_atomic(&af_process_info_file, &af_process_info)?;

    info!("successfully sorted and deduplicated records and created the output BED file.");
    Ok(SortStageOutput {
        sort_duration_secs: af_sort_duration.as_secs_f64(),
        sort_cmd: sort_cmd_string,
    })
}

// NOTE: we assume that check_progs has already been called and so version constraints have
// already been checked.
fn af_gpl(af_home_path: &Path, opts: &ProcessOpts) -> anyhow::Result<GplStageOutput> {
    let rp: ReqProgs = context::load_required_programs(af_home_path)?;

    let af_prog_info = rp
        .alevin_fry
        .as_ref()
        .context("alevin-fry program info is missing; please run `simpleaf set-paths`.")?;

    let filter_meth_opt;

    use crate::utils::af_utils;
    // based on the filtering method
    let pl_file = &opts.unfiltered_pl;
    {
        // NOTE: unfiltered_pl is of type Option<Option<PathBuf>> so being in here
        // tells us nothing about the inner option.  We handle that now.

        // if the -u flag is passed and some file is provided, then the inner
        // Option is Some(PathBuf)
        if let Some(pl_file) = pl_file {
            // the user has explicily passed a file along, so try
            // to use that
            if pl_file.is_file() {
                let min_cells = opts.min_reads;
                filter_meth_opt = Some(af_utils::CellFilterMethod::UnfilteredExternalList(
                    pl_file.to_string_lossy().into_owned(),
                    min_cells,
                ));
            } else {
                bail!(
                    "The provided path {} does not exist as a regular file.",
                    pl_file.display()
                );
            }
        } else {
            // here, the -u flag is provided
            // but no file is provided, then the
            // inner option is None and we will try to get the permit list automatically if
            // using 10xv2, 10xv3, or 10x-multi

            // check the chemistry
            let rc = af_utils::Chemistry::Atac(opts.chemistry);
            let pl_res = af_utils::get_permit_if_absent(af_home_path, &rc)?;
            let min_cells = opts.min_reads;
            match pl_res {
                af_utils::PermitListResult::DownloadSuccessful(p)
                | af_utils::PermitListResult::AlreadyPresent(p) => {
                    filter_meth_opt = Some(af_utils::CellFilterMethod::UnfilteredExternalList(
                        p.to_string_lossy().into_owned(),
                        min_cells,
                    ));
                }
                af_utils::PermitListResult::UnregisteredChemistry => {
                    bail!(
                        "Cannot automatically obtain an unfiltered permit list for an unregistered chemistry : {}.",
                        opts.chemistry.as_str()
                    );
                }
            }
        }
    }
    /*else {
        bail!(
            "Only the unfiltered permit list option is currently supported in atac-seq processing."
        );
    }*/

    // see if we need to reverse complement barcodes
    let custom_chem_p = af_home_path.join(CHEMISTRIES_PATH);
    let permit_bc_ori = if let Some(ori) = &opts.permit_barcode_ori {
        info!("Using user-provided permitlist barcode orientation");
        match ori {
            ExpectedOri::Forward => "fw",
            ExpectedOri::Reverse => "rc",
            _ => "rc",
        }
    } else {
        info!("Fetching permitlits barcode orientation from file");
        let mut pbco = "rc";
        if custom_chem_p.is_file() {
            let chem_key = opts.chemistry.registry_key();
            if let Some(chem_obj) = get_single_custom_chem_from_file(&custom_chem_p, chem_key)? {
                if let Some(serde_json::Value::Object(meta_obj)) = chem_obj.meta() {
                    let fw_str = serde_json::Value::String(String::from("forward"));
                    let dir_str = meta_obj.get("barcode_ori").unwrap_or(&fw_str);
                    match dir_str.as_str() {
                        Some("reverse") => {
                            info!("\treverse-complement");
                            pbco = "rc";
                        }
                        Some("forward") => {
                            info!("\tforward");
                            pbco = "fw";
                        }
                        Some(s) => {
                            warn!("barcode_ori \"{}\" is unknown; assuming forward.", s);
                        }
                        None => {
                            warn!(
                                "couldn't interpret value associated with \"barcode_ori\" as a string; assuming forward."
                            );
                        }
                    }
                } else {
                    warn!(
                        "No meta field present for the chemistry so can't check if barcodes should be reverse complemented."
                    );
                }
            }
        } else {
            warn!(
                "Couldn't find expected chemistry registry {} so can't check if barcodes should be reverse complemented.",
                custom_chem_p.display()
            );
        }
        pbco
    };

    let map_file = opts.output.join("af_map");
    let mut af_gpl = std::process::Command::new(format!("{}", &af_prog_info.exe_path.display()));
    af_gpl
        .arg("atac")
        .arg("generate-permit-list")
        .arg("--input")
        .arg(map_file)
        .arg("--permit-bc-ori")
        .arg(permit_bc_ori);

    let out_dir = opts.output.join("af_process");
    af_gpl.arg("--output-dir").arg(out_dir);

    if let Some(fm) = filter_meth_opt {
        match fm {
            af_utils::CellFilterMethod::UnfilteredExternalList(p, _mc) => {
                af_gpl.arg("-u").arg(p);
                af_gpl.arg("--min-reads").arg(format!("{}", opts.min_reads));
            }
            _ => bail!("unsupported filter method in atac-seq process."),
        }
    } else {
        bail!("unsupported filter method in atac-seq process.");
    }

    // if the user requested more threads than can be used
    let (threads, capped_at) = runtime::cap_threads(opts.threads);
    if let Some(max_threads) = capped_at {
        warn!(
            "The maximum available parallelism is {}, but {} threads were requested.",
            max_threads, opts.threads
        );
        warn!("setting number of threads to {}", max_threads);
    }
    af_gpl.arg("--threads").arg(format!("{}", threads));

    let gpl_cmd_string = prog_utils::get_cmd_line_string(&af_gpl);
    info!("gpl command : {}", gpl_cmd_string);

    let af_gpl_start = Instant::now();
    exec::run_checked(&mut af_gpl, "[atac::af_gpl]")?;
    let af_gpl_duration = af_gpl_start.elapsed();
    info!(
        "permit list generation completed successfully in {:#?}",
        af_gpl_duration
    );

    let af_process_info_file = opts.output.join("simpleaf_process_log.json");
    let json_file = std::fs::File::open(af_process_info_file.clone())
        .with_context(|| format!("couldn't open file {}", af_process_info_file.display()))?;
    let json_reader = BufReader::new(json_file);
    let mut af_process_info: serde_json::Value = serde_json::from_reader(json_reader)
        .with_context(|| {
            format!(
                "couldn't parse JSON content from {}",
                af_process_info_file.display()
            )
        })?;

    af_process_info["time_info"]["gpl_time"] = json!(af_gpl_duration.as_secs_f64());
    af_process_info["cmd_info"]["gpl_cmd"] = json!(gpl_cmd_string);

    // write the relevant info about
    // our run to file.
    io::write_json_pretty_atomic(&af_process_info_file, &af_process_info)?;

    info!("successfully performed cell barcode detection and correction.");
    Ok(GplStageOutput {
        gpl_duration_secs: af_gpl_duration.as_secs_f64(),
        gpl_cmd: gpl_cmd_string,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::atac::commands::{AtacChemistry, Macs3GenomeSize};

    use super::*;

    fn base_process_opts() -> ProcessOpts {
        ProcessOpts {
            index: PathBuf::from("/tmp/index"),
            reads1: None,
            reads2: None,
            reads: None,
            barcode_reads: vec![],
            chemistry: AtacChemistry::TenxV2,
            barcode_length: 16,
            output: PathBuf::from("/tmp/out"),
            threads: 1,
            call_peaks: false,
            permit_barcode_ori: None,
            unfiltered_pl: None,
            min_reads: 10,
            compress: false,
            ignore_ambig_hits: false,
            no_poison: false,
            use_chr: false,
            thr: 0.8,
            bin_size: 50,
            bin_overlap: 2,
            no_tn5_shift: false,
            check_kmer_orphan: false,
            max_ec_card: 4096,
            max_hit_occ: 64,
            max_hit_occ_recover: 1024,
            max_read_occ: 250,
            gsize: Macs3GenomeSize::KnownOpt("hs"),
            qvalue: 0.1,
            extsize: 50,
        }
    }

    #[test]
    fn add_read_args_succeeds_for_paired_end_inputs() {
        let td = tempfile::tempdir().expect("failed to create tempdir");
        let r1 = td.path().join("r1.fastq");
        let r2 = td.path().join("r2.fastq");
        let bc = td.path().join("bc.fastq");
        fs::write(&r1, "").expect("failed to write r1");
        fs::write(&r2, "").expect("failed to write r2");
        fs::write(&bc, "").expect("failed to write bc");

        let mut opts = base_process_opts();
        opts.reads1 = Some(vec![r1]);
        opts.reads2 = Some(vec![r2]);
        opts.barcode_reads = vec![bc];

        let mut cmd = std::process::Command::new("echo");
        add_read_args(&mut cmd, &opts).expect("expected add_read_args to succeed");
    }

    #[test]
    fn add_read_args_fails_for_mismatched_read_counts() {
        let td = tempfile::tempdir().expect("failed to create tempdir");
        let r1 = td.path().join("r1.fastq");
        let r2a = td.path().join("r2a.fastq");
        let r2b = td.path().join("r2b.fastq");
        let bc = td.path().join("bc.fastq");
        fs::write(&r1, "").expect("failed to write r1");
        fs::write(&r2a, "").expect("failed to write r2a");
        fs::write(&r2b, "").expect("failed to write r2b");
        fs::write(&bc, "").expect("failed to write bc");

        let mut opts = base_process_opts();
        opts.reads1 = Some(vec![r1]);
        opts.reads2 = Some(vec![r2a, r2b]);
        opts.barcode_reads = vec![bc];

        let mut cmd = std::process::Command::new("echo");
        let err = add_read_args(&mut cmd, &opts).expect_err("expected mismatch to fail");
        assert!(
            format!("{:#}", err).contains("Cannot proceed"),
            "unexpected error: {:#}",
            err
        );
    }
}
