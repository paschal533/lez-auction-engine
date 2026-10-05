//! Shared fixtures for the S2 / S3 / S7 in-process `V03State` tests.
#![allow(dead_code)]

use std::collections::VecDeque;

use lee::{
    error::LeeError,
    program::Program,
    program_deployment_transaction::{self, ProgramDeploymentTransaction},
    public_transaction, PrivateKey, PublicKey, PublicTransaction, V03State,
};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{ChainedCall, ProgramId, ProgramOutput},
};
use risc0_zkvm::{default_executor, ExecutorEnv};
use serde::Serialize;
use spike_core::{
    named_pda, CalleeConfig, ENGINE_AUTH_SEED, CALLEE_CFG_SEED,
};
use spike_methods::{
    CALLEE_ELF, CALLEE_ID, CALLER_ELF, CALLER_ID, IMPOSTOR_ELF, IMPOSTOR_ID,
};

/// Maximum program executions per public transaction in lee v0.2.4 (S1: 1 top-level + 10 chained).
pub const MAX_EXECUTIONS: usize = 11;

pub fn callee_id() -> ProgramId {
    CALLEE_ID
}
pub fn caller_id() -> ProgramId {
    CALLER_ID
}
pub fn impostor_id() -> ProgramId {
    IMPOSTOR_ID
}
pub fn token_id() -> ProgramId {
    token_methods::TOKEN_ID
}

pub fn key(n: u8) -> PrivateKey {
    PrivateKey::try_new([n; 32]).expect("valid private key")
}

pub fn id_of(k: &PrivateKey) -> AccountId {
    AccountId::from(&PublicKey::new_from_private_key(k))
}

/// A user account owned by a placeholder non-default program (as the lez-programs fixtures do),
/// so its unchanged post-state survives the nonce bump of a second transaction.
pub fn user_account() -> Account {
    Account {
        program_owner: [7u32; 8],
        ..Account::default()
    }
}

pub fn engine_authority() -> AccountId {
    named_pda(CALLER_ID, ENGINE_AUTH_SEED)
}

pub fn callee_config_id() -> AccountId {
    named_pda(CALLEE_ID, CALLEE_CFG_SEED)
}

pub fn callee_config_account(cfg: &CalleeConfig) -> Account {
    Account {
        program_owner: CALLEE_ID,
        data: Data::from(cfg),
        ..Account::default()
    }
}

/// The config the callee trusts: engine = `caller`, authority = `caller`'s `ENGINE_AUTH` PDA.
pub fn engine_config() -> CalleeConfig {
    CalleeConfig {
        engine_program_id: CALLER_ID,
        engine_authority: engine_authority(),
    }
}

pub fn deploy(state: &mut V03State, elf: &[u8]) {
    state
        .transition_from_program_deployment_transaction(&ProgramDeploymentTransaction::new(
            program_deployment_transaction::Message::new(elf.to_vec()),
        ))
        .expect("program deployment must succeed");
}

/// A fresh state with callee, caller, impostor and token deployed and the callee config inserted.
pub fn base_state() -> V03State {
    let mut state = V03State::new();
    deploy(&mut state, CALLEE_ELF);
    deploy(&mut state, CALLER_ELF);
    deploy(&mut state, IMPOSTOR_ELF);
    deploy(&mut state, token_methods::TOKEN_ELF);
    state.force_insert_account(callee_config_id(), callee_config_account(&engine_config()));
    for n in [31u8, 32, 33] {
        state.force_insert_account(id_of(&key(n)), user_account());
    }
    state
}

fn nonce_of(state: &V03State, id: AccountId) -> lee_core::account::Nonce {
    state.get_account_by_id(id).nonce
}

pub fn build_tx<I: Serialize>(
    state: &V03State,
    program_id: ProgramId,
    accounts: Vec<AccountId>,
    signers: &[&PrivateKey],
    instruction: I,
) -> PublicTransaction {
    let nonces = signers.iter().map(|k| nonce_of(state, id_of(k))).collect();
    let message = public_transaction::Message::try_new(program_id, accounts, nonces, instruction)
        .expect("message is valid");
    let witness_set = public_transaction::WitnessSet::for_message(&message, signers);
    PublicTransaction::new(message, witness_set)
}

