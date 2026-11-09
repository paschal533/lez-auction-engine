//! D4: bids on a stale feed (spec §3.2.3-§3.2.4, ADR-074). While the feed is invalid a bid is
//! priced at the last-good TWAP `T_lg`, if it has `min_obs` samples and its newest sample is at
//! most `stale_bid_max_age_ms` old, and the price is held at the auction's stale floor: the
//! collateral never sells for less coin per unit than `(1 - d_max) / P_liq`, the worst price
//! the auction's own schedule reaches at its opening price.

use auction_core::{
    math::{MathError, mul_div_floor},
    schedule::Schedule,
    units::RAY,
};

use crate::{args::RiskArgs, error::RiskError, twap::TwapRing, twap::twap};

/// The stale cap stored at `Seized`: `floor(P_liq · RAY / (RAY - d_max))`, with `d_max` the last
/// point of the auction's snapshotted schedule.
///
/// # Errors
/// `Overflow` if the cap does not fit a `u128` (or `d_max >= RAY`, which a valid schedule cannot
/// hold: its discounts are at most 30 %).
pub fn stale_cap(p_liq_ray: u128, schedule: &Schedule) -> Result<u128, MathError> {
    let keep = RAY
        .checked_sub(schedule.max_discount_ray())
        .ok_or(MathError::Overflow)?;
    mul_div_floor(p_liq_ray, RAY, keep)
}

/// The fallback bid price `min(P_fb, floor(stale_cap · (RAY - d) / RAY))` at the bid's (frozen)
/// discount `d`. A `stale_cap` of 0 (an auction opened while D4 was off) gives 0, which the
/// quote refuses (`quote_bid` refuses a zero price), so such an auction is never sold unfloored.
#[must_use]
pub fn floored_price(p_fb: u128, stale_cap_ray: u128, d_ray: u128) -> u128 {
    let keep = RAY.saturating_sub(d_ray);
    // `keep <= RAY`, so the quotient is at most `stale_cap` and cannot error; 0 is the safe side.
    let floor = mul_div_floor(stale_cap_ray, keep, RAY).unwrap_or(0);
    p_fb.min(floor)
}

/// The last-good TWAP of a stale feed (spec §3.2.3), with `max_age_ms` the feed's max age (the
/// TWAP anchor freezes at `newest.ts + max_age_ms`).
///
/// `Ok(None)` when D4 is off (`stale_bid_max_age_ms == 0` or `twap_window_ms == 0`): the caller
/// refuses with the feed's own code.
///
/// # Errors
/// `TwapUnavailable` without `min_obs` samples; `LastGoodTwapTooOld` when the newest sample is
/// more than `stale_bid_max_age_ms` before `now_ms`.
pub fn fallback_twap(
    ring: &TwapRing,
    now_ms: u64,
    risk: &RiskArgs,
    max_age_ms: u64,
) -> Result<Option<u128>, RiskError> {
    if risk.stale_bid_max_age_ms == 0 || risk.twap_window_ms == 0 {
        return Ok(None);
    }
    let (t, newest_ms) = twap(
        ring,
        now_ms,
        risk.twap_window_ms,
        risk.twap_min_obs,
        max_age_ms,
    )
    .ok_or(RiskError::TwapUnavailable)?;
    if now_ms.saturating_sub(newest_ms) > risk.stale_bid_max_age_ms {
        return Err(RiskError::LastGoodTwapTooOld);
    }
    Ok(Some(t))
}

#[cfg(test)]
mod tests {
    use super::*;
    use auction_core::{schedule::SchedulePoint, units::bps_to_ray};

    fn schedule(last_bps: u16) -> Schedule {
        Schedule::new(&[
            SchedulePoint {
                elapsed_ms: 0,
                discount_bps: 200,
            },
            SchedulePoint {
                elapsed_ms: 2_700_000,
                discount_bps: last_bps,
            },
        ])
        .expect("a valid schedule")
    }

    #[test]
    fn floor_uses_the_capped_max_discount() {
        let at = |bps| {
            let keep = RAY.checked_sub(bps_to_ray(bps)).expect("below RAY");
            mul_div_floor(RAY / 2, RAY, keep).expect("fits")
        };
        let cap = stale_cap(RAY / 2, &schedule(1_000)).expect("fits");
        assert_eq!(cap, at(1_000));
        let wide = stale_cap(RAY / 2, &schedule(3_000)).expect("fits");
        assert_eq!(wide, at(3_000));
        assert!(wide > cap);
        assert_eq!(
            stale_cap(u128::MAX, &schedule(1_000)),
            Err(MathError::Overflow)
        );
    }

    #[test]
    fn fallback_price_clamped_to_the_floor() {
        let d = bps_to_ray(1_000);
        assert_eq!(floored_price(RAY, RAY, d), RAY / 10 * 9);
        assert_eq!(floored_price(RAY / 2, RAY, d), RAY / 2);
        // No cap recorded: the price is 0, which the quote refuses.
        assert_eq!(floored_price(RAY, 0, d), 0);
    }

    #[cfg(feature = "presets")]
    #[test]
    fn fallback_twap_follows_the_d4_conditions() {
        use crate::twap::record;
        /// The recommended spacing and a feed max age below its stale window, ms.
        const SPACING: u64 = 300_000;
        const MAX_AGE: u64 = 600_000;
        let rec = RiskArgs::recommended();
        let warm = [0, SPACING, 2 * SPACING]
            .iter()
            .fold(TwapRing::default(), |r, &ts| {
                record(&r, ts, RAY, 0, ts, SPACING)
            });
        const NEWEST: u64 = 2 * SPACING;
        let at = |risk: &RiskArgs, ring: &TwapRing, after: u64| {
            let now = NEWEST.checked_add(after).expect("time fits");
            fallback_twap(ring, now, risk, MAX_AGE)
        };
        // Legacy: D4 off.
        assert_eq!(at(&RiskArgs::legacy(), &warm, 1_000_000), Ok(None));
        // TWAP on, D4 off.
        let no_d4 = RiskArgs {
            stale_bid_max_age_ms: 0,
            ..rec
        };
        assert_eq!(at(&no_d4, &warm, 1_000_000), Ok(None));
        // An empty ring has no TWAP.
        assert_eq!(
            at(&rec, &TwapRing::default(), 1_000_000),
            Err(RiskError::TwapUnavailable)
        );
        // Past the max age, even with a TWAP.
        assert_eq!(
            at(&rec, &warm, 3_600_001),
            Err(RiskError::LastGoodTwapTooOld)
        );
        // At the max age exactly: the frozen TWAP of three samples at RAY.
        let t = at(&rec, &warm, 3_600_000).expect("in time").expect("D4 on");
        assert!(t <= RAY && t.saturating_add(7) >= RAY, "{t}");
        assert_eq!(at(&rec, &warm, MAX_AGE + 1), Ok(Some(t)), "frozen");
    }
}
