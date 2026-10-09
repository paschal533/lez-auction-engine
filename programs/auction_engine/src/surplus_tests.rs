//! Handler-level tests of the F5 surplus auction and the protocol-token helpers: every guard is
//! exercised without the guest, so each can be mutation-checked in seconds.

use auction_engine_core::{
    pda::{
        accounting_pda, buffer_pda, buffer_seed, surplus_auction_pda, surplus_vault_pda,
        surplus_vault_seed, unused_slot_pda, SLOT_REF_ORACLE,
    },
    state::*,
    EngineError,
};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::AccountPostState,
};
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::OraclePriceAccount;

use crate::{
    admin_tests::{config_state, fresh, meta, rejected, ENGINE, ONE, STABLE, TOKEN},
    collateral_tests::{cfg_id, clock_at, engine_owned},
    settle::buffer_burn,
    surplus::*,
    token_calls, EngineResult,
};

pub(crate) const NOW: u64 = 1_700_000_000_000;
pub(crate) const PROTOCOL: AccountId = AccountId::new([0x91; 32]);
pub(crate) const REF_ORACLE: AccountId = AccountId::new([0x92; 32]);
pub(crate) const CALLER: AccountId = AccountId::new([0x93; 32]);
pub(crate) const TAKER_PROTOCOL: AccountId = AccountId::new([0x94; 32]);
pub(crate) const TAKER_STABLE: AccountId = AccountId::new([0x95; 32]);
pub(crate) const FOREIGN: AccountId = AccountId::new([0x96; 32]);
pub(crate) const OTHER_PROGRAM: [u32; 8] = [0x77; 8];

pub(crate) const LOT: u128 = 500;
pub(crate) const BUFFER: u128 = 1_500;
pub(crate) const ADMIN_REF: u128 = 2 * ONE;
pub(crate) const TTL: u64 = 14_400_000;

pub(crate) fn protocol_config(oracle: bool) -> EngineConfigState {
    EngineConfigState {
        protocol_token: Some(PROTOCOL),
        protocol_token_ref_price: ADMIN_REF,
        protocol_token_oracle_id: oracle.then_some(REF_ORACLE),
        ..config_state()
    }
}

pub(crate) fn config_account(cfg: &EngineConfigState) -> AccountWithMetadata {
    meta(cfg_id(), engine_owned(cfg.into()), false)
}

pub(crate) fn accounting_account(state: &AccountingState) -> AccountWithMetadata {
    meta(
        accounting_pda(&ENGINE, &cfg_id()),
        engine_owned(state.into()),
        false,
    )
}

pub(crate) fn holding(
    id: AccountId,
    definition: AccountId,
    balance: u128,
    signed: bool,
) -> AccountWithMetadata {
    meta(
        id,
        Account {
            program_owner: TOKEN,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: definition,
                balance,
            }),
            ..Account::default()
        },
        signed,
    )
}

pub(crate) fn stable_definition() -> AccountWithMetadata {
    meta(
        STABLE,
        Account {
            program_owner: TOKEN,
            ..Account::default()
        },
        false,
    )
}

pub(crate) fn protocol_definition() -> AccountWithMetadata {
    meta(
        PROTOCOL,
        Account {
            program_owner: TOKEN,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("MOCK"),
                total_supply: 10_000,
                metadata_id: None,
                authority: None,
            }),
            ..Account::default()
        },
        false,
    )
}

pub(crate) fn buffer_id() -> AccountId {
    buffer_pda(&ENGINE, &cfg_id())
}

pub(crate) fn auction_id(nonce: u64) -> AccountId {
    surplus_auction_pda(&ENGINE, &cfg_id(), nonce)
}

pub(crate) fn vault_id(nonce: u64) -> AccountId {
    surplus_vault_pda(&ENGINE, &auction_id(nonce))
}

pub(crate) fn placeholder(slot: u64) -> AccountWithMetadata {
    fresh(unused_slot_pda(&ENGINE, &cfg_id(), slot))
}

