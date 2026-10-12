//! Plan 3a Task 8 / Review Focus 1: SDK quotes equal on-chain results at inclusion time, and a stale
//! quote is protected by the slippage bound derived from it. Clock: a quote is taken at `w.now`, which
//! is the `CLOCK_01` value the next transaction executes under (see `common/sdk.rs`).
mod common;

use auction_engine_core::state::*;
use auction_sdk::{error::SdkError, plan, quote};
use common::*;
use lee_core::account::AccountId;

fn bid_plan(
    v: &auction_sdk::verify::VerifiedConfig,
    a: AccountId,
    coin: u128,
    min_out: u128,
    with_settlement: bool,
) -> auction_sdk::plan::Plan {
    plan::bid(
        v,
        &plan::BidArgs {
            bidder_stable: id(k::BIDDER_STABLE),
            bidder_collateral: id(k::BIDDER_COLL),
            bidder_collateral_is_fresh: false,
            auction: a,
            coin_in_max: coin,
            min_collateral_out: min_out,
            with_settlement,
            oracle: oracle(),
        },
    )
    .expect("plan")
}

/// The on-chain error text carries the engine error's name; the SDK error class must be the same one.
fn assert_same_error_class(
    sdk: Result<impl std::fmt::Debug, SdkError>,
    chain: Result<(), lee::error::LeeError>,
) {
    let name = match sdk.expect_err("the quote must refuse") {
        SdkError::Engine { name, .. } => name,
        other => panic!("expected an engine error class, got {other}"),
    };
    assert_err(chain, &name);
}

#[test]
fn liquidation_quote_equals_the_auction_the_engine_opens() {
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    let v = w.sdk_config();
    let q = quote::quote_liquidation(&WorldChain(&w), &v, &pos, w.now).expect("quote");
    assert!(q.health.unsafe_at_min_cr);
    assert_eq!(
        (q.quote.amount_to_raise, q.quote.keeper_reward, q.quote.lot),
        (LIQ_TARGET, LIQ_REWARD, LIQ_LOT)
    );
    w.liquidate(pos, q.nonce).expect("liquidate");
    let a: AuctionState = w.read(auction(pos, q.nonce));
    assert_eq!(a.keeper, id(k::KEEPER));
    assert_eq!(
        (
            a.collateral_seized,
            a.keeper_reward,
            a.collateral_left,
            a.amount_to_raise,
            a.debt_principal
        ),
        (
            q.quote.seized,
            q.quote.keeper_reward,
            q.quote.lot,
            q.quote.amount_to_raise,
            q.quote.debt_principal
        )
    );
    assert_eq!(q.auction, auction(pos, q.nonce));
}

#[test]
fn a_collateralized_position_is_refused_by_the_quote_and_by_the_real_host() {
    let mut w = World::with_engine();
    // Opened at min CR 1.5 and left there: 500_000 collateral at 0.5 against 600_000 debt is safe.
    let pos = w.open_position(
        k::OWNER,
        k::OWNER_COLL,
        k::OWNER_STABLE,
        0,
        LIQ_COLLATERAL,
        LIQ_DEBT,
    );
    let v = w.sdk_config();
    let e = quote::quote_liquidation(&WorldChain(&w), &v, &pos, w.now)
        .expect_err("collateralized: the quote refuses");
    assert!(matches!(e, SdkError::HostReject(_)), "{e}");
    assert_err(w.liquidate(pos, 0), "collateralized");
    assert_eq!(
        w.read::<AccountingState>(accounting()).unbacked_debt,
        0,
        "nothing was applied"
    );
}

