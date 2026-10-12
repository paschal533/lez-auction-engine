//! Everything a command needs: the chain, the trusted deployment, the config, the wallet, the state
//! directory (scan cursor, in-flight locks) and the confirmation settings.

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use auction_sdk::{
    chain::{clock_ms, Chain},
    confirm::{submit, ConfirmSettings, Outcome},
    deployment::Deployment,
    keystore::Keystore,
    plan::Plan,
    verify::{verify_config, VerifiedConfig},
};
use lee_core::account::{Account, AccountId};

use crate::{
    error::CliError,
    output::kv,
    tx_report::{print_applied, TxMeta},
};

pub struct Env<C: Chain> {
    pub chain: C,
    pub deployment: Deployment,
    /// `--config`; falls back to the deployment file's `config`.
    pub config: Option<AccountId>,
    pub wallet_home: Option<PathBuf>,
    pub state_dir: PathBuf,
    /// `--at-ms`: evaluate quotes at this clock instead of `CLOCK_01`.
    pub at_ms: Option<u64>,
    pub confirm: ConfirmSettings,
    pub sleep: fn(Duration),
}

impl<C: Chain> Env<C> {
    pub fn config_id(&self) -> Result<AccountId, CliError> {
        self.config.or(self.deployment.config).ok_or_else(|| {
            CliError::Usage(
                "no engine config: pass --config or set \"config\" in the deployment file".into(),
            )
        })
    }

    /// I4 / ADR-027: every command that uses a config verifies it first.
    pub fn verified(&self) -> Result<VerifiedConfig, CliError> {
        Ok(verify_config(
            &self.chain,
            &self.deployment,
            &self.config_id()?,
        )?)
    }

    pub fn now(&self) -> Result<u64, CliError> {
        match self.at_ms {
            Some(t) => Ok(t),
            None => Ok(clock_ms(&self.chain)?),
        }
    }

    /// The chain's `CLOCK_01`, never `--at-ms`: transactions execute under the chain clock, so their
    /// slippage bounds and pre-checks must be quoted against it.
    pub fn chain_now(&self) -> Result<u64, CliError> {
        Ok(clock_ms(&self.chain)?)
    }

    pub fn keystore(&self) -> Result<Keystore, CliError> {
        let path = Keystore::default_path(self.wallet_home.as_deref())?;
        Ok(Keystore::open(&path)?)
    }

    /// A base58 account id, or a wallet label (which needs the keystore).
    pub fn account(&self, arg: &str) -> Result<AccountId, CliError> {
        if let Ok(id) = arg.parse::<AccountId>() {
            return Ok(id);
        }
        Ok(self.keystore()?.resolve(arg)?)
    }

    pub fn is_fresh(&self, id: &AccountId) -> Result<bool, CliError> {
        Ok(self.chain.account(id)? == Account::default())
    }

    #[must_use]
    pub fn cursor_path(&self, config: &AccountId) -> PathBuf {
        self.state_dir.join(format!("{config}.cursor.json"))
    }

    #[must_use]
    pub fn lock_dir(&self) -> PathBuf {
        self.state_dir.join("locks")
    }

    /// Prints the config's admin as read from the chain, and where the trust in it comes from.
    /// When the deployment file names no operator (`engine_admin: null`) the admin is whatever the
    /// config on the node says, so the user must see it before signing anything: every command that
    /// signs against a verified config calls this first (Tasks 14 and 15).
    pub fn announce_admin(&self, v: &VerifiedConfig, out: &mut dyn Write) -> Result<(), CliError> {
        kv(out, "admin", &v.state.admin.to_string())?;
        let source = if self.deployment.engine_admin.is_some() {
            "deployment file (checked)"
        } else {
            "chain only (the deployment file names no operator; make sure it is yours)"
        };
        kv(out, "admin source", source)
    }

    /// Lock, sign with the wallet, send, confirm (spec §6.5). Dropped is an error (exit 3); an unknown
    /// outcome (send uncertain, no verdict) keeps the lock, says how to check, and is never resent.
    pub fn send(
        &self,
        v: &VerifiedConfig,
        plan: &Plan,
        meta: &TxMeta,
        out: &mut dyn Write,
    ) -> Result<(), CliError> {
        let keystore = self.keystore()?;
        kv(
            out,
            "plan",
            &format!(
                "{} ({} accounts, {} signer(s))",
                meta.what,
                plan.account_ids.len(),
                plan.signing_requirements.len()
            ),
        )?;
        let s = submit(
            &self.chain,
            &keystore,
            plan,
            &self.lock_dir(),
            &self.confirm,
            &self.sleep,
        )
        .map_err(|e| self.with_check_hint(CliError::from(e), meta))?;
        kv(out, "submitted", &hex::encode(s.hash))?;
        match s.outcome {
            Outcome::Included { nonce_after } => {
                kv(out, "included", &format!("signer nonce {nonce_after}"))?;
                print_applied(self, v, &meta.touched, out)
            }
            Outcome::Dropped { height } => Err(CliError::Dropped { height }),
        }
    }

    fn with_check_hint(&self, e: CliError, meta: &TxMeta) -> CliError {
        match e {
            CliError::Unknown(m) => CliError::Unknown(format!(
                "outcome unknown, lock kept ({m}). Do not resend: check with `auction-cli {}` or wait {} blocks and check again; the lock in {} clears itself once the window has passed",
                meta.check,
                self.confirm.drop_after_blocks,
                self.lock_dir().display()
            )),
            other => other,
        }
    }
}
