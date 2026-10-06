//! Engine-only accounting (spec §5.8, ADR-010). Immutable updates.
use crate::auction::{Auction, BidFill, ChainEnd, Status};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Accounting {
    pub unbacked_debt: u128,
    pub total_shortfall_booked: u128,
    pub total_penalty_collected: u128,
    pub total_debt_burned: u128,
    pub total_debt_settled: u128,
    pub surplus_nonce: u64,
    pub debt_nonce: u64,
    pub host_frozen_by_engine: bool,
    /// Unbacked debt reserved by a live debt auction. Invariant: debt_on_auction <= unbacked_debt.
    pub debt_on_auction: u128,
    /// Stablecoin burned by debt-auction takers (kept apart from `total_debt_burned`, which is collateral auctions).
    pub total_debt_auctioned: u128,
}

impl Accounting {
    pub fn record_bid(self, f: &BidFill) -> Self {
        Self {
            total_debt_burned: self.total_debt_burned.saturating_add(f.debt_part),
            total_penalty_collected: self.total_penalty_collected.saturating_add(f.penalty_part),
            ..self
        }
    }

    pub fn record_chain_end(self, e: &ChainEnd) -> Self {
        Self {
            unbacked_debt: self.unbacked_debt.saturating_add(e.shortfall),
            total_shortfall_booked: self.total_shortfall_booked.saturating_add(e.shortfall),
            ..self
        }
    }

    /// Chain-end fold (ADR-023): `Accounting` is not written by non-final bids, so the settling
    /// transition adds the auction's cumulative raised amounts and books its shortfall, once.
    ///
    /// Preconditions: call exactly once per auction, after it is `Settled`, with the `ChainEnd`
    /// produced by that same settlement (`apply_bid`'s `Some(end)` or `settle_empty`). `e` is not
    /// validated against `a`. The fold is not idempotent: a second call double-counts. A
    /// non-`Settled` auction is a no-op (returns `self` unchanged) so an early call cannot
    /// fold a partial raise or book a shortfall; it never panics.
    pub fn record_settled(self, a: &Auction, e: &ChainEnd) -> Self {
        if a.status != Status::Settled {
            return self;
        }
        Self {
            total_debt_burned: self.total_debt_burned.saturating_add(a.debt_raised),
            total_penalty_collected: self
                .total_penalty_collected
                .saturating_add(a.penalty_raised),
            ..self
        }
        .record_chain_end(e)
    }

    /// A debt auction of `lot` starts: that much unbacked debt is reserved (cannot be settled from the buffer).
    pub fn record_debt_start(self, lot: u128) -> Self {
        Self {
            debt_on_auction: self.debt_on_auction.saturating_add(lot),
            ..self
        }
    }

    /// A debt-auction take: the taker's `coin_taken` is burned, reducing both unbacked debt and the reservation.
    /// `coin_taken` is `old.lot_left - new.lot_left`, not the requested amount.
    pub fn record_debt_take(self, coin_taken: u128) -> Self {
        Self {
            unbacked_debt: self.unbacked_debt.saturating_sub(coin_taken),
            debt_on_auction: self.debt_on_auction.saturating_sub(coin_taken),
            total_debt_auctioned: self.total_debt_auctioned.saturating_add(coin_taken),
            ..self
        }
    }

    /// A debt auction closes with `lot_left` untaken: the reservation is released and the debt is settleable again.
    pub fn record_debt_close(self, lot_left: u128) -> Self {
        Self {
            debt_on_auction: self.debt_on_auction.saturating_sub(lot_left),
            ..self
        }
    }

