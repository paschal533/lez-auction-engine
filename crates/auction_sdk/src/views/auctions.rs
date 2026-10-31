//! Live and Settled auctions: [`quote::auction_view`] (the bid path's observation) plus the auction's
//! own core state; the schedule figures come from `auction_core`'s `Schedule::discount_ray`.

use auction_core::auction::{settle_empty, Auction, Status};
use auction_engine_core::EngineError;
use lee_core::account::AccountId;
use serde::Serialize;

use super::{list, Listed};
use crate::{
    chain::Chain,
    error::SdkResult,
    quote::{self, AuctionView},
    scan::Cursor,
    verify::VerifiedConfig,
};

const MINUTE_MS: u64 = 60_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LiveAuctionView {
    pub id: AccountId,
    /// The lot put up for sale (`collateral_sold + collateral_left`, the core invariant).
    #[serde(with = "crate::dec")]
    pub lot: u128,
    #[serde(with = "crate::dec")]
    pub raised: u128,
    /// `amount_to_raise` (debt plus penalty).
    #[serde(with = "crate::dec")]
    pub target: u128,
    #[serde(with = "crate::dec")]
    pub collateral_left: u128,
    /// The cap reservation the auction holds until it settles.
    #[serde(with = "crate::dec")]
    pub reserved: u128,
    #[serde(with = "crate::dec")]
    pub discount_ray: u128,
    /// How much the discount grows over the next minute of effective time (0 once at the maximum).
    #[serde(with = "crate::dec")]
    pub rate_per_min_ray: u128,
    pub t_eff_ms: u64,
    /// Always `None`: the engine has no expiry (ADR-012). An unsold auction is held at the maximum
    /// discount (`held_at_max`) and keeps its reservation until a bid or `settle_empty` ends it.
    pub ends_at_ms: Option<u64>,
    /// Clock time the discount reaches its maximum, assuming no pause from now on.
    pub max_at_ms: u64,
    pub held_at_max: bool,
    /// Why a bid would revert now (`Paused`, `PriceFeedStale`, ...), if it would.
    pub paused: Option<String>,
    /// Included bids the scan has seen.
    pub bids: u32,
    pub position: AccountId,
    pub started_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SettledAuctionView {
    pub id: AccountId,
    #[serde(with = "crate::dec")]
    pub lot: u128,
    #[serde(with = "crate::dec")]
    pub sold: u128,
    /// `debt_repaid + penalty`.
    #[serde(with = "crate::dec")]
    pub raised: u128,
    #[serde(with = "crate::dec")]
    pub target: u128,
    #[serde(with = "crate::dec")]
    pub debt_repaid: u128,
    #[serde(with = "crate::dec")]
    pub penalty: u128,
    /// Debt left unrepaid, booked as unbacked debt when the auction ended.
    #[serde(with = "crate::dec")]
    pub shortfall: u128,
    /// Collateral claimable by the position owner (`leftover_claimable`).
    #[serde(with = "crate::dec")]
    pub leftover: u128,
    #[serde(with = "crate::dec")]
    pub claimed: u128,
    pub owner: AccountId,
    pub position: AccountId,
    /// Always `None` for now: the engine does not record when an auction ended, and the scan does
    /// not record the block of the ending transaction.
    pub ended_at_ms: Option<u64>,
    pub bids: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuctionEntry {
    Live(LiveAuctionView),
    Settled(SettledAuctionView),
}

/// The shortfall `auction_core` books when an auction ends (`ChainEnd.shortfall`, the rule
/// `apply_bid` and `settle_empty` share), evaluated through `settle_empty` on the final figures so
/// that the rule is not restated here. Its preconditions (auctioning, empty lot) hold by
/// construction; should `settle_empty` ever refuse anyway, the error is returned, never a 0.
fn shortfall(core: &Auction) -> SdkResult<u128> {
    let ending = Auction {
        status: Status::Auctioning,
        collateral_left: 0,
        ..*core
    };
    let (_, end) = settle_empty(&ending).map_err(EngineError::from)?;
    Ok(end.shortfall)
}

fn entry(view: &AuctionView, bids: u32, now_ms: u64) -> SdkResult<AuctionEntry> {
    let s = &view.state;
    let core = s.to_core()?;
    let lot = core.collateral_sold.saturating_add(core.collateral_left);
    if s.settled {
        return Ok(AuctionEntry::Settled(SettledAuctionView {
            id: view.id,
            lot,
            sold: core.collateral_sold,
            raised: core.raised(),
            target: core.amount_to_raise,
            debt_repaid: core.debt_raised,
            penalty: core.penalty_raised,
            shortfall: shortfall(&core)?,
            leftover: core.leftover_claimable,
            claimed: core.leftover_claimed,
            owner: s.owner,
            position: s.position,
            ended_at_ms: None,
            bids,
        }));
    }
    let last = core.schedule.points().last().map_or(0, |p| p.elapsed_ms);
    let next_minute = core
        .schedule
        .discount_ray(view.t_eff_ms.saturating_add(MINUTE_MS));
    Ok(AuctionEntry::Live(LiveAuctionView {
        id: view.id,
        lot,
        raised: core.raised(),
        target: core.amount_to_raise,
        collateral_left: core.collateral_left,
        reserved: core.reserved_amount,
        discount_ray: view.discount_ray,
        rate_per_min_ray: next_minute.saturating_sub(view.discount_ray),
        t_eff_ms: view.t_eff_ms,
        ends_at_ms: None,
        max_at_ms: now_ms.saturating_add(last).saturating_sub(view.t_eff_ms),
        held_at_max: view.held_at_max,
        paused: view.paused.clone(),
        bids,
        position: s.position,
        started_at_ms: s.start_ms,
    }))
}

/// One auction's view at `now_ms`; `bids` from the scan cursor.
pub fn auction_entry<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    id: &AccountId,
    bids: u32,
    now_ms: u64,
) -> SdkResult<AuctionEntry> {
    entry(&quote::auction_view(chain, v, id, now_ms)?, bids, now_ms)
}

/// Every auction the cursor knows, live and settled, in cursor order. Bid counts come from the
/// cursor (see `Cursor::bids`: 0 for a pre-Task-8 cursor until `rescan --from 1`).
pub fn auctions<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    cursor: &Cursor,
    now_ms: u64,
) -> SdkResult<Listed<AuctionEntry>> {
    list(cursor.auction_ids()?, |id| {
        let bids = cursor.bids.get(&id.to_string()).copied().unwrap_or(0);
        auction_entry(chain, v, id, bids, now_ms).map(Some)
    })
}
