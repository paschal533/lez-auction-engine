//! Seed-byte helpers of the engine PDAs (Shared Reference R1). Encoding = SPEL's (ADR-020, verified
//! in S3): literal → UTF-8 zero-padded to 32 bytes, u64 → little-endian zero-padded, several seeds →
//! SHA-256 of the concatenation. LEZ-free, so the v0.2.4 and v0.4 workspaces share one encoding.

use risc0_zkvm::sha::{Impl, Sha256 as _};

#[must_use]
pub fn seed_from_str(s: &str) -> [u8; 32] {
    let src = s.as_bytes();
    assert!(src.len() <= 32, "seed literal exceeds 32 bytes");
    let mut out = [0u8; 32];
    out[..src.len()].copy_from_slice(src);
    out
}

#[must_use]
pub fn u64_seed(n: u64) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[..8].copy_from_slice(&n.to_le_bytes());
    out
}

#[must_use]
pub fn combine_seeds(seeds: &[&[u8; 32]]) -> [u8; 32] {
    assert!(!seeds.is_empty(), "a PDA needs at least one seed");
    if seeds.len() == 1 {
        return *seeds[0];
    }
    let mut buf = Vec::with_capacity(seeds.len() * 32);
    for s in seeds {
        buf.extend_from_slice(*s);
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(Impl::hash_bytes(&buf).as_bytes());
    out
}
