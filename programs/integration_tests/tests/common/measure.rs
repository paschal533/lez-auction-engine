//! Replays a transaction's whole chain with the risc0 executor (S1 step 6 / S2 method) and prints one
//! line per program execution: executor cycles of natively built guests in dev mode, not proving
//! cost. With `AUCTION_PROVE=1` (and `RISC0_DEV_MODE=0`) every execution is proved instead
//! (`prove.rs`, ADR-045) and a `PROOF[...]` line is printed per execution.

use std::collections::VecDeque;
use std::io::Write as _;

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
    /// Empty unless `AUCTION_PROVE=1`.
    pub proofs: Vec<super::prove::ProvedExec>,
}

pub fn elf_for(program_id: ProgramId) -> (&'static str, &'static [u8]) {
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

/// The guest input of one call, as the LEZ executor feeds it (program id, caller, pre-states,
/// instruction). The one source of the input format for both executing and proving; `po2` sets the
/// segment limit when proving.
pub fn guest_env(
    call: &ChainedCall,
    caller: Option<ProgramId>,
    po2: Option<u32>,
) -> ExecutorEnv<'static> {
    let mut builder = ExecutorEnv::builder();
    builder.session_limit(Some(32 * 1024 * 1024));
    if let Some(po2) = po2 {
        builder.segment_limit_po2(po2);
    }
    builder.write(&call.program_id).unwrap();
    builder.write(&caller).unwrap();
    builder.write(&call.pre_states).unwrap();
    builder.write(&call.instruction_data).unwrap();
    builder.build().unwrap()
}

/// Appends `v` as one JSON line to the file named by the environment variable `var`; a no-op when
/// the variable is unset (evidence reports for CI).
pub fn append_json_line(var: &str, v: serde_json::Value) {
    let Ok(path) = std::env::var(var) else {
        return;
    };
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .unwrap_or_else(|e| panic!("{var} {path}: {e}"));
    writeln!(f, "{v}").unwrap_or_else(|e| panic!("{var} {path}: {e}"));
}

fn execute_one(
    call: &ChainedCall,
    caller: Option<ProgramId>,
) -> (&'static str, ProgramOutput, u64) {
    let (name, elf) = elf_for(call.program_id);
    let info = default_executor()
        .execute(guest_env(call, caller, None), elf)
        .expect("guest executes");
    let out: ProgramOutput = info.journal.decode().unwrap();
    (name, out, info.cycles())
}

/// The top-level call of a transaction, as the sequencer builds it from the message.
pub fn first_call<I: Serialize>(
    state: &V03State,
    program_id: ProgramId,
    accounts: &[AccountId],
    signers: &[&PrivateKey],
    instruction: &I,
) -> ChainedCall {
    let signer_ids: Vec<AccountId> = signers
        .iter()
        .map(|k| AccountId::from(&PublicKey::new_from_private_key(k)))
        .collect();
    ChainedCall {
        program_id,
        instruction_data: Program::serialize_instruction(instruction).unwrap(),
        pre_states: accounts
            .iter()
            .map(|id| {
                AccountWithMetadata::new(state.get_account_by_id(*id), signer_ids.contains(id), *id)
            })
            .collect(),
        pda_seeds: vec![],
    }
}

pub fn measure<I: Serialize>(
    label: &str,
    state: &V03State,
    program_id: ProgramId,
    accounts: &[AccountId],
    signers: &[&PrivateKey],
    instruction: &I,
) -> Measured {
    let first = first_call(state, program_id, accounts, signers, instruction);
    let prove = super::prove::proving_enabled();
    let settings = super::prove::ProveSettings::from_env();
    let mut queue = VecDeque::from([(first, None, 0usize)]);
    let (mut per_exec, mut total, mut proofs) = (Vec::new(), 0u64, Vec::new());
    while let Some((call, caller, depth)) = queue.pop_front() {
        let n = per_exec.len() + 1;
        let (name, out, cycles) = if prove {
            let (name, out, p) = super::prove::prove_one(&call, caller, &settings);
            println!(
                "PROOF[{label}] exec#{n} depth={depth} program={name} user_cycles={} total_cycles={} segments={} seconds={:.1} seal_bytes={} po2={}",
                p.user_cycles, p.total_cycles, p.segments, p.seconds, p.seal_bytes, p.po2
            );
            super::prove::report(label, &p);
            let cycles = p.user_cycles;
            proofs.push(p);
            (name, out, cycles)
        } else {
            let (name, out, cycles) = execute_one(&call, caller);
            println!("CYCLES[{label}] exec#{n} depth={depth} program={name} cycles={cycles}");
            (name, out, cycles)
        };
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
    Measured {
        per_exec,
        total,
        proofs,
    }
}
