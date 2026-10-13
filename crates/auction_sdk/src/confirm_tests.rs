use super::*;
use crate::testing::standard::{self, k, ORACLE};
use crate::testing::{id, SendMode, TestKeys};
use crate::verify::verify_config;

fn fast() -> ConfirmSettings {
    ConfirmSettings {
        poll_interval: Duration::ZERO,
        drop_after_blocks: 5,
        max_polls: 50,
    }
}

fn observe_plan(fx: &standard::Fixture) -> Plan {
    let v = verify_config(&fx.chain, &fx.deployment, &standard::config_id()).expect("verifies");
    crate::plan::observe_feed(&v, id(k::CALLER), ORACLE).expect("plan")
}

fn read_record(path: &Path) -> LockRecord {
    let text = fs::read_to_string(path).expect("lock file exists");
    serde_json::from_str(&text).expect("lock parses")
}

fn is_empty_dir(dir: &Path) -> bool {
    fs::read_dir(dir).expect("dir").next().is_none()
}

fn run_observe(fx: &standard::Fixture, dir: &Path, s: &ConfirmSettings) -> SdkResult<Submitted> {
    submit(
        &fx.chain,
        &TestKeys::of(&[k::CALLER]),
        &observe_plan(fx),
        dir,
        s,
        &|_| {},
    )
}

/// A chain whose transaction lands between the first and the second read of one poll: before the
/// first read the nonce is `n0` and the tip is `h0`; after it the nonce is `n0 + 1` and the tip is
/// `h0 + 5` (the sequencer applies state before it moves the tip).
struct LandsBetweenReads {
    reads: std::cell::Cell<u32>,
    h0: u64,
}

impl LandsBetweenReads {
    fn landed(&self) -> bool {
        let n = self.reads.get();
        self.reads.set(n + 1);
        n >= 1
    }
}

impl Chain for LandsBetweenReads {
    fn account(&self, _: &AccountId) -> SdkResult<lee_core::account::Account> {
        Ok(lee_core::account::Account::default())
    }
    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<lee_core::account::Nonce>> {
        let n = u128::from(self.landed());
        Ok(ids.iter().map(|_| lee_core::account::Nonce(n)).collect())
    }
    fn last_block_id(&self) -> SdkResult<u64> {
        Ok(if self.landed() { self.h0 + 5 } else { self.h0 })
    }
    fn blocks(&self, _: u64, _: u64) -> SdkResult<Vec<crate::chain::BlockSummary>> {
        Ok(vec![])
    }
    fn send(&self, _: &lee::PublicTransaction) -> SdkResult<[u8; 32]> {
        Ok([0; 32])
    }
}

#[test]
fn a_transaction_landing_between_the_reads_is_included_not_dropped() {
    let chain = LandsBetweenReads {
        reads: std::cell::Cell::new(0),
        h0: 10,
    };
    let o = confirm(&chain, &id(k::CALLER), 0, 10, &fast(), &|_| {}).expect("verdict");
    assert_eq!(o, Outcome::Included { nonce_after: 1 });
}

#[test]
fn included_transaction_reports_the_new_nonce_and_releases_the_lock() {
    let fx = standard::standard();
    let dir = tempfile::tempdir().expect("tmp");
    let s = run_observe(&fx, dir.path(), &fast()).expect("submitted");
    assert_eq!(s.outcome, Outcome::Included { nonce_after: 1 });
    assert!(is_empty_dir(dir.path()), "lock released");
}

#[test]
fn dropped_transaction_reports_dropped_and_releases_the_lock() {
    let fx = standard::standard();
    fx.chain.set_send_mode(SendMode::Drop);
    fx.chain.set_height_step(1);
    let dir = tempfile::tempdir().expect("tmp");
    let s = run_observe(&fx, dir.path(), &fast()).expect("a verdict");
    assert!(matches!(s.outcome, Outcome::Dropped { .. }));
    assert_eq!(
        fx.chain.nonces(&[id(k::CALLER)]).expect("nonce")[0].0,
        0,
        "nothing applied"
    );
    assert_eq!(fx.chain.sent().len(), 1, "never resent automatically");
    assert!(is_empty_dir(dir.path()), "lock released");
}

