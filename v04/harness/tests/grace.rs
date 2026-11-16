//! Pinned readings in the engine (spec §3.3 E3, ADR-075, ruling R10-9): a reading at or beyond
//! the feed's own answer bound is invalid. It refuses `Liquidate` with `PriceFeedPinned` (6076),
//! pauses the type like any invalid feed, is not offered to the TWAP ring, and gives a bid the D4
//! fallback (or, with D4 off, the pinned code).
//!
//! Numbers: the Tier 2 world (R = 0.5 RAY, RAY-scale oracle so raw = RAY price, sanity window
//! `[0.25, 1]` RAY) under the recommended preset with pin bounds `(RAY / 4, RAY)`, the edges of
//! the sanity window, and grace off. The live price `P = 0.5 RAY` sits strictly between them.
//!
//! The grace period (spec §3.3 E3, ADR-075, rulings R10-11 and R10-12): after an outage of at
//! least `grace_min_gap_ms` the type refuses `Liquidate` with `GracePeriod` (6072) until
//! `grace_until_ms`, while bids run. Grace tests use the recommended preset (grace 1_800_000 ms,
//! minimum gap 600_000 ms) with the pin bounds off.

use engine_core::{
    bid::{BidQuote, quote},
    check::fail_text,
    state::AuctionState,
};
use harness::{
    World, bidding::Bidder, expect_revert, invariants, tier2::tier2_collateral_args, world::RAY,
};
use lee::AccountId;
use risk_core::{RiskArgs, RiskError, fallback_bid_price, floored_price, twap};

const COLLATERAL: u128 = 500_000;
const DEBT: u128 = 600_000;
/// The oracle price the TWAP warms at, = R.
const P: u128 = RAY / 2;
/// The Tier 2 world's redemption price.
const R: u128 = RAY / 2;
/// The low pin bound, raw (= RAY price).
const PIN_LO: u128 = RAY / 4;
/// The high pin bound, raw (= RAY price).
const PIN_HI: u128 = RAY;
/// The recommended preset's sample spacing, ms.
const SPACING: u64 = 300_000;
/// The chain time the position opens at, ms.
const T0: u64 = 1_000_000;
/// A partial bid's coin in.
const BID_IN: u128 = 100_000;
/// A bidder's stablecoin.
const FUNDS: u128 = 1_000_000;

/// The recommended preset with the pin bounds `(PIN_LO, PIN_HI)` and grace off.
fn pinned_risk() -> RiskArgs {
    RiskArgs {
        feed_min_raw: PIN_LO,
        feed_max_raw: PIN_HI,
        grace_ms: 0,
        grace_min_gap_ms: 0,
        ..RiskArgs::recommended()
    }
}

/// Advances the world's time by `ms`.
fn later(w: &mut World, ms: u64) {
    let now = w.now_ms.checked_add(ms).expect("time fits");
    w.set_now_ms(now);
}

fn auction_state(w: &World, position: AccountId) -> AuctionState {
    w.chain
        .raw_state(w.auction(position, 0))
        .and_then(|bytes| AuctionState::decode(&bytes))
        .expect("an auction state")
}

/// The world under `risk` with one unsafe position, three samples at `P`, liquidated 1_000 ms
/// after the newest, and a funded bidder. The feed is still live.
fn liquidated(risk: RiskArgs) -> (World, AccountId, Bidder) {
    let mut w = World::new_tier2(risk);
    w.set_now_ms(T0);
    let position = w.open_unsafe_position(COLLATERAL, DEBT);
    w.set_price(P);
    w.warm_twap(P, 3, SPACING);
    later(&mut w, 1_000);
    w.liquidate(position, 0).expect("the keeper liquidates");
    let bidder = w.fund_bidder("bidder", FUNDS);
    invariants::check(&w);
    (w, position, bidder)
}

/// The D4 price `band(edge(T_lg))` with the ring's last confidence, at the world's time.
fn p_fb(w: &World) -> u128 {
    let coll = w.collateral_state();
    let risk = coll.ext.risk;
    let (t, _) = twap(
        &coll.ext.ring,
        w.now_ms,
        risk.twap_window_ms,
        risk.twap_min_obs,
        coll.max_oracle_age_ms,
    )
    .expect("a last-good twap");
    let feed = coll.feed_params().expect("feed");
    fallback_bid_price(t, coll.ext.ring.last_conf_bps, R, &feed)
}

