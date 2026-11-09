//! Stepped pause levels in the engine (spec §3.2.1-§3.2.2, ADR-073): `SetPauseLevel` at the
//! admin's proxy writes the config's `system_level` and the type's `pause_level`, then observes,
//! so the pause clock records the change. Level 1 stops liquidations, level 2 also the system
//! starts, level 3 also the bids and the system takes; the legacy `admin_paused` flag is level 3
//! for the type's collateral operations with v0.2.4's code `Paused`, and leaves the system alone.
//!
//! Every refusal asserts `World::snapshot_all` is unchanged; `harness::invariants::check` after
//! every step.
//!
//! Numbers: `World::new_at(RAY / 2)` (legacy risk), 500_000 collateral and 600_000 debt, the
//! oracle at 0.5 RAY; the system tests use `World::with_system` (protocol token on).

use auction_engine_params::{
    EngineError,
    args::{CollateralArgs, EngineParamsArgs},
};
use engine_core::{
    actors::config_actor,
    check::{Refusal, fail_text},
    msg::EngineMsg,
    state::ConfigState,
};
use harness::{
    World, expect_revert, invariants,
    system::TAKER_GENESIS,
    world::{RAY, SALT},
};
use lee::{AccountId, Actor, error::LeeError};
use lee_core::program::TransactionEvent;
use risk_core::RiskError;

const COLLATERAL: u128 = 500_000;
const DEBT: u128 = 600_000;
/// A bidder's stablecoin.
const FUNDS: u128 = 1_000_000;
/// A bid's coin in.
const BID_IN: u128 = 100_000;
/// The chain time the tests start at, ms.
const T: u64 = 1_000_000;
/// `EngineParamsArgs::defaults_for_tests().system_ttl_ms`.
const TTL: u64 = 14_400_000;
/// The pause the clock test holds level 3 for, ms.
const HELD_MS: u64 = 60_000;
/// When the clock test raises the level to 3, and when it lowers it again, ms.
const RAISED_AT: u64 = T + 1_000;
const LOWERED_AT: u64 = RAISED_AT + HELD_MS;
/// After the surplus auction's TTL, ms.
const AFTER_TTL: u64 = T + TTL;

type Sent = Result<Vec<TransactionEvent>, LeeError>;

/// `World::new_at(RAY / 2)` at `T` with one unsafe position and the oracle back at 0.5 RAY.
fn world() -> (World, AccountId) {
    let mut w = World::new_at(RAY / 2);
    w.set_now_ms(T);
    let position = w.open_unsafe_position(COLLATERAL, DEBT);
    w.set_price(RAY / 2);
    invariants::check(&w);
    (w, position)
}

/// `World::with_system` (protocol token on) at `T` with 1_500 stablecoin in the buffer, so a
/// surplus auction of 500 can start.
fn system_world() -> World {
    let mut w = World::with_system(EngineParamsArgs {
        protocol_token_enabled: true,
        ..World::engine_params()
    });
    w.set_now_ms(T);
    let buffer = w.buffer().account_id;
    w.donate_stable(buffer, 1_500);
    invariants::check(&w);
    w
}

/// The admin sets `level`, which must apply.
fn set_level(w: &mut World, level: u8) {
    w.set_pause_level(level)
        .unwrap_or_else(|err| panic!("level {level} applies: {err}"));
    invariants::check(w);
    assert_eq!(w.collateral_state().ext.pause_level, level);
    assert_eq!(config_state(w).ext.system_level, level);
}

/// The admin's `SetCollateral` with the default arguments and `admin_paused`.
fn admin_pause(w: &mut World, admin_paused: bool) {
    let set = w.set_collateral_msg(CollateralArgs {
        admin_paused,
        ..World::collateral_args()
    });
    let key = w.admin_key.clone();
    w.send_engine(w.admin, &[&key], &set)
        .expect("the admin sets the flag");
    invariants::check(w);
}

