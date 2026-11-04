//! `check_solvency` (design note §4.2 row 16, §5; v0.2.4 `solvency.rs`, spec §5.11, ADR-017):
//! the implicit settle against the buffer's actual balance, then `auction_core`'s decision.
//! Kept apart from `system.rs` (ruling R-6a, file size); `system` re-exports
//! [`check_solvency_op`].
//!
//! - `Healthy` / `NoAction`: only the implicit settle is applied.
//! - `RecoveryInProgress` (a debt auction is live): the whole check reverts, settle included.
//! - `Freeze`: the settle, and `host_frozen_by_engine` set; the accounting then asks its config
//!   to freeze new debt at the host. `host_frozen_by_engine` records that the engine froze this
//!   host at least once, not the host's current state (the engine never clears it).

pub use auction_core::system::SolvencyDecision;
use auction_core::system::{SystemAuctionError, check_solvency, start_debt};
use auction_engine_params::EngineError;

use crate::{state::AccountingState, system::implicit_settle, system_state::SystemCtx};

/// v0.2.4 `check_solvency` against the buffer's actual balance `buffer`, at the reference price
/// `reference` and the claimed time `now_ms`: the accounting after the implicit settle (and the
/// freeze flag, on `Freeze`), the amount the settle burns from the buffer, and the decision.
///
/// The start check is `auction_core::start_debt` on the settled books with `token_ok` /
/// `mint_authority_ok` `true` by construction (the protocol definition is the engine's PDA with
/// minter `K`, see `start_debt_op`), so only `MintCapConfig` makes an auction unstartable; with
/// no protocol token configured it is `TokenNotConfigured`. The relist delay is not part of it
/// (v0.2.4 checks it in `start_debt_auction` only).
///
/// # Errors
/// `RecoveryInProgress` while a debt auction is live and the debt is above the threshold; the
/// stored system parameters' validation errors; `Math` on overflow.
pub fn check_solvency_op(
    acc: &AccountingState,
    ctx: &SystemCtx,
    buffer: u128,
    reference: u128,
    now_ms: u64,
) -> Result<(AccountingState, u128, SolvencyDecision), EngineError> {
    let (settled, burned) = implicit_settle(acc, buffer);
    let buffer_after = buffer.checked_sub(burned).ok_or(EngineError::Math)?;
    let configured = ctx.protocol_token.is_some();
    let start_check = if configured {
        start_debt(
            &settled.core(),
            buffer_after,
            true,
            true,
            true,
            reference,
            &ctx.system.to_core()?,
            now_ms,
            acc.debt_live,
        )
        .map(|_| ())
    } else {
        Err(SystemAuctionError::TokenNotConfigured)
    };
    let decision = check_solvency(
        settled.unbacked_debt,
        ctx.bad_debt_freeze_threshold,
        configured,
        ctx.freeze_on_bad_debt,
        acc.debt_live,
        start_check,
    );
    let next = match decision {
        SolvencyDecision::RecoveryInProgress => return Err(EngineError::RecoveryInProgress),
        SolvencyDecision::Freeze => AccountingState {
            host_frozen_by_engine: true,
            ..settled
        },
        SolvencyDecision::Healthy | SolvencyDecision::NoAction => settled,
    };
    Ok((next, burned, decision))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{
        ConfigState,
        fixtures::{config, id},
    };

    const RAY: u128 = 1_000_000_000_000_000_000_000_000_000;
    const NOW: u64 = 10_000_000;
    /// The fixture config's threshold.
    const THRESHOLD: u128 = 600;

    /// The fixture config (threshold 600, freeze on), admin price 2 RAY, with or without a
    /// protocol token.
    fn ctx(token: bool) -> SystemCtx {
        let c = ConfigState {
            protocol_token_ref_price: 2 * RAY,
            protocol_token: token.then(|| id(50)),
            ..config()
        };
        SystemCtx::from_config(id(40), &c)
    }

    fn unbacked(debt: u128) -> AccountingState {
        AccountingState {
            unbacked_debt: debt,
            ..AccountingState::new()
        }
    }

    fn check(acc: &AccountingState, ctx: &SystemCtx, buffer: u128) -> SolvencyDecision {
        check_solvency_op(acc, ctx, buffer, 2 * RAY, NOW)
            .expect("decides")
            .2
    }

    /// No protocol token: `unbacked ≤ threshold` is healthy, one unit above freezes, whatever
    /// `freeze_on_bad_debt` says (ADR-017: there is no recovery path).
    #[test]
    fn without_a_token_the_threshold_alone_decides() {
        let c = ctx(false);
        assert_eq!(
            check(&unbacked(THRESHOLD), &c, 0),
            SolvencyDecision::Healthy
        );
        let (next, burned, decision) =
            check_solvency_op(&unbacked(THRESHOLD + 1), &c, 0, 2 * RAY, NOW).expect("decides");
        assert_eq!((burned, decision), (0, SolvencyDecision::Freeze));
        assert!(next.host_frozen_by_engine);
        let disabled = SystemCtx {
            freeze_on_bad_debt: false,
            ..c
        };
        assert_eq!(
            check(&unbacked(THRESHOLD + 1), &disabled, 0),
            SolvencyDecision::Freeze
        );
    }

    /// The implicit settle runs first and is applied in every outcome but the revert.
    #[test]
    fn implicit_settle_runs_before_the_decision() {
        let c = ctx(false);
        let (next, burned, decision) =
            check_solvency_op(&unbacked(2_000), &c, 1_400, 2 * RAY, NOW).expect("decides");
        assert_eq!((burned, decision), (1_400, SolvencyDecision::Healthy));
        assert_eq!((next.unbacked_debt, next.total_debt_settled), (600, 1_400));
        assert!(!next.host_frozen_by_engine);
        let (next, burned, decision) =
            check_solvency_op(&unbacked(2_000), &c, 1_000, 2 * RAY, NOW).expect("decides");
        assert_eq!((burned, decision), (1_000, SolvencyDecision::Freeze));
        assert_eq!(
            (next.unbacked_debt, next.host_frozen_by_engine),
            (1_000, true)
        );
    }

    /// A live debt auction above the threshold reverts, even with the freeze disabled; at or
    /// below the threshold it is healthy.
    #[test]
    fn live_debt_auction_is_recovery_in_progress() {
        let live = AccountingState {
            debt_live: true,
            debt_on_auction: 500,
            ..unbacked(2_000)
        };
        let c = ctx(true);
        let disabled = SystemCtx {
            freeze_on_bad_debt: false,
            ..c.clone()
        };
        for ctx in [&c, &disabled] {
            assert_eq!(
                check_solvency_op(&live, ctx, 0, 2 * RAY, NOW),
                Err(EngineError::RecoveryInProgress)
            );
        }
        let calm = AccountingState {
            unbacked_debt: THRESHOLD,
            ..live
        };
        assert_eq!(check(&calm, &c, 0), SolvencyDecision::Healthy);
    }

    /// A working protocol token takes no action (a keeper should start a debt auction); one
    /// whose start amount exceeds `max_mint_per_auction` freezes, unless the freeze is disabled.
    #[test]
    fn working_token_takes_no_action_and_an_unstartable_one_freezes() {
        let c = ctx(true);
        let (next, _, decision) =
            check_solvency_op(&unbacked(2_000), &c, 0, 2 * RAY, NOW).expect("decides");
        assert_eq!(decision, SolvencyDecision::NoAction);
        assert!(!next.host_frozen_by_engine);
        // Start amount 500 × RAY / 2 RAY = 250 tokens.
        let mut capped = c.clone();
        capped.system.max_mint_per_auction = 250;
        assert_eq!(
            check(&unbacked(2_000), &capped, 0),
            SolvencyDecision::NoAction
        );
        capped.system.max_mint_per_auction = 249;
        assert_eq!(
            check(&unbacked(2_000), &capped, 0),
            SolvencyDecision::Freeze
        );
        capped.freeze_on_bad_debt = false;
        assert_eq!(
            check(&unbacked(2_000), &capped, 0),
            SolvencyDecision::NoAction
        );
    }

    /// A second freeze keeps the flag; a later healthy check never clears it.
    #[test]
    fn the_freeze_flag_is_never_cleared() {
        let frozen = AccountingState {
            host_frozen_by_engine: true,
            ..unbacked(THRESHOLD)
        };
        let (next, _, decision) =
            check_solvency_op(&frozen, &ctx(false), 0, 2 * RAY, NOW).expect("decides");
        assert_eq!(decision, SolvencyDecision::Healthy);
        assert!(next.host_frozen_by_engine);
    }
}
