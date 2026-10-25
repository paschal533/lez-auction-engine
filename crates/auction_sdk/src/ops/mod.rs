//! Per-command orchestration, the one source of every figure a front end shows before signing: each
//! `prepare_*` resolves the accounts of one command, quotes where a price is involved (the quote sets
//! the slippage bound), refuses client-side what the engine or the host would reject, and returns the
//! plans to sign in order, the facts to show before them and the accounts to re-read after inclusion.
//! Nothing here signs or sends. The CLI prints a `Prepared`; the FFI (Plan 4) serialises it.
//!
//! The caller verifies the config first (I4) and announces its admin; `now_ms` is the chain's
//! `CLOCK_01` (transactions execute under it, never under an invented clock).

mod admin;
mod bid;
mod claim;
mod dev;
mod liquidate;
mod system;

pub use admin::{
    check_protocol_token, prepare_set_engine_params, require_engine_admin, require_host_admin,
    SetEngineParamsRequest,
};
pub use bid::{prepare_bid, BidRequest};
pub use claim::{
    prepare_claim_leftover, prepare_settle_empty, ClaimLeftoverRequest, SettleEmptyRequest,
};
pub use dev::{
    prepare_oracle_publish, prepare_token_transfer, require_dev_network, OraclePublishRequest,
    TokenTransferRequest, DEV_NETWORKS,
};
pub use liquidate::{prepare_liquidate, LiquidateRequest};
pub use system::{
    prepare_check_solvency, prepare_close_debt, prepare_close_surplus, prepare_observe_feed,
    prepare_settle_debt, prepare_start_debt, prepare_start_surplus, prepare_take_debt,
    prepare_take_surplus, SettleDebtRequest, TakeRequest,
};

use lee_core::account::{Account, AccountId};

use crate::{chain::Chain, error::SdkResult, plan::Plan};

/// Read command that shows the outcome of an auction transaction whose result is unknown.
pub const CHECK_AUCTIONS: &str = "list-auctions";
/// Same for transactions that only move the engine's own accounts.
pub const CHECK_CONFIG: &str = "config";
/// Same for engine admin transactions.
pub const CHECK_VERIFY: &str = "admin verify";
/// Same for the oracle publish (host rotation).
pub const CHECK_HOST: &str = "host status";
/// Same for token transfers and the other dev setup steps.
pub const CHECK_BALANCE: &str = "dev balance --account <account>";

/// One `key value` line shown before the plans (e.g. `quoted out`, `min out`), in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub key: &'static str,
    pub value: String,
}

/// An account a command touches, read back after inclusion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Touch {
    /// A token holding (balance); `definition` is the token the holding must be of.
    Holding {
        label: String,
        id: AccountId,
        definition: AccountId,
    },
    /// A liquidation auction.
    Auction(AccountId),
    /// The engine's accounting account.
    Accounting,
}

/// One transaction to sign and send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub plan: Plan,
    /// Instruction name, for the `plan` line.
    pub what: &'static str,
    /// Read command that shows the outcome when it is unknown (`auction-cli <check>`).
    pub check: &'static str,
}

/// What a command will do: its steps in sending order, the facts to show before signing, and the
/// accounts to re-read after the last inclusion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub steps: Vec<Step>,
    pub facts: Vec<Fact>,
    pub touched: Vec<Touch>,
}

/// The request of the commands whose only flag is `--caller`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerRequest {
    pub caller: String,
}

/// Account-argument resolution, injected: a base58 id or a wallet label (CLI: the keystore).
pub struct Accounts<'a> {
    pub resolve: &'a dyn Fn(&str) -> SdkResult<AccountId>,
}

impl Accounts<'_> {
    pub fn id(&self, arg: &str) -> SdkResult<AccountId> {
        (self.resolve)(arg)
    }

    pub fn optional(&self, arg: Option<&str>) -> SdkResult<Option<AccountId>> {
        arg.map(|s| self.id(s)).transpose()
    }
}

impl Prepared {
    fn one(plan: Plan, what: &'static str, check: &'static str) -> Self {
        Self {
            steps: vec![Step { plan, what, check }],
            facts: Vec::new(),
            touched: Vec::new(),
        }
    }

    fn with_facts(self, facts: Vec<Fact>) -> Self {
        Self { facts, ..self }
    }

    fn with_touched(self, touched: Vec<Touch>) -> Self {
        Self { touched, ..self }
    }
}

fn fact(key: &'static str, value: impl ToString) -> Fact {
    Fact {
        key,
        value: value.to_string(),
    }
}

/// A never-initialised account (a fresh recipient must sign).
fn is_fresh<C: Chain + ?Sized>(chain: &C, id: &AccountId) -> SdkResult<bool> {
    Ok(chain.account(id)? == Account::default())
}

#[cfg(test)]
#[path = "../ops_tests.rs"]
mod tests;
