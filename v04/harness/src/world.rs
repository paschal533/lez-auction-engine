//! The shared engine test world: token, oracle, mock CDP host and engine deployed on one
//! [`Chain`], with the engine's config, accounting and collateral actors for one admin and salt.
//!
//! [`World::bare`] stops before the engine is initialised; [`World::new`] also runs `Init` and
//! `SetCollateral` through the admin's signer proxy and binds the host to the config actor.

use auction_engine_params::args::{CollateralArgs, EngineParamsArgs};
use engine_core::{
    actors::{
        accounting_actor, auction_actor, collateral_actor, config_actor, record_actor,
        vault_holding, vault_seed as engine_vault_seed,
    },
    msg::EngineMsg,
};
use lee::{AccountId, Actor, PrivateKey, PublicAccountEvidence, error::LeeError};
use lee_core::program::TransactionEvent;
use mock_host_core::{HostMsg, is_unsafe, position_id, vault_id, vault_seed};
use oracle_core::OracleMsg;
use token_core::{Holding, TokenMsg};

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
    /// The keeper that liquidates (a signer; its reward goes to its own holding `(keeper, T)`).
    pub keeper: AccountId,
    pub keeper_key: PrivateKey,
    /// The chain time the world last set, ms.
    pub now_ms: u64,
}

impl World {
    /// Everything deployed and the host initialised; the engine is not initialised yet and the
    /// host is not bound.
    pub fn bare() -> Self {
        Self::bare_at(RAY)
    }

