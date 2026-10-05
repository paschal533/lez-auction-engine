#![cfg_attr(not(test), no_main)]

use spel_framework::prelude::*;
use spike_core::{named_pda_seed, CalleeInstruction, ENGINE_AUTH_SEED};

#[cfg(not(test))]
risc0_zkvm::guest::entry!(main);

#[lez_program(instruction = "spike_core::ImpostorInstruction")]
mod impostor {
    #[allow(unused_imports)]
    use super::*;

    /// Tries to pass `callee.guarded_*` (see `ImpostorInstruction::Attack`).
    #[instruction]
    pub fn attack(
        #[account(signer)] user: AccountWithMetadata,
        config: AccountWithMetadata,
        authority: AccountWithMetadata,
        callee_program_id: ProgramId,
        variant: u8,
        mode: u8,
    ) -> SpelResult {
        let mut forged = authority.clone();
        forged.is_authorized = true;
        let seeds = if mode == 0 {
            vec![]
        } else {
            vec![named_pda_seed(ENGINE_AUTH_SEED)]
        };
        let call = ChainedCall::new(
            callee_program_id,
            vec![config.clone(), forged, user.clone()],
            &CalleeInstruction::from_variant(variant),
        )
        .with_pda_seeds(seeds);
        Ok(spel_framework::SpelOutput::execute(
            vec![user.account, config.account, authority.account],
            vec![call],
        ))
    }

    /// Re-enter `callee.guarded_*` with the authorisation inherited from the engine's chained call.
    #[instruction]
    pub fn launder(
        config: AccountWithMetadata,
        authority: AccountWithMetadata,
        #[account(signer)] user: AccountWithMetadata,
        callee_program_id: ProgramId,
        variant: u8,
    ) -> SpelResult {
        let call = ChainedCall::new(
            callee_program_id,
            vec![config.clone(), authority.clone(), user.clone()],
            &CalleeInstruction::from_variant(variant),
        );
        Ok(SpelOutput::execute(vec![config, authority, user], vec![call]))
    }
}
