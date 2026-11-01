//! The mock CDP host: owner actions through the proxy, the engine binding, and the
//! `LiquidatePosition` hook (whole-actor check, oracle read, sweep to custody, then `Seized`).

use borsh::BorshSerialize;
use harness::{Chain, expect_revert};
use lee::{AccountId, Actor, PrivateKey, PublicAccountEvidence, error::LeeError};
use lee_core::program::TransactionEvent;
use mock_host_core::{
    HostMsg, Params, Position, Seized, TAG_POSITION, position_id, vault_id, vault_seed,
};
use oracle_core::OracleMsg;
use token_core::{Definition, Holding, TokenMsg};

const RAY: u128 = 1_000_000_000_000_000_000_000_000_000;
const MIN_CR_RAY: u128 = RAY * 3 / 2;
const OWNER_COLLATERAL: u128 = 10_000;
const COLLATERAL: u128 = 1_000;
const DEBT: u128 = 600;
/// At price 2, 1_000 collateral is worth 2_000 against 1.5 × 600 = 900: safe.
const SAFE_PRICE: u128 = 2 * RAY;
/// At price 0.5, 1_000 collateral is worth 500 against 900: unsafe.
const UNSAFE_PRICE: u128 = RAY / 2;

/// The relay guests' root message: forward `message` to `to` as a sync message.
#[derive(BorshSerialize)]
struct Forward {
    to: Actor,
    message: Vec<u8>,
}

fn forward(to: Actor, msg: &impl BorshSerialize) -> Forward {
    Forward {
        to,
        message: borsh::to_vec(msg).expect("encodes"),
    }
}

struct World {
    chain: Chain,
    host: AccountId,
    params_key: PrivateKey,
    params: Actor,
    owner_key: PrivateKey,
    owner: AccountId,
    proxy: Actor,
    owner_holding: Actor,
    collateral_def: Actor,
    stable_def: Actor,
    oracle: Actor,
    publisher: Actor,
    engine: Actor,
    custody: Actor,
    position: Actor,
    vault: Actor,
}

impl World {
    /// Token, oracle (price `SAFE_PRICE`), and an initialised host; the engine actor `(K, relay)`
    /// is bound only if `bind`. The owner holds `OWNER_COLLATERAL` and no position yet.
    fn new(bind: bool) -> Self {
        let mut chain = Chain::new(&[
            ("token", methods::TOKEN_ELF),
            ("oracle", methods::ORACLE_ELF),
            ("mock_host", methods::MOCK_HOST_ELF),
            ("relay", methods::RELAY_ELF),
            ("twin", methods::RELAY_TWIN_ELF),
        ]);
        let (token, host, relay) = (
            chain.program("token"),
            chain.program("mock_host"),
            chain.program("relay"),
        );
        let (params_key, params_account) = chain.key("params");
        let (owner_key, owner) = chain.key("owner");
        let (cdef_key, cdef_account) = chain.key("collateral-def");
        let (sdef_key, sdef_account) = chain.key("stable-def");
        let (oracle_key, oracle_account) = chain.key("oracle");
        let (_, publisher_account) = chain.key("publisher");
        let (_, k) = chain.key("engine-config");
        let (_, custody_account) = chain.key("custody");
        for account in [params_account, owner, publisher_account, k, custody_account] {
            chain.fund(account, 1);
        }
        let params = Actor::new(params_account, host);
        let mut w = Self {
            host,
            params_key,
            params,
            owner,
            proxy: Actor::new(owner, host),
            owner_holding: Actor::new(owner, token),
            collateral_def: Actor::new(cdef_account, token),
            stable_def: Actor::new(sdef_account, token),
            oracle: Actor::new(oracle_account, chain.program("oracle")),
            publisher: Actor::new(publisher_account, relay),
            engine: Actor::new(k, relay),
            custody: Actor::new(custody_account, token),
            position: Actor::new(position_id(host, owner), host),
            vault: Actor::new(vault_id(host, owner), token),
            owner_key,
            chain,
        };

        let (cdef, sdef, owner_holding) = (w.collateral_def, w.stable_def, w.owner_holding);
        let collateral_def = TokenMsg::NewDefinition {
            minter: None,
            initial_to: Some(owner),
            initial_supply: OWNER_COLLATERAL,
        };
        w.chain
            .send(cdef, &[cdef, owner_holding], &[&cdef_key], &collateral_def)
            .expect("the collateral definition is created");
        let stable_def = TokenMsg::NewDefinition {
            minter: Some(params),
            initial_to: None,
            initial_supply: 0,
        };
        w.chain
            .send(sdef, &[sdef], &[&sdef_key], &stable_def)
            .expect("the stablecoin definition is created");

        let oracle = w.oracle;
        let init_oracle = OracleMsg::Init {
            publisher: w.publisher,
        };
        w.chain
            .send(oracle, &[oracle], &[&oracle_key], &init_oracle)
            .expect("the oracle is initialised");
        w.set_price(SAFE_PRICE);

        let init = HostMsg::Init {
            admin: params,
            token,
            stable_def: sdef_account,
            collateral_def: cdef_account,
            oracle,
            redemption_price_ray: RAY,
            min_cr_ray: MIN_CR_RAY,
        };
        w.chain
            .send(params, &[params], &[&w.params_key], &init)
            .expect("the host is initialised");
        if bind {
            let engine_actor = w.engine;
            w.bind(engine_actor).expect("the engine is bound");
        }
        w
    }

