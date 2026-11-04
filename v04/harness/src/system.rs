//! System operations on the [`World`] (design note §4.2 rows 9-16): every system root is a
//! permissionless message at a config `K = (config, E)`, which hands the operation to its
//! accounting; the accounting reads the buffer's stablecoin holding (and the reference oracle).
//!
//! A buffer holding that was never credited (no bid has paid a penalty yet) is an absent actor:
//! the transaction admits it only with `Pda` evidence for the token program's holding seed
//! ([`World::buffer_evidence`]), and it then reads as 0.
//!
//! The surplus auction (rows 10-12): [`World::with_system`] enables the protocol token with a
//! genesis for the [`TAKER`] signer; `StartSurplus` / `TakeSurplus` / `CloseSurplus` are roots at
//! the config too ([`World::start_surplus`], [`World::take_surplus`], [`World::close_surplus`]).
//!
//! The debt auction (rows 13-15): `StartDebt` / `TakeDebt` / `CloseDebt`, roots at the config
//! ([`World::start_debt`], [`World::take_debt`], [`World::close_debt`]); a take burns the taker's
//! stablecoin and mints protocol tokens through the config, the definition's minter.
//!
//! `check_solvency` (row 16): `CheckSolvency`, a root at the config ([`World::check_solvency`]);
//! a `Freeze` decision freezes new debt at the bound host ([`World::host_frozen`]).

use auction_engine_params::args::EngineParamsArgs;
use engine_core::{
    actors::{
        accounting_actor, buffer_holding, debt_actor, protocol_def, surplus_actor,
        surplus_vault_holding,
    },
    msg::{EngineMsg, ProtocolGenesis},
    state::{AccountingState, ConfigState},
    system_state::{DebtAuctionState, SurplusAuctionState},
};
use lee::{AccountId, Actor, PrivateKey, PublicAccountEvidence, error::LeeError};
use lee_core::program::TransactionEvent;
use mock_host_core::Params;
use oracle_core::OracleMsg;
use token_core::{TokenMsg, holding_seed};

use crate::{World, world::Forward};

/// The signer [`World::with_system`] credits the protocol token's genesis to, and that takes.
pub const TAKER: &str = "taker";
/// The protocol token's genesis supply under [`World::with_system`].
pub const TAKER_GENESIS: u128 = 10_000;

/// Collateral and debt of the position whose empty lot books a shortfall (Plan 5's lot-3 case):
/// the keeper reward takes the whole 3-unit seizure.
const SHORTFALL_COLLATERAL: u128 = 3;
const SHORTFALL_DEBT: u128 = 10_000;

impl World {
    /// A fresh funder gets `amount` of stablecoin minted by the host (against a position of its
    /// own) and transfers it to `to`'s stablecoin holding.
    pub fn donate_stable(&mut self, to: AccountId, amount: u128) {
        let label = (0..)
            .map(|n| format!("donor-{n}"))
            .find(|label| {
                let (_, account) = self.chain.key(label);
                !self.chain.account_exists(account)
            })
            .expect("a free donor label");
        let funder = self.fund_bidder(&label, amount);
        let sdef = self.stable_def;
        let from = Actor::new(funder.account, self.token);
        let transfer = TokenMsg::Transfer {
            definition: sdef,
            to,
            amount,
        };
        let actors = [
            from,
            self.holding(funder.account, sdef),
            self.holding(to, sdef),
        ];
        self.chain
            .send(from, &actors, &[&funder.key], &transfer)
            .expect("the funder donates");
    }

    /// Plan 5's lot-3 `SettleEmpty` case: a fresh 3 / 10_000 position, liquidated at the host's
    /// redemption price (the keeper reward takes the whole seizure), then settled empty, which
    /// books its debt as a shortfall. Returns the accounting's new `unbacked_debt`.
    pub fn book_shortfall(&mut self) -> u128 {
        let redemption = self
            .chain
            .raw_state(self.params)
            .and_then(|bytes| Params::decode(&bytes))
            .expect("the host's params")
            .redemption_price_ray;
        let position = self.open_unsafe_position(SHORTFALL_COLLATERAL, SHORTFALL_DEBT);
        self.set_price(redemption);
        self.liquidate(position, 0)
            .expect("the reward takes the lot");
        self.settle_empty(position, 0)
            .expect("the empty lot settles");
        self.accounting_state().unbacked_debt
    }

