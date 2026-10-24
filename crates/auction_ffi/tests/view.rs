mod common;

use auction_ffi::ctx::Ctx;
use auction_ffi::view;
use serde_json::json;

use common::world;

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
}

#[test]
fn deployment_view_of_a_non_dev_network_is_not_dev() {
    let w = world("mainnet");
    let v = view::deployment(&w.ctx).expect("view");
    assert_eq!(v["dev"], false);
    assert_eq!(v["genesis_ok"], true);
}

#[test]
fn deployment_view_without_a_genesis_hash_is_not_dev_and_not_ok() {
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
