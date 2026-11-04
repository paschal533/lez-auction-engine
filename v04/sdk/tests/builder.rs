//! The SDK's plans and simulation against the in-process harness (no sequencer): every plan
//! declares the actors World declares and every actor its run changed; gas fitting; and a plan
//! missing a payer or a fresh recipient holding is caught by the SDK's simulation path, while a
//! fresh holding needs no evidence (ADR-062). The system plans declare exactly the actors their
//! runs touch (leave-one-out) and carry the buffer's evidence only while its holding is absent.

use std::collections::BTreeSet;

use auction_engine_params::args::{DEBT_RELIST_DELAY_MS, EngineParamsArgs};
use engine_core::{msg::EngineMsg, system_state::SystemPhase};
use harness::{
    Chain, World,
    bidding::Bidder,
    system::{TAKER, TAKER_GENESIS},
    world::{RAY, SALT, SLACK_MS},
};
use lee::{AccountId, Actor, PrivateKey, PublicAccountEvidence};
use sdk::{
    BidArgs, Deployment, InitArgs, LiquidateArgs, MAX_GAS_EXEC, Plan, SdkError,
    SetEngineParamsArgs, StateSource, TakeArgs, buffer_evidence, fit_gas, plan_bid,
    plan_check_solvency, plan_claim_leftover, plan_close_debt, plan_close_surplus, plan_init,
    plan_liquidate, plan_observe_feed, plan_set_collateral, plan_set_engine_params,
    plan_settle_debt, plan_settle_empty, plan_start_debt, plan_start_surplus, plan_take_debt,
    plan_take_surplus, simulate_with,
};
use token_core::holding_seed;

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
            ref_oracle: None,
            protocol_genesis: None,
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
    let host_vault = w.host_vault(position);
    let host_actors = [
        Actor::new(position, w.host_prog),
        host_vault,
        w.holding(host_vault.account_id, w.collateral_def),
    ];
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
    assert!(
        liq.evidence.is_empty(),
        "the fresh vault holding is granted"
    );
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
        let bid = plan_bid(&d, &args).expect("bid plan");
        let bid_msg = w.bid_msg(bidder.account, position, 0, (max_debt_in, 0), T);
        assert_eq!(bid.message, borsh::to_vec(&bid_msg).expect("encodes"));
        assert_eq!(bid.root, w.collateral);
        assert_eq!(bid.signers, vec![bidder.account]);
        assert!(bid.evidence.is_empty(), "{label}");
        assert_eq!(
            set(&bid.actors),
            set(&w.bid_actors(bidder.account, position, 0))
        );
        run(&mut w, label, &bid, &[&bidder.key]);
    }

    let owner = w.owner_of(position);
    let (owner_account, owner_key) = (owner.account, owner.key.clone());
    let claim =
        plan_claim_leftover(&d, w.collateral_def, owner_account, position, 0).expect("claim plan");
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

/// Review Focus 4: a bid plan that leaves out the bidder's token actor (the payer of the charge)
/// or the fresh buffer stablecoin holding (the penalty's recipient) is caught by the SDK's
/// simulation path as `WouldRevert` naming the actor, without a panic; the plan as built carries
/// no evidence (the fresh holding is granted by the token's `HOLDING` seed) and simulates.
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
    let plan = plan_bid(&d, &args).expect("bid plan");
    let buffer_stable = w.holding(w.buffer().account_id, w.stable_def);
    assert!(
        !w.chain.account_exists(buffer_stable.account_id),
        "the buffer holding is fresh before the first bid"
    );
    assert!(
        plan.evidence.is_empty(),
        "a fresh holding needs no evidence"
    );

    for (label, left_out) in [
        ("bidder's token actor", Actor::new(bidder.account, w.token)),
        ("fresh buffer holding", buffer_stable),
    ] {
        let without = Plan {
            actors: plan
                .actors
                .iter()
                .copied()
                .filter(|actor| *actor != left_out)
                .collect(),
            ..plan.clone()
        };
        match simulate(&w, &without) {
            Err(SdkError::WouldRevert(reason)) => {
                println!("undeclared {label}: {reason}");
                assert!(
                    reason.contains("UndeclaredActor")
                        && reason.contains(&left_out.account_id.to_string()),
                    "the revert names the undeclared {label}: {reason}"
                );
            }
            other => panic!("expected WouldRevert for the undeclared {label}, got {other:?}"),
        }
    }

    let cycles = simulate(&w, &plan).expect("the plan as built simulates");
    assert!(cycles > 0);
}