    /// `SettleDebt { amount }` at the world's config.
    pub fn settle_debt_msg(&self, amount: u128) -> EngineMsg {
        EngineMsg::SettleDebt {
            config: self.config.account_id,
            amount,
        }
    }

    /// The actors a system root at `config` touches, the config (root) first:
    ///
    /// - `SettleDebt`: its accounting, the buffer's stablecoin holding (read), the buffer's token
    ///   actor and the stablecoin definition (burn);
    /// - `StartSurplus`: those, the clock, the reference oracle (if the config names one), the
    ///   surplus auction and its vault's stablecoin holding (the lot's credit);
    /// - `TakeSurplus` / `CloseSurplus`: see [`World::surplus_actors`];
    /// - the debt roots: see [`World::debt_actors`];
    /// - `CheckSolvency`: see [`World::solvency_actors`].
    ///
    /// # Panics
    /// For a message that is not a system root.
    pub fn system_actors(&self, msg: &EngineMsg) -> Vec<Actor> {
        if let EngineMsg::SettleDebt { config, .. } = msg {
            return self.settle_actors(*config);
        }
        if let Some(actors) = self.debt_actors(msg) {
            return actors;
        }
        if let EngineMsg::CheckSolvency { config, .. } = msg {
            return self.solvency_actors(*config);
        }
        let EngineMsg::StartSurplus { config, nonce, .. } = msg else {
            return self.surplus_actors(msg);
        };
        let mut actors = self.settle_actors(*config);
        actors.push(self.chain.clock());
        actors.extend(self.config_state_of(*config).protocol_token_oracle);
        let surplus = surplus_actor(self.engine, *config, *nonce);
        let vault = surplus_vault_holding(self.engine, self.token, surplus.account_id);
        actors.extend([surplus, self.holding(vault.account_id, self.stable_def)]);
        actors
    }

    /// The actors a debt root touches, the config (root) first, or `None` for another message:
    ///
    /// - `StartDebt`: those of `SettleDebt`, the clock, the reference oracle (if the config names
    ///   one) and the debt auction;
    /// - `TakeDebt`: the clock, the debt auction, the accounting, the taker's token actor and
    ///   stablecoin holding, the stablecoin definition (burn), the protocol definition and the
    ///   taker's protocol holding (mint);
    /// - `CloseDebt`: the clock, the debt auction and the accounting.
    pub fn debt_actors(&self, msg: &EngineMsg) -> Option<Vec<Actor>> {
        if let EngineMsg::StartDebt { config, nonce, .. } = msg {
            let mut actors = self.settle_actors(*config);
            actors.push(self.chain.clock());
            actors.extend(self.config_state_of(*config).protocol_token_oracle);
            actors.push(debt_actor(self.engine, *config, *nonce));
            return Some(actors);
        }
        let (config, nonce, taker) = if let EngineMsg::TakeDebt {
            taker,
            config,
            nonce,
            ..
        } = msg
        {
            (*config, *nonce, Some(*taker))
        } else if let EngineMsg::CloseDebt { config, nonce, .. } = msg {
            (*config, *nonce, None)
        } else {
            return None;
        };
        let mut actors = vec![
            Actor::new(config, self.engine),
            self.chain.clock(),
            debt_actor(self.engine, config, nonce),
            accounting_actor(self.engine, config),
        ];
        if let Some(taker) = taker {
            let definition = protocol_def(self.engine, config);
            actors.extend([
                Actor::new(taker, self.token),
                self.holding(taker, self.stable_def),
                Actor::new(self.stable_def, self.token),
                Actor::new(definition, self.token),
                self.holding(taker, definition),
            ]);
        }
        Some(actors)
    }

    /// The actors a `CheckSolvency` at `config` touches, the config (root) first: those of
    /// `SettleDebt`, the clock, the reference oracle (if the config names one) and the config's
    /// bound host `P` (a `Freeze` decision's `EngineFreezeNewDebt`).
    pub fn solvency_actors(&self, config: AccountId) -> Vec<Actor> {
        let c = self.config_state_of(config);
        let mut actors = self.settle_actors(config);
        actors.push(self.chain.clock());
        actors.extend(c.protocol_token_oracle);
        actors.push(c.host);
        actors
    }

