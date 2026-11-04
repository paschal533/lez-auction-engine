//! Pure system-operation computations (design note §4.2 rows 9-16), each a call into
//! `auction_core` as v0.2.4 makes it: the implicit settle against the buffer's actual balance
//! (v0.2.4 `settle::implicit_settle`) and the protocol token's reference price (v0.2.4
//! `system::ref_price`).

use auction_core::{
    error::PauseReason,
    price::{FeedParams, MAX_CONF_BPS, OracleScale, effective_ref_price, validate_feed},
};

use crate::{
    state::AccountingState,
    system_state::{RefReading, SystemCtx},
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{
        ConfigState, OracleReadingRec,
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
}
