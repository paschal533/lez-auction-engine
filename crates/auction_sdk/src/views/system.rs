//! The System tab and the header: engine accounting, the host's parameters, the oracle.

use auction_core::{error::PauseReason, price::to_ray};
use auction_engine_core::EngineError;
use auction_engine_program::feed;
use lee_core::{account::AccountId, program::ProgramId};
use serde::Serialize;
use stablecoin_core::ProtocolParameters;

use crate::{
    addresses::HostAddresses, chain::Chain, decode::Reader, error::SdkResult, quote,
    verify::VerifiedConfig,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AccountingView {
    /// The engine's stablecoin buffer balance (0 while the holding is uninitialised).
    #[serde(with = "crate::dec")]
    pub buffer: u128,
    #[serde(with = "crate::dec")]
    pub unbacked_debt: u128,
    #[serde(with = "crate::dec")]
    pub debt_on_auction: u128,
    /// `bad_debt_freeze_threshold` of the config.
    #[serde(with = "crate::dec")]
    pub freeze_threshold: u128,
    pub surplus_live: bool,
    /// The nonce the next surplus auction will take.
    pub surplus_nonce: u64,
    pub debt_live: bool,
    pub debt_nonce: u64,
    /// Lot left of the live surplus auction, if one is live.
    #[serde(with = "crate::dec::opt")]
    pub surplus_lot_left: Option<u128>,
    #[serde(with = "crate::dec::opt")]
    pub debt_lot_left: Option<u128>,
    /// When the next debt auction may start (ADR-039), if one has ended before.
    pub earliest_debt_start_ms: Option<u64>,
}

pub fn accounting<C: Chain + ?Sized>(chain: &C, v: &VerifiedConfig) -> SdkResult<AccountingView> {
    let r = Reader::new(chain, v.engine.program);
    let a = r.accounting(&v.engine.accounting)?;
    let buffer = r
        .balance(
            &v.engine.buffer,
            &v.state.token_program_id,
            &v.state.stablecoin_definition_id,
        )?
        .unwrap_or(0);
    let surplus_lot_left = match (a.surplus_live, a.surplus_nonce.checked_sub(1)) {
        (true, Some(n)) => Some(r.surplus_auction(&v.engine.surplus_auction(n))?.lot_left),
        _ => None,
    };
    let debt_lot_left = match (a.debt_live, a.debt_nonce.checked_sub(1)) {
        (true, Some(n)) => Some(r.debt_auction(&v.engine.debt_auction(n))?.lot_left),
        _ => None,
    };
    Ok(AccountingView {
        buffer,
        unbacked_debt: a.unbacked_debt,
        debt_on_auction: a.debt_on_auction,
        freeze_threshold: v.state.bad_debt_freeze_threshold,
        surplus_live: a.surplus_live,
        surplus_nonce: a.surplus_nonce,
        debt_live: a.debt_live,
        debt_nonce: a.debt_nonce,
        surplus_lot_left,
        debt_lot_left,
        earliest_debt_start_ms: quote::earliest_debt_start(&a),
    })
}

/// The host's `ProtocolParameters` as `host status` shows them. `frozen` is the current state (the
/// engine's `host_frozen_by_engine` only means "has ever frozen").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostView {
    pub frozen: bool,
    pub admin: AccountId,
    pub freeze_authority: AccountId,
    pub market_oracle: AccountId,
    #[serde(with = "crate::dec")]
    pub min_cr_ray: u128,
    pub max_oracle_age_ms: u64,
}

impl From<&ProtocolParameters> for HostView {
    fn from(p: &ProtocolParameters) -> Self {
        Self {
            frozen: p.is_frozen,
            admin: p.admin_account_id,
            freeze_authority: p.freeze_authority_account_id,
            market_oracle: p.market_price_oracle_id,
            min_cr_ray: p.minimum_collateralization_ratio,
            max_oracle_age_ms: p.maximum_oracle_price_age_milliseconds,
        }
    }
}

pub fn host<C: Chain + ?Sized>(chain: &C, host: &HostAddresses) -> SdkResult<HostView> {
    // A host read never consults the engine program id the reader carries.
    let r = Reader::new(chain, ProgramId::default());
    let p: ProtocolParameters = r.host(
        &host.protocol_parameters,
        &host.program,
        "ProtocolParameters",
    )?;
    Ok(HostView::from(&p))
}

/// The engine's market oracle at `now_ms`, judged by the engine's own feed check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OracleView {
    /// The market price in RAY (the oracle's raw price at the collateral config's scale).
    #[serde(with = "crate::dec")]
    pub price_ray: u128,
    pub age_ms: u64,
    /// The collateral config's `max_oracle_age_ms` (the engine's limit, not the host's).
    pub max_age_ms: u64,
    /// Not stale by `auction_core::price::validate_feed` (a feed can be fresh and out of range).
    pub fresh: bool,
    /// The validity range `[R/2, 2R]` of `validate_feed`, at R now.
    #[serde(with = "crate::dec")]
    pub range_lo_ray: u128,
    #[serde(with = "crate::dec")]
    pub range_hi_ray: u128,
}

pub fn oracle<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    now_ms: u64,
) -> SdkResult<OracleView> {
    let r = Reader::new(chain, v.engine.program);
    let cfg = r.config(&v.config_id)?;
    let coll = r.coll_config(&v.coll_config_id())?;
    let o = r.oracle(&coll.oracle_id)?;
    let params = coll.feed_params()?;
    let redemption =
        feed::redemption_price(&cfg, &r.meta(&v.host.redemption_price_state)?, now_ms)?;
    let obs = feed::observe(&coll, &r.meta(&coll.oracle_id)?, redemption, now_ms)?;
    let price_ray = to_ray(o.price, params.scale).map_err(EngineError::from)?;
    Ok(OracleView {
        price_ray,
        // `validate_feed` measures age the same way: a timestamp ahead of the clock is age 0.
        age_ms: now_ms.saturating_sub(o.timestamp),
        max_age_ms: params.max_age_ms,
        fresh: !matches!(obs.feed, Err(PauseReason::Stale)),
        // The bounds `validate_feed` applies (`m < R / 2 || m > R.saturating_mul(2)` is out of
        // range); `auction_core` does not export them. Pinned to the engine by
        // `system_views_read_the_fixture`, which probes both ends through `check_new_price`.
        range_lo_ray: redemption / 2,
        range_hi_ray: redemption.saturating_mul(2),
    })
}
