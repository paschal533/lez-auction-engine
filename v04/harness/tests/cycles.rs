//! Cycles and fees per instruction of the liquidation slice (`--nocapture` prints the table; it
//! is also written to `target/cycles-v04.md`, see `v04/CYCLES.md`).
//!
//! Each transaction is the one the other tests send: built by the same `World` helpers, with the
//! admission evidence it needs, and measured with `Chain::cycles*` right before it is sent.

use std::{env, fs, path::PathBuf};

use engine_core::msg::EngineMsg;
use harness::{World, bidding::Bidder, world::RAY};
use lee::{AccountId, Actor, PrivateKey, PublicAccountEvidence};

/// `max_gas_exec`, the per-transaction gas cap (spike S8).
const GAS_CAP: u64 = 10_000_000;
/// The base fee per cycle the table prices at.
const BASE_FEE: u64 = 8;
const COLLATERAL: u128 = 500_000;
const DEBT: u128 = 600_000;
const TARGET: u128 = 660_000;
const FUNDS: u128 = 1_000_000;
/// The chain time of the liquidation and the bids, ms.
const T: u64 = 1_000_000;

/// Executions (messages delivered, replies included) of each transaction, counted from the
/// as-built message sequences: `lee::simulate_public_transaction` returns cycles only.
const INIT_EXECUTIONS: u32 = 7;
const SET_COLLATERAL_EXECUTIONS: u32 = 11;
const LIQUIDATE_EXECUTIONS: u32 = 27;
const PARTIAL_BID_EXECUTIONS: u32 = 13;
const FINAL_BID_EXECUTIONS: u32 = 18;
const CLAIM_EXECUTIONS: u32 = 5;
const SETTLE_EMPTY_EXECUTIONS: u32 = 6;

struct Row {
    instruction: &'static str,
    executions: u32,
    cycles: u64,
}

/// Cycles of one simulated transaction, which must succeed and fit the gas cap.
fn measured(
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

fn proxied(
    w: &World,
    signer: AccountId,
    key: &PrivateKey,
    msg: &EngineMsg,
) -> (u64, Result<(), lee::error::LeeError>) {
    let actors = w.proxy_actors(signer, msg);
    w.chain
        .cycles(Actor::new(signer, w.engine), &actors, &[key], msg)
}

fn table(rows: &[Row]) -> String {
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

fn liquidated() -> (World, AccountId, Bidder) {
    let mut w = World::new_at(RAY / 2);
    w.set_now_ms(T);
    let position = w.open_unsafe_position(COLLATERAL, DEBT);
    w.set_price(RAY / 2);
    w.liquidate(position, 0).expect("the keeper liquidates");
    let bidder = w.fund_bidder("bidder", FUNDS);
    (w, position, bidder)
}

fn bid_cycles(
    w: &World,
    bidder: &Bidder,
    msg: &EngineMsg,
    position: AccountId,
) -> (u64, Result<(), lee::error::LeeError>) {
    let actors = w.bid_actors(bidder.account, position, 0);
    let evidence = w.bid_evidence();
    w.chain
        .cycles_with_evidence(w.collateral, &actors, &[&bidder.key], &evidence, msg)
}

#[test]
fn every_slice_instruction_fits_the_gas_cap() {
    let mut rows = Vec::new();

    // Init and SetCollateral through the admin's proxy.
    let mut w = World::bare_at(RAY / 2);
    let (admin, admin_key) = (w.admin, w.admin_key.clone());
    let init = w.init_msg();
    rows.push(measured(
        "Init",
        INIT_EXECUTIONS,
        proxied(&w, admin, &admin_key, &init),
    ));
    w.send_engine(admin, &[&admin_key], &init).expect("init");
    w.set_now_ms(T);
    let set = w.set_collateral_msg(World::collateral_args());
    rows.push(measured(
        "SetCollateral",
        SET_COLLATERAL_EXECUTIONS,
        proxied(&w, admin, &admin_key, &set),
    ));

    // Liquidate at the keeper's proxy, with the auction vault's admission evidence.
    let mut w = World::new_at(RAY / 2);
    w.set_now_ms(T);
    let position = w.open_unsafe_position(COLLATERAL, DEBT);
    w.set_price(RAY / 2);
    let msg = w.liquidate_msg(position, 0, w.now_ms);
    let actors = w.proxy_actors(w.keeper, &msg);
    let evidence: [PublicAccountEvidence; 1] = w.liquidate_evidence(position, 0);
    let keeper_key = w.keeper_key.clone();
    rows.push(measured(
        "Liquidate",
        LIQUIDATE_EXECUTIONS,
        w.chain.cycles_with_evidence(
            Actor::new(w.keeper, w.engine),
            &actors,
            &[&keeper_key],
            &evidence,
            &msg,
        ),
    ));

    // Partial and final Bid (the first carries the buffer's admission evidence), ClaimLeftover.
    let (mut w, position, bidder) = liquidated();
    let partial = w.bid_msg(bidder.account, position, 0, (100_000, 0), w.now_ms);
    rows.push(measured(
        "Bid (partial)",
        PARTIAL_BID_EXECUTIONS,
        bid_cycles(&w, &bidder, &partial, position),
    ));
    w.send_bid(&partial, &[&bidder.key]).expect("partial bid");
    let last = w.bid_msg(bidder.account, position, 0, (TARGET, 0), w.now_ms);
    rows.push(measured(
        "Bid (final)",
        FINAL_BID_EXECUTIONS,
        bid_cycles(&w, &bidder, &last, position),
    ));
    w.send_bid(&last, &[&bidder.key]).expect("final bid");
    let owner = w.owner_of(position);
    let (owner_account, owner_key) = (owner.account, owner.key.clone());
    let claim = w.claim_msg(position, 0);
    rows.push(measured(
        "ClaimLeftover",
        CLAIM_EXECUTIONS,
        proxied(&w, owner_account, &owner_key, &claim),
    ));
    w.claim(position, 0).expect("claim applies");

    // SettleEmpty: the keeper reward takes the whole 3-unit lot, rooted at the config.
    let (mut w, _, _) = liquidated();
    let empty = w.open_unsafe_position(3, 10_000);
    w.set_price(RAY / 2);
    w.liquidate(empty, 0).expect("the reward takes the lot");
    let msg = w.settle_empty_msg(empty, 0);
    let actors = w.settle_empty_actors(empty, 0);
    rows.push(measured(
        "SettleEmpty",
        SETTLE_EMPTY_EXECUTIONS,
        w.chain.cycles(w.config, &actors, &[], &msg),
    ));
    w.settle_empty(empty, 0).expect("settles");

    let markdown = table(&rows);
    println!("{markdown}");
    let dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target"));
    fs::create_dir_all(&dir).expect("target dir");
    fs::write(dir.join("cycles-v04.md"), markdown).expect("table written");
}
