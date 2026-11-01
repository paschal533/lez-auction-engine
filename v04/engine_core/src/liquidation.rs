//! The liquidation quote (v0.2.4 `liquidate.rs`, spec §5.6, §5.9) split over the two auction
//! handlers: `Open` stores the quote inputs observed in this transaction, `Seized` quotes the
//! host's report with `auction_core` exactly as v0.2.4 did and opens the core auction.
//!
//! The quote books the REPORTED amounts (ruling R-5e): the host's `Seized.collateral` is both
//! the position collateral and the swept balance, so `seized == collateral`. A donation that made
//! the host vault larger lands in the auction vault as unbooked surplus.

use auction_core::{
    auction::Auction,
    liquidation::{CapState, LiquidationInput, LiquidationQuote, quote_liquidation},
};
use auction_engine_params::EngineError;
use lee_core::account::{AccountId, Actor};

use crate::state::{AuctionPhase, AuctionScratch, AuctionState, QuoteCtx, TAG_AUCTION};

/// Who and what an auction is, fixed at `Open`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuctionIdentity {
    pub config: AccountId,
    pub position: AccountId,
    pub nonce: u64,
    pub host: Actor,
    pub keeper: AccountId,
    pub start_ms: u64,
}

/// The auction `Open` writes: phase `AwaitingSeize`, no core amounts yet, the quote inputs in
/// its scratch. The schedule and pause snapshot are the collateral actor's as observed now.
#[must_use]
pub fn awaiting_seize(id: &AuctionIdentity, ctx: QuoteCtx) -> AuctionState {
    AuctionState {
        tag: TAG_AUCTION,
        config: id.config,
        position: id.position,
        nonce: id.nonce,
        host: id.host,
        phase: AuctionPhase::AwaitingSeize,
        owner: AccountId::new([0; 32]),
        keeper: id.keeper,
        collateral_seized: 0,
        collateral_sold: 0,
        collateral_left: 0,
        keeper_reward: 0,
        leftover_claimable: 0,
        leftover_claimed: 0,
        debt_principal: 0,
        amount_to_raise: 0,
        reserved_amount: 0,
        debt_raised: 0,
        penalty_raised: 0,
        start_ms: id.start_ms,
        schedule: ctx.collateral.schedule.clone(),
        type_paused_total_at_start: ctx.collateral.paused_total_ms,
        scratch: Some(AuctionScratch::Quote(Box::new(ctx))),
    }
}

/// The host's report, as the auction books it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeizeReport {
    pub owner: AccountId,
    pub collateral: u128,
    pub nominal_debt: u128,
}

/// v0.2.4's quote (`quote_liquidation` with the type and global caps) and `Auction::open` for the
/// reported seizure. Returns the opened auction (phase `Auctioning`, scratch cleared) and the quote.
pub fn quote_seized(
    awaiting: &AuctionState,
    ctx: &QuoteCtx,
    report: &SeizeReport,
) -> Result<(AuctionState, LiquidationQuote), EngineError> {
    let params = ctx.collateral.collateral_params()?;
    let caps = CapState {
        type_cap: ctx.collateral.type_cap,
        type_reserved: ctx.collateral.type_reserved,
        global_cap: ctx.global_cap,
        global_reserved: ctx.global_reserved,
    };
    let quote = quote_liquidation(
        &LiquidationInput {
            position_collateral: report.collateral,
            vault_balance: report.collateral,
            nominal_debt: report.nominal_debt,
            price_ray: ctx.price_ray,
        },
        &params,
        &caps,
    )?;
    let opened = Auction::open(
        &quote,
        awaiting.start_ms,
        params.schedule,
        awaiting.type_paused_total_at_start,
    );
    let base = AuctionState {
        owner: report.owner,
        scratch: None,
        ..awaiting.clone()
    };
    Ok((AuctionState::from_core(&base, &opened), quote))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures::{actor, coll, id};
    use auction_core::units::RAY;

    fn ctx(price_ray: u128) -> QuoteCtx {
        QuoteCtx {
            token: id(8),
            price_ray,
            global_cap: 10_000_000,
            global_reserved: 0,
            collateral: crate::state::CollateralState {
                type_cap: 5_000_000,
                type_reserved: 0,
                paused_total_ms: 0,
                ..coll()
            },
        }
    }

    fn identity() -> AuctionIdentity {
        AuctionIdentity {
            config: id(1),
            position: id(2),
            nonce: 0,
            host: actor(3, 4),
            keeper: id(5),
            start_ms: 1_000,
        }
    }

    /// v0.2.4 `liquidate_happy_path` (`LIQ_*`): 500_000 collateral, 600_000 debt at price 0.5:
    /// target 660_000, reward min(5 + 600, 50, 60_000) = 50 coins = 25 collateral.
    #[test]
    fn quotes_v024_happy_path_numbers() {
        let c = ctx(RAY / 2);
        let awaiting = awaiting_seize(&identity(), c.clone());
        assert_eq!(awaiting.phase, AuctionPhase::AwaitingSeize);
        let report = SeizeReport {
            owner: id(9),
            collateral: 500_000,
            nominal_debt: 600_000,
        };
        let (a, q) = quote_seized(&awaiting, &c, &report).expect("quotes");
        assert_eq!(
            (
                a.collateral_seized,
                a.keeper_reward,
                a.collateral_left,
                a.debt_principal,
                a.amount_to_raise,
                a.reserved_amount
            ),
            (500_000, 25, 499_975, 600_000, 660_000, 660_000)
        );
        assert_eq!(q.reserve, 660_000);
        assert_eq!(
            (a.phase, a.owner, a.start_ms, a.scratch.clone()),
            (AuctionPhase::Auctioning, id(9), 1_000, None)
        );
        assert_eq!(a.schedule, c.collateral.schedule);
    }

    #[test]
    fn caps_and_zero_debt_fail_as_in_v024() {
        let full = QuoteCtx {
            global_reserved: 9_999_999,
            ..ctx(RAY)
        };
        let awaiting = awaiting_seize(&identity(), full.clone());
        let report = SeizeReport {
            owner: id(9),
            collateral: 1_000,
            nominal_debt: 1_000,
        };
        assert_eq!(
            quote_seized(&awaiting, &full, &report).map(|(_, q)| q),
            Err(EngineError::CapacityExhausted)
        );
        let none = SeizeReport {
            nominal_debt: 0,
            ..report
        };
        assert_eq!(
            quote_seized(&awaiting, &ctx(RAY), &none).map(|(_, q)| q),
            Err(EngineError::ZeroDebt)
        );
    }
}
