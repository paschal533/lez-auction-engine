//! Every address a client needs, from the engine's and the host's own PDA functions (never
//! re-implemented: `auction_engine_core::pda`, `stablecoin_core::compute_*_pda`).

use auction_engine_core::pda::{self, SLOT_PROTOCOL_TOKEN, SLOT_REF_ORACLE};
use lee_core::{account::AccountId, program::ProgramId};
use stablecoin_core::{
    compute_liquidation_engine_state_pda, compute_position_pda, compute_position_vault_pda,
    compute_protocol_parameters_pda, compute_redemption_price_state_pda,
    compute_stability_fee_accumulator_pda, compute_stablecoin_definition_pda,
    compute_stablecoin_master_holding_pda,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostAddresses {
    pub program: ProgramId,
    pub protocol_parameters: AccountId,
    pub stability_fee_accumulator: AccountId,
    pub redemption_price_state: AccountId,
    pub stablecoin_definition: AccountId,
    pub stablecoin_master_holding: AccountId,
    pub liquidation_engine_state: AccountId,
}

impl HostAddresses {
    #[must_use]
    pub fn new(program: ProgramId) -> Self {
        Self {
            program,
            protocol_parameters: compute_protocol_parameters_pda(program),
            stability_fee_accumulator: compute_stability_fee_accumulator_pda(program),
            redemption_price_state: compute_redemption_price_state_pda(program),
            stablecoin_definition: compute_stablecoin_definition_pda(program),
            stablecoin_master_holding: compute_stablecoin_master_holding_pda(program),
            liquidation_engine_state: compute_liquidation_engine_state_pda(program),
        }
    }

    #[must_use]
    pub fn position(&self, owner: &AccountId, nonce: u64) -> AccountId {
        compute_position_pda(self.program, *owner, nonce)
    }

    #[must_use]
    pub fn position_vault(&self, position: &AccountId) -> AccountId {
        compute_position_vault_pda(self.program, *position)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineAddresses {
    pub program: ProgramId,
    pub config: AccountId,
    pub accounting: AccountId,
    pub buffer: AccountId,
    pub engine_authority: AccountId,
    pub mint_authority: AccountId,
    pub unused_ref_oracle: AccountId,
    pub unused_protocol_token: AccountId,
}

impl EngineAddresses {
    /// The config `["CONFIG", admin, salt]` (ADR-027) and everything seeded by it.
    #[must_use]
    pub fn new(program: ProgramId, admin: &AccountId, salt: u64) -> Self {
        Self::for_config(program, pda::config_pda(&program, admin, salt))
    }

    #[must_use]
    pub fn for_config(program: ProgramId, config: AccountId) -> Self {
        Self {
            program,
            config,
            accounting: pda::accounting_pda(&program, &config),
            buffer: pda::buffer_pda(&program, &config),
            engine_authority: pda::engine_authority_pda(&program, &config),
            mint_authority: pda::mint_authority_pda(&program, &config),
            unused_ref_oracle: pda::unused_slot_pda(&program, &config, SLOT_REF_ORACLE),
            unused_protocol_token: pda::unused_slot_pda(&program, &config, SLOT_PROTOCOL_TOKEN),
        }
    }

    #[must_use]
    pub fn coll_config(&self, collateral_definition: &AccountId) -> AccountId {
        pda::coll_config_pda(&self.program, &self.config, collateral_definition)
    }

    #[must_use]
    pub fn record(&self, position: &AccountId) -> AccountId {
        pda::liqrec_pda(&self.program, &self.config, position)
    }

    #[must_use]
    pub fn auction(&self, position: &AccountId, nonce: u64) -> AccountId {
        pda::auction_pda(&self.program, &self.config, position, nonce)
    }

    #[must_use]
    pub fn auction_vault(&self, auction: &AccountId) -> AccountId {
        pda::vault_pda(&self.program, auction)
    }

    #[must_use]
    pub fn surplus_auction(&self, nonce: u64) -> AccountId {
        pda::surplus_auction_pda(&self.program, &self.config, nonce)
    }

    #[must_use]
    pub fn surplus_vault(&self, nonce: u64) -> AccountId {
        pda::surplus_vault_pda(&self.program, &self.surplus_auction(nonce))
    }

    #[must_use]
    pub fn debt_auction(&self, nonce: u64) -> AccountId {
        pda::debt_auction_pda(&self.program, &self.config, nonce)
    }
}
