//! Handler-level tests of `bid` / `bid_with_settlement`: every guard is exercised without the guest, so each
//! can be mutation-checked in seconds. Fixture: the standard liquidated position (lot 499_975, target 660_000,
//! debt 600_000, penalty 10%, schedule 2% → 10% over 2_700_000 ms) at R = M = 0.5.

use auction_engine_core::{
    pda::{accounting_pda, auction_pda, buffer_pda, coll_config_pda, vault_pda, vault_seed},
    state::*,
    EngineError,
};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall},
};
use token_core::TokenHolding;

use crate::{
    admin_tests::{
        coll_state, config_state, meta, rejected, ADMIN, COLLATERAL, ENGINE, HOST, ONE, ORACLE,
        STABLE, TOKEN,
    },
    bid::{bid, BidAccounts},
    collateral_tests::{cfg_id, clock_at, engine_owned, oracle_with, rps_with, NOW, R},
    token_calls, EngineResult,
};

const TARGET: u128 = 660_000;
const DEBT: u128 = 600_000;
const LOT: u128 = 499_975;
const PARTIAL_OUT: u128 = 51_020; // floor(100_000 × 0.5 / 0.98)
const POSITION: AccountId = AccountId::new([0x77; 32]);
const BIDDER_STABLE: AccountId = AccountId::new([0x61; 32]);
const BIDDER_COLL: AccountId = AccountId::new([0x62; 32]);
const FOREIGN: AccountId = AccountId::new([0x63; 32]);

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

fn schedule() -> Vec<SchedulePointRec> {
    vec![
        SchedulePointRec {
            elapsed_ms: 0,
            discount_bps: 200,
        },
        SchedulePointRec {
            elapsed_ms: 2_700_000,
            discount_bps: 1_000,
        },
    ]
}

fn auction_state() -> AuctionState {
    AuctionState {
        tag: TAG_AUCTION,
        config: cfg_id(),
        position: POSITION,
        nonce: 0,
        owner: ADMIN,
        keeper: ADMIN,
        settled: false,
        collateral_seized: 500_000,
        collateral_sold: 0,
        collateral_left: LOT,
        keeper_reward: 25,
        leftover_claimable: 0,
        leftover_claimed: 0,
        debt_principal: DEBT,
        amount_to_raise: TARGET,
        reserved_amount: TARGET,
        debt_raised: 0,
        penalty_raised: 0,
        start_ms: NOW,
        schedule: schedule(),
        type_paused_total_at_start: 0,
    }
}

fn auction_id() -> AccountId {
    auction_pda(&ENGINE, &cfg_id(), &POSITION, 0)
}

fn coll_account_state() -> CollateralConfigState {
    CollateralConfigState {
        min_bid: 25,
        debt_floor: 100,
        tip: 5,
        chip_bps: 10,
        reward_cap: 50,
        type_reserved: TARGET,
        schedule: schedule(),
        ..coll_state(cfg_id(), 5_000_000)
    }
}

fn config_with_reserved() -> AccountWithMetadata {
    let mut c = config_state();
    c.global_reserved = TARGET;
    meta(cfg_id(), engine_owned((&c).into()), false)
}

fn accounting_account(state: &AccountingState) -> AccountWithMetadata {
    meta(
        accounting_pda(&ENGINE, &cfg_id()),
        engine_owned(state.into()),
        false,
    )
}

fn fixture() -> BidAccounts {
    BidAccounts {
        bidder_stable_holding: holding(BIDDER_STABLE, STABLE, 1_000_000, true),
        bidder_collateral_holding: holding(BIDDER_COLL, COLLATERAL, 0, false),
        config: config_with_reserved(),
        coll_config: meta(
            coll_config_pda(&ENGINE, &cfg_id(), &COLLATERAL),
            engine_owned((&coll_account_state()).into()),
            false,
        ),
        auction: meta(auction_id(), engine_owned((&auction_state()).into()), false),
        auction_vault: holding(vault_pda(&ENGINE, &auction_id()), COLLATERAL, LOT, false),
        buffer: holding(buffer_pda(&ENGINE, &cfg_id()), STABLE, 0, false),
        stablecoin_definition: meta(
            STABLE,
            Account {
                program_owner: TOKEN,
                ..Account::default()
            },
            false,
        ),
        redemption_price_state: rps_with(R, ONE, NOW),
        oracle: oracle_with(ORACLE, R, NOW, 0),
        clock: clock_at(NOW),
        accounting: None,
    }
}

