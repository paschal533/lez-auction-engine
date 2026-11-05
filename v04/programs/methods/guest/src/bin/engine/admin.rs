//! `Init`, `SetEngineParams`, `SetSlack` and `SetCollateral` past the proxy (design note §4.2
//! rows 0-2; v0.2.4 `admin.rs` `init_config` / `set_engine_params` / `set_collateral_config`).
//!
//! - Config `K = config_actor(E, admin, salt)`: accepts only `from == (admin, E)` and re-derives
//!   its own address from that sender and the salt.
//! - Accounting `accounting_actor(E, K)` and collateral `collateral_actor(E, K, def)`: accept only
//!   their config, re-derived from the sender and checked against their own address. The config
//!   relays a validated `SetCollateral` as `Configure` with its observe context; the collateral
//!   actor writes the parameters and then observes with the new admin flag (row 2, ruling R-7b).
//! - `Init` also creates the protocol token's definition `(protocol_def(E, K), T)`, minter `K`:
//!   `K` sends `NewDefinition` with the PROTOCOL seed from `InitReads` (an R-8b exception: an
//!   engine seed on a message to the token program, whose only descendant is the `Credit` leaf).
//! - `SetEngineParams` at `K` writes the validated parameters, then asks its recorded collateral
//!   actor for `AssertCapAtMost` and, if a locked field changes, the accounting for
//!   `AssertSystemIdle`; both accept only their config and send nothing.
//! - `SetSlack` at `K` writes a bounded `slack_ms`, then relays it to its recorded collateral
//!   actor as `SlackUpdate`, which accepts it only from its config (ADR-064).

use auction_engine_params::{
    EngineError,
    args::{CollateralArgs, EngineParamsArgs, check_ref_price_step},
};
use engine_core::{
    actors::{
        accounting_actor, accounting_seed, collateral_actor, collateral_seed, config_actor,
        protocol_def, protocol_def_seed,
    },
    assertions,
    check::{expect_from, fail, slack_in_range},
    msg::{EngineMsg, ProbeKind, ProtocolGenesis},
    state::{
        AccountingState, CollateralState, ConfigState, ObserveCtx, SystemParamsRec, TAG_COLL,
        TAG_CONFIG, recs_from_schedule,
    },
};
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, SyncMessage},
};
use mock_host_core::HostMsg;
use oracle_core::OracleMsg;
use token_core::TokenMsg;

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
        ref_oracle,
        protocol_genesis,
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
        ref_oracle: *ref_oracle,
        protocol_genesis: *protocol_genesis,
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
    ref_oracle: Option<Actor>,
    protocol_genesis: Option<ProtocolGenesis>,
}

/// The reference-oracle slot (v0.2.4 `optional_slot` / `check_distinct_slots`): an actor iff
/// enabled (`ZeroId` when enabled without one, `WrongAccount("unused slot")` when given while
/// disabled), never zero, never at one of the `distinct` accounts (the stablecoin and the
/// protocol definitions) and never an actor of the token program `token`, so never the buffer's
/// holding a system op reads beside it (ruling R-3b; `WrongAccount("ref_oracle")`).
fn ref_oracle_slot(
    enabled: bool,
    actor: Option<Actor>,
    token: AccountId,
    distinct: &[AccountId],
) -> Option<Actor> {
    match (enabled, actor) {
        (false, None) => None,
        (true, None) => fail(EngineError::ZeroId),
        (false, Some(_)) => fail(EngineError::WrongAccount("unused slot")),
        (true, Some(oracle)) => {
            nonzero_actor(oracle);
            if oracle.program_account_id == token || distinct.contains(&oracle.account_id) {
                fail(EngineError::WrongAccount("ref_oracle"));
            }
            Some(oracle)
        }
    }
}

/// v0.2.4 `check_distinct_slots` (`WrongAccount("protocol_token")`): `def`, a stablecoin or
/// collateral definition, is never the receiving config's own engine-mintable protocol
/// definition `protocol_def(E, K)`, whether or not the protocol token is enabled yet.
fn not_the_protocol_def(input: &ReceiveInput, def: AccountId) {
    if def == protocol_def(engine(input), input.receiver.account_id) {
        fail(EngineError::WrongAccount("protocol_token"));
    }
}

/// The protocol token the config `k` names: its definition while enabled.
fn protocol_token(engine: AccountId, k: AccountId, enabled: bool) -> Option<AccountId> {
    enabled.then(|| protocol_def(engine, k))
}

