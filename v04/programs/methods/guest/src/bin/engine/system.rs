//! System operations (design note §4.2 rows 9-16, §4.3 a): the one Read block at the accounting
//! actor `Acc = accounting_actor(E, K)` that `settle_debt`, the surplus and debt auctions and
//! `check_solvency` share.
//!
//! 1. Root at the config `K = (config, E)`, permissionless ([`root`]): the receiver must be the
//!    named config (`WrongAccount("config")`); `AssertTimestamp { now, now + slack }` to the clock
//!    for every op but `SettleDebt`, which claims no time; then `System` with the config's
//!    [`SystemCtx`] to its accounting. Every system root is at `K` (ruling R-9d): `Acc` does not
//!    store its config, and an absent surplus/debt auction could not yield the engine's error.
//! 2. `System` at `Acc` ([`on_system`]), only from `(k, E)` with `receiver ==
//!    accounting_actor(E, k)` and `ctx.config == k`: writes a [`SystemPending`], sends `ReadState`
//!    to the buffer's stablecoin holding, `ReadState` to the reference oracle (every op but
//!    `SettleDebt`, when configured), and `Proceed` to itself.
//! 3. [`on_reply`] takes each reading from exactly the actor asked, once; a second reply fails
//!    `WrongAccount("pending")`. An empty buffer holding (never credited) reads as 0; a reference
//!    reply that is not an oracle state is `RefReading::Undecodable` (admin price, never a panic).
//! 4. [`proceed`] needs every reading, clears the scratch and runs the op: `SettleDebt` burns
//!    `min(amount, buffer, unbacked - debt_on_auction)` from the buffer under its `BUFFER` seed.

use auction_engine_params::EngineError;
use borsh::BorshDeserialize;
use engine_core::{
    SystemCtx, SystemOp, SystemPending,
    actors::{accounting_actor, buffer_holding, buffer_seed},
    check::{expect_root, fail},
    msg::{ClockMsg, EngineMsg},
    state::{AccountingState, ConfigState, OracleReadingRec},
    system_state::RefReading,
};
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, StateReply, SyncMessage},
};
use oracle_core::{OracleMsg, OracleState};
use token_core::{Holding, TokenMsg, holding_actor};

use crate::admin::engine;

/// Root `SettleDebt` at the named config (row 9).
pub fn on_settle_debt(input: &ReceiveInput, config: AccountId, amount: u128) -> Response {
    expect_root(input);
    root(input, config, SystemOp::SettleDebt { amount }, 0)
}

/// A system root at the config `config`: asserts the clock claim `now_ms` (except for
/// `SettleDebt`) and hands `op` to the config's accounting.
pub fn root(input: &ReceiveInput, config: AccountId, op: SystemOp, now_ms: u64) -> Response {
    let e = engine(input);
    if input.receiver != Actor::new(config, e) {
        fail(EngineError::WrongAccount("config"));
    }
    let c = ConfigState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("config")));
    let mut response = Response::keep_state();
    if !matches!(op, SystemOp::SettleDebt { .. }) {
        let at_most = now_ms
            .checked_add(c.slack_ms)
            .unwrap_or_else(|| fail(EngineError::Math));
        let assert = ClockMsg::AssertTimestamp {
            at_least: now_ms,
            at_most,
        };
        response = response.send_sync(c.clock, &assert);
    }
    let system = EngineMsg::System {
        op,
        now_ms,
        ctx: Box::new(SystemCtx::from_config(config, &c)),
    };
    response.send_sync(accounting_actor(e, config), &system)
}

/// The buffer's stablecoin holding of `ctx`'s config, the one actor whose balance counts.
fn buffer_reader(e: AccountId, ctx: &SystemCtx) -> Actor {
    let buffer = buffer_holding(e, ctx.token, ctx.config);
    holding_actor(ctx.token, buffer.account_id, ctx.stable_def)
}

/// `System` at the accounting, from exactly its config: start the Read block.
pub fn on_system(input: &ReceiveInput, op: SystemOp, now_ms: u64, ctx: SystemCtx) -> Response {
    let e = engine(input);
    let k = input
        .from
        .filter(|from| from.program_account_id == e)
        .map(|from| from.account_id)
        .unwrap_or_else(|| fail(EngineError::WrongAccount("accounting")));
    if input.receiver != accounting_actor(e, k) || ctx.config != k {
        fail(EngineError::WrongAccount("accounting"));
    }
    let acc = AccountingState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("accounting")));
    if acc.pending.is_some() {
        fail(EngineError::WrongAccount("pending"));
    }
    precheck(op);
    let pending = SystemPending {
        op,
        now_ms,
        ctx,
        buffer: None,
        reference: None,
    };
    let oracle = pending
        .ctx
        .ref_oracle
        .filter(|_| pending.awaits_reference());
    let buffer = buffer_reader(e, &pending.ctx);
    let next = AccountingState {
        pending: Some(pending),
        ..acc
    };
    let mut response = Response::set_state(next.encode()).send_sync(buffer, &TokenMsg::ReadState);
    if let Some(oracle) = oracle {
        response = response.send_sync(oracle, &OracleMsg::ReadState);
    }
    response.send_sync(input.receiver, &EngineMsg::Proceed)
}

