use tracing::{info, warn};
use tracing_subscriber::{filter::LevelFilter, fmt, prelude::*, EnvFilter};

use anyhow::{bail, Context};
use clap::Parser;
use cmd_lib::{run_cmd, run_fun};
use serde_json::json;

use time::{Duration, Instant};

use std::io::BufReader;
// use std::io::{Seek, SeekFrom};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::{env, fs};

mod utils;
use utils::af_utils::*;
use utils::prog_utils::*;
use utils::workflow_utils::*;

use crate::utils::prog_utils;

mod simpleaf_commands;
use simpleaf_commands::*;

/// simplifying alevin-fry workflows
#[derive(Debug, Parser)]
#[command(author, version, about)]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

fn set_paths(af_home_path: PathBuf, set_path_args: Commands) -> anyhow::Result<()> {
    const AF_HOME: &str = "ALEVIN_FRY_HOME";
    match set_path_args {
        Commands::SetPaths {
            salmon,
            piscem,
            alevin_fry,
            pyroe,
        } => {
            // create AF_HOME if needed
            if !af_home_path.as_path().is_dir() {
                info!(
                    "The {} directory, {}, doesn't exist, creating...",
                    AF_HOME,
                    af_home_path.display()
                );
                fs::create_dir_all(af_home_path.as_path())?;
            }

            let rp = get_required_progs_from_paths(salmon, piscem, alevin_fry, pyroe)?;

            let have_mapper = rp.salmon.is_some() || rp.piscem.is_some();
            if !have_mapper {
                bail!("Suitable executable for piscem or salmon not found — at least one of these must be available.");
            }
            if rp.alevin_fry.is_none() {
                bail!("Suitable alevin_fry executable not found.");
            }
            if rp.pyroe.is_none() {
                bail!("Suitable pyroe executable not found.");
            }

            let simpleaf_info_file = af_home_path.join("simpleaf_info.json");
            let simpleaf_info = json!({ "prog_info": rp });

            std::fs::write(
                &simpleaf_info_file,
                serde_json::to_string_pretty(&simpleaf_info).unwrap(),
            )
            .with_context(|| format!("could not write {}", simpleaf_info_file.display()))?;
        }
        _ => {
            bail!("unexpected command")
        }
    }
    Ok(())
}

