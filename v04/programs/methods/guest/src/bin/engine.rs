//! The auction engine on LEZ 0.4 (design note §3, §4): one program, many actors. Which actor a
//! message is for follows from the message, its sender and the receiver's address, never from a
//! caller-supplied account list.
//!
//! - `proxy`: root `Init` / `SetEngineParams` / `SetCollateral` / `Liquidate` / `ClaimLeftover` at
//!   the signer's own engine actor `(signer, E)`, forwarded only if the signer signed (Proxy
//!   block).
//! - `admin`: the config, accounting and collateral handlers of `Init` / `SetEngineParams` /
//!   `SetCollateral` (with the protocol definition, `AssertCapAtMost` and `AssertSystemIdle`),
//!   each accepting only the one actor that may send it (Relay block, whole-actor checks).
//! - `read`: the config's oracle read at `Init` (Read block), the `Proceed` sentinels, and the
//!   routing of every `StateReply` to the actor kind awaiting it.
//! - `observe`: the collateral actor's Observe block (host and oracle reads, clock assert,
//!   `Proceed`), run by `SetCollateral`, root `ObserveFeed`, `Liquidate` and `Bid`.
//! - `liquidate`: the config's liquidation context, the record's `Bump`, and the reservations.
//! - `auction`: the auction's `Open`, `Seized` (the quote), vault check and `Finish` sentinel.
//! - `bid`: root `Bid` at the collateral actor (Observe), the auction's `BidCtx` (quote, token
//!   moves, chain end) and the accounting's `Settled`.
//! - `claim`: the auction's `ClaimLeftover`, forwarded by the owner's proxy.
//! - `settle`: the auction's `SettleEmpty`, forwarded by its config (permissionless root).
//! - `system`: roots `SettleDebt` / `CheckSolvency` at the config, the accounting's system Read
//!   block (buffer and reference-oracle reads, `Proceed`) and the config's `Freeze`.
//! - `surplus`: roots `StartSurplus` / `TakeSurplus` / `CloseSurplus` at the config, the surplus
//!   auction's `SurplusOpen` / `SurplusTake` / `SurplusClose` (vault read, `Proceed`) and the
//!   accounting's `SurplusTaken` / `SurplusReturned`.
//! - `debt`: roots `StartDebt` / `TakeDebt` / `CloseDebt` at the config, the debt auction's
//!   `DebtOpen` / `DebtTake` / `DebtClose`, the accounting's `DebtTaken` / `DebtClosed` and the
//!   config's `Mint` (variants 34-42, one [`debt::dispatch`] call).
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
#[path = "engine/debt.rs"]
mod debt;
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
#[path = "engine/surplus.rs"]
mod surplus;
#[path = "engine/system.rs"]
mod system;

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
            ref risk,
        } => admin::on_set_collateral(input, salt, collateral_def, args, now_ms, risk),
        EngineMsg::Configure {
            collateral_def,
            ref args,
            ctx,
            ref risk,
        } => admin::on_configure(input, collateral_def, args, ctx, risk),
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
            paused_since_ms: _,
            fallback: _,
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
        EngineMsg::SetEngineParams { .. } => admin::on_set_engine_params(input, &message),
        EngineMsg::SetSlack { .. } => admin::on_set_slack(input, &message),
        EngineMsg::SlackUpdate { slack_ms } => admin::on_slack_update(input, slack_ms),
        EngineMsg::ObserveFeed {
            config,
            collateral_def,
            now_ms,
        } => observe::on_observe_feed(input, config, collateral_def, now_ms),
        EngineMsg::AssertCapAtMost { global_cap } => {
            admin::on_assert_cap_at_most(input, global_cap)
        }
        EngineMsg::AssertSystemIdle => admin::on_assert_system_idle(input),
        EngineMsg::SettleDebt { config, amount } => system::on_settle_debt(input, config, amount),
        EngineMsg::System { op, now_ms, ctx } => system::on_system(input, op, now_ms, *ctx),
        EngineMsg::StartSurplus {
            config,
            nonce,
            now_ms,
        } => surplus::on_start(input, config, nonce, now_ms),
        EngineMsg::TakeSurplus {
            taker,
            config,
            nonce,
            amount_coin,
            max_protocol_in,
            now_ms,
        } => surplus::on_take_root(
            input,
            (config, nonce),
            &surplus::TakeArgs {
                taker,
                amount_coin,
                max_protocol_in,
                now_ms,
            },
        ),
        EngineMsg::CloseSurplus {
            config,
            nonce,
            now_ms,
        } => surplus::on_close_root(input, (config, nonce), now_ms),
        EngineMsg::SurplusOpen {
            config,
            nonce,
            lot,
            start_ms,
            ref_ray,
        } => surplus::on_open(input, (config, nonce), lot, start_ms, ref_ray),
        EngineMsg::SurplusTake {
            taker,
            amount_coin,
            max_protocol_in,
            now_ms,
            ctx,
        } => surplus::on_take(
            input,
            &surplus::TakeArgs {
                taker,
                amount_coin,
                max_protocol_in,
                now_ms,
            },
            &ctx,
        ),
        EngineMsg::SurplusClose { now_ms, ctx } => surplus::on_close(input, now_ms, &ctx),
        EngineMsg::SurplusTaken {
            config,
            nonce,
            protocol_burned,
            ended,
            now_ms,
        } => surplus::on_taken(input, (config, nonce), protocol_burned, ended, now_ms),
        EngineMsg::SurplusReturned {
            config,
            nonce,
            amount,
            now_ms,
        } => surplus::on_returned(input, (config, nonce), amount, now_ms),
        message @ (EngineMsg::StartDebt { .. }
        | EngineMsg::TakeDebt { .. }
        | EngineMsg::CloseDebt { .. }
        | EngineMsg::DebtOpen { .. }
        | EngineMsg::DebtTake { .. }
        | EngineMsg::DebtClose { .. }
        | EngineMsg::DebtTaken { .. }
        | EngineMsg::DebtClosed { .. }
        | EngineMsg::Mint { .. }) => debt::dispatch(input, message),
        EngineMsg::CheckSolvency { config, now_ms } => {
            system::on_check_solvency(input, config, now_ms)
        }
        EngineMsg::Freeze => system::on_freeze(input),
    }
}
