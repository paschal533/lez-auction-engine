mod common;

use auction_ffi::ctx::Ctx;
use auction_ffi::{envelope, view};
use auction_sdk::decode::Reader;
use auction_sdk::quote;
use auction_sdk::testing::standard::{self, ENGINE, R, T0};
use serde_json::{json, Value};

use common::{world, World};

fn run(w: &World, req: Value) -> Result<Value, envelope::FfiError> {
    view::dispatch(&w.ctx, &req)
}

fn ok(w: &World, req: Value) -> Value {
    run(w, req.clone()).unwrap_or_else(|e| panic!("{req}: {e:?}"))
}

fn kind_of(r: Result<Value, envelope::FfiError>) -> String {
    let v = envelope::to_value(r);
    assert_eq!(v["ok"], false, "{v}");
    v["error"]["kind"].as_str().expect("kind").to_owned()
}

/// Scans the whole fixture chain (3 blocks) into the cursor under `state_dir`.
fn scanned(network: &str) -> World {
    let w = world(network);
    let v = ok(&w, json!({ "view": "scan", "max_blocks": 100 }));
    assert_eq!(v["done"], true, "{v}");
    w
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or_else(|| panic!("not a string: {v}"))
}

#[test]
fn deployment_view_of_a_dev_network_on_the_recorded_chain() {
    let w = world("localnet");
    let v = view::deployment(&w.ctx).expect("view");
    assert_eq!(v["network"], "localnet");
    assert_eq!(v["dev"], true);
    assert_eq!(v["genesis_ok"], true);
    assert_eq!(
        v["config"],
        w.ctx.deployment.config.expect("config").to_string()
    );
    assert_eq!(ok(&w, json!({ "view": "deployment" })), v);
}

#[test]
fn deployment_view_of_a_non_dev_network_is_not_dev() {
    let w = world("mainnet");
    let v = view::deployment(&w.ctx).expect("view");
    assert_eq!(v["dev"], false);
    assert_eq!(v["genesis_ok"], true);
}

#[test]
fn dev_is_false_without_a_genesis_hash() {
    let mut w = world("localnet");
    let mut d = w.ctx.deployment.clone();
    d.chain_genesis = None;
    d.config = None;
    w.ctx = Ctx::with_chain(Box::new(w.chain), d, &w.json).expect("ctx");
    let v = view::deployment(&w.ctx).expect("view");
    assert_eq!(v["dev"], false);
    assert_eq!(v["genesis_ok"], false);
    assert_eq!(v["config"], json!(null));
}

#[test]
fn deployment_view_on_another_chain_is_not_dev() {
    let mut w = world("testnet-legacy");
    let mut d = w.ctx.deployment.clone();
    d.chain_genesis = Some("00".repeat(32));
    w.ctx = Ctx::with_chain(Box::new(w.chain), d, &w.json).expect("ctx");
    let v = view::deployment(&w.ctx).expect("view");
    assert_eq!(v["dev"], false);
    assert_eq!(v["genesis_ok"], false);
}

#[test]
fn unknown_or_missing_view_is_a_bad_request() {
    let w = world("localnet");
    assert_eq!(kind_of(run(&w, json!({ "view": "nope" }))), "bad_request");
    assert_eq!(kind_of(run(&w, json!({}))), "bad_request");
    assert_eq!(
        kind_of(view::handle(json!({ "view": "nope" }))),
        "bad_request"
    );
}

#[test]
fn overview_view_reads_the_fixture() {
    let w = scanned("localnet");
    let v = ok(&w, json!({ "view": "overview" }));
    assert_eq!(v["block"], 3);
    let a = &v["accounting"];
    assert_eq!(a["buffer"], "2000");
    assert_eq!(a["unbacked_debt"], "1000");
    assert_eq!(a["debt_on_auction"], "500");
    assert_eq!(a["surplus_live"], true);
    assert_eq!(v["host"]["frozen"], false);
    assert_eq!(v["oracle"]["price_ray"], R.to_string());
    assert_eq!(v["oracle"]["fresh"], true);
    assert_eq!(
        v["counts"],
        json!({ "live": 1, "at_risk": 1, "settled": 0 })
    );
}

