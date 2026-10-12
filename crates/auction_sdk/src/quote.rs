//! Quotes (spec §6.1): the program's own pricing path, evaluated on chain state at a chosen `now`.
//! Each quote calls the same `auction_engine_program::feed` and `auction_core` functions, in the same
//! order, as its handler (`liquidate.rs`, `bid.rs`, `surplus.rs`, `debt.rs`), and simulates
//! `observed()` before any `t_eff` (ADR-024). A quote is a pure function of (chain state, `now`).
//! `programs/integration_tests/tests/sdk_quotes.rs` proves quote == on-chain result on the real guests.
//!
//! `now_ms` must be the `CLOCK_01` value the transaction will execute under (`chain::clock_ms`: the
//! previous block's timestamp), never wall-clock time.
//!
//! Quotes predict pricing and the engine/host state checks, NOT the caller's balances or recipient
//! shape. Reverts a quote does not predict: takes: `InsufficientBalance`, `VaultBelowBook`,
//! `RecipientMustSign`; bid: `VaultBelowBook`, `InsufficientBalance`, `RecipientMustSign`, and
//! `ChainEndAccountsMissing` (avoided by listing `Accounting` per `BidQuote::needs_settlement`).

use alloy_primitives::U512;
use auction_core::{
    auction::{quote_bid as core_quote_bid, BidFill, BidRequest},
    liquidation::{
        quote_liquidation as core_quote_liquidation, CapState, LiquidationInput, LiquidationQuote,
    },
    math::{mul_div_ceil, mul_div_floor},
    price::bid_price,
    system::{take_debt as core_take_debt, take_surplus as core_take_surplus},
    units::{BPS, RAY},
};
use auction_engine_core::{
    args::DEBT_RELIST_DELAY_MS,
    state::{AccountingState, AuctionState, EngineConfigState},
    EngineError,
};
use auction_engine_program::{feed, system};
use lee_core::account::AccountId;
use stablecoin_core::{
    math::{compute_current_accumulated_rate_wide, compute_current_redemption_price_wide},
    Position, ProtocolParameters, RedemptionPriceState, StabilityFeeAccumulator,
};
use stablecoin_program::checks::is_position_collateralized;

use crate::{
    chain::Chain,
    decode::Reader,
    error::{SdkError, SdkResult},
    verify::VerifiedConfig,
};

fn eng<E: Into<EngineError>>(e: E) -> SdkError {
    SdkError::from(e.into())
}

/// The host-side inputs the handler loads before touching the oracle (`liquidate::load_host`).
struct HostPricing {
    params: ProtocolParameters,
    rate_wide: U512,
}

fn load_host_pricing<C: Chain + ?Sized>(
    r: &Reader<'_, C>,
    v: &VerifiedConfig,
    now_ms: u64,
) -> SdkResult<HostPricing> {
    let host = v.host.program;
    let params: ProtocolParameters =
        r.host(&v.host.protocol_parameters, &host, "ProtocolParameters")?;
    let acc: StabilityFeeAccumulator = r.host(
        &v.host.stability_fee_accumulator,
        &host,
        "StabilityFeeAccumulator",
    )?;
    let rate_wide = compute_current_accumulated_rate_wide(
        acc.accumulated_rate_at_last_accrual,
        params.stability_fee_per_millisecond,
        acc.last_accrued_at,
        now_ms,
    );
    Ok(HostPricing { params, rate_wide })
}

fn load_position<C: Chain + ?Sized>(
    r: &Reader<'_, C>,
    v: &VerifiedConfig,
    position: &AccountId,
) -> SdkResult<Position> {
    let pos: Position = r.host(position, &v.host.program, "Position")?;
    if *position != v.host.position(&pos.owner_account_id, pos.position_nonce) {
        return Err(EngineError::WrongAccount("position").into());
    }
    if pos.vault_account_id != v.host.position_vault(position) {
        return Err(EngineError::WrongAccount("host_vault").into());
    }
    Ok(pos)
}

