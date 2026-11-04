//! The protocol-token definition created at `Init`, `SetEngineParams` (design note §4.2 row 1;
//! v0.2.4 `set_engine_params`) and the permissionless `ObserveFeed` (row 3), with
//! `harness::invariants::check` after every send.
//!
//! Every `Init` creates the protocol definition `(protocol_def(E, K), T)` with minter `K`; the
//! config names it only while `protocol_token_enabled`. `SetEngineParams` goes through the admin's
//! proxy; the config asks its recorded collateral type to check its cap (`AssertCapAtMost`) and,
//! when a locked field changes, the accounting to confirm no system auction is live
//! (`AssertSystemIdle`).

use auction_engine_params::{
    EngineError,
    args::{CollateralArgs, EngineParamsArgs},
};
use engine_core::{
    actors::{collateral_actor, config_actor},
    check::fail_text,
    msg::{EngineMsg, ProtocolGenesis},
    state::{CollateralState, ConfigState},
};
use harness::{
    World, expect_revert, invariants,
    world::{RAY, SALT, SLACK_MS},
};
use lee::{AccountId, Actor};
use token_core::{Definition, TokenMsg};

/// The genesis supply of the protocol token.
const GENESIS: u128 = 10_000;
/// The chain time the `ObserveFeed` tests start at, ms.
const T: u64 = 1_000_000;
/// `CollateralArgs::defaults_for_tests().max_oracle_age_ms`.
const MAX_AGE_MS: u64 = 3_600_000;
/// `CollateralArgs::defaults_for_tests().type_cap`.
const TYPE_CAP: u128 = 5_000_000;

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

fn definition(w: &World) -> Option<Definition> {
    w.chain
        .raw_state(Actor::new(w.protocol_def(), w.token))
        .map(|bytes| Definition::decode(&bytes).expect("a token definition"))
}

/// `World::bare()` initialised with `params`, the protocol token's genesis going to a fresh
/// `"taker"` signer. Returns the world and the taker.
fn initialised_with(params: EngineParamsArgs) -> (World, AccountId) {
    let mut w = World::bare();
    let (_, taker) = w.chain.key("taker");
    let genesis = ProtocolGenesis {
        to: taker,
        supply: GENESIS,
    };
    let (key, init) = (
        w.admin_key.clone(),
        w.init_msg_full(params, None, Some(genesis)),
    );
    w.send_engine(w.admin, &[&key], &init)
        .expect("the admin initialises");
    invariants::check(&w);
    (w, taker)
}

/// `World::new()` (configured collateral, bound host).
fn configured() -> World {
    let w = World::new();
    invariants::check(&w);
    w
}

/// Sends `SetEngineParams` with `params` at the admin's proxy, naming the world's collateral.
fn set_params(w: &mut World, params: EngineParamsArgs) -> Result<(), String> {
    let key = w.admin_key.clone();
    let msg = w.set_engine_params_msg(params, None);
    let result = w.send_engine(w.admin, &[&key], &msg);
    invariants::check(w);
    result.map(|_| ()).map_err(|err| format!("{err} {err:?}"))
}

fn expect_err(result: Result<(), String>, err: EngineError) {
    let needle = fail_text(err);
    match result {
        Ok(()) => panic!("expected a revert with {needle:?}, but the transaction applied"),
        Err(text) => assert!(text.contains(&needle), "{text} does not mention {needle:?}"),
    }
}

#[test]
fn init_creates_the_protocol_definition_with_minter_k() {
    let enabled = EngineParamsArgs {
        protocol_token_enabled: true,
        ..World::engine_params()
    };
    let (w, taker) = initialised_with(enabled);
    let protocol_def = w.protocol_def();
    assert_eq!(
        definition(&w),
        Some(Definition::new(GENESIS, Some(w.config))),
        "minter is the config actor K"
    );
    assert_eq!(w.balance(taker, protocol_def), GENESIS);
    assert_eq!(config_state(&w).protocol_token, Some(protocol_def));

    // Disabled: the config names no protocol token, but the definition exists all the same.
    let (w, taker) = initialised_with(World::engine_params());
    assert_eq!(config_state(&w).protocol_token, None);
    assert_eq!(
        definition(&w),
        Some(Definition::new(GENESIS, Some(w.config)))
    );
    assert_eq!(w.balance(taker, w.protocol_def()), GENESIS);

    // Without a genesis the definition starts with no supply.
    let w = configured();
    assert_eq!(definition(&w), Some(Definition::new(0, Some(w.config))));
}

