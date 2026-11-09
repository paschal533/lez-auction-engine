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