fn build_ref_and_index(af_home_path: &Path, index_args: Commands) -> anyhow::Result<()> {
    match index_args {
        // if we are building the reference and indexing
        Commands::Index {
            ref_type,
            fasta,
            gtf,
            rlen,
            spliced,
            unspliced,
            dedup,
            keep_duplicates,
            ref_seq,
            output,
            use_piscem,
            kmer_length,
            minimizer_length,
            overwrite,
            sparse,
            mut threads,
        } => {
            let v: Value = inspect_af_home(af_home_path)?;
            // Read the JSON contents of the file as an instance of `User`.
            let rp: ReqProgs = serde_json::from_value(v["prog_info"].clone())?;

            // we are building a custom reference
            if fasta.is_some() {
                // make sure that the spliced+unspliced reference
                // is supported if that's what's being requested.
                match ref_type {
                    ReferenceType::SplicedUnspliced => {
                        let v = rp.pyroe.clone().unwrap().version;
                        if let Err(e) =
                            prog_utils::check_version_constraints("pyroe", ">=0.8.1, <1.0.0", &v)
                        {
                            bail!(e);
                        }
                    }
                    ReferenceType::SplicedIntronic => {
                        // in this branch we are making a spliced+intronic (splici) index, so
                        // the user must have specified the read length.
                        if rlen.is_none() {
                            bail!(format!("A spliced+intronic reference was requested, but no read length argument (--rlen) was provided."));
                        }
                    }
                }
            }

            let info_file = output.join("index_info.json");
            let mut index_info = json!({
                "command" : "index",
                "version_info" : rp,
                "args" : {
                    "output" : output,
                    "overwrite" : overwrite,
                    "keep_duplicates" : keep_duplicates,
                    "sparse" : sparse,
                    "threads" : threads,
                }
            });

            run_fun!(mkdir -p $output)?;

            // wow, the compiler is smart enough to
            // figure out that this one need not be
            // mutable because it is set once in either
            // branch of the conditional below.
            let reference_sequence;
            // these may or may not be set, so must be
            // mutable.
            let mut splici_t2g = None;
            let mut pyroe_duration = None;
            let pyroe_cmd_string: String;

            // if we are generating a splici reference
            if let (Some(fasta), Some(gtf)) = (fasta, gtf) {
                let mut input_files = vec![fasta.clone(), gtf.clone()];

                let outref = output.join("ref");
                run_fun!(mkdir -p $outref)?;

                let read_len;
                let ref_file;
                let t2g_file;

                match ref_type {
                    ReferenceType::SplicedIntronic => {
                        read_len = rlen.unwrap();
                        ref_file = format!("splici_fl{}.fa", read_len - 5);
                        t2g_file = outref.join(format!("splici_fl{}_t2g_3col.tsv", read_len - 5));
                    }
                    ReferenceType::SplicedUnspliced => {
                        read_len = 0;
                        ref_file = String::from("spliceu.fa");
                        t2g_file = outref.join("spliceu_t2g_3col.tsv");
                    }
                }

                index_info["t2g_file"] = json!(&t2g_file);
                index_info["args"]["fasta"] = json!(&fasta);
                index_info["args"]["gtf"] = json!(&gtf);
                index_info["args"]["spliced"] = json!(&spliced);
                index_info["args"]["unspliced"] = json!(&unspliced);
                index_info["args"]["dedup"] = json!(dedup);

                std::fs::write(
                    &info_file,
                    serde_json::to_string_pretty(&index_info).unwrap(),
                )
                .with_context(|| format!("could not write {}", info_file.display()))?;

                // set the splici_t2g option
                splici_t2g = Some(t2g_file);

                let mut pyroe_cmd =
                    std::process::Command::new(format!("{}", rp.pyroe.unwrap().exe_path.display()));
                // select the command to run
                match ref_type {
                    ReferenceType::SplicedIntronic => {
                        pyroe_cmd.arg("make-splici");
                    }
                    ReferenceType::SplicedUnspliced => {
                        pyroe_cmd.arg("make-spliceu");
                    }
                };

                // if the user wants to dedup output sequences
                if dedup {
                    pyroe_cmd.arg(String::from("--dedup-seqs"));
                }

                // extra spliced sequence
                if let Some(es) = spliced {
                    pyroe_cmd.arg(String::from("--extra-spliced"));
                    pyroe_cmd.arg(format!("{}", es.display()));
                    input_files.push(es);
                }

                // extra unspliced sequence
                if let Some(eu) = unspliced {
                    pyroe_cmd.arg(String::from("--extra-unspliced"));
                    pyroe_cmd.arg(format!("{}", eu.display()));
                    input_files.push(eu);
                }

                pyroe_cmd.arg(fasta).arg(gtf);

                // if making splici the second positional argument is the
                // read length.
                if let ReferenceType::SplicedIntronic = ref_type {
                    pyroe_cmd.arg(format!("{}", read_len));
                };

                // the output directory
                pyroe_cmd.arg(&outref);

                check_files_exist(&input_files)?;

                // print pyroe command
                pyroe_cmd_string = get_cmd_line_string(&pyroe_cmd);
                info!("pyroe cmd : {}", pyroe_cmd_string);

                let pyroe_start = Instant::now();
                let cres =
                    prog_utils::execute_command(&mut pyroe_cmd, CommandVerbosityLevel::Verbose)
                        .expect(
                            "could not execute pyroe (for generating reference transcriptome).",
                        );
                pyroe_duration = Some(pyroe_start.elapsed());

                if !cres.status.success() {
                    bail!("pyroe failed to return succesfully {:?}", cres.status);
                }

                reference_sequence = Some(outref.join(ref_file));
            } else {
                // we are running on a set of references directly

                // in this path (due to the argument parser requiring
                // either --fasta or --ref-seq, ref-seq should be safe to
                // unwrap).
                index_info["args"]["ref-seq"] = json!(ref_seq.clone().unwrap());

                std::fs::write(
                    &info_file,
                    serde_json::to_string_pretty(&index_info).unwrap(),
                )
                .with_context(|| format!("could not write {}", info_file.display()))?;

                pyroe_cmd_string = String::from("");
                reference_sequence = ref_seq;
            }

            let ref_seq = reference_sequence.expect(
                "reference sequence should either be generated from --fasta by make-splici or set with --ref-seq",
            );

            let input_files = vec![ref_seq.clone()];
            check_files_exist(&input_files)?;

            let output_index_dir = output.join("index");
            let index_duration;
            let index_cmd_string: String;

            if use_piscem {
                // ensure we have piscem
                if rp.piscem.is_none() {
                    bail!("The construction of a piscem index was requested, but a valid piscem executable was not available. \n\
                            Please either set a path using the `set-paths` command, or ensure the `PISCEM` environment variable is set properly.");
                }

                let mut piscem_index_cmd = std::process::Command::new(format!(
                    "{}",
                    rp.piscem.unwrap().exe_path.display()
                ));

                run_fun!(mkdir -p $output_index_dir)?;
                let output_index_stem = output_index_dir.join("piscem_idx");

                piscem_index_cmd
                    .arg("build")
                    .arg("-k")
                    .arg(kmer_length.to_string())
                    .arg("-m")
                    .arg(minimizer_length.to_string())
                    .arg("-o")
                    .arg(&output_index_stem)
                    .arg("-s")
                    .arg(&ref_seq);

                // if the user requested to overwrite, then pass this option
                if overwrite {
                    info!("will attempt to overwrite any existing piscem index, as requested");
                    piscem_index_cmd.arg("--overwrite");
                }

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

                // print piscem build command
                index_cmd_string = get_cmd_line_string(&piscem_index_cmd);
                info!("piscem build cmd : {}", index_cmd_string);

                let index_start = Instant::now();
                let cres = prog_utils::execute_command(
                    &mut piscem_index_cmd,
                    CommandVerbosityLevel::Quiet,
                )
                .expect("failed to invoke piscem index command");
                index_duration = index_start.elapsed();

                if !cres.status.success() {
                    bail!("piscem index failed to build succesfully {:?}", cres.status);
                }

                // copy over the t2g file to the index
                let mut t2g_out_path: Option<PathBuf> = None;
                if let Some(t2g_file) = splici_t2g {
                    let index_t2g_path = output_index_dir.join("t2g_3col.tsv");
                    t2g_out_path = Some(PathBuf::from("t2g_3col.tsv"));
                    std::fs::copy(t2g_file, index_t2g_path)?;
                }

                let index_json_file = output_index_dir.join("simpleaf_index.json");
                let index_json = json!({
                        "cmd" : index_cmd_string,                        "index_type" : "piscem",
                        "t2g_file" : t2g_out_path,
                        "piscem_index_parameters" : {
                            "k" : kmer_length,
                            "m" : minimizer_length,
                            "overwrite" : overwrite,
                            "threads" : threads,
                            "ref" : ref_seq
                        }
                });
                std::fs::write(
                    &index_json_file,
                    serde_json::to_string_pretty(&index_json).unwrap(),
                )
                .with_context(|| format!("could not write {}", index_json_file.display()))?;
            } else {
                // ensure we have piscem
                if rp.salmon.is_none() {
                    bail!("The construction of a salmon index was requested, but a valid piscem executable was not available. \n\
                            Please either set a path using the `simpleaf set-paths` command, or ensure the `SALMON` environment variable is set properly.");
                }

                let mut salmon_index_cmd = std::process::Command::new(format!(
                    "{}",
                    rp.salmon.unwrap().exe_path.display()
                ));

                salmon_index_cmd
                    .arg("index")
                    .arg("-k")
                    .arg(kmer_length.to_string())
                    .arg("-i")
                    .arg(&output_index_dir)
                    .arg("-t")
                    .arg(&ref_seq);

                // overwrite doesn't do anything special for the salmon index, so mention this to
                // the user.
                if overwrite {
                    info!("As the default salmon behavior is to overwrite an existing index if the same directory is provided, \n\
                        the --overwrite flag will have no additional effect.");
                }

                // if the user requested a sparse index.
                if sparse {
                    salmon_index_cmd.arg("--sparse");
                }

                // if the user requested keeping duplicated sequences.
                if keep_duplicates {
                    salmon_index_cmd.arg("--keepDuplicates");
                }

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

                salmon_index_cmd
                    .arg("--threads")
                    .arg(format!("{}", threads));

                // print salmon index command
                index_cmd_string = get_cmd_line_string(&salmon_index_cmd);
                info!("salmon index cmd : {}", index_cmd_string);

                let index_start = Instant::now();
                let cres = prog_utils::execute_command(
                    &mut salmon_index_cmd,
                    CommandVerbosityLevel::Quiet,
                )
                .expect("failed to invoke salmon index command");
                index_duration = index_start.elapsed();

                if !cres.status.success() {
                    bail!("salmon index failed to build succesfully {:?}", cres.status);
                }

                // copy over the t2g file to the index
                let mut t2g_out_path: Option<PathBuf> = None;
                if let Some(t2g_file) = splici_t2g {
                    let index_t2g_path = output_index_dir.join("t2g_3col.tsv");
                    t2g_out_path = Some(PathBuf::from("t2g_3col.tsv"));
                    std::fs::copy(t2g_file, index_t2g_path)?;
                }

                let index_json_file = output_index_dir.join("simpleaf_index.json");
                let index_json = json!({
                    "cmd" : index_cmd_string,                        "index_type" : "salmon",
                        "t2g_file" : t2g_out_path,
                        "salmon_index_parameters" : {
                            "k" : kmer_length,
                            "overwrite" : overwrite,
                            "sparse" : sparse,
                            "keep_duplicates" : keep_duplicates,
                            "threads" : threads,
                            "ref" : ref_seq
                        }
                });
                std::fs::write(
                    &index_json_file,
                    serde_json::to_string_pretty(&index_json).unwrap(),
                )
                .with_context(|| format!("could not write {}", index_json_file.display()))?;
            }

            let index_log_file = output.join("simpleaf_index_log.json");
            let index_log_info = if let Some(pyroe_duration) = pyroe_duration {
                // if we ran make-splici
                json!({
                    "time_info" : {
                        "pyroe_time" : pyroe_duration,
                        "index_time" : index_duration
                    },
                    "cmd_info" : {
                        "pyroe_cmd" : pyroe_cmd_string,
                        "index_cmd" : index_cmd_string,                    }
                })
            } else {
                // if we indexed provided sequences directly
                json!({
                    "time_info" : {
                        "index_time" : index_duration
                    },
                    "cmd_info" : {
                        "index_cmd" : index_cmd_string,                    }
                })
            };

            std::fs::write(
                &index_log_file,
                serde_json::to_string_pretty(&index_log_info).unwrap(),
            )
            .with_context(|| format!("could not write {}", index_log_file.display()))?;
        }
        _ => {
            bail!("invalid command");
        }
    }
    Ok(())
}



