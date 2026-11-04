//! Surplus auction (design note §4.2 rows 10-12; v0.2.4 `surplus.rs`): instant Dutch, protocol
//! tokens burned in the same transaction. The lot moves from the buffer into the auction's own
//! vault `(pda(E, ["SURPVAULT", S]), T)` at start, so a later `settle_debt` cannot spend it
//! (ADR-031); the reference price is snapshotted at start.
//!
//! - **Start** (root `StartSurplus` at `K`, a system op): at the accounting's `Proceed`,
//!   [`start`] runs `start_surplus_op` and sends, in order, the buffer's `Burn` of the implicit
//!   settle (if any) and its `Transfer` of the lot to the vault (both `BUFFER` seed), then the
//!   seeded leaf `SurplusOpen` that creates `S = surplus_actor(E, K, nonce)`.
//! - **Take** (root `TakeSurplus` at `K`): `K` refuses a config without protocol token, asserts
//!   the clock, sends a seeded `Probe { Surplus }` (an absent `S` fails with the engine's
//!   `WrongAccount("surplus_auction")`) and `SurplusTake`. `S` runs `auction_core`'s
//!   `take_surplus` and sends the taker's protocol `Burn` (the taker signs), the vault's
//!   `Transfer` of the filled amount (`SURPVAULT` seed) and `SurplusTaken` to the accounting.
//! - **Close** (root `CloseSurplus` at `K`): clock, probe, `SurplusClose`. `S` runs
//!   `close_surplus`, ends, reads its vault holding and sends `Proceed` to itself; the reply from
//!   exactly that holding fills the scratch; `Proceed` sweeps the whole balance (donations
//!   included, ADR-033) back to the buffer if any, and reports `SurplusReturned`.
//!
//! The accounting accepts `SurplusTaken` / `SurplusReturned` only from the auction its own config
//! derives for the reported nonce.

use auction_engine_params::EngineError;
use engine_core::{
    SystemCtx, SystemOp, SystemPending,
    actors::{
        accounting_actor, buffer_holding, buffer_seed, surplus_actor, surplus_seed,
        surplus_vault_holding, surplus_vault_seed,
    },
    assertions::surplus_report_sender,
    check::{expect_from, expect_root, fail},
    msg::EngineMsg,
    state::AccountingState,
    system::{close_surplus_op, ref_price, start_surplus_op, take_surplus_op},
    system_state::{SurplusAuctionState, SurplusCloseRec, SystemPhase, TAG_SURPLUS},
};
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, SyncMessage},
};
use token_core::{TokenMsg, holding_actor};

use crate::{admin::engine, proxy::surplus_probe, system};

/// Root `StartSurplus` at the named config (row 10).
pub fn on_start(input: &ReceiveInput, config: AccountId, nonce: u64, now_ms: u64) -> Response {
    expect_root(input);
    system::root(input, config, SystemOp::StartSurplus { nonce }, now_ms)
}

/// At the accounting's `Proceed` for `StartSurplus { nonce }`, with every reading in: books the
/// start, then moves the lot and opens the auction.
pub fn start(
    input: &ReceiveInput,
    acc: &AccountingState,
    pending: &SystemPending,
    (buffer, nonce): (u128, u64),
) -> Response {
    let (ctx, now_ms) = (&pending.ctx, pending.now_ms);
    let reference = ref_price(ctx, pending.reference, now_ms);
    let (next, s, burned) = start_surplus_op(acc, ctx, buffer, reference, now_ms, nonce)
        .unwrap_or_else(|err| fail(err));
    let e = engine(input);
    let holder = buffer_holding(e, ctx.token, ctx.config);
    let from_buffer =
        |msg: &TokenMsg| SyncMessage::new(holder, msg).with_pda_seeds([buffer_seed(ctx.config)]);
    let mut response = Response::set_state(next.encode());
    if burned > 0 {
        let burn = TokenMsg::Burn {
            definition: ctx.stable_def,
            amount: burned,
        };
        response = response.send(from_buffer(&burn));
    }
    let surplus = surplus_actor(e, ctx.config, nonce);
    let lot = TokenMsg::Transfer {
        definition: ctx.stable_def,
        to: surplus_vault_holding(e, ctx.token, surplus.account_id).account_id,
        amount: s.lot_left,
    };
    let open = EngineMsg::SurplusOpen {
        config: ctx.config,
        nonce,
        lot: s.lot_left,
        start_ms: s.start_ms,
        ref_ray: s.ref_ray,
    };
    response
        .send(from_buffer(&lot))
        .send(SyncMessage::new(surplus, &open).with_pda_seeds([surplus_seed(ctx.config, nonce)]))
}

