//! Cycles of the Tier 2 rows (Plan 9, spec §3.1.9, §3.2.7 and §4; ruling R9-10): `Liquidate` and
//! both `Bid`s under `RiskArgs::recommended()` with a full TWAP ring (the worst case), plus
//! `ObserveFeed`, `SetCollateral` and the new `SetPauseLevel`, each asserted against its budget
//! (`--nocapture` prints the table; it is also written to `target/cycles-v04-tier2.md`, see
//! `v04/CYCLES.md`).
//!
//! The scenario is `cycles.rs`'s (collateral 500_000, debt 600_000, R and the oracle at 0.5 RAY)
//! in `World::new_tier2`. The R9-13 e2e values are not used: R9-3 refuses them at
//! `TIER2_MAX_AGE_MS`. A sample recorded at `now == ts` has no weight, so every measured row runs
//! at least 1_000 ms after the newest sample.
//!
//! `Liquidate` and a partial `Bid` are measured a second time at a price of 2_000_000
//! ([`HIGH`]), where `price · weight` of a TWAP term is past u128, against the same budgets.

use engine_core::msg::EngineMsg;
use harness::{
    World,
    bidding::Bidder,
    cycles::{Row, measured, proxied, table, write_table},
    tier2::tier2_collateral_args,
    world::{MIN_CR_RAY, RAY},
};
use lee::{AccountId, Actor, error::LeeError};
use risk_core::{RiskArgs, TWAP_CAPACITY};

/// One measured world: the oracle price (= R), the host's minimum collateral ratio, and a
/// position that is unsafe under them.
#[derive(Clone, Copy)]
struct Scenario {
    price: u128,
    min_cr_ray: u128,
    collateral: u128,
    debt: u128,
    funds: u128,
    /// A partial bid's coin in.
    bid_in: u128,
}

/// `cycles.rs`'s scenario.
const BASE: Scenario = Scenario {
    price: P,
    min_cr_ray: MIN_CR_RAY,
    collateral: 500_000,
    debt: 600_000,
    funds: 1_000_000,
    bid_in: 100_000,
};
/// [`BASE`]'s debt at a price of 2_000_000 RAY (2e33). A full spacing's weight is 300_000 ms
/// and `2e33 · 300_000 > 2^128`, so the TWAP terms of full weight are past `risk_core::math`'s
/// first route.
///
/// The mock host calls a position unsafe when `collateral · price < min_cr · debt`, and a bid
/// pays `coin · price` collateral. At this price no lot is both unsafe at the host's 150 % and
/// large enough to outlast the keeper reward (up to 50 coin, 1e8 collateral) and a partial bid,
/// so the host's ratio is raised to the largest value. A bid that reaches the target does not
/// exist here (the lot is worth 25_000 coin against 660_000), so only `Liquidate` and a partial
/// `Bid` are measured; the final `Bid` prices through the same `bid_pricing` call.
const HIGH: Scenario = Scenario {
    price: 2_000_000_000_000_000_000_000_000_000_000_000,
    min_cr_ray: u128::MAX,
    collateral: 50_000_000_000,
    debt: 600_000,
    funds: 1_000_000,
    bid_in: 5_000,
};
/// [`BASE`]'s target: the debt plus the 10 % penalty.
const TARGET: u128 = 660_000;
/// The oracle price of [`BASE`], = R.
const P: u128 = RAY / 2;
/// The chain time the position opens at, ms.
const T: u64 = 1_000_000;
/// The recommended preset's sample spacing, ms.
const SPACING: u64 = 300_000;
/// How long after the newest sample each measured row runs, ms.
const AFTER_SAMPLE_MS: u64 = 1_000;

/// Budgets (spec §4, ruling R9-10).
const PARTIAL_BID_BUDGET: u64 = 520_000;
const FINAL_BID_BUDGET: u64 = 720_000;
const LIQUIDATE_BUDGET: u64 = 1_000_000;
const SET_PAUSE_LEVEL_BUDGET: u64 = 400_000;
/// Most a row may cost at [`HIGH`]'s price over the same row at [`BASE`]'s. The frozen division
/// costs about 10,000 cycles a call (spike S4) and a full ring has 6 terms past u128, so a TWAP
/// back on the frozen path would add about 60,000.
const HIGH_PRICE_EXTRA: u64 = 20_000;
/// Most a partial bid's cycles may differ between a ring of 1 sample and one of 8 (spec §3.1.9).
const RING_FILL_SPREAD: u64 = 40_000;

