//! `ops` against the standard fixture: each `prepare_*` builds the plan the CLI built before the move
//! (same fixture input) and the facts its golden prints before `plan`.

use std::path::PathBuf;

use auction_engine_core::state::AccountingState;
use lee_core::account::{Account, AccountId};

use super::*;
use crate::{
    chain::clock_ms,
    decode::Reader,
    error::SdkError,
    oracle, params,
    plan::{self, BidArgs, LiquidateArgs, Plan, TakeArgs},
    plan_host, quote,
    testing::{
        id,
        standard::{self, k},
        STANDARD_LABELS,
    },
    verify::{verify_config, VerifiedConfig},
};

struct Setup {
    fx: standard::Fixture,
    v: VerifiedConfig,
    now: u64,
}

fn setup() -> Setup {
    let fx = standard::standard();
    let v = verify_config(&fx.chain, &fx.deployment, &standard::config_id()).expect("verifies");
    let now = clock_ms(&fx.chain).expect("clock");
    Setup { fx, v, now }
}

/// What the CLI's keystore resolves for the standard wallet.
fn resolve(s: &str) -> SdkResult<AccountId> {
    if let Ok(i) = s.parse::<AccountId>() {
        return Ok(i);
    }
    STANDARD_LABELS
        .iter()
        .find(|(l, _)| *l == s)
        .map(|(_, n)| id(*n))
        .ok_or_else(|| SdkError::Chain(format!("no label {s}")))
}

fn accounts() -> Accounts<'static> {
    Accounts { resolve: &resolve }
}

fn fresh(s: &Setup, i: AccountId) -> bool {
    s.fx.chain.account(&i).expect("account") == Account::default()
}

fn facts(p: &Prepared) -> Vec<(&'static str, String)> {
    p.facts.iter().map(|f| (f.key, f.value.clone())).collect()
}

fn single(p: &Prepared) -> &Step {
    assert_eq!(p.steps.len(), 1, "one step");
    &p.steps[0]
}

fn oracle_id(s: &Setup) -> AccountId {
    s.v.coll(&s.fx.chain).expect("coll").oracle_id
}

fn engine_params_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cli/params/localnet-engine.json")
}

fn caller() -> CallerRequest {
    CallerRequest {
        caller: "caller".into(),
    }
}

fn set_accounting(s: &Setup, f: impl FnOnce(&mut AccountingState)) {
    let r = Reader::new(&s.fx.chain, s.v.engine.program);
    let mut acc = r.accounting(&s.v.engine.accounting).expect("accounting");
    f(&mut acc);
    s.fx.chain
        .put_state(s.v.engine.accounting, standard::ENGINE, &acc);
}

fn refused(r: SdkResult<Prepared>) -> String {
    match r {
        Err(SdkError::Refused(m)) => m,
        Err(e) => panic!("expected a refusal, got {e:?}"),
        Ok(_) => panic!("expected a refusal, got a plan"),
    }
}

#[test]
fn prepare_liquidate_matches_the_cli_plan_and_facts() {
    let s = setup();
    let req = LiquidateRequest {
        position: s.fx.unsafe_position.to_string(),
        keeper: "keeper".into(),
        keeper_holding: "keeper-coll".into(),
    };
    let p = prepare_liquidate(&s.fx.chain, &s.v, &req, &accounts(), s.now).expect("prepares");
    let holding = id(k::KEEPER_COLL);
    let q = quote::quote_liquidation(&s.fx.chain, &s.v, &s.fx.unsafe_position, s.now).expect("q");
    let expected = plan::liquidate(
        &s.v,
        &LiquidateArgs {
            keeper: id(k::KEEPER),
            keeper_collateral_holding: holding,
            keeper_holding_is_fresh: fresh(&s, holding),
            position: s.fx.unsafe_position,
            nonce: q.nonce,
            oracle: oracle_id(&s),
        },
    )
    .expect("plan");
    let step = single(&p);
    assert_eq!(step.plan, expected);
    assert_eq!((step.what, step.check), ("liquidate", CHECK_AUCTIONS));
    assert_eq!(
        facts(&p),
        vec![
            ("nonce", "0".to_owned()),
            (
                "auction",
                "7hT7bjKCoiP2fsCtwpHUUt8qDVDPVGHmeQKWTH1EzQTn".to_owned()
            ),
            ("keeper reward", "25".to_owned()),
        ]
    );
    assert_eq!(
        p.touched,
        vec![
            Touch::Auction(q.auction),
            Touch::Holding {
                label: "keeper".into(),
                id: holding,
                definition: s.v.state.collateral_definition_id,
            },
        ]
    );
}

