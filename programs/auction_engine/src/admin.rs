//! Admin instructions. No admin path moves auction collateral, buffer funds or system-auction funds.

use auction_engine_core::{
    args::{check_ref_price_step, EngineParamsArgs},
    pda::{self, SLOT_PROTOCOL_TOKEN, SLOT_REF_ORACLE},
    state::*,
    EngineError,
};
use lee_core::{account::AccountWithMetadata, program::ProgramId};
use stablecoin_core::{compute_protocol_parameters_pda, ProtocolParameters};

use crate::{accounts::*, token_calls, EngineResult};

pub struct InitConfigAccounts {
    pub admin: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    pub buffer: AccountWithMetadata,
    pub protocol_parameters: AccountWithMetadata,
    pub stablecoin_definition: AccountWithMetadata,
    pub protocol_token_definition: AccountWithMetadata,
    pub ref_oracle: AccountWithMetadata,
}

/// Creates `EngineConfig`, `Accounting` and the `SurplusBuffer` holding. Permissionless: the signer
/// becomes this config's admin; a host trusts a config only after its admin binds it (ADR-027).
pub fn init_config(
    program_id: ProgramId,
    a: InitConfigAccounts,
    salt: u64,
    p: &EngineParamsArgs,
) -> EngineResult {
    if !a.admin.is_authorized {
        return Err(EngineError::NotAdmin);
    }
    nonzero_id(&a.admin.account_id)?;
    let system = p.validate()?;
    // ADR-027 (I7): the config PDA includes its admin, so nobody can squat another admin's address.
    expect_id(
        &a.config,
        &pda::config_pda(&program_id, &a.admin.account_id, salt),
        "config",
    )?;
    expect_default(&a.config, "config")?;
    let config_id = a.config.account_id;
    expect_id(
        &a.accounting,
        &pda::accounting_pda(&program_id, &config_id),
        "accounting",
    )?;
    expect_default(&a.accounting, "accounting")?;
    expect_id(
        &a.buffer,
        &pda::buffer_pda(&program_id, &config_id),
        "buffer",
    )?;
    expect_default(&a.buffer, "buffer")?;

    // ADR-030: the host program id is the ProtocolParameters owner, checked against its PDA.
    let host_program_id = a.protocol_parameters.account.program_owner;
    nonzero_program(&host_program_id)?;
    if a.protocol_parameters.account_id != compute_protocol_parameters_pda(host_program_id) {
        return Err(EngineError::HostMismatch);
    }
    let params = ProtocolParameters::try_from(&a.protocol_parameters.account.data)
        .map_err(|_| EngineError::HostMismatch)?;
    expect_id(
        &a.stablecoin_definition,
        &params.stablecoin_definition_id,
        "stablecoin_definition",
    )?;
    let token_program_id = a.stablecoin_definition.account.program_owner;
    nonzero_program(&token_program_id)?;
    nonzero_id(&params.collateral_definition_id)?;
    let protocol_token = optional_slot(
        &program_id,
        &config_id,
        SLOT_PROTOCOL_TOKEN,
        p.protocol_token_enabled,
        &a.protocol_token_definition,
    )?;
    let ref_oracle = optional_slot(
        &program_id,
        &config_id,
        SLOT_REF_ORACLE,
        p.ref_oracle_enabled,
        &a.ref_oracle,
    )?;
    check_distinct_slots(
        protocol_token,
        ref_oracle,
        &params.stablecoin_definition_id,
        &params.collateral_definition_id,
    )?;

    let cfg = EngineConfigState {
        tag: TAG_CONFIG,
        salt,
        admin: a.admin.account_id,
        host_program_id,
        host_protocol_parameters_id: a.protocol_parameters.account_id,
        stablecoin_definition_id: params.stablecoin_definition_id,
        collateral_definition_id: params.collateral_definition_id,
        token_program_id,
        protocol_token,
        protocol_token_ref_price: p.protocol_token_ref_price,
        protocol_token_oracle_id: ref_oracle,
        ref_oracle_max_age_ms: p.ref_oracle_max_age_ms,
        global_cap: p.global_cap,
        global_reserved: 0,
        freeze_on_bad_debt: p.freeze_on_bad_debt,
        bad_debt_freeze_threshold: p.bad_debt_freeze_threshold,
        system: SystemParamsRec::from_core(&system),
    };
    let post = vec![
        echo(&a.admin),
        create(&a.config, &cfg, pda::config_seed(&a.admin.account_id, salt)),
        create(
            &a.accounting,
            &AccountingState::new(),
            pda::accounting_seed(&config_id),
        ),
        echo(&a.buffer),
        echo(&a.protocol_parameters),
        echo(&a.stablecoin_definition),
        echo(&a.protocol_token_definition),
        echo(&a.ref_oracle),
    ];
    let init_buffer = token_calls::initialize_account(
        token_program_id,
        a.stablecoin_definition.clone(),
        token_calls::authorized(&a.buffer),
        vec![pda::buffer_seed(&config_id)],
    );
    Ok((post, vec![init_buffer]))
}