/// Executions, counted from the as-built message sequences as in `cycles.rs` (the TWAP and the
/// level checks add computation, not messages, so the slice rows keep their counts):
///
/// - `Liquidate` 29, `Bid (partial)` 15, `Bid (final)` 21, `SetCollateral` 11, `ObserveFeed` 7:
///   `cycles.rs`'s counts.
/// - `SetPauseLevel` 12: the admin's proxy, the seeded `Probe { Config }`, `SetPauseLevel` at the
///   config, the seeded `Probe { Collateral }`, `PauseLevelUpdate` at the collateral actor, then
///   the Observe block: `Observe`, the host's `ReadState` and its `StateReply`, the oracle's
///   `ReadState` and its `StateReply`, the clock's `AssertTimestamp`, `Proceed`.
const LIQUIDATE_EXECUTIONS: u32 = 29;
const PARTIAL_BID_EXECUTIONS: u32 = 15;
const FINAL_BID_EXECUTIONS: u32 = 21;
const SET_COLLATERAL_EXECUTIONS: u32 = 11;
const OBSERVE_FEED_EXECUTIONS: u32 = 7;
const SET_PAUSE_LEVEL_EXECUTIONS: u32 = 12;

type Simulated = (u64, Result<(), LeeError>);

/// Advances the world's time by `ms`.
fn later(w: &mut World, ms: u64) {
    let now = w.now_ms.checked_add(ms).expect("time fits");
    w.set_now_ms(now);
}

/// The Tier 2 world of `s` under `risk` with one unsafe position, the ring warmed with `samples`
/// samples at the scenario's price, at `AFTER_SAMPLE_MS` past the newest.
fn warmed(s: Scenario, risk: RiskArgs, samples: u8) -> (World, AccountId) {
    let mut w = World::new_tier2_at(s.price, s.min_cr_ray, risk);
    w.set_now_ms(T);
    let position = w.open_unsafe_position(s.collateral, s.debt);
    w.set_price(s.price);
    w.warm_twap(s.price, samples, SPACING);
    later(&mut w, AFTER_SAMPLE_MS);
    assert_eq!(ring_len(&w), usize::from(samples));
    (w, position)
}

fn ring_len(w: &World) -> usize {
    w.collateral_state().ext.ring.samples.len()
}

fn full_ring() -> u8 {
    u8::try_from(TWAP_CAPACITY).expect("the capacity fits u8")
}

fn bid_cycles(w: &World, bidder: &Bidder, msg: &EngineMsg, position: AccountId) -> Simulated {
    let actors = w.bid_actors(bidder.account, position, 0);
    w.chain.cycles(w.collateral, &actors, &[&bidder.key], msg)
}

fn liquidate_cycles(w: &World, position: AccountId) -> Simulated {
    let msg = w.liquidate_msg(position, 0, w.now_ms);
    let actors = w.proxy_actors(w.keeper, &msg);
    let keeper_key = w.keeper_key.clone();
    w.chain.cycles(
        Actor::new(w.keeper, w.engine),
        &actors,
        &[&keeper_key],
        &msg,
    )
}

/// The budgeted rows. Every row is measured and the table printed before any budget is checked,
/// so a failure shows every figure.
const BUDGETS: [(&str, u64); 4] = [
    ("SetPauseLevel", SET_PAUSE_LEVEL_BUDGET),
    ("Liquidate", LIQUIDATE_BUDGET),
    ("Bid (partial)", PARTIAL_BID_BUDGET),
    ("Bid (final)", FINAL_BID_BUDGET),
];
/// The rows [`auction_rows`] measures.
const AUCTION_BUDGETS: [(&str, u64); 2] = [
    ("Liquidate", LIQUIDATE_BUDGET),
    ("Bid (partial)", PARTIAL_BID_BUDGET),
];

/// Every row of `budgets` is present and fits its budget (and, by `measured`, the gas cap).
fn assert_budgets(rows: &[Row], budgets: &[(&str, u64)]) {
    for &(instruction, budget) in budgets {
        let row = rows
            .iter()
            .find(|r| r.instruction == instruction)
            .unwrap_or_else(|| panic!("no {instruction} row"));
        assert!(
            row.cycles <= budget,
            "{instruction}: {} cycles against a budget of {budget}",
            row.cycles
        );
    }
}

/// `SetPauseLevel` and `SetCollateral` through the admin's proxy, then `ObserveFeed` on a fresh
/// publish that evicts the oldest sample of the full ring.
fn admin_and_feed_rows(w: &mut World, rows: &mut Vec<Row>) {
    let (admin, admin_key) = (w.admin, w.admin_key.clone());
    let pause = w.set_pause_level_msg(1);
    rows.push(measured(
        "SetPauseLevel",
        SET_PAUSE_LEVEL_EXECUTIONS,
        proxied(w, admin, &admin_key, &pause),
    ));
    let set = w.set_collateral_msg_with(tier2_collateral_args(), RiskArgs::recommended());
    rows.push(measured(
        "SetCollateral",
        SET_COLLATERAL_EXECUTIONS,
        proxied(w, admin, &admin_key, &set),
    ));

    later(w, SPACING);
    w.set_price(P);
    let observe = EngineMsg::ObserveFeed {
        config: w.config.account_id,
        collateral_def: w.collateral_def,
        now_ms: w.now_ms,
    };
    let actors = [w.collateral, w.params, w.oracle, w.chain.clock()];
    rows.push(measured(
        "ObserveFeed",
        OBSERVE_FEED_EXECUTIONS,
        w.chain.cycles(w.collateral, &actors, &[], &observe),
    ));
    let now = w.now_ms;
    w.observe_feed(now).expect("the feed is observed");
    later(w, AFTER_SAMPLE_MS);
    assert_eq!(ring_len(w), TWAP_CAPACITY);
}

