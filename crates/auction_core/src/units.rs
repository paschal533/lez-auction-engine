//! Fixed-point units shared by every module.
#![allow(clippy::arithmetic_side_effects)] // bps ≤ 10_000 times a constant: cannot overflow
pub const RAY: u128 = 1_000_000_000_000_000_000_000_000_000;
pub const BPS: u128 = 10_000;
/// 1 bps expressed in RAY (1e27 / 1e4).
pub const BPS_TO_RAY: u128 = 100_000_000_000_000_000_000_000;

/// Converts basis points to RAY. `bps` ≤ 10_000, so this never overflows.
pub const fn bps_to_ray(bps: u16) -> u128 {
    (bps as u128) * BPS_TO_RAY
}