/// Creates the config (v0.2.4 checks and codes) with its oracle and host reads pending, and sends
/// nothing: the seed it was granted stays with this message (ruling R-8b). `InitReads` follows.
fn init_config(input: &ReceiveInput, c: &InitConfig<'_>) -> Response {
    // Uninitialised = empty state; without the proxy's seed grant the PDA is not ours to create.
    if !input.pre_state.as_ref().is_empty() || !input.is_authorized {
        fail(EngineError::WrongAccount("config"));
    }
    let system = c.params.validate().unwrap_or_else(|err| fail(err));
    slack_in_range(c.slack_ms).unwrap_or_else(|err| fail(err));
    nonzero_actor(c.host);
    nonzero(c.token);
    nonzero(c.stable_def);
    nonzero_actor(c.oracle);
    nonzero_actor(c.clock);
    not_the_protocol_def(input, c.stable_def);
    let k = input.receiver.account_id;
    let ref_oracle = ref_oracle_slot(
        c.params.ref_oracle_enabled,
        c.ref_oracle,
        c.token,
        &[c.stable_def, protocol_def(engine(input), k)],
    );
    if let Some(genesis) = c.protocol_genesis {
        nonzero(genesis.to);
    }

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
        protocol_token: protocol_token(engine(input), k, c.params.protocol_token_enabled),
        protocol_token_ref_price: c.params.protocol_token_ref_price,
        protocol_token_oracle: ref_oracle,
        ref_oracle_max_age_ms: c.params.ref_oracle_max_age_ms,
        global_cap: c.params.global_cap,
        global_reserved: 0,
        freeze_on_bad_debt: c.params.freeze_on_bad_debt,
        bad_debt_freeze_threshold: c.params.bad_debt_freeze_threshold,
        system: SystemParamsRec::from_core(&system),
        pending_read: Some(c.oracle),
        pending_genesis: c.protocol_genesis,
        collateral_def: None,
        pending_host_read: true,
    };
    Response::set_state(state.encode())
}

/// `InitReads` from an engine actor: at the sender's accounting actor it is the config's
/// creation relay; at a config just created by `Init`, it is the admin proxy's continuation:
/// read the oracle and the host's parameters once each (ADR-065), create the accounting (ACCT seed,
/// a message that sends nothing), create the protocol definition (PROTOCOL seed; the genesis
/// scratch is consumed), and end with the `Proceed` sentinel.
pub fn on_init_reads(input: &ReceiveInput) -> Response {
    let from = engine_sender(input, EngineError::NotAdmin);
    let (e, k) = (engine(input), input.receiver);
    if k == accounting_actor(e, from) {
        return init_accounting(input);
    }
    let config = ConfigState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("config")));
    expect_from(input, Actor::new(config.admin, e), EngineError::NotAdmin);
    if k != config_actor(e, config.admin, config.salt)
        || config.pending_read != Some(config.oracle)
        || !config.pending_host_read
    {
        fail(EngineError::WrongAccount("config"));
    }
    let accounting = SyncMessage::new(accounting_actor(e, k.account_id), &EngineMsg::InitReads)
        .with_pda_seeds([accounting_seed(k.account_id)]);
    let genesis = config.pending_genesis;
    let new_definition = TokenMsg::NewDefinition {
        minter: Some(k),
        initial_to: genesis.map(|g| g.to),
        initial_supply: genesis.map_or(0, |g| g.supply),
    };
    let definition = Actor::new(protocol_def(e, k.account_id), config.token);
    let create = SyncMessage::new(definition, &new_definition)
        .with_pda_seeds([protocol_def_seed(k.account_id)]);
    let (oracle, host) = (config.oracle, config.host);
    let consumed = ConfigState {
        pending_genesis: None,
        ..config
    };
    Response::set_state(consumed.encode())
        .send_sync(oracle, &OracleMsg::ReadState)
        .send_sync(host, &HostMsg::ReadState)
        .send(accounting)
        .send(create)
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

/// `SetEngineParams` at the config, forwarded by the admin's proxy, in v0.2.4's order: admin,
/// `validate`, the reference-oracle slot (as at `Init`), the reference-price step, write. Then
/// the cap check of the config's recorded collateral type and, if the protocol token, the
/// reference oracle or the system parameters change, `AssertSystemIdle` at the accounting, so a
/// cap error is reported before the lock error, as in v0.2.4.
pub fn on_set_engine_params(input: &ReceiveInput, message: &EngineMsg) -> Response {
    let EngineMsg::SetEngineParams {
        salt,
        params,
        ref_oracle,
    } = message
    else {
        panic!("engine: BadMessage")
    };
    let admin = expect_admin_proxy(input, *salt);
    let config = ConfigState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("config")));
    if config.admin != admin {
        fail(EngineError::NotAdmin);
    }
    let next = updated_config(input, &config, params, *ref_oracle);
    let locked_fields_change = next.protocol_token != config.protocol_token
        || next.protocol_token_oracle != config.protocol_token_oracle
        || next.system != config.system;
    let capped = cap_check(input, Response::set_state(next.encode()), &next);
    if locked_fields_change {
        let accounting = accounting_actor(engine(input), input.receiver.account_id);
        capped.send_sync(accounting, &EngineMsg::AssertSystemIdle)
    } else {
        capped
    }
}

