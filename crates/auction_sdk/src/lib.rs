//! Client SDK of the RFP-014 auction engine (spec §6.1): own instruction encoders keyed by enum index,
//! PDAs and decoders, quotes on the program's own pricing path, plan builders, program-id
//! verification, nonce-based confirmation and block-scan discovery. Public transactions only.
#![forbid(unsafe_code)]

pub mod chain;
pub mod encode;
pub mod error;
pub mod plan;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use chain::{BlockSummary, Chain, TxSummary};
pub use error::{SdkError, SdkResult};
pub use plan::Plan;
