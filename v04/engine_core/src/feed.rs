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
    error::PauseReason,
    pause::{Observation, OnsetPolicy, PauseClock},
    price::{OracleReading, to_ray},
};
use auction_engine_params::EngineError;
use risk_core::{
    feed::{FeedFault, FeedPolicy, validate_feed_ext},
    price::conf_bps,
};

use crate::{ext::CollateralExt, state::CollateralState};

/// The confidence of an unconvertible reading, bps: the widest edge.
const CONF_SATURATED_BPS: u16 = 10_000;

/// One observation: the next pause clock, the feed verdict (the market price if valid) and the
/// reading's confidence in bps of the price (0 when the feed is invalid).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observed {
    pub clock: PauseClock,
    pub feed: Result<u128, PauseReason>,
    pub conf_bps: u16,
}

/// The type's feed policy. Plan 9 runs every type under v0.2.4's policy (sanity `[R/2, 2R]`,
/// no pin bounds). A free fn rather than a `CollateralState` method: `state.rs` is at its size
/// limit.
pub fn feed_policy(coll: &CollateralState) -> Result<FeedPolicy, EngineError> {
    Ok(FeedPolicy::legacy(&coll.feed_params()?))
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
    let feed = validate_feed_ext(reading, &policy, r_ray, age_now).map_err(FeedFault::pause_reason);
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

/// Offers this observation's reading to the TWAP ring (spec §3.1.2): `coll` with the ring after
/// `risk_core::record` when the feed is valid and the TWAP is on, else `coll` unchanged.
#[must_use]
pub fn record(
    coll: &CollateralState,
    reading: &OracleReading,
    obs: &Observed,
    now_ms: u64,
) -> CollateralState {
    let risk = coll.ext.risk;
    match obs.feed {
        Ok(m) if risk.twap_window_ms > 0 => CollateralState {
            ext: CollateralExt {
                risk,
                pause_level: coll.ext.pause_level,
                ring: risk_core::record(
                    &coll.ext.ring,
                    reading.timestamp_ms,
                    m,
                    obs.conf_bps,
                    now_ms,
                    risk.twap_min_spacing_ms,
                ),
            },
            ..coll.clone()
        },
        Ok(_) | Err(_) => coll.clone(),
    }
}

/// The market price if the type is live, else the pause as an error (`Paused`, `PriceFeedStale`,
/// `PriceFeedLowConfidence`, `PriceFeedOutOfRange`). A reverting transaction drops its observation;
/// policy (b) recovers the onset from the oracle timestamp at the next successful one (ADR-032).
pub fn require_live(coll: &CollateralState, obs: &Observed) -> Result<u128, EngineError> {
    if coll.admin_paused {
        return Err(EngineError::Paused);
    }
    obs.feed.map_err(EngineError::from)
}

/// v0.2.4 `liquidate`'s price: `require_live`, then the bid price `P = clamp(M, R·lo, R·hi)`.
/// Test-only since Plan 9: production prices through `gate::liquidation_price` and
/// `gate::bid_pricing`; the tests keep it as the v0.2.4 reference those prices must equal at a
/// zero-confidence reading under the legacy preset.
#[cfg(test)]
pub fn live_price(
    coll: &CollateralState,
    obs: &Observed,
    r_ray: u128,
) -> Result<u128, EngineError> {
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
    fn feed_age_counts_the_slack() {
        let c = fresh();
        // max age 3_600_000: at now + slack exactly max age old, still valid.
        let ok = observe(&c, &reading(0), RAY, 3_600_000 - 6_000, 6_000).expect("observes");
        assert_eq!(ok.feed, Ok(RAY));
        let stale = observe(&c, &reading(0), RAY, 3_600_001 - 6_000, 6_000).expect("observes");
        assert_eq!(stale.feed, Err(PauseReason::Stale));
        assert_eq!(require_live(&c, &stale), Err(EngineError::PriceFeedStale));
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
        assert_eq!(require_live(&paused, &obs), Err(EngineError::Paused));
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
            Err(EngineError::PriceFeedOutOfRange)
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
        assert_eq!(record(&off, &good, &obs_off, 1_000), off);
        // Out of range: conf 0 and nothing recorded.
        let low = at(RAY / 2 - 1, 0, 1_000);
        let bad = observe(&on, &low, RAY, 1_000, 6_000).expect("observes");
        assert_eq!(bad.conf_bps, 0);
        assert_eq!(record(&on, &low, &bad, 1_000), on);
    }
}