fn settling() -> BidAccounts {
    BidAccounts {
        accounting: Some(accounting_account(&AccountingState::new())),
        ..fixture()
    }
}

fn run(a: BidAccounts, coin_in_max: u128, min_out: u128) -> EngineResult {
    bid(ENGINE, a, coin_in_max, min_out)
}

fn decode_auction(p: &AccountPostState) -> AuctionState {
    AuctionState::try_from(&p.account().data).expect("auction decodes")
}
fn decode_coll(p: &AccountPostState) -> CollateralConfigState {
    CollateralConfigState::try_from(&p.account().data).expect("coll decodes")
}
fn decode_config(p: &AccountPostState) -> EngineConfigState {
    EngineConfigState::try_from(&p.account().data).expect("config decodes")
}
fn decode_accounting(p: &AccountPostState) -> AccountingState {
    AccountingState::try_from(&p.account().data).expect("accounting decodes")
}

const P_CONFIG: usize = 2;
const P_COLL: usize = 3;
const P_AUCTION: usize = 4;
const P_ACCOUNTING: usize = 11;

fn burn_call(amount: u128) -> ChainedCall {
    let f = fixture();
    token_calls::burn(
        TOKEN,
        f.stablecoin_definition,
        f.bidder_stable_holding,
        amount,
        vec![],
    )
}

fn penalty_call(debt_part: u128, penalty: u128) -> ChainedCall {
    let f = fixture();
    token_calls::transfer(
        TOKEN,
        token_calls::with_balance(&f.bidder_stable_holding, STABLE, 1_000_000 - debt_part),
        f.buffer,
        penalty,
        vec![],
    )
}

fn vault_call(amount: u128) -> ChainedCall {
    let f = fixture();
    token_calls::transfer(
        TOKEN,
        token_calls::authorized(&f.auction_vault),
        f.bidder_collateral_holding,
        amount,
        vec![vault_seed(&auction_id())],
    )
}

#[test]
fn partial_bid_chains_burn_then_vault_transfer_and_writes_one_auction() {
    let (post, calls) = run(fixture(), 100_000, 0).expect("partial bid");
    assert_eq!(post.len(), 11);
    assert_eq!(calls, vec![burn_call(100_000), vault_call(PARTIAL_OUT)]);
    let a = decode_auction(&post[P_AUCTION]);
    assert_eq!(
        (
            a.debt_raised,
            a.penalty_raised,
            a.collateral_left,
            a.settled
        ),
        (100_000, 0, LOT - PARTIAL_OUT, false)
    );
    assert_eq!(decode_coll(&post[P_COLL]).type_reserved, TARGET);
    assert_eq!(decode_config(&post[P_CONFIG]).global_reserved, TARGET);
    assert_eq!(
        decode_coll(&post[P_COLL]).last_seen_oracle_ts,
        Some(NOW),
        "the pause observation is persisted"
    );
}

#[test]
fn dust_and_min_bid_rules_apply_to_non_final_fills() {
    assert_eq!(rejected(run(fixture(), 10, 0)), EngineError::BelowMinBid);
    assert_eq!(
        rejected(run(fixture(), TARGET - 50, 0)),
        EngineError::DustRemainder
    );
}

#[test]
fn the_bidder_stable_holding_must_sign() {
    let mut a = fixture();
    a.bidder_stable_holding.is_authorized = false;
    assert_eq!(
        rejected(run(a, 100_000, 0)),
        EngineError::WrongAccount("bidder_stable_holding")
    );
}

#[test]
fn a_fill_that_reaches_the_target_without_accounting_is_refused() {
    assert_eq!(
        rejected(run(fixture(), TARGET, 0)),
        EngineError::ChainEndAccountsMissing
    );
}

#[test]
fn a_lot_emptying_fill_without_accounting_is_refused() {
    let mut a = fixture();
    a.redemption_price_state = rps_with(2 * R, ONE, NOW);
    a.oracle = oracle_with(ORACLE, 2 * R, NOW, 0);
    assert_eq!(
        rejected(run(a, TARGET, 0)),
        EngineError::ChainEndAccountsMissing
    );
}

