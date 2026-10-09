//! Instruction arguments arrive in spel-encodable types (ADR-030): bps as `u32`, schedules as
//! `Vec<u32>` of interleaved `(elapsed_ms, bps)` pairs. This module converts and validates them.

use auction_core::{
    liquidation::CollateralParams,
    price::{FeedParams, OracleScale},
    schedule::{Schedule, SchedulePoint, MAX_POINTS},
    system::SystemParams,
    units::BPS,
};

use crate::EngineError;

/// System auction ttl bounds (spec §5.12: 10 min – 7 d).
pub const MIN_SYSTEM_TTL_MS: u64 = 600_000;
pub const MAX_SYSTEM_TTL_MS: u64 = 604_800_000;
/// ADR-039: no debt auction starts within this delay after the previous one ended (close or last
/// take), so the admin always has a window in which no debt auction is live (THREAT_MODEL finding 8).
/// Equal to the minimum system-auction TTL (spec §5.12).
pub const DEBT_RELIST_DELAY_MS: u64 = 600_000;

pub fn bps(v: u32) -> Result<u16, EngineError> {
    if u128::from(v) > BPS {
        return Err(EngineError::BpsOutOfRange);
    }
    u16::try_from(v).map_err(|_| EngineError::BpsOutOfRange)
}

pub fn schedule_from_words(words: &[u32]) -> Result<Schedule, EngineError> {
    if words.is_empty() || !words.len().is_multiple_of(2) || words.len() > 2 * MAX_POINTS {
        return Err(EngineError::ScheduleEncoding);
    }
    let mut points = Vec::with_capacity(words.len() / 2);
    for pair in words.chunks_exact(2) {
        points.push(SchedulePoint {
            elapsed_ms: u64::from(pair[0]),
            discount_bps: bps(pair[1])?,
        });
    }
    Ok(Schedule::new(&points)?)
}

/// Flat engine parameters of `init_config` / `set_engine_params`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineParamsArgs {
    pub global_cap: u128,
    pub freeze_on_bad_debt: bool,
    pub bad_debt_freeze_threshold: u128,
    pub protocol_token_enabled: bool,
    pub protocol_token_ref_price: u128,
    pub ref_oracle_enabled: bool,
    pub ref_oracle_max_age_ms: u64,
    pub surplus_buffer_target: u128,
    pub surplus_lot: u128,
    pub surplus_delay_ms: u64,
    pub surplus_premium_bps: u32,
    pub surplus_max_discount_bps: u32,
    pub debt_lot: u128,
    pub debt_initial_discount_bps: u32,
    pub debt_growth: Vec<u32>,
    pub surplus_decay: Vec<u32>,
    pub max_mint_per_auction: u128,
    pub min_take: u128,
    pub system_ttl_ms: u64,
}

impl EngineParamsArgs {
    /// Validates everything ADR-024 requires at config time and returns the core `SystemParams`.
    pub fn validate(&self) -> Result<SystemParams, EngineError> {
        if self.protocol_token_ref_price == 0 {
            return Err(EngineError::ZeroRefPrice);
        }
        if self.global_cap == 0 {
            return Err(EngineError::InvalidConfig);
        }
        if !(MIN_SYSTEM_TTL_MS..=MAX_SYSTEM_TTL_MS).contains(&self.system_ttl_ms) {
            return Err(EngineError::TtlOutOfRange);
        }
        if (self.ref_oracle_enabled && self.ref_oracle_max_age_ms == 0)
            || self.ref_oracle_max_age_ms > auction_core::price::MAX_ORACLE_AGE_MS
        {
            return Err(EngineError::InvalidConfig); // ≤ 24 h, like the collateral feed (spec §5.12)
        }
        let s = SystemParams {
            surplus_buffer_target: self.surplus_buffer_target,
            surplus_lot: self.surplus_lot,
            surplus_delay_ms: self.surplus_delay_ms,
            surplus_premium_bps: bps(self.surplus_premium_bps)?,
            surplus_max_discount_bps: bps(self.surplus_max_discount_bps)?,
            debt_lot: self.debt_lot,
            debt_initial_discount_bps: bps(self.debt_initial_discount_bps)?,
            debt_growth: schedule_from_words(&self.debt_growth)?,
            surplus_decay: schedule_from_words(&self.surplus_decay)?,
            max_mint_per_auction: self.max_mint_per_auction,
            min_take: self.min_take,
            system_ttl_ms: self.system_ttl_ms,
        };
        s.validate()?;
        if self.bad_debt_freeze_threshold < s.debt_lot {
            return Err(EngineError::ThresholdBelowDebtLot);
        }
        Ok(s)
    }

