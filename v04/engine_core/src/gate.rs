//! The Tier 2 price gates (spec §3.1.4-§3.1.5, ADR-071, ADR-072): the liquidation price behind
//! the TWAP and the spot-vs-TWAP breaker, the bid price under the one-sided TWAP anchor, and
//! the D4 fallback bid price on a stale feed (spec §3.2.3, ADR-074).
//!
//! `coll` is always the observed state with this observation's sample already recorded
//! (`feed::record`), so the TWAP includes the current reading when the ring took it. With the
//! TWAP off (`twap_window_ms == 0`, the legacy preset) both prices are `band(edge(M))`, which is
//! v0.2.4's `bid_price(M)` at a zero-confidence reading.

use auction_core::price::FeedParams;
use auction_engine_params::EngineError;
use risk_core::{
    RiskError, fallback_bid_price, fallback_twap, liq_price, live_bid_price,
    pause::{FeedCase, Gate, Level, Op, allowed},
    twap, within_deviation,
};

use crate::{
    check::Refusal,
    feed::{Observed, require_live},
    state::{CollateralState, ConfigState},
};

/// A stored pause level as a [`Level`]; above 3 it is `InvalidConfig`.
fn level_of(level: u8) -> Result<Level, Refusal> {
    Level::from_u8(level).ok_or(Refusal::Engine(EngineError::InvalidConfig))
}

/// The collateral actor's pause gate (spec §3.2.2, ADR-073) for `op` with the feed valid or not.
/// The legacy `admin_paused` flag means level 3 for this type with v0.2.4's code: what level 3
/// refuses is `Paused` (6020). Otherwise the table at the type's `pause_level`.
///
/// # Errors
/// `Paused`; `InvalidConfig` for a stored level above 3; the table's refusal.
pub fn collateral_gate(coll: &CollateralState, op: Op, feed_valid: bool) -> Result<Gate, Refusal> {
    let feed = if feed_valid {
        FeedCase::Live
    } else {
        FeedCase::Stale
    };
    if coll.admin_paused && allowed(op, Level::Halted, feed).is_err() {
        return Err(EngineError::Paused.into());
    }
    Ok(allowed(op, level_of(coll.ext.pause_level)?, feed)?)
}

/// The config's pause gate for a system operation at `ext.system_level`. System operations do
/// not read the collateral feed, so the feed case is `Live`.
///
/// # Errors
/// `InvalidConfig` for a stored level above 3; the table's refusal.
pub fn system_gate(cfg: &ConfigState, op: Op) -> Result<(), Refusal> {
    allowed(op, level_of(cfg.ext.system_level)?, FeedCase::Live)?;
    Ok(())
}

/// The liquidation's market price behind the collateral gate: the gate first, then the live
/// feed. A `FeedRefusal` is the feed's own code; the table gives `Fallback` to bids only, and
/// [`bid_pricing`] handles it, so here it is the feed's own code too.
fn gated_market(coll: &CollateralState, obs: &Observed, op: Op) -> Result<u128, Refusal> {
    match collateral_gate(coll, op, obs.feed.is_ok())? {
        // The feed is valid: its market price.
        Gate::Run => Ok(require_live(coll, obs)?),
        // The feed is invalid: `require_live` returns its own code.
        Gate::FeedRefusal | Gate::Fallback => Ok(require_live(coll, obs)?),
    }
}

/// The bid's price and its stale-feed (D4) context: a live bid has `fallback: false` and
/// `paused_since_ms: None`; a fallback bid carries the observed open pause, so the auction's
/// discount stays frozen (spec §3.2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BidPricing {
    pub price_ray: u128,
    pub fallback: bool,
    pub paused_since_ms: Option<u64>,
}

/// The ring's TWAP at `now_ms` under the type's window, minimum count and feed max age.
fn twap_at(coll: &CollateralState, feed: &FeedParams, now_ms: u64) -> Option<u128> {
    let risk = &coll.ext.risk;
    twap(
        &coll.ext.ring,
        now_ms,
        risk.twap_window_ms,
        risk.twap_min_obs,
        feed.max_age_ms,
    )
    .map(|(t, _)| t)
}