pub(crate) fn start_fixture() -> StartSurplusAccounts {
    StartSurplusAccounts {
        caller: meta(CALLER, Account::default(), true),
        config: config_account(&protocol_config(false)),
        accounting: accounting_account(&AccountingState::new()),
        buffer: holding(buffer_id(), STABLE, BUFFER, false),
        stablecoin_definition: stable_definition(),
        surplus_auction: fresh(auction_id(0)),
        surplus_vault: fresh(vault_id(0)),
        ref_oracle: placeholder(SLOT_REF_ORACLE),
        clock: clock_at(NOW),
    }
}

pub(crate) fn run_start(a: StartSurplusAccounts, nonce: u64) -> EngineResult {
    start_surplus_auction(ENGINE, a, nonce)
}

pub(crate) const S_ACCOUNTING: usize = 2;
pub(crate) const S_AUCTION: usize = 5;

pub(crate) fn accounting_of(p: &AccountPostState) -> AccountingState {
    AccountingState::try_from(&p.account().data).expect("accounting decodes")
}
pub(crate) fn surplus_of(p: &AccountPostState) -> SurplusAuctionState {
    SurplusAuctionState::try_from(&p.account().data).expect("surplus decodes")
}

pub(crate) fn oracle_account(price: u128, timestamp: u64) -> AccountWithMetadata {
    meta(
        REF_ORACLE,
        Account {
            program_owner: [9; 8],
            data: Data::from(&OraclePriceAccount {
                base_asset: PROTOCOL,
                quote_asset: STABLE,
                price,
                timestamp,
                source_id: REF_ORACLE,
                confidence_interval: 0,
            }),
            ..Account::default()
        },
        false,
    )
}

pub(crate) fn surplus_account(state: &SurplusAuctionState) -> AccountWithMetadata {
    meta(auction_id(0), engine_owned(state.into()), false)
}

#[test]
pub(crate) fn start_moves_the_lot_into_its_own_vault_and_snapshots_the_reference() {
    let f = start_fixture();
    let vault = f.surplus_vault.clone();
    let buffer = f.buffer.clone();
    let def = f.stablecoin_definition.clone();
    let (post, calls) = run_start(f, 0).expect("start");
    assert_eq!(post.len(), 9);
    assert_eq!(
        surplus_of(&post[S_AUCTION]),
        SurplusAuctionState {
            tag: TAG_SURPLUS,
            config: cfg_id(),
            nonce: 0,
            lot_left: LOT,
            start_ms: NOW,
            live: true,
            ref_ray: ADMIN_REF,
        }
    );
    let acc = accounting_of(&post[S_ACCOUNTING]);
    assert_eq!(
        (
            acc.surplus_live,
            acc.surplus_nonce,
            acc.total_surplus_lots_out,
            acc.unbacked_debt
        ),
        (true, 1, LOT, 0)
    );
    let expected = vec![
        token_calls::initialize_account(
            TOKEN,
            def,
            token_calls::authorized(&vault),
            vec![surplus_vault_seed(&auction_id(0))],
        ),
        token_calls::transfer(
            TOKEN,
            token_calls::authorized(&buffer),
            token_calls::post_init(&vault, TOKEN, STABLE),
            LOT,
            vec![buffer_seed(&cfg_id())],
        ),
    ];
    assert_eq!(
        calls, expected,
        "init vault, then buffer -> vault under the buffer seed"
    );
}

