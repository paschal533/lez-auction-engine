//! QML fixtures: writes the `value` of every view on the standard fixture (the JSON the
//! `auction_engine` module hands QML) into `apps/auctions/tests/qml/fixtures/`, so the screen tests
//! use real numbers. Ignored in normal runs; `scripts/basecamp/gen-qml-fixtures.sh` runs it.

mod common;

use std::path::PathBuf;

use auction_ffi::view;
use auction_sdk::decode::Reader;
use auction_sdk::testing::standard::{self, ENGINE, R};
use lee_core::account::AccountId;
use serde_json::{json, Value};

use common::{world, World};

fn out_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/auctions/tests/qml/fixtures")
}

fn ok(w: &World, req: &Value) -> Value {
    view::dispatch(&w.ctx, req).unwrap_or_else(|e| panic!("{req}: {e:?}"))
}

fn write(name: &str, v: &Value) {
    let path = out_dir().join(format!("{name}.json"));
    let text = serde_json::to_string_pretty(v).expect("json") + "\n";
    std::fs::write(&path, text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}

/// The standard fixture on a dev network, the whole chain scanned (the first page is kept as `scan`).
fn scanned() -> World {
    let w = world("localnet");
    write("scan", &ok(&w, &json!({ "view": "scan", "max_blocks": 1 })));
    let done = ok(&w, &json!({ "view": "scan", "max_blocks": 100 }));
    assert_eq!(done["done"], true, "{done}");
    write("scan_done", &done);
    w
}

#[test]
#[ignore = "writes apps/auctions/tests/qml/fixtures; run by scripts/basecamp/gen-qml-fixtures.sh"]
fn write_qml_fixtures() {
    std::fs::create_dir_all(out_dir()).expect("fixtures dir");
    let w = scanned();
    let fx = standard::standard();
    let auction = fx.live_auction.to_string();
    let position = fx.unsafe_position.to_string();
    let views: Vec<(&str, Value)> = vec![
        ("deployment", json!({ "view": "deployment" })),
        ("overview", json!({ "view": "overview" })),
        ("live", json!({ "view": "live" })),
        ("at_risk", json!({ "view": "at_risk" })),
        ("auction", json!({ "view": "auction", "id": auction })),
        ("position", json!({ "view": "position", "id": position })),
        (
            "position_refused",
            json!({ "view": "position", "id": fx.auction_position.to_string() }),
        ),
        ("accounts", json!({ "view": "accounts" })),
        (
            "quote_bid",
            json!({ "view": "quote_bid", "auction": auction, "coin_in_max": "100000", "slippage_bps": 50 }),
        ),
        (
            "quote_liquidation",
            json!({ "view": "quote_liquidation", "position": position }),
        ),
        (
            "quote_take_surplus",
            json!({ "view": "quote_take_surplus", "amount": "100", "slippage_bps": 50 }),
        ),
        (
            "quote_take_debt",
            json!({ "view": "quote_take_debt", "amount": "100", "slippage_bps": 50 }),
        ),
        ("quote_settle_debt", json!({ "view": "quote_settle_debt" })),
        (
            "quote_price",
            json!({ "view": "quote_price", "price": R.to_string() }),
        ),
    ];
    for (name, req) in &views {
        write(name, &ok(&w, req));
    }

    // Settled outcomes of the fixture's auction (lot 475,000, debt 600,000, penalty 60,000), states
    // an engine can end in (as `views_tests::settle` builds them): bids pay debt first, then penalty;
    // leftover only when the whole target was raised; otherwise the lot sold out.
    //   settled / auction_settled:             target raised, 400,000 sold, 75,000 left over;
    //   settled_short / auction_settled_short: 450,000 repaid, no penalty, lot sold out, 150,000 short.
    settle(&w, &fx.live_auction, 600_000, 60_000, 75_000);
    write_settled(&w, &auction, "");
    settle(&w, &fx.live_auction, 450_000, 0, 0);
    write_settled(&w, &auction, "_short");
}

/// Marks the auction settled with the given raised amounts and leftover; the rest of the lot sold.
fn settle(w: &World, id: &AccountId, debt: u128, penalty: u128, leftover: u128) {
    let r = Reader::new(w.chain, ENGINE);
    let mut st = r.auction(id).expect("auction");
    let lot = st.collateral_sold + st.collateral_left;
    assert!(debt <= st.debt_principal && debt + penalty <= st.amount_to_raise);
    assert!(
        leftover == 0 || debt + penalty == st.amount_to_raise,
        "a leftover needs the whole target raised"
    );
    st.settled = true;
    st.collateral_sold = lot - leftover;
    st.collateral_left = leftover;
    st.leftover_claimable = leftover;
    st.debt_raised = debt;
    st.penalty_raised = penalty;
    w.chain.put_state(*id, ENGINE, &st);
}

fn write_settled(w: &World, auction: &str, suffix: &str) {
    let settled = ok(w, &json!({ "view": "settled" }));
    assert_eq!(
        settled["items"].as_array().map(Vec::len),
        Some(1),
        "{settled}"
    );
    write(&format!("settled{suffix}"), &settled);
    write(
        &format!("auction_settled{suffix}"),
        &ok(w, &json!({ "view": "auction", "id": auction })),
    );
}
