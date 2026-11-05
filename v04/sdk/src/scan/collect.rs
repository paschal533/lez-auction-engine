//! Turning a block into discoveries (ADR-066): which auctions, positions and system auctions the
//! transactions of one block name. Only root calls to the engine are read; whether a candidate
//! really exists is for the caller to confirm by reading its state.

use borsh::BorshDeserialize;
use engine_core::msg::EngineMsg;
use lee::AccountId;

use super::{BlockSummary, Found, ScanTargets};

/// `found` plus what `block` discovers; `found` itself is not touched.
///
/// A transaction counts when it did not revert, its root call went to the engine program and its
/// message decodes as an [`EngineMsg`] that names `t.config`. `Liquidate` yields an auction
/// `(position, nonce)` and the position; `Bid`, `ClaimLeftover` and `SettleEmpty` yield the auction;
/// `StartSurplus` and `StartDebt` yield the system auction's nonce. Everything else is ignored.
#[must_use]
pub fn collect(found: &Found, block: &BlockSummary, t: &ScanTargets) -> Found {
    let mut next = found.clone();
    collect_into(&mut next, block, t);
    next
}

/// [`collect`] on a value the caller owns, so a scan does not copy the sets once per block.
pub(super) fn collect_into(found: &mut Found, block: &BlockSummary, t: &ScanTargets) {
    for tx in &block.txs {
        let to_engine = tx
            .root_to
            .is_some_and(|root| root.program_account_id == t.engine);
        if tx.reverted || !to_engine {
            continue;
        }
        if let Ok(msg) = EngineMsg::try_from_slice(&tx.message) {
            note(found, &msg, t.config);
        }
    }
}

#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "only the five discovery variants matter; every other message is ignored by design"
)]
fn note(found: &mut Found, msg: &EngineMsg, config: AccountId) {
    match msg {
        EngineMsg::Liquidate {
            config: c,
            position,
            nonce,
            ..
        } if *c == config => {
            found.auctions.insert((*position, *nonce));
            found.positions.insert(*position);
        }
        EngineMsg::Bid {
            config: c,
            position,
            nonce,
            ..
        }
        | EngineMsg::ClaimLeftover {
            config: c,
            position,
            nonce,
        }
        | EngineMsg::SettleEmpty {
            config: c,
            position,
            nonce,
        } if *c == config => {
            found.auctions.insert((*position, *nonce));
        }
        EngineMsg::StartSurplus {
            config: c, nonce, ..
        } if *c == config => {
            found.surplus.insert(*nonce);
        }
        EngineMsg::StartDebt {
            config: c, nonce, ..
        } if *c == config => {
            found.debt.insert(*nonce);
        }
        _ => {}
    }
}
