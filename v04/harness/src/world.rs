//! The shared engine test world: token, oracle, mock CDP host and engine deployed on one
//! [`Chain`], with the engine's config, accounting and collateral actors for one admin and salt.
//!
//! [`World::bare`] stops before the engine is initialised; [`World::new`] also runs `Init` and
//! `SetCollateral` through the admin's signer proxy and binds the host to the config actor.

use auction_engine_params::args::{CollateralArgs, EngineParamsArgs};
use engine_core::{
    actors::{accounting_actor, collateral_actor, config_actor},
    msg::EngineMsg,
};
use lee::{AccountId, Actor, PrivateKey, PublicAccountEvidence, error::LeeError};
use lee_core::program::TransactionEvent;
use mock_host_core::{HostMsg, is_unsafe, position_id, vault_id, vault_seed};
use oracle_core::OracleMsg;
use token_core::TokenMsg;

use crate::Chain;

/// 1e27.
pub const RAY: u128 = 1_000_000_000_000_000_000_000_000_000;
/// The mock host's minimum collateral ratio, 150 %.
pub const MIN_CR_RAY: u128 = RAY * 3 / 2;
/// The oracle price the world starts at.
pub const SAFE_PRICE_RAY: u128 = 2 * RAY;
/// The config salt of the world's admin.
pub const SALT: u64 = 1;
/// The config's clock slack, ms.
pub const SLACK_MS: u64 = 6_000;
/// The collateral supply, all minted to the whale that funds position owners.
pub const WHALE_COLLATERAL: u128 = 1_000_000_000_000;

/// The relay guest's root message: forward `message` to `to` as a sync message.
#[derive(borsh::BorshSerialize)]
pub struct Forward {
    pub to: Actor,
    pub message: Vec<u8>,
}

impl Forward {
    pub fn new(to: Actor, msg: &impl borsh::BorshSerialize) -> Self {
        Self {
            to,
            message: borsh::to_vec(msg).expect("message encodes"),
        }
    }
}

/// A position opened by [`World::open_unsafe_position`].
pub struct Owner {
    pub key: PrivateKey,
    pub account: AccountId,
    /// The host's position PDA.
    pub position: AccountId,
}

pub struct World {
    pub chain: Chain,
    /// The engine program `E`.
    pub engine: AccountId,
    /// The token program `T`.
    pub token: AccountId,
    /// The oracle actor the host and the engine read.
    pub oracle: Actor,
    /// The mock host program `H`.
    pub host_prog: AccountId,
    /// The engine admin (a signer).
    pub admin: AccountId,
    pub admin_key: PrivateKey,
    /// `config_actor(E, admin, SALT)`.
    pub config: Actor,
    /// `collateral_actor(E, config, collateral_def)`.
    pub collateral: Actor,
    /// `accounting_actor(E, config)`.
    pub accounting: Actor,
    /// The stablecoin definition account (minter: the host params actor).
    pub stable_def: AccountId,
    /// The collateral definition account.
    pub collateral_def: AccountId,
    /// The host params actor `P = (params, H)`; its account signs host admin messages.
    pub params: Actor,
    pub params_key: PrivateKey,
    /// The oracle's publisher, a relay actor.
    pub publisher: Actor,
    /// Holds the collateral supply.
    pub whale: AccountId,
    pub whale_key: PrivateKey,
    /// Owners of the positions opened so far, in order.
    pub owners: Vec<Owner>,
    /// The chain time the world last set, ms.
    pub now_ms: u64,
}

