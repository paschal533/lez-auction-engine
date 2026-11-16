//! The grace period after a feed outage (ADR-075): a pure rule over a small state. A feed that
//! comes back after an unobserved or observed gap of at least `grace_min_gap_ms` opens a window
//! of `grace_ms` in which liquidations wait, so borrowers can top up against the fresh price.

/// What the grace rule remembers about the feed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GraceState {
    /// Timestamp of the latest valid reading, if any.
    pub last_good_ts_ms: Option<u64>,
    /// When the current outage began, if an invalid reading has been observed since the last
    /// good one.
    pub feed_invalid_since_ms: Option<u64>,
    /// Liquidations wait while `now < grace_until_ms`.
    pub grace_until_ms: u64,
}

/// One observation of the feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraceObservation {
    /// Whether the reading passed every feed check.
    pub valid: bool,
    /// The reading's own timestamp.
    pub reading_ts_ms: u64,
    /// The block time of the observation.
    pub now_ms: u64,
    /// How old a reading may be and still be valid.
    pub max_age_ms: u64,
}

/// The state after observing the feed once. `grace_ms == 0` turns grace off.
#[must_use]
pub fn observed(
    state: GraceState,
    o: GraceObservation,
    grace_ms: u64,
    grace_min_gap_ms: u64,
) -> GraceState {
    if !o.valid {
        let onset = state.feed_invalid_since_ms.unwrap_or_else(|| {
            state
                .last_good_ts_ms
                .map_or(o.now_ms, |l| l.saturating_add(o.max_age_ms))
                .min(o.now_ms)
        });
        return GraceState {
            feed_invalid_since_ms: Some(onset),
            ..state
        };
    }
    let ts = o.reading_ts_ms.min(o.now_ms);
    let gap = match state.feed_invalid_since_ms {
        Some(since) => o.now_ms.saturating_sub(since),
        None => state
            .last_good_ts_ms
            .map_or(0, |l| ts.saturating_sub(l.saturating_add(o.max_age_ms))),
    };
    let opens = grace_ms > 0 && gap >= grace_min_gap_ms && gap > 0;
    let grace_until_ms = if opens {
        state.grace_until_ms.max(o.now_ms.saturating_add(grace_ms))
    } else {
        state.grace_until_ms
    };
    GraceState {
        last_good_ts_ms: Some(state.last_good_ts_ms.map_or(ts, |l| l.max(ts))),
        feed_invalid_since_ms: None,
        grace_until_ms,
    }
}

/// Whether `now_ms` falls inside the grace window.
#[must_use]
pub const fn in_grace(state: &GraceState, now_ms: u64) -> bool {
    now_ms < state.grace_until_ms
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const MAX_AGE: u64 = 600_000;
    const GRACE: u64 = 1_800_000;
    const MIN_GAP: u64 = 600_000;

    fn obs(valid: bool, now_ms: u64) -> GraceObservation {
        GraceObservation {
            valid,
            reading_ts_ms: now_ms,
            now_ms,
            max_age_ms: MAX_AGE,
        }
    }

    fn step(state: GraceState, o: GraceObservation) -> GraceState {
        observed(state, o, GRACE, MIN_GAP)
    }

    #[test]
    fn observed_outage_starts_grace() {
        let s = step(GraceState::default(), obs(true, 0));
        let s = step(s, obs(false, 700_000));
        assert_eq!(s.feed_invalid_since_ms, Some(600_000));
        let s = step(s, obs(true, 1_300_000));
        assert_eq!(s.grace_until_ms, 3_100_000);
        assert_eq!(s.feed_invalid_since_ms, None);
    }

    #[test]
    fn grace_needs_a_gap_of_at_least_min() {
        let s = step(GraceState::default(), obs(true, 0));
        let s = step(s, obs(false, 700_000));
        let short = step(s, obs(true, 1_199_999));
        assert_eq!(short.grace_until_ms, 0);
        let enough = step(s, obs(true, 1_200_000));
        assert_eq!(enough.grace_until_ms, 3_000_000);
    }

    #[test]
    fn unobserved_gap_starts_grace() {
        let s = step(GraceState::default(), obs(true, 0));
        let s = step(s, obs(true, 1_200_000));
        assert_eq!(s.grace_until_ms, 3_000_000);
    }

    #[test]
    fn first_observation_starts_no_grace() {
        let s = step(GraceState::default(), obs(true, 5_000_000));
        assert_eq!(s.grace_until_ms, 0);
        assert_eq!(s.last_good_ts_ms, Some(5_000_000));
    }

    #[test]
    fn grace_off_never_starts() {
        let s = observed(GraceState::default(), obs(true, 0), 0, MIN_GAP);
        let s = observed(s, obs(true, 9_000_000), 0, MIN_GAP);
        assert_eq!(s.grace_until_ms, 0);
    }

    #[test]
    fn a_second_outage_only_extends() {
        let s = step(GraceState::default(), obs(true, 0));
        let s = step(s, obs(true, 1_200_000));
        let first = s.grace_until_ms;
        let s = step(s, obs(true, 1_300_000));
        assert_eq!(s.grace_until_ms, first);
        let s = step(s, obs(true, 3_000_000));
        assert!(s.grace_until_ms >= first);
    }

    #[test]
    fn grace_expires_at_its_end() {
        let s = GraceState {
            grace_until_ms: 3_000_000,
            ..GraceState::default()
        };
        assert!(in_grace(&s, 2_999_999));
        assert!(!in_grace(&s, 3_000_000));
    }

    #[test]
    fn invalid_onset_is_clamped_to_now() {
        let s = step(GraceState::default(), obs(true, 0));
        let s = step(s, obs(false, 100_000));
        assert_eq!(s.feed_invalid_since_ms, Some(100_000));
        let s = step(GraceState::default(), obs(false, 42));
        assert_eq!(s.feed_invalid_since_ms, Some(42));
    }

    proptest! {
        #[test]
        fn grace_until_is_monotone_and_bounded(
            last_good in proptest::option::of(any::<u64>()),
            invalid_since in proptest::option::of(any::<u64>()),
            until in any::<u64>(),
            valid in any::<bool>(),
            reading_ts in any::<u64>(),
            now in any::<u64>(),
            max_age in any::<u64>(),
            grace_ms in any::<u64>(),
            min_gap in any::<u64>(),
        ) {
            let state = GraceState {
                last_good_ts_ms: last_good,
                feed_invalid_since_ms: invalid_since,
                grace_until_ms: until,
            };
            let o = GraceObservation {
                valid,
                reading_ts_ms: reading_ts,
                now_ms: now,
                max_age_ms: max_age,
            };
            let next = observed(state, o, grace_ms, min_gap);
            prop_assert!(next.grace_until_ms >= state.grace_until_ms);
            prop_assert!(
                next.grace_until_ms <= state.grace_until_ms.max(now.saturating_add(grace_ms))
            );
        }
    }
}