/// `send` must revert with `err` and leave every actor the world knows of unchanged.
fn refused(w: &mut World, send: impl FnOnce(&mut World) -> Sent, err: impl Into<Refusal>) {
    let before = w.snapshot_all();
    expect_revert(send(w), &fail_text(err));
    assert_eq!(w.snapshot_all(), before, "a refusal changes nothing");
    invariants::check(w);
}

fn config_state(w: &World) -> ConfigState {
    w.chain
        .raw_state(w.config)
        .and_then(|bytes| ConfigState::decode(&bytes))
        .expect("a config state")
}

#[test]
fn admin_paused_keeps_the_v024_code() {
    let (mut w, live) = world();
    let other = w.open_unsafe_position(COLLATERAL, DEBT);
    w.set_price(RAY / 2);
    w.liquidate(live, 0).expect("the keeper liquidates");
    let bidder = w.fund_bidder("bidder", FUNDS);
    invariants::check(&w);
    set_level(&mut w, 1);
    admin_pause(&mut w, true);
    refused(&mut w, |w| w.liquidate(other, 0), EngineError::Paused);
    refused(
        &mut w,
        |w| w.bid(&bidder, live, 0, (BID_IN, 0)),
        EngineError::Paused,
    );
    // The flag does not raise the stored levels.
    assert_eq!(w.collateral_state().ext.pause_level, 1);
    assert_eq!(config_state(&w).ext.system_level, 1);
}

#[test]
fn admin_paused_does_not_stop_system_auctions() {
    let mut w = system_world();
    admin_pause(&mut w, true);
    assert!(w.collateral_state().admin_paused);
    w.start_surplus(0).expect("the surplus auction starts");
    invariants::check(&w);
    // tokens = ceil(ceil(500 x 1.05) x RAY / 2 RAY) = 263.
    w.take_surplus(0, 500, 263).expect("the lot is taken");
    invariants::check(&w);
}

#[test]
fn level_2_blocks_starts_not_takes_or_closes() {
    let mut w = system_world();
    w.start_surplus(0).expect("the surplus auction starts");
    invariants::check(&w);
    set_level(&mut w, 2);
    refused(&mut w, |w| w.start_debt(0), RiskError::SystemAuctionsPaused);
    refused(
        &mut w,
        |w| w.start_surplus(1),
        RiskError::SystemAuctionsPaused,
    );
    w.take_surplus(0, 100, TAKER_GENESIS)
        .expect("a take runs at level 2");
    invariants::check(&w);
    w.set_now_ms(AFTER_TTL);
    w.close_surplus(0).expect("a close runs at level 2");
    invariants::check(&w);
    assert!(!w.accounting_state().surplus_live);
}

#[test]
fn level_3_refuses_system_takes_and_bids() {
    let mut w = system_world();
    w.start_surplus(0).expect("the surplus auction starts");
    set_level(&mut w, 3);
    refused(
        &mut w,
        |w| w.take_surplus(0, 100, TAKER_GENESIS),
        RiskError::BidsPaused,
    );
    let (mut w, position) = world();
    w.liquidate(position, 0).expect("the keeper liquidates");
    let bidder = w.fund_bidder("bidder", FUNDS);
    set_level(&mut w, 2);
    w.bid(&bidder, position, 0, (BID_IN, 0))
        .expect("a bid runs at level 2");
    invariants::check(&w);
    set_level(&mut w, 3);
    refused(
        &mut w,
        |w| w.bid(&bidder, position, 0, (BID_IN, 0)),
        RiskError::BidsPaused,
    );
}

#[test]
fn lowering_the_level_resumes_liquidation() {
    let (mut w, position) = world();
    set_level(&mut w, 1);
    refused(
        &mut w,
        |w| w.liquidate(position, 0),
        RiskError::LiquidationsPaused,
    );
    set_level(&mut w, 0);
    w.liquidate(position, 0).expect("the type liquidates again");
    invariants::check(&w);
}

