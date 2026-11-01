//! The SDK's plans and simulation against the in-process harness (no sequencer): every plan
//! declares the actors World declares and every actor its run changed; gas fitting; and a plan
//! missing a recipient or the evidence of a fresh one is caught by the SDK's simulation path.

use std::collections::BTreeSet;

use engine_core::msg::EngineMsg;
use harness::{
    Chain, World,
    bidding::Bidder,
    world::{RAY, SALT, SLACK_MS},
};
use lee::{AccountId, Actor, PrivateKey};
use sdk::{
    BidArgs, Deployment, InitArgs, LiquidateArgs, MAX_GAS_EXEC, Plan, SdkError, StateSource,
    fit_gas, plan_bid, plan_claim_leftover, plan_init, plan_liquidate, plan_set_collateral,
    plan_settle_empty, simulate_with,
};

const COLLATERAL: u128 = 500_000;
const DEBT: u128 = 600_000;
const TARGET: u128 = 660_000;
const FUNDS: u128 = 1_000_000;
const T: u64 = 1_000_000;

/// The harness chain as the SDK's state source.
struct InProcess<'c>(&'c Chain);

impl StateSource for InProcess<'_> {
    fn actor_state(&self, actor: Actor) -> Option<Vec<u8>> {
        self.0.raw_state(actor)
    }

    fn account_exists(&self, account: AccountId) -> bool {
        self.0.account_exists(account)
    }
}

fn set(actors: &[Actor]) -> BTreeSet<Actor> {
    actors.iter().copied().collect()
}

fn simulate(w: &World, plan: &Plan) -> Result<u64, SdkError> {
    simulate_with(&InProcess(&w.chain), w.chain.catalog(), plan, w.admin)
}

fn deployment(w: &World) -> Deployment {
    Deployment::load(&InProcess(&w.chain), w.engine, w.config.account_id).expect("config loads")
}

/// Simulates `plan` (it must pass), applies it in-process signed by `keys`, and checks every
/// actor the run changed is declared. (LEZ itself only lets declared actors change, so the
/// subset check guards that contract; what pins the plan to the harness is the set equality with
/// World's own actor lists asserted before each `run`, plus the plan simulating and applying.)
fn run(w: &mut World, label: &str, plan: &Plan, keys: &[&PrivateKey]) {
    let cycles = simulate(w, plan).unwrap_or_else(|err| panic!("{label} simulates: {err}"));
    assert!(cycles > 0, "{label}: no cycles");
    fit_gas(cycles).unwrap_or_else(|err| panic!("{label} fits the gas cap: {err}"));
    let msg: EngineMsg = borsh::from_slice(&plan.message).expect("an engine message");
    w.chain
        .send_with_evidence(plan.root, &plan.actors, keys, &plan.evidence, &msg)
        .unwrap_or_else(|err| panic!("{label} applies: {err}"));
    let changed = w.chain.changed_actors();
    assert!(!changed.is_empty(), "{label} changed nothing");
    let declared = set(&plan.actors);
    for actor in changed {
        assert!(
            declared.contains(&actor),
            "{label}: changed {actor:?} is not declared"
        );
    }
}

#[test]
fn fit_gas_adds_twenty_percent_and_caps() {
    assert_eq!(fit_gas(0).expect("fits"), 0);
    assert_eq!(fit_gas(1).expect("fits"), 2);
    assert_eq!(fit_gas(100).expect("fits"), 120);
    assert_eq!(fit_gas(1_234_567).expect("fits"), 1_481_481);
    assert_eq!(fit_gas(8_333_333).expect("fits"), MAX_GAS_EXEC);
    assert_eq!(MAX_GAS_EXEC, 10_000_000);
    assert!(matches!(
        fit_gas(8_333_334),
        Err(SdkError::GasAboveCap {
            gas: 10_000_001,
            ..
        })
    ));
    assert!(matches!(
        fit_gas(u64::MAX),
        Err(SdkError::GasAboveCap { .. })
    ));
}

