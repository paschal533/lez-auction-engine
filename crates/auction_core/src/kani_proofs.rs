#[cfg(kani)]
#[allow(clippy::arithmetic_side_effects)]
mod proofs {
    use crate::math::*;
    #[kani::proof]
    fn mul_div_floor_never_exceeds_ceil() {
        let (x, y, d): (u128, u128, u128) = (kani::any(), kani::any(), kani::any());
        kani::assume(d > 0 && x < (1 << 64) && y < (1 << 64));
        if let (Ok(f), Ok(c)) = (mul_div_floor(x, y, d), mul_div_ceil(x, y, d)) {
            assert!(f <= c && c - f <= 1);
        }
    }
    #[kani::proof]
    #[kani::unwind(3)]
    fn schedule_bounded() {
        let t: u64 = kani::any();
        let s = crate::schedule::rai_default();
        assert!(s.discount_ray(t) <= s.max_discount_ray());
    }
}
