//! `check_solvency()` (spec §5.11, ADR-017): implicit `settle_debt`, then the core decision. Freeze ->
//! chain `stablecoin.engine_freeze_new_debt` signed by `["ENGINE_AUTH", config]`; RecoveryInProgress ->
//! revert (freezes nothing); Healthy / NoAction -> only the implicit settle is applied.
//!
//! Variant C (ADR-019): the `ENGINE_AUTH` seed is attached only to the one call whose program id is
//! `cfg.host_program_id`; every forwarded account id is checked against engine-validated config before
//! use, and both optional slots are pinned (ADR-030), so a caller cannot steer the decision.
//!
//! Idempotence: on an already-frozen host the host's freeze is idempotent, so a repeated Freeze is a
//! harmless re-freeze. `host_frozen_by_engine` records that the engine has frozen this host at least
//! once; it is not the current freeze state (an unfreeze by the host's freeze authority does not clear
//! it, and the engine never clears it).

use auction_core::system::{
    check_solvency as decide, start_debt, SolvencyDecision, SystemAuctionError,
};
use auction_engine_core::{pda, state::*, EngineError};
use lee_core::{
    account::AccountWithMetadata,
    program::{ChainedCall, ProgramId},
};
use stablecoin_core::compute_liquidation_engine_state_pda;

use crate::{accounts::*, settle, system, token_calls, EngineResult};

pub struct CheckSolvencyAccounts {
    pub caller: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    pub buffer: AccountWithMetadata,
    pub stablecoin_definition: AccountWithMetadata,
    pub protocol_token_definition: AccountWithMetadata,
    pub ref_oracle: AccountWithMetadata,
    pub engine_authority: AccountWithMetadata,
    pub liquidation_engine_state: AccountWithMetadata,
    pub protocol_parameters: AccountWithMetadata,
    pub clock: AccountWithMetadata,
}

/// Permissionless (any signer).
pub fn check_solvency(program_id: ProgramId, a: CheckSolvencyAccounts) -> EngineResult {
    if !a.caller.is_authorized {
        return Err(EngineError::WrongAccount("caller"));
    }
    let now = read_clock(&a.clock)?;
    let cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let acc = load_accounting(&program_id, &config_id, &a.accounting)?;
    let token = cfg.token_program_id;
    expect_id(
        &a.buffer,
        &pda::buffer_pda(&program_id, &config_id),
        "buffer",
    )?;
    expect_id(
        &a.stablecoin_definition,
        &cfg.stablecoin_definition_id,
        "stablecoin_definition",
    )?;
    let buffer_balance = fungible_balance(&a.buffer, &token, &cfg.stablecoin_definition_id)?;
    expect_id(
        &a.engine_authority,
        &pda::engine_authority_pda(&program_id, &config_id),
        "engine_authority",
    )?;
    expect_id(
        &a.liquidation_engine_state,
        &compute_liquidation_engine_state_pda(cfg.host_program_id),
        "liquidation_engine_state",
    )?;
    expect_id(
        &a.protocol_parameters,
        &cfg.host_protocol_parameters_id,
        "protocol_parameters",
    )?;
    let status =
        system::protocol_token_status(&program_id, &config_id, &cfg, &a.protocol_token_definition)?;
    let reference = system::ref_price(&program_id, &config_id, &cfg, &a.ref_oracle, now)?;

    // Implicit settle_debt(MAX): the shared helper keeps `debt_on_auction` excluded.
    let (settled, burned) = settle::implicit_settle(&acc, buffer_balance);
    let buffer_after = buffer_balance
        .checked_sub(burned)
        .ok_or(EngineError::Math)?;
    let start_check = if status.configured {
        start_debt(
            &settled.core(),
            buffer_after,
            true,
            status.mint_authority_ok,
            status.token_ok,
            reference,
            &cfg.system.to_core()?,
            now,
            acc.debt_live,
        )
        .map(|_| ())
    } else {
        Err(SystemAuctionError::TokenNotConfigured)
    };
    let decision = decide(
        settled.unbacked_debt,
        cfg.bad_debt_freeze_threshold,
        status.configured,
        cfg.freeze_on_bad_debt,
        acc.debt_live,
        start_check,
    );
    let mut calls: Vec<ChainedCall> = settle::buffer_burn(
        &cfg,
        &config_id,
        &a.stablecoin_definition,
        &a.buffer,
        burned,
    )
    .into_iter()
    .collect();
    let next_acc = match decision {
        SolvencyDecision::RecoveryInProgress => return Err(EngineError::RecoveryInProgress),
        SolvencyDecision::Freeze => {
            calls.push(
                ChainedCall::new(
                    cfg.host_program_id,
                    vec![
                        token_calls::authorized(&a.engine_authority),
                        a.liquidation_engine_state.clone(),
                        a.protocol_parameters.clone(),
                    ],
                    &stablecoin_core::Instruction::EngineFreezeNewDebt,
                )
                .with_pda_seeds(vec![pda::engine_authority_seed(&config_id)]),
            );
            AccountingState {
                host_frozen_by_engine: true,
                ..settled
            }
        }
        SolvencyDecision::Healthy | SolvencyDecision::NoAction => settled,
    };
    Ok((
        vec![
            echo(&a.caller),
            echo(&a.config),
            write(&a.accounting, &next_acc),
            echo(&a.buffer),
            echo(&a.stablecoin_definition),
            echo(&a.protocol_token_definition),
            echo(&a.ref_oracle),
            echo(&a.engine_authority),
            echo(&a.liquidation_engine_state),
            echo(&a.protocol_parameters),
            echo(&a.clock),
        ],
        calls,
    ))
}
