//! Piecewise-linear discount schedule (spec §5.3, ADR-006). Snapshotted per auction.
#![allow(clippy::arithmetic_side_effects)] // subtractions are ordered by validation

use crate::error::ConfigError;
use crate::math::mul_div_floor;
use crate::units::bps_to_ray;

pub const MAX_POINTS: usize = 8;
pub const MAX_DISCOUNT_BPS: u16 = 3000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SchedulePoint {
    pub elapsed_ms: u64,
    pub discount_bps: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Schedule {
    points: [SchedulePoint; MAX_POINTS],
    len: u8,
}

impl Schedule {
    pub fn new(points: &[SchedulePoint]) -> Result<Self, ConfigError> {
        let first = points.first().ok_or(ConfigError::EmptySchedule)?;
        if points.len() > MAX_POINTS {
            return Err(ConfigError::TooManyPoints);
        }
        if first.elapsed_ms != 0 {
            return Err(ConfigError::FirstPointNotZero);
        }
        for w in points.windows(2) {
            if w[1].elapsed_ms <= w[0].elapsed_ms {
                return Err(ConfigError::TimeNotIncreasing);
            }
            if w[1].discount_bps < w[0].discount_bps {
                return Err(ConfigError::DiscountDecreasing);
            }
        }
        if points.iter().any(|p| p.discount_bps > MAX_DISCOUNT_BPS) {
            return Err(ConfigError::DiscountAboveMax);
        }
        let mut buf = [SchedulePoint::default(); MAX_POINTS];
        buf[..points.len()].copy_from_slice(points);
        Ok(Self {
            points: buf,
            len: points.len() as u8,
        })
    }

    pub fn points(&self) -> &[SchedulePoint] {
        &self.points[..self.len as usize]
    }

    /// Discount in RAY at effective elapsed time `t`, rounded down (favours the protocol).
    pub fn discount_ray(&self, t: u64) -> u128 {
        let pts = self.points();
        let last = pts[pts.len() - 1];
        if t >= last.elapsed_ms {
            return bps_to_ray(last.discount_bps);
        }
        // pts[0].elapsed_ms == 0 ≤ t < last.elapsed_ms, so a segment exists.
        let i = pts
            .windows(2)
            .position(|w| t < w[1].elapsed_ms)
            .unwrap_or(0);
        let (a, b) = (pts[i], pts[i + 1]);
        let base = bps_to_ray(a.discount_bps);
        let rise = bps_to_ray(b.discount_bps) - base; // non-negative by validation
        let span = (b.elapsed_ms - a.elapsed_ms) as u128; // > 0 by validation
        let into = (t - a.elapsed_ms) as u128; // < span
                                               // rise ≤ 3000 bps in RAY and into < span, so the quotient is < rise; it cannot fail.
        base + mul_div_floor(rise, into, span).unwrap_or(0)
    }

    pub fn max_discount_ray(&self) -> u128 {
        bps_to_ray(self.points()[self.len as usize - 1].discount_bps)
    }
}

pub fn rai_default() -> Schedule {
    Schedule::new(&[
        SchedulePoint {
            elapsed_ms: 0,
            discount_bps: 200,
        },
        SchedulePoint {
            elapsed_ms: 2_700_000,
            discount_bps: 1000,
        },
    ])
    .expect("constant schedule is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{bps_to_ray, BPS_TO_RAY};
    use proptest::prelude::*;

    fn p(e: u64, d: u16) -> SchedulePoint {
        SchedulePoint {
            elapsed_ms: e,
            discount_bps: d,
        }
    }

    #[test]
    fn rai_default_interpolates_and_holds() {
        let s = rai_default();
        assert_eq!(s.discount_ray(0), bps_to_ray(200));
        assert_eq!(s.discount_ray(1_350_000), bps_to_ray(600)); // halfway: 6%
        assert_eq!(s.discount_ray(2_700_000), bps_to_ray(1000));
        assert_eq!(s.discount_ray(u64::MAX), bps_to_ray(1000)); // held forever (no expiry)
    }

    #[test]
    fn interpolation_rounds_down() {
        let s = Schedule::new(&[p(0, 0), p(3, 1)]).unwrap();
        // 1 bps over 3 ms → after 1 ms the exact value is BPS_TO_RAY/3, rounded down
        assert_eq!(s.discount_ray(1), BPS_TO_RAY / 3);
    }

    #[test]
    fn single_point_schedule_is_constant() {
        // Review Focus #5
        let s = Schedule::new(&[p(0, 500)]).unwrap();
        for t in [0u64, 1, 2_700_000, u64::MAX] {
            assert_eq!(s.discount_ray(t), bps_to_ray(500));
        }
    }

    #[test]
    fn validation() {
        assert_eq!(Schedule::new(&[]), Err(ConfigError::EmptySchedule));
        assert_eq!(
            Schedule::new(&[p(5, 0)]),
            Err(ConfigError::FirstPointNotZero)
        );
        assert_eq!(
            Schedule::new(&[p(0, 0), p(0, 1)]),
            Err(ConfigError::TimeNotIncreasing)
        );
        assert_eq!(
            Schedule::new(&[p(0, 5), p(1, 4)]),
            Err(ConfigError::DiscountDecreasing)
        );
        assert_eq!(
            Schedule::new(&[p(0, 3001)]),
            Err(ConfigError::DiscountAboveMax)
        );
        let nine: [SchedulePoint; 9] = core::array::from_fn(|i| p(i as u64, 0));
        assert_eq!(Schedule::new(&nine), Err(ConfigError::TooManyPoints));
    }

    proptest! {
        #[test]
        fn monotone_and_bounded(t1: u64, t2: u64) {
            let s = rai_default();
            let (a, b) = if t1 <= t2 { (t1, t2) } else { (t2, t1) };
            prop_assert!(s.discount_ray(a) <= s.discount_ray(b));
            prop_assert!(s.discount_ray(b) <= s.max_discount_ray());
        }
    }
}