#[test]
fn overview_before_any_scan_counts_nothing() {
    let w = world("localnet");
    let v = ok(&w, json!({ "view": "overview" }));
    assert_eq!(
        v["counts"],
        json!({ "live": 0, "at_risk": 0, "settled": 0 })
    );
}

#[test]
fn scan_view_takes_one_page_and_saves_the_cursor() {
    let w = world("localnet");
    let first = ok(&w, json!({ "view": "scan", "max_blocks": 1 }));
    assert_eq!(
        first,
        json!({ "scanned_to": 1, "head": 3, "done": false, "restarted": false, "note": null })
    );
    let cursor = w.json.state_dir.join(format!(
        "{}.cursor.json",
        w.ctx.deployment.config.expect("config")
    ));
    assert!(cursor.exists(), "{}", cursor.display());
    // The largest page the cap allows.
    let second = ok(&w, json!({ "view": "scan", "max_blocks": 500 }));
    assert_eq!(second["scanned_to"], 3);
    assert_eq!(second["done"], true);
}

#[test]
fn scan_view_without_max_blocks_is_a_bad_request() {
    let w = world("localnet");
    for req in [
        json!({ "view": "scan" }),
        json!({ "view": "scan", "max_blocks": 0 }),
        json!({ "view": "scan", "max_blocks": "5" }),
        json!({ "view": "scan", "max_blocks": -1 }),
        json!({ "view": "scan", "max_blocks": 501 }),
        json!({ "view": "scan", "max_blocks": u64::MAX }),
    ] {
        assert_eq!(kind_of(run(&w, req.clone())), "bad_request", "{req}");
    }
}

#[test]
fn live_view_lists_the_live_auction() {
    let w = scanned("localnet");
    let fx = standard::standard();
    let v = ok(&w, json!({ "view": "live" }));
    let items = v["items"].as_array().expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], fx.live_auction.to_string());
    assert_eq!(items[0]["lot"], "475000");
    assert_eq!(items[0]["ends_at_ms"], json!(null));
    assert_eq!(v["skipped"], json!([]));
    assert_eq!(v["scanned_to"], 3);
}

#[test]
fn settled_view_lists_a_settled_auction() {
    let w = scanned("localnet");
    assert_eq!(ok(&w, json!({ "view": "settled" }))["items"], json!([]));
    let fx = standard::standard();
    let r = Reader::new(w.chain, ENGINE);
    let mut st = r.auction(&fx.live_auction).expect("auction");
    st.settled = true;
    w.chain.put_state(fx.live_auction, ENGINE, &st);
    let v = ok(&w, json!({ "view": "settled" }));
    let items = v["items"].as_array().expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], fx.live_auction.to_string());
    assert_eq!(items[0]["target"], "660000");
    assert_eq!(ok(&w, json!({ "view": "live" }))["items"], json!([]));
}

#[test]
fn at_risk_view_lists_the_unsafe_position() {
    let w = scanned("localnet");
    let fx = standard::standard();
    let v = ok(&w, json!({ "view": "at_risk" }));
    let items = v["items"].as_array().expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["position"], fx.unsafe_position.to_string());
    assert_eq!(items[0]["liquidatable"], true);
    assert_eq!(items[0]["nominal_debt"], "600000");
}

#[test]
fn auction_view_has_the_entry_and_its_schedule() {
    let w = scanned("localnet");
    let fx = standard::standard();
    let v = ok(
        &w,
        json!({ "view": "auction", "id": fx.live_auction.to_string() }),
    );
    assert_eq!(v["kind"], "live");
    assert_eq!(v["id"], fx.live_auction.to_string());
    let st = Reader::new(w.chain, ENGINE)
        .auction(&fx.live_auction)
        .expect("auction");
    let want: Vec<Value> = quote::discount_schedule(&st, 48)
        .expect("schedule")
        .into_iter()
        .map(|(t, d)| json!([t, d.to_string()]))
        .collect();
    assert!(want.len() >= 48);
    assert_eq!(v["schedule"], Value::Array(want));
}

