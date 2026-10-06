//! Type-level pause clock (spec §5.7, ADR-007). Freezes the discount clock of every auction of the type.
#![allow(clippy::arithmetic_side_effects)] // saturating/ordered arithmetic on u64 ms

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnsetPolicy {
    /// (a) The pause starts when someone observes the invalid feed.
    AtObservation,
    /// (b) The pause starts at last valid oracle timestamp + max_age; unobserved gaps are closed on recovery.
    DerivedFromOracle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PauseClock {
    pub paused_since_ms: Option<u64>,
    pub paused_total_ms: u64,
    pub last_seen_oracle_ts: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observation {
    pub now_ms: u64,
    pub feed_valid: bool,
    pub admin_paused: bool,
    pub oracle_ts: u64,
    pub max_age_ms: u64,
}

impl PauseClock {
    pub fn is_paused(&self) -> bool {
        self.paused_since_ms.is_some()
    }

    /// Stale time between the previous known publish going stale and the newest publish `ts`
    /// that nobody observed (policy (b)). Zero when there is no earlier publish or no gap.
    fn unobserved_gap_ms(&self, ts: u64, max_age_ms: u64) -> u64 {
        self.last_seen_oracle_ts
            .map_or(0, |last| ts.saturating_sub(last.saturating_add(max_age_ms)))
    }

    pub fn observed(self, o: Observation, policy: OnsetPolicy) -> Self {
        let paused_now = o.admin_paused || !o.feed_valid;
        let derived = policy == OnsetPolicy::DerivedFromOracle;
        // Clamped: a future ts would both over-pause (griefing) and suppress later gap closure (under-pause).
        let ts = o.oracle_ts.min(o.now_ms);
        let mut next = self;
        match (paused_now, self.paused_since_ms) {
            (false, Some(since)) => {
                next.paused_total_ms = self
                    .paused_total_ms
                    .saturating_add(o.now_ms.saturating_sub(since));
                next.paused_since_ms = None;
            }
            // An open interval already covers any unobserved gap (closing it would double count).
            (true, Some(_)) => {}
            (_, None) => {
                if derived {
                    // Policy (b): close a gap nobody observed (the feed went stale, then recovered unseen).
                    next.paused_total_ms = self
                        .paused_total_ms
                        .saturating_add(self.unobserved_gap_ms(ts, o.max_age_ms));
                }
                if paused_now {
                    let onset = if derived && !o.feed_valid {
                        // The observation's own publish time is valid information even when stale.
                        let latest = self.last_seen_oracle_ts.map_or(ts, |l| l.max(ts));
                        latest.saturating_add(o.max_age_ms).min(o.now_ms)
                    } else {
                        o.now_ms
                    };
                    next.paused_since_ms = Some(onset);
                }
            }
        }
        if o.feed_valid || derived {
            next.last_seen_oracle_ts = Some(self.last_seen_oracle_ts.map_or(ts, |l| l.max(ts)));
        }
        next
    }

    /// t_eff = (now − start) − (paused_total − total_at_start) − open_pause_since_max(since, start); floored at 0.
    pub fn effective_elapsed(&self, start_ms: u64, total_at_start: u64, now_ms: u64) -> u64 {
        let wall = now_ms.saturating_sub(start_ms);
        let closed = self.paused_total_ms.saturating_sub(total_at_start);
        let open = self
            .paused_since_ms
            .map_or(0, |s| now_ms.saturating_sub(s.max(start_ms)));
        wall.saturating_sub(closed).saturating_sub(open)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn obs(now: u64, valid: bool, admin: bool, ts: u64) -> Observation {
        Observation {
            now_ms: now,
            feed_valid: valid,
            admin_paused: admin,
            oracle_ts: ts,
            max_age_ms: 100,
        }
    }
    #[test]
    fn admin_pause_freezes_elapsed() {
        let c0 = PauseClock::default().observed(obs(0, true, false, 0), OnsetPolicy::AtObservation);
        let c1 = c0.observed(obs(50, true, true, 50), OnsetPolicy::AtObservation); // admin pauses at 50
        assert_eq!(c1.effective_elapsed(0, 0, 80), 50); // frozen while open
        let c2 = c1.observed(obs(90, true, false, 90), OnsetPolicy::AtObservation); // resumes at 90
        assert_eq!(c2.effective_elapsed(0, 0, 100), 60); // 100 − 40 paused
    }
    #[test]
    fn auction_started_after_old_pause_ignores_it() {
        let c = PauseClock {
            paused_since_ms: None,
            paused_total_ms: 500,
            last_seen_oracle_ts: Some(0),
        };
        assert_eq!(c.effective_elapsed(1_000, 500, 1_200), 200);
    }
    #[test]
    fn open_pause_that_began_before_start_counts_from_start() {
        let c = PauseClock {
            paused_since_ms: Some(10),
            paused_total_ms: 0,
            last_seen_oracle_ts: Some(0),
        };
        assert_eq!(c.effective_elapsed(20, 0, 50), 0);
    }
    #[test]
    fn derived_onset_closes_unobserved_gap() {
        // valid at 0 (ts 0); nobody observes; the oracle republishes at ts 400; observed valid at 400.
        let c0 =
            PauseClock::default().observed(obs(0, true, false, 0), OnsetPolicy::DerivedFromOracle);
        let c1 = c0.observed(obs(400, true, false, 400), OnsetPolicy::DerivedFromOracle);
        // stale from 0+100 to 400 → 300 ms paused
        assert_eq!(c1.paused_total_ms, 300);
        assert_eq!(c1.effective_elapsed(0, 0, 400), 100);
    }
    #[test]
    fn admin_pause_on_stale_feed_keeps_derived_onset() {
        // valid at ts 0 (max_age 100); first seen again at 450, admin-paused and stale.
        let c0 =
            PauseClock::default().observed(obs(0, true, false, 0), OnsetPolicy::DerivedFromOracle);
        let c1 = c0.observed(obs(450, false, true, 0), OnsetPolicy::DerivedFromOracle);
        assert_eq!(c1.paused_since_ms, Some(100)); // stale from 0+100, not 450
                                                   // admin unpauses and a valid publish (ts 600) is seen at 600: paused [100,600] = 500
        let c2 = c1.observed(obs(600, true, false, 600), OnsetPolicy::DerivedFromOracle);
        assert_eq!(c2.paused_total_ms, 500);
        assert_eq!(c2.effective_elapsed(0, 0, 600), 100); // only the fresh [0,100)
    }
    #[test]
    fn admin_pause_closes_unobserved_gap() {
        // valid at 0; stale from 100; recovers unseen at ts 400; admin pauses at 450 (feed valid, ts 400).
        let c0 =
            PauseClock::default().observed(obs(0, true, false, 0), OnsetPolicy::DerivedFromOracle);
        let c1 = c0.observed(obs(450, true, true, 400), OnsetPolicy::DerivedFromOracle);
        assert_eq!(c1.paused_total_ms, 300); // gap [100,400]
        assert_eq!(c1.paused_since_ms, Some(450));
        // at 460: wall 460 - closed 300 - open (460-450) = 150 = fresh [0,100) + [400,450)
        assert_eq!(c1.effective_elapsed(0, 0, 460), 150);
        let c2 = c1.observed(obs(460, true, false, 400), OnsetPolicy::DerivedFromOracle);
        assert_eq!(c2.paused_total_ms, 310); // + admin [450,460)
        assert_eq!(c2.effective_elapsed(0, 0, 460), 150);
    }
    #[test]
    fn future_oracle_timestamp_cannot_add_pause_time() {
        let c0 =
            PauseClock::default().observed(obs(0, true, false, 0), OnsetPolicy::DerivedFromOracle);
        let c1 = c0.observed(obs(50, true, false, 10_000), OnsetPolicy::DerivedFromOracle);
        assert_eq!(c1.paused_total_ms, 0);
        assert_eq!(c1.last_seen_oracle_ts, Some(50)); // clamped to now
        let c2 = c1.observed(obs(500, true, false, 500), OnsetPolicy::DerivedFromOracle);
        assert_eq!(c2.paused_total_ms, 350); // stale [150,500) after the clamped publish at 50
        assert!(c2.paused_total_ms <= 500);
    }
}
