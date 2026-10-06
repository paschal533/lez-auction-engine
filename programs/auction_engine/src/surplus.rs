//! F5 surplus auction (spec §5.11): instant Dutch, protocol tokens burned in the same transaction.
//! The lot moves from the buffer into its own `["SURPVAULT", surplus_auction]` holding at start, so
//! a later `settle_debt` cannot spend it (ADR-031). The reference price is snapshotted at start, so
//! takes read neither the reference oracle nor its max age.

use auction_core::system::{close_surplus, start_surplus, take_surplus as core_take};
use auction_engine_core::{pda, state::*, EngineError};
use lee_core::{
    account::{AccountId, AccountWithMetadata},
    program::ProgramId,
};

use crate::{accounts::*, settle, system, token_calls, EngineResult};

fn load_surplus(
    program_id: &ProgramId,
    config_id: &AccountId,
    acc: &AccountWithMetadata,
) -> Result<SurplusAuctionState, EngineError> {
    if acc.account.program_owner != *program_id {
        return Err(EngineError::WrongAccount("surplus_auction"));
    }
    let s = SurplusAuctionState::try_from(&acc.account.data)
        .map_err(|_| EngineError::WrongAccount("surplus_auction"))?;
    if s.config != *config_id {
        return Err(EngineError::WrongAccount("surplus_auction"));
    }
    expect_id(
        acc,
        &pda::surplus_auction_pda(program_id, config_id, s.nonce),
        "surplus_auction",
    )?;
    Ok(s)
}

pub struct StartSurplusAccounts {
    pub caller: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    pub buffer: AccountWithMetadata,
    pub stablecoin_definition: AccountWithMetadata,
    pub surplus_auction: AccountWithMetadata,
    pub surplus_vault: AccountWithMetadata,
    pub ref_oracle: AccountWithMetadata,
    pub clock: AccountWithMetadata,
}

/// Permissionless. Runs an implicit `settle_debt(MAX)` first, then the core start rules (no unbacked
/// debt left, buffer >= target + lot, delay since the last end); moves the lot into the new vault.
pub fn start_surplus_auction(
    program_id: ProgramId,
    a: StartSurplusAccounts,
    nonce: u64,
) -> EngineResult {
    if !a.caller.is_authorized {
        return Err(EngineError::WrongAccount("caller"));
    }
    let now = read_clock(&a.clock)?;
    let cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let acc = load_accounting(&program_id, &config_id, &a.accounting)?;
    if nonce != acc.surplus_nonce {
        return Err(EngineError::NonceMismatch);
    }
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
        &a.surplus_auction,
        &pda::surplus_auction_pda(&program_id, &config_id, nonce),
        "surplus_auction",
    )?;
    expect_default(&a.surplus_auction, "surplus_auction")?;
    expect_id(
        &a.surplus_vault,
        &pda::surplus_vault_pda(&program_id, &a.surplus_auction.account_id),
        "surplus_vault",
    )?;
    expect_default(&a.surplus_vault, "surplus_vault")?;
    let reference = system::ref_price(&program_id, &config_id, &cfg, &a.ref_oracle, now)?;

    let (settled, burned) = settle::implicit_settle(&acc, buffer_balance);
    let buffer_after = buffer_balance
        .checked_sub(burned)
        .ok_or(EngineError::Math)?;
    let sys = cfg.system.to_core()?;
    let started = start_surplus(
        &settled.core(),
        buffer_after,
        cfg.protocol_token.is_some(),
        acc.last_surplus_end_ms,
        now,
        &sys,
        acc.surplus_live,
    )?;
    let state = SurplusAuctionState {
        tag: TAG_SURPLUS,
        config: config_id,
        nonce,
        lot_left: started.lot_left,
        start_ms: started.start_ms,
        live: started.live,
        ref_ray: reference,
    };
    let next_acc = AccountingState {
        surplus_live: true,
        surplus_nonce: acc.surplus_nonce.checked_add(1).ok_or(EngineError::Math)?,
        total_surplus_lots_out: settled
            .total_surplus_lots_out
            .checked_add(started.lot_left)
            .ok_or(EngineError::Math)?,
        ..settled
    };

    let vault_seed = pda::surplus_vault_seed(&a.surplus_auction.account_id);
    let mut calls = vec![token_calls::initialize_account(
        token,
        a.stablecoin_definition.clone(),
        token_calls::authorized(&a.surplus_vault),
        vec![vault_seed],
    )];
    calls.extend(settle::buffer_burn(
        &cfg,
        &config_id,
        &a.stablecoin_definition,
        &a.buffer,
        burned,
    ));
    calls.push(token_calls::transfer(
        token,
        token_calls::authorized(&token_calls::with_balance(
            &a.buffer,
            cfg.stablecoin_definition_id,
            buffer_after,
        )),
        token_calls::post_init(&a.surplus_vault, token, cfg.stablecoin_definition_id),
        started.lot_left,
        vec![pda::buffer_seed(&config_id)],
    ));
    Ok((
        vec![
            echo(&a.caller),
            echo(&a.config),
            write(&a.accounting, &next_acc),
            echo(&a.buffer),
            echo(&a.stablecoin_definition),
            create(
                &a.surplus_auction,
                &state,
                pda::surplus_auction_seed(&config_id, nonce),
            ),
            echo(&a.surplus_vault),
            echo(&a.ref_oracle),
            echo(&a.clock),
        ],
        calls,
    ))
}

pub struct TakeSurplusAccounts {
    pub taker_protocol_holding: AccountWithMetadata,
    pub taker_stable_holding: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    pub surplus_auction: AccountWithMetadata,
    pub surplus_vault: AccountWithMetadata,
    pub protocol_token_definition: AccountWithMetadata,
    pub clock: AccountWithMetadata,
}