#[test]
fn the_final_fill_folds_accounting_once_and_releases_the_reservation() {
    let mut a = settling();
    a.accounting = Some(accounting_account(&AccountingState {
        total_debt_burned: 5,
        total_penalty_collected: 7,
        ..AccountingState::new()
    }));
    let (post, calls) = run(a, TARGET, 0).expect("final bid");
    assert_eq!(post.len(), 12);
    let s = decode_auction(&post[P_AUCTION]);
    assert!(s.settled);
    assert_eq!((s.debt_raised, s.penalty_raised), (DEBT, 60_000));
    let acc = decode_accounting(&post[P_ACCOUNTING]);
    assert_eq!(
        (
            acc.total_debt_burned,
            acc.total_penalty_collected,
            acc.unbacked_debt,
            acc.total_shortfall_booked
        ),
        (5 + DEBT, 7 + 60_000, 0, 0)
    );
    assert_eq!(decode_coll(&post[P_COLL]).type_reserved, 0);
    assert_eq!(decode_config(&post[P_CONFIG]).global_reserved, 0);
    assert_eq!(
        calls,
        vec![
            burn_call(DEBT),
            penalty_call(DEBT, 60_000),
            vault_call(336_734), // floor(660_000 × 0.5 / 0.98)
        ]
    );
}

#[test]
fn a_non_final_settlement_bid_validates_and_echoes_accounting() {
    let a = settling();
    let before = a.accounting.clone().unwrap().account;
    let (post, _) = run(a, 100_000, 0).expect("non-final");
    assert_eq!(post.len(), 12);
    assert_eq!(post[P_ACCOUNTING].account(), &before);
    assert_eq!(decode_coll(&post[P_COLL]).type_reserved, TARGET);

    let mut wrong_id = settling();
    wrong_id.accounting.as_mut().unwrap().account_id = FOREIGN;
    assert_eq!(
        rejected(run(wrong_id, 100_000, 0)),
        EngineError::WrongAccount("accounting")
    );
}

#[test]
fn accounting_must_be_the_engines_pda_and_owned_by_the_engine() {
    for final_fill in [false, true] {
        let coin = if final_fill { TARGET } else { 100_000 };
        let mut wrong_id = settling();
        wrong_id.accounting.as_mut().unwrap().account_id = FOREIGN;
        assert_eq!(
            rejected(run(wrong_id, coin, 0)),
            EngineError::WrongAccount("accounting")
        );
        let mut wrong_owner = settling();
        wrong_owner
            .accounting
            .as_mut()
            .unwrap()
            .account
            .program_owner = HOST;
        assert_eq!(
            rejected(run(wrong_owner, coin, 0)),
            EngineError::WrongAccount("accounting")
        );
    }
}

#[test]
fn a_settled_auction_rejects_further_bids_and_never_folds_again() {
    let mut a = settling();
    let mut st = auction_state();
    st.settled = true;
    st.collateral_left = 100;
    a.auction = meta(auction_id(), engine_owned((&st).into()), false);
    assert_eq!(rejected(run(a, 100_000, 0)), EngineError::NotAuctioning);
}

#[test]
fn the_fill_that_reaches_the_target_leaves_no_second_settlement_path() {
    // Chain the real handler twice: the persisted post-state of the final fill is rejected on replay.
    let (post, _) = run(settling(), TARGET, 0).expect("final bid");
    let mut replay = settling();
    replay.auction = meta(auction_id(), post[P_AUCTION].account().clone(), false);
    assert_eq!(
        rejected(run(replay, 100_000, 0)),
        EngineError::NotAuctioning
    );
}

