//! Discovery by block scan (ADR-066): a keeper finds the auctions, positions, surplus auctions
//! and debt auctions of a config from the chain's blocks. There are no events to subscribe to
//! (a standalone sequencer never finalizes) and no account-listing RPC, so the scan reads the
//! transactions of each block, keeps the root calls that name our config, and persists a
//! [`Cursor`]. What it finds are candidates; the caller confirms each by reading its state.
//!
//! [`BlockSource`] is the one thing a scan needs from a node, so the core runs against an
//! in-memory chain in tests. The cursor is plain data: the caller stores it with borsh.

mod collect;
mod cursor;

use std::collections::{BTreeMap, BTreeSet};

use borsh::{BorshDeserialize, BorshSerialize};
use lee::{AccountId, Actor};

use crate::SdkError;
pub use collect::collect;
use cursor::{chain_anchor, on_this_chain, rewind_target};

/// The block whose hash identifies a chain. Not block 1: the genesis block is built with a fixed
/// timestamp, so a restarted node reproduces its hash. Block 2 is the first block stamped with the
/// wall clock, so two runs of the same node differ there.
pub const ANCHOR_BLOCK: u64 = 2;

/// One transaction of a block, reduced to what discovery reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxSummary {
    pub hash: [u8; 32],
    /// The actor the transaction's root call went to; `None` for a transaction without one.
    pub root_to: Option<Actor>,
    /// The root call's message bytes.
    pub message: Vec<u8>,
    /// The transaction was included but its action reverted.
    pub reverted: bool,
}

/// One block, reduced to what a scan reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockSummary {
    pub id: u64,
    pub hash: [u8; 32],
    pub prev_hash: [u8; 32],
    pub timestamp_ms: u64,
    pub txs: Vec<TxSummary>,
}

/// A node's blocks, as the scan reads them.
#[expect(
    async_fn_in_trait,
    reason = "callers pass concrete sources; no Send bound is needed"
)]
pub trait BlockSource {
    /// The id of the node's last block (0 for an empty chain).
    async fn last_block_id(&self) -> Result<u64, SdkError>;
    /// The blocks `from..=to` (inclusive), in order. A block the node cannot return is left out.
    async fn blocks(&self, from: u64, to: u64) -> Result<Vec<BlockSummary>, SdkError>;
}

/// What a scan looks for: the engine program's account and the config it serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanTargets {
    pub engine: AccountId,
    pub config: AccountId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanSettings {
    /// Blocks per read.
    pub batch: u64,
    /// How many blocks deep a reorganisation is followed.
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

/// Candidates a scan discovered. Sets, so a rescan never duplicates.
#[derive(BorshSerialize, BorshDeserialize, Clone, Default, PartialEq, Eq, Debug)]
pub struct Found {
    /// Auctions as `(position, nonce)`.
    pub auctions: BTreeSet<(AccountId, u64)>,
    pub positions: BTreeSet<AccountId>,
    /// Surplus auction nonces.
    pub surplus: BTreeSet<u64>,
    /// Debt auction nonces.
    pub debt: BTreeSet<u64>,
}

/// Where a scan stopped, which chain it was on, and what it found.
#[derive(BorshSerialize, BorshDeserialize, Clone, Default, PartialEq, Eq, Debug)]
pub struct Cursor {
    pub last_block: u64,
    /// Hash of `last_block` (`None` before the first scan).
    pub last_hash: Option<[u8; 32]>,
    /// Hash of block [`ANCHOR_BLOCK`] of the chain this cursor was built on; `None` means unknown
    /// (a fresh cursor, or a chain shorter than the anchor).
    pub anchor: Option<[u8; 32]>,
    /// Hashes of the last `reorg_depth + 1` scanned blocks, so a rewind is checked against what
    /// the scan saw.
    pub recent: BTreeMap<u64, [u8; 32]>,
    pub found: Found,
}

/// A scan result: the new cursor, and whether the stored one was discarded (tell the user).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scanned {
    pub cursor: Cursor,
    pub restarted: bool,
}