    fn bind(&mut self, engine_actor: Actor) -> Result<Vec<TransactionEvent>, LeeError> {
        let params = self.params;
        let msg = HostMsg::SetLiquidationEngine { engine_actor };
        self.chain
            .send(params, &[params], &[&self.params_key], &msg)
    }

    fn set_price(&mut self, price_ray: u128) {
        let (oracle, publisher) = (self.oracle, self.publisher);
        let publish = OracleMsg::Publish {
            price_ray,
            timestamp_ms: 1,
            confidence: 0,
        };
        self.chain
            .send(
                publisher,
                &[publisher, oracle],
                &[],
                &forward(oracle, &publish),
            )
            .expect("the publisher publishes");
    }

    /// The owner's root message at its proxy, wrapping `message` for `P`.
    fn proxied(&self, message: HostMsg) -> HostMsg {
        HostMsg::Proxy {
            params: self.params.account_id,
            message: Box::new(message),
        }
    }

    /// The owner opens a position with `collateral`, signing (or not) at its proxy. The fresh
    /// vault holding is admitted by PDA evidence.
    fn open(
        &mut self,
        owner: AccountId,
        collateral: u128,
        sign: bool,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let msg = self.proxied(HostMsg::OpenPosition { owner, collateral });
        let actors = [
            self.proxy,
            self.params,
            self.position,
            self.owner_holding,
            self.vault,
        ];
        let evidence = [PublicAccountEvidence::Pda {
            program: self.host,
            seed: vault_seed(self.owner),
        }];
        let signers: &[&PrivateKey] = if sign { &[&self.owner_key] } else { &[] };
        let proxy = self.proxy;
        self.chain
            .send_with_evidence(proxy, &actors, signers, &evidence, &msg)
    }

    fn generate_debt(&mut self, normalized_debt: u128) -> Result<Vec<TransactionEvent>, LeeError> {
        let msg = self.proxied(HostMsg::GenerateDebt { normalized_debt });
        let actors = [
            self.proxy,
            self.params,
            self.position,
            self.stable_def,
            self.owner_holding,
        ];
        let proxy = self.proxy;
        self.chain.send(proxy, &actors, &[&self.owner_key], &msg)
    }

