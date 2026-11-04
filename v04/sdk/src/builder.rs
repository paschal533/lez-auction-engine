//! Plans: the root actor, declared actors, signers, admission evidence and message bytes of one
//! engine instruction, built from `engine_core::actors` and the deployment read from state.
//!
//! Token actors (ADR-062): a plan declares the owner token actors `(owner, T)` it debits through
//! and every per-definition holding `holding_actor(T, owner, definition)` it debits or credits.
//!
//! Evidence rule (settlement admits signers ∪ accounts that exist ∪ accounts a message's PDA seed
//! grants ∪ accounts proven public by evidence): no plan in this module carries evidence (the
//! system plans in [`crate::system`] carry one `Pda`, for an absent buffer holding). A fresh
//! holding is a token PDA that the token program's `Credit` grants with its `HOLDING` seed; the
//! engine's vault and buffer token actors are granted by the engine's own seeds on the messages
//! that debit them; engine actors written by their own seed-granted messages need none (S9). No
//! plan touches a fresh non-signer plain account, so none carries `PublicAccountEvidence::Key`.

use auction_engine_params::args::{CollateralArgs, EngineParamsArgs};
use engine_core::{
    actors::{
        accounting_actor, auction_actor, buffer_holding, collateral_actor, config_actor,
        debt_actor, protocol_def, record_actor, surplus_actor, surplus_vault_holding,
        vault_holding,
    },
    msg::{EngineMsg, ProtocolGenesis},
    state::ConfigState,
};
use lee::{AccountId, Actor, PublicAccountEvidence};
use token_core::holding_actor;

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
    pub(crate) fn new(
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
    /// The protocol token's definition `protocol_def(E, config)` (the config names it only while
    /// the protocol token is enabled).
    pub protocol_def: AccountId,
    /// The reference oracle the config names, if any.
    pub ref_oracle: Option<Actor>,
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
            protocol_def: protocol_def(engine, config),
            ref_oracle: state.protocol_token_oracle,
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

    /// The vault's token actor of auction `nonce` of `position` (its collateral is
    /// `holding_of(vault.account_id, collateral_def)`).
    #[must_use]
    pub fn vault(&self, position: AccountId, nonce: u64) -> Actor {
        let auction = self.auction(position, nonce).account_id;
        vault_holding(self.engine, self.token, auction)
    }

    /// The surplus buffer's token actor (its stablecoin is
    /// `holding_of(buffer.account_id, stable_def)`).
    #[must_use]
    pub fn buffer(&self) -> Actor {
        buffer_holding(self.engine, self.token, self.config.account_id)
    }

    /// The surplus auction `surplus_actor(E, config, nonce)`.
    #[must_use]
    pub fn surplus(&self, nonce: u64) -> Actor {
        surplus_actor(self.engine, self.config.account_id, nonce)
    }

    /// The surplus auction `nonce`'s vault token actor (its stablecoin is
    /// `holding_of(surplus_vault.account_id, stable_def)`).
    #[must_use]
    pub fn surplus_vault(&self, nonce: u64) -> Actor {
        surplus_vault_holding(self.engine, self.token, self.surplus(nonce).account_id)
    }

    /// The debt auction `debt_actor(E, config, nonce)`.
    #[must_use]
    pub fn debt(&self, nonce: u64) -> Actor {
        debt_actor(self.engine, self.config.account_id, nonce)
    }

    /// `account`'s own token actor `(account, T)`, the stateless proxy its debits go through.
    #[must_use]
    pub fn holding(&self, account: AccountId) -> Actor {
        Actor::new(account, self.token)
    }

    /// `owner`'s holding of `definition`, `holding_actor(T, owner, definition)`: where the
    /// balance lives (ADR-062).
    #[must_use]
    pub fn holding_of(&self, owner: AccountId, definition: AccountId) -> Actor {
        holding_actor(self.token, owner, definition)
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
    /// The reference oracle, present iff `params.ref_oracle_enabled`.
    pub ref_oracle: Option<Actor>,
    /// The protocol token's genesis supply and recipient, if any.
    pub protocol_genesis: Option<ProtocolGenesis>,
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
/// accounting, the protocol definition `(protocol_def(E, K), T)` and, with a genesis, its
/// recipient's holding (a fresh holding is granted by the token's `Credit`, so no evidence).
///
/// # Errors
/// [`SdkError::Encode`] if the message does not encode.
pub fn plan_init(engine: AccountId, admin: AccountId, args: InitArgs) -> Result<Plan, SdkError> {
    let config = config_actor(engine, admin, args.salt);
    let accounting = accounting_actor(engine, config.account_id);
    let proxy = Actor::new(admin, engine);
    let definition = protocol_def(engine, config.account_id);
    let mut actors = vec![
        proxy,
        config,
        args.oracle,
        accounting,
        Actor::new(definition, args.token),
    ];
    actors.extend(
        args.protocol_genesis
            .map(|genesis| holding_actor(args.token, genesis.to, definition)),
    );
    let msg = EngineMsg::Init {
        salt: args.salt,
        params: Box::new(args.params),
        host: args.host,
        token: args.token,
        stable_def: args.stable_def,
        oracle: args.oracle,
        clock: args.clock,
        slack_ms: args.slack_ms,
        ref_oracle: args.ref_oracle,
        protocol_genesis: args.protocol_genesis,
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
/// position `(position, H)`, the owner's host vault token actor and that vault's collateral
/// holding). The plan adds the auction vault's token actor (the keeper reward's debit) and
/// collateral holding, and the keeper's collateral holding.
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
    let vault = d.vault(args.position, args.nonce);
    actors.extend([
        vault,
        d.holding_of(vault.account_id, args.collateral_def),
        d.holding_of(args.keeper, args.collateral_def),
    ]);
    let msg = EngineMsg::Liquidate {
        config: d.config.account_id,
        collateral_def: args.collateral_def,
        position: args.position,
        nonce: args.nonce,
        now_ms: args.now_ms,
    };
    Plan::new(proxy, actors, vec![args.keeper], Vec::new(), &msg)
}

/// `Bid` at the collateral actor, signed by the bidder: collateral actor, host params, oracle,
/// clock, auction, the bidder's token actor (pays the charge) with its stablecoin and collateral
/// holdings, the stablecoin definition (burn), the buffer's stablecoin holding, the auction
/// vault's token actor and collateral holding, accounting, config.
///
/// # Errors
/// [`SdkError::Encode`] if the message does not encode.
pub fn plan_bid(d: &Deployment, args: &BidArgs) -> Result<Plan, SdkError> {
    let root = d.collateral(args.collateral_def);
    let (cdef, sdef) = (args.collateral_def, d.stable_def);
    let vault = d.vault(args.position, args.nonce);
    let actors = vec![
        root,
        d.host,
        d.oracle,
        d.clock,
        d.auction(args.position, args.nonce),
        d.holding(args.bidder),
        d.holding_of(args.bidder, sdef),
        d.holding_of(args.bidder, cdef),
        d.holding(sdef),
        d.holding_of(d.buffer().account_id, sdef),
        vault,
        d.holding_of(vault.account_id, cdef),
        d.accounting,
        d.config,
    ];
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
    Plan::new(root, actors, vec![args.bidder], Vec::new(), &msg)
}

/// `ClaimLeftover` at the owner's proxy, signed by the owner: proxy, auction, its vault's token
/// actor and collateral holding, the owner's collateral holding.
///
/// # Errors
/// [`SdkError::Encode`] if the message does not encode.
pub fn plan_claim_leftover(
    d: &Deployment,
    collateral_def: AccountId,
    owner: AccountId,
    position: AccountId,
    nonce: u64,
) -> Result<Plan, SdkError> {
    let proxy = d.proxy(owner);
    let vault = d.vault(position, nonce);
    let actors = vec![
        proxy,
        d.auction(position, nonce),
        vault,
        d.holding_of(vault.account_id, collateral_def),
        d.holding_of(owner, collateral_def),
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
