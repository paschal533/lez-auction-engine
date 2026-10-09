//! F4 debt auction (spec §5.11, ADR-017, ADR-024): the taker's stablecoin is burned and reduces
//! `unbacked_debt`; mock protocol tokens are minted by the engine's `["MINT_AUTH", config]` PDA.
//! `debt_on_auction` reserves the lot so `settle_debt` cannot cover the same debt twice.
//!
//! Pairing (ADR-024): `start_debt` <-> `record_debt_start(debt_lot)`; every take <->
//! `record_debt_take(filled)` and a burn of exactly that filled amount; `close_debt` <->
//! `record_debt_close(lot_left)`. The reservation therefore never exceeds the debt it reserves.

use auction_core::system::{close_debt, start_debt, take_debt as core_take};
use auction_engine_core::{args::DEBT_RELIST_DELAY_MS, pda, state::*, EngineError};
use lee_core::{
    account::{AccountId, AccountWithMetadata},
    program::ProgramId,
};

use crate::{accounts::*, settle, system, token_calls, EngineResult};

fn load_debt(
    program_id: &ProgramId,
    config_id: &AccountId,
    acc: &AccountWithMetadata,
) -> Result<DebtAuctionState, EngineError> {
    if acc.account.program_owner != *program_id {
        return Err(EngineError::WrongAccount("debt_auction"));
    }
    let d = DebtAuctionState::try_from(&acc.account.data)
        .map_err(|_| EngineError::WrongAccount("debt_auction"))?;
    if d.config != *config_id {
        return Err(EngineError::WrongAccount("debt_auction"));
    }
    expect_id(
        acc,
        &pda::debt_auction_pda(program_id, config_id, d.nonce),
        "debt_auction",
    )?;
    Ok(d)
}

pub struct StartDebtAccounts {
    pub caller: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    pub buffer: AccountWithMetadata,
    pub stablecoin_definition: AccountWithMetadata,
    pub debt_auction: AccountWithMetadata,
    pub protocol_token_definition: AccountWithMetadata,
    pub ref_oracle: AccountWithMetadata,
    pub clock: AccountWithMetadata,
}

/// Permissionless. Runs an implicit `settle_debt(MAX)` first, then the core start rules (one live
/// auction, token configured and well-formed, engine mint authority, `unbacked - buffer >= debt_lot`,
/// a usable start amount) and reserves the lot. The reference price is snapshotted into the start amount.
pub fn start_debt_auction(program_id: ProgramId, a: StartDebtAccounts, nonce: u64) -> EngineResult {
    if !a.caller.is_authorized {
        return Err(EngineError::WrongAccount("caller"));
    }
    let now = read_clock(&a.clock)?;
    let cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let acc = load_accounting(&program_id, &config_id, &a.accounting)?;
    if nonce != acc.debt_nonce {
        return Err(EngineError::NonceMismatch);
    }
    // ADR-039: no relisting within DEBT_RELIST_DELAY_MS of the previous debt auction's end. `now` is the
    // clock account's time; the sum saturates, so an overflow blocks (fails closed) rather than wraps.
    if acc.last_debt_end_ms > 0 && now < acc.last_debt_end_ms.saturating_add(DEBT_RELIST_DELAY_MS) {
        return Err(EngineError::DelayNotElapsed);
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
        &a.debt_auction,
        &pda::debt_auction_pda(&program_id, &config_id, nonce),
        "debt_auction",
    )?;
    expect_default(&a.debt_auction, "debt_auction")?;
    let status =
        system::protocol_token_status(&program_id, &config_id, &cfg, &a.protocol_token_definition)?;
    let reference = system::ref_price(&program_id, &config_id, &cfg, &a.ref_oracle, now)?;

    let (settled, burned) = settle::implicit_settle(&acc, buffer_balance);
    let buffer_after = buffer_balance
        .checked_sub(burned)
        .ok_or(EngineError::Math)?;
    let sys = cfg.system.to_core()?;
    let started = start_debt(
        &settled.core(),
        buffer_after,
        status.configured,
        status.mint_authority_ok,
        status.token_ok,
        reference,
        &sys,
        now,
        acc.debt_live,
    )?;
    let next_acc = AccountingState {
        debt_live: true,
        debt_nonce: acc.debt_nonce.checked_add(1).ok_or(EngineError::Math)?,
        ..AccountingState::with_core(
            &settled,
            &settled.core().record_debt_start(started.lot_left),
        )
    };
    let state = DebtAuctionState {
        tag: TAG_DEBT,
        config: config_id,
        nonce,
        lot_left: started.lot_left,
        start_amount: started.start_amount,
        start_ms: started.start_ms,
        live: started.live,
    };
    let calls = settle::buffer_burn(
        &cfg,
        &config_id,
        &a.stablecoin_definition,
        &a.buffer,
        burned,
    )
    .into_iter()
    .collect();
    Ok((
        vec![
            echo(&a.caller),
            echo(&a.config),
            write(&a.accounting, &next_acc),
            echo(&a.buffer),
            echo(&a.stablecoin_definition),
            create(
                &a.debt_auction,
                &state,
                pda::debt_auction_seed(&config_id, nonce),
            ),
            echo(&a.protocol_token_definition),
            echo(&a.ref_oracle),
            echo(&a.clock),
        ],
        calls,
    ))
}

