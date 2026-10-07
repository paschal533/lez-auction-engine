//! Attacker programs used only by tests (never deployed outside tests).
#![forbid(unsafe_code)]

use lee_core::{account::AccountId, program::ProgramId};
use serde::{Deserialize, Serialize};

/// The `impostor` guest: a program that is NOT the bound liquidation engine and tries to reach the
/// engine-only hooks (`liquidate_position`, `engine_freeze_new_debt`) or the top-level-only
/// `set_liquidation_engine` of the host.
///
/// `mode` 0: forge `is_authorized` on the given authority, no seeds; mode 1: attach `seed` (authorises
/// `for_public_pda(IMPOSTOR_ID, seed)` legitimately).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImpostorInstruction {
    /// Chains `stablecoin.engine_freeze_new_debt`. Accounts: authority, liquidation_engine_state,
    /// protocol_parameters.
    AttackFreeze {
        host_program_id: ProgramId,
        mode: u8,
        seed: [u8; 32],
    },
    /// Chains `stablecoin.liquidate_position`. Accounts: authority, liquidation_engine_state, position,
    /// vault, custody, stability_fee_accumulator, redemption_price_state, market_price_oracle,
    /// protocol_parameters, clock.
    AttackLiquidate {
        host_program_id: ProgramId,
        mode: u8,
        seed: [u8; 32],
        collateral_to_seize: u128,
        normalized_debt_to_cover: u128,
    },
    /// Chains BOTH `engine_freeze_new_debt` and `liquidate_position` in one transaction. Accounts as
    /// `AttackLiquidate`.
    AttackBoth {
        host_program_id: ProgramId,
        mode: u8,
        seed: [u8; 32],
        collateral_to_seize: u128,
        normalized_debt_to_cover: u128,
    },
    /// Chains `stablecoin.set_liquidation_engine` with the admin's top-level signature forwarded.
    /// Accounts: admin, protocol_parameters, liquidation_engine_state.
    AttackSetEngine {
        host_program_id: ProgramId,
        engine_program_id: ProgramId,
        engine_authority_id: AccountId,
    },
    /// Laundering sink: re-chains `engine_freeze_new_debt` with the account flags it received (no
    /// seeds, nothing forged). Accounts: authority, liquidation_engine_state, protocol_parameters.
    ForwardFreeze { host_program_id: ProgramId },
    /// Laundering sink for `liquidate_position`. Accounts as `AttackLiquidate`.
    ForwardLiquidate {
        host_program_id: ProgramId,
        collateral_to_seize: u128,
        normalized_debt_to_cover: u128,
    },
}

/// The `flawed_engine` guest: models an engine (or any program that holds an engine-authority PDA)
/// that authorises its PDA for a caller-chosen program, which invariant (i) forbids the real engine
/// to do. Used only to reproduce S2's laundering attack end to end.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FlawedEngineInstruction {
    /// Authorises `for_public_pda(FLAWED_ENGINE_ID, seed)` for `target_program_id` and chains
    /// `ImpostorInstruction::ForwardFreeze` to it. Accounts: authority, liquidation_engine_state,
    /// protocol_parameters.
    LaunderFreeze {
        target_program_id: ProgramId,
        host_program_id: ProgramId,
        seed: [u8; 32],
    },
    /// As `LaunderFreeze`, forwarding to `liquidate_position`. Accounts as `AttackLiquidate`.
    LaunderLiquidate {
        target_program_id: ProgramId,
        host_program_id: ProgramId,
        seed: [u8; 32],
        collateral_to_seize: u128,
        normalized_debt_to_cover: u128,
    },
}
