//! `SetPauseLevel` past the proxy (spec §3.2.1, ADR-073):
//!
//! - Config `K`: accepts only the admin's proxy `(admin, E)` for its own salt, checks
//!   `level <= 3` (`InvalidConfig`) and that `collateral_def` is its recorded type
//!   (`WrongAccount("collateral_def")`, Plan 9 ruling R9-9), writes `ext.system_level`, then
//!   relays `PauseLevelUpdate` behind a seeded `Probe { Collateral }` and `Observe { now_ms }`
//!   unseeded, so the pause clock records the change (as `SetCollateral` does).
//! - Collateral actor: accepts `PauseLevelUpdate` only from its own config, writes
//!   `ext.pause_level` and sends nothing.

use auction_engine_params::EngineError;
use engine_core::{
    actors::collateral_actor,
    assertions,
    check::fail,
    ext::{CollateralExt, ConfigExt},
    msg::EngineMsg,
    state::{CollateralState, ConfigState},
};
use lee_core::program::{ReceiveInput, Response};
use risk_core::pause::Level;

use crate::admin::{engine, expect_admin_proxy, to_collateral};

/// `level` if it is a pause level (0 to 3), else `InvalidConfig`.
fn checked_level(level: u8) -> u8 {
    Level::from_u8(level)
        .map(Level::as_u8)
        .unwrap_or_else(|| fail(EngineError::InvalidConfig))
}

/// `SetPauseLevel` at the config, forwarded by the admin's proxy.
pub fn on_set_pause_level(input: &ReceiveInput, message: &EngineMsg) -> Response {
    let EngineMsg::SetPauseLevel {
        salt,
        collateral_def,
        level,
        now_ms,
    } = message
    else {
        panic!("engine: BadMessage")
    };
    let admin = expect_admin_proxy(input, *salt);
    let config = ConfigState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("config")));
    if config.admin != admin {
        fail(EngineError::NotAdmin);
    }
    let level = checked_level(*level);
    if config.collateral_def != Some(*collateral_def) {
        fail(EngineError::WrongAccount("collateral_def"));
    }
    let next = ConfigState {
        ext: ConfigExt {
            system_level: level,
        },
        ..config
    };
    let k = input.receiver.account_id;
    let collateral = collateral_actor(engine(input), k, *collateral_def);
    let update = EngineMsg::PauseLevelUpdate { level };
    to_collateral(
        input,
        Response::set_state(next.encode()),
        *collateral_def,
        &update,
    )
    .send_sync(collateral, &EngineMsg::Observe { now_ms: *now_ms })
}

/// `PauseLevelUpdate` at a collateral actor, from exactly its config: writes `ext.pause_level`,
/// sends nothing.
pub fn on_pause_level_update(input: &ReceiveInput, level: u8) -> Response {
    let coll = assertions::pause_level_update(input).unwrap_or_else(|err| fail(err));
    let level = checked_level(level);
    let next = CollateralState {
        ext: CollateralExt {
            pause_level: level,
            ..coll.ext.clone()
        },
        ..coll
    };
    Response::set_state(next.encode())
}