/// The liquidation price, checked in this order: the pause gate (`Paused` under the admin
/// flag, else `LiquidationsPaused` at level 1 or more); an invalid feed (its
/// own `EngineError`); with the TWAP off, `band(edge(M))`; else no TWAP (`TwapUnavailable`); a
/// spot outside the breaker band when the breaker is on (`SpotTwapDeviation`); then
/// `band(edge(min(M, T)))`.
///
/// # Errors
/// The first refusal above.
pub fn liquidation_price(
    coll: &CollateralState,
    obs: &Observed,
    r_ray: u128,
    now_ms: u64,
) -> Result<u128, Refusal> {
    let m = gated_market(coll, obs, Op::Liquidate)?;
    let feed = coll.feed_params()?;
    let risk = coll.ext.risk;
    if risk.twap_window_ms == 0 {
        return Ok(liq_price(m, None, obs.conf_bps, r_ray, &feed));
    }
    let t = twap_at(coll, &feed, now_ms).ok_or(RiskError::TwapUnavailable)?;
    let dev = risk.max_spot_twap_dev_bps;
    if dev > 0 && !within_deviation(m, t, dev) {
        return Err(RiskError::SpotTwapDeviation.into());
    }
    Ok(liq_price(m, Some(t), obs.conf_bps, r_ray, &feed))
}

/// The D4 fallback price `band(edge(T_lg))` at the ring's last confidence, with the observed
/// open pause; `None` when D4 is off. `coll` is the observed state, whose pause clock the
/// invalid feed has opened.
fn fallback_pricing(
    coll: &CollateralState,
    r_ray: u128,
    now_ms: u64,
) -> Result<Option<BidPricing>, Refusal> {
    let feed = coll.feed_params()?;
    let ring = &coll.ext.ring;
    let Some(t) = fallback_twap(ring, now_ms, &coll.ext.risk, feed.max_age_ms)? else {
        return Ok(None);
    };
    Ok(Some(BidPricing {
        price_ray: fallback_bid_price(t, ring.last_conf_bps, r_ray, &feed),
        fallback: true,
        paused_since_ms: coll.paused_since_ms,
    }))
}

