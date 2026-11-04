//! Plans of the system instructions (design note §4.2 rows 1, 3, 9-16): `SetEngineParams`,
//! `ObserveFeed`, `SettleDebt`, the surplus and debt auctions and `CheckSolvency`.
//!
//! A plan declares the actors its run touches, plus the ones it *may* touch once state another
//! party can change has moved between planning and inclusion (ruling R-8b). An undeclared actor
//! reverts the whole transaction, and a revert is charged its whole gas limit (ADR-061), so an
//! exact prediction would let anyone make a keeper pay for nothing: crediting 1 unit of
//! stablecoin to the buffer (credits are permissionless) turns a no-burn settle into a burn, and
//! a booked shortfall turns a `Healthy` check into a `Freeze`, both through actors an exact plan
//! left out. Hence the may-touch actors, declared whatever today's state predicts:
//!
//! - the buffer's token actor and the stablecoin definition (the implicit settle's `Burn`) on
//!   `StartDebt`, `CheckSolvency` and a non-zero `SettleDebt` whenever the accounting carries
//!   unbacked debt, and on `StartSurplus` only when, in addition, the start simulates as
//!   successful (a refused start declares only what runs before the refusal);
//! - the bound host on `CheckSolvency` whenever the config can freeze (no protocol token, or
//!   `freeze_on_bad_debt`).
//!
//! So a 1-unit donation to the buffer cannot revert a pending `CheckSolvency` or `Start*` (the
//! griefing fix of ruling R-8b). One race remains: the burn pair is keyed on `unbacked_debt` at
//! planning time, so a plan built at zero leaves it out; if a shortfall is booked together with a
//! buffer credit before inclusion, the send reverts and is charged its gas limit (ADR-061), and
//! the keeper re-plans from fresh state.
//!
//! A declared actor the run does not reach costs nothing: it does not execute. Everything else is
//! exact. Where it depends on state the attacker cannot steer cheaply (a sweep only of a
//! non-empty vault, the accounting only if `SetEngineParams` changes a locked field, a start's
//! auction only if the start succeeds), the plan reads `source` and runs the engine's own pure
//! computation (`engine_core::system`) at the claimed time; a plan whose state then changes fails
//! its pre-send simulation rather than being sent. An op the computation refuses declares only
//! what runs before the refusal (and the may-touch actors), so the simulation reports the
//! engine's own error.
//!
//! Evidence (ADR-062): fresh holdings self-admit through the token's `HOLDING` seed, so the only
//! token evidence is `Pda { T, holding_seed(buffer, stable_def) }`, attached only by the plans
//! whose accounting reads the buffer's stablecoin holding (`SettleDebt`, `StartSurplus`,
//! `StartDebt`, `CheckSolvency`) and only while that holding's account is absent (Review Focus 1);
//! it then reads as 0. `CloseSurplus` credits the buffer, which a surplus start needed a balance
//! in, so it never needs evidence. The plans that claim time (`StartSurplus`, `StartDebt`,
//! `CheckSolvency`) also read the reference oracle, which may sit at an account nobody has funded
//! yet (final review Minor 1): while that account is absent they attach the deployment's
//! [`Deployment::ref_oracle_evidence`]. An oracle lives at a key account, so that is `Key`, whose
//! public key the chain does not record: the caller supplies it, and a plan without it fails
//! with [`SdkError::MissingState`]. The absent oracle then reads as unpublished (the admin price).

use auction_engine_params::args::EngineParamsArgs;
use engine_core::{
    SystemCtx,
    actors::{accounting_actor, collateral_actor, config_actor, protocol_def},
    msg::EngineMsg,
    state::{AccountingState, ConfigState, OracleReadingRec, SystemParamsRec},
    system::{ref_price, start_debt_op, start_surplus_op},
    system_state::RefReading,
};
use lee::{AccountId, Actor, PublicAccountEvidence};
use oracle_core::OracleState;
use token_core::{Holding, holding_seed};

use crate::{
    SdkError,
    builder::{Deployment, Plan},
    simulate::StateSource,
};

/// The fields of `SetEngineParams`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetEngineParamsArgs {
    pub salt: u64,
    pub params: EngineParamsArgs,
    /// The reference oracle, present iff `params.ref_oracle_enabled`.
    pub ref_oracle: Option<Actor>,
}