#[test]
fn prepare_liquidate_refuses_what_the_host_would_reject() {
    let s = setup();
    let r = Reader::new(&s.fx.chain, standard::ENGINE);
    let mut pp: stablecoin_core::ProtocolParameters = r
        .host(
            &s.fx.host.protocol_parameters,
            &standard::HOST,
            "ProtocolParameters",
        )
        .expect("pp");
    pp.minimum_collateralization_ratio = standard::ONE * 3 / 2;
    s.fx.chain
        .put_state(s.fx.host.protocol_parameters, standard::HOST, &pp);
    let req = LiquidateRequest {
        position: s.fx.unsafe_position.to_string(),
        keeper: "keeper".into(),
        keeper_holding: "keeper-coll".into(),
    };
    let m = refused(prepare_liquidate(
        &s.fx.chain,
        &s.v,
        &req,
        &accounts(),
        s.now,
    ));
    assert!(
        m.starts_with("refusing before signing: the host would reject this liquidation ("),
        "{m}"
    );
}

fn bid_req(s: &Setup, coin: u128, min_out: Option<u128>) -> BidRequest {
    BidRequest {
        auction: s.fx.live_auction.to_string(),
        coin_in_max: coin,
        bidder_stable: "bidder-stable".into(),
        bidder_collateral: "bidder-coll".into(),
        slippage_bps: 50,
        min_out,
    }
}

fn expected_bid(s: &Setup, coin: u128, min_out: u128, with_settlement: bool) -> Plan {
    let coll = id(k::BIDDER_COLL);
    plan::bid(
        &s.v,
        &BidArgs {
            bidder_stable: id(k::BIDDER_STABLE),
            bidder_collateral: coll,
            bidder_collateral_is_fresh: fresh(s, coll),
            auction: s.fx.live_auction,
            coin_in_max: coin,
            min_collateral_out: min_out,
            with_settlement,
            oracle: oracle_id(s),
        },
    )
    .expect("plan")
}

#[test]
fn prepare_bid_final_is_bid_with_settlement() {
    let s = setup();
    let p = prepare_bid(
        &s.fx.chain,
        &s.v,
        &bid_req(&s, 660_000, None),
        &accounts(),
        s.now,
    )
    .expect("prepares");
    let step = single(&p);
    assert_eq!(step.plan, expected_bid(&s, 660_000, 341_241, true));
    assert_eq!(
        (step.what, step.check),
        ("bid_with_settlement", CHECK_AUCTIONS)
    );
    assert_eq!(
        facts(&p),
        vec![
            ("quoted out", "342956".to_owned()),
            ("min out", "341241".to_owned())
        ]
    );
    assert_eq!(
        p.touched,
        vec![
            Touch::Auction(s.fx.live_auction),
            Touch::Holding {
                label: "bidder coll".into(),
                id: id(k::BIDDER_COLL),
                definition: s.v.state.collateral_definition_id,
            },
            Touch::Holding {
                label: "bidder stable".into(),
                id: id(k::BIDDER_STABLE),
                definition: s.v.state.stablecoin_definition_id,
            },
        ]
    );
}

#[test]
fn prepare_bid_partial_is_a_plain_bid() {
    let s = setup();
    let p = prepare_bid(
        &s.fx.chain,
        &s.v,
        &bid_req(&s, 100_000, None),
        &accounts(),
        s.now,
    )
    .expect("prepares");
    let step = single(&p);
    assert_eq!(step.plan, expected_bid(&s, 100_000, 51_703, false));
    assert_eq!(step.what, "bid");
    assert_eq!(
        facts(&p),
        vec![
            ("quoted out", "51963".to_owned()),
            ("min out", "51703".to_owned())
        ]
    );
}