    /// A position with `COLLATERAL` and `DEBT`.
    fn with_position(bind: bool) -> Self {
        let mut w = Self::new(bind);
        let owner = w.owner;
        w.open(owner, COLLATERAL, true).expect("the position opens");
        w.generate_debt(DEBT).expect("debt is generated");
        w
    }

    /// `from` (a relay actor) sends `LiquidatePosition` to `P` with itself as `reply_to`.
    fn liquidate(&mut self, from: Actor) -> Result<Vec<TransactionEvent>, LeeError> {
        let params = self.params;
        let msg = HostMsg::LiquidatePosition {
            position: self.position.account_id,
            custody: self.custody.account_id,
            reply_to: from,
        };
        let actors = [
            from,
            params,
            self.oracle,
            self.position,
            self.vault,
            self.custody,
        ];
        self.chain.send(from, &actors, &[], &forward(params, &msg))
    }

    fn balance(&self, holding: Actor, definition: Actor) -> u128 {
        self.chain
            .state::<Holding>(holding)
            .map_or(0, |h| h.balance(definition.account_id))
    }

    fn position_state(&self) -> Option<Position> {
        self.chain
            .raw_state(self.position)
            .map(|bytes| Position::decode(&bytes).expect("a position"))
    }

    fn params_state(&self) -> Params {
        let bytes = self.chain.raw_state(self.params).expect("params exist");
        Params::decode(&bytes).expect("params")
    }

    fn expected_position(&self, collateral: u128, normalized_debt: u128) -> Position {
        Position {
            tag: TAG_POSITION,
            params: self.params,
            owner: self.owner,
            collateral,
            normalized_debt,
            vault: self.vault.account_id,
        }
    }
}

#[test]
fn open_position_and_generate_debt_mint_stablecoin() {
    let mut w = World::new(true);
    let (owner, cdef, sdef) = (w.owner, w.collateral_def, w.stable_def);

    // Unsigned at the proxy, or naming another owner, is refused.
    expect_revert(w.open(owner, COLLATERAL, false), "host: Unauthorized");
    let (_, stranger) = w.chain.key("stranger");
    expect_revert(w.open(stranger, COLLATERAL, true), "host: OwnerMismatch");
    assert_eq!(w.position_state(), None);

    w.open(owner, COLLATERAL, true).expect("the position opens");
    assert_eq!(w.position_state(), Some(w.expected_position(COLLATERAL, 0)));
    assert_eq!(w.balance(w.vault, cdef), COLLATERAL);
    assert_eq!(
        w.balance(w.owner_holding, cdef),
        OWNER_COLLATERAL - COLLATERAL
    );

    // One position per owner.
    expect_revert(w.open(owner, COLLATERAL, true), "host: PositionExists");

    w.generate_debt(DEBT).expect("debt is generated");
    assert_eq!(
        w.position_state(),
        Some(w.expected_position(COLLATERAL, DEBT))
    );
    assert_eq!(w.balance(w.owner_holding, sdef), DEBT);
    let supply = w
        .chain
        .state::<Definition>(sdef)
        .expect("stable definition")
        .supply;
    assert_eq!(supply, DEBT);
}

#[test]
fn liquidate_refused_before_binding_and_from_any_other_actor() {
    let mut w = World::with_position(false);
    w.set_price(UNSAFE_PRICE);
    let engine = w.engine;
    let before = w.position_state();

    expect_revert(w.liquidate(engine), "host: NotEngine");

    w.bind(engine).expect("the engine is bound");
    // A rogue config of the same (engine) program, and an attacker program at the config account.
    let (_, rogue_account) = w.chain.key("rogue-config");
    w.chain.fund(rogue_account, 1);
    let rogue = Actor::new(rogue_account, w.chain.program("relay"));
    let attacker = Actor::new(engine.account_id, w.chain.program("twin"));
    expect_revert(w.liquidate(rogue), "host: NotEngine");
    expect_revert(w.liquidate(attacker), "host: NotEngine");

    // A root LiquidatePosition at P has no sender at all.
    let params = w.params;
    let root = HostMsg::LiquidatePosition {
        position: w.position.account_id,
        custody: w.custody.account_id,
        reply_to: engine,
    };
    expect_revert(
        w.chain.send(params, &[params], &[], &root),
        "host: NotEngine",
    );

    // Rebinding is admin-only and root-only.
    let rebind = HostMsg::SetLiquidationEngine {
        engine_actor: attacker,
    };
    expect_revert(
        w.chain.send(params, &[params], &[], &rebind),
        "host: Unauthorized",
    );

    assert_eq!(w.position_state(), before);
    assert_eq!(w.balance(w.vault, w.collateral_def), COLLATERAL);
    assert_eq!(w.params_state().engine_actor, Some(engine));
}

