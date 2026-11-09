//! E1 in the engine (spec §3.1, ADR-070-ADR-072): the TWAP ring the observations fill, the
//! TWAP-backed liquidation price, the spot-vs-TWAP breaker and the TWAP anchor on bid payouts;
//! `harness::invariants::check` at the end of every test.
//!
//! Numbers: the Tier 2 world (R = 0.5 RAY, band `[0.45, 0.75]` RAY, sanity `[0.25, 1.0]` RAY,
//! feed max age 600_000 ms), 500_000 collateral and 600_000 debt, the oracle at p = 0.5 RAY.
//! Samples are 300_000 ms apart (the recommended spacing). A sample weighs the time it has been
//! current, so each liquidation runs 1_000 ms after the newest publish.

use auction_engine_params::args::CollateralArgs;
use engine_core::{
    bid::{BidQuote, quote},
    check::fail_text,
    liquidation::{AuctionIdentity, SeizeReport, awaiting_seize, quote_seized},
    state::{AuctionState, CollateralState, ConfigState, QuoteCtx},
};
use harness::{
    World,
    bidding::Bidder,
    expect_revert, invariants,
    tier2::{TIER2_MAX_AGE_MS, tier2_collateral_args},
    world::RAY,
};
use lee::AccountId;
use risk_core::{RiskArgs, RiskError, TwapRing, edge, liq_price, twap};

const COLLATERAL: u128 = 500_000;
const DEBT: u128 = 600_000;
/// The oracle price the TWAP warms at, = R.
const P: u128 = RAY / 2;
/// The Tier 2 world's redemption price.
const R: u128 = RAY / 2;
/// The recommended preset's sample spacing, ms.
const SPACING: u64 = 300_000;
/// The chain time the position opens at, ms.
const T0: u64 = 1_000_000;
/// The bids' coin in.
const BID_IN: u128 = 100_000;

/// A Tier 2 world under `risk` at `T0` with one unsafe position and the oracle at `P` (not yet
/// observed).
fn world(risk: RiskArgs) -> (World, AccountId) {
    let mut w = World::new_tier2(risk);
    w.set_now_ms(T0);
    let position = w.open_unsafe_position(COLLATERAL, DEBT);
    w.set_price(P);
    (w, position)
}

/// Advances the world's time by `ms`.
fn later(w: &mut World, ms: u64) {
    let now = w.now_ms.checked_add(ms).expect("time fits");
    w.set_now_ms(now);
}

fn auction_state(w: &World, position: AccountId) -> Option<AuctionState> {
    w.chain
        .raw_state(w.auction(position, 0))
        .map(|bytes| AuctionState::decode(&bytes).expect("an auction state"))
}

fn config_state(w: &World) -> ConfigState {
    w.chain
        .raw_state(w.config)
        .and_then(|bytes| ConfigState::decode(&bytes))
        .expect("a config state")
}

/// The recommended TWAP of `ring` at `now_ms` in the Tier 2 world.
fn twap_of(ring: &TwapRing, now_ms: u64) -> u128 {
    let r = RiskArgs::recommended();
    twap(
        ring,
        now_ms,
        r.twap_window_ms,
        r.twap_min_obs,
        TIER2_MAX_AGE_MS,
    )
    .expect("a twap")
    .0
}

/// The keeper reward `auction_core`'s quote gives the world's seizure at `price_ray`, with the
/// collateral and config as they were before the liquidation.
fn reward_at(coll: &CollateralState, config: &ConfigState, w: &World, price_ray: u128) -> u128 {
    let ctx = QuoteCtx {
        price_ray,
        global_cap: config.global_cap,
        global_reserved: config.global_reserved,
        collateral: coll.clone(),
    };
    let id = AuctionIdentity {
        config: w.config.account_id,
        position: w.whale,
        nonce: 0,
        host: w.params,
        keeper: w.keeper,
        start_ms: w.now_ms,
        token: w.token,
        stable_def: w.stable_def,
    };
    let report = SeizeReport {
        owner: w.whale,
        collateral: COLLATERAL,
        nominal_debt: DEBT,
    };
    let (opened, _) =
        quote_seized(&awaiting_seize(&id, ctx.clone()), &ctx, &report).expect("the seizure quotes");
    opened.keeper_reward
}