/// The taker pays protocol tokens (burned from the signed taker holding) for
/// `filled = old.lot_left - new.lot_left` stablecoin. Only the filled amount leaves the vault.
pub fn take_surplus(
    program_id: ProgramId,
    a: TakeSurplusAccounts,
    amount_coin: u128,
    max_protocol_in: u128,
) -> EngineResult {
    if !a.taker_protocol_holding.is_authorized {
        return Err(EngineError::WrongAccount("taker_protocol_holding"));
    }
    let now = read_clock(&a.clock)?;
    let cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let acc = load_accounting(&program_id, &config_id, &a.accounting)?;
    let sa = load_surplus(&program_id, &config_id, &a.surplus_auction)?;
    // Use-time checks of the protocol token definition (Task 8 review): id, owner, fungible type.
    let status =
        system::protocol_token_status(&program_id, &config_id, &cfg, &a.protocol_token_definition)?;
    let protocol = cfg.protocol_token.ok_or(EngineError::TokenNotConfigured)?;
    if !status.token_ok {
        return Err(EngineError::ProtocolTokenMismatch);
    }
    let token = cfg.token_program_id;
    expect_id(
        &a.surplus_vault,
        &pda::surplus_vault_pda(&program_id, &a.surplus_auction.account_id),
        "surplus_vault",
    )?;
    let vault_balance = fungible_balance(&a.surplus_vault, &token, &cfg.stablecoin_definition_id)?;

    let sys = cfg.system.to_core()?;
    let (next, tokens) = core_take(
        &sa.core(),
        amount_coin,
        max_protocol_in,
        sa.ref_ray,
        &sys,
        now,
    )?;
    let filled = sa
        .lot_left
        .checked_sub(next.lot_left)
        .ok_or(EngineError::Math)?;
    if vault_balance < filled {
        return Err(EngineError::VaultBelowBook);
    }
    if fungible_balance(&a.taker_protocol_holding, &token, &protocol)? < tokens {
        return Err(EngineError::InsufficientBalance);
    }
    if a.taker_stable_holding.account_id == a.surplus_vault.account_id {
        return Err(EngineError::WrongAccount("taker_stable_holding"));
    }
    check_recipient(
        &a.taker_stable_holding,
        &token,
        &cfg.stablecoin_definition_id,
    )?;

    let mut next_acc = acc.clone();
    next_acc.total_protocol_burned = next_acc
        .total_protocol_burned
        .checked_add(tokens)
        .ok_or(EngineError::Math)?;
    if !next.live {
        next_acc.surplus_live = false;
        next_acc.last_surplus_end_ms = now;
    }
    let calls = vec![
        token_calls::burn(
            token,
            a.protocol_token_definition.clone(),
            a.taker_protocol_holding.clone(),
            tokens,
            vec![],
        ),
        token_calls::transfer(
            token,
            token_calls::authorized(&a.surplus_vault),
            a.taker_stable_holding.clone(),
            filled,
            vec![pda::surplus_vault_seed(&a.surplus_auction.account_id)],
        ),
    ];
    Ok((
        vec![
            echo(&a.taker_protocol_holding),
            echo(&a.taker_stable_holding),
            echo(&a.config),
            write(&a.accounting, &next_acc),
            write(&a.surplus_auction, &sa.with_core(&next)),
            echo(&a.surplus_vault),
            echo(&a.protocol_token_definition),
            echo(&a.clock),
        ],
        calls,
    ))
}

pub struct CloseSurplusAccounts {
    pub caller: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    pub surplus_auction: AccountWithMetadata,
    pub surplus_vault: AccountWithMetadata,
    pub buffer: AccountWithMetadata,
    pub clock: AccountWithMetadata,
}

/// After `system_ttl_ms` the untaken lot (whole vault balance, ADR-033) returns to the buffer.
pub fn close_surplus_auction(program_id: ProgramId, a: CloseSurplusAccounts) -> EngineResult {
    if !a.caller.is_authorized {
        return Err(EngineError::WrongAccount("caller"));
    }
    let now = read_clock(&a.clock)?;
    let cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let acc = load_accounting(&program_id, &config_id, &a.accounting)?;
    let sa = load_surplus(&program_id, &config_id, &a.surplus_auction)?;
    let (next, _remainder) = close_surplus(&sa.core(), &cfg.system.to_core()?, now)?;
    let token = cfg.token_program_id;
    expect_id(
        &a.surplus_vault,
        &pda::surplus_vault_pda(&program_id, &a.surplus_auction.account_id),
        "surplus_vault",
    )?;
    expect_id(
        &a.buffer,
        &pda::buffer_pda(&program_id, &config_id),
        "buffer",
    )?;
    let vault_balance = fungible_balance(&a.surplus_vault, &token, &cfg.stablecoin_definition_id)?;
    fungible_balance(&a.buffer, &token, &cfg.stablecoin_definition_id)?;
    let next_acc = AccountingState {
        surplus_live: false,
        last_surplus_end_ms: now,
        total_surplus_returned: acc
            .total_surplus_returned
            .checked_add(vault_balance)
            .ok_or(EngineError::Math)?,
        ..acc
    };
    let calls = if vault_balance > 0 {
        vec![token_calls::transfer(
            token,
            token_calls::authorized(&a.surplus_vault),
            a.buffer.clone(),
            vault_balance,
            vec![pda::surplus_vault_seed(&a.surplus_auction.account_id)],
        )]
    } else {
        vec![]
    };
    Ok((
        vec![
            echo(&a.caller),
            echo(&a.config),
            write(&a.accounting, &next_acc),
            write(&a.surplus_auction, &sa.with_core(&next)),
            echo(&a.surplus_vault),
            echo(&a.buffer),
            echo(&a.clock),
        ],
        calls,
    ))
}
