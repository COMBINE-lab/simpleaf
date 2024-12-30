pub mod inspect;
pub use self::inspect::inspect_simpleaf;

pub mod refresh;
pub use self::refresh::refresh_prog_info;

pub mod chemistry;

pub mod paths;
pub use self::paths::set_paths;

pub mod indexing;
pub use self::indexing::build_ref_and_index;

pub mod quant;
pub use self::quant::map_and_quant;

pub mod workflow;
pub use self::workflow::{
    get_workflow, list_workflows, patch_manifest_or_template, refresh_protocol_estuary,
    run_workflow,
};

pub use crate::atac::commands::AtacCommand;
pub use crate::defaults::{DefaultMappingParams, DefaultParams};

use clap::{builder::ArgPredicate, ArgAction, ArgGroup, Args, Subcommand};
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

#[derive(Args, Clone, Debug)]
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
pub struct MapQuantOpts {
    /// The name of a registered chemistry or a quoted string representing a custom geometry specification.
    #[arg(short, long)]
    pub chemistry: String,

    /// Path to the output directory
    #[arg(short, long)]
    pub output: PathBuf,

    /// Number of threads to use when running
    #[arg(short, long, default_value_t = 16)]
    pub threads: u32,

    /// Path to a folder containing the index files
    #[arg(
        short = 'i',
        long = "index",
        help_heading = "Mapping Options",
        requires_ifs([
            (ArgPredicate::IsPresent, "reads1"),
            (ArgPredicate::IsPresent, "reads2")
        ])
    )]
    pub index: Option<PathBuf>,

    /// Comma-separated list of paths to read 1 files. The order must match the read 2 files.
    #[arg(
        short = '1',
        long = "reads1",
        help_heading = "Mapping Options",
        value_delimiter = ',',
        requires = "index",
        conflicts_with = "map_dir"
    )]
    pub reads1: Option<Vec<PathBuf>>,

    /// Comma-separated list of paths to read 2 files. The order must match the read 1 files.
    #[arg(
        short = '2',
        long = "reads2",
        help_heading = "Mapping Options",
        value_delimiter = ',',
        requires = "index",
        conflicts_with = "map_dir"
    )]
    pub reads2: Option<Vec<PathBuf>>,

    // It's currently very confusing to have both `--foo` and
    // `--no-foo` fields in derive mode with `--foo` as the default.
    // The following hack was taken from: https://jwodder.github.io/kbits/posts/clap-bool-negate
    // NOTE: tracking issue https://github.com/clap-rs/clap/issues/815 to clean this
    // up when it's fixed.
    // NOTE: yes, the field names and option names are swapped below, because that's
    // what's required to make this work ...
    /// Don't use the default piscem mapper, instead, use salmon-alevin
    #[arg(long="no-piscem", requires = "index", help_heading = "Mapping Options", action = ArgAction::SetFalse)]
    pub use_piscem: bool,

    /// Use piscem for mapping (requires that index points to the piscem index)
    #[arg(
        long = "use-piscem",
        requires = "index",
        help_heading = "Mapping Options",
        overrides_with = "use_piscem"
    )]
    pub _no_piscem: bool,

    // NOTE: Because of the reversal of `use_piscem` and `_no_piscem` in the parser
    // due to the parsing quirk, the *meaning* of `conflicts_with = "use_piscem"`
    // below is actually that it conflicts with the option `--no-piscem` being passed.
    /// Use selective-alignment for mapping (only if using salmon alevin
    /// as the underlying mapper).
    #[arg(
        short = 's',
        long,
        help_heading = "Mapping Options",
        requires = "use_piscem"
    )]
    pub use_selective_alignment: bool,

    /// If piscem >= 0.7.0, enable structural constraints
    #[arg(
        long,
        help_heading = "Piscem Mapping Options",
        conflicts_with = "use_piscem"
    )]
    pub struct_constraints: bool,

    /// Skip checking of the equivalence classes of k-mers that were too ambiguous to be otherwise
    /// considered (passing this flag can speed up mapping slightly, but may reduce specificity)
    #[arg(
        long,
        conflicts_with = "max_ec_card",
        help_heading = "Piscem Mapping Options",
        conflicts_with = "use_piscem"
    )]
    pub ignore_ambig_hits: bool,

    /// Do not consider poison k-mers, even if the underlying index contains them. In this case,
    /// the mapping results will be identical to those obtained as if no poison table was added to
    /// the index.
    #[arg(
        long,
        help_heading = "Piscem Mapping Options",
        conflicts_with = "use_piscem"
    )]
    pub no_poison: bool,

    /// The skipping strategy to use for k-mer collection
    #[arg(long,
        default_value = &DefaultParams::SKIPPING_STRATEGY,
        value_parser = clap::builder::PossibleValuesParser::new(["permissive", "strict"]), 
        help_heading = "Piscem Mapping Options",
        conflicts_with = "use_piscem")]
    pub skipping_strategy: String,

    /// Determines the maximum cardinality equivalence class
    /// (number of (txp, orientation status) pairs) to examine (cannot be used with
    /// --ignore-ambig-hits).
    #[arg(
        long,
        default_value_t = DefaultParams::MAX_EC_CARD,
        conflicts_with = "ignore_ambig_hits",
        help_heading = "Piscem Mapping Options",
        conflicts_with = "use_piscem")]
    pub max_ec_card: u32,

    /// In the first pass, consider only collected and matched k-mers of a read having <= --max-hit-occ hits.
    #[arg(long,
        default_value_t = DefaultParams::MAX_HIT_OCC,
        help_heading = "Piscem Mapping Options",
        conflicts_with = "use_piscem")]
    pub max_hit_occ: u32,

    /// If all collected and matched k-mers of a read have > --max-hit-occ hits, then make a second pass and consider k-mers
    /// having <= --max-hit-occ-recover hits.
    #[arg(long,
        default_value_t = DefaultParams::MAX_HIT_OCC_RECOVER,
        help_heading = "Piscem Mapping Options",
        conflicts_with = "use_piscem")]
    pub max_hit_occ_recover: u32,

    /// Threshold for discarding reads with too many  mappings
    #[arg(long,
        default_value_t = DefaultParams::MAX_READ_OCC,
        help_heading = "Piscem Mapping Options",
        conflicts_with = "use_piscem")]
    pub max_read_occ: u32,

    /// Path to a mapped output directory containing a RAD file to skip mapping
    #[arg(long = "map-dir", conflicts_with_all = ["index", "reads1", "reads2"], help_heading = "Mapping Options")]
    pub map_dir: Option<PathBuf>,

    /// Use knee filtering mode
    #[arg(short, long, help_heading = "Permit List Generation Options")]
    pub knee: bool,

    /// Use unfiltered permit list
    #[arg(short, long, help_heading = "Permit List Generation Options")]
    pub unfiltered_pl: Option<Option<PathBuf>>,

    /// Use forced number of cells
    #[arg(short, long, help_heading = "Permit List Generation Options")]
    pub forced_cells: Option<usize>,

    /// Use a filtered, explicit permit list
    #[arg(short = 'x', long, help_heading = "Permit List Generation Options")]
    pub explicit_pl: Option<PathBuf>,

    /// Use expected number of cells
    #[arg(short, long, help_heading = "Permit List Generation Options")]
    pub expect_cells: Option<usize>,

    /// The expected direction/orientation of alignments in the chemistry being processed. If
    /// not provided, will default to `fw` for 10xv2/10xv3, otherwise `both`.
    #[arg(short = 'd', long, help_heading="Permit List Generation Options", value_parser = clap::builder::PossibleValuesParser::new(["fw", "rc", "both"]))]
    pub expected_ori: Option<String>,

    /// Minimum read count threshold for a cell to be retained/processed; only use with --unfiltered-pl
    #[arg(
        long,
        help_heading = "Permit List Generation Options",
        default_value_t = 10
    )]
    pub min_reads: usize,

    /// Path to a transcript to gene map file
    #[arg(short = 'm', long, help_heading = "UMI Resolution Options")]
    pub t2g_map: Option<PathBuf>,

    /// UMI resolution mode
    #[arg(short, long, help_heading = "UMI Resolution Options", value_parser = clap::builder::PossibleValuesParser::new(["cr-like", "cr-like-em", "parsimony", "parsimony-em", "parsimony-gene", "parsimony-gene-em"]))]
    pub resolution: String,
}

