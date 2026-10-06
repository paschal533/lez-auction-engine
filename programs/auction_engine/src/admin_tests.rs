//! Handler-level tests of `init_config` / `set_engine_params`: every guard is exercised without the
//! guest, so each can be mutation-checked in seconds.

use auction_engine_core::{
    args::EngineParamsArgs,
    pda::{
        accounting_pda, buffer_pda, coll_config_pda, config_pda, unused_slot_pda,
        SLOT_PROTOCOL_TOKEN, SLOT_REF_ORACLE,
    },
    state::*,
    EngineError,
};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata},
    program::ProgramId,
};
use stablecoin_core::ProtocolParameters;

use crate::{
    accounts::ZERO_ID,
    admin::{init_config, set_engine_params, InitConfigAccounts, SetEngineParamsAccounts},
    EngineResult,
};

const ENGINE: ProgramId = [0xE1; 8];
const HOST: ProgramId = [0xA1; 8];
const TOKEN: ProgramId = [0xB1; 8];
const SALT: u64 = 1;
const ADMIN: AccountId = AccountId::new([0x10; 32]);
const STABLE: AccountId = AccountId::new([0x20; 32]);
const COLLATERAL: AccountId = AccountId::new([0x30; 32]);
const OTHER: AccountId = AccountId::new([0x40; 32]);
const ONE: u128 = 1_000_000_000_000_000_000_000_000_000;
const OLD_REF_PRICE: u128 = 100;

fn meta(id: AccountId, account: Account, signed: bool) -> AccountWithMetadata {
    AccountWithMetadata {
        account,
        is_authorized: signed,
        account_id: id,
    }
}

fn fresh(id: AccountId) -> AccountWithMetadata {
    meta(id, Account::default(), false)
}

fn host_params() -> ProtocolParameters {
    ProtocolParameters {
        admin_account_id: ADMIN,
        freeze_authority_account_id: ADMIN,
        stablecoin_definition_id: STABLE,
        collateral_definition_id: COLLATERAL,
        market_price_oracle_id: OTHER,
        stability_fee_per_millisecond: ONE,
        controller_proportional_gain: 0,
        controller_integral_gain: 0,
        minimum_collateralization_ratio: ONE,
        minimum_milliseconds_between_rate_updates: 0,
        maximum_oracle_price_age_milliseconds: 0,
        is_frozen: false,
    }
}

fn init_accounts(admin: AccountId) -> InitConfigAccounts {
    let cfg = config_pda(&ENGINE, &admin, SALT);
    let parameters = Account {
        program_owner: HOST,
        data: (&host_params()).into(),
        ..Account::default()
    };
    let definition = Account {
        program_owner: TOKEN,
        ..Account::default()
    };
    InitConfigAccounts {
        admin: meta(admin, Account::default(), true),
        config: fresh(cfg),
        accounting: fresh(accounting_pda(&ENGINE, &cfg)),
        buffer: fresh(buffer_pda(&ENGINE, &cfg)),
        protocol_parameters: meta(
            stablecoin_core::compute_protocol_parameters_pda(HOST),
            parameters,
            false,
        ),
        stablecoin_definition: meta(STABLE, definition, false),
        protocol_token_definition: fresh(unused_slot_pda(&ENGINE, &cfg, SLOT_PROTOCOL_TOKEN)),
        ref_oracle: fresh(unused_slot_pda(&ENGINE, &cfg, SLOT_REF_ORACLE)),
    }
}

fn init(a: InitConfigAccounts, p: &EngineParamsArgs) -> EngineResult {
    init_config(ENGINE, a, SALT, p)
}

fn rejected(r: EngineResult) -> EngineError {
    match r {
        Err(e) => e,
        Ok(_) => panic!("handler must reject"),
    }
}

#[test]
fn init_config_happy_path_writes_eight_post_states_and_inits_the_buffer() {
    let (post, calls) = init(
        init_accounts(ADMIN),
        &EngineParamsArgs::defaults_for_tests(),
    )
    .expect("ok");
    assert_eq!(post.len(), 8);
    assert_eq!(calls.len(), 1);
}

#[test]
fn init_config_requires_the_admin_signature() {
    let mut a = init_accounts(ADMIN);
    a.admin.is_authorized = false;
    assert_eq!(
        rejected(init(a, &EngineParamsArgs::defaults_for_tests())),
        EngineError::NotAdmin
    );
}

#[test]
fn init_config_rejects_the_config_address_of_another_admin() {
    let mut a = init_accounts(ADMIN);
    a.config.account_id = config_pda(&ENGINE, &OTHER, SALT);
    assert_eq!(
        rejected(init(a, &EngineParamsArgs::defaults_for_tests())),
        EngineError::WrongAccount("config")
    );
}