#[test]
fn prepare_bid_requotes_an_explicit_min_out() {
    let s = setup();
    let p = prepare_bid(
        &s.fx.chain,
        &s.v,
        &bid_req(&s, 100_000, Some(51_000)),
        &accounts(),
        s.now,
    )
    .expect("a bound below the quote");
    assert_eq!(single(&p).plan, expected_bid(&s, 100_000, 51_000, false));
    let above = prepare_bid(
        &s.fx.chain,
        &s.v,
        &bid_req(&s, 100_000, Some(51_964)),
        &accounts(),
        s.now,
    );
    assert!(
        matches!(&above, Err(SdkError::Engine { name, .. }) if name.contains("Slippage")),
        "{above:?}"
    );
}

#[test]
fn prepare_claim_leftover_matches_the_cli_plan() {
    let s = setup();
    let req = ClaimLeftoverRequest {
        owner: "owner2".into(),
        owner_holding: "owner-coll".into(),
        auction: s.fx.live_auction.to_string(),
    };
    let p = prepare_claim_leftover(&s.fx.chain, &s.v, &req, &accounts(), s.now).expect("ok");
    let holding = id(k::OWNER_COLL);
    let expected = plan::claim_leftover(
        &s.v,
        id(k::OWNER2),
        holding,
        fresh(&s, holding),
        s.fx.live_auction,
    )
    .expect("plan");
    let step = single(&p);
    assert_eq!(step.plan, expected);
    assert_eq!((step.what, step.check), ("claim_leftover", CHECK_AUCTIONS));
    assert!(p.facts.is_empty());
    assert_eq!(
        p.touched,
        vec![
            Touch::Auction(s.fx.live_auction),
            Touch::Holding {
                label: "owner".into(),
                id: holding,
                definition: s.v.state.collateral_definition_id,
            },
        ]
    );
}

#[test]
fn prepare_settle_empty_matches_the_cli_plan() {
    let s = setup();
    let req = SettleEmptyRequest {
        caller: "caller".into(),
        auction: s.fx.live_auction.to_string(),
    };
    let p = prepare_settle_empty(&s.fx.chain, &s.v, &req, &accounts(), s.now).expect("ok");
    let expected = plan::settle_empty(&s.v, id(k::CALLER), s.fx.live_auction).expect("plan");
    assert_eq!(single(&p).plan, expected);
    assert_eq!(single(&p).what, "settle_empty");
    assert_eq!(
        p.touched,
        vec![Touch::Auction(s.fx.live_auction), Touch::Accounting]
    );
}

#[test]
fn prepare_settle_debt_defaults_to_as_much_as_possible() {
    let s = setup();
    let req = SettleDebtRequest {
        caller: "caller".into(),
        amount: None,
    };
    let p = prepare_settle_debt(&s.fx.chain, &s.v, &req, &accounts(), s.now).expect("ok");
    let expected = plan::settle_debt(&s.v, id(k::CALLER), u128::MAX).expect("plan");
    assert_eq!(single(&p).plan, expected);
    assert_eq!(
        (single(&p).what, single(&p).check),
        ("settle_debt", CHECK_CONFIG)
    );
    assert_eq!(p.touched, vec![Touch::Accounting]);
}

#[test]
fn prepare_check_solvency_matches_the_cli_plan() {
    let s = setup();
    let p = prepare_check_solvency(&s.fx.chain, &s.v, &caller(), &accounts(), s.now).expect("ok");
    let expected = plan::check_solvency(&s.v, id(k::CALLER)).expect("plan");
    assert_eq!(single(&p).plan, expected);
    assert_eq!(single(&p).what, "check_solvency");
    assert_eq!(p.touched, vec![Touch::Accounting]);
}

#[test]
fn prepare_observe_feed_touches_nothing() {
    let s = setup();
    let p = prepare_observe_feed(&s.fx.chain, &s.v, &caller(), &accounts(), s.now).expect("ok");
    let expected = plan::observe_feed(&s.v, id(k::CALLER), oracle_id(&s)).expect("plan");
    assert_eq!(single(&p).plan, expected);
    assert_eq!(
        (single(&p).what, single(&p).check),
        ("observe_feed", CHECK_CONFIG)
    );
    assert!(p.touched.is_empty() && p.facts.is_empty());
}

