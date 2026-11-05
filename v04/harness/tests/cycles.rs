//! Cycles and fees per instruction of the liquidation slice and of the system instructions
//! (`--nocapture` prints the tables; they are also written to `target/cycles-v04.md` and
//! `target/cycles-v04-system.md`, see `v04/CYCLES.md`).
//!
//! Each transaction is the one the other tests send: built by the same `World` helpers, and
//! measured with `Chain::cycles` right before it is sent. Fresh holdings are granted by the
//! token's `HOLDING` seed (ADR-062), so the only admission evidence is the buffer holding's `Pda`
//! for a system op that reads it while it is absent (`StartDebt` here).

use std::{env, fs, path::PathBuf};

use auction_engine_params::args::{DEBT_RELIST_DELAY_MS, EngineParamsArgs};
use engine_core::msg::EngineMsg;
use harness::{
    World,
    bidding::Bidder,
    system::{TAKER, TAKER_GENESIS},
    world::RAY,
};
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
/// as-built message sequences: `lee::simulate_public_transaction` returns cycles only. Each token
/// debit is two executions since ADR-062 (the owner's token actor, then its holding).
const INIT_EXECUTIONS: u32 = 7;
const SET_COLLATERAL_EXECUTIONS: u32 = 11;
const LIQUIDATE_EXECUTIONS: u32 = 29;
const PARTIAL_BID_EXECUTIONS: u32 = 15;
const FINAL_BID_EXECUTIONS: u32 = 21;
const CLAIM_EXECUTIONS: u32 = 6;
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
    w.chain.cycles(w.collateral, &actors, &[&bidder.key], msg)
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

    // Liquidate at the keeper's proxy.
    let mut w = World::new_at(RAY / 2);
    w.set_now_ms(T);
    let position = w.open_unsafe_position(COLLATERAL, DEBT);
    w.set_price(RAY / 2);
    let msg = w.liquidate_msg(position, 0, w.now_ms);
    let actors = w.proxy_actors(w.keeper, &msg);
    let keeper_key = w.keeper_key.clone();
    rows.push(measured(
        "Liquidate",
        LIQUIDATE_EXECUTIONS,
        w.chain.cycles(
            Actor::new(w.keeper, w.engine),
            &actors,
            &[&keeper_key],
            &msg,
        ),
    ));

    // Partial and final Bid, ClaimLeftover.
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
    write_table("cycles-v04.md", &markdown);
}

/// Writes `markdown` to `name` in the target directory.
fn write_table(name: &str, markdown: &str) {
    let dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target"));
    fs::create_dir_all(&dir).expect("target dir");
    fs::write(dir.join(name), markdown).expect("table written");
}

/// Executions of the system instructions, counted from the as-built message sequences (a
/// `ReadState` is two: the read and its `StateReply`; a token debit three: the owner's token
/// actor, its holding, then the credit or the definition's `SupplyDown`):
///
/// - `SetEngineParams` (a locked field changes, the type recorded): proxy, config probe, config,
///   collateral probe, `AssertCapAtMost`, `AssertSystemIdle`.
/// - `SetSlack` (the type recorded): proxy, config probe, config, collateral probe, `SlackUpdate`.
/// - `ObserveFeed`: collateral actor, two reads, the clock, `Proceed`.
/// - `SettleDebt` (burns): config, `System`, buffer read, `Proceed`, the buffer's `Burn`.
/// - `StartSurplus` (no burn, no reference oracle): config, clock, `System`, buffer read,
///   `Proceed`, the lot's `Transfer`, `SurplusOpen`.
/// - `TakeSurplus`: config, clock, probe, `SurplusTake`, the protocol `Burn`, the vault's
///   `Transfer`, `SurplusTaken`.
/// - `CloseSurplus` (non-empty vault): config, clock, probe, `SurplusClose`, vault read,
///   `Proceed`, the vault's `Sweep`, `SurplusReturned`.
/// - `StartDebt` (no burn, no reference oracle): config, clock, `System`, buffer read, `Proceed`,
///   `DebtOpen`.
/// - `TakeDebt`: config, clock, probe, `DebtTake`, the stablecoin `Burn`, `DebtTaken`, `Mint`,
///   `MintTo`, its `Credit`.
/// - `CloseDebt`: config, clock, probe, `DebtClose`, `DebtClosed`.
/// - `CheckSolvency` `Healthy` (burns): config, clock, `System`, buffer read, `Proceed`, `Burn`;
///   `Freeze` (no burn): the same without the burn, then `Freeze` and `EngineFreezeNewDebt`.
const SET_ENGINE_PARAMS_EXECUTIONS: u32 = 6;
const SET_SLACK_EXECUTIONS: u32 = 5;
const OBSERVE_FEED_EXECUTIONS: u32 = 7;
const SETTLE_DEBT_EXECUTIONS: u32 = 8;
const START_SURPLUS_EXECUTIONS: u32 = 10;
const TAKE_SURPLUS_EXECUTIONS: u32 = 11;
const CLOSE_SURPLUS_EXECUTIONS: u32 = 11;
const START_DEBT_EXECUTIONS: u32 = 7;
const TAKE_DEBT_EXECUTIONS: u32 = 11;
const CLOSE_DEBT_EXECUTIONS: u32 = 5;
const HEALTHY_EXECUTIONS: u32 = 9;
const FREEZE_EXECUTIONS: u32 = 8;

