//! `ClaimLeftover` (design note §4.2 row 8; v0.2.4 `settle.rs` `claim_leftover`): the owner's
//! proxy `(owner, E)` forwards the claim to the auction, which accepts it only from exactly
//! `(owner recorded at liquidation, E)` (v0.2.4 `NotOwner`), books
//! `leftover_claimed = leftover_claimable` (v0.2.4 `NothingToClaim` unless settled with something
//! left), then sweeps the whole vault to the owner's holding with
//! `at_least = claimable − claimed` (VAULT seed, R-8b's token exception). The sweep pays any
//! donation too (ADR-033) and replaces v0.2.4's vault read (design note §4.3 a).

use auction_engine_params::EngineError;
use engine_core::{
    actors::{vault_holding, vault_seed},
    bid::claim,
    check::{expect_from, fail},
    state::AuctionState,
};
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, SyncMessage},
};
use token_core::TokenMsg;

use crate::admin::engine;

/// At the auction `(config, position, nonce)`, from its owner's proxy.
pub fn on_claim(
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
    expect_from(input, Actor::new(a.owner, e), EngineError::NotOwner);
    let (next, amount) = claim(&a).unwrap_or_else(|err| fail(err));
    let me = input.receiver.account_id;
    let sweep = TokenMsg::Sweep {
        definition: a.collateral_def,
        to: a.owner,
        at_least: amount,
    };
    let vault = vault_holding(e, a.token, me);
    Response::set_state(next.encode())
        .send(SyncMessage::new(vault, &sweep).with_pda_seeds([vault_seed(me)]))
}
