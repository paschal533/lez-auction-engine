//! LEZ-free parts of the `auction_engine` program: argument validation, error codes and PDA seed
//! bytes. No dependency on `lee` / `lee_core`, so the v0.2.4 engine core and the v0.4 workspace share them.
#![forbid(unsafe_code)]

pub mod args;
pub mod error;
pub mod seeds;

pub use error::EngineError;