#[test]
fn prepare_start_surplus_refuses_while_live() {
    let s = setup();
    let m = refused(prepare_start_surplus(
        &s.fx.chain,
        &s.v,
        &caller(),
        &accounts(),
        s.now,
    ));
    assert_eq!(
        m,
        "a surplus auction is live (nonce 0); take it, or close it after its TTL, first"
    );
    set_accounting(&s, |a| a.surplus_live = false);
    let p = prepare_start_surplus(&s.fx.chain, &s.v, &caller(), &accounts(), s.now).expect("ok");
    let expected = plan::start_surplus(&s.v, id(k::CALLER), 1).expect("plan");
    assert_eq!(single(&p).plan, expected);
    assert_eq!(single(&p).what, "start_surplus_auction");
    assert_eq!(p.touched, vec![Touch::Accounting]);
}

fn take_req(payer: &str, receiver: &str) -> TakeRequest {
    TakeRequest {
        payer: payer.into(),
        receiver: receiver.into(),
        amount: 500,
        slippage_bps: 50,
    }
}

#[test]
fn prepare_take_surplus_matches_the_cli_plan_and_facts() {
    let s = setup();
    let req = take_req("taker-protocol", "taker-stable");
    let p = prepare_take_surplus(&s.fx.chain, &s.v, &req, &accounts(), s.now).expect("ok");
    let receiver = id(k::TAKER_STABLE);
    let expected = plan::take_surplus(
        &s.v,
        &TakeArgs {
            payer: id(k::TAKER_PROTOCOL),
            receiver,
            receiver_is_fresh: fresh(&s, receiver),
            nonce: 0,
            amount_coin: 500,
            limit: 201,
        },
    )
    .expect("plan");
    assert_eq!(single(&p).plan, expected);
    assert_eq!(
        (single(&p).what, single(&p).check),
        ("take_surplus", CHECK_CONFIG)
    );
    assert_eq!(
        facts(&p),
        vec![
            ("quoted tokens", "200".to_owned()),
            ("max protocol in", "201".to_owned())
        ]
    );
    assert_eq!(
        p.touched,
        vec![
            Touch::Accounting,
            Touch::Holding {
                label: "payer".into(),
                id: id(k::TAKER_PROTOCOL),
                definition: standard::PROTOCOL,
            },
            Touch::Holding {
                label: "receiver".into(),
                id: receiver,
                definition: s.v.state.stablecoin_definition_id,
            },
        ]
    );
}

#[test]
fn prepare_close_surplus_closes_the_latest() {
    let s = setup();
    let p = prepare_close_surplus(&s.fx.chain, &s.v, &caller(), &accounts(), s.now).expect("ok");
    let expected = plan::close_surplus(&s.v, id(k::CALLER), 0).expect("plan");
    assert_eq!(single(&p).plan, expected);
    assert_eq!(single(&p).what, "close_surplus_auction");
}

#[test]
fn prepare_start_debt_refuses_while_live_and_inside_the_relist_delay() {
    let s = setup();
    let m = refused(prepare_start_debt(
        &s.fx.chain,
        &s.v,
        &caller(),
        &accounts(),
        s.now,
    ));
    assert_eq!(
        m,
        "a debt auction is live (nonce 0); take it, or close it after its TTL, first"
    );
    set_accounting(&s, |a| {
        a.debt_live = false;
        a.last_debt_end_ms = s.now;
    });
    let earliest = s.now + auction_engine_core::args::DEBT_RELIST_DELAY_MS;
    let m = refused(prepare_start_debt(
        &s.fx.chain,
        &s.v,
        &caller(),
        &accounts(),
        s.now,
    ));
    assert_eq!(
        m,
        format!("relist delay (ADR-039): the next debt auction may start at {earliest} ms")
    );
    let p = prepare_start_debt(&s.fx.chain, &s.v, &caller(), &accounts(), earliest)
        .expect("after the delay");
    let expected = plan::start_debt(&s.v, id(k::CALLER), 1).expect("plan");
    assert_eq!(single(&p).plan, expected);
    assert_eq!(single(&p).what, "start_debt_auction");
}

