//! `liquidate`: quote first, refuse a position the host would not let go, then one plan.

use crate::{
    chain::Chain,
    error::{SdkError, SdkResult},
    plan::{self, LiquidateArgs},
    quote,
    verify::{verify_binding, VerifiedConfig},
};

use super::{fact, is_fresh, Accounts, Prepared, Touch, CHECK_AUCTIONS};

/// The flags of `auction-cli liquidate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiquidateRequest {
    pub position: String,
    pub keeper: String,
    /// Collateral holding that receives the keeper reward (signs too if it is fresh).
    pub keeper_holding: String,
}

pub fn prepare_liquidate<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    req: &LiquidateRequest,
    accounts: &Accounts<'_>,
    now_ms: u64,
) -> SdkResult<Prepared> {
    verify_binding(chain, v)?;
    let position = accounts.id(&req.position)?;
    let keeper = accounts.id(&req.keeper)?;
    let holding = accounts.id(&req.keeper_holding)?;
    let q = quote::quote_liquidation(chain, v, &position, now_ms).map_err(|e| match e {
        SdkError::HostReject(m) => SdkError::Refused(format!(
            "refusing before signing: the host would reject this liquidation ({m})"
        )),
        other => other,
    })?;
    let p = plan::liquidate(
        v,
        &LiquidateArgs {
            keeper,
            keeper_collateral_holding: holding,
            keeper_holding_is_fresh: is_fresh(chain, &holding)?,
            position,
            nonce: q.nonce,
            oracle: v.coll(chain)?.oracle_id,
        },
    )?;
    Ok(Prepared::one(p, "liquidate", CHECK_AUCTIONS)
        .with_facts(vec![
            fact("nonce", q.nonce),
            fact("auction", q.auction),
            fact("keeper reward", q.quote.keeper_reward),
        ])
        .with_touched(vec![
            Touch::Auction(q.auction),
            Touch::Holding {
                label: "keeper".into(),
                id: holding,
                definition: v.state.collateral_definition_id,
            },
        ]))
}
