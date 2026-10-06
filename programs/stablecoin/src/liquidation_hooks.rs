//! Liquidation-engine hooks appended by lez-auction-engine (offered upstream).
//!
//! Authentication is Variant C (lez-auction-engine ADR-019): the immediate `caller_program_id`
//! must be the bound engine program AND the bound engine-authority PDA must be authorised.
//! `caller_program_id` alone would accept any program the engine chained to with a forwarded
//! authorisation only if the engine misbehaved; the PDA alone is launderable because lee keeps an
//! authorised account authorised down the call chain (`validated_state_diff/mod.rs:262-290`).

use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, Claim, ProgramId, DEFAULT_PROGRAM_ID},
};
use stablecoin_core::{
    compute_liquidation_engine_state_pda, compute_liquidation_engine_state_pda_seed,
    compute_protocol_parameters_pda, LiquidationEngineState, ProtocolParameters,
};

/// Host bound on the engine's liquidation penalty, used by the proportionality check.
pub const MAX_PENALTY_BPS: u128 = 2_500;

fn load_parameters(
    protocol_parameters: &AccountWithMetadata,
    program_id: ProgramId,
) -> ProtocolParameters {
    ProtocolParameters::try_from(&crate::checks::decode_global(
        protocol_parameters,
        compute_protocol_parameters_pda(program_id),
        program_id,
        "ProtocolParameters",
    ))
    .expect("ProtocolParameters must decode")
}

pub(crate) fn load_engine_state(
    state: &AccountWithMetadata,
    program_id: ProgramId,
) -> LiquidationEngineState {
    LiquidationEngineState::try_from(&crate::checks::decode_global(
        state,
        compute_liquidation_engine_state_pda(program_id),
        program_id,
        "LiquidationEngineState",
    ))
    .expect("LiquidationEngineState must decode")
}

/// Variant C guard shared by both engine-only hooks.
///
/// # Panics
/// If the immediate caller is not the bound engine program, the authority account is not the
/// bound engine authority, or it is not authorised in this call.
pub fn assert_engine_call(
    caller_program_id: ProgramId,
    engine_authority: &AccountWithMetadata,
    state: &LiquidationEngineState,
) {
    assert_eq!(
        caller_program_id, state.engine_program_id,
        "Caller program is not the liquidation engine"
    );
    assert_eq!(
        engine_authority.account_id, state.engine_authority_id,
        "Engine authority account is not the bound engine authority"
    );
    assert!(
        engine_authority.is_authorized,
        "Engine authority authorization is missing"
    );
}

/// Bind (or rotate) the liquidation engine. Admin-only, top-level only.
///
/// # Panics
/// Not a top-level call; admin not authorised or not the stored admin; zero engine program id or
/// zero authority id; `liquidation_engine_state` not at its PDA or owned by another program.
#[allow(
    clippy::too_many_arguments,
    reason = "three accounts plus context and two arguments mirror the guest ABI"
)]
pub fn set_liquidation_engine(
    admin: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    liquidation_engine_state: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    caller_program_id: ProgramId,
    engine_program_id: ProgramId,
    engine_authority_id: AccountId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    assert_eq!(
        caller_program_id, DEFAULT_PROGRAM_ID,
        "set_liquidation_engine must be a top-level call"
    );
    let parameters = load_parameters(&protocol_parameters, stablecoin_program_id);
    crate::checks::assert_admin_authorized(&admin, &parameters);
    assert_ne!(
        engine_program_id, DEFAULT_PROGRAM_ID,
        "Engine program id must be non-zero"
    );
    assert_ne!(
        engine_authority_id,
        AccountId::new([0; 32]),
        "Engine authority id must be non-zero"
    );
    assert_eq!(
        liquidation_engine_state.account_id,
        compute_liquidation_engine_state_pda(stablecoin_program_id),
        "LiquidationEngineState account ID does not match expected PDA derivation"
    );
    let is_new = liquidation_engine_state.account == Account::default();
    if !is_new {
        assert_eq!(
            liquidation_engine_state.account.program_owner, stablecoin_program_id,
            "LiquidationEngineState account must be owned by the stablecoin program"
        );
    }
    let mut state_post = liquidation_engine_state.account;
    state_post.data = Data::from(&LiquidationEngineState {
        engine_program_id,
        engine_authority_id,
    });
    let state_post = if is_new {
        AccountPostState::new_claimed(
            state_post,
            Claim::Pda(compute_liquidation_engine_state_pda_seed()),
        )
    } else {
        AccountPostState::new(state_post)
    };
    (
        vec![
            AccountPostState::new(admin.account),
            AccountPostState::new(protocol_parameters.account),
            state_post,
        ],
        vec![],
    )
}

