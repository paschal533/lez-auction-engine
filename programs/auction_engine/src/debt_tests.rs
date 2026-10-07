//! Handler-level tests of the F4 debt auction: every guard and every reservation/burn/mint amount is
//! exercised without the guest, so each can be mutation-checked in seconds.

use auction_engine_core::{
    pda::{
        buffer_seed, debt_auction_pda, debt_auction_seed, mint_authority_pda, mint_authority_seed,
        SLOT_PROTOCOL_TOKEN, SLOT_REF_ORACLE,
    },
    state::*,
    EngineError,
};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, Claim},
};
use token_core::TokenDefinition;

use crate::{
    admin_tests::{config_state, fresh, meta, rejected, ENGINE, ONE, STABLE, TOKEN},
    collateral_tests::{cfg_id, clock_at, engine_owned},
    debt::*,
    settle::buffer_burn,
    surplus_tests::{
        accounting_account, accounting_of, buffer_id, config_account, holding, oracle_account,
        placeholder, protocol_config, stable_definition, ADMIN_REF, CALLER, FOREIGN, NOW, PROTOCOL,
        TAKER_PROTOCOL, TAKER_STABLE,
    },
    token_calls, EngineResult,
};

const OTHER_PROGRAM: [u32; 8] = [0x77; 8];
const LOT: u128 = 500;
const START_AMOUNT: u128 = 250; // floor(500 / 2)
const UNBACKED: u128 = 2_000;
const BUFFER: u128 = 300;
const TTL: u64 = 14_400_000;
const MINTED_BEFORE: u128 = 7;

fn engine_mint_authority() -> AccountId {
    mint_authority_pda(&ENGINE, &cfg_id())
}

fn debt_id(nonce: u64) -> AccountId {
    debt_auction_pda(&ENGINE, &cfg_id(), nonce)
}

fn debt_def(authority: Option<AccountId>, owner: [u32; 8]) -> AccountWithMetadata {
    meta(
        PROTOCOL,
        Account {
            program_owner: owner,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("MOCK"),
                total_supply: 10_000,
                metadata_id: None,
                authority,
            }),
            ..Account::default()
        },
        false,
    )
}

fn good_def() -> AccountWithMetadata {
    debt_def(Some(engine_mint_authority()), TOKEN)
}

fn mint_authority_account() -> AccountWithMetadata {
    fresh(engine_mint_authority())
}

fn debt_account(state: &DebtAuctionState) -> AccountWithMetadata {
    meta(debt_id(state.nonce), engine_owned(state.into()), false)
}

fn debt_of(p: &AccountPostState) -> DebtAuctionState {
    DebtAuctionState::try_from(&p.account().data).expect("debt auction decodes")
}

fn live_debt(lot_left: u128) -> DebtAuctionState {
    DebtAuctionState {
        tag: TAG_DEBT,
        config: cfg_id(),
        nonce: 0,
        lot_left,
        start_amount: START_AMOUNT,
        start_ms: NOW,
        live: true,
    }
}

// ---- start_debt_auction ----

fn start_accounting() -> AccountingState {
    AccountingState {
        unbacked_debt: UNBACKED,
        total_debt_settled: 11,
        ..AccountingState::new()
    }
}

fn start_fixture() -> StartDebtAccounts {
    StartDebtAccounts {
        caller: meta(CALLER, Default::default(), true),
        config: config_account(&protocol_config(false)),
        accounting: accounting_account(&start_accounting()),
        buffer: holding(buffer_id(), STABLE, BUFFER, false),
        stablecoin_definition: stable_definition(),
        debt_auction: fresh(debt_id(0)),
        protocol_token_definition: good_def(),
        ref_oracle: placeholder(SLOT_REF_ORACLE),
        clock: clock_at(NOW),
    }
}

fn run_start(a: StartDebtAccounts, nonce: u64) -> EngineResult {
    start_debt_auction(ENGINE, a, nonce)
}

fn start_err(a: StartDebtAccounts) -> EngineError {
    rejected(run_start(a, 0))
}

