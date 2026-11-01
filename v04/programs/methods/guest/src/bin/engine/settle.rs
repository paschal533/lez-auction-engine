//! `SettleEmpty` (design note §4.2 row 7; v0.2.4 `settle.rs` `settle_empty`, ruling R-9a): an
//! auction whose lot is empty (the keeper reward took the whole seizure) can never be bid on, so
//! without this it would hold its reservations forever (ADR-012 has no expiry).
//!
//! Permissionless: the root lands at the named config `K` (the proxy block's `SettleEmpty` arm),
//! which sends a seeded `Probe { Auction }` and then this message unseeded. The auction accepts it
//! only from `K`, runs `auction_core::settle_empty` (v0.2.4's `NotAuctioning` unless `Auctioning`
//! with `collateral_left == 0`), writes `Settled`, and ends with the bid's chain-end tail:
//! `Settled` to the accounting, `Release` to the collateral actor and the config.

use auction_engine_params::EngineError;
use engine_core::{
    bid::settle_empty,
    check::{expect_from, fail},
    state::AuctionState,
};
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response},
};

use crate::{admin::engine, bid::chain_end};

/// At the auction `(config, position, nonce)`, from exactly its config.
pub fn on_settle_empty(
    input: &ReceiveInput,
    config: AccountId,
    position: AccountId,
    nonce: u64,
) -> Response {
    let a = AuctionState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("auction")));
    if (a.config, a.position, a.nonce) != (config, position, nonce) {
        fail(EngineError::WrongAccount("auction"));
    }
    let e = engine(input);
    expect_from(
        input,
        Actor::new(a.config, e),
        EngineError::WrongAccount("config"),
    );
    if a.scratch.is_some() {
        fail(EngineError::NotAuctioning);
    }
    let (settled, end) = settle_empty(&a).unwrap_or_else(|err| fail(err));
    let response = Response::set_state(settled.encode());
    chain_end(response, e, &settled, end)
}
