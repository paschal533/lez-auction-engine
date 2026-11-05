//! The cursor's chain decisions (ADR-066, ported from the v0.2.4 scanner, ADR-043): which chain a
//! cursor belongs to, and where a scan resumes when the node's chain no longer matches it.
//!
//! The cursor records which chain it was built on (`anchor`, the hash of block [`ANCHOR_BLOCK`])
//! and the hashes of its last `reorg_depth + 1` blocks (`recent`). A node with another anchor
//! (restarted or replaced) discards the cursor and rescans from block 1. A rewind is accepted only
//! when it reaches genesis or the node still has the rewind block with the hash the scan recorded
//! there; otherwise the scan fails with [`SdkError::ScanFork`] instead of succeeding over blocks
//! it never saw.

use super::{ANCHOR_BLOCK, BlockSource, Cursor, ScanSettings};
use crate::SdkError;

/// The hash of block `id` on the node; `None` for block 0, which has no hash.
///
/// # Errors
/// [`SdkError::MissingBlock`] if the node does not return the block.
pub(super) async fn hash_at(src: &impl BlockSource, id: u64) -> Result<Option<[u8; 32]>, SdkError> {
    if id == 0 {
        return Ok(None);
    }
    let blocks = src.blocks(id, id).await?;
    blocks
        .iter()
        .find(|b| b.id == id)
        .map(|b| Some(b.hash))
        .ok_or(SdkError::MissingBlock(id))
}

/// The node's anchor, or `None` while its chain is shorter than [`ANCHOR_BLOCK`].
pub(super) async fn chain_anchor(
    src: &impl BlockSource,
    head: u64,
) -> Result<Option<[u8; 32]>, SdkError> {
    if head < ANCHOR_BLOCK {
        return Ok(None);
    }
    hash_at(src, ANCHOR_BLOCK).await
}

/// The cursor to continue from on a node whose anchor is `anchor`, or a fresh one (`true`).
///
/// Block 1 is not the anchor: the genesis block carries a fixed timestamp, so a restarted node
/// reproduces its hash. Block 2 is the first block stamped with the wall clock, so two runs of the
/// same node differ there. A cursor without an anchor that recorded nothing past genesis cannot
/// have missed anything and is kept; one that did is rebuilt, because the chain it was built on is
/// unknown.
pub(super) fn on_this_chain(cursor: &Cursor, anchor: Option<[u8; 32]>) -> (Cursor, bool) {
    match cursor.anchor {
        None if cursor.last_block < ANCHOR_BLOCK => (cursor.clone(), false),
        None => (Cursor::default(), true),
        Some(own) if Some(own) != anchor => (Cursor::default(), true),
        Some(_) => (cursor.clone(), false),
    }
}

/// Where to rewind to (block and its hash), or `None` when the cursor block is still on the node
/// with the stored hash.
///
/// A rewind goes back `reorg_depth` blocks, to at most the head. It is accepted only if it reaches
/// genesis or the node's hash of the rewind block is the one the scan recorded there; otherwise
/// blocks below it may differ from the ones scanned and the scan fails with
/// [`SdkError::ScanFork`]. A cursor block within the head that the node cannot return is a
/// transient gap: [`SdkError::MissingBlock`].
pub(super) async fn rewind_target(
    src: &impl BlockSource,
    head: u64,
    cursor: &Cursor,
    s: &ScanSettings,
) -> Result<Option<(u64, Option<[u8; 32]>)>, SdkError> {
    let n = cursor.last_block;
    if n == 0 {
        return Ok(None);
    }
    if n <= head && hash_at(src, n).await? == cursor.last_hash {
        return Ok(None);
    }
    let target = n.saturating_sub(s.reorg_depth).min(head);
    if target == 0 {
        return Ok(Some((0, None)));
    }
    let on_node = hash_at(src, target).await?;
    if cursor.recent.get(&target) != on_node.as_ref() {
        return Err(SdkError::ScanFork { at: target });
    }
    Ok(Some((target, on_node)))
}
