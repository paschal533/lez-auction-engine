//! `mul_div_floor` / `mul_div_ceil` with exactly `auction_core::math`'s results, cheaper in the
//! zkVM. `auction_core`'s versions (frozen, ADR-069) always run a 128-step bit-serial division
//! on u128, which costs thousands of cycles on the 32-bit guest. Two native routes avoid it:
//!
//! 1. `x · y` fits in a u128: the quotient is the native `x · y / d`.
//! 2. Otherwise split one factor by the divisor, `x = q · d + r`: then
//!    `x · y / d = q · y + r · y / d` exactly, and it is native whenever `r · y` fits. It always
//!    fits when `d` and `y` are below 2^64, which is every TWAP term (price · weight / total).
//!
//! Only a product that fits neither way takes the frozen path. Pinned by the differential tests
//! below.

use auction_core::math::{self as core_math, MathError};

/// `(floor(x · y / d), whether the division was exact)` without the frozen path, or `None` when
/// neither native route applies. `d` is nonzero.
fn native(x: u128, y: u128, d: u128) -> Option<Result<(u128, bool), MathError>> {
    if let Some(p) = x.checked_mul(y) {
        return Some(Ok((p.checked_div(d)?, p.checked_rem(d)? == 0)));
    }
    split(x, y, d).or_else(|| split(y, x, d))
}

/// Route 2 with `a` as the split factor: `a = q · d + r`, so `a · b / d = q · b + r · b / d`.
/// `None` when `r · b` does not fit. A `q · b` or a sum past u128 is a quotient past u128.
fn split(a: u128, b: u128, d: u128) -> Option<Result<(u128, bool), MathError>> {
    let rem = a.checked_rem(d)?.checked_mul(b)?;
    let whole = a.checked_div(d)?.checked_mul(b);
    let sum = whole.and_then(|w| w.checked_add(rem.checked_div(d)?));
    Some(
        sum.map(|q| (q, rem.checked_rem(d) == Some(0)))
            .ok_or(MathError::Overflow),
    )
}

/// `floor(x · y / d)`, equal to `auction_core::math::mul_div_floor` on every input.
///
/// # Errors
/// `DivByZero` when `d == 0`; `Overflow` when the quotient exceeds u128.
pub fn mul_div_floor(x: u128, y: u128, d: u128) -> Result<u128, MathError> {
    if d == 0 {
        return Err(MathError::DivByZero);
    }
    match native(x, y, d) {
        Some(r) => r.map(|(q, _)| q),
        None => core_math::mul_div_floor(x, y, d),
    }
}

/// `ceil(x · y / d)`, equal to `auction_core::math::mul_div_ceil` on every input.
///
/// # Errors
/// `DivByZero` when `d == 0`; `Overflow` when the quotient exceeds u128.
pub fn mul_div_ceil(x: u128, y: u128, d: u128) -> Result<u128, MathError> {
    if d == 0 {
        return Err(MathError::DivByZero);
    }
    match native(x, y, d) {
        Some(Ok((q, true))) => Ok(q),
        Some(Ok((q, false))) => q.checked_add(1).ok_or(MathError::Overflow),
        Some(Err(e)) => Err(e),
        None => core_math::mul_div_ceil(x, y, d),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Values that exercise both paths: tiny, around 2^64, RAY-scale, near u128::MAX, or free.
    fn operand() -> impl Strategy<Value = u128> {
        prop_oneof![
            0u128..=3,
            Just(u128::from(u64::MAX)),
            Just(u128::from(u64::MAX).saturating_add(1)),
            Just(1_000_000_000_000_000_000_000_000_000u128),
            Just(u128::MAX),
            Just(u128::MAX - 1),
            any::<u128>(),
            0u128..=(1 << 64),
            0u128..=(1 << 100),
        ]
    }

    #[test]
    fn edge_cases_match_auction_core() {
        let cases = [
            (0, 0, 0),
            (1, 1, 0),
            (7, 3, 2),
            (6, 3, 2),
            (u128::MAX, 1, 1),
            (u128::MAX, 2, 2),
            (u128::MAX, u128::MAX, 1),
            (u128::MAX, u128::MAX, u128::MAX),
            (u128::MAX, 1, u128::MAX),
            (1 << 64, 1 << 64, 1),
            (1 << 64, 1 << 63, 3),
            // Route 2: the product overflows, the divisor and one factor are below 2^64.
            (u128::MAX, 1_800_000, 1_800_001),
            (u128::MAX, u128::from(u64::MAX), u128::from(u64::MAX)),
            (u128::from(u64::MAX), u128::MAX, u128::from(u64::MAX)),
            (u128::MAX - 1, 7, 3),
            (u128::MAX, 3, 2),
            (u128::MAX, u128::MAX, u128::MAX - 1),
        ];
        for (x, y, d) in cases {
            assert_eq!(
                mul_div_floor(x, y, d),
                core_math::mul_div_floor(x, y, d),
                "floor {x} {y} {d}"
            );
            assert_eq!(
                mul_div_ceil(x, y, d),
                core_math::mul_div_ceil(x, y, d),
                "ceil {x} {y} {d}"
            );
        }
    }

    /// A TWAP term at any price: a RAY-scale price, a weight and a total weight below 2^64.
    #[test]
    fn a_twap_term_never_takes_the_frozen_path() {
        for (p, w, total) in [
            (u128::MAX, 1, 1),
            (u128::MAX, 1_799_999, 1_800_000),
            (u128::MAX, u128::from(u64::MAX) - 1, u128::from(u64::MAX)),
            (1 << 127, 300_000, 1_800_000),
        ] {
            assert!(p.checked_mul(w).is_none() || w == 1, "{p} {w}: not route 1");
            assert!(native(p, w, total).is_some(), "{p} {w} {total}");
            assert_eq!(
                mul_div_floor(p, w, total),
                core_math::mul_div_floor(p, w, total),
                "{p} {w} {total}"
            );
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(4096))]
        #[test]
        fn equals_auction_core(x in operand(), y in operand(), d in operand()) {
            prop_assert_eq!(mul_div_floor(x, y, d), core_math::mul_div_floor(x, y, d));
            prop_assert_eq!(mul_div_ceil(x, y, d), core_math::mul_div_ceil(x, y, d));
        }

        /// Route 2's own range: a wide factor, the other factor and the divisor below 2^64.
        #[test]
        fn split_route_equals_auction_core(
            x in (1u128 << 100)..=u128::MAX,
            y in 0u128..=(1 << 64),
            d in 1u128..=(1 << 64),
            swap in any::<bool>(),
        ) {
            let (a, b) = if swap { (y, x) } else { (x, y) };
            prop_assert_eq!(mul_div_floor(a, b, d), core_math::mul_div_floor(a, b, d));
            prop_assert_eq!(mul_div_ceil(a, b, d), core_math::mul_div_ceil(a, b, d));
        }
    }
}