/// `Liquidate` and a partial `Bid` of scenario `s` on the full ring of `w`. Returns the bidder.
fn auction_rows(s: Scenario, w: &mut World, position: AccountId, rows: &mut Vec<Row>) -> Bidder {
    rows.push(measured(
        "Liquidate",
        LIQUIDATE_EXECUTIONS,
        liquidate_cycles(w, position),
    ));
    w.liquidate(position, 0).expect("the keeper liquidates");
    let bidder = w.fund_bidder("bidder", s.funds);
    assert_eq!(ring_len(w), TWAP_CAPACITY);

    let partial = w.bid_msg(bidder.account, position, 0, (s.bid_in, 0), w.now_ms);
    rows.push(measured(
        "Bid (partial)",
        PARTIAL_BID_EXECUTIONS,
        bid_cycles(w, &bidder, &partial, position),
    ));
    w.send_bid(&partial, &[&bidder.key]).expect("partial bid");
    assert_eq!(ring_len(w), TWAP_CAPACITY);
    bidder
}

#[test]
fn tier2_rows_fit_the_budget() {
    let mut rows = Vec::new();
    let (mut w, position) = warmed(BASE, RiskArgs::recommended(), full_ring());
    admin_and_feed_rows(&mut w, &mut rows);
    let bidder = auction_rows(BASE, &mut w, position, &mut rows);
    let last = w.bid_msg(bidder.account, position, 0, (TARGET, 0), w.now_ms);
    rows.push(measured(
        "Bid (final)",
        FINAL_BID_EXECUTIONS,
        bid_cycles(&w, &bidder, &last, position),
    ));
    w.send_bid(&last, &[&bidder.key]).expect("final bid");

    let markdown = table(&rows);
    println!("{markdown}");
    write_table("cycles-v04-tier2.md", &markdown);
    assert_budgets(&rows, &BUDGETS);
}

/// `Liquidate` and a partial `Bid` at [`HIGH`]'s price fit the same budgets and cost at most
/// [`HIGH_PRICE_EXTRA`] more than at [`BASE`]'s: a TWAP term's `price · weight` is past u128
/// there, and `risk_core::math` still divides natively.
#[test]
fn tier2_rows_fit_the_budget_at_a_high_price() {
    let weight = u128::from(SPACING);
    assert!(HIGH.price.checked_mul(weight).is_none(), "past route 1");
    let mut rows = Vec::new();
    let (mut w, position) = warmed(HIGH, RiskArgs::recommended(), full_ring());
    auction_rows(HIGH, &mut w, position, &mut rows);

    let markdown = table(&rows);
    println!("{markdown}");
    write_table("cycles-v04-tier2-high-price.md", &markdown);
    assert_budgets(&rows, &AUCTION_BUDGETS);

    let mut base_rows = Vec::new();
    let (mut w, position) = warmed(BASE, RiskArgs::recommended(), full_ring());
    auction_rows(BASE, &mut w, position, &mut base_rows);
    assert_eq!(rows.len(), base_rows.len());
    for (high, base) in rows.iter().zip(&base_rows) {
        assert!(
            high.cycles <= base.cycles.saturating_add(HIGH_PRICE_EXTRA),
            "{}: {} at the high price, {} at the base price",
            high.instruction,
            high.cycles,
            base.cycles
        );
    }
}

/// A partial bid's cycles with `samples` samples in the ring, `twap_min_obs` 1.
fn partial_bid_cycles(samples: u8) -> u64 {
    let risk = RiskArgs {
        twap_min_obs: 1,
        ..RiskArgs::recommended()
    };
    let (mut w, position) = warmed(BASE, risk, samples);
    w.liquidate(position, 0).expect("the keeper liquidates");
    assert_eq!(
        ring_len(&w),
        usize::from(samples),
        "liquidating adds no sample"
    );
    let bidder = w.fund_bidder("bidder", BASE.funds);
    let partial = w.bid_msg(bidder.account, position, 0, (BASE.bid_in, 0), w.now_ms);
    let row = measured(
        "Bid (partial)",
        PARTIAL_BID_EXECUTIONS,
        bid_cycles(&w, &bidder, &partial, position),
    );
    row.cycles
}

#[test]
fn bid_cost_is_independent_of_ring_fill() {
    let one = partial_bid_cycles(1);
    let full = partial_bid_cycles(full_ring());
    println!("Bid (partial): ring of 1: {one} cycles; ring of 8: {full} cycles");
    assert!(
        one.abs_diff(full) <= RING_FILL_SPREAD,
        "ring of 1: {one}, ring of 8: {full}, spread above {RING_FILL_SPREAD}"
    );
}