/// `SetSlack` at the config, forwarded by the admin's proxy: admin, bounds, write. Slack is not a
/// locked field, so there is no cap check and no `AssertSystemIdle` (ADR-064). With a recorded
/// collateral type it also relays `SlackUpdate` to the collateral actor, which keeps its own copy
/// of the slack in step.
pub fn on_set_slack(input: &ReceiveInput, message: &EngineMsg) -> Response {
    let EngineMsg::SetSlack { salt, slack_ms } = message else {
        panic!("engine: BadMessage")
    };
    let admin = expect_admin_proxy(input, *salt);
    let config = ConfigState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("config")));
    if config.admin != admin {
        fail(EngineError::NotAdmin);
    }
    slack_in_range(*slack_ms).unwrap_or_else(|err| fail(err));
    let next = ConfigState {
        slack_ms: *slack_ms,
        ..config
    };
    let response = Response::set_state(next.encode());
    match next.collateral_def {
        Some(def) => to_collateral(
            input,
            response,
            def,
            &EngineMsg::SlackUpdate {
                slack_ms: *slack_ms,
            },
        ),
        None => response,
    }
}

/// `SlackUpdate` at a collateral actor, from exactly its config: writes `ctx.slack_ms`, sends
/// nothing.
pub fn on_slack_update(input: &ReceiveInput, slack_ms: u64) -> Response {
    let coll = assertions::slack_update(input).unwrap_or_else(|err| fail(err));
    let next = CollateralState {
        ctx: ObserveCtx {
            slack_ms,
            ..coll.ctx
        },
        ..coll
    };
    Response::set_state(next.encode())
}

/// `config` with the validated `params` and reference oracle (v0.2.4 checks and codes).
fn updated_config(
    input: &ReceiveInput,
    config: &ConfigState,
    params: &EngineParamsArgs,
    ref_oracle: Option<Actor>,
) -> ConfigState {
    let system = params.validate().unwrap_or_else(|err| fail(err));
    let (e, k) = (engine(input), input.receiver.account_id);
    let ref_oracle = ref_oracle_slot(
        params.ref_oracle_enabled,
        ref_oracle,
        config.token,
        &[config.stablecoin_definition_id, protocol_def(e, k)],
    );
    check_ref_price_step(
        config.protocol_token_ref_price,
        params.protocol_token_ref_price,
    )
    .unwrap_or_else(|err| fail(err));
    ConfigState {
        protocol_token: protocol_token(e, k, params.protocol_token_enabled),
        protocol_token_ref_price: params.protocol_token_ref_price,
        protocol_token_oracle: ref_oracle,
        ref_oracle_max_age_ms: params.ref_oracle_max_age_ms,
        global_cap: params.global_cap,
        freeze_on_bad_debt: params.freeze_on_bad_debt,
        bad_debt_freeze_threshold: params.bad_debt_freeze_threshold,
        system: SystemParamsRec::from_core(&system),
        ..config.clone()
    }
}

/// Appends the cap check of the config's recorded collateral type (one per config, R-F3): a
/// seeded `Probe { Collateral }`, then the unseeded `AssertCapAtMost` with the new global cap
/// (R-8b). A config with no type yet has no cap to violate.
fn cap_check(input: &ReceiveInput, response: Response, config: &ConfigState) -> Response {
    let Some(def) = config.collateral_def else {
        return response;
    };
    let assert = EngineMsg::AssertCapAtMost {
        global_cap: config.global_cap,
    };
    to_collateral(input, response, def, &assert)
}

/// Appends `msg` for the config's collateral actor of type `def`: a seeded `Probe { Collateral }`,
/// then `msg` unseeded (R-8b).
fn to_collateral(
    input: &ReceiveInput,
    response: Response,
    def: AccountId,
    msg: &EngineMsg,
) -> Response {
    let k = input.receiver.account_id;
    let collateral = collateral_actor(engine(input), k, def);
    let probe = EngineMsg::Probe {
        kind: ProbeKind::Collateral,
    };
    response
        .send(SyncMessage::new(collateral, &probe).with_pda_seeds([collateral_seed(k, def)]))
        .send_sync(collateral, msg)
}

/// `AssertCapAtMost` at a collateral actor, from exactly its config (`assertions::cap_at_most`).
/// Writes and sends nothing.
pub fn on_assert_cap_at_most(input: &ReceiveInput, global_cap: u128) -> Response {
    assertions::cap_at_most(input, global_cap).unwrap_or_else(|err| fail(err));
    Response::keep_state()
}

/// `AssertSystemIdle` at an accounting actor, from exactly its config
/// (`assertions::system_idle`). Writes and sends nothing.
pub fn on_assert_system_idle(input: &ReceiveInput) -> Response {
    assertions::system_idle(input).unwrap_or_else(|err| fail(err));
    Response::keep_state()
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
    not_the_protocol_def(input, collateral_def);
    // One collateral type per config (ruling R-F3): the first `SetCollateral` records it.
    if config
        .collateral_def
        .is_some_and(|recorded| recorded != collateral_def)
    {
        fail(EngineError::WrongAccount("collateral_def"));
    }
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
    let recorded = if config.collateral_def.is_none() {
        let next = ConfigState {
            collateral_def: Some(collateral_def),
            ..config
        };
        Response::set_state(next.encode())
    } else {
        Response::keep_state()
    };
    recorded
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
