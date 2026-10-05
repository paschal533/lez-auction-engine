// Spike S1 step 6: chained-call count and executor cycles for a liquidate-shaped transaction.
//
// This is NOT a standalone file. run.sh appends it to a copy of
// lez-programs/programs/integration_tests/tests/stablecoin.rs (so it reuses that file's
// fixtures: Keys, Ids, Balances, state_for_stablecoin_tests, current_nonce, ...) and runs it as
// the integration test `chain_count`. It needs `risc0-zkvm` as a dev-dependency of the
// integration_tests crate (run.sh adds it to the scratch clone).

use lee_core::program::{ChainedCall, ProgramOutput};
use risc0_zkvm::{default_executor, ExecutorEnv};

const MAX_EXECUTIONS_FROM_LEE_SOURCE: usize = 11; // counter <= MAX_NUMBER_CHAINED_CALLS(10) checked before each call

fn elf_for(program_id: lee_core::program::ProgramId) -> (&'static str, &'static [u8]) {
    if program_id == Ids::stablecoin_program() {
        ("stablecoin", stablecoin_methods::STABLECOIN_ELF)
    } else if program_id == Ids::token_program() {
        ("token", token_methods::TOKEN_ELF)
    } else {
        panic!("unknown program in chain");
    }
}

/// Executes one program the way lee's `Program::execute` does, returning its output and cycles.
fn run_one(
    call: &ChainedCall,
    caller: Option<lee_core::program::ProgramId>,
) -> (&'static str, ProgramOutput, u64, u64) {
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
    (name, out, info.cycles(), info.segments.len() as u64)
}

/// Replays a whole chain breadth-first (same order as lee's validated_state_diff) and prints
/// one line per program execution. Returns (executions, total_cycles).
fn replay(label: &str, first: ChainedCall) -> (usize, u64) {
    let mut queue = std::collections::VecDeque::from([(first, None, 0usize)]);
    let (mut n, mut total) = (0usize, 0u64);
    while let Some((call, caller, depth)) = queue.pop_front() {
        let (name, out, cycles, segs) = run_one(&call, caller);
        n += 1;
        total += cycles;
        println!(
            "CHAIN[{label}] exec#{n} depth={depth} program={name} cycles={cycles} segments={segs} chained_out={}",
            out.chained_calls.len()
        );
        for c in out.chained_calls {
            queue.push_back((c, Some(call.program_id), depth + 1));
        }
    }
    println!("CHAIN[{label}] TOTAL executions={n} cycles={total} (lee allows at most {MAX_EXECUTIONS_FROM_LEE_SOURCE})");
    (n, total)
}

fn pre(state: &V03State, id: AccountId, signer: bool) -> AccountWithMetadata {
    AccountWithMetadata::new(state.get_account_by_id(id), signer, id)
}

#[test]
fn chain_count_liquidate_shaped() {
    let mut state = state_for_stablecoin_tests();

    // 1. open_position (real transaction): chain = stablecoin -> token transfer(s).
    let open = stablecoin_core::Instruction::OpenPosition {
        position_nonce: Ids::position_nonce(),
        initial_collateral_amount: Balances::collateral_deposit(),
    };
    let open_accounts = vec![
        Ids::owner(),
        Ids::position(),
        Ids::vault(),
        Ids::user_holding(),
        Ids::collateral_definition(),
        compute_protocol_parameters_pda(Ids::stablecoin_program()),
        CLOCK_01_PROGRAM_ACCOUNT_ID,
    ];
    let open_call = ChainedCall {
        program_id: Ids::stablecoin_program(),
        instruction_data: Program::serialize_instruction(&open).unwrap(),
        pre_states: open_accounts
            .iter()
            .map(|id| pre(&state, *id, *id == Ids::owner() || *id == Ids::user_holding()))
            .collect(),
        pda_seeds: vec![],
    };
    let (open_n, open_cycles) = replay("open_position", open_call);
    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        open_accounts,
        vec![current_nonce(&state, Ids::owner()), current_nonce(&state, Ids::user_holding())],
        open,
    )
    .unwrap();
    let ws = public_transaction::WitnessSet::for_message(&message, &[&Keys::owner(), &Keys::user_holding()]);
    state
        .transition_from_public_transaction(&PublicTransaction::new(message, ws), 0, 0)
        .expect("open_position succeeds on the real state machine");

    // 2. withdraw_collateral (stand-in for the hook's vault -> winner transfer): real tx + replay.
    let withdraw = stablecoin_core::Instruction::WithdrawCollateral {
        amount: Balances::collateral_withdraw(),
    };
    let w_accounts = vec![
        Ids::owner(),
        Ids::position(),
        Ids::vault(),
        Ids::user_holding(),
        compute_stability_fee_accumulator_pda(Ids::stablecoin_program()),
        compute_redemption_price_state_pda(Ids::stablecoin_program()),
        compute_protocol_parameters_pda(Ids::stablecoin_program()),
        CLOCK_01_PROGRAM_ACCOUNT_ID,
    ];
    let w_call = ChainedCall {
        program_id: Ids::stablecoin_program(),
        instruction_data: Program::serialize_instruction(&withdraw).unwrap(),
        pre_states: w_accounts.iter().map(|id| pre(&state, *id, *id == Ids::owner())).collect(),
        pda_seeds: vec![],
    };
    let (w_n, w_cycles) = replay("withdraw_collateral", w_call);

    // 3. init-holding: Token::InitializeAccount on a fresh account (what a winner's holding needs
    //    before it can receive a transfer).
    let fresh = AccountId::new([0x99; 32]);
    let init = token_core::Instruction::InitializeAccount;
    let init_call = ChainedCall {
        program_id: Ids::token_program(),
        instruction_data: Program::serialize_instruction(&init).unwrap(),
        pre_states: vec![
            pre(&state, Ids::collateral_definition(), false),
            pre(&state, fresh, true),
        ],
        pda_seeds: vec![],
    };
    let (i_n, i_cycles) = replay("init_holding", init_call);

    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        w_accounts,
        vec![current_nonce(&state, Ids::owner())],
        withdraw,
    )
    .unwrap();
    let ws = public_transaction::WitnessSet::for_message(&message, &[&Keys::owner()]);
    state
        .transition_from_public_transaction(&PublicTransaction::new(message, ws), 0, 0)
        .expect("withdraw_collateral succeeds on the real state machine");

    // Modelled liquidate hook: hook program (1 exec, cost unknown here) + init_holding chained
    // + stablecoin withdraw + token transfer.
    let modelled = 1 + i_n + w_n;
    println!(
        "CHAIN[summary] open_position={open_n} execs/{open_cycles} cycles; withdraw_collateral={w_n} execs/{w_cycles} cycles; init_holding={i_n} exec/{i_cycles} cycles"
    );
    println!(
        "CHAIN[summary] modelled liquidate = hook(1) + init_holding({i_n}) + withdraw_collateral({w_n}) = {modelled} executions (limit {MAX_EXECUTIONS_FROM_LEE_SOURCE}); cycles excl. hook = {}",
        i_cycles + w_cycles
    );
}
