//! Engine-only accounting (spec §5.8, ADR-010). Immutable updates.
use crate::auction::{BidFill, ChainEnd};

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
