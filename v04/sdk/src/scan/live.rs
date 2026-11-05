//! The scan on a live sequencer (ADR-066): [`BlockSource`] over `SequencerClient` and the check
//! that turns a scan's candidates into the auctions that really exist.
//!
//! LEZ 0.4 facts this relies on, read from the pinned source: `getBlockRange(from, to)` is
//! inclusive, at most 1024 blocks wide, and stops at the first block the node lacks; a block's
//! `timestamp` is the wall clock in milliseconds; a block hash is a `HashType([u8; 32])`.

use common::block::Block;
use lee::Actor;
use sequencer_service_rpc::{RpcClient as _, SequencerClient};

use super::{BlockSource, BlockSummary, Found, TxSummary};
use crate::{Deployment, SdkError, StateSource};

impl BlockSource for SequencerClient {
    async fn last_block_id(&self) -> Result<u64, SdkError> {
        self.get_last_block_id()
            .await
            .map_err(|err| SdkError::Rpc(format!("getLastBlockId: {err}")))
    }

    async fn blocks(&self, from: u64, to: u64) -> Result<Vec<BlockSummary>, SdkError> {
        let mut out = Vec::new();
        for (lo, hi) in chunks(from, to, MAX_RANGE_BLOCKS) {
            let blocks = self
                .get_block_range(lo, hi)
                .await
                .map_err(|err| SdkError::Rpc(format!("getBlockRange {lo}..={hi}: {err}")))?;
            let short = u64::try_from(blocks.len()).is_ok_and(|n| n <= hi.saturating_sub(lo));
            out.extend(blocks.iter().map(summarize));
            if short {
                break; // the node lacks the next block; later chunks would be empty too
            }
        }
        Ok(out)
    }
}

/// The widest range `getBlockRange` accepts, in blocks.
const MAX_RANGE_BLOCKS: u64 = 1024;

/// `from..=to` cut into inclusive sub-ranges of at most `max` blocks, in order; empty when
/// `from > to`.
fn chunks(from: u64, to: u64, max: u64) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    let mut lo = from;
    while lo <= to {
        let hi = lo.saturating_add(max.max(1).saturating_sub(1)).min(to);
        out.push((lo, hi));
        match hi.checked_add(1) {
            Some(next) => lo = next,
            None => break,
        }
    }
    out
}

/// A block reduced to what a scan reads. Only a public transaction has a root call; every other
/// kind keeps `root_to: None`.
fn summarize(block: &Block) -> BlockSummary {
    let txs = block
        .body
        .transactions
        .iter()
        .enumerate()
        .map(|(i, tx)| {
            let call = tx.public_call();
            TxSummary {
                hash: tx.hash().0,
                root_to: call.map(|(to, _)| to),
                message: call.map_or_else(Vec::new, |(_, message)| message.to_vec()),
                reverted: block.body.is_reverted(i),
            }
        })
        .collect();
    BlockSummary {
        id: block.header.block_id,
        hash: block.header.hash.0,
        prev_hash: block.header.prev_block_hash.0,
        timestamp_ms: block.header.timestamp,
        txs,
    }
}

/// The auctions among `found.auctions` whose engine state is non-empty in `src`, in `found`
/// order (a scan reports candidates; this confirms them).
///
/// # Errors
/// None today; the `Result` leaves room for a source that can fail.
pub fn live_auctions(
    src: &impl StateSource,
    d: &Deployment,
    found: &Found,
) -> Result<Vec<Actor>, SdkError> {
    Ok(found
        .auctions
        .iter()
        .map(|(position, nonce)| d.auction(*position, *nonce))
        .filter(|auction| src.actor_state(*auction).is_some_and(|s| !s.is_empty()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::chunks;

    #[test]
    fn chunks_cover_the_inclusive_range_in_order() {
        assert_eq!(chunks(1, 5, 1024), vec![(1, 5)]);
        assert_eq!(chunks(1, 1024, 1024), vec![(1, 1024)]);
        assert_eq!(chunks(1, 1025, 1024), vec![(1, 1024), (1025, 1025)]);
        assert_eq!(chunks(10, 12, 1), vec![(10, 10), (11, 11), (12, 12)]);
        assert!(chunks(5, 4, 1024).is_empty());
        assert_eq!(
            chunks(u64::MAX - 1, u64::MAX, 1024),
            vec![(u64::MAX - 1, u64::MAX)]
        );
    }
}