/// `SurplusOpen` at `surplus_actor(E, config, nonce)`, from exactly `accounting_actor(E, config)`
/// with the SURP seed, on an empty state: creates the live auction. Sends nothing.
pub fn on_open(
    input: &ReceiveInput,
    (config, nonce): (AccountId, u64),
    lot: u128,
    start_ms: u64,
    ref_ray: u128,
) -> Response {
    let e = engine(input);
    let err = EngineError::WrongAccount("surplus_auction");
    expect_from(input, accounting_actor(e, config), err);
    if input.receiver != surplus_actor(e, config, nonce)
        || !input.pre_state.as_ref().is_empty()
        || !input.is_authorized
    {
        fail(err);
    }
    let state = SurplusAuctionState {
        tag: TAG_SURPLUS,
        config,
        nonce,
        lot_left: lot,
        start_ms,
        ref_ray,
        phase: SystemPhase::Live,
        close: None,
    };
    Response::set_state(state.encode())
}

/// Root `TakeSurplus` / `CloseSurplus` at `K`: the named config, the clock claim and the auction
/// probe; the caller sends its message unseeded as the probe's next sibling.
fn root_at_config(
    input: &ReceiveInput,
    (config, nonce): (AccountId, u64),
    now_ms: u64,
    token_required: bool,
) -> (Response, Actor, Box<SystemCtx>) {
    expect_root(input);
    let (c, response) = system::config_root(input, config, Some(now_ms));
    if token_required && c.protocol_token.is_none() {
        fail(EngineError::TokenNotConfigured);
    }
    let (surplus, probe) = surplus_probe(engine(input), config, nonce);
    let ctx = Box::new(SystemCtx::from_config(config, &c));
    (response.send(probe), surplus, ctx)
}

/// `TakeSurplus` without its routing fields.
pub struct TakeArgs {
    pub taker: AccountId,
    pub amount_coin: u128,
    pub max_protocol_in: u128,
    pub now_ms: u64,
}

/// Root `TakeSurplus` at the named config (row 11).
pub fn on_take_root(input: &ReceiveInput, at: (AccountId, u64), t: &TakeArgs) -> Response {
    let (response, surplus, ctx) = root_at_config(input, at, t.now_ms, true);
    let take = EngineMsg::SurplusTake {
        taker: t.taker,
        amount_coin: t.amount_coin,
        max_protocol_in: t.max_protocol_in,
        now_ms: t.now_ms,
        ctx,
    };
    response.send_sync(surplus, &take)
}

/// Root `CloseSurplus` at the named config (row 12).
pub fn on_close_root(input: &ReceiveInput, at: (AccountId, u64), now_ms: u64) -> Response {
    let (response, surplus, ctx) = root_at_config(input, at, now_ms, false);
    response.send_sync(surplus, &EngineMsg::SurplusClose { now_ms, ctx })
}

/// The receiving auction, which must be the one its config `ctx.config` derives, and the message
/// must come from exactly that config `(config, E)`.
fn load_from_config(input: &ReceiveInput, ctx: &SystemCtx) -> SurplusAuctionState {
    let err = EngineError::WrongAccount("surplus_auction");
    let s = SurplusAuctionState::decode(input.pre_state.as_ref()).unwrap_or_else(|| fail(err));
    let e = engine(input);
    expect_from(
        input,
        Actor::new(s.config, e),
        EngineError::WrongAccount("config"),
    );
    if ctx.config != s.config || input.receiver != surplus_actor(e, s.config, s.nonce) {
        fail(err);
    }
    s
}

/// `SurplusTake` at the auction: v0.2.4's `take_surplus`, then the burn, the payout and the
/// report. Token refusals (an unsigned taker, too few protocol tokens) are the token's.
pub fn on_take(input: &ReceiveInput, t: &TakeArgs, ctx: &SystemCtx) -> Response {
    let s = load_from_config(input, ctx);
    let protocol = ctx
        .protocol_token
        .unwrap_or_else(|| fail(EngineError::TokenNotConfigured));
    let (next, tokens, filled) =
        take_surplus_op(&s, ctx, (t.amount_coin, t.max_protocol_in), t.now_ms)
            .unwrap_or_else(|err| fail(err));
    let (e, me) = (engine(input), input.receiver.account_id);
    let burn = TokenMsg::Burn {
        definition: protocol,
        amount: tokens,
    };
    let pay = TokenMsg::Transfer {
        definition: ctx.stable_def,
        to: t.taker,
        amount: filled,
    };
    let vault = surplus_vault_holding(e, ctx.token, me);
    let taken = EngineMsg::SurplusTaken {
        config: s.config,
        nonce: s.nonce,
        protocol_burned: tokens,
        ended: next.phase == SystemPhase::Ended,
        now_ms: t.now_ms,
    };
    Response::set_state(next.encode())
        .send_sync(Actor::new(t.taker, ctx.token), &burn)
        .send(SyncMessage::new(vault, &pay).with_pda_seeds([surplus_vault_seed(me)]))
        .send_sync(accounting_actor(e, s.config), &taken)
}

