use tracing::warn;

pub fn cap_threads(requested: u32) -> (u32, Option<u32>) {
    cap_threads_with_limit(
        requested,
        std::thread::available_parallelism()
            .ok()
            .map(|n| n.get() as u32),
    )
}

/// Cap `requested` at the parallelism actually available to this process, and
/// explain the cap if one was applied.
///
/// Every caller previously open-coded this warning, and the message was hard to
/// act on: `available_parallelism` reports the CPUs *this process may use*, not
/// how many the machine has, so a scheduler or container that hands out a single
/// CPU produces "maximum available parallelism is 1" on a 48-core node and the
/// run silently drops to one thread (COMBINE-lab/simpleaf#135). Naming the cause
/// is the difference between a confusing log line and a fixable job script.
pub fn cap_threads_warned(requested: u32) -> u32 {
    let (effective, capped_at) = cap_threads(requested);
    if let Some(max_threads) = capped_at {
        warn!(
            "{} threads were requested, but only {} are available to this process; \
             using {}.",
            requested, max_threads, max_threads
        );
        warn!(
            "This reflects the CPU affinity / cgroup limit visible to simpleaf, not the \
             machine's total core count. If the machine has more cores, the limit is \
             usually set by the job scheduler (e.g. an unset or low --cpus-per-task) or \
             by the container runtime."
        );
    }
    effective
}

fn cap_threads_with_limit(requested: u32, limit: Option<u32>) -> (u32, Option<u32>) {
    if let Some(max_threads) = limit
        && requested > max_threads
    {
        return (max_threads, Some(max_threads));
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

    #[test]
    fn reports_no_cap_when_parallelism_is_unknown() {
        // `available_parallelism` can fail; when it does, the user's request
        // must be honoured rather than silently clamped to some fallback.
        let (effective, capped_at) = cap_threads_with_limit(32, None);
        assert_eq!(effective, 32);
        assert_eq!(capped_at, None);
    }
}
