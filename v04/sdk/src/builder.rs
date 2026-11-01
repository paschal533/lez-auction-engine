//! Plans: the root actor, declared actors, signers, admission evidence and message bytes of one
//! engine instruction, built from `engine_core::actors` and the deployment read from state.
//!
//! Evidence rule (settlement admits signers ∪ accounts that exist ∪ accounts proven public by
//! evidence): a plan attaches `PublicAccountEvidence::Pda { program: E, seed }` only for a token
//! holding the engine creates at one of its PDAs while that account is fresh (ruling R-5b): the
//! auction vault on `Liquidate` (always fresh: the auction number is new) and the surplus buffer
//! on a `Bid` while the buffer account does not exist yet (read from the `StateSource` the caller
//! passes to [`plan_bid`]). Engine actors written by their own seed-granted messages need none
//! (S9), nor do signers' own holdings (R-3a). No plan in this slice touches a fresh non-signer
//! plain account, so none carries `PublicAccountEvidence::Key`.

use auction_engine_params::args::{CollateralArgs, EngineParamsArgs};
use engine_core::{
    actors::{
        accounting_actor, auction_actor, buffer_holding, buffer_seed, collateral_actor,
        config_actor, record_actor, vault_holding, vault_seed,
    },
    msg::EngineMsg,
    state::ConfigState,
};
use lee::{AccountId, Actor, PublicAccountEvidence};

use crate::{SdkError, simulate::StateSource};

/// One engine instruction ready to simulate and send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The actor the root message is delivered to.
    pub root: Actor,
    /// Every actor the conversation touches, the root first.
    pub actors: Vec<Actor>,
    /// The accounts that must sign besides the fee payer.
    pub signers: Vec<AccountId>,
    /// Admission evidence for fresh accounts settlement would otherwise not admit.
    pub evidence: Vec<PublicAccountEvidence>,
    /// The borsh encoding of the root [`EngineMsg`].
    pub message: Vec<u8>,
}

impl Plan {
    fn new(
        root: Actor,
        actors: Vec<Actor>,
        signers: Vec<AccountId>,
        evidence: Vec<PublicAccountEvidence>,
        msg: &EngineMsg,
    ) -> Result<Self, SdkError> {
        let message = borsh::to_vec(msg).map_err(|err| SdkError::Encode(err.to_string()))?;
        Ok(Self {
            root,
            actors,
            signers,
            evidence,
            message,
        })
    }
}

/// An initialised engine configuration as its config actor records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deployment {
    /// The engine program `E`.
    pub engine: AccountId,
    /// The config actor `K = config_actor(E, admin, salt)`.
    pub config: Actor,
    pub admin: AccountId,
    pub salt: u64,
    /// The bound host's params actor.
    pub host: Actor,
    /// The token program `T`.
    pub token: AccountId,
    pub stable_def: AccountId,
    pub oracle: Actor,
    pub clock: Actor,
    /// `accounting_actor(E, config)`.
    pub accounting: Actor,
}

impl Deployment {
    /// Reads the config state at `(config, engine)` from `source`.
    ///
    /// # Errors
    /// [`SdkError::MissingState`] if the config is absent, does not decode, or is not the config
    /// actor its own admin and salt derive.
    pub fn load(
        source: &impl StateSource,
        engine: AccountId,
        config: AccountId,
    ) -> Result<Self, SdkError> {
        let actor = Actor::new(config, engine);
        let bytes = source
            .actor_state(actor)
            .ok_or_else(|| SdkError::MissingState(format!("no engine config at {config}")))?;
        let state = ConfigState::decode(&bytes)
            .ok_or_else(|| SdkError::MissingState(format!("{config} holds no engine config")))?;
        if config_actor(engine, state.admin, state.salt) != actor {
            return Err(SdkError::MissingState(format!(
                "{config} is not the config of its admin and salt"
            )));
        }
        Ok(Self {
            engine,
            config: actor,
            admin: state.admin,
            salt: state.salt,
            host: state.host,
            token: state.token,
            stable_def: state.stablecoin_definition_id,
            oracle: state.oracle,
            clock: state.clock,
            accounting: accounting_actor(engine, config),
        })
    }

