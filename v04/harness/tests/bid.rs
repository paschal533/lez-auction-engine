//! `Bid` (partial and final) and `ClaimLeftover` as actor conversations (design note §4.2 rows
//! 5-8, §4.3 a, §6 flows); `harness::invariants::check` after every step.
//!
//! Numbers: v0.2.4 `liquidated_world` (integration `bid.rs`): 500_000 collateral, 600_000 debt,
//! redemption price R = 0.5 and oracle 0.5, so the bid price is 0.5: target 660_000, reward
//! `min(5 + 600, 50, 60_000) × 0.5 = 25`, lot 499_975, discount 2 % at t = 0. A bidder holds
//! 1_000_000 stablecoin.
//!
//! The lot-emptying case (Sweep) uses 400_000 / 600_000 at R = oracle = 2 (the mock host judges
//! safety by the oracle price, so v0.2.4's 500_000 position would be safe at 2): reward 100,
//! lot 399_900, and the lot binds at `ceil(399_900 × 0.98 / 2) = 195_951`.

use auction_engine_params::EngineError;
use engine_core::{
    check::fail_text,
    msg::EngineMsg,
    state::{AccountingState, AuctionPhase, AuctionState, CollateralState, ConfigState},
};
use harness::{
    World,
    bidding::Bidder,
    expect_revert, invariants,
    world::{Forward, RAY},
};
use lee::{AccountId, Actor};
use token_core::{Definition, TokenMsg};

const COLLATERAL: u128 = 500_000;
const DEBT: u128 = 600_000;
const TARGET: u128 = 660_000;
const LOT: u128 = COLLATERAL - 25;
const PENALTY: u128 = TARGET - DEBT;
const FUNDS: u128 = 1_000_000;
/// floor(100_000 × 0.5 / 0.98).
const OUT_100K: u128 = 51_020;
/// floor(460_000 × 0.5 / 0.98).
const OUT_460K: u128 = 234_693;
/// floor(660_000 × 0.5 / 0.98).
const OUT_TARGET: u128 = 336_734;
/// The chain time of the liquidation and the bids, ms.
const T: u64 = 1_000_000;

/// v0.2.4 `liquidated_world`: the position liquidated as auction 0 at price 0.5, a funded bidder.
fn liquidated() -> (World, AccountId, Bidder) {
    liquidated_at(RAY / 2, COLLATERAL)
}

/// `collateral` / 600_000 opened, the oracle at `price` (= the host's R), liquidated at `T`.
fn liquidated_at(price: u128, collateral: u128) -> (World, AccountId, Bidder) {
    let mut w = World::new_at(price);
    w.set_now_ms(T);
    let position = w.open_unsafe_position(collateral, DEBT);
    w.set_price(price);
    w.liquidate(position, 0).expect("the keeper liquidates");
    let bidder = w.fund_bidder("bidder", FUNDS);
    invariants::check(&w);
    (w, position, bidder)
}

fn auction(w: &World, position: AccountId) -> AuctionState {
    w.chain
        .raw_state(w.auction(position, 0))
        .and_then(|bytes| AuctionState::decode(&bytes))
        .expect("an auction state")
}

fn accounting(w: &World) -> AccountingState {
    w.chain
        .raw_state(w.accounting)
        .and_then(|bytes| AccountingState::decode(&bytes))
        .expect("an accounting state")
}

fn collateral_state(w: &World) -> CollateralState {
    w.chain
        .raw_state(w.collateral)
        .and_then(|bytes| CollateralState::decode(&bytes))
        .expect("a collateral state")
}

fn config_state(w: &World) -> ConfigState {
    w.chain
        .raw_state(w.config)
        .and_then(|bytes| ConfigState::decode(&bytes))
        .expect("a config state")
}

fn stable_supply(w: &World) -> u128 {
    w.chain
        .state::<Definition>(Actor::new(w.stable_def, w.token))
        .expect("the stablecoin definition")
        .supply
}

fn vault_balance(w: &World, position: AccountId) -> u128 {
    w.collateral_balance(w.vault(position, 0).account_id)
}