#[test]
fn a_second_transaction_of_the_same_signer_is_refused_while_one_is_in_flight() {
    let fx = standard::standard();
    let dir = tempfile::tempdir().expect("tmp");
    let h = fx.chain.last_block_id().expect("height");
    let held = InFlightLock::acquire(dir.path(), &id(k::CALLER), 0, h, &fast()).expect("first");
    let e = run_observe(&fx, dir.path(), &fast()).expect_err("in flight");
    assert!(matches!(e, SdkError::InFlight { .. }));
    assert!(fx.chain.sent().is_empty(), "nothing was sent");
    assert_eq!(
        read_record(held.path()).owner,
        held.owner(),
        "the live lock is untouched"
    );
    drop(held);
    run_observe(&fx, dir.path(), &fast()).expect("free again");
}

#[test]
fn a_lock_left_by_a_crashed_process_is_recovered() {
    let fx = standard::standard();
    let dir = tempfile::tempdir().expect("tmp");
    let lock = dir.path().join(format!("{}.lock", id(k::CALLER)));
    // A crashed process left a lock taken at height 0, nonce 0 (no owner field).
    fs::write(&lock, "{\"nonce\":0,\"height\":0}").expect("write");
    let left = read_record(&lock);
    assert_eq!(left.owner, "");

    // Height 3 is inside the window 0..5: the lock is live and must not be stolen.
    let h = fx.chain.last_block_id().expect("height");
    assert_eq!(h, 3);
    assert!(!left.is_stale(0, h, &fast()));
    assert!(matches!(
        InFlightLock::acquire(dir.path(), &id(k::CALLER), 0, h, &fast()),
        Err(SdkError::InFlight { .. })
    ));
    assert_eq!(read_record(&lock), left, "the live lock was not modified");

    // Three more blocks: height 6 is past the window, so the lock is stale and is replaced.
    for _ in 0..3 {
        fx.chain.push_block(vec![]);
    }
    let h = fx.chain.last_block_id().expect("height");
    assert_eq!(h, 6);
    assert!(left.is_stale(0, h, &fast()), "detected as stale");
    let taken =
        InFlightLock::acquire(dir.path(), &id(k::CALLER), 0, h, &fast()).expect("recovered");
    let now = read_record(&lock);
    assert_eq!(
        now.owner,
        taken.owner(),
        "the file holds the new owner's identity"
    );
    assert_ne!(now.owner, left.owner);
    assert_eq!(
        (now.nonce, now.height),
        (0, 6),
        "with the new owner's nonce and height"
    );
    drop(taken);
    assert!(is_empty_dir(dir.path()), "the new owner releases it");

    // End to end: a stale lock does not block a submission.
    fs::write(&lock, "{\"nonce\":0,\"height\":0}").expect("write");
    let s = run_observe(&fx, dir.path(), &fast())
        .expect("height 6 passed the window of a lock taken at height 0");
    assert_eq!(s.outcome, Outcome::Included { nonce_after: 1 });

    // A torn lock (crash mid-write) is replaced too.
    fs::write(&lock, "{ torn").expect("write");
    let again = InFlightLock::acquire(dir.path(), &id(k::CALLER), 1, 6, &fast())
        .expect("a torn lock is replaced");
    let now = read_record(&lock);
    assert_eq!(now.owner, again.owner());
    assert_eq!((now.nonce, now.height), (1, 6));
}

#[test]
fn a_lock_is_stale_once_the_nonce_moved_or_the_window_ended() {
    let r = LockRecord {
        nonce: 0,
        height: 10,
        owner: "x".into(),
    };
    assert!(!r.is_stale(0, 12, &fast()));
    assert!(r.is_stale(1, 12, &fast()), "applied: resolved");
    assert!(!r.is_stale(0, 14, &fast()));
    assert!(r.is_stale(0, 15, &fast()), "window over: dropped");
}

#[test]
fn an_old_owner_does_not_remove_a_lock_taken_over_by_another() {
    let dir = tempfile::tempdir().expect("tmp");
    let signer = id(k::CALLER);
    let first = InFlightLock::acquire(dir.path(), &signer, 0, 0, &fast()).expect("first");
    // The window passed; a second process takes over while the first is still alive.
    let second = InFlightLock::acquire(dir.path(), &signer, 0, 9, &fast()).expect("takeover");
    drop(first);
    assert_eq!(
        read_record(second.path()).owner,
        second.owner(),
        "second still holds it"
    );
    drop(second);
    assert!(is_empty_dir(dir.path()));
}

