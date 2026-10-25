//! Every `prepare` action returns exactly the plans, facts and touched accounts of its
//! `auction_sdk::ops::prepare_*` (the function the CLI prints), on the standard fixture.

mod common;

use std::path::PathBuf;

use auction_ffi::{envelope, prepare};
use auction_sdk::decode::Reader;
use auction_sdk::keystore::Keystore;
use auction_sdk::ops::{
    self, Accounts, BidRequest, CallerRequest, ClaimLeftoverRequest, LiquidateRequest,
    OraclePublishRequest, Prepared, SetEngineParamsRequest, SettleDebtRequest, SettleEmptyRequest,
    TakeRequest, TokenTransferRequest, Touch,
};
use auction_sdk::testing::standard::{self, ENGINE, R, T0};
use auction_sdk::verify::VerifiedConfig;
use auction_sdk::SdkResult;
use lee_core::account::AccountId;
use serde_json::{json, Value};

use common::{world, World};

fn touch_json(t: &Touch) -> Value {
    match t {
        Touch::Holding {
            label,
            id,
            definition,
        } => json!({ "kind": "holding", "label": label, "id": id.to_string(),
                     "definition": definition.to_string() }),
        Touch::Auction(id) => json!({ "kind": "auction", "id": id.to_string() }),
        Touch::Accounting => json!({ "kind": "accounting" }),
    }
}

/// What the FFI must return for `p`.
fn expected(p: &Prepared) -> Value {
    json!({
        "steps": p.steps.iter().map(|s| prepare::plan_json(&s.plan, s.what, s.check)).collect::<Vec<_>>(),
        "facts": p.facts.iter().map(|f| json!({ "key": f.key, "value": f.value })).collect::<Vec<_>>(),
        "touched": p.touched.iter().map(touch_json).collect::<Vec<_>>(),
    })
}

