//! Pure system-operation computations (design note §4.2 rows 9-16), each a call into
//! `auction_core` as v0.2.4 makes it: the implicit settle against the buffer's actual balance
//! (v0.2.4 `settle::implicit_settle`) and the protocol token's reference price (v0.2.4
//! `system::ref_price`).

use auction_core::{
    error::PauseReason,
    price::{FeedParams, MAX_CONF_BPS, OracleScale, effective_ref_price, validate_feed},
    system::{close_surplus, start_surplus, take_surplus},
};
use auction_engine_params::EngineError;

use crate::{
    state::AccountingState,
    system_state::{RefReading, SurplusAuctionState, SystemCtx, SystemPhase, TAG_SURPLUS},
};

/// `settle_debt(u128::MAX)` against the buffer's actual balance: the new state and the amount to
/// burn from the buffer.
#[must_use]
pub fn implicit_settle(acc: &AccountingState, buffer: u128) -> (AccountingState, u128) {
    let (next, burned) = acc.core().settle_debt(u128::MAX, buffer);
    (AccountingState::with_core(acc, &next), burned)
}

/// The protocol token's reference price, RAY: the admin price; a configured reference oracle
/// whose reading is valid (age against `now_ms + slack_ms` (ADR-058), confidence, within
/// `[admin/2, 2·admin]`) overrides it, clamped to ±20 % of the admin price. A missing, invalid or
/// undecodable reading falls back to the admin price (v0.2.4's `OutOfRange` fallback).
#[must_use]
pub fn ref_price(ctx: &SystemCtx, reading: Option<RefReading>, now_ms: u64) -> u128 {
    let admin = ctx.ref_price_ray;
    if ctx.ref_oracle.is_none() {
        return admin;
    }
    let params = FeedParams {
        scale: OracleScale::Ray,
        max_age_ms: ctx.ref_max_age_ms,
        max_conf_bps: MAX_CONF_BPS,
        band_lo_bps: 10_000,
        band_hi_bps: 10_000,
    };
    let validity = match reading {
        Some(RefReading::Valid(r)) => validate_feed(
            &r.to_core(),
            &params,
            admin,
            now_ms.saturating_add(ctx.slack_ms),
        ),
        Some(RefReading::Undecodable) | None => Err(PauseReason::OutOfRange),
    };
    effective_ref_price(admin, Some(validity))
}

/// v0.2.4 `start_surplus_auction` against the buffer's actual balance `buffer`: the implicit
/// settle, then `auction_core`'s start rules (no unbacked debt left, buffer ≥ target + lot, the
/// delay since the last end, none live), then the booking (`surplus_live`, the next nonce, the
/// lot out). Returns the accounting, the new auction (reference price `reference` snapshotted)
/// and the amount the settle burns from the buffer.
///
/// # Errors
/// `NonceMismatch` unless `nonce` is the accounting's next surplus nonce; `auction_core`'s start
/// errors; `Math` on overflow.
pub fn start_surplus_op(
    acc: &AccountingState,
    ctx: &SystemCtx,
    buffer: u128,
    reference: u128,
    now_ms: u64,
    nonce: u64,
) -> Result<(AccountingState, SurplusAuctionState, u128), EngineError> {
    if nonce != acc.surplus_nonce {
        return Err(EngineError::NonceMismatch);
    }
    let (settled, burned) = implicit_settle(acc, buffer);
    let buffer_after = buffer.checked_sub(burned).ok_or(EngineError::Math)?;
    let started = start_surplus(
        &settled.core(),
        buffer_after,
        ctx.protocol_token.is_some(),
        acc.last_surplus_end_ms,
        now_ms,
        &ctx.system.to_core()?,
        acc.surplus_live,
    )?;
    let state = SurplusAuctionState {
        tag: TAG_SURPLUS,
        config: ctx.config,
        nonce,
        lot_left: started.lot_left,
        start_ms: started.start_ms,
        ref_ray: reference,
        phase: SystemPhase::Live,
        close: None,
    };
    let next = AccountingState {
        surplus_live: true,
        surplus_nonce: nonce.checked_add(1).ok_or(EngineError::Math)?,
        total_surplus_lots_out: settled
            .total_surplus_lots_out
            .checked_add(started.lot_left)
            .ok_or(EngineError::Math)?,
        ..settled
    };
    Ok((next, state, burned))
}

/// v0.2.4 `take_surplus` on the auction `s`: the next state, the protocol tokens owed and the
/// stablecoin filled (`old.lot_left − new.lot_left`, at most `amount_coin`), priced at the
/// snapshotted `s.ref_ray`.
///
/// # Errors
/// `auction_core`'s take errors (`NotLive`, `ZeroFill`, `DustRemainder`, `Slippage`); the
/// stored system parameters' validation errors.
pub fn take_surplus_op(
    s: &SurplusAuctionState,
    ctx: &SystemCtx,
    (amount_coin, max_protocol_in): (u128, u128),
    now_ms: u64,
) -> Result<(SurplusAuctionState, u128, u128), EngineError> {
    let sys = ctx.system.to_core()?;
    let (next, tokens) = take_surplus(
        &s.core(),
        amount_coin,
        max_protocol_in,
        s.ref_ray,
        &sys,
        now_ms,
    )?;
    let filled = s
        .lot_left
        .checked_sub(next.lot_left)
        .ok_or(EngineError::Math)?;
    Ok((s.with_core(&next), tokens, filled))
}