/// A take from a surplus or debt auction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TakeArgs {
    /// The taker, who signs (its tokens pay).
    pub taker: AccountId,
    pub nonce: u64,
    pub amount_coin: u128,
    /// `max_protocol_in` for a surplus take, `min_protocol_out` for a debt take.
    pub limit: u128,
    /// The clock claim, ms.
    pub now_ms: u64,
}

/// `SetEngineParams` at the admin's proxy, signed by the admin: proxy and config; the config's
/// recorded collateral actor (`AssertCapAtMost`) if it has one; the accounting
/// (`AssertSystemIdle`) if the protocol token, the reference oracle or the system parameters
/// change, as the engine decides it from the config's state in `source`.
///
/// # Errors
/// [`SdkError::MissingState`] if the config is absent or does not decode; [`SdkError::Encode`].
pub fn plan_set_engine_params(
    source: &impl StateSource,
    engine: AccountId,
    admin: AccountId,
    args: SetEngineParamsArgs,
) -> Result<Plan, SdkError> {
    let config = config_actor(engine, admin, args.salt);
    let state = config_state(source, config)?;
    let proxy = Actor::new(admin, engine);
    let mut actors = vec![proxy, config];
    actors.extend(
        state
            .collateral_def
            .map(|def| collateral_actor(engine, config.account_id, def)),
    );
    // A params set the engine refuses reverts at the config, before either assertion.
    if let Ok(system) = args.params.validate() {
        let next_token = args
            .params
            .protocol_token_enabled
            .then(|| protocol_def(engine, config.account_id));
        let locked_fields_change = next_token != state.protocol_token
            || args.ref_oracle != state.protocol_token_oracle
            || SystemParamsRec::from_core(&system) != state.system;
        if locked_fields_change {
            actors.push(accounting_actor(engine, config.account_id));
        }
    }
    let msg = EngineMsg::SetEngineParams {
        salt: args.salt,
        params: Box::new(args.params),
        ref_oracle: args.ref_oracle,
    };
    Plan::new(proxy, actors, vec![admin], Vec::new(), &msg)
}

/// `ObserveFeed` at the collateral actor, permissionless: collateral actor, host params, oracle,
/// clock.
///
/// # Errors
/// [`SdkError::Encode`] if the message does not encode.
pub fn plan_observe_feed(
    d: &Deployment,
    collateral_def: AccountId,
    now_ms: u64,
) -> Result<Plan, SdkError> {
    let root = d.collateral(collateral_def);
    let actors = vec![root, d.host, d.oracle, d.clock];
    let msg = EngineMsg::ObserveFeed {
        config: d.config.account_id,
        collateral_def,
        now_ms,
    };
    Plan::new(root, actors, Vec::new(), Vec::new(), &msg)
}

/// `SettleDebt` at the config, permissionless: config, accounting, the buffer's stablecoin
/// holding (read) and, if the settle may burn (a non-zero `amount` while the accounting carries
/// unbacked debt; see the module doc), the buffer's token actor and the stablecoin definition.
/// Buffer evidence while the holding is absent.
///
/// # Errors
/// [`SdkError::MissingState`] if the config, the accounting or the buffer holding does not
/// decode; [`SdkError::Encode`].
pub fn plan_settle_debt(
    source: &impl StateSource,
    d: &Deployment,
    amount: u128,
) -> Result<Plan, SdkError> {
    let reading = SystemReading::load(source, d, false)?;
    let mut actors = reading.base(d);
    if amount > 0 {
        actors.extend(reading.may_burn(d));
    }
    let msg = EngineMsg::SettleDebt {
        config: d.config.account_id,
        amount,
    };
    Plan::new(d.config, actors, Vec::new(), reading.evidence, &msg)
}

