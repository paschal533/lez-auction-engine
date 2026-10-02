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
pub enum AuctionError {
    ZeroDebt,
    VaultBelowCollateral,
    CapacityExhausted,
    NotAuctioning,
    ZeroFill,
    Slippage,
    BelowMinBid,
    DustRemainder,
    NotOwner,
    NothingToClaim,
    Math,
}

impl From<crate::math::MathError> for AuctionError {
    fn from(_: crate::math::MathError) -> Self {
        AuctionError::Math
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseReason {
    AdminPaused,
    Stale,
    ZeroPrice,
    LowConfidence,
    OutOfRange,
}
