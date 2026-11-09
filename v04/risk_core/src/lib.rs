//! Tier 2 risk logic of the LEZ 0.4 auction engine (spec `2026-11-08-tier2-engine-design.md`):
//! pure, LEZ-free, and v04-only, so the frozen v0.2.4 sources stay untouched (ADR-069).
#![cfg_attr(not(test), no_std)] // unit tests (proptest) need std; the library itself is no_std
#![forbid(unsafe_code)]

pub mod error;
pub mod feed;
mod kani_proofs;

pub use error::RiskError;
pub use feed::{
    FeedFault, FeedPolicy, LEGACY_SANITY_HI_BPS, LEGACY_SANITY_LO_BPS, validate_feed_ext,
};
