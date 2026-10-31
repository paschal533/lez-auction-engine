//! Test relay: a root message `{ to, message }` is forwarded as a sync message from this actor,
//! and any message that arrives from another actor (a `StateReply`, say) is stored verbatim.
//! It lets a test speak as an arbitrary `(account, program)` actor.

use borsh::BorshDeserialize;
use lee_core::{
    account::Actor,
    program::{ReceiveInput, Response, SyncMessage},
};

lee_core::actor_entrypoint!(raw handle_message);

#[derive(BorshDeserialize)]
struct Forward {
    to: Actor,
    message: Vec<u8>,
}

fn handle_message(input: &ReceiveInput) -> Response {
    if input.from.is_some() {
        return Response::set_state(input.message.clone());
    }
    let forward = Forward::try_from_slice(&input.message).expect("forward decodes");
    Response::keep_state().send(SyncMessage {
        to: forward.to,
        message: forward.message,
        pda_seeds: Default::default(),
    })
}
