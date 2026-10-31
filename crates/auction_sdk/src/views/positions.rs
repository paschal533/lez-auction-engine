//! At-risk positions: [`quote::position_health`] (the host's own min-CR test) per scanned position.

use lee_core::account::AccountId;
use serde::Serialize;

use super::{list, Listed};
use crate::{
    chain::Chain,
    error::SdkResult,
    quote::{self, PositionHealth},
    scan::Cursor,
    verify::VerifiedConfig,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AtRiskView {
    pub position: AccountId,
    pub owner: AccountId,
    #[serde(with = "crate::dec")]
    pub collateral: u128,
    #[serde(with = "crate::dec")]
    pub nominal_debt: u128,
    pub cr_bps: u64,
    pub min_cr_bps: u64,
    /// The host would let it be liquidated now (`unsafe_at_min_cr`).
    pub liquidatable: bool,
}

/// A row for a position with debt; none without (nothing to collateralize).
fn row(h: &PositionHealth) -> Option<AtRiskView> {
    Some(AtRiskView {
        position: h.position,
        owner: h.owner,
        collateral: h.collateral,
        nominal_debt: h.nominal_debt,
        cr_bps: h.cr_bps?,
        min_cr_bps: h.min_cr_bps,
        liquidatable: h.unsafe_at_min_cr,
    })
}

/// Every scanned position with debt, in cursor order (the CLI's `list-at-risk` order).
pub fn positions<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    cursor: &Cursor,
    now_ms: u64,
) -> SdkResult<Listed<AtRiskView>> {
    list(cursor.position_ids()?, |id| {
        quote::position_health(chain, v, id, now_ms).map(|h| row(&h))
    })
}

/// [`positions`] sorted lowest CR first (the board's order); equal CRs keep cursor order.
pub fn at_risk<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    cursor: &Cursor,
    now_ms: u64,
) -> SdkResult<Listed<AtRiskView>> {
    let mut listed = positions(chain, v, cursor, now_ms)?;
    listed.items.sort_by_key(|p| p.cr_bps);
    Ok(listed)
}
