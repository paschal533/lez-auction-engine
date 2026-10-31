//! Engine `Init` and `SetCollateral` through the admin's signer proxy (design note §3, §4.2 rows
//! 0 and 2): the proxy forwards only a signed root message, the config accepts only its own
//! admin's proxy, and the accounting and collateral actors accept only their config.

use auction_engine_params::{
    EngineError,
    args::{CollateralArgs, EngineParamsArgs},
};
use engine_core::{
    actors::config_actor,
    check::fail_text,
    msg::EngineMsg,
    state::{
        AccountingState, CollateralState, ConfigState, SchedulePointRec, SystemParamsRec, TAG_COLL,
        TAG_CONFIG,
    },
};
use harness::{
    World, expect_revert,
    world::{Forward, MIN_CR_RAY, SALT, SLACK_MS},
};
use lee::{AccountId, Actor};
use mock_host_core::{Params, Position, is_unsafe};
use oracle_core::OracleState;

fn config_state(w: &World, at: Actor) -> Option<ConfigState> {
    w.chain
        .raw_state(at)
        .map(|bytes| ConfigState::decode(&bytes).expect("a config state"))
}

fn collateral_state(w: &World) -> Option<CollateralState> {
    w.chain
        .raw_state(w.collateral)
        .map(|bytes| CollateralState::decode(&bytes).expect("a collateral state"))
}

fn accounting_state(w: &World) -> Option<AccountingState> {
    w.chain
        .raw_state(w.accounting)
        .map(|bytes| AccountingState::decode(&bytes).expect("an accounting state"))
}

/// The config `Init` with `World::engine_params()` must write, spelled out field by field.
fn expected_config(w: &World) -> ConfigState {
    let p = World::engine_params();
    let system = p.validate().expect("valid parameters");
    ConfigState {
        tag: TAG_CONFIG,
        salt: SALT,
        admin: w.admin,
        host: w.params,
        token: w.token,
        stablecoin_definition_id: w.stable_def,
        oracle: w.oracle,
        clock: w.chain.clock(),
        slack_ms: SLACK_MS,
        protocol_token: None,
        protocol_token_ref_price: p.protocol_token_ref_price,
        protocol_token_oracle: None,
        ref_oracle_max_age_ms: p.ref_oracle_max_age_ms,
        global_cap: p.global_cap,
        global_reserved: 0,
        freeze_on_bad_debt: p.freeze_on_bad_debt,
        bad_debt_freeze_threshold: p.bad_debt_freeze_threshold,
        system: SystemParamsRec::from_core(&system),
        pending_read: None,
    }
}

/// The collateral state a first `SetCollateral` with `CollateralArgs::defaults_for_tests()`
/// writes (values copied from those defaults; schedule words `[0, 200, 2_700_000, 1_000]`).
fn expected_collateral(w: &World) -> CollateralState {
    CollateralState {
        tag: TAG_COLL,
        config: w.config.account_id,
        collateral_definition_id: w.collateral_def,
        oracle_scale: 0,
        max_oracle_age_ms: 3_600_000,
        max_conf_bps: 100,
        band_lo_bps: 9_000,
        band_hi_bps: 11_000,
        penalty_bps: 1_000,
        tip: 5,
        chip_bps: 10,
        reward_cap: 50,
        debt_floor: 100,
        min_bid: 25,
        type_cap: 5_000_000,
        type_reserved: 0,
        schedule: vec![
            SchedulePointRec {
                elapsed_ms: 0,
                discount_bps: 200,
            },
            SchedulePointRec {
                elapsed_ms: 2_700_000,
                discount_bps: 1_000,
            },
        ],
        admin_paused: false,
        paused_since_ms: None,
        paused_total_ms: 0,
        last_seen_oracle_ts: None,
    }
}

/// `World::bare()` with the admin's `Init` applied.
fn initialised() -> World {
    let mut w = World::bare();
    let (key, init) = (w.admin_key.clone(), w.init_msg());
    w.send_engine(w.admin, &[&key], &init)
        .expect("the admin initialises");
    w
}

#[test]
fn init_creates_config_and_accounting() {
    let mut w = World::bare();
    assert_eq!(config_state(&w, w.config), None);
    assert_eq!(accounting_state(&w), None);

    let (key, init) = (w.admin_key.clone(), w.init_msg());
    w.send_engine(w.admin, &[&key], &init)
        .expect("the admin initialises");

    assert_eq!(config_state(&w, w.config), Some(expected_config(&w)));
    assert_eq!(accounting_state(&w), Some(AccountingState::new()));
    // The proxy is stateless and no collateral exists yet.
    let proxy = Actor::new(w.admin, w.engine);
    assert_eq!(w.chain.raw_state(proxy), None);
    assert_eq!(collateral_state(&w), None);
}

