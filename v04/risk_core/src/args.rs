//! The per-type Tier 2 risk parameters (spec §3.1.6, §3.2.4, ADR-082). Every value arrives in
//! `SetCollateral` and is checked here; the engine has no defaults. The named presets live in
//! [`crate::presets`], behind the `presets` feature, so no production value is on chain.

use auction_core::price::MAX_ORACLE_AGE_MS;

use crate::engine_ext::{SANITY_HI_MAX_BPS, SANITY_LO_MIN_BPS};

/// Shortest nonzero TWAP window, ms (1 min).
pub const TWAP_WINDOW_MIN_MS: u64 = 60_000;
/// Longest TWAP window, ms (24 h).
pub const TWAP_WINDOW_MAX_MS: u64 = 86_400_000;
/// Shortest sample spacing while the TWAP is on, ms.
pub const TWAP_SPACING_MIN_MS: u64 = 1_000;
/// Most samples a TWAP may require: the ring's capacity.
pub const TWAP_MIN_OBS_MAX: u8 = 8;
/// Narrowest nonzero spot-vs-TWAP band, bps.
pub const DEV_MIN_BPS: u16 = 100;
/// Widest spot-vs-TWAP band, bps.
pub const DEV_MAX_BPS: u16 = 5_000;
/// Longest grace period and longest minimum outage gap, ms (24 h).
pub const GRACE_MAX_MS: u64 = 86_400_000;

/// Per-type TWAP, deviation-breaker, D4, feed-window and grace parameters. `twap_window_ms = 0`
/// turns the TWAP off, `max_spot_twap_dev_bps = 0` the breaker, `stale_bid_max_age_ms = 0` D4,
/// `grace_ms = 0` the grace period. `sanity_*_bps` is the feed's sanity window around the
/// redemption price and `feed_min_raw..feed_max_raw` the feed's own answer bounds (Plan 10, R10-7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "borsh",
    derive(borsh::BorshSerialize, borsh::BorshDeserialize)
)]
pub struct RiskArgs {
    pub twap_window_ms: u64,
    pub twap_min_spacing_ms: u64,
    pub twap_min_obs: u8,
    pub max_spot_twap_dev_bps: u16,
    pub stale_bid_max_age_ms: u64,
    pub sanity_lo_bps: u32,
    pub sanity_hi_bps: u32,
    pub feed_min_raw: u128,
    pub feed_max_raw: u128,
    pub grace_ms: u64,
    pub grace_min_gap_ms: u64,
}

/// The first field [`RiskArgs::validate`] refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RiskArgsError {
    pub field: &'static str,
}

pub(crate) const fn refuse(field: &'static str) -> Result<(), RiskArgsError> {
    Err(RiskArgsError { field })
}

impl RiskArgs {
    /// Whether D4 (bids on a stale feed, spec §3.2.3) is on: a stale window and a TWAP window
    /// both set. The one predicate both the stale cap at `Seized` and the fallback bid use.
    #[must_use]
    pub const fn d4_on(&self) -> bool {
        self.stale_bid_max_age_ms > 0 && self.twap_window_ms > 0
    }

    /// Checks the §3.1.6 and §3.2.4 ranges, then the Plan 9 ruling R9-3 bounds, then the Plan 10
    /// feed-window and grace bounds (R10-7), in that fixed order; the first failure is returned.
    /// `max_oracle_age_ms` and the two band edges are the type's own.
    pub fn validate(
        &self,
        max_oracle_age_ms: u64,
        band_lo_bps: u32,
        band_hi_bps: u32,
    ) -> Result<(), RiskArgsError> {
        self.check_ranges()?;
        self.check_reachable(max_oracle_age_ms)?;
        self.check_off()?;
        self.check_feed_and_grace(band_lo_bps, band_hi_bps)
    }