#[test]
fn liquidate_refused_while_safe() {
    let mut w = World::with_position(true);
    let engine = w.engine;
    expect_revert(w.liquidate(engine), "host: PositionSafe");
    assert_eq!(
        w.position_state(),
        Some(w.expected_position(COLLATERAL, DEBT))
    );
    assert_eq!(w.balance(w.vault, w.collateral_def), COLLATERAL);
    assert_eq!(w.balance(w.custody, w.collateral_def), 0);
    assert_eq!(w.params_state().pending, None);
    assert_eq!(w.chain.raw_state(engine), None, "no report was sent");
}

/// The bound engine (a relay sink that stores what it receives) liquidates an unsafe position:
/// custody holds the whole vault, the position is zeroed and the stored report is the `Seized`
/// of the position. This checks content and final balances only; the Transfer-before-`Seized`
/// order is pinned by `liquidate_opens_an_auction_and_pays_the_keeper` (liquidate.rs), whose
/// keeper reward is paid from the auction vault inside the `Seized` subtree (ADR-060).
#[test]
fn liquidate_sweeps_to_custody_then_reports_seized() {
    let mut w = World::with_position(true);
    w.set_price(UNSAFE_PRICE);
    let engine = w.engine;
    w.liquidate(engine).expect("the position is liquidated");

    assert_eq!(w.balance(w.custody, w.collateral_def), COLLATERAL);
    assert_eq!(w.balance(w.vault, w.collateral_def), 0);
    assert_eq!(w.position_state(), Some(w.expected_position(0, 0)));
    assert_eq!(w.params_state().pending, None);

    let report = w.chain.raw_state(engine).expect("the engine got a report");
    assert_eq!(report.first(), Some(&mock_host_core::SEIZED_INDEX));
    assert_eq!(
        Seized::decode(&report),
        Some(Seized {
            position: w.position.account_id,
            owner: w.owner,
            collateral: COLLATERAL,
            normalized_debt: DEBT,
            nominal_debt: DEBT,
        })
    );

    // The position is empty now; a second liquidation finds nothing unsafe.
    expect_revert(w.liquidate(engine), "host: PositionSafe");
}

/// An attacker's own params actor `P'` under the same host program: its own oracle (with its own
/// publisher) and its own relay bound as `P'`'s engine.
struct RogueParams {
    params: Actor,
    oracle: Actor,
    engine: Actor,
}