#[test]
pub(crate) fn start_burns_the_implicit_settle_first_and_measures_the_buffer_after_it() {
    // 400 of debt on a 1_500 buffer: burn 400, then 1_100 < 1_000 + 500 fails the threshold.
    let mut f = start_fixture();
    f.accounting = accounting_account(&AccountingState {
        unbacked_debt: 400,
        ..AccountingState::new()
    });
    assert_eq!(rejected(run_start(f, 0)), EngineError::BufferBelowTarget);

    // With a bigger buffer the burn is chained before the lot moves.
    let mut f = start_fixture();
    f.accounting = accounting_account(&AccountingState {
        unbacked_debt: 400,
        ..AccountingState::new()
    });
    f.buffer = holding(buffer_id(), STABLE, 1_900, false);
    let def = f.stablecoin_definition.clone();
    let buffer = f.buffer.clone();
    let vault = f.surplus_vault.clone();
    let (post, calls) = run_start(f, 0).expect("start");
    let acc = accounting_of(&post[S_ACCOUNTING]);
    assert_eq!(
        (
            acc.unbacked_debt,
            acc.total_debt_settled,
            acc.total_surplus_lots_out
        ),
        (0, 400, LOT)
    );
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls[1],
        buffer_burn(&protocol_config(false), &cfg_id(), &def, &buffer, 400).expect("burn")
    );
    assert_eq!(
        calls[2],
        token_calls::transfer(
            TOKEN,
            token_calls::authorized(&token_calls::with_balance(&buffer, STABLE, 1_500)),
            token_calls::post_init(&vault, TOKEN, STABLE),
            LOT,
            vec![buffer_seed(&cfg_id())],
        )
    );
}

#[test]
pub(crate) fn start_is_blocked_by_unbacked_debt_the_buffer_cannot_cover() {
    let mut f = start_fixture();
    f.accounting = accounting_account(&AccountingState {
        unbacked_debt: 5_000,
        ..AccountingState::new()
    });
    assert_eq!(rejected(run_start(f, 0)), EngineError::UnbackedDebtPresent);
    // Debt a live debt auction reserves is not settled from the buffer, so it still blocks.
    let mut f = start_fixture();
    f.accounting = accounting_account(&AccountingState {
        unbacked_debt: 200,
        debt_on_auction: 200,
        ..AccountingState::new()
    });
    assert_eq!(rejected(run_start(f, 0)), EngineError::UnbackedDebtPresent);
}

#[test]
pub(crate) fn start_preconditions() {
    let mut f = start_fixture();
    f.buffer = holding(buffer_id(), STABLE, 1_499, false);
    assert_eq!(rejected(run_start(f, 0)), EngineError::BufferBelowTarget);

    let mut f = start_fixture();
    f.config = config_account(&config_state());
    assert_eq!(rejected(run_start(f, 0)), EngineError::TokenNotConfigured);

    assert_eq!(
        rejected(run_start(start_fixture(), 1)),
        EngineError::NonceMismatch
    );

    let mut f = start_fixture();
    f.accounting = accounting_account(&AccountingState {
        surplus_live: true,
        surplus_nonce: 0,
        ..AccountingState::new()
    });
    assert_eq!(
        rejected(run_start(f, 0)),
        EngineError::SurplusAuctionAlreadyLive
    );

    let mut cfg = protocol_config(false);
    cfg.system.surplus_delay_ms = 1_000;
    let mut f = start_fixture();
    f.config = config_account(&cfg);
    f.accounting = accounting_account(&AccountingState {
        last_surplus_end_ms: NOW - 999,
        ..AccountingState::new()
    });
    assert_eq!(rejected(run_start(f, 0)), EngineError::DelayNotElapsed);
    let mut ok = start_fixture();
    ok.config = config_account(&cfg);
    ok.accounting = accounting_account(&AccountingState {
        last_surplus_end_ms: NOW - 1_000,
        ..AccountingState::new()
    });
    run_start(ok, 0).expect("the delay has elapsed exactly");
}