/// `StartSurplus` at the config, permissionless: config, accounting, the buffer's stablecoin
/// holding (read), the clock, the reference oracle (if configured); then, if the start succeeds,
/// the buffer's token actor (the lot's transfer), the stablecoin definition (if the implicit
/// settle may burn), the new auction and its vault's stablecoin holding (the lot's credit). Buffer
/// evidence while the holding is absent.
///
/// # Errors
/// As [`plan_settle_debt`].
pub fn plan_start_surplus(
    source: &impl StateSource,
    d: &Deployment,
    nonce: u64,
    now_ms: u64,
) -> Result<Plan, SdkError> {
    let reading = SystemReading::load(source, d, true)?;
    let mut actors = reading.base(d);
    let reference = reading.reference(now_ms);
    let started = start_surplus_op(
        &reading.acc,
        &reading.ctx,
        reading.buffer,
        reference,
        now_ms,
        nonce,
    );
    if started.is_ok() {
        // The lot's transfer debits through the buffer's token actor, whether or not it burns.
        let burn = reading.may_burn(d);
        if burn.is_empty() {
            actors.push(d.buffer());
        }
        actors.extend(burn);
        let vault = d.surplus_vault(nonce);
        actors.extend([
            d.surplus(nonce),
            d.holding_of(vault.account_id, d.stable_def),
        ]);
    }
    let msg = EngineMsg::StartSurplus {
        config: d.config.account_id,
        nonce,
        now_ms,
    };
    Plan::new(d.config, actors, Vec::new(), reading.evidence, &msg)
}

/// `TakeSurplus` at the config, signed by the taker: config, clock, the auction, its vault's
/// token actor and stablecoin holding, the accounting, the taker's token actor (the protocol
/// burn) with its protocol and stablecoin holdings, the protocol definition.
///
/// # Errors
/// [`SdkError::Encode`] if the message does not encode.
pub fn plan_take_surplus(d: &Deployment, args: TakeArgs) -> Result<Plan, SdkError> {
    let vault = d.surplus_vault(args.nonce);
    let actors = vec![
        d.config,
        d.clock,
        d.surplus(args.nonce),
        vault,
        d.holding_of(vault.account_id, d.stable_def),
        d.accounting,
        d.holding(args.taker),
        d.holding_of(args.taker, d.protocol_def),
        d.holding(d.protocol_def),
        d.holding_of(args.taker, d.stable_def),
    ];
    let msg = EngineMsg::TakeSurplus {
        taker: args.taker,
        config: d.config.account_id,
        nonce: args.nonce,
        amount_coin: args.amount_coin,
        max_protocol_in: args.limit,
        now_ms: args.now_ms,
    };
    Plan::new(d.config, actors, vec![args.taker], Vec::new(), &msg)
}

/// `CloseSurplus` at the config, permissionless: config, clock, the auction, its vault's
/// stablecoin holding (read), the accounting; and, if the vault holds anything, the vault's
/// token actor and the buffer's stablecoin holding (the sweep). No evidence.
///
/// # Errors
/// [`SdkError::MissingState`] if the vault holding does not decode; [`SdkError::Encode`].
pub fn plan_close_surplus(
    source: &impl StateSource,
    d: &Deployment,
    nonce: u64,
    now_ms: u64,
) -> Result<Plan, SdkError> {
    let vault = d.surplus_vault(nonce);
    let vault_stable = d.holding_of(vault.account_id, d.stable_def);
    let mut actors = vec![
        d.config,
        d.clock,
        d.surplus(nonce),
        vault_stable,
        d.accounting,
    ];
    if balance(source, vault_stable)? > 0 {
        actors.extend([vault, d.holding_of(d.buffer().account_id, d.stable_def)]);
    }
    let msg = EngineMsg::CloseSurplus {
        config: d.config.account_id,
        nonce,
        now_ms,
    };
    Plan::new(d.config, actors, Vec::new(), Vec::new(), &msg)
}

/// `StartDebt` at the config, permissionless: config, accounting, the buffer's stablecoin
/// holding (read), the clock, the reference oracle (if configured), the buffer's token actor and
/// the stablecoin definition (if the implicit settle may burn); then, if the start succeeds, the
/// new auction. Buffer evidence while the holding is absent.
///
/// # Errors
/// As [`plan_settle_debt`].
pub fn plan_start_debt(
    source: &impl StateSource,
    d: &Deployment,
    nonce: u64,
    now_ms: u64,
) -> Result<Plan, SdkError> {
    let reading = SystemReading::load(source, d, true)?;
    let mut actors = reading.base(d);
    actors.extend(reading.may_burn(d));
    let reference = reading.reference(now_ms);
    let started = start_debt_op(
        &reading.acc,
        &reading.ctx,
        reading.buffer,
        reference,
        now_ms,
        nonce,
    );
    if started.is_ok() {
        actors.push(d.debt(nonce));
    }
    let msg = EngineMsg::StartDebt {
        config: d.config.account_id,
        nonce,
        now_ms,
    };
    Plan::new(d.config, actors, Vec::new(), reading.evidence, &msg)
}

