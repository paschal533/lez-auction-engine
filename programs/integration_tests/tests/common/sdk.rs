//! The SDK against the real in-process guests (Plan 3a Task 8): `WorldChain` reads the V03State, and
//! `run_plan` signs an SDK plan with the test keys and applies it like any other transaction.
//!
//! Clock: `World::submit` and `run_plan` seed `CLOCK_01` with `World.now` before executing, so a
//! transaction executes under exactly `World.now` (on a real chain: the previous block's timestamp).
//! `World::advance` moves `now` and re-seeds the clock; quotes in these tests are taken at `w.now`,
//! which equals `auction_sdk::chain::clock_ms(&WorldChain(&w))`.
use super::*;
use auction_sdk::{
    chain::{BlockSummary, Chain},
    deployment::Deployment,
    error::{SdkError, SdkResult},
    plan::Plan,
    verify::{verify_config, VerifiedConfig},
};
use lee::public_transaction::{Message, WitnessSet};

pub struct WorldChain<'a>(pub &'a World);

impl Chain for WorldChain<'_> {
    fn account(&self, id: &AccountId) -> SdkResult<Account> {
        Ok(self.0.state.get_account_by_id(*id))
    }
    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<Nonce>> {
        Ok(ids
            .iter()
            .map(|i| self.0.state.get_account_by_id(*i).nonce)
            .collect())
    }
    fn last_block_id(&self) -> SdkResult<u64> {
        Ok(self.0.block)
    }
    fn blocks(&self, _from: u64, _to: u64) -> SdkResult<Vec<BlockSummary>> {
        Err(SdkError::Chain(
            "the in-process World keeps no blocks".into(),
        ))
    }
    fn send(&self, _tx: &PublicTransaction) -> SdkResult<[u8; 32]> {
        Err(SdkError::Chain("use World::run_plan".into()))
    }
}

pub fn sdk_deployment() -> Deployment {
    Deployment {
        network: "in-process".into(),
        sequencer_url: String::new(),
        engine_program: engine_program(),
        host_program: host_program(),
        token_program: token_program(),
        twap_oracle_program: None,
        engine_admin: Some(id(k::ENGINE_ADMIN)),
        config: Some(config()),
    }
}

impl World {
    pub fn sdk_config(&self) -> VerifiedConfig {
        verify_config(&WorldChain(self), &sdk_deployment(), &config())
            .expect("the World config verifies")
    }

    /// Signs `plan` with the test keys of `signers` (== `plan.signing_requirements`) and applies it.
    pub fn run_plan(&mut self, plan: &Plan, signers: &[u8]) -> Result<(), LeeError> {
        assert_eq!(
            signers.iter().map(|n| id(*n)).collect::<Vec<_>>(),
            plan.signing_requirements,
            "signers follow the plan"
        );
        self.seed_clock();
        let keys: Vec<PrivateKey> = signers.iter().map(|n| key(*n)).collect();
        let refs: Vec<&PrivateKey> = keys.iter().collect();
        let nonces = plan
            .signing_requirements
            .iter()
            .map(|a| self.state.get_account_by_id(*a).nonce)
            .collect();
        let message = Message::new_preserialized(
            plan.program_id,
            plan.account_ids.clone(),
            nonces,
            plan.instruction_words.clone(),
        );
        let witness = WitnessSet::for_message(&message, &refs);
        let result = self
            .state
            .transition_from_public_transaction(
                &PublicTransaction::new(message, witness),
                self.block,
                self.now,
            )
            .map(|_| ());
        if result.is_ok() {
            self.block += 1;
        }
        result
    }
}
