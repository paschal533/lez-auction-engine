//! Engine errors (Shared Reference R4). The guest maps them to `SpelError::Custom { code, message }`
//! (`Program error {code}: {message}`); the entry point prefixes `[6000+code]`.

use auction_core::{
    error::{AuctionError, ConfigError, PauseReason},
    math::MathError,
    system::SystemAuctionError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineError {
    NotAdmin,
    ZeroId,
    ZeroRefPrice,
    ThresholdBelowDebtLot,
    InvalidConfig,
    TypeCapAboveGlobal,
    BpsOutOfRange,
    ScheduleEncoding,
    TtlOutOfRange,
    ChipAbovePenalty,
    HostMismatch,
    WrongAccount(&'static str),
    OracleMismatch,
    WrongClock,
    ProtocolTokenLocked,
    RefPriceStepTooLarge,
    Paused,
    PriceFeedStale,
    PriceFeedLowConfidence,
    PriceFeedOutOfRange,
    NonceMismatch,
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
    ChainEndAccountsMissing,
    InsufficientBalance,
    RecipientMustSign,
    WrongDefinition,
    VaultBelowBook,
    TokenNotConfigured,
    UnbackedDebtPresent,
    BufferBelowTarget,
    DelayNotElapsed,
    InsufficientBadDebt,
    MintCapConfig,
    MintAuthorityMissing,
    ProtocolTokenMismatch,
    DebtAuctionAlreadyLive,
    SurplusAuctionAlreadyLive,
    NotLive,
    RecoveryInProgress,
    Math,
}

impl EngineError {
    #[must_use]
    pub const fn code(&self) -> u32 {
        match self {
            Self::NotAdmin => 1,
            Self::ZeroId => 2,
            Self::ZeroRefPrice => 3,
            Self::ThresholdBelowDebtLot => 4,
            Self::InvalidConfig => 5,
            Self::TypeCapAboveGlobal => 6,
            Self::BpsOutOfRange => 7,
            Self::ScheduleEncoding => 8,
            Self::TtlOutOfRange => 9,
            Self::ChipAbovePenalty => 10,
            Self::HostMismatch => 11,
            Self::WrongAccount(_) => 12,
            Self::OracleMismatch => 13,
            Self::WrongClock => 14,
            Self::ProtocolTokenLocked => 15,
            Self::RefPriceStepTooLarge => 16,
            Self::Paused => 20,
            Self::PriceFeedStale => 21,
            Self::PriceFeedLowConfidence => 22,
            Self::PriceFeedOutOfRange => 23,
            Self::NonceMismatch => 30,
            Self::ZeroDebt => 31,
            Self::VaultBelowCollateral => 32,
            Self::CapacityExhausted => 33,
            Self::NotAuctioning => 34,
            Self::ZeroFill => 35,
            Self::Slippage => 36,
            Self::BelowMinBid => 37,
            Self::DustRemainder => 38,
            Self::NotOwner => 39,
            Self::NothingToClaim => 40,
            Self::ChainEndAccountsMissing => 41,
            Self::InsufficientBalance => 42,
            Self::RecipientMustSign => 43,
            Self::WrongDefinition => 44,
            Self::VaultBelowBook => 45,
            Self::TokenNotConfigured => 50,
            Self::UnbackedDebtPresent => 51,
            Self::BufferBelowTarget => 52,
            Self::DelayNotElapsed => 53,
            Self::InsufficientBadDebt => 54,
            Self::MintCapConfig => 55,
            Self::MintAuthorityMissing => 56,
            Self::ProtocolTokenMismatch => 57,
            Self::DebtAuctionAlreadyLive => 58,
            Self::SurplusAuctionAlreadyLive => 59,
            Self::NotLive => 60,
            Self::RecoveryInProgress => 61,
            Self::Math => 99,
        }
    }
}

impl core::fmt::Display for EngineError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl From<AuctionError> for EngineError {
    fn from(e: AuctionError) -> Self {
        match e {
            AuctionError::ZeroDebt => Self::ZeroDebt,
            AuctionError::VaultBelowCollateral => Self::VaultBelowCollateral,
            AuctionError::CapacityExhausted => Self::CapacityExhausted,
            AuctionError::NotAuctioning => Self::NotAuctioning,
            AuctionError::ZeroFill => Self::ZeroFill,
            AuctionError::Slippage => Self::Slippage,
            AuctionError::BelowMinBid => Self::BelowMinBid,
            AuctionError::DustRemainder => Self::DustRemainder,
            AuctionError::NotOwner => Self::NotOwner,
            AuctionError::NothingToClaim => Self::NothingToClaim,
            AuctionError::Math => Self::Math,
        }
    }
}

impl From<SystemAuctionError> for EngineError {
    fn from(e: SystemAuctionError) -> Self {
        match e {
            SystemAuctionError::TokenNotConfigured => Self::TokenNotConfigured,
            SystemAuctionError::UnbackedDebtPresent => Self::UnbackedDebtPresent,
            SystemAuctionError::BufferBelowTarget => Self::BufferBelowTarget,
            SystemAuctionError::DelayNotElapsed => Self::DelayNotElapsed,
            SystemAuctionError::InsufficientBadDebt => Self::InsufficientBadDebt,
            SystemAuctionError::MintCapConfig => Self::MintCapConfig,
            SystemAuctionError::MintAuthorityMissing => Self::MintAuthorityMissing,
            SystemAuctionError::ProtocolTokenMismatch => Self::ProtocolTokenMismatch,
            SystemAuctionError::DebtAuctionAlreadyLive => Self::DebtAuctionAlreadyLive,
            SystemAuctionError::SurplusAuctionAlreadyLive => Self::SurplusAuctionAlreadyLive,
            SystemAuctionError::NotLive => Self::NotLive,
            SystemAuctionError::Slippage => Self::Slippage,
            SystemAuctionError::DustRemainder => Self::DustRemainder,
            SystemAuctionError::ZeroFill => Self::ZeroFill,
            SystemAuctionError::Math => Self::Math,
        }
    }
}

impl From<ConfigError> for EngineError {
    fn from(_: ConfigError) -> Self {
        Self::InvalidConfig
    }
}

impl From<MathError> for EngineError {
    fn from(_: MathError) -> Self {
        Self::Math
    }
}

impl From<PauseReason> for EngineError {
    fn from(r: PauseReason) -> Self {
        match r {
            PauseReason::AdminPaused => Self::Paused,
            PauseReason::Stale => Self::PriceFeedStale,
            PauseReason::LowConfidence => Self::PriceFeedLowConfidence,
            PauseReason::ZeroPrice | PauseReason::OutOfRange => Self::PriceFeedOutOfRange,
        }
    }
}
