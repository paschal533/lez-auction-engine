//! Read block and its Sentinel at the config (design note §4.1). `Init` stores the oracle actor
//! in `pending_read`, sends `ReadState`, and ends with `Proceed` to itself. The oracle's
//! `StateReply` is accepted only from exactly the pending actor and must hold an oracle state;
//! `Proceed`, run after every earlier subtree, fails the transaction if the read never completed.

use auction_engine_params::EngineError;
use borsh::BorshDeserialize;
use engine_core::{
    check::{expect_from, fail},
    state::ConfigState,
};
use lee_core::program::{ReceiveInput, Response, StateReply};
use oracle_core::OracleState;

/// The pending read's reply, if `input` is one: at a config awaiting a read, from exactly the
/// actor it asked. Anything else is left to the message dispatch.
pub fn on_reply(input: &ReceiveInput) -> Option<Response> {
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

/// The sentinel: only from the config itself, and only once its read has been answered.
pub fn proceed(input: &ReceiveInput) -> Response {
    expect_from(input, input.receiver, EngineError::WrongAccount("proceed"));
    let config = ConfigState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("config")));
    if config.pending_read.is_some() {
        fail(EngineError::OracleMismatch);
    }
    Response::keep_state()
}
