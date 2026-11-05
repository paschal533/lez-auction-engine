//! `slack_ms` is bounded (ADR-064): `Init` and the admin-only `SetSlack` accept
//! `MIN_SLACK_MS..=MAX_SLACK_MS` and revert with `InvalidConfig` outside it.
//!
//! The new value reaches the config at once, the collateral actor through `SlackUpdate` in the
//! same transaction, and the system instructions at their next relay (they copy it from the
//! config every time).

use auction_engine_params::EngineError;
use engine_core::{
    actors::config_actor,
    check::{MAX_SLACK_MS, MIN_SLACK_MS, fail_text},
    msg::EngineMsg,
    state::{CollateralState, ConfigState},
};
use harness::{
    World, expect_revert, invariants,
    world::{RAY, SALT, SLACK_MS},
};
use lee::Actor;
use lee::error::LeeError;
use lee_core::program::TransactionEvent;

/// The chain time the observation tests start at, ms.
const T: u64 = 1_000_000;
/// A slack inside the bounds and above the world's.
const WIDER_MS: u64 = 30_000;

fn config_state(w: &World) -> ConfigState {
    w.chain
        .raw_state(w.config)
        .and_then(|bytes| ConfigState::decode(&bytes))
        .expect("a config state")
}

fn collateral_state(w: &World) -> CollateralState {
    w.chain
        .raw_state(w.collateral)
        .and_then(|bytes| CollateralState::decode(&bytes))
        .expect("a collateral state")
}

/// The admin sends `SetSlack { slack_ms }`.
fn set_slack(w: &mut World, slack_ms: u64) -> Result<Vec<TransactionEvent>, LeeError> {
    let (key, msg) = (w.admin_key.clone(), w.set_slack_msg(slack_ms));
    let sent = w.send_engine(w.admin, &[&key], &msg);
    invariants::check(w);
    sent
}

#[test]
fn set_slack_updates_config_and_next_observation_uses_it() {
    let mut w = World::new();
    w.set_now_ms(T);
    w.set_price(RAY);
    assert_eq!(collateral_state(&w).ctx.slack_ms, SLACK_MS);

    // Before: a claim 20 s behind the clock is outside the 6 s window.
    let claim = T - 20_000;
    expect_revert(w.observe_feed(claim), "Clock timestamp");

    set_slack(&mut w, WIDER_MS).expect("the admin widens the slack");
    assert_eq!(config_state(&w).slack_ms, WIDER_MS);
    // `SlackUpdate` carried it to the collateral actor, with no `SetCollateral`.
    assert_eq!(collateral_state(&w).ctx.slack_ms, WIDER_MS);

    w.observe_feed(claim)
        .expect("a claim within the new slack is admitted");
    invariants::check(&w);
}

#[test]
fn set_slack_without_collateral_type_only_updates_config() {
    let mut w = World::bare();
    let (key, init) = (w.admin_key.clone(), w.init_msg());
    w.send_engine(w.admin, &[&key], &init)
        .expect("the admin initialises");
    assert_eq!(config_state(&w).collateral_def, None);
    let actors = w.proxy_actors(w.admin, &w.set_slack_msg(WIDER_MS));
    assert_eq!(actors, vec![Actor::new(w.admin, w.engine), w.config]);
    set_slack(&mut w, WIDER_MS).expect("no relay, and the update succeeds");
    assert_eq!(config_state(&w).slack_ms, WIDER_MS);
    assert_eq!(w.chain.raw_state(w.collateral), None);
}

#[test]
fn slack_update_rejected_from_non_config_sender() {
    let mut w = World::new();
    // Another admin's config `K2`, so an actor can sit at its (existing) account.
    let (stranger_key, stranger) = w.chain.key("stranger");
    w.chain.fund(stranger, 1);
    let init = w.init_msg();
    w.send_engine(stranger, &[&stranger_key], &init)
        .expect("the stranger's own config");
    let k2 = config_actor(w.engine, stranger, SALT).account_id;
    let before = collateral_state(&w);
    let msg = EngineMsg::SlackUpdate { slack_ms: WIDER_MS };
    let to = w.collateral;
    let needle = fail_text(EngineError::NotAdmin);
    expect_revert(w.chain.send(to, &[to], &[], &msg), &needle);
    for at in [w.admin, k2, w.config.account_id] {
        let impostor = w.impostor(at);
        expect_revert(w.speak(impostor, to, &msg), &needle);
    }
    invariants::check(&w);
    assert_eq!(collateral_state(&w), before);
}

#[test]
fn set_slack_accepts_bounds() {
    let mut w = World::new();
    for slack_ms in [MIN_SLACK_MS, MAX_SLACK_MS] {
        set_slack(&mut w, slack_ms).expect("a bound applies");
        assert_eq!(config_state(&w).slack_ms, slack_ms);
    }
}

#[test]
fn set_slack_rejects_out_of_range() {
    let mut w = World::new();
    let before = config_state(&w);
    for slack_ms in [MIN_SLACK_MS - 1, MAX_SLACK_MS + 1, 0, u64::MAX] {
        expect_revert(
            set_slack(&mut w, slack_ms),
            &fail_text(EngineError::InvalidConfig),
        );
        assert_eq!(config_state(&w), before);
    }
}

#[test]
fn set_slack_rejects_non_admin() {
    let mut w = World::new();
    let before = config_state(&w);
    let msg = w.set_slack_msg(WIDER_MS);
    let (stranger_key, stranger) = w.chain.key("stranger");
    w.chain.fund(stranger, 1);
    let needle = fail_text(EngineError::NotAdmin);
    // Another signer at the admin's proxy, or nobody.
    expect_revert(w.send_engine(w.admin, &[&stranger_key], &msg), &needle);
    expect_revert(w.send_engine(w.admin, &[], &msg), &needle);
    // A root straight at the config.
    let (config, actors) = (w.config, w.proxy_actors(w.admin, &msg));
    expect_revert(w.chain.send(config, &actors, &[], &msg), &needle);
    invariants::check(&w);
    assert_eq!(config_state(&w), before);
}

#[test]
fn init_rejects_out_of_range_slack() {
    for slack_ms in [0, MIN_SLACK_MS - 1, MAX_SLACK_MS + 1] {
        let mut w = World::bare();
        let (key, mut init) = (w.admin_key.clone(), w.init_msg());
        let EngineMsg::Init { slack_ms: slot, .. } = &mut init else {
            panic!("an Init message")
        };
        *slot = slack_ms;
        expect_revert(
            w.send_engine(w.admin, &[&key], &init),
            &fail_text(EngineError::InvalidConfig),
        );
        assert_eq!(
            w.chain.raw_state(w.config),
            None,
            "no config for {slack_ms}"
        );
    }
}
