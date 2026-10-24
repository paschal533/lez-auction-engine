mod common;

use auction_ffi::ctx::Ctx;
use auction_ffi::{envelope, track};
use auction_sdk::testing::{id, standard::k};
use auction_sdk::{BlockSummary, Chain, SdkError, SdkResult};
use lee_core::account::{Account, AccountId, Nonce};
use serde_json::{json, Value};

use common::{lock_dir, world, World};

fn signer() -> String {
    id(k::CALLER).to_string()
}

fn begin(w: &World) -> Result<Value, envelope::FfiError> {
    track::begin(&w.ctx, &json!({ "signer": signer() }))
}

fn poll(w: &World, tracking: &Value) -> Value {
    track::poll(&w.ctx, &json!({ "tracking": tracking })).expect("poll")
}

fn kind_of(r: Result<Value, envelope::FfiError>) -> String {
    let v = envelope::to_value(r);
    assert_eq!(v["ok"], false, "{v}");
    v["error"]["kind"].as_str().expect("kind").to_owned()
}

#[test]
fn tx_begin_twice_for_one_signer_is_in_flight() {
    let w = world("localnet");
    let first = begin(&w).expect("first");
    assert!(std::path::Path::new(first["lock_path"].as_str().expect("path")).exists());
    assert_eq!(kind_of(begin(&w)), "in_flight");
}

#[test]
fn tx_poll_reports_included() {
    let w = world("localnet");
    let t = begin(&w).expect("begin");
    assert_eq!(poll(&w, &t)["state"], "pending");
    w.chain.set_nonce(id(k::CALLER), 1);
    assert_eq!(
        poll(&w, &t),
        json!({"state": "included", "nonce_after": "1"})
    );
}

#[test]
fn tx_poll_reports_dropped_after_the_window() {
    let w = world("localnet");
    let t = begin(&w).expect("begin");
    let h0 = t["height_before"].as_u64().expect("h0");
    // The fixture chain advances the height by `step` on every `last_block_id`.
    w.chain.set_height_step(10);
    let v = poll(&w, &t);
    assert_eq!(v["state"], "dropped", "{v}");
    assert!(v["height"].as_u64().expect("height") >= h0 + 5);
}

#[test]
fn tx_poll_reports_blocks_waited() {
    let w = world("localnet");
    let t = begin(&w).expect("begin");
    let h0 = t["height_before"].as_u64().expect("h0");
    // The first `last_block_id` after this returns `h0 + 2`.
    w.chain.set_height_step(h0 + 2);
    let v = poll(&w, &t);
    assert_eq!(v["state"], "pending", "{v}");
    assert_eq!(v["blocks_waited"], 2);
}

#[test]
fn drop_after_blocks_comes_from_the_context() {
    let mut w = world("localnet");
    w.json.drop_after_blocks = Some(2);
    let deployment = w.ctx.deployment.clone();
    w.ctx = Ctx::with_chain(Box::new(w.chain), deployment, &w.json).expect("ctx");
    let t = begin(&w).expect("begin");
    let h0 = t["height_before"].as_u64().expect("h0");
    // Two blocks pass: over a 2-block window, but inside the default 5.
    w.chain.set_height_step(h0 + 2);
    assert_eq!(poll(&w, &t)["state"], "dropped");
}

#[test]
fn a_zero_drop_window_is_refused() {
    let mut w = world("localnet");
    w.json.drop_after_blocks = Some(0);
    let deployment = w.ctx.deployment.clone();
    let e = Ctx::with_chain(Box::new(w.chain), deployment, &w.json)
        .err()
        .expect("refused");
    assert_eq!(kind_of(Err(e)), "bad_request");
}

struct Down;

impl Chain for Down {
    fn account(&self, _: &AccountId) -> SdkResult<Account> {
        Err(SdkError::Chain("down".into()))
    }
    fn nonces(&self, _: &[AccountId]) -> SdkResult<Vec<Nonce>> {
        Err(SdkError::Chain("down".into()))
    }
    fn last_block_id(&self) -> SdkResult<u64> {
        Err(SdkError::Chain("down".into()))
    }
    fn blocks(&self, _: u64, _: u64) -> SdkResult<Vec<BlockSummary>> {
        Err(SdkError::Chain("down".into()))
    }
    fn send(&self, _: &lee::PublicTransaction) -> SdkResult<[u8; 32]> {
        Err(SdkError::Chain("down".into()))
    }
}