const S_ACCOUNTING: usize = 2;
const S_AUCTION: usize = 5;

#[test]
fn start_reserves_the_lot_burns_the_implicit_settle_and_creates_the_auction() {
    let f = start_fixture();
    let (def, buffer) = (f.stablecoin_definition.clone(), f.buffer.clone());
    let (post, calls) = run_start(f, 0).expect("start");
    assert_eq!(post.len(), 9);
    assert_eq!(
        debt_of(&post[S_AUCTION]),
        DebtAuctionState {
            tag: TAG_DEBT,
            config: cfg_id(),
            nonce: 0,
            lot_left: LOT,
            start_amount: START_AMOUNT,
            start_ms: NOW,
            live: true,
        }
    );
    assert_eq!(
        post[S_AUCTION].required_claim(),
        Some(Claim::Pda(debt_auction_seed(&cfg_id(), 0)))
    );
    let acc = accounting_of(&post[S_ACCOUNTING]);
    // The buffer's 300 settle first (unbacked 2_000 -> 1_700), then the 500-coin lot is reserved.
    assert_eq!(
        (
            acc.unbacked_debt,
            acc.debt_on_auction,
            acc.total_debt_settled,
            acc.debt_live,
            acc.debt_nonce
        ),
        (UNBACKED - BUFFER, LOT, 11 + BUFFER, true, 1)
    );
    assert_eq!(acc.total_debt_auctioned, 0);
    assert!(acc.debt_on_auction <= acc.unbacked_debt);
    assert_eq!(
        calls,
        vec![buffer_burn(&protocol_config(false), &cfg_id(), &def, &buffer, BUFFER).expect("burn")],
        "only the implicit settle is chained, under the buffer seed"
    );
    assert_eq!(calls[0].pda_seeds, vec![buffer_seed(&cfg_id())]);
}

#[test]
fn start_without_buffer_funds_chains_nothing() {
    let mut f = start_fixture();
    f.buffer = holding(buffer_id(), STABLE, 0, false);
    let (post, calls) = run_start(f, 0).expect("start");
    assert!(calls.is_empty());
    let acc = accounting_of(&post[S_ACCOUNTING]);
    assert_eq!((acc.unbacked_debt, acc.debt_on_auction), (UNBACKED, LOT));
}

#[test]
fn start_eligibility_is_unbacked_minus_the_buffer_at_least_the_lot() {
    let mut f = start_fixture();
    f.accounting = accounting_account(&AccountingState {
        unbacked_debt: BUFFER + LOT - 1,
        ..AccountingState::new()
    });
    assert_eq!(start_err(f), EngineError::InsufficientBadDebt);
    let mut f = start_fixture();
    f.accounting = accounting_account(&AccountingState {
        unbacked_debt: BUFFER + LOT,
        ..AccountingState::new()
    });
    let (post, _) = run_start(f, 0).expect("exactly the lot beyond the buffer");
    assert_eq!(accounting_of(&post[S_ACCOUNTING]).unbacked_debt, LOT);
    // A buffer larger than the debt covers it entirely.
    let mut f = start_fixture();
    f.buffer = holding(buffer_id(), STABLE, UNBACKED + 5_000, false);
    assert_eq!(start_err(f), EngineError::InsufficientBadDebt);
    // No debt at all.
    let mut f = start_fixture();
    f.accounting = accounting_account(&AccountingState::new());
    assert_eq!(start_err(f), EngineError::InsufficientBadDebt);
}

#[test]
fn start_allows_only_one_live_auction_and_the_next_nonce() {
    assert_eq!(
        rejected(run_start(start_fixture(), 1)),
        EngineError::NonceMismatch
    );
    let mut f = start_fixture();
    f.accounting = accounting_account(&AccountingState {
        debt_live: true,
        debt_nonce: 0,
        debt_on_auction: LOT,
        ..start_accounting()
    });
    assert_eq!(start_err(f), EngineError::DebtAuctionAlreadyLive);
    // Nonce 1 after a first auction.
    let mut f = start_fixture();
    f.accounting = accounting_account(&AccountingState {
        debt_nonce: 1,
        ..start_accounting()
    });
    f.debt_auction = fresh(debt_id(1));
    let (post, _) = run_start(f, 1).expect("second auction");
    assert_eq!(accounting_of(&post[S_ACCOUNTING]).debt_nonce, 2);
    assert_eq!(debt_of(&post[S_AUCTION]).nonce, 1);
}

