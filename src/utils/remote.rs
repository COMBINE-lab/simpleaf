/// Checks the provided URL to determine if it is a remote or local URL.
/// The current implementation is a heuristic, and may not cover all cases.
/// Decide if we want to pull in a crate like [url](https://crates.io/crates/url)
/// instead to do more comprehensive testing.
pub(crate) fn is_remote_url<T: AsRef<str>>(p: T) -> bool {
    let pr = p.as_ref();
    pr.starts_with("www.") || pr.starts_with("http://") || pr.starts_with("https://")
}