    /// Defaults used by tests and the localnet smoke (raw units; spec §5.12 shapes).
    #[must_use]
    pub fn defaults_for_tests() -> Self {
        Self {
            global_cap: 10_000_000,
            freeze_on_bad_debt: true,
            bad_debt_freeze_threshold: 1_000,
            protocol_token_enabled: false,
            protocol_token_ref_price: 2 * auction_core::units::RAY,
            ref_oracle_enabled: false,
            ref_oracle_max_age_ms: 3_600_000,
            surplus_buffer_target: 1_000,
            surplus_lot: 500,
            surplus_delay_ms: 0,
            surplus_premium_bps: 500,
            surplus_max_discount_bps: 2_000,
            debt_lot: 500,
            debt_initial_discount_bps: 0,
            debt_growth: vec![0, 0, 1_000, 3_000],
            surplus_decay: vec![0, 0, 1_000, 2_500],
            max_mint_per_auction: 5_000,
            min_take: 10,
            system_ttl_ms: 14_400_000,
        }
    }
}

/// Flat collateral parameters of `set_collateral_config`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollateralArgs {
    pub oracle_scale: u8,
    pub max_oracle_age_ms: u64,
    pub max_conf_bps: u32,
    pub band_lo_bps: u32,
    pub band_hi_bps: u32,
    pub penalty_bps: u32,
    pub tip: u128,
    pub chip_bps: u32,
    pub reward_cap: u128,
    pub debt_floor: u128,
    pub min_bid: u128,
    pub type_cap: u128,
    pub schedule: Vec<u32>,
    pub admin_paused: bool,
}

impl CollateralArgs {
    pub fn validate(
        &self,
        global_cap: u128,
    ) -> Result<(CollateralParams, FeedParams), EngineError> {
        let scale = match self.oracle_scale {
            0 => OracleScale::Ray,
            1 => OracleScale::Q64x64,
            _ => return Err(EngineError::InvalidConfig),
        };
        let band_hi = u16::try_from(self.band_hi_bps).map_err(|_| EngineError::BpsOutOfRange)?;
        let feed = FeedParams {
            scale,
            max_age_ms: self.max_oracle_age_ms,
            max_conf_bps: bps(self.max_conf_bps)?,
            band_lo_bps: bps(self.band_lo_bps)?,
            band_hi_bps: band_hi,
        };
        feed.validate()?;
        if feed.max_age_ms == 0 {
            return Err(EngineError::InvalidConfig);
        }
        let params = CollateralParams {
            penalty_bps: bps(self.penalty_bps)?,
            tip: self.tip,
            chip_bps: bps(self.chip_bps)?,
            reward_cap: self.reward_cap,
            debt_floor: self.debt_floor,
            min_bid: self.min_bid,
            schedule: schedule_from_words(&self.schedule)?,
        };
        params.validate()?;
        if params.chip_bps > params.penalty_bps {
            return Err(EngineError::ChipAbovePenalty);
        }
        if self.type_cap == 0 {
            return Err(EngineError::InvalidConfig);
        }
        if self.type_cap > global_cap {
            return Err(EngineError::TypeCapAboveGlobal);
        }
        Ok((params, feed))
    }

    #[must_use]
    pub fn defaults_for_tests() -> Self {
        Self {
            oracle_scale: 0,
            max_oracle_age_ms: 3_600_000,
            max_conf_bps: 100,
            band_lo_bps: 9_000,
            band_hi_bps: 11_000,
            penalty_bps: 1_000,
            tip: 5,
            chip_bps: 10,
            reward_cap: 50,
            debt_floor: 100,
            min_bid: 25,
            type_cap: 5_000_000,
            schedule: vec![0, 200, 2_700_000, 1_000],
            admin_paused: false,
        }
    }
}

