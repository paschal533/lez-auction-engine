//! System-operation types (design note §4.2 rows 9-16, §4.3 a): the context the config hands the
//! accounting actor with a `System` message, the operation itself, and the accounting's Read-block
//! scratch. Kept apart from `state.rs` (ruling R-3a, file size); the surplus and debt auctions'
//! states join them here.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::{AccountId, Actor};

use crate::state::{ConfigState, OracleReadingRec, SystemParamsRec};

/// What a system operation needs from its config, copied by the config `K` when it sends
/// `System` to its accounting (the accounting does not store its config).
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SystemCtx {
    /// The config's account `K`; the accounting accepts `System` only from `(config, E)`.
    pub config: AccountId,
    /// The token program `T`.
    pub token: AccountId,
    pub stable_def: AccountId,
    pub protocol_token: Option<AccountId>,
    /// The reference oracle (a minimal-oracle actor, RAY scale), if one is configured.
    pub ref_oracle: Option<Actor>,
    /// The admin reference price of the protocol token, RAY.
    pub ref_price_ray: u128,
    pub ref_max_age_ms: u64,
    pub slack_ms: u64,
    pub freeze_on_bad_debt: bool,
    pub bad_debt_freeze_threshold: u128,
    pub system: SystemParamsRec,
}

impl SystemCtx {
    /// The context of the config at `config` whose state is `c`.
    #[must_use]
    pub fn from_config(config: AccountId, c: &ConfigState) -> Self {
        Self {
            config,
            token: c.token,
            stable_def: c.stablecoin_definition_id,
            protocol_token: c.protocol_token,
            ref_oracle: c.protocol_token_oracle,
            ref_price_ray: c.protocol_token_ref_price,
            ref_max_age_ms: c.ref_oracle_max_age_ms,
            slack_ms: c.slack_ms,
            freeze_on_bad_debt: c.freeze_on_bad_debt,
            bad_debt_freeze_threshold: c.bad_debt_freeze_threshold,
            system: c.system.clone(),
        }
    }
}

/// A system operation. Variant order is wire format (indices 0-3 pinned).
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum SystemOp {
    /// 0. Row 9 (v0.2.4 `settle_debt`): burn `min(amount, buffer, unbacked - on_auction)`.
    SettleDebt { amount: u128 },
    /// 1. Rows 10-12: start surplus auction `nonce`.
    StartSurplus { nonce: u64 },
    /// 2. Rows 13-15: start debt auction `nonce`.
    StartDebt { nonce: u64 },
    /// 3. Row 16: `check_solvency`.
    CheckSolvency,
}

/// The reference oracle's reply: a decoded oracle state, or anything else (an empty or foreign
/// state), which prices at the admin price (v0.2.4's `OutOfRange` fallback).
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum RefReading {
    Valid(OracleReadingRec),
    Undecodable,
}

/// A system operation in flight at the accounting actor: set by `System`, filled by the buffer's
/// (and the reference oracle's) `StateReply`, consumed and cleared by `Proceed`, so `None` in
/// every committed state.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SystemPending {
    pub op: SystemOp,
    /// The caller's clock claim (asserted by `K`); 0 for `SettleDebt`, which claims no time.
    pub now_ms: u64,
    pub ctx: SystemCtx,
    /// The buffer's stablecoin balance, once its reply is in.
    pub buffer: Option<u128>,
    /// The reference oracle's reading, once its reply is in.
    pub reference: Option<RefReading>,
}

impl SystemPending {
    /// Whether the operation reads the reference oracle: every op but `SettleDebt`, when one is
    /// configured.
    #[must_use]
    pub fn awaits_reference(&self) -> bool {
        self.ctx.ref_oracle.is_some() && !matches!(self.op, SystemOp::SettleDebt { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures::{actor, config, id};

    #[test]
    fn ctx_copies_the_config() {
        let c = ConfigState {
            protocol_token_oracle: Some(actor(30, 31)),
            ..config()
        };
        let ctx = SystemCtx::from_config(id(40), &c);
        assert_eq!(
            (ctx.config, ctx.token, ctx.stable_def, ctx.protocol_token),
            (
                id(40),
                c.token,
                c.stablecoin_definition_id,
                c.protocol_token
            )
        );
        assert_eq!(ctx.ref_oracle, Some(actor(30, 31)));
        assert_eq!(
            (ctx.ref_price_ray, ctx.ref_max_age_ms, ctx.slack_ms),
            (42, 3_600_000, 6_000)
        );
        assert_eq!(
            (ctx.freeze_on_bad_debt, ctx.bad_debt_freeze_threshold),
            (true, 600)
        );
        assert_eq!(ctx.system, c.system);
    }

    #[test]
    fn only_non_settle_ops_with_an_oracle_await_the_reference() {
        let with_oracle = SystemCtx::from_config(
            id(40),
            &ConfigState {
                protocol_token_oracle: Some(actor(30, 31)),
                ..config()
            },
        );
        let pending = |op, ctx: &SystemCtx| SystemPending {
            op,
            now_ms: 0,
            ctx: ctx.clone(),
            buffer: None,
            reference: None,
        };
        let settle = SystemOp::SettleDebt { amount: 1 };
        assert!(!pending(settle, &with_oracle).awaits_reference());
        assert!(pending(SystemOp::CheckSolvency, &with_oracle).awaits_reference());
        let without = SystemCtx::from_config(id(40), &config());
        assert!(!pending(SystemOp::CheckSolvency, &without).awaits_reference());
    }
}
