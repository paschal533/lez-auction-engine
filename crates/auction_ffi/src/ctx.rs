//! The request context: which deployment, where state lives, which wallet, and the one place an
//! `RpcChain` is built.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use auction_sdk::{
    chain::{BlockSummary, Chain},
    confirm::ConfirmSettings,
    deployment::Deployment,
    rpc::{ReadRetry, RpcChain},
    verify::{verify_chain_identity, verify_config, VerifiedConfig},
    SdkError, SdkResult,
};
use lee::PublicTransaction;
use lee_core::account::{Account, AccountId, Nonce};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::envelope::{parse, FfiError};

/// IPC calls between modules time out after 20 s. Each FFI call has a deadline, `CALL_BUDGET` from
/// its start unless `ctx.budget_ms` sets it. Against the node (`RpcChain::with_deadline`) every read
/// try has `READ_TIMEOUT` capped at the time left, a retry is made only if its pause ends before the
/// deadline, and no read starts after it (`chain` error). So a call ends within its budget plus the
/// time to build its reply: at most 12 s by default, 15 s with the largest `budget_ms`.
const READ_TIMEOUT: Duration = Duration::from_secs(5);
pub const CALL_BUDGET: Duration = Duration::from_secs(12);
/// Bounds of the optional `ctx.budget_ms`: the module passes what is left of its own 15 s budget
/// (`MODULE_CALL_BUDGET_MS` in modules/auction_engine/src/budget.h), less what it keeps for later steps.
const MIN_BUDGET_MS: u64 = 1;
const MAX_BUDGET_MS: u64 = 15_000;
/// Views and `prepare`: one retry after 1 s.
const VIEW_RETRY: ReadRetry = ReadRetry {
    attempts: 2,
    first_delay: Duration::from_secs(1),
};
/// `tx_begin` and `tx_poll`: a single try, the module polls again.
const TX_RETRY: ReadRetry = ReadRetry::NONE;
/// The named dev networks that sending dev operations (mock tokens) may act on; the same list as the
/// CLI's `DEV_NETWORKS`. A name on this list is never enough: block 1 must also match the deployment.
pub const DEV_NETWORKS: &[&str] = &["localnet", "testnet-legacy"];
/// Subdirectory of the state directory holding the in-flight locks (the CLI's layout).
pub const LOCK_DIR: &str = "locks";

/// The `"ctx"` member of every request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CtxJson {
    pub deployment: PathBuf,
    pub state_dir: PathBuf,
    #[serde(default)]
    pub wallet_home: Option<PathBuf>,
    /// Base58 engine config; falls back to the deployment file's.
    #[serde(default)]
    pub config: Option<String>,
    /// Blocks after which an unapplied transaction counts as dropped (default 5, at least 1).
    #[serde(default)]
    pub drop_after_blocks: Option<u64>,
    /// Deadline of this call in milliseconds, clamped to [1, 15000]; default `CALL_BUDGET`.
    #[serde(default)]
    pub budget_ms: Option<u64>,
}

impl CtxJson {
    /// The `"ctx"` member of `req`.
    pub fn of(req: &Value) -> Result<Self, FfiError> {
        parse(req.get("ctx").unwrap_or(&Value::Null))
    }

    /// The call's deadline: `budget_ms` (clamped) from now, else `default` from now.
    #[must_use]
    pub fn deadline(&self, default: Duration) -> Instant {
        let budget = self.budget_ms.map_or(default, |ms| {
            Duration::from_millis(ms.clamp(MIN_BUDGET_MS, MAX_BUDGET_MS))
        });
        Instant::now() + budget
    }

    /// The CLI's confirmation defaults, with `drop_after_blocks` from the context.
    pub fn confirm_settings(&self) -> Result<ConfirmSettings, FfiError> {
        let mut s = ConfirmSettings::default();
        if let Some(n) = self.drop_after_blocks {
            if n == 0 {
                return Err(FfiError::bad("ctx.drop_after_blocks must be at least 1"));
            }
            s.drop_after_blocks = n;
        }
        Ok(s)
    }
}

pub struct Ctx {
    pub chain: Box<dyn Chain>,
    pub deployment: Deployment,
    pub state_dir: PathBuf,
    pub wallet_home: Option<PathBuf>,
    pub config: Option<AccountId>,
    pub confirm: ConfirmSettings,
}

impl Ctx {
    /// Loads the deployment file and connects to its sequencer (lazily: no request is sent yet).
    pub fn open(json: &CtxJson) -> Result<Self, FfiError> {
        Self::open_with(json, VIEW_RETRY)
    }

    fn open_with(json: &CtxJson, retry: ReadRetry) -> Result<Self, FfiError> {
        let deadline = json.deadline(CALL_BUDGET);
        let deployment = Deployment::load(&json.deployment)?;
        let chain = RpcChain::connect_with_timeout(&deployment.sequencer_url, READ_TIMEOUT)?
            .with_read_retry(retry)
            .with_deadline(deadline);
        Self::assemble(Box::new(chain), deployment, json, deadline)
    }

