//! The oracle actor: init, publish (whole-actor publisher check) and read.

use borsh::BorshSerialize;
use harness::{Chain, expect_revert};
use lee::{Actor, PrivateKey, error::LeeError};
use lee_core::program::{StateReply, TransactionEvent};
use oracle_core::{OracleMsg, OracleState};

const RAY: u128 = 1_000_000_000_000_000_000_000_000_000;
const CONFIDENCE: u128 = 7;

/// The relay guest's root message: forward `message` to `to` as a sync message.
#[derive(BorshSerialize)]
struct Forward {
    to: Actor,
    message: Vec<u8>,
}

fn forward(to: Actor, msg: &OracleMsg) -> Forward {
    Forward {
        to,
        message: borsh::to_vec(msg).expect("encodes"),
    }
}

struct World {
    chain: Chain,
    oracle_key: PrivateKey,
    oracle: Actor,
    publisher: Actor,
}

impl World {
    /// A chain with an initialised oracle whose publisher is `(P, relay)`.
    fn new() -> Self {
        let mut chain = Chain::new(&[
            ("oracle", methods::ORACLE_ELF),
            ("relay", methods::RELAY_ELF),
        ]);
        let (oracle_key, o_account) = chain.key("oracle");
        let (_, p_account) = chain.key("publisher");
        chain.fund(p_account, 1);
        let oracle = Actor::new(o_account, chain.program("oracle"));
        let publisher = Actor::new(p_account, chain.program("relay"));
        chain
            .send(
                oracle,
                &[oracle],
                &[&oracle_key],
                &OracleMsg::Init { publisher },
            )
            .expect("the oracle is initialised");
        Self {
            chain,
            oracle_key,
            oracle,
            publisher,
        }
    }

    /// `from` (a relay actor) publishes `price_ray` at `timestamp_ms`.
    fn publish(
        &mut self,
        from: Actor,
        price_ray: u128,
        timestamp_ms: u64,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let oracle = self.oracle;
        let msg = forward(
            oracle,
            &OracleMsg::Publish {
                price_ray,
                timestamp_ms,
                confidence: CONFIDENCE,
            },
        );
        self.chain.send(from, &[from, oracle], &[], &msg)
    }

    fn values(&self) -> (u128, u64, u128) {
        let state = self
            .chain
            .state::<OracleState>(self.oracle)
            .expect("oracle exists");
        (state.price_ray, state.timestamp_ms, state.confidence)
    }
}

#[test]
fn publish_only_from_the_publisher_actor() {
    let mut w = World::new();
    assert_eq!(w.values(), (0, 0, 0));
    let (oracle, publisher) = (w.oracle, w.publisher);

    // A root message has no `from`.
    let direct = OracleMsg::Publish {
        price_ray: RAY,
        timestamp_ms: 1,
        confidence: CONFIDENCE,
    };
    expect_revert(
        w.chain.send(oracle, &[oracle], &[], &direct),
        "oracle: NotPublisher",
    );
    assert_eq!(w.values(), (0, 0, 0));

    w.publish(publisher, 2 * RAY, 1_000)
        .expect("the publisher publishes");
    assert_eq!(w.values(), (2 * RAY, 1_000, CONFIDENCE));

    // A second Init cannot replace the publisher.
    let init = OracleMsg::Init { publisher: oracle };
    expect_revert(
        w.chain.send(oracle, &[oracle], &[&w.oracle_key], &init),
        "oracle: AlreadyInitialized",
    );
    assert_eq!(w.values(), (2 * RAY, 1_000, CONFIDENCE));
}

#[test]
fn publish_by_another_actor_of_the_same_program_reverts() {
    let mut w = World::new();
    let (_, q_account) = w.chain.key("impostor");
    w.chain.fund(q_account, 1);
    // Same program as the publisher (relay), different account.
    let impostor = Actor::new(q_account, w.chain.program("relay"));
    expect_revert(w.publish(impostor, 5 * RAY, 9), "oracle: NotPublisher");
    assert_eq!(w.values(), (0, 0, 0));
}

#[test]
fn read_state_returns_the_last_publication() {
    let mut w = World::new();
    let (oracle, publisher) = (w.oracle, w.publisher);
    w.publish(publisher, 3 * RAY, 4_242)
        .expect("the publisher publishes");

    let (_, reader_account) = w.chain.key("reader");
    w.chain.fund(reader_account, 1);
    let reader = Actor::new(reader_account, w.chain.program("relay"));
    w.chain
        .send(
            reader,
            &[reader, oracle],
            &[],
            &forward(oracle, &OracleMsg::ReadState),
        )
        .expect("read applies");

    let raw = w
        .chain
        .raw_state(reader)
        .expect("the relay stored the reply");
    let reply: StateReply = borsh::from_slice(&raw).expect("a StateReply");
    let state = OracleState::decode(reply.state.as_ref()).expect("an oracle state");
    assert_eq!(state.publisher, publisher);
    assert_eq!(
        (state.price_ray, state.timestamp_ms, state.confidence),
        (3 * RAY, 4_242, CONFIDENCE)
    );

    // A root read has nobody to reply to.
    expect_revert(
        w.chain.send(oracle, &[oracle], &[], &OracleMsg::ReadState),
        "oracle: Unroutable",
    );
}
