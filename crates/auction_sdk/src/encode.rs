//! Own instruction encoders (spec §6.1). Each program's instruction enum is encoded with risc0 serde
//! through `lee::program::Program::serialize_instruction`, exactly as `Message::try_new` does; the first
//! word is the ENUM index the guest decodes. Never an IDL position: spel-cli encodes the IDL position,
//! and the stablecoin IDL differs from its enum for positions 8..=18 (spec §3.2).

use auction_engine_core::{
    args::{CollateralArgs, EngineParamsArgs},
    Instruction,
};
use lee::program::Program;
use serde::Serialize;

use crate::error::{SdkError, SdkResult};

/// IDL position -> enum index of the vendored `idl/stablecoin-idl.json` (docs/architecture/programs.md).
pub const STABLECOIN_IDL_POSITION_TO_ENUM: [u32; 22] = [
    0, 1, 2, 3, 4, 5, 6, 7, 17, 18, 14, 8, 9, 11, 12, 13, 10, 15, 16, 19, 20, 21,
];

/// Instruction words of any program instruction (risc0 serde; first word = enum index).
pub fn words<T: Serialize>(ix: &T) -> SdkResult<Vec<u32>> {
    Program::serialize_instruction(ix).map_err(|e| SdkError::Encode(format!("{e:?}")))
}

#[must_use]
pub fn variant_index(words: &[u32]) -> Option<u32> {
    words.first().copied()
}

/// IDL name of a stablecoin instruction. Exhaustive: a new upstream variant does not compile until
/// it is named here (drift test `stablecoin_idl_position_maps_to_enum_index`).
#[must_use]
pub fn stablecoin_name(ix: &stablecoin_core::Instruction) -> &'static str {
    use stablecoin_core::Instruction as I;
    match ix {
        I::InitializeProgram { .. } => "initialize_program",
        I::AccrueStabilityFee => "accrue_stability_fee",
        I::UpdateRedemptionRate => "update_redemption_rate",
        I::RefreshGlobals => "refresh_globals",
        I::OpenPosition { .. } => "open_position",
        I::DepositCollateral { .. } => "deposit_collateral",
        I::GenerateDebt { .. } => "generate_debt",
        I::ClosePosition => "close_position",
        I::SetMinimumCollateralizationRatio { .. } => "set_minimum_collateralization_ratio",
        I::SetControllerGains { .. } => "set_controller_gains",
        I::SetMarketPriceOracle => "set_market_price_oracle",
        I::SetTimingParameters { .. } => "set_timing_parameters",
        I::SetAdmin { .. } => "set_admin",
        I::SetFreezeAuthority { .. } => "set_freeze_authority",
        I::SetStabilityFeePerMillisecond { .. } => "set_stability_fee_per_millisecond",
        I::Freeze => "freeze",
        I::Unfreeze => "unfreeze",
        I::WithdrawCollateral { .. } => "withdraw_collateral",
        I::RepayDebt { .. } => "repay_debt",
        I::SetLiquidationEngine { .. } => "set_liquidation_engine",
        I::LiquidatePosition { .. } => "liquidate_position",
        I::EngineFreezeNewDebt => "engine_freeze_new_debt",
    }
}

#[must_use]
pub fn init_config_ix(salt: u64, p: &EngineParamsArgs) -> Instruction {
    Instruction::InitConfig {
        salt,
        global_cap: p.global_cap,
        freeze_on_bad_debt: p.freeze_on_bad_debt,
        bad_debt_freeze_threshold: p.bad_debt_freeze_threshold,
        protocol_token_enabled: p.protocol_token_enabled,
        protocol_token_ref_price: p.protocol_token_ref_price,
        ref_oracle_enabled: p.ref_oracle_enabled,
        ref_oracle_max_age_ms: p.ref_oracle_max_age_ms,
        surplus_buffer_target: p.surplus_buffer_target,
        surplus_lot: p.surplus_lot,
        surplus_delay_ms: p.surplus_delay_ms,
        surplus_premium_bps: p.surplus_premium_bps,
        surplus_max_discount_bps: p.surplus_max_discount_bps,
        debt_lot: p.debt_lot,
        debt_initial_discount_bps: p.debt_initial_discount_bps,
        debt_growth: p.debt_growth.clone(),
        surplus_decay: p.surplus_decay.clone(),
        max_mint_per_auction: p.max_mint_per_auction,
        min_take: p.min_take,
        system_ttl_ms: p.system_ttl_ms,
    }
}

