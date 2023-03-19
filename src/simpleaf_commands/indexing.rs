use crate::utils::prog_utils;
use crate::utils::prog_utils::{CommandVerbosityLevel, ReqProgs};

use anyhow::{bail, Context};
use cmd_lib::run_fun;
use serde_json::json;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{info, warn};

use super::{Commands, ReferenceType};

pub fn build_ref_and_index(af_home_path: &Path, index_args: Commands) -> anyhow::Result<()> {
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
            let v: Value = prog_utils::inspect_af_home(af_home_path)?;
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

                prog_utils::check_files_exist(&input_files)?;

                // print pyroe command
                pyroe_cmd_string = prog_utils::get_cmd_line_string(&pyroe_cmd);
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
            prog_utils::check_files_exist(&input_files)?;

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
                index_cmd_string = prog_utils::get_cmd_line_string(&piscem_index_cmd);
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
                index_cmd_string = prog_utils::get_cmd_line_string(&salmon_index_cmd);
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
