#![cfg_attr(not(test), no_main)]

use spel_framework::prelude::*;

#[cfg(not(test))]
risc0_zkvm::guest::entry!(main);

/// mode 0: forge the flag on `authority` and attach no seeds; mode 1: attach `seed`, which legitimately
/// authorises the impostor's OWN PDA `for_public_pda(IMPOSTOR_ID, seed)`.
fn attack_authority(
    authority: &AccountWithMetadata,
    mode: u8,
    seed: [u8; 32],
) -> (AccountWithMetadata, Vec<PdaSeed>) {
    let mut claimed = authority.clone();
    claimed.is_authorized = true;
    let seeds = if mode == 1 {
        vec![PdaSeed::new(seed)]
    } else {
        vec![]
    };
    (claimed, seeds)
}

fn freeze_call(
    host_program_id: ProgramId,
    authority: AccountWithMetadata,
    les: &AccountWithMetadata,
    protocol_parameters: &AccountWithMetadata,
    seeds: Vec<PdaSeed>,
) -> ChainedCall {
    ChainedCall::new(
        host_program_id,
        vec![authority, les.clone(), protocol_parameters.clone()],
        &stablecoin_core::Instruction::EngineFreezeNewDebt,
    )
    .with_pda_seeds(seeds)
}

fn liquidate_call(
    host_program_id: ProgramId,
    accounts: Vec<AccountWithMetadata>,
    collateral_to_seize: u128,
    normalized_debt_to_cover: u128,
    seeds: Vec<PdaSeed>,
) -> ChainedCall {
    ChainedCall::new(
        host_program_id,
        accounts,
        &stablecoin_core::Instruction::LiquidatePosition {
            collateral_to_seize,
            normalized_debt_to_cover,
        },
    )
    .with_pda_seeds(seeds)
}

/// Echoes every pre-state unchanged (the impostor owns none of them).
fn echo(accounts: Vec<AccountWithMetadata>) -> Vec<AccountPostState> {
    accounts
        .into_iter()
        .map(|a| AccountPostState::new(a.account))
        .collect()
}

#[lez_program(instruction = "test_programs_core::ImpostorInstruction")]
mod impostor {
    #[allow(unused_imports)]
    use super::*;

    #[instruction]
    pub fn attack_freeze(
        authority: AccountWithMetadata,
        liquidation_engine_state: AccountWithMetadata,
        #[account(mut)] protocol_parameters: AccountWithMetadata,
        host_program_id: ProgramId,
        mode: u8,
        seed: [u8; 32],
    ) -> SpelResult {
        let (claimed, seeds) = attack_authority(&authority, mode, seed);
        let call = freeze_call(
            host_program_id,
            claimed,
            &liquidation_engine_state,
            &protocol_parameters,
            seeds,
        );
        Ok(spel_framework::SpelOutput::execute(
            echo(vec![
                authority,
                liquidation_engine_state,
                protocol_parameters,
            ]),
            vec![call],
        ))
    }

    #[instruction]
    #[allow(clippy::too_many_arguments)]
    pub fn attack_liquidate(
        authority: AccountWithMetadata,
        liquidation_engine_state: AccountWithMetadata,
        #[account(mut)] position: AccountWithMetadata,
        #[account(mut)] vault: AccountWithMetadata,
        #[account(mut)] custody: AccountWithMetadata,
        stability_fee_accumulator: AccountWithMetadata,
        redemption_price_state: AccountWithMetadata,
        market_price_oracle: AccountWithMetadata,
        protocol_parameters: AccountWithMetadata,
        clock: AccountWithMetadata,
        host_program_id: ProgramId,
        mode: u8,
        seed: [u8; 32],
        collateral_to_seize: u128,
        normalized_debt_to_cover: u128,
    ) -> SpelResult {
        let (claimed, seeds) = attack_authority(&authority, mode, seed);
        let call = liquidate_call(
            host_program_id,
            vec![
                claimed,
                liquidation_engine_state.clone(),
                position.clone(),
                vault.clone(),
                custody.clone(),
                stability_fee_accumulator.clone(),
                redemption_price_state.clone(),
                market_price_oracle.clone(),
                protocol_parameters.clone(),
                clock.clone(),
            ],
            collateral_to_seize,
            normalized_debt_to_cover,
            seeds,
        );
        Ok(spel_framework::SpelOutput::execute(
            echo(vec![
                authority,
                liquidation_engine_state,
                position,
                vault,
                custody,
                stability_fee_accumulator,
                redemption_price_state,
                market_price_oracle,
                protocol_parameters,
                clock,
            ]),
            vec![call],
        ))
    }