#[test]
fn init_is_refused_without_the_admin_signature() {
    let mut w = World::bare();
    let init = w.init_msg();
    expect_revert(
        w.send_engine(w.admin, &[], &init),
        &fail_text(EngineError::NotAdmin),
    );
    // A root `Init` straight at the (not yet existing) config PDA: LEZ admits no message to a
    // fresh account that nothing authorized, so it fails before the engine runs.
    let config = w.config;
    let actors = w.proxy_actors(w.admin, &init);
    expect_revert(
        w.chain.send(config, &actors, &[], &init),
        "UnadmittedPublicActor",
    );
    assert_eq!(config_state(&w, w.config), None);
    assert_eq!(accounting_state(&w), None);
}

#[test]
fn init_twice_reverts() {
    let mut w = initialised();
    let before = config_state(&w, w.config);

    let key = w.admin_key.clone();
    let again = w.init_msg_with(EngineParamsArgs {
        global_cap: 7,
        ..World::engine_params()
    });
    expect_revert(
        w.send_engine(w.admin, &[&key], &again),
        &fail_text(EngineError::WrongAccount("config")),
    );
    assert_eq!(config_state(&w, w.config), before);
    assert_eq!(accounting_state(&w), Some(AccountingState::new()));
}

#[test]
fn squatter_cannot_init_another_admins_config() {
    let mut w = initialised();
    let before = config_state(&w, w.config);
    let (squatter_key, squatter) = w.chain.key("squatter");
    w.chain.fund(squatter, 1);
    let init = w.init_msg();

    // The squatter signs, but the root is at the admin's proxy: not authorized there.
    expect_revert(
        w.send_engine(w.admin, &[&squatter_key], &init),
        &fail_text(EngineError::NotAdmin),
    );
    // A foreign program's actor at the admin's account is not the admin's proxy.
    let relay = w.chain.program("relay");
    let forged = Actor::new(w.admin, relay);
    let mut actors = w.proxy_actors(w.admin, &init);
    actors.push(forged);
    expect_revert(
        w.chain
            .send(forged, &actors, &[], &Forward::new(w.config, &init)),
        &fail_text(EngineError::NotAdmin),
    );
    // With the admin's salt at its own proxy, the squatter gets its own, different config.
    w.send_engine(squatter, &[&squatter_key], &init)
        .expect("the squatter initialises its own config");
    let own = config_actor(w.engine, squatter, SALT);
    assert_ne!(own, w.config);
    assert_eq!(
        config_state(&w, own).map(|state| state.admin),
        Some(squatter)
    );
    assert_eq!(config_state(&w, w.config), before);

    // Nor can the squatter configure the admin's collateral.
    let set = w.set_collateral_msg(World::collateral_args());
    expect_revert(
        w.send_engine(w.admin, &[&squatter_key], &set),
        &fail_text(EngineError::NotAdmin),
    );
    assert_eq!(collateral_state(&w), None);
}

#[test]
fn set_collateral_only_through_the_admin_proxy() {
    // Before `Init` there is no config to forward to.
    let mut w = World::bare();
    let key = w.admin_key.clone();
    let set = w.set_collateral_msg(World::collateral_args());
    expect_revert(
        w.send_engine(w.admin, &[&key], &set),
        &fail_text(EngineError::WrongAccount("config")),
    );

    let mut w = initialised();
    let key = w.admin_key.clone();
    // Unsigned at the proxy, or a root straight at the config: not authorized.
    expect_revert(
        w.send_engine(w.admin, &[], &set),
        &fail_text(EngineError::NotAdmin),
    );
    let (config, collateral) = (w.config, w.collateral);
    let actors = w.proxy_actors(w.admin, &set);
    expect_revert(
        w.chain.send(config, &actors, &[], &set),
        &fail_text(EngineError::NotAdmin),
    );
    // Straight at the collateral PDA before it exists: LEZ refuses the unadmitted account.
    expect_revert(
        w.chain.send(collateral, &actors, &[], &set),
        "UnadmittedPublicActor",
    );
    // A stranger's own proxy forwards to the stranger's (empty) config.
    let (stranger_key, stranger) = w.chain.key("stranger");
    w.chain.fund(stranger, 1);
    expect_revert(
        w.send_engine(stranger, &[&stranger_key], &set),
        &fail_text(EngineError::WrongAccount("config")),
    );
    assert_eq!(collateral_state(&w), None);

    w.send_engine(w.admin, &[&key], &set)
        .expect("the admin configures the collateral");
    assert_eq!(collateral_state(&w), Some(expected_collateral(&w)));
    // Once it exists, a root straight at the collateral actor reaches the engine, which refuses.
    expect_revert(
        w.chain.send(collateral, &actors, &[], &set),
        &fail_text(EngineError::NotAdmin),
    );

    // A second `SetCollateral` replaces the parameters in place.
    let update = w.set_collateral_msg(CollateralArgs {
        penalty_bps: 1_500,
        admin_paused: true,
        ..World::collateral_args()
    });
    w.send_engine(w.admin, &[&key], &update)
        .expect("the admin updates the collateral");
    assert_eq!(
        collateral_state(&w),
        Some(CollateralState {
            penalty_bps: 1_500,
            admin_paused: true,
            ..expected_collateral(&w)
        })
    );
}