/// One page of a scan: the result, the node's head when the page started, and whether the cursor
/// reached it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanPage {
    pub scanned: Scanned,
    pub head: u64,
    pub done: bool,
}

/// Scans the blocks after `cursor` and returns the new cursor; never mutates its input, so a
/// failed scan leaves the caller's cursor as it was. A cursor built on another chain is replaced
/// by a full rescan, reported in `restarted`. A loop over [`scan_page`].
///
/// # Errors
/// [`SdkError::ScanFork`] if the chain diverged deeper than `reorg_depth`;
/// [`SdkError::MissingBlock`] if the node fails to return a block; the source's own errors.
pub async fn scan(
    src: &impl BlockSource,
    cursor: &Cursor,
    t: &ScanTargets,
    s: &ScanSettings,
) -> Result<Scanned, SdkError> {
    let mut page = scan_page(src, cursor, t, s, u64::MAX).await?;
    let mut restarted = page.scanned.restarted;
    while !page.done {
        page = scan_page(src, &page.scanned.cursor, t, s, u64::MAX).await?;
        restarted |= page.scanned.restarted;
    }
    Ok(Scanned {
        cursor: page.scanned.cursor,
        restarted,
    })
}

/// [`scan`] limited to `max_blocks` new blocks (0 counts as 1, since a page of none never
/// advances), so a caller with a time budget scans a backlog page by page. The chain checks run
/// on every page: a replaced chain discards the cursor on the page that sees it.
///
/// # Errors
/// As [`scan`].
pub async fn scan_page(
    src: &impl BlockSource,
    cursor: &Cursor,
    t: &ScanTargets,
    s: &ScanSettings,
    max_blocks: u64,
) -> Result<ScanPage, SdkError> {
    let head = src.last_block_id().await?;
    let anchor = chain_anchor(src, head).await?;
    let (mut next, restarted) = on_this_chain(cursor, anchor);
    if let Some((target, hash)) = rewind_target(src, head, &next, s).await? {
        // The node no longer has the block we stopped at: rewind and rescan (sets never duplicate).
        next.last_block = target;
        next.last_hash = hash;
        next.recent.retain(|id, _| *id <= target);
    }
    next.anchor = anchor;
    let end = head.min(next.last_block.saturating_add(max_blocks.max(1)));
    scan_blocks(src, t, &mut next, end, s.batch).await?;
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
/// `next` is the scan's own copy; the caller's cursor is never touched.
async fn scan_blocks(
    src: &impl BlockSource,
    t: &ScanTargets,
    next: &mut Cursor,
    end: u64,
    batch: u64,
) -> Result<(), SdkError> {
    let batch = batch.max(1);
    let mut from = next.last_block.saturating_add(1);
    while from <= end {
        let to = end.min(from.saturating_add(batch.saturating_sub(1)));
        let blocks = src.blocks(from, to).await?;
        for (want, b) in (from..=to).zip(&blocks) {
            check_block(b, want, next)?;
            collect::collect_into(&mut next.found, b, t);
            next.last_block = b.id;
            next.last_hash = Some(b.hash);
            next.recent.insert(b.id, b.hash);
        }
        let got = u64::try_from(blocks.len()).unwrap_or(u64::MAX);
        if got < to.saturating_sub(from).saturating_add(1) {
            return Err(SdkError::MissingBlock(from.saturating_add(got)));
        }
        from = to.saturating_add(1);
    }
    Ok(())
}

fn check_block(b: &BlockSummary, want: u64, next: &Cursor) -> Result<(), SdkError> {
    if b.id != want {
        return Err(SdkError::MissingBlock(want));
    }
    if next.last_hash.is_some_and(|last| b.prev_hash != last) {
        return Err(SdkError::ScanFork { at: b.id });
    }
    if b.id == ANCHOR_BLOCK && Some(b.hash) != next.anchor {
        return Err(SdkError::ScanFork { at: ANCHOR_BLOCK });
    }
    Ok(())
}
