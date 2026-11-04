//! ADR-062: one token holding per (owner, definition). Junk credited to an engine or host account
//! lands in other actors, so it never changes the cost of a liquidation or a bid; the holding lives
//! at the token PDA and the owner's own token actor stays a stateless proxy; only that proxy can
//! debit a holding; a credit reaches only the holding it names; and a credit can no longer
//! pre-block a definition address.

use harness::{Chain, World, bidding::Bidder, expect_revert, world::RAY};
use lee::{AccountId, Actor, PrivateKey, error::LeeError};
use lee_core::program::TransactionEvent;
use token_core::{DebitAction, Definition, Holding, TokenMsg, holding_actor};

const COLLATERAL: u128 = 500_000;
const DEBT: u128 = 600_000;
const FUNDS: u128 = 1_000_000;
/// A debt-first partial bid: no penalty part, so it never reaches the buffer.
const PARTIAL: u128 = 100_000;
/// The whole charge, debt plus penalty: the final bid credits the penalty to the buffer.
const TARGET: u128 = 660_000;
/// The chain time of the liquidation and the bids, ms.
const T: u64 = 1_000_000;
/// Junk definitions credited to each target account.
const JUNK: usize = 64;

/// 64 fresh signers each create a one-unit definition whose supply goes to `to`.
fn stuff_junk(w: &mut World, prefix: &str, to: AccountId) {
    for i in 0..JUNK {
        let (key, account) = w.chain.key(&format!("{prefix}-{i}"));
        let definition = Actor::new(account, w.token);
        let msg = TokenMsg::NewDefinition {
            minter: None,
            initial_to: Some(to),
            initial_supply: 1,
        };
        let holding = w.holding(to, account);
        w.chain
            .send(definition, &[definition, holding], &[&key], &msg)
            .expect("a junk definition credits the target");
    }
}

/// A world with one unsafe position, priced for liquidation at `T`.
fn unsafe_world() -> (World, AccountId) {
    let mut w = World::new_at(RAY / 2);
    w.set_now_ms(T);
    let position = w.open_unsafe_position(COLLATERAL, DEBT);
    w.set_price(RAY / 2);
    (w, position)
}

fn liquidate_cycles(w: &World, position: AccountId) -> (u64, Result<(), LeeError>) {
    let msg = w.liquidate_msg(position, 0, w.now_ms);
    let actors = w.proxy_actors(w.keeper, &msg);
    let root = Actor::new(w.keeper, w.engine);
    w.chain.cycles(root, &actors, &[&w.keeper_key], &msg)
}

#[test]
fn junk_definitions_in_a_host_vault_do_not_change_liquidation_cost() {
    let (baseline, position) = unsafe_world();
    let (base_cycles, base_outcome) = liquidate_cycles(&baseline, position);
    base_outcome.expect("the baseline liquidation simulates");

    let (mut w, position) = unsafe_world();
    let host_vault = w.host_vault(position).account_id;
    stuff_junk(&mut w, "junk", host_vault);
    let (cycles, outcome) = liquidate_cycles(&w, position);
    outcome.expect("the liquidation simulates with junk in the host vault");
    assert_eq!(cycles, base_cycles, "junk changed the liquidation's cost");
    w.liquidate(position, 0)
        .expect("the junk-stuffed vault liquidates");
}

fn liquidated() -> (World, AccountId, Bidder) {
    let (mut w, position) = unsafe_world();
    w.liquidate(position, 0).expect("the keeper liquidates");
    let bidder = w.fund_bidder("bidder", FUNDS);
    (w, position, bidder)
}

/// Simulates `bidder`'s bid of up to `max_debt_in` (it must simulate), then applies it; returns
/// its cycles.
fn measured_bid(w: &mut World, bidder: &Bidder, position: AccountId, max_debt_in: u128) -> u64 {
    let msg = w.bid_msg(bidder.account, position, 0, (max_debt_in, 0), w.now_ms);
    let actors = w.bid_actors(bidder.account, position, 0);
    let (cycles, outcome) = w.chain.cycles(w.collateral, &actors, &[&bidder.key], &msg);
    outcome.unwrap_or_else(|err| panic!("the bid of {max_debt_in} simulates: {err:?}"));
    w.send_bid(&msg, &[&bidder.key])
        .unwrap_or_else(|err| panic!("the bid of {max_debt_in} applies: {err:?}"));
    cycles
}

/// The partial bid (debt first: it debits the bidder and pays out of the auction vault) and the
/// final bid (it also credits the penalty to the buffer's stablecoin holding), in order; asserts
/// the final bid really reached the buffer.
fn bid_cycles(w: &mut World, bidder: &Bidder, position: AccountId) -> (u64, u64) {
    let partial = measured_bid(w, bidder, position, PARTIAL);
    let buffer_stable = w.holding(w.buffer().account_id, w.stable_def);
    assert_eq!(w.chain.raw_state(buffer_stable), None, "no penalty yet");
    let last = measured_bid(w, bidder, position, TARGET);
    assert!(
        w.chain
            .state::<Holding>(buffer_stable)
            .is_some_and(|h| h.balance > 0),
        "the final bid credited the penalty to the buffer's stablecoin holding"
    );
    (partial, last)
}

#[test]
fn junk_in_the_buffer_and_an_auction_vault_does_not_change_bid_cost() {
    let (mut baseline, position, bidder) = liquidated();
    let base = bid_cycles(&mut baseline, &bidder, position);

    let (mut w, position, bidder) = liquidated();
    let (buffer, vault) = (w.buffer().account_id, w.vault(position, 0).account_id);
    stuff_junk(&mut w, "junk-buffer", buffer);
    stuff_junk(&mut w, "junk-vault", vault);
    let junked = bid_cycles(&mut w, &bidder, position);
    assert_eq!(junked, base, "junk changed the (partial, final) bid cost");
}

