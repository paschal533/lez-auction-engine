//! D4 in the engine (spec §3.2.3-§3.2.4, ADR-074): while the feed is invalid, a bid at level 2 or
//! below is priced at the last-good TWAP `band(edge(T_lg))`, held at the auction's stale floor
//! `floor(stale_cap · (RAY - d) / RAY)`, with the discount frozen over the open pause. It is
//! refused with `TwapUnavailable` without `min_obs` samples and with `LastGoodTwapTooOld` once
//! the newest sample is more than `stale_bid_max_age_ms` old.
//!
//! Every refusal asserts `World::snapshot_all` is unchanged; every test ends with
//! `harness::invariants::check`.
//!
//! Numbers: the Tier 2 world under the recommended preset (R = 0.5 RAY, band `[0.45, 0.75]` RAY,
//! feed max age 600_000 ms, stale window 3_600_000 ms), 500_000 collateral and 600_000 debt, the
//! oracle at p = 0.5 RAY, three samples 300_000 ms apart, the liquidation 1_000 ms after the
//! newest, then `World::go_stale`.

use auction_engine_params::EngineError;
use engine_core::{
    bid::{BidQuote, quote},
    check::{Refusal, fail_text},
    state::{AuctionPhase, AuctionState, ConfigState},
};
use harness::{
    World, bidding::Bidder, expect_revert, invariants, tier2::tier2_collateral_args, world::RAY,
};
use lee::{AccountId, error::LeeError};
use lee_core::program::TransactionEvent;
use risk_core::{RiskArgs, RiskError, fallback_bid_price, floored_price, twap};

const COLLATERAL: u128 = 500_000;
const DEBT: u128 = 600_000;
/// The auction's target: the debt plus the 10 % penalty.
const TARGET: u128 = 660_000;
/// The oracle price the TWAP warms at, = R.
const P: u128 = RAY / 2;
/// The Tier 2 world's redemption price.
const R: u128 = RAY / 2;
/// The recommended preset's sample spacing, ms.
const SPACING: u64 = 300_000;
/// The recommended preset's stale window, ms.
const STALE_MAX_AGE: u64 = 3_600_000;
/// The chain time the position opens at, ms.
const T0: u64 = 1_000_000;
/// A partial bid's coin in.
const BID_IN: u128 = 100_000;
/// A bidder's stablecoin.
const FUNDS: u128 = 1_000_000;
/// Ten minutes, ms.
const TEN_MIN: u64 = 600_000;

type Sent = Result<Vec<TransactionEvent>, LeeError>;