    /// The collateral actor of `collateral_def`.
    #[must_use]
    pub fn collateral(&self, collateral_def: AccountId) -> Actor {
        collateral_actor(self.engine, self.config.account_id, collateral_def)
    }

    /// The auction `nonce` of `position`.
    #[must_use]
    pub fn auction(&self, position: AccountId, nonce: u64) -> Actor {
        auction_actor(self.engine, self.config.account_id, position, nonce)
    }

    /// The vault holding of auction `nonce` of `position`.
    #[must_use]
    pub fn vault(&self, position: AccountId, nonce: u64) -> Actor {
        let auction = self.auction(position, nonce).account_id;
        vault_holding(self.engine, self.token, auction)
    }

    /// The surplus buffer holding.
    #[must_use]
    pub fn buffer(&self) -> Actor {
        buffer_holding(self.engine, self.token, self.config.account_id)
    }

    /// `account`'s own token holding `(account, T)`.
    #[must_use]
    pub fn holding(&self, account: AccountId) -> Actor {
        Actor::new(account, self.token)
    }

    /// The signer's proxy `(signer, E)`.
    #[must_use]
    pub fn proxy(&self, signer: AccountId) -> Actor {
        Actor::new(signer, self.engine)
    }
}

/// The fields of `Init` (the config does not exist yet, so nothing is read from state).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitArgs {
    pub salt: u64,
    pub params: EngineParamsArgs,
    pub host: Actor,
    pub token: AccountId,
    pub stable_def: AccountId,
    pub oracle: Actor,
    pub clock: Actor,
    pub slack_ms: u64,
}

/// The keeper's `Liquidate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiquidateArgs {
    pub keeper: AccountId,
    pub collateral_def: AccountId,
    pub position: AccountId,
    /// The expected auction number.
    pub nonce: u64,
    /// The clock claim, ms.
    pub now_ms: u64,
}

/// A bidder's `Bid`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BidArgs {
    pub bidder: AccountId,
    pub collateral_def: AccountId,
    pub position: AccountId,
    pub nonce: u64,
    pub max_debt_in: u128,
    pub min_collateral_out: u128,
    /// The clock claim, ms.
    pub now_ms: u64,
}

/// `Init` at the admin's proxy `(admin, E)`, signed by the admin: proxy, config, oracle,
/// accounting.
///
/// # Errors
/// [`SdkError::Encode`] if the message does not encode.
pub fn plan_init(engine: AccountId, admin: AccountId, args: InitArgs) -> Result<Plan, SdkError> {
    let config = config_actor(engine, admin, args.salt);
    let accounting = accounting_actor(engine, config.account_id);
    let proxy = Actor::new(admin, engine);
    let actors = vec![proxy, config, args.oracle, accounting];
    let msg = EngineMsg::Init {
        salt: args.salt,
        params: Box::new(args.params),
        host: args.host,
        token: args.token,
        stable_def: args.stable_def,
        oracle: args.oracle,
        clock: args.clock,
        slack_ms: args.slack_ms,
    };
    Plan::new(proxy, actors, vec![admin], Vec::new(), &msg)
}

/// `SetCollateral` at the admin's proxy, signed by the admin: proxy, config, collateral actor,
/// and what the collateral actor observes (host params, oracle, clock).
///
/// # Errors
/// [`SdkError::Encode`] if the message does not encode.
pub fn plan_set_collateral(
    d: &Deployment,
    collateral_def: AccountId,
    args: CollateralArgs,
    now_ms: u64,
) -> Result<Plan, SdkError> {
    let proxy = d.proxy(d.admin);
    let actors = vec![
        proxy,
        d.config,
        d.collateral(collateral_def),
        d.host,
        d.oracle,
        d.clock,
    ];
    let msg = EngineMsg::SetCollateral {
        salt: d.salt,
        collateral_def,
        args,
        now_ms,
    };
    Plan::new(proxy, actors, vec![d.admin], Vec::new(), &msg)
}