/// The token program alone, a relay and a definition issuing 1 000 to A.
struct Tokens {
    chain: Chain,
    token: AccountId,
    def: AccountId,
    a: AccountId,
    a_key: PrivateKey,
    b: AccountId,
}

impl Tokens {
    fn new() -> Self {
        let mut chain = Chain::new(&[("token", methods::TOKEN_ELF), ("relay", methods::RELAY_ELF)]);
        let token = chain.program("token");
        let (a_key, a) = chain.key("a");
        let (_, b) = chain.key("b");
        let (d_key, def) = chain.key("definition");
        chain.fund(a, 1);
        let definition = Actor::new(def, token);
        let msg = TokenMsg::NewDefinition {
            minter: None,
            initial_to: Some(a),
            initial_supply: 1_000,
        };
        let a_holding = holding_actor(token, a, def);
        chain
            .send(definition, &[definition, a_holding], &[&d_key], &msg)
            .expect("the definition is created");
        Self {
            chain,
            token,
            def,
            a,
            a_key,
            b,
        }
    }

    fn holding(&self, owner: AccountId) -> Actor {
        holding_actor(self.token, owner, self.def)
    }

    /// A, signing, transfers `amount` to `to` through its own token actor.
    fn transfer_from_a(
        &mut self,
        to: AccountId,
        amount: u128,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let a = Actor::new(self.a, self.token);
        let actors = [a, self.holding(self.a), self.holding(to)];
        let msg = TokenMsg::Transfer {
            definition: self.def,
            to,
            amount,
        };
        self.chain.send(a, &actors, &[&self.a_key], &msg)
    }

    /// A relay actor at `account` forwards `msg` to `to`.
    fn relay(
        &mut self,
        account: AccountId,
        to: Actor,
        msg: &TokenMsg,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let relay = Actor::new(account, self.chain.program("relay"));
        let forward = harness::world::Forward::new(to, msg);
        self.chain.send(relay, &[relay, to], &[], &forward)
    }
}

#[test]
fn holding_lives_at_the_token_pda_and_owner_actor_stays_stateless() {
    let mut t = Tokens::new();
    t.transfer_from_a(t.b, 250).expect("A pays B");
    assert_eq!(
        t.chain.state::<Holding>(t.holding(t.b)),
        Some(Holding {
            balance: 250,
            ..Holding::new(t.b, t.def)
        })
    );
    assert_eq!(t.chain.raw_state(Actor::new(t.b, t.token)), None);
    assert_eq!(t.chain.raw_state(Actor::new(t.a, t.token)), None);
    assert_eq!(
        t.chain.state::<Holding>(t.holding(t.a)).map(|h| h.balance),
        Some(750)
    );
}

#[test]
fn debit_only_from_the_owner_actor() {
    let mut t = Tokens::new();
    let (_, c) = t.chain.key("c");
    t.chain.fund(c, 1);
    t.chain.fund(t.b, 1);
    let a_holding = t.holding(t.a);
    let debit = TokenMsg::Debit {
        definition: t.def,
        action: DebitAction::Transfer { to: c, amount: 1 },
    };
    // An impostor program at A's own account and at another account, and a root message.
    for sender in [t.a, c] {
        expect_revert(t.relay(sender, a_holding, &debit), "token: NotHoldingOwner");
    }
    expect_revert(
        t.chain.send(a_holding, &[a_holding], &[], &debit),
        "token: NotHoldingOwner",
    );
    // `(C, T)` debiting A's holding is refused by the same check (unit test
    // `token_core::tests::only_the_owner_actor_may_debit`): no token path sends it.
    assert_eq!(
        t.chain.state::<Holding>(a_holding).map(|h| h.balance),
        Some(1_000)
    );
}

#[test]
fn credit_to_a_mismatched_holding_reverts() {
    let mut t = Tokens::new();
    let (_, x) = t.chain.key("x");
    t.chain.fund(x, 1);
    let credit_a = TokenMsg::Credit {
        owner: t.a,
        definition: t.def,
        amount: 1,
    };
    let credit_b = TokenMsg::Credit {
        owner: t.b,
        definition: t.def,
        amount: 1,
    };
    // A's own token actor, and A's holding credited in B's name.
    for (to, credit) in [
        (Actor::new(t.a, t.token), &credit_a),
        (t.holding(t.a), &credit_b),
    ] {
        expect_revert(t.relay(x, to, credit), "token: WrongHolding");
        expect_revert(t.chain.send(to, &[to], &[], credit), "token: WrongHolding");
    }
    assert_eq!(
        t.chain.state::<Holding>(t.holding(t.a)).map(|h| h.balance),
        Some(1_000)
    );
}

#[test]
fn a_credit_cannot_pre_block_a_definition_address() {
    let mut t = Tokens::new();
    let (x_key, x) = t.chain.key("x");
    t.transfer_from_a(x, 5)
        .expect("A pays X before X is a definition");

    let at_x = Actor::new(x, t.token);
    assert_eq!(
        t.chain.raw_state(at_x),
        None,
        "the credit left (X, T) empty"
    );
    let msg = TokenMsg::NewDefinition {
        minter: None,
        initial_to: Some(t.a),
        initial_supply: 77,
    };
    let a_holding = holding_actor(t.token, t.a, x);
    t.chain
        .send(at_x, &[at_x, a_holding], &[&x_key], &msg)
        .expect("X still becomes a definition");
    assert_eq!(
        t.chain.state::<Definition>(at_x).map(|d| d.supply),
        Some(77)
    );
    assert_eq!(
        t.chain.state::<Holding>(t.holding(x)).map(|h| h.balance),
        Some(5)
    );
}
