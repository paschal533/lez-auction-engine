//! Proxy block (design note §3, §4.1): a root admin message is sent to the signer's own engine
//! actor `(signer, E)`, where `is_authorized` holds only if the signer signed (a root message
//! carries no PDA seed). The stateless proxy forwards the same message to the signer's config
//! `config_actor(E, signer, salt)`, which checks it came from exactly `(signer, E)`.

use auction_engine_params::EngineError;
use engine_core::{
    actors::{config_actor, config_seed},
    check::fail,
    msg::EngineMsg,
};
use lee_core::program::{ReceiveInput, Response, SyncMessage};

/// Forwards a root `Init` / `SetCollateral` to the signer's config with the config seed.
///
/// `Init` needs the seed to create the config. `SetCollateral` gets it too: LEZ refuses a message
/// to an account that does not exist yet unless it is authorized (`UnadmittedPublicActor`), so
/// without the seed a `SetCollateral` before `Init` would fail in LEZ instead of with the
/// engine's v0.2.4 `WrongAccount("config")`. The config writes no state on `SetCollateral`.
pub fn forward(input: &ReceiveInput, message: &EngineMsg, salt: u64) -> Response {
    if !input.is_authorized {
        fail(EngineError::NotAdmin);
    }
    let engine = input.receiver.program_account_id;
    let signer = input.receiver.account_id;
    let to = SyncMessage::new(config_actor(engine, signer, salt), message)
        .with_pda_seeds([config_seed(signer, salt)]);
    Response::keep_state().send(to)
}