/// The Tier 2 world under the recommended preset: one unsafe position, three samples at `P`,
/// liquidated 1_000 ms after the newest, and a funded bidder. The feed is still live.
fn liquidated() -> (World, AccountId, Bidder) {
    let mut w = World::new_tier2(RiskArgs::recommended());
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

fn config_state(w: &World) -> ConfigState {
    w.chain
        .raw_state(w.config)
        .and_then(|bytes| ConfigState::decode(&bytes))
        .expect("a config state")
}

/// `send` must revert with `err` and leave every actor the world knows of unchanged.
fn refused(w: &mut World, send: impl FnOnce(&mut World) -> Sent, err: impl Into<Refusal>) {
    let before = w.snapshot_all();
    expect_revert(send(w), &fail_text(err));
    assert_eq!(w.snapshot_all(), before, "a refusal changes nothing");
    invariants::check(w);
}

/// `bidder` bids `coin_in` on auction 0 of `position` and returns the collateral it received.
fn bid_out(w: &mut World, bidder: &Bidder, position: AccountId, coin_in: u128) -> u128 {
    let before = w.collateral_balance(bidder.account);
    w.bid(bidder, position, 0, (coin_in, 0))
        .expect("the bid fills");
    invariants::check(w);
    w.collateral_balance(bidder.account)
        .checked_sub(before)
        .expect("the bidder gains collateral")
}

/// The last-good TWAP of the world's ring at its time (recommended window and `min_obs`, the
/// type's feed max age).
fn t_lg(w: &World) -> u128 {
    let coll = w.collateral_state();
    let risk = RiskArgs::recommended();
    twap(
        &coll.ext.ring,
        w.now_ms,
        risk.twap_window_ms,
        risk.twap_min_obs,
        coll.max_oracle_age_ms,
    )
    .expect("a last-good twap")
    .0
}

/// The D4 price `band(edge(T_lg))` with the ring's last confidence, at the world's time.
fn p_fb(w: &World) -> u128 {
    let coll = w.collateral_state();
    let feed = coll.feed_params().expect("feed");
    fallback_bid_price(t_lg(w), coll.ext.ring.last_conf_bps, R, &feed)
}

/// What a live quote of `coin_in` on `a` at `price_ray` pays out, at the world's time and with
/// the collateral actor's current pause clock (after the stale bid: the open pause).
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
fn stale_feed_bid_uses_the_last_good_twap() {
    let (mut w, position, bidder) = liquidated();
    w.go_stale();
    let a = auction_state(&w, position);
    assert!(a.stale_cap_ray > P, "D4 on: the stale cap is stored");
    let ring = w.collateral_state().ext.ring;
    let price = p_fb(&w);
    let out = bid_out(&mut w, &bidder, position, BID_IN);
    // The observation opened the pause at the bid's claim and did not record the stale reading.
    let coll = w.collateral_state();
    assert_eq!(coll.paused_since_ms, Some(w.now_ms));
    assert_eq!(coll.ext.ring, ring);
    // Below the floor at P_liq, so the price is P_fb itself.
    assert_eq!(
        floored_price(price, a.stale_cap_ray, discount(&w, &a)),
        price
    );
    assert_eq!(out, quoted(&w, &a, price, BID_IN));
}

#[test]
fn stale_feed_bid_refused_past_max_age() {
    let (mut w, position, bidder) = liquidated();
    w.go_stale();
    let newest = w
        .collateral_state()
        .ext
        .ring
        .newest()
        .expect("a sample")
        .ts_ms;
    // At exactly the stale window the bid still fills.
    w.set_now_ms(newest.checked_add(STALE_MAX_AGE).expect("time fits"));
    bid_out(&mut w, &bidder, position, BID_IN);
    later(&mut w, 1);
    refused(
        &mut w,
        |w| w.bid(&bidder, position, 0, (BID_IN, 0)),
        RiskError::LastGoodTwapTooOld,
    );
}

#[test]
fn stale_feed_bid_refused_without_min_obs() {
    let mut w = World::new_tier2(RiskArgs::legacy());
    w.set_now_ms(T0);
    let position = w.open_unsafe_position(COLLATERAL, DEBT);
    w.set_price(P);
    w.liquidate(position, 0).expect("the keeper liquidates");
    let set = w.set_collateral_msg_with(tier2_collateral_args(), RiskArgs::recommended());
    let key = w.admin_key.clone();
    w.send_engine(w.admin, &[&key], &set)
        .expect("the TWAP and D4 are turned on");
    assert_eq!(
        w.collateral_state().ext.ring.samples.len(),
        1,
        "below min_obs 3"
    );
    let bidder = w.fund_bidder("bidder", FUNDS);
    invariants::check(&w);
    w.go_stale();
    refused(
        &mut w,
        |w| w.bid(&bidder, position, 0, (BID_IN, 0)),
        RiskError::TwapUnavailable,
    );
}

/// An auction opened while D4 was off stored no stale cap (0): once D4 is turned on, a stale-feed
/// bid on it is refused with the stale-feed code, as if D4 were still off, and moves nothing.
#[test]
fn stale_feed_bid_refused_on_an_auction_opened_without_d4() {
    let mut w = World::new_tier2(RiskArgs::legacy());
    w.set_now_ms(T0);
    let position = w.open_unsafe_position(COLLATERAL, DEBT);
    w.set_price(P);
    w.liquidate(position, 0).expect("the keeper liquidates");
    assert_eq!(auction_state(&w, position).stale_cap_ray, 0);
    let set = w.set_collateral_msg_with(tier2_collateral_args(), RiskArgs::recommended());
    let key = w.admin_key.clone();
    w.send_engine(w.admin, &[&key], &set)
        .expect("the TWAP and D4 are turned on");
    w.warm_twap(P, 3, SPACING);
    let bidder = w.fund_bidder("bidder", FUNDS);
    invariants::check(&w);
    w.go_stale();
    // D4 itself would price this bid: the last-good TWAP exists and is fresh enough.
    assert!(p_fb(&w) > 0);
    refused(
        &mut w,
        |w| w.bid(&bidder, position, 0, (BID_IN, 0)),
        EngineError::PriceFeedStale,
    );
}

#[test]
fn stale_feed_bid_held_at_the_floor() {
    let (mut w, position, bidder) = liquidated();
    // 1.4 p = 0.7 RAY three times: the TWAP rises above P_liq.
    w.warm_twap(P / 10 * 14, 3, SPACING);
    w.go_stale();
    let a = auction_state(&w, position);
    let price = p_fb(&w);
    let out = bid_out(&mut w, &bidder, position, BID_IN);
    let floor = floored_price(price, a.stale_cap_ray, discount(&w, &a));
    assert!(floor < price, "the floor binds: {floor} < {price}");
    assert_eq!(out, quoted(&w, &a, floor, BID_IN));
    assert!(out < quoted(&w, &a, price, BID_IN));
}

#[test]
fn discount_stays_frozen_across_fallback_bids() {
    let (mut w, position, bidder) = liquidated();
    w.go_stale();
    // Past `newest + max_age`, so the TWAP anchor is frozen for both bids.
    let slack = w.collateral_state().ctx.slack_ms;
    later(&mut w, slack);
    // The floor does not bind, so the equal payouts below come from the frozen discount, not
    // from a floor that pays `coin · cap / RAY` whatever the discount.
    let a = auction_state(&w, position);
    let price = p_fb(&w);
    assert_eq!(
        floored_price(price, a.stale_cap_ray, discount(&w, &a)),
        price
    );
    let first = bid_out(&mut w, &bidder, position, BID_IN);
    let onset = w.collateral_state().paused_since_ms;
    assert!(onset.is_some(), "the pause is open");
    later(&mut w, TEN_MIN);
    let second = bid_out(&mut w, &bidder, position, BID_IN);
    assert_eq!(w.collateral_state().paused_since_ms, onset);
    assert_eq!(first, second, "the same collateral per coin");
}

#[test]
fn fallback_final_bid_settles_and_releases() {
    let (mut w, position, bidder) = liquidated();
    assert_eq!(w.collateral_state().type_reserved, TARGET);
    assert_eq!(config_state(&w).global_reserved, TARGET);
    w.go_stale();
    let a = auction_state(&w, position);
    let price = p_fb(&w);
    let out = bid_out(&mut w, &bidder, position, TARGET);
    assert_eq!(out, quoted(&w, &a, price, TARGET));
    let settled = auction_state(&w, position);
    assert_eq!(settled.phase, AuctionPhase::Settled);
    assert_eq!(
        (settled.debt_raised, settled.penalty_raised),
        (DEBT, TARGET - DEBT)
    );
    assert_eq!(w.collateral_state().type_reserved, 0);
    assert_eq!(config_state(&w).global_reserved, 0);
    assert_eq!(w.accounting_state().total_debt_burned, DEBT);
}
