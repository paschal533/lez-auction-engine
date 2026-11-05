//! Read block and its Sentinel (design note §4.1), and the routing of replies.
//!
//! - Config: `Init` stores the oracle actor in `pending_read` and sets `pending_host_read`, sends
//!   a `ReadState` to each, and ends with `Proceed` to itself. The oracle's `StateReply` is
//!   accepted only from exactly the pending actor and must hold an oracle state; the host's, only
//!   from exactly the config's host, must hold `Params` naming the config's stablecoin, token and
//!   oracle (ADR-065). `Proceed`, run after every earlier subtree, fails the transaction if
//!   either read never completed.
//! - Collateral (`observe`), auction (`auction`), accounting (`system`) and surplus auction
//!   (`surplus`, its close's vault read) keep their own pending
//!   reads; a reply reaches them by the receiver's state tag. A message that is not an awaited reply is left to the
//!   message dispatch.

use auction_engine_params::EngineError;
use borsh::BorshDeserialize;
use engine_core::{
    check::{expect_from, fail, host_params_match},
    state::{
        AccountingState, CollateralState, ConfigState, TAG_ACCOUNTING, TAG_AUCTION, TAG_COLL,
        TAG_CONFIG,
    },
    system_state::{SurplusAuctionState, TAG_SURPLUS},
};
use lee_core::program::{ReceiveInput, Response, StateReply};
use mock_host_core::Params;
use oracle_core::OracleState;

use crate::{auction, observe, surplus, system};

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
    } else if tag == TAG_ACCOUNTING {
        system::on_reply(input)
    } else if tag == TAG_SURPLUS {
        surplus::on_reply(input)
    } else {
        None
    }
}

/// At a config awaiting a read, from exactly the actor it asked: the oracle, or the host while
/// `pending_host_read`.
fn on_config_reply(input: &ReceiveInput) -> Option<Response> {
    let config = ConfigState::decode(input.pre_state.as_ref())?;
    if config.pending_read.is_some() && input.from == config.pending_read {
        return Some(on_oracle_reply(input, config));
    }
    if config.pending_host_read && input.from == Some(config.host) {
        return Some(on_host_reply(input, config));
    }
    None
}

/// The oracle's `StateReply`: it must hold an oracle state.
fn on_oracle_reply(input: &ReceiveInput, config: ConfigState) -> Response {
    let reply = StateReply::try_from_slice(&input.message)
        .unwrap_or_else(|_| fail(EngineError::OracleMismatch));
    if OracleState::decode(reply.state.as_ref()).is_none() {
        fail(EngineError::OracleMismatch);
    }
    let read = ConfigState {
        pending_read: None,
        ..config
    };
    Response::set_state(read.encode())
}

/// The host's `StateReply` (ADR-065): it must hold the mock host's `Params`, whose stablecoin,
/// token and oracle are the config's. This reads the mock's layout; re-point it to the upstream
/// host's parameters when that lands.
fn on_host_reply(input: &ReceiveInput, config: ConfigState) -> Response {
    let reply = StateReply::try_from_slice(&input.message)
        .unwrap_or_else(|_| fail(EngineError::HostMismatch));
    let params =
        Params::decode(reply.state.as_ref()).unwrap_or_else(|| fail(EngineError::HostMismatch));
    host_params_match(&config, params.stable_def, params.token, params.oracle)
        .unwrap_or_else(|err| fail(err));
    let read = ConfigState {
        pending_host_read: false,
        ..config
    };
    Response::set_state(read.encode())
}

/// The `Proceed` sentinel, only from the receiver itself: at a collateral actor it ends an
/// observation, at the accounting a system op, at a surplus auction its close; at the config,
/// only once its read has been answered.
pub fn proceed(input: &ReceiveInput) -> Response {
    expect_from(input, input.receiver, EngineError::WrongAccount("proceed"));
    if CollateralState::decode(input.pre_state.as_ref()).is_some() {
        return observe::proceed(input);
    }
    if AccountingState::decode(input.pre_state.as_ref()).is_some() {
        return system::proceed(input);
    }
    if SurplusAuctionState::decode(input.pre_state.as_ref()).is_some() {
        return surplus::proceed(input);
    }
    let config = ConfigState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("config")));
    if config.pending_read.is_some() {
        fail(EngineError::OracleMismatch);
    }
    if config.pending_host_read {
        fail(EngineError::HostMismatch);
    }
    Response::keep_state()
}
