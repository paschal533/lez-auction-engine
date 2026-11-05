//! `live_auctions` (ADR-066): the candidates a scan found, narrowed to the auctions whose engine
//! state exists, against a state stub.

use std::collections::BTreeMap;

use lee::{AccountId, Actor};
use sdk::{
    Deployment, StateSource,
    scan::{Found, live_auctions},
};

const ENGINE: AccountId = AccountId::new([0xE0; 32]);
const CONFIG: AccountId = AccountId::new([0xC0; 32]);
const POSITION_A: AccountId = AccountId::new([0xA1; 32]);
const POSITION_B: AccountId = AccountId::new([0xA2; 32]);

/// Actor states by actor; an account exists when any of its actors holds state.
struct Stub(BTreeMap<Actor, Vec<u8>>);

impl StateSource for Stub {
    fn actor_state(&self, actor: Actor) -> Option<Vec<u8>> {
        self.0.get(&actor).cloned()
    }

    fn account_exists(&self, account: AccountId) -> bool {
        self.0.keys().any(|actor| actor.account_id == account)
    }
}

fn deployment() -> Deployment {
    let actor = |byte: u8| Actor::new(AccountId::new([byte; 32]), ENGINE);
    Deployment {
        engine: ENGINE,
        config: Actor::new(CONFIG, ENGINE),
        admin: AccountId::new([0xAD; 32]),
        salt: 1,
        host: actor(0x01),
        token: AccountId::new([0x70; 32]),
        stable_def: AccountId::new([0x5D; 32]),
        oracle: actor(0x02),
        clock: actor(0x03),
        accounting: actor(0x04),
        protocol_def: AccountId::new([0x9D; 32]),
        ref_oracle: None,
        ref_oracle_evidence: None,
    }
}

fn found(auctions: &[(AccountId, u64)]) -> Found {
    Found {
        auctions: auctions.iter().copied().collect(),
        ..Found::default()
    }
}

#[test]
fn live_auctions_drops_candidates_without_state() {
    let d = deployment();
    let kept = d.auction(POSITION_A, 0);
    let stub = Stub(BTreeMap::from([(kept, vec![1, 2, 3])]));
    let candidates = found(&[(POSITION_A, 0), (POSITION_B, 7)]);

    let live = live_auctions(&stub, &d, &candidates).expect("reads");

    assert_eq!(live, vec![kept]);
}

#[test]
fn live_auctions_empty_when_nothing_found() {
    let d = deployment();
    let stub = Stub(BTreeMap::from([(d.auction(POSITION_A, 0), vec![1])]));

    let live = live_auctions(&stub, &d, &Found::default()).expect("reads");

    assert!(live.is_empty());
}
