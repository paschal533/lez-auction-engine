//! The `SetSlack` plan (ADR-064): the admin's proxy, the config and, when a collateral type is recorded,
//! the collateral actor; nothing else.

use engine_core::msg::EngineMsg;
use harness::World;
use lee::{AccountId, Actor};
use sdk::{Deployment, StateSource, plan_set_slack};

/// The harness chain as the SDK's state source.
struct InProcess<'c>(&'c harness::Chain);

impl StateSource for InProcess<'_> {
    fn actor_state(&self, actor: Actor) -> Option<Vec<u8>> {
        self.0.raw_state(actor)
    }

    fn account_exists(&self, account: AccountId) -> bool {
        self.0.account_exists(account)
    }
}

#[test]
fn plan_set_slack_declares_proxy_config_and_collateral() {
    let w = World::new();
    let d = Deployment::load(&InProcess(&w.chain), w.engine, w.config.account_id)
        .expect("config loads");
    let plan = plan_set_slack(&InProcess(&w.chain), &d, w.admin, 30_000).expect("set-slack plan");
    let proxy = Actor::new(w.admin, w.engine);
    assert_eq!(plan.root, proxy);
    assert_eq!(plan.actors, vec![proxy, w.config, w.collateral]);
    assert_eq!(plan.signers, vec![w.admin]);
    assert!(plan.evidence.is_empty());
    assert_eq!(
        plan.message,
        borsh::to_vec(&w.set_slack_msg(30_000)).expect("encodes")
    );
    let decoded: EngineMsg = borsh::from_slice(&plan.message).expect("an engine message");
    assert!(matches!(
        decoded,
        EngineMsg::SetSlack {
            slack_ms: 30_000,
            ..
        }
    ));
}

#[test]
fn plan_set_slack_without_collateral_declares_proxy_and_config() {
    let mut w = World::bare();
    let (key, init) = (w.admin_key.clone(), w.init_msg());
    w.send_engine(w.admin, &[&key], &init).expect("init");
    let d = Deployment::load(&InProcess(&w.chain), w.engine, w.config.account_id)
        .expect("config loads");
    let plan = plan_set_slack(&InProcess(&w.chain), &d, w.admin, 30_000).expect("plan");
    assert_eq!(plan.actors, vec![Actor::new(w.admin, w.engine), w.config]);
}