/// The op's checks before any read. Ops 1-3 arrive with Tasks 4-6.
fn precheck(op: SystemOp) {
    match op {
        SystemOp::SettleDebt { .. } => {}
        SystemOp::StartSurplus { .. } | SystemOp::StartDebt { .. } | SystemOp::CheckSolvency => {
            panic!("engine: BadMessage")
        }
    }
}

/// A reading for the pending system op, from exactly the buffer holding or the reference oracle,
/// each once.
pub fn on_reply(input: &ReceiveInput) -> Option<Response> {
    let acc = AccountingState::decode(input.pre_state.as_ref())?;
    let mut pending = acc.pending.clone()?;
    let from = input.from?;
    let buffer = buffer_reader(engine(input), &pending.ctx);
    let reference = pending
        .ctx
        .ref_oracle
        .filter(|_| pending.awaits_reference());
    if from == buffer && pending.buffer.is_none() {
        pending.buffer = Some(buffer_balance(input));
    } else if Some(from) == reference && pending.reference.is_none() {
        pending.reference = Some(reference_reading(input));
    } else if from == buffer || Some(from) == reference {
        fail(EngineError::WrongAccount("pending"));
    } else {
        return None;
    }
    let next = AccountingState {
        pending: Some(pending),
        ..acc
    };
    Some(Response::set_state(next.encode()))
}

/// The buffer holding's balance: 0 for an empty state (never credited), else a holding's.
fn buffer_balance(input: &ReceiveInput) -> u128 {
    let reply = StateReply::try_from_slice(&input.message)
        .unwrap_or_else(|_| fail(EngineError::WrongAccount("buffer")));
    let state = reply.state.as_ref();
    if state.is_empty() {
        return 0;
    }
    Holding::decode(state)
        .unwrap_or_else(|| fail(EngineError::WrongAccount("buffer")))
        .balance
}

/// The reference oracle's reading; anything but an oracle state is `Undecodable`.
fn reference_reading(input: &ReceiveInput) -> RefReading {
    StateReply::try_from_slice(&input.message)
        .ok()
        .and_then(|reply| OracleState::decode(reply.state.as_ref()))
        .map_or(RefReading::Undecodable, |o| {
            RefReading::Valid(OracleReadingRec {
                price_raw: o.price_ray,
                confidence_raw: o.confidence,
                timestamp_ms: o.timestamp_ms,
            })
        })
}

/// The sentinel at the accounting (the caller checked it came from the actor itself): every
/// reading must be in; clear the scratch and run the op.
pub fn proceed(input: &ReceiveInput) -> Response {
    let acc = AccountingState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("accounting")));
    let pending = acc
        .pending
        .clone()
        .unwrap_or_else(|| fail(EngineError::WrongAccount("proceed")));
    let buffer = pending
        .buffer
        .unwrap_or_else(|| fail(EngineError::WrongAccount("buffer")));
    if pending.awaits_reference() && pending.reference.is_none() {
        fail(EngineError::WrongAccount("ref_oracle"));
    }
    let cleared = AccountingState {
        pending: None,
        ..acc
    };
    match pending.op {
        SystemOp::SettleDebt { amount } => {
            settle_debt(input, &cleared, &pending.ctx, (amount, buffer))
        }
        SystemOp::StartSurplus { .. } | SystemOp::StartDebt { .. } | SystemOp::CheckSolvency => {
            panic!("engine: BadMessage")
        }
    }
}

/// v0.2.4 `settle_debt`: books the burn, then `Burn` at the buffer's token actor (`BUFFER` seed,
/// R-8b's token exception) if anything burns; a `Burn` on an absent holding would write a fresh
/// state the transaction cannot admit.
fn settle_debt(
    input: &ReceiveInput,
    acc: &AccountingState,
    ctx: &SystemCtx,
    (amount, buffer): (u128, u128),
) -> Response {
    let (next, burned) = acc.core().settle_debt(amount, buffer);
    let response = Response::set_state(AccountingState::with_core(acc, &next).encode());
    if burned == 0 {
        return response;
    }
    let burn = TokenMsg::Burn {
        definition: ctx.stable_def,
        amount: burned,
    };
    let holder = buffer_holding(engine(input), ctx.token, ctx.config);
    response.send(SyncMessage::new(holder, &burn).with_pda_seeds([buffer_seed(ctx.config)]))
}