    /// `open` over any chain, for tests.
    #[cfg(any(test, feature = "testing"))]
    pub fn with_chain(
        chain: Box<dyn Chain>,
        deployment: Deployment,
        json: &CtxJson,
    ) -> Result<Self, FfiError> {
        Self::assemble(chain, deployment, json, json.deadline(CALL_BUDGET))
    }

    /// `with_chain` with another per-call budget, so tests need not wait 12 s.
    #[cfg(any(test, feature = "testing"))]
    pub fn with_chain_budget(
        chain: Box<dyn Chain>,
        deployment: Deployment,
        json: &CtxJson,
        budget: Duration,
    ) -> Result<Self, FfiError> {
        Self::assemble(chain, deployment, json, json.deadline(budget))
    }

    fn assemble(
        chain: Box<dyn Chain>,
        deployment: Deployment,
        json: &CtxJson,
        deadline: Instant,
    ) -> Result<Self, FfiError> {
        let chain: Box<dyn Chain> = Box::new(Deadline {
            inner: chain,
            deadline,
        });
        let config = json
            .config
            .as_deref()
            .map(|c| {
                c.parse::<AccountId>()
                    .map_err(|_| FfiError::bad("ctx.config is not a base58 account id"))
            })
            .transpose()?;
        Ok(Self {
            chain,
            deployment,
            state_dir: json.state_dir.clone(),
            wallet_home: json.wallet_home.clone(),
            config,
            confirm: json.confirm_settings()?,
        })
    }

    /// The request's context, opened (reads retry once).
    pub fn from_request(req: &Value) -> Result<Self, FfiError> {
        Self::open(&CtxJson::of(req)?)
    }

    /// The context for `tx_begin` / `tx_poll`: reads are tried once.
    pub fn for_tx(req: &Value) -> Result<Self, FfiError> {
        Self::open_with(&CtxJson::of(req)?, TX_RETRY)
    }

    /// `ctx.config`, else the deployment file's.
    #[must_use]
    pub fn config_id(&self) -> Option<AccountId> {
        self.config.or(self.deployment.config)
    }

    /// I4 / ADR-027: every operation that uses a config verifies it first.
    pub fn verified(&self) -> SdkResult<VerifiedConfig> {
        let id = self.config_id().ok_or_else(|| SdkError::Deployment {
            path: String::new(),
            reason: "no engine config: set ctx.config or \"config\" in the deployment file".into(),
        })?;
        verify_config(self.chain.as_ref(), &self.deployment, &id)
    }

    #[must_use]
    pub fn lock_dir(&self) -> PathBuf {
        self.state_dir.join(LOCK_DIR)
    }

    /// Whether the deployment names a dev network (by name only; see `require_dev`).
    #[must_use]
    pub fn is_dev_network(&self) -> bool {
        DEV_NETWORKS.contains(&self.deployment.network.as_str())
    }

    /// The dev guard of `dev token-transfer`: a named dev network AND the chain the deployment file
    /// recorded (block 1), in that order.
    pub fn require_dev(&self) -> SdkResult<()> {
        if !self.is_dev_network() {
            return Err(SdkError::Mismatch {
                what: "this operation sends on dev networks only (DEV_NETWORKS)",
                expected: DEV_NETWORKS.join(", "),
                found: self.deployment.network.clone(),
            });
        }
        verify_chain_identity(self.chain.as_ref(), &self.deployment)
    }
}

/// Refuses to start a read once the call's deadline has passed (over any chain; `RpcChain` also cuts
/// off a read that is still running at the deadline).
struct Deadline {
    inner: Box<dyn Chain>,
    deadline: Instant,
}

impl Deadline {
    fn check(&self) -> SdkResult<()> {
        if Instant::now() >= self.deadline {
            return Err(SdkError::Chain(
                "the call's time budget ran out before the node answered".into(),
            ));
        }
        Ok(())
    }
}

impl Chain for Deadline {
    fn account(&self, id: &AccountId) -> SdkResult<Account> {
        self.check()?;
        self.inner.account(id)
    }
    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<Nonce>> {
        self.check()?;
        self.inner.nonces(ids)
    }
    fn last_block_id(&self) -> SdkResult<u64> {
        self.check()?;
        self.inner.last_block_id()
    }
    fn blocks(&self, from: u64, to: u64) -> SdkResult<Vec<BlockSummary>> {
        self.check()?;
        self.inner.blocks(from, to)
    }
    fn send(&self, tx: &PublicTransaction) -> SdkResult<[u8; 32]> {
        self.check()?;
        self.inner.send(tx)
    }
}
