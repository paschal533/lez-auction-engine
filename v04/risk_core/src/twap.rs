//! The TWAP observation ring (spec §3.1.1-§3.1.3, ADR-070): at most [`TWAP_CAPACITY`] validated
//! mids, oldest first, each weighted by how long it was the current publication. A gap longer
//! than the feed's maximum age breaks the TWAP, and the anchor freezes when the newest sample goes
//! stale, so the last-good TWAP is constant for the whole of an outage.

use alloc::vec::Vec;
use auction_core::math::mul_div_floor;

/// The ring's protocol capacity, like `MAX_POINTS`.
pub const TWAP_CAPACITY: usize = 8;

/// One validated mid `M` (RAY) at its oracle publish time (clamped to the observation time).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    feature = "borsh",
    derive(borsh::BorshSerialize, borsh::BorshDeserialize)
)]
pub struct Sample {
    pub ts_ms: u64,
    pub price_ray: u128,
}

/// The per-collateral observation ring: oldest first, `samples.len() <= TWAP_CAPACITY`, strictly
/// increasing `ts_ms`. `last_conf_bps` is the confidence of the newest recorded sample.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(
    feature = "borsh",
    derive(borsh::BorshSerialize, borsh::BorshDeserialize)
)]
pub struct TwapRing {
    pub samples: Vec<Sample>,
    pub last_conf_bps: u16,
}

impl TwapRing {
    /// The newest sample, if any.
    #[must_use]
    pub fn newest(&self) -> Option<Sample> {
        self.samples.last().cloned()
    }
}

/// Records one validated reading (spec §3.1.2) and returns the new ring; `ring` is unchanged.
///
/// The sample time is `min(reading_ts_ms, now_ms)`. The reading is skipped (the ring is returned
/// as is) when it is not newer than the newest sample (the same publish seen again) or is less
/// than `spacing_ms` after it. Otherwise it is appended, the oldest is dropped past
/// [`TWAP_CAPACITY`], and `last_conf_bps` is set; it is set only when a sample is pushed.
#[must_use]
pub fn record(
    ring: &TwapRing,
    reading_ts_ms: u64,
    price_ray: u128,
    conf_bps: u16,
    now_ms: u64,
    spacing_ms: u64,
) -> TwapRing {
    let ts_ms = reading_ts_ms.min(now_ms);
    if let Some(n) = ring.samples.last()
        && (ts_ms <= n.ts_ms || ts_ms.saturating_sub(n.ts_ms) < spacing_ms)
    {
        return ring.clone();
    }
    let excess = ring
        .samples
        .len()
        .saturating_add(1)
        .saturating_sub(TWAP_CAPACITY);
    let samples = ring
        .samples
        .iter()
        .skip(excess)
        .cloned()
        .chain(core::iter::once(Sample { ts_ms, price_ray }))
        .collect();
    TwapRing {
        samples,
        last_conf_bps: conf_bps,
    }
}