    #[instruction]
    #[allow(clippy::too_many_arguments)]
    pub fn attack_both(
        authority: AccountWithMetadata,
        liquidation_engine_state: AccountWithMetadata,
        #[account(mut)] position: AccountWithMetadata,
        #[account(mut)] vault: AccountWithMetadata,
        #[account(mut)] custody: AccountWithMetadata,
        stability_fee_accumulator: AccountWithMetadata,
        redemption_price_state: AccountWithMetadata,
        market_price_oracle: AccountWithMetadata,
        #[account(mut)] protocol_parameters: AccountWithMetadata,
        clock: AccountWithMetadata,
        host_program_id: ProgramId,
        mode: u8,
        seed: [u8; 32],
        collateral_to_seize: u128,
        normalized_debt_to_cover: u128,
    ) -> SpelResult {
        let (claimed, seeds) = attack_authority(&authority, mode, seed);
        let freeze = freeze_call(
            host_program_id,
            claimed.clone(),
            &liquidation_engine_state,
            &protocol_parameters,
            seeds.clone(),
        );
        let liquidate = liquidate_call(
            host_program_id,
            vec![
                claimed,
                liquidation_engine_state.clone(),
                position.clone(),
                vault.clone(),
                custody.clone(),
                stability_fee_accumulator.clone(),
                redemption_price_state.clone(),
                market_price_oracle.clone(),
                protocol_parameters.clone(),
                clock.clone(),
            ],
            collateral_to_seize,
            normalized_debt_to_cover,
            seeds,
        );
        Ok(spel_framework::SpelOutput::execute(
            echo(vec![
                authority,
                liquidation_engine_state,
                position,
                vault,
                custody,
                stability_fee_accumulator,
                redemption_price_state,
                market_price_oracle,
                protocol_parameters,
                clock,
            ]),
            vec![freeze, liquidate],
        ))
    }

    #[instruction]
    pub fn attack_set_engine(
        #[account(signer)] admin: AccountWithMetadata,
        protocol_parameters: AccountWithMetadata,
        #[account(mut)] liquidation_engine_state: AccountWithMetadata,
        host_program_id: ProgramId,
        engine_program_id: ProgramId,
        engine_authority_id: AccountId,
    ) -> SpelResult {
        // The admin's top-level signature is legitimately forwarded (the flag is genuinely true).
        let call = ChainedCall::new(
            host_program_id,
            vec![
                admin.clone(),
                protocol_parameters.clone(),
                liquidation_engine_state.clone(),
            ],
            &stablecoin_core::Instruction::SetLiquidationEngine {
                engine_program_id,
                engine_authority_id,
            },
        );
        Ok(spel_framework::SpelOutput::execute(
            echo(vec![admin, protocol_parameters, liquidation_engine_state]),
            vec![call],
        ))
    }

    /// Laundering sink: forwards the account flags exactly as received (nothing forged, no seeds).
    #[instruction]
    pub fn forward_freeze(
        authority: AccountWithMetadata,
        liquidation_engine_state: AccountWithMetadata,
        #[account(mut)] protocol_parameters: AccountWithMetadata,
        host_program_id: ProgramId,
    ) -> SpelResult {
        let call = freeze_call(
            host_program_id,
            authority.clone(),
            &liquidation_engine_state,
            &protocol_parameters,
            vec![],
        );
        Ok(spel_framework::SpelOutput::execute(
            echo(vec![
                authority,
                liquidation_engine_state,
                protocol_parameters,
            ]),
            vec![call],
        ))
    }

    #[instruction]
    #[allow(clippy::too_many_arguments)]
    pub fn forward_liquidate(
        authority: AccountWithMetadata,
        liquidation_engine_state: AccountWithMetadata,
        #[account(mut)] position: AccountWithMetadata,
        #[account(mut)] vault: AccountWithMetadata,
        #[account(mut)] custody: AccountWithMetadata,
        stability_fee_accumulator: AccountWithMetadata,
        redemption_price_state: AccountWithMetadata,
        market_price_oracle: AccountWithMetadata,
        protocol_parameters: AccountWithMetadata,
        clock: AccountWithMetadata,
        host_program_id: ProgramId,
        collateral_to_seize: u128,
        normalized_debt_to_cover: u128,
    ) -> SpelResult {
        let accounts = vec![
            authority,
            liquidation_engine_state,
            position,
            vault,
            custody,
            stability_fee_accumulator,
            redemption_price_state,
            market_price_oracle,
            protocol_parameters,
            clock,
        ];
        let call = liquidate_call(
            host_program_id,
            accounts.clone(),
            collateral_to_seize,
            normalized_debt_to_cover,
            vec![],
        );
        Ok(spel_framework::SpelOutput::execute(
            echo(accounts),
            vec![call],
        ))
    }
}
