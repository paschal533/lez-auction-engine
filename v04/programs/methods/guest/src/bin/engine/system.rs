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
//!    `min(amount, buffer, unbacked - debt_on_auction)` from the buffer under its `BUFFER` seed;
//!    `StartSurplus` (its nonce checked before the reads) starts a surplus auction
//!    (`surplus::start`); `StartDebt` (its nonce and relist delay checked before the reads)
//!    starts a debt auction (`debt::start`); `CheckSolvency` settles, then refuses while a debt
//!    auction is live or, on a `Freeze` decision, sends `Freeze` to the config, which sends
//!    `EngineFreezeNewDebt` to its bound host ([`on_freeze`]).

use auction_engine_params::EngineError;
use borsh::BorshDeserialize;
use engine_core::{
    SystemCtx, SystemOp, SystemPending,
    actors::{accounting_actor, buffer_holding, buffer_seed},
    assertions::freeze_sender,
    check::{expect_root, fail},
    msg::{ClockMsg, EngineMsg},
    solvency::SolvencyDecision,
    state::{AccountingState, ConfigState, OracleReadingRec},
    system::{check_solvency_op, debt_precheck, ref_price},
    system_state::RefReading,
};
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, StateReply, SyncMessage},
};
use mock_host_core::HostMsg;
use oracle_core::{OracleMsg, OracleState};
use token_core::{Holding, TokenMsg, holding_actor};

use crate::{admin::engine, debt, surplus};

/// Root `SettleDebt` at the named config (row 9).
pub fn on_settle_debt(input: &ReceiveInput, config: AccountId, amount: u128) -> Response {
    expect_root(input);
    root(input, config, SystemOp::SettleDebt { amount }, 0)
}

/// Root `CheckSolvency` at the named config (row 16).
pub fn on_check_solvency(input: &ReceiveInput, config: AccountId, now_ms: u64) -> Response {
    expect_root(input);
    root(input, config, SystemOp::CheckSolvency, now_ms)
}

/// `Freeze` at the config, from exactly its accounting (`freeze_sender`): `EngineFreezeNewDebt`
/// to the config's bound host, which accepts it only from this config actor.
pub fn on_freeze(input: &ReceiveInput) -> Response {
    let c = freeze_sender(input).unwrap_or_else(|err| fail(err));
    Response::keep_state().send_sync(c.host, &HostMsg::EngineFreezeNewDebt)
}

/// A system root at the config `config`: asserts the clock claim `now_ms` (except for
/// `SettleDebt`) and hands `op` to the config's accounting.
pub fn root(input: &ReceiveInput, config: AccountId, op: SystemOp, now_ms: u64) -> Response {
    let claim = (!matches!(op, SystemOp::SettleDebt { .. })).then_some(now_ms);
    let (c, response) = config_root(input, config, claim);
    let system = EngineMsg::System {
        op,
        now_ms,
        ctx: Box::new(SystemCtx::from_config(config, &c)),
    };
    response.send_sync(accounting_actor(engine(input), config), &system)
}

/// A root at the config `config` (every system root, ruling R-9d): the receiver must be that
/// config (`WrongAccount("config")`); the config's state, and a response that asserts the clock
/// claim `now_ms`, if any, on the config's clock (`AssertTimestamp { now, now + slack }`).
pub fn config_root(
    input: &ReceiveInput,
    config: AccountId,
    now_ms: Option<u64>,
) -> (ConfigState, Response) {
    if input.receiver != Actor::new(config, engine(input)) {
        fail(EngineError::WrongAccount("config"));
    }
    let c = ConfigState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("config")));
    let response = Response::keep_state();
    let Some(now_ms) = now_ms else {
        return (c, response);
    };
    let at_most = now_ms
        .checked_add(c.slack_ms)
        .unwrap_or_else(|| fail(EngineError::Math));
    let assert = ClockMsg::AssertTimestamp {
        at_least: now_ms,
        at_most,
    };
    let response = response.send_sync(c.clock, &assert);
    (c, response)
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
    precheck(op, now_ms, &acc);
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

