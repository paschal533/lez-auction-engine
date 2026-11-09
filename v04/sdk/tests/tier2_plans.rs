//! The Tier 2 plans against the in-process harness: `SetPauseLevel` (ADR-073) declares exactly
//! World's actors and every actor its run changed, as `plans_declare_every_touched_actor` in
//! `builder.rs` does for the slice.

use std::collections::BTreeSet;

use engine_core::msg::EngineMsg;
use harness::{Chain, World};
use lee::{AccountId, Actor, PrivateKey};
use sdk::{Deployment, Plan, StateSource, fit_gas, plan_set_pause_level, simulate_with};

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
