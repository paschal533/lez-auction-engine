//! The minimal oracle program (design note §6): a price feed with one publisher actor.
//!
//! `Publish` is accepted only from exactly the stored publisher, compared as a whole actor
//! (account and program), never by program alone. Errors are panics `oracle: <reason>`.

use lee_core::program::{ReceiveInput, Response, StateReply};
use oracle_core::{OracleMsg, OracleState};

lee_core::actor_entrypoint!(handle_message);

fn encode(state: &OracleState) -> Vec<u8> {
    borsh::to_vec(state).expect("state encodes")
}

fn load_state(input: &ReceiveInput) -> OracleState {
    OracleState::decode(input.pre_state.as_ref()).expect("oracle: NotAnOracle")
}

fn handle_message(input: &ReceiveInput, message: OracleMsg) -> Response {
    match message {
        OracleMsg::Init { publisher } => {
            assert!(input.is_authorized, "oracle: Unauthorized");
            assert!(
                input.pre_state.as_ref().is_empty(),
                "oracle: AlreadyInitialized"
            );
            Response::set_state(encode(&OracleState::new(publisher)))
        }
        OracleMsg::Publish {
            price_ray,
            timestamp_ms,
            confidence,
        } => {
            let mut state = load_state(input);
            assert_eq!(input.from, Some(state.publisher), "oracle: NotPublisher");
            state.price_ray = price_ray;
            state.timestamp_ms = timestamp_ms;
            state.confidence = confidence;
            Response::set_state(encode(&state))
        }
        OracleMsg::ReadState => {
            let reader = input.from.expect("oracle: Unroutable");
            Response::keep_state().send_sync(reader, &StateReply::from(input))
        }
    }
}
