//! The token actor: holdings, transfer, sweep, burn, mint, credit and read.

use borsh::BorshSerialize;
use harness::{Chain, expect_revert};
use lee::{AccountId, Actor, PrivateKey, PublicAccountEvidence, PublicKey, error::LeeError};
use lee_core::program::{StateReply, TransactionEvent};
use token_core::{Definition, Holding, TokenMsg};

/// The relay guest's root message: forward `message` to `to` as a sync message.
#[derive(BorshSerialize)]
struct Forward {
    to: Actor,
    message: Vec<u8>,
}

fn forward(to: Actor, msg: &TokenMsg) -> Forward {
    Forward {
        to,
        message: borsh::to_vec(msg).expect("encodes"),
    }
}

/// Proof that a key's account is public, which a fresh unsigned account needs to be admitted.
fn evidence(key: &PrivateKey) -> PublicAccountEvidence {
    PublicAccountEvidence::Key(PublicKey::new_from_private_key(key))
}

fn new_chain() -> Chain {
    Chain::new(&[
        ("token", methods::TOKEN_ELF),
        ("relay", methods::RELAY_ELF),
        ("twin", methods::RELAY_TWIN_ELF),
    ])
}

struct World {
    chain: Chain,
    token: AccountId,
    definition: Actor,
    a_key: PrivateKey,
    a: Actor,
    b_key: PrivateKey,
    b: Actor,
}

impl World {
    /// A chain with the token, relay and twin programs, and a definition issuing `supply` to A.
    fn new(supply: u128, minter: Option<Actor>) -> Self {
        let mut chain = new_chain();
        let token = chain.program("token");
        let (a_key, a_account) = chain.key("a");
        let (b_key, b_account) = chain.key("b");
        let (d_key, d_account) = chain.key("definition");
        chain.fund(a_account, 1);
        let mut world = Self {
            token,
            definition: Actor::new(d_account, token),
            a_key,
            a: Actor::new(a_account, token),
            b_key,
            b: Actor::new(b_account, token),
            chain,
        };
        let msg = TokenMsg::NewDefinition {
            minter,
            initial_to: Some(a_account),
            initial_supply: supply,
        };
        let (definition, a) = (world.definition, world.a);
        world
            .chain
            .send_with_evidence(definition, &[definition, a], &[], &[evidence(&d_key)], &msg)
            .expect("the definition is created");
        world
    }

    fn balance(&self, holding: Actor) -> Option<u128> {
        self.chain.state::<Holding>(holding).map(|h| h.balance)
    }

    fn supply(&self) -> u128 {
        self.chain
            .state::<Definition>(self.definition)
            .expect("definition exists")
            .supply
    }

    /// A signed root message at A's holding. B's account is proven public, as a fresh holding
    /// is admitted only with that evidence (a declared, unsigned, missing account is refused).
    fn send_a(
        &mut self,
        actors: &[Actor],
        msg: &TokenMsg,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let a = self.a;
        self.chain
            .send_with_evidence(a, actors, &[&self.a_key], &[evidence(&self.b_key)], msg)
    }
}

#[test]
fn new_definition_then_transfer_credits_a_fresh_holding() {
    let mut w = World::new(1_000, None);
    assert_eq!(w.balance(w.a), Some(1_000));
    assert_eq!(w.balance(w.b), None);

    let msg = TokenMsg::Transfer {
        to: w.b.account_id,
        amount: 250,
    };
    w.send_a(&[w.a, w.b], &msg).expect("transfer applies");

    assert_eq!(w.balance(w.b), Some(250));
    assert_eq!(w.balance(w.a), Some(750));
    assert_eq!(w.supply(), 1_000);
    let holding = w.chain.state::<Holding>(w.b).expect("holding");
    assert_eq!(holding.definition, w.definition.account_id);
}

#[test]
fn transfer_without_authorization_reverts() {
    let mut w = World::new(1_000, None);
    let msg = TokenMsg::Transfer {
        to: w.b.account_id,
        amount: 1,
    };
    let (a, b) = (w.a, w.b);
    expect_revert(w.chain.send(a, &[a, b], &[], &msg), "token: Unauthorized");
    assert_eq!(w.balance(a), Some(1_000));
}

#[test]
fn transfer_beyond_the_balance_reverts() {
    let mut w = World::new(10, None);
    let msg = TokenMsg::Transfer {
        to: w.b.account_id,
        amount: 11,
    };
    expect_revert(w.send_a(&[w.a, w.b], &msg), "token: InsufficientBalance");
}

#[test]
fn sweep_moves_everything_and_enforces_at_least() {
    let mut w = World::new(1_000, None);
    let too_much = TokenMsg::Sweep {
        to: w.b.account_id,
        at_least: 1_001,
    };
    expect_revert(w.send_a(&[w.a, w.b], &too_much), "token: BelowAtLeast");
    assert_eq!(w.balance(w.a), Some(1_000));

    let exact = TokenMsg::Sweep {
        to: w.b.account_id,
        at_least: 1_000,
    };
    w.send_a(&[w.a, w.b], &exact).expect("sweep applies");
    assert_eq!(w.balance(w.a), Some(0));
    assert_eq!(w.balance(w.b), Some(1_000));
}