#[test]
pub(crate) fn start_needs_a_signing_caller_and_validates_every_account() {
    let mut f = start_fixture();
    f.caller.is_authorized = false;
    assert_eq!(
        rejected(run_start(f, 0)),
        EngineError::WrongAccount("caller")
    );

    let mut f = start_fixture();
    f.buffer.account_id = FOREIGN;
    assert_eq!(
        rejected(run_start(f, 0)),
        EngineError::WrongAccount("buffer")
    );

    let mut f = start_fixture();
    f.buffer = holding(buffer_id(), PROTOCOL, BUFFER, false);
    assert_eq!(rejected(run_start(f, 0)), EngineError::WrongDefinition);

    let mut f = start_fixture();
    f.stablecoin_definition.account_id = FOREIGN;
    assert_eq!(
        rejected(run_start(f, 0)),
        EngineError::WrongAccount("stablecoin_definition")
    );

    let mut f = start_fixture();
    f.surplus_auction.account_id = auction_id(7);
    assert_eq!(
        rejected(run_start(f, 0)),
        EngineError::WrongAccount("surplus_auction")
    );

    let mut f = start_fixture();
    f.surplus_auction = surplus_account(&SurplusAuctionState {
        tag: TAG_SURPLUS,
        config: cfg_id(),
        nonce: 0,
        lot_left: 1,
        start_ms: 1,
        live: false,
        ref_ray: ONE,
    });
    assert_eq!(
        rejected(run_start(f, 0)),
        EngineError::WrongAccount("surplus_auction")
    );

    let mut f = start_fixture();
    f.surplus_vault.account_id = vault_id(1);
    assert_eq!(
        rejected(run_start(f, 0)),
        EngineError::WrongAccount("surplus_vault")
    );

    let mut f = start_fixture();
    f.surplus_vault = holding(vault_id(0), STABLE, 0, false);
    assert_eq!(
        rejected(run_start(f, 0)),
        EngineError::WrongAccount("surplus_vault")
    );

    let mut f = start_fixture();
    f.accounting.account_id = FOREIGN;
    assert_eq!(
        rejected(run_start(f, 0)),
        EngineError::WrongAccount("accounting")
    );

    let mut f = start_fixture();
    f.config.account_id = FOREIGN;
    assert_eq!(
        rejected(run_start(f, 0)),
        EngineError::WrongAccount("config")
    );

    let mut f = start_fixture();
    f.clock.account_id = FOREIGN;
    assert_eq!(rejected(run_start(f, 0)), EngineError::WrongClock);

    // The reference-oracle slot is pinned: a real oracle where the placeholder belongs is refused.
    let mut f = start_fixture();
    f.ref_oracle = oracle_account(2 * ONE, NOW);
    assert_eq!(
        rejected(run_start(f, 0)),
        EngineError::WrongAccount("unused slot")
    );
}

#[test]
pub(crate) fn start_snapshots_the_clamped_oracle_price() {
    let cfg = protocol_config(true);
    let run = |price: u128, ts: u64| {
        let mut f = start_fixture();
        f.config = config_account(&cfg);
        f.ref_oracle = oracle_account(price, ts);
        let (post, _) = run_start(f, 0).expect("start");
        surplus_of(&post[S_AUCTION]).ref_ray
    };
    assert_eq!(
        run(3 * ONE, NOW),
        24 * ONE / 10,
        "valid oracle above +20% is clamped"
    );
    assert_eq!(
        run(ONE, NOW),
        16 * ONE / 10,
        "valid oracle below -20% is clamped"
    );
    assert_eq!(
        run(19 * ONE / 10, NOW),
        19 * ONE / 10,
        "inside the clamp it is used as is"
    );
    assert_eq!(
        run(3 * ONE, NOW - cfg.ref_oracle_max_age_ms - 1),
        ADMIN_REF,
        "a stale oracle falls back to the admin price"
    );
}

// ---- take_surplus ----

pub(crate) fn live_auction(lot_left: u128) -> SurplusAuctionState {
    SurplusAuctionState {
        tag: TAG_SURPLUS,
        config: cfg_id(),
        nonce: 0,
        lot_left,
        start_ms: NOW,
        live: true,
        ref_ray: ADMIN_REF,
    }
}

pub(crate) fn live_accounting() -> AccountingState {
    AccountingState {
        surplus_live: true,
        surplus_nonce: 1,
        total_surplus_lots_out: LOT,
        total_protocol_burned: 7,
        ..AccountingState::new()
    }
}

// ---- close_surplus_auction ----

