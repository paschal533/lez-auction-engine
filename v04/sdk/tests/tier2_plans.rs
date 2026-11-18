//! The Tier 2 plans against the in-process harness: `SetPauseLevel` (ADR-073) and
//! `SetCollateral` with risk parameters (ADR-082) declare exactly World's actors and every actor
//! their runs changed, as `plans_declare_every_touched_actor` in `builder.rs` does for the slice;
//! `Init` and `SetEngineParams` with non-legacy engine parameters (R10-1) do the same; and the
//! recommended risk preset takes the oracle's answer bounds (R10-8).

use std::collections::BTreeSet;

use engine_core::msg::EngineMsg;
use harness::{
    Chain, World,
    tier2::tier2_collateral_args,
    world::{RAY, SALT, SLACK_MS},
};
use lee::{AccountId, Actor, PrivateKey};
use risk_core::EngineExtArgs;
use sdk::{
    Deployment, InitArgs, OracleBounds, Plan, RiskArgs, SetEngineParamsArgs, StateSource, fit_gas,
    plan_init, plan_set_collateral_with, plan_set_engine_params, plan_set_pause_level,
    recommended_risk, simulate_with,
};

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

/// Simulates `plan` (it must pass and fit the gas cap), applies it signed by `keys`, and checks
/// every actor the run changed is declared.
fn run(w: &mut World, label: &str, plan: &Plan, keys: &[&PrivateKey]) {
    let cycles = simulate_with(&InProcess(&w.chain), w.chain.catalog(), plan, w.admin)
        .unwrap_or_else(|err| panic!("{label} simulates: {err}"));
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
fn set_pause_level_plan_declares_every_touched_actor() {
    let mut w = World::new();
    let d = Deployment::load(&InProcess(&w.chain), w.engine, w.config.account_id)
        .expect("config loads");
    let key = w.admin_key.clone();
    for level in [3, 0] {
        let msg = w.set_pause_level_msg(level);
        let plan =
            plan_set_pause_level(&d, w.collateral_def, level, w.now_ms).expect("the plan builds");
        assert_eq!(plan.message, borsh::to_vec(&msg).expect("encodes"));
        assert_eq!(plan.root, Actor::new(w.admin, w.engine));
        assert_eq!(plan.signers, vec![w.admin]);
        assert!(plan.evidence.is_empty());
        assert_eq!(set(&plan.actors), set(&w.proxy_actors(w.admin, &msg)));
        run(&mut w, "set_pause_level", &plan, &[&key]);
        assert_eq!(w.collateral_state().ext.pause_level, level);
        harness::invariants::check(&w);
    }
}

#[test]
fn set_collateral_with_plan_declares_every_touched_actor() {
    let mut w = World::new_tier2(RiskArgs::legacy());
    let d = Deployment::load(&InProcess(&w.chain), w.engine, w.config.account_id)
        .expect("config loads");
    let key = w.admin_key.clone();
    let wider = RiskArgs {
        max_spot_twap_dev_bps: 1_000,
        ..RiskArgs::recommended()
    };
    for risk in [RiskArgs::recommended(), wider, RiskArgs::legacy()] {
        let args = tier2_collateral_args();
        let msg = w.set_collateral_msg_with(args.clone(), risk);
        let plan = plan_set_collateral_with(&d, w.collateral_def, args, risk, w.now_ms)
            .expect("the plan builds");
        assert_eq!(plan.message, borsh::to_vec(&msg).expect("encodes"));
        assert_eq!(plan.root, Actor::new(w.admin, w.engine));
        assert_eq!(plan.signers, vec![w.admin]);
        assert!(plan.evidence.is_empty());
        assert_eq!(set(&plan.actors), set(&w.proxy_actors(w.admin, &msg)));
        run(&mut w, "set_collateral_with", &plan, &[&key]);
        assert_eq!(w.collateral_state().ext.risk, risk);
        harness::invariants::check(&w);
    }
}

#[test]
fn recommended_risk_takes_the_oracles_bounds() {
    assert_eq!(
        recommended_risk(OracleBounds::Undeclared),
        RiskArgs::recommended()
    );
    let declared = recommended_risk(OracleBounds::Declared {
        min_raw: 5,
        max_raw: 9,
    });
    assert_eq!(declared.feed_min_raw, 5);
    assert_eq!(declared.feed_max_raw, 9);
    assert_eq!(
        declared,
        RiskArgs {
            feed_min_raw: 5,
            feed_max_raw: 9,
            ..RiskArgs::recommended()
        }
    );
}

#[test]
fn init_and_set_engine_params_plans_declare_every_touched_actor() {
    let mut w = World::bare_at(RAY / 2);
    let key = w.admin_key.clone();
    let clamped = EngineExtArgs {
        ref_clamp_bps: 1_500,
        ..EngineExtArgs::legacy()
    };
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
            ext: clamped,
        },
    )
    .expect("the init plan builds");
    let msg = w.init_msg_ext(World::engine_params(), clamped, None, None);
    assert_eq!(init.message, borsh::to_vec(&msg).expect("encodes"));
    assert_eq!(init.root, Actor::new(w.admin, w.engine));
    assert_eq!(init.signers, vec![w.admin]);
    assert!(init.evidence.is_empty());
    assert_eq!(set(&init.actors), set(&w.proxy_actors(w.admin, &msg)));
    run(&mut w, "init", &init, &[&key]);
    assert_eq!(w.config_state().ext.engine, clamped);

    for ext in [EngineExtArgs::legacy(), clamped] {
        let args = SetEngineParamsArgs {
            salt: SALT,
            params: World::engine_params(),
            ref_oracle: None,
            ext,
        };
        let plan = plan_set_engine_params(&InProcess(&w.chain), w.engine, w.admin, args)
            .expect("the set-engine-params plan builds");
        let msg = w.set_engine_params_msg_ext(World::engine_params(), ext, None);
        assert_eq!(plan.message, borsh::to_vec(&msg).expect("encodes"));
        assert_eq!(plan.root, Actor::new(w.admin, w.engine));
        assert_eq!(plan.signers, vec![w.admin]);
        assert!(plan.evidence.is_empty());
        // No collateral type is recorded and no locked field changes (R10-5: `ref_clamp_bps`
        // is not one), so neither the collateral actor nor the accounting is declared; World's
        // list always carries the accounting.
        assert_eq!(set(&plan.actors), set(&[init.root, w.config]));
        run(&mut w, "set_engine_params", &plan, &[&key]);
        assert_eq!(w.config_state().ext.engine, ext);
    }
}