#[test]
fn prepare_take_debt_matches_the_cli_plan_and_facts() {
    let s = setup();
    let req = take_req("taker-stable", "taker-protocol");
    let p = prepare_take_debt(&s.fx.chain, &s.v, &req, &accounts(), s.now).expect("ok");
    let receiver = id(k::TAKER_PROTOCOL);
    let expected = plan::take_debt(
        &s.v,
        &TakeArgs {
            payer: id(k::TAKER_STABLE),
            receiver,
            receiver_is_fresh: fresh(&s, receiver),
            nonce: 0,
            amount_coin: 500,
            limit: 323,
        },
    )
    .expect("plan");
    assert_eq!(single(&p).plan, expected);
    assert_eq!(single(&p).what, "take_debt");
    assert_eq!(
        facts(&p),
        vec![
            ("quoted tokens", "325".to_owned()),
            ("min protocol out", "323".to_owned())
        ]
    );
    assert_eq!(
        p.touched[1],
        Touch::Holding {
            label: "payer".into(),
            id: id(k::TAKER_STABLE),
            definition: s.v.state.stablecoin_definition_id,
        }
    );
}

#[test]
fn prepare_close_debt_closes_the_latest() {
    let s = setup();
    let p = prepare_close_debt(&s.fx.chain, &s.v, &caller(), &accounts(), s.now).expect("ok");
    let expected = plan::close_debt(&s.v, id(k::CALLER), 0).expect("plan");
    assert_eq!(single(&p).plan, expected);
    assert_eq!(single(&p).what, "close_debt_auction");
}

fn engine_params_req(protocol_token: Option<AccountId>, disable: bool) -> SetEngineParamsRequest {
    SetEngineParamsRequest {
        admin: "engine-admin".into(),
        params: engine_params_file(),
        protocol_token: protocol_token.map(|t| t.to_string()),
        ref_oracle: None,
        disable_protocol_token: disable,
        disable_ref_oracle: false,
    }
}

#[test]
fn prepare_set_engine_params_matches_the_cli_plan() {
    let s = setup();
    let req = engine_params_req(Some(standard::PROTOCOL), false);
    let p = prepare_set_engine_params(&s.fx.chain, &s.v, &req, &accounts(), s.now).expect("ok");
    let file = params::engine_params(&engine_params_file(), true, false).expect("file");
    let expected = plan::set_engine_params(
        &s.v,
        id(k::ENGINE_ADMIN),
        &file,
        Some(standard::PROTOCOL),
        None,
    )
    .expect("plan");
    assert_eq!(single(&p).plan, expected);
    assert_eq!(
        (single(&p).what, single(&p).check),
        ("set_engine_params", CHECK_VERIFY)
    );
    assert!(p.facts.is_empty(), "nothing changes: {:?}", facts(&p));
}

#[test]
fn prepare_set_engine_params_disables_the_protocol_token_only_when_asked() {
    let s = setup();
    let m = refused(prepare_set_engine_params(
        &s.fx.chain,
        &s.v,
        &engine_params_req(None, false),
        &accounts(),
        s.now,
    ));
    assert!(m.contains("would disable the protocol token"), "{m}");
    let p = prepare_set_engine_params(
        &s.fx.chain,
        &s.v,
        &engine_params_req(None, true),
        &accounts(),
        s.now,
    )
    .expect("disabled on request");
    assert_eq!(
        facts(&p),
        vec![("protocol token", format!("{} -> none", standard::PROTOCOL))]
    );
}

#[test]
fn prepare_set_engine_params_refuses_a_signer_that_is_not_the_admin() {
    let s = setup();
    let req = SetEngineParamsRequest {
        admin: "keeper".into(),
        ..engine_params_req(Some(standard::PROTOCOL), false)
    };
    let r = prepare_set_engine_params(&s.fx.chain, &s.v, &req, &accounts(), s.now);
    assert!(matches!(r, Err(SdkError::Mismatch { .. })), "{:?}", r.err());
}

