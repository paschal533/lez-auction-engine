//! The confidence edge and the price per operation (spec §3.1.4, ADR-071, ADR-072). Every price
//! here is RAY collateral per coin and is a payout, so each one values the collateral at the high
//! edge of the oracle's confidence interval and rounds toward the smaller price: the confidence is
//! rounded up, the edge and the TWAP anchor are floored, and `band(x)` is v0.2.4's `bid_price`
//! clamp to `[R·lo, R·hi]`.

use auction_core::{
    math::{mul_div_ceil, mul_div_floor},
    price::{FeedParams, bid_price},
};

/// One in basis points.
const BPS: u128 = 10_000;
/// [`BPS`] as a `u16`, the saturation value of a confidence.
const BPS_U16: u16 = 10_000;

/// The confidence in basis points of `M`: `ceil(c · 10_000 / M)`, saturating at 10_000, and
/// 10_000 when `M == 0`. Rounded up, so the edge is never narrower than the oracle's.
#[must_use]
pub fn conf_bps(conf_ray: u128, m_ray: u128) -> u16 {
    if m_ray == 0 {
        return BPS_U16;
    }
    mul_div_ceil(conf_ray, BPS, m_ray)
        .ok()
        .and_then(|b| u16::try_from(b).ok())
        .map_or(BPS_U16, |b| b.min(BPS_U16))
}

/// The confidence edge: `floor(x · (10_000 - conf_bps) / 10_000)`, with `conf_bps` saturated at
/// 10_000. Floored, so it never exceeds `x`.
#[must_use]
pub fn edge(x: u128, conf_bps: u16) -> u128 {
    let keep = BPS.saturating_sub(u128::from(conf_bps.min(BPS_U16)));
    // `keep <= BPS`, so the quotient is at most `x` and cannot error; 0 is the safe side anyway.
    mul_div_floor(x, keep, BPS).unwrap_or(0)
}

/// The TWAP anchor `floor(T · (10_000 + dev) / 10_000)`, saturating to `u128::MAX` on overflow.
fn anchor(t: u128, dev_bps: u16) -> u128 {
    let up = BPS.saturating_add(u128::from(dev_bps));
    mul_div_floor(t, up, BPS).unwrap_or(u128::MAX)
}

/// The liquidation price `band(edge(min(M, T)))`, or `band(edge(M))` without a TWAP: a one-print
/// spike in either cannot inflate the keeper reward or the floor.
#[must_use]
pub fn liq_price(m: u128, t: Option<u128>, conf_bps: u16, r: u128, feed: &FeedParams) -> u128 {
    let base = t.map_or(m, |t| m.min(t));
    bid_price(edge(base, conf_bps), r, feed)
}

/// The live bid price `band(min(edge(M), floor(T · (10_000 + dev) / 10_000)))`: it follows the
/// market but is anchored from above only. Without a TWAP, or with `dev_bps == 0` (no limit,
/// ruling R9-5), it is `band(edge(M))`.
#[must_use]
pub fn live_bid_price(
    m: u128,
    t: Option<u128>,
    conf_bps: u16,
    dev_bps: u16,
    r: u128,
    feed: &FeedParams,
) -> u128 {
    let e = edge(m, conf_bps);
    let capped = match t {
        Some(t) if dev_bps != 0 => e.min(anchor(t, dev_bps)),
        Some(_) | None => e,
    };
    bid_price(capped, r, feed)
}

/// The stale-feed (D4) bid price `band(edge(T_lg))`, with the last-good TWAP's confidence.
#[must_use]
pub fn fallback_bid_price(t_lg: u128, last_conf_bps: u16, r: u128, feed: &FeedParams) -> u128 {
    bid_price(edge(t_lg, last_conf_bps), r, feed)
}

/// The spot/TWAP deviation test `floor(|M - T| · 10_000 / T) <= dev`, inclusive; `false` when
/// `T == 0` or the quotient does not fit.
#[must_use]
pub fn within_deviation(m: u128, t: u128, dev_bps: u16) -> bool {
    if t == 0 {
        return false;
    }
    mul_div_floor(m.abs_diff(t), BPS, t).is_ok_and(|d| d <= u128::from(dev_bps))
}

#[cfg(test)]
mod tests {
    use super::*;
    use auction_core::{math::mul_wide, price::OracleScale, units::RAY};
    use proptest::prelude::*;

    fn feed(band_lo_bps: u16, band_hi_bps: u16) -> FeedParams {
        FeedParams {
            scale: OracleScale::Ray,
            max_age_ms: 3_600_000,
            max_conf_bps: 1_000,
            band_lo_bps,
            band_hi_bps,
        }
    }

