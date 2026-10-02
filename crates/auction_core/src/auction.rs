//! Collateral auction state machine (spec §5.5, §5.6, §5.8). No expiry; Settled is the only terminal state.
#![allow(clippy::arithmetic_side_effects)] // subtractions are guarded by the min()/checks noted inline
use crate::error::AuctionError;
use crate::liquidation::LiquidationQuote;
use crate::math::{mul_div_ceil, mul_div_floor};
use crate::schedule::Schedule;
use crate::units::RAY;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Auctioning,
    Settled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Auction {
    pub status: Status,
    pub collateral_seized: u128,
    pub collateral_sold: u128,
    pub collateral_left: u128,
    pub keeper_reward: u128,
    pub leftover_claimable: u128,
    pub leftover_claimed: u128,
    pub debt_principal: u128,
    pub amount_to_raise: u128,
    pub reserved_amount: u128,
    pub debt_raised: u128,
    pub penalty_raised: u128,
    pub start_ms: u64,
    pub schedule: Schedule,
    pub type_paused_total_at_start: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BidRequest {
    pub coin_in_max: u128,
    pub min_collateral_out: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BidFill {
    pub charged: u128,
    pub collateral_out: u128,
    pub debt_part: u128,
    pub penalty_part: u128,
    pub discount_ray: u128,
    pub ends_auction: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainEnd {
    pub shortfall: u128,
    pub release: u128,
    pub leftover: u128,
}

impl Auction {
    pub fn open(
        q: &LiquidationQuote,
        start_ms: u64,
        schedule: Schedule,
        type_paused_total_at_start: u64,
    ) -> Self {
        Self {
            status: Status::Auctioning,
            collateral_seized: q.seized,
            collateral_sold: 0,
            collateral_left: q.lot,
            keeper_reward: q.keeper_reward,
            leftover_claimable: 0,
            leftover_claimed: 0,
            debt_principal: q.debt_principal,
            amount_to_raise: q.amount_to_raise,
            reserved_amount: q.reserve,
            debt_raised: 0,
            penalty_raised: 0,
            start_ms,
            schedule,
            type_paused_total_at_start,
        }
    }
    pub fn raised(&self) -> u128 {
        // Guard: both counters only grow by amounts capped at the target, so the sum stays <= amount_to_raise.
        self.debt_raised + self.penalty_raised
    }
    pub fn remaining_target(&self) -> u128 {
        self.amount_to_raise - self.raised()
    } // raised ≤ target (invariant 3)
}

pub fn quote_bid(
    a: &Auction,
    req: &BidRequest,
    price_ray: u128,
    t_eff_ms: u64,
    min_bid: u128,
    chost: u128,
) -> Result<BidFill, AuctionError> {
    if a.status != Status::Auctioning {
        return Err(AuctionError::NotAuctioning);
    }
    // Unreachable via validate_feed/bid_price (bid price >= M > 0); stops the final-fill exception paying out at a zero price.
    if price_ray == 0 {
        return Err(AuctionError::Math);
    }
    let remaining = a.remaining_target();
    let coin = req.coin_in_max.min(remaining);
    let d = a.schedule.discount_ray(t_eff_ms);
    let denom = RAY - d; // d ≤ 30% of RAY
    let full_out = mul_div_floor(coin, price_ray, denom)?;
    let (collateral_out, charged) = if full_out >= a.collateral_left {
        // The lot binds: pay only for what is left, rounded up (owed). The `.min(coin)` is provably a
        // no-op (full_out >= L with integer coin implies coin >= ceil(L * denom / P)); it is kept only as
        // a defensive bound, not a real cap.
        let c = mul_div_ceil(a.collateral_left, denom, price_ray)?.min(coin);
        (a.collateral_left, c)
    } else if full_out == 0 && coin == remaining && a.collateral_left > 0 {
        // Liveness exception (the only place a payout rounds UP): the target-reaching fill is worth
        // < 1 collateral unit, so flooring would make the auction unbiddable forever. Pay out exactly
        // 1 base unit for the full remaining target; bounded by 1 unit per auction.
        (1, coin)
    } else {
        (full_out, coin)
    };
    if collateral_out == 0 || charged == 0 {
        return Err(AuctionError::ZeroFill);
    }
    if collateral_out < req.min_collateral_out {
        return Err(AuctionError::Slippage);
    }
    let remaining_after = remaining - charged;
    let lot_empties = collateral_out == a.collateral_left;
    let ends_auction = remaining_after == 0 || lot_empties;
    if !ends_auction && charged < min_bid {
        return Err(AuctionError::BelowMinBid);
    }
    if !ends_auction && remaining_after < chost {
        return Err(AuctionError::DustRemainder);
    }
    let debt_left = a.debt_principal - a.debt_raised.min(a.debt_principal);
    let debt_part = charged.min(debt_left);
    Ok(BidFill {
        charged,
        collateral_out,
        debt_part,
        penalty_part: charged - debt_part,
        discount_ray: d,
        ends_auction,
    })
}

/// Applies a fill to the auction.
///
/// Precondition: `f` must come from `quote_bid` on this exact state; the fill is trusted.
pub fn apply_bid(a: &Auction, f: &BidFill) -> (Auction, Option<ChainEnd>) {
    debug_assert!(
        a.status == Status::Auctioning
            && f.collateral_out <= a.collateral_left
            && f.charged <= a.remaining_target()
    );
    let mut n = *a;
    n.collateral_left -= f.collateral_out; // guard: collateral_out <= collateral_left (quote_bid)
    n.collateral_sold += f.collateral_out; // sold + left == lot, cannot overflow
    n.debt_raised += f.debt_part; // guard: debt_part + penalty_part = charged <= remaining target
    n.penalty_raised += f.penalty_part;
    if !f.ends_auction {
        return (n, None);
    }
    n.status = Status::Settled;
    let target_reached = n.raised() == n.amount_to_raise;
    n.leftover_claimable = if target_reached { n.collateral_left } else { 0 };
    let end = ChainEnd {
        shortfall: n.debt_principal.saturating_sub(n.debt_raised),
        release: n.reserved_amount,
        leftover: n.leftover_claimable,
    };
    (n, Some(end))
}

/// Settles an auction whose lot is empty (e.g. the keeper reward consumed the whole seizure).
/// Permissionless. No bid can ever succeed on `collateral_left == 0`, so without this the auction
/// would hold its reservation forever (ADR-012 has no expiry).
pub fn settle_empty(a: &Auction) -> Result<(Auction, ChainEnd), AuctionError> {
    if a.status != Status::Auctioning || a.collateral_left != 0 {
        return Err(AuctionError::NotAuctioning);
    }
    let mut n = *a;
    n.status = Status::Settled;
    let end = ChainEnd {
        shortfall: n.debt_principal.saturating_sub(n.debt_raised),
        release: n.reserved_amount,
        leftover: 0,
    };
    Ok((n, end))
}

pub fn claim_leftover(a: &Auction) -> Result<(Auction, u128), AuctionError> {
    let amount = a.leftover_claimable - a.leftover_claimed;
    if a.status != Status::Settled || amount == 0 {
        return Err(AuctionError::NothingToClaim);
    }
    let mut n = *a;
    n.leftover_claimed = n.leftover_claimable;
    Ok((n, amount))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::liquidation::LiquidationQuote;
    use crate::schedule::{rai_default, Schedule, SchedulePoint};
    use crate::units::RAY;

    fn quote(lot: u128, debt: u128, target: u128) -> LiquidationQuote {
        LiquidationQuote {
            debt_principal: debt,
            amount_to_raise: target,
            reserve: target,
            keeper_reward: 0,
            lot,
            seized: lot,
        }
    }
    fn flat(bps: u16) -> Schedule {
        Schedule::new(&[SchedulePoint {
            elapsed_ms: 0,
            discount_bps: bps,
        }])
        .unwrap()
    }

    #[test]
    fn partial_bid_debt_first() {
        // P = 1 collateral per coin, discount 0: 400 coin → 400 collateral
        let a = Auction::open(&quote(1_000, 900, 990), 0, flat(0), 0);
        let f = quote_bid(
            &a,
            &BidRequest {
                coin_in_max: 400,
                min_collateral_out: 400,
            },
            RAY,
            0,
            25,
            110,
        )
        .unwrap();
        assert_eq!(
            (
                f.charged,
                f.collateral_out,
                f.debt_part,
                f.penalty_part,
                f.ends_auction
            ),
            (400, 400, 400, 0, false)
        );
        let (a2, end) = apply_bid(&a, &f);
        assert!(end.is_none());
        assert_eq!(
            (a2.collateral_left, a2.collateral_sold, a2.debt_raised),
            (600, 400, 400)
        );
    }

    #[test]
    fn discount_increases_collateral_out_rounded_down() {
        let a = Auction::open(&quote(10_000, 900, 990), 0, flat(1000), 0); // 10% off
        let f = quote_bid(
            &a,
            &BidRequest {
                coin_in_max: 100,
                min_collateral_out: 0,
            },
            RAY,
            0,
            25,
            110,
        )
        .unwrap();
        assert_eq!(f.collateral_out, 111); // floor(100 / 0.9)
    }

    #[test]
    fn lot_binds_charges_only_for_lot() {
        let a = Auction::open(&quote(50, 900, 990), 0, flat(0), 0);
        let f = quote_bid(
            &a,
            &BidRequest {
                coin_in_max: 400,
                min_collateral_out: 0,
            },
            RAY,
            0,
            25,
            110,
        )
        .unwrap();
        assert_eq!(
            (f.collateral_out, f.charged, f.ends_auction),
            (50, 50, true)
        );
        let (a2, end) = apply_bid(&a, &f);
        assert_eq!(a2.status, Status::Settled);
        assert_eq!(
            end.unwrap(),
            ChainEnd {
                shortfall: 850,
                release: 990,
                leftover: 0
            }
        );
    }

    #[test]
    fn target_and_lot_end_together() {
        // Review Focus #1
        let a = Auction::open(&quote(990, 900, 990), 0, flat(0), 0);
        let f = quote_bid(
            &a,
            &BidRequest {
                coin_in_max: 990,
                min_collateral_out: 990,
            },
            RAY,
            0,
            25,
            110,
        )
        .unwrap();
        assert!(f.charged <= 990 && f.ends_auction);
        let (_, end) = apply_bid(&a, &f);
        assert_eq!(
            end.unwrap(),
            ChainEnd {
                shortfall: 0,
                release: 990,
                leftover: 0
            }
        );
    }

    #[test]
    fn huge_coin_in_is_capped_at_target() {
        // Review Focus #2
        let a = Auction::open(&quote(1_000_000, 900, 990), 0, flat(0), 0);
        let f = quote_bid(
            &a,
            &BidRequest {
                coin_in_max: u128::MAX,
                min_collateral_out: 0,
            },
            RAY,
            0,
            25,
            110,
        )
        .unwrap();
        assert_eq!(
            (f.charged, f.collateral_out, f.penalty_part),
            (990, 990, 90)
        );
        let (a2, end) = apply_bid(&a, &f);
        assert_eq!(
            end.unwrap(),
            ChainEnd {
                shortfall: 0,
                release: 990,
                leftover: 1_000_000 - 990
            }
        );
        let (a3, paid) = claim_leftover(&a2).unwrap();
        assert_eq!(paid, 999_010);
        assert_eq!(claim_leftover(&a3), Err(AuctionError::NothingToClaim));
    }

    #[test]
    fn rejections() {
        let a = Auction::open(&quote(1_000, 900, 990), 0, rai_default(), 0);
        let r = |coin, min| {
            quote_bid(
                &a,
                &BidRequest {
                    coin_in_max: coin,
                    min_collateral_out: min,
                },
                RAY,
                0,
                25,
                110,
            )
        };
        assert_eq!(r(10, 0), Err(AuctionError::BelowMinBid));
        assert_eq!(r(100, 1_000), Err(AuctionError::Slippage));
        assert_eq!(r(900, 0), Err(AuctionError::DustRemainder)); // leaves 90 < chost 110
        let settled = Auction {
            status: Status::Settled,
            ..a
        };
        assert_eq!(
            quote_bid(
                &settled,
                &BidRequest {
                    coin_in_max: 100,
                    min_collateral_out: 0
                },
                RAY,
                0,
                25,
                110
            ),
            Err(AuctionError::NotAuctioning)
        );
    }

    fn bid_req(coin: u128) -> BidRequest {
        BidRequest {
            coin_in_max: coin,
            min_collateral_out: 0,
        }
    }

    #[test]
    fn lot_zero_settles_via_settle_empty() {
        // lot 0: every bid takes the lot-binds branch with charged = ceil(0) = 0 -> ZeroFill.
        let a = Auction::open(&quote(0, 900, 990), 0, flat(0), 0);
        assert_eq!(
            quote_bid(&a, &bid_req(100), RAY, 0, 25, 110),
            Err(AuctionError::ZeroFill)
        );
        // settle_empty: shortfall = 900 - 0 = 900, release = reserve = 990, leftover = 0.
        let (s, end) = settle_empty(&a).unwrap();
        assert_eq!(s.status, Status::Settled);
        assert_eq!(s.leftover_claimable, 0);
        assert_eq!(
            end,
            ChainEnd {
                shortfall: 900,
                release: 990,
                leftover: 0
            }
        );
        assert_eq!(settle_empty(&s), Err(AuctionError::NotAuctioning));
        let live = Auction::open(&quote(5, 900, 990), 0, flat(0), 0);
        assert_eq!(settle_empty(&live), Err(AuctionError::NotAuctioning));
    }

    #[test]
    fn sub_unit_remainder_final_fill_takes_one_unit() {
        // P = 752e-6 RAY: 990 coin * 752e-6 = 0.744 -> floor 0. coin == remaining (990) and
        // lot 1000 > 0 -> final-fill exception: out 1, charged 990, ends (remaining_after 0).
        let p = 752 * RAY / 1_000_000;
        let a = Auction::open(&quote(1_000, 900, 990), 0, flat(0), 0);
        let f = quote_bid(&a, &bid_req(990), p, 0, 25, 110).unwrap();
        assert_eq!(
            (f.collateral_out, f.charged, f.ends_auction),
            (1, 990, true)
        );
        // debt_part 900, penalty 90; collateral_left 999; target reached -> leftover 999.
        let (_, end) = apply_bid(&a, &f);
        assert_eq!(
            end.unwrap(),
            ChainEnd {
                shortfall: 0,
                release: 990,
                leftover: 999
            }
        );
        // coin 500 < remaining 990: 500 * 752e-6 = 0.376 -> floor 0, no exception.
        assert_eq!(
            quote_bid(&a, &bid_req(500), p, 0, 25, 110),
            Err(AuctionError::ZeroFill)
        );
    }

    #[test]
    fn lot_binds_charge_rounds_up() {
        // P = 3 collateral/coin, d = 0. coin 400 -> full_out 1200 >= lot 50 -> lot binds.
        // charged = ceil(50 / 3) = ceil(16.67) = 17 (floor would be 16).
        let a = Auction::open(&quote(50, 900, 990), 0, flat(0), 0);
        let f = quote_bid(&a, &bid_req(400), 3 * RAY, 0, 25, 110).unwrap();
        assert_eq!(
            (f.collateral_out, f.charged, f.ends_auction),
            (50, 17, true)
        );
    }

    #[test]
    fn zero_price_is_math_error() {
        // price 0 is rejected up front with Math for any coin, small or target-reaching; no panic.
        let a = Auction::open(&quote(1_000, 900, 990), 0, flat(0), 0);
        assert_eq!(
            quote_bid(&a, &bid_req(100), 0, 0, 25, 110),
            Err(AuctionError::Math)
        );
        assert_eq!(
            quote_bid(&a, &bid_req(990), 0, 0, 25, 110),
            Err(AuctionError::Math)
        );
        let z = Auction::open(&quote(0, 900, 990), 0, flat(0), 0);
        assert_eq!(
            quote_bid(&z, &bid_req(100), 0, 0, 25, 110),
            Err(AuctionError::Math)
        );
    }

    #[test]
    fn debt_recovered_penalty_short() {
        // lot 950, debt 900, target 990, P = 1, d = 0, min_bid 25, chost 50.
        // Bid 1: coin 900 -> out 900, charged 900, remaining_after 90 >= 50: not ending.
        //   debt_raised 900, collateral_left 50.
        let a = Auction::open(&quote(950, 900, 990), 0, flat(0), 0);
        let f1 = quote_bid(&a, &bid_req(900), RAY, 0, 25, 50).unwrap();
        assert!(!f1.ends_auction);
        let (a2, _) = apply_bid(&a, &f1);
        // Bid 2: coin 90 -> full_out 90 >= 50 -> lot binds: out 50, charged 50.
        //   debt_left 0 -> penalty_part 50; raised 950 != 990 -> leftover 0, shortfall 900-900 = 0.
        let f2 = quote_bid(&a2, &bid_req(90), RAY, 0, 25, 50).unwrap();
        assert_eq!((f2.charged, f2.debt_part, f2.penalty_part), (50, 0, 50));
        let (a3, end) = apply_bid(&a2, &f2);
        assert_eq!(a3.status, Status::Settled);
        assert_eq!(
            end.unwrap(),
            ChainEnd {
                shortfall: 0,
                release: 990,
                leftover: 0
            }
        );
    }

    #[test]
    fn target_and_lot_end_together_with_discount() {
        // P = 2 collateral/coin, d = 10% (denom 0.9 RAY), lot 2199, target 990, debt 900.
        // coin 990: full_out = floor(990 * 2 / 0.9) = 2200 >= 2199 -> lot binds.
        // charged = ceil(2199 * 0.9 / 2) = ceil(989.55) = 990 <= coin: target and lot end together.
        let a = Auction::open(&quote(2_199, 900, 990), 0, flat(1000), 0);
        let f = quote_bid(&a, &bid_req(990), 2 * RAY, 0, 25, 110).unwrap();
        assert_eq!(
            (f.collateral_out, f.charged, f.ends_auction),
            (2_199, 990, true)
        );
        assert!(f.charged <= 990);
        let (s, end) = apply_bid(&a, &f);
        assert_eq!(s.status, Status::Settled);
        assert_eq!(
            end.unwrap(),
            ChainEnd {
                shortfall: 0,
                release: 990,
                leftover: 0
            }
        );
    }
}
