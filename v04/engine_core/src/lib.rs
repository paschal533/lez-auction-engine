//! Data model of the LEZ 0.4 auction engine (design note §3, §4): actor states, the message set,
//! actor/PDA helpers, the checks every handler starts with, and the pure v0.2.4 computations the
//! handlers call (`feed`, `liquidation`). No handler code lives here, so the crate builds for the
//! host and for the riscv guest alike.
#![forbid(unsafe_code)]

pub mod actors;
pub mod bid;
pub mod check;
pub mod feed;
pub mod liquidation;
pub mod msg;
pub mod scratch;
pub mod state;