/// Set `ProtocolParameters.is_frozen` on the engine's behalf (F4 "pause new debt generation").
///
/// # Panics
/// The Variant C guard fails, or a global does not validate.
pub fn engine_freeze_new_debt(
    engine_authority: AccountWithMetadata,
    liquidation_engine_state: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    caller_program_id: ProgramId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    let state = load_engine_state(&liquidation_engine_state, stablecoin_program_id);
    assert_engine_call(caller_program_id, &engine_authority, &state);
    let mut parameters = load_parameters(&protocol_parameters, stablecoin_program_id);
    parameters.is_frozen = true;
    let mut parameters_post = protocol_parameters.account;
    parameters_post.data = Data::from(&parameters);
    (
        vec![
            AccountPostState::new(engine_authority.account),
            AccountPostState::new(liquidation_engine_state.account),
            AccountPostState::new(parameters_post),
        ],
        vec![],
    )
}

#[cfg(test)]
#[allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "test fixtures build accounts from constants, index fixed-size outputs and assert panics via #[should_panic]"
)]
mod tests {
    use lee_core::{
        account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
        program::{ProgramId, DEFAULT_PROGRAM_ID},
    };
    use stablecoin_core::{
        compute_liquidation_engine_state_pda, LiquidationEngineState, ProtocolParameters,
    };

    use super::*;
    use crate::test_support::*;

    pub(super) const ENGINE: ProgramId = [0xE1; 8];
    pub(super) const IMPOSTOR: ProgramId = [0xBA; 8];