/// The collateral a `BID_IN` bid on `a` at `price_ray` pays out, at the world's time.
fn out_at(w: &World, a: &AuctionState, price_ray: u128) -> u128 {
    let coll = w.collateral_state();
    let params = coll.collateral_params().expect("params");
    let q = BidQuote {
        max_debt_in: BID_IN,
        min_collateral_out: 0,
        price_ray,
        now_ms: w.now_ms,
        paused_total_ms: coll.paused_total_ms,
        min_bid: params.min_bid,
        chost: params.chost().expect("chost"),
    };
    quote(a, &q).expect("the bid quotes").fill.collateral_out
}

/// `bidder` bids `BID_IN` on auction 0 of `position` and returns the collateral it received.
fn bid_out(w: &mut World, bidder: &Bidder, position: AccountId) -> u128 {
    let before = w.collateral_balance(bidder.account);
    w.bid(bidder, position, 0, (BID_IN, 0))
        .expect("the bid fills");
    w.collateral_balance(bidder.account)
        .checked_sub(before)
        .expect("the bidder gains collateral")
}

#[test]
fn liquidate_refused_until_twap_has_min_obs() {
    let (mut w, position) = world(RiskArgs::recommended());
    w.warm_twap(P, 2, SPACING);
    later(&mut w, 1_000);
    let before = w.collateral_state();
    expect_revert(
        w.liquidate(position, 0),
        &fail_text(RiskError::TwapUnavailable),
    );
    assert_eq!(w.collateral_state(), before);
    assert_eq!(auction_state(&w, position), None);

    w.warm_twap(P, 1, SPACING);
    later(&mut w, 1_000);
    w.liquidate(position, 0)
        .expect("three samples carry weight");
    assert!(auction_state(&w, position).is_some());
    invariants::check(&w);
}

#[test]
fn keeper_reward_uses_the_smaller_of_spot_and_twap() {
    let (mut w, position) = world(RiskArgs::recommended());
    w.warm_twap(P, 3, SPACING);
    // 1.1 p, 1_000 ms after the newest sample: closer than the spacing, so it is not recorded.
    let spike = P / 10 * 11;
    later(&mut w, 1_000);
    w.set_price(spike);
    later(&mut w, 1_000);
    let (coll, config) = (w.collateral_state(), config_state(&w));
    w.liquidate(position, 0).expect("within the breaker band");

    let ring = w.collateral_state().ext.ring;
    assert_eq!(ring, coll.ext.ring, "the spike was not recorded");
    let t = twap_of(&ring, w.now_ms);
    let feed = coll.feed_params().expect("feed");
    let p_liq = liq_price(spike, Some(t), 0, R, &feed);
    assert!(
        p_liq < liq_price(spike, None, 0, R, &feed),
        "below band(1.1 p)"
    );
    let expected = reward_at(&coll, &config, &w, p_liq);
    assert!(expected < reward_at(&coll, &config, &w, spike));
    let a = auction_state(&w, position).expect("an auction");
    assert_eq!(a.keeper_reward, expected);
    invariants::check(&w);
}

#[test]
fn a_spot_spike_is_refused_by_the_deviation_breaker() {
    let (mut w, position) = world(RiskArgs::recommended());
    w.warm_twap(P, 3, SPACING);
    later(&mut w, 1_000);
    w.set_price(P / 10 * 13);
    let before = w.collateral_state();
    expect_revert(
        w.liquidate(position, 0),
        &fail_text(RiskError::SpotTwapDeviation),
    );
    assert_eq!(w.collateral_state(), before);
    assert_eq!(auction_state(&w, position), None);
    invariants::check(&w);
}

#[test]
fn bid_price_takes_the_confidence_edge() {
    let (mut w, position) = world(RiskArgs::recommended());
    w.warm_twap(P, 3, SPACING);
    later(&mut w, 1_000);
    w.liquidate(position, 0).expect("the keeper liquidates");
    // 50 bps, within max_conf 100.
    w.set_price_with_confidence(P, P / 200);
    let bidder = w.fund_bidder("bidder", 1_000_000);
    let a = auction_state(&w, position).expect("an auction");
    let feed = w.collateral_state().feed_params().expect("feed");
    let price = edge(liq_price(P, None, 0, R, &feed), 50);
    let expected = out_at(&w, &a, price);
    assert!(expected < out_at(&w, &a, P), "the edge pays less");
    assert_eq!(bid_out(&mut w, &bidder, position), expected);
    invariants::check(&w);
}

