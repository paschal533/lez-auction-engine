//! Liquidation-engine hooks appended by lez-auction-engine (offered upstream).
//!
//! Authentication is Variant C (lez-auction-engine ADR-019): the immediate `caller_program_id`
//! must be the bound engine program AND the bound engine-authority PDA must be authorised.
//! `caller_program_id` alone trusts every engine code path; the authority PDA alone is launderable
//! because lee propagates authorisation down the chain (ADR-019,
//! `validated_state_diff/mod.rs:262-290`).

use alloy_primitives::U512;
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, Claim, ProgramId, DEFAULT_PROGRAM_ID},
};
use stablecoin_core::{
    compute_liquidation_engine_state_pda, compute_liquidation_engine_state_pda_seed,
    compute_position_pda, compute_protocol_parameters_pda, compute_redemption_price_state_pda,
    compute_stability_fee_accumulator_pda,
    math::{compute_current_accumulated_rate_wide, compute_current_redemption_price_wide},
    verify_position_vault_and_get_seed, LiquidationEngineState, Position, ProtocolParameters,
    RedemptionPriceState, StabilityFeeAccumulator,
};
use token_core::TokenHolding;

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
    assert_ne!(
        state.engine_program_id, DEFAULT_PROGRAM_ID,
        "Bound engine program id must be non-zero"
    );
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

const BPS: u128 = 10_000;

/// `min(collateral, ceil(collateral × cover / debt × (1e4 + MAX_PENALTY_BPS) / 1e4))` in U512
/// (owner protection independent of the engine, lez-auction-engine spec §5.10). `debt > 0`.
pub(crate) fn proportional_seize_bound(collateral: u128, cover: u128, debt: u128) -> u128 {
    // Checked U512 arithmetic (upstream's denied `arithmetic_side_effects` style); cannot overflow:
    // u128 × u128 × 12_500 < 2^270.
    let factor = U512::from(BPS.checked_add(MAX_PENALTY_BPS).expect("constant"));
    let numerator = U512::from(collateral)
        .checked_mul(U512::from(cover))
        .and_then(|v| v.checked_mul(factor))
        .expect("fits U512");
    let denominator = U512::from(debt)
        .checked_mul(U512::from(BPS))
        .expect("fits U512");
    let bound = numerator
        .checked_add(denominator)
        .and_then(|v| v.checked_sub(U512::from(1u64)))
        .and_then(|v| v.checked_div(denominator))
        .expect("debt > 0 is asserted before the bound");
    if bound >= U512::from(collateral) {
        collateral
    } else {
        bound.to::<u128>()
    }
}

fn fungible_balance(holding: &TokenHolding, label: &str) -> u128 {
    match holding {
        TokenHolding::Fungible { balance, .. } => *balance,
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            panic!("{label} must be a fungible holding")
        }
    }
}

fn decode<T: for<'a> TryFrom<&'a Data>>(
    account: &AccountWithMetadata,
    expected: AccountId,
    program_id: ProgramId,
    label: &str,
) -> T {
    T::try_from(&crate::checks::decode_global(
        account, expected, program_id, label,
    ))
    .unwrap_or_else(|_| panic!("{label} must decode"))
}

