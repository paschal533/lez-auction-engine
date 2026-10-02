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

    /// Burns min(amount, buffer_balance, unbacked_debt). `buffer_balance` is the ACTUAL holding balance.
    pub fn settle_debt(self, amount: u128, buffer_balance: u128) -> (Self, u128) {
        let burn = amount.min(buffer_balance).min(self.unbacked_debt);
        (
            Self {
                unbacked_debt: self.unbacked_debt.saturating_sub(burn), // burn <= unbacked
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
