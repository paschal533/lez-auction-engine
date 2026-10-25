//! Read-side views the Auctions app renders and the CLI prints (one source of truth for every
//! number). Each view is assembled from the SDK's quotes and `auction_core`'s own functions; nothing
//! here re-derives engine or host maths. Views are `serde::Serialize`; u128 amounts cross as decimal
//! strings ([`crate::dec`]).
//!
//! Lists come from the scan cursor, in cursor order. Scanner candidates can be stale (ADR-043): an id
//! whose account is gone or no longer decodes is dropped silently, one that decodes but is the wrong
//! account (`Mismatch`) or that the engine rejects (`Engine`) is reported in `skipped`, and any other
//! error fails the view.

mod accounts;
mod auctions;
mod positions;
mod system;

use lee_core::account::AccountId;
use serde::Serialize;

use crate::error::{SdkError, SdkResult};

pub use accounts::{accounts, AccountView, ROLES};
pub use auctions::{auction_entry, auctions, AuctionEntry, LiveAuctionView, SettledAuctionView};
pub use positions::{at_risk, positions, AtRiskView};
pub use system::{accounting, host, oracle, AccountingView, HostView, OracleView};

/// An id the cursor named that a list skipped, and why (a warning for the user).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skipped {
    pub id: AccountId,
    pub reason: String,
}

/// The rows of a list and the candidates it skipped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Listed<T> {
    pub items: Vec<T>,
    pub skipped: Vec<Skipped>,
}

/// One row per id, in order; `row` returns `None` for an id that has no row (e.g. a position
/// without debt). Errors follow the stale-candidate rule of the module docs.
fn list<T>(
    ids: Vec<AccountId>,
    mut row: impl FnMut(&AccountId) -> SdkResult<Option<T>>,
) -> SdkResult<Listed<T>> {
    let mut listed = Listed {
        items: Vec::new(),
        skipped: Vec::new(),
    };
    for id in ids {
        match row(&id) {
            Ok(Some(item)) => listed.items.push(item),
            Ok(None) | Err(SdkError::Missing { .. } | SdkError::Decode { .. }) => {}
            Err(e @ (SdkError::Mismatch { .. } | SdkError::Engine { .. })) => {
                listed.skipped.push(Skipped {
                    id,
                    reason: e.to_string(),
                });
            }
            Err(other) => return Err(other),
        }
    }
    Ok(listed)
}

#[cfg(test)]
#[path = "../views_tests.rs"]
mod tests;
