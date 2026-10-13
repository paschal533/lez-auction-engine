//! Real proofs (ADR-045). Every test is `#[ignore]`d: run one at a time through
//! `scripts/measure-proof.sh` (sets `AUCTION_PROVE=1`, `RISC0_DEV_MODE=0`, a timeout, and records peak
//! RSS) or the `real-proofs.yml` jobs. The scenarios are the ADR-036 ones (`chains.rs`).

mod common;

use std::collections::HashMap;
use std::time::Instant;

use auction_engine_core::Instruction as Ix;
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID as CLOCK;
use common::*;
use lee::{
    execute_and_prove, privacy_preserving_transaction::circuit::ProgramWithDependencies,
    program::Program,
};
use lee_core::{account::AccountWithMetadata, InputAccountIdentity};

fn require_proving() {
    assert!(
        prove::proving_enabled() && !prove::dev_mode(),
        "run through scripts/measure-proof.sh (AUCTION_PROVE=1, RISC0_DEV_MODE=0)"
    );
}

/// Cheapest instruction of ADR-036: `settle_debt` on an empty buffer (1 execution, 591,029 cycles).
#[test]
#[ignore = "real proof: scripts/measure-proof.sh"]
fn prove_settle_debt() {
    require_proving();
    let mut w = World::with_engine();
    let (n, r) = w.measured(
        "settle_debt",
        vec![
            id(k::CALLER),
            config(),
            accounting(),
            buffer(),
            stable_def(),
        ],
        &[k::CALLER],
        Ix::SettleDebt { amount: u128::MAX },
    );
    r.expect("settle_debt");
    assert_eq!(n, 1);
}

/// Most expensive instruction of ADR-036: `liquidate` (5 executions, 3,151,610 cycles).
#[test]
#[ignore = "real proof: scripts/measure-proof.sh"]
fn prove_liquidate() {
    require_proving();
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    w.holding(k::KEEPER_COLL, collateral_def(), 0);
    let accounts = w.liquidate_accounts(pos, 0, id(k::KEEPER_COLL));
    let (n, r) = w.measured(
        "liquidate",
        accounts,
        &[k::KEEPER],
        Ix::Liquidate { nonce: 0 },
    );
    r.expect("liquidate");
    assert_eq!(n, 5);
}

/// Non-final bid (3 executions); the ADR-023 re-measure needs it (Task 7).
#[test]
#[ignore = "real proof: scripts/measure-proof.sh"]
fn prove_bid() {
    require_proving();
    let (mut w, pos) = liquidated_world();
    let accounts = w.bid_accounts(pos, 0, k::BIDDER_STABLE, k::BIDDER_COLL, false);
    let (n, r) = w.measured(
        "bid",
        accounts,
        &[k::BIDDER_STABLE],
        Ix::Bid {
            coin_in_max: 100_000,
            min_collateral_out: 0,
        },
    );
    r.expect("bid");
    assert_eq!(n, 3);
}

/// The final bid of the end-to-end path liquidate -> bid -> settle (4 executions).
#[test]
#[ignore = "real proof: scripts/measure-proof.sh"]
fn prove_bid_with_settlement() {
    require_proving();
    let (mut w, pos) = liquidated_world();
    let accounts = w.bid_accounts(pos, 0, k::BIDDER_STABLE, k::BIDDER_COLL, true);
    let (n, r) = w.measured(
        "bid_with_settlement",
        accounts,
        &[k::BIDDER_STABLE],
        Ix::BidWithSettlement {
            coin_in_max: LIQ_TARGET,
            min_collateral_out: 0,
        },
    );
    r.expect("final bid");
    assert_eq!(n, 4);
}

/// The LEZ privacy circuit (execute_and_prove) over `observe_feed` with all-public identities: the
/// proving path a private transaction takes (ADR-034). Measured for the record (M4 sizing).
#[test]
#[ignore = "real proof: scripts/measure-proof.sh"]
fn prove_ppe_observe_feed() {
    assert!(
        !prove::dev_mode(),
        "run through scripts/measure-proof.sh (RISC0_DEV_MODE=0)"
    );
    let w = World::with_engine();
    let ids = [
        id(k::CALLER),
        config(),
        coll_config(),
        redemption_state(),
        oracle(),
        CLOCK,
    ];
    let pre = ids
        .iter()
        .map(|i| AccountWithMetadata::new(w.state.get_account_by_id(*i), *i == id(k::CALLER), *i))
        .collect();
    let program = ProgramWithDependencies::new(
        Program::new(auction_engine_methods::AUCTION_ENGINE_ELF.to_vec().into()).expect("ELF"),
        HashMap::new(),
    );
    let start = Instant::now();
    let result = execute_and_prove(
        pre,
        Program::serialize_instruction(Ix::ObserveFeed).expect("instruction"),
        ids.iter().map(|_| InputAccountIdentity::Public).collect(),
        &program,
    );
    println!(
        "PROOF[ppe_observe_feed] seconds={:.1} ok={}",
        start.elapsed().as_secs_f64(),
        result.is_ok()
    );
    result.expect("the privacy circuit proves observe_feed");
}