/// Everything a refused bid must leave as it was.
#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    auction: AuctionState,
    accounting: AccountingState,
    reserved: (u128, u128),
    bidder: (u128, u128),
    vault: u128,
    buffer: u128,
    supply: u128,
}

fn snapshot(w: &World, position: AccountId, bidder: &Bidder) -> Snapshot {
    Snapshot {
        auction: auction(w, position),
        accounting: accounting(w),
        reserved: (
            collateral_state(w).type_reserved,
            config_state(w).global_reserved,
        ),
        bidder: (
            w.stable_balance(bidder.account),
            w.collateral_balance(bidder.account),
        ),
        vault: vault_balance(w, position),
        buffer: w.stable_balance(w.buffer().account_id),
        supply: stable_supply(w),
    }
}

/// The whale sends `amount` collateral straight into the auction vault.
fn donate_to_vault(w: &mut World, position: AccountId, amount: u128) {
    let whale = Actor::new(w.whale, w.token);
    let vault = w.vault(position, 0);
    let donate = TokenMsg::Transfer {
        definition: w.collateral_def,
        to: vault.account_id,
        amount,
    };
    let key = w.whale_key.clone();
    w.chain
        .send(whale, &[whale, vault], &[&key], &donate)
        .expect("the donation lands");
}

#[test]
fn partial_bid_then_final_bid_settles_once() {
    let (mut w, position, bidder) = liquidated();
    let supply = stable_supply(&w);

    w.bid(&bidder, position, 0, (100_000, OUT_100K))
        .expect("partial bid");
    invariants::check(&w);
    let a = auction(&w, position);
    assert_eq!(
        (a.phase, a.debt_raised, a.penalty_raised, a.collateral_left),
        (AuctionPhase::Auctioning, 100_000, 0, LOT - OUT_100K),
        "debt first"
    );
    assert_eq!(w.collateral_balance(bidder.account), OUT_100K);
    assert_eq!(w.stable_balance(bidder.account), FUNDS - 100_000);
    assert_eq!(stable_supply(&w), supply - 100_000, "the debt part burns");
    assert_eq!(w.chain.raw_state(w.buffer()), None, "no penalty yet");
    assert_eq!(
        accounting(&w),
        AccountingState::new(),
        "no per-bid accounting"
    );
    assert_eq!(collateral_state(&w).type_reserved, TARGET);

    w.bid(&bidder, position, 0, (100_000, 0))
        .expect("second partial bid");
    invariants::check(&w);
    assert_eq!(accounting(&w), AccountingState::new());

    w.bid(&bidder, position, 0, (460_000, 0))
        .expect("final bid");
    invariants::check(&w);
    let a = auction(&w, position);
    assert_eq!(a.phase, AuctionPhase::Settled);
    assert_eq!((a.debt_raised, a.penalty_raised), (DEBT, PENALTY));
    let left = LOT - 2 * OUT_100K - OUT_460K;
    assert_eq!((a.collateral_left, a.leftover_claimable), (left, left));
    let acc = accounting(&w);
    assert_eq!(
        (
            acc.total_debt_burned,
            acc.total_penalty_collected,
            acc.unbacked_debt,
            acc.total_shortfall_booked
        ),
        (DEBT, PENALTY, 0, 0)
    );
    assert_eq!(w.stable_balance(w.buffer().account_id), PENALTY);
    assert_eq!(stable_supply(&w), supply - DEBT);
    assert_eq!(w.stable_balance(bidder.account), FUNDS - TARGET);
    assert_eq!(
        w.collateral_balance(bidder.account),
        2 * OUT_100K + OUT_460K
    );
    assert_eq!(
        vault_balance(&w, position),
        left,
        "the target binds: no sweep"
    );
    assert_eq!(
        (
            collateral_state(&w).type_reserved,
            config_state(&w).global_reserved
        ),
        (0, 0)
    );

    // A settled auction never settles again.
    let settled = snapshot(&w, position, &bidder);
    expect_revert(
        w.bid(&bidder, position, 0, (1_000, 0)),
        &fail_text(EngineError::NotAuctioning),
    );
    assert_eq!(snapshot(&w, position, &bidder), settled);
    invariants::check(&w);
}