/// `SurplusClose` at the auction: v0.2.4's `close_surplus` (`NotLive`, `DelayNotElapsed`), then
/// the vault read and the `Proceed` sentinel.
pub fn on_close(input: &ReceiveInput, now_ms: u64, ctx: &SystemCtx) -> Response {
    let s = load_from_config(input, ctx);
    let ended = close_surplus_op(&s, ctx, now_ms).unwrap_or_else(|err| fail(err));
    let (e, me) = (engine(input), input.receiver.account_id);
    let vault = surplus_vault_holding(e, ctx.token, me);
    let holding = holding_actor(ctx.token, vault.account_id, ctx.stable_def);
    let close = SurplusCloseRec {
        vault: holding,
        amount: None,
        buffer: buffer_holding(e, ctx.token, s.config).account_id,
        stable_def: ctx.stable_def,
        now_ms,
    };
    let closing = SurplusAuctionState {
        close: Some(close),
        ..ended
    };
    Response::set_state(closing.encode())
        .send_sync(holding, &TokenMsg::ReadState)
        .send_sync(input.receiver, &EngineMsg::Proceed)
}

/// The vault holding's balance for a pending close, from exactly that holding, once.
pub fn on_reply(input: &ReceiveInput) -> Option<Response> {
    let s = SurplusAuctionState::decode(input.pre_state.as_ref())?;
    let mut close = s.close?;
    if input.from != Some(close.vault) {
        return None;
    }
    if close.amount.is_some() {
        fail(EngineError::WrongAccount("pending"));
    }
    let err = EngineError::WrongAccount("surplus_vault");
    close.amount = Some(system::holding_balance(input, err));
    let read = SurplusAuctionState {
        close: Some(close),
        ..s
    };
    Some(Response::set_state(read.encode()))
}

/// The close's sentinel at the auction (the caller checked it came from the actor itself): the
/// vault's balance must be in; clear the scratch, sweep the balance to the buffer and report it.
pub fn proceed(input: &ReceiveInput) -> Response {
    let s = SurplusAuctionState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("surplus_auction")));
    let close = s
        .close
        .unwrap_or_else(|| fail(EngineError::WrongAccount("proceed")));
    let amount = close
        .amount
        .unwrap_or_else(|| fail(EngineError::WrongAccount("surplus_vault")));
    let (e, me) = (engine(input), input.receiver.account_id);
    let returned = EngineMsg::SurplusReturned {
        config: s.config,
        nonce: s.nonce,
        amount,
        now_ms: close.now_ms,
    };
    let accounting = accounting_actor(e, s.config);
    let mut response = Response::set_state(SurplusAuctionState { close: None, ..s }.encode());
    if amount > 0 {
        let sweep = TokenMsg::Sweep {
            definition: close.stable_def,
            to: close.buffer,
            at_least: amount,
        };
        let vault = surplus_vault_holding(e, close.vault.program_account_id, me);
        response =
            response.send(SyncMessage::new(vault, &sweep).with_pda_seeds([surplus_vault_seed(me)]));
    }
    response.send_sync(accounting, &returned)
}

/// The accounting of `config`, receiving from exactly `surplus_actor(E, config, nonce)`
/// (`surplus_report_sender`).
fn accounting_from(input: &ReceiveInput, config: AccountId, nonce: u64) -> AccountingState {
    surplus_report_sender(input, config, nonce).unwrap_or_else(|err| fail(err))
}

/// `SurplusTaken` at the accounting: books the burn; a take that emptied the lot ends the auction.
pub fn on_taken(
    input: &ReceiveInput,
    (config, nonce): (AccountId, u64),
    protocol_burned: u128,
    ended: bool,
    now_ms: u64,
) -> Response {
    let acc = accounting_from(input, config, nonce);
    let total_protocol_burned = acc
        .total_protocol_burned
        .checked_add(protocol_burned)
        .unwrap_or_else(|| fail(EngineError::Math));
    let booked = AccountingState {
        total_protocol_burned,
        ..acc
    };
    let next = if ended {
        AccountingState {
            surplus_live: false,
            last_surplus_end_ms: now_ms,
            ..booked
        }
    } else {
        booked
    };
    Response::set_state(next.encode())
}

/// `SurplusReturned` at the accounting: the auction ended, `amount` back in the buffer.
pub fn on_returned(
    input: &ReceiveInput,
    (config, nonce): (AccountId, u64),
    amount: u128,
    now_ms: u64,
) -> Response {
    let acc = accounting_from(input, config, nonce);
    let total_surplus_returned = acc
        .total_surplus_returned
        .checked_add(amount)
        .unwrap_or_else(|| fail(EngineError::Math));
    let next = AccountingState {
        surplus_live: false,
        last_surplus_end_ms: now_ms,
        total_surplus_returned,
        ..acc
    };
    Response::set_state(next.encode())
}