    /// [`World::bare`] with the host's redemption price `redemption_price_ray`.
    pub fn bare_at(redemption_price_ray: u128) -> Self {
        let mut chain = Chain::new(&[
            ("token", methods::TOKEN_ELF),
            ("oracle", methods::ORACLE_ELF),
            ("mock_host", methods::MOCK_HOST_ELF),
            ("relay", methods::RELAY_ELF),
            ("engine", methods::ENGINE_ELF),
            ("grant_probe", methods::GRANT_PROBE_ELF),
            ("impostor", methods::IMPOSTOR_ELF),
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
        let (keeper_key, keeper) = chain.key("keeper");
        for account in [admin, params_account, whale, publisher_account, keeper] {
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
            keeper,
            keeper_key,
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
            redemption_price_ray,
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
        Self::new_at(RAY)
    }

    /// [`World::new`] with the host's redemption price `redemption_price_ray`.
    pub fn new_at(redemption_price_ray: u128) -> Self {
        let mut w = Self::bare_at(redemption_price_ray);
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

    /// `SetCollateral` of the world's collateral definition for salt [`SALT`], claiming the
    /// world's current time.
    pub fn set_collateral_msg(&self, args: CollateralArgs) -> EngineMsg {
        EngineMsg::SetCollateral {
            salt: SALT,
            collateral_def: self.collateral_def,
            args,
            now_ms: self.now_ms,
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
            // The collateral actor observes: host params, oracle, clock.
            let clock = self.chain.clock();
            return vec![at, config, collateral, self.params, self.oracle, clock];
        }
        if let EngineMsg::Liquidate {
            position, nonce, ..
        } = msg
        {
            let mut actors = vec![at];
            actors.extend(self.liquidate_actors(*position, *nonce));
            return actors;
        }
        if let EngineMsg::ClaimLeftover {
            position, nonce, ..
        } = msg
        {
            let mut actors = vec![at];
            actors.extend(self.claim_actors(*position, *nonce));
            return actors;
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
        let (key, owner, position) = self.open_position(&label, collateral, debt);
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
            position,
        });
        position
    }

    /// A fresh signer `label` gets `collateral` from the whale, opens a host position with it
    /// and generates `debt`, which the host mints to its own holding. Returns its key, account
    /// and position.
    pub fn open_position(
        &mut self,
        label: &str,
        collateral: u128,
        debt: u128,
    ) -> (PrivateKey, AccountId, AccountId) {
        let (key, owner) = self.chain.key(label);
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
        (key, owner, position.account_id)
    }

    /// The owner whose position is `position`.
    ///
    /// # Panics
    /// If the world did not open `position`.
    pub fn owner_of(&self, position: AccountId) -> &Owner {
        self.owners
            .iter()
            .find(|owner| owner.position == position)
            .unwrap_or_else(|| panic!("the world opened no position {position:?}"))
    }

    /// `position`'s liquidation record `record_actor(E, config, position)`.
    pub fn record(&self, position: AccountId) -> Actor {
        record_actor(self.engine, self.config.account_id, position)
    }

    /// The auction `auction_actor(E, config, position, nonce)`.
    pub fn auction(&self, position: AccountId, nonce: u64) -> Actor {
        auction_actor(self.engine, self.config.account_id, position, nonce)
    }

    /// The auction's vault holding `(pda(E, ["VAULT", auction]), T)`.
    pub fn vault(&self, position: AccountId, nonce: u64) -> Actor {
        let auction = self.auction(position, nonce).account_id;
        vault_holding(self.engine, self.token, auction)
    }

    /// The host vault holding of `position`'s owner.
    pub fn host_vault(&self, position: AccountId) -> Actor {
        let owner = self.owner_of(position).account;
        Actor::new(vault_id(self.host_prog, owner), self.token)
    }

    /// `account`'s balance of `definition`; zero if it holds nothing.
    pub fn balance(&self, account: AccountId, definition: AccountId) -> u128 {
        self.chain
            .raw_state(Actor::new(account, self.token))
            .map_or(0, |bytes| {
                Holding::decode(&bytes)
                    .expect("a token holding")
                    .balance(definition)
            })
    }

    /// `account`'s collateral balance.
    pub fn collateral_balance(&self, account: AccountId) -> u128 {
        self.balance(account, self.collateral_def)
    }

    /// The keeper's `Liquidate` of `position` with expected auction number `nonce`, claiming
    /// `now_ms`.
    pub fn liquidate_msg(&self, position: AccountId, nonce: u64, now_ms: u64) -> EngineMsg {
        EngineMsg::Liquidate {
            config: self.config.account_id,
            collateral_def: self.collateral_def,
            position,
            nonce,
            now_ms,
        }
    }

    /// Every actor a liquidation of `position` as auction `nonce` touches, except the proxy:
    /// collateral, host params, oracle, clock, config, record, auction, host position and vault,
    /// the auction vault and the keeper's holding.
    pub fn liquidate_actors(&self, position: AccountId, nonce: u64) -> Vec<Actor> {
        vec![
            self.collateral,
            self.params,
            self.oracle,
            self.chain.clock(),
            self.config,
            self.record(position),
            self.auction(position, nonce),
            Actor::new(position, self.host_prog),
            self.host_vault(position),
            self.vault(position, nonce),
            Actor::new(self.keeper, self.token),
        ]
    }

    /// The admission evidence a liquidation needs: the auction vault is a fresh engine PDA that
    /// a token `Credit` creates (ruling R-5b).
    pub fn liquidate_evidence(
        &self,
        position: AccountId,
        nonce: u64,
    ) -> [PublicAccountEvidence; 1] {
        let auction = self.auction(position, nonce).account_id;
        [PublicAccountEvidence::Pda {
            program: self.engine,
            seed: engine_vault_seed(auction),
        }]
    }

    /// The keeper liquidates `position` as auction `nonce`, claiming the world's current time.
    pub fn liquidate(
        &mut self,
        position: AccountId,
        nonce: u64,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        self.liquidate_at(position, nonce, self.now_ms)
    }

    /// The keeper liquidates `position` as auction `nonce`, claiming `now_ms`.
    pub fn liquidate_at(
        &mut self,
        position: AccountId,
        nonce: u64,
        now_ms: u64,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let msg = self.liquidate_msg(position, nonce, now_ms);
        let actors = self.proxy_actors(self.keeper, &msg);
        let evidence = self.liquidate_evidence(position, nonce);
        let root = Actor::new(self.keeper, self.engine);
        let key = self.keeper_key.clone();
        self.chain
            .send_with_evidence(root, &actors, &[&key], &evidence, &msg)
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