// System instructions (rows 1, 3, 9-16).

/// The lot-3 shortfall `World::book_shortfall` books.
const U: u128 = 10_000;
/// `EngineParamsArgs::defaults_for_tests().system_ttl_ms`.
const TTL: u64 = 14_400_000;

fn encode(msg: &EngineMsg) -> Vec<u8> {
    borsh::to_vec(msg).expect("encodes")
}

fn token_params() -> EngineParamsArgs {
    EngineParamsArgs {
        protocol_token_enabled: true,
        ..World::engine_params()
    }
}

/// `World::with_system(params)` at `T`.
fn system_world(params: EngineParamsArgs) -> World {
    let mut w = World::with_system(params);
    w.set_now_ms(T);
    w
}

/// Simulates `plan` (it must pass and fit the gas cap); then the plan without each declared actor
/// in turn must fail as `UndeclaredActor` naming that actor's account, so every declared actor is
/// one the run reads or writes; then applies it signed by `keys` and checks every actor it
/// changed is declared. Together: the declared actors are exactly the touched ones.
fn run_exact(w: &mut World, label: &str, plan: &Plan, keys: &[&PrivateKey]) {
    run_may(w, label, plan, keys, &[]);
}

/// [`run_exact`] for a path whose plan also declares the may-touch actors `may` (ruling R-8b:
/// actors the run reaches only if another party moves state first). Each must be declared; they
/// are exempt from the leave-one-out, every other declared actor stays exact.
fn run_may(w: &mut World, label: &str, plan: &Plan, keys: &[&PrivateKey], may: &[Actor]) {
    for actor in may {
        assert!(
            plan.actors.contains(actor),
            "{label}: may-touch {actor:?} undeclared"
        );
    }
    let cycles = simulate(w, plan).unwrap_or_else(|err| panic!("{label} simulates: {err}"));
    fit_gas(cycles).unwrap_or_else(|err| panic!("{label} fits the gas cap: {err}"));
    for left_out in plan.actors.iter().filter(|actor| !may.contains(actor)) {
        let without = Plan {
            actors: plan
                .actors
                .iter()
                .copied()
                .filter(|actor| actor != left_out)
                .collect(),
            ..plan.clone()
        };
        match simulate(w, &without) {
            // LEZ refuses a message whose root is not declared before running anything.
            Err(SdkError::WouldRevert(reason)) if *left_out == plan.root => assert!(
                reason.contains("Root actor is not declared"),
                "{label} without its root: {reason}"
            ),
            Err(SdkError::WouldRevert(reason)) => assert!(
                reason.contains("UndeclaredActor")
                    && reason.contains(&left_out.account_id.to_string()),
                "{label} without {left_out:?}: {reason}"
            ),
            other => panic!("{label} without {left_out:?} still simulates: {other:?}"),
        }
    }
    let msg: EngineMsg = borsh::from_slice(&plan.message).expect("an engine message");
    w.chain
        .send_with_evidence(plan.root, &plan.actors, keys, &plan.evidence, &msg)
        .unwrap_or_else(|err| panic!("{label} applies: {err}"));
    let declared = set(&plan.actors);
    for actor in w.chain.changed_actors() {
        assert!(
            declared.contains(&actor),
            "{label}: changed {actor:?} is not declared"
        );
    }
}

/// The admin's `SetEngineParams` plan for `params` and `ref_oracle`, checked against World's.
fn set_params(w: &World, params: EngineParamsArgs, ref_oracle: Option<Actor>) -> Plan {
    let args = SetEngineParamsArgs {
        salt: SALT,
        params: params.clone(),
        ref_oracle,
    };
    let plan = plan_set_engine_params(&InProcess(&w.chain), w.engine, w.admin, args)
        .expect("set-engine-params plan");
    assert_eq!(
        plan.message,
        encode(&w.set_engine_params_msg(params, ref_oracle))
    );
    assert_eq!(plan.root, Actor::new(w.admin, w.engine));
    assert_eq!(plan.signers, vec![w.admin]);
    assert!(plan.evidence.is_empty());
    plan
}

/// The implicit settle's burn actors: the buffer's token actor and the stablecoin definition.
fn burn_pair(d: &Deployment) -> [Actor; 2] {
    [d.buffer(), d.holding(d.stable_def)]
}

