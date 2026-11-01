//! `Bid` (design note §4.2 rows 5-6, §6 flows; v0.2.4 `bid.rs`): one message for both of v0.2.4's
//! `bid` and `bid_with_settlement`, since the engine names the accounting itself.
//!
//! 1. Root `Bid` at the collateral actor `C = collateral_actor(E, config, def)`: Observe with the
//!    bid's clock claim (`observe::start`); at `Proceed`, `require_live` and the bid price, then a
//!    seeded `Probe { Auction }` (ruling R-9b) and `BidCtx` unseeded to the auction
//!    `auction_actor(E, config, position, nonce)` ([`bid_ctx`]).
//! 2. `BidCtx` at the auction, from exactly its collateral actor: v0.2.4's `quote_bid` and
//!    `apply_bid` (`engine_core::bid::quote`). Every refusal of the quote (`NotAuctioning`,
//!    `ZeroFill`, `Slippage`, `BelowMinBid`, `DustRemainder`) happens here, before any token
//!    moves. Then, in order (debt first, ADR-010):
//!    - `Burn(debt part)` at the bidder's holding `(bidder, T)` (authorized by its signature);
//!    - `Transfer(penalty part)` from the bidder's holding to the buffer `(pda(E, BUFFER), T)`;
//!    - the payout from the vault (VAULT seed, R-8b's token exception): `Transfer(collateral_out)`,
//!      or for the lot-emptying fill `Sweep { at_least: collateral_out }`, which pays donations
//!      too (ADR-033) and refuses a vault below the booked remainder (R6 floor, ruling R-5e);
//!    - at a chain end: `Settled` to the accounting, `Release` to the collateral actor and the
//!      config.
//! 3. `Settled` at the accounting, from exactly the auction `(config, position, nonce)` of the
//!    accounting's own config: v0.2.4's `record_settled` fold.

use auction_engine_params::EngineError;
use engine_core::{
    actors::{
        accounting_actor, auction_actor, buffer_holding, collateral_actor, vault_holding,
        vault_seed,
    },
    bid::{BidQuote, ChainEnd, SettledTotals, fold_settled, quote},
    check::{expect_from, expect_root, fail},
    msg::EngineMsg,
    state::{AccountingState, AuctionState, BidRequestRec, CollateralState, ObserveThen},
};
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, SyncMessage},
};
use token_core::TokenMsg;

use crate::{admin::engine, observe, proxy::auction_probe};

/// The root `Bid`'s routing fields; the rest is the request.
pub struct BidRoute {
    pub config: AccountId,
    pub collateral_def: AccountId,
    pub now_ms: u64,
}

/// Root `Bid` at the named collateral actor: the type must be configured; observe, then hand the
/// bid to the auction.
pub fn on_bid(input: &ReceiveInput, route: &BidRoute, request: BidRequestRec) -> Response {
    expect_root(input);
    if input.receiver != collateral_actor(engine(input), route.config, route.collateral_def) {
        fail(EngineError::WrongAccount("coll_config"));
    }
    let coll = CollateralState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("coll_config")));
    observe::start(input, coll, route.now_ms, ObserveThen::Bid(request))
}

/// At the collateral actor's `Proceed`, after a live observation: `BidCtx` for the auction, with
/// the type's current `min_bid` and `chost` (v0.2.4 quotes with the current parameters).
pub fn bid_ctx(
    input: &ReceiveInput,
    observed: &CollateralState,
    (price_ray, now_ms): (u128, u64),
    request: &BidRequestRec,
) -> (SyncMessage, Actor, EngineMsg) {
    let params = observed.collateral_params().unwrap_or_else(|err| fail(err));
    let chost = params
        .chost()
        .unwrap_or_else(|err| fail(EngineError::from(err)));
    let (auction, probe) = auction_probe(
        engine(input),
        observed.config,
        request.position,
        request.nonce,
    );
    let ctx = EngineMsg::BidCtx {
        bidder: request.bidder,
        position: request.position,
        nonce: request.nonce,
        max_debt_in: request.max_debt_in,
        min_collateral_out: request.min_collateral_out,
        price_ray,
        now_ms,
        paused_total_ms: observed.paused_total_ms,
        min_bid: params.min_bid,
        chost,
    };
    (probe, auction, ctx)
}

