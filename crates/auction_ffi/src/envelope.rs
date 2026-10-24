//! The JSON envelope every `auction_ffi_*` call returns, and the error kinds inside it.
//!
//! Success: `{"ok":true,"value":...}`. Failure: `{"ok":false,"error":{"kind":...,"message":...}}`
//! where `kind` is the snake_case name of an `SdkError` variant, or `bad_request` (the request itself
//! is unusable) or `panic` (the library panicked; the panic never crosses the C boundary).

use std::panic::{catch_unwind, AssertUnwindSafe};

use auction_sdk::SdkError;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

pub const KIND_BAD_REQUEST: &str = "bad_request";
pub const KIND_PANIC: &str = "panic";

#[derive(Debug)]
pub enum FfiError {
    /// The request is null, not UTF-8, not JSON, or lacks or mistypes a field.
    BadRequest(String),
    Sdk(SdkError),
}

impl FfiError {
    pub fn bad(message: impl Into<String>) -> Self {
        Self::BadRequest(message.into())
    }

    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => KIND_BAD_REQUEST,
            Self::Sdk(e) => kind_of(e),
        }
    }

    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::BadRequest(m) => m.clone(),
            Self::Sdk(e) => e.to_string(),
        }
    }
}

impl From<SdkError> for FfiError {
    fn from(e: SdkError) -> Self {
        Self::Sdk(e)
    }
}

/// The snake_case name of an `SdkError` variant. Exhaustive on purpose: a new variant must get a kind.
#[must_use]
pub fn kind_of(e: &SdkError) -> &'static str {
    match e {
        SdkError::Encode(_) => "encode",
        SdkError::Missing { .. } => "missing",
        SdkError::Decode { .. } => "decode",
        SdkError::Mismatch { .. } => "mismatch",
        SdkError::Engine { .. } => "engine",
        SdkError::HostReject(_) => "host_reject",
        SdkError::Deployment { .. } => "deployment",
        SdkError::Chain(_) => "chain",
        SdkError::Keystore(_) => "keystore",
        SdkError::InFlight { .. } => "in_flight",
        SdkError::SendUncertain(_) => "send_uncertain",
        SdkError::Unconfirmed(_) => "unconfirmed",
        SdkError::Scan(_) => "scan",
        SdkError::Io { .. } => "io",
    }
}

/// The envelope for one result.
#[must_use]
pub fn to_value(result: Result<Value, FfiError>) -> Value {
    match result {
        Ok(value) => json!({ "ok": true, "value": value }),
        Err(e) => error_value(e.kind(), &e.message()),
    }
}

fn error_value(kind: &str, message: &str) -> Value {
    json!({ "ok": false, "error": { "kind": kind, "message": message } })
}

/// Deserializes part of a request; a mismatch is a `bad_request` naming the problem.
pub fn parse<T: DeserializeOwned>(v: &Value) -> Result<T, FfiError> {
    serde_json::from_value(v.clone()).map_err(|e| FfiError::bad(e.to_string()))
}

/// A `bad_request` reply, for a request that never became JSON.
#[must_use]
pub fn reject(message: &str) -> String {
    error_value(KIND_BAD_REQUEST, message).to_string()
}

/// Parses the request, runs `op` on it and returns the serialized envelope. `None` is a null request.
/// A panic inside `op` is caught and reported as `kind: "panic"`.
pub fn run(req: Option<&str>, op: impl FnOnce(Value) -> Result<Value, FfiError>) -> String {
    let Some(text) = req else {
        return reject("the request is null");
    };
    let value: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return reject(&format!("the request is not JSON: {e}")),
    };
    match catch_unwind(AssertUnwindSafe(|| op(value))) {
        Ok(result) => to_value(result).to_string(),
        Err(payload) => error_value(KIND_PANIC, &panic_message(payload.as_ref())).to_string(),
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "the library panicked".to_owned())
}