#[test]
fn init_config_rejects_an_initialised_config() {
    let mut a = init_accounts(ADMIN);
    a.config.account.program_owner = ENGINE;
    assert_eq!(
        rejected(init(a, &EngineParamsArgs::defaults_for_tests())),
        EngineError::WrongAccount("config")
    );
}

#[test]
fn init_config_rejects_a_zero_host_program() {
    let mut a = init_accounts(ADMIN);
    a.protocol_parameters.account.program_owner = [0; 8];
    assert_eq!(
        rejected(init(a, &EngineParamsArgs::defaults_for_tests())),
        EngineError::ZeroId
    );
}

#[test]
fn init_config_rejects_a_zero_token_program() {
    let mut a = init_accounts(ADMIN);
    a.stablecoin_definition.account.program_owner = [0; 8];
    assert_eq!(
        rejected(init(a, &EngineParamsArgs::defaults_for_tests())),
        EngineError::ZeroId
    );
}

#[test]
fn init_config_rejects_a_zero_collateral_definition() {
    let mut a = init_accounts(ADMIN);
    let mut pp = host_params();
    pp.collateral_definition_id = ZERO_ID;
    a.protocol_parameters.account.data = (&pp).into();
    assert_eq!(
        rejected(init(a, &EngineParamsArgs::defaults_for_tests())),
        EngineError::ZeroId
    );
}

#[test]
fn init_config_rejects_a_zero_protocol_token_id() {
    let enabled = EngineParamsArgs {
        protocol_token_enabled: true,
        ..EngineParamsArgs::defaults_for_tests()
    };
    let mut a = init_accounts(ADMIN);
    a.protocol_token_definition = fresh(ZERO_ID);
    assert_eq!(rejected(init(a, &enabled)), EngineError::ZeroId);
}

#[test]
fn init_config_rejects_overlapping_token_slots() {
    let enabled = EngineParamsArgs {
        protocol_token_enabled: true,
        ..EngineParamsArgs::defaults_for_tests()
    };
    let mut a = init_accounts(ADMIN);
    a.protocol_token_definition = fresh(COLLATERAL);
    assert_eq!(
        rejected(init(a, &enabled)),
        EngineError::WrongAccount("protocol_token")
    );
}

#[test]
fn init_config_validates_the_parameters() {
    let p = EngineParamsArgs {
        bad_debt_freeze_threshold: 499,
        ..EngineParamsArgs::defaults_for_tests()
    };
    assert_eq!(
        rejected(init(init_accounts(ADMIN), &p)),
        EngineError::ThresholdBelowDebtLot
    );
}

fn config_state() -> EngineConfigState {
    let system = EngineParamsArgs::defaults_for_tests()
        .validate()
        .expect("valid defaults");
    EngineConfigState {
        tag: TAG_CONFIG,
        salt: SALT,
        admin: ADMIN,
        host_program_id: HOST,
        host_protocol_parameters_id: OTHER,
        stablecoin_definition_id: STABLE,
        collateral_definition_id: COLLATERAL,
        token_program_id: TOKEN,
        protocol_token: None,
        protocol_token_ref_price: OLD_REF_PRICE,
        protocol_token_oracle_id: None,
        ref_oracle_max_age_ms: 3_600_000,
        global_cap: 10_000_000,
        global_reserved: 0,
        freeze_on_bad_debt: true,
        bad_debt_freeze_threshold: 1_000,
        system: SystemParamsRec::from_core(&system),
    }
}

fn coll_state(config: AccountId, type_cap: u128) -> CollateralConfigState {
    CollateralConfigState {
        tag: TAG_COLL,
        config,
        collateral_definition_id: COLLATERAL,
        oracle_id: OTHER,
        oracle_scale: 0,
        max_oracle_age_ms: 3_600_000,
        max_conf_bps: 100,
        band_lo_bps: 9_000,
        band_hi_bps: 11_000,
        penalty_bps: 1_000,
        tip: 0,
        chip_bps: 0,
        reward_cap: 0,
        debt_floor: 0,
        min_bid: 0,
        type_cap,
        type_reserved: 0,
        schedule: vec![SchedulePointRec {
            elapsed_ms: 0,
            discount_bps: 200,
        }],
        admin_paused: false,
        paused_since_ms: None,
        paused_total_ms: 0,
        last_seen_oracle_ts: None,
    }
}

fn set_accounts(
    signer: AccountId,
    signed: bool,
    type_cap: Option<u128>,
    accounting: &AccountingState,
) -> SetEngineParamsAccounts {
    let cfg_id = config_pda(&ENGINE, &ADMIN, SALT);
    let owned = |data| Account {
        program_owner: ENGINE,
        data,
        ..Account::default()
    };
    let coll = type_cap.map_or_else(Account::default, |cap| {
        owned((&coll_state(cfg_id, cap)).into())
    });
    SetEngineParamsAccounts {
        admin: meta(signer, Account::default(), signed),
        config: meta(cfg_id, owned((&config_state()).into()), false),
        accounting: meta(
            accounting_pda(&ENGINE, &cfg_id),
            owned(accounting.into()),
            false,
        ),
        coll_config: meta(coll_config_pda(&ENGINE, &cfg_id, &COLLATERAL), coll, false),
        protocol_token_definition: fresh(unused_slot_pda(&ENGINE, &cfg_id, SLOT_PROTOCOL_TOKEN)),
        ref_oracle: fresh(unused_slot_pda(&ENGINE, &cfg_id, SLOT_REF_ORACLE)),
    }
}