fn map_and_quant(af_home_path: &Path, quant_cmd: Commands) -> anyhow::Result<()> {
    match quant_cmd {
        Commands::Quant {
            index,
            use_piscem,
            map_dir,
            reads1,
            reads2,
            mut threads,
            use_selective_alignment,
            expected_ori,
            knee,
            unfiltered_pl,
            explicit_pl,
            forced_cells,
            expect_cells,
            min_reads,
            resolution,
            mut t2g_map,
            chemistry,
            output,
        } => {
            // Read the JSON contents of the file as an instance of `User`.
            let v: Value = inspect_af_home(af_home_path)?;
            let rp: ReqProgs = serde_json::from_value(v["prog_info"].clone())?;

            // info!("prog info = {:?}", rp);

            let mut had_simpleaf_index_json = false;
            let mut index_type_str = String::new();
            if let Some(index) = index.clone() {
                let index_json_path = index.join("simpleaf_index.json");
                match index_json_path.try_exists() {
                    Ok(true) => {
                        // we have the simpleaf_index.json file, so parse it.
                        let index_json_file =
                            std::fs::File::open(&index_json_path).with_context({
                                || format!("Could not open file {}", index_json_path.display())
                            })?;

                        let index_json_reader = BufReader::new(&index_json_file);
                        let v: Value = serde_json::from_reader(index_json_reader)?;
                        had_simpleaf_index_json = true;
                        index_type_str = serde_json::from_value(v["index_type"].clone())?;
                        // if the user didn't pass in a t2g_map, try and populate it
                        // automatically here
                        if t2g_map.is_none() {
                            let t2g_opt: Option<PathBuf> =
                                serde_json::from_value(v["t2g_file"].clone())?;
                            if let Some(t2g_val) = t2g_opt {
                                let t2g_loc = index.join(t2g_val);
                                info!("found local t2g file at {}, will attempt to use this since none was provided explicitly", t2g_loc.display());
                                t2g_map = Some(t2g_loc);
                            }
                        }
                    }
                    Ok(false) => {
                        had_simpleaf_index_json = false;
                    }
                    Err(e) => {
                        bail!(e);
                    }
                }
            }

            // at this point make sure we have a t2g value
            let t2g_map_file = t2g_map.context("A transcript-to-gene map (t2g) file was not provided via `--t2g-map`|`-m` and could \
                    not be inferred from the index. Please provide a t2g map explicitly to the quant command.")?;
            check_files_exist(&[t2g_map_file.clone()])?;

            // figure out what type of index we expect
            let index_type;
            // only bother with this if we are mapping reads and not if we are
            // starting from a RAD file
            if let Some(index) = index.clone() {
                // if the user said piscem explicitly, believe them
                if !use_piscem {
                    if had_simpleaf_index_json {
                        match index_type_str.as_ref() {
                            "salmon" => {
                                index_type = IndexType::Salmon(index);
                            }
                            "piscem" => {
                                index_type = IndexType::Piscem(index.join("piscem_idx"));
                            }
                            _ => {
                                bail!(
                                    "unknown index type {} present in simpleaf_index.json",
                                    index_type_str,
                                );
                            }
                        }
                    } else {
                        index_type = IndexType::Salmon(index);
                    }
                } else {
                    index_type = IndexType::Piscem(index);
                }
            } else {
                index_type = IndexType::NoIndex;
            }

            // make sure we have an program matching the
            // appropriate index type
            match index_type {
                IndexType::Piscem(_) => {
                    if rp.piscem.is_none() {
                        bail!("A piscem index is being used, but no piscem executable is provided. Please set one with `simpleaf set-paths`.");
                    }
                }
                IndexType::Salmon(_) => {
                    if rp.salmon.is_none() {
                        bail!("A salmon index is being used, but no piscem executable is provided. Please set one with `simpleaf set-paths`.");
                    }
                }
                IndexType::NoIndex => {}
            }

            // do we have a custom chemistry file
            let custom_chem_p = af_home_path.join("custom_chemistries.json");
            let custom_chem_exists = custom_chem_p.is_file();

            let chem = match chemistry.as_str() {
                "10xv2" => Chemistry::TenxV2,
                "10xv3" => Chemistry::TenxV3,
                s => {
                    if custom_chem_exists {
                        // parse the custom chemistry json file
                        let custom_chem_file =
                            std::fs::File::open(&custom_chem_p).with_context({
                                || {
                                    format!(
                                        "couldn't open the custom chemistry file {}",
                                        custom_chem_p.display()
                                    )
                                }
                            })?;
                        let custom_chem_reader = BufReader::new(custom_chem_file);
                        let v: Value = serde_json::from_reader(custom_chem_reader)?;
                        let rchem = match v[s.to_string()].as_str() {
                            Some(chem_str) => {
                                info!("custom chemistry {} maps to geometry {}", s, &chem_str);
                                Chemistry::Other(chem_str.to_string())
                            }
                            None => Chemistry::Other(s.to_string()),
                        };
                        rchem
                    } else {
                        // pass along whatever the user gave us
                        Chemistry::Other(s.to_string())
                    }
                }
            };

            let ori;
            // if the user set the orientation, then
            // use that explicitly
            if let Some(o) = expected_ori {
                ori = o;
            } else {
                // otherwise, this was not set explicitly. In that case
                // if we have 10xv2 or 10xv3 chemistry, set ori = "fw"
                // otherwise set ori = "both"
                match chem {
                    Chemistry::TenxV2 | Chemistry::TenxV3 => {
                        ori = "fw".to_string();
                    }
                    _ => {
                        ori = "both".to_string();
                    }
                }
            }

            let mut filter_meth_opt = None;

            // based on the filtering method
            if let Some(pl_file) = unfiltered_pl {
                // NOTE: unfiltered_pl is of type Option<Option<PathBuf>> so being in here
                // tells us nothing about the inner option.  We handle that now.

                // if the -u flag is passed and some file is provided, then the inner
                // Option is Some(PathBuf)
                if let Some(pl_file) = pl_file {
                    // the user has explicily passed a file along, so try
                    // to use that
                    if pl_file.is_file() {
                        let min_cells = min_reads;
                        filter_meth_opt = Some(CellFilterMethod::UnfilteredExternalList(
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
                    // using 10xv2 or 10xv3

                    // check the chemistry
                    let pl_res = get_permit_if_absent(af_home_path, &chem)?;
                    let min_cells = min_reads;
                    match pl_res {
                        PermitListResult::DownloadSuccessful(p)
                        | PermitListResult::AlreadyPresent(p) => {
                            filter_meth_opt = Some(CellFilterMethod::UnfilteredExternalList(
                                p.to_string_lossy().into_owned(),
                                min_cells,
                            ));
                        }
                        PermitListResult::UnregisteredChemistry => {
                            bail!(
                                    "Cannot automatically obtain an unfiltered permit list for non-Chromium chemistry : {}.",
                                    chem.as_str()
                                    );
                        }
                    }
                }
            } else {
                if let Some(filtered_path) = explicit_pl {
                    filter_meth_opt = Some(CellFilterMethod::ExplicitList(
                        filtered_path.to_string_lossy().into_owned(),
                    ));
                };
                if let Some(num_forced) = forced_cells {
                    filter_meth_opt = Some(CellFilterMethod::ForceCells(num_forced));
                };
                if let Some(num_expected) = expect_cells {
                    filter_meth_opt = Some(CellFilterMethod::ExpectCells(num_expected));
                };
            }
            // otherwise it must have been knee;
            if knee {
                filter_meth_opt = Some(CellFilterMethod::KneeFinding);
            }

            if filter_meth_opt.is_none() {
                bail!("No valid filtering strategy was provided!");
            }

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

            // here we must be safe to unwrap
            let filter_meth = filter_meth_opt.unwrap();

            let sc_mapper: String;
            let map_cmd_string: String;
            let map_output: PathBuf;
            let map_duration: Duration;

            // if we are mapping against an index
            if let Some(index) = index {
                let reads1 = reads1.expect(
                    "since mapping against an index is requested, read1 files must be provided.",
                );
                let reads2 = reads2.expect(
                    "since mapping against an index is requested, read2 files must be provided.",
                );
                assert_eq!(
                    reads1.len(),
                    reads2.len(),
                    "{} read1 files and {} read2 files were given; Cannot proceed!",
                    reads1.len(),
                    reads2.len()
                );

                match index_type {
                    IndexType::Piscem(index_base) => {
                        // using a piscem index
                        let mut piscem_quant_cmd = std::process::Command::new(format!(
                            "{}",
                            rp.piscem.unwrap().exe_path.display()
                        ));
                        let index_path = format!("{}", index_base.display());
                        piscem_quant_cmd
                            .arg("map-sc")
                            .arg("--index")
                            .arg(index_path);

                        // location of output directory, number of threads
                        map_output = output.join("af_map");
                        piscem_quant_cmd
                            .arg("--threads")
                            .arg(format!("{}", threads))
                            .arg("-o")
                            .arg(&map_output);

                        // check if we can parse the geometry directly, or if we are dealing with a
                        // "complex" geometry.
                        let frag_lib_xform = add_or_transform_fragment_library(
                            MapperType::Piscem,
                            chem.as_str(),
                            &reads1,
                            &reads2,
                            &mut piscem_quant_cmd,
                        )?;

                        map_cmd_string = get_cmd_line_string(&piscem_quant_cmd);
                        info!("piscem map-sc cmd : {}", map_cmd_string);
                        sc_mapper = String::from("piscem");

                        let mut input_files = vec![
                            index_base.with_extension("ctab"),
                            index_base.with_extension("refinfo"),
                            index_base.with_extension("sshash"),
                        ];
                        input_files.extend_from_slice(&reads1);
                        input_files.extend_from_slice(&reads2);

                        check_files_exist(&input_files)?;

                        let map_start = Instant::now();
                        let cres = prog_utils::execute_command(
                            &mut piscem_quant_cmd,
                            CommandVerbosityLevel::Quiet,
                        )
                        .expect("failed to execute piscem [mapping phase]");

                        // if we had to filter the reads through a fifo
                        // wait for the thread feeding the fifo to finish
                        match frag_lib_xform {
                            FragmentTransformationType::TransformedIntoFifo(xform_data) => {
                                // wait for it to join
                                match xform_data.join_handle.join() {
                                    Ok(join_res) => {
                                        let xform_stats = join_res?;
                                        let total = xform_stats.total_fragments;
                                        let failed = xform_stats.failed_parsing;
                                        info!(
                                            "seq_geom_xform : observed {} input fragments. {} ({:.2}%) of them failed to parse and were not transformed",
                                            total, failed, if total > 0 { (failed as f64) / (total as f64) } else { 0_f64 } * 100_f64
                                        );
                                    }
                                    Err(e) => {
                                        bail!("Thread panicked with {:?}", e);
                                    }
                                }
                            }
                            FragmentTransformationType::Identity => {
                                // nothing to do.
                            }
                        }

                        map_duration = map_start.elapsed();

                        if !cres.status.success() {
                            bail!("piscem mapping failed with exit status {:?}", cres.status);
                        }
                    }
                    IndexType::Salmon(index_base) => {
                        // using a salmon index
                        let mut salmon_quant_cmd = std::process::Command::new(format!(
                            "{}",
                            rp.salmon.unwrap().exe_path.display()
                        ));

                        // set the input index and library type
                        let index_path = format!("{}", index_base.display());
                        salmon_quant_cmd
                            .arg("alevin")
                            .arg("--index")
                            .arg(index_path)
                            .arg("-l")
                            .arg("A");

                        // check if we can parse the geometry directly, or if we are dealing with a
                        // "complex" geometry.
                        let frag_lib_xform = add_or_transform_fragment_library(
                            MapperType::Salmon,
                            chem.as_str(),
                            &reads1,
                            &reads2,
                            &mut salmon_quant_cmd,
                        )?;

                        // location of output directory, number of threads
                        map_output = output.join("af_map");
                        salmon_quant_cmd
                            .arg("--threads")
                            .arg(format!("{}", threads))
                            .arg("-o")
                            .arg(&map_output);

                        // if the user explicitly requested to use selective-alignment
                        // then enable that
                        if use_selective_alignment {
                            salmon_quant_cmd.arg("--rad");
                        } else {
                            // otherwise default to sketch mode
                            salmon_quant_cmd.arg("--sketch");
                        }

                        map_cmd_string = get_cmd_line_string(&salmon_quant_cmd);
                        info!("salmon alevin cmd : {}", map_cmd_string);
                        sc_mapper = String::from("salmon");

                        let mut input_files = vec![index];
                        input_files.extend_from_slice(&reads1);
                        input_files.extend_from_slice(&reads2);

                        check_files_exist(&input_files)?;

                        let map_start = Instant::now();
                        let cres = prog_utils::execute_command(
                            &mut salmon_quant_cmd,
                            CommandVerbosityLevel::Quiet,
                        )
                        .expect("failed to execute salmon [mapping phase]");

                        // if we had to filter the reads through a fifo
                        // wait for the thread feeding the fifo to finish
                        match frag_lib_xform {
                            FragmentTransformationType::TransformedIntoFifo(xform_data) => {
                                // wait for it to join
                                match xform_data.join_handle.join() {
                                    Ok(join_res) => {
                                        let xform_stats = join_res?;
                                        let total = xform_stats.total_fragments;
                                        let failed = xform_stats.failed_parsing;
                                        info!(
                                            "seq_geom_xform : observed {} input fragments. {} ({:.2}%) of them failed to parse and were not transformed",
                                            total, failed, if total > 0 { (failed as f64) / (total as f64) } else { 0_f64 } * 100_f64
                                        );
                                    }
                                    Err(e) => {
                                        bail!("Thread panicked with {:?}", e);
                                    }
                                }
                            }
                            FragmentTransformationType::Identity => {
                                // nothing to do.
                            }
                        }

                        map_duration = map_start.elapsed();

                        if !cres.status.success() {
                            bail!("salmon mapping failed with exit status {:?}", cres.status);
                        }
                    }
                    IndexType::NoIndex => {
                        bail!("Cannot perform mapping an quantification without known (piscem or salmon) index!");
                    }
                }
            } else {
                map_cmd_string = String::from("");
                sc_mapper = String::from("");
                map_output = map_dir
                    .expect("map-dir must be provided, since index, read1 and read2 were not.");
                map_duration = Duration::new(0, 0);
            }

            let map_output_string = map_output.display().to_string();

            let alevin_fry = rp.alevin_fry.unwrap().exe_path;
            // alevin-fry generate permit list
            let mut alevin_gpl_cmd =
                std::process::Command::new(format!("{}", &alevin_fry.display()));

            alevin_gpl_cmd.arg("generate-permit-list");
            alevin_gpl_cmd.arg("-i").arg(&map_output);
            alevin_gpl_cmd.arg("-d").arg(&ori);

            // add the filter mode
            add_to_args(&filter_meth, &mut alevin_gpl_cmd);

            let gpl_output = output.join("af_quant");
            alevin_gpl_cmd.arg("-o").arg(&gpl_output);

            info!(
                "alevin-fry generate-permit-list cmd : {}",
                get_cmd_line_string(&alevin_gpl_cmd)
            );
            let input_files = vec![map_output.clone()];
            check_files_exist(&input_files)?;

            let gpl_start = Instant::now();
            let gpl_proc_out =
                prog_utils::execute_command(&mut alevin_gpl_cmd, CommandVerbosityLevel::Quiet)
                    .expect("could not execute [generate permit list]");
            let gpl_duration = gpl_start.elapsed();

            if !gpl_proc_out.status.success() {
                bail!(
                    "alevin-fry generate-permit-list failed with exit status {:?}",
                    gpl_proc_out.status
                );
            }

            //
            // collate
            //
            let mut alevin_collate_cmd =
                std::process::Command::new(format!("{}", &alevin_fry.display()));

            alevin_collate_cmd.arg("collate");
            alevin_collate_cmd.arg("-i").arg(&gpl_output);
            alevin_collate_cmd.arg("-r").arg(&map_output);
            alevin_collate_cmd.arg("-t").arg(format!("{}", threads));

            info!(
                "alevin-fry collate cmd : {}",
                get_cmd_line_string(&alevin_collate_cmd)
            );
            let input_files = vec![gpl_output.clone(), map_output];
            check_files_exist(&input_files)?;

            let collate_start = Instant::now();
            let collate_proc_out =
                prog_utils::execute_command(&mut alevin_collate_cmd, CommandVerbosityLevel::Quiet)
                    .expect("could not execute [collate]");
            let collate_duration = collate_start.elapsed();

            if !collate_proc_out.status.success() {
                bail!(
                    "alevin-fry collate failed with exit status {:?}",
                    collate_proc_out.status
                );
            }

            //
            // quant
            //
            let mut alevin_quant_cmd =
                std::process::Command::new(format!("{}", &alevin_fry.display()));

            alevin_quant_cmd
                .arg("quant")
                .arg("-i")
                .arg(&gpl_output)
                .arg("-o")
                .arg(&gpl_output);
            alevin_quant_cmd.arg("-t").arg(format!("{}", threads));
            alevin_quant_cmd.arg("-m").arg(t2g_map_file.clone());
            alevin_quant_cmd.arg("-r").arg(resolution);

            info!("cmd : {:?}", alevin_quant_cmd);

            let input_files = vec![gpl_output, t2g_map_file];
            check_files_exist(&input_files)?;

            let quant_start = Instant::now();
            let quant_proc_out =
                prog_utils::execute_command(&mut alevin_quant_cmd, CommandVerbosityLevel::Quiet)
                    .expect("could not execute [quant]");
            let quant_duration = quant_start.elapsed();

            if !quant_proc_out.status.success() {
                bail!("quant failed with exit status {:?}", quant_proc_out.status);
            }

            let af_quant_info_file = output.join("simpleaf_quant_log.json");
            let af_quant_info = json!({
                "time_info" : {
                    "map_time" : map_duration,
                    "gpl_time" : gpl_duration,
                    "collate_time" : collate_duration,
                    "quant_time" : quant_duration
                },
                "cmd_info" : {
                    "map_cmd" : map_cmd_string,
                    "gpl_cmd" : get_cmd_line_string(&alevin_gpl_cmd),
                    "collate_cmd" : get_cmd_line_string(&alevin_collate_cmd),
                    "quant_cmd" : get_cmd_line_string(&alevin_quant_cmd)
                    },
                "map_info" : {
                    "mapper" : sc_mapper,
                    "map_cmd" : map_cmd_string,
                    "map_outdir": map_output_string
                    }
            });

            // write the relevant info about
            // our run to file.
            std::fs::write(
                &af_quant_info_file,
                serde_json::to_string_pretty(&af_quant_info).unwrap(),
            )
            .with_context(|| format!("could not write {}", af_quant_info_file.display()))?;
        }
        _ => {
            bail!("unknown command")
        }
    }
    Ok(())
}

/// ### Program Name
/// simpleaf generate-workflow
///
/// ### Program Input
/// A json file that records all top level variables needed by the template
///                  and optionally, some extra variables
/// ### Program Output
/// A json file that contains the actual simpelaf workflow information, which can be
///         consumed directly by the simpleaf run-workflow command. Additionally, if --execute is specified,
///          the generated simpleaf workflow will be executed.
/// ### Program Description
/// This program is used for generating a simpleaf workflow JSON file
/// that can be consumed directly by the `simpleaf workflow` program.\
/// This program takes a template from our template library as the input
/// and do the following:
/// 1. It loads the required arguments of that template and
///      find them in the user-provided JSON file.
/// 2. It validates the files in the user-provided JSON file.
///      This can be checking the existance and validate the first few records
/// 3. It feeds the template the required inputs, and
///      generates a simpleaf workflow JSON file.
///      This JSON file contains the simpleaf programs need to be run and
///      the required arguments.

// TODO:
// 1. figure out the layout of protocol estuary
// 2. find workflow using name, if doesn't exist, find similar names and return error
// 3. copy the config file from af_home protocol estuary dir to the output dir.
// 4. allow name change?

fn get_workflow_config(af_home_path: &Path, gw_cmd: Commands) -> anyhow::Result<()> {
    match gw_cmd {
        Commands::GetWorkflowConfig {
            output,
            workflow,
            // essential_only: _,
        } => {
            // get af_home
            let v: Value = inspect_af_home(af_home_path)?;
            // Read the JSON contents of the file as an instance of `User`.
            let rp: ReqProgs = serde_json::from_value(v["prog_info"].clone())?;

            // get protocol library path
            let protocol_estuary = get_protocol_estuary(af_home_path)?;
            // get the corresponding workflow directory path
            let workflow_path = protocol_estuary.protocols_dir.join(workflow.as_str());
            // make output dir
            let mut output_dir_name = workflow.clone();
            output_dir_name.push_str("_config");
            let output_path = output.join(output_dir_name);

            // check if workflow path exists
            match workflow_path.try_exists() {
                // if it exists, then copy this folder to the output dir
                Ok(true) => {
                    info!("Exporting workflow files to the output folder");

                    match copy_dir_all(workflow_path.as_path(), output_path.as_path()) {
                        Ok(_) => {}
                        Err(e) => {
                            bail!("Could not copy workflow files to the output folder. The error was: {}", e);
                        }
                    };
                }
                Ok(false) => {
                    // if doesn't exist, check if there are similar workflow names
                    // return with error and report similar workflow name if any.
                    let protocol_library_dir = fs::read_dir(
                        protocol_estuary.protocols_dir.as_path(),
                    )
                    .with_context(|| {
                        format!(
                            "Could not get protocol library in directory: {} ",
                            protocol_estuary.protocols_dir.display()
                        )
                    })?;
                    let mut similar_names: Vec<String> = Vec::new();
                    // iterate over protocol library folder
                    for p in protocol_library_dir {
                        let pp = p
                            .expect("Could not read directory protocol library directory")
                            .path();
                        let curr_workflow_name = pp
                            .file_name()
                            .expect("Could not get the directory name")
                            .to_str()
                            .expect("Could not convert dir name to str.");
                        // if finds similar file names, push to the vec
                        if curr_workflow_name.contains(workflow.as_str()) {
                            similar_names.push(curr_workflow_name.to_string());
                        }
                    }

                    // decide the final log info
                    let similar_name_hints = if similar_names.is_empty() {
                        String::from("")
                    } else {
                        similar_names.insert(
                            0,
                            String::from("Workflows with a similar name exist, which are"),
                        );
                        similar_names.join(", ")
                    };

                    // return with an error
                    bail!(
                        "Could not find a workflow with name: {}. {}",
                        workflow,
                        similar_name_hints
                    );
                }
                Err(e) => {
                    bail!(e)
                }
            }

            // write log
            let gwc_info_path = output_path.join("get_workflow_config.json");
            let gwc_info = json!({
                "command" : "get-workflow-config",
                "version_info" : rp,
                "workflow dir": output_path,

                "args" : {
                    "output" : output,
                    "workflow" : workflow,
                    // "essential_only" : essential_only,
                }
            });

            std::fs::write(
                &gwc_info_path,
                serde_json::to_string_pretty(&gwc_info).unwrap(),
            )
            .with_context(|| format!("could not write {}", gwc_info_path.display()))?;

            info!(
                "Successfully export {} workflow to {}",
                workflow,
                output_path.display()
            );
        }
        _ => {
            bail!("Unknown Command.")
        }
    }
    Ok(())
}

/// ## simpleaf run-workflow
/// #### Input
/// one or more simpleaf workflow JSON file (s) with all required fields
///
/// #### Output
/// the output of the simpleaf commands recorded in the input JSON file
///
/// #### Description
/// This program is used for running the commands recorded in the
/// user-provided simpleaf workflow JSON file(s).
/// Simpleaf Workflow JSON format required fields:
/// 1. json_type: This field has to exist and have the value "Simpleaf Workflow"
/// 2. simpleaf_version: This field has to exist and contains the version of simpleaf
///     used for making the file. If the files are made manually, this value has to be
///      higher than 0.11.0
/// 3. index: (Optional): this field records all simpleaf index commands that need to be run.
/// 4. quant: (Optional): this field records all simpleaf quant commands that need to be run.

// TODO: add a `skip` argument for skipping steps
fn workflow(af_home_path: &Path, workflow_cmd: Commands) -> anyhow::Result<()> {
    match workflow_cmd {
        Commands::Workflow {
            config_path,
            workflow_path,
            output,
            // TODO: write JSON only if no execution
            no_execution,
            start_at,
            resume,
            lib_paths,
            skip_step,
        } => {
            run_fun!(mkdir -p $output)?;

            let simpleaf_workflow: SimpleafWorkflow;
            let mut workflow_log: WorkflowLog;

            let final_start_at = if resume {
                update_start_at(output.as_path())?
            } else {
                start_at
            };

            let final_skip_step = if let Some(ss) = skip_step {
                ss
            } else {
                Vec::new()
            };

            // we will have either a config_path or a workflow_path
            // if we see config files. process it
            if let Some(cp) = config_path {
                //  check the validity of the file
                if !cp.exists() {
                    bail!("the path of the given workflow configuratioin file doesn't exist; Cannot proceed.")
                }

                info!("Processing simpleaf workflow configuration file.");

                // iterate json files and parse records to commands
                // convert files into json string vector
                let workflow_json_string = parse_workflow_config(
                    af_home_path,
                    cp.as_path(),
                    output.as_path(),
                    &lib_paths,
                )?;

                // write complete workflow json to output folder
                // the `Step` of each command in this json file will be changed to "-1"
                // once the command is run successfully.
                // The final workflow file name will be the same as the input config but
                // with json as the extention.
                let workflow_json_value: Value =
                    serde_json::from_str(workflow_json_string.as_str())?;

                // initialize simpleaf workflow and log struct
                // TODO: print some log using meta_info fields
                (simpleaf_workflow, workflow_log) = initialize_workflow(
                    af_home_path,
                    cp.as_path(),
                    output.as_path(),
                    workflow_json_value,
                    final_start_at,
                    final_skip_step,
                )?;
            } else {
                // This file has to exist
                let wp = workflow_path.expect(
                    "Neither configuration file nor workflow file is provided; Cannot proceed.",
                );

                // check the existence of the file
                if !wp.exists() {
                    bail!("the path of the given workflow configuratioin file doesn't exist; Cannot proceed.")
                }
                // load each file as a wrapper struct of a vector of simpleaf commands
                let json_file = fs::File::open(wp.as_path())
                    .with_context(|| format!("Could not open JSON file {}.", wp.display()))?;

                // TODO: print some log using meta_info fields
                let workflow_json_value: Value = serde_json::from_reader(json_file)?;

                (simpleaf_workflow, workflow_log) = initialize_workflow(
                    af_home_path,
                    wp.as_path(),
                    output.as_path(),
                    workflow_json_value,
                    final_start_at,
                    final_skip_step,
                )?;
            }
            if !no_execution {
                for cr in simpleaf_workflow.cmd_queue {
                    let pn = cr.program_name;
                    let step = cr.step;
                    // this if statement is no longer needed as commands with a negative exec order
                    // are ignore when constructing the the cmd queue
                    // say something
                    info!("Running {} command with step {}.", pn, step,);

                    // initiliaze a stopwatch
                    workflow_log.timeit(step);

                    if let Some(cmd) = cr.simpleaf_cmd {
                        let exec_result = match cmd {
                            Commands::Index {
                                ref_type,
                                fasta,
                                gtf,
                                rlen,
                                spliced,
                                unspliced,
                                dedup,
                                keep_duplicates,
                                ref_seq,
                                output,
                                use_piscem,
                                kmer_length,
                                minimizer_length,
                                overwrite,
                                sparse,
                                threads,
                            } => build_ref_and_index(
                                af_home_path,
                                Commands::Index {
                                    ref_type,
                                    fasta,
                                    gtf,
                                    rlen,
                                    spliced,
                                    unspliced,
                                    dedup,
                                    keep_duplicates,
                                    ref_seq,
                                    output,
                                    use_piscem,
                                    kmer_length,
                                    minimizer_length,
                                    overwrite,
                                    sparse,
                                    threads,
                                },
                            ),

                            // if we are running mapping and quantification
                            Commands::Quant {
                                index,
                                use_piscem,
                                map_dir,
                                reads1,
                                reads2,
                                threads,
                                use_selective_alignment,
                                expected_ori,
                                knee,
                                unfiltered_pl,
                                explicit_pl,
                                forced_cells,
                                expect_cells,
                                min_reads,
                                resolution,
                                t2g_map,
                                chemistry,
                                output,
                            } => map_and_quant(
                                af_home_path,
                                Commands::Quant {
                                    index,
                                    use_piscem,
                                    map_dir,
                                    reads1,
                                    reads2,
                                    threads,
                                    use_selective_alignment,
                                    expected_ori,
                                    knee,
                                    unfiltered_pl,
                                    explicit_pl,
                                    forced_cells,
                                    expect_cells,
                                    min_reads,
                                    resolution,
                                    t2g_map,
                                    chemistry,
                                    output,
                                },
                            ),
                            _ => todo!(),
                        };
                        if let Err(e) = exec_result {
                            workflow_log.write(false)?;
                            info!("Execution terminated at {} command with step {}", pn, step);
                            return Err(e);
                        } else {
                            info!("Successfully ran {} command with step {}", pn, step);

                            workflow_log.update(&cr.field_trajectory_vec[..]);
                        }
                    }

                    // If this is an external command, then initialize it and run
                    if let Some(mut cmd) = cr.external_cmd {
                        // log
                        let cmd_string = get_cmd_line_string(&cmd);
                        info!("Invoking command : {}", cmd_string);

                        // initiate a stopwatch
                        workflow_log.timeit(cr.step);

                        match cmd.output() {
                            Ok(cres) => {
                                // check the return status of external command
                                if cres.status.success() {
                                    // succeed. update log
                                    workflow_log.update(&cr.field_trajectory_vec[..]);
                                } else {
                                    let cmd_string = get_cmd_line_string(&cmd);
                                    match run_cmd!(sh -c $cmd_string) {
                                        Ok(_) => {
                                            // succeed. update log
                                            workflow_log.update(&cr.field_trajectory_vec[..]);
                                        }
                                        Err(e2) => {
                                            workflow_log.write(false)?;
                                            bail!(
                                                "{} with step {} failed in two different attempts.\n\
                                                The exit status of the first attempt was: {:?}. \n\
                                                The stderr of the first attempt was: {:?}. \n\
                                                The error message of the second attempt was: {:?}.",
                                                pn, step,
                                                cres.status,
                                                std::str::from_utf8(&cres.stderr[..]).unwrap(),
                                                e2
                                            );
                                        }
                                    };
                                }
                            }
                            Err(e) => {
                                let cmd_string = get_cmd_line_string(&cmd);
                                match run_cmd!(sh -c $cmd_string) {
                                    Ok(_) => {
                                        workflow_log.update(&cr.field_trajectory_vec[..]);
                                    }
                                    Err(e2) => {
                                        workflow_log.write(false)?;
                                        bail!(
                                            "{} command with step {} failed in two different attempts.\n\
                                            The stderr of the first attempt was: {:?}. \n\
                                            The error message of the second attempt was: {:?}.",
                                            pn, step,
                                            e,
                                            e2
                                        );
                                    }
                                };
                            } // TODO: use this in the log somewhere.
                        } // invoke external cmd

                        info!("Successfully ran {} command with step {}.", pn, step);
                    } // for cmd_queue
                }
                // write log
                workflow_log.write(true)?;

                info!("All commands ran successfully.");
            } else {
                workflow_log.write(false)?;
            }
        } //
        _ => {
            bail!("unknown command")
        }
    } // match Commands::Workflow
    Ok(())
}

enum IndexType {
    Salmon(PathBuf),
    Piscem(PathBuf),
    NoIndex,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();
    const AF_HOME: &str = "ALEVIN_FRY_HOME";
    let af_home_path = match env::var(AF_HOME) {
        Ok(p) => PathBuf::from(p),
        Err(e) => {
            bail!(
                "${} is unset {}, please set this environment variable to continue.",
                AF_HOME,
                e
            );
        }
    };

    let cli_args = Cli::parse();

    match cli_args.command {
        // set the paths where the relevant tools live
        Commands::SetPaths {
            salmon,
            piscem,
            alevin_fry,
            pyroe,
        } => set_paths(
            af_home_path,
            Commands::SetPaths {
                salmon,
                piscem,
                alevin_fry,
                pyroe,
            },
        ),
        Commands::AddChemistry { name, geometry } => {
            add_chemistry(af_home_path, Commands::AddChemistry { name, geometry })
        }
        Commands::Inspect {} => inspect_simpleaf(af_home_path),
        // if we are building the reference and indexing
        Commands::Index {
            ref_type,
            fasta,
            gtf,
            rlen,
            spliced,
            unspliced,
            dedup,
            keep_duplicates,
            ref_seq,
            output,
            use_piscem,
            kmer_length,
            minimizer_length,
            overwrite,
            sparse,
            threads,
        } => build_ref_and_index(
            af_home_path.as_path(),
            Commands::Index {
                ref_type,
                fasta,
                gtf,
                rlen,
                spliced,
                unspliced,
                dedup,
                keep_duplicates,
                ref_seq,
                output,
                use_piscem,
                kmer_length,
                minimizer_length,
                overwrite,
                sparse,
                threads,
            },
        ),

        // if we are running mapping and quantification
        Commands::Quant {
            index,
            use_piscem,
            map_dir,
            reads1,
            reads2,
            threads,
            use_selective_alignment,
            expected_ori,
            knee,
            unfiltered_pl,
            explicit_pl,
            forced_cells,
            expect_cells,
            min_reads,
            resolution,
            t2g_map,
            chemistry,
            output,
        } => map_and_quant(
            af_home_path.as_path(),
            Commands::Quant {
                index,
                use_piscem,
                map_dir,
                reads1,
                reads2,
                threads,
                use_selective_alignment,
                expected_ori,
                knee,
                unfiltered_pl,
                explicit_pl,
                forced_cells,
                expect_cells,
                min_reads,
                resolution,
                t2g_map,
                chemistry,
                output,
            },
        ),
        Commands::Workflow {
            config_path,
            workflow_path,
            output,
            no_execution,
            start_at,
            resume,
            lib_paths,
            skip_step,
        } => workflow(
            af_home_path.as_path(),
            Commands::Workflow {
                config_path,
                workflow_path,
                output,
                no_execution,
                start_at,
                resume,
                lib_paths,
                skip_step,
            },
        ),
        Commands::GetWorkflowConfig {
            output,
            workflow,
            // essential_only,
        } => get_workflow_config(
            af_home_path.as_path(),
            Commands::GetWorkflowConfig {
                output,
                workflow,
                // essential_only,
            },
        ),
    }
    // success, yay!
}
