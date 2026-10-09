//! The engine instruction enum. Variant order == guest function order == IDL order (spel-cli
//! encodes by IDL position; the guest decodes by enum index). Never reorder; only append, in
//! Shared Reference R2 order.
//! Account lists: Shared Reference R2 / `docs/architecture/programs.md`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Instruction {
    /// Accounts: admin(s), config(i), accounting(i), buffer, protocol_parameters, stablecoin_definition,
    /// protocol_token_definition [slot 1], ref_oracle [slot 0].
    InitConfig {
        salt: u64,
        global_cap: u128,
        freeze_on_bad_debt: bool,
        bad_debt_freeze_threshold: u128,
        protocol_token_enabled: bool,
        protocol_token_ref_price: u128,
        ref_oracle_enabled: bool,
        ref_oracle_max_age_ms: u64,
        surplus_buffer_target: u128,
        surplus_lot: u128,
        surplus_delay_ms: u64,
        surplus_premium_bps: u32,
        surplus_max_discount_bps: u32,
        debt_lot: u128,
        debt_initial_discount_bps: u32,
        debt_growth: Vec<u32>,
        surplus_decay: Vec<u32>,
        max_mint_per_auction: u128,
        min_take: u128,
        system_ttl_ms: u64,
    },
    /// Accounts: admin(s), config, accounting, coll_config (read, may be uninitialised), protocol_token_definition
    /// [slot 1], ref_oracle [slot 0].
    SetEngineParams {
        global_cap: u128,
        freeze_on_bad_debt: bool,
        bad_debt_freeze_threshold: u128,
        protocol_token_enabled: bool,
        protocol_token_ref_price: u128,
        ref_oracle_enabled: bool,
        ref_oracle_max_age_ms: u64,
        surplus_buffer_target: u128,
        surplus_lot: u128,
        surplus_delay_ms: u64,
        surplus_premium_bps: u32,
        surplus_max_discount_bps: u32,
        debt_lot: u128,
        debt_initial_discount_bps: u32,
        debt_growth: Vec<u32>,
        surplus_decay: Vec<u32>,
        max_mint_per_auction: u128,
        min_take: u128,
        system_ttl_ms: u64,
    },
    /// Accounts: admin(s), config, collateral_definition, coll_config(m), oracle, protocol_parameters,
    /// redemption_price_state, clock.
    SetCollateralConfig {
        oracle_scale: u8,
        max_oracle_age_ms: u64,
        max_conf_bps: u32,
        band_lo_bps: u32,
        band_hi_bps: u32,
        penalty_bps: u32,
        tip: u128,
        chip_bps: u32,
        reward_cap: u128,
        debt_floor: u128,
        min_bid: u128,
        type_cap: u128,
        schedule: Vec<u32>,
        admin_paused: bool,
    },
    /// Accounts: caller(s), config, coll_config(m), redemption_price_state, oracle, clock.
    ObserveFeed,
    /// Accounts: keeper(s), keeper_collateral_holding(m), config(m), coll_config(m), position(m), record(m),
    /// auction(i,m), auction_vault(m), engine_authority, liquidation_engine_state, host_vault(m),
    /// collateral_definition, stability_fee_accumulator, redemption_price_state, oracle, protocol_parameters, clock.
    Liquidate { nonce: u64 },
    /// Accounts: bidder_stable_holding(s,m), bidder_collateral_holding(m), config, coll_config(m), auction(m),
    /// auction_vault(m), buffer(m), stablecoin_definition(m), redemption_price_state, oracle, clock.
    /// Reverts with `ChainEndAccountsMissing` if the fill would end the auction (ADR-029).
    Bid {
        coin_in_max: u128,
        min_collateral_out: u128,
    },
    /// Accounts: as `Bid`, then accounting(m). May end the auction (chain-end effects applied once).
    BidWithSettlement {
        coin_in_max: u128,
        min_collateral_out: u128,
    },
    /// Accounts: caller(s), config(m), coll_config(m), accounting(m), auction(m). Settles a lot-0 auction.
    SettleEmpty,
    /// Accounts: owner(s), owner_collateral_holding(m), config, auction(m), auction_vault(m).
    ClaimLeftover,
    /// Accounts: caller(s), config, accounting(m), buffer(m), stablecoin_definition(m).
    SettleDebt { amount: u128 },
    /// Accounts: caller(s), config, accounting(m), buffer(m), stablecoin_definition(m), surplus_auction(i,m),
    /// surplus_vault(m), ref_oracle [slot 0], clock.
    StartSurplusAuction { nonce: u64 },
    /// Accounts: taker_protocol_holding(s,m), taker_stable_holding(m), config, accounting(m), surplus_auction(m),
    /// surplus_vault(m), protocol_token_definition(m) [slot 1], clock.
    TakeSurplus {
        amount_coin: u128,
        max_protocol_in: u128,
    },
    /// Accounts: caller(s), config, accounting(m), surplus_auction(m), surplus_vault(m), buffer(m), clock.
    CloseSurplusAuction,
    /// Accounts: caller(s), config, accounting(m), buffer(m), stablecoin_definition(m), debt_auction(i,m),
    /// protocol_token_definition [slot 1], ref_oracle [slot 0], clock.
    StartDebtAuction { nonce: u64 },
    /// Accounts: taker_stable_holding(s,m), taker_protocol_holding(m), config, accounting(m), debt_auction(m),
    /// stablecoin_definition(m), protocol_token_definition(m), mint_authority, clock.
    TakeDebt {
        amount_coin: u128,
        min_protocol_out: u128,
    },
    /// Accounts: caller(s), config, accounting(m), debt_auction(m), clock.
    CloseDebtAuction,
    /// Accounts: caller(s), config, accounting(m), buffer(m), stablecoin_definition(m), protocol_token_definition
    /// [slot 1], ref_oracle [slot 0], engine_authority, liquidation_engine_state, protocol_parameters(m), clock.
    CheckSolvency,
}