    /// R10-7: the sanity window must contain the type's band, the pin bounds must be ordered,
    /// and a grace period needs a nonzero outage gap (else it would start on every reading).
    fn check_feed_and_grace(
        &self,
        band_lo_bps: u32,
        band_hi_bps: u32,
    ) -> Result<(), RiskArgsError> {
        if !(SANITY_LO_MIN_BPS..=band_lo_bps).contains(&self.sanity_lo_bps) {
            return refuse("sanity_lo_bps");
        }
        if !(band_hi_bps..=SANITY_HI_MAX_BPS).contains(&self.sanity_hi_bps) {
            return refuse("sanity_hi_bps");
        }
        if self.feed_min_raw >= self.feed_max_raw {
            return refuse("feed_min_raw");
        }
        if self.grace_ms > GRACE_MAX_MS {
            return refuse("grace_ms");
        }
        if self.grace_min_gap_ms > GRACE_MAX_MS || (self.grace_ms > 0 && self.grace_min_gap_ms == 0)
        {
            return refuse("grace_min_gap_ms");
        }
        Ok(())
    }

    /// The §3.1.6 and §3.2.4 ranges, field by field.
    fn check_ranges(&self) -> Result<(), RiskArgsError> {
        let window = self.twap_window_ms;
        let on = window > 0;
        if on && !(TWAP_WINDOW_MIN_MS..=TWAP_WINDOW_MAX_MS).contains(&window) {
            return refuse("twap_window_ms");
        }
        if on && !(TWAP_SPACING_MIN_MS..=window).contains(&self.twap_min_spacing_ms) {
            return refuse("twap_min_spacing_ms");
        }
        if on && !(1..=TWAP_MIN_OBS_MAX).contains(&self.twap_min_obs) {
            return refuse("twap_min_obs");
        }
        let dev = self.max_spot_twap_dev_bps;
        if dev > 0 && (!on || !(DEV_MIN_BPS..=DEV_MAX_BPS).contains(&dev)) {
            return refuse("max_spot_twap_dev_bps");
        }
        let stale = self.stale_bid_max_age_ms;
        if stale > MAX_ORACLE_AGE_MS || (stale > 0 && !on) {
            return refuse("stale_bid_max_age_ms");
        }
        Ok(())
    }

    /// R9-3: a TWAP that can fill and that the ring can span, and a D4 that can fire.
    fn check_reachable(&self, max_oracle_age_ms: u64) -> Result<(), RiskArgsError> {
        let window = self.twap_window_ms;
        let spacing = self.twap_min_spacing_ms;
        if spacing > max_oracle_age_ms {
            return refuse("twap_min_spacing_ms");
        }
        if window > 0 {
            let fits = window.checked_div(spacing).unwrap_or(0);
            if u64::from(self.twap_min_obs) > fits {
                return refuse("twap_min_obs");
            }
            // The ring keeps `TWAP_MIN_OBS_MAX` samples at least `spacing` apart, and a new one
            // evicts the oldest. A longer window could be cut to the ring's span by observing
            // once per spacing; at this bound the ring always spans at least 7/8 of the window.
            if window > u64::from(TWAP_MIN_OBS_MAX).saturating_mul(spacing) {
                return refuse("twap_window_ms");
            }
        }
        let stale = self.stale_bid_max_age_ms;
        if stale > 0 && stale <= max_oracle_age_ms {
            return refuse("stale_bid_max_age_ms");
        }
        // On a stale feed the anchor is `newest.ts + max_age` and the k-th newest sample is at
        // most `newest.ts - (k - 1) * spacing`, so a window at or below this never holds min_obs.
        let reach = max_oracle_age_ms
            .saturating_add(u64::from(self.twap_min_obs.saturating_sub(2)).saturating_mul(spacing));
        if stale > 0 && window <= reach {
            return refuse("twap_window_ms");
        }
        Ok(())
    }