#[test]
fn a_lot_binding_fill_pays_the_whole_vault_and_books_the_shortfall() {
    let mut a = settling();
    a.redemption_price_state = rps_with(2 * R, ONE, NOW);
    a.oracle = oracle_with(ORACLE, 2 * R, NOW, 0);
    a.auction_vault = holding(
        vault_pda(&ENGINE, &auction_id()),
        COLLATERAL,
        LOT + 7,
        false,
    );
    // Price 1.0 / 0.98 per coin: the lot binds at ceil(499_975 × 0.98 / 1.0) = 489_976 coin.
    let (post, calls) = run(a, u128::MAX, 0).expect("lot-binding bid");
    let s = decode_auction(&post[P_AUCTION]);
    assert_eq!(
        (s.collateral_left, s.settled, s.leftover_claimable),
        (0, true, 0)
    );
    let acc = decode_accounting(&post[P_ACCOUNTING]);
    assert_eq!(acc.unbacked_debt, DEBT - s.debt_raised);
    assert_eq!(acc.total_shortfall_booked, DEBT - s.debt_raised);
    let last = calls.last().expect("calls");
    assert_eq!(*last, {
        let mut c = vault_call(LOT + 7);
        c.pre_states[0] = token_calls::authorized(&holding(
            vault_pda(&ENGINE, &auction_id()),
            COLLATERAL,
            LOT + 7,
            false,
        ));
        c
    });
}

#[test]
fn a_partial_fill_pays_the_booked_amount_not_the_vault_balance() {
    let mut a = fixture();
    a.auction_vault = holding(
        vault_pda(&ENGINE, &auction_id()),
        COLLATERAL,
        LOT + 7,
        false,
    );
    let (_, calls) = run(a, 100_000, 0).expect("partial with donation");
    let mut expected = vault_call(PARTIAL_OUT);
    expected.pre_states[0] = token_calls::authorized(&holding(
        vault_pda(&ENGINE, &auction_id()),
        COLLATERAL,
        LOT + 7,
        false,
    ));
    assert_eq!(*calls.last().unwrap(), expected);
}

#[test]
fn the_vault_must_hold_at_least_the_booked_collateral() {
    let mut a = fixture();
    a.auction_vault = holding(
        vault_pda(&ENGINE, &auction_id()),
        COLLATERAL,
        PARTIAL_OUT - 1,
        false,
    );
    assert_eq!(rejected(run(a, 100_000, 0)), EngineError::VaultBelowBook);
    let mut ok = fixture();
    ok.auction_vault = holding(
        vault_pda(&ENGINE, &auction_id()),
        COLLATERAL,
        PARTIAL_OUT,
        false,
    );
    run(ok, 100_000, 0).expect("exactly the booked amount is enough (>=)");
}

#[test]
fn the_bidder_must_hold_the_charge() {
    let mut a = fixture();
    a.bidder_stable_holding = holding(BIDDER_STABLE, STABLE, 99_999, true);
    assert_eq!(
        rejected(run(a, 100_000, 0)),
        EngineError::InsufficientBalance
    );
    let mut exact = fixture();
    exact.bidder_stable_holding = holding(BIDDER_STABLE, STABLE, 100_000, true);
    run(exact, 100_000, 0).expect("exact balance pays");
}

#[test]
fn max_coin_in_charges_only_the_remaining_target() {
    let (_, calls) = run(settling(), u128::MAX, 0).expect("u128::MAX");
    assert_eq!(calls[0], burn_call(DEBT));
    assert_eq!(calls[1], penalty_call(DEBT, 60_000));
}

#[test]
fn slippage_bound_is_enforced_at_the_quoted_amount() {
    assert_eq!(
        rejected(run(fixture(), 100_000, PARTIAL_OUT + 1)),
        EngineError::Slippage
    );
    run(fixture(), 100_000, PARTIAL_OUT).expect("exact minimum");
}

type Mutation = fn(&mut BidAccounts);

#[test]
fn every_account_is_bound_to_this_config_and_auction() {
    let cases: [(Mutation, EngineError); 7] = [
        (
            |a| a.stablecoin_definition.account_id = FOREIGN,
            EngineError::WrongAccount("stablecoin_definition"),
        ),
        (
            |a| a.buffer.account_id = FOREIGN,
            EngineError::WrongAccount("buffer"),
        ),
        (
            |a| a.auction_vault.account_id = FOREIGN,
            EngineError::WrongAccount("auction_vault"),
        ),
        (
            |a| a.buffer = holding(buffer_pda(&ENGINE, &cfg_id()), COLLATERAL, 0, false),
            EngineError::WrongDefinition,
        ),
        (
            |a| {
                a.auction_vault = holding(vault_pda(&ENGINE, &auction_id()), STABLE, LOT, false);
            },
            EngineError::WrongDefinition,
        ),
        (
            |a| a.bidder_stable_holding = holding(BIDDER_STABLE, COLLATERAL, 1_000_000, true),
            EngineError::WrongDefinition,
        ),
        (
            |a| a.bidder_collateral_holding = holding(BIDDER_COLL, STABLE, 0, false),
            EngineError::WrongDefinition,
        ),
    ];
    for (mutate, expected) in cases {
        let mut a = fixture();
        mutate(&mut a);
        assert_eq!(rejected(run(a, 100_000, 0)), expected);
    }
}

