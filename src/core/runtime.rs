pub fn cap_threads(requested: u32) -> (u32, Option<u32>) {
    cap_threads_with_limit(
        requested,
        std::thread::available_parallelism()
            .ok()
            .map(|n| n.get() as u32),
    )
}

fn cap_threads_with_limit(requested: u32, limit: Option<u32>) -> (u32, Option<u32>) {
    if let Some(max_threads) = limit {
        if requested > max_threads {
            return (max_threads, Some(max_threads));
        }
    }
    (requested, None)
}

#[cfg(test)]
mod tests {
    use super::cap_threads_with_limit;

    #[test]
    fn caps_requested_threads_when_over_limit() {
        let (effective, capped_at) = cap_threads_with_limit(32, Some(8));
        assert_eq!(effective, 8);
        assert_eq!(capped_at, Some(8));
    }

    #[test]
    fn keeps_requested_threads_when_within_limit() {
        let (effective, capped_at) = cap_threads_with_limit(8, Some(32));
        assert_eq!(effective, 8);
        assert_eq!(capped_at, None);
    }
}