/// Bound on one admin change of the protocol-token reference price: the new price must lie in
/// `[old / 2, 2 * old]` (both ends inclusive; plan review ruling I5, recorded in ADR-031, Task 13), so a single transaction cannot swing the
/// reference that sizes debt and surplus lots. `old` is the stored (non-zero) price.
///
/// `EngineParamsArgs::validate` does not call this (init has no old value); `set_engine_params`
/// MUST call `check_ref_price_step(stored_old, new)`.
pub fn check_ref_price_step(old: u128, new: u128) -> Result<(), EngineError> {
    if new == 0 {
        return Err(EngineError::ZeroRefPrice);
    }
    if new.saturating_mul(2) < old || new > old.saturating_mul(2) {
        return Err(EngineError::RefPriceStepTooLarge);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_params() -> EngineParamsArgs {
        EngineParamsArgs::defaults_for_tests()
    }
    fn ok_coll() -> CollateralArgs {
        CollateralArgs::defaults_for_tests()
    }
    const CAP: u128 = 10_000_000;

    #[test]
    fn bps_accepts_up_to_10_000_only() {
        assert_eq!(bps(0), Ok(0));
        assert_eq!(bps(10_000), Ok(10_000));
        assert_eq!(bps(10_001), Err(EngineError::BpsOutOfRange));
        assert_eq!(bps(u32::MAX), Err(EngineError::BpsOutOfRange));
    }

    #[test]
    fn system_bps_fields_are_range_checked() {
        let ok = ok_params();
        let a = EngineParamsArgs {
            surplus_premium_bps: 10_001,
            ..ok.clone()
        };
        assert_eq!(a.validate(), Err(EngineError::BpsOutOfRange));
        let b = EngineParamsArgs {
            surplus_max_discount_bps: 70_000,
            ..ok.clone()
        };
        assert_eq!(b.validate(), Err(EngineError::BpsOutOfRange));
        let c = EngineParamsArgs {
            debt_initial_discount_bps: 10_001,
            ..ok.clone()
        };
        assert_eq!(c.validate(), Err(EngineError::BpsOutOfRange));
        let d = EngineParamsArgs {
            debt_growth: vec![0, 10_001],
            ..ok.clone()
        };
        assert_eq!(d.validate(), Err(EngineError::BpsOutOfRange));
        let e = EngineParamsArgs {
            surplus_decay: vec![0, 10_001],
            ..ok
        };
        assert_eq!(e.validate(), Err(EngineError::BpsOutOfRange));
    }

    #[test]
    fn collateral_bps_fields_are_range_checked() {
        let ok = ok_coll();
        let conf = CollateralArgs {
            max_conf_bps: 10_001,
            ..ok.clone()
        };
        assert_eq!(conf.validate(CAP), Err(EngineError::BpsOutOfRange));
        let lo = CollateralArgs {
            band_lo_bps: 10_001,
            ..ok.clone()
        };
        assert_eq!(lo.validate(CAP), Err(EngineError::BpsOutOfRange));
        let pen = CollateralArgs {
            penalty_bps: 10_001,
            ..ok.clone()
        };
        assert_eq!(pen.validate(CAP), Err(EngineError::BpsOutOfRange));
        let chip = CollateralArgs {
            chip_bps: 10_001,
            ..ok.clone()
        };
        assert_eq!(chip.validate(CAP), Err(EngineError::BpsOutOfRange));
        let hi_wide = CollateralArgs {
            band_hi_bps: 65_536,
            ..ok.clone()
        };
        assert_eq!(hi_wide.validate(CAP), Err(EngineError::BpsOutOfRange));
        let hi_bad = CollateralArgs {
            band_hi_bps: 20_001,
            ..ok.clone()
        };
        assert_eq!(hi_bad.validate(CAP), Err(EngineError::InvalidConfig));
        let hi_max = CollateralArgs {
            band_hi_bps: 20_000,
            ..ok
        };
        assert!(hi_max.validate(CAP).is_ok());
    }

    #[test]
    fn schedule_length_and_discount_boundaries() {
        // 8 points is the maximum; 9 is an encoding error; odd length and empty too.
        let pts = |n: u32| -> Vec<u32> { (0..n).flat_map(|i| [i * 1_000, i * 100]).collect() };
        assert!(schedule_from_words(&pts(8)).is_ok());
        assert_eq!(
            schedule_from_words(&pts(9)),
            Err(EngineError::ScheduleEncoding)
        );
        assert_eq!(
            schedule_from_words(&[0]),
            Err(EngineError::ScheduleEncoding)
        );
        assert_eq!(
            schedule_from_words(&[0, 100, 1_000]),
            Err(EngineError::ScheduleEncoding)
        );
        // 3000 bps is the cap, 3001 is not.
        assert!(schedule_from_words(&[0, 3_000]).is_ok());
        assert_eq!(
            schedule_from_words(&[0, 3_001]),
            Err(EngineError::InvalidConfig)
        );
        // the 10_000 boundary of bps() inside a schedule
        assert_eq!(
            schedule_from_words(&[0, 10_001]),
            Err(EngineError::BpsOutOfRange)
        );
    }

    #[test]
    fn ref_oracle_max_age_is_at_most_24_hours() {
        let ok = ok_params();
        let at = EngineParamsArgs {
            ref_oracle_max_age_ms: 86_400_000,
            ..ok.clone()
        };
        assert!(at.validate().is_ok());
        let over = EngineParamsArgs {
            ref_oracle_max_age_ms: 86_400_001,
            ..ok.clone()
        };
        assert_eq!(over.validate(), Err(EngineError::InvalidConfig));
        let enabled_zero = EngineParamsArgs {
            ref_oracle_enabled: true,
            ref_oracle_max_age_ms: 0,
            ..ok.clone()
        };
        assert_eq!(enabled_zero.validate(), Err(EngineError::InvalidConfig));
        let disabled_zero = EngineParamsArgs {
            ref_oracle_enabled: false,
            ref_oracle_max_age_ms: 0,
            ..ok
        };
        assert!(disabled_zero.validate().is_ok());
    }

    #[test]
    fn ttl_and_threshold_boundaries() {
        let ok = ok_params();
        let lo = EngineParamsArgs {
            system_ttl_ms: MIN_SYSTEM_TTL_MS,
            ..ok.clone()
        };
        assert!(lo.validate().is_ok());
        let hi = EngineParamsArgs {
            system_ttl_ms: MAX_SYSTEM_TTL_MS,
            ..ok.clone()
        };
        assert!(hi.validate().is_ok());
        let over = EngineParamsArgs {
            system_ttl_ms: MAX_SYSTEM_TTL_MS + 1,
            ..ok.clone()
        };
        assert_eq!(over.validate(), Err(EngineError::TtlOutOfRange));
        let exact = EngineParamsArgs {
            bad_debt_freeze_threshold: ok.debt_lot,
            ..ok
        };
        assert!(exact.validate().is_ok());
    }

    #[test]
    fn caps_boundaries() {
        let ok = ok_coll();
        assert!(
            ok.validate(ok.type_cap).is_ok(),
            "type cap equal to the global cap is allowed"
        );
        assert_eq!(
            ok.validate(ok.type_cap - 1),
            Err(EngineError::TypeCapAboveGlobal)
        );
        let zero = CollateralArgs { type_cap: 0, ..ok };
        assert_eq!(zero.validate(CAP), Err(EngineError::InvalidConfig));
    }

    #[test]
    fn chip_equal_to_penalty_is_allowed() {
        let ok = ok_coll();
        let eq = CollateralArgs {
            chip_bps: ok.penalty_bps,
            ..ok.clone()
        };
        assert!(eq.validate(CAP).is_ok());
        let zero_age = CollateralArgs {
            max_oracle_age_ms: 0,
            ..ok
        };
        assert_eq!(zero_age.validate(CAP), Err(EngineError::InvalidConfig));
    }

    #[test]
    fn ref_price_step_bounds_are_inclusive() {
        assert_eq!(check_ref_price_step(100, 50), Ok(()));
        assert_eq!(
            check_ref_price_step(100, 49),
            Err(EngineError::RefPriceStepTooLarge)
        );
        assert_eq!(check_ref_price_step(100, 200), Ok(()));
        assert_eq!(
            check_ref_price_step(100, 201),
            Err(EngineError::RefPriceStepTooLarge)
        );
        assert_eq!(
            check_ref_price_step(101, 50),
            Err(EngineError::RefPriceStepTooLarge)
        );
        assert_eq!(check_ref_price_step(101, 51), Ok(()));
        assert_eq!(check_ref_price_step(100, 100), Ok(()));
        assert_eq!(check_ref_price_step(100, 0), Err(EngineError::ZeroRefPrice));
        assert_eq!(check_ref_price_step(u128::MAX, u128::MAX), Ok(()));
        assert_eq!(
            check_ref_price_step(1, u128::MAX),
            Err(EngineError::RefPriceStepTooLarge)
        );
    }
}