#[test]
fn a_rejected_send_releases_the_lock_and_no_verdict_times_out() {
    let fx = standard::standard();
    let dir = tempfile::tempdir().expect("tmp");
    fx.chain.set_send_mode(SendMode::Reject);
    let e = run_observe(&fx, dir.path(), &fast()).expect_err("rejected");
    assert!(matches!(e, SdkError::Chain(_)));
    assert!(is_empty_dir(dir.path()));
    fx.chain.set_send_mode(SendMode::Drop);
    let few = ConfirmSettings {
        max_polls: 3,
        ..fast()
    };
    let e = run_observe(&fx, dir.path(), &few).expect_err("no verdict");
    assert!(matches!(e, SdkError::Unconfirmed(m) if m.contains("no confirmation")));
    assert!(
        !is_empty_dir(dir.path()),
        "an ambiguous outcome keeps the lock: the transaction may still land"
    );
    // The kept lock blocks a second submission inside the window and clears itself after it.
    let e = run_observe(&fx, dir.path(), &fast()).expect_err("still guarded");
    assert!(matches!(e, SdkError::InFlight { .. }));
    for _ in 0..5 {
        fx.chain.push_block(vec![]);
    }
    fx.chain.set_send_mode(SendMode::Include);
    run_observe(&fx, dir.path(), &fast()).expect("the stale kept lock is replaced");
    assert!(is_empty_dir(dir.path()));
}

/// Delegates to a fixture but reports every send as having an unknown result.
struct UncertainSend<'a>(&'a crate::testing::FixtureChain);

impl Chain for UncertainSend<'_> {
    fn account(&self, i: &AccountId) -> SdkResult<lee_core::account::Account> {
        self.0.account(i)
    }
    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<lee_core::account::Nonce>> {
        self.0.nonces(ids)
    }
    fn last_block_id(&self) -> SdkResult<u64> {
        self.0.last_block_id()
    }
    fn blocks(&self, a: u64, b: u64) -> SdkResult<Vec<crate::chain::BlockSummary>> {
        self.0.blocks(a, b)
    }
    fn send(&self, _: &lee::PublicTransaction) -> SdkResult<[u8; 32]> {
        Err(SdkError::SendUncertain("connection reset".into()))
    }
}

/// Delegates to a fixture, but every `nonces` call after the two pre-send reads (before and under
/// the lock) fails.
struct FlakyPolling<'a>(&'a crate::testing::FixtureChain, std::cell::Cell<u32>);

impl Chain for FlakyPolling<'_> {
    fn account(&self, i: &AccountId) -> SdkResult<lee_core::account::Account> {
        self.0.account(i)
    }
    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<lee_core::account::Nonce>> {
        self.1.set(self.1.get() + 1);
        if self.1.get() > 2 {
            return Err(SdkError::Chain("connection lost".into()));
        }
        self.0.nonces(ids)
    }
    fn last_block_id(&self) -> SdkResult<u64> {
        self.0.last_block_id()
    }
    fn blocks(&self, a: u64, b: u64) -> SdkResult<Vec<crate::chain::BlockSummary>> {
        self.0.blocks(a, b)
    }
    fn send(&self, tx: &lee::PublicTransaction) -> SdkResult<[u8; 32]> {
        self.0.send(tx)
    }
}

/// Delegates to a fixture; right after the first `nonces` read, another transaction of the same
/// signer lands (its nonce moves), as if a concurrent process had just released its lock.
struct LandsBeforeTheLock<'a>(&'a crate::testing::FixtureChain, std::cell::Cell<u32>);

impl Chain for LandsBeforeTheLock<'_> {
    fn account(&self, i: &AccountId) -> SdkResult<lee_core::account::Account> {
        self.0.account(i)
    }
    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<lee_core::account::Nonce>> {
        let read = self.0.nonces(ids);
        self.1.set(self.1.get() + 1);
        if self.1.get() == 1 {
            self.0
                .set_nonce(ids[0], read.as_ref().expect("read")[0].0 + 1);
        }
        read
    }
    fn last_block_id(&self) -> SdkResult<u64> {
        self.0.last_block_id()
    }
    fn blocks(&self, a: u64, b: u64) -> SdkResult<Vec<crate::chain::BlockSummary>> {
        self.0.blocks(a, b)
    }
    fn send(&self, tx: &lee::PublicTransaction) -> SdkResult<[u8; 32]> {
        self.0.send(tx)
    }
}

