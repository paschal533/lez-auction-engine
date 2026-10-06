//! Host-testable handlers of the `auction_engine` program: a thin layer over `auction_core`.
//! Handlers validate accounts, call the core, and return post-states (one per input account,
//! in order) plus chained calls. Checks → effects → chained calls.
#![forbid(unsafe_code)]

pub mod accounts;
pub mod admin;
#[cfg(test)]
mod admin_tests;
pub mod bid;
#[cfg(test)]
mod bid_tests;
#[cfg(test)]
mod collateral_tests;
pub mod feed;
pub mod liquidate;
pub mod observe;
pub mod settle;
#[cfg(test)]
mod settle_tests;
pub mod surplus;
#[cfg(test)]
mod surplus_tests;
pub mod system;
pub mod token_calls;

use auction_engine_core::EngineError;
use lee_core::program::{AccountPostState, ChainedCall};

pub type EngineResult = Result<(Vec<AccountPostState>, Vec<ChainedCall>), EngineError>;