#[test]
fn set_pause_level_only_through_the_admin_proxy() {
    let (mut w, _) = world();
    let (stranger_key, stranger) = w.chain.key("stranger");
    w.chain.fund(stranger, 1);
    let msg = w.set_pause_level_msg(1);
    // Another signer at the admin's proxy.
    refused(
        &mut w,
        |w| w.send_engine(w.admin, &[&stranger_key], &msg),
        EngineError::NotAdmin,
    );
    // A stranger's own proxy forwards to the stranger's (absent) config.
    refused(
        &mut w,
        |w| w.send_engine(stranger, &[&stranger_key], &msg),
        EngineError::WrongAccount("config"),
    );
    // A root straight at the config.
    let (config, actors) = (w.config, w.proxy_actors(w.admin, &msg));
    refused(
        &mut w,
        |w| w.chain.send(config, &actors, &[], &msg),
        EngineError::NotAdmin,
    );
    // The admin, naming a type the config does not record (ruling R9-9).
    let wrong = EngineMsg::SetPauseLevel {
        salt: SALT,
        collateral_def: stranger,
        level: 1,
        now_ms: w.now_ms,
    };
    let (proxy, key) = (Actor::new(w.admin, w.engine), w.admin_key.clone());
    refused(
        &mut w,
        |w| w.chain.send(proxy, &actors, &[&key], &wrong),
        EngineError::WrongAccount("collateral_def"),
    );
    assert_eq!(w.collateral_state().ext.pause_level, 0);
}

#[test]
fn level_change_records_the_pause_clock() {
    let (mut w, _) = world();
    // The world configured its type on the `Init` price, outside the sanity band: one live
    // observation closes that pause first.
    w.observe_feed(T).expect("the feed is observed");
    let c = w.collateral_state();
    assert_eq!(c.paused_since_ms, None);
    let total = c.paused_total_ms;
    let held = total.checked_add(HELD_MS).expect("fits");
    w.set_now_ms(RAISED_AT);
    w.set_price(RAY / 2);
    set_level(&mut w, 3);
    let c = w.collateral_state();
    assert_eq!(
        (c.paused_since_ms, c.paused_total_ms),
        (Some(RAISED_AT), total)
    );
    w.set_now_ms(LOWERED_AT);
    w.set_price(RAY / 2);
    set_level(&mut w, 0);
    let c = w.collateral_state();
    assert_eq!((c.paused_since_ms, c.paused_total_ms), (None, held));
    // Levels 1 and 2 leave the discount clock running.
    for level in [1, 2] {
        set_level(&mut w, level);
        let c = w.collateral_state();
        assert_eq!((c.paused_since_ms, c.paused_total_ms), (None, held));
    }
}

/// A forged `PauseLevelUpdate` (message 48) at the collateral actor: refused unless it comes
/// from the type's own config, and nothing changes (as `SlackUpdate`, `tests/slack.rs`).
#[test]
fn pause_level_update_rejected_from_non_config_sender() {
    let (mut w, _) = world();
    // Another admin's config `K2`, so an actor can sit at its (existing) account.
    let (stranger_key, stranger) = w.chain.key("stranger");
    w.chain.fund(stranger, 1);
    let init = w.init_msg();
    w.send_engine(stranger, &[&stranger_key], &init)
        .expect("the stranger's own config");
    let k2 = config_actor(w.engine, stranger, SALT).account_id;
    invariants::check(&w);
    let msg = EngineMsg::PauseLevelUpdate { level: 3 };
    let to = w.collateral;
    // A root straight at the collateral actor.
    refused(
        &mut w,
        |w| w.chain.send(to, &[to], &[], &msg),
        EngineError::NotAdmin,
    );
    // An impostor at the admin, at another config and at the config's own account.
    for at in [w.admin, k2, w.config.account_id] {
        let impostor = w.impostor(at);
        refused(
            &mut w,
            |w| w.speak(impostor, to, &msg),
            EngineError::NotAdmin,
        );
    }
    assert_eq!(w.collateral_state().ext.pause_level, 0);
}

#[test]
fn set_pause_level_rejects_level_above_3() {
    let (mut w, _) = world();
    for level in [4, u8::MAX] {
        refused(
            &mut w,
            |w| w.set_pause_level(level),
            EngineError::InvalidConfig,
        );
    }
}
