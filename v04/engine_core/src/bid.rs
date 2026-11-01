//! The bid (v0.2.4 `bid.rs`, spec §5.6 row 3) and its chain-end fold (ADR-023), on the auction's
//! own state plus the observation context the collateral actor sends in `BidCtx`.
//!
//! v0.2.4 observed, required a live type, priced with `bid_price`, took `t_eff` from the observed
//! pause clock, then ran `quote_bid` and `apply_bid` with the type's current `min_bid` and
//! `chost`. The collateral actor does the first part (Observe, `require_live`, `P`); [`quote`]
//! does the rest. `BidCtx` is sent only after a live observation, and a live observation closes
//! any open pause, so the pause clock `t_eff` needs is `paused_since = None` with the observed
//! `paused_total`.

pub use auction_core::auction::ChainEnd;
use auction_core::{
    accounting::Accounting,
    auction::{
        BidFill, BidRequest, apply_bid, claim_leftover, quote_bid,
        settle_empty as core_settle_empty,
    },
    pause::PauseClock,
};
use auction_engine_params::EngineError;

use crate::state::{AccountingState, AuctionState};

/// What the collateral actor observed, and the bid it forwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BidQuote {
    pub max_debt_in: u128,
    pub min_collateral_out: u128,
    /// The bid price `P` (v0.2.4 `bid_price`).
    pub price_ray: u128,
    /// The bid's clock claim.
    pub now_ms: u64,
    /// The type's `paused_total_ms` after this observation.
    pub paused_total_ms: u64,
    pub min_bid: u128,
    pub chost: u128,
}

/// A quoted and applied bid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BidOutcome {
    /// The auction after the fill; `Settled` at a chain end.
    pub next: AuctionState,
    pub fill: BidFill,
    /// `Some` exactly when this fill ends the auction (target reached or lot exhausted).
    pub end: Option<ChainEnd>,
    /// The fill takes the whole remaining lot, so the payout sweeps the vault (ADR-033).
    pub lot_empties: bool,
}

/// v0.2.4's `quote_bid` and `apply_bid` on the auction `a` (any phase but `Auctioning` fails
/// `NotAuctioning`, as v0.2.4's settled flag did).
pub fn quote(a: &AuctionState, q: &BidQuote) -> Result<BidOutcome, EngineError> {
    let current = a.to_core()?;
    let clock = PauseClock {
        paused_since_ms: None,
        paused_total_ms: q.paused_total_ms,
        last_seen_oracle_ts: None,
    };
    let t_eff = clock.effective_elapsed(
        current.start_ms,
        current.type_paused_total_at_start,
        q.now_ms,
    );
    let fill = quote_bid(
        &current,
        &BidRequest {
            coin_in_max: q.max_debt_in,
            min_collateral_out: q.min_collateral_out,
        },
        q.price_ray,
        t_eff,
        q.min_bid,
        q.chost,
    )?;
    let (next, end) = apply_bid(&current, &fill);
    Ok(BidOutcome {
        next: AuctionState::from_core(a, &next),
        fill,
        end,
        lot_empties: fill.collateral_out == current.collateral_left,
    })
}

/// The cumulative totals a settling auction reports to the accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettledTotals {
    pub debt_raised: u128,
    pub penalty_raised: u128,
    pub end: ChainEnd,
}

/// v0.2.4 `Accounting::record_settled` from the reported totals: the raised amounts are folded
/// and the shortfall booked, once (the auction sends `Settled` only on its own chain end).
#[must_use]
pub fn fold_settled(acc: &AccountingState, s: &SettledTotals) -> AccountingState {
    let core = acc.core();
    let folded = Accounting {
        total_debt_burned: core.total_debt_burned.saturating_add(s.debt_raised),
        total_penalty_collected: core
            .total_penalty_collected
            .saturating_add(s.penalty_raised),
        ..core
    }
    .record_chain_end(&s.end);
    AccountingState::with_core(acc, &folded)
}

/// v0.2.4 `settle_empty`: settles an `Auctioning` auction whose lot is empty, with its chain end
/// (anything else is `NotAuctioning`, so a second settlement and any later bid fail).
pub fn settle_empty(a: &AuctionState) -> Result<(AuctionState, ChainEnd), EngineError> {
    let (next, end) = core_settle_empty(&a.to_core()?)?;
    Ok((AuctionState::from_core(a, &next), end))
}

