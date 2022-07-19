extern crate env_logger;
#[macro_use]
extern crate log;

use anyhow::{anyhow, bail, Context, Result};
use clap::{ArgGroup, Parser, Subcommand};
use cmd_lib::run_fun;
use env_logger::Env;
use serde_json::json;
use time::Instant;

use std::env;
use std::io::BufReader;
use std::path::PathBuf;

mod utils;
use utils::af_utils::*;
use utils::prog_utils::*;

#[derive(Debug, Subcommand)]
enum Commands {
    /// build the splici index
    #[clap(arg_required_else_help = true)]
    Index {
        /// reference genome
        #[clap(short, long, value_parser)]
        fasta: PathBuf,

        /// reference GTF file
        #[clap(short, long, value_parser)]
        gtf: PathBuf,

        /// the target read length the index will be built for
        #[clap(short, long, value_parser)]
        rlen: u32,

        /// path to output directory (will be created if it doesn't exist)
        #[clap(short, long, value_parser)]
        output: PathBuf,

        /// path to FASTA file with extra spliced sequence to add to the index
        #[clap(short, long, value_parser)]
        spliced: Option<PathBuf>,

        /// path to FASTA file with extra unspliced sequence to add to the index
        #[clap(short, long, value_parser)]
        unspliced: Option<PathBuf>,

        /// deduplicate identical sequences inside the R script when building the splici reference
        #[clap(short = 'd', long = "dedup", action)]
        dedup: bool,

        /// if this flag is passed, build the sparse rather than dense index for mapping
        #[clap(short = 'p', long = "sparse", action)]
        sparse: bool,

        /// number of threads to use when running [default: min(16, num cores)]"
        #[clap(short, long, default_value_t = 16, value_parser)]
        threads: u32,
    },
    /// quantify a sample
    #[clap(arg_required_else_help = true)]
    #[clap(group(
            ArgGroup::new("filter")
            .required(true)
            .args(&["knee", "unfiltered-pl", "forced-cells", "expect-cells"])
            ))]
    Quant {
        /// path to index
        #[clap(short, long, value_parser)]
        index: PathBuf,

        /// path to read 1 files
        #[clap(short = '1', long = "reads1", value_parser)]
        reads1: Vec<PathBuf>,

        /// path to read 2 files
        #[clap(short = '2', long = "reads2", value_parser)]
        reads2: Vec<PathBuf>,

        /// number of threads to use when running [default: min(16, num cores)]"
        #[clap(short, long, default_value_t = 16, value_parser)]
        threads: u32,

        /// use knee filtering mode
        #[clap(short, long, action)]
        knee: bool,

        /// use unfiltered permit list
        #[clap(short, long, action)]
        unfiltered_pl: bool,

        /// use a filtered, explicit permit list
        #[clap(short, long, value_parser)]
        explicit_pl: Option<PathBuf>,

        /// use forced number of cells
        #[clap(short, long, value_parser)]
        forced_cells: Option<usize>,

        /// use expected number of cells
        #[clap(short, long, value_parser)]
        expect_cells: Option<usize>,

        /// resolution mode
        #[clap(short, long, value_parser = clap::builder::PossibleValuesParser::new(["cr-like", "cr-like-em", "parsimony", "parsimony-em", "parsimony-gene", "parsimony-gene-em"]))]
        resolution: String,

        /// chemistry
        #[clap(short, long, value_parser)]
        chemistry: String,

        /// transcript to gene map
        #[clap(short = 'm', long, value_parser)]
        t2g_map: PathBuf,

        /// output directory
        #[clap(short, long, value_parser)]
        output: PathBuf,
    },
    /// set paths to the programs that simpleaf will use
    SetPaths {
        /// path to salmon to use
        #[clap(short, long, value_parser)]
        salmon: Option<PathBuf>,
        /// path to alein-fry to use
        #[clap(short, long, value_parser)]
        alevin_fry: Option<PathBuf>,
        /// path to pyroe to use
        #[clap(short, long, value_parser)]
        pyroe: Option<PathBuf>,
    },
}

/// simplifying alevin-fry workflows
#[derive(Debug, Parser)]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

enum Chemistry {
    TenxV2,
    TenxV3,
    Other(String),
}

enum PermitListResult {
    DownloadSuccessful(PathBuf),
    AlreadyPresent(PathBuf),
    UnregisteredChemistry,
}