impl World {
    /// Everything deployed and the host initialised; the engine is not initialised yet and the
    /// host is not bound.
    pub fn bare() -> Self {
        let mut chain = Chain::new(&[
            ("token", methods::TOKEN_ELF),
            ("oracle", methods::ORACLE_ELF),
            ("mock_host", methods::MOCK_HOST_ELF),
            ("relay", methods::RELAY_ELF),
            ("engine", methods::ENGINE_ELF),
        ]);
        let (token, host_prog, engine) = (
            chain.program("token"),
            chain.program("mock_host"),
            chain.program("engine"),
        );
        let (admin_key, admin) = chain.key("admin");
        let (params_key, params_account) = chain.key("params");
        let (whale_key, whale) = chain.key("whale");
        let (cdef_key, collateral_def) = chain.key("collateral-def");
        let (sdef_key, stable_def) = chain.key("stable-def");
        let (oracle_key, oracle_account) = chain.key("oracle");
        let (_, publisher_account) = chain.key("publisher");
        for account in [admin, params_account, whale, publisher_account] {
            chain.fund(account, 1);
        }
        let config = config_actor(engine, admin, SALT);
        let params = Actor::new(params_account, host_prog);
        let mut w = Self {
            engine,
            token,
            oracle: Actor::new(oracle_account, chain.program("oracle")),
            host_prog,
            admin,
            admin_key,
            config,
            collateral: collateral_actor(engine, config.account_id, collateral_def),
            accounting: accounting_actor(engine, config.account_id),
            stable_def,
            collateral_def,
            params,
            params_key,
            publisher: Actor::new(publisher_account, chain.program("relay")),
            whale,
            whale_key,
            owners: Vec::new(),
            now_ms: 0,
            chain,
        };

        let cdef = Actor::new(collateral_def, token);
        let new_collateral = TokenMsg::NewDefinition {
            minter: None,
            initial_to: Some(whale),
            initial_supply: WHALE_COLLATERAL,
        };
        let whale_holding = Actor::new(whale, token);
        w.chain
            .send(cdef, &[cdef, whale_holding], &[&cdef_key], &new_collateral)
            .expect("the collateral definition is created");
        let sdef = Actor::new(stable_def, token);
        let new_stable = TokenMsg::NewDefinition {
            minter: Some(params),
            initial_to: None,
            initial_supply: 0,
        };
        w.chain
            .send(sdef, &[sdef], &[&sdef_key], &new_stable)
            .expect("the stablecoin definition is created");

        let oracle = w.oracle;
        let init_oracle = OracleMsg::Init {
            publisher: w.publisher,
        };
        w.chain
            .send(oracle, &[oracle], &[&oracle_key], &init_oracle)
            .expect("the oracle is initialised");
        w.set_price(SAFE_PRICE_RAY);

        let init_host = HostMsg::Init {
            admin: params,
            token,
            stable_def,
            collateral_def,
            oracle,
            redemption_price_ray: RAY,
            min_cr_ray: MIN_CR_RAY,
        };
        w.chain
            .send(params, &[params], &[&w.params_key], &init_host)
            .expect("the host is initialised");
        w
    }

    /// [`World::bare`], then the admin's `Init` and `SetCollateral` and the host bound to the
    /// config actor.
    pub fn new() -> Self {
        let mut w = Self::bare();
        let key = w.admin_key.clone();
        let init = w.init_msg();
        w.send_engine(w.admin, &[&key], &init)
            .expect("the engine is initialised");
        let set = w.set_collateral_msg(Self::collateral_args());
        w.send_engine(w.admin, &[&key], &set)
            .expect("the collateral is configured");
        w.bind_host().expect("the host binds the config actor");
        w
    }

    /// The engine parameters the world initialises with.
    pub fn engine_params() -> EngineParamsArgs {
        EngineParamsArgs::defaults_for_tests()
    }

    /// The collateral parameters the world configures.
    pub fn collateral_args() -> CollateralArgs {
        CollateralArgs::defaults_for_tests()
    }

    /// `Init` for salt [`SALT`] with [`World::engine_params`].
    pub fn init_msg(&self) -> EngineMsg {
        self.init_msg_with(Self::engine_params())
    }

    /// `Init` for salt [`SALT`] with `params`.
    pub fn init_msg_with(&self, params: EngineParamsArgs) -> EngineMsg {
        EngineMsg::Init {
            salt: SALT,
            params: Box::new(params),
            host: self.params,
            token: self.token,
            stable_def: self.stable_def,
            oracle: self.oracle,
            clock: self.chain.clock(),
            slack_ms: SLACK_MS,
        }
    }

    /// `SetCollateral` of the world's collateral definition for salt [`SALT`].
    pub fn set_collateral_msg(&self, args: CollateralArgs) -> EngineMsg {
        EngineMsg::SetCollateral {
            salt: SALT,
            collateral_def: self.collateral_def,
            args,
        }
    }

    /// The actors a root engine message at the proxy `(proxy, E)` touches.
    ///
    /// # Panics
    /// For a message that is not sent through the proxy.
    pub fn proxy_actors(&self, proxy: AccountId, msg: &EngineMsg) -> Vec<Actor> {
        let at = Actor::new(proxy, self.engine);
        if let EngineMsg::Init { salt, oracle, .. } = msg {
            let config = config_actor(self.engine, proxy, *salt);
            let accounting = accounting_actor(self.engine, config.account_id);
            return vec![at, config, *oracle, accounting];
        }
        if let EngineMsg::SetCollateral {
            salt,
            collateral_def,
            ..
        } = msg
        {
            let config = config_actor(self.engine, proxy, *salt);
            let collateral = collateral_actor(self.engine, config.account_id, *collateral_def);
            return vec![at, config, collateral];
        }
        panic!("not a proxied engine message: {msg:?}")
    }

