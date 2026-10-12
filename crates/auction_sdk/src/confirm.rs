//! Transaction confirmation (spec §6.5, ADR-009, ADR-042). On LEZ v0.2.4 `sendTransaction` success means
//! accepted, not executed: a transaction that fails at block build is silently dropped, and the signer
//! nonce advances only on apply. So: record the primary signer's nonce `n0` and the height `h0`, send,
//! poll the nonce; `nonce > n0` -> Included; still `n0` at height `h0 + drop_after_blocks` -> Dropped
//! (nothing was applied: re-quote). One transaction in flight per signer, enforced by a lock file.
//! A dropped transaction is never resent by the SDK.
//!
//! Lock staleness is decided on chain facts only (nonce and height), never on a process id or a file
//! age: a pid is meaningless across machines, containers, WSL and Windows, and can be reused, so a
//! pid test could steal a live lock or keep a dead one forever. The `owner` field only identifies
//! which process wrote the file, so that a process never removes a lock it no longer owns.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lee_core::account::AccountId;
use serde::{Deserialize, Serialize};

use crate::{
    chain::Chain,
    error::{SdkError, SdkResult},
    plan::Plan,
    tx::{sign, KeySource},
};

/// Re-reads of an empty lock file (a writer between `create_new` and its single `write_all`).
const EMPTY_LOCK_RETRIES: u32 = 5;
const EMPTY_LOCK_WAIT: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfirmSettings {
    pub poll_interval: Duration,
    pub drop_after_blocks: u64,
    pub max_polls: u32,
}

impl Default for ConfirmSettings {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(2),
            drop_after_blocks: 5,
            max_polls: 900,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Included {
        nonce_after: u128,
    },
    /// Nothing was applied; re-quote before sending anything again.
    Dropped {
        height: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Submitted {
    pub hash: [u8; 32],
    pub outcome: Outcome,
}

/// The signer's nonce; an empty reply is an error, never nonce 0.
fn signer_nonce<C: Chain + ?Sized>(chain: &C, signer: &AccountId) -> SdkResult<u128> {
    chain
        .nonces(std::slice::from_ref(signer))?
        .first()
        .map(|n| n.0)
        .ok_or_else(|| SdkError::Chain(format!("the node returned no nonce for {signer}")))
}

pub fn confirm<C: Chain + ?Sized>(
    chain: &C,
    signer: &AccountId,
    n0: u128,
    h0: u64,
    s: &ConfirmSettings,
    sleep: &dyn Fn(Duration),
) -> SdkResult<Outcome> {
    for _ in 0..s.max_polls {
        // Height FIRST, then nonce: the sequencer applies state (the nonce moves) before it moves the
        // tip, so a transaction that lands while we poll shows up in the later nonce read. Reading the
        // nonce first could pair a stale nonce with a fresh height and report a false Dropped, which
        // the caller would answer by acting twice.
        let height = chain.last_block_id()?;
        let nonce = signer_nonce(chain, signer)?;
        if nonce > n0 {
            return Ok(Outcome::Included { nonce_after: nonce });
        }
        if height >= h0.saturating_add(s.drop_after_blocks) {
            return Ok(Outcome::Dropped { height });
        }
        sleep(s.poll_interval);
    }
    Err(SdkError::Chain(format!(
        "no confirmation after {} polls; the transaction may still land: check the nonce of {signer} before sending again",
        s.max_polls
    )))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LockRecord {
    nonce: u128,
    height: u64,
    /// Identity of the process that wrote the lock (`pid-nanos-counter`); empty in locks written
    /// without one.
    #[serde(default)]
    owner: String,
}

impl LockRecord {
    /// A lock is resolved (nothing can still be in flight under it) once the signer's nonce moved past
    /// it (the transaction was applied) or the drop window ended (it was dropped).
    fn is_stale(&self, n0: u128, h0: u64, s: &ConfirmSettings) -> bool {
        n0 > self.nonce || h0 >= self.window_end(s)
    }

    fn window_end(&self, s: &ConfirmSettings) -> u64 {
        self.height.saturating_add(s.drop_after_blocks)
    }
}

fn new_owner() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{nanos}-{n}", std::process::id())
}

fn io_err(path: &Path, e: &std::io::Error) -> SdkError {
    SdkError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    }
}

/// What `read_lock` found: nothing, a lock record, or text that does not parse (torn).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Found {
    Record(LockRecord),
    Torn(String),
}

