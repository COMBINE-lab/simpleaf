extern crate env_logger;
#[macro_use]
extern crate log;

use anyhow::{bail, Context};
use clap::{builder::ArgPredicate, ArgGroup, Parser, Subcommand};
use cmd_lib::run_fun;
use env_logger::Env;
use serde_json::json;
use time::{Duration, Instant};

use std::io::BufReader;
use std::io::Write;
use std::io::{Seek, SeekFrom};
use std::path::PathBuf;
use std::{env, fs};

mod utils;
use utils::af_utils::*;
use utils::prog_utils::*;

#[derive(Debug, Subcommand)]
enum Commands {
    /// build the splici index
    #[command(arg_required_else_help = true)]
    #[command(group(
             ArgGroup::new("reftype")
             .required(true)
             .args(["fasta", "refseq"])
    ))]
    Index {
        /// reference genome to be used for splici construction
        #[arg(short, long, help_heading = "splici-ref", display_order = 1, 
              requires_ifs([
                (ArgPredicate::IsPresent, "gtf"), 
                (ArgPredicate::IsPresent, "rlen")
              ]),
              conflicts_with = "refseq")]
        fasta: Option<PathBuf>,

        /// reference GTF file
        #[arg(
            short,
            long,
            help_heading = "splici-ref",
            display_order = 2,
            requires = "fasta",
            conflicts_with = "refseq"
        )]
        gtf: Option<PathBuf>,

        /// the target read length the index will be built for
        #[arg(
            short,
            long,
            help_heading = "splici-ref",
            display_order = 3,
            requires = "fasta",
            conflicts_with = "refseq"
        )]
        rlen: Option<u32>,

        /// path to FASTA file with extra spliced sequence to add to the index
        #[arg(
            short,
            long,
            help_heading = "splici-ref",
            display_order = 4,
            requires = "fasta",
            conflicts_with = "refseq"
        )]
        spliced: Option<PathBuf>,

        /// path to FASTA file with extra unspliced sequence to add to the index
        #[arg(
            short,
            long,
            help_heading = "splici-ref",
            display_order = 5,
            requires = "fasta",
            conflicts_with = "refseq"
        )]
        unspliced: Option<PathBuf>,

        /// deduplicate identical sequences inside the R script when building the splici reference
        #[arg(
            short = 'd',
            long = "dedup",
            help_heading = "splici-ref",
            display_order = 6,
            requires = "fasta",
            conflicts_with = "refseq"
        )]
        dedup: bool,

        /// target sequences (provide target sequences directly; avoid splici construction)
        #[arg(long, help_heading = "direct-ref", display_order = 7,
              conflicts_with_all = ["dedup", "unspliced", "spliced", "rlen", "gtf", "fasta"])]
        refseq: Option<PathBuf>,

        /// path to output directory (will be created if it doesn't exist)
        #[arg(short, long, display_order = 8)]
        output: PathBuf,

        /// the value of k that should be used to construct the index
        #[arg(short = 'k', long = "kmer-length", default_value_t = 31)]
        kmer_length: u32,

        /// if this flag is passed, build the sparse rather than dense index for mapping
        #[arg(short = 'p', long = "sparse")]
        sparse: bool,

        /// number of threads to use when running
        #[arg(short, long, default_value_t = 16)]
        threads: u32,
    },
    /// add a new custom chemistry to geometry mapping
    #[command(arg_required_else_help = true)]
    AddChemistry {
        /// the name to give the chemistry
        #[arg(short, long)]
        name: String,
        /// the geometry to which the chemistry maps
        #[arg(short, long)]
        geometry: String,
    },
    /// inspect the current configuration
    Inspect {},
    /// quantify a sample
    #[command(arg_required_else_help = true)]
    #[command(group(
            ArgGroup::new("filter")
            .required(true)
            .args(["knee", "unfiltered_pl", "forced_cells", "expect_cells"])
            ))]
    #[command(group(
            ArgGroup::new("input-type")
            .required(true)
            .args(["index", "map_dir"])
            ))]
    Quant {
        /// path to index
        #[arg(
            short = 'i',
            long = "index",
            help_heading = "mapping options",
            requires_ifs([
                (ArgPredicate::IsPresent, "reads1"),
                (ArgPredicate::IsPresent, "reads2")
            ])
        )]
        index: Option<PathBuf>,

        /// path to a mapped output directory containing a RAD file to be quantified
        #[arg(long = "map-dir", conflicts_with_all = ["index", "reads1", "reads2"], help_heading = "mapping options")]
        map_dir: Option<PathBuf>,

        /// path to read 1 files
        #[arg(
            short = '1',
            long = "reads1",
            help_heading = "mapping options",
            value_delimiter = ',',
            requires = "index",
            conflicts_with = "map_dir"
        )]
        reads1: Option<Vec<PathBuf>>,

        /// path to read 2 files
        #[arg(
            short = '2',
            long = "reads2",
            help_heading = "mapping options",
            value_delimiter = ',',
            requires = "index",
            conflicts_with = "map_dir"
        )]
        reads2: Option<Vec<PathBuf>>,

        /// number of threads to use when running
        #[arg(short, long, default_value_t = 16)]
        threads: u32,

        /// use selective-alignment for mapping (instead of pseudoalignment with structural
        /// constraints).
        #[arg(short = 's', long, help_heading = "mapping options")]
        use_selective_alignment: bool,

        /// The expected direction/orientation of alignments in the chemistry being processed. If
        /// not provided, will default to `fw` for 10xv2/10xv3, otherwise `both`.
        #[arg(short = 'd', long, help_heading="permit list generation options", value_parser = clap::builder::PossibleValuesParser::new(["fw", "rc", "both"]))]
        expected_ori: Option<String>,

        /// use knee filtering mode
        #[arg(short, long, help_heading = "permit list generation options")]
        knee: bool,

        /// use unfiltered permit list
        #[arg(short, long, help_heading = "permit list generation options")]
        unfiltered_pl: Option<Option<PathBuf>>,

        /// use a filtered, explicit permit list
        #[arg(short = 'x', long, help_heading = "permit list generation options")]
        explicit_pl: Option<PathBuf>,

        /// use forced number of cells
        #[arg(short, long, help_heading = "permit list generation options")]
        forced_cells: Option<usize>,

        /// use expected number of cells
        #[arg(short, long, help_heading = "permit list generation options")]
        expect_cells: Option<usize>,

        /// minimum read count threshold for a cell to be retained/processed; only used with --unfiltered-pl
        #[arg(
            long,
            help_heading = "permit list generation options",
            default_value_t = 10
        )]
        min_reads: usize,

        /// resolution mode
        #[arg(short, long, help_heading = "UMI resolution options", value_parser = clap::builder::PossibleValuesParser::new(["cr-like", "cr-like-em", "parsimony", "parsimony-em", "parsimony-gene", "parsimony-gene-em"]))]
        resolution: String,

        /// chemistry
        #[arg(short, long)]
        chemistry: String,

        /// transcript to gene map
        #[arg(short = 'm', long, help_heading = "UMI resolution options")]
        t2g_map: PathBuf,

        /// output directory
        #[arg(short, long)]
        output: PathBuf,
    },
    /// set paths to the programs that simpleaf will use
    SetPaths {
        /// path to salmon to use
        #[arg(short, long)]
        salmon: Option<PathBuf>,
        /// path to alein-fry to use
        #[arg(short, long)]
        alevin_fry: Option<PathBuf>,
        /// path to pyroe to use
        #[arg(short, long)]
        pyroe: Option<PathBuf>,
    },
}

