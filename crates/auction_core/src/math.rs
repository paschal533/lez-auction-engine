//! 256-bit-intermediate multiply-divide. Never write `a * b / c` on u128 elsewhere.
#![allow(clippy::arithmetic_side_effects)] // every operation below is bounded; see comments

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathError {
    DivByZero,
    Overflow,
}

const LO64: u128 = u64::MAX as u128;

/// Full 256-bit product of two u128s as (hi, lo).
pub fn mul_wide(a: u128, b: u128) -> (u128, u128) {
    let (a1, a0) = (a >> 64, a & LO64);
    let (b1, b0) = (b >> 64, b & LO64);
    // Each partial product is a u64*u64 and fits in u128.
    let p00 = a0 * b0;
    let p01 = a0 * b1;
    let p10 = a1 * b0;
    let p11 = a1 * b1;
    // mid ≤ 3·(2^64−1) < 2^66, so it fits.
    let mid = (p00 >> 64) + (p01 & LO64) + (p10 & LO64);
    let lo = (p00 & LO64) | (mid << 64);
    let hi = p11 + (p01 >> 64) + (p10 >> 64) + (mid >> 64);
    (hi, lo)
}

/// (hi·2^128 + lo) / d as (quotient, remainder). Requires hi < d so the quotient fits in u128.
pub fn div_wide(hi: u128, lo: u128, d: u128) -> Result<(u128, u128), MathError> {
    if d == 0 {
        return Err(MathError::DivByZero);
    }
    if hi >= d {
        return Err(MathError::Overflow);
    }
    let mut rem = hi; // invariant: rem < d at the top of each iteration
    let mut q: u128 = 0;
    for i in (0..128).rev() {
        let carry = rem >> 127;
        rem = (rem << 1) | ((lo >> i) & 1);
        q <<= 1;
        // The true value is carry·2^128 + rem < 2d; subtracting d once restores rem < d.
        if carry == 1 || rem >= d {
            rem = rem.wrapping_sub(d);
            q |= 1;
        }
    }
    Ok((q, rem))
}

/// floor(x·y / d)
pub fn mul_div_floor(x: u128, y: u128, d: u128) -> Result<u128, MathError> {
    let (hi, lo) = mul_wide(x, y);
    div_wide(hi, lo, d).map(|(q, _)| q)
}

/// ceil(x·y / d)
pub fn mul_div_ceil(x: u128, y: u128, d: u128) -> Result<u128, MathError> {
    let (hi, lo) = mul_wide(x, y);
    let (q, r) = div_wide(hi, lo, d)?;
    if r == 0 {
        Ok(q)
    } else {
        q.checked_add(1).ok_or(MathError::Overflow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use ruint::aliases::U256;

    #[test]
    fn small_values() {
        assert_eq!(mul_div_floor(7, 3, 2), Ok(10));
        assert_eq!(mul_div_ceil(7, 3, 2), Ok(11));
        assert_eq!(mul_div_ceil(6, 3, 2), Ok(9));
    }

    #[test]
    fn product_beyond_u128_but_quotient_fits() {
        // 1e27 * 2000e18 / 1e27 = 2000e18; the product is 2e48 > u128::MAX
        let ray = crate::units::RAY;
        let x = 2_000_000_000_000_000_000_000u128;
        assert_eq!(mul_div_floor(x, ray, ray), Ok(x));
    }

    #[test]
    fn errors() {
        assert_eq!(mul_div_floor(1, 1, 0), Err(MathError::DivByZero));
        assert_eq!(
            mul_div_floor(u128::MAX, u128::MAX, 1),
            Err(MathError::Overflow)
        );
        assert_eq!(mul_div_ceil(u128::MAX, 1, 1), Ok(u128::MAX));
        assert_eq!(mul_div_ceil(u128::MAX, 2, 2), Ok(u128::MAX));
    }

    #[test]
    fn overflow_with_small_divisor() {
        // Small divisor with very large operands overflows the quotient
        assert_eq!(
            mul_div_floor(u128::MAX, u128::MAX, 3),
            Err(MathError::Overflow)
        );
    }

    #[test]
    fn large_divisor() {
        let large_d = 1u128 << 127;
        // (2^127) * 4 / 2^127 = 4
        assert_eq!(mul_div_floor(large_d, 4, large_d), Ok(4));
        // u128::MAX * 2^127 / 2^127 = u128::MAX
        assert_eq!(mul_div_floor(u128::MAX, large_d, large_d), Ok(u128::MAX));
    }

    #[test]
    fn divisor_is_max() {
        // u128::MAX * u128::MAX / u128::MAX = u128::MAX (exact)
        assert_eq!(
            mul_div_floor(u128::MAX, u128::MAX, u128::MAX),
            Ok(u128::MAX)
        );
    }

    #[test]
    fn exact_multiples() {
        // Exact division: no remainder, floor = ceil
        assert_eq!(mul_div_floor(6, 10, 4), Ok(15));
        assert_eq!(mul_div_ceil(6, 10, 4), Ok(15));
    }

    #[test]
    fn ceil_overflow_with_remainder() {
        // Construct x·y such that dividing by d yields q = u128::MAX with r = 1.
        // Use x = 7, y = 2·(u128::MAX / 7) + 1, d = 2.
        // Reasoning: 2^128 ≡ 4 (mod 7), so 2^129 - 1 ≡ 0 (mod 7).
        // Thus y = (2^129 - 1) / 7 exactly, and x·y = 2^129 - 1.
        // Dividing by 2: q = u128::MAX, r = 1.
        // ceil adds 1 to u128::MAX, causing overflow.
        let x = 7u128;
        let y = 2 * (u128::MAX / 7) + 1;
        let d = 2u128;
        assert_eq!(mul_div_floor(x, y, d), Ok(u128::MAX));
        assert_eq!(mul_div_ceil(x, y, d), Err(MathError::Overflow));
    }

    fn reference(x: u128, y: u128, d: u128) -> Option<(u128, u128)> {
        let p = U256::from(x) * U256::from(y);
        let (q, r) = p.div_rem(U256::from(d));
        if q > U256::from(u128::MAX) {
            return None;
        }
        Some((q.to::<u128>(), r.to::<u128>()))
    }

    proptest! {
        #[test]
        fn matches_u256_reference(x: u128, y: u128, d in 1u128..) {
            match reference(x, y, d) {
                None => prop_assert_eq!(mul_div_floor(x, y, d), Err(MathError::Overflow)),
                Some((q, r)) => {
                    prop_assert_eq!(mul_div_floor(x, y, d), Ok(q));
                    let c = if r == 0 { Some(q) } else { q.checked_add(1) };
                    match c {
                        Some(c) => prop_assert_eq!(mul_div_ceil(x, y, d), Ok(c)),
                        None => prop_assert_eq!(mul_div_ceil(x, y, d), Err(MathError::Overflow)),
                    }
                }
            }
        }
    }
}