/// Runs `f` with the wallet's resolver, as the CLI does.
fn with_accounts<R>(w: &World, f: impl FnOnce(&Accounts<'_>) -> R) -> R {
    let home: PathBuf = w.json.wallet_home.clone().expect("wallet");
    let path = Keystore::default_path(Some(&home)).expect("path");
    let resolve = move |arg: &str| -> SdkResult<AccountId> {
        if let Ok(id) = arg.parse::<AccountId>() {
            return Ok(id);
        }
        Ok(Keystore::open(&path)?.resolve(arg)?)
    };
    f(&Accounts { resolve: &resolve })
}

fn verified(w: &World) -> VerifiedConfig {
    w.ctx.verified().expect("verified")
}

fn ffi(w: &World, req: &Value) -> Value {
    prepare::handle(&w.ctx, req).unwrap_or_else(|e| panic!("{req}: {e:?}"))
}

fn kind_of(r: Result<Value, envelope::FfiError>) -> String {
    let v = envelope::to_value(r);
    assert_eq!(v["ok"], false, "{v}");
    v["error"]["kind"].as_str().expect("kind").to_owned()
}

/// Asserts the FFI's answer to `req` equals `want` (computed by `ops`).
fn same(w: &World, req: &Value, want: SdkResult<Prepared>) {
    let want = want.unwrap_or_else(|e| panic!("ops refused {req}: {e}"));
    assert!(!want.steps.is_empty());
    assert_eq!(ffi(w, req), expected(&want), "{req}");
}

fn caller(c: &str) -> CallerRequest {
    CallerRequest {
        caller: c.to_owned(),
    }
}

/// The fixture with neither a surplus nor a debt auction live.
fn idle_world() -> World {
    let w = world("localnet");
    let v = verified(&w);
    let r = Reader::new(w.chain, ENGINE);
    let mut acc = r.accounting(&v.engine.accounting).expect("accounting");
    acc.surplus_live = false;
    acc.debt_live = false;
    acc.debt_on_auction = 0;
    w.chain.put_state(v.engine.accounting, ENGINE, &acc);
    w
}

#[test]
fn liquidate_plan_equals_ops() {
    let w = world("localnet");
    let fx = standard::standard();
    let pos = fx.unsafe_position.to_string();
    let req = LiquidateRequest {
        position: pos.clone(),
        keeper: "keeper".into(),
        keeper_holding: "keeper-coll".into(),
    };
    let want = with_accounts(&w, |a| {
        ops::prepare_liquidate(w.chain, &verified(&w), &req, a, T0)
    });
    same(
        &w,
        &json!({ "action": "liquidate", "position": pos, "keeper": "keeper", "keeper_holding": "keeper-coll" }),
        want,
    );
}

#[test]
fn liquidate_of_a_safe_position_is_refused() {
    let w = world("localnet");
    let fx = standard::standard();
    let req = json!({ "action": "liquidate", "position": fx.auction_position.to_string(),
                      "keeper": "keeper", "keeper_holding": "keeper-coll" });
    let ffi_kind = kind_of(prepare::handle(&w.ctx, &req));
    let ops_req = LiquidateRequest {
        position: fx.auction_position.to_string(),
        keeper: "keeper".into(),
        keeper_holding: "keeper-coll".into(),
    };
    let ops_err = with_accounts(&w, |a| {
        ops::prepare_liquidate(w.chain, &verified(&w), &ops_req, a, T0)
    })
    .expect_err("ops refuses too");
    assert_eq!(ffi_kind, envelope::kind_of(&ops_err));
}

#[test]
fn bid_plan_equals_ops() {
    let w = world("localnet");
    let fx = standard::standard();
    let auction = fx.live_auction.to_string();
    for (min_out, req_min) in [(None, Value::Null), (Some(50_000u128), json!("50000"))] {
        let req = BidRequest {
            auction: auction.clone(),
            coin_in_max: 100_000,
            bidder_stable: "bidder-stable".into(),
            bidder_collateral: "bidder-coll".into(),
            slippage_bps: 50,
            min_out,
        };
        let want = with_accounts(&w, |a| {
            ops::prepare_bid(w.chain, &verified(&w), &req, a, T0)
        });
        same(
            &w,
            &json!({ "action": "bid", "auction": auction, "coin_in_max": "100000",
                     "bidder_stable": "bidder-stable", "bidder_collateral": "bidder-coll",
                     "slippage_bps": 50, "min_out": req_min }),
            want,
        );
    }
}

#[test]
fn bid_refuses_amounts_that_are_not_decimal_strings() {
    let w = world("localnet");
    let fx = standard::standard();
    let auction = fx.live_auction.to_string();
    for coin in [json!(100_000), json!("1e5"), json!("-1")] {
        let req = json!({ "action": "bid", "auction": auction, "coin_in_max": coin,
                          "bidder_stable": "bidder-stable", "bidder_collateral": "bidder-coll",
                          "slippage_bps": 50 });
        assert_eq!(
            kind_of(prepare::handle(&w.ctx, &req)),
            "bad_request",
            "{req}"
        );
    }
    let too_much_slippage = json!({ "action": "bid", "auction": auction, "coin_in_max": "1",
                          "bidder_stable": "bidder-stable", "bidder_collateral": "bidder-coll",
                          "slippage_bps": 10_000 });
    assert_eq!(
        kind_of(prepare::handle(&w.ctx, &too_much_slippage)),
        "bad_request"
    );
}

#[test]
fn claim_leftover_plan_equals_ops() {
    let w = world("localnet");
    let fx = standard::standard();
    let auction = fx.live_auction.to_string();
    let req = ClaimLeftoverRequest {
        owner: "owner2".into(),
        owner_holding: "owner-coll".into(),
        auction: auction.clone(),
    };
    let want = with_accounts(&w, |a| {
        ops::prepare_claim_leftover(w.chain, &verified(&w), &req, a, T0)
    });
    same(
        &w,
        &json!({ "action": "claim_leftover", "owner": "owner2", "owner_holding": "owner-coll", "auction": auction }),
        want,
    );
}

#[test]
fn settle_empty_plan_equals_ops() {
    let w = world("localnet");
    let fx = standard::standard();
    let auction = fx.live_auction.to_string();
    let req = SettleEmptyRequest {
        caller: "caller".into(),
        auction: auction.clone(),
    };
    let want = with_accounts(&w, |a| {
        ops::prepare_settle_empty(w.chain, &verified(&w), &req, a, T0)
    });
    same(
        &w,
        &json!({ "action": "settle_empty", "caller": "caller", "auction": auction }),
        want,
    );
}

#[test]
fn observe_feed_plan_equals_ops() {
    let w = world("localnet");
    let want = with_accounts(&w, |a| {
        ops::prepare_observe_feed(w.chain, &verified(&w), &caller("caller"), a, T0)
    });
    same(
        &w,
        &json!({ "action": "observe_feed", "caller": "caller" }),
        want,
    );
}

#[test]
fn settle_debt_plan_equals_ops() {
    let w = world("localnet");
    for (amount, req_amount) in [(None, Value::Null), (Some(300u128), json!("300"))] {
        let req = SettleDebtRequest {
            caller: "caller".into(),
            amount,
        };
        let want = with_accounts(&w, |a| {
            ops::prepare_settle_debt(w.chain, &verified(&w), &req, a, T0)
        });
        same(
            &w,
            &json!({ "action": "settle_debt", "caller": "caller", "amount": req_amount }),
            want,
        );
    }
}

#[test]
fn check_solvency_plan_equals_ops() {
    let w = world("localnet");
    let want = with_accounts(&w, |a| {
        ops::prepare_check_solvency(w.chain, &verified(&w), &caller("caller"), a, T0)
    });
    same(
        &w,
        &json!({ "action": "check_solvency", "caller": "caller" }),
        want,
    );
}

#[test]
fn start_surplus_plan_equals_ops() {
    let w = idle_world();
    let want = with_accounts(&w, |a| {
        ops::prepare_start_surplus(w.chain, &verified(&w), &caller("caller"), a, T0)
    });
    same(
        &w,
        &json!({ "action": "start_surplus", "caller": "caller" }),
        want,
    );
}

#[test]
fn start_surplus_while_one_is_live_is_refused() {
    let w = world("localnet");
    assert_eq!(
        kind_of(prepare::handle(
            &w.ctx,
            &json!({ "action": "start_surplus", "caller": "caller" })
        )),
        "refused"
    );
}

fn take(payer: &str, receiver: &str) -> TakeRequest {
    TakeRequest {
        payer: payer.into(),
        receiver: receiver.into(),
        amount: 100,
        slippage_bps: 50,
    }
}

#[test]
fn take_surplus_plan_equals_ops() {
    let w = world("localnet");
    let req = take("taker-protocol", "taker-stable");
    let want = with_accounts(&w, |a| {
        ops::prepare_take_surplus(w.chain, &verified(&w), &req, a, T0)
    });
    same(
        &w,
        &json!({ "action": "take_surplus", "payer": "taker-protocol", "receiver": "taker-stable",
                 "amount": "100", "slippage_bps": 50 }),
        want,
    );
}

#[test]
fn close_surplus_plan_equals_ops() {
    let w = world("localnet");
    let want = with_accounts(&w, |a| {
        ops::prepare_close_surplus(w.chain, &verified(&w), &caller("caller"), a, T0)
    });
    same(
        &w,
        &json!({ "action": "close_surplus", "caller": "caller" }),
        want,
    );
}

#[test]
fn start_debt_plan_equals_ops() {
    let w = idle_world();
    let want = with_accounts(&w, |a| {
        ops::prepare_start_debt(w.chain, &verified(&w), &caller("caller"), a, T0)
    });
    same(
        &w,
        &json!({ "action": "start_debt", "caller": "caller" }),
        want,
    );
}

#[test]
fn take_debt_plan_equals_ops() {
    let w = world("localnet");
    let req = take("taker-stable", "taker-protocol");
    let want = with_accounts(&w, |a| {
        ops::prepare_take_debt(w.chain, &verified(&w), &req, a, T0)
    });
    same(
        &w,
        &json!({ "action": "take_debt", "payer": "taker-stable", "receiver": "taker-protocol",
                 "amount": "100", "slippage_bps": 50 }),
        want,
    );
}

#[test]
fn close_debt_plan_equals_ops() {
    let w = world("localnet");
    let want = with_accounts(&w, |a| {
        ops::prepare_close_debt(w.chain, &verified(&w), &caller("caller"), a, T0)
    });
    same(
        &w,
        &json!({ "action": "close_debt", "caller": "caller" }),
        want,
    );
}

#[test]
fn token_transfer_plan_equals_ops() {
    let w = world("localnet");
    let req = TokenTransferRequest {
        from: "owner".into(),
        to: "keeper".into(),
        amount: 1500,
    };
    let want = with_accounts(&w, |a| {
        ops::prepare_token_transfer(&w.ctx.deployment, &req, a)
    });
    same(
        &w,
        &json!({ "action": "token_transfer", "from": "owner", "to": "keeper", "amount": "1500" }),
        want,
    );
}

fn publish_request(price: &str) -> Value {
    json!({ "action": "oracle_publish", "price_source": "caller", "price": price,
            "host_admin": "host-admin", "engine_admin": "engine-admin" })
}

#[test]
fn oracle_publish_plan_equals_ops() {
    let w = world("localnet");
    let req = OraclePublishRequest {
        price_source: "caller".into(),
        price: R,
        host_admin: "host-admin".into(),
        engine_admin: "engine-admin".into(),
        base_window_ms: 3_600_000,
    };
    let want = with_accounts(&w, |a| {
        ops::prepare_oracle_publish(w.chain, &w.ctx.deployment, &verified(&w), &req, a, T0)
    });
    assert_eq!(want.as_ref().expect("ops").steps.len(), 3);
    same(&w, &publish_request(&R.to_string()), want);
    let mut with_window = publish_request(&R.to_string());
    with_window["base_window_ms"] = json!(3_600_000);
    assert_eq!(
        ffi(&w, &with_window),
        ffi(&w, &publish_request(&R.to_string()))
    );
}

#[test]
fn oracle_publish_is_refused_off_dev_networks_and_out_of_range() {
    let w = world("mainnet");
    assert_eq!(
        kind_of(prepare::handle(&w.ctx, &publish_request(&R.to_string()))),
        "mismatch"
    );
    let w = world("localnet");
    assert_eq!(
        kind_of(prepare::handle(
            &w.ctx,
            &publish_request(&(3 * R).to_string())
        )),
        "refused"
    );
}

#[test]
fn disable_protocol_token_plan_equals_the_cli_demo_step() {
    let w = world("localnet");
    // scripts/lib/demo-scenario.sh: `admin set-engine-params --admin engine-admin
    // --params cli/params/demo-engine.json --disable-protocol-token`.
    let params =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cli/params/demo-engine.json");
    let req = SetEngineParamsRequest {
        admin: "engine-admin".into(),
        params,
        protocol_token: None,
        ref_oracle: None,
        disable_protocol_token: true,
        disable_ref_oracle: false,
    };
    let want = with_accounts(&w, |a| {
        ops::prepare_set_engine_params(w.chain, &verified(&w), &req, a, T0)
    });
    assert!(!want.as_ref().expect("ops").facts.is_empty());
    same(
        &w,
        &json!({ "action": "disable_protocol_token", "admin": "engine-admin" }),
        want,
    );
}

#[test]
fn disable_protocol_token_never_disables_a_ref_oracle_silently() {
    let w = world("localnet");
    let v = verified(&w);
    let r = Reader::new(w.chain, ENGINE);
    let mut cfg = r.config(&v.config_id).expect("config");
    cfg.protocol_token_oracle_id = Some(standard::ORACLE);
    w.chain.put_state(v.config_id, ENGINE, &cfg);
    let v = envelope::to_value(prepare::handle(
        &w.ctx,
        &json!({ "action": "disable_protocol_token", "admin": "engine-admin" }),
    ));
    assert_eq!(v["error"]["kind"], "refused", "{v}");
    let message = v["error"]["message"].as_str().expect("message");
    assert!(message.contains("ref oracle"), "{message}");
}

#[test]
fn disable_protocol_token_by_another_signer_is_refused() {
    let w = world("localnet");
    assert_eq!(
        kind_of(prepare::handle(
            &w.ctx,
            &json!({ "action": "disable_protocol_token", "admin": "caller" })
        )),
        "mismatch"
    );
}

#[test]
fn unknown_action_or_missing_field_is_a_bad_request() {
    let w = world("localnet");
    for req in [
        json!({ "action": "swap" }),
        json!({ "action": "observe_feed" }),
        json!({ "action": "take_debt", "payer": "a", "receiver": "b" }),
        json!({}),
    ] {
        assert_eq!(
            kind_of(prepare::handle(&w.ctx, &req)),
            "bad_request",
            "{req}"
        );
    }
}
