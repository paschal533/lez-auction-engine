//! Observe block at the collateral actor `C = collateral_actor(E, config, def)` (design note §4.1,
//! v0.2.4 `feed.rs` / `observe.rs`), shared by `SetCollateral` and `Liquidate`:
//!
//! 1. `start` stores a [`PendingObserve`] and sends `ReadState` to the host params actor
//!    (redemption price; its oracle must be ours, ADR-028), `ReadState` to the oracle,
//!    `AssertTimestamp { now, now + slack }` to the clock, and `Proceed` to itself.
//! 2. `on_reply` takes each reading only from exactly the actor asked, once.
//! 3. `proceed` (run after every earlier subtree) needs both readings, runs `feed::observe`,
//!    writes the pause clock, clears the scratch and runs the continuation: nothing more for
//!    `SetCollateral`; for `Liquidate`, `require_live` and the bid price, then `LiqCtx` to the
//!    config (row 4).
//!
//! While an observation is pending the actor accepts nothing but the awaited replies and its
//! own `Proceed` (design note §4.3 c, guard 2).

use auction_engine_params::EngineError;
use borsh::BorshDeserialize;
use engine_core::{
    actors::collateral_actor,
    check::{expect_from, fail},
    feed,
    msg::{ClockMsg, EngineMsg},
    state::{CollateralState, ObserveThen, OracleReadingRec, PendingObserve},
};
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, StateReply},
};
use mock_host_core::{HostMsg, Params};
use oracle_core::{OracleMsg, OracleState};

use crate::admin::{engine, engine_sender};

/// A keeper's `Liquidate`, as the proxy forwarded it.
pub struct LiquidateRequest {
    pub config: AccountId,
    pub collateral_def: AccountId,
    pub position: AccountId,
    pub nonce: u64,
    pub now_ms: u64,
}

/// `Liquidate` at the named collateral actor, forwarded by the keeper's proxy `(keeper, E)`: the
/// type must be configured; observe, then hand over to the config.
pub fn on_liquidate(input: &ReceiveInput, r: &LiquidateRequest) -> Response {
    let keeper = engine_sender(input, EngineError::WrongAccount("keeper"));
    if input.receiver != collateral_actor(engine(input), r.config, r.collateral_def) {
        fail(EngineError::WrongAccount("coll_config"));
    }
    let coll = CollateralState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("coll_config")));
    let then = ObserveThen::Liquidate {
        keeper,
        position: r.position,
        nonce: r.nonce,
    };
    start(input, coll, r.now_ms, then)
}

/// `Observe` at the collateral actor, from exactly its config (after its seeded `Configure`):
/// observe at `now_ms` and only record the pause clock.
pub fn on_observe(input: &ReceiveInput, now_ms: u64) -> Response {
    let coll = CollateralState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("coll_config")));
    let config = Actor::new(coll.config, engine(input));
    expect_from(input, config, EngineError::NotAdmin);
    start(input, coll, now_ms, ObserveThen::Record)
}

/// Writes `coll` with a fresh pending observation at the clock claim `now_ms` and sends the two
/// reads, the clock assertion and the `Proceed` sentinel, in that order.
pub fn start(
    input: &ReceiveInput,
    coll: CollateralState,
    now_ms: u64,
    then: ObserveThen,
) -> Response {
    if coll.pending.is_some() {
        fail(EngineError::WrongAccount("pending"));
    }
    let ctx = coll.ctx;
    let at_most = now_ms
        .checked_add(ctx.slack_ms)
        .unwrap_or_else(|| fail(EngineError::Math));
    let pending = PendingObserve {
        now_ms,
        redemption_ray: None,
        reading: None,
        then,
    };
    let next = CollateralState {
        pending: Some(pending),
        ..coll
    };
    let assert = ClockMsg::AssertTimestamp {
        at_least: now_ms,
        at_most,
    };
    Response::set_state(next.encode())
        .send_sync(ctx.host, &HostMsg::ReadState)
        .send_sync(ctx.oracle, &OracleMsg::ReadState)
        .send_sync(ctx.clock, &assert)
        .send_sync(input.receiver, &EngineMsg::Proceed)
}

/// A reading for a pending observation, from exactly the host or the oracle, each once.
pub fn on_reply(input: &ReceiveInput) -> Option<Response> {
    let coll = CollateralState::decode(input.pre_state.as_ref())?;
    let mut pending = coll.pending.clone()?;
    let from = input.from?;
    if from == coll.ctx.host && pending.redemption_ray.is_none() {
        let params = reply_state(input, Params::decode, EngineError::HostMismatch);
        // ADR-028: the price comes from the oracle the host's health gate uses.
        if params.oracle != coll.ctx.oracle {
            fail(EngineError::OracleMismatch);
        }
        pending.redemption_ray = Some(params.redemption_price_ray);
    } else if from == coll.ctx.oracle && pending.reading.is_none() {
        let oracle = reply_state(input, OracleState::decode, EngineError::OracleMismatch);
        pending.reading = Some(OracleReadingRec {
            price_raw: oracle.price_ray,
            confidence_raw: oracle.confidence,
            timestamp_ms: oracle.timestamp_ms,
        });
    } else {
        return None;
    }
    let next = CollateralState {
        pending: Some(pending),
        ..coll
    };
    Some(Response::set_state(next.encode()))
}

/// The `StateReply`'s state decoded with `decode`, or `err`.
fn reply_state<T>(input: &ReceiveInput, decode: fn(&[u8]) -> Option<T>, err: EngineError) -> T {
    let reply = StateReply::try_from_slice(&input.message).unwrap_or_else(|_| fail(err));
    decode(reply.state.as_ref()).unwrap_or_else(|| fail(err))
}

/// The sentinel at the collateral actor (the caller checked it came from the actor itself).
pub fn proceed(input: &ReceiveInput) -> Response {
    let coll = CollateralState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("coll_config")));
    let pending = coll
        .pending
        .clone()
        .unwrap_or_else(|| fail(EngineError::WrongAccount("proceed")));
    let r_ray = pending
        .redemption_ray
        .unwrap_or_else(|| fail(EngineError::HostMismatch));
    let reading = pending
        .reading
        .unwrap_or_else(|| fail(EngineError::OracleMismatch));
    let obs = feed::observe(
        &coll,
        &reading.to_core(),
        r_ray,
        pending.now_ms,
        coll.ctx.slack_ms,
    )
    .unwrap_or_else(|err| fail(err));
    let observed = CollateralState {
        pending: None,
        ..coll.with_pause_clock(obs.clock)
    };
    let response = Response::set_state(observed.encode());
    match pending.then {
        ObserveThen::Record => response,
        ObserveThen::Liquidate {
            keeper,
            position,
            nonce,
        } => {
            let price_ray = feed::live_price(&coll, &obs, r_ray).unwrap_or_else(|err| fail(err));
            let config = Actor::new(observed.config, engine(input));
            let ctx = EngineMsg::LiqCtx {
                position,
                nonce,
                keeper,
                now_ms: pending.now_ms,
                price_ray,
                collateral: Box::new(observed),
            };
            response.send_sync(config, &ctx)
        }
    }
}