/// The engine's minimum charge for a non-final bid, as the bid card shows it (R31).
#[test]
fn live_rows_and_the_auction_view_carry_the_min_bid() {
    let w = scanned("localnet");
    let fx = standard::standard();
    let v = w.ctx.verified().expect("verified");
    let want = Reader::new(w.chain, ENGINE)
        .coll_config(&v.coll_config_id())
        .expect("coll config")
        .collateral_params()
        .expect("params")
        .min_bid
        .to_string();
    let live = ok(&w, json!({ "view": "live" }));
    assert_eq!(live["items"][0]["min_bid"], want.as_str());
    let auction = ok(
        &w,
        json!({ "view": "auction", "id": fx.live_auction.to_string() }),
    );
    assert_eq!(auction["min_bid"], want.as_str());
    // Settled rows carry no bid floor.
    let r = Reader::new(w.chain, ENGINE);
    let mut st = r.auction(&fx.live_auction).expect("auction");
    st.settled = true;
    w.chain.put_state(fx.live_auction, ENGINE, &st);
    let settled = ok(
        &w,
        json!({ "view": "auction", "id": fx.live_auction.to_string() }),
    );
    assert_eq!(settled["kind"], "settled");
    assert_eq!(settled.get("min_bid"), None);
}

#[test]
fn auction_view_needs_an_id() {
    let w = scanned("localnet");
    assert_eq!(
        kind_of(run(&w, json!({ "view": "auction" }))),
        "bad_request"
    );
    assert_eq!(
        kind_of(run(&w, json!({ "view": "auction", "id": "not-base58!" }))),
        "bad_request"
    );
}

#[test]
fn position_view_has_the_health_and_the_liquidation_quote() {
    let w = scanned("localnet");
    let fx = standard::standard();
    let v = ok(
        &w,
        json!({ "view": "position", "id": fx.unsafe_position.to_string() }),
    );
    assert_eq!(v["position"], fx.unsafe_position.to_string());
    assert_eq!(v["liquidatable"], true);
    assert_eq!(v["collateral"], "500000");
    assert_eq!(v["quote"]["keeper_reward"], "25");
    assert_eq!(v["quote"]["lot"], "499975");
    assert_eq!(v["quote"]["nonce"], 0);
    assert_eq!(v["refused"], json!(null));
}

#[test]
fn position_view_without_a_quote_says_why() {
    let w = scanned("localnet");
    let fx = standard::standard();
    // The liquidated position has no debt left: no CR, and the engine would not liquidate it.
    let v = ok(
        &w,
        json!({ "view": "position", "id": fx.auction_position.to_string() }),
    );
    assert_eq!(v["cr_bps"], json!(null));
    assert_eq!(v["liquidatable"], false);
    assert_eq!(v["quote"], json!(null));
    assert!(v["refused"].is_string(), "{v}");
}

#[test]
fn accounts_view_lists_the_roles_first() {
    let w = world("localnet");
    let v = ok(&w, json!({ "view": "accounts" }));
    let rows = v.as_array().expect("rows");
    let labels: Vec<&str> = rows.iter().map(|r| s(&r["label"])).collect();
    // `views::ROLES` order (roles present in the wallet), then the other names.
    assert_eq!(
        &labels[..5],
        ["owner", "keeper", "caller", "taker", "engine-admin"]
    );
    let bidder = rows
        .iter()
        .find(|r| r["label"] == "bidder")
        .expect("bidder row");
    assert_eq!(bidder["stable_balance"], "1000000");
}

#[test]
fn quote_bid_view_quotes_the_live_auction() {
    let w = scanned("localnet");
    let fx = standard::standard();
    let v = ok(
        &w,
        json!({ "view": "quote_bid", "auction": fx.live_auction.to_string(),
                "coin_in_max": "100000", "slippage_bps": 50 }),
    );
    assert_eq!(v["collateral_out"], "51963");
    assert_eq!(v["min_out"], "51703");
    assert_eq!(v["instruction"], "bid");
}

