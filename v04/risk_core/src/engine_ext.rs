//! The engine-wide parameters the admin sets at `Init` and `SetEngineParams` (Plan 10 rulings
//! R10-1, R10-2; ADR-082). They were constants before; the engine has no defaults for them. The
//! named presets live in [`crate::presets`], behind the `presets` feature.

use crate::args::{RiskArgsError, refuse};

/// Widest reference-feed confidence, bps (the engine's `MAX_CONF_BPS`).
pub const REF_CONF_MAX_BPS: u16 = 1_000;
/// Widest clamp of the reference price against the live feed, bps.
pub const REF_CLAMP_MAX_BPS: u16 = 5_000;
/// Shortest debt relist delay, ms (1 min).
pub const RELIST_DELAY_MIN_MS: u64 = 60_000;
/// Longest debt relist delay, ms (7 days; the engine's `MAX_SYSTEM_TTL_MS`).
pub const RELIST_DELAY_MAX_MS: u64 = 604_800_000;
/// Lowest value of the sanity window's lower edge, bps.
pub const SANITY_LO_MIN_BPS: u32 = 1_000;
/// Highest value of the sanity window's upper edge, bps.
pub const SANITY_HI_MAX_BPS: u32 = 100_000;

/// The sanity window's lower edge may not pass 100 %, its upper edge may not fall below it.
const SANITY_PAR_BPS: u32 = 10_000;
/// Highest `ref_scale`: the reference oracle is read at scale 0 or 1.
const REF_SCALE_MAX: u8 = 1;

/// Engine-wide reference-oracle and relist parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "borsh",
    derive(borsh::BorshSerialize, borsh::BorshDeserialize)
)]
pub struct EngineExtArgs {
    pub ref_conf_bps: u16,
    pub ref_clamp_bps: u16,
    pub debt_relist_delay_ms: u64,
    pub ref_sanity_lo_bps: u32,
    pub ref_sanity_hi_bps: u32,
    pub ref_scale: u8,
}

impl EngineExtArgs {
    /// Checks the R10-2 bounds, fields in declaration order; the first failure is returned.
    pub fn validate(&self) -> Result<(), RiskArgsError> {
        if self.ref_conf_bps > REF_CONF_MAX_BPS {
            return refuse("ref_conf_bps");
        }
        if self.ref_clamp_bps > REF_CLAMP_MAX_BPS {
            return refuse("ref_clamp_bps");
        }
        if !(RELIST_DELAY_MIN_MS..=RELIST_DELAY_MAX_MS).contains(&self.debt_relist_delay_ms) {
            return refuse("debt_relist_delay_ms");
        }
        if !(SANITY_LO_MIN_BPS..=SANITY_PAR_BPS).contains(&self.ref_sanity_lo_bps) {
            return refuse("ref_sanity_lo_bps");
        }
        if !(SANITY_PAR_BPS..=SANITY_HI_MAX_BPS).contains(&self.ref_sanity_hi_bps) {
            return refuse("ref_sanity_hi_bps");
        }
        if self.ref_scale > REF_SCALE_MAX {
            return refuse("ref_scale");
        }
        Ok(())
    }
}

#[cfg(all(test, feature = "presets"))]
mod tests {
    use super::*;

    fn field(e: EngineExtArgs) -> Result<(), &'static str> {
        e.validate().map_err(|err| err.field)
    }

    #[test]
    fn engine_ext_presets_validate() {
        assert_eq!(EngineExtArgs::legacy().validate(), Ok(()));
        assert_eq!(EngineExtArgs::recommended().validate(), Ok(()));
        assert_eq!(EngineExtArgs::legacy(), EngineExtArgs::recommended());
    }

    #[test]
    fn engine_ext_bounds_are_inclusive() {
        let b = EngineExtArgs::legacy();
        for v in [0, 1_000] {
            assert_eq!(
                field(EngineExtArgs {
                    ref_conf_bps: v,
                    ..b
                }),
                Ok(()),
                "ref_conf_bps {v}"
            );
        }
        for v in [0, 5_000] {
            assert_eq!(
                field(EngineExtArgs {
                    ref_clamp_bps: v,
                    ..b
                }),
                Ok(()),
                "ref_clamp_bps {v}"
            );
        }
        for v in [60_000, 604_800_000] {
            assert_eq!(
                field(EngineExtArgs {
                    debt_relist_delay_ms: v,
                    ..b
                }),
                Ok(()),
                "debt_relist_delay_ms {v}"
            );
        }
        for v in [1_000, 10_000] {
            assert_eq!(
                field(EngineExtArgs {
                    ref_sanity_lo_bps: v,
                    ..b
                }),
                Ok(()),
                "ref_sanity_lo_bps {v}"
            );
        }
        for v in [10_000, 100_000] {
            assert_eq!(
                field(EngineExtArgs {
                    ref_sanity_hi_bps: v,
                    ..b
                }),
                Ok(()),
                "ref_sanity_hi_bps {v}"
            );
        }
        for v in [0, 1] {
            assert_eq!(
                field(EngineExtArgs { ref_scale: v, ..b }),
                Ok(()),
                "ref_scale {v}"
            );
        }
    }

    #[test]
    fn engine_ext_refuses_the_first_bad_field() {
        let b = EngineExtArgs::legacy();
        assert_eq!(
            field(EngineExtArgs {
                ref_conf_bps: 1_001,
                ..b
            }),
            Err("ref_conf_bps")
        );
        assert_eq!(
            field(EngineExtArgs {
                ref_clamp_bps: 5_001,
                ..b
            }),
            Err("ref_clamp_bps")
        );
        for v in [59_999, 604_800_001] {
            assert_eq!(
                field(EngineExtArgs {
                    debt_relist_delay_ms: v,
                    ..b
                }),
                Err("debt_relist_delay_ms"),
                "{v}"
            );
        }
        for v in [999, 10_001] {
            assert_eq!(
                field(EngineExtArgs {
                    ref_sanity_lo_bps: v,
                    ..b
                }),
                Err("ref_sanity_lo_bps"),
                "{v}"
            );
        }
        for v in [9_999, 100_001] {
            assert_eq!(
                field(EngineExtArgs {
                    ref_sanity_hi_bps: v,
                    ..b
                }),
                Err("ref_sanity_hi_bps"),
                "{v}"
            );
        }
        assert_eq!(field(EngineExtArgs { ref_scale: 2, ..b }), Err("ref_scale"));
        // Declaration order: with several bad fields, the first one is named.
        assert_eq!(
            field(EngineExtArgs {
                ref_conf_bps: 1_001,
                ref_scale: 2,
                ..b
            }),
            Err("ref_conf_bps")
        );
    }
}
