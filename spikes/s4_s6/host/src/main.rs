//! Runs the S4/S6 bench guest under the risc0 executor (RISC0_DEV_MODE=1; no proving) and prints cycle counts.
//! For every case: one run with reps=1 (gross: includes the guest's fixed set-up) and one with reps=REPS;
//! marginal = (cycles(REPS) - cycles(1)) / (REPS - 1) is the cost of one more execution of the operation.
use risc0_zkvm::{default_executor, ExecutorEnv};
use s4_s6_methods::BENCH_ELF;
use lee_core::account::{Account, AccountId, AccountWithMetadata, Data, Nonce};

const REPS: u128 = 11;
const RAY: u128 = 1_000_000_000_000_000_000_000_000_000;

/// `n` real LEZ `AccountWithMetadata` values, each carrying `data_len` bytes of data (the Accounting is 129 bytes).
fn accounts(n: usize, data_len: usize) -> Vec<AccountWithMetadata> {
    (0..n)
        .map(|i| {
            let account = Account {
                program_owner: [i as u32 + 1; 8],
                balance: 1_000_000,
                data: Data::try_from(vec![0xAB; data_len]).unwrap(),
                nonce: Nonce(3),
            };
            AccountWithMetadata::new(account, i == 0, AccountId::new([i as u8 + 1; 32]))
        })
        .collect()
}

struct Run {
    cycles: u64,
    segments: Vec<(u32, u32)>,
    checksum: u128,
}

fn run(input: &[u128], accts: &[AccountWithMetadata]) -> Run {
    let env = ExecutorEnv::builder()
        .session_limit(Some(64 << 20))
        .write(&input.to_vec())
        .unwrap()
        .write(&accts)
        .unwrap()
        .build()
        .unwrap();
    let info = default_executor().execute(env, BENCH_ELF).expect("guest executes");
    Run {
        cycles: info.cycles(),
        segments: info.segments.iter().map(|s| (s.po2, s.cycles)).collect(),
        checksum: info.journal.decode::<u128>().expect("journal decodes"),
    }
}

/// `extra` are the op-specific inputs after `[op, reps]`.
fn measure(name: &str, op: u128, extra: &[u128], accts: &[AccountWithMetadata]) -> (u64, u64) {
    let mut one = vec![op, 1];
    one.extend_from_slice(extra);
    let mut many = vec![op, REPS];
    many.extend_from_slice(extra);
    let (a, b) = (run(&one, accts), run(&many, accts));
    let marginal = (b.cycles - a.cycles) / (REPS as u64 - 1);
    println!(
        "CYCLES[{name}] op={op} gross_reps1={} reps{REPS}={} marginal_per_op={marginal} segments(po2,cycles)={:?}/{:?} checksum={}",
        a.cycles, b.cycles, a.segments, b.segments, a.checksum
    );
    (a.cycles, marginal)
}

fn main() {
    println!("NOTE: executor cycles (session.cycles()), RISC0_DEV_MODE executor only; not proving cost.");
    let none = accounts(0, 0);
    measure("noop", 0, &[], &none);
    measure("mul_div_floor_worst", 1, &[], &none);
    measure("mul_div_floor_small", 2, &[], &none);
    measure("schedule_new_8pts", 9, &[], &none);
    measure("discount_ray_2pts", 3, &[], &none);
    measure("discount_ray_8pts", 4, &[], &none);
    measure("quote_liquidation", 5, &[], &none);
    measure("quote_bid+apply_bid", 6, &[], &none);
    measure("take_debt", 7, &[], &none);

    // S4 exponential variant: RAY * (1 - r)^t, r per second.
    let r = RAY / 20_000; // 0.005 % per second
    for t in [7_200u128, 6_143, 4_095, 100] {
        measure(&format!("exp_discount_t{t}"), 8, &[t, r], &none);
    }

    // S6: Accounting serialise / read-modify-write, and an extra account through the guest I/O.
    measure("accounting_borsh_serialise", 10, &[], &none);
    measure("accounting_rmw_per_bid", 11, &[], &none);
    for (op, label) in [(20u128, "in_only"), (21, "in_pre_echo_post")] {
        for n in [0usize, 1, 3] {
            let a = accounts(n, 129);
            let r = run(&[op, 1], &a);
            println!("CYCLES[account_{label}_n{n}_data129] gross_reps1={} segments(po2,cycles)={:?}", r.cycles, r.segments);
        }
    }
}