#[test]
fn protocol_definition_cannot_be_pre_blocked() {
    let mut w = World::bare();
    let protocol_def = w.protocol_def();
    // The whale credits the derivable definition account before `Init` (ADR-062: the credit
    // lands in the account's holding of the collateral, never at `(protocol_def, T)`).
    let whale = Actor::new(w.whale, w.token);
    let transfer = TokenMsg::Transfer {
        definition: w.collateral_def,
        to: protocol_def,
        amount: 1,
    };
    let actors = [
        whale,
        w.holding(w.whale, w.collateral_def),
        w.holding(protocol_def, w.collateral_def),
    ];
    let key = w.whale_key.clone();
    w.chain
        .send(whale, &actors, &[&key], &transfer)
        .expect("anyone may credit the derivable account");
    assert_eq!(w.balance(protocol_def, w.collateral_def), 1);
    assert_eq!(definition(&w), None);

    let (key, init) = (w.admin_key.clone(), w.init_msg());
    w.send_engine(w.admin, &[&key], &init)
        .expect("the credit does not block Init");
    invariants::check(&w);
    assert_eq!(definition(&w), Some(Definition::new(0, Some(w.config))));
}

#[test]
fn init_checks_the_reference_oracle_slot() {
    let mut w = World::bare();
    let key = w.admin_key.clone();
    let enabled = EngineParamsArgs {
        ref_oracle_enabled: true,
        ..World::engine_params()
    };
    let cases = [
        (
            w.init_msg_full(enabled.clone(), None, None),
            EngineError::ZeroId,
        ),
        (
            w.init_msg_full(World::engine_params(), Some(w.oracle), None),
            EngineError::WrongAccount("unused slot"),
        ),
        (
            w.init_msg_full(
                enabled.clone(),
                Some(Actor::new(w.stable_def, w.token)),
                None,
            ),
            EngineError::WrongAccount("ref_oracle"),
        ),
        // v0.2.4 `check_distinct_slots`: not the protocol token either.
        (
            w.init_msg_full(
                enabled.clone(),
                Some(Actor::new(w.protocol_def(), w.oracle.program_account_id)),
                None,
            ),
            EngineError::WrongAccount("ref_oracle"),
        ),
    ];
    for (init, err) in cases {
        expect_revert(w.send_engine(w.admin, &[&key], &init), &fail_text(err));
    }
    let init = w.init_msg_full(enabled, Some(w.oracle), None);
    w.send_engine(w.admin, &[&key], &init)
        .expect("an enabled slot with its actor");
    invariants::check(&w);
    assert_eq!(config_state(&w).protocol_token_oracle, Some(w.oracle));
}

#[test]
fn set_engine_params_only_through_the_admin_proxy() {
    let mut w = configured();
    let before = config_state(&w);
    let msg = w.set_engine_params_msg(
        EngineParamsArgs {
            global_cap: 9_000_000,
            ..World::engine_params()
        },
        None,
    );
    let (stranger_key, stranger) = w.chain.key("stranger");
    w.chain.fund(stranger, 1);
    // Another signer at the admin's proxy, or nobody: not authorized.
    expect_revert(
        w.send_engine(w.admin, &[&stranger_key], &msg),
        &fail_text(EngineError::NotAdmin),
    );
    expect_revert(
        w.send_engine(w.admin, &[], &msg),
        &fail_text(EngineError::NotAdmin),
    );
    // A root straight at the config.
    let (config, actors) = (w.config, w.proxy_actors(w.admin, &msg));
    expect_revert(
        w.chain.send(config, &actors, &[], &msg),
        &fail_text(EngineError::NotAdmin),
    );
    // The stranger's own proxy forwards to the stranger's (absent) config.
    expect_revert(
        w.send_engine(stranger, &[&stranger_key], &msg),
        &fail_text(EngineError::WrongAccount("config")),
    );
    invariants::check(&w);
    assert_eq!(config_state(&w), before);
}