#[test]
fn quote_bid_view_refuses_bad_amounts() {
    let w = scanned("localnet");
    let fx = standard::standard();
    let a = fx.live_auction.to_string();
    for req in [
        json!({ "view": "quote_bid", "auction": a, "coin_in_max": 100_000 }),
        json!({ "view": "quote_bid", "auction": a, "coin_in_max": "-1" }),
        json!({ "view": "quote_bid", "auction": a, "coin_in_max": "1", "slippage_bps": 10_000 }),
        json!({ "view": "quote_bid", "coin_in_max": "1" }),
    ] {
        assert_eq!(kind_of(run(&w, req.clone())), "bad_request", "{req}");
    }
}

#[test]
fn quote_liquidation_view_quotes_the_unsafe_position() {
    let w = world("localnet");
    let fx = standard::standard();
    let v = ok(
        &w,
        json!({ "view": "quote_liquidation", "position": fx.unsafe_position.to_string() }),
    );
    assert_eq!(
        v["auction"],
        fx.engine.auction(&fx.unsafe_position, 0).to_string()
    );
    assert_eq!(v["amount_to_raise"], "660000");
    assert_eq!(v["keeper_reward"], "25");
    assert_eq!(v["lot"], "499975");
    assert_eq!(v["price_ray"], R.to_string());
}

#[test]
fn quote_take_surplus_view_equals_the_sdk_quote() {
    let w = world("localnet");
    let v = ok(
        &w,
        json!({ "view": "quote_take_surplus", "amount": "100", "slippage_bps": 50 }),
    );
    let ver = w.ctx.verified().expect("verified");
    let q = quote::quote_take_surplus(w.chain, &ver, 0, 100, T0).expect("quote");
    assert_eq!(v["nonce"], 0);
    assert_eq!(v["filled"], q.filled.to_string());
    assert_eq!(v["tokens"], q.tokens.to_string());
    assert_eq!(v["lot_left_after"], q.lot_left_after.to_string());
    assert_eq!(v["limit"], quote::max_in(q.tokens, 50).to_string());
}

#[test]
fn quote_take_debt_view_equals_the_sdk_quote() {
    let w = world("localnet");
    let v = ok(
        &w,
        json!({ "view": "quote_take_debt", "amount": "100", "slippage_bps": 50 }),
    );
    let ver = w.ctx.verified().expect("verified");
    let q = quote::quote_take_debt(w.chain, &ver, 0, 100, T0).expect("quote");
    assert_eq!(v["nonce"], 0);
    assert_eq!(v["filled"], q.filled.to_string());
    assert_eq!(v["tokens"], q.tokens.to_string());
    assert_eq!(v["limit"], quote::min_out(q.tokens, 50).to_string());
}

#[test]
fn quote_take_views_refuse_when_no_auction_was_started() {
    let w = world("localnet");
    let ver = w.ctx.verified().expect("verified");
    let r = Reader::new(w.chain, ENGINE);
    let mut acc = r.accounting(&ver.engine.accounting).expect("accounting");
    acc.surplus_nonce = 0;
    acc.debt_nonce = 0;
    w.chain.put_state(ver.engine.accounting, ENGINE, &acc);
    for name in ["quote_take_surplus", "quote_take_debt"] {
        assert_eq!(
            kind_of(run(&w, json!({ "view": name, "amount": "1" }))),
            "refused",
            "{name}"
        );
    }
}

#[test]
fn quote_settle_debt_view_burns_what_the_engine_would() {
    let w = world("localnet");
    // buffer 2000, unbacked 1000 of which 500 is on auction: the engine burns 500.
    let v = ok(&w, json!({ "view": "quote_settle_debt" }));
    assert_eq!(v["amount"], "500");
    assert_eq!(v["buffer_before"], "2000");
    assert_eq!(v["buffer_after"], "1500");
    assert_eq!(v["unbacked_before"], "1000");
    assert_eq!(v["unbacked_after"], "500");
    let capped = ok(&w, json!({ "view": "quote_settle_debt", "amount": "30" }));
    assert_eq!(capped["amount"], "30");
    assert_eq!(capped["unbacked_after"], "970");
}

