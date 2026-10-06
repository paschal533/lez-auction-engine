#![cfg_attr(not(test), no_main)]

use auction_engine_core::{args::EngineParamsArgs, EngineError};
use auction_engine_program as engine;
use spel_framework::prelude::*;

#[cfg(not(test))]
risc0_zkvm::guest::entry!(main);

/// `SpelError::Custom { code, message }` displays as `Program error {code}: {message}` (raw code);
/// the guest entry point prefixes `[{error_code()}]`, which is 6000 + code (Shared Reference R4).
fn to_spel(e: EngineError) -> spel_framework::error::SpelError {
    spel_framework::error::SpelError::custom(e.code(), e.to_string())
}

/// Three-segment `spel_framework::SpelOutput::execute` path: the macro does not rewrite it, so the
/// handler's explicit claims are kept (ADR-020).
fn finish(result: engine::EngineResult) -> SpelResult {
    let (post_states, chained_calls) = result.map_err(to_spel)?;
    Ok(spel_framework::SpelOutput::execute(post_states, chained_calls))
}

#[lez_program(instruction = "auction_engine_core::Instruction")]
mod auction_engine {
    #[allow(unused_imports)]
    use super::*;

    #[instruction]
    #[allow(clippy::too_many_arguments, reason = "flat spel-encodable arguments (ADR-030)")]
    pub fn init_config(
        ctx: ProgramContext,
        #[account(signer)] admin: AccountWithMetadata,
        #[account(init, pda = [literal("CONFIG"), account("admin"), arg("salt")])] config: AccountWithMetadata,
        #[account(init, pda = [literal("ACCT"), account("config")])] accounting: AccountWithMetadata,
        #[account(mut, pda = [literal("BUFFER"), account("config")])] buffer: AccountWithMetadata,
        protocol_parameters: AccountWithMetadata,
        stablecoin_definition: AccountWithMetadata,
        protocol_token_definition: AccountWithMetadata,
        ref_oracle: AccountWithMetadata,
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
    ) -> SpelResult {
        let p = EngineParamsArgs {
            global_cap, freeze_on_bad_debt, bad_debt_freeze_threshold, protocol_token_enabled,
            protocol_token_ref_price, ref_oracle_enabled, ref_oracle_max_age_ms, surplus_buffer_target,
            surplus_lot, surplus_delay_ms, surplus_premium_bps, surplus_max_discount_bps, debt_lot,
            debt_initial_discount_bps, debt_growth, surplus_decay, max_mint_per_auction, min_take, system_ttl_ms,
        };
        let a = engine::admin::InitConfigAccounts {
            admin, config, accounting, buffer, protocol_parameters, stablecoin_definition,
            protocol_token_definition, ref_oracle,
        };
        finish(engine::admin::init_config(ctx.self_program_id, a, salt, &p))
    }

    #[instruction]
    #[allow(clippy::too_many_arguments, reason = "flat spel-encodable arguments (ADR-030)")]
    pub fn set_engine_params(
        ctx: ProgramContext,
        #[account(signer)] admin: AccountWithMetadata,
        #[account(mut)] config: AccountWithMetadata,
        accounting: AccountWithMetadata,
        coll_config: AccountWithMetadata,
        protocol_token_definition: AccountWithMetadata,
        ref_oracle: AccountWithMetadata,
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
    ) -> SpelResult {
        let p = EngineParamsArgs {
            global_cap, freeze_on_bad_debt, bad_debt_freeze_threshold, protocol_token_enabled,
            protocol_token_ref_price, ref_oracle_enabled, ref_oracle_max_age_ms, surplus_buffer_target,
            surplus_lot, surplus_delay_ms, surplus_premium_bps, surplus_max_discount_bps, debt_lot,
            debt_initial_discount_bps, debt_growth, surplus_decay, max_mint_per_auction, min_take, system_ttl_ms,
        };
        let a = engine::admin::SetEngineParamsAccounts { admin, config, accounting, coll_config, protocol_token_definition, ref_oracle };
        finish(engine::admin::set_engine_params(ctx.self_program_id, a, &p))
    }
}
