//! LEZ-free parts of the `auction_engine` program: argument validation, error codes and PDA seed
//! bytes. No dependency on `lee` / `lee_core`, so the v0.2.4 engine core and the v0.4 workspace share them.
//!
//! `args` and `error` are compiled straight from the v0.2.4 engine core's sources (`#[path]`), so
//! those files, and with them the committed v0.2.4 guest, stay byte-identical (ruling R-F1: the
//! guest embeds their paths and line numbers). Anything params-only lives in its own module here.
#![forbid(unsafe_code)]

#[path = "../../../programs/auction_engine/core/src/args.rs"]
pub mod args;
#[path = "../../../programs/auction_engine/core/src/error.rs"]
pub mod error;
pub mod seeds;

#[cfg(feature = "borsh")]
mod wire;

pub use error::EngineError;