    fn wide() -> FeedParams {
        feed(5_000, 20_000)
    }

    /// With `r = u128::MAX` this band is the identity: `lo = 0` and `hi` saturates.
    fn open() -> FeedParams {
        feed(0, u16::MAX)
    }

    /// `a · b` as a 256-bit `(hi, lo)` pair, ordered like the number.
    fn w(a: u128, b: u128) -> (u128, u128) {
        mul_wide(a, b)
    }

    #[test]
    fn edge_lowers_p_by_the_confidence() {
        assert_eq!(edge(RAY, 100), RAY / 10_000 * 9_900);
        assert_eq!(edge(RAY, 0), RAY);
        assert_eq!(conf_bps(RAY / 100, RAY), 100);
        assert_eq!(conf_bps(1, RAY), 1);
        assert_eq!(conf_bps(0, RAY), 0);
    }

    #[test]
    fn liq_price_is_the_smaller_of_spot_and_twap_at_the_edge() {
        let f = wide();
        assert_eq!(
            liq_price(RAY, Some(RAY / 10 * 9), 100, RAY, &f),
            edge(RAY / 10 * 9, 100)
        );
        assert_eq!(liq_price(RAY, Some(RAY * 2), 100, RAY, &f), edge(RAY, 100));
        assert_eq!(liq_price(RAY, None, 100, RAY, &f), edge(RAY, 100));
    }

    #[test]
    fn bid_price_is_anchored_from_above_only() {
        let f = wide();
        assert_eq!(
            live_bid_price(RAY / 10 * 15, Some(RAY), 0, 2_000, RAY, &f),
            RAY / 10 * 12
        ); // capped
        assert_eq!(
            live_bid_price(RAY / 10 * 8, Some(RAY), 0, 2_000, RAY, &f),
            RAY / 10 * 8
        ); // not raised
        assert_eq!(
            live_bid_price(RAY / 10 * 15, Some(RAY), 0, 0, RAY, &f),
            RAY / 10 * 15
        ); // dev 0: off
        assert_eq!(
            live_bid_price(RAY / 10 * 15, None, 0, 2_000, RAY, &f),
            RAY / 10 * 15
        ); // no TWAP
    }

    #[test]
    fn deviation_is_floor_bps_inclusive() {
        assert!(within_deviation(1_200, 1_000, 2_000));
        assert!(!within_deviation(1_201, 1_000, 2_000));
        assert!(within_deviation(800, 1_000, 2_000));
        assert!(!within_deviation(799, 1_000, 2_000));
        assert!(!within_deviation(1, 0, 2_000));
    }

    #[test]
    fn prices_stay_inside_the_band() {
        let f = feed(9_000, 11_000);
        assert_eq!(liq_price(RAY * 2, None, 0, RAY, &f), RAY / 10 * 11);
        assert_eq!(fallback_bid_price(RAY / 2, 0, RAY, &f), RAY / 10 * 9);
    }

    /// Each rounding is pinned toward the protocol (the smaller price, the larger confidence).
    #[test]
    fn rounding_favours_the_protocol() {
        // conf rounds up: 10_000 / 10_001 = 0.9999 -> 1; 100.01 -> 101.
        assert_eq!(conf_bps(1, 10_001), 1);
        assert_eq!(conf_bps(10_001, 1_000_000), 101);
        assert_eq!(conf_bps(10_000, 1_000_000), 100); // exact stays exact
        // edge floors: 10_001 · 9_999 / 10_000 = 9_999.9999 -> 9_999.
        assert_eq!(edge(10_001, 1), 9_999);
        assert_eq!(edge(9_999, 1), 9_998); // 9_998.0001 -> 9_998
        assert_eq!(edge(1, 1), 0);
        // anchor floors: 10_001 · 10_001 / 10_000 = 10_002.0001 -> 10_002.
        assert_eq!(anchor(10_001, 1), 10_002);
        let f = open();
        assert_eq!(
            live_bid_price(20_000, Some(10_001), 0, 1, u128::MAX, &f),
            10_002
        );
        // the deviation floors, so 2_000.9 bps is still within 2_000 (inclusive floor rule).
        assert!(within_deviation(1_200_090, 1_000_000, 2_000));
        assert!(!within_deviation(1_200_100, 1_000_000, 2_000));
        // liq at the edge of the smaller of spot and TWAP, floored: 0.9 RAY · 0.99.
        assert_eq!(
            liq_price(RAY, Some(RAY / 10 * 9), 100, RAY, &wide()),
            RAY / 1_000 * 891
        );
    }