#[test]
fn a_fresh_collateral_holding_must_sign() {
    let mut a = fixture();
    a.bidder_collateral_holding = meta(BIDDER_COLL, Account::default(), false);
    assert_eq!(rejected(run(a, 100_000, 0)), EngineError::RecipientMustSign);
    let mut signed = fixture();
    signed.bidder_collateral_holding = meta(BIDDER_COLL, Account::default(), true);
    run(signed, 100_000, 0).expect("fresh holding that signs");
}

#[test]
fn an_invalid_feed_or_admin_pause_stops_the_bid() {
    let mut stale = fixture();
    stale.oracle = oracle_with(ORACLE, R, NOW - 3_600_001, 0);
    assert_eq!(
        rejected(run(stale, 100_000, 0)),
        EngineError::PriceFeedStale
    );
    let mut conf = fixture();
    conf.oracle = oracle_with(ORACLE, R, NOW, R / 50);
    assert_eq!(
        rejected(run(conf, 100_000, 0)),
        EngineError::PriceFeedLowConfidence
    );
    let mut range = fixture();
    range.oracle = oracle_with(ORACLE, 3 * R, NOW, 0);
    assert_eq!(
        rejected(run(range, 100_000, 0)),
        EngineError::PriceFeedOutOfRange
    );
    let mut paused = fixture();
    let mut c = coll_account_state();
    c.admin_paused = true;
    paused.coll_config = meta(
        paused.coll_config.account_id,
        engine_owned((&c).into()),
        false,
    );
    assert_eq!(rejected(run(paused, 100_000, 0)), EngineError::Paused);
}

#[test]
fn the_discount_does_not_advance_over_the_paused_interval() {
    // 2_350_000 ms elapsed, 1_000_000 of them paused during this auction → t_eff 1_350_000 → 6%.
    let later = NOW + 2_350_000;
    let mut c = coll_account_state();
    c.paused_total_ms = 1_500_000;
    let mut st = auction_state();
    st.type_paused_total_at_start = 500_000;
    let mut a = fixture();
    a.coll_config = meta(a.coll_config.account_id, engine_owned((&c).into()), false);
    a.auction = meta(auction_id(), engine_owned((&st).into()), false);
    a.clock = clock_at(later);
    a.oracle = oracle_with(ORACLE, R, later, 0);
    a.redemption_price_state = rps_with(R, ONE, later);
    let (_, calls) = run(a, 100_000, 0).expect("bid");
    assert_eq!(calls, vec![burn_call(100_000), vault_call(53_191)]);
}

#[test]
fn the_snapshotted_schedule_prices_the_bid_not_the_current_one() {
    let mut c = coll_account_state();
    c.schedule = vec![SchedulePointRec {
        elapsed_ms: 0,
        discount_bps: 3_000,
    }];
    let mut a = fixture();
    a.coll_config = meta(a.coll_config.account_id, engine_owned((&c).into()), false);
    let (_, calls) = run(a, 100_000, 0).expect("bid");
    assert_eq!(*calls.last().unwrap(), vault_call(PARTIAL_OUT));
}

#[test]
fn releasing_more_than_is_reserved_is_a_math_error() {
    let mut c = coll_account_state();
    c.type_reserved = 1;
    let mut a = settling();
    a.coll_config = meta(a.coll_config.account_id, engine_owned((&c).into()), false);
    assert_eq!(rejected(run(a, TARGET, 0)), EngineError::Math);
    let mut g = config_state();
    g.global_reserved = 1;
    let mut b = settling();
    b.config = meta(cfg_id(), engine_owned((&g).into()), false);
    assert_eq!(rejected(run(b, TARGET, 0)), EngineError::Math);
}
