//! Spec §3.2/§5.10: IDL position == enum index for every engine instruction (Shared Reference R2).

const R2: [&str; 17] = [
    "init_config",
    "set_engine_params",
    "set_collateral_config",
    "observe_feed",
    "liquidate",
    "bid",
    "bid_with_settlement",
    "settle_empty",
    "claim_leftover",
    "settle_debt",
    "start_surplus_auction",
    "take_surplus",
    "close_surplus_auction",
    "start_debt_auction",
    "take_debt",
    "close_debt_auction",
    "check_solvency",
];

fn idl_names() -> Vec<String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../idl/auction_engine-idl.json"
    );
    let idl: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("engine IDL"))
            .expect("IDL json");
    idl["instructions"]
        .as_array()
        .expect("array")
        .iter()
        .map(|i| i["name"].as_str().expect("name").to_owned())
        .collect()
}

#[test]
fn engine_idl_is_a_prefix_of_the_reference_order() {
    let names = idl_names();
    assert!(!names.is_empty() && names.len() <= R2.len());
    for (i, name) in names.iter().enumerate() {
        assert_eq!(name, R2[i], "IDL position {i}");
    }
}

#[test]
fn engine_idl_is_complete() {
    assert_eq!(
        idl_names(),
        R2.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>()
    );
}
