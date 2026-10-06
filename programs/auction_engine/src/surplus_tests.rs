//! Handler-level tests of the F5 surplus auction and the protocol-token helpers: every guard is
//! exercised without the guest, so each can be mutation-checked in seconds.

use auction_engine_core::{
    pda::{
        accounting_pda, buffer_pda, buffer_seed, mint_authority_pda, surplus_auction_pda,
        surplus_vault_pda, surplus_vault_seed, unused_slot_pda, SLOT_PROTOCOL_TOKEN,
        SLOT_REF_ORACLE,
    },
    state::*,
    EngineError,
};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall},
};
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::OraclePriceAccount;

use crate::{
    admin_tests::{config_state, fresh, meta, rejected, ENGINE, ONE, STABLE, TOKEN},
    collateral_tests::{cfg_id, clock_at, engine_owned},
    settle::buffer_burn,
    surplus::*,
    system::{protocol_token_status, ref_price, TokenStatus},
    token_calls, EngineResult,
};

const NOW: u64 = 1_700_000_000_000;
const PROTOCOL: AccountId = AccountId::new([0x91; 32]);
const REF_ORACLE: AccountId = AccountId::new([0x92; 32]);
const CALLER: AccountId = AccountId::new([0x93; 32]);
const TAKER_PROTOCOL: AccountId = AccountId::new([0x94; 32]);
const TAKER_STABLE: AccountId = AccountId::new([0x95; 32]);
const FOREIGN: AccountId = AccountId::new([0x96; 32]);
const OTHER_PROGRAM: [u32; 8] = [0x77; 8];

const LOT: u128 = 500;
const BUFFER: u128 = 1_500;
const ADMIN_REF: u128 = 2 * ONE;
const TTL: u64 = 14_400_000;

fn protocol_config(oracle: bool) -> EngineConfigState {
    EngineConfigState {
        protocol_token: Some(PROTOCOL),
        protocol_token_ref_price: ADMIN_REF,
        protocol_token_oracle_id: oracle.then_some(REF_ORACLE),
        ..config_state()
    }
}

fn config_account(cfg: &EngineConfigState) -> AccountWithMetadata {
    meta(cfg_id(), engine_owned(cfg.into()), false)
}

fn accounting_account(state: &AccountingState) -> AccountWithMetadata {
    meta(
        accounting_pda(&ENGINE, &cfg_id()),
        engine_owned(state.into()),
        false,
    )
}