#[test]
fn start_rejections_follow_the_freeze_list() {
    let mut f = start_fixture();
    f.config = config_account(&config_state());
    f.protocol_token_definition = placeholder(SLOT_PROTOCOL_TOKEN);
    assert_eq!(start_err(f), EngineError::TokenNotConfigured);

    let mut f = start_fixture();
    f.protocol_token_definition = debt_def(Some(engine_mint_authority()), OTHER_PROGRAM);
    assert_eq!(start_err(f), EngineError::ProtocolTokenMismatch);
    let mut f = start_fixture();
    f.protocol_token_definition.account.data = Data::from(&TokenDefinition::NonFungible {
        name: String::from("N"),
        printable_supply: 1,
        metadata_id: FOREIGN,
    });
    assert_eq!(start_err(f), EngineError::ProtocolTokenMismatch);
    let mut f = start_fixture();
    f.protocol_token_definition = fresh(PROTOCOL);
    assert_eq!(start_err(f), EngineError::ProtocolTokenMismatch);

    for authority in [None, Some(FOREIGN)] {
        let mut f = start_fixture();
        f.protocol_token_definition = debt_def(authority, TOKEN);
        assert_eq!(start_err(f), EngineError::MintAuthorityMissing);
    }
    let mut f = start_fixture();
    f.protocol_token_definition.account_id = FOREIGN;
    assert_eq!(start_err(f), EngineError::WrongAccount("slot"));
}

#[test]
fn start_rejects_a_reference_price_that_cannot_mint_usefully() {
    // 500 coin at 1/100 coin per token = 50_000 tokens > max_mint_per_auction (5_000).
    let mut cfg = protocol_config(false);
    cfg.protocol_token_ref_price = ONE / 100;
    let mut f = start_fixture();
    f.config = config_account(&cfg);
    assert_eq!(start_err(f), EngineError::MintCapConfig);
    // 500 coin at 600 coin per token floors to a zero start amount.
    let mut cfg = protocol_config(false);
    cfg.protocol_token_ref_price = 600 * ONE;
    let mut f = start_fixture();
    f.config = config_account(&cfg);
    assert_eq!(start_err(f), EngineError::MintCapConfig);
    // Exactly at the cap is fine: 500 / (1/10) = 5_000.
    let mut cfg = protocol_config(false);
    cfg.protocol_token_ref_price = ONE / 10;
    let mut f = start_fixture();
    f.config = config_account(&cfg);
    let (post, _) = run_start(f, 0).expect("start amount equal to the cap");
    assert_eq!(debt_of(&post[S_AUCTION]).start_amount, 5_000);
}

#[test]
fn start_snapshots_the_clamped_oracle_price_into_the_start_amount() {
    let cfg = protocol_config(true);
    let oracle_id = cfg.protocol_token_oracle_id.expect("oracle configured");
    let run = |price: u128| {
        let mut f = start_fixture();
        f.config = config_account(&cfg);
        f.ref_oracle = oracle_account(price, NOW);
        assert_eq!(f.ref_oracle.account_id, oracle_id);
        let (post, _) = run_start(f, 0).expect("start");
        debt_of(&post[S_AUCTION]).start_amount
    };
    // 2.2 coin per token (+10%): floor(500 / 2.2) = 227.
    assert_eq!(run(ADMIN_REF + ADMIN_REF / 10), 227);
    // 3.9 coin per token is clamped to +20% (2.4): floor(500 / 2.4) = 208.
    assert_eq!(run(ADMIN_REF + ADMIN_REF * 19 / 20), 208);
}

