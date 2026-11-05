//! `Init` checks the host's parameters (R-7c, ADR-065): the config asks the host it names for its
//! state and refuses, with the whole transaction, a host whose stablecoin or token differs
//! (`HostMismatch`, 11) or whose oracle differs (`OracleMismatch`, 13), or a host that never
//! answers (`HostMismatch` from the `Proceed` sentinel).

use auction_engine_params::EngineError;
use engine_core::{check::fail_text, msg::EngineMsg, state::ConfigState};
use harness::{
    World, expect_revert,
    impostor::{encode, script},
    invariants,
    world::{MIN_CR_RAY, RAY},
};
use lee::{AccountId, Actor};
use lee_core::program::{StateReply, TransactionEvent};
use mock_host_core::{Params, TAG_PARAMS};

/// A definition account the engine does not name.
fn other_account() -> AccountId {
    AccountId::new([0xAB; 32])
}

/// The admin sends the world's `Init`, with `edit` applied to it.
fn init_with(
    w: &mut World,
    edit: impl FnOnce(&mut EngineMsg),
) -> Result<Vec<TransactionEvent>, lee::error::LeeError> {
    let (key, mut init) = (w.admin_key.clone(), w.init_msg());
    edit(&mut init);
    w.send_engine(w.admin, &[&key], &init)
}

/// Nothing `Init` creates exists: config, accounting, protocol token definition.
fn assert_nothing_created(w: &World) {
    assert_eq!(w.chain.raw_state(w.config), None, "no config");
    assert_eq!(w.chain.raw_state(w.accounting), None, "no accounting");
    let protocol = Actor::new(w.protocol_def(), w.token);
    assert_eq!(w.chain.raw_state(protocol), None, "no protocol definition");
}

#[test]
fn init_succeeds_with_matching_host() {
    let mut w = World::bare();
    init_with(&mut w, |_| {}).expect("a matching host passes the check");
    let config = w
        .chain
        .raw_state(w.config)
        .and_then(|bytes| ConfigState::decode(&bytes))
        .expect("the config exists");
    assert!(!config.pending_host_read, "the host read completed");
    assert_eq!(config.pending_read, None);
    invariants::check(&w);
}

#[test]
fn init_reverts_when_host_names_another_stablecoin() {
    let mut w = World::bare_with_host(|host| host.stable_def = other_account());
    expect_revert(
        init_with(&mut w, |_| {}),
        &fail_text(EngineError::HostMismatch),
    );
    assert_nothing_created(&w);
}

#[test]
fn init_reverts_when_host_names_another_token() {
    let mut w = World::bare_with_host(|host| host.token = other_account());
    expect_revert(
        init_with(&mut w, |_| {}),
        &fail_text(EngineError::HostMismatch),
    );
    assert_nothing_created(&w);
}

#[test]
fn init_reverts_when_host_names_another_oracle() {
    let other = Actor::new(other_account(), other_account());
    let mut w = World::bare_with_host(|host| host.oracle = other);
    expect_revert(
        init_with(&mut w, |_| {}),
        &fail_text(EngineError::OracleMismatch),
    );
    assert_nothing_created(&w);
}

#[test]
fn init_reverts_when_host_never_replies() {
    let mut w = World::bare();
    // An impostor with no state accepts any message and sends nothing: a silent host.
    let (_, silent) = w.fresh_impostor("silent-host");
    expect_revert(
        init_with(&mut w, |init| {
            if let EngineMsg::Init { host, .. } = init {
                *host = silent;
            }
        }),
        &fail_text(EngineError::HostMismatch),
    );
    assert_nothing_created(&w);
}

#[test]
fn init_refuses_a_matching_params_reply_from_an_actor_that_is_not_the_host() {
    let mut w = World::bare();
    // The forger answers as itself with `Params` that match the config exactly.
    let matching = Params {
        tag: TAG_PARAMS,
        admin: w.params,
        token: w.token,
        stable_def: w.stable_def,
        collateral_def: w.collateral_def,
        oracle: w.oracle,
        redemption_price_ray: RAY,
        min_cr_ray: MIN_CR_RAY,
        engine_actor: None,
        pending: None,
        is_frozen: false,
    };
    let reply = encode(&StateReply {
        state: encode(&matching).into(),
    });
    let forger = w.impostor_with_state("forger", script(&[(w.config, reply)]));
    // The named host never answers; its only act is to ping the forger, which then speaks.
    let silent = w.impostor_with_state("named-host", script(&[(forger, Vec::new())]));
    let (key, mut init) = (w.admin_key.clone(), w.init_msg());
    if let EngineMsg::Init { host, .. } = &mut init {
        *host = silent;
    }
    let mut actors = w.proxy_actors(w.admin, &init);
    actors.push(forger);
    let root = Actor::new(w.admin, w.engine);
    // The reply is not the host's, so the config does not take it as the awaited answer: it is
    // left to the message dispatch, which refuses it, and the whole transaction reverts.
    expect_revert(w.chain.send(root, &actors, &[&key], &init), "BadMessage");
    assert_nothing_created(&w);
}