fn plain(signer: AccountId, signed: bool) -> SetEngineParamsAccounts {
    set_accounts(signer, signed, None, &AccountingState::new())
}

fn set(a: SetEngineParamsAccounts, p: &EngineParamsArgs) -> EngineResult {
    set_engine_params(ENGINE, a, p)
}

fn at_price(price: u128) -> EngineParamsArgs {
    EngineParamsArgs {
        protocol_token_ref_price: price,
        ..EngineParamsArgs::defaults_for_tests()
    }
}

#[test]
fn set_engine_params_is_admin_only() {
    assert_eq!(
        rejected(set(plain(OTHER, true), &at_price(OLD_REF_PRICE))),
        EngineError::NotAdmin
    );
    assert_eq!(
        rejected(set(plain(ADMIN, false), &at_price(OLD_REF_PRICE))),
        EngineError::NotAdmin
    );
    assert!(set(plain(ADMIN, true), &at_price(OLD_REF_PRICE)).is_ok());
}

#[test]
fn set_engine_params_bounds_the_ref_price_step_in_both_directions() {
    assert!(set(plain(ADMIN, true), &at_price(2 * OLD_REF_PRICE)).is_ok());
    assert_eq!(
        rejected(set(plain(ADMIN, true), &at_price(2 * OLD_REF_PRICE + 1))),
        EngineError::RefPriceStepTooLarge
    );
    assert!(set(plain(ADMIN, true), &at_price(OLD_REF_PRICE / 2)).is_ok());
    assert_eq!(
        rejected(set(plain(ADMIN, true), &at_price(OLD_REF_PRICE / 2 - 1))),
        EngineError::RefPriceStepTooLarge
    );
}

#[test]
fn set_engine_params_rechecks_global_cap_against_type_cap() {
    let acct = AccountingState::new();
    let low = EngineParamsArgs {
        global_cap: 4_999_999,
        ..at_price(OLD_REF_PRICE)
    };
    assert_eq!(
        rejected(set(set_accounts(ADMIN, true, Some(5_000_000), &acct), &low)),
        EngineError::TypeCapAboveGlobal
    );
    let equal = EngineParamsArgs {
        global_cap: 5_000_000,
        ..at_price(OLD_REF_PRICE)
    };
    assert!(set(set_accounts(ADMIN, true, Some(5_000_000), &acct), &equal).is_ok());
}

#[test]
fn set_engine_params_validates_the_parameters() {
    let p = EngineParamsArgs {
        bad_debt_freeze_threshold: 499,
        ..at_price(OLD_REF_PRICE)
    };
    assert_eq!(
        rejected(set(plain(ADMIN, true), &p)),
        EngineError::ThresholdBelowDebtLot
    );
}

#[test]
fn set_engine_params_rejects_overlapping_token_slots() {
    let p = EngineParamsArgs {
        protocol_token_enabled: true,
        ..at_price(OLD_REF_PRICE)
    };
    let mut a = plain(ADMIN, true);
    a.protocol_token_definition = fresh(STABLE);
    assert_eq!(
        rejected(set(a, &p)),
        EngineError::WrongAccount("protocol_token")
    );
}

#[test]
fn set_engine_params_locks_system_parameters_while_an_auction_is_live() {
    let live = AccountingState {
        surplus_live: true,
        ..AccountingState::new()
    };
    let changed = EngineParamsArgs {
        min_take: 11,
        ..at_price(OLD_REF_PRICE)
    };
    assert_eq!(
        rejected(set(set_accounts(ADMIN, true, None, &live), &changed)),
        EngineError::ProtocolTokenLocked
    );
    assert!(set(
        set_accounts(ADMIN, true, None, &live),
        &at_price(OLD_REF_PRICE)
    )
    .is_ok());
}

#[test]
fn init_config_rejects_a_zero_admin() {
    let mut a = init_accounts(ADMIN);
    a.admin.account_id = ZERO_ID;
    assert_eq!(
        rejected(init(a, &EngineParamsArgs::defaults_for_tests())),
        EngineError::ZeroId
    );
}

#[test]
fn init_config_rejects_a_parameters_account_that_is_not_the_host_pda() {
    let mut a = init_accounts(ADMIN);
    a.protocol_parameters.account_id = OTHER;
    assert_eq!(
        rejected(init(a, &EngineParamsArgs::defaults_for_tests())),
        EngineError::HostMismatch
    );
}