/// What a quote of `coin_in` on `a` at `price_ray` pays out, at the world's time and with the
/// collateral actor's current pause clock.
fn quoted(w: &World, a: &AuctionState, price_ray: u128, coin_in: u128) -> u128 {
    let coll = w.collateral_state();
    let params = coll.collateral_params().expect("params");
    let q = BidQuote {
        max_debt_in: coin_in,
        min_collateral_out: 0,
        price_ray,
        now_ms: w.now_ms,
        paused_total_ms: coll.paused_total_ms,
        min_bid: params.min_bid,
        chost: params.chost().expect("chost"),
        paused_since_ms: coll.paused_since_ms,
        fallback: false,
    };
    quote(a, &q).expect("the bid quotes").fill.collateral_out
}

/// The auction's discount at the world's time under the collateral actor's pause clock.
fn discount(w: &World, a: &AuctionState) -> u128 {
    let clock = w.collateral_state().pause_clock();
    let t_eff = clock.effective_elapsed(a.start_ms, a.type_paused_total_at_start, w.now_ms);
    a.to_core().expect("core").schedule.discount_ray(t_eff)
}

#[test]
fn pinned_low_and_high_readings_pause_the_type() {
    let mut w = World::new_tier2(pinned_risk());
    w.set_now_ms(T0);
    let position = w.open_unsafe_position(COLLATERAL, DEBT);
    for bound in [PIN_LO, PIN_HI] {
        later(&mut w, 1_000);
        w.set_price(bound);
        let before = w.snapshot_all();
        expect_revert(
            w.liquidate(position, 0),
            &fail_text(RiskError::PriceFeedPinned),
        );
        assert_eq!(w.snapshot_all(), before, "a refusal changes nothing");
        w.observe_feed(w.now_ms)
            .expect("a pinned feed is recorded, not refused");
        assert!(
            w.collateral_state().paused_since_ms.is_some(),
            "{bound}: the pinned reading pauses the type"
        );
        // A live reading closes the pause before the next bound.
        later(&mut w, 1_000);
        w.set_price(P);
        w.observe_feed(w.now_ms).expect("the feed is observed");
        assert_eq!(w.collateral_state().paused_since_ms, None);
        invariants::check(&w);
    }
}

#[test]
fn pinned_reading_is_not_recorded() {
    let mut w = World::new_tier2(pinned_risk());
    w.set_now_ms(T0);
    w.warm_twap(P, 3, SPACING);
    let before = w.collateral_state().ext.ring;
    assert_eq!(before.samples.len(), 3);
    later(&mut w, SPACING);
    w.set_price(PIN_LO);
    w.observe_feed(w.now_ms)
        .expect("a pinned feed is recorded, not refused");
    assert_eq!(w.collateral_state().ext.ring, before);
    invariants::check(&w);
}

#[test]
fn a_bid_on_a_pinned_feed_uses_d4_or_the_pinned_code() {
    // D4 on: the bid fills at the last-good TWAP, under the auction's stale floor.
    let (mut w, position, bidder) = liquidated(pinned_risk());
    later(&mut w, 1_000);
    w.set_price(PIN_HI);
    let a = auction_state(&w, position);
    assert!(a.stale_cap_ray > P, "D4 on: the stale cap is stored");
    let ring = w.collateral_state().ext.ring;
    let price = p_fb(&w);
    let before = w.collateral_balance(bidder.account);
    w.bid(&bidder, position, 0, (BID_IN, 0))
        .expect("the fallback bid fills");
    invariants::check(&w);
    let out = w
        .collateral_balance(bidder.account)
        .checked_sub(before)
        .expect("the bidder gains collateral");
    let coll = w.collateral_state();
    assert!(coll.paused_since_ms.is_some(), "the pinned feed pauses");
    assert_eq!(coll.ext.ring, ring, "the pinned reading is not recorded");
    let floor = floored_price(price, a.stale_cap_ray, discount(&w, &a));
    assert_eq!(out, quoted(&w, &a, floor, BID_IN));

    // D4 off (no stale window, TWAP still on): the bid is refused with the pinned code.
    let (mut w, position, bidder) = liquidated(RiskArgs {
        stale_bid_max_age_ms: 0,
        ..pinned_risk()
    });
    later(&mut w, 1_000);
    w.set_price(PIN_HI);
    let before = w.snapshot_all();
    expect_revert(
        w.bid(&bidder, position, 0, (BID_IN, 0)),
        &fail_text(RiskError::PriceFeedPinned),
    );
    assert_eq!(w.snapshot_all(), before, "a refusal changes nothing");
    invariants::check(&w);
}