/// R-P3: each instruction of the slice, built by the SDK from `engine_core::actors` and state,
/// declares exactly World's actor list and evidence, and a superset of what its run changed.
#[test]
fn plans_declare_every_touched_actor() {
    let mut w = World::bare_at(RAY / 2);
    let (admin_key, keeper_key) = (w.admin_key.clone(), w.keeper_key.clone());

    let init = plan_init(
        w.engine,
        w.admin,
        InitArgs {
            salt: SALT,
            params: World::engine_params(),
            host: w.params,
            token: w.token,
            stable_def: w.stable_def,
            oracle: w.oracle,
            clock: w.chain.clock(),
            slack_ms: SLACK_MS,
        },
    )
    .expect("init plan");
    assert_eq!(init.message, borsh::to_vec(&w.init_msg()).expect("encodes"));
    assert_eq!(init.root, Actor::new(w.admin, w.engine));
    assert_eq!(init.signers, vec![w.admin]);
    assert!(init.evidence.is_empty());
    assert_eq!(
        set(&init.actors),
        set(&w.proxy_actors(w.admin, &w.init_msg()))
    );
    run(&mut w, "init", &init, &[&admin_key]);

    let d = deployment(&w);
    assert_eq!(d.config, w.config);
    let set_msg = w.set_collateral_msg(World::collateral_args());
    let setc = plan_set_collateral(&d, w.collateral_def, World::collateral_args(), w.now_ms)
        .expect("set-collateral plan");
    assert_eq!(setc.message, borsh::to_vec(&set_msg).expect("encodes"));
    assert_eq!(setc.root, Actor::new(w.admin, w.engine));
    assert_eq!(setc.signers, vec![w.admin]);
    assert!(setc.evidence.is_empty());
    assert_eq!(set(&setc.actors), set(&w.proxy_actors(w.admin, &set_msg)));
    run(&mut w, "set_collateral", &setc, &[&admin_key]);
    w.bind_host().expect("the host binds the config actor");

    w.set_now_ms(T);
    let position = w.open_unsafe_position(COLLATERAL, DEBT);
    w.set_price(RAY / 2);
    let host_actors = [Actor::new(position, w.host_prog), w.host_vault(position)];
    let liq = plan_liquidate(
        &d,
        &LiquidateArgs {
            keeper: w.keeper,
            collateral_def: w.collateral_def,
            position,
            nonce: 0,
            now_ms: T,
        },
        &host_actors,
    )
    .expect("liquidate plan");
    let liq_msg = w.liquidate_msg(position, 0, T);
    assert_eq!(liq.message, borsh::to_vec(&liq_msg).expect("encodes"));
    assert_eq!(liq.root, Actor::new(w.keeper, w.engine));
    assert_eq!(liq.signers, vec![w.keeper]);
    assert_eq!(liq.evidence, w.liquidate_evidence(position, 0).to_vec());
    assert_eq!(set(&liq.actors), set(&w.proxy_actors(w.keeper, &liq_msg)));
    run(&mut w, "liquidate", &liq, &[&keeper_key]);

    let bidder: Bidder = w.fund_bidder("bidder", FUNDS);
    for (label, max_debt_in) in [("partial bid", 100_000), ("final bid", TARGET)] {
        let args = BidArgs {
            bidder: bidder.account,
            collateral_def: w.collateral_def,
            position,
            nonce: 0,
            max_debt_in,
            min_collateral_out: 0,
            now_ms: T,
        };
        let bid = plan_bid(&InProcess(&w.chain), &d, &args).expect("bid plan");
        let bid_msg = w.bid_msg(bidder.account, position, 0, (max_debt_in, 0), T);
        assert_eq!(bid.message, borsh::to_vec(&bid_msg).expect("encodes"));
        assert_eq!(bid.root, w.collateral);
        assert_eq!(bid.signers, vec![bidder.account]);
        assert_eq!(bid.evidence, w.bid_evidence(), "{label}");
        assert_eq!(
            set(&bid.actors),
            set(&w.bid_actors(bidder.account, position, 0))
        );
        run(&mut w, label, &bid, &[&bidder.key]);
    }

    let owner = w.owner_of(position);
    let (owner_account, owner_key) = (owner.account, owner.key.clone());
    let claim = plan_claim_leftover(&d, owner_account, position, 0).expect("claim plan");
    let claim_msg = w.claim_msg(position, 0);
    assert_eq!(claim.message, borsh::to_vec(&claim_msg).expect("encodes"));
    assert_eq!(claim.root, Actor::new(owner_account, w.engine));
    assert_eq!(claim.signers, vec![owner_account]);
    assert!(claim.evidence.is_empty());
    assert_eq!(
        set(&claim.actors),
        set(&w.proxy_actors(owner_account, &claim_msg))
    );
    run(&mut w, "claim_leftover", &claim, &[&owner_key]);

    // An empty-lot auction (ruling R-9a numbers): only SettleEmpty settles it.
    let empty = w.open_unsafe_position(3, 10_000);
    w.set_price(RAY / 2);
    w.liquidate(empty, 0).expect("the reward takes the lot");
    let settle = plan_settle_empty(&d, w.collateral_def, empty, 0).expect("settle plan");
    assert_eq!(
        settle.message,
        borsh::to_vec(&w.settle_empty_msg(empty, 0)).expect("encodes")
    );
    assert_eq!(settle.root, w.config);
    assert!(settle.signers.is_empty());
    assert!(settle.evidence.is_empty());
    assert_eq!(set(&settle.actors), set(&w.settle_empty_actors(empty, 0)));
    run(&mut w, "settle_empty", &settle, &[]);
}

