//! The bid quote on the standard live auction with arbitrary amounts, clocks and oracle prices (valid,
//! band-clamped, out of range, zero): an error or a fill that respects invariant 3 (spec §5.13).
//!
//! Amounts are scaled to the fixture auction (lot and target below) so that most inputs reach the
//! assertions; one input in 256 uses raw `u128` amounts to keep the full range reachable.
#![no_main]
use arbitrary::Arbitrary;
use auction_sdk::{
    quote::quote_bid,
    testing::{standard, standard::Fixture},
    verify::{verify_config, VerifiedConfig},
};
use libfuzzer_sys::fuzz_target;

/// `collateral_left` of the standard live auction.
const LOT: u128 = 475_000;
/// `amount_to_raise` of the standard live auction.
const TARGET: u128 = 660_000;
/// Step denominator: a step count of `STEP_ONE` is exactly the lot (target); `u16::MAX` is about 2x.
const STEP_ONE: u128 = 32_768;

#[derive(Debug, Arbitrary)]
struct Input {
    /// `coin_in_max` in 1/32768 of the target, 0 to about 2 targets.
    coin_steps: u16,
    /// `min_out` in 1/32768 of the lot, 0 to about 2 lots.
    min_steps: u16,
    /// Zero (one input in 256) switches to the raw amounts below.
    raw_selector: u8,
    raw_coin_in_max: u128,
    raw_min_out: u128,
    clock_offset_ms: u32,
    /// Oracle price: 0.5 R to 1.5 R in 1/65535 steps (the feed band is 0.9 R to 1.1 R).
    price_steps: u16,
    /// Oracle price in 1/10_000 of R, 0 to about 6.5 R; used with the raw amounts.
    raw_price_steps: u16,
}

struct Case {
    fx: Fixture,
    v: VerifiedConfig,
}

thread_local! {
    // `FixtureChain` holds `RefCell`s, so it is not `Sync`: one fixture per (single) fuzzing thread.
    static CASE: Case = {
        let fx = standard::standard();
        let v = verify_config(&fx.chain, &fx.deployment, &standard::config_id())
            .expect("fixture verifies");
        Case { fx, v }
    };
}

fuzz_target!(|i: Input| {
    CASE.with(|c| {
        let (coin_in_max, min_out) = if i.raw_selector == 0 {
            (i.raw_coin_in_max, i.raw_min_out)
        } else {
            (
                TARGET * u128::from(i.coin_steps) / STEP_ONE,
                LOT * u128::from(i.min_steps) / STEP_ONE,
            )
        };
        let now = standard::T0 + u64::from(i.clock_offset_ms);
        c.fx.chain.set_clock(now);
        let price = if i.raw_selector == 0 {
            standard::R / 10_000 * u128::from(i.raw_price_steps)
        } else {
            standard::R / 2 + standard::R * u128::from(i.price_steps) / u128::from(u16::MAX)
        };
        c.fx.chain.put_state(
            standard::ORACLE,
            standard::ORACLE_OWNER,
            &standard::oracle_state(price, now, &c.fx.host),
        );
        if let Ok(q) = quote_bid(
            &c.fx.chain,
            &c.v,
            &c.fx.live_auction,
            coin_in_max,
            min_out,
            now,
        ) {
            assert!(
                q.fill.charged <= coin_in_max,
                "charged {} > coin_in_max {}",
                q.fill.charged,
                coin_in_max
            );
            assert!(
                q.fill.collateral_out >= min_out,
                "out {} < min_out {}",
                q.fill.collateral_out,
                min_out
            );
            assert!(
                q.fill.collateral_out <= q.collateral_left,
                "out above the lot"
            );
        }
    });
});