impl RogueParams {
    /// Inits `P'` with the victim world's token and definitions, the attacker's oracle publishing
    /// `price_ray`, and binds the attacker's relay as `P'`'s engine.
    fn new(w: &mut World, price_ray: u128) -> Self {
        let (params_key, params_account) = w.chain.key("rogue-params");
        let (oracle_key, oracle_account) = w.chain.key("rogue-oracle");
        let (_, publisher_account) = w.chain.key("rogue-publisher");
        let (_, engine_account) = w.chain.key("rogue-engine");
        for account in [params_account, publisher_account, engine_account] {
            w.chain.fund(account, 1);
        }
        let relay = w.chain.program("relay");
        let params = Actor::new(params_account, w.host);
        let oracle = Actor::new(oracle_account, w.chain.program("oracle"));
        let publisher = Actor::new(publisher_account, relay);
        let engine = Actor::new(engine_account, relay);

        w.chain
            .send(
                oracle,
                &[oracle],
                &[&oracle_key],
                &OracleMsg::Init { publisher },
            )
            .expect("the rogue oracle is initialised");
        let publish = OracleMsg::Publish {
            price_ray,
            timestamp_ms: 1,
            confidence: 0,
        };
        w.chain
            .send(
                publisher,
                &[publisher, oracle],
                &[],
                &forward(oracle, &publish),
            )
            .expect("the rogue publisher publishes");

        let token = w.owner_holding.program_account_id;
        let init = HostMsg::Init {
            admin: params,
            token,
            stable_def: w.stable_def.account_id,
            collateral_def: w.collateral_def.account_id,
            oracle,
            redemption_price_ray: RAY,
            min_cr_ray: MIN_CR_RAY,
        };
        w.chain
            .send(params, &[params], &[&params_key], &init)
            .expect("the rogue params actor is initialised");
        let bind = HostMsg::SetLiquidationEngine {
            engine_actor: engine,
        };
        w.chain
            .send(params, &[params], &[&params_key], &bind)
            .expect("the rogue engine is bound to P'");
        Self {
            params,
            oracle,
            engine,
        }
    }
}

/// A position obeys only the `P` that created it: a `Seize` that a foreign params actor `P'` of
/// the same host program sends (after its own bound engine asked and its own oracle called the
/// position unsafe) fails `host: NotItsParams`, and the victim's position and vault are untouched.
#[test]
fn seize_from_another_params_actor_reverts() {
    let mut w = World::with_position(true);
    // Safe under P's oracle; P' reads its own oracle, which calls it unsafe.
    let rogue = RogueParams::new(&mut w, 1);
    let msg = HostMsg::LiquidatePosition {
        position: w.position.account_id,
        custody: w.custody.account_id,
        reply_to: rogue.engine,
    };
    let actors = [
        rogue.engine,
        rogue.params,
        rogue.oracle,
        w.position,
        w.vault,
        w.custody,
    ];
    expect_revert(
        w.chain
            .send(rogue.engine, &actors, &[], &forward(rogue.params, &msg)),
        "host: NotItsParams",
    );

    assert_eq!(
        w.position_state(),
        Some(w.expected_position(COLLATERAL, DEBT))
    );
    assert_eq!(w.balance(w.vault, w.collateral_def), COLLATERAL);
    assert_eq!(w.balance(w.custody, w.collateral_def), 0);
    assert_eq!(w.chain.raw_state(rogue.engine), None, "no report was sent");
}

/// The same guard on `AddDebt`: an owner action routed through a foreign `P'` (here the victim
/// signs a `GenerateDebt` naming `P'`) reaches the victim's position from `P'` and fails
/// `host: NotItsParams`; no debt is added and nothing is minted.
#[test]
fn add_debt_from_another_params_actor_reverts() {
    let mut w = World::with_position(true);
    let rogue = RogueParams::new(&mut w, SAFE_PRICE);
    let msg = HostMsg::Proxy {
        params: rogue.params.account_id,
        message: Box::new(HostMsg::GenerateDebt {
            normalized_debt: DEBT,
        }),
    };
    let actors = [
        w.proxy,
        rogue.params,
        w.position,
        w.stable_def,
        w.owner_holding,
    ];
    let (proxy, key) = (w.proxy, w.owner_key.clone());
    expect_revert(
        w.chain.send(proxy, &actors, &[&key], &msg),
        "host: NotItsParams",
    );

    assert_eq!(
        w.position_state(),
        Some(w.expected_position(COLLATERAL, DEBT))
    );
    assert_eq!(w.balance(w.owner_holding, w.stable_def), DEBT);
    assert_eq!(w.balance(w.vault, w.collateral_def), COLLATERAL);
}
