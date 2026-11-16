//! Oracle validity and the type-level pause observation (v0.2.4 `feed.rs`, spec §5.2, §5.7) on
//! readings that arrive by `StateReply` instead of account data.
//!
//! Time (design note §4.1): `now_ms` is the caller's claim, asserted to lie in
//! `[clock − slack, clock]`. The feed-age check uses `now_ms + slack_ms`, so an understated claim
//! cannot make a stale feed look fresh; the pause clock records `now_ms`. The bias of a claim is
//! bounded by `slack_ms` but does not always favour the protocol: a pause closed at an
//! understated `now_ms` is shorter, and an auction's `start_ms` can be up to `slack_ms` in the
//! past (ADR-058; design note §4.1).

use auction_core::{
    pause::{Observation, OnsetPolicy, PauseClock},
    price::{OracleReading, to_ray},
};
use auction_engine_params::EngineError;
use risk_core::{
    RiskError,
    feed::{FeedFault, FeedPolicy, validate_feed_ext},
    grace::{self, GraceObservation},
    price::conf_bps,
};

use crate::{check::Refusal, ext::CollateralExt, state::CollateralState};

/// The confidence of an unconvertible reading, bps: the widest edge.
const CONF_SATURATED_BPS: u16 = 10_000;

/// One observation: the next pause clock, the feed verdict (the market price if valid, else the
/// fault) and the reading's confidence in bps of the price (0 when the feed is invalid).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observed {
    pub clock: PauseClock,
    pub feed: Result<u128, FeedFault>,
    pub conf_bps: u16,
}

/// The type's feed policy: v0.2.4's feed parameters with the sanity window and the pin bounds
/// from the type's `RiskArgs`. A free fn rather than a `CollateralState` method: `state.rs` is at
/// its size limit.
pub fn feed_policy(coll: &CollateralState) -> Result<FeedPolicy, EngineError> {
    let risk = &coll.ext.risk;
    Ok(FeedPolicy {
        sanity_lo_bps: risk.sanity_lo_bps,
        sanity_hi_bps: risk.sanity_hi_bps,
        feed_min_raw: risk.feed_min_raw,
        feed_max_raw: risk.feed_max_raw,
        ..FeedPolicy::legacy(&coll.feed_params()?)
    })
}

/// One pause observation at `now_ms` (policy (b), ADR-007). `feed_valid` comes from
/// `validate_feed_ext` (equal to v0.2.4's `validate_feed` under the legacy policy) at
/// `now_ms + slack_ms` with the same oracle timestamp and max age that `observed` receives
/// (ADR-024 obligation, with the conservative age of design note §4.1).
pub fn observe(
    coll: &CollateralState,
    reading: &OracleReading,
    r_ray: u128,
    now_ms: u64,
    slack_ms: u64,
) -> Result<Observed, EngineError> {
    let policy = feed_policy(coll)?;
    let age_now = now_ms.checked_add(slack_ms).ok_or(EngineError::Math)?;
    let feed = validate_feed_ext(reading, &policy, r_ray, age_now);
    let clock = coll.pause_clock().observed(
        Observation {
            now_ms,
            feed_valid: feed.is_ok(),
            // Only an effective level 3 (or an invalid feed) stops the discount (spec §3.2.1).
            admin_paused: coll.effective_level() == 3,
            oracle_ts: reading.timestamp_ms,
            max_age_ms: policy.max_age_ms,
        },
        OnsetPolicy::DerivedFromOracle,
    );
    // A valid feed already converted the confidence; the saturated edge is the safe side anyway.
    let conf_bps = feed.map_or(0, |m| {
        to_ray(reading.confidence_raw, policy.scale)
            .map_or(CONF_SATURATED_BPS, |conf| conf_bps(conf, m))
    });
    Ok(Observed {
        clock,
        feed,
        conf_bps,
    })
}