    #[test]
    fn saturation_at_the_extremes() {
        assert_eq!(conf_bps(5, 0), 10_000);
        assert_eq!(conf_bps(0, 0), 10_000);
        assert_eq!(conf_bps(RAY, RAY / 2), 10_000);
        assert_eq!(conf_bps(u128::MAX, 1), 10_000);
        assert_eq!(conf_bps(u128::MAX, u128::MAX), 10_000);
        assert_eq!(conf_bps(1, u128::MAX), 1);
        assert_eq!(edge(u128::MAX, 0), u128::MAX);
        assert_eq!(edge(u128::MAX, 10_000), 0);
        assert_eq!(edge(u128::MAX, u16::MAX), 0);
        assert_eq!(anchor(u128::MAX, 1), u128::MAX);
        assert_eq!(anchor(u128::MAX, 0), u128::MAX);
        assert_eq!(anchor(0, u16::MAX), 0);
        let f = wide();
        assert_eq!(
            live_bid_price(u128::MAX, Some(u128::MAX), 0, 2_000, u128::MAX, &f),
            u128::MAX
        );
        assert!(within_deviation(u128::MAX, u128::MAX, 0));
        assert!(!within_deviation(u128::MAX, 1, u16::MAX));
        assert!(!within_deviation(0, u128::MAX, 9_999));
        assert!(within_deviation(0, u128::MAX, 10_000));
    }

    /// A u128 biased onto 0, 1, the top of the range and RAY, else free.
    fn edge_u128() -> impl Strategy<Value = u128> {
        prop_oneof![
            Just(0u128),
            Just(1u128),
            Just(2u128),
            Just(RAY),
            Just(u128::MAX / 10_000),
            Just(u128::MAX / 10_000 + 1),
            Just(u128::MAX - 1),
            Just(u128::MAX),
            1u128..(1 << 100),
            any::<u128>(),
        ]
    }

    /// A basis-point value biased onto 0, 1, the 10_000 saturation and the u16 extreme.
    fn edge_bps() -> impl Strategy<Value = u16> {
        prop_oneof![
            Just(0u16),
            Just(1u16),
            Just(100u16),
            Just(1_000u16),
            Just(2_000u16),
            Just(9_999u16),
            Just(10_000u16),
            Just(10_001u16),
            Just(u16::MAX),
            any::<u16>(),
        ]
    }

    /// `base ± delta` for `delta` in -1..=1, saturating.
    fn nudge(base: u128, delta: i8) -> u128 {
        match delta {
            ..=-1 => base.saturating_sub(1),
            0 => base,
            1.. => base.saturating_add(1),
        }
    }

    /// A band edge of `r`: `R·lo` or `R·hi`, ±1, or `free`.
    fn at_band(r: u128, f: &FeedParams, sel: u8, delta: i8, free: u128) -> u128 {
        let at = |bps: u16| mul_div_floor(r, u128::from(bps), BPS).unwrap_or(u128::MAX);
        match sel {
            0 => nudge(at(f.band_lo_bps), delta),
            1 => nudge(at(f.band_hi_bps), delta),
            _ => free,
        }
    }

