//! Engine admin: `set_engine_params`, and the signer checks every admin command runs before signing.

use std::path::PathBuf;

use auction_engine_core::args::EngineParamsArgs;
use lee_core::{account::AccountId, program::ProgramId};
use stablecoin_core::ProtocolParameters;
use token_core::TokenDefinition;

use crate::{
    chain::Chain,
    error::{SdkError, SdkResult},
    params, plan,
    verify::VerifiedConfig,
};

use super::{fact, Accounts, Fact, Prepared, CHECK_VERIFY};

/// The flags of `auction-cli admin set-engine-params`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetEngineParamsRequest {
    pub admin: String,
    /// Engine parameter file (`params::engine_params`).
    pub params: PathBuf,
    pub protocol_token: Option<String>,
    pub ref_oracle: Option<String>,
    /// Allow the change to disable the protocol token the config currently has.
    pub disable_protocol_token: bool,
    /// Allow the change to disable the reference oracle the config currently has.
    pub disable_ref_oracle: bool,
}

/// `set_engine_params` without the parameter file: the signer and the two optional slots, as in
/// [`SetEngineParamsRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineParamsSlots {
    pub admin: String,
    pub protocol_token: Option<String>,
    pub ref_oracle: Option<String>,
    pub disable_protocol_token: bool,
    pub disable_ref_oracle: bool,
}

impl From<&SetEngineParamsRequest> for EngineParamsSlots {
    fn from(r: &SetEngineParamsRequest) -> Self {
        Self {
            admin: r.admin.clone(),
            protocol_token: r.protocol_token.clone(),
            ref_oracle: r.ref_oracle.clone(),
            disable_protocol_token: r.disable_protocol_token,
            disable_ref_oracle: r.disable_ref_oracle,
        }
    }
}

/// Refused before signing (exit 2 in the CLI) unless `signer` is the config admin.
pub fn require_engine_admin(v: &VerifiedConfig, signer: AccountId) -> SdkResult<()> {
    if signer != v.state.admin {
        return Err(SdkError::Mismatch {
            what: "config admin (refusing before signing: the signer is not the config admin)",
            expected: v.state.admin.to_string(),
            found: signer.to_string(),
        });
    }
    Ok(())
}

/// Refused before signing unless `signer` is the host admin recorded in `ProtocolParameters`: the
/// host would drop the transaction, and resending could not help.
pub fn require_host_admin(params: &ProtocolParameters, signer: AccountId) -> SdkResult<()> {
    if signer != params.admin_account_id {
        return Err(SdkError::Mismatch {
            what: "host admin (refusing before signing: the signer is not the host admin)",
            expected: params.admin_account_id.to_string(),
            found: signer.to_string(),
        });
    }
    Ok(())
}

/// ADR-038: the protocol token must be a fungible definition owned by the token program.
pub fn check_protocol_token<C: Chain + ?Sized>(
    chain: &C,
    token_program: ProgramId,
    id: &AccountId,
) -> SdkResult<()> {
    let acc = chain.account(id)?;
    let fungible = acc.program_owner == token_program
        && matches!(
            TokenDefinition::try_from(&acc.data),
            Ok(TokenDefinition::Fungible { .. })
        );
    if fungible {
        return Ok(());
    }
    Err(SdkError::Refused(format!(
        "refusing before signing: the protocol token {id} is not a fungible token definition of \
         the token program (ADR-038); create it first with `dev token-new`"
    )))
}

/// An omitted slot flag disables the slot on chain, so a change that would do so needs its explicit
/// `--disable-*` flag.
fn guard_slot(
    what: &str,
    flag: &str,
    (old, new): (Option<AccountId>, Option<AccountId>),
    disable: bool,
) -> SdkResult<()> {
    if old.is_some() && new.is_none() && !disable {
        return Err(SdkError::Refused(format!(
            "refusing before signing: this would disable the {what} the config has; name it \
             again or pass --{flag} to confirm"
        )));
    }
    Ok(())
}

/// `old -> new` of a slot, shown before signing whenever it changes.
fn slot_change(what: &'static str, old: Option<AccountId>, new: Option<AccountId>) -> Option<Fact> {
    if old == new {
        return None;
    }
    let show = |s: Option<AccountId>| s.map_or_else(|| "none".to_owned(), |i| i.to_string());
    Some(fact(what, format!("{} -> {}", show(old), show(new))))
}

/// `admin set-engine-params`: the parameters come from `req.params` (`params::engine_params`).
pub fn prepare_set_engine_params<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    req: &SetEngineParamsRequest,
    accounts: &Accounts<'_>,
    now_ms: u64,
) -> SdkResult<Prepared> {
    prepare_set_engine_params_with(
        chain,
        v,
        &EngineParamsSlots::from(req),
        |token, oracle| params::engine_params(&req.params, token, oracle),
        accounts,
        now_ms,
    )
}

/// [`prepare_set_engine_params`] with the parameters from `load(protocol_token_enabled,
/// ref_oracle_enabled)` instead of a file (the app passes `demo::engine_params()`). `load` runs
/// after the signer and slot checks, with the slot flags of the resolved slots; its result is
/// validated by the engine's own rules before anything is built.
pub fn prepare_set_engine_params_with<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    req: &EngineParamsSlots,
    load: impl FnOnce(bool, bool) -> SdkResult<EngineParamsArgs>,
    accounts: &Accounts<'_>,
    _now_ms: u64,
) -> SdkResult<Prepared> {
    let admin = accounts.id(&req.admin)?;
    require_engine_admin(v, admin)?;
    let token = accounts.optional(req.protocol_token.as_deref())?;
    let oracle = accounts.optional(req.ref_oracle.as_deref())?;
    let old_token = v.state.protocol_token;
    let old_oracle = v.state.protocol_token_oracle_id;
    guard_slot(
        "protocol token",
        "disable-protocol-token",
        (old_token, token),
        req.disable_protocol_token,
    )?;
    guard_slot(
        "ref oracle",
        "disable-ref-oracle",
        (old_oracle, oracle),
        req.disable_ref_oracle,
    )?;
    if let Some(t) = token {
        check_protocol_token(chain, v.state.token_program_id, &t)?;
    }
    let mut facts: Vec<Fact> = [
        slot_change("protocol token", old_token, token),
        slot_change("ref oracle", old_oracle, oracle),
    ]
    .into_iter()
    .flatten()
    .collect();
    let p = load(token.is_some(), oracle.is_some())?;
    // Same rules the engine applies at config time (ADR-024); `params::engine_params` already ran
    // them on a file, a caller-built value has not.
    p.validate()
        .map_err(|e| SdkError::Refused(format!("invalid engine parameters: {e}")))?;
    if p.protocol_token_ref_price != v.state.protocol_token_ref_price {
        facts.push(fact(
            "ref price change",
            format!(
                "{} -> {} (record it in DEPLOYMENTS.md, ADR-017)",
                v.state.protocol_token_ref_price, p.protocol_token_ref_price
            ),
        ));
    }
    let plan = plan::set_engine_params(v, admin, &p, token, oracle)?;
    Ok(Prepared::one(plan, "set_engine_params", CHECK_VERIFY).with_facts(facts))
}
