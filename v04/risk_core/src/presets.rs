//! The two named `RiskArgs` presets (ADR-082, Plan 9 ruling R9-1). Behind the `presets` feature,
//! which only the SDK and the harness enable: on-chain code carries no production values (F8).

use crate::args::RiskArgs;
use crate::engine_ext::EngineExtArgs;

impl EngineExtArgs {
    /// v04 as built: 10 % reference confidence, 20 % clamp, 10 min relist delay, a 50 %..200 %
    /// sanity window and the reference oracle read at scale 0.
    #[must_use]
    pub const fn legacy() -> Self {
        Self {
            ref_conf_bps: 1_000,
            ref_clamp_bps: 2_000,
            debt_relist_delay_ms: 600_000,
            ref_sanity_lo_bps: 5_000,
            ref_sanity_hi_bps: 20_000,
            ref_scale: 0,
        }
    }

    /// The production proposal: the same values as [`Self::legacy`] (spec §3.9: E9 is about who
    /// chooses, not what).
    #[must_use]
    pub const fn recommended() -> Self {
        Self::legacy()
    }
}

impl RiskArgs {
    /// v04 as built: TWAP, deviation breaker and D4 all off.
    #[must_use]
    pub const fn legacy() -> Self {
        Self {
            twap_window_ms: 0,
            twap_min_spacing_ms: 0,
            twap_min_obs: 0,
            max_spot_twap_dev_bps: 0,
            stale_bid_max_age_ms: 0,
        }
    }

    /// The production proposal (`f8-parameters.md`): a 30 min TWAP of 5 min samples, at least 3
    /// of them, a ±20 % spot-vs-TWAP band, and stale bids for up to 1 h after the newest sample.
    #[must_use]
    pub const fn recommended() -> Self {
        Self {
            twap_window_ms: 1_800_000,
            twap_min_spacing_ms: 300_000,
            twap_min_obs: 3,
            max_spot_twap_dev_bps: 2_000,
            stale_bid_max_age_ms: 3_600_000,
        }
    }
}