fn holding(
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

fn stable_definition() -> AccountWithMetadata {
    meta(
        STABLE,
        Account {
            program_owner: TOKEN,
            ..Account::default()
        },
        false,
    )
}

fn protocol_definition() -> AccountWithMetadata {
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

fn buffer_id() -> AccountId {
    buffer_pda(&ENGINE, &cfg_id())
}

fn auction_id(nonce: u64) -> AccountId {
    surplus_auction_pda(&ENGINE, &cfg_id(), nonce)
}

fn vault_id(nonce: u64) -> AccountId {
    surplus_vault_pda(&ENGINE, &auction_id(nonce))
}

fn placeholder(slot: u64) -> AccountWithMetadata {
    fresh(unused_slot_pda(&ENGINE, &cfg_id(), slot))
}

fn start_fixture() -> StartSurplusAccounts {
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

fn run_start(a: StartSurplusAccounts, nonce: u64) -> EngineResult {
    start_surplus_auction(ENGINE, a, nonce)
}

const S_ACCOUNTING: usize = 2;
const S_AUCTION: usize = 5;

fn accounting_of(p: &AccountPostState) -> AccountingState {
    AccountingState::try_from(&p.account().data).expect("accounting decodes")
}
fn surplus_of(p: &AccountPostState) -> SurplusAuctionState {
    SurplusAuctionState::try_from(&p.account().data).expect("surplus decodes")
}

fn oracle_account(price: u128, timestamp: u64) -> AccountWithMetadata {
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

fn surplus_account(state: &SurplusAuctionState) -> AccountWithMetadata {
    meta(auction_id(0), engine_owned(state.into()), false)
}

#[test]
fn start_moves_the_lot_into_its_own_vault_and_snapshots_the_reference() {
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
fn start_burns_the_implicit_settle_first_and_measures_the_buffer_after_it() {
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
fn start_is_blocked_by_unbacked_debt_the_buffer_cannot_cover() {
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
fn start_preconditions() {
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
fn start_needs_a_signing_caller_and_validates_every_account() {
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
fn start_snapshots_the_clamped_oracle_price() {
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

// ---- ref_price / protocol_token_status ----

fn ref_with(
    cfg: &EngineConfigState,
    slot: &AccountWithMetadata,
    now: u64,
) -> Result<u128, EngineError> {
    ref_price(&ENGINE, &cfg_id(), cfg, slot, now)
}

#[test]
fn ref_price_is_the_admin_price_without_an_oracle_and_pins_the_placeholder() {
    let cfg = protocol_config(false);
    assert_eq!(
        ref_with(&cfg, &placeholder(SLOT_REF_ORACLE), NOW),
        Ok(ADMIN_REF)
    );
    assert_eq!(
        ref_with(&cfg, &fresh(FOREIGN), NOW),
        Err(EngineError::WrongAccount("unused slot"))
    );
    // The placeholder of the other slot is not accepted either.
    assert_eq!(
        ref_with(&cfg, &placeholder(SLOT_PROTOCOL_TOKEN), NOW),
        Err(EngineError::WrongAccount("unused slot"))
    );
}

#[test]
fn ref_price_pins_the_configured_oracle_and_rejects_invalid_readings() {
    let cfg = protocol_config(true);
    let mut wrong = oracle_account(2 * ONE, NOW);
    wrong.account_id = FOREIGN;
    assert_eq!(
        ref_with(&cfg, &wrong, NOW),
        Err(EngineError::WrongAccount("slot"))
    );
    // Undecodable data and a price outside [admin/2, 2 admin] both fall back to the admin price.
    let garbage = meta(REF_ORACLE, Account::default(), false);
    assert_eq!(ref_with(&cfg, &garbage, NOW), Ok(ADMIN_REF));
    assert_eq!(
        ref_with(&cfg, &oracle_account(5 * ONE, NOW), NOW),
        Ok(ADMIN_REF)
    );
    assert_eq!(
        ref_with(&cfg, &oracle_account(ONE / 2, NOW), NOW),
        Ok(ADMIN_REF)
    );
    assert_eq!(
        ref_with(&cfg, &oracle_account(24 * ONE / 10, NOW), NOW),
        Ok(24 * ONE / 10)
    );
    assert_eq!(
        ref_with(
            &cfg,
            &oracle_account(3 * ONE, NOW - cfg.ref_oracle_max_age_ms),
            NOW
        ),
        Ok(24 * ONE / 10),
        "age exactly at the maximum is still valid"
    );
}

#[test]
fn protocol_token_status_reports_id_owner_type_and_mint_authority() {
    let status = |cfg: &EngineConfigState, slot: &AccountWithMetadata| {
        protocol_token_status(&ENGINE, &cfg_id(), cfg, slot)
    };
    let cfg = protocol_config(false);
    let mint_authority = mint_authority_pda(&ENGINE, &cfg_id());
    let def = |authority: Option<AccountId>, owner: [u32; 8]| {
        let mut d = protocol_definition();
        d.account.program_owner = owner;
        d.account.data = Data::from(&TokenDefinition::Fungible {
            name: String::from("MOCK"),
            total_supply: 1,
            metadata_id: None,
            authority,
        });
        d
    };
    let status_of = |configured, token_ok, mint_authority_ok| {
        Ok(TokenStatus {
            configured,
            token_ok,
            mint_authority_ok,
        })
    };
    // Not configured: the placeholder is required and nothing else is reported.
    assert_eq!(
        status(&config_state(), &placeholder(SLOT_PROTOCOL_TOKEN)),
        status_of(false, false, false)
    );
    assert_eq!(
        status(&config_state(), &protocol_definition()),
        Err(EngineError::WrongAccount("unused slot"))
    );
    // Configured and correct.
    assert_eq!(
        status(&cfg, &def(Some(mint_authority), TOKEN)),
        status_of(true, true, true)
    );
    // Wrong or missing mint authority.
    assert_eq!(
        status(&cfg, &def(Some(FOREIGN), TOKEN)),
        status_of(true, true, false)
    );
    assert_eq!(
        status(&cfg, &def(None, TOKEN)),
        status_of(true, true, false)
    );
    // Foreign owner, non-fungible, and a swapped account id.
    assert_eq!(
        status(&cfg, &def(Some(mint_authority), OTHER_PROGRAM)),
        status_of(true, false, false)
    );
    let mut nft = protocol_definition();
    nft.account.data = Data::from(&TokenDefinition::NonFungible {
        name: String::from("N"),
        printable_supply: 1,
        metadata_id: FOREIGN,
    });
    assert_eq!(status(&cfg, &nft), status_of(true, false, false));
    let mut swapped = def(Some(mint_authority), TOKEN);
    swapped.account_id = FOREIGN;
    assert_eq!(
        status(&cfg, &swapped),
        Err(EngineError::WrongAccount("slot"))
    );
}

// ---- take_surplus ----

fn live_auction(lot_left: u128) -> SurplusAuctionState {
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

fn live_accounting() -> AccountingState {
    AccountingState {
        surplus_live: true,
        surplus_nonce: 1,
        total_surplus_lots_out: LOT,
        total_protocol_burned: 7,
        ..AccountingState::new()
    }
}

fn take_fixture() -> TakeSurplusAccounts {
    TakeSurplusAccounts {
        taker_protocol_holding: holding(TAKER_PROTOCOL, PROTOCOL, 10_000, true),
        taker_stable_holding: holding(TAKER_STABLE, STABLE, 0, false),
        config: config_account(&protocol_config(false)),
        accounting: accounting_account(&live_accounting()),
        surplus_auction: surplus_account(&live_auction(LOT)),
        surplus_vault: holding(vault_id(0), STABLE, LOT, false),
        protocol_token_definition: protocol_definition(),
        clock: clock_at(NOW),
    }
}

fn run_take(a: TakeSurplusAccounts, coin: u128, max_in: u128) -> EngineResult {
    take_surplus(ENGINE, a, coin, max_in)
}

fn take_err(a: TakeSurplusAccounts) -> EngineError {
    run_take(a, LOT, u128::MAX).expect_err("take must be rejected")
}

const T_ACCOUNTING: usize = 3;
const T_AUCTION: usize = 4;

fn burn_and_pay(tokens: u128, filled: u128) -> Vec<ChainedCall> {
    let f = take_fixture();
    vec![
        token_calls::burn(
            TOKEN,
            f.protocol_token_definition,
            f.taker_protocol_holding,
            tokens,
            vec![],
        ),
        token_calls::transfer(
            TOKEN,
            token_calls::authorized(&f.surplus_vault),
            f.taker_stable_holding,
            filled,
            vec![surplus_vault_seed(&auction_id(0))],
        ),
    ]
}

#[test]
fn full_take_burns_the_ceil_price_from_the_taker_and_pays_the_lot() {
    // 500 coin: ceil(500 x 10500 / 10000) = 525 coin-equivalents, ceil(525 / 2) = 263 tokens.
    let (post, calls) = run_take(take_fixture(), LOT, 263).expect("take");
    assert_eq!(post.len(), 8);
    assert_eq!(calls, burn_and_pay(263, LOT));
    let acc = accounting_of(&post[T_ACCOUNTING]);
    assert_eq!(
        (
            acc.surplus_live,
            acc.total_protocol_burned,
            acc.last_surplus_end_ms
        ),
        (false, 7 + 263, NOW)
    );
    let sa = surplus_of(&post[T_AUCTION]);
    assert_eq!((sa.lot_left, sa.live, sa.ref_ray), (0, false, ADMIN_REF));
}

#[test]
fn the_burn_is_authorised_by_the_taker_not_by_a_pda() {
    let (_, calls) = run_take(take_fixture(), LOT, u128::MAX).expect("take");
    assert!(calls[0].pda_seeds.is_empty(), "no engine seed on the burn");
    assert!(
        calls[0].pre_states[1].is_authorized,
        "the taker's signed holding"
    );
    assert_eq!(calls[0].pre_states[1].account_id, TAKER_PROTOCOL);
    assert_eq!(calls[0].pre_states[0].account_id, PROTOCOL);
}

#[test]
fn max_amount_fills_at_most_the_lot_and_moves_only_the_filled_amount() {
    let (post, calls) = run_take(take_fixture(), u128::MAX, u128::MAX).expect("max take");
    assert_eq!(calls, burn_and_pay(263, LOT));
    assert!(!surplus_of(&post[T_AUCTION]).live);
    // The vault holds a donation on top; only the filled amount moves (ADR-033).
    let mut f = take_fixture();
    f.surplus_vault = holding(vault_id(0), STABLE, LOT + 900, false);
    let (_, calls) = run_take(f, u128::MAX, u128::MAX).expect("donated vault");
    let mut expected = burn_and_pay(263, LOT);
    expected[1] = token_calls::transfer(
        TOKEN,
        token_calls::authorized(&holding(vault_id(0), STABLE, LOT + 900, false)),
        holding(TAKER_STABLE, STABLE, 0, false),
        LOT,
        vec![surplus_vault_seed(&auction_id(0))],
    );
    assert_eq!(calls, expected);
}

#[test]
fn partial_take_keeps_the_auction_live_and_prices_the_filled_part_only() {
    // 250 coin: ceil(250 x 1.05) = 263, ceil(263 / 2) = 132.
    let (post, calls) = run_take(take_fixture(), 250, 132).expect("partial");
    assert_eq!(calls, burn_and_pay(132, 250));
    let sa = surplus_of(&post[T_AUCTION]);
    assert_eq!((sa.lot_left, sa.live), (250, true));
    let acc = accounting_of(&post[T_ACCOUNTING]);
    assert_eq!(
        (
            acc.surplus_live,
            acc.total_protocol_burned,
            acc.last_surplus_end_ms
        ),
        (true, 7 + 132, 0)
    );
}

#[test]
fn take_price_follows_the_snapshot_and_the_decay_not_the_config_price() {
    // The config price changes after start (4.0): the take still uses the snapshotted 2.0.
    let mut cfg = protocol_config(false);
    cfg.protocol_token_ref_price = 4 * ONE;
    let mut f = take_fixture();
    f.config = config_account(&cfg);
    assert_eq!(
        run_take(f, LOT, 262).expect_err("slippage"),
        EngineError::Slippage
    );
    let mut f = take_fixture();
    f.config = config_account(&cfg);
    run_take(f, LOT, 263).expect("snapshot price");
    // Decay: after 1_000 ms the multiplier is 10500 - 2500 = 8000 -> 400 coin-eq -> 200 tokens.
    let mut f = take_fixture();
    f.clock = clock_at(NOW + 1_000);
    run_take(f, LOT, 200).expect("decayed");
    let mut f = take_fixture();
    f.clock = clock_at(NOW + 1_000);
    assert_eq!(
        run_take(f, LOT, 199).expect_err("slippage"),
        EngineError::Slippage
    );
}

#[test]
fn take_core_rejections() {
    assert_eq!(
        run_take(take_fixture(), 495, u128::MAX).expect_err("dust"),
        EngineError::DustRemainder
    );
    assert_eq!(
        run_take(take_fixture(), 250, 131).expect_err("slippage"),
        EngineError::Slippage
    );
    assert_eq!(
        run_take(take_fixture(), 0, u128::MAX).expect_err("zero"),
        EngineError::ZeroFill
    );
    let mut f = take_fixture();
    f.surplus_auction = surplus_account(&SurplusAuctionState {
        live: false,
        ..live_auction(0)
    });
    assert_eq!(take_err(f), EngineError::NotLive);
}

#[test]
fn take_needs_the_taker_signature_and_enough_protocol_tokens() {
    let mut f = take_fixture();
    f.taker_protocol_holding.is_authorized = false;
    assert_eq!(
        take_err(f),
        EngineError::WrongAccount("taker_protocol_holding")
    );
    let mut f = take_fixture();
    f.taker_protocol_holding = holding(TAKER_PROTOCOL, PROTOCOL, 262, true);
    assert_eq!(take_err(f), EngineError::InsufficientBalance);
    let mut f = take_fixture();
    f.taker_protocol_holding = holding(TAKER_PROTOCOL, PROTOCOL, 263, true);
    run_take(f, LOT, u128::MAX).expect("exactly enough");
    let mut f = take_fixture();
    f.taker_protocol_holding = holding(TAKER_PROTOCOL, STABLE, 10_000, true);
    assert_eq!(take_err(f), EngineError::WrongDefinition);
}

#[test]
fn take_checks_the_protocol_token_definition_id_owner_and_type() {
    // Carry-forward 1 (Task 8 review).
    let mut f = take_fixture();
    f.protocol_token_definition.account_id = FOREIGN;
    assert_eq!(take_err(f), EngineError::WrongAccount("slot"));
    let mut f = take_fixture();
    f.protocol_token_definition.account.program_owner = OTHER_PROGRAM;
    assert_eq!(take_err(f), EngineError::ProtocolTokenMismatch);
    let mut f = take_fixture();
    f.protocol_token_definition.account.data = Data::from(&TokenDefinition::NonFungible {
        name: String::from("N"),
        printable_supply: 1,
        metadata_id: FOREIGN,
    });
    assert_eq!(take_err(f), EngineError::ProtocolTokenMismatch);
    let mut f = take_fixture();
    f.config = config_account(&config_state());
    f.protocol_token_definition = placeholder(SLOT_PROTOCOL_TOKEN);
    assert_eq!(take_err(f), EngineError::TokenNotConfigured);
}

#[test]
fn take_pins_vault_recipient_and_book() {
    let mut f = take_fixture();
    f.surplus_vault.account_id = vault_id(1);
    assert_eq!(take_err(f), EngineError::WrongAccount("surplus_vault"));
    let mut f = take_fixture();
    f.surplus_vault = holding(vault_id(0), PROTOCOL, LOT, false);
    assert_eq!(take_err(f), EngineError::WrongDefinition);
    let mut f = take_fixture();
    f.surplus_vault = holding(vault_id(0), STABLE, LOT - 1, false);
    assert_eq!(take_err(f), EngineError::VaultBelowBook);
    let mut f = take_fixture();
    f.taker_stable_holding = holding(vault_id(0), STABLE, LOT, false);
    assert_eq!(
        take_err(f),
        EngineError::WrongAccount("taker_stable_holding")
    );
    let mut f = take_fixture();
    f.taker_stable_holding = holding(TAKER_STABLE, PROTOCOL, 0, false);
    assert_eq!(take_err(f), EngineError::WrongDefinition);
    // A fresh stablecoin holding must sign; signed it is accepted.
    let mut f = take_fixture();
    f.taker_stable_holding = fresh(TAKER_STABLE);
    assert_eq!(take_err(f), EngineError::RecipientMustSign);
    let mut f = take_fixture();
    f.taker_stable_holding = meta(TAKER_STABLE, Account::default(), true);
    run_take(f, LOT, u128::MAX).expect("fresh signed recipient");
    // The auction belongs to this config, at its own PDA, owned by the engine.
    let mut f = take_fixture();
    f.surplus_auction = surplus_account(&SurplusAuctionState {
        config: FOREIGN,
        ..live_auction(LOT)
    });
    assert_eq!(take_err(f), EngineError::WrongAccount("surplus_auction"));
    let mut f = take_fixture();
    f.surplus_auction.account_id = auction_id(3);
    assert_eq!(take_err(f), EngineError::WrongAccount("surplus_auction"));
    let mut f = take_fixture();
    f.surplus_auction.account.program_owner = OTHER_PROGRAM;
    assert_eq!(take_err(f), EngineError::WrongAccount("surplus_auction"));
    let mut f = take_fixture();
    f.accounting.account_id = FOREIGN;
    assert_eq!(take_err(f), EngineError::WrongAccount("accounting"));
}

// ---- close_surplus_auction ----

fn close_fixture(vault_balance: u128, lot_left: u128) -> CloseSurplusAccounts {
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

fn close_err(a: CloseSurplusAccounts) -> EngineError {
    close_surplus_auction(ENGINE, a).expect_err("close must be rejected")
}

const C_ACCOUNTING: usize = 2;
const C_AUCTION: usize = 3;

#[test]
fn close_returns_the_whole_vault_to_the_buffer_after_the_ttl() {
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
fn close_sweeps_donations_too_and_chains_nothing_for_an_empty_vault() {
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
fn close_waits_for_the_ttl_and_needs_a_live_auction() {
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
fn close_pins_the_accounts() {
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