/// Records this observation (spec §3.1.2, ADR-075): the grace state after
/// `risk_core::grace::observed` on every observation, valid or not; and the ring after
/// `risk_core::record` when the feed is valid and the TWAP is on. Everything else is `coll`'s.
#[must_use]
pub fn record(
    coll: &CollateralState,
    reading: &OracleReading,
    obs: &Observed,
    now_ms: u64,
) -> CollateralState {
    let risk = coll.ext.risk;
    let grace = grace::observed(
        coll.ext.grace(),
        GraceObservation {
            valid: obs.feed.is_ok(),
            reading_ts_ms: reading.timestamp_ms,
            now_ms,
            max_age_ms: coll.max_oracle_age_ms,
        },
        risk.grace_ms,
        risk.grace_min_gap_ms,
    );
    let graced = CollateralExt {
        last_good_ts_ms: grace.last_good_ts_ms,
        feed_invalid_since_ms: grace.feed_invalid_since_ms,
        grace_until_ms: grace.grace_until_ms,
        ..coll.ext.clone()
    };
    let ext = match obs.feed {
        Ok(m) if risk.twap_window_ms > 0 => CollateralExt {
            ring: risk_core::record(
                &coll.ext.ring,
                reading.timestamp_ms,
                m,
                obs.conf_bps,
                now_ms,
                risk.twap_min_spacing_ms,
            ),
            ..graced
        },
        Ok(_) | Err(_) => graced,
    };
    CollateralState {
        ext,
        ..coll.clone()
    }
}

/// A feed fault's refusal (ruling R10-9): `Pinned` is `PriceFeedPinned` (6076); every other
/// fault keeps v0.2.4's `EngineError` for its pause reason.
#[must_use]
pub fn feed_refusal(fault: FeedFault) -> Refusal {
    // `pause_reason` is `None` for `Pinned` only.
    fault
        .pause_reason()
        .map_or(Refusal::Risk(RiskError::PriceFeedPinned), |reason| {
            Refusal::Engine(EngineError::from(reason))
        })
}

/// The market price if the type is live, else the pause as an error (`Paused`, `PriceFeedStale`,
/// `PriceFeedLowConfidence`, `PriceFeedOutOfRange`, `PriceFeedPinned`). A reverting transaction
/// drops its observation; policy (b) recovers the onset from the oracle timestamp at the next
/// successful one (ADR-032).
///
/// # Errors
/// The refusals above.
pub fn require_live(coll: &CollateralState, obs: &Observed) -> Result<u128, Refusal> {
    if coll.admin_paused {
        return Err(EngineError::Paused.into());
    }
    obs.feed.map_err(feed_refusal)
}

