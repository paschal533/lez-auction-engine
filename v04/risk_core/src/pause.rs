//! Stepped pause levels (spec §3.2, ADR-073): one rule table, [`allowed`], that every gated
//! handler calls, so the matrix test and the code share it (Plan 9 ruling R9-7).
//!
//! | Level | Meaning |
//! |---|---|
//! | 0 | Normal |
//! | 1 | No new liquidations of this type |
//! | 2 | Level 1, and no new system auctions (`StartSurplus`, `StartDebt`) |
//! | 3 | Level 2, and no bids on this type's auctions and no system takes |
//!
//! Risk-reducing operations always run. No level moves collateral or cancels an auction.

use crate::RiskError;

/// Every operation of the spec §3.2.2 matrix (Plan 9 ruling R9-8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Liquidate,
    Bid,
    StartSurplus,
    StartDebt,
    TakeSurplus,
    TakeDebt,
    SettleEmpty,
    ClaimLeftover,
    SettleDebt,
    CheckSolvency,
    CloseSurplus,
    CloseDebt,
    ObserveFeed,
    SetCollateral,
    SetEngineParams,
    SetSlack,
    SetPauseLevel,
}

impl Op {
    /// Every operation, in declaration order.
    pub const ALL: [Op; 17] = [
        Op::Liquidate,
        Op::Bid,
        Op::StartSurplus,
        Op::StartDebt,
        Op::TakeSurplus,
        Op::TakeDebt,
        Op::SettleEmpty,
        Op::ClaimLeftover,
        Op::SettleDebt,
        Op::CheckSolvency,
        Op::CloseSurplus,
        Op::CloseDebt,
        Op::ObserveFeed,
        Op::SetCollateral,
        Op::SetEngineParams,
        Op::SetSlack,
        Op::SetPauseLevel,
    ];
}

/// A pause level (spec §3.2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// 0: everything runs.
    Normal,
    /// 1: no new liquidations.
    NoLiquidations,
    /// 2: level 1, and no new system auctions.
    NoSystemStarts,
    /// 3: level 2, and no bids and no system takes.
    Halted,
}

impl Level {
    /// Every level, 0 to 3.
    pub const ALL: [Level; 4] = [
        Level::Normal,
        Level::NoLiquidations,
        Level::NoSystemStarts,
        Level::Halted,
    ];

    /// The level stored as `level`; `None` above 3.
    #[must_use]
    pub const fn from_u8(level: u8) -> Option<Level> {
        match level {
            0 => Some(Level::Normal),
            1 => Some(Level::NoLiquidations),
            2 => Some(Level::NoSystemStarts),
            3 => Some(Level::Halted),
            _ => None,
        }
    }

    /// The stored form, 0 to 3.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        match self {
            Level::Normal => 0,
            Level::NoLiquidations => 1,
            Level::NoSystemStarts => 2,
            Level::Halted => 3,
        }
    }
}

/// The collateral feed as the gate sees it: valid, or stale / invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedCase {
    Live,
    Stale,
}

impl FeedCase {
    /// Both cases.
    pub const ALL: [FeedCase; 2] = [FeedCase::Live, FeedCase::Stale];
}

/// What an allowed operation does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Runs as usual.
    Run,
    /// Refused with the feed's own code (a liquidation on a stale feed).
    FeedRefusal,
    /// A bid on a stale feed: the D4 fallback decides (spec §3.2.3).
    Fallback,
}

