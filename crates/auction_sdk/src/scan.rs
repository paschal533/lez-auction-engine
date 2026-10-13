//! Discovery by block scan (spec §6.4, ADR-008, ADR-043): no account-listing RPC and no events exist
//! on v0.2.4. The scan keeps engine `liquidate` transactions that name our config (auction and
//! position ids) and host `open_position` transactions that name our `ProtocolParameters`
//! (position ids), and persists a cursor `{last_block, last_hash, anchor, recent, auctions,
//! positions}`.
//!
//! Completeness (ADR-043, amended): the cursor records which chain it was built on (`anchor`, the
//! hash of block [`ANCHOR_BLOCK`]) and the hashes of its last `reorg_depth + 1` blocks (`recent`).
//! A node with another anchor (restarted or replaced) discards the cursor and rescans from block 1.
//! A rewind is accepted only when it reaches genesis or the node still has the rewind block with the
//! hash the scan recorded there; otherwise the scan fails and asks for `rescan --from 1` instead of
//! succeeding over blocks it never saw.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use auction_engine_core::Instruction;
use lee_core::{account::AccountId, program::ProgramId};
use serde::{Deserialize, Serialize};

use crate::{
    chain::{BlockSummary, Chain},
    encode::{variant_index, words},
    error::{SdkError, SdkResult},
    verify::VerifiedConfig,
};

/// The block whose hash identifies a chain. Not block 1: the LEZ v0.2.4 genesis block is built with
/// `timestamp: 0` from the sequencer config, so a restarted localnet reproduces its hash. Block 2 is
/// the first block stamped with the wall clock, so two runs of the same node differ there.
pub const ANCHOR_BLOCK: u64 = 2;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub last_block: u64,
    /// Hex hash of `last_block` ("" before the first scan).
    pub last_hash: String,
    /// Hex hash of block [`ANCHOR_BLOCK`] of the chain this cursor was built on. "" means unknown:
    /// a fresh cursor, a chain shorter than the anchor, or a cursor written before anchors existed.
    #[serde(default)]
    pub anchor: String,
    /// Hex hashes of the last `reorg_depth + 1` scanned blocks, so that a rewind can be checked
    /// against what the scan actually saw.
    #[serde(default)]
    pub recent: BTreeMap<u64, String>,
    pub auctions: BTreeSet<String>,
    pub positions: BTreeSet<String>,
}

fn parse_ids(set: &BTreeSet<String>) -> SdkResult<Vec<AccountId>> {
    set.iter()
        .map(|s| {
            s.parse::<AccountId>()
                .map_err(|_| SdkError::Scan(format!("cursor holds an invalid account id {s}")))
        })
        .collect()
}

impl Cursor {
    pub fn auction_ids(&self) -> SdkResult<Vec<AccountId>> {
        parse_ids(&self.auctions)
    }

    pub fn position_ids(&self) -> SdkResult<Vec<AccountId>> {
        parse_ids(&self.positions)
    }
}

/// Why a scan discarded the stored cursor and started again from block 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restart {
    /// The node's block [`ANCHOR_BLOCK`] differs from the cursor's anchor: another chain.
    ChainReplaced,
    /// The cursor predates anchors, so the chain it was built on is unknown.
    NoAnchor,
}

impl std::fmt::Display for Restart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ChainReplaced => write!(
                f,
                "the node serves another chain than the cursor was built on (block {ANCHOR_BLOCK} differs: restarted or replaced node); discarded the cursor and rescanned from block 1"
            ),
            Self::NoAnchor => write!(
                f,
                "the cursor does not record which chain it was built on (older format); rescanned from block 1"
            ),
        }
    }
}

/// A scan result: the new cursor, and whether the stored one was discarded (tell the user).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scanned {
    pub cursor: Cursor,
    pub restarted: Option<Restart>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanTargets {
    pub engine_program: ProgramId,
    pub config: AccountId,
    pub host_program: ProgramId,
    pub protocol_parameters: AccountId,
}

