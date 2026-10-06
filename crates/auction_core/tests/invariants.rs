//! Core-level invariants 1, 3, 4, 5, 6, 8, 13, 14, 15, 16 (spec 5.13); 2, 7, 9-12, 17 are Plan 2 integration tests (plan Task 11 coverage split).
use auction_core::{
    accounting::Accounting, auction::*, error::AuctionError, liquidation::*, math::mul_div_floor,
    pause::*, schedule::*, units::RAY,
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

fn no_caps() -> CapState {
    CapState {
        type_cap: u128::MAX,
        type_reserved: 0,
        global_cap: u128::MAX,
        global_reserved: 0,
    }
}

/// Settles a lot-0 auction through `settle_empty` and checks the chain-end bookkeeping.
fn check_settle_empty(q: &LiquidationQuote) -> Result<(), TestCaseError> {
    let a = Auction::open(q, 0, rai_default(), 0);
    let (s, e) = settle_empty(&a).unwrap();
    prop_assert_eq!(s.status, Status::Settled);
    prop_assert_eq!(e.shortfall, q.debt_principal);
    prop_assert_eq!(e.release, q.reserve);
    prop_assert_eq!(e.leftover, 0);
    let acc = Accounting::default().record_settled(&s, &e);
    prop_assert_eq!(acc.unbacked_debt, q.debt_principal);
    prop_assert_eq!(acc.total_shortfall_booked, q.debt_principal);
    prop_assert_eq!(settle_empty(&s), Err(AuctionError::NotAuctioning));
    let req = BidRequest {
        coin_in_max: 1_000,
        min_collateral_out: 0,
    };
    prop_assert_eq!(
        quote_bid(&s, &req, RAY, 0, 1, 1),
        Err(AuctionError::NotAuctioning)
    );
    Ok(())
}

proptest! {
    #[test]
    fn auction_invariants(collateral in 1u128..1_000_000, vault_extra in 0u128..1_000, debt in 1u128..200_000,
                          price_micro in 100u128..2_000_000, seq in acts()) {
        let price = RAY * price_micro / 1_000_000; // 0.0001 .. 2 collateral per coin
        let caps = no_caps();
        let i = LiquidationInput { position_collateral: collateral, vault_balance: collateral + vault_extra, nominal_debt: debt, price_ray: price };
        let q = quote_liquidation(&i, &params(), &caps).unwrap();
        // Invariant 16: reward ≤ min(penalty value, c)
        prop_assert!(q.keeper_reward <= q.seized);
        let penalty_coin = mul_div_floor(debt, params().penalty_bps as u128, 10_000).unwrap();
        let penalty_value = mul_div_floor(penalty_coin, price, RAY).unwrap();
        prop_assert!(q.keeper_reward <= penalty_value);
        // Invariant 1 at open
        prop_assert_eq!(q.seized, q.keeper_reward + q.lot);
        if q.lot == 0 {
            check_settle_empty(&q)?;
            return Ok(());
        }
        let mut a = Auction::open(&q, 0, params().schedule, 0);
        let mut acc = Accounting::default();
        let mut per_bid = Accounting::default(); // shadow: the old per-bid fold, cross-checked at chain end
        let mut t = 0u64; let mut last_d = 0u128; let mut ends = 0u32;
        let (mut sum_debt, mut sum_pen) = (0u128, 0u128);
        let chost = params().chost().unwrap();
        for Act::Bid { coin, dt } in seq {
            t = t.saturating_add(dt);
            let req = BidRequest { coin_in_max: coin, min_collateral_out: 0 };
            let Ok(f) = quote_bid(&a, &req, price, t, 25, chost) else { continue };
            // Invariant 3 and 4
            prop_assert!(f.charged <= coin);
            prop_assert!(f.discount_ray >= last_d && f.discount_ray <= a.schedule.max_discount_ray());
            last_d = f.discount_ray;
            prop_assert_eq!(f.debt_part + f.penalty_part, f.charged);
            sum_debt += f.debt_part;
            sum_pen += f.penalty_part;
            let (n, end) = apply_bid(&a, &f);
            per_bid = per_bid.record_bid(&f);
            // Invariant 8 (running): independent sums match the auction's counters
            prop_assert_eq!(n.debt_raised, sum_debt);
            prop_assert_eq!(n.penalty_raised, sum_pen);
            prop_assert!(sum_debt <= n.debt_principal);
            // Invariant 1: seized = reward + sold + left
            prop_assert_eq!(n.collateral_seized, n.keeper_reward + n.collateral_sold + n.collateral_left);
            // Invariant 3: raised ≤ target
            prop_assert!(n.raised() <= n.amount_to_raise);
            // Invariant 6: no dust remainder unless the auction ended
            prop_assert!(n.status == Status::Settled || n.remaining_target() == 0 || n.remaining_target() >= chost);
            if let Some(e) = end {
                ends += 1;
                acc = acc.record_settled(&n, &e);
                // ADR-023: the chain-end fold equals the per-bid fold plus the chain end
                prop_assert_eq!(acc, per_bid.record_chain_end(&e));
                // Invariant 8: raised debt + shortfall = principal
                prop_assert_eq!(n.debt_raised + e.shortfall, n.debt_principal);
                prop_assert_eq!(e.shortfall, n.debt_principal - sum_debt);
                prop_assert_eq!(e.release, n.reserved_amount);
                // Invariant 15: leftover only when the target is reached
                if n.raised() < n.amount_to_raise { prop_assert_eq!(e.leftover, 0); }
                // Invariant 15 converse: target reached => the whole remaining lot is claimable, exactly once
                if n.raised() == n.amount_to_raise {
                    prop_assert_eq!(e.leftover, n.collateral_left);
                    if e.leftover > 0 {
                        let (c1, paid) = claim_leftover(&n).unwrap();
                        prop_assert_eq!(paid, e.leftover);
                        prop_assert_eq!(claim_leftover(&c1), Err(AuctionError::NothingToClaim));
                    } else {
                        prop_assert_eq!(claim_leftover(&n), Err(AuctionError::NothingToClaim));
                    }
                } else {
                    prop_assert_eq!(claim_leftover(&n), Err(AuctionError::NothingToClaim));
                }
                // Single auction: unbacked debt equals the booked shortfall equals this end's shortfall
                prop_assert_eq!(acc.unbacked_debt, e.shortfall);
                prop_assert_eq!(acc.total_shortfall_booked, e.shortfall);
                // Invariant 5, explicit: a settled auction rejects further bids
                let again = BidRequest { coin_in_max: u128::MAX, min_collateral_out: 0 };
                prop_assert_eq!(quote_bid(&n, &again, price, t, 25, chost), Err(AuctionError::NotAuctioning));
            }
            // Invariant 5: terminal is absorbing
            if a.status == Status::Settled { prop_assert_eq!(n, a); }
            a = n;
        }
        prop_assert!(ends <= 1); // chain end applied once
        // ADR-023: the counters lag until chain end, then equal the auction's cumulative raise.
        if a.status == Status::Settled {
            prop_assert_eq!(acc.total_debt_burned + acc.total_penalty_collected, a.raised());
        } else {
            prop_assert_eq!(acc.total_debt_burned + acc.total_penalty_collected, 0);
        }
    }

    /// Reward consumes the whole seizure (lot 0); settle_empty is the only way out.
    #[test]
    fn reward_equals_seized_settles_empty(collateral in 1u128..50, debt in 1_000u128..200_000,
                                          price_milli in 500u128..2_000) {
        let p = CollateralParams { tip: 1_000_000, reward_cap: u128::MAX, chip_bps: 1000, ..params() };
        let price = RAY * price_milli / 1_000;
        let i = LiquidationInput { position_collateral: collateral, vault_balance: collateral, nominal_debt: debt, price_ray: price };
        let q = quote_liquidation(&i, &p, &no_caps()).unwrap();
        prop_assert_eq!(q.keeper_reward, q.seized);
        prop_assert_eq!(q.lot, 0);
        check_settle_empty(&q)?;
    }

    /// The final-fill exception. Debt < debt_floor (no keeper reward) and low prices make the
    /// target-reaching fill floor to 0; the bid must then pay exactly 1 unit for the whole remaining target.
    #[test]
    fn final_fill_exception_pays_one_unit(collateral in 1u128..1_000, debt in 1u128..100,
                                          price_micro in 100u128..5_000, t in 0u64..3_000_000) {
        let price = RAY * price_micro / 1_000_000;
        let i = LiquidationInput { position_collateral: collateral, vault_balance: collateral, nominal_debt: debt, price_ray: price };
        let q = quote_liquidation(&i, &params(), &no_caps()).unwrap();
        prop_assert_eq!(q.keeper_reward, 0);
        let a = Auction::open(&q, 0, rai_default(), 0);
        let remaining = a.remaining_target();
        let denom = RAY - a.schedule.discount_ray(t);
        // precondition: the plain floor would be 0 (so the exception is the branch under test)
        prop_assert_eq!(mul_div_floor(remaining, price, denom).unwrap(), 0);
        let req = BidRequest { coin_in_max: u128::MAX, min_collateral_out: 0 };
        let f = quote_bid(&a, &req, price, t, 25, params().chost().unwrap()).unwrap();
        prop_assert_eq!(f.collateral_out, 1);
        prop_assert_eq!(f.charged, remaining);
        prop_assert!(f.ends_auction);
        let (n, end) = apply_bid(&a, &f);
        prop_assert_eq!(n.collateral_sold, 1);
        prop_assert_eq!(end.unwrap().leftover, q.lot - 1);
        // A partial bid below the target floors to 0 and is rejected (the exception is not a general round-up).
        if remaining > 1 {
            let part = BidRequest { coin_in_max: remaining - 1, min_collateral_out: 0 };
            prop_assert!(quote_bid(&a, &part, price, t, 1, 1).is_err());
        }
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

    /// Invariant 13 at low prices with parts that can reach the target (final-fill exception reachable).
    ///
    /// Bounds. Let v_i = c_i*P/denom (exact). Without the exception every fill is floor(v_i), so
    /// sum(floor(v_i)) <= floor(sum(v_i)) = the single bid: sum <= one. The exception rounds one fill UP to 1
    /// (the target-reaching one, only when its floor is 0), so the split path can exceed the single bid by
    /// at most 1: sum <= one + ff_split. In the other direction each part loses < 1 to flooring (a part
    /// rejected as ZeroFill loses < 1 as well) and the single bid may itself gain the exception:
    /// one <= sum + n + ff_one.
    #[test]
    fn split_bid_neutrality_low_price(parts in prop::collection::vec(1u128..2_000, 1..8),
                                      target in 50u128..4_000, price_micro in 100u128..20_000,
                                      t in 0u64..3_000_000) {
        let p = RAY * price_micro / 1_000_000;
        let q = LiquidationQuote { debt_principal: target, amount_to_raise: target, reserve: target, keeper_reward: 0, lot: 1_000_000_000_000, seized: 1_000_000_000_000 };
        let a0 = Auction::open(&q, 0, rai_default(), 0);
        let denom = RAY - a0.schedule.discount_ray(t);
        let total: u128 = parts.iter().sum();
        let x = total.min(target);
        let one_req = BidRequest { coin_in_max: total, min_collateral_out: 0 };
        let (one, ff_one) = match quote_bid(&a0, &one_req, p, t, 1, 1) {
            Ok(f) => (f.collateral_out, mul_div_floor(x, p, denom).unwrap() == 0 && f.collateral_out == 1),
            Err(e) => { prop_assert_eq!(e, AuctionError::ZeroFill); (0, false) }
        };
        let mut a = a0; let mut sum = 0u128; let mut ff_split = 0u128; let n = parts.len() as u128;
        for c in parts {
            let req = BidRequest { coin_in_max: c, min_collateral_out: 0 };
            let rem = a.remaining_target();
            match quote_bid(&a, &req, p, t, 1, 1) {
                Ok(f) => {
                    if mul_div_floor(c.min(rem), p, denom).unwrap() == 0 && f.collateral_out == 1 { ff_split += 1; }
                    sum += f.collateral_out;
                    a = apply_bid(&a, &f).0;
                }
                Err(AuctionError::ZeroFill) => {}
                Err(AuctionError::NotAuctioning) => { prop_assert_eq!(a.status, Status::Settled); break; }
                Err(e) => prop_assert!(false, "unexpected {:?}", e),
            }
        }
        prop_assert!(ff_split <= 1);
        prop_assert!(sum <= one + ff_split);
        prop_assert!(one <= sum + n + ff_one as u128);
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

    /// Same isolation under DerivedFromOracle (the policy ADR-007 accepted): onset = last publish + max_age,
    /// with a genuinely stale feed at the failing observation. B publishes within max_age throughout.
    #[test]
    fn two_type_pause_isolation_derived(stale_extra in 1u64..900, resume_after in 1u64..1_000, now_extra in 0u64..1_000) {
        const MAX_AGE: u64 = 100;
        let d = OnsetPolicy::DerivedFromOracle;
        let o = |now, valid, ts| Observation { now_ms: now, feed_valid: valid, admin_paused: false, oracle_ts: ts, max_age_ms: MAX_AGE };
        let a0 = PauseClock::default().observed(o(0, true, 0), d);
        let pause_at = MAX_AGE + stale_extra; // oracle_ts 0 + max_age < now: genuinely stale
        let a1 = a0.observed(o(pause_at, false, 0), d);
        prop_assert_eq!(a1.paused_since_ms, Some(MAX_AGE)); // onset = 0 + max_age, not the observation time
        prop_assert_eq!(a1.effective_elapsed(0, 0, pause_at + now_extra), MAX_AGE); // frozen while open
        let resume = pause_at + resume_after;
        let a2 = a1.observed(o(resume, true, resume), d);
        prop_assert_eq!(a2.paused_since_ms, None);
        prop_assert_eq!(a2.paused_total_ms, resume - MAX_AGE);
        let now = resume + now_extra;
        prop_assert_eq!(a2.effective_elapsed(0, 0, now), now - (resume - MAX_AGE));
        // B: a fresh publish at least every max_age, observed each time.
        let mut b = a0; let mut cur = 0u64;
        while cur < now {
            cur = (cur + MAX_AGE).min(now);
            b = b.observed(o(cur, true, cur), d);
        }
        prop_assert_eq!(b.paused_since_ms, None);
        prop_assert_eq!(b.paused_total_ms, 0);
        prop_assert_eq!(b.effective_elapsed(0, 0, now), now);
    }

    /// Isolation with a non-zero auction start, a non-zero paused total at start, an open pause at `now`
    /// (feed failure or admin pause), and the discount a bid actually receives on each type's clock.
    #[test]
    fn pause_isolation_offsets_open_pause_and_discount(
        start in 1u64..1_000, pre_total in 0u64..500, p1 in 1u64..600_000, p2 in 1u64..600_000,
        g in 1u64..600_000, open_len in 0u64..600_000, admin in any::<bool>()) {
        let d = OnsetPolicy::AtObservation;
        let o = |now: u64, bad: bool| Observation {
            now_ms: now, feed_valid: !bad || admin, admin_paused: bad && admin, oracle_ts: now, max_age_ms: 100,
        };
        let base = PauseClock { paused_since_ms: None, paused_total_ms: pre_total, last_seen_oracle_ts: Some(0) };
        let t_pause = start + p1;
        let t_resume = t_pause + p2;
        let t_pause2 = t_resume + g;
        let now = t_pause2 + open_len;
        let a = base.observed(o(t_pause, true), d).observed(o(t_resume, false), d).observed(o(t_pause2, true), d);
        let mut b = base;
        for ts in [t_pause, t_resume, t_pause2] { b = b.observed(o(ts, false), d); }
        prop_assert!(a.is_paused() && !b.is_paused());
        let a_el = a.effective_elapsed(start, pre_total, now);
        let b_el = b.effective_elapsed(start, pre_total, now);
        prop_assert_eq!(a_el, p1 + g); // wall minus the closed pause p2 minus the open pause
        prop_assert_eq!(b_el, now - start);
        // End to end: the discount a bid gets follows each type's own effective clock.
        let q = LiquidationQuote { debt_principal: 1_000_000, amount_to_raise: 1_100_000, reserve: 1_100_000, keeper_reward: 0, lot: 10_000_000_000, seized: 10_000_000_000 };
        let aa = Auction::open(&q, start, rai_default(), pre_total);
        let req = BidRequest { coin_in_max: 1_000, min_collateral_out: 0 };
        let fa = quote_bid(&aa, &req, RAY, a_el, 1, 1).unwrap();
        let fb = quote_bid(&aa, &req, RAY, b_el, 1, 1).unwrap();
        prop_assert_eq!(fa.discount_ray, rai_default().discount_ray(p1 + g));
        prop_assert_eq!(fb.discount_ray, rai_default().discount_ray(now - start));
        prop_assert!(fa.discount_ray <= fb.discount_ray);
    }
}

/// Deterministic proof that the final-fill branch is taken (and that it is the only round-up).
#[test]
fn final_fill_branch_is_hit_deterministically() {
    let q = LiquidationQuote {
        debt_principal: 10,
        amount_to_raise: 11,
        reserve: 11,
        keeper_reward: 0,
        lot: 100,
        seized: 100,
    };
    let a = Auction::open(&q, 0, rai_default(), 0);
    let p = RAY / 1_000; // 0.001 collateral per coin: 11 coin floors to 0
    assert_eq!(mul_div_floor(11, p, RAY).unwrap(), 0);
    let req = BidRequest {
        coin_in_max: u128::MAX,
        min_collateral_out: 0,
    };
    let f = quote_bid(&a, &req, p, 0, 25, 11).unwrap();
    assert_eq!((f.collateral_out, f.charged, f.ends_auction), (1, 11, true));
    let part = BidRequest {
        coin_in_max: 10,
        min_collateral_out: 0,
    };
    assert_eq!(
        quote_bid(&a, &part, p, 0, 1, 1),
        Err(AuctionError::ZeroFill)
    );
}
