//! Plan 3a Task 8: every SDK plan runs on the real token, stablecoin and engine guests.
mod common;

use auction_engine_core::{
    args::{CollateralArgs, EngineParamsArgs, DEBT_RELIST_DELAY_MS},
    state::*,
};
use auction_sdk::{
    addresses::HostAddresses, chain::clock_ms, plan, plan_host, quote, verify::verify_binding,
};
use common::*;

#[test]
fn the_world_config_verifies_and_the_host_binds_it() {
    let w = World::with_engine();
    let v = w.sdk_config();
    verify_binding(&WorldChain(&w), &v).expect("bound");
    assert_eq!(v.engine.buffer, buffer());
    assert_eq!(
        clock_ms(&WorldChain(&w)).expect("clock"),
        w.now,
        "the clock a quote must use is the one the transaction executes under"
    );
}

#[test]
fn sdk_addresses_and_account_lists_equal_the_harness() {
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    let v = w.sdk_config();
    assert_eq!(v.engine.accounting, accounting());
    assert_eq!(v.engine.engine_authority, engine_authority());
    assert_eq!(v.engine.mint_authority, mint_authority());
    assert_eq!(v.coll_config_id(), coll_config());
    assert_eq!(v.engine.record(&pos), record(pos));
    assert_eq!(v.engine.auction(&pos, 0), auction(pos, 0));
    assert_eq!(
        v.engine.auction_vault(&auction(pos, 0)),
        auction_vault(pos, 0)
    );
    assert_eq!(v.host.position_vault(&pos), host_vault(pos));
    assert_eq!(v.host.liquidation_engine_state, les());
    let liq = plan::liquidate(
        &v,
        &plan::LiquidateArgs {
            keeper: id(k::KEEPER),
            keeper_collateral_holding: id(k::KEEPER_COLL),
            keeper_holding_is_fresh: true,
            position: pos,
            nonce: 0,
            oracle: oracle(),
        },
    )
    .expect("plan");
    assert_eq!(
        liq.account_ids,
        w.liquidate_accounts(pos, 0, id(k::KEEPER_COLL)),
        "SDK liquidate list == harness list"
    );
    for with_settlement in [false, true] {
        let bid = plan::bid(
            &v,
            &plan::BidArgs {
                bidder_stable: id(k::BIDDER_STABLE),
                bidder_collateral: id(k::BIDDER_COLL),
                bidder_collateral_is_fresh: false,
                auction: auction(pos, 0),
                coin_in_max: 1,
                min_collateral_out: 0,
                with_settlement,
                oracle: oracle(),
            },
        )
        .expect("plan");
        assert_eq!(
            bid.account_ids,
            w.bid_accounts(pos, 0, k::BIDDER_STABLE, k::BIDDER_COLL, with_settlement),
            "SDK bid list (settlement: {with_settlement}) == harness list"
        );
    }
}

#[test]
fn liquidate_bid_settle_and_claim_via_sdk_plans() {
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    let v = w.sdk_config();
    let q = quote::quote_liquidation(&WorldChain(&w), &v, &pos, w.now).expect("quote");
    let p = plan::liquidate(
        &v,
        &plan::LiquidateArgs {
            keeper: id(k::KEEPER),
            keeper_collateral_holding: id(k::KEEPER_COLL),
            keeper_holding_is_fresh: true,
            position: pos,
            nonce: q.nonce,
            oracle: oracle(),
        },
    )
    .expect("plan");
    w.run_plan(&p, &[k::KEEPER, k::KEEPER_COLL])
        .expect("liquidate");
    w.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 1_000_000);
    let a = auction(pos, 0);
    let bid = |with_settlement: bool, coin: u128| plan::BidArgs {
        bidder_stable: id(k::BIDDER_STABLE),
        bidder_collateral: id(k::BIDDER_COLL),
        bidder_collateral_is_fresh: false,
        auction: a,
        coin_in_max: coin,
        min_collateral_out: 0,
        with_settlement,
        oracle: oracle(),
    };
    w.run_plan(
        &plan::bid(&v, &bid(false, 100_000)).expect("plan"),
        &[k::BIDDER_STABLE],
    )
    .expect("partial bid");
    w.run_plan(
        &plan::bid(&v, &bid(true, u128::MAX)).expect("plan"),
        &[k::BIDDER_STABLE],
    )
    .expect("final bid with settlement");
    let st: AuctionState = w.read(a);
    assert!(st.settled && st.leftover_claimable > 0);
    let claim = plan::claim_leftover(&v, id(k::OWNER), id(k::OWNER_COLL), false, a).expect("plan");
    w.run_plan(&claim, &[k::OWNER]).expect("claim");
    assert_eq!(
        w.read::<AuctionState>(a).leftover_claimed,
        st.leftover_claimable
    );
}