impl Instruction {
    /// IDL name of the variant (== guest function name). Exhaustive on purpose: a new variant does not
    /// compile until it is named here; `instruction_samples` must then gain its sample, which the
    /// integration test `every_engine_variant_has_a_sample_and_an_idl_entry_at_its_enum_index` checks
    /// against the regenerated IDL.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::InitConfig { .. } => "init_config",
            Self::SetEngineParams { .. } => "set_engine_params",
            Self::SetCollateralConfig { .. } => "set_collateral_config",
            Self::ObserveFeed => "observe_feed",
            Self::Liquidate { .. } => "liquidate",
            Self::Bid { .. } => "bid",
            Self::BidWithSettlement { .. } => "bid_with_settlement",
            Self::SettleEmpty => "settle_empty",
            Self::ClaimLeftover => "claim_leftover",
            Self::SettleDebt { .. } => "settle_debt",
            Self::StartSurplusAuction { .. } => "start_surplus_auction",
            Self::TakeSurplus { .. } => "take_surplus",
            Self::CloseSurplusAuction => "close_surplus_auction",
            Self::StartDebtAuction { .. } => "start_debt_auction",
            Self::TakeDebt { .. } => "take_debt",
            Self::CloseDebtAuction => "close_debt_auction",
            Self::CheckSolvency => "check_solvency",
        }
    }
}

/// One sample per `Instruction` variant, in enum order (index `i` is variant `i`).
///
/// Test support: lets tests derive per-variant tables (admin set, enum indices) from the enum
/// instead of keeping a hand-written list. Not part of the public API.
#[doc(hidden)]
#[must_use]
pub fn instruction_samples() -> Vec<Instruction> {
    vec![
        Instruction::InitConfig {
            salt: 0,
            global_cap: 0,
            freeze_on_bad_debt: false,
            bad_debt_freeze_threshold: 0,
            protocol_token_enabled: false,
            protocol_token_ref_price: 0,
            ref_oracle_enabled: false,
            ref_oracle_max_age_ms: 0,
            surplus_buffer_target: 0,
            surplus_lot: 0,
            surplus_delay_ms: 0,
            surplus_premium_bps: 0,
            surplus_max_discount_bps: 0,
            debt_lot: 0,
            debt_initial_discount_bps: 0,
            debt_growth: vec![],
            surplus_decay: vec![],
            max_mint_per_auction: 0,
            min_take: 0,
            system_ttl_ms: 0,
        },
        Instruction::SetEngineParams {
            global_cap: 0,
            freeze_on_bad_debt: false,
            bad_debt_freeze_threshold: 0,
            protocol_token_enabled: false,
            protocol_token_ref_price: 0,
            ref_oracle_enabled: false,
            ref_oracle_max_age_ms: 0,
            surplus_buffer_target: 0,
            surplus_lot: 0,
            surplus_delay_ms: 0,
            surplus_premium_bps: 0,
            surplus_max_discount_bps: 0,
            debt_lot: 0,
            debt_initial_discount_bps: 0,
            debt_growth: vec![],
            surplus_decay: vec![],
            max_mint_per_auction: 0,
            min_take: 0,
            system_ttl_ms: 0,
        },
        Instruction::SetCollateralConfig {
            oracle_scale: 0,
            max_oracle_age_ms: 0,
            max_conf_bps: 0,
            band_lo_bps: 0,
            band_hi_bps: 0,
            penalty_bps: 0,
            tip: 0,
            chip_bps: 0,
            reward_cap: 0,
            debt_floor: 0,
            min_bid: 0,
            type_cap: 0,
            schedule: vec![],
            admin_paused: false,
        },
        Instruction::ObserveFeed,
        Instruction::Liquidate { nonce: 0 },
        Instruction::Bid {
            coin_in_max: 0,
            min_collateral_out: 0,
        },
        Instruction::BidWithSettlement {
            coin_in_max: 0,
            min_collateral_out: 0,
        },
        Instruction::SettleEmpty,
        Instruction::ClaimLeftover,
        Instruction::SettleDebt { amount: 0 },
        Instruction::StartSurplusAuction { nonce: 0 },
        Instruction::TakeSurplus {
            amount_coin: 0,
            max_protocol_in: 0,
        },
        Instruction::CloseSurplusAuction,
        Instruction::StartDebtAuction { nonce: 0 },
        Instruction::TakeDebt {
            amount_coin: 0,
            min_protocol_out: 0,
        },
        Instruction::CloseDebtAuction,
        Instruction::CheckSolvency,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instruction_samples_cover_every_variant_in_order() {
        let samples = instruction_samples();
        for (i, ix) in samples.iter().enumerate() {
            let words = risc0_zkvm::serde::to_vec(ix).expect("serializes");
            assert_eq!(
                usize::try_from(words[0]).expect("u32 fits usize"),
                i,
                "sample {i} ({}) is variant {i}",
                ix.name()
            );
        }
        let mut names: Vec<&str> = samples.iter().map(Instruction::name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), samples.len(), "one sample per variant");
    }
}
