//! Debt auction (design note §4.2 rows 13-15; v0.2.4 `debt.rs`): instant Dutch, the taker's
//! stablecoin is burned and reduces `unbacked_debt`, protocol tokens are minted through the
//! config `K`, the protocol definition's minter (ADR-056). `debt_on_auction` reserves the lot so
//! `settle_debt` cannot cover the same debt twice (ADR-024 pairing: start ↔ `record_debt_start`,
//! take ↔ `record_debt_take(filled)` and a burn of exactly `filled`, close ↔ `record_debt_close`).
//!
//! - **Start** (root `StartDebt` at `K`, a system op): the accounting pre-checks the nonce and the
//!   relist delay at the claimed time (ADR-039) before its reads; at `Proceed`, [`start`] runs
//!   `start_debt_op` and sends the buffer's `Burn` of the implicit settle (if any, `BUFFER` seed),
//!   then the seeded leaf `DebtOpen` that creates `D = debt_actor(E, K, nonce)`.
//! - **Take** (root `TakeDebt` at `K`): `K` refuses a config without protocol token, asserts the
//!   clock, sends a seeded `Probe { Debt }` and `DebtTake`. `D` runs `take_debt` and sends the
//!   taker's stablecoin `Burn` (the taker signs), `DebtTaken` to the accounting and `Mint` to `K`,
//!   which accepts it only from `debt_actor(E, K, nonce)` and sends `MintTo` to its definition.
//! - **Close** (root `CloseDebt` at `K`): clock, probe, `DebtClose`; `D` runs `close_debt` and
//!   reports `DebtClosed` with the released lot.
//!
//! The accounting accepts `DebtTaken` / `DebtClosed` only from the auction its own config derives
//! for the reported nonce.

use auction_engine_params::EngineError;
use engine_core::{
    SystemCtx, SystemOp, SystemPending,
    actors::{accounting_actor, buffer_holding, buffer_seed, debt_actor, debt_seed, protocol_def},
    assertions::mint_sender,
    check::{expect_from, expect_root, fail},
    msg::EngineMsg,
    state::{AccountingState, ConfigState},
    system::{
        book_debt_close, book_debt_take, close_debt_op, ref_price, start_debt_op, take_debt_op,
    },
    system_state::{DebtAuctionState, SystemPhase, TAG_DEBT},
};
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, SyncMessage},
};
use token_core::TokenMsg;

use crate::{admin::engine, proxy::debt_probe, system};

/// Variants 34-42 (the caller matched only these).
pub fn dispatch(input: &ReceiveInput, message: EngineMsg) -> Response {
    match message {
        EngineMsg::StartDebt {
            config,
            nonce,
            now_ms,
        } => on_start(input, config, nonce, now_ms),
        EngineMsg::TakeDebt {
            taker,
            config,
            nonce,
            amount_coin,
            min_protocol_out,
            now_ms,
        } => on_take_root(
            input,
            (config, nonce),
            &TakeArgs {
                taker,
                amount_coin,
                min_protocol_out,
                now_ms,
            },
        ),
        EngineMsg::CloseDebt {
            config,
            nonce,
            now_ms,
        } => on_close_root(input, (config, nonce), now_ms),
        EngineMsg::DebtOpen {
            config,
            nonce,
            lot,
            start_amount,
            start_ms,
        } => on_open(input, (config, nonce), lot, start_amount, start_ms),
        message => dispatch_internal(input, message),
    }
}

/// Variants 38-42.
fn dispatch_internal(input: &ReceiveInput, message: EngineMsg) -> Response {
    match message {
        EngineMsg::DebtTake {
            taker,
            amount_coin,
            min_protocol_out,
            now_ms,
            ctx,
        } => on_take(
            input,
            &TakeArgs {
                taker,
                amount_coin,
                min_protocol_out,
                now_ms,
            },
            &ctx,
        ),
        EngineMsg::DebtClose { now_ms, ctx } => on_close(input, now_ms, &ctx),
        EngineMsg::DebtTaken {
            config,
            nonce,
            filled,
            minted,
            ended,
            now_ms,
        } => on_taken(input, (config, nonce), (filled, minted), ended, now_ms),
        EngineMsg::DebtClosed {
            config,
            nonce,
            lot_left,
            now_ms,
        } => on_closed(input, (config, nonce), lot_left, now_ms),
        EngineMsg::Mint { nonce, to, amount } => on_mint(input, nonce, to, amount),
        _ => panic!("engine: BadMessage"),
    }
}

