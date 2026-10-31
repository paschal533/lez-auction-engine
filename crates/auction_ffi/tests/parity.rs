//! The app shows the CLI's numbers: the FFI's views on the standard fixture against the CLI's own
//! golden output for the same fixture (`cli/tests/golden/*.txt`).

mod common;

use auction_cli::output::{bps, yes_no};
use auction_ffi::view;
use serde_json::{json, Value};

use common::world;

const QUOTE_BID: &str = include_str!("../../../cli/tests/golden/quote_bid.txt");
const CONFIG: &str = include_str!("../../../cli/tests/golden/config.txt");

/// The value of the golden line whose key is `key` (`kv` pads keys with spaces).
fn golden(text: &str, key: &str) -> String {
    text.lines()
        .find_map(|l| {
            l.strip_prefix(key)
                .filter(|rest| rest.starts_with("  "))
                .map(|rest| rest.trim().to_owned())
        })
        .unwrap_or_else(|| panic!("no {key:?} line in the golden"))
}

fn s(v: &Value) -> String {
    v.as_str()
        .unwrap_or_else(|| panic!("not a string: {v}"))
        .to_owned()
}

#[test]
fn quote_bid_view_equals_the_cli_quote() {
    let w = world("localnet");
    let auction = golden(QUOTE_BID, "auction");
    // The CLI golden ran `quote-bid --coin-in-max 100000` with the default 50 bps.
    let q = view::dispatch(
        &w.ctx,
        &json!({ "view": "quote_bid", "auction": auction, "coin_in_max": "100000", "slippage_bps": 50 }),
    )
    .expect("quote");
    assert_eq!(s(&q["auction"]), auction);
    assert_eq!(q["now_ms"].to_string(), golden(QUOTE_BID, "clock (ms)"));
    assert_eq!(q["t_eff_ms"].to_string(), golden(QUOTE_BID, "t_eff (ms)"));
    let discount: u128 = s(&q["discount_ray"]).parse().expect("ray");
    assert_eq!(bps(discount), golden(QUOTE_BID, "discount"));
    for (field, key) in [
        ("price_ray", "price (RAY)"),
        ("remaining_target", "remaining target"),
        ("collateral_left", "collateral left"),
        ("charged", "charged"),
        ("collateral_out", "collateral out"),
        ("min_out", "min out (50 bps)"),
        ("debt_part", "debt part"),
        ("penalty_part", "penalty part"),
    ] {
        assert_eq!(s(&q[field]), golden(QUOTE_BID, key), "{field}");
    }
    let ends = q["ends_auction"].as_bool().expect("bool");
    assert_eq!(yes_no(ends), golden(QUOTE_BID, "ends auction"));
    assert_eq!(s(&q["instruction"]), golden(QUOTE_BID, "instruction"));
}

#[test]
fn overview_accounting_equals_the_cli_config_line() {
    let w = world("localnet");
    let o = view::dispatch(&w.ctx, &json!({ "view": "overview" })).expect("overview");
    let a = &o["accounting"];
    let nonce = |k: &str| a[k].as_u64().expect("nonce");
    let flag = |k: &str| yes_no(a[k].as_bool().expect("flag"));
    // Rebuilt from the FFI's fields in the CLI's wording, then compared with the CLI's line.
    let line = format!(
        "unbacked debt {}, debt on auction {}, surplus live {} (next nonce {}), debt live {} (next nonce {})",
        s(&a["unbacked_debt"]),
        s(&a["debt_on_auction"]),
        flag("surplus_live"),
        nonce("surplus_nonce"),
        flag("debt_live"),
        nonce("debt_nonce"),
    );
    assert_eq!(line, golden(CONFIG, "accounting"));
    assert_eq!(
        s(&a["surplus_lot_left"]),
        golden(CONFIG, "surplus lot left")
    );
    assert_eq!(s(&a["debt_lot_left"]), golden(CONFIG, "debt lot left"));
}
