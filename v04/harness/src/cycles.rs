//! The cycle-table helpers the `cycles` and `cycles_tier2` tests share: one simulated row that
//! must succeed and fit the gas cap, a proxied simulation, the markdown table and its file in the
//! target directory (see `v04/CYCLES.md`).

use std::{env, fs, path::PathBuf};

use engine_core::msg::EngineMsg;
use lee::{AccountId, Actor, PrivateKey};

use crate::World;

/// `max_gas_exec`, the per-transaction gas cap (spike S8).
pub const GAS_CAP: u64 = 10_000_000;
/// The base fee per cycle the table prices at.
pub const BASE_FEE: u64 = 8;

pub struct Row {
    pub instruction: &'static str,
    pub executions: u32,
    pub cycles: u64,
}

/// Cycles of one simulated transaction, which must succeed and fit the gas cap.
pub fn measured(
    instruction: &'static str,
    executions: u32,
    (cycles, outcome): (u64, Result<(), lee::error::LeeError>),
) -> Row {
    outcome.unwrap_or_else(|err| panic!("{instruction} does not simulate: {err:?}"));
    assert!(
        cycles > 0 && cycles < GAS_CAP,
        "{instruction}: {cycles} cycles against a cap of {GAS_CAP}"
    );
    Row {
        instruction,
        executions,
        cycles,
    }
}

pub fn proxied(
    w: &World,
    signer: AccountId,
    key: &PrivateKey,
    msg: &EngineMsg,
) -> (u64, Result<(), lee::error::LeeError>) {
    let actors = w.proxy_actors(signer, msg);
    w.chain
        .cycles(Actor::new(signer, w.engine), &actors, &[key], msg)
}

pub fn table(rows: &[Row]) -> String {
    let mut out = String::from(
        "| Instruction | Executions | Cycles | Fee at base fee 8 (cycles x 8) |\n|---|---|---|---|\n",
    );
    for r in rows {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            r.instruction,
            r.executions,
            r.cycles,
            r.cycles.checked_mul(BASE_FEE).expect("fee fits u64")
        ));
    }
    out
}

/// Writes `markdown` to `name` in the target directory.
pub fn write_table(name: &str, markdown: &str) {
    let dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target"));
    fs::create_dir_all(&dir).expect("target dir");
    fs::write(dir.join(name), markdown).expect("table written");
}