#[test]
fn bid_after_settlement_reverts() {
    let (mut w, position, bidder) = liquidated();
    w.bid(&bidder, position, 0, (TARGET, 0))
        .expect("one bid reaches the target");
    invariants::check(&w);
    assert_eq!(auction(&w, position).phase, AuctionPhase::Settled);
    assert_eq!(w.collateral_balance(bidder.account), OUT_TARGET);
    let settled = snapshot(&w, position, &bidder);
    for amounts in [(1_000, 0), (TARGET, 0), (u128::MAX, 0)] {
        expect_revert(
            w.bid(&bidder, position, 0, amounts),
            &fail_text(EngineError::NotAuctioning),
        );
        assert_eq!(snapshot(&w, position, &bidder), settled);
        invariants::check(&w);
    }
}

#[test]
fn split_bid_neutrality_holds() {
    let (mut whole, position, bidder) = liquidated();
    whole
        .bid(&bidder, position, 0, (200_001, 0))
        .expect("one bid");
    invariants::check(&whole);

    let (mut split, position_2, bidder_2) = liquidated();
    assert_eq!(position_2, position, "the worlds are deterministic");
    for part in [100_000, 100_001] {
        split
            .bid(&bidder_2, position, 0, (part, 0))
            .expect("half a bid");
        invariants::check(&split);
    }

    let one = whole.collateral_balance(bidder.account);
    let sum = split.collateral_balance(bidder_2.account);
    assert_eq!(one, 102_041, "floor(200_001 × 0.5 / 0.98)");
    assert_eq!(
        sum,
        2 * OUT_100K,
        "floor(100_001 × 0.5 / 0.98) is 51_020 too"
    );
    assert!(sum <= one, "split {sum} > whole {one}");
    let (a, b) = (auction(&whole, position), auction(&split, position));
    assert_eq!(a.debt_raised, b.debt_raised, "same charge");
    assert_eq!(
        whole.stable_balance(bidder.account),
        split.stable_balance(bidder_2.account)
    );
    assert_eq!(b.collateral_left, LOT - sum);
}

#[test]
fn zero_bid_and_oversized_min_out_are_refused_without_token_moves() {
    let (mut w, position, bidder) = liquidated();
    let before = snapshot(&w, position, &bidder);
    assert_eq!(before.bidder, (FUNDS, 0));
    expect_revert(
        w.bid(&bidder, position, 0, (0, 0)),
        &fail_text(EngineError::ZeroFill),
    );
    assert_eq!(snapshot(&w, position, &bidder), before);
    invariants::check(&w);
    expect_revert(
        w.bid(&bidder, position, 0, (100_000, OUT_100K + 1)),
        &fail_text(EngineError::Slippage),
    );
    assert_eq!(snapshot(&w, position, &bidder), before);
    invariants::check(&w);
    // v0.2.4's other non-final rules, at the same handler.
    expect_revert(
        w.bid(&bidder, position, 0, (10, 0)),
        &fail_text(EngineError::BelowMinBid),
    );
    expect_revert(
        w.bid(&bidder, position, 0, (TARGET - 50, 0)),
        &fail_text(EngineError::DustRemainder),
    );
    assert_eq!(snapshot(&w, position, &bidder), before);
}

#[test]
fn a_bid_needs_the_bidder_signature_and_funds() {
    let (mut w, position, bidder) = liquidated();
    let poor = w.fund_bidder("poor", 1_000);
    let before = snapshot(&w, position, &bidder);
    let msg = w.bid_msg(bidder.account, position, 0, (100_000, 0), w.now_ms);
    expect_revert(w.send_bid(&msg, &[]), "token: Unauthorized");
    assert_eq!(snapshot(&w, position, &bidder), before);
    expect_revert(
        w.bid(&poor, position, 0, (100_000, 0)),
        "token: InsufficientBalance",
    );
    assert_eq!(snapshot(&w, position, &bidder), before);
    assert_eq!(w.stable_balance(poor.account), 1_000);
    invariants::check(&w);
}

