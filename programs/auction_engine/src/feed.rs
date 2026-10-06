//! Redemption price, oracle validity and the type-level pause observation (spec §5.2, §5.7).

use alloy_primitives::U512;
use auction_core::{
    error::PauseReason,
    pause::{Observation, OnsetPolicy, PauseClock},
    price::{validate_feed, OracleReading},
};
use auction_engine_core::{state::*, EngineError};
use lee_core::account::AccountWithMetadata;
use stablecoin_core::{
    compute_redemption_price_state_pda, math::compute_current_redemption_price_wide,
    RedemptionPriceState,
};
use twap_oracle_core::OraclePriceAccount;

pub fn u512_to_u128(v: U512) -> Result<u128, EngineError> {
    if v > U512::from(u128::MAX) {
        Err(EngineError::Math)
    } else {
        Ok(v.to::<u128>())
    }
}

/// R at `now`: the host's own projection, asserted to fit `u128` (spec §5.2).
pub fn redemption_price(
    cfg: &EngineConfigState,
    rps: &AccountWithMetadata,
    now: u64,
) -> Result<u128, EngineError> {
    if rps.account_id != compute_redemption_price_state_pda(cfg.host_program_id)
        || rps.account.program_owner != cfg.host_program_id
    {
        return Err(EngineError::WrongAccount("redemption_price_state"));
    }
    let s = RedemptionPriceState::try_from(&rps.account.data)
        .map_err(|_| EngineError::WrongAccount("redemption_price_state"))?;
    u512_to_u128(compute_current_redemption_price_wide(
        s.redemption_price_at_last_update,
        s.redemption_rate_per_millisecond,
        s.last_updated_at,
        now,
    ))
}

pub struct Observed {
    pub clock: PauseClock,
    pub feed: Result<u128, PauseReason>,
}

/// One pause observation at `now` (policy (b), ADR-007). `feed_valid` is derived with `validate_feed`
/// from the same `(oracle_ts, now, max_age)` that `observed` receives (ADR-024 obligation).
pub fn observe(
    coll: &CollateralConfigState,
    oracle: &AccountWithMetadata,
    r_ray: u128,
    now: u64,
) -> Result<Observed, EngineError> {
    if oracle.account_id != coll.oracle_id {
        return Err(EngineError::OracleMismatch);
    }
    let o = OraclePriceAccount::try_from(&oracle.account.data)
        .map_err(|_| EngineError::WrongAccount("oracle"))?;
    let params = coll.feed_params()?;
    let reading = OracleReading {
        price_raw: o.price,
        confidence_raw: o.confidence_interval,
        timestamp_ms: o.timestamp,
    };
    let feed = validate_feed(&reading, &params, r_ray, now);
    let clock = coll.pause_clock().observed(
        Observation {
            now_ms: now,
            feed_valid: feed.is_ok(),
            admin_paused: coll.admin_paused,
            oracle_ts: o.timestamp,
            max_age_ms: params.max_age_ms,
        },
        OnsetPolicy::DerivedFromOracle,
    );
    Ok(Observed { clock, feed })
}

/// The market price if the type is live, else the pause as an error (`Paused`, `PriceFeedStale`,
/// `PriceFeedLowConfidence`, `PriceFeedOutOfRange`). A reverting transaction drops its observation;
/// policy (b) recovers the onset from the oracle timestamp at the next successful one (ADR-032).
pub fn require_live(coll: &CollateralConfigState, obs: &Observed) -> Result<u128, EngineError> {
    if coll.admin_paused {
        return Err(EngineError::Paused);
    }
    obs.feed.map_err(EngineError::from)
}