    proptest! {
        /// `conf_bps(c, M) <= limit` exactly when `c · 10_000 <= limit · M`: the ceiling never
        /// lets a reading past the limit, biased onto `c` at the limit and one past it.
        #[test]
        fn conf_bps_is_the_exact_ceiling(
            m in edge_u128(), limit in edge_bps(), delta in -1i8..=1,
            exact in any::<bool>(), free in edge_u128(),
        ) {
            let limit = limit.min(BPS_U16);
            let c = if exact {
                nudge(mul_div_floor(m, u128::from(limit), BPS).unwrap_or(u128::MAX), delta)
            } else {
                free
            };
            let b = conf_bps(c, m);
            prop_assert!(b <= BPS_U16);
            if m == 0 {
                prop_assert_eq!(b, BPS_U16);
            } else {
                // At limit 10_000 the saturation makes every reading pass.
                prop_assert_eq!(
                    b <= limit,
                    limit == BPS_U16 || w(c, BPS) <= w(u128::from(limit), m)
                );
                if c == 0 { prop_assert_eq!(b, 0); }
                if c >= m { prop_assert_eq!(b, BPS_U16); }
            }
        }

        /// `edge(x, b)` is the floor: `e · 10_000 <= x · keep < (e + 1) · 10_000`, never above
        /// `x`, and falling as the confidence grows.
        #[test]
        fn edge_is_the_floor_and_monotone(x in edge_u128(), b in edge_bps(), b2 in edge_bps()) {
            let e = edge(x, b);
            let keep = BPS.saturating_sub(u128::from(b.min(BPS_U16)));
            prop_assert!(e <= x);
            prop_assert!(w(e, BPS) <= w(x, keep));
            prop_assert!(w(x, keep) < w(e.saturating_add(1), BPS) || e == u128::MAX);
            if b <= b2 { prop_assert!(edge(x, b2) <= e); }
        }

        /// Liquidation and fallback prices: inside the band, never above the spot-only price,
        /// and `liq(M, Some(T)) == liq(min(M, T), None) == fallback(min(M, T))`.
        #[test]
        fn liq_and_fallback_stay_in_band_and_take_the_smaller(
            r in edge_u128(), lo in edge_bps(), hi in edge_bps(), conf in edge_bps(),
            m_sel in 0u8..=2, t_sel in 0u8..=2, dm in -1i8..=1, dt in -1i8..=1,
            m_free in edge_u128(), t_free in prop::option::of(edge_u128()),
        ) {
            let f = feed(lo.min(hi), lo.max(hi));
            let m = at_band(r, &f, m_sel, dm, m_free);
            let t = t_free.map(|tf| at_band(r, &f, t_sel, dt, tf));
            let p = liq_price(m, t, conf, r, &f);
            let band_lo = mul_div_floor(r, u128::from(f.band_lo_bps), BPS).unwrap_or(0);
            let band_hi = mul_div_floor(r, u128::from(f.band_hi_bps), BPS).unwrap_or(u128::MAX);
            prop_assert!(band_lo <= p && p <= band_hi);
            prop_assert!(p <= liq_price(m, None, conf, r, &f));
            let base = t.map_or(m, |t| m.min(t));
            prop_assert_eq!(p, liq_price(base, None, conf, r, &f));
            prop_assert_eq!(p, fallback_bid_price(base, conf, r, &f));
        }

        /// With an identity band and no confidence, the live bid is `M` exactly when
        /// `M · 10_000 <= T · (10_000 + dev)`, else the floored anchor (saturating); it is never
        /// above the unanchored price. Biased onto `M` at the anchor and one past it.
        #[test]
        fn live_bid_is_capped_at_the_floored_anchor(
            t in edge_u128(), dev in edge_bps(), delta in -1i8..=1,
            exact in any::<bool>(), m_free in edge_u128(), conf in edge_bps(),
            r in edge_u128(), lo in edge_bps(), hi in edge_bps(),
        ) {
            let f = open();
            let a = anchor(t, dev);
            let m = if exact { nudge(a, delta) } else { m_free };
            let p = live_bid_price(m, Some(t), 0, dev, u128::MAX, &f);
            if dev == 0 {
                prop_assert_eq!(p, m);
            } else {
                let up = BPS.saturating_add(u128::from(dev));
                if a < u128::MAX {
                    prop_assert!(w(a, BPS) <= w(t, up));
                    prop_assert!(w(t, up) < w(a.saturating_add(1), BPS));
                } else {
                    // Saturated (or exactly u128::MAX): the true quotient is at least u128::MAX.
                    prop_assert!(w(t, up) >= w(u128::MAX, BPS));
                }
                let want = if w(m, BPS) <= w(t, up) { m } else { a };
                prop_assert_eq!(p, want);
            }
            // Anchored from above only, in any band and at any confidence.
            let g = feed(lo.min(hi), lo.max(hi));
            prop_assert!(
                live_bid_price(m, Some(t), conf, dev, r, &g)
                    <= live_bid_price(m, None, conf, dev, r, &g)
            );
        }

        /// `within_deviation` is `|M - T| · 10_000 < (dev + 1) · T` (the inclusive floor rule),
        /// biased onto `M` exactly at `T ± T · dev / 10_000` and one past it.
        #[test]
        fn deviation_matches_the_exact_rule(
            t in edge_u128(), dev in edge_bps(), above in any::<bool>(), delta in -1i8..=1,
            exact in any::<bool>(), m_free in edge_u128(),
        ) {
            let off = mul_div_floor(t, u128::from(dev), BPS).unwrap_or(u128::MAX);
            let m = if !exact {
                m_free
            } else if above {
                nudge(t.saturating_add(off), delta)
            } else {
                nudge(t.saturating_sub(off), delta)
            };
            let got = within_deviation(m, t, dev);
            if t == 0 {
                prop_assert!(!got);
            } else {
                let lim = u128::from(dev).saturating_add(1);
                prop_assert_eq!(got, w(m.abs_diff(t), BPS) < w(lim, t));
            }
        }
    }
}