    /// Burns min(amount, buffer_balance, unbacked_debt - debt_on_auction). `buffer_balance` is the ACTUAL holding
    /// balance. Debt reserved by a live debt auction is excluded so it cannot be covered twice.
    pub fn settle_debt(self, amount: u128, buffer_balance: u128) -> (Self, u128) {
        let settleable = self.unbacked_debt.saturating_sub(self.debt_on_auction);
        let burn = amount.min(buffer_balance).min(settleable);
        (
            Self {
                unbacked_debt: self.unbacked_debt.saturating_sub(burn), // burn <= settleable <= unbacked
                total_debt_settled: self.total_debt_settled.saturating_add(burn),
                ..self
            },
            burn,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auction::{BidFill, ChainEnd};
    #[test]
    fn records_and_settles() {
        let a = Accounting::default()
            .record_bid(&BidFill {
                charged: 990,
                collateral_out: 0,
                debt_part: 900,
                penalty_part: 90,
                discount_ray: 0,
                ends_auction: true,
            })
            .record_chain_end(&ChainEnd {
                shortfall: 40,
                release: 990,
                leftover: 0,
            });
        assert_eq!(
            (
                a.total_debt_burned,
                a.total_penalty_collected,
                a.unbacked_debt,
                a.total_shortfall_booked
            ),
            (900, 90, 40, 40)
        );
        let (a2, burned) = a.settle_debt(u128::MAX, 25);
        assert_eq!(
            (burned, a2.unbacked_debt, a2.total_debt_settled),
            (25, 15, 25)
        );
        let (a3, burned) = a2.settle_debt(u128::MAX, 1_000);
        assert_eq!((burned, a3.unbacked_debt), (15, 0));
    }
}

#[cfg(test)]
mod fix1_tests {
    use super::*;

    #[test]
    fn reserved_debt_is_not_settled_twice() {
        // unbacked 500 is entirely on auction (debt_lot 500); 300 coin reaches the buffer.
        let a = Accounting {
            unbacked_debt: 500,
            ..Accounting::default()
        }
        .record_debt_start(500);
        assert_eq!(a.debt_on_auction, 500);
        let (a, burned) = a.settle_debt(u128::MAX, 300); // min(300, 500 - 500) = 0
        assert_eq!((burned, a.unbacked_debt), (0, 500));
        let a = a.record_debt_take(500); // taker burns 500 coin: unbacked 0, on_auction 0
        assert_eq!(
            (a.unbacked_debt, a.debt_on_auction, a.total_debt_auctioned),
            (0, 0, 500)
        );
        assert_eq!(a.total_debt_burned, 0); // collateral-auction counter untouched
        let (a, burned) = a.settle_debt(u128::MAX, 300); // nothing left to burn; 300 stays surplus
        assert_eq!((burned, a.unbacked_debt), (0, 0));
    }

    #[test]
    fn partial_take_then_close_releases_remainder() {
        let a = Accounting {
            unbacked_debt: 500,
            ..Accounting::default()
        }
        .record_debt_start(500);
        let a = a.record_debt_take(300); // unbacked 200, on_auction 200
        assert_eq!((a.unbacked_debt, a.debt_on_auction), (200, 200));
        let (a, burned) = a.settle_debt(u128::MAX, 1_000);
        assert_eq!(burned, 0);
        let a = a.record_debt_close(200); // untaken remainder released
        assert_eq!(a.debt_on_auction, 0);
        let (a, burned) = a.settle_debt(u128::MAX, 1_000);
        assert_eq!((burned, a.unbacked_debt), (200, 0));
    }
}

#[cfg(test)]
#[allow(clippy::arithmetic_side_effects)] // the crate denies it; test sums of small constants
mod record_settled_tests {
    use super::*;
    use crate::auction::{
        apply_bid, quote_bid, settle_empty, Auction, BidRequest, ChainEnd, Status,
    };
    use crate::liquidation::LiquidationQuote;
    use crate::schedule::{Schedule, SchedulePoint};
    use crate::units::RAY;

    fn flat() -> Schedule {
        Schedule::new(&[SchedulePoint {
            elapsed_ms: 0,
            discount_bps: 0,
        }])
        .unwrap()
    }

    #[test]
    fn fold_at_chain_end_equals_per_bid_fold() {
        let q = LiquidationQuote {
            debt_principal: 900,
            amount_to_raise: 990,
            reserve: 990,
            keeper_reward: 0,
            lot: 2_000,
            seized: 2_000,
        };
        let mut a = Auction::open(&q, 0, flat(), 0);
        let mut per_bid = Accounting::default();
        let mut end = None;
        for coin in [400u128, 590] {
            let req = BidRequest {
                coin_in_max: coin,
                min_collateral_out: 0,
            };
            let f = quote_bid(&a, &req, RAY, 0, 25, 110).unwrap();
            per_bid = per_bid.record_bid(&f);
            let (n, e) = apply_bid(&a, &f);
            a = n;
            end = e;
        }
        let e: ChainEnd = end.expect("second bid reaches the target");
        assert_eq!(a.status, Status::Settled);
        let per_bid = per_bid.record_chain_end(&e);
        let at_end = Accounting::default().record_settled(&a, &e);
        assert_eq!(at_end, per_bid);
        assert_eq!(
            at_end.total_debt_burned + at_end.total_penalty_collected,
            a.raised()
        );
    }

    #[test]
    fn settle_empty_with_earlier_raise_books_raised_and_shortfall() {
        let q = LiquidationQuote {
            debt_principal: 900,
            amount_to_raise: 990,
            reserve: 990,
            keeper_reward: 0,
            lot: 0,
            seized: 0,
        };
        let mut a = Auction::open(&q, 0, flat(), 0);
        a.debt_raised = 300; // raised by earlier bids before the lot ran out
        let (s, e) = settle_empty(&a).unwrap();
        let acc = Accounting::default().record_settled(&s, &e);
        assert_eq!(acc.total_debt_burned, 300);
        assert_eq!(acc.unbacked_debt, 600);
        assert_eq!(acc.total_shortfall_booked, 600);
    }

    #[test]
    fn record_settled_on_live_auction_is_a_no_op() {
        let q = LiquidationQuote {
            debt_principal: 900,
            amount_to_raise: 990,
            reserve: 990,
            keeper_reward: 0,
            lot: 2_000,
            seized: 2_000,
        };
        let mut a = Auction::open(&q, 0, flat(), 0);
        a.debt_raised = 300;
        a.penalty_raised = 30;
        assert_eq!(a.status, Status::Auctioning);
        let e = ChainEnd {
            shortfall: 600,
            release: 990,
            leftover: 0,
        };
        let before = Accounting {
            unbacked_debt: 7,
            total_debt_burned: 11,
            ..Accounting::default()
        };
        assert_eq!(before.record_settled(&a, &e), before);
    }
}
