//! One error type for the SDK. Messages name accounts and programs, never key material.

use auction_engine_core::EngineError;

#[derive(Debug, thiserror::Error)]
pub enum SdkError {
    #[error("instruction encoding failed: {0}")]
    Encode(String),
    #[error("account {id} ({what}) is not initialised")]
    Missing { what: &'static str, id: String },
    #[error("account {id} is not a valid {what}: {reason}")]
    Decode {
        what: &'static str,
        id: String,
        reason: String,
    },
    #[error(
        "{what}: expected {expected}, found {found}; refusing to build a transaction (I4, ADR-027)"
    )]
    Mismatch {
        what: &'static str,
        expected: String,
        found: String,
    },
    #[error("the engine would reject this: {name} (code {code})")]
    Engine { name: String, code: u32 },
    #[error("the host would reject this: {0}")]
    HostReject(String),
    #[error("deployment file {path}: {reason}")]
    Deployment { path: String, reason: String },
    #[error("chain: {0}")]
    Chain(String),
    #[error("signer {signer} already has a transaction in flight (lock {lock}); it resolves within {blocks} blocks")]
    InFlight {
        signer: String,
        lock: String,
        blocks: u64,
    },
    #[error("scanner: {0}")]
    Scan(String),
    #[error("{path}: {reason}")]
    Io { path: String, reason: String },
}

impl From<EngineError> for SdkError {
    fn from(e: EngineError) -> Self {
        Self::Engine {
            name: e.to_string(),
            code: e.code(),
        }
    }
}

pub type SdkResult<T> = Result<T, SdkError>;
