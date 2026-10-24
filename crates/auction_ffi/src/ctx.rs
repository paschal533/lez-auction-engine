//! The request context: which deployment, where state lives, which wallet, and the one place an
//! `RpcChain` is built.

use std::path::PathBuf;
use std::time::Duration;

use auction_sdk::{
    chain::Chain,
    confirm::ConfirmSettings,
    deployment::Deployment,
    rpc::{ReadRetry, RpcChain},
    verify::{verify_chain_identity, verify_config, VerifiedConfig},
    SdkError, SdkResult,
};
use lee_core::account::AccountId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::envelope::{parse, FfiError};

/// IPC calls between modules time out after 20 s, so no FFI read may run long: 5 s per request and one
/// retry after 1 s (at most about 11 s with a dead node).
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const READ_RETRY: ReadRetry = ReadRetry {
    attempts: 2,
    first_delay: Duration::from_secs(1),
};
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
}

impl CtxJson {
    /// The `"ctx"` member of `req`.
    pub fn of(req: &Value) -> Result<Self, FfiError> {
        parse(req.get("ctx").unwrap_or(&Value::Null))
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
        let deployment = Deployment::load(&json.deployment)?;
        let chain = RpcChain::connect_with_timeout(&deployment.sequencer_url, READ_TIMEOUT)?
            .with_read_retry(READ_RETRY);
        Self::assemble(Box::new(chain), deployment, json)
    }

    /// `open` over any chain, for tests.
    #[cfg(any(test, feature = "testing"))]
    pub fn with_chain(
        chain: Box<dyn Chain>,
        deployment: Deployment,
        json: &CtxJson,
    ) -> Result<Self, FfiError> {
        Self::assemble(chain, deployment, json)
    }

    fn assemble(
        chain: Box<dyn Chain>,
        deployment: Deployment,
        json: &CtxJson,
    ) -> Result<Self, FfiError> {
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

    /// The request's context, opened.
    pub fn from_request(req: &Value) -> Result<Self, FfiError> {
        Self::open(&CtxJson::of(req)?)
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
