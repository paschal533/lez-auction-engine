//! Auction maintenance: `claim_leftover` and `settle_empty`.

use crate::{chain::Chain, error::SdkResult, plan, verify::VerifiedConfig};

use super::{is_fresh, Accounts, Prepared, Touch, CHECK_AUCTIONS};

/// The flags of `auction-cli claim-leftover`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimLeftoverRequest {
    pub owner: String,
    pub owner_holding: String,
    pub auction: String,
}

/// The flags of `auction-cli settle-empty`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettleEmptyRequest {
    pub caller: String,
    pub auction: String,
}

pub fn prepare_claim_leftover<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    req: &ClaimLeftoverRequest,
    accounts: &Accounts<'_>,
    _now_ms: u64,
) -> SdkResult<Prepared> {
    let holding = accounts.id(&req.owner_holding)?;
    let auction = accounts.id(&req.auction)?;
    let p = plan::claim_leftover(
        v,
        accounts.id(&req.owner)?,
        holding,
        is_fresh(chain, &holding)?,
        auction,
    )?;
    Ok(
        Prepared::one(p, "claim_leftover", CHECK_AUCTIONS).with_touched(vec![
            Touch::Auction(auction),
            Touch::Holding {
                label: "owner".into(),
                id: holding,
                definition: v.state.collateral_definition_id,
            },
        ]),
    )
}

pub fn prepare_settle_empty<C: Chain + ?Sized>(
    _chain: &C,
    v: &VerifiedConfig,
    req: &SettleEmptyRequest,
    accounts: &Accounts<'_>,
    _now_ms: u64,
) -> SdkResult<Prepared> {
    let auction = accounts.id(&req.auction)?;
    let p = plan::settle_empty(v, accounts.id(&req.caller)?, auction)?;
    Ok(Prepared::one(p, "settle_empty", CHECK_AUCTIONS)
        .with_touched(vec![Touch::Auction(auction), Touch::Accounting]))
}
