use crate::atac::defaults::{AtacIndexParams, DefaultAtacParams};
use crate::defaults::{DefaultMappingParams, DefaultParams};
use crate::utils::chem_utils::{ExpectedOri, QueryInRegistry};
use clap::{
    builder::{ArgPredicate, PossibleValue},
    Args, Subcommand, ValueEnum,
};
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use strum_macros::EnumIter;

fn barcode_ori_parser(s: &str) -> Result<ExpectedOri, String> {
    match s {
        "forward" | "fw" | "+" => Ok(ExpectedOri::Forward),
        "reverse" | "rc" | "-" => Ok(ExpectedOri::Reverse),
        t => Err(format!(
            "Do not recognize expected permit-list barcode orientation {}",
            t
        )),
    }
}

#[derive(Clone, Debug)]
pub enum Macs3GenomeSize {
    KnownOpt(&'static str),
    Len(usize),
}

impl FromStr for Macs3GenomeSize {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "hs" => Ok(Macs3GenomeSize::KnownOpt("hs")),
            "mm" => Ok(Macs3GenomeSize::KnownOpt("mm")),
            "ce" => Ok(Macs3GenomeSize::KnownOpt("ce")),
            "dm" => Ok(Macs3GenomeSize::KnownOpt("dm")),
            x => {
                if let Ok(n) = x.parse::<usize>() {
                    Ok(Macs3GenomeSize::Len(n))
                } else {
                    anyhow::bail!("Invalid argument to custom arg");
                }
            }
        }
    }
}

impl Macs3GenomeSize {
    pub fn as_arg_str(&self) -> String {
        match self {
            Macs3GenomeSize::KnownOpt(s) => s.to_string(),
            Macs3GenomeSize::Len(n) => format!("{}", n),
        }
    }
}

fn validate_prob(p: &str) -> Result<f32, String> {
    let v = p
        .parse::<f32>()
        .map_err(|_| "Cannot parse provided argument as a float")?;
    if v > 0.0 && v <= 1.0 {
        Ok(v)
    } else {
        Err(format!(
            "The provided value must be a probability in (0,1], but {} was provided.",
            v
        ))
    }
}

#[derive(EnumIter, Copy, Clone, Eq, PartialEq)]
pub enum AtacChemistry {
    TenxV11,
    TenxV2,
    TenxMulti,
}

/// [Debug] representations of the different geometries.
impl fmt::Debug for AtacChemistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AtacChemistry::TenxV11 => write!(f, "10xv1"),
            AtacChemistry::TenxV2 => write!(f, "10xv2"),
            AtacChemistry::TenxMulti => write!(f, "10xmulti"),
        }
    }
}

impl QueryInRegistry for AtacChemistry {
    fn registry_key(&self) -> &str {
        match self {
            Self::TenxV11 => "10x-atac-v1",
            Self::TenxV2 => "10x-atac-v2",
            Self::TenxMulti => "10x-arc-atac-v1",
        }
    }
}

impl AtacChemistry {
    #[allow(dead_code)]
    pub fn possible_values() -> impl Iterator<Item = PossibleValue> {
        Self::value_variants()
            .iter()
            .filter_map(clap::ValueEnum::to_possible_value)
    }

    #[allow(dead_code)]
    pub fn resource_key(&self) -> String {
        match self {
            Self::TenxV11 => String::from("10x-atac-v1"),
            Self::TenxV2 => String::from("10x-atac-v2"),
            Self::TenxMulti => String::from("10x-arc-atac-v1"),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            AtacChemistry::TenxV11 => "10x-atac-v1",
            AtacChemistry::TenxV2 => "10x-atac-v2",
            AtacChemistry::TenxMulti => "10x-arc-atac-v1",
        }
    }
}

impl std::str::FromStr for AtacChemistry {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "10xv1" => Ok(AtacChemistry::TenxV11),
            "10xv11" => Ok(AtacChemistry::TenxV11),
            "10xv2" => Ok(AtacChemistry::TenxV2),
            "10xmulti" => Ok(AtacChemistry::TenxMulti),
            t => Err(format!("invalid atac chemistry : {t}")),
        }
    }
}

impl clap::ValueEnum for AtacChemistry {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::TenxV11, Self::TenxV2, Self::TenxMulti]
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(match self {
            Self::TenxV11 => PossibleValue::new("10x-v1"),
            Self::TenxV2 => PossibleValue::new("10x-v2"),
            Self::TenxMulti => PossibleValue::new("10x-multi"),
        })
    }
}