/// The Tier 2 world under `risk`, observed without an outage up to `T0` (`World::new_tier2_live`,
/// `World::advance_observed`): no grace is open.
fn live_at_t0(risk: RiskArgs) -> World {
    let mut w = World::new_tier2_live(risk);
    w.advance_observed(T0, P);
    assert_eq!(w.collateral_state().ext.grace_until_ms, 0, "no grace yet");
    w
}

/// The world under the recommended preset with two unsafe positions and three samples at `P`;
/// 1_000 ms after the newest sample the first is liquidated and a bidder funded. Returns the
/// world, the auctioned position, the open one and the bidder. The feed is still live and no
/// grace is open.
fn auction_and_open() -> (World, AccountId, AccountId, Bidder) {
    let mut w = live_at_t0(RiskArgs::recommended());
    let auctioned = w.open_unsafe_position(COLLATERAL, DEBT);
    let open = w.open_unsafe_position(COLLATERAL, DEBT);
    w.set_price(P);
    w.warm_twap(P, 3, SPACING);
    later(&mut w, 1_000);
    w.liquidate(auctioned, 0).expect("the keeper liquidates");
    let bidder = w.fund_bidder("bidder", FUNDS);
    assert_eq!(w.collateral_state().ext.grace_until_ms, 0, "no grace yet");
    invariants::check(&w);
    (w, auctioned, open, bidder)
}

#[test]
fn grace_blocks_liquidate_not_bid() {
    let (mut w, auctioned, open, bidder) = auction_and_open();
    w.start_grace();
    let coll = w.collateral_state();
    assert!(w.now_ms < coll.ext.grace_until_ms, "inside grace");
    assert_eq!(
        coll.ext.feed_invalid_since_ms, None,
        "the feed is live again"
    );
    expect_revert(w.liquidate(open, 0), &fail_text(RiskError::GracePeriod));
    let before = w.collateral_balance(bidder.account);
    w.bid(&bidder, auctioned, 0, (BID_IN, 0))
        .expect("a bid during grace fills");
    assert!(w.collateral_balance(bidder.account) > before);
    invariants::check(&w);
}

#[test]
fn grace_refusal_moves_nothing() {
    let (mut w, _, open, _) = auction_and_open();
    w.start_grace();
    let before = w.snapshot_all();
    expect_revert(w.liquidate(open, 0), &fail_text(RiskError::GracePeriod));
    assert_eq!(w.snapshot_all(), before, "a refusal changes nothing");
    invariants::check(&w);
}

#[test]
fn grace_expires_at_its_end() {
    let (mut w, _, open, _) = auction_and_open();
    w.start_grace();
    let until = w.collateral_state().ext.grace_until_ms;
    // Regular samples through the grace period keep the TWAP warm and do not extend it.
    w.warm_twap(P, 5, SPACING);
    assert_eq!(w.collateral_state().ext.grace_until_ms, until);
    w.set_now_ms(until.checked_sub(1).expect("grace is open"));
    w.set_price(P);
    expect_revert(w.liquidate(open, 0), &fail_text(RiskError::GracePeriod));
    w.set_now_ms(until);
    w.set_price(P);
    w.liquidate(open, 0)
        .expect("at grace_until_ms the keeper liquidates");
    invariants::check(&w);
}