#[test]
fn quote_price_view_checks_the_price_and_keeps_cr_under_r() {
    let w = scanned("localnet");
    let fx = standard::standard();
    let v = ok(&w, json!({ "view": "quote_price", "price": R.to_string() }));
    assert_eq!(v["valid"], true);
    assert_eq!(v["reason"], json!(null));
    assert_eq!(v["redemption_ray"], R.to_string());
    assert_eq!(v["redemption_ray_after"], R.to_string());
    let rows = v["positions"].as_array().expect("positions");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["position"], fx.unsafe_position.to_string());
    assert_eq!(rows[0]["cr_bps_now"], rows[0]["cr_bps_after"]);
    let at_risk = ok(&w, json!({ "view": "at_risk" }));
    assert_eq!(rows[0]["cr_bps_now"], at_risk["items"][0]["cr_bps"]);

    let out_of_range = ok(
        &w,
        json!({ "view": "quote_price", "price": (3 * R).to_string() }),
    );
    assert_eq!(out_of_range["valid"], false);
    assert!(out_of_range["reason"].is_string(), "{out_of_range}");
    assert_eq!(
        kind_of(run(&w, json!({ "view": "quote_price", "price": 5 }))),
        "bad_request"
    );
}

/// Every JSON number fits a double exactly (an amount that was serialised as a number would not).
fn assert_no_wide_numbers(path: &str, v: &Value) {
    const MAX_SAFE: u64 = 1 << 53;
    match v {
        Value::Number(n) => assert!(
            n.as_u64().is_some_and(|n| n <= MAX_SAFE) || n.as_i64().is_some(),
            "{path} is a wide number: {n}"
        ),
        Value::Array(a) => a
            .iter()
            .enumerate()
            .for_each(|(i, x)| assert_no_wide_numbers(&format!("{path}[{i}]"), x)),
        Value::Object(o) => o
            .iter()
            .for_each(|(k, x)| assert_no_wide_numbers(&format!("{path}.{k}"), x)),
        _ => {}
    }
}

