//! `Init` and `SetCollateral` past the proxy (design note §4.2 rows 0 and 2; v0.2.4
//! `admin.rs` `init_config` / `set_collateral_config`).
//!
//! - Config `K = config_actor(E, admin, salt)`: accepts only `from == (admin, E)` and re-derives
//!   its own address from that sender and the salt.
//! - Accounting `accounting_actor(E, K)` and collateral `collateral_actor(E, K, def)`: accept only
//!   their config, re-derived from the sender and checked against their own address. The config
//!   relays a validated `SetCollateral` as `Configure` with its observe context; the collateral
//!   actor writes the parameters and then observes with the new admin flag (row 2, ruling R-7b).

use auction_engine_params::{
    EngineError,
    args::{CollateralArgs, EngineParamsArgs},
};
use engine_core::{
    actors::{accounting_actor, accounting_seed, collateral_actor, collateral_seed, config_actor},
    check::{expect_from, fail},
    msg::EngineMsg,
    state::{
        AccountingState, CollateralState, ConfigState, ObserveCtx, SystemParamsRec, TAG_COLL,
        TAG_CONFIG, recs_from_schedule,
    },
};
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, SyncMessage},
};
use oracle_core::OracleMsg;

/// The engine program `E`: the receiver's program.
pub fn engine(input: &ReceiveInput) -> AccountId {
    input.receiver.program_account_id
}

/// The sender's account; the sender must be an actor of this engine program.
pub fn engine_sender(input: &ReceiveInput, err: EngineError) -> AccountId {
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

/// `Init` at the config, forwarded by the admin's proxy with the CONFIG seed.
pub fn on_init(input: &ReceiveInput, message: &EngineMsg) -> Response {
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
    init_config(input, &config)
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

/// Creates the config (v0.2.4 checks and codes) with its oracle read pending, and sends nothing:
/// the seed it was granted stays with this message (ruling R-8b). `InitReads` follows.
fn init_config(input: &ReceiveInput, c: &InitConfig<'_>) -> Response {
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
    Response::set_state(state.encode())
}

/// `InitReads` from an engine actor: at the sender's accounting actor it is the config's
/// creation relay; at a config just created by `Init`, it is the admin proxy's continuation:
/// read the oracle once, create the accounting (ACCT seed, a message that sends nothing), and end
/// with the `Proceed` sentinel.
pub fn on_init_reads(input: &ReceiveInput) -> Response {
    let from = engine_sender(input, EngineError::NotAdmin);
    let (e, k) = (engine(input), input.receiver);
    if k == accounting_actor(e, from) {
        return init_accounting(input);
    }
    let config = ConfigState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("config")));
    expect_from(input, Actor::new(config.admin, e), EngineError::NotAdmin);
    if k != config_actor(e, config.admin, config.salt) || config.pending_read != Some(config.oracle)
    {
        fail(EngineError::WrongAccount("config"));
    }
    let accounting = SyncMessage::new(accounting_actor(e, k.account_id), &EngineMsg::InitReads)
        .with_pda_seeds([accounting_seed(k.account_id)]);
    Response::keep_state()
        .send_sync(config.oracle, &OracleMsg::ReadState)
        .send(accounting)
        .send_sync(k, &EngineMsg::Proceed)
}

/// At `accounting_actor(E, K)`, relayed by `K` with the ACCT seed: creates the accounting and
/// sends nothing.
fn init_accounting(input: &ReceiveInput) -> Response {
    if !input.pre_state.as_ref().is_empty() || !input.is_authorized {
        fail(EngineError::WrongAccount("accounting"));
    }
    Response::set_state(AccountingState::new().encode())
}

/// `SetCollateral` at the config, forwarded by the admin's proxy: checks the admin and the
/// arguments (v0.2.4 codes), then relays `Configure` with the config's observe context to
/// `collateral_actor(E, K, def)` with the COLL seed (it creates the actor and sends nothing),
/// followed by the unseeded `Observe` (ruling R-8b).
pub fn on_set_collateral(
    input: &ReceiveInput,
    salt: u64,
    collateral_def: AccountId,
    args: &CollateralArgs,
    now_ms: u64,
) -> Response {
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
    let configure = EngineMsg::Configure {
        collateral_def,
        args: args.clone(),
        ctx: ObserveCtx {
            host: config.host,
            oracle: config.oracle,
            clock: config.clock,
            slack_ms: config.slack_ms,
        },
    };
    let collateral = collateral_actor(engine(input), k, collateral_def);
    let relay = SyncMessage::new(collateral, &configure)
        .with_pda_seeds([collateral_seed(k, collateral_def)]);
    Response::keep_state()
        .send(relay)
        .send_sync(collateral, &EngineMsg::Observe { now_ms })
}

/// `Configure` at `collateral_actor(E, K, def)`, relayed by `K` (which validated `args` against
/// its global cap) with the COLL seed: creates or replaces the parameters, keeping the
/// reservation and the pause clock of an existing state. Sends nothing; the config's `Observe`
/// follows.
pub fn on_configure(
    input: &ReceiveInput,
    collateral_def: AccountId,
    args: &CollateralArgs,
    ctx: ObserveCtx,
) -> Response {
    let config = engine_sender(input, EngineError::NotAdmin);
    if input.receiver != collateral_actor(engine(input), config, collateral_def)
        || !input.is_authorized
    {
        fail(EngineError::WrongAccount("coll_config"));
    }
    let base = if input.pre_state.as_ref().is_empty() {
        fresh_collateral(config, collateral_def, ctx)
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
        ctx,
        ..base
    };
    if next.pending.is_some() {
        fail(EngineError::WrongAccount("pending"));
    }
    Response::set_state(next.encode())
}

fn fresh_collateral(
    config: AccountId,
    collateral_def: AccountId,
    ctx: ObserveCtx,
) -> CollateralState {
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
        ctx,
        pending: None,
    }
}