pub struct TakeDebtAccounts {
    pub taker_stable_holding: AccountWithMetadata,
    pub taker_protocol_holding: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    pub debt_auction: AccountWithMetadata,
    pub stablecoin_definition: AccountWithMetadata,
    pub protocol_token_definition: AccountWithMetadata,
    pub mint_authority: AccountWithMetadata,
    pub clock: AccountWithMetadata,
}

/// The taker burns `filled = old.lot_left - new.lot_left` stablecoin (its signed holding) and receives
/// `floor(offered x filled / debt_lot)` freshly minted protocol tokens, capped by `max_mint_per_auction`
/// in the core. A fresh taker protocol holding must sign (the token program claims it, S7).
pub fn take_debt(
    program_id: ProgramId,
    a: TakeDebtAccounts,
    amount_coin: u128,
    min_protocol_out: u128,
) -> EngineResult {
    if !a.taker_stable_holding.is_authorized {
        return Err(EngineError::WrongAccount("taker_stable_holding"));
    }
    let now = read_clock(&a.clock)?;
    let cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let acc = load_accounting(&program_id, &config_id, &a.accounting)?;
    let da = load_debt(&program_id, &config_id, &a.debt_auction)?;
    // Use-time checks of the protocol token definition: id, owner, fungible type, mint authority.
    let status =
        system::protocol_token_status(&program_id, &config_id, &cfg, &a.protocol_token_definition)?;
    let protocol = cfg.protocol_token.ok_or(EngineError::TokenNotConfigured)?;
    if !status.token_ok {
        return Err(EngineError::ProtocolTokenMismatch);
    }
    if !status.mint_authority_ok {
        return Err(EngineError::MintAuthorityMissing);
    }
    expect_id(
        &a.stablecoin_definition,
        &cfg.stablecoin_definition_id,
        "stablecoin_definition",
    )?;
    expect_id(
        &a.mint_authority,
        &pda::mint_authority_pda(&program_id, &config_id),
        "mint_authority",
    )?;
    let token = cfg.token_program_id;

    let (next, minted) = core_take(
        &da.core(),
        amount_coin,
        min_protocol_out,
        &cfg.system.to_core()?,
        now,
    )?;
    let filled = da
        .lot_left
        .checked_sub(next.lot_left)
        .ok_or(EngineError::Math)?;
    if fungible_balance(
        &a.taker_stable_holding,
        &token,
        &cfg.stablecoin_definition_id,
    )? < filled
    {
        return Err(EngineError::InsufficientBalance);
    }
    if a.taker_protocol_holding.account_id == a.taker_stable_holding.account_id {
        return Err(EngineError::WrongAccount("taker_protocol_holding"));
    }
    check_recipient(&a.taker_protocol_holding, &token, &protocol)?;
    // The reservation can only be released by what was reserved (ADR-024 pairing).
    if filled > acc.debt_on_auction {
        return Err(EngineError::Math);
    }

    let mut next_acc = AccountingState::with_core(&acc, &acc.core().record_debt_take(filled));
    next_acc.total_protocol_minted = next_acc
        .total_protocol_minted
        .checked_add(minted)
        .ok_or(EngineError::Math)?;
    if !next.live {
        next_acc.debt_live = false;
        next_acc.last_debt_end_ms = now; // ADR-039
    }
    let calls = vec![
        token_calls::burn(
            token,
            a.stablecoin_definition.clone(),
            a.taker_stable_holding.clone(),
            filled,
            vec![],
        ),
        token_calls::mint_with_authority(
            token,
            a.protocol_token_definition.clone(),
            a.taker_protocol_holding.clone(),
            token_calls::authorized(&a.mint_authority),
            minted,
            vec![pda::mint_authority_seed(&config_id)],
        ),
    ];
    Ok((
        vec![
            echo(&a.taker_stable_holding),
            echo(&a.taker_protocol_holding),
            echo(&a.config),
            write(&a.accounting, &next_acc),
            write(&a.debt_auction, &da.with_core(&next)),
            echo(&a.stablecoin_definition),
            echo(&a.protocol_token_definition),
            echo(&a.mint_authority),
            echo(&a.clock),
        ],
        calls,
    ))
}

pub struct CloseDebtAccounts {
    pub caller: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    pub debt_auction: AccountWithMetadata,
    pub clock: AccountWithMetadata,
}

/// After `system_ttl_ms`: nothing is minted for the untaken part; its reservation is released so
/// `settle_debt` can cover that debt again.
pub fn close_debt_auction(program_id: ProgramId, a: CloseDebtAccounts) -> EngineResult {
    if !a.caller.is_authorized {
        return Err(EngineError::WrongAccount("caller"));
    }
    let now = read_clock(&a.clock)?;
    let cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let acc = load_accounting(&program_id, &config_id, &a.accounting)?;
    let da = load_debt(&program_id, &config_id, &a.debt_auction)?;
    let (next, released) = close_debt(&da.core(), &cfg.system.to_core()?, now)?;
    if released > acc.debt_on_auction {
        return Err(EngineError::Math);
    }
    let next_acc = AccountingState {
        debt_live: false,
        last_debt_end_ms: now, // ADR-039
        ..AccountingState::with_core(&acc, &acc.core().record_debt_close(released))
    };
    Ok((
        vec![
            echo(&a.caller),
            echo(&a.config),
            write(&a.accounting, &next_acc),
            write(&a.debt_auction, &da.with_core(&next)),
            echo(&a.clock),
        ],
        vec![],
    ))
}
