//! Feed validity with a parameterised sanity window (spec §5.2): v0.2.4's `validate_feed` with
//! its hard-coded `[R/2, 2R]` window lifted into a [`FeedPolicy`]. Under
//! [`FeedPolicy::legacy`] it returns exactly what `validate_feed` returns (differential below).

use auction_core::{
    error::PauseReason,
    math::mul_div_floor,
    price::{FeedParams, OracleReading, OracleScale, to_ray},
    units::BPS,
};

/// v0.2.4's lower sanity bound, `R / 2`, in basis points of `R`.
pub const LEGACY_SANITY_LO_BPS: u32 = 5_000;
/// v0.2.4's upper sanity bound, `2 · R`, in basis points of `R`.
pub const LEGACY_SANITY_HI_BPS: u32 = 20_000;

/// The per-type feed policy: v0.2.4's [`FeedParams`] fields plus the sanity window and the raw
/// pin bounds (carried, not checked, in Plan 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedPolicy {
    pub scale: OracleScale,
    pub max_age_ms: u64,
    pub max_conf_bps: u16,
    pub sanity_lo_bps: u32,
    pub sanity_hi_bps: u32,
    pub feed_min_raw: u128,
    pub feed_max_raw: u128,
}

impl FeedPolicy {
    /// The v0.2.4 policy: sanity window `[R/2, 2R]`, no pin bounds.
    #[must_use]
    pub fn legacy(p: &FeedParams) -> Self {
        Self {
            scale: p.scale,
            max_age_ms: p.max_age_ms,
            max_conf_bps: p.max_conf_bps,
            sanity_lo_bps: LEGACY_SANITY_LO_BPS,
            sanity_hi_bps: LEGACY_SANITY_HI_BPS,
            feed_min_raw: 0,
            feed_max_raw: u128::MAX,
        }
    }
}

/// Why a reading is unusable. Maps one-to-one onto the feed [`PauseReason`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedFault {
    Stale,
    ZeroPrice,
    LowConfidence,
    OutOfRange,
}

impl FeedFault {
    /// The pause this fault causes.
    #[must_use]
    pub const fn pause_reason(self) -> PauseReason {
        match self {
            Self::Stale => PauseReason::Stale,
            Self::ZeroPrice => PauseReason::ZeroPrice,
            Self::LowConfidence => PauseReason::LowConfidence,
            Self::OutOfRange => PauseReason::OutOfRange,
        }
    }
}