    fn settle_actors(&self, config: AccountId) -> Vec<Actor> {
        let buffer = buffer_holding(self.engine, self.token, config);
        vec![
            Actor::new(config, self.engine),
            accounting_actor(self.engine, config),
            self.holding(buffer.account_id, self.stable_def),
            buffer,
            Actor::new(self.stable_def, self.token),
        ]
    }

    /// The actors a `TakeSurplus` / `CloseSurplus` at `config` touches: the config (root), the
    /// clock, the surplus auction, its vault's token actor and stablecoin holding, the
    /// accounting; for a take also the taker's token actor, its protocol and stablecoin holdings
    /// and the protocol definition (burn); for a close the buffer's stablecoin holding.
    ///
    /// # Panics
    /// For any other message (not a system root).
    pub fn surplus_actors(&self, msg: &EngineMsg) -> Vec<Actor> {
        let (config, nonce, taker) = if let EngineMsg::TakeSurplus {
            taker,
            config,
            nonce,
            ..
        } = msg
        {
            (*config, *nonce, Some(*taker))
        } else if let EngineMsg::CloseSurplus { config, nonce, .. } = msg {
            (*config, *nonce, None)
        } else {
            panic!("not a system root: {msg:?}")
        };
        let surplus = surplus_actor(self.engine, config, nonce);
        let vault = surplus_vault_holding(self.engine, self.token, surplus.account_id);
        let mut actors = vec![
            Actor::new(config, self.engine),
            self.chain.clock(),
            surplus,
            vault,
            self.holding(vault.account_id, self.stable_def),
            accounting_actor(self.engine, config),
        ];
        let Some(taker) = taker else {
            let buffer = buffer_holding(self.engine, self.token, config).account_id;
            actors.push(self.holding(buffer, self.stable_def));
            return actors;
        };
        let definition = protocol_def(self.engine, config);
        actors.extend([
            Actor::new(taker, self.token),
            self.holding(taker, definition),
            Actor::new(definition, self.token),
            self.holding(taker, self.stable_def),
        ]);
        actors
    }

    /// Sends the system root `msg` at its config with `evidence`, signed by the signer `signer`
    /// (funded if new), or by no one if `signer` is empty.
    pub fn send_system(
        &mut self,
        msg: &EngineMsg,
        signer: &str,
        evidence: &[PublicAccountEvidence],
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let actors = self.system_actors(msg);
        let root = *actors.first().expect("the root config");
        let key: Option<PrivateKey> = (!signer.is_empty()).then(|| {
            let (key, account) = self.chain.key(signer);
            if !self.chain.account_exists(account) {
                self.chain.fund(account, 1);
            }
            key
        });
        let signers: Vec<&PrivateKey> = key.iter().collect();
        self.chain
            .send_with_evidence(root, &actors, &signers, evidence, msg)
    }

    /// The state of the config at `config`.
    fn config_state_of(&self, config: AccountId) -> ConfigState {
        self.chain
            .raw_state(Actor::new(config, self.engine))
            .and_then(|bytes| ConfigState::decode(&bytes))
            .expect("a config state")
    }

    /// The world's config state.
    pub fn config_state(&self) -> ConfigState {
        self.config_state_of(self.config.account_id)
    }

    /// The world's accounting state.
    pub fn accounting_state(&self) -> AccountingState {
        self.chain
            .raw_state(self.accounting)
            .and_then(|bytes| AccountingState::decode(&bytes))
            .expect("an accounting state")
    }

    /// The evidence a system root of the world's config needs: `Pda` for the buffer's stablecoin
    /// holding (the token program's `HOLDING` seed) while that holding's *account* is absent,
    /// else none (settlement admits an existing account, even one an evidenced read left without
    /// token state; the SDK's `buffer_evidence` uses the same rule).
    pub fn buffer_evidence(&self) -> Vec<PublicAccountEvidence> {
        let buffer = self.buffer().account_id;
        let holding = self.holding(buffer, self.stable_def).account_id;
        if self.chain.account_exists(holding) {
            return Vec::new();
        }
        vec![PublicAccountEvidence::Pda {
            program: self.token,
            seed: holding_seed(buffer, self.stable_def),
        }]
    }
}