/// Important 1 (ruling R-8b): a `CheckSolvency` planned while the buffer holding is absent stays
/// valid after anyone credits the buffer 1 unit. The plan declared the burn this now makes (and
/// the host), so the check freezes instead of reverting at its full gas limit.
#[test]
fn check_solvency_plan_survives_a_donation_to_the_buffer() {
    let mut w = World::new();
    w.set_now_ms(T);
    assert_eq!(w.book_shortfall(), U);
    let d = deployment(&w);
    let plan = plan_check_solvency(&InProcess(&w.chain), &d, T).expect("check plan");
    assert_eq!(
        plan.evidence.len(),
        1,
        "planned on an absent buffer holding"
    );
    let buffer = w.buffer().account_id;
    w.donate_stable(buffer, 1);
    let msg: EngineMsg = borsh::from_slice(&plan.message).expect("an engine message");
    w.chain
        .send_with_evidence(plan.root, &plan.actors, &[], &plan.evidence, &msg)
        .expect("the stale plan still applies");
    assert!(w.host_frozen());
    let acc = w.accounting_state();
    assert_eq!((acc.unbacked_debt, acc.total_debt_settled), (U - 1, 1));
    assert_eq!(w.stable_balance(buffer), 0, "the donation was burned");
}

/// No protocol token: `ObserveFeed`, `SetEngineParams` without a locked change, `SettleDebt`
/// with and without a burn, `CheckSolvency` `Healthy` (with and without a burn) and `Freeze`.
fn no_token_paths() {
    let mut w = World::new();
    w.set_now_ms(T);
    assert_eq!(w.book_shortfall(), U);
    let admin_key = w.admin_key.clone();
    let d = deployment(&w);
    assert_eq!((d.protocol_def, d.ref_oracle), (w.protocol_def(), None));

    let observe = plan_observe_feed(&d, w.collateral_def, T).expect("observe plan");
    assert_eq!(observe.root, w.collateral);
    assert!(observe.signers.is_empty() && observe.evidence.is_empty());
    run_exact(&mut w, "observe_feed", &observe, &[]);

    // Review Focus 1: the buffer holding is absent and reads as 0, so nothing burns.
    let settle = plan_settle_debt(&InProcess(&w.chain), &d, u128::MAX).expect("settle plan");
    assert_eq!(settle.message, encode(&w.settle_debt_msg(u128::MAX)));
    assert_eq!((settle.root, settle.evidence.len()), (w.config, 1));
    assert!(settle.signers.is_empty());
    let burn = burn_pair(&d);
    run_may(&mut w, "settle_debt (absent buffer)", &settle, &[], &burn);

    // No locked field changes: the accounting is not asked.
    let threshold_u = EngineParamsArgs {
        bad_debt_freeze_threshold: U,
        ..World::engine_params()
    };
    let set_u = set_params(&w, threshold_u, None);
    assert!(!set_u.actors.contains(&w.accounting));
    run_exact(
        &mut w,
        "set_engine_params (threshold)",
        &set_u,
        &[&admin_key],
    );

    let healthy = plan_check_solvency(&InProcess(&w.chain), &d, T).expect("check plan");
    assert_eq!(healthy.message, encode(&w.check_solvency_msg()));
    // No protocol token: the config can always freeze, so the host is declared (may-touch).
    let burn_or_freeze = [burn[0], burn[1], w.params];
    run_may(
        &mut w,
        "check_solvency (Healthy, absent buffer)",
        &healthy,
        &[],
        &burn_or_freeze,
    );

    let buffer = w.buffer().account_id;
    w.donate_stable(buffer, 4_000);
    let settle = plan_settle_debt(&InProcess(&w.chain), &d, 100).expect("settle plan");
    assert!(settle.evidence.is_empty());
    run_exact(&mut w, "settle_debt (burns 100)", &settle, &[]);
    let healthy = plan_check_solvency(&InProcess(&w.chain), &d, T).expect("check plan");
    let host = w.params;
    run_may(
        &mut w,
        "check_solvency (Healthy, burns)",
        &healthy,
        &[],
        &[host],
    );
    assert_eq!(w.accounting_state().unbacked_debt, U - 4_000);

    let set_low = set_params(&w, World::engine_params(), None);
    run_exact(
        &mut w,
        "set_engine_params (threshold back)",
        &set_low,
        &[&admin_key],
    );
    w.donate_stable(buffer, 100);
    let freeze = plan_check_solvency(&InProcess(&w.chain), &d, T).expect("check plan");
    assert!(freeze.actors.contains(&w.params), "Freeze asks the host");
    run_exact(&mut w, "check_solvency (Freeze, burns 100)", &freeze, &[]);
    assert!(w.host_frozen());
    assert_eq!(w.accounting_state().unbacked_debt, U - 4_100);
}

