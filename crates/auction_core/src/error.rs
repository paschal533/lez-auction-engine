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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseReason {
    AdminPaused,
    Stale,
    ZeroPrice,
    LowConfidence,
    OutOfRange,
}