#[test]
fn bid_quote_at_inclusion_time_equals_the_on_chain_fill() {
    let (mut w, pos) = liquidated_world();
    let a = auction(pos, 0);
    let v = w.sdk_config();
    let early = quote::quote_bid(&WorldChain(&w), &v, &a, 100_000, 0, w.now).expect("quote at T");
    w.advance(300_000);
    w.set_oracle(R, w.now); // keep the feed fresh; the discount alone moves
    let at_inclusion =
        quote::quote_bid(&WorldChain(&w), &v, &a, 100_000, 0, w.now).expect("quote at T + 300 s");
    assert!(
        at_inclusion.fill.collateral_out > early.fill.collateral_out,
        "the discount grew"
    );
    let (stable_before, coll_before) = (
        w.balance(id(k::BIDDER_STABLE)),
        w.balance(id(k::BIDDER_COLL)),
    );
    w.run_plan(
        &bid_plan(
            &v,
            a,
            100_000,
            quote::min_out(early.fill.collateral_out, 0),
            false,
        ),
        &[k::BIDDER_STABLE],
    )
    .expect("bid lands at the inclusion clock");
    assert_eq!(
        w.balance(id(k::BIDDER_COLL)) - coll_before,
        at_inclusion.fill.collateral_out
    );
    assert_eq!(
        stable_before - w.balance(id(k::BIDDER_STABLE)),
        at_inclusion.fill.charged
    );
    let st: AuctionState = w.read(a);
    assert_eq!(
        (st.debt_raised + st.penalty_raised, st.collateral_sold),
        (
            at_inclusion.fill.debt_part + at_inclusion.fill.penalty_part,
            at_inclusion.fill.collateral_out
        )
    );
}

#[test]
fn stale_quote_is_protected_by_min_collateral_out() {
    let (mut w, pos) = liquidated_world();
    let a = auction(pos, 0);
    let v = w.sdk_config();
    let q = quote::quote_bid(&WorldChain(&w), &v, &a, 100_000, 0, w.now).expect("quote");
    // The oracle moves 5% against the bidder (fewer collateral per coin) before inclusion.
    w.set_oracle(R * 95 / 100, w.now);
    let moved = quote::quote_bid(&WorldChain(&w), &v, &a, 100_000, 0, w.now).expect("re-quote");
    assert!(
        moved.fill.collateral_out < q.fill.collateral_out,
        "the move is adverse: {} < {}",
        moved.fill.collateral_out,
        q.fill.collateral_out
    );
    let tight = bid_plan(
        &v,
        a,
        100_000,
        quote::min_out(q.fill.collateral_out, 50),
        false,
    );
    assert_err(w.run_plan(&tight, &[k::BIDDER_STABLE]), "Slippage");
    assert_eq!(
        w.balance(id(k::BIDDER_STABLE)),
        1_000_000,
        "the reverted bid charged nothing"
    );
    let loose = bid_plan(
        &v,
        a,
        100_000,
        quote::min_out(q.fill.collateral_out, 1_000),
        false,
    );
    let coll_before = w.balance(id(k::BIDDER_COLL));
    w.run_plan(&loose, &[k::BIDDER_STABLE])
        .expect("within a 10% margin");
    assert_eq!(
        w.balance(id(k::BIDDER_COLL)) - coll_before,
        moved.fill.collateral_out,
        "and the fill equals the re-quote"
    );
}

#[test]
fn a_final_fill_quote_ends_the_auction_and_needs_settlement() {
    let (mut w, pos) = liquidated_world();
    let a = auction(pos, 0);
    let v = w.sdk_config();
    let q = quote::quote_bid(&WorldChain(&w), &v, &a, u128::MAX, 0, w.now).expect("quote");
    assert!(q.fill.ends_auction && q.needs_settlement(50));
    let plain = bid_plan(&v, a, u128::MAX, 0, false);
    assert_err(
        w.run_plan(&plain, &[k::BIDDER_STABLE]),
        "ChainEndAccountsMissing",
    );
    let before = w.balance(id(k::BIDDER_STABLE));
    w.run_plan(&bid_plan(&v, a, u128::MAX, 0, true), &[k::BIDDER_STABLE])
        .expect("bid_with_settlement");
    assert_eq!(before - w.balance(id(k::BIDDER_STABLE)), q.fill.charged);
    assert!(w.read::<AuctionState>(a).settled);
}