#[test]
fn observe_settle_debt_and_check_solvency_via_sdk_plans() {
    let mut w = World::with_engine();
    let v = w.sdk_config();
    for p in [
        plan::observe_feed(&v, id(k::CALLER), oracle()),
        plan::settle_debt(&v, id(k::CALLER), u128::MAX),
        plan::check_solvency(&v, id(k::CALLER)),
    ] {
        w.run_plan(&p.expect("plan"), &[k::CALLER])
            .expect("applies");
    }
    assert!(!w.host_frozen());
}

#[test]
fn settle_empty_via_sdk_plan() {
    let mut w = World::with_engine();
    let pos = w.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 0, 1_000, 1_000);
    w.set_redemption_price(250 * ONE);
    w.set_oracle(250 * ONE, w.now);
    w.liquidate(pos, 0).expect("liquidate");
    let v = w.sdk_config();
    let p = plan::settle_empty(&v, id(k::CALLER), auction(pos, 0)).expect("plan");
    w.run_plan(&p, &[k::CALLER]).expect("settle_empty");
    assert!(w.read::<AuctionState>(auction(pos, 0)).settled);
    assert_err(w.run_plan(&p, &[k::CALLER]), "NotAuctioning");
}

#[test]
fn surplus_auction_via_sdk_plans() {
    let mut w = World::with_system(false);
    w.donate_stable(buffer(), 1_500);
    let v = w.sdk_config();
    w.run_plan(
        &plan::start_surplus(&v, id(k::CALLER), 0).expect("plan"),
        &[k::CALLER],
    )
    .expect("start");
    let q = quote::quote_take_surplus(&WorldChain(&w), &v, 0, 500, w.now).expect("quote");
    let before = w.balance(id(k::TAKER_PROTOCOL));
    let take = plan::TakeArgs {
        payer: id(k::TAKER_PROTOCOL),
        receiver: id(k::TAKER_STABLE),
        receiver_is_fresh: false,
        nonce: 0,
        amount_coin: 500,
        limit: quote::max_in(q.tokens, 50),
    };
    w.run_plan(
        &plan::take_surplus(&v, &take).expect("plan"),
        &[k::TAKER_PROTOCOL],
    )
    .expect("take");
    assert_eq!(
        before - w.balance(id(k::TAKER_PROTOCOL)),
        q.tokens,
        "burned == quote"
    );
    assert_eq!(w.balance(id(k::TAKER_STABLE)), 500);
}

#[test]
fn close_surplus_via_sdk_plan_after_the_ttl() {
    let mut w = World::with_system(false);
    w.donate_stable(buffer(), 1_500);
    let v = w.sdk_config();
    w.run_plan(
        &plan::start_surplus(&v, id(k::CALLER), 0).expect("plan"),
        &[k::CALLER],
    )
    .expect("start");
    let close = plan::close_surplus(&v, id(k::CALLER), 0).expect("plan");
    assert_err(w.run_plan(&close, &[k::CALLER]), "DelayNotElapsed");
    w.advance(v.state.system.system_ttl_ms);
    w.run_plan(&close, &[k::CALLER]).expect("close after ttl");
    assert!(!w.read::<AccountingState>(accounting()).surplus_live);
}

#[test]
fn debt_auction_via_sdk_plans() {
    let mut w = World::with_system(false);
    w.book_shortfall();
    w.fund_stable(k::TAKER_STABLE, 1_000);
    let v = w.sdk_config();
    w.run_plan(
        &plan::start_debt(&v, id(k::CALLER), 0).expect("plan"),
        &[k::CALLER],
    )
    .expect("start");
    let q = quote::quote_take_debt(&WorldChain(&w), &v, 0, 500, w.now).expect("quote");
    let before = w.balance(id(k::TAKER_PROTOCOL));
    let take = plan::TakeArgs {
        payer: id(k::TAKER_STABLE),
        receiver: id(k::TAKER_PROTOCOL),
        receiver_is_fresh: false,
        nonce: 0,
        amount_coin: 500,
        limit: quote::min_out(q.tokens, 50),
    };
    w.run_plan(
        &plan::take_debt(&v, &take).expect("plan"),
        &[k::TAKER_STABLE],
    )
    .expect("take");
    assert_eq!(
        w.balance(id(k::TAKER_PROTOCOL)) - before,
        q.tokens,
        "minted == quote"
    );
    let start_again = plan::start_debt(&w.sdk_config(), id(k::CALLER), 1).expect("plan");
    assert_err(w.run_plan(&start_again, &[k::CALLER]), "DelayNotElapsed");
    w.advance(DEBT_RELIST_DELAY_MS);
    w.run_plan(&start_again, &[k::CALLER])
        .expect("relist after the delay (ADR-039)");
}