/// v0.2.4 `liquidate`'s price: `require_live`, then the bid price `P = clamp(M, R·lo, R·hi)`.
/// Test-only since Plan 9: production prices through `gate::liquidation_price` and
/// `gate::bid_pricing`; the tests keep it as the v0.2.4 reference those prices must equal at a
/// zero-confidence reading under the legacy preset.
#[cfg(test)]
pub fn live_price(coll: &CollateralState, obs: &Observed, r_ray: u128) -> Result<u128, Refusal> {
    let m = require_live(coll, obs)?;
    Ok(auction_core::price::bid_price(
        m,
        r_ray,
        &coll.feed_params()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures::coll;
    use auction_core::units::RAY;

    fn reading(ts: u64) -> OracleReading {
        OracleReading {
            price_raw: RAY,
            confidence_raw: 0,
            timestamp_ms: ts,
        }
    }

    fn fresh() -> CollateralState {
        CollateralState {
            paused_since_ms: None,
            paused_total_ms: 0,
            last_seen_oracle_ts: None,
            ..coll()
        }
    }

    #[test]
    fn sanity_window_is_a_parameter() {
        let wide = OracleReading {
            price_raw: 3 * RAY,
            ..reading(0)
        };
        let legacy = fresh();
        let obs = observe(&legacy, &wide, RAY, 0, 0).expect("observes");
        assert_eq!(obs.feed, Err(FeedFault::OutOfRange));
        let mut widened = fresh();
        widened.ext.risk.sanity_hi_bps = 40_000;
        let obs = observe(&widened, &wide, RAY, 0, 0).expect("observes");
        assert_eq!(obs.feed, Ok(3 * RAY));
    }

    #[test]
    fn feed_age_counts_the_slack() {
        let c = fresh();
        // max age 3_600_000: at now + slack exactly max age old, still valid.
        let ok = observe(&c, &reading(0), RAY, 3_600_000 - 6_000, 6_000).expect("observes");
        assert_eq!(ok.feed, Ok(RAY));
        let stale = observe(&c, &reading(0), RAY, 3_600_001 - 6_000, 6_000).expect("observes");
        assert_eq!(stale.feed, Err(FeedFault::Stale));
        assert_eq!(
            require_live(&c, &stale),
            Err(EngineError::PriceFeedStale.into())
        );
        // The derived onset is ts + max age, capped at the claimed now.
        assert_eq!(stale.clock.paused_since_ms, Some(3_600_001 - 6_000));
    }

    #[test]
    fn admin_pause_starts_the_clock_at_now_and_refuses_liquidation() {
        let paused = CollateralState {
            admin_paused: true,
            ..fresh()
        };
        let obs = observe(&paused, &reading(100), RAY, 500, 6_000).expect("observes");
        assert_eq!(obs.clock.paused_since_ms, Some(500));
        assert_eq!(require_live(&paused, &obs), Err(EngineError::Paused.into()));
        let resumed = CollateralState {
            admin_paused: false,
            ..paused.with_pause_clock(obs.clock)
        };
        let obs = observe(&resumed, &reading(10_000), RAY, 10_500, 6_000).expect("observes");
        assert_eq!(
            (obs.clock.paused_since_ms, obs.clock.paused_total_ms),
            (None, 10_000)
        );
        assert_eq!(require_live(&resumed, &obs), Ok(RAY));
    }

    #[test]
    fn live_price_clamps_to_the_band() {
        let c = fresh();
        let at = |price_raw| {
            let r = OracleReading {
                price_raw,
                ..reading(0)
            };
            live_price(&c, &observe(&c, &r, RAY, 0, 6_000).expect("observes"), RAY)
        };
        // Band [0.9, 1.1] × R.
        assert_eq!(at(RAY * 3 / 4), Ok(RAY * 9 / 10));
        assert_eq!(at(RAY), Ok(RAY));
        assert_eq!(at(RAY * 3 / 2), Ok(RAY * 11 / 10));
    }

    #[test]
    fn out_of_band_and_unknown_scale_fail_as_in_v024() {
        let c = fresh();
        let low = OracleReading {
            price_raw: RAY / 2 - 1,
            ..reading(0)
        };
        let obs = observe(&c, &low, RAY, 0, 6_000).expect("observes");
        assert_eq!(
            require_live(&c, &obs),
            Err(EngineError::PriceFeedOutOfRange.into())
        );
        let bad = CollateralState {
            oracle_scale: 9,
            ..c
        };
        assert_eq!(
            observe(&bad, &reading(0), RAY, 0, 6_000),
            Err(EngineError::InvalidConfig)
        );
    }

    #[test]
    fn pinned_reading_pauses_refuses_with_its_own_code_and_is_not_recorded() {
        let on = CollateralState {
            ext: CollateralExt::new(risk_core::RiskArgs {
                feed_min_raw: RAY / 4,
                feed_max_raw: RAY * 2,
                ..risk_core::RiskArgs::recommended()
            }),
            ..fresh()
        };
        for price_raw in [RAY / 4, RAY * 2] {
            let pinned = OracleReading {
                price_raw,
                ..reading(1_000)
            };
            let obs = observe(&on, &pinned, RAY, 1_000, 6_000).expect("observes");
            assert_eq!(obs.feed, Err(FeedFault::Pinned));
            assert_eq!(obs.conf_bps, 0);
            assert_eq!(obs.clock.paused_since_ms, Some(1_000));
            assert_eq!(
                require_live(&on, &obs),
                Err(Refusal::Risk(RiskError::PriceFeedPinned))
            );
            assert_eq!(graced_like(record(&on, &pinned, &obs, 1_000), &on), on);
        }
        // A stale fault keeps its v0.2.4 code.
        assert_eq!(
            feed_refusal(FeedFault::Stale),
            Refusal::Engine(EngineError::PriceFeedStale)
        );
    }

    /// `coll` with the three grace fields of `like` (Task 7: `record` writes them on every
    /// observation, so the ring checks below compare everything else).
    fn graced_like(coll: CollateralState, like: &CollateralState) -> CollateralState {
        CollateralState {
            ext: CollateralExt {
                last_good_ts_ms: like.ext.last_good_ts_ms,
                feed_invalid_since_ms: like.ext.feed_invalid_since_ms,
                grace_until_ms: like.ext.grace_until_ms,
                ..coll.ext.clone()
            },
            ..coll
        }
    }

    #[test]
    fn record_tracks_the_grace_state() {
        // Recommended preset: grace 1_800_000, minimum gap 600_000; fixture max age 3_600_000.
        let on = CollateralState {
            ext: CollateralExt::new(risk_core::RiskArgs::recommended()),
            ..fresh()
        };
        let step = |c: &CollateralState, r: &OracleReading, now: u64| {
            let obs = observe(c, r, RAY, now, 6_000).expect("observes");
            record(c, r, &obs, now)
        };
        let good = step(&on, &reading(1_000), 1_000);
        assert_eq!(good.ext.last_good_ts_ms, Some(1_000));
        assert_eq!(good.ext.ring.samples.len(), 1);
        // Stale at 3_700_000: the outage began when the reading aged out, at 3_601_000.
        let invalid = step(&good, &reading(1_000), 3_700_000);
        assert_eq!(invalid.ext.feed_invalid_since_ms, Some(3_601_000));
        assert_eq!(invalid.ext.last_good_ts_ms, Some(1_000));
        assert_eq!(invalid.ext.ring, good.ext.ring, "the ring is left");
        assert_eq!(invalid.ext.grace_until_ms, 0);
        // Back after 499_000 ms: below the minimum gap, no grace.
        let short = step(&invalid, &reading(4_100_000), 4_100_000);
        assert_eq!(short.ext.feed_invalid_since_ms, None);
        assert_eq!(short.ext.last_good_ts_ms, Some(4_100_000));
        assert_eq!(short.ext.grace_until_ms, 0);
        // Back after 699_000 ms: grace until now + 1_800_000.
        let back = step(&invalid, &reading(4_300_000), 4_300_000);
        assert_eq!(back.ext.feed_invalid_since_ms, None);
        assert_eq!(back.ext.last_good_ts_ms, Some(4_300_000));
        assert_eq!(back.ext.grace_until_ms, 6_100_000);
        assert_eq!(
            back.ext.ring.samples.len(),
            2,
            "the valid reading is recorded"
        );
    }

    #[test]
    fn observe_reports_conf_bps_and_record_takes_valid_readings_only() {
        let off = fresh();
        let on = CollateralState {
            ext: CollateralExt::new(risk_core::RiskArgs::recommended()),
            ..fresh()
        };
        let at = |price_raw, confidence_raw, ts| OracleReading {
            price_raw,
            confidence_raw,
            timestamp_ms: ts,
        };
        // 50 bps confidence on a valid reading; 0 on an invalid one.
        let good = at(RAY, RAY / 200, 1_000);
        let obs = observe(&on, &good, RAY, 1_000, 6_000).expect("observes");
        assert_eq!((obs.feed, obs.conf_bps), (Ok(RAY), 50));
        let recorded = record(&on, &good, &obs, 1_000);
        assert_eq!(recorded.ext.ring.samples.len(), 1);
        assert_eq!(recorded.ext.ring.last_conf_bps, 50);
        assert_eq!(
            CollateralState {
                ext: on.ext.clone(),
                ..recorded
            },
            on,
            "only the ring changes"
        );
        // TWAP off: nothing recorded.
        let obs_off = observe(&off, &good, RAY, 1_000, 6_000).expect("observes");
        assert_eq!(graced_like(record(&off, &good, &obs_off, 1_000), &off), off);
        // Out of range: conf 0 and nothing recorded.
        let low = at(RAY / 2 - 1, 0, 1_000);
        let bad = observe(&on, &low, RAY, 1_000, 6_000).expect("observes");
        assert_eq!(bad.conf_bps, 0);
        assert_eq!(graced_like(record(&on, &low, &bad, 1_000), &on), on);
    }
}
