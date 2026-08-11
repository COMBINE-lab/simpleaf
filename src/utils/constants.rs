/// NOTE: the "custom_chemistries.json" is deprecated and no longer used.
/// Now, all chemistries, built-in, "blessed" and custom should be added
/// to the "chemistries.json" registry. However, for a while we will
/// retain knowledge about this file and merge it in to the refreshed
/// chemistries file if it exists.
pub(crate) static CUSTOM_CHEMISTRIES_PATH: &str = "custom_chemistries.json";

pub(crate) static CHEMISTRIES_PATH: &str = "chemistries.json";

/// The chemistry registry is served from `dev`, deliberately, and not from the
/// release branch. Chemistry definitions are data rather than code: a new or
/// corrected geometry needs to reach users who are already on a released
/// simpleaf, without waiting for the next release. `simpleaf chemistry refresh`
/// therefore always fetches the newest registry.
///
/// The trade-off is that a released binary's chemistry definitions are whatever
/// `dev` holds at the time of the fetch, not what shipped with it. Keep `dev`'s
/// copy of `resources/chemistries.json` in sync when landing registry changes
/// on `main`, or a released simpleaf will not see them.
pub(crate) static CHEMISTRIES_URL: &str =
    "https://raw.githubusercontent.com/COMBINE-lab/simpleaf/dev/resources/chemistries.json";

pub(crate) static NUM_SAMPLE_LINES: usize = 100;