/// Reads a lock file. `Ok(None)` when absent. An empty file is re-read briefly (its writer may be
/// between create and write); text that still does not parse is `Found::Torn`.
fn read_lock(path: &Path) -> SdkResult<Option<Found>> {
    for attempt in 0..=EMPTY_LOCK_RETRIES {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io_err(path, &e)),
        };
        if text.is_empty() && attempt < EMPTY_LOCK_RETRIES {
            std::thread::sleep(EMPTY_LOCK_WAIT);
            continue;
        }
        return Ok(Some(match serde_json::from_str(&text) {
            Ok(r) => Found::Record(r),
            Err(_) => Found::Torn(text),
        }));
    }
    Ok(None)
}

/// Creates `path` holding `record`, failing with `already_held()` when it exists. The record is
/// written to a private temp file and hard-linked to `path`, so the lock appears atomically with its
/// full content (a slow writer is never seen as an empty or torn lock). Where hard links are not
/// available (some network or `/mnt/c` mounts) it falls back to `create_new` + write, which is still
/// exclusive but briefly visible empty (readers retry an empty file). A failed write leaves no file.
fn create_exclusive(
    path: &Path,
    record: &str,
    owner: &str,
    already_held: impl Fn() -> SdkError,
) -> SdkResult<()> {
    let tmp = path.with_extension(format!("lock.{owner}.tmp"));
    fs::write(&tmp, record.as_bytes()).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        io_err(&tmp, &e)
    })?;
    let linked = fs::hard_link(&tmp, path);
    let _ = fs::remove_file(&tmp);
    match linked {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(already_held()),
        Err(_) => create_new_fallback(path, record, already_held),
    }
}

fn create_new_fallback(
    path: &Path,
    record: &str,
    already_held: impl Fn() -> SdkError,
) -> SdkResult<()> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => already_held(),
            // Windows reports a lock file that is being deleted by its holder as access denied.
            std::io::ErrorKind::PermissionDenied if cfg!(windows) => already_held(),
            _ => io_err(path, &e),
        })?;
    f.write_all(record.as_bytes()).map_err(|e| {
        drop(f);
        let _ = fs::remove_file(path);
        io_err(path, &e)
    })
}

/// One in-flight transaction per signer, across processes (spec §6.5 step 4).
#[derive(Debug)]
pub struct InFlightLock {
    path: PathBuf,
    owner: String,
    /// When set, dropping the lock leaves the file for the chain-fact staleness rule to clear.
    keep_on_drop: bool,
}

