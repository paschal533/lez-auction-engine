//! Transaction tracking across separate FFI calls (the library keeps no state between them): the
//! in-flight lock file lives on disk from `begin` until `end`; the caller carries the `Tracking`.

use std::path::Path;

use auction_sdk::confirm::{poll_once, track_begin, track_end, Outcome, Poll, Tracking};
use lee_core::account::AccountId;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    ctx::{Ctx, CtxJson, LOCK_DIR},
    envelope::{parse, FfiError},
};

#[derive(Deserialize)]
struct BeginReq {
    signer: String,
}

#[derive(Deserialize)]
struct PollReq {
    tracking: Tracking,
}

#[derive(Deserialize)]
struct EndReq {
    tracking: Tracking,
    outcome: String,
}

/// `tx_begin {ctx, signer}` -> the `Tracking`.
pub fn begin(ctx: &Ctx, req: &Value) -> Result<Value, FfiError> {
    let r: BeginReq = parse(req)?;
    let signer: AccountId = r
        .signer
        .parse()
        .map_err(|_| FfiError::bad("signer is not a base58 account id"))?;
    let t = track_begin(ctx.chain.as_ref(), &signer, &ctx.lock_dir(), &ctx.confirm)?;
    serde_json::to_value(&t).map_err(|e| FfiError::bad(e.to_string()))
}

/// `tx_poll {ctx, tracking}` -> pending, included or dropped. A failed read is a `chain` error, never
/// a dropped transaction.
pub fn poll(ctx: &Ctx, req: &Value) -> Result<Value, FfiError> {
    let r: PollReq = parse(req)?;
    check_lock_path(&ctx.state_dir, &r.tracking)?;
    Ok(
        match poll_once(ctx.chain.as_ref(), &r.tracking, &ctx.confirm)? {
            Poll::Pending { blocks_waited } => {
                json!({ "state": "pending", "blocks_waited": blocks_waited })
            }
            Poll::Done(Outcome::Included { nonce_after }) => {
                json!({ "state": "included", "nonce_after": nonce_after.to_string() })
            }
            Poll::Done(Outcome::Dropped { height }) => {
                json!({ "state": "dropped", "height": height })
            }
        },
    )
}

/// `tx_end {ctx, tracking, outcome}` -> `{}`. `included` and `dropped` release the lock; `unknown`
/// keeps it for the chain-fact staleness rule. Needs no node, so it takes the raw context.
pub fn end(ctx: &CtxJson, req: &Value) -> Result<Value, FfiError> {
    let r: EndReq = parse(req)?;
    check_lock_path(&ctx.state_dir, &r.tracking)?;
    let outcome = match r.outcome.as_str() {
        "included" => Some(Outcome::Included {
            nonce_after: r.tracking.nonce_before.saturating_add(1),
        }),
        "dropped" => Some(Outcome::Dropped {
            height: r.tracking.height_before,
        }),
        "unknown" => None,
        other => {
            return Err(FfiError::bad(format!(
                "outcome must be included, dropped or unknown, not {other:?}"
            )))
        }
    };
    track_end(&r.tracking, outcome.as_ref())?;
    Ok(json!({}))
}

/// The tracking comes from the caller, and `track_end` deletes `lock_path`: refuse any path that is
/// not this signer's lock file directly inside `<state_dir>/locks/`. Parents are compared
/// canonicalised, so `..` and symlinked directories do not get around it.
fn check_lock_path(state_dir: &Path, t: &Tracking) -> Result<(), FfiError> {
    let outside = || {
        FfiError::bad(format!(
            "tracking.lock_path {} is not a lock file in {}",
            t.lock_path.display(),
            state_dir.join(LOCK_DIR).display()
        ))
    };
    let locks = state_dir
        .join(LOCK_DIR)
        .canonicalize()
        .map_err(|_| outside())?;
    let parent = t
        .lock_path
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .ok_or_else(outside)?;
    let name_ok = t
        .lock_path
        .file_name()
        .is_some_and(|n| n.to_string_lossy() == format!("{}.lock", t.signer));
    if parent == locks && name_ok {
        Ok(())
    } else {
        Err(outside())
    }
}