#[test]
fn start_needs_a_signing_caller_and_validates_every_account() {
    let mut f = start_fixture();
    f.caller.is_authorized = false;
    assert_eq!(start_err(f), EngineError::WrongAccount("caller"));

    let mut f = start_fixture();
    f.buffer.account_id = FOREIGN;
    assert_eq!(start_err(f), EngineError::WrongAccount("buffer"));
    let mut f = start_fixture();
    f.buffer = holding(buffer_id(), PROTOCOL, BUFFER, false);
    assert_eq!(start_err(f), EngineError::WrongDefinition);
    let mut f = start_fixture();
    f.stablecoin_definition.account_id = FOREIGN;
    assert_eq!(
        start_err(f),
        EngineError::WrongAccount("stablecoin_definition")
    );
    let mut f = start_fixture();
    f.debt_auction.account_id = debt_id(7);
    assert_eq!(start_err(f), EngineError::WrongAccount("debt_auction"));
    let mut f = start_fixture();
    f.debt_auction = debt_account(&DebtAuctionState {
        live: false,
        ..live_debt(0)
    });
    assert_eq!(start_err(f), EngineError::WrongAccount("debt_auction"));
    let mut f = start_fixture();
    f.accounting.account_id = FOREIGN;
    assert_eq!(start_err(f), EngineError::WrongAccount("accounting"));
    let mut f = start_fixture();
    f.config.account_id = FOREIGN;
    assert_eq!(start_err(f), EngineError::WrongAccount("config"));
    let mut f = start_fixture();
    f.clock.account_id = FOREIGN;
    assert_eq!(start_err(f), EngineError::WrongClock);
    // The reference-oracle slot is pinned: a real oracle where the placeholder belongs is refused.
    let mut f = start_fixture();
    f.ref_oracle = oracle_account(2 * ONE, NOW);
    assert_eq!(start_err(f), EngineError::WrongAccount("unused slot"));
}

// ---- take_debt ----

fn take_accounting() -> AccountingState {
    AccountingState {
        unbacked_debt: UNBACKED,
        debt_on_auction: LOT,
        debt_live: true,
        debt_nonce: 1,
        total_protocol_minted: MINTED_BEFORE,
        ..AccountingState::new()
    }
}

fn take_fixture() -> TakeDebtAccounts {
    TakeDebtAccounts {
        taker_stable_holding: holding(TAKER_STABLE, STABLE, 1_000, true),
        taker_protocol_holding: holding(TAKER_PROTOCOL, PROTOCOL, 10_000, false),
        config: config_account(&protocol_config(false)),
        accounting: accounting_account(&take_accounting()),
        debt_auction: debt_account(&live_debt(LOT)),
        stablecoin_definition: stable_definition(),
        protocol_token_definition: good_def(),
        mint_authority: mint_authority_account(),
        clock: clock_at(NOW),
    }
}

fn run_take(a: TakeDebtAccounts, coin: u128, min_out: u128) -> EngineResult {
    take_debt(ENGINE, a, coin, min_out)
}

fn take_err(a: TakeDebtAccounts) -> EngineError {
    rejected(run_take(a, LOT, 0))
}

const T_ACCOUNTING: usize = 3;
const T_AUCTION: usize = 4;

/// The two chained calls of a take: burn the filled coin from the taker, mint under the engine PDA.
fn burn_and_mint(filled: u128, minted: u128) -> Vec<ChainedCall> {
    let f = take_fixture();
    vec![
        token_calls::burn(
            TOKEN,
            f.stablecoin_definition,
            f.taker_stable_holding,
            filled,
            vec![],
        ),
        token_calls::mint_with_authority(
            TOKEN,
            f.protocol_token_definition,
            f.taker_protocol_holding,
            token_calls::authorized(&f.mint_authority),
            minted,
            vec![mint_authority_seed(&cfg_id())],
        ),
    ]
}

