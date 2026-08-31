use tracing::warn;

pub fn cap_threads(requested: u32) -> (u32, Option<u32>) {
    cap_threads_with_limit(
        requested,
        std::thread::available_parallelism()
            .ok()
            .map(|n| n.get() as u32),
    )
}

/// Resolve `requested` against the two-thread floor and the parallelism
/// available to this process, and explain every adjustment.
///
/// Every caller previously open-coded this warning, and the message was hard to
/// act on: `available_parallelism` reports the CPUs *this process may use*, not
/// how many the machine has, so a scheduler or container that hands out a single
/// CPU produces "maximum available parallelism is 1" on a 48-core node and the
/// run used to silently drop to one thread (COMBINE-lab/simpleaf#135). A report
/// of one is now the sole exception to the upper cap: simpleaf warns and still
/// attempts the practical minimum of two threads.
pub fn cap_threads_warned(requested: u32) -> u32 {
    let (effective, capped_at) = cap_threads(requested);
    if requested < 2 {
        warn!(
            "{} thread(s) were requested, but 2 threads is the practical minimum for simpleaf; using 2.",
            requested
        );
    }
    if let Some(max_threads) = capped_at {
        if max_threads < 2 {
            warn!(
                "std::thread::available_parallelism() reported {} thread, but 2 threads is the practical minimum for simpleaf; attempting to use 2.",
                max_threads
            );
        } else {
            warn!(
                "{} threads were requested, but only {} are available to this process; using {}.",
                requested, max_threads, max_threads
            );
            warn!(
                "This reflects the CPU affinity / cgroup limit visible to simpleaf, not the machine's total core count. If the machine has more cores, the limit is usually set by the job scheduler (e.g. an unset or low --cpus-per-task) or by the container runtime."
            );
        }
    }
    effective
}

fn cap_threads_with_limit(requested: u32, limit: Option<u32>) -> (u32, Option<u32>) {
    let requested = requested.max(2);
    if let Some(max_threads) = limit {
        if max_threads < 2 {
            return (2, Some(max_threads));
        }
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

    #[test]
    fn raises_zero_and_one_to_the_practical_minimum() {
        assert_eq!(cap_threads_with_limit(0, Some(8)), (2, None));
        assert_eq!(cap_threads_with_limit(1, Some(8)), (2, None));
        assert_eq!(cap_threads_with_limit(2, Some(8)), (2, None));
    }

    #[test]
    fn attempts_two_when_available_parallelism_reports_one() {
        assert_eq!(cap_threads_with_limit(1, Some(1)), (2, Some(1)));
        assert_eq!(cap_threads_with_limit(16, Some(1)), (2, Some(1)));
    }

    #[test]
    fn reports_no_cap_when_parallelism_is_unknown() {
        // `available_parallelism` can fail; when it does, the user's request
        // must be honoured rather than silently clamped to some fallback.
        let (effective, capped_at) = cap_threads_with_limit(32, None);
        assert_eq!(effective, 32);
        assert_eq!(capped_at, None);
    }

    #[test]
    fn unknown_parallelism_still_applies_the_thread_floor() {
        assert_eq!(cap_threads_with_limit(0, None), (2, None));
        assert_eq!(cap_threads_with_limit(1, None), (2, None));
    }
}