#[test]
fn surplus_take_quote_at_a_later_clock_equals_the_burn() {
    let mut w = World::with_system(false);
    w.donate_stable(buffer(), 1_500);
    let v = w.sdk_config();
    w.run_plan(
        &plan::start_surplus(&v, id(k::CALLER), 0).expect("plan"),
        &[k::CALLER],
    )
    .expect("start");
    let first = quote::quote_take_surplus(&WorldChain(&w), &v, 0, 200, w.now).expect("quote");
    w.advance(1_500_000);
    let q = quote::quote_take_surplus(&WorldChain(&w), &v, 0, 200, w.now).expect("later quote");
    assert_ne!(q.tokens, first.tokens, "the schedule moved the price");
    let before = w.balance(id(k::TAKER_PROTOCOL));
    let take = plan::TakeArgs {
        payer: id(k::TAKER_PROTOCOL),
        receiver: id(k::TAKER_STABLE),
        receiver_is_fresh: false,
        nonce: 0,
        amount_coin: 200,
        limit: quote::max_in(q.tokens, 0),
    };
    w.run_plan(
        &plan::take_surplus(&v, &take).expect("plan"),
        &[k::TAKER_PROTOCOL],
    )
    .expect("take");
    assert_eq!(before - w.balance(id(k::TAKER_PROTOCOL)), q.tokens);
    assert_eq!(w.balance(id(k::TAKER_STABLE)), q.filled);
    let sa: SurplusAuctionState = w.read(surplus_auction(0));
    assert_eq!(sa.lot_left, q.lot_left_after);
}

#[test]
fn debt_take_quote_at_a_later_clock_equals_the_mint() {
    let mut w = World::with_system(false);
    w.book_shortfall();
    w.fund_stable(k::TAKER_STABLE, 1_000);
    let v = w.sdk_config();
    w.run_plan(
        &plan::start_debt(&v, id(k::CALLER), 0).expect("plan"),
        &[k::CALLER],
    )
    .expect("start");
    let first = quote::quote_take_debt(&WorldChain(&w), &v, 0, 200, w.now).expect("quote");
    w.advance(1_500_000);
    let q = quote::quote_take_debt(&WorldChain(&w), &v, 0, 200, w.now).expect("later quote");
    assert_ne!(q.tokens, first.tokens, "the schedule moved the price");
    let before = w.balance(id(k::TAKER_PROTOCOL));
    let take = plan::TakeArgs {
        payer: id(k::TAKER_STABLE),
        receiver: id(k::TAKER_PROTOCOL),
        receiver_is_fresh: false,
        nonce: 0,
        amount_coin: 200,
        limit: quote::min_out(q.tokens, 0),
    };
    w.run_plan(
        &plan::take_debt(&v, &take).expect("plan"),
        &[k::TAKER_STABLE],
    )
    .expect("take");
    assert_eq!(w.balance(id(k::TAKER_PROTOCOL)) - before, q.tokens);
    let da: DebtAuctionState = w.read(debt_auction(0));
    assert_eq!(da.lot_left, q.lot_left_after);
}

#[test]
fn quote_refusals_are_in_the_same_error_class_as_the_chain() {
    // A stale feed stops quote and bid with the same engine error.
    let (mut w, pos) = liquidated_world();
    let a = auction(pos, 0);
    let v = w.sdk_config();
    w.advance(3_600_001);
    let sdk = quote::quote_bid(&WorldChain(&w), &v, &a, 100_000, 0, w.now);
    let chain = w.run_plan(&bid_plan(&v, a, 100_000, 0, true), &[k::BIDDER_STABLE]);
    assert_same_error_class(sdk, chain);

    // A settled auction refuses both.
    let (mut w, pos) = liquidated_world();
    let a = auction(pos, 0);
    let v = w.sdk_config();
    w.run_plan(&bid_plan(&v, a, u128::MAX, 0, true), &[k::BIDDER_STABLE])
        .expect("final bid");
    let sdk = quote::quote_bid(&WorldChain(&w), &v, &a, 1_000, 0, w.now);
    let chain = w.run_plan(&bid_plan(&v, a, 1_000, 0, true), &[k::BIDDER_STABLE]);
    assert_same_error_class(sdk, chain);
}
