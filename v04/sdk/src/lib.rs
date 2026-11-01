//! Client SDK for the LEZ 0.4 auction engine: build an instruction's [`Plan`] (root, declared
//! actors, signers, admission evidence, message), simulate it against any [`StateSource`], fit
//! its gas, and (in [`live`]) simulate and send it over a sequencer.
#![forbid(unsafe_code)]

pub mod builder;
pub mod decode;
pub mod live;
pub mod simulate;

use std::fmt;

pub use builder::{
    BidArgs, Deployment, InitArgs, LiquidateArgs, Plan, plan_bid, plan_claim_leftover, plan_init,
    plan_liquidate, plan_set_collateral, plan_settle_empty,
};
pub use live::{Sent, Snapshot, confirm, max_fee, send, send_signed, simulate};
pub use simulate::{MAX_GAS_EXEC, StateSource, fit_gas, simulate_with};

/// What can go wrong building, simulating or sending a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SdkError {
    /// Simulation says the transaction would fail; LEZ's reason.
    WouldRevert(String),
    /// The fitted gas exceeds the per-transaction cap.
    GasAboveCap { gas: u64, cap: u64 },
    /// A message or transaction does not encode.
    Encode(String),
    /// State the plan needs is absent or does not decode.
    MissingState(String),
    /// The sequencer or wallet failed.
    Rpc(String),
    /// The wallet holds no signing key for a payer or signer.
    MissingKey(lee::AccountId),
    /// The transaction was included (and charged) but its action reverted.
    Reverted(String),
}

impl fmt::Display for SdkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WouldRevert(reason) => write!(f, "the transaction would revert: {reason}"),
            Self::GasAboveCap { gas, cap } => write!(f, "gas {gas} exceeds the cap {cap}"),
            Self::Encode(reason) => write!(f, "encoding failed: {reason}"),
            Self::MissingState(reason) => write!(f, "missing state: {reason}"),
            Self::Rpc(reason) => write!(f, "sequencer: {reason}"),
            Self::MissingKey(account) => write!(f, "no signing key for {account}"),
            Self::Reverted(reason) => write!(f, "reverted on chain: {reason}"),
        }
    }
}

impl std::error::Error for SdkError {}
