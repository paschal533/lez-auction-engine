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
    // Tasks 14–15 append the remaining variants here, in Shared Reference R2 order
    // (start_debt_auction = 13 … check_solvency = 16). Each appending task also appends the guest
    // function and an index assertion to `instruction_indices_follow_reference_order`.
}
