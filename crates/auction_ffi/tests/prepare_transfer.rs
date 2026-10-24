mod common;

use std::time::{Duration, Instant};

use auction_ffi::ctx::Ctx;
use auction_ffi::{envelope, prepare};
use auction_sdk::deployment::program_id_hex;
use auction_sdk::plan_host;
use auction_sdk::testing::{
    id,
    standard::{k, TOKEN},
};
use serde_json::{json, Value};

use common::world;

fn req(from: &str, to: &str, amount: &str) -> Value {
    json!({ "action": "token_transfer", "from": from, "to": to, "amount": amount })
}

fn kind_of(r: Result<Value, envelope::FfiError>) -> String {
    let v = envelope::to_value(r);
    assert_eq!(v["ok"], false, "{v}");
    v["error"]["kind"].as_str().expect("kind").to_owned()
}

#[test]
fn token_transfer_plan_matches_the_cli() {
    let w = world("localnet");
    let out = prepare::handle(&w.ctx, &req("owner", "keeper", "1500")).expect("plan");
    let want = plan_host::token_transfer(TOKEN, id(k::OWNER), id(k::KEEPER), 1500).expect("plan");
    assert_eq!(out["facts"], json!([]));
    let steps = out["steps"].as_array().expect("steps");
    assert_eq!(steps.len(), 1);
    let s = &steps[0];
    assert_eq!(s["program_id"], program_id_hex(&want.program_id));
    let accounts = s["accounts"].as_array().expect("accounts");
    let got_ids: Vec<&str> = accounts
        .iter()
        .map(|a| a["id"].as_str().expect("id"))
        .collect();
    let want_ids: Vec<String> = want
        .account_ids
        .iter()
        .map(|a| hex::encode(a.value()))
        .collect();
    assert_eq!(got_ids, want_ids);
    let flags: Vec<bool> = accounts
        .iter()
        .map(|a| a["signer"].as_bool().expect("flag"))
        .collect();
    assert_eq!(flags, [true, false]);
    let words: Vec<u32> = serde_json::from_value(s["instruction_words"].clone()).expect("words");
    assert_eq!(words, want.instruction_words);
    assert_eq!(s["signers"], json!([id(k::OWNER).to_string()]));
    assert_eq!(s["what"], "transfer");
    assert_eq!(s["check"], "dev balance --account <account>");
}

#[test]
fn token_transfer_accepts_base58_ids_and_the_testnet_legacy_network() {
    let w = world("testnet-legacy");
    let out = prepare::handle(
        &w.ctx,
        &req(&id(k::OWNER).to_string(), &id(k::KEEPER).to_string(), "1"),
    )
    .expect("plan");
    assert_eq!(out["steps"].as_array().expect("steps").len(), 1);
}

#[test]
fn token_transfer_is_refused_off_dev_networks() {
    let w = world("mainnet");
    let v = envelope::to_value(prepare::handle(&w.ctx, &req("owner", "keeper", "1")));
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["error"]["kind"], "mismatch");
    let message = v["error"]["message"].as_str().expect("message");
    assert!(message.contains("DEV_NETWORKS"), "{message}");
    assert!(message.contains("localnet"), "{message}");
}

#[test]
fn token_transfer_is_refused_on_another_chain() {
    let mut w = world("localnet");
    let mut d = w.ctx.deployment.clone();
    d.chain_genesis = Some("00".repeat(32));
    w.ctx = Ctx::with_chain(Box::new(w.chain), d, &w.json).expect("ctx");
    assert_eq!(
        kind_of(prepare::handle(&w.ctx, &req("owner", "keeper", "1"))),
        "mismatch"
    );
}

#[test]
fn token_transfer_refuses_bad_input() {
    let w = world("localnet");
    let too_big = format!("1{}", u128::MAX);
    let bad = [
        req("owner", "keeper", "-1"),
        req("owner", "keeper", "1.5"),
        req("owner", "keeper", &too_big),
        json!({ "action": "swap" }),
        json!({}),
    ];
    for r in bad {
        assert_eq!(kind_of(prepare::handle(&w.ctx, &r)), "bad_request");
    }
    assert_eq!(
        kind_of(prepare::handle(&w.ctx, &req("nobody", "keeper", "1"))),
        "keystore"
    );
}

#[test]
fn view_against_a_closed_port_fails_within_12_s() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("deployment.json");
    let id_hex = program_id_hex(&TOKEN);
    std::fs::write(
        &path,
        json!({
            "network": "localnet",
            "sequencer_url": "http://127.0.0.1:9",
            "engine_program_id": id_hex,
            "host_program_id": id_hex,
            "token_program_id": id_hex,
            "chain_genesis_hash": "11".repeat(32),
        })
        .to_string(),
    )
    .expect("write");
    let request = json!({
        "view": "deployment",
        "ctx": { "deployment": path, "state_dir": dir.path().join("s"), "wallet_home": null, "config": null },
    })
    .to_string();
    let started = Instant::now();
    let reply = envelope::run(Some(&request), auction_ffi::view::handle);
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(12), "{elapsed:?}");
    let v: Value = serde_json::from_str(&reply).expect("json");
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["error"]["kind"], "chain", "{v}");
}