/// Root `StartDebt` at the named config (row 13).
fn on_start(input: &ReceiveInput, config: AccountId, nonce: u64, now_ms: u64) -> Response {
    expect_root(input);
    system::root(input, config, SystemOp::StartDebt { nonce }, now_ms)
}

/// At the accounting's `Proceed` for `StartDebt { nonce }`, with every reading in: books the
/// start (implicit settle, reservation), burns the settled amount and opens the auction.
pub fn start(
    input: &ReceiveInput,
    acc: &AccountingState,
    pending: &SystemPending,
    (buffer, nonce): (u128, u64),
) -> Response {
    let (ctx, now_ms) = (&pending.ctx, pending.now_ms);
    let reference = ref_price(ctx, pending.reference, now_ms);
    let (next, d, burned) =
        start_debt_op(acc, ctx, buffer, reference, now_ms, nonce).unwrap_or_else(|err| fail(err));
    let e = engine(input);
    let mut response = Response::set_state(next.encode());
    if burned > 0 {
        let burn = TokenMsg::Burn {
            definition: ctx.stable_def,
            amount: burned,
        };
        let holder = buffer_holding(e, ctx.token, ctx.config);
        response = response
            .send(SyncMessage::new(holder, &burn).with_pda_seeds([buffer_seed(ctx.config)]));
    }
    let open = EngineMsg::DebtOpen {
        config: ctx.config,
        nonce,
        lot: d.lot_left,
        start_amount: d.start_amount,
        start_ms: d.start_ms,
    };
    let debt = debt_actor(e, ctx.config, nonce);
    response.send(SyncMessage::new(debt, &open).with_pda_seeds([debt_seed(ctx.config, nonce)]))
}

/// `DebtOpen` at `debt_actor(E, config, nonce)`, from exactly `accounting_actor(E, config)` with
/// the DEBT seed, on an empty state: creates the live auction. Sends nothing.
fn on_open(
    input: &ReceiveInput,
    (config, nonce): (AccountId, u64),
    lot: u128,
    start_amount: u128,
    start_ms: u64,
) -> Response {
    let e = engine(input);
    let err = EngineError::WrongAccount("debt_auction");
    expect_from(input, accounting_actor(e, config), err);
    if input.receiver != debt_actor(e, config, nonce)
        || !input.pre_state.as_ref().is_empty()
        || !input.is_authorized
    {
        fail(err);
    }
    let state = DebtAuctionState {
        tag: TAG_DEBT,
        config,
        nonce,
        lot_left: lot,
        start_amount,
        start_ms,
        phase: SystemPhase::Live,
    };
    Response::set_state(state.encode())
}

/// Root `TakeDebt` / `CloseDebt` at `K`: the named config, the clock claim and the auction probe;
/// the caller sends its message unseeded as the probe's next sibling.
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
    let (debt, probe) = debt_probe(engine(input), config, nonce);
    let ctx = Box::new(SystemCtx::from_config(config, &c));
    (response.send(probe), debt, ctx)
}

/// `TakeDebt` without its routing fields.
struct TakeArgs {
    taker: AccountId,
    amount_coin: u128,
    min_protocol_out: u128,
    now_ms: u64,
}

/// Root `TakeDebt` at the named config (row 14).
fn on_take_root(input: &ReceiveInput, at: (AccountId, u64), t: &TakeArgs) -> Response {
    let (response, debt, ctx) = root_at_config(input, at, t.now_ms, true);
    let take = EngineMsg::DebtTake {
        taker: t.taker,
        amount_coin: t.amount_coin,
        min_protocol_out: t.min_protocol_out,
        now_ms: t.now_ms,
        ctx,
    };
    response.send_sync(debt, &take)
}

/// Root `CloseDebt` at the named config (row 15).
fn on_close_root(input: &ReceiveInput, at: (AccountId, u64), now_ms: u64) -> Response {
    let (response, debt, ctx) = root_at_config(input, at, now_ms, false);
    response.send_sync(debt, &EngineMsg::DebtClose { now_ms, ctx })
}

/// The receiving auction, which must be the one its config `ctx.config` derives, and the message
/// must come from exactly that config `(config, E)`.
fn load_from_config(input: &ReceiveInput, ctx: &SystemCtx) -> DebtAuctionState {
    let err = EngineError::WrongAccount("debt_auction");
    let d = DebtAuctionState::decode(input.pre_state.as_ref()).unwrap_or_else(|| fail(err));
    let e = engine(input);
    expect_from(
        input,
        Actor::new(d.config, e),
        EngineError::WrongAccount("config"),
    );
    if ctx.config != d.config || input.receiver != debt_actor(e, d.config, d.nonce) {
        fail(err);
    }
    d
}

