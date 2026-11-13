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

    /// Exact replacement for `auction_core::math::mul_div_floor` inside `twap_bounded_by_samples`
    /// only. `twap` calls `crate::math::mul_div_floor`, which divides natively when `x · y` fits
    /// a u128 and reaches `auction_core::math::mul_div_floor` only when it does not. Under this
    /// harness's bounds (prices below `2^64`, `u64` weights) the product always fits, so the
    /// native path runs and this stub only guards the fallback. The `assert!` makes any call that
    /// does reach it fail the proof rather than silently return `Overflow`. `d == 0` gives
    /// `DivByZero`, as the original does.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "d != 0 is checked; the product is checked"
    )]
    fn mul_div_floor_narrow(
        x: u128,
        y: u128,
        d: u128,
    ) -> Result<u128, auction_core::math::MathError> {
        if d == 0 {
            return Err(auction_core::math::MathError::DivByZero);
        }
        let n = x.checked_mul(y);
        assert!(
            n.is_some(),
            "mul_div_floor stub used outside its exact range"
        );
        n.map(|n| n / d)
            .ok_or(auction_core::math::MathError::Overflow)
    }

    /// For any ring of up to 3 samples (strictly increasing times, prices below `2^64`) and any
    /// query, a returned TWAP lies in `[min - 7, max]` of the prices of the samples it actually
    /// used (spec §3.1.3). The used set is rebuilt here with the span rule of the proptest's
    /// reference model. Each sample is current from its own time to `min(next.ts, anchor)`, or
    /// to the anchor for the newest, clipped below at `anchor - window`. A non-newest sample
    /// current for longer than `max_age` cuts itself and everything older.
    ///
    /// Unwinding: `unwind(9)` bounds the ring loops only, which have at most 3 iterations here.
    /// The 128-iteration loop in `auction_core::math::div_wide` is not entered: `twap`'s
    /// `crate::math::mul_div_floor` takes its native path for these products, and its fallback
    /// to `auction_core::math::mul_div_floor` is stubbed by the exact `mul_div_floor_narrow`.
    /// Run it with `cargo kani -p risk_core --harness twap_bounded_by_samples -Z stubbing`.
    /// Status: written, not run (Kani is not installed). Do not cite it as proven.
    #[kani::proof]
    #[kani::unwind(9)]
    #[kani::stub(auction_core::math::mul_div_floor, mul_div_floor_narrow)]
    fn twap_bounded_by_samples() {
        use crate::twap::{Sample, TwapRing, twap};
        use alloc::vec::Vec;
        let len: usize = kani::any();
        kani::assume(len >= 1 && len <= 3);
        let mut samples: Vec<Sample> = Vec::new();
        let mut ts: u64 = kani::any();
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
            samples.push(Sample {
                ts_ms: ts,
                price_ray,
            });
        }
        let ring = TwapRing {
            samples,
            last_conf_bps: 0,
        };
        let (now, window, min_obs, max_age): (u64, u64, u8, u64) =
            (kani::any(), kani::any(), kani::any(), kani::any());
        let Some((t, _)) = twap(&ring, now, window, min_obs, max_age) else {
            return;
        };
        // Rebuild the used samples' price range: newest first, span rule as documented above.
        let Some(newest) = ring.samples.last() else {
            return;
        };
        let anchor = now.min(newest.ts_ms.saturating_add(max_age));
        let lo = anchor.saturating_sub(window);
        let (mut lo_p, mut hi_p, mut used) = (u128::MAX, 0u128, 0usize);
        for (i, s) in ring.samples.iter().enumerate().rev() {
            let next = ring.samples.get(i.saturating_add(1));
            let end = next.map_or(anchor, |n| n.ts_ms.min(anchor));
            if next.is_some() && end.saturating_sub(s.ts_ms) > max_age {
                break;
            }
            if end.saturating_sub(s.ts_ms.max(lo)) > 0 {
                lo_p = lo_p.min(s.price_ray);
                hi_p = hi_p.max(s.price_ray);
                used = used.saturating_add(1);
            }
        }
        assert!(used >= 1 && used >= usize::from(min_obs));
        assert!(lo_p.saturating_sub(7) <= t && t <= hi_p);
    }

    /// D4 (spec §3.2.4): for any fallback price, stale cap and discount up to the schedule
    /// maximum, the floored price pays out at most the cap per coin:
    /// `floor(floored_price(p_fb, cap, d) · RAY / (RAY - d)) <= cap`.
    /// Status: written, not run (Kani is not installed). Do not cite it as proven.
    #[kani::proof]
    fn fallback_price_never_breaches_the_floor() {
        use crate::stale::floored_price;
        use auction_core::{
            math::mul_div_floor,
            schedule::MAX_DISCOUNT_BPS,
            units::{RAY, bps_to_ray},
        };
        let (p_fb, cap, d): (u128, u128, u128) = (kani::any(), kani::any(), kani::any());
        kani::assume(p_fb < 1 << 100 && cap < 1 << 100 && d <= bps_to_ray(MAX_DISCOUNT_BPS));
        let price = floored_price(p_fb, cap, d);
        let Some(keep) = RAY.checked_sub(d) else {
            return;
        };
        let back = mul_div_floor(price, RAY, keep);
        assert!(matches!(back, Ok(b) if b <= cap));
    }
}