#[test]
fn a_short_outage_starts_no_grace() {
    let mut w = live_at_t0(RiskArgs::recommended());
    let open = w.open_unsafe_position(COLLATERAL, DEBT);
    w.set_price(P);
    w.warm_twap(P, 3, SPACING);
    w.go_stale();
    w.observe_feed(w.now_ms)
        .expect("the stale feed is observed");
    let since = w
        .collateral_state()
        .ext
        .feed_invalid_since_ms
        .expect("the outage is recorded");
    let min_gap = RiskArgs::recommended().grace_min_gap_ms;
    // Back 1 ms before the minimum gap.
    let back = since
        .checked_add(min_gap)
        .and_then(|t| t.checked_sub(1))
        .expect("time fits");
    assert!(back >= w.now_ms);
    w.set_now_ms(back);
    w.set_price(P);
    w.observe_feed(back).expect("the fresh feed is observed");
    let coll = w.collateral_state();
    assert_eq!(coll.ext.feed_invalid_since_ms, None);
    assert_eq!(coll.ext.grace_until_ms, 0, "no grace after a short outage");
    // A grace period would outlast these samples; the liquidation runs.
    w.warm_twap(P, 3, SPACING);
    w.liquidate(open, 0)
        .expect("the keeper liquidates after a short outage");
    invariants::check(&w);
}

/// Ruling R10-12: grace reads the state after the observation, so a `Liquidate` whose own
/// observation ends the outage is refused with `GracePeriod`. The guest fails the whole
/// transaction on a refusal, so it persists nothing: the stored state still has no grace and
/// the outage recorded by the earlier `ObserveFeed`.
#[test]
fn a_liquidate_that_ends_the_outage_is_refused() {
    let mut w = live_at_t0(RiskArgs::recommended());
    let open = w.open_unsafe_position(COLLATERAL, DEBT);
    w.set_price(P);
    w.warm_twap(P, 3, SPACING);
    w.go_stale();
    w.observe_feed(w.now_ms)
        .expect("the stale feed is observed");
    let since = w
        .collateral_state()
        .ext
        .feed_invalid_since_ms
        .expect("the outage is recorded");
    let back = since
        .checked_add(RiskArgs::recommended().grace_min_gap_ms)
        .expect("time fits");
    w.set_now_ms(back);
    w.set_price(P);
    expect_revert(w.liquidate(open, 0), &fail_text(RiskError::GracePeriod));
    let coll = w.collateral_state();
    assert_eq!(
        coll.ext.grace_until_ms, 0,
        "the refused grace is not stored"
    );
    assert_eq!(coll.ext.feed_invalid_since_ms, Some(since));
    invariants::check(&w);
}

#[test]
fn turning_grace_on_does_not_start_one() {
    let mut w = live_at_t0(RiskArgs::legacy());
    w.warm_twap(P, 3, SPACING);
    assert!(w.collateral_state().ext.last_good_ts_ms.is_some());
    later(&mut w, SPACING);
    w.set_price(P);
    let set = w.set_collateral_msg_with(tier2_collateral_args(), RiskArgs::recommended());
    let key = w.admin_key.clone();
    w.send_engine(w.admin, &[&key], &set)
        .expect("grace is turned on");
    assert_eq!(w.collateral_state().ext.risk, RiskArgs::recommended());
    assert_eq!(w.collateral_state().ext.grace_until_ms, 0);
    w.warm_twap(P, 1, SPACING);
    assert_eq!(
        w.collateral_state().ext.grace_until_ms,
        0,
        "turning grace on starts none"
    );
    invariants::check(&w);
}

#[test]
fn set_collateral_keeps_the_grace_state() {
    let (mut w, _, _, _) = auction_and_open();
    w.start_grace();
    let until = w.collateral_state().ext.grace_until_ms;
    let set = w.set_collateral_msg_with(tier2_collateral_args(), RiskArgs::recommended());
    let key = w.admin_key.clone();
    w.send_engine(w.admin, &[&key], &set)
        .expect("the collateral is reconfigured");
    assert_eq!(w.collateral_state().ext.grace_until_ms, until);
    invariants::check(&w);
}
