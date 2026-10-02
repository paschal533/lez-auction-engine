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
    let remaining = a.remaining_target();
    let coin = req.coin_in_max.min(remaining);
    let d = a.schedule.discount_ray(t_eff_ms);
    let denom = RAY - d; // d ≤ 30% of RAY
    let full_out = mul_div_floor(coin, price_ray, denom)?;
    let (collateral_out, charged) = if full_out >= a.collateral_left {
        // The lot binds: pay only for what is left, rounded up (owed), never above `coin`.
        let c = mul_div_ceil(a.collateral_left, denom, price_ray)?.min(coin);
        (a.collateral_left, c)
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

pub fn apply_bid(a: &Auction, f: &BidFill) -> (Auction, Option<ChainEnd>) {
    let mut n = *a;
    n.collateral_left -= f.collateral_out;
    n.collateral_sold += f.collateral_out;
    n.debt_raised += f.debt_part;
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
}
