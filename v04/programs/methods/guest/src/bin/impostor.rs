//! Test impostor (design note §4.3 c): an attacker's program that speaks as whichever actor it
//! sits at, typically one at an engine account (`(config, Impostor)`) or at a host position
//! (`(position, Impostor)`) when the engine is bound to it as its host. Roles:
//!
//! - Root `Forward { to, message }`: sends `message` (any bytes, usually an `EngineMsg`) to `to`
//!   as this actor. Root `SetState(bytes)`: stores `bytes` as this actor's state.
//! - From an actor, with a mock host `Params` state (a host stand-in): answers
//!   `HostMsg::ReadState` with its state like the mock host. Any other message is accepted; it is
//!   passed on as an empty ping to `params.admin`, unless `params.admin` is this actor itself, in
//!   which case nothing is sent (a host that never replies).
//! - From an actor, with a script state (`SCRIPT_TAG` + borsh `Vec<(Actor, Vec<u8>)>`): sends
//!   every scripted message, as this actor, whatever arrived.
//! - Anything else: accepted, nothing sent.
//!
//! It differs from `relay` in that it can hold the host role and replay a script, which is what a
//! bound impostor host needs to report `Seized` from `(position, Impostor)`.

use borsh::{BorshDeserialize, from_slice};
use lee_core::{
    account::Actor,
    program::{ReceiveInput, Response, StateReply, SyncMessage},
};
use mock_host_core::{HostMsg, Params};

lee_core::actor_entrypoint!(raw handle_message);

/// The leading bytes of a script state.
const SCRIPT_TAG: [u8; 8] = *b"IMPSCRPT";

#[derive(BorshDeserialize)]
enum ImpostorMsg {
    Forward { to: Actor, message: Vec<u8> },
    SetState(Vec<u8>),
}

fn send(response: Response, to: Actor, message: Vec<u8>) -> Response {
    response.send(SyncMessage {
        to,
        message,
        pda_seeds: Default::default(),
    })
}

fn handle_message(input: &ReceiveInput) -> Response {
    let Some(from) = input.from else {
        return match ImpostorMsg::try_from_slice(&input.message).expect("impostor: BadRoot") {
            ImpostorMsg::Forward { to, message } => send(Response::keep_state(), to, message),
            ImpostorMsg::SetState(bytes) => Response::set_state(bytes),
        };
    };
    let state = input.pre_state.as_ref();
    if let Some(params) = Params::decode(state) {
        if matches!(
            HostMsg::try_from_slice(&input.message),
            Ok(HostMsg::ReadState)
        ) {
            return Response::keep_state().send_sync(from, &StateReply::from(input));
        }
        if params.admin == input.receiver {
            return Response::keep_state();
        }
        return send(Response::keep_state(), params.admin, Vec::new());
    }
    if let Some(script) = state.strip_prefix(&SCRIPT_TAG) {
        let forwards: Vec<(Actor, Vec<u8>)> = from_slice(script).expect("impostor: BadScript");
        return forwards
            .into_iter()
            .fold(Response::keep_state(), |response, (to, message)| {
                send(response, to, message)
            });
    }
    Response::keep_state()
}
