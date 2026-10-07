//! Replays a transaction's whole chain with the risc0 executor (S1 step 6 / S2 method) and prints one
//! line per program execution. Executor cycles of natively built guests, dev mode: not proving cost.

use std::collections::VecDeque;

use lee::{program::Program, PrivateKey, PublicKey, V03State};
use lee_core::{
    account::{AccountId, AccountWithMetadata},
    program::{ChainedCall, ProgramId, ProgramOutput},
};
use risc0_zkvm::{default_executor, ExecutorEnv};
use serde::Serialize;

pub struct Measured {
    pub per_exec: Vec<(&'static str, u64)>,
    pub total: u64,
}

fn elf_for(program_id: ProgramId) -> (&'static str, &'static [u8]) {
    if program_id == token_methods::TOKEN_ID {
        ("token", token_methods::TOKEN_ELF)
    } else if program_id == stablecoin_methods::STABLECOIN_ID {
        ("stablecoin", stablecoin_methods::STABLECOIN_ELF)
    } else if program_id == auction_engine_methods::AUCTION_ENGINE_ID {
        ("auction_engine", auction_engine_methods::AUCTION_ENGINE_ELF)
    } else {
        panic!("unknown program in chain")
    }
}

fn run_one(call: &ChainedCall, caller: Option<ProgramId>) -> (&'static str, ProgramOutput, u64) {
    let (name, elf) = elf_for(call.program_id);
    let mut builder = ExecutorEnv::builder();
    builder.session_limit(Some(32 * 1024 * 1024));
    builder.write(&call.program_id).unwrap();
    builder.write(&caller).unwrap();
    builder.write(&call.pre_states).unwrap();
    builder.write(&call.instruction_data).unwrap();
    let info = default_executor()
        .execute(builder.build().unwrap(), elf)
        .expect("guest executes");
    let out: ProgramOutput = info.journal.decode().unwrap();
    (name, out, info.cycles())
}

pub fn measure<I: Serialize>(
    label: &str,
    state: &V03State,
    program_id: ProgramId,
    accounts: &[AccountId],
    signers: &[&PrivateKey],
    instruction: &I,
) -> Measured {
    let signer_ids: Vec<AccountId> = signers
        .iter()
        .map(|k| AccountId::from(&PublicKey::new_from_private_key(k)))
        .collect();
    let first = ChainedCall {
        program_id,
        instruction_data: Program::serialize_instruction(instruction).unwrap(),
        pre_states: accounts
            .iter()
            .map(|id| {
                AccountWithMetadata::new(state.get_account_by_id(*id), signer_ids.contains(id), *id)
            })
            .collect(),
        pda_seeds: vec![],
    };
    let mut queue = VecDeque::from([(first, None, 0usize)]);
    let (mut per_exec, mut total) = (Vec::new(), 0u64);
    while let Some((call, caller, depth)) = queue.pop_front() {
        let (name, out, cycles) = run_one(&call, caller);
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
