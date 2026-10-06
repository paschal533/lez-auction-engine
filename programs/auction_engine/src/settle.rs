//! `settle_empty` (ADR-024), `claim_leftover` (pull model, owner check, ADR-033), `settle_debt`
//! (burns from the buffer's ACTUAL balance, never the debt a live debt auction is selling).

use auction_core::auction::{claim_leftover as core_claim, settle_empty as core_settle_empty};
use auction_engine_core::{
    pda,
    state::{AccountingState, AuctionState, EngineConfigState},
    EngineError,
};
use lee_core::{
    account::{AccountId, AccountWithMetadata},
    program::{ChainedCall, ProgramId},
};

use crate::{accounts::*, token_calls, EngineResult};

pub struct SettleEmptyAccounts {
    pub caller: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub coll_config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    pub auction: AccountWithMetadata,
}

/// Settles a lot-0 auction: books the shortfall, folds the raised amounts and releases the
/// reservation from both caps, exactly once (`settle_empty` fails on anything but an empty
/// `Auctioning` lot, so a second call and any later bid fail `NotAuctioning`).
pub fn settle_empty(program_id: ProgramId, a: SettleEmptyAccounts) -> EngineResult {
    if !a.caller.is_authorized {
        return Err(EngineError::WrongAccount("caller"));
    }
    let mut cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let mut coll = load_coll(&program_id, &config_id, &cfg, &a.coll_config)?;
    let acc = load_accounting(&program_id, &config_id, &a.accounting)?;
    let st = load_auction(&program_id, &config_id, &a.auction)?;
    let (next, end) = core_settle_empty(&st.to_core()?)?;
    coll.type_reserved = coll
        .type_reserved
        .checked_sub(end.release)
        .ok_or(EngineError::Math)?;
    cfg.global_reserved = cfg
        .global_reserved
        .checked_sub(end.release)
        .ok_or(EngineError::Math)?;
    let folded = AccountingState::with_core(&acc, &acc.core().record_settled(&next, &end));
    Ok((
        vec![
            echo(&a.caller),
            write(&a.config, &cfg),
            write(&a.coll_config, &coll),
            write(&a.accounting, &folded),
            write(&a.auction, &AuctionState::from_core(&st, &next)),
        ],
        vec![],
    ))
}

pub struct ClaimLeftoverAccounts {
    pub owner: AccountWithMetadata,
    pub owner_collateral_holding: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub auction: AccountWithMetadata,
    pub auction_vault: AccountWithMetadata,
}

/// Pays `leftover_claimable - leftover_claimed` by sweeping the whole vault (ADR-033). Only the
/// position owner recorded at liquidation may claim (`NotOwner`).
pub fn claim_leftover(program_id: ProgramId, a: ClaimLeftoverAccounts) -> EngineResult {
    let cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let st = load_auction(&program_id, &config_id, &a.auction)?;
    if !a.owner.is_authorized || a.owner.account_id != st.owner {
        return Err(EngineError::NotOwner);
    }
    let (next, amount) = core_claim(&st.to_core()?)?;
    expect_id(
        &a.auction_vault,
        &pda::vault_pda(&program_id, &a.auction.account_id),
        "auction_vault",
    )?;
    if a.owner_collateral_holding.account_id == a.auction_vault.account_id {
        return Err(EngineError::WrongAccount("owner_collateral_holding"));
    }
    let token = cfg.token_program_id;
    let vault_balance = fungible_balance(&a.auction_vault, &token, &cfg.collateral_definition_id)?;
    if vault_balance < amount {
        return Err(EngineError::VaultBelowBook);
    }
    check_recipient(
        &a.owner_collateral_holding,
        &token,
        &cfg.collateral_definition_id,
    )?;
    let pay = token_calls::transfer(
        token,
        token_calls::authorized(&a.auction_vault),
        a.owner_collateral_holding.clone(),
        vault_balance,
        vec![pda::vault_seed(&a.auction.account_id)],
    );
    Ok((
        vec![
            echo(&a.owner),
            echo(&a.owner_collateral_holding),
            echo(&a.config),
            write(&a.auction, &AuctionState::from_core(&st, &next)),
            echo(&a.auction_vault),
        ],
        vec![pay],
    ))
}

/// `settle_debt(MAX)` against the actual buffer balance; returns (new state, amount to burn).
#[must_use]
pub fn implicit_settle(acc: &AccountingState, buffer_balance: u128) -> (AccountingState, u128) {
    let (next, burned) = acc.core().settle_debt(u128::MAX, buffer_balance);
    (AccountingState::with_core(acc, &next), burned)
}

/// `Token::Burn(stablecoin, buffer)` authorised by the buffer's PDA seed; `None` if nothing burns.
#[must_use]
pub fn buffer_burn(
    cfg: &EngineConfigState,
    config_id: &AccountId,
    stablecoin_definition: &AccountWithMetadata,
    buffer: &AccountWithMetadata,
    burned: u128,
) -> Option<ChainedCall> {
    (burned > 0).then(|| {
        token_calls::burn(
            cfg.token_program_id,
            stablecoin_definition.clone(),
            token_calls::authorized(buffer),
            burned,
            vec![pda::buffer_seed(config_id)],
        )
    })
}

pub struct SettleDebtAccounts {
    pub caller: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    pub buffer: AccountWithMetadata,
    pub stablecoin_definition: AccountWithMetadata,
}

/// Permissionless. Burns `min(amount, buffer actual balance, unbacked - debt_on_auction)` from the
/// buffer under its PDA seed (S7 prerequisite: burn from an engine-PDA-held holding).
pub fn settle_debt(program_id: ProgramId, a: SettleDebtAccounts, amount: u128) -> EngineResult {
    if !a.caller.is_authorized {
        return Err(EngineError::WrongAccount("caller"));
    }
    let cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let acc = load_accounting(&program_id, &config_id, &a.accounting)?;
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
    let balance = fungible_balance(
        &a.buffer,
        &cfg.token_program_id,
        &cfg.stablecoin_definition_id,
    )?;
    let (core_next, burned) = acc.core().settle_debt(amount, balance);
    let next = AccountingState::with_core(&acc, &core_next);
    let calls = buffer_burn(
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
            write(&a.accounting, &next),
            echo(&a.buffer),
            echo(&a.stablecoin_definition),
        ],
        calls,
    ))
}
