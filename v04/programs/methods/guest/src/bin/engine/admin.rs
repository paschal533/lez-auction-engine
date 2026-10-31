//! `Init` and `SetCollateral` past the proxy (design note §4.2 rows 0 and 2; v0.2.4
//! `admin.rs` `init_config` / `set_collateral_config`).
//!
//! - Config `K = config_actor(E, admin, salt)`: accepts only `from == (admin, E)` and re-derives
//!   its own address from that sender and the salt.
//! - Accounting `accounting_actor(E, K)` and collateral `collateral_actor(E, K, def)`: accept only
//!   their config, re-derived from the sender and checked against their own address.

use auction_engine_params::{
    EngineError,
    args::{CollateralArgs, EngineParamsArgs},
};
use engine_core::{
    actors::{accounting_actor, accounting_seed, collateral_actor, collateral_seed, config_actor},
    check::{expect_from, fail},
    msg::EngineMsg,
    state::{
        AccountingState, CollateralState, ConfigState, SystemParamsRec, TAG_COLL, TAG_CONFIG,
        recs_from_schedule,
    },
};
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, SyncMessage},
};
use oracle_core::OracleMsg;

fn engine(input: &ReceiveInput) -> AccountId {
    input.receiver.program_account_id
}

/// The sender's account; the sender must be an actor of this engine program.
fn engine_sender(input: &ReceiveInput, err: EngineError) -> AccountId {
    let from = input.from.unwrap_or_else(|| fail(err));
    expect_from(input, Actor::new(from.account_id, engine(input)), err);
    from.account_id
}

fn nonzero(id: AccountId) {
    if id == AccountId::new([0; 32]) {
        fail(EngineError::ZeroId);
    }
}

fn nonzero_actor(actor: Actor) {
    nonzero(actor.account_id);
    nonzero(actor.program_account_id);
}

/// At the config: the admin whose proxy `(admin, E)` sent the message, with the receiver being
/// exactly that admin's config for `salt`.
fn expect_admin_proxy(input: &ReceiveInput, salt: u64) -> AccountId {
    let admin = engine_sender(input, EngineError::NotAdmin);
    if input.receiver != config_actor(engine(input), admin, salt) {
        fail(EngineError::WrongAccount("config"));
    }
    admin
}

/// `Init` from an engine actor: at the sender's accounting actor it is the config's relay,
/// anywhere else it must be the admin proxy's forward to the config.
pub fn on_init(input: &ReceiveInput, message: &EngineMsg) -> Response {
    let from = engine_sender(input, EngineError::NotAdmin);
    if input.receiver == accounting_actor(engine(input), from) {
        return init_accounting(input);
    }
    let EngineMsg::Init {
        salt,
        params,
        host,
        token,
        stable_def,
        oracle,
        clock,
        slack_ms,
    } = message
    else {
        panic!("engine: BadMessage")
    };
    let admin = expect_admin_proxy(input, *salt);
    let config = InitConfig {
        admin,
        salt: *salt,
        params,
        host: *host,
        token: *token,
        stable_def: *stable_def,
        oracle: *oracle,
        clock: *clock,
        slack_ms: *slack_ms,
    };
    init_config(input, message, &config)
}

struct InitConfig<'a> {
    admin: AccountId,
    salt: u64,
    params: &'a EngineParamsArgs,
    host: Actor,
    token: AccountId,
    stable_def: AccountId,
    oracle: Actor,
    clock: Actor,
    slack_ms: u64,
}

/// Creates the config (v0.2.4 checks and codes), reads the oracle once, relays `Init` to the
/// accounting actor, and ends with the `Proceed` sentinel.
fn init_config(input: &ReceiveInput, message: &EngineMsg, c: &InitConfig<'_>) -> Response {
    // Uninitialised = empty state; without the proxy's seed grant the PDA is not ours to create.
    if !input.pre_state.as_ref().is_empty() || !input.is_authorized {
        fail(EngineError::WrongAccount("config"));
    }
    let system = c.params.validate().unwrap_or_else(|err| fail(err));
    // This slice's `Init` carries no protocol-token definition or reference oracle, so enabling
    // either names a zero id (v0.2.4 `optional_slot`).
    if c.params.protocol_token_enabled || c.params.ref_oracle_enabled {
        fail(EngineError::ZeroId);
    }
    nonzero_actor(c.host);
    nonzero(c.token);
    nonzero(c.stable_def);
    nonzero_actor(c.oracle);
    nonzero_actor(c.clock);

    let k = input.receiver;
    let state = ConfigState {
        tag: TAG_CONFIG,
        salt: c.salt,
        admin: c.admin,
        host: c.host,
        token: c.token,
        stablecoin_definition_id: c.stable_def,
        oracle: c.oracle,
        clock: c.clock,
        slack_ms: c.slack_ms,
        protocol_token: None,
        protocol_token_ref_price: c.params.protocol_token_ref_price,
        protocol_token_oracle: None,
        ref_oracle_max_age_ms: c.params.ref_oracle_max_age_ms,
        global_cap: c.params.global_cap,
        global_reserved: 0,
        freeze_on_bad_debt: c.params.freeze_on_bad_debt,
        bad_debt_freeze_threshold: c.params.bad_debt_freeze_threshold,
        system: SystemParamsRec::from_core(&system),
        pending_read: Some(c.oracle),
    };
    let accounting = SyncMessage::new(accounting_actor(engine(input), k.account_id), message)
        .with_pda_seeds([accounting_seed(k.account_id)]);
    Response::set_state(state.encode())
        .send_sync(c.oracle, &OracleMsg::ReadState)
        .send(accounting)
        .send_sync(k, &EngineMsg::Proceed)
}