/// The op's checks before any read: a surplus start names the accounting's next nonce
/// (`NonceMismatch`); a debt start too, and its relist delay has elapsed at the claimed time
/// `now_ms` (`DelayNotElapsed`, ADR-039); `SettleDebt` and `CheckSolvency` check nothing.
fn precheck(op: SystemOp, now_ms: u64, acc: &AccountingState) {
    match op {
        SystemOp::SettleDebt { .. } | SystemOp::CheckSolvency => {}
        SystemOp::StartSurplus { nonce } => {
            if nonce != acc.surplus_nonce {
                fail(EngineError::NonceMismatch);
            }
        }
        SystemOp::StartDebt { nonce } => {
            debt_precheck(acc, nonce, now_ms).unwrap_or_else(|err| fail(err));
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
        pending.buffer = Some(holding_balance(input, EngineError::WrongAccount("buffer")));
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

/// A holding's balance from its `StateReply`: 0 for an empty state (never credited), else a
/// holding's; anything else fails with `err`.
pub fn holding_balance(input: &ReceiveInput, err: EngineError) -> u128 {
    let reply = StateReply::try_from_slice(&input.message).unwrap_or_else(|_| fail(err));
    let state = reply.state.as_ref();
    if state.is_empty() {
        return 0;
    }
    Holding::decode(state).unwrap_or_else(|| fail(err)).balance
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
        SystemOp::StartSurplus { nonce } => {
            surplus::start(input, &cleared, &pending, (buffer, nonce))
        }
        SystemOp::StartDebt { nonce } => debt::start(input, &cleared, &pending, (buffer, nonce)),
        SystemOp::CheckSolvency => check_solvency(input, &cleared, &pending, buffer),
    }
}

/// v0.2.4 `settle_debt`: books the burn, then the buffer's `Burn` ([`buffer_burn`]).
fn settle_debt(
    input: &ReceiveInput,
    acc: &AccountingState,
    ctx: &SystemCtx,
    (amount, buffer): (u128, u128),
) -> Response {
    let (next, burned) = acc.core().settle_debt(amount, buffer);
    let response = Response::set_state(AccountingState::with_core(acc, &next).encode());
    buffer_burn(input, ctx, response, burned)
}

/// v0.2.4 `check_solvency` at the accounting's `Proceed`: books the implicit settle (and, on a
/// `Freeze`, `host_frozen_by_engine`), sends the buffer's `Burn` first, then on a `Freeze`
/// decision `Freeze` to the config, which asks the host. `RecoveryInProgress` reverts the whole
/// transaction, settle included; so does a host that refuses the freeze (Review Focus 4).
fn check_solvency(
    input: &ReceiveInput,
    acc: &AccountingState,
    pending: &SystemPending,
    buffer: u128,
) -> Response {
    let (ctx, now_ms) = (&pending.ctx, pending.now_ms);
    let reference = ref_price(ctx, pending.reference, now_ms);
    let (next, burned, decision) =
        check_solvency_op(acc, ctx, buffer, reference, now_ms).unwrap_or_else(|err| fail(err));
    let response = buffer_burn(input, ctx, Response::set_state(next.encode()), burned);
    if decision != SolvencyDecision::Freeze {
        return response;
    }
    response.send_sync(Actor::new(ctx.config, engine(input)), &EngineMsg::Freeze)
}

/// Appends the buffer's `Burn` of `burned` at its token actor (`BUFFER` seed, R-8b's token
/// exception) if anything burns; a `Burn` on an absent holding would write a fresh state the
/// transaction cannot admit.
fn buffer_burn(
    input: &ReceiveInput,
    ctx: &SystemCtx,
    response: Response,
    burned: u128,
) -> Response {
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