pub fn submit<I: Serialize>(
    state: &mut V03State,
    program_id: ProgramId,
    accounts: Vec<AccountId>,
    signers: &[&PrivateKey],
    instruction: I,
) -> Result<(), LeeError> {
    let tx = build_tx(state, program_id, accounts, signers, instruction);
    state.transition_from_public_transaction(&tx, 0, 0).map(|_| ())
}

/// Formats an error the way the tests match on it.
pub fn err_text(res: Result<(), LeeError>) -> String {
    format!("{:?}", res.expect_err("transaction was expected to fail"))
}

// ---------------------------------------------------------------------------
// Cycle measurement (same method as spike S1 step 6: replay the whole chain, breadth-first,
// with the risc0 executor exactly as `Program::execute` feeds a guest).
// ---------------------------------------------------------------------------

fn elf_for(program_id: ProgramId) -> (&'static str, &'static [u8]) {
    if program_id == CALLEE_ID {
        ("callee", CALLEE_ELF)
    } else if program_id == CALLER_ID {
        ("caller", CALLER_ELF)
    } else if program_id == IMPOSTOR_ID {
        ("impostor", IMPOSTOR_ELF)
    } else if program_id == token_methods::TOKEN_ID {
        ("token", token_methods::TOKEN_ELF)
    } else {
        panic!("unknown program in chain");
    }
}

fn run_one(
    call: &ChainedCall,
    caller: Option<ProgramId>,
) -> (&'static str, ProgramOutput, u64) {
    let (name, elf) = elf_for(call.program_id);
    let mut builder = ExecutorEnv::builder();
    builder.session_limit(Some(1024 * 1024 * 32));
    builder.write(&call.program_id).unwrap();
    builder.write(&caller).unwrap();
    builder.write(&call.pre_states).unwrap();
    builder.write(&call.instruction_data).unwrap();
    let env = builder.build().unwrap();
    let info = default_executor().execute(env, elf).expect("guest executes");
    let out: ProgramOutput = info.journal.decode().unwrap();
    (name, out, info.cycles())
}

#[derive(Debug, Clone)]
pub struct Measured {
    pub per_exec: Vec<(&'static str, u64)>,
    pub total: u64,
}

impl Measured {
    pub fn cycles_of(&self, program: &str) -> u64 {
        self.per_exec
            .iter()
            .filter(|(n, _)| *n == program)
            .map(|(_, c)| *c)
            .sum()
    }
}

/// Builds the top-level call exactly as lee does (pre-states from the live state, `is_authorized`
/// = account signs the transaction) and replays the whole chain, printing one line per execution.
pub fn measure<I: Serialize>(
    label: &str,
    state: &V03State,
    program_id: ProgramId,
    accounts: &[AccountId],
    signers: &[&PrivateKey],
    instruction: &I,
) -> Measured {
    let signer_ids: Vec<AccountId> = signers.iter().map(|k| id_of(k)).collect();
    let first = ChainedCall {
        program_id,
        instruction_data: Program::serialize_instruction(instruction).unwrap(),
        pre_states: accounts
            .iter()
            .map(|id| {
                AccountWithMetadata::new(
                    state.get_account_by_id(*id),
                    signer_ids.contains(id),
                    *id,
                )
            })
            .collect(),
        pda_seeds: vec![],
    };
    let mut queue = VecDeque::from([(first, None, 0usize)]);
    let mut per_exec = Vec::new();
    let mut total = 0u64;
    while let Some((call, caller, depth)) = queue.pop_front() {
        let (name, out, cycles) = run_one(&call, caller);
        assert!(per_exec.len() < MAX_EXECUTIONS, "chain too long");
        println!(
            "CYCLES[{label}] exec#{} depth={depth} program={name} cycles={cycles}",
            per_exec.len() + 1
        );
        per_exec.push((name, cycles));
        total += cycles;
        for c in out.chained_calls {
            queue.push_back((c, Some(call.program_id), depth + 1));
        }
    }
    println!(
        "CYCLES[{label}] TOTAL executions={} cycles={total}",
        per_exec.len()
    );
    Measured { per_exec, total }
}