/// The bid observes at its clock claim (Observe, `require_live`), as v0.2.4 did: a feed that is
/// stale at `now + slack` stops it, and the reverted observation is dropped.
#[test]
fn a_stale_feed_stops_the_bid() {
    let (mut w, position, bidder) = liquidated();
    let max_age =
        auction_engine_params::args::CollateralArgs::defaults_for_tests().max_oracle_age_ms;
    w.set_now_ms(T + max_age);
    let before = (snapshot(&w, position, &bidder), collateral_state(&w));
    expect_revert(
        w.bid(&bidder, position, 0, (100_000, 0)),
        &fail_text(EngineError::PriceFeedStale),
    );
    assert_eq!(
        (snapshot(&w, position, &bidder), collateral_state(&w)),
        before
    );
    invariants::check(&w);
}

#[test]
fn lot_emptying_final_bid_sweeps_the_vault_and_books_the_shortfall() {
    let (mut w, position, bidder) = liquidated_at(2 * RAY, 400_000);
    let lot = 400_000 - 100;
    assert_eq!(vault_balance(&w, position), lot);
    donate_to_vault(&mut w, position, 7);
    invariants::check(&w);

    w.bid(&bidder, position, 0, (TARGET, 0))
        .expect("the lot binds");
    invariants::check(&w);
    let charged = 195_951;
    let a = auction(&w, position);
    assert_eq!(
        (
            a.phase,
            a.debt_raised,
            a.collateral_left,
            a.leftover_claimable
        ),
        (AuctionPhase::Settled, charged, 0, 0)
    );
    assert_eq!(w.stable_balance(bidder.account), FUNDS - charged);
    assert_eq!(
        w.collateral_balance(bidder.account),
        lot + 7,
        "the sweep pays the donation too (ADR-033)"
    );
    assert_eq!(vault_balance(&w, position), 0);
    let acc = accounting(&w);
    assert_eq!(
        (
            acc.unbacked_debt,
            acc.total_shortfall_booked,
            acc.total_debt_burned
        ),
        (DEBT - charged, DEBT - charged, charged)
    );
    assert_eq!(w.chain.raw_state(w.buffer()), None, "no penalty raised");
    assert_eq!(
        (
            collateral_state(&w).type_reserved,
            config_state(&w).global_reserved
        ),
        (0, 0)
    );
    expect_revert(
        w.claim(position, 0),
        &fail_text(EngineError::NothingToClaim),
    );
}

#[test]
fn claim_leftover_only_by_the_owner_and_only_once() {
    let (mut w, position, bidder) = liquidated();
    let owner = w.owner_of(position).account;
    expect_revert(
        w.claim(position, 0),
        &fail_text(EngineError::NothingToClaim),
    );
    w.bid(&bidder, position, 0, (TARGET, 0))
        .expect("the target is reached");
    invariants::check(&w);
    let leftover = LOT - OUT_TARGET;
    assert_eq!(auction(&w, position).leftover_claimable, leftover);
    donate_to_vault(&mut w, position, 7);

    let key = bidder.key.clone();
    expect_revert(
        w.claim_as(bidder.account, &[&key], position, 0),
        &fail_text(EngineError::NotOwner),
    );
    expect_revert(
        w.claim_as(owner, &[], position, 0),
        &fail_text(EngineError::NotOwner),
    );
    invariants::check(&w);
    assert_eq!(vault_balance(&w, position), leftover + 7);

    let owner_before = w.collateral_balance(owner);
    w.claim(position, 0).expect("the owner claims");
    invariants::check(&w);
    assert_eq!(
        w.collateral_balance(owner),
        owner_before + leftover + 7,
        "the claim sweeps the vault"
    );
    assert_eq!(vault_balance(&w, position), 0);
    let a = auction(&w, position);
    assert_eq!(a.leftover_claimed, leftover);

    expect_revert(
        w.claim(position, 0),
        &fail_text(EngineError::NothingToClaim),
    );
    assert_eq!(auction(&w, position), a);
    invariants::check(&w);
}

