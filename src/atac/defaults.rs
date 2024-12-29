/// default parameters for ATAC-seq
use crate::defaults::DefaultParams;

pub(super) struct AtacIndexParams;

impl AtacIndexParams {
    pub const K: u32 = 25;
    pub const M: u32 = 17;
}

pub trait DefaultAtacParams {
    const BIN_SIZE: u32;
    const BIN_OVERLAP: u32;
    const KMER_FRACTION: f64;
}

impl DefaultAtacParams for DefaultParams {
    const BIN_SIZE: u32 = 1000;
    const BIN_OVERLAP: u32 = 300;
    const KMER_FRACTION: f64 = 0.7;
}
