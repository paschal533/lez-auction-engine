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
}
