//! The auction engine on LEZ 0.4 (design note §3, §4): one program, many actors. Which actor a
//! message is for follows from the message, its sender and the receiver's address, never from a
//! caller-supplied account list.
//!
//! - `proxy`: root `Init` / `SetCollateral` / `Liquidate` / `ClaimLeftover` at the signer's own engine actor
//!   `(signer, E)`, forwarded only if the signer signed (Proxy block).
//! - `admin`: the config, accounting and collateral handlers of `Init` / `SetCollateral`, each
//!   accepting only the one actor that may send it (Relay block, whole-actor checks).
//! - `read`: the config's oracle read at `Init` (Read block), the `Proceed` sentinels, and the
//!   routing of every `StateReply` to the actor kind awaiting it.
//! - `observe`: the collateral actor's Observe block (host and oracle reads, clock assert,
//!   `Proceed`), run by `SetCollateral` and `Liquidate`.
//! - `liquidate`: the config's liquidation context, the record's `Bump`, and the reservations.
//! - `auction`: the auction's `Open`, `Seized` (the quote), vault check and `Finish` sentinel.
//! - `bid`: root `Bid` at the collateral actor (Observe), the auction's `BidCtx` (quote, token
//!   moves, chain end) and the accounting's `Settled`.
//! - `claim`: the auction's `ClaimLeftover`, forwarded by the owner's proxy.
//! - `settle`: the auction's `SettleEmpty`, forwarded by its config (permissionless root).
//!
//! Errors are panics with the v0.2.4 text (`engine_core::check::fail`).

// A binary's root resolves modules next to itself, so point them into `engine/`.
#[path = "engine/admin.rs"]
mod admin;
#[path = "engine/auction.rs"]
mod auction;
#[path = "engine/bid.rs"]
mod bid;
#[path = "engine/claim.rs"]
mod claim;
#[path = "engine/liquidate.rs"]
mod liquidate;
#[path = "engine/observe.rs"]
mod observe;
#[path = "engine/proxy.rs"]
mod proxy;
#[path = "engine/read.rs"]
mod read;
#[path = "engine/settle.rs"]
mod settle;

use borsh::BorshDeserialize;
use engine_core::{
    bid::{BidQuote, ChainEnd, SettledTotals},
    msg::EngineMsg,
    state::BidRequestRec,
};
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
        } => liquidate::on_reservation(
            input,
            (position, nonce),
            amount,
            liquidate::Reservation::Reserve,
        ),
        EngineMsg::Release {
            position,
            nonce,
            amount,
        } => liquidate::on_reservation(
            input,
            (position, nonce),
            amount,
            liquidate::Reservation::Release,
        ),
        EngineMsg::Open {
            position,
            nonce,
            keeper,
            host,
            token,
            stable_def,
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
                stable_def,
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
        EngineMsg::Bid {
            bidder,
            config,
            collateral_def,
            position,
            nonce,
            max_debt_in,
            min_collateral_out,
            now_ms,
        } => bid::on_bid(
            input,
            &bid::BidRoute {
                config,
                collateral_def,
                now_ms,
            },
            BidRequestRec {
                bidder,
                position,
                nonce,
                max_debt_in,
                min_collateral_out,
            },
        ),
        EngineMsg::BidCtx {
            bidder,
            position,
            nonce,
            max_debt_in,
            min_collateral_out,
            price_ray,
            now_ms,
            paused_total_ms,
            min_bid,
            chost,
        } => bid::on_bid_ctx(
            input,
            &bid::BidCtxArgs {
                bidder,
                position,
                nonce,
                quote: BidQuote {
                    max_debt_in,
                    min_collateral_out,
                    price_ray,
                    now_ms,
                    paused_total_ms,
                    min_bid,
                    chost,
                },
            },
        ),
        EngineMsg::Settled {
            config,
            position,
            nonce,
            debt_raised,
            penalty_raised,
            shortfall,
            release,
            leftover,
        } => bid::on_settled(
            input,
            (config, position, nonce),
            &SettledTotals {
                debt_raised,
                penalty_raised,
                end: ChainEnd {
                    shortfall,
                    release,
                    leftover,
                },
            },
        ),
        EngineMsg::ClaimLeftover {
            config,
            position,
            nonce,
        } => claim::on_claim(input, config, position, nonce),
        EngineMsg::SettleEmpty {
            config,
            position,
            nonce,
        } => settle::on_settle_empty(input, config, position, nonce),
    }
}
