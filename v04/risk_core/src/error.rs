//! The v04-only refusal codes (ADR-076), above the shared `EngineError`'s highest non-`Math` code
//! (61) and below `Math` (99). They print the same `Program error [6000+code]` text through
//! `engine_core::check::Refusal`.

/// Why a Tier 2 check refused a message. Wire code = 6000 + [`RiskError::code`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskError {
    /// No usable TWAP: too few samples or too short a window (6070).
    TwapUnavailable,
    /// Spot and TWAP differ by more than the configured band (6071).
    SpotTwapDeviation,
    /// Liquidations are paused at this level (6073).
    LiquidationsPaused,
    /// Surplus and debt auctions are paused at the system level (6074).
    SystemAuctionsPaused,
    /// Bids and takes are paused at this level (6075).
    BidsPaused,
    /// The last good TWAP is older than the D4 window allows (6077).
    LastGoodTwapTooOld,
}

impl RiskError {
    /// The refusal code; the wire code is `6000 + code`.
    #[must_use]
    pub const fn code(&self) -> u32 {
        match self {
            Self::TwapUnavailable => 70,
            Self::SpotTwapDeviation => 71,
            Self::LiquidationsPaused => 73,
            Self::SystemAuctionsPaused => 74,
            Self::BidsPaused => 75,
            Self::LastGoodTwapTooOld => 77,
        }
    }
}

impl core::fmt::Display for RiskError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_error_codes_are_70_and_up() {
        use RiskError::*;
        let codes: Vec<u32> = [
            TwapUnavailable,
            SpotTwapDeviation,
            LiquidationsPaused,
            SystemAuctionsPaused,
            BidsPaused,
            LastGoodTwapTooOld,
        ]
        .iter()
        .map(RiskError::code)
        .collect();
        assert_eq!(codes, [70, 71, 73, 74, 75, 77]);
    }
}
