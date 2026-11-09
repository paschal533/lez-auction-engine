//! Tier 2 risk logic of the LEZ 0.4 auction engine (spec `2026-11-08-tier2-engine-design.md`):
//! pure, LEZ-free, and v04-only, so the frozen v0.2.4 sources stay untouched (ADR-069).
#![forbid(unsafe_code)]

pub mod error;

pub use error::RiskError;