#[derive(Args, Clone, Debug)]
#[command(arg_required_else_help = true)]
#[command(group(
        ArgGroup::new("reftype")
        .required(true)
        .args(["fasta", "ref_seq"])
))]
pub struct IndexOpts {
    /// Specify whether an expanded reference, spliced+intronic (or splici) or spliced+unspliced (or spliceu), should be built
    #[arg(long, help_heading="Expanded Reference Options", display_order = 1, default_value = "spliced+intronic", value_parser = ref_type_parser)]
    pub ref_type: ReferenceType,

    /// Path to a reference genome to be used for the expanded reference construction
    #[arg(short, long, help_heading="Expanded Reference Options", display_order = 2, 
              requires_ifs([
                (ArgPredicate::IsPresent, "gtf") 
              ]),
              conflicts_with = "ref_seq")]
    pub fasta: Option<PathBuf>,

    /// Path to a reference GTF/GFF3 file to be used for the expanded reference construction
    #[arg(
        short,
        long,
        help_heading = "Expanded Reference Options",
        display_order = 3,
        requires = "fasta",
        conflicts_with = "ref_seq"
    )]
    pub gtf: Option<PathBuf>,

    /// Denotes that the input annotation is a GFF3 (instead of GTF) file
    #[arg(
        long,
        display_order = 4,
        requires = "fasta",
        conflicts_with = "ref_seq"
    )]
    pub gff3_format: bool,

    /// The Read length used in roers to add flanking lengths to intronic sequences
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
    pub rlen: i64,

    /// Deduplicate identical sequences in roers when building the expanded reference
    #[arg(
        long = "dedup",
        help_heading = "Expanded Reference Options",
        display_order = 6,
        requires = "fasta",
        conflicts_with = "ref_seq"
    )]
    pub dedup: bool,

    /// Reference sequences to directly build index on, and avoid expanded reference construction
    #[arg(long, alias = "refseq", help_heading = "Direct Reference Options", display_order = 7,
              conflicts_with_all = ["dedup", "unspliced", "spliced", "rlen", "gtf", "fasta"])]
    pub ref_seq: Option<PathBuf>,

    /// Path to FASTA file with extra spliced sequence to add to the index
    #[arg(
        long,
        help_heading = "Expanded Reference Options",
        display_order = 8,
        requires = "fasta",
        conflicts_with = "ref_seq"
    )]
    pub spliced: Option<PathBuf>,

    /// Path to a FASTA file with extra unspliced sequence to add to the index
    #[arg(
        long,
        help_heading = "Expanded Reference Options",
        display_order = 9,
        requires = "fasta",
        conflicts_with = "ref_seq"
    )]
    pub unspliced: Option<PathBuf>,

    // It's currently very confusing to have both `--foo` and
    // `--no-foo` fields in derive mode with `--foo` as the default.
    // The following hack was taken from: https://jwodder.github.io/kbits/posts/clap-bool-negate
    // NOTE: tracking issue https://github.com/clap-rs/clap/issues/815 to clean this
    // up when it's fixed.
    // NOTE: yes, the field names and option names are swapped below, because that's
    // what's required to make this work ...
    /// Use piscem instead of salmon for indexing and mapping (default)
    #[arg(
        long = "use-piscem",
        help_heading = "Piscem Index Options",
        overrides_with = "use_piscem"
    )]
    pub _no_piscem: bool,

    /// Don't use the default piscem mapper, instead, use salmon-alevin
    #[arg(long="no-piscem", help_heading = "Alternative salmon-alevin Index Options", action = ArgAction::SetFalse)]
    pub use_piscem: bool,

    /// Minimizer length to be used to construct the piscem index (must be < k)
    #[arg(
        short = 'm',
        long = "minimizer-length",
        default_value_t = 19,
        conflicts_with = "use_piscem",
        help_heading = "Piscem Index Options",
        display_order = 2
    )]
    pub minimizer_length: u32,

    /// Paths to decoy sequence FASTA files used to insert poison
    /// k-mer information into the index (only if using piscem >= 0.7).
    #[arg(
        long,
        conflicts_with = "use_piscem",
        help_heading = "Piscem Index Options",
        value_delimiter = ',',
        display_order = 3
    )]
    pub decoy_paths: Option<Vec<PathBuf>>,

    /// The seed value to use in SSHash index construction
    /// (try changing this in the rare event index build fails).
    #[arg(
        long = "seed",
        conflicts_with = "use_piscem",
        help_heading = "Piscem Index Options",
        default_value_t = 1,
        display_order = 4
    )]
    pub hash_seed: u64,

    /// The working directory where temporary files should be placed
    #[arg(
        long = "work-dir",
        conflicts_with = "use_piscem",
        help_heading = "Piscem Index Options",
        default_value = "./workdir.noindex",
        display_order = 5
    )]
    pub work_dir: PathBuf,

    /// Path to output directory (will be created if it doesn't exist)
    #[arg(short, long, display_order = 1)]
    pub output: PathBuf,

    /// Overwrite existing files if the output directory is already populated
    #[arg(long, display_order = 6)]
    pub overwrite: bool,

    /// Number of threads to use when running
    #[arg(short, long, default_value_t = 16, display_order = 2)]
    pub threads: u32,

    /// The value of k to be used to construct the index
    #[arg(
        short = 'k',
        long = "kmer-length",
        default_value_t = 31,
        display_order = 3
    )]
    pub kmer_length: u32,

    /// Keep duplicated identical sequences when constructing the index
    #[arg(long, display_order = 4)]
    pub keep_duplicates: bool,

    /// If this flag is passed, build the sparse rather than dense index for mapping
    #[arg(
        long,
        short = 'p',
        help_heading = "Alternative salmon-alevin Index Options",
        long = "sparse",
        requires = "use_piscem",
        display_order = 2
    )]
    pub sparse: bool,
}

