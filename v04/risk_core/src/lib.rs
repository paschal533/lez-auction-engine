//! Tier 2 risk logic of the LEZ 0.4 auction engine (spec `2026-11-08-tier2-engine-design.md`):
//! pure, LEZ-free, and v04-only, so the frozen v0.2.4 sources stay untouched (ADR-069).
#![cfg_attr(not(test), no_std)] // unit tests (proptest) need std; the library itself is no_std
#![forbid(unsafe_code)]

extern crate alloc;

pub mod args;
pub mod error;
pub mod feed;
mod kani_proofs;
pub mod pause;
#[cfg(feature = "presets")]
pub mod presets;
pub mod price;
pub mod stale;
pub mod twap;

pub use args::{
    DEV_MAX_BPS, DEV_MIN_BPS, RiskArgs, RiskArgsError, TWAP_MIN_OBS_MAX, TWAP_SPACING_MIN_MS,
    TWAP_WINDOW_MAX_MS, TWAP_WINDOW_MIN_MS,
};
pub use error::RiskError;
pub use feed::{
    FeedFault, FeedPolicy, LEGACY_SANITY_HI_BPS, LEGACY_SANITY_LO_BPS, validate_feed_ext,
};
pub use pause::{FeedCase, Gate, Level, Op, allowed};
pub use price::{conf_bps, edge, fallback_bid_price, liq_price, live_bid_price, within_deviation};
pub use stale::{fallback_twap, floored_price, stale_cap};
pub use twap::{Sample, TWAP_CAPACITY, TwapRing, record, twap};
