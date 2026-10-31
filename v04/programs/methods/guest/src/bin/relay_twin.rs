//! Test relay: a root message `{ to, message }` is forwarded as a sync message from this actor,
//! and any message that arrives from another actor is stored behind a leading byte 2, which also makes
//! this binary (and so its program id) differ from `relay`. It lets a test speak as another actor.

use borsh::BorshDeserialize;
use lee_core::{
    account::Actor,
    program::{ReceiveInput, Response, SyncMessage},
};

lee_core::actor_entrypoint!(raw handle_message);

// Makes this binary, and so its program id, differ from `relay`.
const TWIN: u8 = 2;

#[derive(BorshDeserialize)]
struct Forward {
    to: Actor,
    message: Vec<u8>,
}

fn handle_message(input: &ReceiveInput) -> Response {
    if input.from.is_some() {
        return Response::set_state(
            [TWIN]
                .into_iter()
                .chain(input.message.iter().copied())
                .collect::<Vec<u8>>(),
        );
    }
    let forward = Forward::try_from_slice(&input.message).expect("forward decodes");
    Response::keep_state().send(SyncMessage {
        to: forward.to,
        message: forward.message,
        pda_seeds: Default::default(),
    })
}