/// The bid price. The pause gate runs first (`Paused` under the admin flag, `BidsPaused` at
/// level 3). On an invalid feed at level 2 or below the D4 fallback decides (spec §3.2.3):
/// `TwapUnavailable` without `min_obs` samples, `LastGoodTwapTooOld` past
/// `stale_bid_max_age_ms`, else the fallback price; with D4 off, the feed's own code.
///
/// A live bid is priced at `band(min(edge(M), floor(T · (10_000 + dev) / 10_000)))`. The anchor
/// applies only with the TWAP and the breaker band both on, and only once the TWAP is
/// available: a bid during warm-up is priced at `band(edge(M))`, never refused (ruling R9-5).
///
/// # Errors
/// `Paused`, `BidsPaused`, `InvalidConfig`, `TwapUnavailable`, `LastGoodTwapTooOld` or the
/// feed's own `EngineError`.
pub fn bid_pricing(
    coll: &CollateralState,
    obs: &Observed,
    r_ray: u128,
    now_ms: u64,
) -> Result<BidPricing, Refusal> {
    let gate = collateral_gate(coll, Op::Bid, obs.feed.is_ok())?;
    if gate == Gate::Fallback
        && let Some(pricing) = fallback_pricing(coll, r_ray, now_ms)?
    {
        return Ok(pricing);
    }
    // `Run`: the live market. `FeedRefusal`, or `Fallback` with D4 off: the feed's own code.
    let m = require_live(coll, obs)?;
    let feed = coll.feed_params()?;
    let risk = coll.ext.risk;
    let dev = risk.max_spot_twap_dev_bps;
    let t = if risk.twap_window_ms > 0 && dev > 0 {
        twap_at(coll, &feed, now_ms)
    } else {
        None
    };
    Ok(BidPricing {
        price_ray: live_bid_price(m, t, obs.conf_bps, dev, r_ray, &feed),
        fallback: false,
        paused_since_ms: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ext::{CollateralExt, ConfigExt};
    use crate::state::fixtures;
    use auction_core::{error::PauseReason, math::mul_div_floor, price::bid_price, units::RAY};
    use auction_engine_params::EngineError;
    use risk_core::{RiskArgs, TwapRing, edge, record};

    /// Spacing of the recommended preset, ms.
    const SPACING: u64 = 300_000;

    /// The fixture under `risk` with `ring`, live (no admin pause).
    fn coll_with(risk: RiskArgs, ring: TwapRing) -> CollateralState {
        let base = fixtures::coll();
        CollateralState {
            admin_paused: false,
            ext: CollateralExt {
                risk,
                ring,
                ..base.ext.clone()
            },
            ..base
        }
    }

    /// The fixture under the recommended preset with an empty ring.
    fn coll() -> CollateralState {
        coll_with(RiskArgs::recommended(), TwapRing::default())
    }

    /// Three samples at RAY at 0, SPACING and 2 · SPACING.
    fn warm_ring() -> TwapRing {
        [0, SPACING, 2 * SPACING]
            .iter()
            .fold(TwapRing::default(), |r, &ts| {
                record(&r, ts, RAY, 0, ts, SPACING)
            })
    }

    fn obs(c: &CollateralState, feed: Result<u128, PauseReason>, conf_bps: u16) -> Observed {
        Observed {
            clock: c.pause_clock(),
            feed,
            conf_bps,
        }
    }

    /// 1_000 ms after the newest sample.
    const NOW: u64 = 2 * SPACING + 1_000;

    #[test]
    fn liq_gate_order_admin_feed_twap_deviation() {
        let paused = CollateralState {
            admin_paused: true,
            ..coll()
        };
        let stale = Err(PauseReason::Stale);
        // 1. Admin pause before an invalid feed and a missing TWAP.
        assert_eq!(
            liquidation_price(&paused, &obs(&paused, stale, 0), RAY, NOW),
            Err(Refusal::Engine(EngineError::Paused))
        );
        // 2. The feed's own code before a missing TWAP.
        let c = coll();
        assert_eq!(
            liquidation_price(&c, &obs(&c, stale, 0), RAY, NOW),
            Err(Refusal::Engine(EngineError::PriceFeedStale))
        );
        // 3. No TWAP before the breaker: a spike with an empty ring is TwapUnavailable.
        assert_eq!(
            liquidation_price(&c, &obs(&c, Ok(RAY * 13 / 10), 0), RAY, NOW),
            Err(Refusal::Risk(RiskError::TwapUnavailable))
        );
        // 4. With the TWAP at RAY, a 30 % spike trips the 20 % breaker.
        let warm = coll_with(RiskArgs::recommended(), warm_ring());
        assert_eq!(
            liquidation_price(&warm, &obs(&warm, Ok(RAY * 13 / 10), 0), RAY, NOW),
            Err(Refusal::Risk(RiskError::SpotTwapDeviation))
        );
        // 5. Inside the band: band(edge(min(M, T))).
        let m = RAY * 105 / 100;
        let t = twap_at(&warm, &warm.feed_params().expect("feed"), NOW).expect("a twap");
        assert!(t <= RAY && t.saturating_add(7) >= RAY, "{t}");
        assert_eq!(
            liquidation_price(&warm, &obs(&warm, Ok(m), 30), RAY, NOW),
            Ok(bid_price(
                edge(t, 30),
                RAY,
                &warm.feed_params().expect("feed")
            ))
        );
    }

    #[test]
    fn legacy_liquidation_price_is_band_edge_m() {
        let c = coll_with(RiskArgs::legacy(), TwapRing::default());
        let feed = c.feed_params().expect("feed");
        for (m, conf) in [(RAY, 0), (RAY, 50), (RAY * 12 / 10, 50), (RAY * 8 / 10, 0)] {
            assert_eq!(
                liquidation_price(&c, &obs(&c, Ok(m), conf), RAY, NOW),
                Ok(bid_price(edge(m, conf), RAY, &feed)),
                "{m} {conf}"
            );
        }
        // At a zero-confidence reading it is v0.2.4's price.
        let o = obs(&c, Ok(RAY * 12 / 10), 0);
        assert_eq!(
            liquidation_price(&c, &o, RAY, NOW).ok(),
            crate::feed::live_price(&c, &o, RAY).ok()
        );
    }

    #[test]
    fn bid_pricing_anchors_once_warm_and_skips_the_breaker() {
        // Fixture band [0.9 R, 1.1 R]; R = 1.3 RAY keeps 1.2 and 1.4 RAY inside it.
        let r = RAY * 13 / 10;
        let warm = coll_with(RiskArgs::recommended(), warm_ring());
        let feed = warm.feed_params().expect("feed");
        let t = twap_at(&warm, &feed, NOW).expect("a twap");
        let anchor = mul_div_floor(t, 12_000, 10_000).expect("fits");
        assert!(anchor < RAY * 14 / 10 && bid_price(anchor, r, &feed) == anchor);
        // A 40 % spike is not refused for a bid; it pays at the anchor floor(T · 1.2).
        let priced = bid_pricing(&warm, &obs(&warm, Ok(RAY * 14 / 10), 0), r, NOW);
        assert_eq!(
            priced,
            Ok(BidPricing {
                price_ray: anchor,
                fallback: false,
                paused_since_ms: None,
            })
        );
        // Warm-up (empty ring): band(edge(M)), not TwapUnavailable.
        let cold = coll();
        let priced = bid_pricing(&cold, &obs(&cold, Ok(RAY * 14 / 10), 50), r, NOW);
        assert_eq!(
            priced.map(|p| p.price_ray),
            Ok(bid_price(edge(RAY * 14 / 10, 50), r, &feed))
        );
        // Admin pause still refuses.
        let paused = CollateralState {
            admin_paused: true,
            ..warm
        };
        assert_eq!(
            bid_pricing(&paused, &obs(&paused, Ok(RAY), 0), r, NOW),
            Err(Refusal::Engine(EngineError::Paused))
        );
    }

    #[test]
    fn admin_pause_maps_level_3_refusals_to_paused() {
        let at = |level: u8, admin_paused: bool| {
            let base = coll();
            CollateralState {
                admin_paused,
                ext: CollateralExt {
                    pause_level: level,
                    ..base.ext.clone()
                },
                ..base
            }
        };
        let paused = Err(Refusal::Engine(EngineError::Paused));
        // `admin_paused` at level 1: what level 3 refuses is `Paused` (v0.2.4's code), live or
        // stale.
        let admin = at(1, true);
        for feed_valid in [true, false] {
            assert_eq!(collateral_gate(&admin, Op::Liquidate, feed_valid), paused);
            assert_eq!(collateral_gate(&admin, Op::Bid, feed_valid), paused);
        }
        // What level 3 allows still runs under the admin flag.
        assert_eq!(
            collateral_gate(&admin, Op::ObserveFeed, false),
            Ok(Gate::Run)
        );
        assert_eq!(
            collateral_gate(&admin, Op::SettleEmpty, true),
            Ok(Gate::Run)
        );
        // Level 3 without the admin flag keeps the Tier 2 codes.
        let halted = at(3, false);
        assert_eq!(
            collateral_gate(&halted, Op::Liquidate, true),
            Err(Refusal::Risk(RiskError::LiquidationsPaused))
        );
        assert_eq!(
            collateral_gate(&halted, Op::Bid, true),
            Err(Refusal::Risk(RiskError::BidsPaused))
        );
        // Level 0: a stale feed is the feed's refusal for a liquidation, the fallback for a bid.
        let normal = at(0, false);
        assert_eq!(
            collateral_gate(&normal, Op::Liquidate, false),
            Ok(Gate::FeedRefusal)
        );
        assert_eq!(collateral_gate(&normal, Op::Bid, false), Ok(Gate::Fallback));
        // An out-of-range stored level is `InvalidConfig`.
        assert_eq!(
            collateral_gate(&at(4, false), Op::Bid, true),
            Err(Refusal::Engine(EngineError::InvalidConfig))
        );
        // The prices refuse at the gate: level 1 stops a liquidation, not a bid.
        let one = at(1, false);
        assert_eq!(
            liquidation_price(&one, &obs(&one, Ok(RAY), 0), RAY, NOW),
            Err(Refusal::Risk(RiskError::LiquidationsPaused))
        );
        assert!(bid_pricing(&one, &obs(&one, Ok(RAY), 0), RAY, NOW).is_ok());
        // A stale bid at level 0 takes the D4 fallback: with an empty ring, TwapUnavailable.
        assert_eq!(
            bid_pricing(&normal, &obs(&normal, Err(PauseReason::Stale), 0), RAY, NOW),
            Err(Refusal::Risk(RiskError::TwapUnavailable))
        );
    }

    #[test]
    fn stale_bid_prices_at_the_last_good_twap() {
        let r = RAY * 13 / 10;
        let stale = Err(PauseReason::Stale);
        // A feed max age that passes ruling R9-3 (the fixture's 1 h would leave only the newest
        // sample inside the frozen window, so D4 could never fire).
        let warm = CollateralState {
            paused_since_ms: Some(NOW),
            max_oracle_age_ms: 600_000,
            ..coll_with(RiskArgs::recommended(), warm_ring())
        };
        let feed = warm.feed_params().expect("feed");
        let t = twap_at(&warm, &feed, NOW).expect("a twap");
        let expected = BidPricing {
            price_ray: risk_core::fallback_bid_price(t, warm.ext.ring.last_conf_bps, r, &feed),
            fallback: true,
            paused_since_ms: Some(NOW),
        };
        assert_eq!(
            bid_pricing(&warm, &obs(&warm, stale, 0), r, NOW),
            Ok(expected)
        );
        // Every invalid feed takes the fallback, not only a stale one.
        let wide = Err(PauseReason::LowConfidence);
        assert_eq!(
            bid_pricing(&warm, &obs(&warm, wide, 0), r, NOW),
            Ok(expected)
        );
        // The newest sample more than stale_bid_max_age_ms before now.
        const NEWEST: u64 = 2 * SPACING;
        assert_eq!(
            bid_pricing(&warm, &obs(&warm, stale, 0), r, NEWEST + 3_600_001),
            Err(Refusal::Risk(RiskError::LastGoodTwapTooOld))
        );
        assert!(bid_pricing(&warm, &obs(&warm, stale, 0), r, NEWEST + 3_600_000).is_ok());
        // D4 off (legacy, or no stale window): the feed's own code.
        let legacy = coll_with(RiskArgs::legacy(), warm_ring());
        assert_eq!(
            bid_pricing(&legacy, &obs(&legacy, stale, 0), r, NOW),
            Err(Refusal::Engine(EngineError::PriceFeedStale))
        );
        let no_d4 = coll_with(
            RiskArgs {
                stale_bid_max_age_ms: 0,
                ..RiskArgs::recommended()
            },
            warm_ring(),
        );
        assert_eq!(
            bid_pricing(&no_d4, &obs(&no_d4, wide, 0), r, NOW),
            Err(Refusal::Engine(EngineError::PriceFeedLowConfidence))
        );
        // Level 3 and the admin flag still refuse first.
        let halted = CollateralState {
            ext: CollateralExt {
                pause_level: 3,
                ..warm.ext.clone()
            },
            ..warm.clone()
        };
        assert_eq!(
            bid_pricing(&halted, &obs(&halted, stale, 0), r, NOW),
            Err(Refusal::Risk(RiskError::BidsPaused))
        );
        let paused = CollateralState {
            admin_paused: true,
            ..warm
        };
        assert_eq!(
            bid_pricing(&paused, &obs(&paused, stale, 0), r, NOW),
            Err(Refusal::Engine(EngineError::Paused))
        );
    }

    #[test]
    fn system_gate_reads_the_system_level() {
        let base = fixtures::config();
        let at = |system_level: u8| ConfigState {
            ext: ConfigExt { system_level },
            ..base.clone()
        };
        assert_eq!(system_gate(&at(1), Op::StartSurplus), Ok(()));
        assert_eq!(
            system_gate(&at(2), Op::StartDebt),
            Err(Refusal::Risk(RiskError::SystemAuctionsPaused))
        );
        assert_eq!(system_gate(&at(2), Op::TakeSurplus), Ok(()));
        assert_eq!(
            system_gate(&at(3), Op::TakeDebt),
            Err(Refusal::Risk(RiskError::BidsPaused))
        );
        assert_eq!(system_gate(&at(3), Op::CloseDebt), Ok(()));
        assert_eq!(
            system_gate(&at(4), Op::CloseDebt),
            Err(Refusal::Engine(EngineError::InvalidConfig))
        );
    }
}