/// `BidCtx`, `Settled` and `Release` from anyone but their one sender (design note §4.3 c, guard
/// 3), as root messages and relayed by a foreign actor; and a root `Bid` relayed by an actor.
#[test]
fn forged_bid_ctx_settled_and_release_are_refused() {
    let (mut w, position, bidder) = liquidated();
    let before = snapshot(&w, position, &bidder);
    let auction = w.auction(position, 0);
    let ctx = EngineMsg::BidCtx {
        bidder: bidder.account,
        position,
        nonce: 0,
        max_debt_in: TARGET,
        min_collateral_out: 0,
        price_ray: RAY * 1_000,
        now_ms: T,
        paused_total_ms: 0,
        min_bid: 0,
        chost: 0,
    };
    let settled = EngineMsg::Settled {
        config: w.config.account_id,
        position,
        nonce: 0,
        debt_raised: 0,
        penalty_raised: 0,
        shortfall: 1_000_000,
        release: TARGET,
        leftover: 0,
    };
    let release = EngineMsg::Release {
        position,
        nonce: 0,
        amount: TARGET,
    };
    let bid = w.bid_msg(bidder.account, position, 0, (100_000, 0), T);
    let relay = w.publisher;
    let cases = [
        (auction, &ctx, EngineError::WrongAccount("coll_config")),
        (w.accounting, &settled, EngineError::WrongAccount("auction")),
        (w.collateral, &release, EngineError::WrongAccount("auction")),
        (w.config, &release, EngineError::WrongAccount("auction")),
    ];
    for (to, msg, err) in cases {
        expect_revert(w.chain.send(to, &[to], &[], msg), &fail_text(err));
        expect_revert(
            w.chain
                .send(relay, &[relay, to], &[], &Forward::new(to, msg)),
            &fail_text(err),
        );
    }
    let collateral = w.collateral;
    expect_revert(
        w.chain.send(
            relay,
            &[relay, collateral],
            &[],
            &Forward::new(collateral, &bid),
        ),
        &fail_text(EngineError::WrongAccount("root message expected")),
    );
    assert_eq!(snapshot(&w, position, &bidder), before);
    invariants::check(&w);
}

/// Ruling R-9a: the keeper reward takes the whole seizure (3 collateral, 10_000 debt at P = 1:
/// reward `min(5 + 10, 50) = 15` capped at the lot 3), so every bid fails `ZeroFill` and only
/// `SettleEmpty` (v0.2.4 `settle_empty`) settles it, books the shortfall and frees the caps.
#[test]
fn empty_lot_auction_settles_and_releases_reservations() {
    let (mut w, live, bidder) = liquidated();
    let position = w.open_unsafe_position(3, 10_000);
    w.set_price(RAY / 2);
    w.liquidate(position, 0).expect("the reward takes the lot");
    invariants::check(&w);
    let a = auction(&w, position);
    assert_eq!(
        (
            a.phase,
            a.keeper_reward,
            a.collateral_left,
            a.reserved_amount
        ),
        (AuctionPhase::Auctioning, 3, 0, 11_000)
    );
    assert_eq!(collateral_state(&w).type_reserved, TARGET + 11_000);
    expect_revert(
        w.bid(&bidder, position, 0, (11_000, 0)),
        &fail_text(EngineError::ZeroFill),
    );
    // A live auction with a lot left is not empty.
    expect_revert(
        w.settle_empty(live, 0),
        &fail_text(EngineError::NotAuctioning),
    );

    let msg = w.settle_empty_msg(position, 0);
    let actors = w.settle_empty_actors(position, 0);
    let (cycles, outcome) = w.chain.cycles(w.config, &actors, &[], &msg);
    outcome.expect("simulates");
    println!("cycles: settle_empty {cycles}");
    w.settle_empty(position, 0).expect("settles the empty lot");
    invariants::check(&w);
    let a = auction(&w, position);
    assert_eq!(a.phase, AuctionPhase::Settled);
    assert_eq!(
        (
            collateral_state(&w).type_reserved,
            config_state(&w).global_reserved
        ),
        (TARGET, TARGET),
        "only the live auction keeps its reservation"
    );
    let acc = accounting(&w);
    assert_eq!(
        (
            acc.unbacked_debt,
            acc.total_shortfall_booked,
            acc.total_debt_burned
        ),
        (10_000, 10_000, 0)
    );

    let settled = (auction(&w, position), accounting(&w));
    expect_revert(
        w.settle_empty(position, 0),
        &fail_text(EngineError::NotAuctioning),
    );
    assert_eq!((auction(&w, position), accounting(&w)), settled);
    expect_revert(
        w.settle_empty(position, 1),
        &fail_text(EngineError::WrongAccount("auction")),
    );
    invariants::check(&w);
}