fn get_permit_if_absent(chem: Chemistry) -> Result<PermitListResult> {
    let chem_file;
    let dl_url;
    match chem {
        Chemistry::TenxV2 => {
            chem_file = "10x_v2_permit.txt";
            dl_url = "https://umd.box.com/shared/static/jbs2wszgbj7k4ic2hass9ts6nhqkwq1p";
        }
        Chemistry::TenxV3 => {
            chem_file = "10x_v3_permit.txt";
            dl_url = "https://umd.box.com/shared/static/eo0qlkfqf2v24ws6dfnxty6gqk1otf2h";
        }
        _ => {
            return Ok(PermitListResult::UnregisteredChemistry);
        }
    }
    match env::var("ALEVIN_FRY_HOME") {
        Ok(p) => {
            let odir = PathBuf::from(p).join("plist");
            if odir.join(chem_file).exists() {
                Ok(PermitListResult::AlreadyPresent(odir.join(chem_file)))
            } else {
                run_fun!(mkdir -p $odir)?;
                let mut dl_cmd = std::process::Command::new("wget");
                dl_cmd
                    .arg("-v")
                    .arg("-O")
                    .arg(odir.join(chem_file).to_string_lossy().to_string())
                    .arg("-L")
                    .arg(dl_url);
                let r = dl_cmd.output()?;
                if !r.status.success() {
                    return Err(anyhow!("failed to download permit list {:?}", r.status));
                }
                Ok(PermitListResult::DownloadSuccessful(odir.join(chem_file)))
            }
        }
        Err(e) => {
            return Err(anyhow!(
                "could not resolve $ALEVIN_FRY_HOME environment variable : {}",
                e
            ));
        }
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
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
        Commands::SetPaths {
            salmon,
            alevin_fry,
            pyroe,
        } => {
            let rp = get_required_progs_from_paths(salmon, alevin_fry, pyroe)?;

            if rp.salmon.is_none() {
                bail!("Suitable salmon executable not found");
            }
            if rp.alevin_fry.is_none() {
                bail!("Suitable alevin_fry executable not found");
            }
            if rp.pyroe.is_none() {
                bail!("Suitable pyroe executable not found");
            }

            let simpleaf_info_file = af_home_path.join("simpleaf_info.json");
            let simpleaf_info = json!({ "prog_info": rp });

            std::fs::write(
                &simpleaf_info_file,
                serde_json::to_string_pretty(&simpleaf_info).unwrap(),
            )
            .with_context(|| format!("could not write {}", simpleaf_info_file.display()))?;
        }
        Commands::Index {
            fasta,
            gtf,
            rlen,
            output,
            spliced,
            unspliced,
            dedup,
            sparse,
            mut threads,
        } => {
            // Open the file in read-only mode with buffer.
            let af_info_p = af_home_path.join("simpleaf_info.json");
            let simpleaf_info_file = std::fs::File::open(&af_info_p).with_context({
                ||
                format!("Could not open file {}; please run the set-paths command before using `index` or `quant`", af_info_p.display())
            })?;

            let simpleaf_info_reader = BufReader::new(simpleaf_info_file);

            // Read the JSON contents of the file as an instance of `User`.
            let v: serde_json::Value = serde_json::from_reader(simpleaf_info_reader)?;
            let rp: ReqProgs = serde_json::from_value(v["prog_info"].clone())?;

            run_fun!(mkdir -p $output)?;
            let ref_file = format!("splici_fl{}.fa", rlen - 5);

            let outref = output.join("ref");
            run_fun!(mkdir -p $outref)?;

            let t2g_file = outref.join(format!("splici_fl{}_t2g_3col.tsv", rlen - 5));
            let info_file = output.join("index_info.json");
            let index_info = json!({
                "command" : "index",
                "version_info" : rp,
                "t2g_file" : t2g_file,
                "args" : {
                    "fasta" : fasta,
                    "gtf" : gtf,
                    "rlen" : rlen,
                    "output" : output,
                    "spliced" : spliced,
                    "unspliced" : unspliced,
                    "dedup" : dedup,
                    "sparse" : sparse,
                    "threads" : threads
                }
            });

            std::fs::write(
                &info_file,
                serde_json::to_string_pretty(&index_info).unwrap(),
            )
            .with_context(|| format!("could not write {}", info_file.display()))?;

            let mut cmd =
                std::process::Command::new(format!("{}", rp.pyroe.unwrap().exe_path.display()));
            // we will run the make-splici command
            cmd.arg("make-splici");

            // if the user wants to dedup output sequences
            if dedup {
                cmd.arg(String::from("--dedup-seqs"));
            }

            // extra spliced sequence
            match spliced {
                Some(es) => {
                    cmd.arg(String::from("--extra-spliced"));
                    cmd.arg(format!("{}", es.display()));
                }
                None => {}
            }

            // extra unspliced sequence
            match unspliced {
                Some(eu) => {
                    cmd.arg(String::from("--extra-unspliced"));
                    cmd.arg(format!("{}", eu.display()));
                }
                None => {}
            }

            cmd.arg(fasta)
                .arg(gtf)
                .arg(format!("{}", rlen))
                .arg(&outref);

            let pyroe_start = Instant::now();
            let cres = cmd.output()?;
            let pyroe_duration = pyroe_start.elapsed();

            if !cres.status.success() {
                bail!("pyroe failed to return succesfully {:?}", cres.status);
            }

            let mut salmon_index_cmd =
                std::process::Command::new(format!("{}", rp.salmon.unwrap().exe_path.display()));
            let ref_seq = outref.join(ref_file);

            let output_index_dir = output.join("index");
            salmon_index_cmd
                .arg("index")
                .arg("-i")
                .arg(&output_index_dir)
                .arg("-t")
                .arg(ref_seq);

            // if the user requested a sparse index.
            if sparse {
                salmon_index_cmd.arg("--sparse");
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

            let index_start = Instant::now();
            salmon_index_cmd
                .output()
                .expect("failed to run salmon index");
            let index_duration = index_start.elapsed();

            // copy over the t2g file to the index
            let index_t2g_path = output_index_dir.join("t2g_3col.tsv");
            std::fs::copy(t2g_file, index_t2g_path)?;

            let index_log_file = output.join("simpleaf_index_log.json");
            let index_log_info = json!({
                "time_info" : {
                    "pyroe_time" : pyroe_duration,
                    "index_time" : index_duration
                }
            });

            std::fs::write(
                &index_log_file,
                serde_json::to_string_pretty(&index_log_info).unwrap(),
            )
            .with_context(|| format!("could not write {}", index_log_file.display()))?;
        }
        Commands::Quant {
            index,
            reads1,
            reads2,
            threads,
            knee,
            unfiltered_pl,
            explicit_pl,
            forced_cells,
            expect_cells,
            resolution,
            t2g_map,
            chemistry,
            output,
        } => {
            // Open the file in read-only mode with buffer.
            let af_info_p = af_home_path.join("simpleaf_info.json");
            let simpleaf_info_file = std::fs::File::open(&af_info_p).with_context({
                ||
                format!("Could not open file {}; please run the set-paths command before using `index` or `quant`", af_info_p.display())
            })?;

            let simpleaf_info_reader = BufReader::new(&simpleaf_info_file);

            // Read the JSON contents of the file as an instance of `User`.
            info!("deserializing from {:?}", simpleaf_info_file);
            let v: serde_json::Value = serde_json::from_reader(simpleaf_info_reader)?;
            let rp: ReqProgs = serde_json::from_value(v["prog_info"].clone())?;

            info!("prog info = {:?}", rp);

            let mut filter_meth_opt = None;
            let chem = match chemistry.as_str() {
                "10xv2" => Chemistry::TenxV2,
                "10xv3" => Chemistry::TenxV3,
                s => Chemistry::Other(s.to_string()),
            };

            // based on the filtering method
            if unfiltered_pl {
                // check the chemistry
                let pl_res = get_permit_if_absent(chem)?;
                let min_cells = 10usize;
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
                            "Cannot use unrecognized chemistry {} with unfiltered permit list.",
                            chemistry.as_str()
                        );
                    }
                }
            } else {
                match explicit_pl {
                    Some(filtered_path) => {
                        filter_meth_opt = Some(CellFilterMethod::ExplicitList(
                            filtered_path.to_string_lossy().into_owned(),
                        ));
                    }
                    None => {}
                };
                match forced_cells {
                    Some(num_forced) => {
                        filter_meth_opt = Some(CellFilterMethod::ForceCells(num_forced));
                    }
                    None => {}
                };
                match expect_cells {
                    Some(num_expected) => {
                        filter_meth_opt = Some(CellFilterMethod::ExpectCells(num_expected));
                    }
                    None => {}
                };
            }
            // otherwise it must have been knee;
            if knee {
                filter_meth_opt = Some(CellFilterMethod::KneeFinding);
            }

            if filter_meth_opt.is_none() {
                bail!("It seems no valid filtering strategy was provided!");
            }

            // here we must be safe to unwrap
            let filter_meth = filter_meth_opt.unwrap();

            let mut salmon_quant_cmd =
                std::process::Command::new(format!("{}", rp.salmon.unwrap().exe_path.display()));

            // set the input index and library type
            let index_path = format!("{}", index.display());
            salmon_quant_cmd
                .arg("alevin")
                .arg("--index")
                .arg(index_path)
                .arg("-l")
                .arg("A");

            // location of the reads
            let r1_str = reads1
                .iter()
                .map(|x| format!("{}", x.display()))
                .collect::<Vec<String>>()
                .join(",");
            let r2_str = reads2
                .iter()
                .map(|x| format!("{}", x.display()))
                .collect::<Vec<String>>()
                .join(",");
            salmon_quant_cmd.arg("-1").arg(r1_str).arg("-2").arg(r2_str);

            // location of outptu directory, number of threads
            let map_output = output.join("af_map");
            salmon_quant_cmd
                .arg("--threads")
                .arg(format!("{}", threads))
                .arg("-o")
                .arg(&map_output);
            salmon_quant_cmd.arg("--sketch");

            // setting the technology / chemistry
            match chemistry.as_str() {
                "10xv2" => {
                    salmon_quant_cmd.arg("--chromium");
                }
                "10xv3" => {
                    salmon_quant_cmd.arg("--chromiumV3");
                }
                s => {
                    salmon_quant_cmd.arg(format!("--{}", s));
                }
            };

            info!("cmd : {:?}", salmon_quant_cmd);
            let map_start = Instant::now();
            let map_proc_out = salmon_quant_cmd
                .output()
                .expect("failed to execute salmon alevin [mapping phase]");
            let map_duration = map_start.elapsed();

            if !map_proc_out.status.success() {
                bail!("mapping failed with exit status {:?}", map_proc_out.status);
            }

            let alevin_fry = rp.alevin_fry.unwrap().exe_path;
            // alevin-fry generate permit list
            let mut alevin_gpl_cmd =
                std::process::Command::new(format!("{}", &alevin_fry.display()));

            alevin_gpl_cmd.arg("generate-permit-list");
            alevin_gpl_cmd.arg("-i").arg(&map_output);
            alevin_gpl_cmd.arg("-d").arg("fw");

            // add the filter mode
            add_to_args(&filter_meth, &mut alevin_gpl_cmd);

            let gpl_output = output.join("af_quant");
            alevin_gpl_cmd.arg("-o").arg(&gpl_output);

            info!("cmd : {:?}", alevin_gpl_cmd);

            let gpl_start = Instant::now();
            let gpl_proc_out = alevin_gpl_cmd
                .output()
                .expect("could not execute [generate permit list]");
            let gpl_duration = gpl_start.elapsed();

            if !gpl_proc_out.status.success() {
                bail!(
                    "generate-permit-list failed with exit status {:?}",
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

            info!("cmd : {:?}", alevin_collate_cmd);
            let collate_start = Instant::now();
            let collate_proc_out = alevin_collate_cmd
                .output()
                .expect("could not execute [collate]");
            let collate_duration = collate_start.elapsed();

            if !collate_proc_out.status.success() {
                bail!(
                    "collate failed with exit status {:?}",
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
            alevin_quant_cmd.arg("-m").arg(t2g_map);
            alevin_quant_cmd.arg("-r").arg(resolution);

            info!("cmd : {:?}", alevin_quant_cmd);
            let quant_start = Instant::now();
            let quant_proc_out = alevin_quant_cmd
                .output()
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
                }
            });

            std::fs::write(
                &af_quant_info_file,
                serde_json::to_string_pretty(&af_quant_info).unwrap(),
            )
            .with_context(|| format!("could not write {}", af_quant_info_file.display()))?;
        }
    }
    Ok(())
}