impl ScanTargets {
    #[must_use]
    pub fn of(v: &VerifiedConfig) -> Self {
        Self {
            engine_program: v.engine.program,
            config: v.config_id,
            host_program: v.host.program,
            protocol_parameters: v.host.protocol_parameters,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanSettings {
    pub batch: u64,
    pub reorg_depth: u64,
}

impl Default for ScanSettings {
    fn default() -> Self {
        Self {
            batch: 100,
            reorg_depth: 10,
        }
    }
}

fn index_of<T: serde::Serialize>(ix: &T) -> SdkResult<u32> {
    variant_index(&words(ix)?).ok_or_else(|| SdkError::Encode("empty instruction".into()))
}

fn collect(t: &ScanTargets, b: &BlockSummary, next: &mut Cursor, liquidate: u32, open: u32) {
    for tx in &b.txs {
        let kind = variant_index(&tx.instruction_words);
        if tx.program_id == t.engine_program
            && kind == Some(liquidate)
            && tx.account_ids.get(2) == Some(&t.config)
        {
            if let Some(a) = tx.account_ids.get(6) {
                next.auctions.insert(a.to_string());
            }
            if let Some(p) = tx.account_ids.get(4) {
                next.positions.insert(p.to_string());
            }
        }
        if tx.program_id == t.host_program
            && kind == Some(open)
            && tx.account_ids.contains(&t.protocol_parameters)
        {
            if let Some(p) = tx.account_ids.get(1) {
                next.positions.insert(p.to_string());
            }
        }
    }
}

fn hash_at<C: Chain + ?Sized>(chain: &C, id: u64) -> SdkResult<String> {
    if id == 0 {
        return Ok(String::new());
    }
    let b = chain.blocks(id, id)?;
    b.first()
        .map(|b| hex::encode(b.hash))
        .ok_or_else(|| SdkError::Chain(format!("block {id} not found")))
}

/// The node's anchor, or "" while its chain is shorter than [`ANCHOR_BLOCK`].
fn chain_anchor<C: Chain + ?Sized>(chain: &C, head: u64) -> SdkResult<String> {
    if head < ANCHOR_BLOCK {
        return Ok(String::new());
    }
    hash_at(chain, ANCHOR_BLOCK)
}

/// The cursor to continue from on a node whose anchor is `anchor`, or a fresh one and why.
/// A cursor without an anchor that recorded nothing past genesis cannot have missed anything and is
/// kept; one that did is rebuilt, because the chain it was built on is unknown.
fn on_this_chain(cursor: &Cursor, anchor: &str) -> (Cursor, Option<Restart>) {
    if cursor.anchor.is_empty() {
        if cursor.last_block < ANCHOR_BLOCK {
            return (cursor.clone(), None);
        }
        return (Cursor::default(), Some(Restart::NoAnchor));
    }
    if cursor.anchor != anchor {
        return (Cursor::default(), Some(Restart::ChainReplaced));
    }
    (cursor.clone(), None)
}

/// Where to rewind to (block and its hash), or `None` when the cursor block is still on the node
/// with the stored hash. A rewind goes back `reorg_depth` blocks, to at most the head, and is
/// accepted only if it reaches genesis or the node's hash of the rewind block is the one the scan
/// recorded there; otherwise blocks below it may differ from the ones scanned and the scan fails.
/// A cursor block within the head that the node cannot return is a transient gap: the scan fails.
fn rewind_target<C: Chain + ?Sized>(
    chain: &C,
    head: u64,
    cursor: &Cursor,
    s: &ScanSettings,
) -> SdkResult<Option<(u64, String)>> {
    let n = cursor.last_block;
    if n == 0 {
        return Ok(None);
    }
    if n <= head {
        let on_node = hash_at(chain, n).map_err(|e| {
            SdkError::Scan(format!(
                "cursor block {n} is not on the node ({e}); run `rescan`"
            ))
        })?;
        if on_node == cursor.last_hash {
            return Ok(None);
        }
    }
    let target = n.saturating_sub(s.reorg_depth).min(head);
    if target == 0 {
        return Ok(Some((0, String::new())));
    }
    let on_node = hash_at(chain, target).map_err(|e| {
        SdkError::Scan(format!(
            "cannot rewind to block {target} ({e}); run `rescan --from 1`"
        ))
    })?;
    if cursor.recent.get(&target) != Some(&on_node) {
        let why = if n > head {
            format!("the cursor is at block {n} but the node's head is {head}")
        } else {
            format!("block {n} changed")
        };
        return Err(SdkError::Scan(format!(
            "{why}, and the node's block {target} is not the one scanned (the node was restarted or rebuilt more than {} blocks deep); blocks before it may have been missed: run `rescan --from 1`",
            s.reorg_depth
        )));
    }
    Ok(Some((target, on_node)))
}

/// `scan_report` without the restart flag.
pub fn scan<C: Chain + ?Sized>(
    chain: &C,
    t: &ScanTargets,
    cursor: &Cursor,
    s: &ScanSettings,
) -> SdkResult<Cursor> {
    scan_report(chain, t, cursor, s).map(|r| r.cursor)
}

fn check_block(b: &BlockSummary, want: u64, next: &Cursor) -> SdkResult<()> {
    if b.id != want {
        return Err(SdkError::Scan(format!(
            "expected block {want}, got {}",
            b.id
        )));
    }
    if !next.last_hash.is_empty() && hex::encode(b.prev_hash) != next.last_hash {
        return Err(SdkError::Scan(format!(
            "block {} does not extend the scanned chain; run `rescan`",
            b.id
        )));
    }
    if b.id == ANCHOR_BLOCK && hex::encode(b.hash) != next.anchor {
        return Err(SdkError::Scan(format!(
            "block {ANCHOR_BLOCK} changed during the scan; run the command again"
        )));
    }
    Ok(())
}

/// Scans the blocks after the cursor and returns the new cursor; never mutates its input, so a
/// failed scan leaves the caller's cursor as it was. A cursor built on another chain, or without an
/// anchor, is replaced by a full rescan, reported in `restarted`.
pub fn scan_report<C: Chain + ?Sized>(
    chain: &C,
    t: &ScanTargets,
    cursor: &Cursor,
    s: &ScanSettings,
) -> SdkResult<Scanned> {
    let liquidate = index_of(&Instruction::Liquidate { nonce: 0 })?;
    let open = index_of(&stablecoin_core::Instruction::OpenPosition {
        position_nonce: 0,
        initial_collateral_amount: 0,
    })?;
    let head = chain.last_block_id()?;
    let anchor = chain_anchor(chain, head)?;
    let (mut next, restarted) = on_this_chain(cursor, &anchor);
    if let Some((target, hash)) = rewind_target(chain, head, &next, s)? {
        // The node no longer has the block we stopped at: rewind and rescan (sets never duplicate).
        next.last_block = target;
        next.last_hash = hash;
        next.recent.retain(|id, _| *id <= target);
    }
    next.anchor = anchor;
    let batch = s.batch.max(1);
    let mut from = next.last_block + 1;
    while from <= head {
        let to = head.min(from.saturating_add(batch - 1));
        let blocks = chain.blocks(from, to)?;
        let expected =
            usize::try_from(to - from + 1).map_err(|_| SdkError::Scan("range too large".into()))?;
        if blocks.len() != expected {
            return Err(SdkError::Scan(format!(
                "the node returned {} blocks for {from}..={to}",
                blocks.len()
            )));
        }
        for (i, b) in blocks.iter().enumerate() {
            let want = from + u64::try_from(i).map_err(|_| SdkError::Scan("index".into()))?;
            check_block(b, want, &next)?;
            collect(t, b, &mut next, liquidate, open);
            next.last_block = b.id;
            next.last_hash = hex::encode(b.hash);
            next.recent.insert(b.id, next.last_hash.clone());
        }
        from = to + 1;
    }
    let keep_from = next.last_block.saturating_sub(s.reorg_depth);
    next.recent.retain(|id, _| *id >= keep_from);
    Ok(Scanned {
        cursor: next,
        restarted,
    })
}

/// `cursor` positioned so that the next scan starts at block `from` (`rescan --from`). The user
/// chose the start, so the block before it is taken from the node as it is now. A cursor built on
/// another chain, or an anchorless one past the anchor block, is discarded first (its discoveries may
/// belong to another chain), reported in `restarted`.
pub fn start_at<C: Chain + ?Sized>(chain: &C, cursor: &Cursor, from: u64) -> SdkResult<Scanned> {
    let head = chain.last_block_id()?;
    let anchor = chain_anchor(chain, head)?;
    let (base, restarted) = on_this_chain(cursor, &anchor);
    let start = from.saturating_sub(1);
    let last_hash = hash_at(chain, start)?;
    let recent = if start == 0 {
        BTreeMap::new()
    } else {
        BTreeMap::from([(start, last_hash.clone())])
    };
    Ok(Scanned {
        cursor: Cursor {
            last_block: start,
            last_hash,
            anchor,
            recent,
            ..base
        },
        restarted,
    })
}

pub fn load_cursor(path: &Path) -> SdkResult<Cursor> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Cursor::default()),
        Err(e) => {
            return Err(SdkError::Io {
                path: path.display().to_string(),
                reason: e.to_string(),
            })
        }
    };
    serde_json::from_str(&text).map_err(|e| SdkError::Io {
        path: path.display().to_string(),
        reason: format!("corrupt cursor ({e}); delete it or run `rescan --from 1`"),
    })
}

