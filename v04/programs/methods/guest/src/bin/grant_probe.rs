//! Test probe for inherited authorization (design note §4.3 c, guard 5). One program, two roles:
//!
//! - **Host stand-in** (its state is a mock host `Params`, written by a root message): answers
//!   `HostMsg::ReadState` with its state like the mock host, and on any other message from an
//!   actor sends an empty ping to `params.admin`.
//! - **Ping target** (no state, typically an actor at an engine PDA's account): panics
//!   `probe: authorized` if `is_authorized` holds, so a grant inherited down the engine's call
//!   tree makes the transaction fail with that text.

use borsh::BorshDeserialize;
use lee_core::program::{ReceiveInput, Response, StateReply, SyncMessage};
use mock_host_core::{HostMsg, Params};

lee_core::actor_entrypoint!(raw handle_message);

fn handle_message(input: &ReceiveInput) -> Response {
    let Some(from) = input.from else {
        // Root: the message bytes become the state.
        return Response::set_state(input.message.clone());
    };
    let Some(params) = Params::decode(input.pre_state.as_ref()) else {
        assert!(!input.is_authorized, "probe: authorized");
        return Response::keep_state();
    };
    if matches!(
        HostMsg::try_from_slice(&input.message),
        Ok(HostMsg::ReadState)
    ) {
        return Response::keep_state().send_sync(from, &StateReply::from(input));
    }
    Response::keep_state().send(SyncMessage {
        to: params.admin,
        message: Vec::new(),
        pda_seeds: Default::default(),
    })
}
