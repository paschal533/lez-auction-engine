//! `prepare`: builds the plans (`Plan` as JSON) the module signs and sends through `lez_core`.
//! This crate never signs.

use auction_sdk::{deployment::program_id_hex, keystore::Keystore, plan_host, Plan, SdkResult};
use lee_core::account::AccountId;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    ctx::Ctx,
    envelope::{parse, FfiError},
};

/// What `dev balance` takes; shown to the user as the way to check a transfer.
const CHECK_BALANCE: &str = "dev balance --account <account>";

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum Action {
    TokenTransfer {
        from: String,
        to: String,
        amount: String,
    },
}

/// The `prepare` op: `{"steps":[Plan,...],"facts":[]}`.
pub fn handle(ctx: &Ctx, req: &Value) -> Result<Value, FfiError> {
    match parse::<Action>(req)? {
        Action::TokenTransfer { from, to, amount } => token_transfer(ctx, &from, &to, &amount),
    }
}

fn token_transfer(ctx: &Ctx, from: &str, to: &str, amount: &str) -> Result<Value, FfiError> {
    let amount: u128 = amount
        .parse()
        .map_err(|_| FfiError::bad("amount must be a decimal integer"))?;
    ctx.require_dev()?;
    let (from, to) = (account(ctx, from)?, account(ctx, to)?);
    let plan = plan_host::token_transfer(ctx.deployment.token_program, from, to, amount)?;
    Ok(json!({ "steps": [plan_json(&plan, "transfer", CHECK_BALANCE)], "facts": [] }))
}

/// A base58 account id, or a wallet label (which needs the wallet).
fn account(ctx: &Ctx, arg: &str) -> SdkResult<AccountId> {
    if let Ok(id) = arg.parse::<AccountId>() {
        return Ok(id);
    }
    let path = Keystore::default_path(ctx.wallet_home.as_deref())?;
    Ok(Keystore::open(&path)?.resolve(arg)?)
}

/// The plan as the module consumes it: account ids as hex (the `lez_core` form), signers as base58
/// (the lock names).
#[must_use]
pub fn plan_json(plan: &Plan, what: &str, check: &str) -> Value {
    let accounts: Vec<Value> = plan
        .account_ids
        .iter()
        .map(|id| {
            json!({
                "id": hex::encode(id.value()),
                "signer": plan.signing_requirements.contains(id),
            })
        })
        .collect();
    let signers: Vec<String> = plan
        .signing_requirements
        .iter()
        .map(ToString::to_string)
        .collect();
    json!({
        "program_id": program_id_hex(&plan.program_id),
        "accounts": accounts,
        "instruction_words": plan.instruction_words,
        "signers": signers,
        "what": what,
        "check": check,
    })
}