/// `TakeDebt` at the config, signed by the taker: config, clock, the auction, the accounting,
/// the taker's token actor and stablecoin holding, the stablecoin definition (the burn), the
/// protocol definition and the taker's protocol holding (the mint through the config).
///
/// # Errors
/// [`SdkError::Encode`] if the message does not encode.
pub fn plan_take_debt(d: &Deployment, args: TakeArgs) -> Result<Plan, SdkError> {
    let actors = vec![
        d.config,
        d.clock,
        d.debt(args.nonce),
        d.accounting,
        d.holding(args.taker),
        d.holding_of(args.taker, d.stable_def),
        d.holding(d.stable_def),
        d.holding(d.protocol_def),
        d.holding_of(args.taker, d.protocol_def),
    ];
    let msg = EngineMsg::TakeDebt {
        taker: args.taker,
        config: d.config.account_id,
        nonce: args.nonce,
        amount_coin: args.amount_coin,
        min_protocol_out: args.limit,
        now_ms: args.now_ms,
    };
    Plan::new(d.config, actors, vec![args.taker], Vec::new(), &msg)
}

/// `CloseDebt` at the config, permissionless: config, clock, the auction, the accounting.
///
/// # Errors
/// [`SdkError::Encode`] if the message does not encode.
pub fn plan_close_debt(d: &Deployment, nonce: u64, now_ms: u64) -> Result<Plan, SdkError> {
    let actors = vec![d.config, d.clock, d.debt(nonce), d.accounting];
    let msg = EngineMsg::CloseDebt {
        config: d.config.account_id,
        nonce,
        now_ms,
    };
    Plan::new(d.config, actors, Vec::new(), Vec::new(), &msg)
}

/// `CheckSolvency` at the config, permissionless: config, accounting, the buffer's stablecoin
/// holding (read), the clock, the reference oracle (if configured); the buffer's token actor and
/// the stablecoin definition if the implicit settle may burn; the bound host whenever the config
/// can freeze (routed finding 1, ruling R-8b: without a protocol token the threshold alone
/// freezes, ADR-017; with one only if `freeze_on_bad_debt`), whatever the decision on today's
/// state, so a later donation or booked shortfall cannot revert the check. Buffer evidence while
/// the holding is absent.
///
/// # Errors
/// As [`plan_settle_debt`].
pub fn plan_check_solvency(
    source: &impl StateSource,
    d: &Deployment,
    now_ms: u64,
) -> Result<Plan, SdkError> {
    let reading = SystemReading::load(source, d, true)?;
    let mut actors = reading.base(d);
    actors.extend(reading.may_burn(d));
    if reading.ctx.protocol_token.is_none() || reading.ctx.freeze_on_bad_debt {
        actors.push(reading.config.host);
    }
    let msg = EngineMsg::CheckSolvency {
        config: d.config.account_id,
        now_ms,
    };
    Plan::new(d.config, actors, Vec::new(), reading.evidence, &msg)
}

/// The evidence a system plan that reads the buffer needs: `Pda` for the buffer's stablecoin
/// holding (the token program's `HOLDING` seed) while its account is absent, else none.
#[must_use]
pub fn buffer_evidence(source: &impl StateSource, d: &Deployment) -> Vec<PublicAccountEvidence> {
    let buffer = d.buffer().account_id;
    if source.account_exists(d.holding_of(buffer, d.stable_def).account_id) {
        return Vec::new();
    }
    vec![PublicAccountEvidence::Pda {
        program: d.token,
        seed: holding_seed(buffer, d.stable_def),
    }]
}

/// The evidence a timed system plan needs for the config's reference oracle `oracle`: none
/// without one or while its account exists; else `supplied` (the deployment's
/// [`Deployment::ref_oracle_evidence`]), which must prove that account. The accounting reads an
/// absent oracle as `Undecodable` (an empty reply), so the plan then prices at the admin price.
fn ref_oracle_evidence(
    source: &impl StateSource,
    oracle: Option<Actor>,
    supplied: Option<&PublicAccountEvidence>,
) -> Result<Vec<PublicAccountEvidence>, SdkError> {
    let Some(oracle) = oracle else {
        return Ok(Vec::new());
    };
    if source.account_exists(oracle.account_id) {
        return Ok(Vec::new());
    }
    match supplied {
        Some(evidence) if evidence.account_id() == oracle.account_id => Ok(vec![evidence.clone()]),
        _ => Err(SdkError::MissingState(format!(
            "the reference oracle's account {} is absent: supply its evidence \
             (Deployment::with_ref_oracle_evidence)",
            oracle.account_id
        ))),
    }
}