#[test]
fn prices_and_amounts_are_decimal_strings() {
    let w = scanned("localnet");
    let fx = standard::standard();
    let auction = fx.live_auction.to_string();
    let position = fx.unsafe_position.to_string();
    const LIVE: &[&str] = &[
        "/lot",
        "/raised",
        "/target",
        "/collateral_left",
        "/reserved",
        "/discount_ray",
        "/rate_per_min_ray",
        "/min_bid",
    ];
    const LIQUIDATION: &[&str] = &[
        "/collateral",
        "/nominal_debt",
        "/vault_balance",
        "/redemption_ray",
        "/market_ray",
        "/price_ray",
        "/debt_principal",
        "/amount_to_raise",
        "/reserve",
        "/keeper_reward",
        "/lot",
        "/seized",
    ];
    // (request, u128 fields as JSON pointers; "*" maps over an array)
    let cases: Vec<(Value, Vec<String>)> = vec![
        (
            json!({ "view": "overview" }),
            [
                "/accounting/buffer",
                "/accounting/unbacked_debt",
                "/accounting/debt_on_auction",
                "/accounting/freeze_threshold",
                "/accounting/surplus_lot_left",
                "/accounting/debt_lot_left",
                "/host/min_cr_ray",
                "/oracle/price_ray",
                "/oracle/range_lo_ray",
                "/oracle/range_hi_ray",
            ]
            .map(String::from)
            .to_vec(),
        ),
        (
            json!({ "view": "live" }),
            LIVE.iter().map(|f| format!("/items/*{f}")).collect(),
        ),
        (
            json!({ "view": "at_risk" }),
            vec!["/items/*/collateral".into(), "/items/*/nominal_debt".into()],
        ),
        (
            json!({ "view": "auction", "id": auction }),
            LIVE.iter()
                .map(|f| (*f).to_owned())
                .chain(["/schedule/*/1".to_owned()])
                .collect(),
        ),
        (
            json!({ "view": "position", "id": position }),
            ["/collateral", "/nominal_debt"]
                .iter()
                .map(|f| (*f).to_owned())
                .chain(LIQUIDATION.iter().map(|f| format!("/quote{f}")))
                .collect(),
        ),
        (
            json!({ "view": "accounts" }),
            vec!["/*/stable_balance".into(), "/*/coll_balance".into()],
        ),
        (
            json!({ "view": "quote_bid", "auction": auction, "coin_in_max": "100000" }),
            [
                "/charged",
                "/collateral_out",
                "/min_out",
                "/debt_part",
                "/penalty_part",
                "/discount_ray",
                "/redemption_ray",
                "/price_ray",
                "/remaining_target",
                "/collateral_left",
                "/coin_in_max",
            ]
            .map(String::from)
            .to_vec(),
        ),
        (
            json!({ "view": "quote_liquidation", "position": position }),
            LIQUIDATION.iter().map(|f| (*f).to_owned()).collect(),
        ),
        (
            json!({ "view": "quote_take_surplus", "amount": "100" }),
            ["/filled", "/tokens", "/lot_left_after", "/limit"]
                .map(String::from)
                .to_vec(),
        ),
        (
            json!({ "view": "quote_take_debt", "amount": "100" }),
            ["/filled", "/tokens", "/lot_left_after", "/limit"]
                .map(String::from)
                .to_vec(),
        ),
        (
            json!({ "view": "quote_settle_debt" }),
            [
                "/amount",
                "/buffer_before",
                "/buffer_after",
                "/unbacked_before",
                "/unbacked_after",
                "/debt_on_auction",
            ]
            .map(String::from)
            .to_vec(),
        ),
        (
            json!({ "view": "quote_price", "price": R.to_string() }),
            ["/price", "/redemption_ray", "/redemption_ray_after"]
                .map(String::from)
                .to_vec(),
        ),
    ];
    for (req, fields) in &cases {
        let v = ok(&w, req.clone());
        let name = s(&req["view"]);
        assert_no_wide_numbers(name, &v);
        for f in fields {
            assert_decimals(name, &v, f);
        }
    }
    // The settled rows, on the fixture's auction marked settled.
    let r = Reader::new(w.chain, ENGINE);
    let mut st = r.auction(&fx.live_auction).expect("auction");
    st.settled = true;
    w.chain.put_state(fx.live_auction, ENGINE, &st);
    let settled = ok(&w, json!({ "view": "settled" }));
    for f in [
        "lot",
        "sold",
        "raised",
        "target",
        "debt_repaid",
        "penalty",
        "shortfall",
        "leftover",
        "claimed",
    ] {
        assert_decimals("settled", &settled, &format!("/items/*/{f}"));
    }
}

/// Every value at `pointer` ("*" maps over an array, which must not be empty) is a decimal u128
/// string; an `Option<u128>` field may be null, but at least one value must be a string.
fn assert_decimals(view: &str, v: &Value, pointer: &str) {
    let values = select(v, pointer);
    assert!(!values.is_empty(), "{view}{pointer}: nothing there in {v}");
    let mut strings = 0;
    for x in values {
        match x {
            Value::String(t) => {
                t.parse::<u128>()
                    .unwrap_or_else(|_| panic!("{view}{pointer}: {t:?} is not a decimal u128"));
                strings += 1;
            }
            Value::Null => {}
            other => panic!("{view}{pointer} is not a string: {other}"),
        }
    }
    assert!(strings > 0, "{view}{pointer}: only nulls in {v}");
}

fn select<'a>(v: &'a Value, pointer: &str) -> Vec<&'a Value> {
    match pointer.split_once("/*") {
        None => v.pointer(pointer).into_iter().collect(),
        Some((head, rest)) => v
            .pointer(head)
            .and_then(Value::as_array)
            .map(|a| a.iter().flat_map(|x| select(x, rest)).collect())
            .unwrap_or_default(),
    }
}