    pub(super) fn engine_authority_id() -> AccountId {
        AccountId::new([0xEA; 32])
    }
    pub(super) fn signer(id: AccountId) -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: true,
            account_id: id,
        }
    }
    pub(super) fn engine_authority(authorized: bool) -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: authorized,
            account_id: engine_authority_id(),
        }
    }
    pub(super) fn engine_state() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: STABLECOIN_PROGRAM_ID,
                balance: 0,
                data: Data::from(&LiquidationEngineState {
                    engine_program_id: ENGINE,
                    engine_authority_id: engine_authority_id(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: compute_liquidation_engine_state_pda(STABLECOIN_PROGRAM_ID),
        }
    }
    fn unbound_state() -> AccountWithMetadata {
        uninitialized(compute_liquidation_engine_state_pda(STABLECOIN_PROGRAM_ID))
    }
    fn decode_params(post: &lee_core::program::AccountPostState) -> ProtocolParameters {
        ProtocolParameters::try_from(&post.account().data).unwrap()
    }

    #[test]
    fn admin_binds_engine_on_first_use() {
        let (post, calls) = set_liquidation_engine(
            signer(admin_id()),
            protocol_parameters_account(ParameterOverrides::default()),
            unbound_state(),
            STABLECOIN_PROGRAM_ID,
            DEFAULT_PROGRAM_ID,
            ENGINE,
            engine_authority_id(),
        );
        assert!(calls.is_empty());
        let state = LiquidationEngineState::try_from(&post[2].account().data).unwrap();
        assert_eq!(state.engine_program_id, ENGINE);
        assert_eq!(state.engine_authority_id, engine_authority_id());
        assert!(
            post[2].required_claim().is_some(),
            "first bind claims the PDA"
        );
    }

    #[test]
    fn admin_can_rotate_the_binding() {
        let (post, _) = set_liquidation_engine(
            signer(admin_id()),
            protocol_parameters_account(ParameterOverrides::default()),
            engine_state(),
            STABLECOIN_PROGRAM_ID,
            DEFAULT_PROGRAM_ID,
            [0xE2; 8],
            AccountId::new([0xEB; 32]),
        );
        let state = LiquidationEngineState::try_from(&post[2].account().data).unwrap();
        assert_eq!(state.engine_program_id, [0xE2; 8]);
        assert!(post[2].required_claim().is_none());
    }

    #[test]
    #[should_panic(expected = "set_liquidation_engine must be a top-level call")]
    fn bind_through_another_program_is_rejected() {
        set_liquidation_engine(
            signer(admin_id()),
            protocol_parameters_account(ParameterOverrides::default()),
            unbound_state(),
            STABLECOIN_PROGRAM_ID,
            IMPOSTOR,
            ENGINE,
            engine_authority_id(),
        );
    }

    #[test]
    #[should_panic(expected = "Signer is not the protocol's admin")]
    fn non_admin_cannot_bind() {
        set_liquidation_engine(
            signer(caller_id()),
            protocol_parameters_account(ParameterOverrides::default()),
            unbound_state(),
            STABLECOIN_PROGRAM_ID,
            DEFAULT_PROGRAM_ID,
            ENGINE,
            engine_authority_id(),
        );
    }

    #[test]
    #[should_panic(expected = "Engine program id must be non-zero")]
    fn zero_engine_program_id_rejected() {
        set_liquidation_engine(
            signer(admin_id()),
            protocol_parameters_account(ParameterOverrides::default()),
            unbound_state(),
            STABLECOIN_PROGRAM_ID,
            DEFAULT_PROGRAM_ID,
            DEFAULT_PROGRAM_ID,
            engine_authority_id(),
        );
    }

    #[test]
    #[should_panic(expected = "Engine authority id must be non-zero")]
    fn zero_engine_authority_rejected() {
        set_liquidation_engine(
            signer(admin_id()),
            protocol_parameters_account(ParameterOverrides::default()),
            unbound_state(),
            STABLECOIN_PROGRAM_ID,
            DEFAULT_PROGRAM_ID,
            ENGINE,
            AccountId::new([0; 32]),
        );
    }

    #[test]
    fn engine_freeze_sets_is_frozen() {
        let (post, calls) = engine_freeze_new_debt(
            engine_authority(true),
            engine_state(),
            protocol_parameters_account(ParameterOverrides::default()),
            STABLECOIN_PROGRAM_ID,
            ENGINE,
        );
        assert!(calls.is_empty());
        assert!(decode_params(&post[2]).is_frozen);
    }

    #[test]
    #[should_panic(expected = "Caller program is not the liquidation engine")]
    fn freeze_from_impostor_program_rejected() {
        engine_freeze_new_debt(
            engine_authority(true),
            engine_state(),
            protocol_parameters_account(ParameterOverrides::default()),
            STABLECOIN_PROGRAM_ID,
            IMPOSTOR,
        );
    }

    #[test]
    #[should_panic(expected = "Caller program is not the liquidation engine")]
    fn freeze_top_level_rejected() {
        engine_freeze_new_debt(
            engine_authority(true),
            engine_state(),
            protocol_parameters_account(ParameterOverrides::default()),
            STABLECOIN_PROGRAM_ID,
            DEFAULT_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "Engine authority authorization is missing")]
    fn freeze_without_authority_rejected() {
        engine_freeze_new_debt(
            engine_authority(false),
            engine_state(),
            protocol_parameters_account(ParameterOverrides::default()),
            STABLECOIN_PROGRAM_ID,
            ENGINE,
        );
    }

    #[test]
    #[should_panic(expected = "Engine authority account is not the bound engine authority")]
    fn freeze_with_authorised_but_wrong_authority_rejected() {
        engine_freeze_new_debt(
            signer(AccountId::new([0xEB; 32])),
            engine_state(),
            protocol_parameters_account(ParameterOverrides::default()),
            STABLECOIN_PROGRAM_ID,
            ENGINE,
        );
    }

    #[test]
    #[should_panic(expected = "LiquidationEngineState account must be initialized")]
    fn freeze_with_no_binding_rejected() {
        engine_freeze_new_debt(
            engine_authority(true),
            unbound_state(),
            protocol_parameters_account(ParameterOverrides::default()),
            STABLECOIN_PROGRAM_ID,
            ENGINE,
        );
    }

    #[test]
    #[should_panic(expected = "Signer is not the protocol's freeze authority")]
    fn only_freeze_authority_can_unfreeze() {
        let frozen = protocol_parameters_account(ParameterOverrides {
            is_frozen: true,
            ..ParameterOverrides::default()
        });
        crate::freeze::unfreeze(engine_authority(true), frozen, STABLECOIN_PROGRAM_ID);
    }
}
