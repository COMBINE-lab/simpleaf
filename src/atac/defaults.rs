use crate::defaults::DefaultParams;

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
