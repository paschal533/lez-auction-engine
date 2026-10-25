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
    /// Included `bid` / `bid_with_settlement` transactions of our config, per auction id.
    #[serde(default)]
    pub bids: BTreeMap<String, u32>,
    /// Hex hashes of the bid transactions counted in `bids`, so that a rewind or a `rescan --from`
    /// over blocks already scanned never counts a bid twice (the same rule as the id sets).
    /// Limits: a cursor written before these fields loads with every count at 0 until
    /// `rescan --from 1`; a bid a reorg drops is not decremented; `bid_txs` grows with every bid
    /// ever seen (like the id sets, it is never pruned).
    #[serde(default)]
    pub bid_txs: BTreeSet<String>,
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

/// The enum indices of the instructions the scan looks for (from our own encoder).
struct Kinds {
    liquidate: u32,
    bid: u32,
    bid_with_settlement: u32,
    open: u32,
}

impl Kinds {
    fn new() -> SdkResult<Self> {
        let (coin_in_max, min_collateral_out) = (0, 0);
        Ok(Self {
            liquidate: index_of(&Instruction::Liquidate { nonce: 0 })?,
            bid: index_of(&Instruction::Bid {
                coin_in_max,
                min_collateral_out,
            })?,
            bid_with_settlement: index_of(&Instruction::BidWithSettlement {
                coin_in_max,
                min_collateral_out,
            })?,
            open: index_of(&stablecoin_core::Instruction::OpenPosition {
                position_nonce: 0,
                initial_collateral_amount: 0,
            })?,
        })
    }
}

fn collect(t: &ScanTargets, b: &BlockSummary, next: &mut Cursor, k: &Kinds) {
    for tx in &b.txs {
        let kind = variant_index(&tx.instruction_words);
        let engine_of_ours =
            tx.program_id == t.engine_program && tx.account_ids.get(2) == Some(&t.config);
        if engine_of_ours && kind == Some(k.liquidate) {
            if let Some(a) = tx.account_ids.get(6) {
                next.auctions.insert(a.to_string());
            }
            if let Some(p) = tx.account_ids.get(4) {
                next.positions.insert(p.to_string());
            }
        }
        // `plan::bid`: the auction is account 4, for both variants.
        if engine_of_ours && (kind == Some(k.bid) || kind == Some(k.bid_with_settlement)) {
            if let Some(a) = tx.account_ids.get(4) {
                if next.bid_txs.insert(hex::encode(tx.hash)) {
                    let n = next.bids.entry(a.to_string()).or_insert(0);
                    *n = n.saturating_add(1);
                }
            }
        }
        if tx.program_id == t.host_program
            && kind == Some(k.open)
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
/// anchor, is replaced by a full rescan, reported in `restarted`. A loop over [`scan_page`].
pub fn scan_report<C: Chain + ?Sized>(
    chain: &C,
    t: &ScanTargets,
    cursor: &Cursor,
    s: &ScanSettings,
) -> SdkResult<Scanned> {
    let mut page = scan_page(chain, t, cursor, s, u64::MAX)?;
    let mut restarted = page.scanned.restarted;
    while !page.done {
        page = scan_page(chain, t, &page.scanned.cursor, s, u64::MAX)?;
        restarted = restarted.or(page.scanned.restarted);
    }
    Ok(Scanned {
        cursor: page.scanned.cursor,
        restarted,
    })
}

/// One page of a scan: the result, the node's head when the page started, and whether the cursor
/// reached it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanPage {
    pub scanned: Scanned,
    pub head: u64,
    pub done: bool,
}

/// [`scan_report`] limited to at most `max_blocks` (>= 1) new blocks, so that a caller with a time budget
/// (an FFI call must not outlast the IPC timeout) scans a backlog page by page. The chain checks run
/// on every page: a replaced chain discards the cursor on the page that sees it (`restarted`), so a
/// list built from the returned cursor is empty rather than stale.
pub fn scan_page<C: Chain + ?Sized>(
    chain: &C,
    t: &ScanTargets,
    cursor: &Cursor,
    s: &ScanSettings,
    max_blocks: u64,
) -> SdkResult<ScanPage> {
    if max_blocks == 0 {
        // A zero-block page never advances: a caller paging until `done` would loop forever.
        return Err(SdkError::Scan(
            "max_blocks must be at least 1 (a page of 0 blocks never advances)".into(),
        ));
    }
    let kinds = Kinds::new()?;
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
    let end = head.min(next.last_block.saturating_add(max_blocks));
    scan_blocks(chain, t, &mut next, end, s.batch, &kinds)?;
    let keep_from = next.last_block.saturating_sub(s.reorg_depth);
    next.recent.retain(|id, _| *id >= keep_from);
    Ok(ScanPage {
        done: next.last_block >= head,
        head,
        scanned: Scanned {
            cursor: next,
            restarted,
        },
    })
}

/// Scans blocks `next.last_block + 1 ..= end` in `batch`-sized reads, checking each extends the last.
fn scan_blocks<C: Chain + ?Sized>(
    chain: &C,
    t: &ScanTargets,
    next: &mut Cursor,
    end: u64,
    batch: u64,
    kinds: &Kinds,
) -> SdkResult<()> {
    let batch = batch.max(1);
    let mut from = next.last_block + 1;
    while from <= end {
        let to = end.min(from.saturating_add(batch - 1));
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
            check_block(b, want, next)?;
            collect(t, b, next, kinds);
            next.last_block = b.id;
            next.last_hash = hex::encode(b.hash);
            next.recent.insert(b.id, next.last_hash.clone());
        }
        from = to + 1;
    }
    Ok(())
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
#[path = "scan_tests.rs"]
mod tests;