/// The market price `M` in RAY when the feed is usable, else the fault. Checks in v0.2.4's
/// order: age, scale, zero, confidence, sanity window.
pub fn validate_feed_ext(
    r: &OracleReading,
    policy: &FeedPolicy,
    redemption_ray: u128,
    age_now_ms: u64,
) -> Result<u128, FeedFault> {
    if age_now_ms.saturating_sub(r.timestamp_ms) > policy.max_age_ms {
        return Err(FeedFault::Stale);
    }
    let m = to_ray(r.price_raw, policy.scale).map_err(|_| FeedFault::OutOfRange)?;
    if m == 0 {
        return Err(FeedFault::ZeroPrice);
    }
    let conf = to_ray(r.confidence_raw, policy.scale).map_err(|_| FeedFault::LowConfidence)?;
    let conf_limit = mul_div_floor(m, u128::from(policy.max_conf_bps), BPS)
        .map_err(|_| FeedFault::LowConfidence)?;
    if conf > conf_limit {
        return Err(FeedFault::LowConfidence);
    }
    // Under the legacy policy these are exactly v0.2.4's `R / 2` and `R.saturating_mul(2)`.
    let lo = mul_div_floor(redemption_ray, u128::from(policy.sanity_lo_bps), BPS).unwrap_or(0);
    let hi =
        mul_div_floor(redemption_ray, u128::from(policy.sanity_hi_bps), BPS).unwrap_or(u128::MAX);
    if m < lo || m > hi {
        return Err(FeedFault::OutOfRange);
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use auction_core::{
        price::{MAX_ORACLE_AGE_MS, validate_feed},
        units::RAY,
    };
    use proptest::prelude::*;

    fn params(scale: OracleScale) -> FeedParams {
        FeedParams {
            scale,
            max_age_ms: 3_600_000,
            max_conf_bps: 100,
            band_lo_bps: 9_000,
            band_hi_bps: 11_000,
        }
    }

    proptest! {
        #[test]
        fn validate_feed_ext_matches_v024_under_legacy_policy(
            price_raw in any::<u128>(), confidence_raw in any::<u128>(), ts in any::<u64>(),
            q64 in any::<bool>(), max_age_ms in 0..=MAX_ORACLE_AGE_MS, max_conf_bps in 0u16..=1_000,
            r in any::<u128>(), age_now in any::<u64>(),
        ) {
            let p = FeedParams { scale: if q64 { OracleScale::Q64x64 } else { OracleScale::Ray },
                max_age_ms, max_conf_bps, band_lo_bps: 9_000, band_hi_bps: 11_000 };
            let reading = OracleReading { price_raw, confidence_raw, timestamp_ms: ts };
            prop_assert_eq!(
                validate_feed_ext(&reading, &FeedPolicy::legacy(&p), r, age_now)
                    .map_err(FeedFault::pause_reason),
                validate_feed(&reading, &p, r, age_now));
        }
    }

    const Q64_ONE: u128 = 1 << 64;

    /// Redemption prices at and around the places the two bound formulas could differ:
    /// zero, odd, the last value before `2R` saturates, the first after, and the maximum.
    fn edge_r() -> impl Strategy<Value = u128> {
        prop_oneof![
            Just(0u128),
            Just(1u128),
            Just(3u128),
            Just(u128::MAX / 2),
            Just(u128::MAX / 2 + 1),
            Just(u128::MAX),
            1u128..(1 << 100),
            any::<u128>(),
        ]
    }

    /// A target market price in RAY relative to `r`: one of `R/2 - 1, R/2, R/2 + 1,
    /// 2R - 1, 2R, 2R + 1` (saturating), or `free`.
    fn target_m(r: u128, sel: u8, free: u128) -> u128 {
        let half = r.checked_div(2).unwrap_or_default();
        let double = r.saturating_mul(2);
        match sel {
            0 => half.saturating_sub(1),
            1 => half,
            2 => half.saturating_add(1),
            3 => double.saturating_sub(1),
            4 => double,
            5 => double.saturating_add(1),
            _ => free,
        }
    }

    /// The raw oracle value whose RAY reading is (at or just below) `ray`.
    fn to_raw(ray: u128, scale: OracleScale, bump: u128) -> u128 {
        let raw = match scale {
            OracleScale::Ray => ray,
            OracleScale::Q64x64 => mul_div_floor(ray, Q64_ONE, RAY).unwrap_or(ray),
        };
        raw.saturating_add(bump)
    }

    proptest! {
        /// The legacy differential, biased onto the age, confidence and sanity-window boundaries
        /// and onto Q64 readings that survive the scale check.
        #[test]
        fn validate_feed_ext_matches_v024_at_the_boundaries(
            r in edge_r(), m_sel in 0u8..=6, free_m in any::<u128>(),
            q64 in any::<bool>(), q64_free in prop::option::of(0u128..(1 << 100)),
            raw_bump in 0u128..=1, conf_sel in 0u8..=2,
            ts in 0u64..(1 << 62), max_age_ms in 0..=MAX_ORACLE_AGE_MS, age_sel in 0u8..=2,
            max_conf_bps in 0u16..=1_000,
        ) {
            let scale = if q64 { OracleScale::Q64x64 } else { OracleScale::Ray };
            // Q64: sometimes a free raw below 2^100, which `to_ray` always converts.
            let price_raw = q64_free
                .filter(|_| q64)
                .unwrap_or_else(|| to_raw(target_m(r, m_sel, free_m), scale, raw_bump));
            // The confidence limit of the reading's own RAY price: hit `limit`, `limit + 1`, 0.
            let m = to_ray(price_raw, scale).unwrap_or_default();
            let limit = mul_div_floor(m, u128::from(max_conf_bps), BPS).unwrap_or_default();
            let confidence_raw = match conf_sel {
                0 => to_raw(limit, scale, 0),
                1 => to_raw(limit.saturating_add(1), scale, raw_bump),
                _ => 0,
            };
            // Age exactly `max_age` (fresh), `max_age + 1` (stale), or a future timestamp.
            let age_now = match age_sel {
                0 => ts.saturating_add(max_age_ms),
                1 => ts.saturating_add(max_age_ms).saturating_add(1),
                _ => ts.saturating_sub(1),
            };
            let p = FeedParams { scale, max_age_ms, max_conf_bps,
                band_lo_bps: 9_000, band_hi_bps: 11_000 };
            let reading = OracleReading { price_raw, confidence_raw, timestamp_ms: ts };
            prop_assert_eq!(
                validate_feed_ext(&reading, &FeedPolicy::legacy(&p), r, age_now)
                    .map_err(FeedFault::pause_reason),
                validate_feed(&reading, &p, r, age_now));
        }

        /// The only steps where ext differs in form from v0.2.4: the legacy bounds equal
        /// `R / 2` and `R.saturating_mul(2)` for every `R`.
        #[test]
        fn legacy_sanity_bounds_equal_v024(r in prop_oneof![edge_r(), any::<u128>()]) {
            prop_assert_eq!(
                mul_div_floor(r, u128::from(LEGACY_SANITY_LO_BPS), BPS).unwrap_or(0),
                r.checked_div(2).unwrap_or_default());
            prop_assert_eq!(
                mul_div_floor(r, u128::from(LEGACY_SANITY_HI_BPS), BPS).unwrap_or(u128::MAX),
                r.saturating_mul(2));
        }
    }

    #[test]
    fn validate_feed_ext_saturates_the_high_bound_like_v024() {
        // R above u128::MAX / 2: v0.2.4's hi is u128::MAX, so a huge M is in range.
        let p = params(OracleScale::Ray);
        let r = u128::MAX / 2 + 1;
        let reading = OracleReading {
            price_raw: u128::MAX,
            confidence_raw: 0,
            timestamp_ms: 0,
        };
        assert_eq!(validate_feed(&reading, &p, r, 0), Ok(u128::MAX));
        assert_eq!(
            validate_feed_ext(&reading, &FeedPolicy::legacy(&p), r, 0),
            Ok(u128::MAX)
        );
    }

    #[test]
    fn validate_feed_ext_checks_in_v024_order_and_maps_faults() {
        let p = params(OracleScale::Ray);
        let policy = FeedPolicy::legacy(&p);
        let at = |price_raw, confidence_raw, now| {
            let reading = OracleReading {
                price_raw,
                confidence_raw,
                timestamp_ms: 0,
            };
            validate_feed_ext(&reading, &policy, RAY, now)
        };
        // Stale wins over a zero price.
        assert_eq!(at(0, 0, 3_600_001), Err(FeedFault::Stale));
        assert_eq!(at(0, 0, 0), Err(FeedFault::ZeroPrice));
        assert_eq!(at(RAY, RAY / 50, 0), Err(FeedFault::LowConfidence));
        assert_eq!(at(RAY / 2 - 1, 0, 0), Err(FeedFault::OutOfRange));
        assert_eq!(at(RAY * 2 + 1, 0, 0), Err(FeedFault::OutOfRange));
        assert_eq!(at(RAY / 2, 0, 0), Ok(RAY / 2));
        assert_eq!(at(RAY * 2, 0, 0), Ok(RAY * 2));
        assert_eq!(
            [
                FeedFault::Stale,
                FeedFault::ZeroPrice,
                FeedFault::LowConfidence,
                FeedFault::OutOfRange
            ]
            .map(FeedFault::pause_reason),
            [
                PauseReason::Stale,
                PauseReason::ZeroPrice,
                PauseReason::LowConfidence,
                PauseReason::OutOfRange
            ]
        );
    }

    #[test]
    fn legacy_policy_pins_nothing() {
        let p = params(OracleScale::Q64x64);
        let policy = FeedPolicy::legacy(&p);
        assert_eq!(
            (
                policy.scale,
                policy.max_age_ms,
                policy.max_conf_bps,
                policy.sanity_lo_bps,
                policy.sanity_hi_bps,
                policy.feed_min_raw,
                policy.feed_max_raw
            ),
            (
                OracleScale::Q64x64,
                3_600_000,
                100,
                5_000,
                20_000,
                0,
                u128::MAX
            )
        );
    }
}