/// Nominal debt, owed -> rounded up (`liquidate.rs`).
fn nominal_debt(pos: &Position, rate_wide: U512) -> SdkResult<u128> {
    let rate = feed::u512_to_u128(rate_wide)?;
    mul_div_ceil(pos.normalized_debt_amount, rate, RAY).map_err(eng)
}

/// ProtocolParameters' market oracle must be the collateral config's oracle (ADR-028).
fn check_oracle_binding(
    coll_oracle: &AccountId,
    params: &ProtocolParameters,
) -> Result<(), EngineError> {
    if *coll_oracle == params.market_price_oracle_id {
        Ok(())
    } else {
        Err(EngineError::OracleMismatch)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionHealth {
    pub position: AccountId,
    pub owner: AccountId,
    pub position_nonce: u64,
    pub collateral: u128,
    pub normalized_debt: u128,
    pub nominal_debt: u128,
    /// The host's own test (`is_position_collateralized` at `minimum_collateralization_ratio`, R at now).
    pub unsafe_at_min_cr: bool,
}

fn health_of(
    position: &AccountId,
    pos: &Position,
    host: &HostPricing,
    redemption_wide: U512,
) -> SdkResult<PositionHealth> {
    Ok(PositionHealth {
        position: *position,
        owner: pos.owner_account_id,
        position_nonce: pos.position_nonce,
        collateral: pos.collateral_amount,
        normalized_debt: pos.normalized_debt_amount,
        nominal_debt: nominal_debt(pos, host.rate_wide)?,
        unsafe_at_min_cr: !is_position_collateralized(
            pos,
            host.rate_wide,
            redemption_wide,
            host.params.minimum_collateralization_ratio,
        ),
    })
}

fn redemption_wide<C: Chain + ?Sized>(
    r: &Reader<'_, C>,
    v: &VerifiedConfig,
    now_ms: u64,
) -> SdkResult<U512> {
    let rps: RedemptionPriceState = r.host(
        &v.host.redemption_price_state,
        &v.host.program,
        "RedemptionPriceState",
    )?;
    Ok(compute_current_redemption_price_wide(
        rps.redemption_price_at_last_update,
        rps.redemption_rate_per_millisecond,
        rps.last_updated_at,
        now_ms,
    ))
}

pub fn position_health<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    position: &AccountId,
    now_ms: u64,
) -> SdkResult<PositionHealth> {
    let r = Reader::new(chain, v.engine.program);
    let host = load_host_pricing(&r, v, now_ms)?;
    let pos = load_position(&r, v, position)?;
    health_of(position, &pos, &host, redemption_wide(&r, v, now_ms)?)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiquidationQuoteView {
    pub health: PositionHealth,
    /// `LiquidationRecord.count`: the nonce `liquidate` must carry (ADR-004).
    pub nonce: u64,
    pub auction: AccountId,
    pub now_ms: u64,
    pub vault_balance: u128,
    pub redemption_ray: u128,
    pub market_ray: u128,
    pub price_ray: u128,
    pub quote: LiquidationQuote,
}

/// Mirrors `liquidate.rs` step for step (see the Task 6 report for the table).
pub fn quote_liquidation<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    position: &AccountId,
    now_ms: u64,
) -> SdkResult<LiquidationQuoteView> {
    let r = Reader::new(chain, v.engine.program);
    let cfg = r.config(&v.config_id)?;
    let coll = r.coll_config(&v.coll_config_id())?;
    let host = load_host_pricing(&r, v, now_ms)?;
    check_oracle_binding(&coll.oracle_id, &host.params)?;
    let redemption =
        feed::redemption_price(&cfg, &r.meta(&v.host.redemption_price_state)?, now_ms)?;
    let obs = feed::observe(&coll, &r.meta(&coll.oracle_id)?, redemption, now_ms)?;
    let market = feed::require_live(&coll, &obs)?;

    let pos = load_position(&r, v, position)?;
    let vault_balance = r
        .balance(
            &v.host.position_vault(position),
            &cfg.token_program_id,
            &cfg.collateral_definition_id,
        )?
        .ok_or(EngineError::WrongDefinition)?;
    let health = health_of(position, &pos, &host, redemption_wide(&r, v, now_ms)?)?;
    let price = bid_price(market, redemption, &coll.feed_params()?);
    let quote = core_quote_liquidation(
        &LiquidationInput {
            position_collateral: pos.collateral_amount,
            vault_balance,
            nominal_debt: health.nominal_debt,
            price_ray: price,
        },
        &coll.collateral_params()?,
        &CapState {
            type_cap: coll.type_cap,
            type_reserved: coll.type_reserved,
            global_cap: cfg.global_cap,
            global_reserved: cfg.global_reserved,
        },
    )
    .map_err(eng)?;
    // Engine errors above take precedence, as on-chain. Then the host hook (`liquidate_position`
    // in programs/stablecoin/src/liquidation_hooks.rs) asserts the position is not collateralized
    // and the redemption price is non-zero; `health_of` is its exact predicate.
    if redemption_wide(&r, v, now_ms)?.is_zero() {
        return Err(SdkError::HostReject(
            "redemption price projected to zero (host hook)".into(),
        ));
    }
    if !health.unsafe_at_min_cr {
        return Err(SdkError::HostReject(
            "position is collateralized, not liquidatable (host hook)".into(),
        ));
    }
    let nonce = r.record_count(&v.engine.record(position))?;
    Ok(LiquidationQuoteView {
        health,
        nonce,
        auction: v.engine.auction(position, nonce),
        now_ms,
        vault_balance,
        redemption_ray: redemption,
        market_ray: market,
        price_ray: price,
        quote,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BidQuote {
    pub auction: AccountId,
    pub now_ms: u64,
    pub t_eff_ms: u64,
    pub redemption_ray: u128,
    pub price_ray: u128,
    pub remaining_target: u128,
    pub collateral_left: u128,
    pub coin_in_max: u128,
    pub fill: BidFill,
}

impl BidQuote {
    /// ADR-029: list `Accounting` (`bid_with_settlement`) when the fill ends the auction, when
    /// `coin_in_max` covers the remaining target, or when the fill is within the slippage margin of
    /// emptying the lot (a later price or discount could make it final in flight).
    #[must_use]
    pub fn needs_settlement(&self, slippage_bps: u32) -> bool {
        let near_empty = match mul_div_ceil(
            self.fill.collateral_out,
            BPS.saturating_add(u128::from(slippage_bps)),
            BPS,
        ) {
            Ok(margin) => margin >= self.collateral_left,
            Err(_) => true,
        };
        self.fill.ends_auction || self.coin_in_max >= self.remaining_target || near_empty
    }
}

/// `load_auction` of the handlers: owner and tag (Reader), config, and the PDA of (position, nonce).
fn load_auction<C: Chain + ?Sized>(
    r: &Reader<'_, C>,
    v: &VerifiedConfig,
    id: &AccountId,
) -> SdkResult<AuctionState> {
    let st = r.auction(id)?;
    if st.config != v.config_id || *id != v.engine.auction(&st.position, st.nonce) {
        return Err(EngineError::WrongAccount("auction").into());
    }
    Ok(st)
}

/// Mirrors `bid.rs`: observation precedes every `t_eff` use (ADR-024).
pub fn quote_bid<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    auction: &AccountId,
    coin_in_max: u128,
    min_collateral_out: u128,
    now_ms: u64,
) -> SdkResult<BidQuote> {
    let r = Reader::new(chain, v.engine.program);
    let cfg = r.config(&v.config_id)?;
    let coll = r.coll_config(&v.coll_config_id())?;
    let st = load_auction(&r, v, auction)?;
    let current = st.to_core()?;

    let redemption =
        feed::redemption_price(&cfg, &r.meta(&v.host.redemption_price_state)?, now_ms)?;
    let obs = feed::observe(&coll, &r.meta(&coll.oracle_id)?, redemption, now_ms)?;
    let market = feed::require_live(&coll, &obs)?;
    let params = coll.collateral_params()?;
    let price = bid_price(market, redemption, &coll.feed_params()?);
    let t_eff =
        obs.clock
            .effective_elapsed(current.start_ms, current.type_paused_total_at_start, now_ms);
    let fill = core_quote_bid(
        &current,
        &BidRequest {
            coin_in_max,
            min_collateral_out,
        },
        price,
        t_eff,
        params.min_bid,
        params.chost().map_err(eng)?,
    )
    .map_err(eng)?;
    Ok(BidQuote {
        auction: *auction,
        now_ms,
        t_eff_ms: t_eff,
        redemption_ray: redemption,
        price_ray: price,
        remaining_target: current.remaining_target(),
        collateral_left: current.collateral_left,
        coin_in_max,
        fill,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuctionView {
    pub id: AccountId,
    pub state: AuctionState,
    pub t_eff_ms: u64,
    pub discount_ray: u128,
    /// Past the schedule's last point: unsold, held at the maximum discount, still holding its cap
    /// reservation (no expiry, ADR-012; THREAT_MODEL client obligation).
    pub held_at_max: bool,
    /// Why a bid would revert now (`Paused`, `PriceFeedStale`, ...), if it would.
    pub paused: Option<String>,
}

/// Same observation as `quote_bid`, but a paused or invalid feed is reported, not an error.
pub fn auction_view<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    auction: &AccountId,
    now_ms: u64,
) -> SdkResult<AuctionView> {
    let r = Reader::new(chain, v.engine.program);
    let cfg = r.config(&v.config_id)?;
    let coll = r.coll_config(&v.coll_config_id())?;
    let st = load_auction(&r, v, auction)?;
    let current = st.to_core()?;
    let redemption =
        feed::redemption_price(&cfg, &r.meta(&v.host.redemption_price_state)?, now_ms)?;
    let obs = feed::observe(&coll, &r.meta(&coll.oracle_id)?, redemption, now_ms)?;
    let paused = feed::require_live(&coll, &obs).err().map(|e| e.to_string());
    let t_eff =
        obs.clock
            .effective_elapsed(current.start_ms, current.type_paused_total_at_start, now_ms);
    let last_point = current.schedule.points().last().map_or(0, |p| p.elapsed_ms);
    Ok(AuctionView {
        id: *auction,
        discount_ray: current.schedule.discount_ray(t_eff),
        held_at_max: !st.settled && t_eff >= last_point,
        t_eff_ms: t_eff,
        paused,
        state: st,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemTakeQuote {
    pub nonce: u64,
    pub now_ms: u64,
    /// Coin actually filled (capped at the lot).
    pub filled: u128,
    /// Surplus: protocol tokens the taker pays (rounded up). Debt: protocol tokens minted (rounded down).
    pub tokens: u128,
    pub lot_left_after: u128,
}

/// The `protocol_token_status` gate both system takes apply before pricing.
fn check_protocol_token<C: Chain + ?Sized>(
    r: &Reader<'_, C>,
    v: &VerifiedConfig,
    cfg: &EngineConfigState,
    need_mint_authority: bool,
) -> SdkResult<()> {
    let slot = cfg.protocol_token.unwrap_or(v.engine.unused_protocol_token);
    let status =
        system::protocol_token_status(&v.engine.program, &v.config_id, cfg, &r.meta(&slot)?)?;
    if cfg.protocol_token.is_none() {
        return Err(EngineError::TokenNotConfigured.into());
    }
    if !status.token_ok {
        return Err(EngineError::ProtocolTokenMismatch.into());
    }
    if need_mint_authority && !status.mint_authority_ok {
        return Err(EngineError::MintAuthorityMissing.into());
    }
    Ok(())
}

/// Mirrors `surplus.rs::take_surplus`: the snapshotted `ref_ray` and the config's `SystemParams`;
/// `max_protocol_in` is unbounded because the quote reports the price, not a slippage check.
pub fn quote_take_surplus<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    nonce: u64,
    amount_coin: u128,
    now_ms: u64,
) -> SdkResult<SystemTakeQuote> {
    let r = Reader::new(chain, v.engine.program);
    let cfg = r.config(&v.config_id)?;
    let sa = r.surplus_auction(&v.engine.surplus_auction(nonce))?;
    if sa.config != v.config_id || sa.nonce != nonce {
        return Err(EngineError::WrongAccount("surplus_auction").into());
    }
    check_protocol_token(&r, v, &cfg, false)?;
    let (next, tokens) = core_take_surplus(
        &sa.core(),
        amount_coin,
        u128::MAX,
        sa.ref_ray,
        &cfg.system.to_core()?,
        now_ms,
    )
    .map_err(eng)?;
    let filled = sa
        .lot_left
        .checked_sub(next.lot_left)
        .ok_or(EngineError::Math)?;
    Ok(SystemTakeQuote {
        nonce,
        now_ms,
        filled,
        tokens,
        lot_left_after: next.lot_left,
    })
}

/// Mirrors `debt.rs::take_debt`: the snapshotted `start_amount` and the config's `SystemParams`;
/// `min_protocol_out` is 0 because the quote reports the price, not a slippage check.
pub fn quote_take_debt<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    nonce: u64,
    amount_coin: u128,
    now_ms: u64,
) -> SdkResult<SystemTakeQuote> {
    let r = Reader::new(chain, v.engine.program);
    let cfg = r.config(&v.config_id)?;
    let acc = r.accounting(&v.engine.accounting)?;
    let da = r.debt_auction(&v.engine.debt_auction(nonce))?;
    if da.config != v.config_id || da.nonce != nonce {
        return Err(EngineError::WrongAccount("debt_auction").into());
    }
    check_protocol_token(&r, v, &cfg, true)?;
    let (next, minted) =
        core_take_debt(&da.core(), amount_coin, 0, &cfg.system.to_core()?, now_ms).map_err(eng)?;
    let filled = da
        .lot_left
        .checked_sub(next.lot_left)
        .ok_or(EngineError::Math)?;
    // The reservation can only be released by what was reserved (ADR-024 pairing), as in the handler.
    if filled > acc.debt_on_auction {
        return Err(EngineError::Math.into());
    }
    Ok(SystemTakeQuote {
        nonce,
        now_ms,
        filled,
        tokens: minted,
        lot_left_after: next.lot_left,
    })
}

/// Minimum acceptable output after `slippage_bps` (rounded down).
#[must_use]
pub fn min_out(amount: u128, slippage_bps: u32) -> u128 {
    let keep = BPS.saturating_sub(u128::from(slippage_bps));
    mul_div_floor(amount, keep, BPS).unwrap_or(0)
}

/// Maximum acceptable input after `slippage_bps` (rounded up).
#[must_use]
pub fn max_in(amount: u128, slippage_bps: u32) -> u128 {
    mul_div_ceil(amount, BPS.saturating_add(u128::from(slippage_bps)), BPS).unwrap_or(u128::MAX)
}

/// When the next debt auction may start (ADR-039), if one has ended before.
#[must_use]
pub fn earliest_debt_start(acc: &AccountingState) -> Option<u64> {
    (acc.last_debt_end_ms > 0).then(|| acc.last_debt_end_ms.saturating_add(DEBT_RELIST_DELAY_MS))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SdkError;
    use crate::testing::standard::{self, oracle_state, ORACLE, ORACLE_OWNER, R, T0};
    use crate::verify::verify_config;
    use auction_core::units::bps_to_ray;
    use stablecoin_core::Position;

    fn setup() -> (standard::Fixture, VerifiedConfig) {
        let fx = standard::standard();
        let v = verify_config(&fx.chain, &fx.deployment, &standard::config_id()).expect("verifies");
        (fx, v)
    }

    #[test]
    fn liquidation_quote_matches_the_hand_computed_standard_position() {
        let (fx, v) = setup();
        let q = quote_liquidation(&fx.chain, &v, &fx.unsafe_position, T0).expect("quote");
        assert!(q.health.unsafe_at_min_cr);
        assert_eq!(q.health.nominal_debt, 600_000);
        assert_eq!(
            (
                q.quote.amount_to_raise,
                q.quote.keeper_reward,
                q.quote.lot,
                q.quote.seized
            ),
            (660_000, 25, 499_975, 500_000)
        );
        assert_eq!((q.nonce, q.price_ray), (0, R));
        assert_eq!(q.auction, v.engine.auction(&fx.unsafe_position, 0));
        let next = quote_liquidation(&fx.chain, &v, &fx.auction_position, T0)
            .expect_err("a zero-debt position is not liquidatable");
        assert!(matches!(next, SdkError::Engine { ref name, .. } if name == "ZeroDebt"));
    }

    #[test]
    fn liquidating_a_safe_position_is_the_host_rejection() {
        let (fx, v) = setup();
        let mut pos: Position = Reader::new(&fx.chain, standard::ENGINE)
            .host(&fx.unsafe_position, &standard::HOST, "Position")
            .expect("position");
        pos.normalized_debt_amount = 100_000;
        fx.chain.put_state(fx.unsafe_position, standard::HOST, &pos);
        let h = position_health(&fx.chain, &v, &fx.unsafe_position, T0).expect("health displays");
        assert!(!h.unsafe_at_min_cr);
        let e = quote_liquidation(&fx.chain, &v, &fx.unsafe_position, T0).expect_err("safe");
        assert!(matches!(e, SdkError::HostReject(ref m) if m.contains("collateralized")));
    }

    #[test]
    fn system_take_token_checks_follow_the_handler_order() {
        use auction_engine_core::state::EngineConfigState;
        use token_core::TokenDefinition;
        let name = |e: SdkError| match e {
            SdkError::Engine { name, .. } => name,
            other => panic!("unexpected {other}"),
        };
        // Not configured.
        let (fx, v) = setup();
        let r = Reader::new(&fx.chain, standard::ENGINE);
        let mut cfg: EngineConfigState = r.config(&v.config_id).expect("cfg");
        cfg.protocol_token = None;
        fx.chain.put_state(v.config_id, standard::ENGINE, &cfg);
        let s = quote_take_surplus(&fx.chain, &v, 0, 500, T0).expect_err("unset");
        let d = quote_take_debt(&fx.chain, &v, 0, 500, T0).expect_err("unset");
        assert_eq!(
            (name(s), name(d)),
            ("TokenNotConfigured".into(), "TokenNotConfigured".into())
        );
        // Definition owned by the wrong program.
        let (fx, v) = setup();
        let def = fx.chain.account(&standard::PROTOCOL).expect("def");
        fx.chain.put(
            standard::PROTOCOL,
            lee_core::account::Account {
                program_owner: [7; 8],
                ..def
            },
        );
        let s = quote_take_surplus(&fx.chain, &v, 0, 500, T0).expect_err("owner");
        let d = quote_take_debt(&fx.chain, &v, 0, 500, T0).expect_err("owner");
        assert_eq!(
            (name(s), name(d)),
            (
                "ProtocolTokenMismatch".into(),
                "ProtocolTokenMismatch".into()
            )
        );
        // No engine mint authority: the surplus take does not need it, the debt take does.
        let (fx, v) = setup();
        fx.chain.put_state(
            standard::PROTOCOL,
            standard::TOKEN,
            &TokenDefinition::Fungible {
                name: "MOCK".into(),
                total_supply: 10_000_000,
                metadata_id: None,
                authority: None,
            },
        );
        quote_take_surplus(&fx.chain, &v, 0, 500, T0).expect("surplus needs no mint authority");
        let d = quote_take_debt(&fx.chain, &v, 0, 500, T0).expect_err("authority");
        assert_eq!(name(d), "MintAuthorityMissing");
    }

    #[test]
    fn bid_quote_uses_the_auction_schedule_at_t_eff() {
        let (fx, v) = setup();
        let q = quote_bid(&fx.chain, &v, &fx.live_auction, 100_000, 0, T0).expect("quote");
        assert_eq!(
            (q.t_eff_ms, q.price_ray, q.remaining_target),
            (600_000, R, 660_000)
        );
        let expected_d =
            bps_to_ray(200) + (bps_to_ray(1_000) - bps_to_ray(200)) * 600_000 / 2_700_000;
        assert_eq!(q.fill.discount_ray, expected_d);
        assert_eq!((q.fill.charged, q.fill.collateral_out), (100_000, 51_963));
        assert!(!q.fill.ends_auction);
        assert!(!q.needs_settlement(50), "a partial bid lists no Accounting");
        let all = quote_bid(&fx.chain, &v, &fx.live_auction, u128::MAX, 0, T0).expect("quote");
        assert!(all.fill.ends_auction && all.needs_settlement(50));
    }

    #[test]
    fn bid_quote_simulates_observed_before_t_eff() {
        // ADR-024: an unobserved stale gap (oracle last seen 5_000_000 ms ago, max age 3_600_000) is
        // folded into the pause clock first, which freezes the discount; a naive t_eff would be 600_000.
        let (fx, v) = setup();
        let mut coll = v.coll(&fx.chain).expect("coll");
        coll.last_seen_oracle_ts = Some(T0 - 5_000_000);
        fx.chain
            .put_state(v.coll_config_id(), standard::ENGINE, &coll);
        let q = quote_bid(&fx.chain, &v, &fx.live_auction, 100_000, 0, T0).expect("quote");
        assert_eq!(q.t_eff_ms, 0);
        assert_eq!(q.fill.discount_ray, bps_to_ray(200));
        assert_eq!(q.fill.collateral_out, 51_020);
    }

    #[test]
    fn a_stale_feed_is_the_engine_error_and_the_view_reports_the_pause() {
        let (fx, v) = setup();
        fx.chain.put_state(
            ORACLE,
            ORACLE_OWNER,
            &oracle_state(R, T0 - 3_600_001, &v.host),
        );
        let e = quote_bid(&fx.chain, &v, &fx.live_auction, 100_000, 0, T0).expect_err("stale");
        assert!(matches!(e, SdkError::Engine { ref name, .. } if name == "PriceFeedStale"));
        let view = auction_view(&fx.chain, &v, &fx.live_auction, T0)
            .expect("view never fails on a bad feed");
        assert_eq!(view.paused.as_deref(), Some("PriceFeedStale"));
    }

    #[test]
    fn auction_view_flags_an_auction_held_at_max_discount() {
        let (fx, v) = setup();
        let at_t0 = auction_view(&fx.chain, &v, &fx.live_auction, T0).expect("view");
        assert!(!at_t0.held_at_max && at_t0.paused.is_none());
        let later = T0 + 2_100_000; // t_eff = 2_700_000, the schedule's last point
        fx.chain
            .put_state(ORACLE, ORACLE_OWNER, &oracle_state(R, later, &v.host));
        let view = auction_view(&fx.chain, &v, &fx.live_auction, later).expect("view");
        assert!(view.held_at_max);
        assert_eq!(view.discount_ray, bps_to_ray(1_000));
    }

    #[test]
    fn system_take_quotes_use_the_snapshot_and_the_schedule() {
        let (fx, v) = setup();
        let s = quote_take_surplus(&fx.chain, &v, 0, 500, T0).expect("surplus quote");
        assert_eq!((s.filled, s.tokens, s.lot_left_after), (500, 200, 0));
        let d = quote_take_debt(&fx.chain, &v, 0, 500, T0).expect("debt quote");
        assert_eq!((d.filled, d.tokens, d.lot_left_after), (500, 325, 0));
        let e = quote_take_debt(&fx.chain, &v, 0, 495, T0).expect_err("dust remainder");
        assert!(matches!(e, SdkError::Engine { ref name, .. } if name == "DustRemainder"));
    }

    #[test]
    fn slippage_helpers_round_in_the_protocol_direction() {
        assert_eq!(min_out(10_000, 50), 9_950);
        assert_eq!(min_out(10_001, 50), 9_950);
        assert_eq!(max_in(200, 50), 201);
        assert_eq!(min_out(5, 20_000), 0);
        let acc = AccountingState {
            last_debt_end_ms: 100,
            ..AccountingState::new()
        };
        assert_eq!(earliest_debt_start(&acc), Some(600_100));
        assert_eq!(earliest_debt_start(&AccountingState::new()), None);
    }
}
