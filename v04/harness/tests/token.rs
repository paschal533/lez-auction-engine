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
            .send(definition, &[definition, a], &[&d_key], &msg)
            .expect("the definition is created");
        world
    }

    fn balance_of(&self, holding: Actor, definition: Actor) -> Option<u128> {
        self.chain
            .state::<Holding>(holding)
            .map(|h| h.balance(definition.account_id))
    }

    fn balance(&self, holding: Actor) -> Option<u128> {
        self.balance_of(holding, self.definition)
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

    fn transfer_a_to_b(&self, amount: u128) -> TokenMsg {
        TokenMsg::Transfer {
            definition: self.definition.account_id,
            to: self.b.account_id,
            amount,
        }
    }
}

#[test]
fn new_definition_then_transfer_credits_a_fresh_holding() {
    let mut w = World::new(1_000, None);
    assert_eq!(w.balance(w.a), Some(1_000));
    assert_eq!(w.balance(w.b), None);

    let msg = w.transfer_a_to_b(250);
    w.send_a(&[w.a, w.b], &msg).expect("transfer applies");

    assert_eq!(w.balance(w.b), Some(250));
    assert_eq!(w.balance(w.a), Some(750));
    assert_eq!(w.supply(), 1_000);
}

#[test]
fn transfer_without_authorization_reverts() {
    let mut w = World::new(1_000, None);
    let msg = w.transfer_a_to_b(1);
    let (a, b) = (w.a, w.b);
    expect_revert(w.chain.send(a, &[a, b], &[], &msg), "token: Unauthorized");
    assert_eq!(w.balance(a), Some(1_000));
}

#[test]
fn transfer_beyond_the_balance_reverts() {
    let mut w = World::new(10, None);
    let msg = w.transfer_a_to_b(11);
    expect_revert(w.send_a(&[w.a, w.b], &msg), "token: InsufficientBalance");
}

#[test]
fn sweep_moves_everything_and_enforces_at_least() {
    let mut w = World::new(1_000, None);
    let sweep = |at_least| TokenMsg::Sweep {
        definition: w.definition.account_id,
        to: w.b.account_id,
        at_least,
    };
    let (too_much, exact) = (sweep(1_001), sweep(1_000));
    expect_revert(w.send_a(&[w.a, w.b], &too_much), "token: BelowAtLeast");
    assert_eq!(w.balance(w.a), Some(1_000));

    w.send_a(&[w.a, w.b], &exact).expect("sweep applies");
    assert_eq!(w.balance(w.a), Some(0));
    assert_eq!(w.balance(w.b), Some(1_000));
    let holding = w.chain.state::<Holding>(w.a).expect("holding stays");
    assert!(holding.balances.is_empty(), "a zero entry is removed");
}

#[test]
fn burn_lowers_supply() {
    let mut w = World::new(1_000, None);
    let burn = TokenMsg::Burn {
        definition: w.definition.account_id,
        amount: 400,
    };
    w.send_a(&[w.a, w.definition], &burn).expect("burn applies");
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
        .send(definition, &[definition], &[&d_key], &new_definition)
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
    let held = chain.state::<Holding>(a).map(|h| h.balance(d_account));
    assert_eq!(held, Some(5));
}

/// Creates a second token whose whole supply goes to B; returns its definition actor.
fn junk_token_to_b(w: &mut World, supply: u128) -> Actor {
    let (j_key, j_account) = w.chain.key("junk");
    let junk = Actor::new(j_account, w.token);
    let msg = TokenMsg::NewDefinition {
        minter: None,
        initial_to: Some(w.b.account_id),
        initial_supply: supply,
    };
    w.chain
        .send_with_evidence(junk, &[junk, w.b], &[&j_key], &[evidence(&w.b_key)], &msg)
        .expect("junk definition");
    junk
}

#[test]
fn two_definitions_coexist_in_one_holding() {
    let mut w = World::new(1_000, None);
    let junk = junk_token_to_b(&mut w, 7);
    let msg = w.transfer_a_to_b(100);
    w.send_a(&[w.a, w.b], &msg)
        .expect("credit of a second token");
    assert_eq!(w.balance_of(w.b, junk), Some(7));
    assert_eq!(w.balance(w.b), Some(100));

    // B debits the first token; the second is intact.
    let back = TokenMsg::Transfer {
        definition: w.definition.account_id,
        to: w.a.account_id,
        amount: 100,
    };
    let b = w.b;
    w.chain
        .send(b, &[b, w.a], &[&w.b_key], &back)
        .expect("B transfers back");
    assert_eq!(w.balance_of(w.b, junk), Some(7));
    let holding = w.chain.state::<Holding>(w.b).expect("holding");
    assert_eq!(holding.balances.len(), 1, "the emptied entry is removed");
    assert_eq!(w.balance(w.a), Some(1_000));
}

#[test]
fn squatting_a_fresh_holding_with_a_junk_token_does_not_block_the_real_one() {
    let mut w = World::new(1_000, None);
    junk_token_to_b(&mut w, 1);
    assert_eq!(w.balance(w.b), Some(0), "B holds only the junk token");

    let msg = w.transfer_a_to_b(300);
    w.send_a(&[w.a, w.b], &msg).expect("the real credit lands");
    assert_eq!(w.balance(w.b), Some(300));

    let onward = TokenMsg::Transfer {
        definition: w.definition.account_id,
        to: w.a.account_id,
        amount: 120,
    };
    let b = w.b;
    w.chain
        .send(b, &[b, w.a], &[&w.b_key], &onward)
        .expect("B moves the real token on");
    assert_eq!(w.balance(w.b), Some(180));
    assert_eq!(w.balance(w.a), Some(820));
}

#[test]
fn new_definition_requires_authorization() {
    let mut chain = new_chain();
    let token = chain.program("token");
    let (d_key, d_account) = chain.key("definition");
    let definition = Actor::new(d_account, token);
    let msg = TokenMsg::NewDefinition {
        minter: None,
        initial_to: None,
        initial_supply: 0,
    };
    // Admitted (the key's account is proven public) but not authorised: nobody signed.
    let result =
        chain.send_with_evidence(definition, &[definition], &[], &[evidence(&d_key)], &msg);
    expect_revert(result, "token: Unauthorized");
    assert_eq!(chain.raw_state(definition), None);

    chain
        .send(definition, &[definition], &[&d_key], &msg)
        .expect("the definition account signs");
    assert!(chain.state::<Definition>(definition).is_some());
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

    // The same messages from another program's actors, the real inflation path.
    let (_, x_account) = w.chain.key("x");
    w.chain.fund(x_account, 1);
    let relay = w.chain.program("relay");
    for foreign in [
        Actor::new(x_account, relay),
        Actor::new(a.account_id, relay),
    ] {
        let credit = forward(a, &credit);
        let result = w.chain.send(foreign, &[foreign, a], &[], &credit);
        expect_revert(result, "token: ForeignCredit");
        let down = forward(definition, &down);
        let result = w.chain.send(foreign, &[foreign, definition], &[], &down);
        expect_revert(result, "token: ForeignSupplyDown");
    }
    assert_eq!(w.balance(a), Some(1_000));
    assert_eq!(w.supply(), 1_000);
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
    let holding = Holding::decode(reply.state.as_ref()).expect("a holding");
    assert_eq!(holding.balance(w.definition.account_id), 1_000);

    // A root read has nobody to reply to.
    expect_revert(
        w.chain.send(a, &[a], &[], &TokenMsg::ReadState),
        "token: Unroutable",
    );
}
