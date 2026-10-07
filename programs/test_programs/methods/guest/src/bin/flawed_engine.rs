#![cfg_attr(not(test), no_main)]

use spel_framework::prelude::*;
use test_programs_core::ImpostorInstruction;

#[cfg(not(test))]
risc0_zkvm::guest::entry!(main);

fn echo(accounts: Vec<AccountWithMetadata>) -> Vec<AccountPostState> {
    accounts
        .into_iter()
        .map(|a| AccountPostState::new(a.account))
        .collect()
}

/// FLAW (test only; S2 `call_untrusted`): authorises its PDA for a caller-chosen program. The real
/// engine never does this (ADR-019 invariant (i)); the tests bind the host to this program to prove that
/// even then the immediate-caller check stops the laundered call.
#[lez_program(instruction = "test_programs_core::FlawedEngineInstruction")]
mod flawed_engine {
    #[allow(unused_imports)]
    use super::*;

    #[instruction]
    pub fn launder_freeze(
        authority: AccountWithMetadata,
        liquidation_engine_state: AccountWithMetadata,
        #[account(mut)] protocol_parameters: AccountWithMetadata,
        target_program_id: ProgramId,
        host_program_id: ProgramId,
        seed: [u8; 32],
    ) -> SpelResult {
        let mut authorised = authority.clone();
        authorised.is_authorized = true;
        let call = ChainedCall::new(
            target_program_id,
            vec![
                authorised,
                liquidation_engine_state.clone(),
                protocol_parameters.clone(),
            ],
            &ImpostorInstruction::ForwardFreeze { host_program_id },
        )
        .with_pda_seeds(vec![PdaSeed::new(seed)]);
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
    pub fn launder_liquidate(
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
        target_program_id: ProgramId,
        host_program_id: ProgramId,
        seed: [u8; 32],
        collateral_to_seize: u128,
        normalized_debt_to_cover: u128,
    ) -> SpelResult {
        let mut authorised = authority.clone();
        authorised.is_authorized = true;
        let call = ChainedCall::new(
            target_program_id,
            vec![
                authorised,
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
            &ImpostorInstruction::ForwardLiquidate {
                host_program_id,
                collateral_to_seize,
                normalized_debt_to_cover,
            },
        )
        .with_pda_seeds(vec![PdaSeed::new(seed)]);
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
}