/// `EngineParamsArgs::defaults_for_tests().system_ttl_ms`.
const TTL: u64 = 14_400_000;
/// The lot-3 shortfall `World::book_shortfall` books.
const SHORTFALL: u128 = 10_000;

/// Cycles of the system root `msg` at its config, with `evidence`, signed by `keys`.
fn system_cycles(
    w: &World,
    msg: &EngineMsg,
    keys: &[&PrivateKey],
    evidence: &[PublicAccountEvidence],
) -> (u64, Result<(), lee::error::LeeError>) {
    let actors = w.system_actors(msg);
    let root = *actors.first().expect("the root config");
    w.chain
        .cycles_with_evidence(root, &actors, keys, evidence, msg)
}

/// The admin sets the world's engine parameters with `threshold`.
fn set_threshold(w: &mut World, threshold: u128) {
    let params = EngineParamsArgs {
        bad_debt_freeze_threshold: threshold,
        ..World::engine_params()
    };
    let msg = w.set_engine_params_msg(params, None);
    let key = w.admin_key.clone();
    w.send_engine(w.admin, &[&key], &msg)
        .expect("the threshold is set");
}

/// No protocol token: `ObserveFeed`, `SettleDebt`, `CheckSolvency` `Healthy` and `Freeze`.
fn plain_rows(rows: &mut Vec<Row>) {
    let mut w = World::new();
    w.set_now_ms(T);
    assert_eq!(w.book_shortfall(), SHORTFALL);
    let (admin_key, slack) = (w.admin_key.clone(), w.set_slack_msg(30_000));
    rows.push(measured(
        "SetSlack",
        SET_SLACK_EXECUTIONS,
        proxied(&w, w.admin, &admin_key, &slack),
    ));
    let observe = EngineMsg::ObserveFeed {
        config: w.config.account_id,
        collateral_def: w.collateral_def,
        now_ms: T,
    };
    let actors = [w.collateral, w.params, w.oracle, w.chain.clock()];
    rows.push(measured(
        "ObserveFeed",
        OBSERVE_FEED_EXECUTIONS,
        w.chain.cycles(w.collateral, &actors, &[], &observe),
    ));

    let buffer = w.buffer().account_id;
    w.donate_stable(buffer, 4_000);
    let settle = w.settle_debt_msg(100);
    rows.push(measured(
        "SettleDebt",
        SETTLE_DEBT_EXECUTIONS,
        system_cycles(&w, &settle, &[], &[]),
    ));
    w.send_system(&settle, "", &[]).expect("settles 100");

    set_threshold(&mut w, SHORTFALL);
    let check = w.check_solvency_msg();
    rows.push(measured(
        "CheckSolvency (Healthy, burns the buffer)",
        HEALTHY_EXECUTIONS,
        system_cycles(&w, &check, &[], &[]),
    ));
    w.check_solvency().expect("healthy");
    set_threshold(&mut w, World::engine_params().bad_debt_freeze_threshold);
    rows.push(measured(
        "CheckSolvency (Freeze)",
        FREEZE_EXECUTIONS,
        system_cycles(&w, &check, &[], &[]),
    ));
    w.check_solvency().expect("freezes");
    assert!(w.host_frozen());
}

fn token_world() -> World {
    let mut w = World::with_system(EngineParamsArgs {
        protocol_token_enabled: true,
        ..World::engine_params()
    });
    w.set_now_ms(T);
    w
}

