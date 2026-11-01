//! Oracle validity and the type-level pause observation (v0.2.4 `feed.rs`, spec §5.2, §5.7) on
//! readings that arrive by `StateReply` instead of account data.
//!
//! Time (design note §4.1): `now_ms` is the caller's claim, asserted to lie in
//! `[clock − slack, clock]`. The feed-age check uses `now_ms + slack_ms`, so an understated claim
//! cannot make a stale feed look fresh; the pause clock records `now_ms`, which favours the
//! protocol for `t_eff` as ADR-024 intends.

use auction_core::{
    error::PauseReason,
    pause::{Observation, OnsetPolicy, PauseClock},
    price::{OracleReading, bid_price, validate_feed},
};
use auction_engine_params::EngineError;

use crate::state::CollateralState;

/// One observation: the next pause clock and the feed verdict (the market price if valid).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observed {
    pub clock: PauseClock,
    pub feed: Result<u128, PauseReason>,
}

/// One pause observation at `now_ms` (policy (b), ADR-007). `feed_valid` comes from
/// `validate_feed` at `now_ms + slack_ms` with the same oracle timestamp and max age that
/// `observed` receives (ADR-024 obligation, with the conservative age of design note §4.1).
pub fn observe(
    coll: &CollateralState,
    reading: &OracleReading,
    r_ray: u128,
    now_ms: u64,
    slack_ms: u64,
) -> Result<Observed, EngineError> {
    let params = coll.feed_params()?;
    let age_now = now_ms.checked_add(slack_ms).ok_or(EngineError::Math)?;
    let feed = validate_feed(reading, &params, r_ray, age_now);
    let clock = coll.pause_clock().observed(
        Observation {
            now_ms,
            feed_valid: feed.is_ok(),
            admin_paused: coll.admin_paused,
            oracle_ts: reading.timestamp_ms,
            max_age_ms: params.max_age_ms,
        },
        OnsetPolicy::DerivedFromOracle,
    );
    Ok(Observed { clock, feed })
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
pub fn live_price(
    coll: &CollateralState,
    obs: &Observed,
    r_ray: u128,
) -> Result<u128, EngineError> {
    let m = require_live(coll, obs)?;
    Ok(bid_price(m, r_ray, &coll.feed_params()?))
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
}
