#![cfg_attr(not(test), no_main)]

use spel_framework::prelude::*;
use spike_core::CalleeConfig;

#[cfg(not(test))]
risc0_zkvm::guest::entry!(main);

fn load_config(config: &AccountWithMetadata) -> CalleeConfig {
    CalleeConfig::try_from(&config.account.data).expect("callee config must decode")
}

/// Variant A: the program that invoked us must be the stored engine program.
fn guard_a(ctx: &ProgramContext, cfg: &CalleeConfig) {
    assert_eq!(
        ctx.caller_program_id, cfg.engine_program_id,
        "guard A: caller program is not the engine program"
    );
}

/// Variant B: the stored engine authority PDA must be present and authorised in this call.
fn guard_b(authority: &AccountWithMetadata, cfg: &CalleeConfig) {
    assert_eq!(
        authority.account_id, cfg.engine_authority,
        "guard B: authority account is not the stored engine authority"
    );
    assert!(
        authority.is_authorized,
        "guard B: engine authority is not authorised in this call"
    );
}

#[lez_program(instruction = "spike_core::CalleeInstruction")]
mod callee {
    #[allow(unused_imports)]
    use super::*;

    /// S2 variant A.
    #[instruction]
    pub fn guarded_a(
        ctx: ProgramContext,
        #[account(owner = self_program_id, pda = literal("CALLEE_CFG"))] config: AccountWithMetadata,
        authority: AccountWithMetadata,
        bystander: AccountWithMetadata,
    ) -> SpelResult {
        guard_a(&ctx, &load_config(&config));
        Ok(spel_framework::SpelOutput::execute(
            vec![config.account, authority.account, bystander.account],
            vec![],
        ))
    }

    /// S2 variant B.
    #[instruction]
    pub fn guarded_b(
        #[account(owner = self_program_id, pda = literal("CALLEE_CFG"))] config: AccountWithMetadata,
        authority: AccountWithMetadata,
        bystander: AccountWithMetadata,
    ) -> SpelResult {
        guard_b(&authority, &load_config(&config));
        Ok(spel_framework::SpelOutput::execute(
            vec![config.account, authority.account, bystander.account],
            vec![],
        ))
    }

    /// S2 variant C (A and B).
    #[instruction]
    pub fn guarded_c(
        ctx: ProgramContext,
        #[account(owner = self_program_id, pda = literal("CALLEE_CFG"))] config: AccountWithMetadata,
        authority: AccountWithMetadata,
        bystander: AccountWithMetadata,
    ) -> SpelResult {
        let cfg = load_config(&config);
        guard_a(&ctx, &cfg);
        guard_b(&authority, &cfg);
        Ok(spel_framework::SpelOutput::execute(
            vec![config.account, authority.account, bystander.account],
            vec![],
        ))
    }
}
