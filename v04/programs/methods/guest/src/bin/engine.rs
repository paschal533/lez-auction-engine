//! The auction engine on LEZ 0.4 (design note §3, §4): one program, many actors. Which actor a
//! message is for follows from the message, its sender and the receiver's address, never from a
//! caller-supplied account list.
//!
//! - `proxy`: root `Init` / `SetCollateral` at the signer's own engine actor `(signer, E)`,
//!   forwarded to the signer's config only if the signer signed (Proxy block).
//! - `admin`: the config, accounting and collateral handlers of `Init` / `SetCollateral`, each
//!   accepting only the one actor that may send it (Relay block, whole-actor checks).
//! - `read`: the config's oracle read at `Init` (Read block) and its `Proceed` sentinel.
//!
//! Errors are panics with the v0.2.4 text (`engine_core::check::fail`).

// A binary's root resolves modules next to itself, so point them into `engine/`.
#[path = "engine/admin.rs"]
mod admin;
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
    if let Some(response) = read::on_reply(input) {
        return response;
    }
    let message = EngineMsg::try_from_slice(&input.message).expect("engine: BadMessage");
    match &message {
        EngineMsg::Init { salt, .. } | EngineMsg::SetCollateral { salt, .. }
            if input.from.is_none() =>
        {
            proxy::forward(input, &message, *salt)
        }
        EngineMsg::Init { .. } => admin::on_init(input, &message),
        EngineMsg::SetCollateral {
            salt,
            collateral_def,
            args,
        } => admin::on_set_collateral(input, &message, *salt, *collateral_def, args),
        EngineMsg::Proceed => read::proceed(input),
        EngineMsg::Liquidate { .. }
        | EngineMsg::Bid { .. }
        | EngineMsg::ClaimLeftover { .. }
        | EngineMsg::Seized { .. }
        | EngineMsg::LiqCtx { .. }
        | EngineMsg::Bump { .. }
        | EngineMsg::Open { .. }
        | EngineMsg::Reserve { .. }
        | EngineMsg::Release { .. }
        | EngineMsg::Settled { .. }
        | EngineMsg::Finish
        | EngineMsg::BidCtx { .. } => panic!("engine: NotYetSupported"),
    }
}
