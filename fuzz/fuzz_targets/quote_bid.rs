//! The bid quote on the standard live auction with arbitrary amounts, clocks and oracle prices (valid,
//! band-clamped, out of range, zero): an error or a fill that respects invariant 3 (spec §5.13).
#![no_main]
use arbitrary::Arbitrary;
use auction_sdk::{quote::quote_bid, testing::standard, verify::verify_config};
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct Input {
    coin_in_max: u128,
    min_out: u128,
    clock_offset_ms: u32,
    /// Oracle price in 1/10_000 of R: 0 to about 6.5 R.
    price_steps: u16,
}

fuzz_target!(|i: Input| {
    let fx = standard::standard();
    let v =
        verify_config(&fx.chain, &fx.deployment, &standard::config_id()).expect("fixture verifies");
    let now = standard::T0 + u64::from(i.clock_offset_ms);
    fx.chain.set_clock(now);
    let price = standard::R / 10_000 * u128::from(i.price_steps);
    fx.chain.put_state(
        standard::ORACLE,
        standard::ORACLE_OWNER,
        &standard::oracle_state(price, now, &fx.host),
    );
    if let Ok(q) = quote_bid(
        &fx.chain,
        &v,
        &fx.live_auction,
        i.coin_in_max,
        i.min_out,
        now,
    ) {
        assert!(
            q.fill.charged <= i.coin_in_max,
            "charged {} > coin_in_max {}",
            q.fill.charged,
            i.coin_in_max
        );
        assert!(
            q.fill.collateral_out >= i.min_out,
            "out {} < min_out {}",
            q.fill.collateral_out,
            i.min_out
        );
        assert!(
            q.fill.collateral_out <= q.collateral_left,
            "out above the lot"
        );
    }
});