/// The reference oracle's reading as the accounting takes it: `Undecodable` unless `oracle`
/// holds an oracle state.
fn ref_reading(source: &impl StateSource, oracle: Actor) -> RefReading {
    source
        .actor_state(oracle)
        .and_then(|bytes| OracleState::decode(&bytes))
        .map_or(RefReading::Undecodable, |o| {
            RefReading::Valid(OracleReadingRec {
                price_raw: o.price_ray,
                confidence_raw: o.confidence,
                timestamp_ms: o.timestamp_ms,
            })
        })
}

/// What the accounting reads for a system op, from `source`: the config (its context), the
/// accounting, the buffer's balance (0 while absent) and the reference oracle's state.
struct SystemReading {
    config: ConfigState,
    ctx: SystemCtx,
    acc: AccountingState,
    buffer: u128,
    reference: Option<RefReading>,
    evidence: Vec<PublicAccountEvidence>,
    /// Whether the op claims time, and so reads the clock and the reference oracle.
    timed: bool,
}

impl SystemReading {
    /// The reading for an op that claims time iff `timed`: its evidence is the buffer's and, if
    /// `timed`, the reference oracle's.
    fn load(source: &impl StateSource, d: &Deployment, timed: bool) -> Result<Self, SdkError> {
        let config = config_state(source, d.config)?;
        let ctx = SystemCtx::from_config(d.config.account_id, &config);
        let acc = source
            .actor_state(d.accounting)
            .and_then(|bytes| AccountingState::decode(&bytes))
            .ok_or_else(|| SdkError::MissingState("no engine accounting".into()))?;
        let buffer = balance(source, d.holding_of(d.buffer().account_id, d.stable_def))?;
        let reference = ctx.ref_oracle.map(|oracle| ref_reading(source, oracle));
        let mut evidence = buffer_evidence(source, d);
        if timed {
            let supplied = d.ref_oracle_evidence.as_ref();
            evidence.extend(ref_oracle_evidence(source, ctx.ref_oracle, supplied)?);
        }
        Ok(Self {
            config,
            ctx,
            acc,
            buffer,
            reference,
            evidence,
            timed,
        })
    }

    /// The reference price the accounting computes at `now_ms`.
    fn reference(&self, now_ms: u64) -> u128 {
        ref_price(&self.ctx, self.reference, now_ms)
    }

    /// The buffer's token actor and the stablecoin definition while the accounting carries
    /// unbacked debt: an implicit settle (or a `SettleDebt`) may then burn from the buffer, since
    /// anyone can credit it between planning and inclusion (ruling R-8b, may-touch).
    fn may_burn(&self, d: &Deployment) -> Vec<Actor> {
        if self.acc.unbacked_debt == 0 {
            return Vec::new();
        }
        vec![d.buffer(), d.holding(d.stable_def)]
    }

    /// The actors every run of the op touches: config, accounting, the buffer's stablecoin
    /// holding; for an op that claims time, the clock and the reference oracle.
    fn base(&self, d: &Deployment) -> Vec<Actor> {
        let mut actors = vec![
            d.config,
            d.accounting,
            d.holding_of(d.buffer().account_id, d.stable_def),
        ];
        if self.timed {
            actors.push(d.clock);
            actors.extend(self.ctx.ref_oracle);
        }
        actors
    }
}

/// The config state at `config`.
fn config_state(source: &impl StateSource, config: Actor) -> Result<ConfigState, SdkError> {
    source
        .actor_state(config)
        .and_then(|bytes| ConfigState::decode(&bytes))
        .ok_or_else(|| SdkError::MissingState(format!("no engine config at {}", config.account_id)))
}

/// The balance of the holding `holding`, 0 while absent.
fn balance(source: &impl StateSource, holding: Actor) -> Result<u128, SdkError> {
    let Some(bytes) = source.actor_state(holding) else {
        return Ok(0);
    };
    Holding::decode(&bytes)
        .map(|h| h.balance)
        .ok_or_else(|| SdkError::MissingState(format!("{} holds no holding", holding.account_id)))
}
