//! Kani harnesses for `risk_core` (run with `cargo kani -p risk_core`; Kani is not part of the
//! normal build). Written, not run, until a Kani toolchain is available (ADR-015).

#[cfg(kani)]
mod proofs {
    use crate::feed::{FeedFault, FeedPolicy, validate_feed_ext};
    use auction_core::price::{FeedParams, OracleReading, OracleScale, validate_feed};

    /// Under the legacy policy, `validate_feed_ext` equals v0.2.4's `validate_feed`.
    #[kani::proof]
    fn validate_feed_ext_equals_validate_feed_legacy() {
        let bound: u128 = 1 << 96;
        let (price_raw, confidence_raw, r): (u128, u128, u128) =
            (kani::any(), kani::any(), kani::any());
        kani::assume(price_raw < bound && confidence_raw < bound && r < bound);
        let p = FeedParams {
            scale: if kani::any() {
                OracleScale::Q64x64
            } else {
                OracleScale::Ray
            },
            max_age_ms: kani::any(),
            max_conf_bps: kani::any(),
            band_lo_bps: kani::any(),
            band_hi_bps: kani::any(),
        };
        let reading = OracleReading {
            price_raw,
            confidence_raw,
            timestamp_ms: kani::any(),
        };
        let now: u64 = kani::any();
        assert_eq!(
            validate_feed_ext(&reading, &FeedPolicy::legacy(&p), r, now)
                .map_err(FeedFault::pause_reason),
            validate_feed(&reading, &p, r, now)
        );
    }

    /// The legacy sanity bounds equal v0.2.4's `R / 2` and `R.saturating_mul(2)` for every `R`,
    /// including the `hi` saturation the bounded harness above cannot reach.
    #[kani::proof]
    fn legacy_sanity_bounds_equal_v024() {
        use crate::feed::{LEGACY_SANITY_HI_BPS, LEGACY_SANITY_LO_BPS};
        use auction_core::{math::mul_div_floor, units::BPS};
        let r: u128 = kani::any();
        assert_eq!(
            mul_div_floor(r, u128::from(LEGACY_SANITY_LO_BPS), BPS).unwrap_or(0),
            r / 2
        );
        assert_eq!(
            mul_div_floor(r, u128::from(LEGACY_SANITY_HI_BPS), BPS).unwrap_or(u128::MAX),
            r.saturating_mul(2)
        );
    }

    /// For any ring of up to 3 samples (strictly increasing times, prices below `2^64`) and any
    /// query, a returned TWAP lies in `[min - 7, max]` of the ring's prices (spec §3.1.3).
    #[kani::proof]
    #[kani::unwind(9)]
    fn twap_bounded_by_samples() {
        use crate::twap::{Sample, TwapRing, twap};
        use alloc::vec::Vec;
        let len: usize = kani::any();
        kani::assume(len >= 1 && len <= 3);
        let mut samples: Vec<Sample> = Vec::new();
        let mut ts: u64 = kani::any();
        let (mut lo_p, mut hi_p) = (u128::MAX, 0u128);
        for i in 0..len {
            if i != 0 {
                let gap: u64 = kani::any();
                kani::assume(gap >= 1);
                let Some(next) = ts.checked_add(gap) else {
                    return;
                };
                ts = next;
            }
            let price_ray: u128 = kani::any();
            kani::assume(price_ray < 1 << 64);
            lo_p = lo_p.min(price_ray);
            hi_p = hi_p.max(price_ray);
            samples.push(Sample {
                ts_ms: ts,
                price_ray,
            });
        }
        let ring = TwapRing {
            samples,
            last_conf_bps: 0,
        };
        if let Some((t, _)) = twap(&ring, kani::any(), kani::any(), kani::any(), kani::any()) {
            assert!(lo_p.saturating_sub(7) <= t && t <= hi_p);
        }
    }
}