#[must_use]
pub fn set_engine_params_ix(p: &EngineParamsArgs) -> Instruction {
    Instruction::SetEngineParams {
        global_cap: p.global_cap,
        freeze_on_bad_debt: p.freeze_on_bad_debt,
        bad_debt_freeze_threshold: p.bad_debt_freeze_threshold,
        protocol_token_enabled: p.protocol_token_enabled,
        protocol_token_ref_price: p.protocol_token_ref_price,
        ref_oracle_enabled: p.ref_oracle_enabled,
        ref_oracle_max_age_ms: p.ref_oracle_max_age_ms,
        surplus_buffer_target: p.surplus_buffer_target,
        surplus_lot: p.surplus_lot,
        surplus_delay_ms: p.surplus_delay_ms,
        surplus_premium_bps: p.surplus_premium_bps,
        surplus_max_discount_bps: p.surplus_max_discount_bps,
        debt_lot: p.debt_lot,
        debt_initial_discount_bps: p.debt_initial_discount_bps,
        debt_growth: p.debt_growth.clone(),
        surplus_decay: p.surplus_decay.clone(),
        max_mint_per_auction: p.max_mint_per_auction,
        min_take: p.min_take,
        system_ttl_ms: p.system_ttl_ms,
    }
}

#[must_use]
pub fn set_collateral_config_ix(c: &CollateralArgs) -> Instruction {
    Instruction::SetCollateralConfig {
        oracle_scale: c.oracle_scale,
        max_oracle_age_ms: c.max_oracle_age_ms,
        max_conf_bps: c.max_conf_bps,
        band_lo_bps: c.band_lo_bps,
        band_hi_bps: c.band_hi_bps,
        penalty_bps: c.penalty_bps,
        tip: c.tip,
        chip_bps: c.chip_bps,
        reward_cap: c.reward_cap,
        debt_floor: c.debt_floor,
        min_bid: c.min_bid,
        type_cap: c.type_cap,
        schedule: c.schedule.clone(),
        admin_paused: c.admin_paused,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::support::{idl_instruction_names, stablecoin_samples};
    use auction_engine_core::instruction::instruction_samples;

    #[test]
    fn engine_encoding_is_the_enum_index_and_equals_the_idl_position() {
        let names = idl_instruction_names("auction_engine-idl.json");
        let samples = instruction_samples();
        assert_eq!(samples.len(), names.len());
        for (i, ix) in samples.iter().enumerate() {
            let w = words(ix).expect("encodes");
            assert_eq!(variant_index(&w), Some(u32::try_from(i).expect("small")));
            assert_eq!(ix.name(), names[i], "IDL position {i}");
        }
    }

    #[test]
    fn stablecoin_encoding_uses_the_enum_index_not_the_idl_position() {
        let names = idl_instruction_names("stablecoin-idl.json");
        assert_eq!(names.len(), STABLECOIN_IDL_POSITION_TO_ENUM.len());
        let samples = stablecoin_samples();
        assert_eq!(samples.len(), names.len(), "one sample per variant");
        for (position, name) in names.iter().enumerate() {
            let ix = samples
                .iter()
                .find(|s| stablecoin_name(s) == name)
                .unwrap_or_else(|| panic!("no sample for {name}"));
            assert_eq!(
                variant_index(&words(ix).expect("encodes")),
                Some(STABLECOIN_IDL_POSITION_TO_ENUM[position]),
                "{name}"
            );
        }
        // Spec §3.2 example: Withdraw is enum 17 but IDL position 8.
        let w = words(&stablecoin_core::Instruction::WithdrawCollateral { amount: 1 })
            .expect("encodes");
        assert_eq!(w[0], 17);
    }

    #[test]
    fn negative_controller_gains_encode_and_round_trip() {
        let ix = stablecoin_core::Instruction::SetControllerGains {
            new_proportional_gain: -5_000_000_000_000_000_000_000_000_000,
            new_integral_gain: -1,
        };
        let w = words(&ix).expect("i128 encodes");
        assert_eq!(w[0], 9);
        let back: stablecoin_core::Instruction =
            risc0_zkvm::serde::from_slice(&w).expect("the guest decodes it");
        assert_eq!(format!("{back:?}"), format!("{ix:?}"));
    }

    #[test]
    fn config_instruction_builders_round_trip() {
        let p = EngineParamsArgs::defaults_for_tests();
        let w = words(&init_config_ix(7, &p)).expect("encodes");
        let back: Instruction = risc0_zkvm::serde::from_slice(&w).expect("decodes");
        assert_eq!(back.name(), "init_config");
        assert!(format!("{back:?}").contains("salt: 7"));
        let w = words(&set_collateral_config_ix(
            &CollateralArgs::defaults_for_tests(),
        ))
        .expect("encodes");
        assert_eq!(variant_index(&w), Some(2));
        let w = words(&set_engine_params_ix(&p)).expect("encodes");
        assert_eq!(variant_index(&w), Some(1));
    }
}
