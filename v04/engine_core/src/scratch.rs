//! Scratch types (design note §4.1, §4.3 a): what an actor keeps between the message that starts
//! a Read or a quote and the reply or sentinel that consumes it. Every scratch field is `None` in
//! every committed state (`harness::invariants` asserts it).

use auction_core::price::OracleReading;
use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::{AccountId, Actor};

use crate::state::CollateralState;

/// One oracle publication as the engine reads it (v0.2.4 `OracleReading`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct OracleReadingRec {
    pub price_raw: u128,
    pub confidence_raw: u128,
    pub timestamp_ms: u64,
}

impl OracleReadingRec {
    #[must_use]
    pub fn to_core(self) -> OracleReading {
        OracleReading {
            price_raw: self.price_raw,
            confidence_raw: self.confidence_raw,
            timestamp_ms: self.timestamp_ms,
        }
    }
}

/// An observation in flight at the collateral actor: `now_ms` is the caller's clock claim (asserted
/// against the clock), the readings arrive as `StateReply`s, `then` runs at `Proceed`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PendingObserve {
    pub now_ms: u64,
    /// The host's redemption price, once its reply is in.
    pub redemption_ray: Option<u128>,
    /// The oracle's publication, once its reply is in.
    pub reading: Option<OracleReadingRec>,
    pub then: ObserveThen,
}

/// What the collateral actor does once an observation is complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum ObserveThen {
    /// Only record the pause clock (`SetCollateral`).
    Record,
    /// Require a live type, then hand the liquidation to the config (row 4).
    Liquidate {
        keeper: AccountId,
        position: AccountId,
        nonce: u64,
    },
    /// Require a live type, then hand the bid to the auction (rows 5-6).
    Bid(BidRequestRec),
}

/// A bidder's `Bid`, as the collateral actor keeps it while it observes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct BidRequestRec {
    /// Pays from, and is paid to, its own holding `(bidder, T)`; must sign.
    pub bidder: AccountId,
    pub position: AccountId,
    pub nonce: u64,
    pub max_debt_in: u128,
    pub min_collateral_out: u128,
}

/// See [`AuctionState::scratch`](crate::state::AuctionState::scratch).
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum AuctionScratch {
    /// Set by `Open` (phase `AwaitingSeize`): what the `Seized` report is quoted with.
    Quote(Box<QuoteCtx>),
    /// Set by `Seized` (phase `Auctioning`): the vault read must show at least `at_least` of
    /// `definition` (R6 as a floor, ruling R-5e).
    VaultRead {
        vault: Actor,
        definition: AccountId,
        at_least: u128,
    },
}

/// The liquidation context the config hands the auction at `Open`: the observed bid price, the
/// global reservation and the collateral actor's state as observed in this transaction.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct QuoteCtx {
    pub price_ray: u128,
    pub global_cap: u128,
    pub global_reserved: u128,
    pub collateral: CollateralState,
}
