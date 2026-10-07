//! Host-side embedding of the attacker guests: `IMPOSTOR_ELF/ID`, `FLAWED_ENGINE_ELF/ID`.
#![forbid(unsafe_code)]
include!(concat!(env!("OUT_DIR"), "/methods.rs"));