/// `Liquidate` at the keeper's proxy, signed by the keeper. `host_actors` are the actors the
/// bound host touches when it seizes the position (host-specific; for the mock host: the
/// position `(position, H)` and the owner's host vault holding). Evidence: the auction vault,
/// a fresh engine PDA the token `Credit` creates (R-5b).
///
/// # Errors
/// [`SdkError::Encode`] if the message does not encode.
pub fn plan_liquidate(
    d: &Deployment,
    args: &LiquidateArgs,
    host_actors: &[Actor],
) -> Result<Plan, SdkError> {
    let proxy = d.proxy(args.keeper);
    let auction = d.auction(args.position, args.nonce);
    let mut actors = vec![
        proxy,
        d.collateral(args.collateral_def),
        d.host,
        d.oracle,
        d.clock,
        d.config,
        record_actor(d.engine, d.config.account_id, args.position),
        auction,
    ];
    actors.extend_from_slice(host_actors);
    actors.extend([d.vault(args.position, args.nonce), d.holding(args.keeper)]);
    let evidence = vec![PublicAccountEvidence::Pda {
        program: d.engine,
        seed: vault_seed(auction.account_id),
    }];
    let msg = EngineMsg::Liquidate {
        config: d.config.account_id,
        collateral_def: args.collateral_def,
        position: args.position,
        nonce: args.nonce,
        now_ms: args.now_ms,
    };
    Plan::new(proxy, actors, vec![args.keeper], evidence, &msg)
}

/// `Bid` at the collateral actor, signed by the bidder: collateral actor, host params, oracle,
/// clock, auction, the bidder's holding (pays the charge, receives the collateral), the
/// stablecoin definition (burn), buffer, auction vault, accounting, config. `source` says whether
/// the buffer exists; while it does not, the plan carries its PDA evidence (R-5b).
///
/// # Errors
/// [`SdkError::Encode`] if the message does not encode.
pub fn plan_bid(
    source: &impl StateSource,
    d: &Deployment,
    args: &BidArgs,
) -> Result<Plan, SdkError> {
    let root = d.collateral(args.collateral_def);
    let buffer = d.buffer();
    let actors = vec![
        root,
        d.host,
        d.oracle,
        d.clock,
        d.auction(args.position, args.nonce),
        d.holding(args.bidder),
        d.holding(d.stable_def),
        buffer,
        d.vault(args.position, args.nonce),
        d.accounting,
        d.config,
    ];
    let evidence = if source.account_exists(buffer.account_id) {
        Vec::new()
    } else {
        vec![PublicAccountEvidence::Pda {
            program: d.engine,
            seed: buffer_seed(d.config.account_id),
        }]
    };
    let msg = EngineMsg::Bid {
        bidder: args.bidder,
        config: d.config.account_id,
        collateral_def: args.collateral_def,
        position: args.position,
        nonce: args.nonce,
        max_debt_in: args.max_debt_in,
        min_collateral_out: args.min_collateral_out,
        now_ms: args.now_ms,
    };
    Plan::new(root, actors, vec![args.bidder], evidence, &msg)
}

/// `ClaimLeftover` at the owner's proxy, signed by the owner: proxy, auction, its vault, the
/// owner's holding (a signer's own holding: no evidence, R-3a).
///
/// # Errors
/// [`SdkError::Encode`] if the message does not encode.
pub fn plan_claim_leftover(
    d: &Deployment,
    owner: AccountId,
    position: AccountId,
    nonce: u64,
) -> Result<Plan, SdkError> {
    let proxy = d.proxy(owner);
    let actors = vec![
        proxy,
        d.auction(position, nonce),
        d.vault(position, nonce),
        d.holding(owner),
    ];
    let msg = EngineMsg::ClaimLeftover {
        config: d.config.account_id,
        position,
        nonce,
    };
    Plan::new(proxy, actors, vec![owner], Vec::new(), &msg)
}

/// `SettleEmpty` at the config, permissionless (no signer besides the payer): config, auction,
/// accounting, collateral actor.
///
/// # Errors
/// [`SdkError::Encode`] if the message does not encode.
pub fn plan_settle_empty(
    d: &Deployment,
    collateral_def: AccountId,
    position: AccountId,
    nonce: u64,
) -> Result<Plan, SdkError> {
    let actors = vec![
        d.config,
        d.auction(position, nonce),
        d.accounting,
        d.collateral(collateral_def),
    ];
    let msg = EngineMsg::SettleEmpty {
        config: d.config.account_id,
        position,
        nonce,
    };
    Plan::new(d.config, actors, Vec::new(), Vec::new(), &msg)
}
