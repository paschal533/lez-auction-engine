//! The two named `RiskArgs` presets (ADR-082, Plan 9 ruling R9-1). Behind the `presets` feature,
//! which only the SDK and the harness enable: on-chain code carries no production values (F8).

use crate::args::RiskArgs;

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