#[test]
fn burn_lowers_supply() {
    let mut w = World::new(1_000, None);
    w.send_a(&[w.a, w.definition], &TokenMsg::Burn { amount: 400 })
        .expect("burn applies");
    assert_eq!(w.balance(w.a), Some(600));
    assert_eq!(w.supply(), 600);
}

#[test]
fn mint_to_only_from_the_minter_actor() {
    let mut chain = new_chain();
    let (token, relay, twin) = (
        chain.program("token"),
        chain.program("relay"),
        chain.program("twin"),
    );
    let (_, k) = chain.key("k");
    let (_, other) = chain.key("other");
    let (a_key, a_account) = chain.key("a");
    let (d_key, d_account) = chain.key("definition");
    for account in [k, other] {
        chain.fund(account, 1);
    }
    let minter = Actor::new(k, relay);
    let definition = Actor::new(d_account, token);
    let a = Actor::new(a_account, token);
    let new_definition = TokenMsg::NewDefinition {
        minter: Some(minter),
        initial_to: None,
        initial_supply: 0,
    };
    chain
        .send_with_evidence(
            definition,
            &[definition],
            &[],
            &[evidence(&d_key)],
            &new_definition,
        )
        .expect("definition without supply");
    let mint = forward(
        definition,
        &TokenMsg::MintTo {
            to: a_account,
            amount: 5,
        },
    );

    // (K, other program) and (other, E_fake) are refused.
    for impostor in [Actor::new(k, twin), Actor::new(other, relay)] {
        let result = chain.send_with_evidence(
            impostor,
            &[impostor, definition, a],
            &[],
            &[evidence(&a_key)],
            &mint,
        );
        expect_revert(result, "token: NotMinter");
    }
    // A root message has no `from`.
    let direct = TokenMsg::MintTo {
        to: a_account,
        amount: 5,
    };
    expect_revert(
        chain.send_with_evidence(
            definition,
            &[definition, a],
            &[],
            &[evidence(&a_key)],
            &direct,
        ),
        "token: NotMinter",
    );
    assert_eq!(
        chain.state::<Definition>(definition).map(|d| d.supply),
        Some(0)
    );

    chain
        .send_with_evidence(
            minter,
            &[minter, definition, a],
            &[],
            &[evidence(&a_key)],
            &mint,
        )
        .expect("the minter mints");
    assert_eq!(
        chain.state::<Definition>(definition).map(|d| d.supply),
        Some(5)
    );
    assert_eq!(chain.state::<Holding>(a).map(|h| h.balance), Some(5));
}

#[test]
fn credit_with_another_definition_reverts() {
    let mut w = World::new(1_000, None);
    // A second token: B's holding of it comes from a second definition's initial supply.
    let (d2_key, d2_account) = w.chain.key("definition2");
    let b_evidence = evidence(&w.b_key);
    let d2 = Actor::new(d2_account, w.token);
    let new_definition = TokenMsg::NewDefinition {
        minter: None,
        initial_to: Some(w.b.account_id),
        initial_supply: 7,
    };
    w.chain
        .send_with_evidence(
            d2,
            &[d2, w.b],
            &[],
            &[evidence(&d2_key), b_evidence],
            &new_definition,
        )
        .expect("second definition");
    assert_eq!(w.balance(w.b), Some(7));

    // A sends token-1 units to B, who holds token 2.
    let msg = TokenMsg::Transfer {
        to: w.b.account_id,
        amount: 1,
    };
    expect_revert(w.send_a(&[w.a, w.b], &msg), "token: WrongDefinition");
    assert_eq!(w.balance(w.a), Some(1_000));
    assert_eq!(w.balance(w.b), Some(7));
}

#[test]
fn credit_and_supply_down_from_outside_the_token_program_revert() {
    let mut w = World::new(1_000, None);
    let (a, definition) = (w.a, w.definition);
    let credit = TokenMsg::Credit {
        definition: definition.account_id,
        amount: 1,
    };
    expect_revert(w.chain.send(a, &[a], &[], &credit), "token: ForeignCredit");
    let down = TokenMsg::SupplyDown { amount: 1 };
    expect_revert(
        w.chain.send(definition, &[definition], &[], &down),
        "token: ForeignSupplyDown",
    );
}

#[test]
fn read_state_replies_to_from() {
    let mut w = World::new(1_000, None);
    let (_, reader_account) = w.chain.key("reader");
    w.chain.fund(reader_account, 1);
    let reader = Actor::new(reader_account, w.chain.program("relay"));
    let a = w.a;

    let read = forward(a, &TokenMsg::ReadState);
    w.chain
        .send(reader, &[reader, a], &[], &read)
        .expect("read applies");

    let raw = w
        .chain
        .raw_state(reader)
        .expect("the relay stored the reply");
    let reply: StateReply = borsh::from_slice(&raw).expect("a StateReply");
    assert_eq!(
        Holding::decode(reply.state.as_ref()),
        Some(Holding::new(w.definition.account_id, 1_000))
    );

    // A root read has nobody to reply to.
    expect_revert(
        w.chain.send(a, &[a], &[], &TokenMsg::ReadState),
        "token: Unroutable",
    );
}
