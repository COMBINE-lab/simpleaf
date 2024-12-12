use crate::atac::defaults::DefaultAtacParams;
use crate::defaults::{DefaultMappingParams, DefaultParams};
use clap::{builder::ArgPredicate, Args, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum AtacCommand {
    Index(IndexOpts),
    Process(ProcessOpts),
}

/// build a piscem index over the genome for
/// scATAC-seq mapping.
#[derive(Args, Clone, Debug)]
#[command(arg_required_else_help = true)]
pub struct IndexOpts {
    /// number of threads to use when running
    #[arg(short, long, default_value_t = 16, display_order = 5)]
    pub threads: u32,

    /// the value of k to be used to construct the index
    #[arg(
        short = 'k',
        long = "kmer-length",
        default_value_t = 31,
        help_heading = "Index Configuration Options",
        display_order = 3
    )]
    pub kmer_length: u32,

    /// the value of m to be used to construct the piscem index (must be < k)
    #[arg(
        short = 'm',
        long = "minimizer-length",
        default_value_t = 19,
        help_heading = "Index Configuration Options",
        display_order = 4
    )]
    pub minimizer_length: u32,

    /// target sequences (provide target sequences directly; avoid expanded reference construction)
    #[arg(short, long, display_order = 1)]
    pub input: PathBuf,

    /// path to output directory (will be created if it doesn't exist)
    #[arg(short, long, display_order = 2)]
    pub output: PathBuf,
}

/// process a scATAC-seq sample by performing
/// mapping, barcode correction, and sorted
/// (deduplicated) BED file generation.
#[derive(Args, Clone, Debug)]
#[command(arg_required_else_help = true)]
pub struct ProcessOpts {
    /// path to index
    #[arg(short = 'i', long = "index", help_heading = "Mapping Options")]
    pub index: PathBuf,

    /// comma-separated list of paths to read 1 files
    #[arg(
        short = '1',
        long = "reads1",
        help_heading = "Mapping Options",
        value_delimiter = ',',
        requires = "barcode-reads",
        requires_ifs([
                (ArgPredicate::IsPresent, "reads2") 
        ]),
    )]
    pub reads1: Option<Vec<PathBuf>>,

    /// comma-separated list of paths to read 2 files
    #[arg(
        short = '2',
        long = "reads2",
        help_heading = "Mapping Options",
        value_delimiter = ',',
        requires = "barcode-reads",
        requires_ifs([
                (ArgPredicate::IsPresent, "reads1") 
        ]),
    )]
    pub reads2: Option<Vec<PathBuf>>,

    /// path to the read files containing single-end reads
    #[arg(
        short = 'r',
        long = "reads",
        help_heading = "Mapping Options",
        value_delimiter = ',',
        conflicts_with_all =  ["reads1", "reads2"],
        required_unless_present = "reads1",
        required_unless_present = "reads2",
        requires = "barcode-reads"
    )]
    pub reads: Option<Vec<PathBuf>>,

    /// path to the read files containing the cell barcodes
    #[arg(
        short = 'b',
        long = "barcode-reads",
        help_heading = "Mapping Options",
        value_delimiter = ',',
        required = true
    )]
    pub barcode_reads: Vec<PathBuf>,

    /// the length of the barcode read from which to extract the barcode
    /// (usually this is the length of the entire read, and reads shorter
    /// than this will be discarded)
    #[arg(
        long = "barcode-length",
        default_value_t = 16,
        help_heading = "Mapping Options"
    )]
    pub barcode_length: u32,

    // output directory where mapping and processed BED file will be written
    #[arg(long = "output")]
    pub output: PathBuf,

    /// number of threads to use when running
    #[arg(short, long, default_value_t = 16, display_order = 5)]
    pub threads: u32,

    /// skip checking of the equivalence classes of k-mers that were too
    /// ambiguous to be otherwise considered (passing this flag can speed
    /// up mapping slightly, but may reduce specificity)
    #[arg(long, help_heading = "Advanced Options")]
    pub ignore_ambig_hits: bool,

    /// do not consider poison k-mers, even if the underlying index
    /// contains them. In this case, the mapping results will be identical
    /// to those obtained as if no poison table was added to the index
    #[arg(long, help_heading = "Advanced Options")]
    pub no_poison: bool,

    /// use chromosomes as color
    #[arg(long, help_heading = "Advanced Options")]
    pub use_chr: bool,

    /// threshold to be considered for pseudoalignment, default set to 0.7
    #[arg(long, default_value_t = DefaultParams::KMER_FRACTION, help_heading = "Advanced Options")]
    pub thr: f64,

    /// size of virtual color, default set to 1000 [default: 1000]
    #[arg(long, default_value_t = DefaultParams::BIN_SIZE, help_heading = "Advanced Options")]
    pub bin_size: u32,

    /// size for bin overlap, default set to 300 [default: 300]
    #[arg(long, default_value_t = DefaultParams::BIN_OVERLAP, help_heading = "Advanced Options")]
    pub bin_overlap: u32,

    /// do not apply Tn5 shift to mapped positions
    #[arg(long, help_heading = "Advanced Options")]
    pub no_tn5_shift: bool,

    /// Check if any mapping kmer exist for a mate which is not mapped,
    /// but there exists mapping for the other read. If set to true and a
    /// mapping kmer exists, then the pair would not be mapped
    #[arg(long, help_heading = "Advanced Options")]
    pub check_kmer_orphan: bool,

    /// determines the maximum cardinality equivalence class (number of
    /// (txp, orientation status) pairs) to examine (cannot be used with
    /// --ignore-ambig-hits)
    #[arg(long, default_value_t = DefaultParams::MAX_EC_CARD, help_heading = "Advanced Options")]
    pub max_ec_card: u32,

    /// in the first pass, consider only k-mers having <= --max-hit-occ
    /// hits
    #[arg(long, default_value_t = DefaultParams::MAX_HIT_OCC, help_heading = "Advanced Options")]
    pub max_hit_occ: u32,

    /// if all k-mers have > --max-hit-occ hits, then make a second pass
    /// and consider k-mers having <= --max-hit-occ-recover hits
    #[arg(long, default_value_t = DefaultParams::MAX_HIT_OCC_RECOVER, help_heading = "Advanced Options")]
    pub max_hit_occ_recover: u32,

    /// reads with more than this number of mappings will not have their
    /// mappings reported
    #[arg(long, default_value_t = DefaultParams::MAX_READ_OCC, help_heading = "Advanced Options")]
    pub max_read_occ: u32,
}