/// At `accounting_actor(E, K)`, relayed by `K` with the ACCT seed: creates the accounting.
fn init_accounting(input: &ReceiveInput) -> Response {
    if !input.pre_state.as_ref().is_empty() || !input.is_authorized {
        fail(EngineError::WrongAccount("accounting"));
    }
    Response::set_state(AccountingState::new().encode())
}

/// `SetCollateral` from an engine actor: at the sender's collateral actor for `collateral_def`
/// it is the config's relay, anywhere else it must be the admin proxy's forward to the config.
pub fn on_set_collateral(
    input: &ReceiveInput,
    message: &EngineMsg,
    salt: u64,
    collateral_def: AccountId,
    args: &CollateralArgs,
) -> Response {
    let from = engine_sender(input, EngineError::NotAdmin);
    if input.receiver == collateral_actor(engine(input), from, collateral_def) {
        return write_collateral(input, from, collateral_def, args);
    }
    let admin = expect_admin_proxy(input, salt);
    let config = ConfigState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("config")));
    if config.admin != admin {
        fail(EngineError::NotAdmin);
    }
    nonzero(collateral_def);
    args.validate(config.global_cap)
        .unwrap_or_else(|err| fail(err));
    let k = input.receiver.account_id;
    let relay = SyncMessage::new(collateral_actor(engine(input), k, collateral_def), message)
        .with_pda_seeds([collateral_seed(k, collateral_def)]);
    Response::keep_state().send(relay)
}

/// At `collateral_actor(E, K, def)`, relayed by `K` (which validated `args` against its global
/// cap) with the COLL seed: creates or replaces the parameters, keeping the reservation and the
/// pause clock of an existing state.
fn write_collateral(
    input: &ReceiveInput,
    config: AccountId,
    collateral_def: AccountId,
    args: &CollateralArgs,
) -> Response {
    if !input.is_authorized {
        fail(EngineError::WrongAccount("coll_config"));
    }
    let base = if input.pre_state.as_ref().is_empty() {
        fresh_collateral(config, collateral_def)
    } else {
        CollateralState::decode(input.pre_state.as_ref())
            .filter(|state| {
                state.config == config && state.collateral_definition_id == collateral_def
            })
            .unwrap_or_else(|| fail(EngineError::WrongAccount("coll_config")))
    };
    // Already checked against the global cap at the config; this only parses.
    let (params, feed) = args.validate(u128::MAX).unwrap_or_else(|err| fail(err));
    let next = CollateralState {
        oracle_scale: args.oracle_scale,
        max_oracle_age_ms: feed.max_age_ms,
        max_conf_bps: feed.max_conf_bps,
        band_lo_bps: feed.band_lo_bps,
        band_hi_bps: feed.band_hi_bps,
        penalty_bps: params.penalty_bps,
        tip: params.tip,
        chip_bps: params.chip_bps,
        reward_cap: params.reward_cap,
        debt_floor: params.debt_floor,
        min_bid: params.min_bid,
        type_cap: args.type_cap,
        schedule: recs_from_schedule(&params.schedule),
        admin_paused: args.admin_paused,
        ..base
    };
    Response::set_state(next.encode())
}

fn fresh_collateral(config: AccountId, collateral_def: AccountId) -> CollateralState {
    CollateralState {
        tag: TAG_COLL,
        config,
        collateral_definition_id: collateral_def,
        oracle_scale: 0,
        max_oracle_age_ms: 0,
        max_conf_bps: 0,
        band_lo_bps: 0,
        band_hi_bps: 0,
        penalty_bps: 0,
        tip: 0,
        chip_bps: 0,
        reward_cap: 0,
        debt_floor: 0,
        min_bid: 0,
        type_cap: 0,
        type_reserved: 0,
        schedule: Vec::new(),
        admin_paused: false,
        paused_since_ms: None,
        paused_total_ms: 0,
        last_seen_oracle_ts: None,
    }
}
