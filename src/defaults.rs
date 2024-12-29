/// default parameters for RNA-seq
pub trait DefaultMappingParams {
    const MAX_EC_CARD: u32;
    const MAX_HIT_OCC: u32;
    const MAX_HIT_OCC_RECOVER: u32;
    const MAX_READ_OCC: u32;
    const SKIPPING_STRATEGY: &'static str;
}

pub struct DefaultParams;

impl DefaultMappingParams for DefaultParams {
    const MAX_EC_CARD: u32 = 4096;
    const MAX_HIT_OCC: u32 = 256;
    const MAX_HIT_OCC_RECOVER: u32 = 1024;
    const MAX_READ_OCC: u32 = 2500;
    const SKIPPING_STRATEGY: &'static str = "permissive";
}
