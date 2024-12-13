use crate::atac::commands::IndexOpts;
use crate::utils::{
    prog_utils,
    prog_utils::{CommandVerbosityLevel, ReqProgs},
};
use anyhow;
use anyhow::{bail, Context};
use cmd_lib::run_fun;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Instant;
use tracing::{info, warn};

pub(crate) fn piscem_index(af_home_path: &Path, opts: &IndexOpts) -> anyhow::Result<()> {
    // Read the JSON contents of the file as an instance of `User`.
    let v: Value = prog_utils::inspect_af_home(af_home_path)?;
    let rp: ReqProgs = serde_json::from_value(v["prog_info"].clone())?;

    let piscem_prog_info = rp
        .piscem
        .as_ref()
        .expect("piscem program info should be properly set.");

    match prog_utils::check_version_constraints(
        "piscem",
        ">=0.11.0, <1.0.0",
        &piscem_prog_info.version,
    ) {
        Ok(af_ver) => info!("found piscem version {:#}, proceeding", af_ver),
        Err(e) => return Err(e),
    }

    let output = opts.output.clone();
    let output_index_dir = output.join("index");

    let mut piscem_index_cmd =
        std::process::Command::new(format!("{}", piscem_prog_info.exe_path.display()));

    run_fun!(mkdir -p $output_index_dir)?;
    let output_index_stem = output_index_dir.join("piscem_idx");

    piscem_index_cmd
        .arg("build")
        .arg("-k")
        .arg(opts.kmer_length.to_string())
        .arg("-m")
        .arg(opts.minimizer_length.to_string())
        .arg("-o")
        .arg(&output_index_stem)
        .arg("-s")
        .arg(&opts.input)
        .arg("--seed")
        .arg(opts.hash_seed.to_string())
        .arg("-w")
        .arg(&opts.work_dir);

    // if the user requested to overwrite, then pass this option
    if opts.overwrite {
        info!("will attempt to overwrite any existing piscem index, as requested");
        piscem_index_cmd.arg("--overwrite");
    }

    let mut threads = opts.threads;
    // if the user requested more threads than can be used
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

    piscem_index_cmd
        .arg("--threads")
        .arg(format!("{}", threads));

    // if the user is requesting a poison k-mer table, ensure the
    // piscem version is at least 0.7.0
    if let Some(ref decoy_paths) = opts.decoy_paths {
        let path_args = decoy_paths
            .iter()
            .map(|x| x.to_string_lossy().into_owned())
            .collect::<Vec<String>>()
            .join(",");
        piscem_index_cmd.arg("--decoy-paths").arg(path_args);
    }

    // print piscem build command
    let index_cmd_string = prog_utils::get_cmd_line_string(&piscem_index_cmd);
    info!("piscem build cmd : {}", index_cmd_string);

    let index_start = Instant::now();
    let cres = prog_utils::execute_command(&mut piscem_index_cmd, CommandVerbosityLevel::Quiet)
        .expect("failed to invoke piscem index command");
    let index_duration = index_start.elapsed();

    if !cres.status.success() {
        bail!("piscem index failed to build succesfully {:?}", cres.status);
    }

    let index_json_file = output_index_dir.join("simpleaf_index.json");
    let index_json = json!({
            "cmd" : index_cmd_string,
            "index_type" : "piscem",
            "time_info" : {
                "index_time" : index_duration.as_secs_f64()
            },
            "piscem_index_parameters" : {
                "k" : opts.kmer_length,
                "m" : opts.minimizer_length,
                "overwrite" : opts.overwrite,
                "threads" : threads,
                "ref" : &opts.input
            }
    });
    std::fs::write(
        &index_json_file,
        serde_json::to_string_pretty(&index_json).unwrap(),
    )
    .with_context(|| format!("could not write {}", index_json_file.display()))?;

    Ok(())
}
