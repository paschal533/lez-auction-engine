//! The Tier 2 state extensions (spec §3.0): `ConfigState.ext` and `CollateralState.ext`, each
//! appended last so every earlier field keeps its offset.

use auction_engine_params::EngineError;
use borsh::{BorshDeserialize, BorshSerialize};
use risk_core::{RiskArgs, TwapRing};

use crate::state::CollateralState;

/// Engine-level Tier 2 state on the config.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ConfigExt {
    /// The system pause level (spec §3.2.1): 0 = none.
    pub system_level: u8,
}

/// Per-type Tier 2 state on the collateral actor.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CollateralExt {
    /// The validated risk parameters of the last `SetCollateral`.
    pub risk: RiskArgs,
    /// The type's pause level (spec §3.2.1), 0..=3; `admin_paused` counts as 3.
    pub pause_level: u8,
    /// The TWAP observation ring (spec §3.1.1).
    pub ring: TwapRing,
}

impl CollateralExt {
    /// A fresh extension: level 0 and an empty ring.
    #[must_use]
    pub fn new(risk: RiskArgs) -> Self {
        Self {
            risk,
            pause_level: 0,
            ring: TwapRing::default(),
        }
    }
}

impl CollateralState {
    /// The level the gates use: the pause level, or 3 while the admin flag is set.
    #[must_use]
    pub fn effective_level(&self) -> u8 {
        let admin = if self.admin_paused { 3 } else { 0 };
        self.ext.pause_level.max(admin)
    }

    /// The state as a quote carries it: the ring is dropped, everything else is kept.
    #[must_use]
    pub fn for_quote(&self) -> Self {
        // One deep clone, then the ring reset in place on the copy (the original is untouched).
        let mut quoted = self.clone();
        quoted.ext.ring = TwapRing::default();
        quoted
    }
}

/// `RiskArgs::validate` as an engine refusal: any out-of-bound field is `InvalidConfig`.
pub fn validate_risk(risk: &RiskArgs, max_oracle_age_ms: u64) -> Result<(), EngineError> {
    risk.validate(max_oracle_age_ms)
        .map_err(|_| EngineError::InvalidConfig)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures::coll;
    use risk_core::record;

    #[test]
    fn for_quote_strips_the_ring() {
        let base = coll();
        let ring = [1_000, 2_000, 3_000]
            .iter()
            .fold(TwapRing::default(), |r, &ts| record(&r, ts, 5, 1, ts, 0));
        assert_eq!(ring.samples.len(), 3);
        let full = CollateralState {
            ext: CollateralExt {
                ring,
                ..base.ext.clone()
            },
            ..base.clone()
        };
        let quoted = full.for_quote();
        assert_eq!(quoted.ext.ring, TwapRing::default());
        assert_eq!(
            CollateralState {
                ext: CollateralExt {
                    ring: full.ext.ring.clone(),
                    ..quoted.ext.clone()
                },
                ..quoted
            },
            full
        );
    }

    #[test]
    fn effective_level_is_3_under_admin_pause() {
        let base = coll();
        let at = |level: u8, admin_paused: bool| CollateralState {
            admin_paused,
            ext: CollateralExt {
                pause_level: level,
                ..base.ext.clone()
            },
            ..base.clone()
        };
        assert_eq!(at(1, true).effective_level(), 3);
        assert_eq!(at(2, false).effective_level(), 2);
        assert_eq!(at(0, false).effective_level(), 0);
    }

    #[test]
    fn validate_risk_refuses_as_invalid_config() {
        let bad = RiskArgs {
            twap_window_ms: 1,
            ..RiskArgs::legacy()
        };
        assert_eq!(
            validate_risk(&bad, 3_600_000),
            Err(EngineError::InvalidConfig)
        );
        assert_eq!(validate_risk(&RiskArgs::legacy(), 3_600_000), Ok(()));
    }
}