/// `BidCtx` without the routing fields.
pub struct BidCtxArgs {
    pub bidder: AccountId,
    pub position: AccountId,
    pub nonce: u64,
    pub quote: BidQuote,
}

/// `BidCtx` at the auction, from exactly `collateral_actor(E, config, collateral_def)`.
pub fn on_bid_ctx(input: &ReceiveInput, c: &BidCtxArgs) -> Response {
    let a = AuctionState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("auction")));
    let e = engine(input);
    expect_from(
        input,
        collateral_actor(e, a.config, a.collateral_def),
        EngineError::WrongAccount("coll_config"),
    );
    if (c.position, c.nonce) != (a.position, a.nonce) {
        fail(EngineError::WrongAccount("auction"));
    }
    if a.scratch.is_some() {
        fail(EngineError::NotAuctioning);
    }
    let outcome = quote(&a, &c.quote).unwrap_or_else(|err| fail(err));
    let fill = outcome.fill;
    let me = input.receiver.account_id;
    let holding = Actor::new(c.bidder, a.token);
    let mut response = Response::set_state(outcome.next.encode());
    if fill.debt_part > 0 {
        let burn = TokenMsg::Burn {
            definition: a.stable_def,
            amount: fill.debt_part,
        };
        response = response.send_sync(holding, &burn);
    }
    if fill.penalty_part > 0 {
        let penalty = TokenMsg::Transfer {
            definition: a.stable_def,
            to: buffer_holding(e, a.token, a.config).account_id,
            amount: fill.penalty_part,
        };
        response = response.send_sync(holding, &penalty);
    }
    let payout = if outcome.lot_empties {
        TokenMsg::Sweep {
            definition: a.collateral_def,
            to: c.bidder,
            at_least: fill.collateral_out,
        }
    } else {
        TokenMsg::Transfer {
            definition: a.collateral_def,
            to: c.bidder,
            amount: fill.collateral_out,
        }
    };
    let vault = vault_holding(e, a.token, me);
    response = response.send(SyncMessage::new(vault, &payout).with_pda_seeds([vault_seed(me)]));
    match outcome.end {
        Some(end) => chain_end(response, e, &outcome.next, end),
        None => response,
    }
}

/// The chain-end tail (bid, `SettleEmpty`): `Settled` with the settled auction's cumulative
/// totals to the accounting, then `Release` of the reservation to the collateral actor and the
/// config.
pub fn chain_end(
    response: Response,
    e: AccountId,
    settled: &AuctionState,
    end: ChainEnd,
) -> Response {
    let report = EngineMsg::Settled {
        config: settled.config,
        position: settled.position,
        nonce: settled.nonce,
        debt_raised: settled.debt_raised,
        penalty_raised: settled.penalty_raised,
        shortfall: end.shortfall,
        release: end.release,
        leftover: end.leftover,
    };
    let release = EngineMsg::Release {
        position: settled.position,
        nonce: settled.nonce,
        amount: end.release,
    };
    response
        .send_sync(accounting_actor(e, settled.config), &report)
        .send_sync(
            collateral_actor(e, settled.config, settled.collateral_def),
            &release,
        )
        .send_sync(Actor::new(settled.config, e), &release)
}

/// `Settled` at `accounting_actor(E, config)`, from exactly `auction_actor(E, config, position,
/// nonce)`.
pub fn on_settled(
    input: &ReceiveInput,
    (config, position, nonce): (AccountId, AccountId, u64),
    totals: &SettledTotals,
) -> Response {
    let e = engine(input);
    expect_from(
        input,
        auction_actor(e, config, position, nonce),
        EngineError::WrongAccount("auction"),
    );
    if input.receiver != accounting_actor(e, config) {
        fail(EngineError::WrongAccount("accounting"));
    }
    let acc = AccountingState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("accounting")));
    Response::set_state(fold_settled(&acc, totals).encode())
}
