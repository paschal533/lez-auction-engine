//! `view`: read-only questions about the deployment. Task 9 adds the rest of the views.

use auction_sdk::{verify::verify_chain_identity, SdkError};
use serde_json::{json, Value};

use crate::{ctx::Ctx, envelope::FfiError};

/// The `view` op: dispatches on `"view"`.
pub fn handle(req: Value) -> Result<Value, FfiError> {
    let name = req
        .get("view")
        .and_then(Value::as_str)
        .ok_or_else(|| FfiError::bad("the request names no \"view\""))?;
    match name {
        "deployment" => deployment(&Ctx::from_request(&req)?),
        other => Err(FfiError::bad(format!("unknown view {other}"))),
    }
}

/// `{network, dev, config, genesis_ok}`. `genesis_ok` is the chain-identity check (false when the
/// deployment records no genesis hash or the node's block 1 differs); `dev` needs a named dev
/// network as well. A node that cannot be read is an error, not `false`.
pub fn deployment(ctx: &Ctx) -> Result<Value, FfiError> {
    let genesis_ok = match verify_chain_identity(ctx.chain.as_ref(), &ctx.deployment) {
        Ok(()) => true,
        Err(SdkError::Mismatch { .. }) => false,
        Err(e) => return Err(e.into()),
    };
    Ok(json!({
        "network": ctx.deployment.network,
        "dev": ctx.is_dev_network() && genesis_ok,
        "config": ctx.config_id().map(|c| c.to_string()),
        "genesis_ok": genesis_ok,
    }))
}