    /// Sends `msg` as the root message at the proxy `(proxy, E)`, signed by `signers`.
    pub fn send_engine(
        &mut self,
        proxy: AccountId,
        signers: &[&PrivateKey],
        msg: &EngineMsg,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let actors = self.proxy_actors(proxy, msg);
        let root = Actor::new(proxy, self.engine);
        self.chain.send(root, &actors, signers, msg)
    }

    /// Binds the host to the config actor `(config, E)`.
    pub fn bind_host(&mut self) -> Result<Vec<TransactionEvent>, LeeError> {
        let params = self.params;
        let bind = HostMsg::SetLiquidationEngine {
            engine_actor: self.config,
        };
        self.chain
            .send(params, &[params], &[&self.params_key], &bind)
    }

    /// Sets chain time to `ms`; later publications carry it as their timestamp.
    pub fn set_now_ms(&mut self, ms: u64) {
        self.now_ms = ms;
        self.chain.set_now_ms(ms);
    }

    /// The publisher publishes `price_ray` at the world's current time.
    pub fn set_price(&mut self, price_ray: u128) {
        let (oracle, publisher) = (self.oracle, self.publisher);
        let publish = OracleMsg::Publish {
            price_ray,
            timestamp_ms: self.now_ms,
            confidence: 0,
        };
        self.chain
            .send(
                publisher,
                &[publisher, oracle],
                &[],
                &Forward::new(oracle, &publish),
            )
            .expect("the publisher publishes");
    }

    /// A fresh owner gets `collateral` from the whale, opens a position with it through the
    /// host's proxy and generates `debt`; then the oracle publishes a price at which the position
    /// is unsafe at [`MIN_CR_RAY`] (half the price it would need). Returns the position account.
    pub fn open_unsafe_position(&mut self, collateral: u128, debt: u128) -> AccountId {
        let label = format!("owner-{}", self.owners.len());
        let (key, owner) = self.chain.key(&label);
        self.chain.fund(owner, 1);
        let (token, host) = (self.token, self.host_prog);
        let (owner_holding, proxy) = (Actor::new(owner, token), Actor::new(owner, host));
        let position = Actor::new(position_id(host, owner), host);

        let whale_holding = Actor::new(self.whale, token);
        let fund = TokenMsg::Transfer {
            definition: self.collateral_def,
            to: owner,
            amount: collateral,
        };
        self.chain
            .send(
                whale_holding,
                &[whale_holding, owner_holding],
                &[&self.whale_key],
                &fund,
            )
            .expect("the whale funds the owner");

        let open = self.host_proxied(HostMsg::OpenPosition { owner, collateral });
        let vault = Actor::new(vault_id(host, owner), token);
        let evidence = [PublicAccountEvidence::Pda {
            program: host,
            seed: vault_seed(owner),
        }];
        self.chain
            .send_with_evidence(
                proxy,
                &[proxy, self.params, position, owner_holding, vault],
                &[&key],
                &evidence,
                &open,
            )
            .expect("the position opens");

        let generate = self.host_proxied(HostMsg::GenerateDebt {
            normalized_debt: debt,
        });
        let sdef = Actor::new(self.stable_def, token);
        self.chain
            .send(
                proxy,
                &[proxy, self.params, position, sdef, owner_holding],
                &[&key],
                &generate,
            )
            .expect("debt is generated");

        let price = debt
            .checked_mul(MIN_CR_RAY)
            .and_then(|needed| needed.checked_div(collateral))
            .and_then(|at_ratio| at_ratio.checked_div(2))
            .expect("collateral is non-zero and the debt small enough")
            .max(1);
        assert!(
            is_unsafe(collateral, debt, price, MIN_CR_RAY),
            "the position must be unsafe at the published price"
        );
        self.set_price(price);
        self.owners.push(Owner {
            key,
            account: owner,
            position: position.account_id,
        });
        position.account_id
    }

    fn host_proxied(&self, message: HostMsg) -> HostMsg {
        HostMsg::Proxy {
            params: self.params.account_id,
            message: Box::new(message),
        }
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}
