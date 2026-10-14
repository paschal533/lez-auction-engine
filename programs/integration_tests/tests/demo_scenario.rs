//! ADR-051: the one-command demo's economic path on the real guests with the demo's parameters
//! (`auction_sdk::demo`). The localnet run (`scripts/demo.sh`, R = 1 RAY, debt 300_000) follows the
//! same steps at the same proportions; this test pins that each condition holds when the demo needs it.

mod common;

use auction_engine_core::{
    args::EngineParamsArgs,
    state::{AccountingState, AuctionState},
};
use auction_sdk::demo;
use common::*;

fn demo_world() -> World {
    let mut w = World::new();
    w.create_protocol_token(10_000, mint_authority())
        .expect("mock protocol token");
    w.init_engine_config(&demo::engine_params())
        .expect("init_config");
    w.bind_engine().expect("bind");
    w.set_collateral_config(k::ENGINE_ADMIN, &demo::collateral_args())
        .expect("collateral config");
    w.holding(k::TAKER_STABLE, stable_def(), 0);
    w
}

fn acc(w: &World) -> AccountingState {
    w.read(accounting())
}

#[test]
fn demo_scenario_runs_f1_to_f5_and_the_freeze_path() {
    let mut w = demo_world();
    // The demo's p0 and p2, opened while safe, made unsafe together by the min-ratio raise.
    let p0 = w.open_position(
        k::OWNER,
        k::OWNER_COLL,
        k::OWNER_STABLE,
        0,
        LIQ_COLLATERAL,
        LIQ_DEBT,
    );
    let p2 = w.open_position(
        k::OWNER,
        k::OWNER_COLL,
        k::OWNER_STABLE,
        2,
        LIQ_COLLATERAL,
        LIQ_DEBT,
    );
    w.set_min_cr(2 * ONE);

    // F1-F3 at the redemption price: partial bid, final bid, penalty into the buffer.
    w.liquidate(p0, 0).expect("liquidate p0");
    w.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 2_000_000);
    w.bid(p0, 100_000, 0).expect("partial bid");
    w.bid_settle(p0, LIQ_TARGET, 0).expect("final bid");
    let penalty = LIQ_TARGET - LIQ_DEBT;
    assert_eq!(w.balance(buffer()), penalty);

    // F5: part of the buffer sold for burned protocol tokens.
    w.start_surplus(0).expect("start surplus");
    w.take_surplus(0, 500, u128::MAX).expect("take surplus");
    let buffer_after_f5 = w.balance(buffer());
    assert_eq!(buffer_after_f5, penalty - 500);

    // The price moves to 1.9 R: valid (inside [R/2, 2R]), above the default band, inside the demo band.
    w.set_oracle(R * demo::PRICE_MOVE_NUM / demo::PRICE_MOVE_DEN, w.now);
    w.liquidate(p2, 0).expect("liquidate p2 at the moved price");
    w.bid_settle(p2, LIQ_TARGET, 0).expect("lot-binding bid");
    let a: AuctionState = w.read(auction(p2, 0));
    assert!(
        a.debt_raised < a.debt_principal,
        "the lot does not cover the debt at 1.9 R"
    );
    let shortfall = a.debt_principal - a.debt_raised;
    assert_eq!(acc(&w).unbacked_debt, shortfall);

    // F4: settle against the buffer, then one debt auction lot mints against burned coin.
    w.settle_debt(u128::MAX).expect("settle_debt");
    assert_eq!(acc(&w).unbacked_debt, shortfall - buffer_after_f5);
    assert!(
        acc(&w).unbacked_debt >= demo::DEBT_LOT,
        "enough bad debt for one lot"
    );
    w.start_debt(0).expect("start debt auction");
    w.fund_stable(k::TAKER_STABLE, demo::DEBT_LOT);
    let tokens_before = w.balance(id(k::TAKER_PROTOCOL));
    w.take_debt(
        0,
        k::TAKER_STABLE,
        k::TAKER_PROTOCOL,
        &[k::TAKER_STABLE],
        demo::DEBT_LOT,
        0,
    )
    .expect("take the whole lot");
    assert!(
        w.balance(id(k::TAKER_PROTOCOL)) > tokens_before,
        "protocol tokens minted"
    );
    let remaining = shortfall - buffer_after_f5 - demo::DEBT_LOT;
    assert_eq!(acc(&w).unbacked_debt, remaining);
    assert!(
        remaining > demo::FREEZE_THRESHOLD,
        "still above the threshold after one lot"
    );

    // Freeze path: without a protocol token, bad debt above the threshold freezes new debt.
    w.set_engine_params(
        k::ENGINE_ADMIN,
        &EngineParamsArgs {
            protocol_token_enabled: false,
            ..demo::engine_params()
        },
    )
    .expect("disable the protocol token");
    assert!(!w.host_frozen());
    w.check_solvency().expect("check_solvency");
    assert!(w.host_frozen(), "the host froze new debt");
    assert!(acc(&w).host_frozen_by_engine);
}
