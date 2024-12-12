use crate::atac::commands::ProcessOpts;
use crate::utils::{prog_utils, prog_utils::ReqProgs};
use anyhow;
use anyhow::{bail, Context};
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

fn push_advanced_piscem_options(
    piscem_quant_cmd: &mut std::process::Command,
    opts: &ProcessOpts,
) -> anyhow::Result<()> {
    if opts.ignore_ambig_hits {
        piscem_quant_cmd.arg("--ignore-ambig-hits");
    } else {
        piscem_quant_cmd
            .arg("--max-ec-card")
            .arg(format!("{}", opts.max_ec_card));
    }

    if opts.no_poison {
        piscem_quant_cmd.arg("--no-poison");
    }

    piscem_quant_cmd
        .arg("--max-hit-occ")
        .arg(format!("{}", opts.max_hit_occ));

    piscem_quant_cmd
        .arg("--max-hit-occ-recover")
        .arg(format!("{}", opts.max_hit_occ_recover));

    piscem_quant_cmd
        .arg("--max-read-occ")
        .arg(format!("{}", opts.max_read_occ));

    Ok(())
}

fn add_read_args(map_cmd: &mut std::process::Command, opts: &ProcessOpts) -> anyhow::Result<()> {
    if let Some(ref reads1) = opts.reads1 {
        let reads2 = opts
            .reads2
            .as_ref()
            .expect("since reads1 files is given, read2 files must be provided.");
        let barcode_reads: &Vec<PathBuf> = &opts.barcode_reads;
        if reads1.len() != reads2.len() || reads1.len() != barcode_reads.len() {
            bail!(
                "{} read1 files, {} read2 files, and {} barcode read files were given; Cannot proceed!",
                reads1.len(),
                reads2.len(),
                barcode_reads.len()
            );
        }
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
        let reads = opts.reads.as_ref().expect(
            "since reads1 and reads2 are not provided, the single-end reads must be provided.",
        );
        let barcode_reads: &Vec<PathBuf> = &opts.barcode_reads;
        if reads.len() != barcode_reads.len() {
            bail!(
                "{} read files and {} barcode read files were given; Cannot proceed!",
                reads.len(),
                barcode_reads.len()
            );
        }
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
pub(crate) fn map_reads(af_home_path: &Path, opts: &ProcessOpts) -> anyhow::Result<()> {
    /*
                    map-sc-atac \
                --index {params.ind_pref} \
                --read1 {params.read1} \
                --read2 {params.read2} \
                --barcode {params.barcode} \
                --output {params.out_dir} \
                --thr {params.thr} \
                --bin-size {params.bin_size} \
                {params.orp} \
                --threads {params.threads}
    */
    // Read the JSON contents of the file as an instance of `User`.
    let v: Value = prog_utils::inspect_af_home(af_home_path)?;
    let rp: ReqProgs = serde_json::from_value(v["prog_info"].clone())?;

    let piscem_prog_info = rp
        .piscem
        .as_ref()
        .expect("piscem program info should be properly set.");

    // figure out what type of index we expect
    let index_base;

    let mut index = opts.index.clone();
    // If the user built the index using simpleaf, there are
    // 2 possibilities here:
    //  1. They are passing in the directory containing the index
    //  2. They are passing in the prefix stem of the index files
    // The code below is to check, in both cases, if we can automatically
    // detect if the index was constructed with simpleaf.

    // If we are in case 1., the passed in path is a directory and
    // we can check for the simpleaf_index.json file directly,
    // Otherwise if the path is not a directory, we check if it
    // ends in piscem_idx (the suffix that simpleaf uses when
    // making a piscem index). Then we test the directory we
    // get after stripping off this suffix.
    let removed_piscem_idx_suffix = if !index.is_dir() && index.ends_with("piscem_idx") {
        // remove the piscem_idx part
        index.pop();
        true
    } else {
        false
    };

    let index_json_path = index.join("simpleaf_index.json");
    match index_json_path.try_exists() {
        Ok(true) => {
            // we have the simpleaf_index.json file, so parse it.
            let index_json_file = std::fs::File::open(&index_json_path).with_context({
                || format!("Could not open file {}", index_json_path.display())
            })?;

            let index_json_reader = BufReader::new(&index_json_file);
            let v: Value = serde_json::from_reader(index_json_reader)?;

            let index_type_str: String = serde_json::from_value(v["index_type"].clone())?;

            // here, set the index type based on what we found as the
            // value for the `index_type` key.
            match index_type_str.as_ref() {
                "piscem" => {
                    // here, either the user has provided us with just
                    // the directory containing the piscem index, or
                    // we have "popped" off the "piscem_idx" suffix, so
                    // add it (back).
                    index_base = index.join("piscem_idx");
                }
                _ => {
                    bail!(
                        "unknown index type {} present in simpleaf_index.json",
                        index_type_str,
                    );
                }
            }
        }
        Ok(false) => {
            // at this point, we have inferred that simpleaf wasn't
            // used to construct the index, so fall back to what the user
            // requested directly.
            // if we have previously removed the piscem_idx suffix, add it back
            if removed_piscem_idx_suffix {
                index.push("piscem_idx");
            }
            index_base = index;
        }
        Err(e) => {
            bail!(e);
        }
    }

    // using a piscem index
    let mut piscem_map_cmd =
        std::process::Command::new(format!("{}", &piscem_prog_info.exe_path.display()));
    let index_path = format!("{}", index_base.display());
    piscem_map_cmd
        .arg("map-sc-atac")
        .arg("--index")
        .arg(index_path);

    // if the user requested more threads than can be used
    let mut threads = opts.threads;
    if let Ok(max_threads_usize) = std::thread::available_parallelism() {
        let max_threads = max_threads_usize.get() as u32;
        if threads > max_threads {
            warn!(
                "The maximum available parallelism is {}, but {} threads were requested.",
                max_threads, threads
            );
            warn!("setting number of threads to {}", max_threads);
            threads = max_threads;
        }
    }

    // location of output directory, number of threads
    let map_output = opts.output.join("af_map");
    piscem_map_cmd
        .arg("--threads")
        .arg(format!("{}", threads))
        .arg("-o")
        .arg(&map_output);

    // add either the paired-end or single-end read arguments
    add_read_args(&mut piscem_map_cmd, &opts)?;

    // if the user is requesting a mapping option that required
    // piscem version >= 0.7.0, ensure we have that
    if let Ok(_piscem_ver) = prog_utils::check_version_constraints(
        "piscem",
        ">=0.11.0, <1.0.0",
        &piscem_prog_info.version,
    ) {
        push_advanced_piscem_options(&mut piscem_map_cmd, &opts)?;
    } else {
        info!(
            r#"
Simpleaf is currently using piscem version {}, but you must be using version >= 0.11.0 in order to use the 
mapping options specific to this, or later versions. If you wish to use these options, please upgrade your 
piscem version or, if you believe you have a sufficiently new version installed, update the executable 
being used by simpleaf"#,
            &piscem_prog_info.version
        );
    }
    Ok(())
}
