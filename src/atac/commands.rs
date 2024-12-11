use clap::{builder::ArgPredicate, ArgAction, ArgGroup, Args, Subcommand};
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
pub struct ProcessOpts {}