#[test]
fn the_baseline_is_re_read_under_the_lock() {
    let fx = standard::standard();
    let dir = tempfile::tempdir().expect("tmp");
    let chain = LandsBeforeTheLock(&fx.chain, std::cell::Cell::new(0));
    let s = submit(
        &chain,
        &TestKeys::of(&[k::CALLER]),
        &observe_plan(&fx),
        dir.path(),
        &fast(),
        &|_| {},
    )
    .expect("submitted");
    let sent = fx.chain.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].message().nonces,
        vec![lee_core::account::Nonce(1)],
        "signed at the nonce read under the lock, not the stale 0"
    );
    // Confirmed against nonce 1: only this transaction (nonce 1 -> 2) counts as Included.
    assert_eq!(s.outcome, Outcome::Included { nonce_after: 2 });
    assert!(is_empty_dir(dir.path()), "lock released");
}

/// Delegates to a fixture; the first height read is 10, every later one 11 (a block between the
/// read before the lock and the read under it). Every send has an unknown result, so the lock stays
/// on disk for inspection.
struct BlockBeforeTheLock<'a>(&'a crate::testing::FixtureChain, std::cell::Cell<u32>);

impl Chain for BlockBeforeTheLock<'_> {
    fn account(&self, i: &AccountId) -> SdkResult<lee_core::account::Account> {
        self.0.account(i)
    }
    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<lee_core::account::Nonce>> {
        self.0.nonces(ids)
    }
    fn last_block_id(&self) -> SdkResult<u64> {
        self.1.set(self.1.get() + 1);
        Ok(if self.1.get() == 1 { 10 } else { 11 })
    }
    fn blocks(&self, a: u64, b: u64) -> SdkResult<Vec<crate::chain::BlockSummary>> {
        self.0.blocks(a, b)
    }
    fn send(&self, _: &lee::PublicTransaction) -> SdkResult<[u8; 32]> {
        Err(SdkError::SendUncertain("connection reset".into()))
    }
}

#[test]
fn the_lock_record_holds_the_height_read_under_the_lock() {
    let fx = standard::standard();
    let dir = tempfile::tempdir().expect("tmp");
    let chain = BlockBeforeTheLock(&fx.chain, std::cell::Cell::new(0));
    let e = submit(
        &chain,
        &TestKeys::of(&[k::CALLER]),
        &observe_plan(&fx),
        dir.path(),
        &fast(),
        &|_| {},
    )
    .expect_err("unknown result");
    assert!(matches!(e, SdkError::SendUncertain(_)));
    let lock = dir.path().join(format!("{}.lock", id(k::CALLER)));
    let r = read_record(&lock);
    assert_eq!(
        (r.nonce, r.height),
        (0, 11),
        "the re-read baseline, not height 10"
    );
    assert!(!r.owner.is_empty());
    assert_eq!(
        fs::read_dir(dir.path()).expect("dir").count(),
        1,
        "no temp files left"
    );
}

#[test]
fn rebase_refuses_a_lock_taken_over_by_another_process() {
    let dir = tempfile::tempdir().expect("tmp");
    let lock = InFlightLock::acquire(dir.path(), &id(k::CALLER), 0, 3, &fast()).expect("lock");
    fs::write(
        lock.path(),
        "{\"nonce\":0,\"height\":3,\"owner\":\"someone-else\"}",
    )
    .expect("takeover");
    let e = lock.rebase(0, 4).expect_err("not ours");
    assert!(matches!(e, SdkError::InFlight { .. }));
    assert_eq!(read_record(lock.path()).owner, "someone-else", "untouched");
}

