//! The engine's reference-oracle and relist parameters on the wire and in state (Plan 10 Task 2,
//! rulings R10-1, R10-4, R10-5): `Init` and `SetEngineParams` carry an `EngineExtArgs` last, the
//! config validates it (`InvalidConfig` out of bound) and stores it in `ConfigExt.engine`.
//! Changing `ref_scale` is a locked-field change (`AssertSystemIdle`); the other fields are not.
//! `harness::invariants::check` after every send.

use auction_engine_params::{EngineError, args::EngineParamsArgs};
use engine_core::check::fail_text;
use harness::{World, expect_revert, invariants};
use lee::error::LeeError;
use lee_core::program::TransactionEvent;
use risk_core::EngineExtArgs;

/// The chain time the debt-auction test starts at, ms.
const T0: u64 = 1_000_000;

type Sent = Result<Vec<TransactionEvent>, LeeError>;

/// The legacy engine parameters with the clamp at 1_500 bps.
fn clamp_1500() -> EngineExtArgs {
    EngineExtArgs {
        ref_clamp_bps: 1_500,
        ..EngineExtArgs::legacy()
    }
}

fn checked(w: &World, sent: Sent) -> Sent {
    invariants::check(w);
    sent
}

/// The admin's `Init` of a bare world with `ext`.
fn init_with(w: &mut World, ext: EngineExtArgs) -> Sent {
    let key = w.admin_key.clone();
    let init = w.init_msg_ext(World::engine_params(), ext, None, None);
    let sent = w.send_engine(w.admin, &[&key], &init);
    checked(w, sent)
}

/// The admin's `SetEngineParams` with `params` and `ext`, no reference oracle.
fn set_params(w: &mut World, params: EngineParamsArgs, ext: EngineExtArgs) -> Sent {
    let key = w.admin_key.clone();
    let msg = w.set_engine_params_msg_ext(params, ext, None);
    let sent = w.send_engine(w.admin, &[&key], &msg);
    checked(w, sent)
}

/// A bare world initialised with `ext`, then `SetCollateral` and the host bound, as
/// [`World::new`].
fn configured_with(ext: EngineExtArgs) -> World {
    let mut w = World::bare();
    init_with(&mut w, ext).expect("the admin initialises");
    let (key, set) = (
        w.admin_key.clone(),
        w.set_collateral_msg(World::collateral_args()),
    );
    w.send_engine(w.admin, &[&key], &set)
        .expect("the collateral is configured");
    w.bind_host().expect("the host binds the config actor");
    invariants::check(&w);
    w
}

#[test]
fn init_writes_the_engine_ext() {
    let mut w = World::bare();
    init_with(&mut w, clamp_1500()).expect("the admin initialises");
    let ext = w.config_state().ext;
    assert_eq!(ext.engine.ref_clamp_bps, 1_500);
    assert_eq!((ext.engine, ext.system_level), (clamp_1500(), 0));
}

#[test]
fn init_rejects_an_out_of_bound_engine_ext() {
    let mut w = World::bare();
    let bad = EngineExtArgs {
        debt_relist_delay_ms: 59_999,
        ..EngineExtArgs::legacy()
    };
    expect_revert(
        init_with(&mut w, bad),
        &fail_text(EngineError::InvalidConfig),
    );
    assert_eq!(w.chain.raw_state(w.config), None, "no config is created");
}

#[test]
fn set_engine_params_replaces_the_engine_ext() {
    let mut w = World::new();
    w.set_pause_level(1).expect("level 1 applies");
    invariants::check(&w);
    let next = EngineExtArgs {
        ref_conf_bps: 500,
        ref_clamp_bps: 1_500,
        debt_relist_delay_ms: 120_000,
        ref_sanity_lo_bps: 8_000,
        ref_sanity_hi_bps: 12_000,
        ..EngineExtArgs::legacy()
    };
    set_params(&mut w, World::engine_params(), next).expect("the admin sets the ext");
    let ext = w.config_state().ext;
    assert_eq!(ext.engine, next);
    assert_eq!(ext.system_level, 1, "the system level is kept");
}

#[test]
fn set_engine_params_rejects_an_out_of_bound_engine_ext() {
    let mut w = World::new();
    let before = w.chain.raw_state(w.config);
    let bad = EngineExtArgs {
        ref_scale: 2,
        ..EngineExtArgs::legacy()
    };
    expect_revert(
        set_params(&mut w, World::engine_params(), bad),
        &fail_text(EngineError::InvalidConfig),
    );
    assert_eq!(
        w.chain.raw_state(w.config),
        before,
        "the config is unchanged"
    );
}

/// Review Focus 4: the same reference-oracle bytes would be read at another scale mid-auction,
/// so a `ref_scale` change waits for an idle system, like replacing the oracle (the refusal is
/// `AssertSystemIdle`'s, as in `debt.rs`'s `protocol_token_locked_while_debt_auction_live`). The
/// clamp is not locked.
#[test]
fn changing_ref_scale_needs_an_idle_system() {
    let params = EngineParamsArgs {
        protocol_token_enabled: true,
        ..World::engine_params()
    };
    let mut w = World::with_system(params.clone());
    w.set_now_ms(T0);
    w.book_shortfall();
    invariants::check(&w);
    let sent = w.start_debt(0);
    checked(&w, sent).expect("the debt auction starts");
    let before = w.chain.raw_state(w.config);
    let scaled = EngineExtArgs {
        ref_scale: 1,
        ..EngineExtArgs::legacy()
    };
    expect_revert(
        set_params(&mut w, params.clone(), scaled),
        &fail_text(EngineError::ProtocolTokenLocked),
    );
    assert_eq!(
        w.chain.raw_state(w.config),
        before,
        "the config is unchanged"
    );
    set_params(&mut w, params, clamp_1500()).expect("the clamp is not locked");
    assert_eq!(w.config_state().ext.engine, clamp_1500());
}

/// Review Focus 5: `SetPauseLevel` changes the level and keeps the engine parameters.
#[test]
fn set_pause_level_keeps_the_engine_ext() {
    let mut w = configured_with(clamp_1500());
    w.set_pause_level(2).expect("level 2 applies");
    invariants::check(&w);
    let ext = w.config_state().ext;
    assert_eq!(ext.engine.ref_clamp_bps, 1_500);
    assert_eq!(ext.system_level, 2);
    assert_eq!(ext.engine, clamp_1500());
}