/// The surplus auction on the [`World`] (rows 10-12).
impl World {
    /// [`World::bare`] initialised by [`World::init_system`] with `params`; when they enable the
    /// reference oracle, a fresh, never-published one ([`World::new_ref_oracle`]).
    pub fn with_system(params: EngineParamsArgs) -> Self {
        let mut w = Self::bare();
        let ref_oracle = params
            .ref_oracle_enabled
            .then(|| w.new_ref_oracle("ref-oracle"));
        w.init_system(params, ref_oracle);
        w
    }

    /// On a bare world: the admin's `Init` with `params` (the protocol token enabled), the
    /// reference oracle `ref_oracle` and [`TAKER_GENESIS`] protocol tokens credited to the
    /// `"taker"` signer; then `SetCollateral` and the host bound, as [`World::new`].
    ///
    /// # Panics
    /// If any of these transactions fails.
    pub fn init_system(&mut self, params: EngineParamsArgs, ref_oracle: Option<Actor>) {
        let (_, taker) = self.chain.key(TAKER);
        let genesis = ProtocolGenesis {
            to: taker,
            supply: TAKER_GENESIS,
        };
        let params = EngineParamsArgs {
            protocol_token_enabled: true,
            ..params
        };
        let key = self.admin_key.clone();
        let init = self.init_msg_full(params, ref_oracle, Some(genesis));
        self.send_engine(self.admin, &[&key], &init)
            .expect("the engine is initialised");
        let set = self.set_collateral_msg(Self::collateral_args());
        self.send_engine(self.admin, &[&key], &set)
            .expect("the collateral is configured");
        self.bind_host().expect("the host binds the config actor");
    }

    /// A minimal oracle at `label`'s account, published to by the world's publisher, never
    /// published yet (price and timestamp zero).
    pub fn new_ref_oracle(&mut self, label: &str) -> Actor {
        let (key, account) = self.chain.key(label);
        let oracle = Actor::new(account, self.oracle.program_account_id);
        let init = OracleMsg::Init {
            publisher: self.publisher,
        };
        self.chain
            .send(oracle, &[oracle], &[&key], &init)
            .expect("the reference oracle is initialised");
        oracle
    }

