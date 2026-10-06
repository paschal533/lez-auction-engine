//! Host-side embedding of the auction_engine guest: `AUCTION_ENGINE_ELF`, `AUCTION_ENGINE_ID`.
#![forbid(unsafe_code)]
include!(concat!(env!("OUT_DIR"), "/methods.rs"));