#[test]
fn invalid_params_revert_with_v024_error_codes() {
    let mut w = World::bare();
    let key = w.admin_key.clone();
    let ok = World::engine_params();
    let bad_params = [
        (
            EngineParamsArgs {
                bad_debt_freeze_threshold: 499,
                ..ok.clone()
            },
            EngineError::ThresholdBelowDebtLot,
        ),
        (
            EngineParamsArgs {
                protocol_token_ref_price: 0,
                ..ok.clone()
            },
            EngineError::ZeroRefPrice,
        ),
        (
            EngineParamsArgs {
                system_ttl_ms: 1,
                ..ok.clone()
            },
            EngineError::TtlOutOfRange,
        ),
        (
            EngineParamsArgs {
                surplus_premium_bps: 10_001,
                ..ok.clone()
            },
            EngineError::BpsOutOfRange,
        ),
        (
            EngineParamsArgs {
                debt_growth: vec![0],
                ..ok.clone()
            },
            EngineError::ScheduleEncoding,
        ),
        (
            EngineParamsArgs {
                global_cap: 0,
                ..ok.clone()
            },
            EngineError::InvalidConfig,
        ),
        // `Init` carries no protocol-token definition, so enabling one names a zero id.
        (
            EngineParamsArgs {
                protocol_token_enabled: true,
                ..ok.clone()
            },
            EngineError::ZeroId,
        ),
    ];
    for (params, err) in bad_params {
        let init = w.init_msg_with(params);
        expect_revert(w.send_engine(w.admin, &[&key], &init), &fail_text(err));
    }
    let mut zero_oracle = w.init_msg();
    if let EngineMsg::Init { oracle, .. } = &mut zero_oracle {
        *oracle = Actor::new(AccountId::new([0; 32]), w.oracle.program_account_id);
    }
    expect_revert(
        w.send_engine(w.admin, &[&key], &zero_oracle),
        &fail_text(EngineError::ZeroId),
    );
    // An "oracle" that never replies (a relay actor stores the read and stays silent): the
    // Read block's `Proceed` sentinel finds the read still pending.
    let mut not_an_oracle = w.init_msg();
    if let EngineMsg::Init { oracle, .. } = &mut not_an_oracle {
        *oracle = w.publisher;
    }
    expect_revert(
        w.send_engine(w.admin, &[&key], &not_an_oracle),
        &fail_text(EngineError::OracleMismatch),
    );
    assert_eq!(config_state(&w, w.config), None);

    let init = w.init_msg();
    w.send_engine(w.admin, &[&key], &init)
        .expect("the admin initialises");
    let ok = World::collateral_args();
    let bad_args = [
        (
            CollateralArgs {
                chip_bps: 1_001,
                ..ok.clone()
            },
            EngineError::ChipAbovePenalty,
        ),
        (
            CollateralArgs {
                type_cap: World::engine_params().global_cap + 1,
                ..ok.clone()
            },
            EngineError::TypeCapAboveGlobal,
        ),
        (
            CollateralArgs {
                type_cap: 0,
                ..ok.clone()
            },
            EngineError::InvalidConfig,
        ),
        (
            CollateralArgs {
                max_conf_bps: 10_001,
                ..ok.clone()
            },
            EngineError::BpsOutOfRange,
        ),
        (
            CollateralArgs {
                schedule: vec![0, 200, 1],
                ..ok
            },
            EngineError::ScheduleEncoding,
        ),
    ];
    for (args, err) in bad_args {
        let set = w.set_collateral_msg(args);
        expect_revert(w.send_engine(w.admin, &[&key], &set), &fail_text(err));
    }
    assert_eq!(collateral_state(&w), None);
}

#[test]
fn world_binds_the_config_and_opens_an_unsafe_position() {
    let mut w = World::new();
    assert_eq!(config_state(&w, w.config), Some(expected_config(&w)));
    assert_eq!(collateral_state(&w), Some(expected_collateral(&w)));
    let params = w
        .chain
        .raw_state(w.params)
        .and_then(|bytes| Params::decode(&bytes))
        .expect("host params");
    assert_eq!(params.engine_actor, Some(w.config), "bound to (config, E)");

    let position = w.open_unsafe_position(1_000, 600);
    let at = Actor::new(position, w.host_prog);
    let state = w
        .chain
        .raw_state(at)
        .and_then(|bytes| Position::decode(&bytes))
        .expect("a position");
    assert_eq!((state.collateral, state.normalized_debt), (1_000, 600));
    let oracle = w
        .chain
        .raw_state(w.oracle)
        .and_then(|bytes| OracleState::decode(&bytes))
        .expect("an oracle");
    assert!(is_unsafe(1_000, 600, oracle.price_ray, MIN_CR_RAY));
    let second = w.open_unsafe_position(2_000, 900);
    assert_ne!(second, position, "each call opens a fresh owner's position");
}
