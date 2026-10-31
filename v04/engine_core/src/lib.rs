//! Data model of the LEZ 0.4 auction engine (design note §3, §4): actor states, the message set,
//! actor/PDA helpers and the checks every handler starts with. No handler code lives here, so the
//! crate builds for the host and for the riscv guest alike.
#![forbid(unsafe_code)]

pub mod actors;
pub mod check;
pub mod msg;
pub mod state;