/// The time-weighted average of the ring (spec §3.1.3) as `(T, newest.ts_ms)`, or `None` when
/// fewer than `min_obs` samples carry weight or the total weight is zero.
///
/// The anchor is `min(now, newest + max_age)`: live it is `now`, stale it freezes at the moment
/// the newest sample went stale. Walking newest first, each sample weighs the time it was current,
/// clipped to `[anchor - window, anchor]`; a non-newest sample current for longer than `max_age`
/// (a stale period) stops the walk, so older samples are excluded. `T = Σ floor(p_i · w_i / Σw)`,
/// so `min(p_i) - 7 <= T <= max(p_i)` over the used samples (saturating at 0): each of the at most
/// 8 terms floors away less than one unit.
#[must_use]
pub fn twap(
    ring: &TwapRing,
    now_ms: u64,
    window_ms: u64,
    min_obs: u8,
    max_age_ms: u64,
) -> Option<(u128, u64)> {
    let newest = ring.samples.last()?;
    let anchor = now_ms.min(newest.ts_ms.saturating_add(max_age_ms));
    let lo = anchor.saturating_sub(window_ms);
    let mut end = anchor;
    let mut total: u64 = 0;
    let mut terms: Vec<(u128, u64)> = Vec::with_capacity(TWAP_CAPACITY);
    for (i, s) in ring.samples.iter().rev().take(TWAP_CAPACITY).enumerate() {
        if i != 0 && end.saturating_sub(s.ts_ms) > max_age_ms {
            break; // a gap longer than the feed's max age breaks the TWAP
        }
        let start = s.ts_ms.max(lo);
        if end > start {
            let w = end.saturating_sub(start);
            terms.push((s.price_ray, w));
            total = total.checked_add(w)?;
        }
        if s.ts_ms <= lo {
            break;
        }
        end = end.min(s.ts_ms); // never raised: `now` may sit before the newest sample
    }
    if terms.len() < usize::from(min_obs) || total == 0 {
        return None;
    }
    let t = terms.iter().try_fold(0u128, |acc, &(p, w)| {
        // `w <= total`, so each term is at most `p` and cannot error.
        acc.checked_add(mul_div_floor(p, u128::from(w), u128::from(total)).ok()?)
    })?;
    Some((t, newest.ts_ms))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn ring(s: &[(u64, u128)]) -> TwapRing {
        TwapRing {
            samples: s
                .iter()
                .map(|&(ts_ms, price_ray)| Sample { ts_ms, price_ray })
                .collect(),
            last_conf_bps: 0,
        }
    }

    #[test]
    fn twap_single_sample_is_its_price() {
        assert_eq!(
            twap(&ring(&[(1_000, 500)]), 2_000, 60_000, 1, 600_000),
            Some((500, 1_000))
        );
    }

    #[test]
    fn twap_weights_by_time_current() {
        assert_eq!(
            twap(
                &ring(&[(0, 100), (10_000, 200)]),
                40_000,
                60_000,
                2,
                600_000
            ),
            Some((175, 10_000))
        );
    }

    #[test]
    fn twap_ignores_samples_before_the_window() {
        assert_eq!(
            twap(
                &ring(&[(0, 100), (10_000, 100), (50_000, 300)]),
                70_000,
                20_000,
                1,
                600_000
            ),
            Some((300, 50_000))
        );
    }

    #[test]
    fn twap_partly_weights_the_sample_crossing_the_window() {
        assert_eq!(
            twap(
                &ring(&[(0, 100), (30_000, 400)]),
                50_000,
                40_000,
                2,
                600_000
            ),
            Some((250, 30_000))
        );
    }

    #[test]
    fn twap_gap_longer_than_max_age_drops_older_samples() {
        let r = ring(&[(0, 100), (30_000, 300), (35_000, 300)]);
        assert_eq!(twap(&r, 40_000, 60_000, 2, 10_000), Some((300, 35_000)));
        assert_eq!(twap(&r, 40_000, 60_000, 3, 10_000), None);
    }

    #[test]
    fn twap_below_min_obs_is_unavailable() {
        assert_eq!(twap(&ring(&[(1_000, 5)]), 2_000, 60_000, 2, 600_000), None);
        assert_eq!(twap(&TwapRing::default(), 2_000, 60_000, 1, 600_000), None);
    }

    #[test]
    fn twap_is_constant_during_an_outage() {
        let r = ring(&[(0, 100), (10_000, 200)]);
        let at = |now| twap(&r, now, 60_000, 2, 20_000);
        assert_eq!(at(30_000), at(100_000));
        assert_eq!(at(30_000), at(1_000_000));
        assert!(at(30_000).is_some());
    }

    #[test]
    fn twap_with_now_before_the_newest_sample_does_not_underflow() {
        assert_eq!(
            twap(&ring(&[(0, 100), (10_000, 200)]), 5_000, 60_000, 1, 600_000),
            Some((100, 10_000))
        );
    }

    #[test]
    fn record_skips_same_publish_and_close_spacing() {
        let a = record(&TwapRing::default(), 1_000, 7, 3, 1_000, 10_000);
        assert_eq!(a.samples.len(), 1);
        assert_eq!(a.last_conf_bps, 3);
        assert_eq!(record(&a, 1_000, 9, 5, 2_000, 10_000), a); // same publish
        assert_eq!(record(&a, 5_000, 9, 5, 6_000, 10_000), a); // closer than spacing
        let b = record(&a, 11_000, 9, 5, 12_000, 10_000);
        assert_eq!((b.samples.len(), b.last_conf_bps), (2, 5));
    }

    #[test]
    fn record_clamps_a_future_timestamp_to_now() {
        assert_eq!(
            record(&TwapRing::default(), 9_000, 7, 0, 5_000, 1_000)
                .newest()
                .map(|s| s.ts_ms),
            Some(5_000)
        );
    }

    #[test]
    fn ring_drops_the_oldest_past_capacity() {
        // 9 records at ts 1_000, 2_000, ..., 9_000 with spacing 1_000 -> len 8, oldest ts 2_000.
        let full = (1..=9u64).fold(TwapRing::default(), |r, k| {
            let ts = k.saturating_mul(1_000);
            record(&r, ts, u128::from(k), 0, ts, 1_000)
        });
        assert_eq!(full.samples.len(), TWAP_CAPACITY);
        assert_eq!(full.samples.first().map(|s| s.ts_ms), Some(2_000));
        assert_eq!(full.newest().map(|s| s.ts_ms), Some(9_000));
    }

    #[test]
    fn twap_floor_rounding_can_fall_below_the_smallest_price() {
        // prices 1 and 1, weights 1 and 2 (ts 0 and 1, now 3, window 3): T = floor(1/3) + floor(2/3) = 0.
        assert_eq!(
            twap(&ring(&[(0, 1), (1, 1)]), 3, 3, 2, 600_000),
            Some((0, 1))
        );
    }

    const DAY_MS: u64 = 86_400_000;
    const MAX_PRICE: u128 = u128::MAX >> 4;

    /// A window or max age: the smallest, the largest, or anything between.
    fn edge_ms() -> impl Strategy<Value = u64> {
        prop_oneof![Just(1u64), Just(DAY_MS), 1u64..=DAY_MS]
    }

    /// The gap to the next sample: 1, exactly `max_age` (kept), `max_age + 1` (breaks), exactly
    /// `window`, `window + 1`, or free.
    fn gap(sel: u8, free: u64, max_age: u64, window: u64) -> u64 {
        match sel {
            0 => 1,
            1 => max_age,
            2 => max_age.saturating_add(1),
            3 => window,
            4 => window.saturating_add(1),
            _ => free,
        }
    }

    /// A price: 1, the largest generated, equal to the previous one, or free.
    fn price(sel: u8, free: u128, prev: u128) -> u128 {
        match sel {
            0 => 1,
            1 => MAX_PRICE,
            2 => prev,
            _ => free,
        }
    }

    /// The query time: before the newest sample, at it, at the stale edge `newest + max_age`, one
    /// past it, exactly one window after sample `k` (so `lo` lands on it while live), one past
    /// that, or free.
    fn query_now(sel: u8, samples: &[Sample], k: usize, edges: (u64, u64), free: u64) -> u64 {
        let (max_age, window) = edges;
        let newest = samples.last().map_or(0, |s| s.ts_ms);
        let at_k = samples
            .get(k)
            .map_or(newest, |s| s.ts_ms)
            .saturating_add(window);
        match sel {
            0 => newest.saturating_sub(1),
            1 => newest,
            2 => newest.saturating_add(max_age),
            3 => newest.saturating_add(max_age).saturating_add(1),
            4 => at_k,
            5 => at_k.saturating_add(1),
            _ => newest.saturating_add(free),
        }
    }

    /// Reference model: the `(price, weight)` of every sample that carries weight. Each sample is
    /// current from its own time to the next one's (the newest's to the anchor), clipped to
    /// `[anchor - window, anchor]`; a non-newest sample whose current span exceeds `max_age` cuts
    /// itself and everything older.
    fn used_terms(s: &[Sample], now: u64, window: u64, max_age: u64) -> Vec<(u128, u64)> {
        let Some(newest) = s.last() else {
            return Vec::new();
        };
        let anchor = now.min(newest.ts_ms.saturating_add(max_age));
        let lo = anchor.saturating_sub(window);
        let ends: Vec<u64> = s
            .iter()
            .skip(1)
            .map(|x| x.ts_ms.min(anchor))
            .chain(core::iter::once(anchor))
            .collect();
        let mut out = Vec::new();
        for (i, (x, &end)) in s.iter().zip(ends.iter()).enumerate().rev() {
            let is_newest = i.saturating_add(1) == s.len();
            if !is_newest && end.saturating_sub(x.ts_ms) > max_age {
                break;
            }
            let w = end.saturating_sub(x.ts_ms.max(lo));
            if w > 0 {
                out.push((x.price_ray, w));
            }
        }
        out
    }

    /// `twap` agrees with the reference on availability and the newest time, and `T` lies in
    /// `[min - (k - 1), max]` of the `k` used prices, hence in `[min - 7, max]`.
    fn check_twap(
        r: &TwapRing,
        now: u64,
        window: u64,
        min_obs: u8,
        max_age: u64,
    ) -> Result<(), TestCaseError> {
        let used = used_terms(&r.samples, now, window, max_age);
        let total = used
            .iter()
            .try_fold(0u64, |acc, &(_, w)| acc.checked_add(w))
            .unwrap_or_default();
        let expect_some = used.len() >= usize::from(min_obs) && total > 0;
        let got = twap(r, now, window, min_obs, max_age);
        prop_assert_eq!(got.is_some(), expect_some, "used {:?}", used);
        if let Some((t, ts)) = got {
            let lo_p = used.iter().map(|&(p, _)| p).min().unwrap_or_default();
            let hi_p = used.iter().map(|&(p, _)| p).max().unwrap_or_default();
            let slack = u128::try_from(used.len().saturating_sub(1)).unwrap_or(u128::MAX);
            prop_assert!(
                lo_p.saturating_sub(7) <= t && t <= hi_p,
                "t {} not in [{} - 7, {}]",
                t,
                lo_p,
                hi_p
            );
            prop_assert!(lo_p.saturating_sub(slack) <= t);
            prop_assert_eq!(Some(ts), r.newest().map(|s| s.ts_ms));
        }
        Ok(())
    }

    proptest! {
        // (floor rounding: up to 8 terms, each loses < 1 unit)
        #[test]
        fn twap_within_sample_range(
            n in 1usize..=8, start in prop_oneof![Just(0u64), 0u64..(1 << 40)],
            gaps in prop::collection::vec((0u8..=5, 1u64..=DAY_MS), 8),
            prices in prop::collection::vec((0u8..=3, 1u128..=MAX_PRICE), 8),
            window in edge_ms(), max_age in edge_ms(), min_obs in 0u8..=9,
            now_sel in 0u8..=6, k in 0usize..8, now_free in 0u64..=(2 * DAY_MS),
        ) {
            let mut samples: Vec<Sample> = Vec::new();
            let mut ts = start;
            let mut prev = prices.first().map_or(1, |&(_, f)| f);
            for (&(gs, gf), &(ps, pf)) in gaps.iter().zip(prices.iter()).take(n) {
                if !samples.is_empty() {
                    ts = ts.saturating_add(gap(gs, gf, max_age, window));
                }
                prev = price(ps, pf, prev);
                samples.push(Sample { ts_ms: ts, price_ray: prev });
            }
            let now = query_now(now_sel, &samples, k, (max_age, window), now_free);
            check_twap(&TwapRing { samples, last_conf_bps: 0 }, now, window, min_obs, max_age)?;
        }

        /// Up to 12 readings through `record`, so the ring wraps: it stays within capacity,
        /// strictly increasing, at least `spacing` apart, never ahead of its observation time,
        /// and `twap` over the wrapped ring (live and at the stale edge) keeps its bounds.
        #[test]
        fn record_keeps_the_ring_spaced_and_bounded(
            readings in prop::collection::vec(
                (0u8..=4, 0u64..=DAY_MS, 0u8..=3, 1u128..=MAX_PRICE, any::<u16>(), 0u8..=2),
                1..=12),
            spacing in prop_oneof![Just(1u64), Just(1_000u64), 1u64..=DAY_MS],
            window in edge_ms(), max_age in edge_ms(), min_obs in 0u8..=9,
        ) {
            let mut r = TwapRing::default();
            let mut now = 0u64;
            let mut pushes = 0usize;
            let mut prev = 1u128;
            for &(ds, df, ps, pf, conf, when) in &readings {
                // Publish time from the newest: the same, one short of the spacing, exactly the
                // spacing, one past it, or free.
                let base = r.newest().map_or(now, |s| s.ts_ms);
                let reading_ts = base.saturating_add(match ds {
                    0 => 0,
                    1 => spacing.saturating_sub(1),
                    2 => spacing,
                    3 => spacing.saturating_add(1),
                    _ => df,
                });
                // Observation time (never backwards): at the reading, one before it (a future
                // reading, clamped), or later.
                now = now.max(match when {
                    0 => reading_ts,
                    1 => reading_ts.saturating_sub(1),
                    _ => reading_ts.saturating_add(df),
                });
                prev = price(ps, pf, prev);
                let next = record(&r, reading_ts, prev, conf, now, spacing);
                if next == r {
                    continue;
                }
                pushes = pushes.saturating_add(1);
                prop_assert_eq!(next.last_conf_bps, conf);
                prop_assert_eq!(next.newest().map(|s| s.ts_ms), Some(reading_ts.min(now)));
                r = next;
            }
            prop_assert_eq!(r.samples.len(), pushes.min(TWAP_CAPACITY));
            for (a, b) in r.samples.iter().zip(r.samples.iter().skip(1)) {
                prop_assert!(b.ts_ms > a.ts_ms && b.ts_ms.saturating_sub(a.ts_ms) >= spacing);
            }
            prop_assert!(r.newest().is_some_and(|s| s.ts_ms <= now));
            check_twap(&r, now, window, min_obs, max_age)?;
            check_twap(&r, now.saturating_add(max_age), window, min_obs, max_age)?;
        }
    }
}