/// Per-process, per-call suffix so concurrent writers never share a temporary file.
fn unique_suffix() -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{}-{nanos}-{n}", std::process::id())
}

/// Atomic: write a unique temp file next to `path`, then rename over `path`.
pub fn save_cursor(path: &Path, c: &Cursor) -> SdkResult<()> {
    let io = |e: std::io::Error| SdkError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let tmp = path.with_extension(format!("json.{}.tmp", unique_suffix()));
    let text = serde_json::to_string_pretty(c).map_err(|e| SdkError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    let written = std::fs::File::create(&tmp).and_then(|mut f| {
        std::io::Write::write_all(&mut f, text.as_bytes())?;
        f.sync_all()
    });
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(io(e));
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io(e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::standard;
    use crate::verify::verify_config;

    fn setup() -> (standard::Fixture, ScanTargets) {
        let fx = standard::standard();
        let v = verify_config(&fx.chain, &fx.deployment, &standard::config_id()).expect("verifies");
        (fx, ScanTargets::of(&v))
    }

    #[test]
    fn scanner_finds_the_standard_auction_and_positions_incrementally() {
        let (fx, t) = setup();
        let c = scan(&fx.chain, &t, &Cursor::default(), &ScanSettings::default()).expect("scan");
        assert_eq!(c.last_block, 3);
        assert_eq!(c.auction_ids().expect("ids"), vec![fx.live_auction]);
        let mut positions = c.position_ids().expect("ids");
        positions.sort();
        let mut expected = vec![fx.unsafe_position, fx.auction_position];
        expected.sort();
        assert_eq!(positions, expected);
        let again = scan(&fx.chain, &t, &c, &ScanSettings::default()).expect("no new blocks");
        assert_eq!(again, c);
        let small = ScanSettings {
            batch: 1,
            ..ScanSettings::default()
        };
        assert_eq!(
            scan(&fx.chain, &t, &Cursor::default(), &small).expect("batch 1"),
            c
        );
    }

    #[test]
    fn scanner_rewinds_when_the_cursor_block_hash_changed() {
        let (fx, t) = setup();
        let c = scan(&fx.chain, &t, &Cursor::default(), &ScanSettings::default()).expect("scan");
        fx.chain.rewrite_from(3, 7); // the node rebuilt block 3 (a shallow reorg)
        let after = scan_report(&fx.chain, &t, &c, &ScanSettings::default()).expect("rescan");
        assert_eq!(after.restarted, None, "same chain: no restart");
        let after = after.cursor;
        assert_ne!(after.last_hash, c.last_hash);
        assert_eq!(
            after.auctions, c.auctions,
            "nothing missed, nothing duplicated"
        );
        assert_eq!(after.last_block, 3);
    }

    fn hash_hex(fx: &standard::Fixture, id: u64) -> String {
        hex::encode(fx.chain.blocks(id, id).expect("block")[0].hash)
    }

    fn extend(fx: &standard::Fixture, blocks: u64) {
        for _ in 0..blocks {
            fx.chain.push_block(vec![]);
        }
    }

    fn full_scan(fx: &standard::Fixture, t: &ScanTargets) -> Cursor {
        scan(&fx.chain, t, &Cursor::default(), &ScanSettings::default()).expect("full scan")
    }

    fn is_rescan_from_1(e: &SdkError) -> bool {
        matches!(e, SdkError::Scan(m) if m.contains("run `rescan --from 1`"))
    }

    #[test]
    fn rewind_arithmetic_is_exact_and_follows_the_rebuilt_chain() {
        let (fx, t) = setup();
        let c = full_scan(&fx, &t);
        fx.chain.rewrite_from(3, 7);
        let one = ScanSettings {
            reorg_depth: 1,
            ..ScanSettings::default()
        };
        let after = scan(&fx.chain, &t, &c, &one).expect("rescan from block 2");
        assert_eq!(after.last_block, 3);
        assert_eq!(after.last_hash, hash_hex(&fx, 3));
        assert_ne!(after.last_hash, c.last_hash);
        assert_eq!(after.auctions, c.auctions);
        assert_eq!(after.recent.keys().copied().collect::<Vec<_>>(), vec![2, 3]);
        // Rebuilt from block 1: the anchor (block 2) changed, so the whole chain is rescanned.
        fx.chain.rewrite_from(1, 8);
        let deep = scan_report(&fx.chain, &t, &after, &one).expect("rebuilt");
        assert_eq!(deep.restarted, Some(Restart::ChainReplaced));
        assert_eq!(deep.cursor.last_hash, hash_hex(&fx, 3));
        assert_eq!(deep.cursor.anchor, hash_hex(&fx, ANCHOR_BLOCK));
        assert_eq!(deep.cursor.auctions, c.auctions);
    }

    #[test]
    fn a_replaced_chain_shorter_than_the_cursor_is_rescanned_from_block_1() {
        // The cursor of an old, long chain; the node now serves a new chain of 3 blocks whose
        // block 2 differs and which holds a liquidation the old cursor never saw.
        let (fx, t) = setup();
        let old = Cursor {
            last_block: 500,
            last_hash: "aa".repeat(32),
            anchor: "bb".repeat(32),
            recent: BTreeMap::from([(490, "cc".repeat(32))]),
            ..Cursor::default()
        };
        let r = scan_report(&fx.chain, &t, &old, &ScanSettings::default()).expect("restarts");
        assert_eq!(r.restarted, Some(Restart::ChainReplaced));
        assert_eq!(r.cursor, full_scan(&fx, &t));
        assert_eq!(r.cursor.auction_ids().expect("ids"), vec![fx.live_auction]);
    }

    #[test]
    fn a_replaced_chain_longer_than_the_cursor_is_rescanned_from_block_1() {
        // Head 20 is above `last_block - reorg_depth` (15 - 10): without the anchor the scan would
        // rewind to block 5, take its hash from the new node and never see blocks 1..=5.
        let (fx, t) = setup();
        extend(&fx, 17);
        let old = Cursor {
            last_block: 15,
            last_hash: "aa".repeat(32),
            anchor: "bb".repeat(32),
            ..Cursor::default()
        };
        let r = scan_report(&fx.chain, &t, &old, &ScanSettings::default()).expect("restarts");
        assert_eq!(r.restarted, Some(Restart::ChainReplaced));
        assert_eq!(r.cursor.last_block, 20);
        assert_eq!(r.cursor.auction_ids().expect("ids"), vec![fx.live_auction]);
        // The same through a real rebuild: the node restarted and produced more blocks since.
        let c = r.cursor;
        fx.chain.rewrite_from(1, 9);
        extend(&fx, 5);
        let r = scan_report(&fx.chain, &t, &c, &ScanSettings::default()).expect("restarts");
        assert_eq!(r.restarted, Some(Restart::ChainReplaced));
        assert_eq!(r.cursor, full_scan(&fx, &t));
    }

    #[test]
    fn a_matching_anchor_scans_incrementally() {
        let (fx, t) = setup();
        let c = full_scan(&fx, &t);
        assert_eq!(c.anchor, hash_hex(&fx, ANCHOR_BLOCK));
        extend(&fx, 2);
        // Block 1 is unreadable now: only an incremental scan (blocks 4..=5) can succeed.
        fx.chain.hide_block(1);
        let r = scan_report(&fx.chain, &t, &c, &ScanSettings::default()).expect("incremental");
        assert_eq!(r.restarted, None);
        assert_eq!(r.cursor.last_block, 5);
        assert_eq!(r.cursor.anchor, c.anchor);
        assert_eq!(r.cursor.auctions, c.auctions);
    }

    #[test]
    fn a_cursor_without_an_anchor_loads_and_is_rebuilt_once() {
        let (fx, t) = setup();
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("cursor.json");
        // Written before anchors existed: no `anchor`, no `recent`.
        std::fs::write(
            &path,
            format!(
                r#"{{"last_block":3,"last_hash":"{}","auctions":[],"positions":[]}}"#,
                hash_hex(&fx, 3)
            ),
        )
        .expect("write");
        let old = load_cursor(&path).expect("old cursors still load");
        assert_eq!((old.anchor.as_str(), old.recent.len()), ("", 0));
        let r = scan_report(&fx.chain, &t, &old, &ScanSettings::default()).expect("rebuilt");
        assert_eq!(r.restarted, Some(Restart::NoAnchor));
        assert_eq!(
            r.cursor,
            full_scan(&fx, &t),
            "a full rescan, anchor recorded"
        );
        let again = scan_report(&fx.chain, &t, &r.cursor, &ScanSettings::default()).expect("ok");
        assert_eq!(again.restarted, None, "only once");
        // A cursor without an anchor that recorded only genesis has nothing to lose: kept quietly.
        let genesis_only = Cursor {
            last_block: 1,
            last_hash: hash_hex(&fx, 1),
            ..Cursor::default()
        };
        let r = scan_report(&fx.chain, &t, &genesis_only, &ScanSettings::default()).expect("ok");
        assert_eq!(r.restarted, None);
        assert_eq!(r.cursor.last_block, 3, "continues after block 1");
        assert_eq!(r.cursor.anchor, hash_hex(&fx, ANCHOR_BLOCK));
    }

    #[test]
    fn a_cursor_above_the_head_of_the_same_chain_rewinds_only_when_provable() {
        let (fx, t) = setup();
        extend(&fx, 15);
        let c = full_scan(&fx, &t); // block 18, recent 8..=18
                                    // Within reach of the recorded hashes: rewinds to block 8 (provably unchanged).
        fx.chain.truncate(12);
        let r = scan_report(&fx.chain, &t, &c, &ScanSettings::default()).expect("rewinds");
        assert_eq!(r.restarted, None);
        assert_eq!(
            (r.cursor.last_block, r.cursor.last_hash.clone()),
            (12, hash_hex(&fx, 12))
        );
        assert_eq!(r.cursor.auctions, c.auctions);
        // Out of reach: the rewind would land on block 5, which the cursor has no record of.
        fx.chain.truncate(5);
        let e = scan(&fx.chain, &t, &c, &ScanSettings::default()).expect_err("cannot prove");
        assert!(is_rescan_from_1(&e), "{e}");
        assert!(e.to_string().contains("head is 5"), "{e}");
        // A cursor within `reorg_depth` of genesis rewinds to genesis, which is always complete.
        let short = scan(&fx.chain, &t, &Cursor::default(), &ScanSettings::default()).expect("ok");
        fx.chain.truncate(3);
        let r = scan(&fx.chain, &t, &short, &ScanSettings::default()).expect("to genesis");
        assert_eq!(r, full_scan(&fx, &t));
    }

    #[test]
    fn a_fork_deeper_than_reorg_depth_on_the_same_chain_fails() {
        let (fx, t) = setup();
        extend(&fx, 15);
        let c = full_scan(&fx, &t);
        fx.chain.rewrite_from(10, 7); // within reach: block 8 unchanged
        let shallow = scan(&fx.chain, &t, &c, &ScanSettings::default()).expect("shallow reorg");
        assert_eq!(shallow.last_hash, hash_hex(&fx, 18));
        fx.chain.rewrite_from(5, 8); // block 8 changed too, block 2 did not
        let e = scan(&fx.chain, &t, &shallow, &ScanSettings::default()).expect_err("too deep");
        assert!(is_rescan_from_1(&e), "{e}");
    }

    #[test]
    fn start_at_discards_a_cursor_of_another_chain() {
        let (fx, t) = setup();
        let c = full_scan(&fx, &t);
        let same = start_at(&fx.chain, &c, 3).expect("positioned");
        assert_eq!(same.restarted, None);
        assert_eq!(same.cursor.auctions, c.auctions, "keeps its discoveries");
        assert_eq!(same.cursor.recent, BTreeMap::from([(2, hash_hex(&fx, 2))]));
        let foreign = Cursor {
            anchor: "bb".repeat(32),
            ..c.clone()
        };
        let other = start_at(&fx.chain, &foreign, 3).expect("positioned");
        assert_eq!(other.restarted, Some(Restart::ChainReplaced));
        assert!(other.cursor.auctions.is_empty());
        assert_eq!(other.cursor.anchor, c.anchor);
        let resumed = scan(&fx.chain, &t, &other.cursor, &ScanSettings::default()).expect("scan");
        assert_eq!(resumed.last_block, 3);
    }

    #[test]
    fn start_at_resets_an_anchorless_cursor_past_the_anchor_block() {
        // Plan 3a N2: an old-format cursor may belong to a replaced chain; `rescan --from N` must
        // treat it like `scan` does (NoAnchor reset), not keep its ids and hide the skipped blocks.
        let (fx, t) = setup();
        let c = full_scan(&fx, &t);
        let legacy = Cursor {
            anchor: String::new(),
            ..c.clone()
        };
        let placed = start_at(&fx.chain, &legacy, 3).expect("positioned");
        assert_eq!(placed.restarted, Some(Restart::NoAnchor));
        assert!(
            placed.cursor.auctions.is_empty(),
            "ids of an unknown chain dropped"
        );
        let early = Cursor {
            last_block: 1,
            ..Cursor::default()
        };
        assert_eq!(
            start_at(&fx.chain, &early, 2)
                .expect("positioned")
                .restarted,
            None,
            "an anchorless cursor that saw only block 1 cannot have missed anything"
        );
    }

    #[test]
    fn a_missing_cursor_block_within_the_head_is_an_actionable_error() {
        let (fx, t) = setup();
        let c = scan(&fx.chain, &t, &Cursor::default(), &ScanSettings::default()).expect("scan");
        fx.chain.hide_block(3);
        let e = scan(&fx.chain, &t, &c, &ScanSettings::default()).expect_err("gap");
        assert!(
            matches!(&e, SdkError::Scan(m) if m.contains("cursor block 3") && m.contains("rescan"))
        );
    }

    #[test]
    fn scanner_leaves_the_cursor_unchanged_on_a_missing_block() {
        let (fx, t) = setup();
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("cursor.json");
        save_cursor(&path, &Cursor::default()).expect("save");
        fx.chain.hide_block(2);
        let start = load_cursor(&path).expect("load");
        let e = scan(&fx.chain, &t, &start, &ScanSettings::default()).expect_err("gap");
        assert!(matches!(e, SdkError::Chain(m) if m.contains("block 2")));
        assert_eq!(
            load_cursor(&path).expect("load"),
            Cursor::default(),
            "not advanced"
        );
    }

    #[test]
    fn cursor_write_is_atomic_and_a_corrupt_cursor_is_an_error() {
        let (fx, t) = setup();
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("cursor.json");
        assert_eq!(
            load_cursor(&path).expect("missing = genesis"),
            Cursor::default()
        );
        let c = scan(&fx.chain, &t, &Cursor::default(), &ScanSettings::default()).expect("scan");
        save_cursor(&path, &c).expect("save");
        assert_eq!(load_cursor(&path).expect("round trip"), c);
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .expect("dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec!["cursor.json".to_owned()],
            "no temporary file left behind"
        );
        std::fs::write(dir.path().join("cursor.json.tmp"), "garbage").expect("stray temp");
        save_cursor(&path, &c).expect("a stray temp file is harmless");
        assert_eq!(load_cursor(&path).expect("reload"), c);
        std::fs::remove_file(dir.path().join("cursor.json.tmp")).expect("rm stray");
        std::fs::write(&path, "{ half").expect("write");
        let e = load_cursor(&path).expect_err("corrupt");
        assert!(matches!(e, SdkError::Io { ref path, .. } if path.ends_with("cursor.json")));
    }

    #[test]
    fn a_failed_save_leaves_no_temporary_file() {
        // The target is a non-empty directory, so the final rename fails (portable stand-in for a
        // failure after the temp file was written; read-only directories are unreliable on /mnt/c).
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("cursor.json");
        std::fs::create_dir(&path).expect("dir");
        std::fs::write(path.join("keep"), "x").expect("file");
        let e = save_cursor(&path, &Cursor::default()).expect_err("rename fails");
        assert!(matches!(e, SdkError::Io { .. }));
        assert!(
            path.join("keep").exists(),
            "the existing target is untouched"
        );
        let names: Vec<_> = std::fs::read_dir(dir.path()).expect("dir").collect();
        assert_eq!(names.len(), 1, "no temporary file left behind");
    }
}