pub(crate) fn close_fixture(vault_balance: u128, lot_left: u128) -> CloseSurplusAccounts {
    CloseSurplusAccounts {
        caller: meta(CALLER, Account::default(), true),
        config: config_account(&protocol_config(false)),
        accounting: accounting_account(&live_accounting()),
        surplus_auction: surplus_account(&live_auction(lot_left)),
        surplus_vault: holding(vault_id(0), STABLE, vault_balance, false),
        buffer: holding(buffer_id(), STABLE, 1_000, false),
        clock: clock_at(NOW + TTL),
    }
}

pub(crate) fn close_err(a: CloseSurplusAccounts) -> EngineError {
    close_surplus_auction(ENGINE, a).expect_err("close must be rejected")
}

pub(crate) const C_ACCOUNTING: usize = 2;
pub(crate) const C_AUCTION: usize = 3;

#[test]
pub(crate) fn close_returns_the_whole_vault_to_the_buffer_after_the_ttl() {
    let f = close_fixture(250, 250);
    let (vault, buffer) = (f.surplus_vault.clone(), f.buffer.clone());
    let (post, calls) = close_surplus_auction(ENGINE, f).expect("close");
    assert_eq!(post.len(), 7);
    assert_eq!(
        calls,
        vec![token_calls::transfer(
            TOKEN,
            token_calls::authorized(&vault),
            buffer,
            250,
            vec![surplus_vault_seed(&auction_id(0))],
        )]
    );
    let sa = surplus_of(&post[C_AUCTION]);
    assert_eq!((sa.lot_left, sa.live), (0, false));
    let acc = accounting_of(&post[C_ACCOUNTING]);
    assert_eq!(
        (
            acc.surplus_live,
            acc.total_surplus_returned,
            acc.last_surplus_end_ms
        ),
        (false, 250, NOW + TTL)
    );
}

#[test]
pub(crate) fn close_sweeps_donations_too_and_chains_nothing_for_an_empty_vault() {
    let (post, calls) = close_surplus_auction(ENGINE, close_fixture(1_150, 250)).expect("close");
    assert_eq!(calls.len(), 1);
    assert_eq!(
        accounting_of(&post[C_ACCOUNTING]).total_surplus_returned,
        1_150
    );
    let (_, calls) = close_surplus_auction(ENGINE, close_fixture(0, 0)).expect("empty");
    assert!(calls.is_empty());
}

#[test]
pub(crate) fn close_waits_for_the_ttl_and_needs_a_live_auction() {
    let mut f = close_fixture(250, 250);
    f.clock = clock_at(NOW + TTL - 1);
    assert_eq!(close_err(f), EngineError::DelayNotElapsed);
    let mut f = close_fixture(250, 250);
    f.surplus_auction = surplus_account(&SurplusAuctionState {
        live: false,
        ..live_auction(0)
    });
    assert_eq!(close_err(f), EngineError::NotLive);
}

#[test]
pub(crate) fn close_pins_the_accounts() {
    let mut f = close_fixture(250, 250);
    f.caller.is_authorized = false;
    assert_eq!(close_err(f), EngineError::WrongAccount("caller"));
    let mut f = close_fixture(250, 250);
    f.surplus_vault.account_id = vault_id(1);
    assert_eq!(close_err(f), EngineError::WrongAccount("surplus_vault"));
    let mut f = close_fixture(250, 250);
    f.buffer.account_id = FOREIGN;
    assert_eq!(close_err(f), EngineError::WrongAccount("buffer"));
    let mut f = close_fixture(250, 250);
    f.surplus_vault = holding(vault_id(0), PROTOCOL, 250, false);
    assert_eq!(close_err(f), EngineError::WrongDefinition);
    let mut f = close_fixture(250, 250);
    f.accounting.account_id = FOREIGN;
    assert_eq!(close_err(f), EngineError::WrongAccount("accounting"));
    let mut f = close_fixture(250, 250);
    f.surplus_auction.account_id = auction_id(2);
    assert_eq!(close_err(f), EngineError::WrongAccount("surplus_auction"));
}