/// Seize collateral from an unsafe position into the engine's custody holding.
///
/// Full seizure (both arguments equal the position's values) zeroes the position and moves the
/// WHOLE vault balance, sweeping donations. Partial seizure subtracts both amounts and moves
/// exactly `collateral_to_seize`. Allowed while frozen (it reduces risk).
///
/// # Panics
/// Variant C guard; uninitialised or foreign position or wrong PDA; zero debt; out-of-bounds
/// amounts or above the proportional bound; vault PDA, definition or balance (`>=` collateral);
/// custody uninitialised, of another token program or definition; stale or future-dated oracle;
/// redemption price projected to zero; the position is collateralised at the minimum ratio.
#[allow(
    clippy::too_many_arguments,
    reason = "ten account inputs plus context and two arguments mirror the guest ABI"
)]
pub fn liquidate_position(
    engine_authority: AccountWithMetadata,
    liquidation_engine_state: AccountWithMetadata,
    position: AccountWithMetadata,
    vault: AccountWithMetadata,
    custody: AccountWithMetadata,
    stability_fee_accumulator: AccountWithMetadata,
    redemption_price_state: AccountWithMetadata,
    market_price_oracle: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    clock: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    caller_program_id: ProgramId,
    collateral_to_seize: u128,
    normalized_debt_to_cover: u128,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    let engine = load_engine_state(&liquidation_engine_state, stablecoin_program_id);
    assert_engine_call(caller_program_id, &engine_authority, &engine);

    let parameters = load_parameters(&protocol_parameters, stablecoin_program_id);
    let accumulator: StabilityFeeAccumulator = decode(
        &stability_fee_accumulator,
        compute_stability_fee_accumulator_pda(stablecoin_program_id),
        stablecoin_program_id,
        "StabilityFeeAccumulator",
    );
    let redemption: RedemptionPriceState = decode(
        &redemption_price_state,
        compute_redemption_price_state_pda(stablecoin_program_id),
        stablecoin_program_id,
        "RedemptionPriceState",
    );
    let now = crate::accrue_stability_fee::read_clock(&clock);
    let oracle = crate::update_redemption_rate::decode_oracle(&market_price_oracle, &parameters);
    let oracle_age = now
        .checked_sub(oracle.timestamp)
        .expect("Market price oracle observation is dated in the future");
    assert!(
        oracle_age <= parameters.maximum_oracle_price_age_milliseconds,
        "Market price oracle observation is stale"
    );

    assert_ne!(
        position.account,
        Account::default(),
        "Position account must be initialized"
    );
    assert_eq!(
        position.account.program_owner, stablecoin_program_id,
        "Position is not owned by this stablecoin program"
    );
    let current = Position::try_from(&position.account.data)
        .expect("Position account must hold valid Position state");
    assert_eq!(
        position.account_id,
        compute_position_pda(
            stablecoin_program_id,
            current.owner_account_id,
            current.position_nonce
        ),
        "Position account ID does not match expected derivation"
    );
    assert!(current.normalized_debt_amount > 0, "Position has no debt");
    assert!(
        normalized_debt_to_cover > 0 && normalized_debt_to_cover <= current.normalized_debt_amount,
        "normalized_debt_to_cover out of bounds"
    );
    assert!(
        collateral_to_seize > 0 && collateral_to_seize <= current.collateral_amount,
        "collateral_to_seize out of bounds"
    );
    assert!(
        collateral_to_seize
            <= proportional_seize_bound(
                current.collateral_amount,
                normalized_debt_to_cover,
                current.normalized_debt_amount
            ),
        "collateral_to_seize exceeds the proportional bound"
    );

    let vault_seed =
        verify_position_vault_and_get_seed(&vault, position.account_id, stablecoin_program_id);
    assert_eq!(
        current.vault_account_id, vault.account_id,
        "Position vault_account_id does not match the vault account"
    );
    let vault_holding = TokenHolding::try_from(&vault.account.data)
        .expect("Vault account must hold a valid TokenHolding");
    assert_eq!(
        vault_holding.definition_id(),
        parameters.collateral_definition_id,
        "Vault holding does not match the protocol's collateral definition"
    );
    let vault_balance = fungible_balance(&vault_holding, "Vault holding");
    assert!(
        vault_balance >= current.collateral_amount,
        "Vault balance is below the position collateral"
    );
    let token_program_id = vault.account.program_owner;
    assert_ne!(
        custody.account,
        Account::default(),
        "Custody holding must be initialized"
    );
    assert_eq!(
        custody.account.program_owner, token_program_id,
        "Custody holding must be owned by the vault's Token Program"
    );
    let custody_holding = TokenHolding::try_from(&custody.account.data)
        .expect("Custody account must hold a valid TokenHolding");
    assert_eq!(
        custody_holding.definition_id(),
        parameters.collateral_definition_id,
        "Custody holding does not match the protocol's collateral definition"
    );

    let redemption_price = compute_current_redemption_price_wide(
        redemption.redemption_price_at_last_update,
        redemption.redemption_rate_per_millisecond,
        redemption.last_updated_at,
        now,
    );
    assert!(
        !redemption_price.is_zero(),
        "Redemption price projected to zero"
    );
    let collateralized = crate::checks::is_position_collateralized(
        &current,
        compute_current_accumulated_rate_wide(
            accumulator.accumulated_rate_at_last_accrual,
            parameters.stability_fee_per_millisecond,
            accumulator.last_accrued_at,
            now,
        ),
        redemption_price,
        parameters.minimum_collateralization_ratio,
    );
    assert!(
        !collateralized,
        "Position is not liquidatable: it is collateralized"
    );

    let full = collateral_to_seize == current.collateral_amount
        && normalized_debt_to_cover == current.normalized_debt_amount;
    let updated = Position {
        owner_account_id: current.owner_account_id,
        position_nonce: current.position_nonce,
        vault_account_id: current.vault_account_id,
        collateral_amount: current
            .collateral_amount
            .checked_sub(collateral_to_seize)
            .expect("bounded above"),
        normalized_debt_amount: current
            .normalized_debt_amount
            .checked_sub(normalized_debt_to_cover)
            .expect("bounded above"),
        opened_at: current.opened_at,
    };
    let amount = if full {
        vault_balance
    } else {
        collateral_to_seize
    };

    let mut position_post = position.account.clone();
    position_post.data = Data::from(&updated);
    let post_states = vec![
        AccountPostState::new(engine_authority.account),
        AccountPostState::new(liquidation_engine_state.account),
        AccountPostState::new(position_post),
        AccountPostState::new(vault.account.clone()),
        AccountPostState::new(custody.account.clone()),
        AccountPostState::new(stability_fee_accumulator.account),
        AccountPostState::new(redemption_price_state.account),
        AccountPostState::new(market_price_oracle.account),
        AccountPostState::new(protocol_parameters.account),
        AccountPostState::new(clock.account),
    ];
    let mut vault_authorized = vault;
    vault_authorized.is_authorized = true;
    let transfer = ChainedCall::new(
        token_program_id,
        vec![vault_authorized, custody],
        &token_core::Instruction::Transfer {
            amount_to_transfer: amount,
        },
    )
    .with_pda_seeds(vec![vault_seed]);
    (post_states, vec![transfer])
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
        let input = ProtocolParameters::try_from(
            &protocol_parameters_account(ParameterOverrides::default())
                .account
                .data,
        )
        .unwrap();
        assert!(!input.is_frozen);
        assert_eq!(
            decode_params(&post[2]),
            ProtocolParameters {
                is_frozen: true,
                ..input
            }
        );
    }

    #[test]
    #[should_panic(expected = "Bound engine program id must be non-zero")]
    fn guard_rejects_state_with_zero_engine_program() {
        let state = LiquidationEngineState {
            engine_program_id: DEFAULT_PROGRAM_ID,
            engine_authority_id: engine_authority_id(),
        };
        assert_engine_call(DEFAULT_PROGRAM_ID, &engine_authority(true), &state);
    }

    #[test]
    #[should_panic(expected = "does not match expected PDA derivation")]
    fn bind_to_wrong_state_account_rejected() {
        set_liquidation_engine(
            signer(admin_id()),
            protocol_parameters_account(ParameterOverrides::default()),
            protocol_parameters_account(ParameterOverrides::default()),
            STABLECOIN_PROGRAM_ID,
            DEFAULT_PROGRAM_ID,
            ENGINE,
            engine_authority_id(),
        );
    }

    #[test]
    #[should_panic(expected = "must be owned by the stablecoin program")]
    fn bind_over_foreign_owned_state_rejected() {
        let mut foreign = engine_state();
        foreign.account.program_owner = IMPOSTOR;
        set_liquidation_engine(
            signer(admin_id()),
            protocol_parameters_account(ParameterOverrides::default()),
            foreign,
            STABLECOIN_PROGRAM_ID,
            DEFAULT_PROGRAM_ID,
            ENGINE,
            engine_authority_id(),
        );
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

    use stablecoin_core::{compute_position_pda, compute_position_vault_pda, Position};
    use token_core::TokenHolding;

    const TOKEN: ProgramId = [6u32; 8];

    fn owner_id() -> AccountId {
        AccountId::new([0x0E; 32])
    }
    fn position_id() -> AccountId {
        compute_position_pda(STABLECOIN_PROGRAM_ID, owner_id(), 0)
    }
    fn vault_id() -> AccountId {
        compute_position_vault_pda(STABLECOIN_PROGRAM_ID, position_id())
    }
    fn custody_id() -> AccountId {
        AccountId::new([0xC5; 32])
    }
    fn position(collateral: u128, debt: u128) -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: STABLECOIN_PROGRAM_ID,
                balance: 0,
                data: Data::from(&Position {
                    owner_account_id: owner_id(),
                    position_nonce: 0,
                    vault_account_id: vault_id(),
                    collateral_amount: collateral,
                    normalized_debt_amount: debt,
                    opened_at: 0,
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: position_id(),
        }
    }
    fn holding(id: AccountId, definition: AccountId, balance: u128) -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN,
                balance: 0,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: definition,
                    balance,
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: id,
        }
    }

    struct Case {
        caller: ProgramId,
        authorized: bool,
        collateral: u128,
        debt: u128,
        vault_balance: u128,
        seize: u128,
        cover: u128,
        oracle_ts: u64,
        frozen: bool,
    }

    /// Defaults: min CR 1.5, redemption 0.5, accumulator 1.0 → liquidatable iff collateral < 0.75 × debt.
    fn unsafe_case() -> Case {
        Case {
            caller: ENGINE,
            authorized: true,
            collateral: 1_000,
            debt: 2_000,
            vault_balance: 1_000,
            seize: 1_000,
            cover: 2_000,
            oracle_ts: NOW,
            frozen: false,
        }
    }

    fn run(
        c: Case,
    ) -> (
        Vec<lee_core::program::AccountPostState>,
        Vec<lee_core::program::ChainedCall>,
    ) {
        let accounts = Accounts::for_case(&c);
        run_with(&c, accounts)
    }

    struct Accounts {
        position: AccountWithMetadata,
        vault: AccountWithMetadata,
        custody: AccountWithMetadata,
        redemption: AccountWithMetadata,
    }

    impl Accounts {
        fn for_case(c: &Case) -> Self {
            Self {
                position: position(c.collateral, c.debt),
                vault: holding(vault_id(), collateral_definition_id(), c.vault_balance),
                custody: holding(custody_id(), collateral_definition_id(), 0),
                redemption: redemption_price_state_account(NOW),
            }
        }
    }

    fn run_with(
        c: &Case,
        a: Accounts,
    ) -> (
        Vec<lee_core::program::AccountPostState>,
        Vec<lee_core::program::ChainedCall>,
    ) {
        liquidate_position(
            engine_authority(c.authorized),
            engine_state(),
            a.position,
            a.vault,
            a.custody,
            accumulator_account(ACCUMULATOR_ANCHOR, NOW),
            a.redemption,
            oracle_account(c.oracle_ts, REDEMPTION_PRICE_ANCHOR),
            protocol_parameters_account(ParameterOverrides {
                is_frozen: c.frozen,
                ..ParameterOverrides::default()
            }),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
            c.caller,
            c.seize,
            c.cover,
        )
    }

    fn transferred(calls: &[lee_core::program::ChainedCall], amount: u128) -> bool {
        let expected = lee_core::program::ChainedCall::new(
            TOKEN,
            vec![],
            &token_core::Instruction::Transfer {
                amount_to_transfer: amount,
            },
        );
        calls.len() == 1
            && calls[0].program_id == TOKEN
            && calls[0].instruction_data == expected.instruction_data
            && calls[0].pda_seeds.len() == 1
            && AccountId::for_public_pda(&STABLECOIN_PROGRAM_ID, &calls[0].pda_seeds[0])
                == vault_id()
            && calls[0].pre_states[0].account_id == vault_id()
            && calls[0].pre_states[0].is_authorized
            && calls[0].pre_states[1].account_id == custody_id()
    }

    fn position_after(post: &[lee_core::program::AccountPostState]) -> Position {
        Position::try_from(&post[2].account().data).unwrap()
    }

    #[test]
    fn full_seizure_zeroes_position_and_moves_vault() {
        let (post, calls) = run(unsafe_case());
        let p = position_after(&post);
        assert_eq!((p.collateral_amount, p.normalized_debt_amount), (0, 0));
        assert!(transferred(&calls, 1_000));
    }

    #[test]
    fn donation_into_vault_is_swept() {
        // Review Focus 3: a third party raised the vault balance above the position's collateral.
        let (_, calls) = run(Case {
            vault_balance: 1_005,
            ..unsafe_case()
        });
        assert!(transferred(&calls, 1_005));
    }

    #[test]
    fn partial_seizure_moves_exactly_the_amount() {
        // bound = ceil(1000 × 1000 / 2000 × 1.25) = 625
        let (post, calls) = run(Case {
            seize: 400,
            cover: 1_000,
            vault_balance: 1_010,
            ..unsafe_case()
        });
        let p = position_after(&post);
        assert_eq!(
            (p.collateral_amount, p.normalized_debt_amount),
            (600, 1_000)
        );
        assert!(transferred(&calls, 400));
    }

    #[test]
    fn allowed_while_frozen() {
        let (post, _) = run(Case {
            frozen: true,
            ..unsafe_case()
        });
        assert_eq!(position_after(&post).collateral_amount, 0);
    }

    #[test]
    #[should_panic(expected = "collateral_to_seize exceeds the proportional bound")]
    fn partial_seizure_above_bound_rejected() {
        run(Case {
            seize: 626,
            cover: 1_000,
            ..unsafe_case()
        });
    }

    #[test]
    #[should_panic(expected = "Position is not liquidatable: it is collateralized")]
    fn healthy_position_rejected() {
        run(Case {
            collateral: 2_000,
            debt: 1_000,
            vault_balance: 2_000,
            seize: 2_000,
            cover: 1_000,
            ..unsafe_case()
        });
    }

    #[test]
    #[should_panic(expected = "Caller program is not the liquidation engine")]
    fn non_engine_caller_rejected() {
        run(Case {
            caller: IMPOSTOR,
            ..unsafe_case()
        });
    }

    #[test]
    #[should_panic(expected = "Caller program is not the liquidation engine")]
    fn direct_top_level_call_rejected() {
        run(Case {
            caller: DEFAULT_PROGRAM_ID,
            ..unsafe_case()
        });
    }

    #[test]
    #[should_panic(expected = "Engine authority authorization is missing")]
    fn unauthorised_authority_rejected() {
        run(Case {
            authorized: false,
            ..unsafe_case()
        });
    }

    #[test]
    #[should_panic(expected = "collateral_to_seize out of bounds")]
    fn seize_more_than_position_rejected() {
        run(Case {
            seize: 1_001,
            vault_balance: 1_001,
            ..unsafe_case()
        });
    }

    #[test]
    #[should_panic(expected = "normalized_debt_to_cover out of bounds")]
    fn cover_more_than_debt_rejected() {
        run(Case {
            cover: 2_001,
            ..unsafe_case()
        });
    }

    #[test]
    #[should_panic(expected = "normalized_debt_to_cover out of bounds")]
    fn zero_cover_rejected() {
        run(Case {
            cover: 0,
            ..unsafe_case()
        });
    }

    #[test]
    #[should_panic(expected = "Vault balance is below the position collateral")]
    fn vault_below_collateral_rejected() {
        run(Case {
            vault_balance: 999,
            ..unsafe_case()
        });
    }

    #[test]
    #[should_panic(expected = "Position has no debt")]
    fn zero_debt_rejected() {
        run(Case {
            debt: 0,
            cover: 0,
            ..unsafe_case()
        });
    }

    #[test]
    #[should_panic(expected = "Market price oracle observation is stale")]
    fn stale_oracle_rejected() {
        run(Case {
            oracle_ts: NOW - TEST_MAXIMUM_ORACLE_PRICE_AGE_MILLISECONDS - 1,
            ..unsafe_case()
        });
    }

    #[test]
    #[should_panic(
        expected = "Custody holding does not match the protocol's collateral definition"
    )]
    fn custody_of_wrong_definition_rejected() {
        liquidate_position(
            engine_authority(true),
            engine_state(),
            position(1_000, 2_000),
            holding(vault_id(), collateral_definition_id(), 1_000),
            holding(custody_id(), stablecoin_definition_id(), 0),
            accumulator_account(ACCUMULATOR_ANCHOR, NOW),
            redemption_price_state_account(NOW),
            oracle_account(NOW, REDEMPTION_PRICE_ANCHOR),
            protocol_parameters_account(ParameterOverrides::default()),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
            ENGINE,
            1_000,
            2_000,
        );
    }

    fn run_mutated(f: impl FnOnce(&mut Accounts)) {
        let c = unsafe_case();
        let mut a = Accounts::for_case(&c);
        f(&mut a);
        run_with(&c, a);
    }

    #[test]
    fn full_collateral_with_partial_debt_moves_only_the_seized_amount() {
        // bound = min(1000, ceil(1000 x 1600 / 2000 x 1.25)) = 1000, so all collateral but not all debt may go;
        // that is a partial seizure: the donation stays in the vault and the debt remainder stays on the position.
        let (post, calls) = run(Case {
            seize: 1_000,
            cover: 1_600,
            vault_balance: 1_010,
            ..unsafe_case()
        });
        let p = position_after(&post);
        assert_eq!((p.collateral_amount, p.normalized_debt_amount), (0, 400));
        assert!(transferred(&calls, 1_000));
    }

    #[test]
    #[should_panic(expected = "collateral_to_seize out of bounds")]
    fn zero_seize_rejected() {
        run(Case {
            seize: 0,
            ..unsafe_case()
        });
    }

    #[test]
    #[should_panic(expected = "Market price oracle observation is dated in the future")]
    fn future_dated_oracle_rejected() {
        run(Case {
            oracle_ts: NOW + 1,
            ..unsafe_case()
        });
    }

    #[test]
    #[should_panic(expected = "Position is not owned by this stablecoin program")]
    fn foreign_owned_position_rejected() {
        run_mutated(|a| a.position.account.program_owner = IMPOSTOR);
    }

    #[test]
    #[should_panic(expected = "Position account ID does not match expected derivation")]
    fn position_at_wrong_address_rejected() {
        run_mutated(|a| a.position.account_id = AccountId::new([0x77; 32]));
    }

    #[test]
    #[should_panic(expected = "Position vault account ID does not match expected derivation")]
    fn vault_at_wrong_address_rejected() {
        run_mutated(|a| a.vault.account_id = custody_id());
    }

    #[test]
    #[should_panic(expected = "Position vault_account_id does not match the vault account")]
    fn position_pointing_at_another_vault_rejected() {
        run_mutated(|a| {
            let mut stored = Position::try_from(&a.position.account.data).unwrap();
            stored.vault_account_id = custody_id();
            a.position.account.data = Data::from(&stored);
        });
    }

    #[test]
    #[should_panic(expected = "Vault holding does not match the protocol's collateral definition")]
    fn vault_of_wrong_definition_rejected() {
        run_mutated(|a| a.vault = holding(vault_id(), stablecoin_definition_id(), 1_000));
    }

    #[test]
    #[should_panic(expected = "Custody holding must be initialized")]
    fn uninitialised_custody_rejected() {
        run_mutated(|a| a.custody = uninitialized(custody_id()));
    }

    #[test]
    #[should_panic(expected = "Custody holding must be owned by the vault's Token Program")]
    fn custody_of_another_program_rejected() {
        run_mutated(|a| a.custody.account.program_owner = IMPOSTOR);
    }

    #[test]
    #[should_panic(expected = "Redemption price projected to zero")]
    fn zero_redemption_price_rejected() {
        run_mutated(|a| {
            let mut stored = RedemptionPriceState::try_from(&a.redemption.account.data).unwrap();
            stored.redemption_price_at_last_update = 0;
            a.redemption.account.data = Data::from(&stored);
        });
    }

    #[test]
    fn proportional_bound_is_capped_at_collateral_and_rounds_up() {
        assert_eq!(proportional_seize_bound(1_000, 2_000, 2_000), 1_000); // 1250 capped
        assert_eq!(proportional_seize_bound(1_000, 1_000, 2_000), 625);
        assert_eq!(proportional_seize_bound(3, 1, 7), 1); // ceil(3 × 1 × 12500 / 70000) = ceil(0.535…)
        assert_eq!(
            proportional_seize_bound(u128::MAX, u128::MAX, u128::MAX),
            u128::MAX
        );
    }
}