#[test]
fn set_engine_params_updates_the_config() {
    let mut w = configured();
    let before = config_state(&w);
    let params = EngineParamsArgs {
        global_cap: 9_000_000,
        bad_debt_freeze_threshold: 2_000,
        protocol_token_enabled: true,
        ..World::engine_params()
    };
    set_params(&mut w, params).expect("the admin sets the engine params");
    assert_eq!(
        config_state(&w),
        ConfigState {
            global_cap: 9_000_000,
            bad_debt_freeze_threshold: 2_000,
            protocol_token: Some(w.protocol_def()),
            ..before.clone()
        }
    );
    // Disabling it again names no protocol token.
    set_params(&mut w, World::engine_params()).expect("the admin disables the protocol token");
    assert_eq!(config_state(&w), before);
}

#[test]
fn ref_price_step_is_bounded() {
    let mut w = configured();
    let old = World::engine_params().protocol_token_ref_price;
    let step = |price| EngineParamsArgs {
        protocol_token_ref_price: price,
        ..World::engine_params()
    };
    expect_err(
        set_params(&mut w, step(old * 2 + 1)),
        EngineError::RefPriceStepTooLarge,
    );
    assert_eq!(config_state(&w).protocol_token_ref_price, old);
    set_params(&mut w, step(old * 2)).expect("a doubling is allowed");
    assert_eq!(config_state(&w).protocol_token_ref_price, old * 2);
}

#[test]
fn lowering_global_cap_below_the_type_cap_reverts() {
    let mut w = configured();
    let before = config_state(&w);
    let lower = |global_cap| EngineParamsArgs {
        global_cap,
        ..World::engine_params()
    };
    expect_err(
        set_params(&mut w, lower(TYPE_CAP - 1)),
        EngineError::TypeCapAboveGlobal,
    );
    assert_eq!(config_state(&w), before);
    set_params(&mut w, lower(TYPE_CAP)).expect("the type cap may equal the global cap");
    assert_eq!(config_state(&w).global_cap, TYPE_CAP);
}

#[test]
fn config_without_collateral_accepts_any_global_cap() {
    let mut w = World::bare();
    let (key, init) = (w.admin_key.clone(), w.init_msg());
    w.send_engine(w.admin, &[&key], &init)
        .expect("the admin initialises");
    assert_eq!(config_state(&w).collateral_def, None);
    // No type yet: no cap to violate, and no collateral actor is asked (none is declared).
    set_params(
        &mut w,
        EngineParamsArgs {
            global_cap: 1,
            ..World::engine_params()
        },
    )
    .expect("any global cap");
    assert_eq!(config_state(&w).global_cap, 1);
    // The first type is then validated against that cap.
    let set = w.set_collateral_msg(World::collateral_args());
    expect_revert(
        w.send_engine(w.admin, &[&key], &set),
        &fail_text(EngineError::TypeCapAboveGlobal),
    );
    invariants::check(&w);
}

#[test]
fn second_collateral_type_on_a_config_reverts() {
    let mut w = configured();
    assert_eq!(config_state(&w).collateral_def, Some(w.collateral_def));
    let key = w.admin_key.clone();
    let (_, other_def) = w.chain.key("other-def");
    let second = EngineMsg::SetCollateral {
        salt: SALT,
        collateral_def: other_def,
        args: World::collateral_args(),
        now_ms: w.now_ms,
    };
    expect_revert(
        w.send_engine(w.admin, &[&key], &second),
        &fail_text(EngineError::WrongAccount("collateral_def")),
    );
    invariants::check(&w);
    assert_eq!(config_state(&w).collateral_def, Some(w.collateral_def));
    // The recorded type can still be re-configured.
    let again = w.set_collateral_msg(CollateralArgs {
        penalty_bps: 1_500,
        ..World::collateral_args()
    });
    w.send_engine(w.admin, &[&key], &again)
        .expect("the same type is re-configured");
    invariants::check(&w);
    assert_eq!(collateral_state(&w).penalty_bps, 1_500);
}

