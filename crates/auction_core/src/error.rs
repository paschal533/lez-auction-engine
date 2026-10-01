//! Error types shared across auction_core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    EmptySchedule,
    TooManyPoints,
    FirstPointNotZero,
    TimeNotIncreasing,
    DiscountDecreasing,
    DiscountAboveMax,
    PenaltyAboveMax,
    BandInvalid,
    OracleAgeAboveMax,
    ConfidenceAboveMax,
    ChostBelowMinBid,
    MintCapAboveHardCap,
}
