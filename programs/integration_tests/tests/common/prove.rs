//! Real proofs (Plan 3b, ADR-045). With `AUCTION_PROVE=1` and `RISC0_DEV_MODE=0`, `measure::measure`
//! proves every execution of a transaction's chain with the risc0 prover instead of executing it.
//! The prover is the external `r0vm` 3.0.5 (risc0-zkvm default features), so proving cost lives in
//! that child process and the test binary can stay in the dev profile.
//!
//! On LEZ v0.2.4 the sequencer executes public transactions without proving them; these receipts are
//! the ones a proving pipeline (v0.3 port, private path) must produce for the same executions.
//! Every receipt is verified against the program id of its call (= its ImageID) before it is counted.

use std::time::Instant;

use lee_core::program::{ChainedCall, ProgramId, ProgramOutput};
use risc0_zkvm::{default_prover, ProverOpts};

pub struct ProvedExec {
    pub program: &'static str,
    pub user_cycles: u64,
    pub total_cycles: u64,
    pub segments: usize,
    pub seconds: f64,
    pub seal_bytes: usize,
    pub po2: u32,
}

pub struct ProveSettings {
    pub po2: u32,
    pub succinct: bool,
}

impl ProveSettings {
    /// `AUCTION_PROVE_PO2` (segment limit, default 20), `AUCTION_PROVE_KIND` (`composite` default, or
    /// `succinct`).
    pub fn from_env() -> Self {
        let po2 = std::env::var("AUCTION_PROVE_PO2")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(20);
        let succinct = std::env::var("AUCTION_PROVE_KIND").as_deref() == Ok("succinct");
        Self { po2, succinct }
    }
}

pub fn proving_enabled() -> bool {
    std::env::var("AUCTION_PROVE").as_deref() == Ok("1")
}

/// Whether the default prover would run in dev mode (`RISC0_DEV_MODE`). Replaces the deprecated
/// `risc0_zkvm::is_dev_mode`; `ProverOpts` is what the prover actually honours.
pub fn dev_mode() -> bool {
    ProverOpts::default().dev_mode()
}

/// Proves one call with the same inputs `measure::execute_one` gives the executor (both build them
/// with `measure::guest_env`), verifies the receipt against the call's program id, and returns the
/// program output from the journal.
pub fn prove_one(
    call: &ChainedCall,
    caller: Option<ProgramId>,
    s: &ProveSettings,
) -> (&'static str, ProgramOutput, ProvedExec) {
    let opts = if s.succinct {
        ProverOpts::succinct()
    } else {
        ProverOpts::composite()
    };
    assert!(
        !opts.dev_mode(),
        "AUCTION_PROVE=1 needs RISC0_DEV_MODE=0: a dev-mode receipt proves nothing"
    );
    let (name, elf) = super::measure::elf_for(call.program_id);
    let env = super::measure::guest_env(call, caller, Some(s.po2));
    let start = Instant::now();
    let info = default_prover()
        .prove_with_opts(env, elf, &opts)
        .unwrap_or_else(|e| panic!("proving {name} failed: {e}"));
    let seconds = start.elapsed().as_secs_f64();
    info.receipt
        .verify(call.program_id)
        .unwrap_or_else(|e| panic!("{name}: the receipt does not verify against its ImageID: {e}"));
    let out: ProgramOutput = info.receipt.journal.decode().unwrap();
    let p = ProvedExec {
        program: name,
        user_cycles: info.stats.user_cycles,
        total_cycles: info.stats.total_cycles,
        segments: info.stats.segments,
        seconds,
        seal_bytes: info.receipt.seal_size(),
        po2: s.po2,
    };
    (name, out, p)
}

/// Appends one JSON line per proved execution to `$PROOF_REPORT` (CI evidence; Task 7's checker).
pub fn report(label: &str, p: &ProvedExec) {
    super::measure::append_json_line(
        "PROOF_REPORT",
        serde_json::json!({
            "label": label,
            "program": p.program,
            "user_cycles": p.user_cycles,
            "total_cycles": p.total_cycles,
            "segments": p.segments,
            "seconds": p.seconds,
            "seal_bytes": p.seal_bytes,
            "po2": p.po2,
            "verified": true,
        }),
    );
}