#[test]
fn a_polling_failure_after_the_send_is_unconfirmed_and_keeps_the_lock() {
    let fx = standard::standard();
    let dir = tempfile::tempdir().expect("tmp");
    let chain = FlakyPolling(&fx.chain, std::cell::Cell::new(0));
    let e = submit(
        &chain,
        &TestKeys::of(&[k::CALLER]),
        &observe_plan(&fx),
        dir.path(),
        &fast(),
        &|_| {},
    )
    .expect_err("polling fails");
    assert!(
        matches!(&e, SdkError::Unconfirmed(m) if m.contains("connection lost")),
        "{e}"
    );
    assert_eq!(fx.chain.sent().len(), 1);
    assert!(dir.path().join(format!("{}.lock", id(k::CALLER))).exists());
}

#[test]
fn an_ambiguous_send_error_keeps_the_lock() {
    let fx = standard::standard();
    let dir = tempfile::tempdir().expect("tmp");
    let chain = UncertainSend(&fx.chain);
    let plan = observe_plan(&fx);
    let e = submit(
        &chain,
        &TestKeys::of(&[k::CALLER]),
        &plan,
        dir.path(),
        &fast(),
        &|_| {},
    )
    .expect_err("unknown result");
    assert!(matches!(e, SdkError::SendUncertain(_)));
    let lock = dir.path().join(format!("{}.lock", id(k::CALLER)));
    assert!(lock.exists(), "kept");
    let e = submit(
        &chain,
        &TestKeys::of(&[k::CALLER]),
        &plan,
        dir.path(),
        &fast(),
        &|_| {},
    )
    .expect_err("guarded");
    assert!(matches!(e, SdkError::InFlight { .. }));
    assert_eq!(
        fs::read_dir(dir.path()).expect("dir").count(),
        1,
        "no temp files left"
    );
}

#[test]
fn an_empty_nonce_reply_is_an_error_not_nonce_zero() {
    struct NoNonces;
    impl Chain for NoNonces {
        fn account(&self, _: &AccountId) -> SdkResult<lee_core::account::Account> {
            Ok(lee_core::account::Account::default())
        }
        fn nonces(&self, _: &[AccountId]) -> SdkResult<Vec<lee_core::account::Nonce>> {
            Ok(vec![])
        }
        fn last_block_id(&self) -> SdkResult<u64> {
            Ok(0)
        }
        fn blocks(&self, _: u64, _: u64) -> SdkResult<Vec<crate::chain::BlockSummary>> {
            Ok(vec![])
        }
        fn send(&self, _: &lee::PublicTransaction) -> SdkResult<[u8; 32]> {
            Ok([0; 32])
        }
    }
    let e = confirm(&NoNonces, &id(k::CALLER), 0, 0, &fast(), &|_| {}).expect_err("empty reply");
    assert!(matches!(e, SdkError::Chain(m) if m.contains("no nonce")));
}

/// Height 10 at the first read and 14 afterwards, drop window 5: the re-read under the lock is one
/// block short of the window end (15). The record still says 10 until `rebase` renames it, so a
/// block landing in between would let another process judge it stale and take it over (Plan 3a N1).
/// The lock is re-taken from a fresh baseline instead of being rebased so close to the edge.
struct NearWindowEnd<'a> {
    fx: &'a crate::testing::FixtureChain,
    reads: std::cell::Cell<u32>,
    reads_at_send: std::cell::Cell<u32>,
}

impl Chain for NearWindowEnd<'_> {
    fn account(&self, i: &AccountId) -> SdkResult<lee_core::account::Account> {
        self.fx.account(i)
    }
    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<lee_core::account::Nonce>> {
        self.fx.nonces(ids)
    }
    fn last_block_id(&self) -> SdkResult<u64> {
        self.reads.set(self.reads.get() + 1);
        Ok(if self.reads.get() == 1 { 10 } else { 14 })
    }
    fn blocks(&self, a: u64, b: u64) -> SdkResult<Vec<crate::chain::BlockSummary>> {
        self.fx.blocks(a, b)
    }
    fn send(&self, _: &lee::PublicTransaction) -> SdkResult<[u8; 32]> {
        self.reads_at_send.set(self.reads.get());
        Err(SdkError::SendUncertain("connection reset".into()))
    }
}