#[test]
fn tx_poll_surfaces_read_errors_as_chain_errors() {
    let w = world("localnet");
    let t = begin(&w).expect("begin");
    let ctx = Ctx::with_chain(Box::new(Down), w.ctx.deployment.clone(), &w.json).expect("ctx");
    let e = track::poll(&ctx, &json!({ "tracking": t }));
    assert_eq!(kind_of(e), "chain");
}

#[test]
fn tx_end_included_releases_the_lock_and_unknown_keeps_it() {
    let w = world("localnet");
    let t = begin(&w).expect("begin");
    let lock = std::path::PathBuf::from(t["lock_path"].as_str().expect("path"));
    let end = |tracking: &Value, outcome: &str| {
        track::end(
            &w.json,
            &json!({ "tracking": tracking, "outcome": outcome }),
        )
    };
    assert_eq!(end(&t, "unknown").expect("end"), json!({}));
    assert!(lock.exists(), "an unknown outcome keeps the lock");
    assert_eq!(end(&t, "included").expect("end"), json!({}));
    assert!(!lock.exists(), "a definite outcome releases it");
    let t2 = begin(&w).expect("begin again");
    let lock2 = std::path::PathBuf::from(t2["lock_path"].as_str().expect("path"));
    end(&t2, "dropped").expect("end");
    assert!(!lock2.exists());
}

#[test]
fn tx_end_refuses_an_unknown_outcome_word() {
    let w = world("localnet");
    let t = begin(&w).expect("begin");
    let e = track::end(&w.json, &json!({ "tracking": t, "outcome": "maybe" }));
    assert_eq!(kind_of(e), "bad_request");
}

#[test]
fn tracking_outside_the_lock_dir_is_refused() {
    let w = world("localnet");
    let t = begin(&w).expect("begin");
    let victim = w.dir.path().join("precious.txt");
    std::fs::write(&victim, "keep me").expect("write");
    let mut forged = t.clone();
    forged["lock_path"] = json!(victim.to_str().expect("utf-8"));
    assert_eq!(
        kind_of(track::end(
            &w.json,
            &json!({ "tracking": forged, "outcome": "included" })
        )),
        "bad_request"
    );
    assert_eq!(
        kind_of(track::poll(&w.ctx, &json!({ "tracking": forged }))),
        "bad_request"
    );
    assert!(victim.exists(), "nothing was deleted");
    // A path that climbs out of the lock dir by `..` is outside too.
    let sneaky = lock_dir(&w.json.state_dir).join("..").join("precious.txt");
    forged["lock_path"] = json!(sneaky.to_str().expect("utf-8"));
    assert_eq!(
        kind_of(track::end(
            &w.json,
            &json!({ "tracking": forged, "outcome": "included" })
        )),
        "bad_request"
    );
    assert!(victim.exists());
    // And so is a file in the lock dir that is not this signer's lock.
    let other = lock_dir(&w.json.state_dir).join("notes.txt");
    std::fs::write(&other, "x").expect("write");
    forged["lock_path"] = json!(other.to_str().expect("utf-8"));
    assert_eq!(
        kind_of(track::end(
            &w.json,
            &json!({ "tracking": forged, "outcome": "included" })
        )),
        "bad_request"
    );
    assert!(other.exists());
}

#[test]
fn tx_ops_refuse_malformed_requests() {
    let w = world("localnet");
    let results = [
        track::begin(&w.ctx, &json!({})),
        track::begin(&w.ctx, &json!({ "signer": "not base58!" })),
        track::poll(&w.ctx, &json!({ "tracking": { "signer": 1 } })),
        track::end(&w.json, &json!({ "outcome": "included" })),
    ];
    for r in results {
        assert_eq!(kind_of(r), "bad_request");
    }
}