/// The surplus auction, then `SetEngineParams` changing a system parameter.
fn surplus_rows(rows: &mut Vec<Row>) {
    let mut w = token_world();
    let (taker_key, taker) = w.chain.key(TAKER);
    // The signer must exist for the simulation to admit it (`World::send_system` funds it too).
    w.chain.fund(taker, 1);
    let buffer = w.buffer().account_id;
    w.donate_stable(buffer, 1_500);
    let config = w.config.account_id;
    let start = EngineMsg::StartSurplus {
        config,
        nonce: 0,
        now_ms: T,
    };
    rows.push(measured(
        "StartSurplus",
        START_SURPLUS_EXECUTIONS,
        system_cycles(&w, &start, &[], &[]),
    ));
    w.start_surplus(0).expect("starts");
    for instruction in ["TakeSurplus (partial)", "TakeSurplus (final)"] {
        let take = EngineMsg::TakeSurplus {
            taker,
            config,
            nonce: 0,
            amount_coin: 250,
            max_protocol_in: TAKER_GENESIS,
            now_ms: T,
        };
        rows.push(measured(
            instruction,
            TAKE_SURPLUS_EXECUTIONS,
            system_cycles(&w, &take, &[&taker_key], &[]),
        ));
        w.take_surplus(0, 250, TAKER_GENESIS).expect("takes");
    }

    w.donate_stable(buffer, 500);
    w.start_surplus(1).expect("the second auction starts");
    w.set_now_ms(T + TTL);
    let close = EngineMsg::CloseSurplus {
        config,
        nonce: 1,
        now_ms: T + TTL,
    };
    rows.push(measured(
        "CloseSurplus",
        CLOSE_SURPLUS_EXECUTIONS,
        system_cycles(&w, &close, &[], &[]),
    ));
    w.close_surplus(1).expect("closes");

    let params = EngineParamsArgs {
        protocol_token_enabled: true,
        surplus_lot: 400,
        ..World::engine_params()
    };
    let set = w.set_engine_params_msg(params, None);
    let (admin, admin_key) = (w.admin, w.admin_key.clone());
    rows.push(measured(
        "SetEngineParams",
        SET_ENGINE_PARAMS_EXECUTIONS,
        proxied(&w, admin, &admin_key, &set),
    ));
}

/// The debt auction on the lot-3 shortfall (the buffer holding still absent at the start).
fn debt_rows(rows: &mut Vec<Row>) {
    let mut w = token_world();
    assert_eq!(w.book_shortfall(), SHORTFALL);
    let (taker_key, taker) = w.chain.key(TAKER);
    w.fund_bidder(TAKER, 1_000);
    let config = w.config.account_id;
    let start = EngineMsg::StartDebt {
        config,
        nonce: 0,
        now_ms: T,
    };
    let evidence = w.buffer_evidence();
    assert_eq!(evidence.len(), 1, "the buffer holding is absent");
    rows.push(measured(
        "StartDebt",
        START_DEBT_EXECUTIONS,
        system_cycles(&w, &start, &[], &evidence),
    ));
    w.start_debt(0).expect("starts");
    let take = EngineMsg::TakeDebt {
        taker,
        config,
        nonce: 0,
        amount_coin: 500,
        min_protocol_out: 250,
        now_ms: T,
    };
    rows.push(measured(
        "TakeDebt",
        TAKE_DEBT_EXECUTIONS,
        system_cycles(&w, &take, &[&taker_key], &[]),
    ));
    w.take_debt(0, 500, 250).expect("takes the lot");

    let relist = T + DEBT_RELIST_DELAY_MS;
    w.set_now_ms(relist);
    w.start_debt(1).expect("the second auction starts");
    w.set_now_ms(relist + TTL);
    let close = EngineMsg::CloseDebt {
        config,
        nonce: 1,
        now_ms: relist + TTL,
    };
    rows.push(measured(
        "CloseDebt",
        CLOSE_DEBT_EXECUTIONS,
        system_cycles(&w, &close, &[], &[]),
    ));
    w.close_debt(1).expect("closes");
}

/// The table's row order.
const SYSTEM_ROWS: [&str; 13] = [
    "SetEngineParams",
    "SetSlack",
    "ObserveFeed",
    "SettleDebt",
    "StartSurplus",
    "TakeSurplus (partial)",
    "TakeSurplus (final)",
    "CloseSurplus",
    "StartDebt",
    "TakeDebt",
    "CloseDebt",
    "CheckSolvency (Healthy, burns the buffer)",
    "CheckSolvency (Freeze)",
];

#[test]
fn every_system_instruction_fits_the_gas_cap() {
    let mut rows = Vec::new();
    plain_rows(&mut rows);
    surplus_rows(&mut rows);
    debt_rows(&mut rows);
    rows.sort_by_key(|r| {
        SYSTEM_ROWS
            .iter()
            .position(|name| *name == r.instruction)
            .expect("a listed row")
    });
    assert_eq!(rows.len(), SYSTEM_ROWS.len());
    let markdown = table(&rows);
    println!("{markdown}");
    write_table("cycles-v04-system.md", &markdown);
}