/// v0.2.4 `claim_leftover`: books `leftover_claimed = leftover_claimable` and returns the amount
/// owed, `claimable − claimed` (`NothingToClaim` unless settled with something left).
pub fn claim(a: &AuctionState) -> Result<(AuctionState, u128), EngineError> {
    let (next, amount) = claim_leftover(&a.to_core()?)?;
    Ok((AuctionState::from_core(a, &next), amount))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        liquidation::{AuctionIdentity, SeizeReport, awaiting_seize, quote_seized},
        state::{AuctionPhase, CollateralState, QuoteCtx, fixtures},
    };
    use auction_core::{auction::Status, units::RAY};

    const START: u64 = 1_000;

    /// v0.2.4 `liquidated_world`: 500_000 / 600_000 at price 0.5, defaults (2 % at t = 0, penalty
    /// 10 %, min_bid 25, chost 110): lot 499_975, target 660_000.
    fn liquidated() -> AuctionState {
        let args = auction_engine_params::args::CollateralArgs::defaults_for_tests();
        let collateral = CollateralState {
            schedule: crate::state::recs_from_schedule(
                &args.validate(u128::MAX).expect("valid").0.schedule,
            ),
            type_cap: args.type_cap,
            type_reserved: 0,
            paused_total_ms: 0,
            ..fixtures::coll()
        };
        let ctx = QuoteCtx {
            price_ray: RAY / 2,
            global_cap: 10_000_000,
            global_reserved: 0,
            collateral,
        };
        let id = AuctionIdentity {
            config: fixtures::id(1),
            position: fixtures::id(2),
            nonce: 0,
            host: fixtures::actor(3, 4),
            keeper: fixtures::id(5),
            start_ms: START,
            token: fixtures::id(8),
            stable_def: fixtures::id(6),
        };
        let report = SeizeReport {
            owner: fixtures::id(9),
            collateral: 500_000,
            nominal_debt: 600_000,
        };
        let awaiting = awaiting_seize(&id, ctx.clone());
        quote_seized(&awaiting, &ctx, &report).expect("quotes").0
    }

    fn bid(max_debt_in: u128, min_collateral_out: u128) -> BidQuote {
        BidQuote {
            max_debt_in,
            min_collateral_out,
            price_ray: RAY / 2,
            now_ms: START,
            paused_total_ms: 0,
            min_bid: 25,
            chost: 110,
        }
    }

    /// v0.2.4 `final_bid_reaches_target_and_settles_once`: 100_000, 100_000, then 460_000.
    #[test]
    fn partial_then_final_bids_match_v024() {
        let a = liquidated();
        assert_eq!((a.collateral_left, a.amount_to_raise), (499_975, 660_000));
        let first = quote(&a, &bid(100_000, 51_020)).expect("partial");
        assert_eq!(
            (first.fill.debt_part, first.fill.penalty_part),
            (100_000, 0)
        );
        assert_eq!(
            first.fill.collateral_out, 51_020,
            "floor(100_000 × 0.5 / 0.98)"
        );
        assert_eq!((first.end, first.lot_empties), (None, false));
        assert_eq!(first.next.phase, AuctionPhase::Auctioning);
        let second = quote(&first.next, &bid(100_000, 0)).expect("partial");
        let last = quote(&second.next, &bid(460_000, 0)).expect("final");
        assert_eq!(
            (last.fill.debt_part, last.fill.penalty_part),
            (400_000, 60_000)
        );
        assert_eq!(last.fill.collateral_out, 234_693);
        assert!(!last.lot_empties, "the target binds, not the lot");
        let s = &last.next;
        assert_eq!(s.phase, AuctionPhase::Settled);
        assert_eq!((s.debt_raised, s.penalty_raised), (600_000, 60_000));
        assert_eq!(s.leftover_claimable, 499_975 - 2 * 51_020 - 234_693);
        assert_eq!(
            last.end,
            Some(ChainEnd {
                shortfall: 0,
                release: 660_000,
                leftover: s.leftover_claimable
            })
        );
        assert_eq!(
            quote(s, &bid(1_000, 0)).map(|o| o.fill),
            Err(EngineError::NotAuctioning)
        );
    }

    #[test]
    fn zero_bid_slippage_min_bid_and_dust_fail_as_in_v024() {
        let a = liquidated();
        let err = |q: BidQuote| quote(&a, &q).map(|o| o.fill).expect_err("refused");
        assert_eq!(err(bid(0, 0)), EngineError::ZeroFill);
        assert_eq!(err(bid(100_000, 51_021)), EngineError::Slippage);
        assert_eq!(err(bid(10, 0)), EngineError::BelowMinBid);
        assert_eq!(err(bid(660_000 - 50, 0)), EngineError::DustRemainder);
    }

    #[test]
    fn the_discount_does_not_advance_over_a_pause_closed_after_the_start() {
        let a = liquidated();
        // 600 s later with 600 s of pause closed since the start: t_eff = 0, as at the start.
        let paused = BidQuote {
            now_ms: START + 600_000,
            paused_total_ms: 600_000,
            ..bid(100_000, 0)
        };
        assert_eq!(quote(&a, &paused).expect("bid").fill.collateral_out, 51_020);
        let unpaused = BidQuote {
            now_ms: START + 600_000,
            ..bid(100_000, 0)
        };
        assert!(quote(&a, &unpaused).expect("bid").fill.collateral_out > 51_020);
    }

    #[test]
    fn a_lot_emptying_fill_is_flagged() {
        let a = AuctionState {
            collateral_left: 1_000,
            ..liquidated()
        };
        let o = quote(&a, &bid(660_000, 0)).expect("lot binds");
        assert!(o.lot_empties);
        assert_eq!(o.fill.collateral_out, 1_000);
        assert_eq!(o.next.phase, AuctionPhase::Settled);
        assert_eq!(o.next.leftover_claimable, 0);
    }

    #[test]
    fn claim_pays_the_leftover_once_and_only_after_settlement() {
        let a = liquidated();
        assert_eq!(claim(&a).map(|c| c.1), Err(EngineError::NothingToClaim));
        let settled = quote(&a, &bid(660_000, 0)).expect("final").next;
        let left = settled.leftover_claimable;
        assert_eq!(left, 499_975 - 336_734);
        let (claimed, amount) = claim(&settled).expect("claims");
        assert_eq!((amount, claimed.leftover_claimed), (left, left));
        assert_eq!(
            claim(&claimed).map(|c| c.1),
            Err(EngineError::NothingToClaim)
        );
    }

    #[test]
    fn settle_empty_settles_an_empty_lot_once() {
        let live = liquidated();
        assert_eq!(
            settle_empty(&live).map(|s| s.1),
            Err(EngineError::NotAuctioning),
            "a lot is left"
        );
        let empty = AuctionState {
            collateral_left: 0,
            ..live
        };
        let (settled, end) = settle_empty(&empty).expect("settles");
        assert_eq!(settled.phase, AuctionPhase::Settled);
        assert_eq!(
            end,
            ChainEnd {
                shortfall: 600_000,
                release: 660_000,
                leftover: 0
            }
        );
        assert_eq!(
            settle_empty(&settled).map(|s| s.1),
            Err(EngineError::NotAuctioning)
        );
    }

    #[test]
    fn fold_settled_equals_record_settled() {
        let a = liquidated();
        let o = quote(&a, &bid(660_000, 0)).expect("final");
        let end = o.end.expect("chain end");
        let core_next = o.next.to_core().expect("core");
        assert_eq!(core_next.status, Status::Settled);
        let before = AccountingState {
            unbacked_debt: 7,
            total_debt_burned: 11,
            ..AccountingState::new()
        };
        let expected =
            AccountingState::with_core(&before, &before.core().record_settled(&core_next, &end));
        let totals = SettledTotals {
            debt_raised: o.next.debt_raised,
            penalty_raised: o.next.penalty_raised,
            end,
        };
        assert_eq!(fold_settled(&before, &totals), expected);
        let shortfall = SettledTotals {
            end: ChainEnd {
                shortfall: 5,
                ..end
            },
            ..totals
        };
        let booked = fold_settled(&before, &shortfall);
        assert_eq!(
            (booked.unbacked_debt, booked.total_shortfall_booked),
            (12, 5)
        );
    }
}
