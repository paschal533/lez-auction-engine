//! Binding between this stablecoin instance and its liquidation engine (lez-auction-engine hooks).
//!
//! Read by `liquidate_position` and `engine_freeze_new_debt` to authenticate the engine with both
//! the immediate caller program id and an authorised engine PDA (lez-auction-engine ADR-019, Variant C).

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{
    account::{AccountId, Data},
    program::{PdaSeed, ProgramId},
};
use serde::{Deserialize, Serialize};
use spel_framework_macros::account_type;

/// PDA seed domain for the single-instance [`LiquidationEngineState`] account.
const LIQUIDATION_ENGINE_STATE_PDA_DOMAIN: &[u8; 32] = b"STABLECOIN__LIQUIDATION_ENGINE__";

/// The engine allowed to call the engine-only hooks.
#[account_type]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct LiquidationEngineState {
    /// Must equal the hook's immediate `caller_program_id`.
    pub engine_program_id: ProgramId,
    /// Engine PDA that must be authorised in the hook call.
    pub engine_authority_id: AccountId,
}

impl TryFrom<&Data> for LiquidationEngineState {
    type Error = std::io::Error;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        Self::try_from_slice(data.as_ref())
    }
}

impl From<&LiquidationEngineState> for Data {
    fn from(state: &LiquidationEngineState) -> Self {
        let len = borsh::object_length(state).expect("LiquidationEngineState length must be known");
        let mut buf = Vec::with_capacity(len);
        BorshSerialize::serialize(state, &mut buf)
            .expect("LiquidationEngineState serialization should not fail");
        Self::try_from(buf).expect("LiquidationEngineState encoded data should fit into Data")
    }
}

/// PDA seed for [`LiquidationEngineState`] (single instance per stablecoin program id).
#[must_use]
pub fn compute_liquidation_engine_state_pda_seed() -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256 as _};

    let mut out = [0u8; 32];
    out.copy_from_slice(Impl::hash_bytes(LIQUIDATION_ENGINE_STATE_PDA_DOMAIN).as_bytes());
    PdaSeed::new(out)
}

/// Account id of the [`LiquidationEngineState`] PDA under `stablecoin_program_id`.
#[must_use]
pub fn compute_liquidation_engine_state_pda(stablecoin_program_id: ProgramId) -> AccountId {
    AccountId::for_public_pda(
        &stablecoin_program_id,
        &compute_liquidation_engine_state_pda_seed(),
    )
}
