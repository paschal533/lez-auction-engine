//! The auction engine on LEZ 0.4 (design note §3, §4): one program, many actors. Which actor a
//! message is for follows from the message, its sender and the receiver's address, never from a
//! caller-supplied account list.
//!
//! - `proxy`: root `Init` / `SetCollateral` / `Liquidate` at the signer's own engine actor
//!   `(signer, E)`, forwarded only if the signer signed (Proxy block).
//! - `admin`: the config, accounting and collateral handlers of `Init` / `SetCollateral`, each
//!   accepting only the one actor that may send it (Relay block, whole-actor checks).
//! - `read`: the config's oracle read at `Init` (Read block), the `Proceed` sentinels, and the
//!   routing of every `StateReply` to the actor kind awaiting it.
//! - `observe`: the collateral actor's Observe block (host and oracle reads, clock assert,
//!   `Proceed`), run by `SetCollateral` and `Liquidate`.
//! - `liquidate`: the config's liquidation context, the record's `Bump`, and the reservations.
//! - `auction`: the auction's `Open`, `Seized` (the quote), vault check and `Finish` sentinel.
//!
//! Errors are panics with the v0.2.4 text (`engine_core::check::fail`).

// A binary's root resolves modules next to itself, so point them into `engine/`.
#[path = "engine/admin.rs"]
mod admin;
#[path = "engine/auction.rs"]
mod auction;
#[path = "engine/liquidate.rs"]
mod liquidate;
#[path = "engine/observe.rs"]
mod observe;
#[path = "engine/proxy.rs"]
mod proxy;
#[path = "engine/read.rs"]
mod read;

use borsh::BorshDeserialize;
use engine_core::msg::EngineMsg;
use lee_core::program::{ReceiveInput, Response};

lee_core::actor_entrypoint!(raw handle_message);

fn handle_message(input: &ReceiveInput) -> Response {
    // A `StateReply` is not an `EngineMsg`: route it before decoding.
    if let Some(response) = read::route_reply(input) {
        return response;
    }
    let message = EngineMsg::try_from_slice(&input.message).expect("engine: BadMessage");
    if input.from.is_none()
        && let Some(response) = proxy::root(input, &message)
    {
        return response;
    }
    match message {
        EngineMsg::Init { .. } => admin::on_init(input, &message),
        EngineMsg::SetCollateral {
            salt,
            collateral_def,
            ref args,
            now_ms,
        } => admin::on_set_collateral(input, salt, collateral_def, args, now_ms),
        EngineMsg::Configure {
            collateral_def,
            ref args,
            ctx,
        } => admin::on_configure(input, collateral_def, args, ctx),
        EngineMsg::InitReads => admin::on_init_reads(input),
        EngineMsg::Observe { now_ms } => observe::on_observe(input, now_ms),
        EngineMsg::Probe { kind } => proxy::probe(input, kind),
        EngineMsg::Liquidate {
            config,
            collateral_def,
            position,
            nonce,
            now_ms,
        } => observe::on_liquidate(
            input,
            &observe::LiquidateRequest {
                config,
                collateral_def,
                position,
                nonce,
                now_ms,
            },
        ),
        EngineMsg::Proceed => read::proceed(input),
        EngineMsg::LiqCtx {
            position,
            nonce,
            keeper,
            now_ms,
            price_ray,
            collateral,
        } => liquidate::on_liq_ctx(
            input,
            &liquidate::LiqCtx {
                position,
                nonce,
                keeper,
                now_ms,
                price_ray,
            },
            *collateral,
        ),
        EngineMsg::Bump { position, nonce } => liquidate::on_bump(input, position, nonce),
        EngineMsg::Reserve {
            position,
            nonce,
            amount,
        } => liquidate::on_reserve(input, position, nonce, amount),
        EngineMsg::Open {
            position,
            nonce,
            keeper,
            host,
            token,
            now_ms,
            price_ray,
            collateral,
            global_cap,
            global_reserved,
        } => auction::on_open(
            input,
            &auction::OpenArgs {
                position,
                nonce,
                keeper,
                host,
                token,
                now_ms,
                price_ray,
                global_cap,
                global_reserved,
            },
            *collateral,
        ),
        EngineMsg::Seized {
            position,
            owner,
            collateral,
            nominal_debt,
            ..
        } => auction::on_seized(input, position, owner, collateral, nominal_debt),
        EngineMsg::Finish => auction::on_finish(input),
        EngineMsg::Bid { .. }
        | EngineMsg::ClaimLeftover { .. }
        | EngineMsg::Release { .. }
        | EngineMsg::Settled { .. }
        | EngineMsg::BidCtx { .. } => panic!("engine: NotYetSupported"),
    }
}