#[test]
fn close_debt_via_sdk_plan_after_the_ttl() {
    let mut w = World::with_system(false);
    w.book_shortfall();
    let v = w.sdk_config();
    w.run_plan(
        &plan::start_debt(&v, id(k::CALLER), 0).expect("plan"),
        &[k::CALLER],
    )
    .expect("start");
    let close = plan::close_debt(&v, id(k::CALLER), 0).expect("plan");
    assert_err(w.run_plan(&close, &[k::CALLER]), "DelayNotElapsed");
    w.advance(v.state.system.system_ttl_ms);
    w.run_plan(&close, &[k::CALLER]).expect("close after ttl");
    assert_err(w.run_plan(&close, &[k::CALLER]), "NotLive");
}

#[test]
fn admin_plans_run_on_the_real_guests() {
    let mut w = World::new();
    w.create_protocol_token(10_000, mint_authority())
        .expect("token first (ADR-038)");
    let p = EngineParamsArgs {
        protocol_token_enabled: true,
        ..EngineParamsArgs::defaults_for_tests()
    };
    let host = HostAddresses::new(host_program());
    let init = plan::init_config(
        engine_program(),
        &host,
        id(k::ENGINE_ADMIN),
        SALT,
        &p,
        Some(protocol_token_def()),
        None,
    )
    .expect("plan");
    w.run_plan(&init, &[k::ENGINE_ADMIN]).expect("init_config");
    let bind = plan_host::set_liquidation_engine(
        &host,
        id(k::HOST_ADMIN),
        engine_program(),
        engine_authority(),
    )
    .expect("plan");
    w.run_plan(&bind, &[k::HOST_ADMIN]).expect("bind");
    let v = w.sdk_config();
    let coll = plan::set_collateral_config(
        &v,
        id(k::ENGINE_ADMIN),
        oracle(),
        &CollateralArgs::defaults_for_tests(),
    )
    .expect("plan");
    w.run_plan(&coll, &[k::ENGINE_ADMIN])
        .expect("set_collateral_config");
    let p2 = EngineParamsArgs {
        protocol_token_ref_price: p.protocol_token_ref_price * 3 / 2,
        ..p.clone()
    };
    let set = plan::set_engine_params(
        &v,
        id(k::ENGINE_ADMIN),
        &p2,
        Some(protocol_token_def()),
        None,
    )
    .expect("plan");
    w.run_plan(&set, &[k::ENGINE_ADMIN])
        .expect("set_engine_params");
    assert_eq!(
        w.read::<EngineConfigState>(config())
            .protocol_token_ref_price,
        p2.protocol_token_ref_price
    );
}

#[test]
fn negative_controller_gains_are_encoded_and_accepted_by_the_host() {
    let mut w = World::new();
    let host = HostAddresses::new(host_program());
    let minus_one = -i128::try_from(ONE).expect("RAY fits i128");
    let p = plan_host::set_controller_gains(&host, id(k::HOST_ADMIN), minus_one, -1).expect("plan");
    w.run_plan(&p, &[k::HOST_ADMIN])
        .expect("signed gains accepted");
    let pp: stablecoin_core::ProtocolParameters = w.read(protocol_parameters());
    assert_eq!(
        (pp.controller_proportional_gain, pp.controller_integral_gain),
        (minus_one, -1)
    );
}

#[test]
fn token_plans_create_initialise_and_transfer() {
    let mut w = World::new();
    let token = token_program();
    // A fresh protocol token with its supply in the taker holding (NewFungibleDefinition, enum 1).
    let new = plan_host::token_new_fungible(
        token,
        protocol_token_def(),
        id(k::TAKER_PROTOCOL),
        "MOCK",
        100,
        None,
    )
    .expect("plan");
    w.run_plan(&new, &[k::PROTOCOL_DEF, k::TAKER_PROTOCOL])
        .expect("new fungible definition");
    // InitializeAccount (enum 3) for a second, empty holding of the same definition.
    let init =
        plan_host::token_initialize_account(token, protocol_token_def(), id(k::OWNER2_STABLE))
            .expect("plan");
    w.run_plan(&init, &[k::OWNER2_STABLE])
        .expect("initialize account");
    let transfer =
        plan_host::token_transfer(token, id(k::TAKER_PROTOCOL), id(k::OWNER2_STABLE), 40)
            .expect("plan");
    w.run_plan(&transfer, &[k::TAKER_PROTOCOL])
        .expect("transfer");
    assert_eq!(
        (
            w.balance(id(k::TAKER_PROTOCOL)),
            w.balance(id(k::OWNER2_STABLE))
        ),
        (60, 40)
    );
}