/// Review Focus 4: a bid plan that leaves out the bidder's holding (the recipient of the
/// collateral and payer of the charge), or the evidence for the fresh buffer holding, is caught
/// by the SDK's simulation path as `WouldRevert` naming the problem, without a panic; the plan as
/// built simulates.
#[test]
fn undeclared_fresh_recipient_is_caught_in_simulation() {
    let mut w = World::new_at(RAY / 2);
    w.set_now_ms(T);
    let position = w.open_unsafe_position(COLLATERAL, DEBT);
    w.set_price(RAY / 2);
    w.liquidate(position, 0).expect("the keeper liquidates");
    let bidder = w.fund_bidder("bidder", FUNDS);
    let d = deployment(&w);
    let args = BidArgs {
        bidder: bidder.account,
        collateral_def: w.collateral_def,
        position,
        nonce: 0,
        max_debt_in: TARGET,
        min_collateral_out: 0,
        now_ms: T,
    };
    let plan = plan_bid(&InProcess(&w.chain), &d, &args).expect("bid plan");
    assert!(
        !w.chain.account_exists(w.buffer().account_id),
        "the buffer is fresh before the first bid"
    );
    assert_eq!(plan.evidence.len(), 1, "the fresh buffer carries evidence");

    let holding = Actor::new(bidder.account, w.token);
    let without_holding = Plan {
        actors: plan
            .actors
            .iter()
            .copied()
            .filter(|actor| *actor != holding)
            .collect(),
        ..plan.clone()
    };
    match simulate(&w, &without_holding) {
        Err(SdkError::WouldRevert(reason)) => {
            println!("undeclared holding: {reason}");
            assert!(
                reason.contains("UndeclaredActor") && reason.contains(&bidder.account.to_string()),
                "the revert names the undeclared holding: {reason}"
            );
        }
        other => panic!("expected WouldRevert for the undeclared holding, got {other:?}"),
    }

    let without_evidence = Plan {
        evidence: Vec::new(),
        ..plan.clone()
    };
    match simulate(&w, &without_evidence) {
        Err(SdkError::WouldRevert(reason)) => {
            println!("missing buffer evidence: {reason}");
            assert!(
                reason.contains("UnadmittedPublicActor")
                    && reason.contains(&w.buffer().account_id.to_string()),
                "the revert names the unadmitted buffer: {reason}"
            );
        }
        other => panic!("expected WouldRevert for the missing evidence, got {other:?}"),
    }

    let cycles = simulate(&w, &plan).expect("the plan as built simulates");
    assert!(cycles > 0);
}