#[test]
fn set_engine_params_checks_the_reference_oracle_slot() {
    let mut w = configured();
    let enabled = EngineParamsArgs {
        ref_oracle_enabled: true,
        ..World::engine_params()
    };
    let key = w.admin_key.clone();
    let oracle_program = w.oracle.program_account_id;
    let cases = [
        (None, EngineError::ZeroId),
        (
            Some(Actor::new(w.stable_def, oracle_program)),
            EngineError::WrongAccount("ref_oracle"),
        ),
        (
            Some(Actor::new(w.protocol_def(), oracle_program)),
            EngineError::WrongAccount("ref_oracle"),
        ),
    ];
    for (ref_oracle, err) in cases {
        let msg = w.set_engine_params_msg(enabled.clone(), ref_oracle);
        expect_revert(w.send_engine(w.admin, &[&key], &msg), &fail_text(err));
    }
    let msg = w.set_engine_params_msg(World::engine_params(), Some(w.oracle));
    expect_revert(
        w.send_engine(w.admin, &[&key], &msg),
        &fail_text(EngineError::WrongAccount("unused slot")),
    );
    let msg = w.set_engine_params_msg(enabled, Some(w.oracle));
    w.send_engine(w.admin, &[&key], &msg)
        .expect("an enabled slot with its actor");
    invariants::check(&w);
    assert_eq!(config_state(&w).protocol_token_oracle, Some(w.oracle));
}

/// LEZ's refusal of a message to `actor` when the transaction does not declare it.
fn undeclared(actor: Actor) -> String {
    format!("UndeclaredActor {{ actor: {actor:?} }}")
}

/// `SetEngineParams` declaring every actor but `left_out`.
fn set_params_without(
    w: &mut World,
    params: EngineParamsArgs,
    left_out: Actor,
) -> Result<(), String> {
    let msg = w.set_engine_params_msg(params, None);
    let actors: Vec<Actor> = w
        .proxy_actors(w.admin, &msg)
        .into_iter()
        .filter(|actor| *actor != left_out)
        .collect();
    let (root, key) = (Actor::new(w.admin, w.engine), w.admin_key.clone());
    let result = w.chain.send(root, &actors, &[&key], &msg);
    invariants::check(w);
    result.map(|_| ()).map_err(|err| format!("{err} {err:?}"))
}

/// Which leaf assertions `K` sends: a message to an actor the transaction does not declare fails,
/// so leaving the accounting (or the collateral actor) out shows whether it is addressed.
#[test]
fn system_idle_is_asked_only_when_a_locked_field_changes() {
    let mut w = configured();
    let (accounting, collateral) = (w.accounting, w.collateral);
    // No locked field changes: the accounting is not asked.
    let unlocked = EngineParamsArgs {
        global_cap: 9_000_000,
        ..World::engine_params()
    };
    set_params_without(&mut w, unlocked, accounting).expect("the accounting is not addressed");
    assert_eq!(config_state(&w).global_cap, 9_000_000);
    // A locked field changes: the accounting is asked.
    let locked = EngineParamsArgs {
        protocol_token_enabled: true,
        ..World::engine_params()
    };
    let err = set_params_without(&mut w, locked.clone(), accounting)
        .expect_err("AssertSystemIdle goes to the undeclared accounting");
    assert!(err.contains(&undeclared(accounting)), "{err}");
    assert_eq!(config_state(&w).protocol_token, None);
    // The recorded type is always asked for its cap, locked change or not.
    let err = set_params_without(&mut w, World::engine_params(), collateral)
        .expect_err("AssertCapAtMost goes to the undeclared collateral actor");
    assert!(err.contains(&undeclared(collateral)), "{err}");
    set_params(&mut w, locked).expect("with every actor declared");
    assert_eq!(config_state(&w).protocol_token, Some(w.protocol_def()));
}

#[test]
fn locked_fields_change_while_no_system_auction_is_live() {
    let mut w = configured();
    let params = EngineParamsArgs {
        protocol_token_enabled: true,
        surplus_lot: 400,
        ..World::engine_params()
    };
    set_params(&mut w, params).expect("no system auction is live");
    let config = config_state(&w);
    assert_eq!(config.protocol_token, Some(w.protocol_def()));
    assert_eq!(config.system.surplus_lot, 400);
}