/// Remove chemistries from the local chemistry registry
#[derive(Args, Clone, Debug)]
#[command(arg_required_else_help = true)]
pub struct ChemistryRemoveOpts {
    /// A chemistry name or a regex pattern matching the names of chemistries in the registry to remove
    #[arg(short, long)]
    pub name: String,
    /// Print the chemistries that would be removed without removing them
    #[arg(short, long)]
    pub dry_run: bool,
}

/// Download the permit list files for registered chemistries
#[derive(Args, Clone, Debug)]
#[command(arg_required_else_help = true)]
pub struct ChemistryFetchOpts {
    /// A comma-separated list of chemistry names to fetch (or a *single* regex pattern for matching multiple chemistries). Use '.*' to fetch for all registered chemistries.
    #[arg(short, long, required = true, value_delimiter = ',')]
    pub name: Vec<String>,
    /// Print the permit list file(s) that will be downloaded without downloading them
    #[arg(short, long)]
    pub dry_run: bool,
}

/// Remove cached permit list files that do not belong to any registered chemistries
#[derive(Args, Clone, Debug)]
#[command(arg_required_else_help = false)]
pub struct ChemistryCleanOpts {
    /// Print the permit list file(s) that will be removed without removing them
    #[arg(short, long)]
    pub dry_run: bool,
}

