//! `mul_div_floor` / `mul_div_ceil` with exactly `auction_core::math`'s results, cheaper in the
//! zkVM. `auction_core`'s versions (frozen, ADR-069) always run a 128-step bit-serial division
//! on u128, which costs thousands of cycles on the 32-bit guest. When `x · y` fits in a u128 the
//! 256-bit product's high half is 0, so the quotient is the native `x · y / d`; only a product
//! beyond u128 takes the frozen path. Pinned by the differential tests below.

use auction_core::math::{self as core_math, MathError};

/// `floor(x · y / d)`, equal to `auction_core::math::mul_div_floor` on every input.
///
/// # Errors
/// `DivByZero` when `d == 0`; `Overflow` when the quotient exceeds u128.
pub fn mul_div_floor(x: u128, y: u128, d: u128) -> Result<u128, MathError> {
    match x.checked_mul(y) {
        Some(p) => p.checked_div(d).ok_or(MathError::DivByZero),
        None => core_math::mul_div_floor(x, y, d),
    }
}

/// `ceil(x · y / d)`, equal to `auction_core::math::mul_div_ceil` on every input.
///
/// # Errors
/// `DivByZero` when `d == 0`; `Overflow` when the quotient exceeds u128.
pub fn mul_div_ceil(x: u128, y: u128, d: u128) -> Result<u128, MathError> {
    let Some(p) = x.checked_mul(y) else {
        return core_math::mul_div_ceil(x, y, d);
    };
    let q = p.checked_div(d).ok_or(MathError::DivByZero)?;
    if p.checked_rem(d) == Some(0) {
        Ok(q)
    } else {
        q.checked_add(1).ok_or(MathError::Overflow)
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

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(4096))]
        #[test]
        fn equals_auction_core(x in operand(), y in operand(), d in operand()) {
            prop_assert_eq!(mul_div_floor(x, y, d), core_math::mul_div_floor(x, y, d));
            prop_assert_eq!(mul_div_ceil(x, y, d), core_math::mul_div_ceil(x, y, d));
        }
    }
}