/// The surplus auction with a reference oracle (never published: the admin price): start, a
/// partial and a final take, a sweeping close, then `SetEngineParams` changing a locked field.
fn surplus_paths() {
    let mut w = system_world(EngineParamsArgs {
        ref_oracle_enabled: true,
        ..token_params()
    });
    let (taker_key, taker) = w.chain.key(TAKER);
    let admin_key = w.admin_key.clone();
    let d = deployment(&w);
    let ref_oracle = d.ref_oracle.expect("a reference oracle");
    let buffer = w.buffer().account_id;
    w.donate_stable(buffer, 1_500);

    let start = plan_start_surplus(&InProcess(&w.chain), &d, 0, T).expect("start plan");
    assert!(start.evidence.is_empty() && start.actors.contains(&ref_oracle));
    assert!(
        !start.actors.contains(&d.holding(d.stable_def)),
        "no debt to settle, nothing burns"
    );
    run_exact(&mut w, "start_surplus", &start, &[]);
    for (label, amount_coin) in [
        ("take_surplus (partial)", 250),
        ("take_surplus (final)", 250),
    ] {
        let args = TakeArgs {
            taker,
            nonce: 0,
            amount_coin,
            limit: TAKER_GENESIS,
            now_ms: T,
        };
        let take = plan_take_surplus(&d, args).expect("take plan");
        assert_eq!(
            (take.signers.clone(), take.evidence.len()),
            (vec![taker], 0)
        );
        run_exact(&mut w, label, &take, &[&taker_key]);
    }
    assert_eq!(w.surplus_state(0).phase, SystemPhase::Ended);

    w.donate_stable(buffer, 500);
    w.start_surplus(1)
        .expect("the second surplus auction starts");
    w.set_now_ms(T + TTL);
    let close = plan_close_surplus(&InProcess(&w.chain), &d, 1, T + TTL).expect("close plan");
    assert!(close.evidence.is_empty());
    run_exact(&mut w, "close_surplus (sweeps the lot)", &close, &[]);

    let lot = EngineParamsArgs {
        ref_oracle_enabled: true,
        surplus_lot: 400,
        ..token_params()
    };
    let set_lot = set_params(&w, lot, Some(ref_oracle));
    assert!(set_lot.actors.contains(&w.accounting), "a locked field");
    run_exact(
        &mut w,
        "set_engine_params (system)",
        &set_lot,
        &[&admin_key],
    );
}

/// The debt auction on the lot-3 shortfall: `CheckSolvency` `NoAction`, start, a partial and a
/// final take, then a second auction closed after the TTL.
fn debt_paths() {
    let mut w = system_world(EngineParamsArgs {
        ref_oracle_enabled: true,
        ..token_params()
    });
    assert_eq!(w.book_shortfall(), U);
    let (taker_key, taker) = w.chain.key(TAKER);
    w.fund_bidder(TAKER, 1_000);
    let d = deployment(&w);
    let ref_oracle = d.ref_oracle.expect("a reference oracle");
    let burn = burn_pair(&d);

    let check = plan_check_solvency(&InProcess(&w.chain), &d, T).expect("check plan");
    assert_eq!(check.evidence.len(), 1, "the buffer holding is absent");
    assert!(check.actors.contains(&ref_oracle));
    // `freeze_on_bad_debt`: the config can freeze, so the host is declared (may-touch).
    let burn_or_freeze = [burn[0], burn[1], w.params];
    run_may(
        &mut w,
        "check_solvency (NoAction)",
        &check,
        &[],
        &burn_or_freeze,
    );
    // The evidenced read left the buffer holding's account in state (with no token state), so
    // settlement now admits it as an existing account: no evidence, and it still reads as 0.
    let holding = w.holding(d.buffer().account_id, d.stable_def);
    assert!(w.chain.account_exists(holding.account_id) && w.chain.raw_state(holding).is_none());
    let start = plan_start_debt(&InProcess(&w.chain), &d, 0, T).expect("start plan");
    assert!(start.evidence.is_empty() && start.actors.contains(&ref_oracle));
    run_may(&mut w, "start_debt (empty buffer)", &start, &[], &burn);
    for (label, amount_coin, limit) in [
        ("take_debt (partial)", 300, 150),
        ("take_debt (final)", 200, 0),
    ] {
        let args = TakeArgs {
            taker,
            nonce: 0,
            amount_coin,
            limit,
            now_ms: T,
        };
        let take = plan_take_debt(&d, args).expect("take plan");
        assert_eq!(
            (take.signers.clone(), take.evidence.len()),
            (vec![taker], 0)
        );
        run_exact(&mut w, label, &take, &[&taker_key]);
    }

    let relist = T + DEBT_RELIST_DELAY_MS;
    w.set_now_ms(relist);
    w.start_debt(1).expect("the second debt auction starts");
    w.set_now_ms(relist + TTL);
    let close = plan_close_debt(&d, 1, relist + TTL).expect("close plan");
    assert!(close.signers.is_empty() && close.evidence.is_empty());
    run_exact(&mut w, "close_debt", &close, &[]);
}