    /// The publisher publishes `price_ray` at `timestamp_ms` on the config's reference oracle.
    ///
    /// # Panics
    /// If the config names no reference oracle.
    pub fn set_ref_price(&mut self, price_ray: u128, timestamp_ms: u64) {
        let oracle = self
            .config_state()
            .protocol_token_oracle
            .expect("a reference oracle");
        let publisher = self.publisher;
        let publish = OracleMsg::Publish {
            price_ray,
            timestamp_ms,
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

    /// The surplus auction `surplus_actor(E, config, nonce)`.
    pub fn surplus(&self, nonce: u64) -> Actor {
        surplus_actor(self.engine, self.config.account_id, nonce)
    }

    /// The surplus auction's vault token actor `(pda(E, ["SURPVAULT", S]), T)`; its stablecoin is
    /// `holding(vault.account_id, stable_def)`.
    pub fn surplus_vault(&self, nonce: u64) -> Actor {
        surplus_vault_holding(self.engine, self.token, self.surplus(nonce).account_id)
    }

    /// The state of surplus auction `nonce`.
    pub fn surplus_state(&self, nonce: u64) -> SurplusAuctionState {
        self.chain
            .raw_state(self.surplus(nonce))
            .and_then(|bytes| SurplusAuctionState::decode(&bytes))
            .expect("a surplus auction state")
    }

    /// Anyone (no signature) starts surplus auction `nonce`, claiming the world's time, with the
    /// buffer's evidence.
    pub fn start_surplus(&mut self, nonce: u64) -> Result<Vec<TransactionEvent>, LeeError> {
        let msg = EngineMsg::StartSurplus {
            config: self.config.account_id,
            nonce,
            now_ms: self.now_ms,
        };
        let evidence = self.buffer_evidence();
        self.send_system(&msg, "", &evidence)
    }

    /// The `"taker"` signer takes `amount_coin` from surplus auction `nonce`, paying at most
    /// `max_protocol_in`, claiming the world's time.
    pub fn take_surplus(
        &mut self,
        nonce: u64,
        amount_coin: u128,
        max_protocol_in: u128,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let msg = EngineMsg::TakeSurplus {
            taker: self.chain.key(TAKER).1,
            config: self.config.account_id,
            nonce,
            amount_coin,
            max_protocol_in,
            now_ms: self.now_ms,
        };
        self.send_system(&msg, TAKER, &[])
    }

    /// Anyone (no signature) closes surplus auction `nonce`, claiming the world's time.
    pub fn close_surplus(&mut self, nonce: u64) -> Result<Vec<TransactionEvent>, LeeError> {
        let msg = EngineMsg::CloseSurplus {
            config: self.config.account_id,
            nonce,
            now_ms: self.now_ms,
        };
        let evidence = self.buffer_evidence();
        self.send_system(&msg, "", &evidence)
    }
}

/// The debt auction on the [`World`] (rows 13-15).
impl World {
    /// The debt auction `debt_actor(E, config, nonce)`.
    pub fn debt(&self, nonce: u64) -> Actor {
        debt_actor(self.engine, self.config.account_id, nonce)
    }

    /// The state of debt auction `nonce`.
    pub fn debt_state(&self, nonce: u64) -> DebtAuctionState {
        self.chain
            .raw_state(self.debt(nonce))
            .and_then(|bytes| DebtAuctionState::decode(&bytes))
            .expect("a debt auction state")
    }

    /// Anyone (no signature) starts debt auction `nonce`, claiming the world's time, with the
    /// buffer's evidence.
    pub fn start_debt(&mut self, nonce: u64) -> Result<Vec<TransactionEvent>, LeeError> {
        self.start_debt_at(nonce, self.now_ms)
    }

    /// Anyone starts debt auction `nonce`, claiming `now_ms` (the clock accepts a claim up to
    /// `slack_ms` behind its time).
    pub fn start_debt_at(
        &mut self,
        nonce: u64,
        now_ms: u64,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let msg = EngineMsg::StartDebt {
            config: self.config.account_id,
            nonce,
            now_ms,
        };
        let evidence = self.buffer_evidence();
        self.send_system(&msg, "", &evidence)
    }

    /// The `"taker"` signer takes `amount_coin` from debt auction `nonce` for at least
    /// `min_protocol_out`, claiming the world's time.
    pub fn take_debt(
        &mut self,
        nonce: u64,
        amount_coin: u128,
        min_protocol_out: u128,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        self.take_debt_as(TAKER, nonce, amount_coin, min_protocol_out)
    }

    /// The signer `label` takes from debt auction `nonce`, claiming the world's time.
    pub fn take_debt_as(
        &mut self,
        label: &str,
        nonce: u64,
        amount_coin: u128,
        min_protocol_out: u128,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let msg = EngineMsg::TakeDebt {
            taker: self.chain.key(label).1,
            config: self.config.account_id,
            nonce,
            amount_coin,
            min_protocol_out,
            now_ms: self.now_ms,
        };
        self.send_system(&msg, label, &[])
    }

    /// Anyone (no signature) closes debt auction `nonce`, claiming the world's time.
    pub fn close_debt(&mut self, nonce: u64) -> Result<Vec<TransactionEvent>, LeeError> {
        let msg = EngineMsg::CloseDebt {
            config: self.config.account_id,
            nonce,
            now_ms: self.now_ms,
        };
        self.send_system(&msg, "", &[])
    }
}

/// `check_solvency` on the [`World`] (row 16).
impl World {
    /// `CheckSolvency` at the world's config, claiming the world's time.
    pub fn check_solvency_msg(&self) -> EngineMsg {
        EngineMsg::CheckSolvency {
            config: self.config.account_id,
            now_ms: self.now_ms,
        }
    }

    /// Anyone (no signature) checks the world's config's solvency, claiming the world's time,
    /// with the buffer's evidence.
    pub fn check_solvency(&mut self) -> Result<Vec<TransactionEvent>, LeeError> {
        let msg = self.check_solvency_msg();
        let evidence = self.buffer_evidence();
        self.send_system(&msg, "", &evidence)
    }

    /// The host's params state at `P`.
    pub fn host_params(&self) -> Params {
        self.chain
            .raw_state(self.params)
            .and_then(|bytes| Params::decode(&bytes))
            .expect("the host's params")
    }

    /// Whether the host refuses new debt (`EngineFreezeNewDebt` was accepted).
    pub fn host_frozen(&self) -> bool {
        self.host_params().is_frozen
    }
}
