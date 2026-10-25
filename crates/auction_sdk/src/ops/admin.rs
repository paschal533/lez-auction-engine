//! Engine admin: `set_engine_params`, and the signer checks every admin command runs before signing.

use std::path::PathBuf;

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

pub fn prepare_set_engine_params<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    req: &SetEngineParamsRequest,
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
    let p = params::engine_params(&req.params, token.is_some(), oracle.is_some())?;
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
