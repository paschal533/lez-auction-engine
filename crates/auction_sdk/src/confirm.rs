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

use lee_core::account::{AccountId, Nonce};
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
    Err(SdkError::Unconfirmed(format!(
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
    /// `n0`/`h0` are the signer's current nonce and the current height. The lock file is written to a
    /// temp file and hard-linked into place (`create_new` where hard links are unavailable), so of two
    /// racing processes exactly one creates it, with its full content. A
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

    /// Rewrites the record with `height` (the baseline re-read under the lock), so the lock and the
    /// confirmation share one baseline. The new record goes to a private temp file that is renamed
    /// over the lock, so a reader sees the old or the new record, never a torn one. Only a lock this
    /// process still owns is rewritten; a lock taken over meanwhile is `InFlight`.
    pub fn rebase(&self, nonce: u128, height: u64) -> SdkResult<()> {
        let taken_over = || SdkError::InFlight {
            signer: self
                .path
                .file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().into_owned()),
            lock: self.path.display().to_string(),
            blocks: 0,
        };
        match read_lock(&self.path)? {
            Some(Found::Record(r)) if r.owner == self.owner => {}
            _ => return Err(taken_over()),
        }
        let record = serde_json::to_string(&LockRecord {
            nonce,
            height,
            owner: self.owner.clone(),
        })
        .map_err(|e| SdkError::Io {
            path: self.path.display().to_string(),
            reason: e.to_string(),
        })?;
        let tmp = self
            .path
            .with_extension(format!("lock.{}.rebase.tmp", self.owner));
        fs::write(&tmp, record.as_bytes())
            .and_then(|()| fs::rename(&tmp, &self.path))
            .map_err(|e| {
                let _ = fs::remove_file(&tmp);
                io_err(&self.path, &e)
            })
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

/// Attempts at taking the lock with a baseline that is still current once the lock is held.
const LOCK_ATTEMPTS: u32 = 3;
/// Blocks that must remain in the drop window when the baseline is re-read under the lock. Until
/// `rebase` renames its record into place, the lock still says `h0`, and a reader judges it stale at
/// `h0 + drop_after_blocks`. With one block of margin a takeover would need two blocks between the
/// re-read and the rename (microseconds apart; blocks are about 15 s), so it cannot happen.
const REBASE_MARGIN_BLOCKS: u64 = 1;

/// The signers' nonces, then the height: one baseline for the lock and for confirmation.
fn baseline<C: Chain + ?Sized>(chain: &C, signers: &[AccountId]) -> SdkResult<(Vec<Nonce>, u64)> {
    let nonces = chain.nonces(signers)?;
    if nonces.len() != signers.len() || nonces.is_empty() {
        return Err(SdkError::Chain(format!(
            "the node returned {} nonces for {} signers",
            nonces.len(),
            signers.len()
        )));
    }
    Ok((nonces, chain.last_block_id()?))
}

/// Takes the signer's lock, then re-reads the baseline under it. A nonce that moved between the
/// first read and the lock (another transaction of this signer landed meanwhile) would make the
/// lock record and the confirmation baseline stale, and a later poll could report that other
/// transaction as this one (`Included`): the lock is released and the read repeated. The height is
/// taken from the re-read and written into the lock record (`rebase`), so the lock and the
/// confirmation share one baseline and the drop window starts no earlier than the signing. A height
/// that already moved a whole drop window (the record would look resolved to others) also retries.
/// A re-read within REBASE_MARGIN_BLOCKS of the window end also retries (Plan 3a N1).
fn lock_with_current_baseline<C: Chain + ?Sized>(
    chain: &C,
    signers: &[AccountId],
    lock_dir: &Path,
    s: &ConfirmSettings,
) -> SdkResult<(InFlightLock, Vec<Nonce>, u64)> {
    let primary = signers[0];
    for _ in 0..LOCK_ATTEMPTS {
        let (nonces, h0) = baseline(chain, signers)?;
        let lock = InFlightLock::acquire(lock_dir, &primary, nonces[0].0, h0, s)?;
        let (current, h1) = baseline(chain, signers)?;
        if current == nonces
            && h1.saturating_add(REBASE_MARGIN_BLOCKS) < h0.saturating_add(s.drop_after_blocks)
        {
            if h1 != h0 {
                lock.rebase(current[0].0, h1)?;
            }
            return Ok((lock, current, h1));
        }
        drop(lock);
    }
    Err(SdkError::InFlight {
        signer: primary.to_string(),
        lock: lock_dir
            .join(format!("{primary}.lock"))
            .display()
            .to_string(),
        blocks: s.drop_after_blocks,
    })
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
    let (mut lock, nonces, h0) =
        lock_with_current_baseline(chain, &plan.signing_requirements, lock_dir, s)?;
    let n0 = nonces[0].0;
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
    // The send is done: any failure to reach a verdict is an unknown outcome, never a rejection.
    let outcome = confirm(chain, &primary, n0, h0, s, sleep).map_err(|e| match e {
        e @ SdkError::Unconfirmed(_) => e,
        other => SdkError::Unconfirmed(format!("sent, but confirming failed: {other}")),
    })?;
    lock.set_keep_on_drop(false);
    Ok(Submitted { hash, outcome })
}

#[cfg(test)]
#[path = "confirm_tests.rs"]
mod tests;
