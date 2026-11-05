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
        let blocks = self
            .get_block_range(from, to)
            .await
            .map_err(|err| SdkError::Rpc(format!("getBlockRange {from}..={to}: {err}")))?;
        Ok(blocks.iter().map(summarize).collect())
    }
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
