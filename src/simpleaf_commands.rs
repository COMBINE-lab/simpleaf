pub mod inspect;
pub use self::inspect::inspect_simpleaf;

pub mod chemistry;
pub use self::chemistry::add_chemistry;

pub mod paths;
pub use self::paths::set_paths;

pub mod indexing;
pub use self::indexing::build_ref_and_index;

pub mod quant;
pub use self::quant::map_and_quant;

pub mod workflow;
pub use self::workflow::{get_workflow_config, workflow};

use clap::{builder::ArgPredicate, ArgGroup, Subcommand};
use std::path::PathBuf;

/// The type of references we might create
/// to map against for quantification with
/// alevin-fry.
#[derive(Clone, Debug)]
pub enum ReferenceType {
    /// The spliced + intronic (splici) reference
    SplicedIntronic,
    /// The spliced + unspliced (splicu) reference
    SplicedUnspliced,
}

fn ref_type_parser(s: &str) -> Result<ReferenceType, String> {
    match s {
        "spliced+intronic" | "splici" => Ok(ReferenceType::SplicedIntronic),
        "spliced+unspliced" | "spliceu" => Ok(ReferenceType::SplicedUnspliced),
        t => Err(format!("Do not recognize reference type {}", t)),
    }
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// build the (expanded) reference index
    #[command(arg_required_else_help = true)]
    #[command(group(
             ArgGroup::new("reftype")
             .required(true)
             .args(["fasta", "ref_seq"])
    ))]
    Index {
        /// specify whether an expanded reference, spliced+intronic (or splici) or spliced+unspliced (or spliceu), should be built
        #[arg(long, help_heading="Expanded Reference Options", display_order = 1, default_value = "spliced+intronic", value_parser = ref_type_parser)]
        ref_type: ReferenceType,

        /// reference genome to be used for the expanded reference construction
        #[arg(short, long, help_heading="Expanded Reference Options", display_order = 2, 
              requires_ifs([
                (ArgPredicate::IsPresent, "gtf") 
              ]),
              conflicts_with = "ref_seq")]
        fasta: Option<PathBuf>,

        /// reference GTF file to be used for the expanded reference construction
        #[arg(
            short,
            long,
            help_heading = "Expanded Reference Options",
            display_order = 3,
            requires = "fasta",
            conflicts_with = "ref_seq"
        )]
        gtf: Option<PathBuf>,

        /// the target read length the splici index will be built for
        #[arg(
            short,
            long,
            help_heading = "Expanded Reference Options",
            display_order = 4,
            requires = "fasta",
            conflicts_with = "ref_seq"
        )]
        rlen: Option<u32>,

        /// deduplicate identical sequences in pyroe when building an expanded reference  reference
        #[arg(
            long = "dedup",
            help_heading = "Expanded Reference Options",
            display_order = 5,
            requires = "fasta",
            conflicts_with = "ref_seq"
        )]
        dedup: bool,

        /// target sequences (provide target sequences directly; avoid expanded reference construction)
        #[arg(long, alias = "refseq", help_heading = "Direct Reference Options", display_order = 6,
              conflicts_with_all = ["dedup", "unspliced", "spliced", "rlen", "gtf", "fasta"])]
        ref_seq: Option<PathBuf>,

        /// path to FASTA file with extra spliced sequence to add to the index
        #[arg(
            long,
            help_heading = "Expanded Reference Options",
            display_order = 7,
            requires = "fasta",
            conflicts_with = "ref_seq"
        )]
        spliced: Option<PathBuf>,

        /// path to FASTA file with extra unspliced sequence to add to the index
        #[arg(
            long,
            help_heading = "Expanded Reference Options",
            display_order = 8,
            requires = "fasta",
            conflicts_with = "ref_seq"
        )]
        unspliced: Option<PathBuf>,

        /// use piscem instead of salmon for indexing and mapping
        #[arg(long, help_heading = "Piscem Index Options", display_order = 1)]
        use_piscem: bool,

        /// the value of m to be used to construct the piscem index (must be < k)
        #[arg(
            short = 'm',
            long = "minimizer-length",
            default_value_t = 19,
            requires = "use_piscem",
            help_heading = "Piscem Index Options",
            display_order = 2
        )]
        minimizer_length: u32,

        /// path to output directory (will be created if it doesn't exist)
        #[arg(short, long, display_order = 1)]
        output: PathBuf,

        /// overwrite existing files if the output directory is already populated
        #[arg(long, display_order = 6)]
        overwrite: bool,

        /// number of threads to use when running
        #[arg(short, long, default_value_t = 16, display_order = 2)]
        threads: u32,

        /// the value of k to be used to construct the index
        #[arg(
            short = 'k',
            long = "kmer-length",
            default_value_t = 31,
            display_order = 3
        )]
        kmer_length: u32,

        /// keep duplicated identical sequences when constructing the index
        #[arg(long, display_order = 4)]
        keep_duplicates: bool,

        /// if this flag is passed, build the sparse rather than dense index for mapping
        #[arg(
            short = 'p',
            long = "sparse",
            conflicts_with = "use_piscem",
            display_order = 5
        )]
        sparse: bool,
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
        /// chemistry
        #[arg(short, long)]
        chemistry: String,

        /// output directory
        #[arg(short, long)]
        output: PathBuf,

        /// number of threads to use when running
        #[arg(short, long, default_value_t = 16)]
        threads: u32,

        /// path to index
        #[arg(
            short = 'i',
            long = "index",
            help_heading = "Mapping Options",
            requires_ifs([
                (ArgPredicate::IsPresent, "reads1"),
                (ArgPredicate::IsPresent, "reads2")
            ])
        )]
        index: Option<PathBuf>,

        /// comma-separated list of paths to read 1 files
        #[arg(
            short = '1',
            long = "reads1",
            help_heading = "Mapping Options",
            value_delimiter = ',',
            requires = "index",
            conflicts_with = "map_dir"
        )]
        reads1: Option<Vec<PathBuf>>,

        /// comma-separated list of paths to read 2 files
        #[arg(
            short = '2',
            long = "reads2",
            help_heading = "Mapping Options",
            value_delimiter = ',',
            requires = "index",
            conflicts_with = "map_dir"
        )]
        reads2: Option<Vec<PathBuf>>,

        /// use selective-alignment for mapping (instead of pseudoalignment with structural
        /// constraints).
        #[arg(short = 's', long, help_heading = "Mapping Options")]
        use_selective_alignment: bool,

        /// use piscem for mapping (requires that index points to the piscem index)
        #[arg(long, requires = "index", help_heading = "Mapping Options")]
        use_piscem: bool,

        /// path to a mapped output directory containing a RAD file to skip mapping
        #[arg(long = "map-dir", conflicts_with_all = ["index", "reads1", "reads2"], help_heading = "Mapping Options")]
        map_dir: Option<PathBuf>,

        /// use knee filtering mode
        #[arg(short, long, help_heading = "Permit List Generation Options")]
        knee: bool,

        /// use unfiltered permit list
        #[arg(short, long, help_heading = "Permit List Generation Options")]
        unfiltered_pl: Option<Option<PathBuf>>,

        /// use forced number of cells
        #[arg(short, long, help_heading = "Permit List Generation Options")]
        forced_cells: Option<usize>,

        /// use a filtered, explicit permit list
        #[arg(short = 'x', long, help_heading = "Permit List Generation Options")]
        explicit_pl: Option<PathBuf>,

        /// use expected number of cells
        #[arg(short, long, help_heading = "Permit List Generation Options")]
        expect_cells: Option<usize>,

        /// The expected direction/orientation of alignments in the chemistry being processed. If
        /// not provided, will default to `fw` for 10xv2/10xv3, otherwise `both`.
        #[arg(short = 'd', long, help_heading="Permit List Generation Options", value_parser = clap::builder::PossibleValuesParser::new(["fw", "rc", "both"]))]
        expected_ori: Option<String>,

        /// minimum read count threshold for a cell to be retained/processed; only used with --unfiltered-pl
        #[arg(
            long,
            help_heading = "Permit List Generation Options",
            default_value_t = 10
        )]
        min_reads: usize,

        /// transcript to gene map
        #[arg(short = 'm', long, help_heading = "UMI Resolution Options")]
        t2g_map: Option<PathBuf>,

        /// resolution mode
        #[arg(short, long, help_heading = "UMI Resolution Options", value_parser = clap::builder::PossibleValuesParser::new(["cr-like", "cr-like-em", "parsimony", "parsimony-em", "parsimony-gene", "parsimony-gene-em"]))]
        resolution: String,
    },
    /// set paths to the programs that simpleaf will use
    SetPaths {
        /// path to salmon to use
        #[arg(short, long)]
        salmon: Option<PathBuf>,
        /// path to piscem to use
        #[arg(short, long)]
        piscem: Option<PathBuf>,
        /// path to alein-fry to use
        #[arg(short, long)]
        alevin_fry: Option<PathBuf>,
        /// path to pyroe to use
        #[arg(short = 'r', long)]
        pyroe: Option<PathBuf>,
    },

    GetWorkflowConfig {
        /// path to output configuration file, the directory will be created if it doesn't exist
        #[arg(short, long)]
        output: PathBuf,

        /// name of the queried workflow.
        #[arg(short, long)]
        workflow: String,
        // only write the essential information without any instructions
        // #[arg(short, long)]
        // essential_only: bool,
    },

    #[command(group(
        ArgGroup::new("workflow file")
        .required(true)
        .args(["config_path", "workflow_path"])
        ))]
    /// parse the input configuration/workflow files and execute the corresponding workflow(s).
    Workflow {
        /// path to a simpleaf workflow configuration file.
        #[arg(short, long, display_order = 1, help_heading = "Workflow File")]
        config_path: Option<PathBuf>,

        /// path to a simpleaf complete workflow JSON file.
        #[arg(short, long, display_order = 2, help_heading = "Workflow File")]
        workflow_path: Option<PathBuf>,

        /// output directory for log files and the workflow outputs that have no explicit output directory.
        #[arg(short, long, display_order = 3)]
        output: PathBuf,

        /// return after parsing the wofklow config or JSON file without executing the commands.
        #[arg(short, long, display_order = 4, conflicts_with_all=["start_at", "resume"])]
        no_execution: bool,

        /// Start the execution from a specific step. All previous steps will be ignored.  
        #[arg(
            short,
            long,
            default_value_t = 1,
            display_order = 5,
            help_heading = "Start Step"
        )]
        start_at: i64,
        // TODO: add a --resume arg which reads the log at starts at the step that failed in the previous run
        /// resume execution from the termination step of a previous run. To use this flag, the output directory must contains the log file from a previous run.
        #[arg(
            short,
            long,
            conflicts_with = "start_at",
            display_order = 6,
            help_heading = "Start Step"
        )]
        resume: bool,

        /// comma separated library search paths when processing the (custom) workflow configuration file. (right-most wins)
        #[arg(
            short,
            long,
            conflicts_with = "workflow_path",
            display_order = 7,
            value_delimiter = ','
        )]
        lib_paths: Option<Vec<PathBuf>>,

        /// comma separated integers indicating which steps (commands) will be skipped during the execution.
        #[arg(
            long,
            conflicts_with = "workflow_path",
            display_order = 7,
            value_delimiter = ','
        )]
        skip_step: Option<Vec<i64>>,
    },
}