#[derive(Debug, Subcommand)]
#[command(arg_required_else_help = true)]
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
        default_value_t = AtacIndexParams::K,
        help_heading = "Index Configuration Options",
        display_order = 3
    )]
    pub kmer_length: u32,

    /// the value of m to be used to construct the piscem index (must be < k)
    #[arg(
        short = 'm',
        long = "minimizer-length",
        default_value_t = AtacIndexParams::M,
        help_heading = "Index Configuration Options",
        display_order = 4
    )]
    pub minimizer_length: u32,

    /// seed value to use in SSHash index construction
    /// (try changing this in the rare event index build fails).
    #[arg(long = "seed", default_value_t = 1, display_order = 7)]
    pub hash_seed: u64,

    /// overwrite existing files if the output directory is already populated
    #[arg(long, display_order = 8)]
    pub overwrite: bool,

    /// working directory where temporary files should be placed
    #[arg(
        long = "work-dir",
        default_value = "./workdir.noindex",
        display_order = 6
    )]
    pub work_dir: PathBuf,

    /// path to (optional) decoy sequence used to insert poison
    /// k-mer information into the index (only if using piscem >= 0.7).
    #[arg(
        long,
        help_heading = "Piscem Index Options",
        value_delimiter = ',',
        display_order = 5
    )]
    pub decoy_paths: Option<Vec<PathBuf>>,

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
        requires = "barcode_reads",
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
        requires = "barcode_reads",
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
        requires = "barcode_reads"
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

    /// chemistry
    #[arg(short, long)]
    pub chemistry: AtacChemistry,

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

    /// do peak calling after generating the bed file
    #[arg(long)]
    pub call_peaks: bool,

    /// The expected orientation of the barcodes in the permit list relative
    /// to the barcodes extracted from the reads. If this is "fw", it is expected
    /// that the sequences will match directly, if "rc" it is expected the reverse
    /// complement of the permit-list sequence will match the reads' barcodes.
    /// If provided, this value will be used, if not provided, simpleaf will attempt
    /// to look up the appropriate orientation in the chemistry registry.
    #[arg(
        long,
        default_value = None,
        value_parser = barcode_ori_parser,
        help_heading = "Permit List Generation Options",
    )]
    pub permit_barcode_ori: Option<ExpectedOri>,

    /// Use the provided file as the unfiltered permit list (i.e. whitelist).
    /// This argument only needs to be provided if you are providing the permit list explicitly,
    /// overriding the default permit list for the provided chemistry.
    #[arg(
        short,
        long,
        default_value = None,
        help_heading = "Permit List Generation Options"
    )]
    pub unfiltered_pl: Option<PathBuf>,

    /// minimum read count threshold for a cell to be retained/processed; only used with --unfiltered-pl
    #[arg(
        long,
        help_heading = "Permit List Generation Options",
        default_value_t = 10
    )]
    pub min_reads: usize,

    /// skip checking of the equivalence classes of k-mers that were too
    /// ambiguous to be otherwise considered (passing this flag can speed
    /// up mapping slightly, but may reduce specificity)
    #[arg(
        long,
        conflicts_with = "max_ec_card",
        help_heading = "Advanced Options"
    )]
    pub ignore_ambig_hits: bool,

    /// do not consider poison k-mers, even if the underlying index
    /// contains them. In this case, the mapping results will be identical
    /// to those obtained as if no poison table was added to the index
    #[arg(long, help_heading = "Advanced Options")]
    pub no_poison: bool,

    /// use chromosomes as color
    #[arg(long, help_heading = "Advanced Options")]
    pub use_chr: bool,

    /// threshold to be considered for pseudoalignment
    #[arg(long, default_value_t = DefaultParams::KMER_FRACTION, help_heading = "Advanced Options")]
    pub thr: f64,

    /// size of virtual color intervals
    #[arg(long, default_value_t = DefaultParams::BIN_SIZE, help_heading = "Advanced Options")]
    pub bin_size: u32,

    /// size for virtual color interval overlap
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
    #[arg(long, default_value_t = DefaultParams::MAX_EC_CARD, conflicts_with = "ignore_ambig_hits", help_heading = "Advanced Options")]
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

    /// The value to be passed to the `macs3` `--gsize` (genome size) option.
    /// Possible values are "hs", "mm", "ce", "dm" or an unsigned integer.
    #[arg(long, help_heading = "Peak Caller Options", default_value = "hs")]
    pub gsize: Macs3GenomeSize,

    /// The value to be passed to the `macs3` `--qvalue` (minimum FDR cutoff) option.
    #[arg(long, help_heading = "Peak Caller Options", value_parser = validate_prob, default_value_t = 0.1_f32)]
    pub qvalue: f32,

    /// The value to be passed to the `macs3` `--extsize` option.
    #[arg(long, help_heading = "Peak Caller Options", default_value_t = 50)]
    pub extsize: usize,
}