/// Look up chemistries in the local registry and print the details
#[derive(Args, Clone, Debug)]
#[command(arg_required_else_help = true)]
pub struct ChemistryLookupOpts {
    /// The name of a registered chemistry, or a regex pattern for matching registered chemistries' names.
    #[arg(short, long)]
    pub name: String,
}

/// Add a new or update an existing chemistry in the local registry
#[derive(Args, Clone, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct ChemistryAddOpts {
    /// The name to give to the chemistry
    #[arg(short, long)]
    pub name: String,
    /// A quoted string representing the geometry to which the chemistry maps
    #[arg(short, long)]
    pub geometry: String,
    /// The direction of the first (most upstream) mappable biological sequence.
    #[arg(short, long, value_parser = clap::builder::PossibleValuesParser::new(["fw", "rc", "both"]))]
    pub expected_ori: String,
    /// The (fully-qualified) path to a local permit list file that will be copied into
    /// the ALEVIN_FRY_HOME directory for future use.
    #[arg(long)]
    pub local_url: Option<PathBuf>,
    /// The url of a remote file that will be downloaded (on demand)
    /// to provide a permit list for use with this chemistry. This file
    /// should be obtainable with the equivalent of `wget <local-url>`.
    /// The file will only be downloaded the first time it is needed and
    /// will be locally cached in ALEVIN_FRY_HOME after that.
    #[arg(long)]
    pub remote_url: Option<String>,
    /// A semver format version tag,
    /// e.g., `0.1.0`, indicating the
    /// version of the chemistry definition.
    /// To update a registered chemistry,
    /// please provide a higher version number,
    /// e.g., `0.2.0`.
    #[arg(long, default_value = "0.0.0")]
    pub version: Option<String>,
}

/// Update the local chemistry registry according to the upstream repository
#[derive(Args, Clone, Debug)]
#[command(disable_version_flag = true)]
pub struct ChemistryRefreshOpts {
    /// overwrite existing chemistries even if the versions aren't newer
    #[arg(short, long)]
    pub force: bool,
    /// print the chemistries that will be added or updated without
    /// modifying the local registry.
    #[arg(short, long)]
    pub dry_run: bool,
}

#[derive(Debug, Subcommand)]
#[command(arg_required_else_help = true)]
pub enum ChemistryCommand {
    Refresh(ChemistryRefreshOpts),
    Add(ChemistryAddOpts),
    Remove(ChemistryRemoveOpts),
    Clean(ChemistryCleanOpts),
    Lookup(ChemistryLookupOpts),
    Fetch(ChemistryFetchOpts),
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// build the (expanded) reference index
    Index(IndexOpts),
    /// operate on or inspect the chemistry registry
    #[command(subcommand)]
    Chemistry(ChemistryCommand),
    /// inspect the current configuration
    Inspect {},
    /// quantify a sample
    Quant(MapQuantOpts),
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
    /// refreshes version information associated with programs used by simpleaf
    RefreshProgInfo {},
    /// run a sub-command dealing with atac-seq data
    #[command(subcommand)]
    Atac(AtacCommand),
    /// simpleaf workflow related command set
    Workflow(WorkflowOpts),
}

#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct WorkflowOpts {
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
        output: Option<PathBuf>,
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
