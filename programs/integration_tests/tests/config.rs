mod common;

use auction_engine_core::{args::EngineParamsArgs, pda, state::*};
use common::*;
use lee_core::account::AccountId;
use token_core::TokenHolding;

#[test]
fn init_config_creates_config_accounting_and_buffer() {
    let mut w = World::new();
    w.init_engine_config(&EngineParamsArgs::defaults_for_tests())
        .expect("init_config");
    let cfg: EngineConfigState = w.read(config());
    assert_eq!(cfg.host_program_id, host_program());
    assert_eq!(cfg.host_protocol_parameters_id, protocol_parameters());
    assert_eq!(cfg.stablecoin_definition_id, stable_def());
    assert_eq!(cfg.collateral_definition_id, collateral_def());
    assert_eq!(cfg.token_program_id, token_program());
    assert_eq!(cfg.admin, id(k::ENGINE_ADMIN));
    assert_eq!(
        (
            cfg.global_reserved,
            cfg.protocol_token,
            cfg.protocol_token_oracle_id
        ),
        (0, None, None)
    );
    let acc: AccountingState = w.read(accounting());
    assert_eq!(acc, AccountingState::new());
    let buffer_holding: TokenHolding = w.read(buffer());
    assert_eq!(
        buffer_holding,
        TokenHolding::Fungible {
            definition_id: stable_def(),
            balance: 0
        }
    );
    assert_eq!(
        w.state.get_account_by_id(buffer()).program_owner,
        token_program()
    );
}

#[test]
fn init_config_twice_is_rejected() {
    let mut w = World::new();
    w.init_engine_config(&EngineParamsArgs::defaults_for_tests())
        .unwrap();
    assert_err(
        w.init_engine_config(&EngineParamsArgs::defaults_for_tests()),
        "AccountAlreadyInitialized",
    );
}

#[test]
fn zero_ref_price_is_rejected() {
    let mut w = World::new();
    let p = EngineParamsArgs {
        protocol_token_ref_price: 0,
        ..EngineParamsArgs::defaults_for_tests()
    };
    assert_err(w.init_engine_config(&p), "ZeroRefPrice");
}

#[test]
fn threshold_below_debt_lot_is_rejected() {
    let mut w = World::new();
    let p = EngineParamsArgs {
        bad_debt_freeze_threshold: 499,
        ..EngineParamsArgs::defaults_for_tests()
    };
    assert_err(w.init_engine_config(&p), "ThresholdBelowDebtLot");
}

#[test]
fn invalid_system_params_are_rejected() {
    let mut w = World::new();
    let p = EngineParamsArgs {
        min_take: 501,
        ..EngineParamsArgs::defaults_for_tests()
    };
    assert_err(w.init_engine_config(&p), "InvalidConfig");
}

#[test]
fn wrong_host_parameters_account_is_rejected() {
    let mut w = World::new();
    let p = EngineParamsArgs::defaults_for_tests();
    let r = w.engine(
        vec![
            id(k::ENGINE_ADMIN),
            config(),
            accounting(),
            buffer(),
            accumulator(),
            stable_def(),
            unused(1),
            unused(0),
        ],
        &[k::ENGINE_ADMIN],
        init_config_ix(SALT, &p),
    );
    assert_err(r, "HostMismatch");
}

#[test]
fn disabled_slot_must_be_the_placeholder() {
    let mut w = World::new();
    let p = EngineParamsArgs::defaults_for_tests();
    let r = w.engine(
        vec![
            id(k::ENGINE_ADMIN),
            config(),
            accounting(),
            buffer(),
            protocol_parameters(),
            stable_def(),
            id(k::ATTACKER),
            unused(0),
        ],
        &[k::ENGINE_ADMIN],
        init_config_ix(SALT, &p),
    );
    assert_err(r, "WrongAccount");
}

#[test]
fn zero_protocol_token_id_is_rejected() {
    let mut w = World::new();
    let p = EngineParamsArgs {
        protocol_token_enabled: true,
        ..EngineParamsArgs::defaults_for_tests()
    };
    let r = w.engine(
        vec![
            id(k::ENGINE_ADMIN),
            config(),
            accounting(),
            buffer(),
            protocol_parameters(),
            stable_def(),
            AccountId::new([0; 32]),
            unused(0),
        ],
        &[k::ENGINE_ADMIN],
        init_config_ix(SALT, &p),
    );
    assert_err(r, "ZeroId");
}

#[test]
fn set_engine_params_admin_only_and_keeps_binding() {
    let mut w = World::new();
    w.init_engine_config(&EngineParamsArgs::defaults_for_tests())
        .unwrap();
    let p = EngineParamsArgs {
        global_cap: 20_000_000,
        ..EngineParamsArgs::defaults_for_tests()
    };
    assert_err(w.set_engine_params(k::ATTACKER, &p), "NotAdmin");
    w.set_engine_params(k::ENGINE_ADMIN, &p)
        .expect("admin updates params");
    let cfg: EngineConfigState = w.read(config());
    assert_eq!(cfg.global_cap, 20_000_000);
    assert_eq!(cfg.host_program_id, host_program());
}

#[test]
fn ref_price_steps_are_bounded_to_half_and_double() {
    // I5 / ADR-031: each admin change of the reference price stays within [old/2, 2·old].
    let mut w = World::new();
    w.init_engine_config(&EngineParamsArgs::defaults_for_tests())
        .unwrap();
    let at = |price: u128| EngineParamsArgs {
        protocol_token_ref_price: price,
        ..EngineParamsArgs::defaults_for_tests()
    };
    assert_err(
        w.set_engine_params(k::ENGINE_ADMIN, &at(4 * ONE + 1)),
        "RefPriceStepTooLarge",
    );
    w.set_engine_params(k::ENGINE_ADMIN, &at(4 * ONE))
        .expect("exactly 2x is allowed");
    assert_err(
        w.set_engine_params(k::ENGINE_ADMIN, &at(2 * ONE - 1)),
        "RefPriceStepTooLarge",
    );
    w.set_engine_params(k::ENGINE_ADMIN, &at(2 * ONE))
        .expect("exactly 1/2 is allowed");
}

#[test]
fn protocol_token_must_differ_from_the_collateral() {
    // M7: protocol token ∉ {stablecoin, collateral, reference oracle}.
    let mut w = World::new();
    let p = EngineParamsArgs {
        protocol_token_enabled: true,
        ..EngineParamsArgs::defaults_for_tests()
    };
    let r = w.engine(
        vec![
            id(k::ENGINE_ADMIN),
            config(),
            accounting(),
            buffer(),
            protocol_parameters(),
            stable_def(),
            collateral_def(),
            unused(0),
        ],
        &[k::ENGINE_ADMIN],
        init_config_ix(SALT, &p),
    );
    assert_err(r, "WrongAccount");
}

#[test]
fn nobody_can_squat_the_admins_config_address() {
    // I7 / ADR-027: the config PDA includes its admin, so another signer cannot create it first.
    let mut w = World::new();
    let e = engine_program();
    let r = w.engine(
        vec![
            id(k::ATTACKER),
            config(),
            accounting(),
            buffer(),
            protocol_parameters(),
            stable_def(),
            unused(1),
            unused(0),
        ],
        &[k::ATTACKER],
        init_config_ix(SALT, &EngineParamsArgs::defaults_for_tests()),
    );
    assert_err(r, "PdaMismatch");
    assert_ne!(pda::config_pda(&e, &id(k::ATTACKER), SALT), config());
}