/// `DebtTake` at the auction: v0.2.4's `take_debt`, then the taker's burn of exactly the filled
/// amount, the report and the mint request. Token refusals (an unsigned taker, too little
/// stablecoin) are the token's.
fn on_take(input: &ReceiveInput, t: &TakeArgs, ctx: &SystemCtx) -> Response {
    let d = load_from_config(input, ctx);
    if ctx.protocol_token.is_none() {
        fail(EngineError::TokenNotConfigured);
    }
    let (next, minted, filled) =
        take_debt_op(&d, ctx, (t.amount_coin, t.min_protocol_out), t.now_ms)
            .unwrap_or_else(|err| fail(err));
    let e = engine(input);
    let burn = TokenMsg::Burn {
        definition: ctx.stable_def,
        amount: filled,
    };
    let taken = EngineMsg::DebtTaken {
        config: d.config,
        nonce: d.nonce,
        filled,
        minted,
        ended: next.phase == SystemPhase::Ended,
        now_ms: t.now_ms,
    };
    let mint = EngineMsg::Mint {
        nonce: d.nonce,
        to: t.taker,
        amount: minted,
    };
    Response::set_state(next.encode())
        .send_sync(Actor::new(t.taker, ctx.token), &burn)
        .send_sync(accounting_actor(e, d.config), &taken)
        .send_sync(Actor::new(d.config, e), &mint)
}

/// `DebtClose` at the auction: v0.2.4's `close_debt` (`NotLive`, `DelayNotElapsed`), then the
/// report of the released lot.
fn on_close(input: &ReceiveInput, now_ms: u64, ctx: &SystemCtx) -> Response {
    let d = load_from_config(input, ctx);
    let (next, released) = close_debt_op(&d, ctx, now_ms).unwrap_or_else(|err| fail(err));
    let closed = EngineMsg::DebtClosed {
        config: d.config,
        nonce: d.nonce,
        lot_left: released,
        now_ms,
    };
    Response::set_state(next.encode()).send_sync(accounting_actor(engine(input), d.config), &closed)
}

/// The accounting of `config`, receiving from exactly `debt_actor(E, config, nonce)`.
fn accounting_from(input: &ReceiveInput, config: AccountId, nonce: u64) -> AccountingState {
    let e = engine(input);
    expect_from(
        input,
        debt_actor(e, config, nonce),
        EngineError::WrongAccount("debt_auction"),
    );
    if input.receiver != accounting_actor(e, config) {
        fail(EngineError::WrongAccount("accounting"));
    }
    AccountingState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("accounting")))
}

/// `DebtTaken` at the accounting: `record_debt_take(filled)`, the mint booked; a take that
/// emptied the lot ends the auction.
fn on_taken(
    input: &ReceiveInput,
    (config, nonce): (AccountId, u64),
    amounts: (u128, u128),
    ended: bool,
    now_ms: u64,
) -> Response {
    let acc = accounting_from(input, config, nonce);
    let next = book_debt_take(&acc, amounts, ended, now_ms).unwrap_or_else(|err| fail(err));
    Response::set_state(next.encode())
}

/// `DebtClosed` at the accounting: the reservation of the untaken lot is released.
fn on_closed(
    input: &ReceiveInput,
    (config, nonce): (AccountId, u64),
    released: u128,
    now_ms: u64,
) -> Response {
    let acc = accounting_from(input, config, nonce);
    let next = book_debt_close(&acc, released, now_ms).unwrap_or_else(|err| fail(err));
    Response::set_state(next.encode())
}

/// `Mint` at the config `K`, only from `debt_actor(E, K, nonce)`: `MintTo` at the protocol
/// definition `(protocol_def(E, K), T)`, which checks the sender is its minter `K`.
fn on_mint(input: &ReceiveInput, nonce: u64, to: AccountId, amount: u128) -> Response {
    mint_sender(input, nonce).unwrap_or_else(|err| fail(err));
    let c = ConfigState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("config")));
    let k = input.receiver;
    let definition = Actor::new(protocol_def(k.program_account_id, k.account_id), c.token);
    Response::keep_state().send_sync(definition, &TokenMsg::MintTo { to, amount })
}