/// v0.2.4 `close_surplus_auction` on the auction `s` (after `system_ttl_ms`): the ended state.
///
/// # Errors
/// `NotLive`, `DelayNotElapsed`; the stored system parameters' validation errors.
pub fn close_surplus_op(
    s: &SurplusAuctionState,
    ctx: &SystemCtx,
    now_ms: u64,
) -> Result<SurplusAuctionState, EngineError> {
    let (next, _remainder) = close_surplus(&s.core(), &ctx.system.to_core()?, now_ms)?;
    Ok(s.with_core(&next))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{
        ConfigState, OracleReadingRec, SchedulePointRec,
        fixtures::{actor, config, id},
    };

    const RAY: u128 = 1_000_000_000_000_000_000_000_000_000;
    const NOW: u64 = 10_000_000;

    /// v0.2.4 `settle_tests.rs` `debt_accounting`: 1_000 unbacked, 200 of it on auction.
    fn debt_accounting() -> AccountingState {
        AccountingState {
            unbacked_debt: 1_000,
            debt_on_auction: 200,
            ..AccountingState::new()
        }
    }

    /// The fixture config's context with admin price 2 RAY and, if `oracle`, a reference oracle.
    fn ctx(oracle: bool) -> SystemCtx {
        let c = ConfigState {
            protocol_token_ref_price: 2 * RAY,
            protocol_token_oracle: oracle.then(|| actor(30, 31)),
            ..config()
        };
        SystemCtx::from_config(id(40), &c)
    }

    fn reading(price_ray: u128, timestamp_ms: u64) -> Option<RefReading> {
        Some(RefReading::Valid(OracleReadingRec {
            price_raw: price_ray,
            confidence_raw: 0,
            timestamp_ms,
        }))
    }

    /// v0.2.4 `implicit_settle_burns_up_to_the_settleable_debt_and_buffer_balance`.
    #[test]
    fn implicit_settle_matches_v024() {
        let (next, burned) = implicit_settle(&debt_accounting(), 5_000);
        assert_eq!(burned, 800);
        assert_eq!((next.unbacked_debt, next.total_debt_settled), (200, 800));
        assert_eq!(next.debt_on_auction, 200, "the rest is untouched");
        let (_, small) = implicit_settle(&debt_accounting(), 10);
        assert_eq!(small, 10);
        let (same, none) = implicit_settle(&debt_accounting(), 0);
        assert_eq!((same, none), (debt_accounting(), 0));
    }

    #[test]
    fn ref_price_without_oracle_is_the_admin_price() {
        let c = ctx(false);
        assert_eq!(ref_price(&c, None, NOW), 2 * RAY);
        // A reading is ignored when no oracle is configured.
        assert_eq!(ref_price(&c, reading(3 * RAY, NOW), NOW), 2 * RAY);
    }

    #[test]
    fn ref_price_clamps_to_twenty_percent() {
        let c = ctx(true);
        assert_eq!(ref_price(&c, reading(3 * RAY, NOW), NOW), 24 * RAY / 10);
        assert_eq!(
            ref_price(&c, reading(RAY + RAY / 2, NOW), NOW),
            16 * RAY / 10
        );
        assert_eq!(
            ref_price(&c, reading(21 * RAY / 10, NOW), NOW),
            21 * RAY / 10
        );
    }

    /// ADR-058: the feed age is measured against `now_ms + slack_ms`.
    #[test]
    fn stale_or_undecodable_reference_falls_back() {
        let c = ctx(true);
        let oldest = NOW + c.slack_ms - c.ref_max_age_ms;
        let fresh = 21 * RAY / 10;
        assert_eq!(ref_price(&c, reading(fresh, oldest), NOW), fresh);
        assert_eq!(ref_price(&c, reading(fresh, oldest - 1), NOW), 2 * RAY);
        assert_eq!(ref_price(&c, Some(RefReading::Undecodable), NOW), 2 * RAY);
        // Out of the [admin/2, 2·admin] band, or zero: the admin price.
        assert_eq!(ref_price(&c, reading(5 * RAY, NOW), NOW), 2 * RAY);
        assert_eq!(ref_price(&c, reading(0, NOW), NOW), 2 * RAY);
        // No reply at all (the Proceed refuses this first): the admin price, never a panic.
        assert_eq!(ref_price(&c, None, NOW), 2 * RAY);
    }

    /// v0.2.4 `start_surplus_auction`: implicit settle, the core start rules, then the booking.
    #[test]
    fn start_surplus_settles_then_books_the_lot() {
        let c = ctx(false);
        let acc = AccountingState {
            unbacked_debt: 300,
            total_surplus_lots_out: 7,
            ..AccountingState::new()
        };
        let (next, s, burned) = start_surplus_op(&acc, &c, 1_800, 2 * RAY, NOW, 0).expect("starts");
        assert_eq!(burned, 300, "the debt is settled from the buffer first");
        assert_eq!((next.unbacked_debt, next.total_debt_settled), (0, 300));
        assert_eq!(
            (
                next.surplus_live,
                next.surplus_nonce,
                next.total_surplus_lots_out
            ),
            (true, 1, 507)
        );
        assert_eq!(
            (s.config, s.nonce, s.lot_left, s.start_ms, s.ref_ray),
            (c.config, 0, 500, NOW, 2 * RAY)
        );
        assert_eq!((s.phase, s.close), (SystemPhase::Live, None));
        assert_eq!(SurplusAuctionState::decode(&s.encode()), Some(s));
    }

    #[test]
    fn start_surplus_rejections() {
        let c = ctx(false);
        let acc = AccountingState::new();
        let start = |acc: &AccountingState, ctx: &SystemCtx, buffer, nonce| {
            start_surplus_op(acc, ctx, buffer, 2 * RAY, NOW, nonce).map(|_| ())
        };
        assert_eq!(start(&acc, &c, 1_500, 1), Err(EngineError::NonceMismatch));
        assert_eq!(
            start(&acc, &c, 1_499, 0),
            Err(EngineError::BufferBelowTarget)
        );
        let no_token = SystemCtx {
            protocol_token: None,
            ..c.clone()
        };
        assert_eq!(
            start(&acc, &no_token, 1_500, 0),
            Err(EngineError::TokenNotConfigured)
        );
        let live = AccountingState {
            surplus_live: true,
            ..acc.clone()
        };
        assert_eq!(
            start(&live, &c, 1_500, 0),
            Err(EngineError::SurplusAuctionAlreadyLive)
        );
        let indebted = AccountingState {
            unbacked_debt: 2_000,
            ..acc
        };
        assert_eq!(
            start(&indebted, &c, 1_500, 0),
            Err(EngineError::UnbackedDebtPresent)
        );
    }

    /// A started auction, under the fixture's system parameters with v0.2.4's test decay
    /// `[0, 0, 1_000, 2_500]` (none at elapsed 0).
    fn started() -> (SurplusAuctionState, SystemCtx) {
        let mut c = ctx(false);
        c.system.surplus_decay = vec![
            SchedulePointRec {
                elapsed_ms: 0,
                discount_bps: 0,
            },
            SchedulePointRec {
                elapsed_ms: 1_000,
                discount_bps: 2_500,
            },
        ];
        let (_, s, _) =
            start_surplus_op(&AccountingState::new(), &c, 1_500, 2 * RAY, NOW, 0).expect("starts");
        (s, c)
    }

    /// v0.2.4 `partial_take_dust_and_slippage` numbers (the fixture's premium 500 bps, no decay).
    #[test]
    fn take_surplus_fills_and_prices_at_the_snapshot() {
        let (s, c) = started();
        let take = |s: &SurplusAuctionState, amounts| take_surplus_op(s, &c, amounts, NOW);
        assert_eq!(
            take(&s, (495, u128::MAX)).map(|_| ()),
            Err(EngineError::DustRemainder)
        );
        assert_eq!(take(&s, (250, 1)).map(|_| ()), Err(EngineError::Slippage));
        assert_eq!(
            take(&s, (0, u128::MAX)).map(|_| ()),
            Err(EngineError::ZeroFill)
        );
        let (half, tokens, filled) = take(&s, (250, u128::MAX)).expect("a partial take");
        assert_eq!((tokens, filled), (132, 250));
        assert_eq!((half.lot_left, half.phase), (250, SystemPhase::Live));
        let (done, tokens, filled) = take(&half, (u128::MAX, u128::MAX)).expect("the rest");
        assert_eq!((tokens, filled), (132, 250));
        assert_eq!((done.lot_left, done.phase), (0, SystemPhase::Ended));
        assert_eq!(
            take(&done, (1, u128::MAX)).map(|_| ()),
            Err(EngineError::NotLive)
        );
    }

    #[test]
    fn close_surplus_after_the_ttl_ends() {
        let (s, c) = started();
        let ttl = c.system.system_ttl_ms;
        assert_eq!(
            close_surplus_op(&s, &c, NOW + ttl - 1),
            Err(EngineError::DelayNotElapsed)
        );
        let closed = close_surplus_op(&s, &c, NOW + ttl).expect("closes");
        assert_eq!((closed.lot_left, closed.phase), (0, SystemPhase::Ended));
        assert_eq!(
            close_surplus_op(&closed, &c, NOW + ttl),
            Err(EngineError::NotLive)
        );
    }
}
