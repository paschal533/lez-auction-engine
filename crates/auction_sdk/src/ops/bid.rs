//! `bid`: quote, derive (or re-check) the slippage bound, pick `bid` or `bid_with_settlement` (ADR-029).

use crate::{
    chain::Chain,
    error::SdkResult,
    plan::{self, BidArgs},
    quote,
    verify::VerifiedConfig,
};

use super::{fact, is_fresh, Accounts, Prepared, Touch, CHECK_AUCTIONS};

/// The flags of `auction-cli bid`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BidRequest {
    pub auction: String,
    pub coin_in_max: u128,
    pub bidder_stable: String,
    pub bidder_collateral: String,
    pub slippage_bps: u32,
    /// Explicit minimum collateral out (replaces the bound derived from `slippage_bps`; the choice of
    /// bid vs bid_with_settlement still uses `slippage_bps` for its near-empty margin).
    pub min_out: Option<u128>,
}

pub fn prepare_bid<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    req: &BidRequest,
    accounts: &Accounts<'_>,
    now_ms: u64,
) -> SdkResult<Prepared> {
    let auction = accounts.id(&req.auction)?;
    let stable = accounts.id(&req.bidder_stable)?;
    let collateral = accounts.id(&req.bidder_collateral)?;
    let q = quote::quote_bid(chain, v, &auction, req.coin_in_max, 0, now_ms)?;
    let min_out = req
        .min_out
        .unwrap_or_else(|| quote::min_out(q.fill.collateral_out, req.slippage_bps));
    if req.min_out.is_some() {
        // The engine's own check, run now: a bound above the quote would revert (Slippage).
        quote::quote_bid(chain, v, &auction, req.coin_in_max, min_out, now_ms)?;
    }
    let with_settlement = q.needs_settlement(req.slippage_bps);
    let p = plan::bid(
        v,
        &BidArgs {
            bidder_stable: stable,
            bidder_collateral: collateral,
            bidder_collateral_is_fresh: is_fresh(chain, &collateral)?,
            auction,
            coin_in_max: req.coin_in_max,
            min_collateral_out: min_out,
            with_settlement,
            oracle: v.coll(chain)?.oracle_id,
        },
    )?;
    let what = if with_settlement {
        "bid_with_settlement"
    } else {
        "bid"
    };
    Ok(Prepared::one(p, what, CHECK_AUCTIONS)
        .with_facts(vec![
            fact("quoted out", q.fill.collateral_out),
            fact("min out", min_out),
        ])
        .with_touched(vec![
            Touch::Auction(auction),
            Touch::Holding {
                label: "bidder coll".into(),
                id: collateral,
                definition: v.state.collateral_definition_id,
            },
            Touch::Holding {
                label: "bidder stable".into(),
                id: stable,
                definition: v.state.stablecoin_definition_id,
            },
        ]))
}
