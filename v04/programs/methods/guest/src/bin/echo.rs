//! Echo: appends the message bytes to its own actor state on the receiving account
//! (LEZ v0.4.0-rc1 `examples/program_deployment` `hello_world`). The harness smoke test runs it.

use lee_core::program::{ReceiveInput, Response};

lee_core::actor_entrypoint!(handle_message);

fn handle_message(input: &ReceiveInput, bytes: Vec<u8>) -> Response {
    let mut state = input.pre_state.to_vec();
    state.extend(bytes);
    Response::set_state(state)
}
