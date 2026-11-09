//! The Tier 2 price gates (spec §3.1.4-§3.1.5, ADR-071, ADR-072): the liquidation price behind
//! the TWAP and the spot-vs-TWAP breaker, and the bid price under the one-sided TWAP anchor.
//!
//! `coll` is always the observed state with this observation's sample already recorded
//! (`feed::record`), so the TWAP includes the current reading when the ring took it. With the
//! TWAP off (`twap_window_ms == 0`, the legacy preset) both prices are `band(edge(M))`, which is
//! v0.2.4's `bid_price(M)` at a zero-confidence reading.

use auction_core::price::FeedParams;
use risk_core::{RiskError, liq_price, live_bid_price, twap, within_deviation};

use crate::{
    check::Refusal,
    feed::{Observed, require_live},
    state::CollateralState,
};

/// The bid's price and its stale-feed (D4) context. Plan 9 Task 7 prices live bids only, so
/// `fallback` is `false` and `paused_since_ms` is `None`.
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

/// The liquidation price, checked in this order: admin pause (`Paused`); an invalid feed (its
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
    let m = require_live(coll, obs)?;
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

/// The live bid price `band(min(edge(M), floor(T · (10_000 + dev) / 10_000)))`. The anchor
/// applies only with the TWAP and the breaker band both on, and only once the TWAP is
/// available: a bid during warm-up is priced at `band(edge(M))`, never refused (ruling R9-5).
/// An admin pause and an invalid feed are refused as before.
///
/// # Errors
/// `Paused` or the feed's own `EngineError`.
pub fn bid_pricing(
    coll: &CollateralState,
    obs: &Observed,
    r_ray: u128,
    now_ms: u64,
) -> Result<BidPricing, Refusal> {
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
    use crate::ext::CollateralExt;
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
}