#[test]
fn full_take_burns_the_lot_mints_the_start_amount_and_releases_the_reservation() {
    let (post, calls) = run_take(take_fixture(), LOT, START_AMOUNT).expect("take");
    assert_eq!(post.len(), 9);
    assert_eq!(calls, burn_and_mint(LOT, START_AMOUNT));
    let acc = accounting_of(&post[T_ACCOUNTING]);
    assert_eq!(
        (
            acc.unbacked_debt,
            acc.debt_on_auction,
            acc.total_debt_auctioned,
            acc.total_protocol_minted,
            acc.debt_live,
            acc.debt_nonce
        ),
        (
            UNBACKED - LOT,
            0,
            LOT,
            MINTED_BEFORE + START_AMOUNT,
            false,
            1
        )
    );
    assert_eq!(
        acc.total_debt_burned, 0,
        "the collateral-auction counter is untouched"
    );
    let da = debt_of(&post[T_AUCTION]);
    assert_eq!(
        (da.lot_left, da.live, da.start_amount, da.start_ms),
        (0, false, START_AMOUNT, NOW)
    );
}

#[test]
fn the_burn_is_authorised_by_the_taker_and_only_the_mint_carries_the_engine_seed() {
    let (_, calls) = run_take(take_fixture(), LOT, 0).expect("take");
    assert!(calls[0].pda_seeds.is_empty(), "no engine seed on the burn");
    assert!(
        calls[0].pre_states[1].is_authorized,
        "the taker's signed holding"
    );
    assert_eq!(calls[0].pre_states[1].account_id, TAKER_STABLE);
    assert_eq!(calls[0].pre_states[0].account_id, STABLE);
    assert_eq!(calls[1].pda_seeds, vec![mint_authority_seed(&cfg_id())]);
    assert!(
        calls[1].pre_states[2].is_authorized,
        "the authority is authorised in the mint"
    );
    assert_eq!(calls[1].pre_states[2].account_id, engine_mint_authority());
    assert_eq!(calls[1].pre_states[0].account_id, PROTOCOL);
    assert_eq!(calls[1].pre_states[1].account_id, TAKER_PROTOCOL);
}

#[test]
fn max_amount_fills_at_most_the_lot_and_burns_and_records_only_the_fill() {
    // Review Focus 4.
    let (post, calls) = run_take(take_fixture(), u128::MAX, 0).expect("max take");
    assert_eq!(calls, burn_and_mint(LOT, START_AMOUNT));
    let acc = accounting_of(&post[T_ACCOUNTING]);
    assert_eq!(
        (
            acc.unbacked_debt,
            acc.total_debt_auctioned,
            acc.debt_on_auction
        ),
        (UNBACKED - LOT, LOT, 0)
    );
    assert!(!debt_of(&post[T_AUCTION]).live);
    // A partial lot: only what is left is burned even with MAX.
    let mut f = take_fixture();
    f.debt_auction = debt_account(&live_debt(200));
    f.accounting = accounting_account(&AccountingState {
        debt_on_auction: 200,
        ..take_accounting()
    });
    let (post, calls) = run_take(f, u128::MAX, 0).expect("remaining lot");
    assert_eq!(calls, burn_and_mint(200, 100));
    assert_eq!(accounting_of(&post[T_ACCOUNTING]).debt_on_auction, 0);
}

#[test]
fn partial_take_burns_and_records_the_fill_and_keeps_the_rest_reserved() {
    let (post, calls) = run_take(take_fixture(), 300, 150).expect("partial");
    assert_eq!(
        calls,
        burn_and_mint(300, 150),
        "mint = floor(250 x 300 / 500)"
    );
    let acc = accounting_of(&post[T_ACCOUNTING]);
    assert_eq!(
        (
            acc.unbacked_debt,
            acc.debt_on_auction,
            acc.total_debt_auctioned,
            acc.total_protocol_minted,
            acc.debt_live
        ),
        (UNBACKED - 300, 200, 300, MINTED_BEFORE + 150, true)
    );
    assert!(acc.debt_on_auction <= acc.unbacked_debt);
    let da = debt_of(&post[T_AUCTION]);
    assert_eq!((da.lot_left, da.live), (200, true));
    // The next take continues from the stored remainder: exactly the paired amounts again.
    let mut f = take_fixture();
    f.debt_auction = debt_account(&da);
    f.accounting = accounting_account(&acc);
    let (post2, calls2) = run_take(f, 200, 0).expect("rest");
    assert_eq!(calls2, burn_and_mint(200, 100));
    let acc2 = accounting_of(&post2[T_ACCOUNTING]);
    assert_eq!(
        (acc2.unbacked_debt, acc2.debt_on_auction, acc2.debt_live),
        (UNBACKED - LOT, 0, false)
    );
}