/// The rule table of spec §3.2.2: whether `op` runs at `level` with the collateral feed `feed`.
///
/// # Errors
/// `LiquidationsPaused` for `Liquidate` at level 1 or more; `SystemAuctionsPaused` for a system
/// start at level 2 or more; `BidsPaused` for `Bid` and the system takes at level 3.
pub fn allowed(op: Op, level: Level, feed: FeedCase) -> Result<Gate, RiskError> {
    let halted = level == Level::Halted;
    match op {
        Op::Liquidate if level != Level::Normal => Err(RiskError::LiquidationsPaused),
        Op::Liquidate => Ok(match feed {
            FeedCase::Live => Gate::Run,
            FeedCase::Stale => Gate::FeedRefusal,
        }),
        Op::Bid if halted => Err(RiskError::BidsPaused),
        Op::Bid => Ok(match feed {
            FeedCase::Live => Gate::Run,
            FeedCase::Stale => Gate::Fallback,
        }),
        Op::StartSurplus | Op::StartDebt => match level {
            Level::Normal | Level::NoLiquidations => Ok(Gate::Run),
            Level::NoSystemStarts | Level::Halted => Err(RiskError::SystemAuctionsPaused),
        },
        Op::TakeSurplus | Op::TakeDebt if halted => Err(RiskError::BidsPaused),
        Op::TakeSurplus
        | Op::TakeDebt
        | Op::SettleEmpty
        | Op::ClaimLeftover
        | Op::SettleDebt
        | Op::CheckSolvency
        | Op::CloseSurplus
        | Op::CloseDebt
        | Op::ObserveFeed
        | Op::SetCollateral
        | Op::SetEngineParams
        | Op::SetSlack
        | Op::SetPauseLevel => Ok(Gate::Run),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Cell = Result<Gate, RiskError>;

    const RUN: Cell = Ok(Gate::Run);
    const FEED: Cell = Ok(Gate::FeedRefusal);
    const FALLBACK: Cell = Ok(Gate::Fallback);
    const LIQ: Cell = Err(RiskError::LiquidationsPaused);
    const SYS: Cell = Err(RiskError::SystemAuctionsPaused);
    const BIDS: Cell = Err(RiskError::BidsPaused);

    /// Spec §3.2.2, row by row: L0-L3 with a live feed, then L0-L3 with a stale feed.
    const TABLE: [(Op, [Cell; 8]); 17] = [
        (Op::Liquidate, [RUN, LIQ, LIQ, LIQ, FEED, LIQ, LIQ, LIQ]),
        (
            Op::Bid,
            [RUN, RUN, RUN, BIDS, FALLBACK, FALLBACK, FALLBACK, BIDS],
        ),
        (Op::StartSurplus, [RUN, RUN, SYS, SYS, RUN, RUN, SYS, SYS]),
        (Op::StartDebt, [RUN, RUN, SYS, SYS, RUN, RUN, SYS, SYS]),
        (Op::TakeSurplus, [RUN, RUN, RUN, BIDS, RUN, RUN, RUN, BIDS]),
        (Op::TakeDebt, [RUN, RUN, RUN, BIDS, RUN, RUN, RUN, BIDS]),
        (Op::SettleEmpty, [RUN; 8]),
        (Op::ClaimLeftover, [RUN; 8]),
        (Op::SettleDebt, [RUN; 8]),
        (Op::CheckSolvency, [RUN; 8]),
        (Op::CloseSurplus, [RUN; 8]),
        (Op::CloseDebt, [RUN; 8]),
        (Op::ObserveFeed, [RUN; 8]),
        (Op::SetCollateral, [RUN; 8]),
        (Op::SetEngineParams, [RUN; 8]),
        (Op::SetSlack, [RUN; 8]),
        (Op::SetPauseLevel, [RUN; 8]),
    ];

    /// Every `(level, feed)` in the column order of [`TABLE`].
    fn columns() -> impl Iterator<Item = (Level, FeedCase)> {
        FeedCase::ALL
            .into_iter()
            .flat_map(|feed| Level::ALL.into_iter().map(move |level| (level, feed)))
    }

    #[test]
    fn pause_table_is_exhaustive() {
        let ops: Vec<Op> = TABLE.iter().map(|(op, _)| *op).collect();
        assert_eq!(ops, Op::ALL, "one row per operation, in order");
        let mut cases = 0_u32;
        for (op, row) in TABLE {
            for ((level, feed), want) in columns().zip(row) {
                assert_eq!(allowed(op, level, feed), want, "{op:?} {level:?} {feed:?}");
                cases = cases.checked_add(1).expect("few cases");
            }
        }
        assert_eq!(cases, 136);
    }

    #[test]
    fn risk_reducing_ops_allowed_everywhere() {
        let reducing = [
            Op::SettleEmpty,
            Op::ClaimLeftover,
            Op::SettleDebt,
            Op::CheckSolvency,
            Op::CloseSurplus,
            Op::CloseDebt,
            Op::ObserveFeed,
        ];
        for op in reducing {
            for (level, feed) in columns() {
                assert_eq!(allowed(op, level, feed), Ok(Gate::Run), "{op:?} {level:?}");
            }
        }
    }

    #[test]
    fn level_round_trips_through_u8() {
        for level in Level::ALL {
            assert_eq!(Level::from_u8(level.as_u8()), Some(level));
        }
        let stored: Vec<u8> = Level::ALL.into_iter().map(Level::as_u8).collect();
        assert_eq!(stored, [0, 1, 2, 3]);
        assert_eq!(Level::from_u8(4), None);
        assert_eq!(Level::from_u8(u8::MAX), None);
    }
}
