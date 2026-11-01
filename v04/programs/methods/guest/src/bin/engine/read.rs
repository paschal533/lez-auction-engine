//! Read block and its Sentinel (design note §4.1), and the routing of replies.
//!
//! - Config: `Init` stores the oracle actor in `pending_read`, sends `ReadState`, and ends with
//!   `Proceed` to itself. The oracle's `StateReply` is accepted only from exactly the pending actor
//!   and must hold an oracle state; `Proceed`, run after every earlier subtree, fails the
//!   transaction if the read never completed.
//! - Collateral (`observe`) and auction (`auction`) keep their own pending reads; a reply reaches
//!   them by the receiver's state tag. A message that is not an awaited reply is left to the
//!   message dispatch.

use auction_engine_params::EngineError;
use borsh::BorshDeserialize;
use engine_core::{
    check::{expect_from, fail},
    state::{CollateralState, ConfigState, TAG_AUCTION, TAG_COLL, TAG_CONFIG},
};
use lee_core::program::{ReceiveInput, Response, StateReply};
use oracle_core::OracleState;

use crate::{auction, observe};

/// The awaited reply's handling, if `input` is one, chosen by the receiver's state tag.
pub fn route_reply(input: &ReceiveInput) -> Option<Response> {
    input.from?;
    let tag = input.pre_state.as_ref().get(..8)?;
    if tag == TAG_CONFIG {
        on_config_reply(input)
    } else if tag == TAG_COLL {
        observe::on_reply(input)
    } else if tag == TAG_AUCTION {
        auction::on_vault_reply(input)
    } else {
        None
    }
}

/// At a config awaiting a read, from exactly the actor it asked.
fn on_config_reply(input: &ReceiveInput) -> Option<Response> {
    let config = ConfigState::decode(input.pre_state.as_ref())?;
    let pending = config.pending_read?;
    if input.from != Some(pending) {
        return None;
    }
    let reply = StateReply::try_from_slice(&input.message)
        .unwrap_or_else(|_| fail(EngineError::OracleMismatch));
    if OracleState::decode(reply.state.as_ref()).is_none() {
        fail(EngineError::OracleMismatch);
    }
    let read = ConfigState {
        pending_read: None,
        ..config
    };
    Some(Response::set_state(read.encode()))
}

/// The `Proceed` sentinel, only from the receiver itself: at a collateral actor it ends an
/// observation; at the config, only once its read has been answered.
pub fn proceed(input: &ReceiveInput) -> Response {
    expect_from(input, input.receiver, EngineError::WrongAccount("proceed"));
    if CollateralState::decode(input.pre_state.as_ref()).is_some() {
        return observe::proceed(input);
    }
    let config = ConfigState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("config")));
    if config.pending_read.is_some() {
        fail(EngineError::OracleMismatch);
    }
    Response::keep_state()
}
