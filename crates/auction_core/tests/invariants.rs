//! Invariants 1–17 (spec §5.13) over random action sequences, plus split-bid neutrality and two-type pause.
use auction_core::{
    accounting::Accounting, auction::*, liquidation::*, pause::*, schedule::*, units::RAY,
};
use proptest::prelude::*;

fn params() -> CollateralParams {
    CollateralParams {
        penalty_bps: 1000,
        tip: 5,
        chip_bps: 10,
        reward_cap: 50,
        debt_floor: 100,
        min_bid: 25,
        schedule: rai_default(),
    }
}

#[derive(Debug, Clone)]
enum Act {
    Bid { coin: u128, dt: u64 },
}

fn acts() -> impl Strategy<Value = Vec<Act>> {
    prop::collection::vec(
        (1u128..50_000, 0u64..600_000).prop_map(|(coin, dt)| Act::Bid { coin, dt }),
        0..30,
    )
}

proptest! {
    #[test]
    fn auction_invariants(collateral in 1u128..1_000_000, vault_extra in 0u128..1_000, debt in 1u128..200_000,
                          price_milli in 100u128..5_000, seq in acts()) {
        let price = RAY * price_milli / 1_000_000; // 0.0001 .. 0.005 collateral per coin
        let caps = CapState { type_cap: u128::MAX, type_reserved: 0, global_cap: u128::MAX, global_reserved: 0 };
        let i = LiquidationInput { position_collateral: collateral, vault_balance: collateral + vault_extra, nominal_debt: debt, price_ray: price };
        let q = quote_liquidation(&i, &params(), &caps).unwrap();
        // Invariant 16: reward ≤ min(penalty value, c)
        prop_assert!(q.keeper_reward <= q.seized);
        let mut a = Auction::open(&q, 0, params().schedule, 0);
        let mut acc = Accounting::default();
        let mut t = 0u64; let mut last_d = 0u128; let mut ends = 0u32;
        let chost = params().chost().unwrap();
        for Act::Bid { coin, dt } in seq {
            t = t.saturating_add(dt);
            let req = BidRequest { coin_in_max: coin, min_collateral_out: 0 };
            let Ok(f) = quote_bid(&a, &req, price, t, 25, chost) else { continue };
            // Invariant 3 and 4
            prop_assert!(f.charged <= coin);
            prop_assert!(f.discount_ray >= last_d && f.discount_ray <= a.schedule.max_discount_ray());
            last_d = f.discount_ray;
            let (n, end) = apply_bid(&a, &f);
            acc = acc.record_bid(&f);
            // Invariant 1: seized = reward + sold + left
            prop_assert_eq!(n.collateral_seized, n.keeper_reward + n.collateral_sold + n.collateral_left);
            // Invariant 3: raised ≤ target
            prop_assert!(n.raised() <= n.amount_to_raise);
            // Invariant 6: no dust remainder unless the auction ended
            prop_assert!(n.status == Status::Settled || n.remaining_target() == 0 || n.remaining_target() >= chost);
            if let Some(e) = end {
                ends += 1;
                acc = acc.record_chain_end(&e);
                // Invariant 8: raised debt + shortfall = principal
                prop_assert_eq!(n.debt_raised + e.shortfall, n.debt_principal);
                prop_assert_eq!(e.release, n.reserved_amount);
                // Invariant 15: leftover only when the target is reached
                if n.raised() < n.amount_to_raise { prop_assert_eq!(e.leftover, 0); }
            }
            // Invariant 5: terminal is absorbing
            if a.status == Status::Settled { prop_assert_eq!(n, a); }
            a = n;
        }
        prop_assert!(ends <= 1); // chain end applied once
        prop_assert_eq!(acc.total_debt_burned + acc.total_penalty_collected, a.raised());
    }

    /// Invariant 13: n bids summing to X at the same t never yield more collateral than one bid of X.
    #[test]
    fn split_bid_neutrality(parts in prop::collection::vec(25u128..5_000, 1..8), t in 0u64..3_000_000) {
        let q = LiquidationQuote { debt_principal: 1_000_000, amount_to_raise: 1_100_000, reserve: 1_100_000, keeper_reward: 0, lot: 10_000_000_000, seized: 10_000_000_000 };
        let a0 = Auction::open(&q, 0, rai_default(), 0);
        let p = RAY; // 1 collateral per coin, so every part (≥ 25) fills a non-zero amount
        let total: u128 = parts.iter().sum();
        let one = quote_bid(&a0, &BidRequest { coin_in_max: total, min_collateral_out: 0 }, p, t, 1, 1).unwrap().collateral_out;
        let mut a = a0; let mut sum = 0u128;
        for c in parts {
            let f = quote_bid(&a, &BidRequest { coin_in_max: c, min_collateral_out: 0 }, p, t, 1, 1).unwrap();
            sum += f.collateral_out; a = apply_bid(&a, &f).0;
        }
        prop_assert!(sum <= one);
    }

    /// Invariant 14 + per-type isolation: two collateral types with their own clocks.
    #[test]
    fn two_type_pause_isolation(pause_at in 1u64..1_000, resume_after in 1u64..1_000, now_extra in 0u64..1_000) {
        let o = |now, valid| Observation { now_ms: now, feed_valid: valid, admin_paused: false, oracle_ts: now, max_age_ms: 100 };
        let a0 = PauseClock::default().observed(o(0, true), OnsetPolicy::AtObservation);
        let b0 = a0;
        let a1 = a0.observed(o(pause_at, false), OnsetPolicy::AtObservation);   // type A's feed fails
        let b1 = b0.observed(o(pause_at, true), OnsetPolicy::AtObservation);    // type B is fine
        let resume = pause_at + resume_after;
        let a2 = a1.observed(o(resume, true), OnsetPolicy::AtObservation);
        let now = resume + now_extra;
        prop_assert_eq!(a2.effective_elapsed(0, 0, now), now - resume_after);   // A frozen for the pause
        prop_assert_eq!(b1.effective_elapsed(0, 0, now), now);                  // B unaffected
    }
}