/// Ruling R-9b: a bid naming an auction that does not exist fails with v0.2.4's
/// `WrongAccount("auction")` (seeded `Probe { Auction }`), not with LEZ's admission error.
#[test]
fn bid_on_an_absent_auction_reverts_with_engine_error() {
    let (mut w, position, bidder) = liquidated();
    // A position that was never liquidated, so its auction 0 does not exist either.
    let fresh = w.open_unsafe_position(1_000, 1_000);
    w.set_price(RAY / 2);
    let before = snapshot(&w, position, &bidder);
    for (target, nonce) in [(position, 1), (fresh, 0)] {
        expect_revert(
            w.bid(&bidder, target, nonce, (100_000, 0)),
            &fail_text(EngineError::WrongAccount("auction")),
        );
        assert_eq!(w.chain.raw_state(w.auction(target, nonce)), None);
        assert_eq!(snapshot(&w, position, &bidder), before);
        invariants::check(&w);
    }
}

/// Ruling R-9b: a claim before the position was ever liquidated fails with v0.2.4's
/// `WrongAccount("auction")`.
#[test]
fn claim_before_liquidation_reverts_with_engine_error() {
    let mut w = World::new_at(RAY / 2);
    w.set_now_ms(T);
    let position = w.open_unsafe_position(COLLATERAL, DEBT);
    expect_revert(
        w.claim(position, 0),
        &fail_text(EngineError::WrongAccount("auction")),
    );
    assert_eq!(w.chain.raw_state(w.auction(position, 0)), None);
    invariants::check(&w);
}

/// Cycles of a partial bid, a final bid and a claim (`--nocapture` prints them).
#[test]
fn bid_and_claim_cycles_fit_the_budget() {
    let (mut w, position, bidder) = liquidated();
    let measure = |w: &World, msg: &EngineMsg, root: Actor, actors: &[Actor]| {
        let evidence = w.bid_evidence();
        let (cycles, outcome) =
            w.chain
                .cycles_with_evidence(root, actors, &[&bidder.key], &evidence, msg);
        outcome.expect("simulates");
        cycles
    };
    let partial = w.bid_msg(bidder.account, position, 0, (100_000, 0), w.now_ms);
    let actors = w.bid_actors(bidder.account, position, 0);
    let partial_cycles = measure(&w, &partial, w.collateral, &actors);
    w.send_bid(&partial, &[&bidder.key]).expect("partial");
    let last = w.bid_msg(bidder.account, position, 0, (TARGET, 0), w.now_ms);
    let final_cycles = measure(&w, &last, w.collateral, &actors);
    w.send_bid(&last, &[&bidder.key]).expect("final");
    let owner = w.owner_of(position);
    let (owner_account, owner_key) = (owner.account, owner.key.clone());
    let claim = w.claim_msg(position, 0);
    let claim_actors = w.proxy_actors(owner_account, &claim);
    let (claim_cycles, outcome) = w.chain.cycles(
        Actor::new(owner_account, w.engine),
        &claim_actors,
        &[&owner_key],
        &claim,
    );
    outcome.expect("simulates");
    println!(
        "cycles: partial bid {partial_cycles}, final bid {final_cycles}, claim {claim_cycles}"
    );
    let budget = lee::program::DEFAULT_PUBLIC_CYCLE_BUDGET;
    for cycles in [partial_cycles, final_cycles, claim_cycles] {
        assert!(cycles > 0 && cycles < budget);
    }
}