#[test]
fn take_price_grows_with_time_and_the_absolute_cap_binds() {
    // growth 30% at 1_000 ms: offered = floor(250 x 1.3) = 325.
    let mut f = take_fixture();
    f.clock = clock_at(NOW + 1_000);
    let (_, calls) = run_take(f, LOT, 0).expect("grown");
    assert_eq!(calls, burn_and_mint(LOT, 325));
    // The absolute cap: the same take with max_mint_per_auction = 300 mints at most 300.
    let mut cfg = protocol_config(false);
    cfg.system.max_mint_per_auction = 300;
    let mut f = take_fixture();
    f.config = config_account(&cfg);
    f.clock = clock_at(NOW + 1_000);
    let (post, calls) = run_take(f, LOT, 0).expect("capped");
    assert_eq!(calls, burn_and_mint(LOT, 300));
    assert_eq!(
        accounting_of(&post[T_ACCOUNTING]).total_protocol_minted,
        MINTED_BEFORE + 300
    );
    // A partial take of a capped offer prorates the CAPPED amount: floor(300 x 100 / 500) = 60.
    let mut f = take_fixture();
    f.config = config_account(&cfg);
    f.clock = clock_at(NOW + 1_000);
    let (_, calls) = run_take(f, 100, 0).expect("partial capped");
    assert_eq!(calls, burn_and_mint(100, 60));
}

#[test]
fn take_core_rejections() {
    let rej = |coin, min_out| rejected(run_take(take_fixture(), coin, min_out));
    assert_eq!(rej(495, 0), EngineError::DustRemainder);
    assert_eq!(rej(LOT, START_AMOUNT + 1), EngineError::Slippage);
    assert_eq!(rej(0, 0), EngineError::ZeroFill);
    let mut f = take_fixture();
    f.debt_auction = debt_account(&DebtAuctionState {
        live: false,
        ..live_debt(0)
    });
    assert_eq!(take_err(f), EngineError::NotLive);
}

#[test]
fn a_take_that_would_mint_nothing_is_a_zero_fill() {
    // start_amount 1: floor(1 x 10 / 500) = 0 tokens for the smallest allowed fill.
    let mut f = take_fixture();
    f.debt_auction = debt_account(&DebtAuctionState {
        start_amount: 1,
        ..live_debt(LOT)
    });
    assert_eq!(rejected(run_take(f, 10, 0)), EngineError::ZeroFill);
}

#[test]
fn take_needs_the_taker_signature_and_enough_stablecoin() {
    let mut f = take_fixture();
    f.taker_stable_holding.is_authorized = false;
    assert_eq!(
        take_err(f),
        EngineError::WrongAccount("taker_stable_holding")
    );
    let mut f = take_fixture();
    f.taker_stable_holding = holding(TAKER_STABLE, STABLE, LOT - 1, true);
    assert_eq!(take_err(f), EngineError::InsufficientBalance);
    let mut f = take_fixture();
    f.taker_stable_holding = holding(TAKER_STABLE, STABLE, LOT, true);
    run_take(f, LOT, 0).expect("exactly enough");
    // Only the FILLED amount must be covered: MAX against a 200-coin lot needs 200.
    let mut f = take_fixture();
    f.debt_auction = debt_account(&live_debt(200));
    f.accounting = accounting_account(&AccountingState {
        debt_on_auction: 200,
        ..take_accounting()
    });
    f.taker_stable_holding = holding(TAKER_STABLE, STABLE, 200, true);
    run_take(f, u128::MAX, 0).expect("MAX against a small lot");
    let mut f = take_fixture();
    f.taker_stable_holding = holding(TAKER_STABLE, PROTOCOL, 1_000, true);
    assert_eq!(take_err(f), EngineError::WrongDefinition);
}

