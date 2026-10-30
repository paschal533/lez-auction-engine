//! The call deadline (Ruling R54): a view shares one deadline across all its reads, each read capped
//! at the time left, so a view on a slow node still finishes and a stalled read ends at the deadline.
mod common;

use std::io::Read as _;
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use auction_ffi::ctx::{Ctx, CtxJson};
use auction_ffi::{envelope, view};
use auction_sdk::deployment::program_id_hex;
use auction_sdk::testing::standard::TOKEN;
use auction_sdk::{BlockSummary, Chain, SdkResult};
use lee_core::account::{Account, AccountId, Nonce};
use serde_json::{json, Value};

use common::world;

/// What the module hands a view: its 15 s budget less the 1 s it keeps to return the reply
/// (budget.h: MODULE_CALL_BUDGET_MS - FFI_REPLY_MS).
const VIEW_BUDGET_MS: u64 = 14_000;
/// About what one read takes on a real node behind the public proxy.
const READ_DELAY: Duration = Duration::from_millis(250);

/// Every read takes `READ_DELAY`, like a remote node; counts the reads.
struct Slow<'a> {
    inner: &'a dyn Chain,
    reads: &'a AtomicUsize,
}

impl Slow<'_> {
    fn wait(&self) {
        self.reads.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(READ_DELAY);
    }
}

impl Chain for Slow<'_> {
    fn account(&self, id: &AccountId) -> SdkResult<Account> {
        self.wait();
        self.inner.account(id)
    }
    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<Nonce>> {
        self.wait();
        self.inner.nonces(ids)
    }
    fn last_block_id(&self) -> SdkResult<u64> {
        self.wait();
        self.inner.last_block_id()
    }
    fn blocks(&self, from: u64, to: u64) -> SdkResult<Vec<BlockSummary>> {
        self.wait();
        self.inner.blocks(from, to)
    }
    fn send(&self, tx: &lee::PublicTransaction) -> SdkResult<[u8; 32]> {
        self.inner.send(tx)
    }
}

#[test]
fn an_overview_of_twenty_slow_reads_finishes_inside_the_view_budget() {
    let w = world("localnet");
    let scan = view::dispatch(&w.ctx, &json!({ "view": "scan", "max_blocks": 100 })).expect("scan");
    assert_eq!(scan["done"], true, "{scan}");
    // Box<dyn Chain> is 'static: the counter lives for the test binary.
    let reads: &'static AtomicUsize = Box::leak(Box::new(AtomicUsize::new(0)));
    let json = CtxJson {
        budget_ms: Some(VIEW_BUDGET_MS),
        ..w.json.clone()
    };
    let slow = Slow {
        inner: w.chain,
        reads,
    };
    let ctx = Ctx::with_chain(Box::new(slow), w.ctx.deployment.clone(), &json).expect("ctx");

    let started = Instant::now();
    let v = view::dispatch(&ctx, &json!({ "view": "overview" }));
    let elapsed = started.elapsed();

    let v = v.unwrap_or_else(|e| panic!("overview failed after {elapsed:?}: {e:?}"));
    assert!(v["counts"].is_object(), "{v}");
    let n = reads.load(Ordering::SeqCst);
    assert!(n >= 20, "the overview made {n} reads");
    // The old 4 s view budget could not hold these reads.
    assert!(elapsed > Duration::from_secs(4), "{elapsed:?}");
    assert!(
        elapsed < Duration::from_millis(VIEW_BUDGET_MS),
        "{elapsed:?}"
    );
}

/// A node that accepts connections and never answers.
fn silent_node() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut stream = stream;
                let mut sink = [0u8; 1024];
                while matches!(stream.read(&mut sink), Ok(n) if n > 0) {}
            });
        }
    });
    url
}

#[test]
fn a_stalled_read_near_the_deadline_ends_at_the_deadline() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("deployment.json");
    let id_hex = program_id_hex(&TOKEN);
    std::fs::write(
        &path,
        json!({
            "network": "localnet",
            "sequencer_url": silent_node(),
            "engine_program_id": id_hex,
            "host_program_id": id_hex,
            "token_program_id": id_hex,
            "chain_genesis_hash": "11".repeat(32),
        })
        .to_string(),
    )
    .expect("write");
    let budget = Duration::from_millis(1500);
    let request = json!({
        "view": "deployment",
        "ctx": {
            "deployment": path,
            "state_dir": dir.path().join("s"),
            "budget_ms": u64::try_from(budget.as_millis()).expect("ms"),
        },
    })
    .to_string();

    let started = Instant::now();
    let reply = envelope::run(Some(&request), view::handle);
    let elapsed = started.elapsed();

    let v: Value = serde_json::from_str(&reply).expect("json");
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["error"]["kind"], "chain", "{v}");
    // A 5 s read timeout and its retry would run on for about 11 s.
    assert!(elapsed < budget + Duration::from_millis(500), "{elapsed:?}");
}
