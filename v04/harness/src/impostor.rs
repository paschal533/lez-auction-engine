//! The `impostor` test guest (design note §4.3 c) as the security tests drive it: an attacker's
//! program that speaks as whichever actor `(account, Impostor)` it sits at, stands in as a bound
//! host, or replays a script (see `programs/methods/guest/src/bin/impostor.rs`).

use borsh::BorshSerialize;
use engine_core::{
    actors::{
        auction_actor, collateral_actor, config_actor, record_actor, vault_holding, vault_seed,
    },
    msg::EngineMsg,
};
use lee::{AccountId, Actor, PrivateKey, PublicAccountEvidence, error::LeeError};
use lee_core::program::TransactionEvent;
use mock_host_core::{Params, TAG_PARAMS};
use token_core::TokenMsg;

use crate::{
    World,
    world::{MIN_CR_RAY, RAY, SALT},
};

/// The leading bytes of an impostor script state.
pub const SCRIPT_TAG: [u8; 8] = *b"IMPSCRPT";

/// The impostor guest's root message.
#[derive(BorshSerialize)]
pub enum ImpostorMsg {
    /// Send `message` to `to` as the impostor actor.
    Forward { to: Actor, message: Vec<u8> },
    /// Store these bytes as the impostor actor's state.
    SetState(Vec<u8>),
}

/// `msg`'s borsh bytes.
pub fn encode(msg: &impl BorshSerialize) -> Vec<u8> {
    borsh::to_vec(msg).expect("message encodes")
}

/// A script state: on any message, the impostor sends each `(to, message)`.
pub fn script(forwards: &[(Actor, Vec<u8>)]) -> Vec<u8> {
    SCRIPT_TAG
        .into_iter()
        .chain(encode(&forwards.to_vec()))
        .collect()
}

impl World {
    /// The impostor program at `account`.
    pub fn impostor(&self, account: AccountId) -> Actor {
        Actor::new(account, self.chain.program("impostor"))
    }

    /// One unsigned transaction in which the impostor `from` sends `msg` to `to`.
    pub fn speak(
        &mut self,
        from: Actor,
        to: Actor,
        msg: &impl BorshSerialize,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let forward = ImpostorMsg::Forward {
            to,
            message: encode(msg),
        };
        self.chain.send(from, &[from, to], &[], &forward)
    }

    /// The impostor at the funded signer `label`'s account, and that signer's key.
    pub fn fresh_impostor(&mut self, label: &str) -> (PrivateKey, Actor) {
        let (key, account) = self.chain.key(label);
        self.chain.fund(account, 1);
        (key, self.impostor(account))
    }

    /// The impostor at `label`'s account, storing `state`.
    pub fn impostor_with_state(&mut self, label: &str, state: Vec<u8>) -> Actor {
        let (key, actor) = self.fresh_impostor(label);
        self.chain
            .send(actor, &[actor], &[&key], &ImpostorMsg::SetState(state))
            .expect("the impostor stores its state");
        actor
    }

    /// An impostor host stand-in at `label`'s account: mock `Params` with the world's token,
    /// definitions and oracle and redemption price 1 RAY. On `LiquidatePosition` it pings `admin`;
    /// with `admin = None` it pings nobody (a host that never replies).
    pub fn impostor_host(&mut self, label: &str, admin: Option<Actor>) -> Actor {
        let (_, account) = self.chain.key(label);
        let host = self.impostor(account);
        let params = Params {
            tag: TAG_PARAMS,
            admin: admin.unwrap_or(host),
            token: self.token,
            stable_def: self.stable_def,
            collateral_def: self.collateral_def,
            oracle: self.oracle,
            redemption_price_ray: RAY,
            min_cr_ray: MIN_CR_RAY,
            engine_actor: None,
            pending: None,
        };
        self.impostor_with_state(label, encode(&params))
    }