impl InFlightLock {
    /// `n0`/`h0` are the signer's current nonce and the current height. The lock file is created with
    /// `create_new` (`O_EXCL` / `CREATE_NEW`), so of two racing processes exactly one creates it. A
    /// stale or torn lock is removed first, and only after re-reading it unchanged, so a lock another
    /// process has just taken is not removed.
    pub fn acquire(
        dir: &Path,
        signer: &AccountId,
        n0: u128,
        h0: u64,
        s: &ConfirmSettings,
    ) -> SdkResult<Self> {
        fs::create_dir_all(dir).map_err(|e| io_err(dir, &e))?;
        let path = dir.join(format!("{signer}.lock"));
        let in_flight = |blocks: u64| SdkError::InFlight {
            signer: signer.to_string(),
            lock: path.display().to_string(),
            blocks,
        };
        if let Some(found) = read_lock(&path)? {
            if let Found::Record(r) = &found {
                if !r.is_stale(n0, h0, s) {
                    return Err(in_flight(r.window_end(s).saturating_sub(h0)));
                }
            }
            // Resolved, or torn by a crash: replace it, unless it changed since it was judged.
            if read_lock(&path)?.as_ref() != Some(&found) {
                return Err(in_flight(s.drop_after_blocks));
            }
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(io_err(&path, &e)),
            }
        }
        let owner = new_owner();
        let record = serde_json::to_string(&LockRecord {
            nonce: n0,
            height: h0,
            owner: owner.clone(),
        })
        .map_err(|e| SdkError::Io {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;
        create_exclusive(&path, &record, &owner, || in_flight(s.drop_after_blocks))?;
        Ok(Self {
            path,
            owner,
            keep_on_drop: false,
        })
    }

    /// While set, dropping the lock does not remove the file. `submit` sets it from the moment a send
    /// is attempted until a definite verdict, so an ambiguous outcome (timeout, unknown send result,
    /// panic) leaves the lock; it clears itself once the nonce or the height window passes it.
    pub fn set_keep_on_drop(&mut self, keep: bool) {
        self.keep_on_drop = keep;
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }
}

impl Drop for InFlightLock {
    fn drop(&mut self) {
        if self.keep_on_drop {
            return;
        }
        // Remove only a lock this process still owns: after a takeover of a stale lock the file
        // belongs to someone else. The read-then-remove is not atomic: a takeover between the two
        // calls can have its fresh lock removed here; the consequence is bounded (that signer may
        // briefly have two transactions in flight, and the loser fails on the nonce). A lock that
        // cannot be removed is recognised as resolved by the next `acquire`, so ignoring that error
        // loses nothing.
        if let Ok(Some(Found::Record(r))) = read_lock(&self.path) {
            if r.owner == self.owner {
                let _ = fs::remove_file(&self.path);
            }
        }
    }
}

pub fn submit<C: Chain + ?Sized>(
    chain: &C,
    keys: &dyn KeySource,
    plan: &Plan,
    lock_dir: &Path,
    s: &ConfirmSettings,
    sleep: &dyn Fn(Duration),
) -> SdkResult<Submitted> {
    let primary = *plan
        .signing_requirements
        .first()
        .ok_or_else(|| SdkError::Encode("plan has no signer".into()))?;
    let nonces = chain.nonces(&plan.signing_requirements)?;
    let n0 = nonces
        .first()
        .map(|n| n.0)
        .ok_or_else(|| SdkError::Chain(format!("the node returned no nonce for {primary}")))?;
    let h0 = chain.last_block_id()?;
    let mut lock = InFlightLock::acquire(lock_dir, &primary, n0, h0, s)?;
    let tx = sign(plan, &nonces, keys)?;
    // From the send attempt until a definite verdict the lock stays on disk, whatever happens
    // (an unwinding panic included): the transaction may still land.
    lock.set_keep_on_drop(true);
    let hash = match chain.send(&tx) {
        Ok(h) => h,
        Err(e @ SdkError::SendUncertain(_)) => return Err(e),
        Err(e) => {
            // A definite rejection: nothing was accepted.
            lock.set_keep_on_drop(false);
            return Err(e);
        }
    };
    let outcome = confirm(chain, &primary, n0, h0, s, sleep)?;
    lock.set_keep_on_drop(false);
    Ok(Submitted { hash, outcome })
}

#[cfg(test)]
mod tests {
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

    fn run_observe(
        fx: &standard::Fixture,
        dir: &Path,
        s: &ConfirmSettings,
    ) -> SdkResult<Submitted> {
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
        assert!(matches!(e, SdkError::Chain(m) if m.contains("no confirmation")));
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
        let e =
            confirm(&NoNonces, &id(k::CALLER), 0, 0, &fast(), &|_| {}).expect_err("empty reply");
        assert!(matches!(e, SdkError::Chain(m) if m.contains("no nonce")));
    }
}
