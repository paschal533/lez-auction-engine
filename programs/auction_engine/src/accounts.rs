//! Account loading and validation shared by every handler. Every engine account is checked for
//! owner, tag and PDA; every holding for token program and definition (spec §10 checklist).

use auction_engine_core::{pda, state::*, EngineError};
use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, Claim, PdaSeed, ProgramId, DEFAULT_PROGRAM_ID},
};
use token_core::TokenHolding;

pub const ZERO_ID: AccountId = AccountId::new([0; 32]);

pub fn nonzero_id(id: &AccountId) -> Result<(), EngineError> {
    if *id == ZERO_ID {
        Err(EngineError::ZeroId)
    } else {
        Ok(())
    }
}

pub fn nonzero_program(p: &ProgramId) -> Result<(), EngineError> {
    if *p == DEFAULT_PROGRAM_ID {
        Err(EngineError::ZeroId)
    } else {
        Ok(())
    }
}

/// Time comes only from `CLOCK_01` (spec §3.2).
pub fn read_clock(clock: &AccountWithMetadata) -> Result<u64, EngineError> {
    if clock.account_id != CLOCK_01_PROGRAM_ACCOUNT_ID || clock.account == Account::default() {
        return Err(EngineError::WrongClock);
    }
    borsh::from_slice::<ClockAccountData>(clock.account.data.as_ref())
        .map(|c| c.timestamp)
        .map_err(|_| EngineError::WrongClock)
}

pub fn require_admin(
    admin: &AccountWithMetadata,
    cfg: &EngineConfigState,
) -> Result<(), EngineError> {
    if admin.is_authorized && admin.account_id == cfg.admin {
        Ok(())
    } else {
        Err(EngineError::NotAdmin)
    }
}

pub fn expect_id(
    acc: &AccountWithMetadata,
    expected: &AccountId,
    what: &'static str,
) -> Result<(), EngineError> {
    if acc.account_id == *expected {
        Ok(())
    } else {
        Err(EngineError::WrongAccount(what))
    }
}

pub fn expect_default(acc: &AccountWithMetadata, what: &'static str) -> Result<(), EngineError> {
    if acc.account == Account::default() {
        Ok(())
    } else {
        Err(EngineError::WrongAccount(what))
    }
}

fn owned<'a>(
    acc: &'a AccountWithMetadata,
    program_id: &ProgramId,
    what: &'static str,
) -> Result<&'a Data, EngineError> {
    if acc.account.program_owner == *program_id {
        Ok(&acc.account.data)
    } else {
        Err(EngineError::WrongAccount(what))
    }
}

pub fn load_config(
    program_id: &ProgramId,
    acc: &AccountWithMetadata,
) -> Result<EngineConfigState, EngineError> {
    let cfg = EngineConfigState::try_from(owned(acc, program_id, "config")?)
        .map_err(|_| EngineError::WrongAccount("config"))?;
    expect_id(
        acc,
        &pda::config_pda(program_id, &cfg.admin, cfg.salt),
        "config",
    )?;
    Ok(cfg)
}

pub fn load_coll(
    program_id: &ProgramId,
    config_id: &AccountId,
    cfg: &EngineConfigState,
    acc: &AccountWithMetadata,
) -> Result<CollateralConfigState, EngineError> {
    let coll = CollateralConfigState::try_from(owned(acc, program_id, "coll_config")?)
        .map_err(|_| EngineError::WrongAccount("coll_config"))?;
    expect_id(
        acc,
        &pda::coll_config_pda(program_id, config_id, &cfg.collateral_definition_id),
        "coll_config",
    )?;
    if coll.config != *config_id {
        return Err(EngineError::WrongAccount("coll_config"));
    }
    Ok(coll)
}

pub fn load_accounting(
    program_id: &ProgramId,
    config_id: &AccountId,
    acc: &AccountWithMetadata,
) -> Result<AccountingState, EngineError> {
    let a = AccountingState::try_from(owned(acc, program_id, "accounting")?)
        .map_err(|_| EngineError::WrongAccount("accounting"))?;
    expect_id(
        acc,
        &pda::accounting_pda(program_id, config_id),
        "accounting",
    )?;
    Ok(a)
}

pub fn load_auction(
    program_id: &ProgramId,
    config_id: &AccountId,
    acc: &AccountWithMetadata,
) -> Result<AuctionState, EngineError> {
    let a = AuctionState::try_from(owned(acc, program_id, "auction")?)
        .map_err(|_| EngineError::WrongAccount("auction"))?;
    if a.config != *config_id {
        return Err(EngineError::WrongAccount("auction"));
    }
    expect_id(
        acc,
        &pda::auction_pda(program_id, config_id, &a.position, a.nonce),
        "auction",
    )?;
    Ok(a)
}