/// simplifying alevin-fry workflows
#[derive(Debug, Parser)]
#[command(author, version, about)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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
        // set the paths where the relevant tools live
        Commands::SetPaths {
            salmon,
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

        Commands::AddChemistry { name, geometry } => {
            // check geometry string, if no good then
            // propagate error.
            let _cg = extract_geometry(&geometry)?;

            // do we have a custom chemistry file
            let custom_chem_p = af_home_path.join("custom_chemistries.json");

            let mut custom_chem_file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(&custom_chem_p)
                .with_context({
                    || {
                        format!(
                            "couldn't open the custom chemistry file {}",
                            custom_chem_p.display()
                        )
                    }
                })?;

            let custom_chem_reader = BufReader::new(&custom_chem_file);
            let mut v: serde_json::Value = match serde_json::from_reader(custom_chem_reader) {
                Ok(sv) => sv,
                Err(_) => {
                    // the file was empty so here return an empty json object
                    json!({})
                }
            };

            if let Some(g) = v.get_mut(&name) {
                let gs = g.as_str().unwrap();
                info!("chemistry {} already existed, with geometry {}; overwriting geometry specification", name, gs);
                *g = json!(geometry);
            } else {
                info!("inserting chemistry {} with geometry {}", name, geometry);
                v[name] = json!(geometry);
            }

            custom_chem_file.set_len(0)?;
            custom_chem_file.seek(SeekFrom::Start(0))?;

            custom_chem_file
                .write_all(serde_json::to_string_pretty(&v).unwrap().as_bytes())
                .with_context(|| format!("could not write {}", custom_chem_p.display()))?;
        }

        Commands::Inspect {} => {
            let af_info_p = af_home_path.join("simpleaf_info.json");
            let simpleaf_info_file = std::fs::File::open(&af_info_p).with_context({
                || {
                    format!(
                        "Could not open file {}; please run the set-paths command",
                        af_info_p.display()
                    )
                }
            })?;

            let simpleaf_info_reader = BufReader::new(simpleaf_info_file);

            // Read the JSON contents of the file as an instance of `User`.
            let v: serde_json::Value = serde_json::from_reader(simpleaf_info_reader)?;
            println!(
                "\n----- simpleaf info -----\n{}",
                serde_json::to_string_pretty(&v).unwrap()
            );

            // do we have a custom chemistry file
            let custom_chem_p = af_home_path.join("custom_chemistries.json");
            if custom_chem_p.is_file() {
                println!(
                    "\nCustom chemistries exist at path: {}\n----- custom chemistries -----\n",
                    custom_chem_p.display()
                );
                // parse the custom chemistry json file
                let custom_chem_file = std::fs::File::open(&custom_chem_p).with_context({
                    || {
                        format!(
                            "couldn't open the custom chemistry file {}",
                            custom_chem_p.display()
                        )
                    }
                })?;
                let custom_chem_reader = BufReader::new(custom_chem_file);
                let v: serde_json::Value = serde_json::from_reader(custom_chem_reader)?;
                println!("{}", serde_json::to_string_pretty(&v).unwrap());
            }
        }
        // if we are building the reference and indexing
        Commands::Index {
            fasta,
            gtf,
            rlen,
            spliced,
            unspliced,
            dedup,
            refseq,
            output,
            kmer_length,
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

            // Read the JSON contents of the file
            let v: serde_json::Value = serde_json::from_reader(simpleaf_info_reader)?;
            let rp: ReqProgs = serde_json::from_value(v["prog_info"].clone())?;

            let info_file = output.join("index_info.json");
            let mut index_info = json!({
                "command" : "index",
                "version_info" : rp,
                "args" : {
                    "output" : output,
                    "sparse" : sparse,
                    "threads" : threads
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

            // if we are generating a splici reference
            if let (Some(fasta), Some(gtf), Some(rlen)) = (fasta, gtf, rlen) {
                let ref_file = format!("splici_fl{}.fa", rlen - 5);
                let outref = output.join("ref");
                run_fun!(mkdir -p $outref)?;

                let t2g_file = outref.join(format!("splici_fl{}_t2g_3col.tsv", rlen - 5));

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

                let mut cmd =
                    std::process::Command::new(format!("{}", rp.pyroe.unwrap().exe_path.display()));
                // we will run the make-splici command
                cmd.arg("make-splici");

                // if the user wants to dedup output sequences
                if dedup {
                    cmd.arg(String::from("--dedup-seqs"));
                }

                // extra spliced sequence
                if let Some(es) = spliced {
                    cmd.arg(String::from("--extra-spliced"));
                    cmd.arg(format!("{}", es.display()));
                }

                // extra unspliced sequence
                if let Some(eu) = unspliced {
                    cmd.arg(String::from("--extra-unspliced"));
                    cmd.arg(format!("{}", eu.display()));
                }

                cmd.arg(fasta)
                    .arg(gtf)
                    .arg(format!("{}", rlen))
                    .arg(&outref);

                let pyroe_start = Instant::now();
                let cres = cmd.output()?;
                pyroe_duration = Some(pyroe_start.elapsed());

                if !cres.status.success() {
                    bail!("pyroe failed to return succesfully {:?}", cres.status);
                }

                reference_sequence = Some(outref.join(ref_file));
            } else {
                // we are running on a set of references directly

                // in this path (due to the argument parser requiring
                // either --fasta or --refseq, refseq should be safe to
                // unwrap).
                index_info["args"]["refseq"] = json!(refseq.clone().unwrap());

                std::fs::write(
                    &info_file,
                    serde_json::to_string_pretty(&index_info).unwrap(),
                )
                .with_context(|| format!("could not write {}", info_file.display()))?;

                reference_sequence = refseq;
            }

            let mut salmon_index_cmd =
                std::process::Command::new(format!("{}", rp.salmon.unwrap().exe_path.display()));
            let ref_seq = reference_sequence.expect(
                "reference sequence should either be generated from --fasta by make-splici or set with --refseq",
            );

            let output_index_dir = output.join("index");
            salmon_index_cmd
                .arg("index")
                .arg("-k")
                .arg(kmer_length.to_string())
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
            if let Some(t2g_file) = splici_t2g {
                let index_t2g_path = output_index_dir.join("t2g_3col.tsv");
                std::fs::copy(t2g_file, index_t2g_path)?;
            }

            let index_log_file = output.join("simpleaf_index_log.json");
            let index_log_info = if let Some(pyroe_duration) = pyroe_duration {
                // if we ran make-splici
                json!({
                    "time_info" : {
                        "pyroe_time" : pyroe_duration,
                        "index_time" : index_duration
                    }
                })
            } else {
                // if we indexed provided sequences directly
                json!({
                    "time_info" : {
                        "index_time" : index_duration
                    }
                })
            };

            std::fs::write(
                &index_log_file,
                serde_json::to_string_pretty(&index_log_info).unwrap(),
            )
            .with_context(|| format!("could not write {}", index_log_file.display()))?;
        }

        // if we are running mapping and quantification
        Commands::Quant {
            index,
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
                        let v: serde_json::Value = serde_json::from_reader(custom_chem_reader)?;
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
                    let pl_res = get_permit_if_absent(&af_home_path, &chem)?;
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
                                    "Cannot automatically obtain an unfiltered permit list for non-Chromium chemistry: {}.",
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
                bail!("It seems no valid filtering strategy was provided!");
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

            let map_output: PathBuf;
            let map_duration: Duration;

            // if we are mapping against an index
            if let Some(index) = index {
                let mut salmon_quant_cmd = std::process::Command::new(format!(
                    "{}",
                    rp.salmon.unwrap().exe_path.display()
                ));

                // set the input index and library type
                let index_path = format!("{}", index.display());
                salmon_quant_cmd
                    .arg("alevin")
                    .arg("--index")
                    .arg(index_path)
                    .arg("-l")
                    .arg("A");

                let reads1 = reads1.expect(
                    "since mapping against an index is requested, read1 files must be provded.",
                );
                let reads2 = reads2.expect(
                    "since mapping against an index is requested, read2 files must be provded.",
                );
                // location of the reads
                // note: salmon uses space so separate
                // these, not commas, so build the proper
                // strings here.
                assert_eq!(reads1.len(), reads2.len());

                salmon_quant_cmd.arg("-1");
                for rf in &reads1 {
                    salmon_quant_cmd.arg(rf);
                }
                salmon_quant_cmd.arg("-2");
                for rf in &reads2 {
                    salmon_quant_cmd.arg(rf);
                }

                // location of outptu directory, number of threads
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

                // setting the technology / chemistry
                add_chemistry_to_args(chem.as_str(), &mut salmon_quant_cmd)?;

                info!("cmd : {:?}", salmon_quant_cmd);
                let map_start = Instant::now();
                let map_proc_out = salmon_quant_cmd
                    .output()
                    .expect("failed to execute salmon alevin [mapping phase]");
                map_duration = map_start.elapsed();

                if !map_proc_out.status.success() {
                    bail!("mapping failed with exit status {:?}", map_proc_out.status);
                }
            } else {
                map_output = map_dir
                    .expect("map-dir must be provided, since index, read1 and read2 were not.");
                map_duration = Duration::new(0, 0);
            }

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

            // write the relevant info about
            // our run to file.
            std::fs::write(
                &af_quant_info_file,
                serde_json::to_string_pretty(&af_quant_info).unwrap(),
            )
            .with_context(|| format!("could not write {}", af_quant_info_file.display()))?;
        }
    }
    // success, yay!
    Ok(())
}