pub struct SetEngineParamsAccounts {
    pub admin: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    /// Read only to re-check `global_cap >= type_cap` (M7); may still be uninitialised.
    pub coll_config: AccountWithMetadata,
    pub protocol_token_definition: AccountWithMetadata,
    pub ref_oracle: AccountWithMetadata,
}

/// Admin update. Admin, host binding, token program, salt and `global_reserved` are immutable (the admin
/// is part of the config PDA). While a surplus or debt auction is live, the protocol token, its oracle
/// and the system-auction parameters cannot change (`ProtocolTokenLocked`): live takes read them. Each
/// change of the reference price stays within [old/2, 2·old] (`RefPriceStepTooLarge`, ADR-031), and
/// `global_cap` may not drop below the collateral type cap (`TypeCapAboveGlobal`).
pub fn set_engine_params(
    program_id: ProgramId,
    a: SetEngineParamsAccounts,
    p: &EngineParamsArgs,
) -> EngineResult {
    let cfg = load_config(&program_id, &a.config)?;
    require_admin(&a.admin, &cfg)?;
    let system = p.validate()?;
    let config_id = a.config.account_id;
    let acc = load_accounting(&program_id, &config_id, &a.accounting)?;
    let protocol_token = optional_slot(
        &program_id,
        &config_id,
        SLOT_PROTOCOL_TOKEN,
        p.protocol_token_enabled,
        &a.protocol_token_definition,
    )?;
    let ref_oracle = optional_slot(
        &program_id,
        &config_id,
        SLOT_REF_ORACLE,
        p.ref_oracle_enabled,
        &a.ref_oracle,
    )?;
    check_distinct_slots(
        protocol_token,
        ref_oracle,
        &cfg.stablecoin_definition_id,
        &cfg.collateral_definition_id,
    )?;
    check_ref_price_step(cfg.protocol_token_ref_price, p.protocol_token_ref_price)?;
    expect_id(
        &a.coll_config,
        &pda::coll_config_pda(&program_id, &config_id, &cfg.collateral_definition_id),
        "coll_config",
    )?;
    if a.coll_config.account != lee_core::account::Account::default() {
        let coll = load_coll(&program_id, &config_id, &cfg, &a.coll_config)?;
        if coll.type_cap > p.global_cap {
            return Err(EngineError::TypeCapAboveGlobal);
        }
    }
    let system_rec = SystemParamsRec::from_core(&system);
    let locked_fields_change = protocol_token != cfg.protocol_token
        || ref_oracle != cfg.protocol_token_oracle_id
        || system_rec != cfg.system;
    if locked_fields_change && (acc.surplus_live || acc.debt_live) {
        return Err(EngineError::ProtocolTokenLocked);
    }
    let next = EngineConfigState {
        protocol_token,
        protocol_token_ref_price: p.protocol_token_ref_price,
        protocol_token_oracle_id: ref_oracle,
        ref_oracle_max_age_ms: p.ref_oracle_max_age_ms,
        global_cap: p.global_cap,
        freeze_on_bad_debt: p.freeze_on_bad_debt,
        bad_debt_freeze_threshold: p.bad_debt_freeze_threshold,
        system: system_rec,
        ..cfg
    };
    Ok((
        vec![
            echo(&a.admin),
            write(&a.config, &next),
            echo(&a.accounting),
            echo(&a.coll_config),
            echo(&a.protocol_token_definition),
            echo(&a.ref_oracle),
        ],
        vec![],
    ))
}
