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
pub use self::workflow::{
    get_wokflow, list_workflows, patch_manifest_or_template, refresh_protocol_estuary, run_workflow,
};

use clap::{builder::ArgPredicate, ArgGroup, Args, Subcommand};
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

        /// reference GTF/GFF3 file to be used for the expanded reference construction
        #[arg(
            short,
            long,
            help_heading = "Expanded Reference Options",
            display_order = 3,
            requires = "fasta",
            conflicts_with = "ref_seq"
        )]
        gtf: Option<PathBuf>,

        /// denotes that the input annotation is a GFF3 (instead of GTF) file
        #[arg(
            long,
            display_order = 4,
            requires = "fasta",
            conflicts_with = "ref_seq"
        )]
        gff3_format: bool,

        /// the target read length the splici index will be built for
        #[arg(
            short,
            long,
            help_heading = "Expanded Reference Options",
            display_order = 5,
            requires = "fasta",
            conflicts_with = "ref_seq",
            default_value_t = 91,
            hide_default_value = true
        )]
        rlen: i64,

        /// deduplicate identical sequences in roers when building an expanded reference  reference
        #[arg(
            long = "dedup",
            help_heading = "Expanded Reference Options",
            display_order = 6,
            requires = "fasta",
            conflicts_with = "ref_seq"
        )]
        dedup: bool,

        /// target sequences (provide target sequences directly; avoid expanded reference construction)
        #[arg(long, alias = "refseq", help_heading = "Direct Reference Options", display_order = 7,
              conflicts_with_all = ["dedup", "unspliced", "spliced", "rlen", "gtf", "fasta"])]
        ref_seq: Option<PathBuf>,

        /// path to FASTA file with extra spliced sequence to add to the index
        #[arg(
            long,
            help_heading = "Expanded Reference Options",
            display_order = 8,
            requires = "fasta",
            conflicts_with = "ref_seq"
        )]
        spliced: Option<PathBuf>,

        /// path to FASTA file with extra unspliced sequence to add to the index
        #[arg(
            long,
            help_heading = "Expanded Reference Options",
            display_order = 9,
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
            .args(["expect_cells", "explicit_pl", "forced_cells", "knee", "unfiltered_pl"])
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
    },
    /// simpleaf workflow related command set
    Workflow(WorkflowArgs),
}

#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct WorkflowArgs {
    #[command(subcommand)]
    pub command: WorkflowCommands,
}

#[derive(Debug, Subcommand)]
pub enum WorkflowCommands {
    /// Print a summary of the currently available workflows.
    List {},

    /// Update the local copy of protocol esturary to the latest version.
    Refresh {},

    #[command(arg_required_else_help = true)]
    /// Get the workflow template and related files of a registered workflow.
    Get {
        /// path to dump the folder containing the workflow related files.
        #[arg(short, long, requires = "name")]
        output: PathBuf,

        /// name of the queried workflow.
        #[arg(short, long)]
        name: String,
        // only write the essential information without any instructions
        // #[arg(short, long)]
        // essential_only: bool,
    },

    #[command(arg_required_else_help = true)]
    #[command(group(
        clap::ArgGroup::new("source")
        .required(true)
        .args(&["manifest", "template"]),
    ))]
    /// Patch a workflow template or instantiated manifest with a subset of parameters
    /// to produce a series of workflow manifests.
    Patch {
        /// fully-instantiated manifest (JSON file) to patch. If this argument
        /// is given, the patch is applied directly to the JSON file in a manner
        /// akin to simple key-value replacement. Since the manifest is
        /// fully-instantiated, no derived values will be affected.
        #[arg(short, long)]
        manifest: Option<PathBuf>,
        /// partially-instantiated template (JSONNET file) to patch. If this
        /// argument is given, the patch is applied *before* the template is
        /// instantiated (i.e. if you override a variable used elswhere in
        /// the template, all derived values will be affected).
        #[arg(short, long)]
        template: Option<PathBuf>,
        /// patch to apply as a ';' separated parameter table with headers
        /// declared as specified in the documentation.
        #[arg(short, long)]
        patch: PathBuf,
        /// output directory where the patched manifest files (i.e. the output 
        /// of applying the patching procedure) should be stored. If no directory
        /// is provided, the patched manifests are stored in the same location
        /// as the input template or manifest to which patching is applied.
        #[arg(short, long)]
        output: Option<PathBuf>
    },

    #[command(arg_required_else_help = true)]
    #[command(group(
        clap::ArgGroup::new("source")
        .required(true)
        .args(&["manifest", "template"]),
    ))]
    /// Parse and instantiate a workflow template and invoke the workflow commands, or run an
    /// instantiated manifest directly.
    Run {
        /// path to an instantiated simpleaf workflow template.
        #[arg(short, long, display_order = 1)]
        template: Option<PathBuf>,

        /// output directory for log files and the workflow outputs that have no explicit output directory.
        // NOTE @DongzeHe  --- per our discussion, we should make the output paramter
        // here optional, and derive it from the template or manifest if it is not provided.
        #[arg(short, long, display_order = 2)]
        output: Option<PathBuf>,

        /// return after instantiating the template (JSONNET file) into a manifest (JSON foramt) without actually executing
        /// the resulting manifest.
        #[arg(short,
            long,
            display_order = 3,
            conflicts_with_all=["start_at", "resume", "skip_step"],
            help_heading = "Control Flow"
        )]
        no_execution: bool,

        /// path to an instantiated simpleaf workflow template.
        #[arg(
            short,
            long,
            display_order = 4,
            conflicts_with_all=["template", "output", "no_execution", "jpaths", "ext_codes"]
        )]
        manifest: Option<PathBuf>,

        /// Start the execution from a specific Step. All previous steps will be ignored.  
        #[arg(
            short,
            long,
            default_value_t = 1,
            display_order = 5,
            conflicts_with_all=["resume"],
            help_heading = "Control Flow"
        )]
        start_at: u64,

        /// resume execution from the termination step of a previous run.
        /// To use this flag, the output directory must contains the JSON file generated from a previous run.
        #[arg(
            short,
            long,
            conflicts_with = "start_at",
            display_order = 6,
            conflicts_with_all=["start_at"],
            help_heading = "Control Flow",
        )]
        resume: bool,

        /// comma separated library search paths passing to internal Jsonnet engine as --jpath flags.
        #[arg(
            short,
            long,
            display_order = 7,
            value_delimiter = ',',
            help_heading = "Jsonnet"
        )]
        jpaths: Option<Vec<PathBuf>>,

        /// comma separated string passing to internal Jsonnet engine as --ext-code flags.
        #[arg(
            short,
            long,
            display_order = 8,
            value_delimiter = ',',
            help_heading = "Jsonnet",
            hide = true
        )]
        ext_codes: Option<Vec<String>>,

        /// comma separated integers indicating which steps (commands) will be skipped during the execution.
        #[arg(
            long,
            display_order = 9,
            value_delimiter = ',',
            help_heading = "Control Flow"
        )]
        skip_step: Option<Vec<u64>>,
    },
}