#[test]
fn a_re_read_one_block_short_of_the_window_end_retakes_the_lock() {
    let fx = standard::standard();
    let dir = tempfile::tempdir().expect("tmp");
    let chain = NearWindowEnd {
        fx: &fx.chain,
        reads: std::cell::Cell::new(0),
        reads_at_send: std::cell::Cell::new(0),
    };
    let e = submit(
        &chain,
        &TestKeys::of(&[k::CALLER]),
        &observe_plan(&fx),
        dir.path(),
        &fast(),
        &|_| {},
    )
    .expect_err("unknown result");
    assert!(matches!(e, SdkError::SendUncertain(_)));
    assert_eq!(
        chain.reads_at_send.get(),
        4,
        "baseline 10, re-read 14 (too close to 15): retry with baseline 14, re-read 14"
    );
    let r = read_record(&dir.path().join(format!("{}.lock", id(k::CALLER))));
    assert_eq!(r.height, 14);
}

/// The signer's nonce moves on every read: every attempt sees a stale baseline under the lock.
struct NonceAlwaysMoves<'a>(&'a crate::testing::FixtureChain, std::cell::Cell<u128>);

impl Chain for NonceAlwaysMoves<'_> {
    fn account(&self, i: &AccountId) -> SdkResult<lee_core::account::Account> {
        self.0.account(i)
    }
    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<lee_core::account::Nonce>> {
        let n = self.1.get();
        self.1.set(n + 1);
        Ok(ids.iter().map(|_| lee_core::account::Nonce(n)).collect())
    }
    fn last_block_id(&self) -> SdkResult<u64> {
        self.0.last_block_id()
    }
    fn blocks(&self, a: u64, b: u64) -> SdkResult<Vec<crate::chain::BlockSummary>> {
        self.0.blocks(a, b)
    }
    fn send(&self, _: &lee::PublicTransaction) -> SdkResult<[u8; 32]> {
        panic!("nothing may be sent without a current baseline")
    }
}

#[test]
fn a_nonce_that_moves_on_every_read_exhausts_the_retries_and_leaves_no_lock() {
    let fx = standard::standard();
    let dir = tempfile::tempdir().expect("tmp");
    let chain = NonceAlwaysMoves(&fx.chain, std::cell::Cell::new(0));
    let e = submit(
        &chain,
        &TestKeys::of(&[k::CALLER]),
        &observe_plan(&fx),
        dir.path(),
        &fast(),
        &|_| {},
    )
    .expect_err("in flight");
    assert!(matches!(e, SdkError::InFlight { .. }), "{e}");
    assert_eq!(chain.1.get(), 6, "LOCK_ATTEMPTS = 3, two nonce reads each");
    assert!(is_empty_dir(dir.path()), "every attempt released its lock");
}

/// The transaction was sent, then the node client panics while polling: the transaction may still
/// land, so unwinding must keep the lock (keep_on_drop is set before `send`).
struct PanicsAfterSend<'a>(&'a crate::testing::FixtureChain, std::cell::Cell<bool>);

impl Chain for PanicsAfterSend<'_> {
    fn account(&self, i: &AccountId) -> SdkResult<lee_core::account::Account> {
        self.0.account(i)
    }
    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<lee_core::account::Nonce>> {
        assert!(!self.1.get(), "node client crashed while polling");
        self.0.nonces(ids)
    }
    fn last_block_id(&self) -> SdkResult<u64> {
        self.0.last_block_id()
    }
    fn blocks(&self, a: u64, b: u64) -> SdkResult<Vec<crate::chain::BlockSummary>> {
        self.0.blocks(a, b)
    }
    fn send(&self, tx: &lee::PublicTransaction) -> SdkResult<[u8; 32]> {
        self.1.set(true);
        self.0.send(tx)
    }
}

#[test]
fn a_panic_after_the_send_keeps_the_lock() {
    let fx = standard::standard();
    let dir = tempfile::tempdir().expect("tmp");
    let chain = PanicsAfterSend(&fx.chain, std::cell::Cell::new(false));
    let plan = observe_plan(&fx);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        submit(
            &chain,
            &TestKeys::of(&[k::CALLER]),
            &plan,
            dir.path(),
            &fast(),
            &|_| {},
        )
    }));
    assert!(caught.is_err(), "the poll panicked");
    let lock = dir.path().join(format!("{}.lock", id(k::CALLER)));
    assert!(lock.exists(), "unwinding after the send keeps the lock");
    assert_eq!(fx.chain.sent().len(), 1);
}
