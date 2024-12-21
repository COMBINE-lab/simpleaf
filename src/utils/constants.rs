/// NOTE: the "custom_chemistries.json" is deprecated and no longer used.
/// Now, all chemistries, built-in, "blessed" and custom should be added
/// to the "chemistries.json" registry. However, for a while we will
/// retain knowledge about this file and merge it in to the refreshed
/// chemistries file if it exists.
pub(crate) static CUSTOM_CHEMISTRIES_PATH: &str = "custom_chemistries.json";

pub(crate) static CHEMISTRIES_PATH: &str = "chemistries.json";
pub(crate) static CHEMISTRIES_URL: &str =
    "https://raw.githubusercontent.com/COMBINE-lab/simpleaf/dev/resources/chemistries.json";

pub(crate) static NUM_SAMPLE_LINES: usize = 100;