/// Root deliveries, and deliveries from impostor-program actors at the admin's account and at
/// another admin's config account (the program half of the sender check). The account half, an
/// engine actor that does not re-derive the receiver (`(admin, E)`, another config `(K2, E)`), is
/// unreachable in a transaction: no engine handler addresses these messages anywhere but its own
/// config's actors. It is unit-tested on the check itself (`engine_core::assertions`).
#[test]
fn assertions_accept_only_their_config() {
    let mut w = configured();
    // Another admin's config `K2`, so an actor can sit at its (existing) account.
    let (stranger_key, stranger) = w.chain.key("stranger");
    w.chain.fund(stranger, 1);
    let init = w.init_msg();
    w.send_engine(stranger, &[&stranger_key], &init)
        .expect("the stranger's own config");
    let k2 = config_actor(w.engine, stranger, SALT).account_id;
    let cases = [
        (w.collateral, EngineMsg::AssertCapAtMost { global_cap: 1 }),
        (w.accounting, EngineMsg::AssertSystemIdle),
    ];
    for (to, msg) in cases {
        expect_revert(
            w.chain.send(to, &[to], &[], &msg),
            &fail_text(EngineError::NotAdmin),
        );
        for at in [w.admin, k2, w.config.account_id] {
            let impostor = w.impostor(at);
            expect_revert(
                w.speak(impostor, to, &msg),
                &fail_text(EngineError::NotAdmin),
            );
        }
    }
    invariants::check(&w);
}

#[test]
fn observe_feed_on_an_unconfigured_type_is_not_admitted() {
    let mut w = configured();
    let (_, other_def) = w.chain.key("other-def");
    let other = collateral_actor(w.engine, w.config.account_id, other_def);
    let msg = EngineMsg::ObserveFeed {
        config: w.config.account_id,
        collateral_def: other_def,
        now_ms: w.now_ms,
    };
    let actors = [other, w.params, w.oracle, w.chain.clock()];
    // No probe (brief decision): the absent actor is refused by LEZ before the engine runs.
    expect_revert(
        w.chain.send(other, &actors, &[], &msg),
        "UnadmittedPublicActor",
    );
    assert_eq!(w.chain.raw_state(other), None);
    invariants::check(&w);
}

#[test]
fn observe_feed_records_a_pause_without_reverting() {
    let mut w = configured();
    w.set_now_ms(T);
    w.set_price(RAY);
    w.observe_feed(T).expect("a live feed");
    invariants::check(&w);
    assert_eq!(collateral_state(&w).paused_since_ms, None);

    // Stale at now + slack: recorded as a pause, and the transaction still applies.
    let stale_at = T + MAX_AGE_MS - SLACK_MS + 1;
    w.set_now_ms(stale_at);
    w.observe_feed(stale_at).expect("a stale feed is recorded");
    invariants::check(&w);
    let paused = collateral_state(&w);
    assert_eq!(
        (paused.paused_since_ms, paused.paused_total_ms),
        (Some(stale_at), 0)
    );

    // Fresh again: the pause closes at the claim.
    let fresh_at = stale_at + 10_000;
    w.set_now_ms(fresh_at);
    w.set_price(RAY);
    w.observe_feed(fresh_at).expect("a fresh feed");
    invariants::check(&w);
    let resumed = collateral_state(&w);
    assert_eq!(
        (resumed.paused_since_ms, resumed.paused_total_ms),
        (None, 10_000)
    );
}

#[test]
fn observe_feed_with_now_outside_the_clock_window_reverts() {
    let mut w = configured();
    w.set_now_ms(T);
    let before = collateral_state(&w);
    for now_ms in [T - SLACK_MS - 1, T + 1] {
        expect_revert(w.observe_feed(now_ms), "Clock timestamp");
        invariants::check(&w);
    }
    assert_eq!(collateral_state(&w), before);
    w.observe_feed(T - SLACK_MS)
        .expect("a claim slack ms early is admitted");
    invariants::check(&w);
}

#[test]
fn set_collateral_still_validates_against_the_new_global_cap() {
    let mut w = configured();
    set_params(
        &mut w,
        EngineParamsArgs {
            global_cap: TYPE_CAP,
            ..World::engine_params()
        },
    )
    .expect("lowered to the type cap");
    let key = w.admin_key.clone();
    let set = w.set_collateral_msg(CollateralArgs {
        type_cap: TYPE_CAP + 1,
        ..World::collateral_args()
    });
    expect_revert(
        w.send_engine(w.admin, &[&key], &set),
        &fail_text(EngineError::TypeCapAboveGlobal),
    );
    invariants::check(&w);
}
