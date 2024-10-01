use crate::utils::prog_utils;
use crate::utils::prog_utils::{CommandVerbosityLevel, ReqProgs};

use anyhow::{bail, Context};
use cmd_lib::run_fun;
use roers;
use serde_json::json;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{error, info, warn};

use super::{IndexOpts, ReferenceType};

fn validate_index_type_opts(opts: &IndexOpts) -> anyhow::Result<()> {
    let mut bail = false;
    if opts.use_piscem && opts.sparse {
        let msg = concat!(
            "The `--sparse` flag implies the salmon index, and is incompatible with `--use-piscem` (the default). ",
            "If you wish to use the salmon index, and the `--sparse` option, please pass both ",
            "`--no-piscem` and `--sparse` to the `index` command."
        );
        error!(msg);
        bail = true;
    }
    if bail {
        bail!("conflicting command line arguments");
    }
    Ok(())
}

pub fn build_ref_and_index(af_home_path: &Path, opts: IndexOpts) -> anyhow::Result<()> {
    validate_index_type_opts(&opts)?;
    let mut threads = opts.threads;
    let output = opts.output;
    let v: Value = prog_utils::inspect_af_home(af_home_path)?;
    // Read the JSON contents of the file as an instance of `User`.
    let rp: ReqProgs = serde_json::from_value(v["prog_info"].clone())?;

    rp.issue_recommended_version_messages();
    // we are building a custom spliced+intronic reference
    // make sure that a read length is available / was provided.
    // if fasta.is_some() && matches!(ref_type, ReferenceType::SplicedIntronic) && rlen.is_none() {
    //     bail!(format!("A spliced+intronic reference was requested, but no read length argument (--rlen) was provided."));
    // }

    let info_file = output.join("index_info.json");
    let mut index_info = json!({
        "command" : "index",
        "version_info" : rp,
        "args" : {
            "output" : output,
            "overwrite" : opts.overwrite,
            "keep_duplicates" : opts.keep_duplicates,
            "sparse" : opts.sparse,
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
    let mut roers_duration = None;
    let mut roers_aug_ref_opt = None;

    // if we are generating a splici reference
    if let (Some(fasta), Some(gtf)) = (opts.fasta, opts.gtf) {
        let input_files = vec![fasta.clone(), gtf.clone()];

        // the "transcript" (spliced transcriptome) is currently implicit
        // in roers, so we don't have to add that. If the user requested
        // a spliced+intronic (splici) transcriptome, then we also want introns
        // whereas if they requested a spliced+unspliced (spliceu) transcriptome,
        // then we also want gene bodies.
        // TODO: Right now, there is not way in simpleaf, from the command line,
        // to specify `TranscriptBody` rather than `GeneBody`, think about if
        // we want to find a way to expose this.
        let aug_type = match opts.ref_type {
            ReferenceType::SplicedIntronic => Some(vec![roers::AugType::Intronic]),
            ReferenceType::SplicedUnspliced => Some(vec![roers::AugType::GeneBody]),
        };

        let outref = output.join("ref");
        run_fun!(mkdir -p $outref)?;

        let roers_opts = roers::AugRefOpts {
            // The path to a genome fasta file.
            genome: fasta.clone(),
            // The path to a gene annotation gtf/gff3 file.
            genes: gtf.clone(),
            // The path to the output directory (will be created if it doesn't exist).
            out_dir: outref.clone(),
            aug_type,
            no_transcript: false,
            read_length: opts.rlen,
            flank_trim_length: 5_i64, // not currently setable from the cmdline
            no_flanking_merge: false, // not currently setable from the cmdline
            filename_prefix: String::from("roers_ref"),
            dedup_seqs: opts.dedup,
            extra_spliced: opts.spliced.clone(),
            extra_unspliced: opts.unspliced.clone(),
            gff3: opts.gff3_format,
        };

        roers_aug_ref_opt = Some(roers_opts.clone());

        let ref_file = outref.join("roers_ref.fa");
        let t2g_file = outref.join("t2g_3col.tsv");

        index_info["t2g_file"] = json!(&t2g_file);
        index_info["args"]["fasta"] = json!(&fasta);
        index_info["args"]["gtf"] = json!(&gtf);
        index_info["args"]["spliced"] = json!(&opts.spliced);
        index_info["args"]["unspliced"] = json!(&opts.unspliced);
        index_info["args"]["dedup"] = json!(opts.dedup);

        std::fs::write(
            &info_file,
            serde_json::to_string_pretty(&index_info).unwrap(),
        )
        .with_context(|| format!("could not write {}", info_file.display()))?;

        // set the splici_t2g option
        splici_t2g = Some(t2g_file);

        prog_utils::check_files_exist(&input_files)?;

        info!("preparing to make reference with roers");

        let roers_start = Instant::now();
        roers::make_ref(roers_opts)?;
        roers_duration = Some(roers_start.elapsed());

        reference_sequence = Some(ref_file);
    } else {
        // we are running on a set of references directly

        // in this path (due to the argument parser requiring
        // either --fasta or --ref-seq, ref-seq should be safe to
        // unwrap).
        index_info["args"]["ref-seq"] = json!(opts.ref_seq.clone().unwrap());

        std::fs::write(
            &info_file,
            serde_json::to_string_pretty(&index_info).unwrap(),
        )
        .with_context(|| format!("could not write {}", info_file.display()))?;

        reference_sequence = opts.ref_seq;
    }

    let ref_seq = reference_sequence.with_context(||
                "Reference sequence should either be generated from --fasta with reftype spliced+intronic / spliced+unspliced or set with --ref-seq",
            )?;

    let input_files = vec![ref_seq.clone()];
    prog_utils::check_files_exist(&input_files)?;

    let output_index_dir = output.join("index");
    let index_duration;
    let index_cmd_string: String;

    if opts.use_piscem {
        // ensure we have piscem
        if rp.piscem.is_none() {
            bail!("The construction of a piscem index was requested, but a valid piscem executable was not available. \n\
                            Please either set a path using the `set-paths` command, or ensure the `PISCEM` environment variable is set properly.");
        }

        let piscem_prog_info = rp
            .piscem
            .as_ref()
            .expect("piscem program info should be properly set.");

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
            .arg(&ref_seq)
            .arg("--seed")
            .arg(opts.hash_seed.to_string())
            .arg("-w")
            .arg(opts.work_dir);

        // if the user requested to overwrite, then pass this option
        if opts.overwrite {
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

        // if the user is requesting a poison k-mer table, ensure the
        // piscem version is at least 0.7.0
        if let Some(decoy_paths) = opts.decoy_paths {
            if let Ok(_piscem_ver) = prog_utils::check_version_constraints(
                "piscem",
                ">=0.7.0, <1.0.0",
                &piscem_prog_info.version,
            ) {
                let path_args = decoy_paths
                    .into_iter()
                    .map(|x| x.to_string_lossy().into_owned())
                    .collect::<Vec<String>>()
                    .join(",");
                piscem_index_cmd.arg("--decoy-paths").arg(path_args);
            } else {
                warn!(
                    r#"
You requested to build a poison k-mer table with {:?}, but you must be using piscem version >= 0.7.0 
to use this feature. Simpleaf is currently using version {}. Please upgrade your piscem version or, 
if you believe you have a sufficiently new version installed, update the executable being used by 
simpleaf"#,
                    decoy_paths, &piscem_prog_info.version
                );
            }
        }

        // print piscem build command
        index_cmd_string = prog_utils::get_cmd_line_string(&piscem_index_cmd);
        info!("piscem build cmd : {}", index_cmd_string);

        let index_start = Instant::now();
        let cres = prog_utils::execute_command(&mut piscem_index_cmd, CommandVerbosityLevel::Quiet)
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
                "cmd" : index_cmd_string,
                "index_type" : "piscem",
                "t2g_file" : t2g_out_path,
                "piscem_index_parameters" : {
                    "k" : opts.kmer_length,
                    "m" : opts.minimizer_length,
                    "overwrite" : opts.overwrite,
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
            bail!("The construction of a salmon index was requested, but a valid salmon executable was not available. \n\
                            Please either set a path using the `simpleaf set-paths` command, or ensure the `SALMON` environment variable is set properly.");
        }

        let mut salmon_index_cmd =
            std::process::Command::new(format!("{}", rp.salmon.unwrap().exe_path.display()));

        salmon_index_cmd
            .arg("index")
            .arg("-k")
            .arg(opts.kmer_length.to_string())
            .arg("-i")
            .arg(&output_index_dir)
            .arg("-t")
            .arg(&ref_seq);

        // overwrite doesn't do anything special for the salmon index, so mention this to
        // the user.
        if opts.overwrite {
            info!("As the default salmon behavior is to overwrite an existing index if the same directory is provided, \n\
                        the --overwrite flag will have no additional effect.");
        }

        // if the user requested a sparse index.
        if opts.sparse {
            salmon_index_cmd.arg("--sparse");
        }

        // if the user requested keeping duplicated sequences.
        if opts.keep_duplicates {
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
        let cres = prog_utils::execute_command(&mut salmon_index_cmd, CommandVerbosityLevel::Quiet)
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
                    "k" : opts.kmer_length,
                    "overwrite" : opts.overwrite,
                    "sparse" : opts.sparse,
                    "keep_duplicates" : opts.keep_duplicates,
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
    let index_log_info = if let Some(roers_duration) = roers_duration {
        // if we ran make-splici
        json!({
            "time_info" : {
                "roers_time" : roers_duration,
                "index_time" : index_duration
            },
            "cmd_info" : {
                "roers_cmd" : roers_aug_ref_opt,
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
    Ok(())
}