    /// R9-3: with the TWAP off, every other field is 0; the first nonzero one is refused.
    fn check_off(&self) -> Result<(), RiskArgsError> {
        if self.twap_window_ms > 0 {
            return Ok(());
        }
        if self.twap_min_spacing_ms > 0 {
            return refuse("twap_min_spacing_ms");
        }
        if self.twap_min_obs > 0 {
            return refuse("twap_min_obs");
        }
        if self.max_spot_twap_dev_bps > 0 {
            return refuse("max_spot_twap_dev_bps");
        }
        if self.stale_bid_max_age_ms > 0 {
            return refuse("stale_bid_max_age_ms");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX_AGE: u64 = 86_400_000;
    const BAND_LO: u32 = 9_000;
    const BAND_HI: u32 = 11_000;

    /// The six Plan 10 fields at their legacy values (the presets are behind a feature).
    const OFF_FEED: RiskArgs = RiskArgs {
        twap_window_ms: 0,
        twap_min_spacing_ms: 0,
        twap_min_obs: 0,
        max_spot_twap_dev_bps: 0,
        stale_bid_max_age_ms: 0,
        sanity_lo_bps: 5_000,
        sanity_hi_bps: 20_000,
        feed_min_raw: 0,
        feed_max_raw: u128::MAX,
        grace_ms: 0,
        grace_min_gap_ms: 0,
    };

    /// TWAP on at the minimum window with 6 spacings in it, the other fields at their lowest
    /// nonzero bound, D4 off.
    fn base() -> RiskArgs {
        RiskArgs {
            twap_window_ms: 60_000,
            twap_min_spacing_ms: 10_000,
            twap_min_obs: 1,
            max_spot_twap_dev_bps: 100,
            stale_bid_max_age_ms: 0,
            ..OFF_FEED
        }
    }

    fn field(r: RiskArgs) -> Result<(), &'static str> {
        r.validate(MAX_AGE, BAND_LO, BAND_HI).map_err(|e| e.field)
    }

    #[test]
    fn d4_on_needs_both_windows() {
        assert!(!base().d4_on(), "no stale window");
        let on = RiskArgs {
            stale_bid_max_age_ms: 3_600_000,
            ..base()
        };
        assert!(on.d4_on());
        let no_twap = RiskArgs {
            twap_window_ms: 0,
            ..on
        };
        assert!(!no_twap.d4_on());
    }

    #[cfg(feature = "presets")]
    #[test]
    fn legacy_and_recommended_presets_validate() {
        assert_eq!(
            RiskArgs::legacy().validate(3_600_000, BAND_LO, BAND_HI),
            Ok(())
        );
        assert_eq!(
            RiskArgs::recommended().validate(600_000, BAND_LO, BAND_HI),
            Ok(())
        );
    }

    #[test]
    fn risk_args_bounds_are_inclusive() {
        let b = base();
        assert_eq!(field(b), Ok(()));
        // The spacing is an eighth of the longest window, so the ring bound passes there.
        for (w, spacing, ok) in [
            (59_999, 10_000, false),
            (60_000, 10_000, true),
            (86_400_000, 10_800_000, true),
            (86_400_001, 10_800_000, false),
        ] {
            let r = field(RiskArgs {
                twap_window_ms: w,
                twap_min_spacing_ms: spacing,
                ..b
            });
            assert_eq!(r, if ok { Ok(()) } else { Err("twap_window_ms") }, "{w}");
        }
        assert_eq!(
            field(RiskArgs {
                twap_min_spacing_ms: 999,
                ..b
            }),
            Err("twap_min_spacing_ms")
        );
        for obs in [0, 9] {
            assert_eq!(
                field(RiskArgs {
                    twap_min_obs: obs,
                    ..b
                }),
                Err("twap_min_obs"),
                "{obs}"
            );
        }
        for (dev, ok) in [(99, false), (100, true), (5_000, true), (5_001, false)] {
            let r = field(RiskArgs {
                max_spot_twap_dev_bps: dev,
                ..b
            });
            assert_eq!(
                r,
                if ok {
                    Ok(())
                } else {
                    Err("max_spot_twap_dev_bps")
                },
                "{dev}"
            );
        }
        assert_eq!(
            field(RiskArgs {
                stale_bid_max_age_ms: 86_400_001,
                ..b
            }),
            Err("stale_bid_max_age_ms")
        );
    }

    /// The true edges of the inclusive bounds the table above does not reach: a spacing equal to
    /// the window, `TWAP_MIN_OBS_MAX` samples, and a stale window of `MAX_ORACLE_AGE_MS`.
    #[test]
    fn risk_args_inclusive_edges() {
        let b = base();
        let window = b.twap_window_ms;
        assert_eq!(
            field(RiskArgs {
                twap_min_spacing_ms: window,
                ..b
            }),
            Ok(())
        );
        assert_eq!(
            field(RiskArgs {
                twap_min_spacing_ms: window.checked_add(1).expect("fits"),
                ..b
            }),
            Err("twap_min_spacing_ms")
        );
        // 8 samples need 8 spacings in the window: 60_000 = 8 · 7_500.
        assert_eq!(
            field(RiskArgs {
                twap_min_spacing_ms: 7_500,
                twap_min_obs: TWAP_MIN_OBS_MAX,
                ..b
            }),
            Ok(())
        );
        assert_eq!(TWAP_MIN_OBS_MAX, 8);
        // D4 at the longest stale window: R9-3 needs the feed max age below it and the window
        // above `max_age + (min_obs - 2) · spacing`. The longest window needs a spacing of an
        // eighth of it, and the spacing may not exceed the feed max age, so a 10_800_000 ms feed.
        let d4 = RiskArgs {
            twap_window_ms: TWAP_WINDOW_MAX_MS,
            twap_min_spacing_ms: 10_800_000,
            stale_bid_max_age_ms: MAX_ORACLE_AGE_MS,
            ..b
        };
        assert_eq!(MAX_ORACLE_AGE_MS, 86_400_000);
        assert_eq!(d4.validate(10_800_000, BAND_LO, BAND_HI), Ok(()));
        assert_eq!(
            RiskArgs {
                stale_bid_max_age_ms: MAX_ORACLE_AGE_MS.checked_add(1).expect("fits"),
                ..d4
            }
            .validate(10_800_000, BAND_LO, BAND_HI)
            .map_err(|e| e.field),
            Err("stale_bid_max_age_ms")
        );
    }

    /// The window is at most 8 spacings, the ring's capacity: a longer one could be cut short by
    /// observing once per spacing, since each new sample past 8 evicts the oldest.
    #[test]
    fn risk_args_refuse_a_window_longer_than_the_ring() {
        let b = base();
        let at_ring = RiskArgs {
            twap_window_ms: 80_000,
            ..b
        };
        assert_eq!(field(at_ring), Ok(()));
        let past_ring = RiskArgs {
            twap_window_ms: 80_001,
            ..b
        };
        assert_eq!(field(past_ring), Err("twap_window_ms"));
        // The settings the bound exists for: a long window of 1 s samples.
        assert_eq!(
            field(RiskArgs {
                twap_window_ms: TWAP_WINDOW_MAX_MS,
                twap_min_spacing_ms: TWAP_SPACING_MIN_MS,
                ..b
            }),
            Err("twap_window_ms")
        );
    }

    #[test]
    fn twap_off_requires_every_field_zero() {
        let off = OFF_FEED;
        assert_eq!(field(off), Ok(()));
        assert_eq!(
            field(RiskArgs {
                twap_min_spacing_ms: 1_000,
                ..off
            }),
            Err("twap_min_spacing_ms")
        );
        assert_eq!(
            field(RiskArgs {
                twap_min_obs: 1,
                ..off
            }),
            Err("twap_min_obs")
        );
        assert_eq!(
            field(RiskArgs {
                stale_bid_max_age_ms: 1,
                ..off
            }),
            Err("stale_bid_max_age_ms")
        );
    }

    #[cfg(feature = "presets")]
    #[test]
    fn risk_args_reject_unreachable_twap_and_dead_d4() {
        let r = RiskArgs::recommended();
        assert_eq!(
            r.validate(299_999, BAND_LO, BAND_HI).map_err(|e| e.field),
            Err("twap_min_spacing_ms")
        );
        assert_eq!(
            RiskArgs {
                twap_min_obs: 7,
                ..r
            }
            .validate(600_000, BAND_LO, BAND_HI)
            .map_err(|e| e.field),
            Err("twap_min_obs")
        );
        assert_eq!(
            RiskArgs {
                stale_bid_max_age_ms: 600_000,
                ..r
            }
            .validate(600_000, BAND_LO, BAND_HI)
            .map_err(|e| e.field),
            Err("stale_bid_max_age_ms")
        );
        // R9-3 window bound: 900_000 is not > 600_000 + (3 - 2) * 300_000; 900_001 is; D4 off lifts it.
        assert_eq!(
            RiskArgs {
                twap_window_ms: 900_000,
                ..r
            }
            .validate(600_000, BAND_LO, BAND_HI)
            .map_err(|e| e.field),
            Err("twap_window_ms")
        );
        assert_eq!(
            RiskArgs {
                twap_window_ms: 900_001,
                ..r
            }
            .validate(600_000, BAND_LO, BAND_HI),
            Ok(())
        );
        assert_eq!(
            RiskArgs {
                twap_window_ms: 900_000,
                stale_bid_max_age_ms: 0,
                ..r
            }
            .validate(600_000, BAND_LO, BAND_HI),
            Ok(())
        );
        assert_eq!(
            RiskArgs {
                max_spot_twap_dev_bps: 2_000,
                ..RiskArgs::legacy()
            }
            .validate(600_000, BAND_LO, BAND_HI)
            .map_err(|e| e.field),
            Err("max_spot_twap_dev_bps")
        );
    }

    #[test]
    fn sanity_window_bounds_are_inclusive() {
        let ok = |r: RiskArgs| field(r);
        for (lo, good) in [(999, false), (1_000, true), (9_000, true), (9_001, false)] {
            let r = ok(RiskArgs {
                sanity_lo_bps: lo,
                ..base()
            });
            assert_eq!(r, if good { Ok(()) } else { Err("sanity_lo_bps") }, "{lo}");
        }
        for (hi, good) in [
            (10_999, false),
            (11_000, true),
            (100_000, true),
            (100_001, false),
        ] {
            let r = ok(RiskArgs {
                sanity_hi_bps: hi,
                ..base()
            });
            assert_eq!(r, if good { Ok(()) } else { Err("sanity_hi_bps") }, "{hi}");
        }
    }

    #[test]
    fn pin_bounds_must_be_ordered() {
        for (min, max, good) in [
            (0, u128::MAX, true),
            (5, 6, true),
            (6, 6, false),
            (7, 6, false),
        ] {
            let r = field(RiskArgs {
                feed_min_raw: min,
                feed_max_raw: max,
                ..base()
            });
            assert_eq!(
                r,
                if good { Ok(()) } else { Err("feed_min_raw") },
                "{min} {max}"
            );
        }
    }

    #[test]
    fn grace_bounds_are_inclusive() {
        let with = |grace_ms, grace_min_gap_ms| {
            field(RiskArgs {
                grace_ms,
                grace_min_gap_ms,
                ..base()
            })
        };
        assert_eq!(with(86_400_000, 1), Ok(()));
        assert_eq!(with(86_400_001, 1), Err("grace_ms"));
        assert_eq!(with(1, 86_400_000), Ok(()));
        assert_eq!(with(1, 86_400_001), Err("grace_min_gap_ms"));
    }

    #[test]
    fn grace_needs_a_nonzero_min_gap() {
        let with = |grace_ms, grace_min_gap_ms| {
            field(RiskArgs {
                grace_ms,
                grace_min_gap_ms,
                ..base()
            })
        };
        assert_eq!(with(1, 0), Err("grace_min_gap_ms"));
        assert_eq!(with(0, 5), Ok(()));
    }

    #[test]
    fn grace_and_pins_work_with_the_twap_off() {
        let r = RiskArgs {
            grace_ms: 1_800_000,
            grace_min_gap_ms: 600_000,
            feed_min_raw: 1,
            ..OFF_FEED
        };
        assert_eq!(field(r), Ok(()));
    }
}