#[test]
fn a_fresh_taker_protocol_holding_must_sign() {
    // Review Focus 2 / S7: the token program claims a fresh recipient with `Claim::Authorized`.
    let mut f = take_fixture();
    f.taker_protocol_holding = fresh(TAKER_PROTOCOL);
    assert_eq!(take_err(f), EngineError::RecipientMustSign);
    let mut f = take_fixture();
    f.taker_protocol_holding = meta(TAKER_PROTOCOL, Default::default(), true);
    let (_, calls) = run_take(f, LOT, 0).expect("fresh signed holding");
    assert!(calls[1].pre_states[1].is_authorized);
    // An initialised holding of another definition is refused; one of the right definition needs no signature.
    let mut f = take_fixture();
    f.taker_protocol_holding = holding(TAKER_PROTOCOL, STABLE, 1, false);
    assert_eq!(take_err(f), EngineError::WrongDefinition);
    let mut f = take_fixture();
    f.taker_protocol_holding = holding(TAKER_STABLE, STABLE, 1_000, true);
    assert_eq!(
        take_err(f),
        EngineError::WrongAccount("taker_protocol_holding")
    );
}

#[test]
fn take_checks_the_protocol_token_definition_and_the_mint_authority() {
    let mut f = take_fixture();
    f.protocol_token_definition.account_id = FOREIGN;
    assert_eq!(take_err(f), EngineError::WrongAccount("slot"));
    let mut f = take_fixture();
    f.protocol_token_definition = debt_def(Some(engine_mint_authority()), OTHER_PROGRAM);
    assert_eq!(take_err(f), EngineError::ProtocolTokenMismatch);
    let mut f = take_fixture();
    f.protocol_token_definition.account.data = Data::from(&TokenDefinition::NonFungible {
        name: String::from("N"),
        printable_supply: 1,
        metadata_id: FOREIGN,
    });
    assert_eq!(take_err(f), EngineError::ProtocolTokenMismatch);
    for authority in [None, Some(FOREIGN)] {
        let mut f = take_fixture();
        f.protocol_token_definition = debt_def(authority, TOKEN);
        assert_eq!(take_err(f), EngineError::MintAuthorityMissing);
    }
    let mut f = take_fixture();
    f.config = config_account(&config_state());
    f.protocol_token_definition = placeholder(SLOT_PROTOCOL_TOKEN);
    assert_eq!(take_err(f), EngineError::TokenNotConfigured);
    // The mint authority account must be the config's PDA.
    let mut f = take_fixture();
    f.mint_authority = fresh(FOREIGN);
    assert_eq!(take_err(f), EngineError::WrongAccount("mint_authority"));
    let mut f = take_fixture();
    f.stablecoin_definition.account_id = FOREIGN;
    assert_eq!(
        take_err(f),
        EngineError::WrongAccount("stablecoin_definition")
    );
}

#[test]
fn take_pins_the_auction_the_book_and_the_pairing() {
    let mut f = take_fixture();
    f.debt_auction.account_id = debt_id(1);
    assert_eq!(take_err(f), EngineError::WrongAccount("debt_auction"));
    let mut f = take_fixture();
    f.debt_auction.account.program_owner = OTHER_PROGRAM;
    assert_eq!(take_err(f), EngineError::WrongAccount("debt_auction"));
    let mut f = take_fixture();
    f.debt_auction = debt_account(&DebtAuctionState {
        config: FOREIGN,
        ..live_debt(LOT)
    });
    assert_eq!(take_err(f), EngineError::WrongAccount("debt_auction"));
    let mut f = take_fixture();
    f.accounting.account_id = FOREIGN;
    assert_eq!(take_err(f), EngineError::WrongAccount("accounting"));
    let mut f = take_fixture();
    f.config.account_id = FOREIGN;
    assert_eq!(take_err(f), EngineError::WrongAccount("config"));
    let mut f = take_fixture();
    f.clock.account_id = FOREIGN;
    assert_eq!(take_err(f), EngineError::WrongClock);
    // A forged book whose reservation is smaller than the fill cannot be released twice.
    let mut f = take_fixture();
    f.accounting = accounting_account(&AccountingState {
        debt_on_auction: LOT - 1,
        ..take_accounting()
    });
    assert_eq!(take_err(f), EngineError::Math);
}

