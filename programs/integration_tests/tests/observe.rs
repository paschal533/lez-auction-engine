mod common;

use auction_engine_core::{args::CollateralArgs, state::CollateralConfigState};
use common::*;

fn coll(w: &World) -> CollateralConfigState {
    w.read(coll_config())
}

#[test]
fn set_collateral_config_creates_then_updates() {
    let mut w = World::with_engine();
    let c = coll(&w);
    assert_eq!(
        (c.oracle_id, c.penalty_bps, c.type_reserved, c.admin_paused),
        (oracle(), 1_000, 0, false)
    );
    assert_eq!(c.last_seen_oracle_ts, Some(T0));
    let tighter = CollateralArgs {
        penalty_bps: 1_200,
        ..CollateralArgs::defaults_for_tests()
    };
    w.set_collateral_config(k::ENGINE_ADMIN, &tighter)
        .expect("update");
    assert_eq!(coll(&w).penalty_bps, 1_200);
}

#[test]
fn set_collateral_config_rejections() {
    let mut w = World::with_engine();
    assert_err(
        w.set_collateral_config(k::ATTACKER, &CollateralArgs::defaults_for_tests()),
        "NotAdmin",
    );
    let too_big = CollateralArgs {
        type_cap: 10_000_001,
        ..CollateralArgs::defaults_for_tests()
    };
    assert_err(
        w.set_collateral_config(k::ENGINE_ADMIN, &too_big),
        "TypeCapAboveGlobal",
    );
    let r = w.engine(
        vec![
            id(k::ENGINE_ADMIN),
            config(),
            collateral_def(),
            coll_config(),
            ref_oracle(),
            protocol_parameters(),
            redemption_state(),
            clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        &[k::ENGINE_ADMIN],
        collateral_ix(&CollateralArgs::defaults_for_tests()),
    );
    assert_err(r, "OracleMismatch");
}

#[test]
fn global_cap_below_type_cap_is_rejected() {
    // M7: set_engine_params re-checks global_cap >= CollateralConfig.type_cap (5_000_000 by default).
    let mut w = World::with_engine();
    let low = auction_engine_core::args::EngineParamsArgs {
        global_cap: 4_999_999,
        ..auction_engine_core::args::EngineParamsArgs::defaults_for_tests()
    };
    assert_err(
        w.set_engine_params(k::ENGINE_ADMIN, &low),
        "TypeCapAboveGlobal",
    );
}

#[test]
fn admin_pause_is_exact() {
    let mut w = World::with_engine();
    w.advance(1_000);
    w.set_oracle(R, w.now);
    let paused = CollateralArgs {
        admin_paused: true,
        ..CollateralArgs::defaults_for_tests()
    };
    w.set_collateral_config(k::ENGINE_ADMIN, &paused).unwrap();
    assert_eq!(coll(&w).paused_since_ms, Some(T0 + 1_000));
    w.advance(10_000);
    w.set_oracle(R, w.now);
    w.set_collateral_config(k::ENGINE_ADMIN, &CollateralArgs::defaults_for_tests())
        .unwrap();
    let c = coll(&w);
    assert_eq!((c.paused_since_ms, c.paused_total_ms), (None, 10_000));
}

#[test]
fn observe_feed_persists_stale_pause() {
    // Review Focus 5: the feed goes stale with nobody watching; observe_feed records the derived onset.
    let mut w = World::with_engine();
    w.advance(3_600_000 + 5_000);
    w.observe_feed()
        .expect("observe_feed never fails on a bad feed");
    assert_eq!(coll(&w).paused_since_ms, Some(T0 + 3_600_000));
    w.advance(1_000);
    w.set_oracle(R, w.now);
    w.observe_feed().unwrap();
    let c = coll(&w);
    assert_eq!(c.paused_since_ms, None);
    assert_eq!(c.paused_total_ms, w.now - (T0 + 3_600_000));
}

#[test]
fn low_confidence_and_out_of_range_open_the_clock() {
    let mut w = World::with_engine();
    w.advance(1_000);
    w.set_oracle_full(R, w.now, R / 50); // 2% > max_conf 1%
    w.observe_feed().unwrap();
    assert_eq!(coll(&w).paused_since_ms, Some(w.now));

    let mut w = World::with_engine();
    w.advance(1_000);
    w.set_oracle(3 * R, w.now); // above 2R
    w.observe_feed().unwrap();
    assert_eq!(coll(&w).paused_since_ms, Some(w.now));
}

#[test]
fn observe_with_wrong_oracle_is_rejected() {
    let mut w = World::with_engine();
    let r = w.engine(
        vec![
            id(k::CALLER),
            config(),
            coll_config(),
            redemption_state(),
            ref_oracle(),
            clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        &[k::CALLER],
        auction_engine_core::Instruction::ObserveFeed,
    );
    assert_err(r, "OracleMismatch");
}