/// Balance of an initialised fungible holding of `definition` owned by `token_program`.
pub fn fungible_balance(
    acc: &AccountWithMetadata,
    token_program: &ProgramId,
    definition: &AccountId,
) -> Result<u128, EngineError> {
    if acc.account == Account::default() || acc.account.program_owner != *token_program {
        return Err(EngineError::WrongDefinition);
    }
    match TokenHolding::try_from(&acc.account.data) {
        Ok(TokenHolding::Fungible {
            definition_id,
            balance,
        }) if definition_id == *definition => Ok(balance),
        _ => Err(EngineError::WrongDefinition),
    }
}

/// A holding that will receive tokens: initialised with `definition`, or fresh and signed by its
/// holder (the token program claims a fresh recipient with `Claim::Authorized`). Review Focus 2.
pub fn check_recipient(
    acc: &AccountWithMetadata,
    token_program: &ProgramId,
    definition: &AccountId,
) -> Result<(), EngineError> {
    if acc.account == Account::default() {
        return if acc.is_authorized {
            Ok(())
        } else {
            Err(EngineError::RecipientMustSign)
        };
    }
    fungible_balance(acc, token_program, definition).map(|_| ())
}

/// Enabled → the slot holds the configured account (non-zero, not the placeholder); disabled → the
/// slot must be the placeholder `["UNUSED", config, slot]` (ADR-030).
pub fn optional_slot(
    program_id: &ProgramId,
    config_id: &AccountId,
    slot: u64,
    enabled: bool,
    acc: &AccountWithMetadata,
) -> Result<Option<AccountId>, EngineError> {
    let placeholder = pda::unused_slot_pda(program_id, config_id, slot);
    if enabled {
        nonzero_id(&acc.account_id)?;
        if acc.account_id == placeholder {
            return Err(EngineError::WrongAccount("slot"));
        }
        Ok(Some(acc.account_id))
    } else {
        expect_id(acc, &placeholder, "unused slot")?;
        Ok(None)
    }
}

/// The protocol token must not be the stablecoin, the collateral or the reference oracle, and the
/// reference oracle must not be the stablecoin or the collateral (M7).
pub fn check_distinct_slots(
    protocol_token: Option<AccountId>,
    ref_oracle: Option<AccountId>,
    stablecoin: &AccountId,
    collateral: &AccountId,
) -> Result<(), EngineError> {
    if let Some(t) = protocol_token {
        if t == *stablecoin || t == *collateral || Some(t) == ref_oracle {
            return Err(EngineError::WrongAccount("protocol_token"));
        }
    }
    if let Some(o) = ref_oracle {
        if o == *stablecoin || o == *collateral {
            return Err(EngineError::WrongAccount("ref_oracle"));
        }
    }
    Ok(())
}

/// At use time: the slot account must be exactly the configured one, or the placeholder.
pub fn expect_slot(
    program_id: &ProgramId,
    config_id: &AccountId,
    slot: u64,
    configured: Option<AccountId>,
    acc: &AccountWithMetadata,
) -> Result<(), EngineError> {
    match configured {
        Some(id) => expect_id(acc, &id, "slot"),
        None => expect_id(
            acc,
            &pda::unused_slot_pda(program_id, config_id, slot),
            "unused slot",
        ),
    }
}

#[must_use]
pub fn echo(acc: &AccountWithMetadata) -> AccountPostState {
    AccountPostState::new(acc.account.clone())
}

pub fn write<T>(acc: &AccountWithMetadata, state: &T) -> AccountPostState
where
    Data: for<'a> From<&'a T>,
{
    let mut a = acc.account.clone();
    a.data = Data::from(state);
    AccountPostState::new(a)
}

pub fn create<T>(acc: &AccountWithMetadata, state: &T, seed: PdaSeed) -> AccountPostState
where
    Data: for<'a> From<&'a T>,
{
    let mut a = acc.account.clone();
    a.data = Data::from(state);
    AccountPostState::new_claimed(a, Claim::Pda(seed))
}

pub fn write_or_create<T>(acc: &AccountWithMetadata, state: &T, seed: PdaSeed) -> AccountPostState
where
    Data: for<'a> From<&'a T>,
{
    let mut a = acc.account.clone();
    a.data = Data::from(state);
    AccountPostState::new_claimed_if_default(a, Claim::Pda(seed))
}
