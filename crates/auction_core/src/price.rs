//! Feed validity and the price used for bids (spec §5.2, ADR-005). No fallback price.
#![allow(clippy::arithmetic_side_effects)] // constant bps arithmetic below 2^16

use crate::error::{ConfigError, PauseReason};
use crate::math::{mul_div_floor, MathError};
use crate::units::{BPS, RAY};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OracleScale {
    Ray,
    Q64x64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OracleReading {
    pub price_raw: u128,
    pub confidence_raw: u128,
    pub timestamp_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeedParams {
    pub scale: OracleScale,
    pub max_age_ms: u64,
    pub max_conf_bps: u16,
    pub band_lo_bps: u16,
    pub band_hi_bps: u16,
}

pub const MAX_ORACLE_AGE_MS: u64 = 86_400_000;
pub const MAX_CONF_BPS: u16 = 1000;
pub const REF_ORACLE_CLAMP_BPS: u16 = 2000;

impl FeedParams {
    pub fn validate(&self) -> Result<(), ConfigError> {
        let ok_band = (5000..=10_000).contains(&self.band_lo_bps)
            && (10_000..=20_000).contains(&self.band_hi_bps);
        if !ok_band {
            return Err(ConfigError::BandInvalid);
        }
        if self.max_age_ms > MAX_ORACLE_AGE_MS {
            return Err(ConfigError::OracleAgeAboveMax);
        }
        if self.max_conf_bps > MAX_CONF_BPS {
            return Err(ConfigError::ConfidenceAboveMax);
        }
        Ok(())
    }
}

pub fn to_ray(raw: u128, scale: OracleScale) -> Result<u128, MathError> {
    match scale {
        OracleScale::Ray => Ok(raw),
        OracleScale::Q64x64 => mul_div_floor(raw, RAY, 1u128 << 64),
    }
}

/// Returns the market price M in RAY when the feed is usable; otherwise why the type is paused.
pub fn validate_feed(
    r: &OracleReading,
    p: &FeedParams,
    redemption_ray: u128,
    now_ms: u64,
) -> Result<u128, PauseReason> {
    if now_ms.saturating_sub(r.timestamp_ms) > p.max_age_ms {
        return Err(PauseReason::Stale);
    }
    let m = to_ray(r.price_raw, p.scale).map_err(|_| PauseReason::OutOfRange)?;
    if m == 0 {
        return Err(PauseReason::ZeroPrice);
    }
    let conf = to_ray(r.confidence_raw, p.scale).map_err(|_| PauseReason::LowConfidence)?;
    // conf / m ≤ max_conf_bps / 1e4  ⇔  conf·1e4 ≤ max_conf_bps·m   (compared via mul_div to avoid overflow)
    let conf_limit =
        mul_div_floor(m, p.max_conf_bps as u128, BPS).map_err(|_| PauseReason::LowConfidence)?;
    if conf > conf_limit {
        return Err(PauseReason::LowConfidence);
    }
    let lo = redemption_ray / 2;
    let hi = redemption_ray.saturating_mul(2);
    if m < lo || m > hi {
        return Err(PauseReason::OutOfRange);
    }
    Ok(m)
}

/// P = clamp(M, R·lo, R·hi). The bounds are rounded down (floor).
/// Callers must pass validated params (FeedParams::validate); the unwrap_or branches are unreachable for validated params.
pub fn bid_price(m_ray: u128, redemption_ray: u128, p: &FeedParams) -> u128 {
    let lo = mul_div_floor(redemption_ray, p.band_lo_bps as u128, BPS).unwrap_or(0);
    let hi = mul_div_floor(redemption_ray, p.band_hi_bps as u128, BPS).unwrap_or(u128::MAX);
    m_ray.max(lo).min(hi)
}

/// Admin price, optionally overridden by a valid oracle clamped to ±20% of it (spec §5.11, ADR-017).
pub fn effective_ref_price(admin_ray: u128, oracle: Option<Result<u128, PauseReason>>) -> u128 {
    match oracle {
        Some(Ok(o)) => {
            let lo = mul_div_floor(admin_ray, (BPS as u16 - REF_ORACLE_CLAMP_BPS) as u128, BPS)
                .unwrap_or(0);
            let hi = mul_div_floor(admin_ray, (BPS as u16 + REF_ORACLE_CLAMP_BPS) as u128, BPS)
                .unwrap_or(u128::MAX);
            o.clamp(lo, hi)
        }
        _ => admin_ray,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::RAY;

    const R: u128 = 752 * RAY / 1_000_000; // 0.000752 collateral per coin
    fn params() -> FeedParams {
        FeedParams {
            scale: OracleScale::Ray,
            max_age_ms: 3_600_000,
            max_conf_bps: 100,
            band_lo_bps: 9000,
            band_hi_bps: 11000,
        }
    }
    fn reading(price: u128, conf: u128, ts: u64) -> OracleReading {
        OracleReading {
            price_raw: price,
            confidence_raw: conf,
            timestamp_ms: ts,
        }
    }

    #[test]
    fn valid_feed_returns_price() {
        assert_eq!(
            validate_feed(&reading(R, 0, 1_000), &params(), R, 2_000),
            Ok(R)
        );
    }
    #[test]
    fn future_timestamp_is_age_zero() {
        // Review Focus #3
        assert_eq!(
            validate_feed(&reading(R, 0, 5_000), &params(), R, 2_000),
            Ok(R)
        );
    }
    #[test]
    fn stale_zero_lowconf_outofrange() {
        let p = params();
        assert_eq!(
            validate_feed(&reading(R, 0, 0), &p, R, 3_600_001),
            Err(PauseReason::Stale)
        );
        assert_eq!(
            validate_feed(&reading(0, 0, 0), &p, R, 0),
            Err(PauseReason::ZeroPrice)
        );
        assert_eq!(
            validate_feed(&reading(R, R / 50, 0), &p, R, 0),
            Err(PauseReason::LowConfidence)
        ); // 2% > 1%
        assert_eq!(
            validate_feed(&reading(R * 2 + 1, 0, 0), &p, R, 0),
            Err(PauseReason::OutOfRange)
        );
        assert_eq!(
            validate_feed(&reading(R / 2 - 1, 0, 0), &p, R, 0),
            Err(PauseReason::OutOfRange)
        );
    }
    #[test]
    fn band_clamp() {
        let p = params();
        assert_eq!(bid_price(R, R, &p), R);
        assert_eq!(bid_price(R * 3 / 2, R, &p), R * 11 / 10);
        assert_eq!(bid_price(R * 6 / 10, R, &p), R * 9 / 10);
    }
    #[test]
    fn q64_conversion() {
        assert_eq!(to_ray(1u128 << 64, OracleScale::Q64x64), Ok(RAY));
        assert_eq!(to_ray(1u128 << 63, OracleScale::Q64x64), Ok(RAY / 2));
        assert_eq!(to_ray(RAY, OracleScale::Ray), Ok(RAY));
    }
    #[test]
    fn ref_price_clamped_to_20_percent() {
        let a = 1_000 * RAY;
        assert_eq!(effective_ref_price(a, None), a);
        assert_eq!(effective_ref_price(a, Some(Err(PauseReason::Stale))), a);
        assert_eq!(effective_ref_price(a, Some(Ok(1_100 * RAY))), 1_100 * RAY);
        assert_eq!(effective_ref_price(a, Some(Ok(5_000 * RAY))), 1_200 * RAY);
        assert_eq!(effective_ref_price(a, Some(Ok(1))), 800 * RAY);
    }
    #[test]
    fn params_validation() {
        let mut p = params();
        p.band_lo_bps = 4999;
        assert_eq!(p.validate(), Err(ConfigError::BandInvalid));
        let mut p = params();
        p.max_age_ms = 86_400_001;
        assert_eq!(p.validate(), Err(ConfigError::OracleAgeAboveMax));
        let mut p = params();
        p.max_conf_bps = 1001;
        assert_eq!(p.validate(), Err(ConfigError::ConfidenceAboveMax));
    }

    #[test]
    fn inverted_band_does_not_panic() {
        // Inverted band (lo > hi) should degrade to hi instead of panicking.
        let mut p = params();
        p.band_lo_bps = 11000;
        p.band_hi_bps = 9000;
        let hi = mul_div_floor(R, 9000, BPS).unwrap_or(u128::MAX);
        // m_ray.max(lo).min(hi) always returns hi when lo > hi (safe degradation)
        assert_eq!(bid_price(R, R, &p), hi);
        // Even with m below lo, the inverted band still degrades to hi
        assert_eq!(bid_price(R / 100, R, &p), hi);
    }

    #[test]
    fn age_exactly_at_max_is_valid() {
        let p = params();
        // age = 3_600_000 - 0 = 3_600_000 = max_age_ms, so valid
        assert_eq!(validate_feed(&reading(R, 0, 0), &p, R, 3_600_000), Ok(R));
    }

    #[test]
    fn confidence_exactly_at_limit_is_valid() {
        let p = params();
        // conf = R / 100, max_conf_bps = 100
        // conf_limit = mul_div_floor(R, 100, 10000) = R / 100
        let conf = R / 100;
        assert_eq!(validate_feed(&reading(R, conf, 0), &p, R, 0), Ok(R));
    }

    #[test]
    fn exact_boundaries_inclusive() {
        let p = params();
        // Exactly R/2 (inclusive lower bound)
        assert_eq!(validate_feed(&reading(R / 2, 0, 0), &p, R, 0), Ok(R / 2));
        // Exactly 2*R (inclusive upper bound)
        assert_eq!(validate_feed(&reading(2 * R, 0, 0), &p, R, 0), Ok(2 * R));
    }

    #[test]
    fn q64x64_feed_validates() {
        // Test Q64x64 scale through validate_feed
        let mut p = params();
        p.scale = OracleScale::Q64x64;
        // q = floor(R·2^64/RAY) = R·2^64/RAY − f; back = floor(q·RAY/2^64) ≥ R − f·RAY/2^64 − 1 > R − RAY/2^64 − 1, and back ≤ R
        let r_q64 = mul_div_floor(R, 1u128 << 64, RAY).unwrap();
        let result = validate_feed(&reading(r_q64, 0, 0), &p, R, 0);
        assert!(result.is_ok());
        let m = result.unwrap();
        // Round-trip error: R − m ≤ ceil(RAY / 2^64) and m ≤ R
        assert!(R - m <= RAY.div_ceil(1u128 << 64));
        assert!(m <= R);
    }

    #[test]
    fn effective_ref_price_at_exact_bounds() {
        let a = 1_000 * RAY;
        // Compute the bounds: lo = a * 8000 / 10000, hi = a * 12000 / 10000
        let lo = mul_div_floor(a, 8000, BPS).unwrap();
        let hi = mul_div_floor(a, 12000, BPS).unwrap();
        // lo should be 800 * RAY, hi should be 1200 * RAY
        assert_eq!(lo, 800 * RAY);
        assert_eq!(hi, 1_200 * RAY);
        // Exactly at the boundaries
        assert_eq!(effective_ref_price(a, Some(Ok(1_200 * RAY))), 1_200 * RAY);
        assert_eq!(effective_ref_price(a, Some(Ok(800 * RAY))), 800 * RAY);
    }
}