#[test]
fn prepare_token_transfer_matches_the_cli_plan() {
    let s = setup();
    let req = TokenTransferRequest {
        from: "owner-stable".into(),
        to: "taker-stable".into(),
        amount: 1,
    };
    let p = prepare_token_transfer(&s.fx.deployment, &req, &accounts()).expect("ok");
    let expected =
        plan_host::token_transfer(standard::TOKEN, id(k::OWNER_STABLE), id(k::TAKER_STABLE), 1)
            .expect("plan");
    assert_eq!(single(&p).plan, expected);
    assert_eq!(
        (single(&p).what, single(&p).check),
        ("transfer", CHECK_BALANCE)
    );
}

#[test]
fn require_dev_network_refuses_a_network_that_is_not_a_dev_network() {
    let s = setup();
    let e = require_dev_network(&s.fx.chain, &s.fx.deployment).expect_err("fixture network");
    assert!(
        e.to_string()
            .starts_with("dev commands that send are for the named dev networks only"),
        "{e}"
    );
}

#[test]
fn prepare_oracle_publish_has_three_steps_with_three_signers() {
    let s = setup();
    let price = standard::R * 19 / 10;
    let req = OraclePublishRequest {
        price_source: "caller".into(),
        price,
        host_admin: "host-admin".into(),
        engine_admin: "engine-admin".into(),
        base_window_ms: 3_600_000,
    };
    let p = prepare_oracle_publish(
        &s.fx.chain,
        &s.fx.deployment,
        &s.v,
        &req,
        &accounts(),
        s.now,
    )
    .expect("prepares");
    let whats: Vec<_> = p.steps.iter().map(|st| (st.what, st.check)).collect();
    assert_eq!(
        whats,
        vec![
            ("create_oracle_price_account", CHECK_HOST),
            ("set_market_price_oracle", CHECK_HOST),
            ("set_collateral_config", CHECK_HOST),
        ]
    );
    let signers: Vec<_> = p
        .steps
        .iter()
        .map(|st| st.plan.signing_requirements.clone())
        .collect();
    assert_eq!(
        signers,
        vec![
            vec![id(k::CALLER)],
            vec![id(k::HOST_ADMIN)],
            vec![id(k::ENGINE_ADMIN)]
        ]
    );
    let r = Reader::new(&s.fx.chain, standard::ENGINE);
    let pp: stablecoin_core::ProtocolParameters = r
        .host(
            &s.fx.host.protocol_parameters,
            &standard::HOST,
            "ProtocolParameters",
        )
        .expect("pp");
    let (create, new_oracle) = plan_host::create_oracle_price_account(
        standard::TWAP,
        id(k::CALLER),
        pp.stablecoin_definition_id,
        pp.collateral_definition_id,
        price,
        3_600_000,
    )
    .expect("create");
    let coll = s.v.coll(&s.fx.chain).expect("coll");
    let follow = plan::set_collateral_config(
        &s.v,
        id(k::ENGINE_ADMIN),
        new_oracle,
        &oracle::collateral_args_from_state(&coll).expect("args"),
    )
    .expect("follow");
    let rotate =
        plan_host::set_market_price_oracle(&s.fx.host, id(k::HOST_ADMIN), new_oracle).expect("r");
    assert_eq!(p.steps[0].plan, create);
    assert_eq!(p.steps[1].plan, rotate);
    assert_eq!(p.steps[2].plan, follow);
    assert_eq!(
        facts(&p),
        vec![
            (
                "price",
                "500000000000000000000000000 -> 950000000000000000000000000".to_owned()
            ),
            (
                "new oracle",
                "6z4Dr9NQZ9pd6s7ABGeHQvmMq4oimiqgqYRnTMqKSmyL".to_owned()
            ),
            ("window ms", "3600000".to_owned()),
        ]
    );
}

#[test]
fn prepare_oracle_publish_refuses_an_invalid_price() {
    let s = setup();
    let req = OraclePublishRequest {
        price_source: "caller".into(),
        price: standard::R * 3,
        host_admin: "host-admin".into(),
        engine_admin: "engine-admin".into(),
        base_window_ms: 3_600_000,
    };
    let m = refused(prepare_oracle_publish(
        &s.fx.chain,
        &s.fx.deployment,
        &s.v,
        &req,
        &accounts(),
        s.now,
    ));
    assert!(m.contains("PriceFeedOutOfRange"), "{m}");
}