/// Each system plan declares exactly the actors its in-process run touches: whatever the run
/// changed is declared, and leaving out any declared actor fails the simulation (closing Plan 5's
/// tautology minor). Paths whose actors depend on state are each run.
#[test]
fn system_plans_declare_every_touched_actor() {
    no_token_paths();
    surplus_paths();
    debt_paths();
}

/// The four plans whose accounting reads the buffer, built on `w`'s state.
fn buffer_readers(w: &World, d: &Deployment) -> Vec<(&'static str, Plan)> {
    let src = InProcess(&w.chain);
    vec![
        ("settle_debt", plan_settle_debt(&src, d, u128::MAX)),
        ("check_solvency", plan_check_solvency(&src, d, T)),
        ("start_debt", plan_start_debt(&src, d, 0, T)),
        ("start_surplus", plan_start_surplus(&src, d, 0, T)),
    ]
    .into_iter()
    .map(|(label, plan)| (label, plan.expect("plan")))
    .collect()
}

/// Review Focus 1: the plans whose accounting reads the buffer carry the `Pda` evidence for its
/// stablecoin holding while that holding is absent (without it the simulation cannot admit it),
/// and none once it exists; takes and closes never carry evidence. (`start_surplus` reverts on
/// the shortfall either way: its simulation must report the engine's error.)
#[test]
fn system_plans_attach_buffer_evidence_only_when_absent() {
    let mut w = system_world(token_params());
    assert_eq!(w.book_shortfall(), U);
    let d = deployment(&w);
    let buffer = d.buffer().account_id;
    let pda = PublicAccountEvidence::Pda {
        program: w.token,
        seed: holding_seed(buffer, d.stable_def),
    };

    assert!(
        !w.chain
            .account_exists(w.holding(buffer, d.stable_def).account_id)
    );
    assert_eq!(buffer_evidence(&InProcess(&w.chain), &d), vec![pda.clone()]);
    for (label, plan) in buffer_readers(&w, &d) {
        assert_eq!(plan.evidence, vec![pda.clone()], "{label}");
        let bare = Plan {
            evidence: Vec::new(),
            ..plan.clone()
        };
        match simulate(&w, &bare) {
            Err(SdkError::WouldRevert(reason)) => assert!(
                reason.contains("UnadmittedPublicActor"),
                "{label} without evidence: {reason}"
            ),
            other => panic!("{label} without evidence: {other:?}"),
        }
        match (label, simulate(&w, &plan)) {
            // A refused op declares only what runs before the refusal: the simulation reports
            // the engine's own error, not an undeclared actor.
            ("start_surplus", Err(SdkError::WouldRevert(reason))) => assert!(
                reason.contains("Program error") && !reason.contains("UndeclaredActor"),
                "the shortfall refuses the start: {reason}"
            ),
            ("start_surplus", other) => panic!("start_surplus on a shortfall: {other:?}"),
            (_, result) => {
                result.unwrap_or_else(|err| panic!("{label} reads 0: {err}"));
            }
        }
    }

    w.donate_stable(buffer, 100);
    assert!(buffer_evidence(&InProcess(&w.chain), &d).is_empty());
    for (label, plan) in buffer_readers(&w, &d) {
        assert!(plan.evidence.is_empty(), "{label}: the holding exists");
        if label != "start_surplus" {
            simulate(&w, &plan).unwrap_or_else(|err| panic!("{label}: {err}"));
        }
    }
    let take = TakeArgs {
        taker: w.chain.key(TAKER).1,
        nonce: 0,
        amount_coin: 100,
        limit: 0,
        now_ms: T,
    };
    let src = InProcess(&w.chain);
    for plan in [
        plan_take_surplus(&d, take),
        plan_take_debt(&d, take),
        plan_close_surplus(&src, &d, 0, T),
        plan_close_debt(&d, 0, T),
    ] {
        assert!(plan.expect("plan").evidence.is_empty());
    }
}