    /// `admin`'s `Init` (salt [`SALT`]) and `SetCollateral` of the world's collateral at its proxy,
    /// with the engine bound to `host`. Returns the config actor.
    pub fn configure_with_host(
        &mut self,
        admin: AccountId,
        admin_key: &PrivateKey,
        host: Actor,
    ) -> Actor {
        let mut init = self.init_msg();
        if let EngineMsg::Init { host: bound, .. } = &mut init {
            *bound = host;
        }
        self.send_engine(admin, &[admin_key], &init)
            .expect("the engine binds the host");
        let set = self.set_collateral_msg(Self::collateral_args());
        let params = self.params;
        let actors: Vec<Actor> = self
            .proxy_actors(admin, &set)
            .into_iter()
            .map(|actor| if actor == params { host } else { actor })
            .collect();
        let root = Actor::new(admin, self.engine);
        self.chain
            .send(root, &actors, &[admin_key], &set)
            .expect("the collateral observes through the host");
        config_actor(self.engine, admin, SALT)
    }

    /// The `Seized` an impostor position reports for itself.
    pub fn impostor_seized(position: AccountId, collateral: u128, debt: u128) -> EngineMsg {
        EngineMsg::Seized {
            position,
            owner: position,
            collateral,
            normalized_debt: debt,
            nominal_debt: debt,
        }
    }

    /// An impostor "position" at `label`'s account whose script reports
    /// `Seized { collateral, debt }` to auction 0 of `config` whenever it is pinged.
    pub fn scripted_position(
        &mut self,
        label: &str,
        config: AccountId,
        collateral: u128,
        debt: u128,
    ) -> Actor {
        let (_, account) = self.chain.key(label);
        let auction = auction_actor(self.engine, config, account, 0);
        let seized = Self::impostor_seized(account, collateral, debt);
        self.impostor_with_state(label, script(&[(auction, encode(&seized))]))
    }

    /// The vault of auction 0 of `(config, position)`.
    pub fn vault_of(&self, config: AccountId, position: AccountId) -> Actor {
        let auction = auction_actor(self.engine, config, position, 0);
        vault_holding(self.engine, self.token, auction.account_id)
    }

    /// The fresh-vault evidence of auction 0 of `(config, position)`, if the vault is absent.
    fn vault_evidence(&self, config: AccountId, position: AccountId) -> Vec<PublicAccountEvidence> {
        if self
            .chain
            .raw_state(self.vault_of(config, position))
            .is_some()
        {
            return Vec::new();
        }
        let auction = auction_actor(self.engine, config, position, 0);
        vec![PublicAccountEvidence::Pda {
            program: self.engine,
            seed: vault_seed(auction.account_id),
        }]
    }

    /// The whale funds the vault of auction 0 of `(config, position)` with `amount` collateral,
    /// so a scripted seizure is really in custody.
    pub fn fund_vault(&mut self, config: AccountId, position: AccountId, amount: u128) {
        let whale = Actor::new(self.whale, self.token);
        let vault = self.vault_of(config, position);
        let donate = TokenMsg::Transfer {
            definition: self.collateral_def,
            to: vault.account_id,
            amount,
        };
        let evidence = self.vault_evidence(config, position);
        let key = self.whale_key.clone();
        self.chain
            .send_with_evidence(whale, &[whale, vault], &[&key], &evidence, &donate)
            .expect("the vault is funded");
    }

    /// The keeper liquidates the impostor `position` as auction 0 of `config`, whose bound host
    /// is `host`, claiming the world's time.
    pub fn liquidate_through(
        &mut self,
        config: AccountId,
        host: Actor,
        position: Actor,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let (e, p) = (self.engine, position.account_id);
        let msg = EngineMsg::Liquidate {
            config,
            collateral_def: self.collateral_def,
            position: p,
            nonce: 0,
            now_ms: self.now_ms,
        };
        let keeper_root = Actor::new(self.keeper, e);
        let actors = [
            keeper_root,
            collateral_actor(e, config, self.collateral_def),
            host,
            self.oracle,
            self.chain.clock(),
            Actor::new(config, e),
            record_actor(e, config, p),
            auction_actor(e, config, p, 0),
            position,
            self.vault_of(config, p),
            Actor::new(self.keeper, self.token),
        ];
        let evidence = self.vault_evidence(config, p);
        let key = self.keeper_key.clone();
        self.chain
            .send_with_evidence(keeper_root, &actors, &[&key], &evidence, &msg)
    }
}