// ---- close_debt_auction ----

fn close_fixture(lot_left: u128, now: u64) -> CloseDebtAccounts {
    CloseDebtAccounts {
        caller: meta(CALLER, Default::default(), true),
        config: config_account(&protocol_config(false)),
        accounting: accounting_account(&AccountingState {
            debt_on_auction: lot_left,
            ..take_accounting()
        }),
        debt_auction: debt_account(&live_debt(lot_left)),
        clock: clock_at(now),
    }
}

fn close_err(a: CloseDebtAccounts) -> EngineError {
    rejected(close_debt_auction(ENGINE, a))
}

#[test]
fn close_releases_exactly_the_untaken_reservation_after_the_ttl() {
    let (post, calls) = close_debt_auction(ENGINE, close_fixture(200, NOW + TTL)).expect("close");
    assert!(calls.is_empty(), "nothing is minted or burned on close");
    assert_eq!(post.len(), 5);
    let acc = accounting_of(&post[2]);
    assert_eq!(
        (
            acc.unbacked_debt,
            acc.debt_on_auction,
            acc.debt_live,
            acc.total_debt_auctioned,
            acc.total_protocol_minted
        ),
        (UNBACKED, 0, false, 0, MINTED_BEFORE)
    );
    let da = debt_of(&post[3]);
    assert_eq!((da.lot_left, da.live), (0, false));
    // Another reservation stays untouched: only `lot_left` is released.
    let mut f = close_fixture(200, NOW + TTL);
    f.accounting = accounting_account(&AccountingState {
        debt_on_auction: 450,
        ..take_accounting()
    });
    let (post, _) = close_debt_auction(ENGINE, f).expect("close");
    assert_eq!(accounting_of(&post[2]).debt_on_auction, 250);
}

#[test]
fn close_waits_for_the_ttl_and_needs_a_live_auction() {
    assert_eq!(
        close_err(close_fixture(200, NOW + TTL - 1)),
        EngineError::DelayNotElapsed
    );
    close_debt_auction(ENGINE, close_fixture(200, NOW + TTL)).expect("exactly at the ttl");
    let mut f = close_fixture(200, NOW + TTL);
    f.debt_auction = debt_account(&DebtAuctionState {
        live: false,
        ..live_debt(0)
    });
    assert_eq!(close_err(f), EngineError::NotLive);
}

#[test]
fn close_pins_the_accounts_and_the_pairing() {
    let mut f = close_fixture(200, NOW + TTL);
    f.caller.is_authorized = false;
    assert_eq!(close_err(f), EngineError::WrongAccount("caller"));
    let mut f = close_fixture(200, NOW + TTL);
    f.debt_auction.account_id = debt_id(3);
    assert_eq!(close_err(f), EngineError::WrongAccount("debt_auction"));
    let mut f = close_fixture(200, NOW + TTL);
    f.accounting.account_id = FOREIGN;
    assert_eq!(close_err(f), EngineError::WrongAccount("accounting"));
    let mut f = close_fixture(200, NOW + TTL);
    f.config.account_id = FOREIGN;
    assert_eq!(close_err(f), EngineError::WrongAccount("config"));
    let mut f = close_fixture(200, NOW + TTL);
    f.clock.account_id = FOREIGN;
    assert_eq!(close_err(f), EngineError::WrongClock);
    let mut f = close_fixture(200, NOW + TTL);
    f.accounting = accounting_account(&AccountingState {
        debt_on_auction: 199,
        ..take_accounting()
    });
    assert_eq!(close_err(f), EngineError::Math);
}
