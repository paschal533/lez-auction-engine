//! The auction's side of `liquidate` (design note §4.2 row 4, §4.3 a; F6 phases).
//!
//! - `Open` (from its config `K`, AUCTION seed, empty state): phase `AwaitingSeize`, host bound,
//!   quote inputs in the scratch.
//! - `Seized` (ruling R-5d): only in `AwaitingSeize` and only from exactly
//!   `expected_seized_sender` = `(requested position, H)`. Quotes the REPORTED amounts with v0.2.4's
//!   code (ruling R-5e), moves to `Auctioning`, then sends: the keeper reward from the vault (VAULT
//!   seed) to the keeper's own holding (ruling R-3a), `Reserve` to the collateral actor and the
//!   config, and `ReadState` to the vault.
//! - The vault's `StateReply`: the vault must hold at least `seized − reward` (R6 as a floor).
//! - `Finish` (from `K`, the conversation's last message): the auction must be `Auctioning` with
//!   its vault check done; a host that never reported or a vault that never answered fails here.

use auction_engine_params::EngineError;
use borsh::BorshDeserialize;
use engine_core::{
    actors::{auction_actor, collateral_actor, vault_holding, vault_seed},
    check::{expect_from, expected_seized_sender, fail},
    liquidation::{AuctionIdentity, SeizeReport, awaiting_seize, quote_seized},
    msg::EngineMsg,
    state::{AuctionPhase, AuctionScratch, AuctionState, CollateralState, QuoteCtx},
};
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, StateReply, SyncMessage},
};
use token_core::{Holding, TokenMsg};

use crate::admin::{engine, engine_sender};

/// `Open` without the collateral state.
pub struct OpenArgs {
    pub position: AccountId,
    pub nonce: u64,
    pub keeper: AccountId,
    pub host: Actor,
    pub token: AccountId,
    pub stable_def: AccountId,
    pub now_ms: u64,
    pub price_ray: u128,
    pub global_cap: u128,
    pub global_reserved: u128,
}

/// At `auction_actor(E, K, position, nonce)`, from its config `K` with the AUCTION seed: creates
/// the auction (an existing one is refused, as v0.2.4's `expect_default`).
pub fn on_open(input: &ReceiveInput, o: &OpenArgs, collateral: CollateralState) -> Response {
    let config = engine_sender(input, EngineError::WrongAccount("auction"));
    if input.receiver != auction_actor(engine(input), config, o.position, o.nonce)
        || !input.pre_state.as_ref().is_empty()
        || !input.is_authorized
    {
        fail(EngineError::WrongAccount("auction"));
    }
    let id = AuctionIdentity {
        config,
        position: o.position,
        nonce: o.nonce,
        host: o.host,
        keeper: o.keeper,
        start_ms: o.now_ms,
        token: o.token,
        stable_def: o.stable_def,
    };
    let ctx = QuoteCtx {
        price_ray: o.price_ray,
        global_cap: o.global_cap,
        global_reserved: o.global_reserved,
        collateral,
    };
    Response::set_state(awaiting_seize(&id, ctx).encode())
}

fn load(input: &ReceiveInput, err: EngineError) -> AuctionState {
    AuctionState::decode(input.pre_state.as_ref()).unwrap_or_else(|| fail(err))
}

/// The host's report: the sender first, then the phase, then v0.2.4's quote.
pub fn on_seized(
    input: &ReceiveInput,
    position: AccountId,
    owner: AccountId,
    collateral: u128,
    nominal_debt: u128,
) -> Response {
    let a = load(input, EngineError::WrongAccount("auction"));
    expect_from(input, expected_seized_sender(&a), EngineError::HostMismatch);
    if position != a.position {
        fail(EngineError::HostMismatch);
    }
    let Some(AuctionScratch::Quote(ctx)) = a.scratch.clone() else {
        fail(EngineError::WrongAccount("auction phase"))
    };
    if a.phase != AuctionPhase::AwaitingSeize {
        fail(EngineError::WrongAccount("auction phase"));
    }
    let report = SeizeReport {
        owner,
        collateral,
        nominal_debt,
    };
    let (opened, quote) = quote_seized(&a, &ctx, &report).unwrap_or_else(|err| fail(err));
    let (e, me) = (engine(input), input.receiver.account_id);
    let vault = vault_holding(e, a.token, me);
    let definition = a.collateral_def;
    let state = AuctionState {
        scratch: Some(AuctionScratch::VaultRead {
            vault,
            definition,
            at_least: quote.lot,
        }),
        ..opened
    };
    let mut response = Response::set_state(state.encode());
    if quote.keeper_reward > 0 {
        let reward = TokenMsg::Transfer {
            definition,
            to: a.keeper,
            amount: quote.keeper_reward,
        };
        response = response.send(SyncMessage::new(vault, &reward).with_pda_seeds([vault_seed(me)]));
    }
    let reserve = EngineMsg::Reserve {
        position: a.position,
        nonce: a.nonce,
        amount: quote.reserve,
    };
    response
        .send_sync(collateral_actor(e, a.config, definition), &reserve)
        .send_sync(Actor::new(a.config, e), &reserve)
        .send_sync(vault, &TokenMsg::ReadState)
}

/// The vault's balance, from exactly the vault, while the check is pending.
pub fn on_vault_reply(input: &ReceiveInput) -> Option<Response> {
    let a = AuctionState::decode(input.pre_state.as_ref())?;
    let Some(AuctionScratch::VaultRead {
        vault,
        definition,
        at_least,
    }) = a.scratch
    else {
        return None;
    };
    if input.from != Some(vault) {
        return None;
    }
    let reply = StateReply::try_from_slice(&input.message)
        .unwrap_or_else(|_| fail(EngineError::VaultBelowBook));
    let held = if reply.state.as_ref().is_empty() {
        0
    } else {
        Holding::decode(reply.state.as_ref())
            .unwrap_or_else(|| fail(EngineError::VaultBelowBook))
            .balance(definition)
    };
    if held < at_least {
        fail(EngineError::VaultBelowBook);
    }
    let checked = AuctionState { scratch: None, ..a };
    Some(Response::set_state(checked.encode()))
}

/// The liquidation's sentinel, from exactly the auction's config.
pub fn on_finish(input: &ReceiveInput) -> Response {
    let a = load(input, EngineError::WrongAccount("finish"));
    expect_from(
        input,
        Actor::new(a.config, engine(input)),
        EngineError::WrongAccount("finish"),
    );
    if a.phase != AuctionPhase::Auctioning {
        fail(EngineError::NotAuctioning);
    }
    if a.scratch.is_some() {
        fail(EngineError::VaultBelowBook);
    }
    Response::keep_state()
}