#[test]
fn bid_payout_capped_by_the_twap_anchor() {
    let (mut w, position) = world(RiskArgs::recommended());
    w.warm_twap(P, 3, SPACING);
    later(&mut w, 1_000);
    w.liquidate(position, 0).expect("the keeper liquidates");
    // 1.4 p = 0.7 RAY: inside the 150 % band, so only the anchor caps it.
    let spike = P / 10 * 14;
    w.set_price(spike);
    let bidder = w.fund_bidder("bidder", 1_000_000);
    let a = auction_state(&w, position).expect("an auction");
    let t = twap_of(&w.collateral_state().ext.ring, w.now_ms);
    let anchor = t
        .checked_mul(12_000)
        .and_then(|x| x.checked_div(10_000))
        .expect("fits");
    assert!(anchor < spike);
    let expected = out_at(&w, &a, anchor);
    assert!(expected < out_at(&w, &a, spike));
    assert_eq!(bid_out(&mut w, &bidder, position), expected);
    assert_eq!(
        twap_of(&w.collateral_state().ext.ring, w.now_ms),
        t,
        "the spike was not recorded"
    );
    invariants::check(&w);
}

#[test]
fn invalid_reading_is_not_recorded() {
    let (mut w, _) = world(RiskArgs::recommended());
    w.warm_twap(P, 3, SPACING);
    let before = w.collateral_state().ext.ring;
    assert_eq!(before.samples.len(), 3);
    later(&mut w, SPACING);
    // Below R / 2: out of the sanity window.
    w.set_price(R / 2 - 1);
    w.observe_feed(w.now_ms)
        .expect("a bad feed is recorded, not refused");
    assert_eq!(w.collateral_state().ext.ring, before);
    invariants::check(&w);
}

#[test]
fn observe_feed_fills_the_ring() {
    let (mut w, _) = world(RiskArgs::recommended());
    w.warm_twap(P, 9, SPACING);
    let ring = w.collateral_state().ext.ring;
    assert_eq!(ring.samples.len(), 8);
    assert!(
        ring.samples
            .windows(2)
            .all(|pair| matches!(pair, [a, b] if a.ts_ms < b.ts_ms)),
        "strictly increasing: {ring:?}"
    );
    assert_eq!(ring.newest().map(|s| s.ts_ms), Some(w.now_ms));
    invariants::check(&w);
}

#[test]
fn bid_during_twap_warm_up_is_not_refused() {
    let (mut w, position) = world(RiskArgs::legacy());
    w.liquidate(position, 0).expect("the keeper liquidates");
    let set = w.set_collateral_msg_with(tier2_collateral_args(), RiskArgs::recommended());
    let key = w.admin_key.clone();
    w.send_engine(w.admin, &[&key], &set)
        .expect("the TWAP is turned on");
    let coll = w.collateral_state();
    assert_eq!(coll.ext.risk, RiskArgs::recommended());
    assert_eq!(coll.ext.ring.samples.len(), 1, "below min_obs 3");
    let bidder = w.fund_bidder("bidder", 1_000_000);
    let a = auction_state(&w, position).expect("an auction");
    let feed = coll.feed_params().expect("feed");
    let expected = out_at(&w, &a, liq_price(P, None, 0, R, &feed));
    assert_eq!(bid_out(&mut w, &bidder, position), expected);
    invariants::check(&w);
}

#[test]
fn set_collateral_keeps_the_ring_and_the_level() {
    let (mut w, _) = world(RiskArgs::recommended());
    w.warm_twap(P, 3, SPACING);
    let before = w.collateral_state();
    // A different breaker band, so the rewrite is visible.
    let risk = RiskArgs {
        max_spot_twap_dev_bps: 1_000,
        ..RiskArgs::recommended()
    };
    let args: CollateralArgs = tier2_collateral_args();
    let set = w.set_collateral_msg_with(args, risk);
    let key = w.admin_key.clone();
    w.send_engine(w.admin, &[&key], &set)
        .expect("the collateral is reconfigured");
    let after = w.collateral_state().ext;
    assert_eq!(after.ring, before.ext.ring);
    assert_eq!(after.ring.samples.len(), 3);
    assert_eq!(after.pause_level, before.ext.pause_level);
    assert_eq!(after.risk, risk);
    invariants::check(&w);
}
