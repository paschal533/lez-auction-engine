//! Pure, LEZ-independent logic for the RFP-014 liquidation & auction engine.
//! Every state transition returns a new value; nothing here performs I/O.
#![cfg_attr(not(test), no_std)] // unit tests (proptest) need std; the library itself is no_std
#![forbid(unsafe_code)]
#![deny(clippy::arithmetic_side_effects)]

pub mod accounting;
pub mod auction;
pub mod error;
pub mod liquidation;
pub mod math;
pub mod pause;
pub mod price;
pub mod schedule;
pub mod system;
pub mod units;
