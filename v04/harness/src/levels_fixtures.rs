//! The operation x pause-level matrix fixtures (spec §3.2.2, ADR-073, Plan 9 Task 10): for each
//! [`Op`], a fresh [`World::new_tier2_system`] in which that operation's own preconditions hold,
//! with the transaction that runs it; and [`expected`], the outcome the rule table
//! [`allowed`] gives it.
//!
//! Every fixture, legacy included, starts the same way, so `World::go_stale` and the collateral
//! arguments match across the matrix: at [`T0`] one unsafe position of 500_000 collateral and
//! 600_000 debt, the oracle at `P = R = 0.5 RAY`, three TWAP samples 300_000 ms apart (the
//! recommended spacing; ignored under legacy), then 1_000 ms on. [`fixture`] dispatches on the
//! groups of operations that share a setup (position, auction, surplus, debt, admin), and builds
//! the transaction last, so [`stale_fixture`] can make the feed stale first and the message's
//! clock claim is the stale time.

use auction_engine_params::{EngineError, args::EngineParamsArgs};
use engine_core::{check::fail_text, msg::EngineMsg};
use lee::{AccountId, Actor, PrivateKey, PublicAccountEvidence};
use risk_core::{
    RiskArgs,
    pause::{FeedCase, Gate, Level, Op, allowed},
};

use crate::{
    World,
    bidding::Bidder,
    system::{TAKER, TAKER_GENESIS},
    tier2::tier2_collateral_args,
    world::RAY,
};

const COLLATERAL: u128 = 500_000;
const DEBT: u128 = 600_000;
/// The auction's target: the debt plus the 10 % penalty. A bid of it settles the auction.
const TARGET: u128 = 660_000;
/// The oracle price, = the Tier 2 world's redemption price.
const P: u128 = RAY / 2;
/// The recommended preset's sample spacing, ms.
const SPACING: u64 = 300_000;
/// The TWAP samples every fixture records.
const SAMPLES: u8 = 3;
/// The chain time the fixtures start at, ms.
const T0: u64 = 1_000_000;
/// After the newest sample, so it carries weight, ms.
const AFTER_SAMPLE_MS: u64 = 1_000;
/// A partial bid's coin in.
const BID_IN: u128 = 100_000;
/// A bidder's stablecoin.
const FUNDS: u128 = 1_000_000;
/// Plan 5's lot-3 empty-lot position: the keeper reward takes the whole seizure.
const EMPTY_COLLATERAL: u128 = 3;
const EMPTY_DEBT: u128 = 10_000;
/// Buffer stablecoin a surplus auction of 500 can start from (`tests/pause_levels.rs`).
const SURPLUS_BUFFER: u128 = 1_500;
/// Buffer stablecoin `SettleDebt` and `CheckSolvency` burn against the shortfall.
const SETTLE_BUFFER: u128 = 4_000;
/// A `SettleDebt`'s amount.
const SETTLE: u128 = 100;
/// A surplus take's coin out.
const SURPLUS_TAKE: u128 = 100;
/// A debt take's coin in and least protocol out (`tests/cycles.rs`).
const DEBT_TAKE: u128 = 500;
const DEBT_MIN_OUT: u128 = 250;
/// The debt taker's stablecoin.
const TAKER_STABLE: u128 = 1_000;
/// The new slack of the `SetSlack` row, ms.
const NEW_SLACK_MS: u64 = 30_000;
/// The new surplus lot of the `SetEngineParams` row (`tests/cycles.rs`).
const NEW_SURPLUS_LOT: u128 = 400;

/// One matrix case: a world in which the operation's preconditions hold, and the transaction
/// that runs it (root, actors, signers, message and admission evidence).
pub struct Fixture {
    pub world: World,
    pub root: Actor,
    pub actors: Vec<Actor>,
    pub signers: Vec<PrivateKey>,
    pub msg: EngineMsg,
    /// The buffer holding's `Pda` evidence while it is absent, for the system roots that read it.
    pub evidence: Vec<PublicAccountEvidence>,
}

/// What an operation's setup left for its transaction: the position it acts on, and the bidder
/// of an auction row.
struct Target {
    position: AccountId,
    bidder: Option<Bidder>,
}

/// A transaction without its world.
struct Tx {
    root: Actor,
    actors: Vec<Actor>,
    signers: Vec<PrivateKey>,
    msg: EngineMsg,
    evidence: Vec<PublicAccountEvidence>,
}

/// The fixture of `op` under `risk` with a live feed.
#[must_use]
pub fn fixture(op: Op, risk: RiskArgs) -> Fixture {
    build(op, risk, |_| {})
}

/// The fixture of `op` under `risk` after `World::go_stale`: the transaction claims the first
/// time at which the feed is stale.
#[must_use]
pub fn stale_fixture(op: Op, risk: RiskArgs) -> Fixture {
    build(op, risk, World::go_stale)
}

/// What the matrix expects of `op` at `level` with the feed `feed` under `risk`: `None` when it
/// runs, else the revert text. From [`allowed`]: a refusal is its own code; `FeedRefusal` is the
/// stale feed's `PriceFeedStale` (`World::go_stale` makes the feed stale); `Fallback` runs when
/// D4 is on (`RiskArgs::d4_on`, with the auction's stale floor) and is `PriceFeedStale` when it
/// is off. The fixtures never set the legacy `admin_paused` flag, so `Paused` does not arise.
#[must_use]
pub fn expected(op: Op, level: Level, feed: FeedCase, risk: &RiskArgs) -> Option<String> {
    let stale = || Some(fail_text(EngineError::PriceFeedStale));
    match allowed(op, level, feed) {
        Err(err) => Some(fail_text(err)),
        Ok(Gate::Run) => None,
        Ok(Gate::FeedRefusal) => stale(),
        Ok(Gate::Fallback) if risk.d4_on() => None,
        Ok(Gate::Fallback) => stale(),
    }
}

fn build(op: Op, risk: RiskArgs, before_tx: impl FnOnce(&mut World)) -> Fixture {
    let mut w = World::new_tier2_system(risk);
    w.set_now_ms(T0);
    let position = w.open_unsafe_position(COLLATERAL, DEBT);
    w.set_price(P);
    w.warm_twap(P, SAMPLES, SPACING);
    later(&mut w, AFTER_SAMPLE_MS);
    let target = prepare(&mut w, op, position);
    before_tx(&mut w);
    let tx = transaction(&mut w, op, &target);
    Fixture {
        world: w,
        root: tx.root,
        actors: tx.actors,
        signers: tx.signers,
        msg: tx.msg,
        evidence: tx.evidence,
    }
}

/// Advances the world's time by `ms`.
fn later(w: &mut World, ms: u64) {
    let now = w.now_ms.checked_add(ms).expect("time fits");
    w.set_now_ms(now);
}

/// Past the system auctions' TTL, with a fresh publish so the feed is live again.
fn past_ttl(w: &mut World) {
    later(w, World::engine_params().system_ttl_ms);
    w.set_price(P);
}

/// `op`'s own preconditions, on top of the common start.
fn prepare(w: &mut World, op: Op, position: AccountId) -> Target {
    let bidder = match op {
        Op::Liquidate
        | Op::ObserveFeed
        | Op::SetCollateral
        | Op::SetEngineParams
        | Op::SetSlack
        | Op::SetPauseLevel => None,
        Op::Bid | Op::ClaimLeftover => Some(prepare_auction(w, op, position)),
        Op::SettleEmpty => return prepare_empty(w),
        Op::StartSurplus | Op::TakeSurplus | Op::CloseSurplus => {
            prepare_surplus(w, op);
            None
        }
        Op::StartDebt | Op::TakeDebt | Op::CloseDebt | Op::SettleDebt | Op::CheckSolvency => {
            prepare_debt(w, op);
            None
        }
    };
    Target { position, bidder }
}

/// A live auction and a funded bidder; for `ClaimLeftover` the bidder's final bid has settled
/// it with leftover collateral.
fn prepare_auction(w: &mut World, op: Op, position: AccountId) -> Bidder {
    w.liquidate(position, 0).expect("the keeper liquidates");
    let bidder = w.fund_bidder("bidder", FUNDS);
    if op == Op::ClaimLeftover {
        w.bid(&bidder, position, 0, (TARGET, 0))
            .expect("the final bid settles");
    }
    bidder
}

/// Plan 5's lot-3 auction (`tests/cycles.rs`): liquidated, its whole lot paid as the keeper
/// reward, not yet settled.
fn prepare_empty(w: &mut World) -> Target {
    let empty = w.open_unsafe_position(EMPTY_COLLATERAL, EMPTY_DEBT);
    w.set_price(P);
    w.liquidate(empty, 0).expect("the reward takes the lot");
    Target {
        position: empty,
        bidder: None,
    }
}

/// Buffer stablecoin for one surplus lot and the taker's account; for a take a live surplus
/// auction, for a close one past its TTL.
fn prepare_surplus(w: &mut World, op: Op) {
    let buffer = w.buffer().account_id;
    w.donate_stable(buffer, SURPLUS_BUFFER);
    let (_, taker) = w.chain.key(TAKER);
    if !w.chain.account_exists(taker) {
        w.chain.fund(taker, 1);
    }
    if op == Op::StartSurplus {
        return;
    }
    w.start_surplus(0).expect("the surplus auction starts");
    if op == Op::CloseSurplus {
        past_ttl(w);
    }
}

/// The lot-3 shortfall booked; for `SettleDebt` and `CheckSolvency` buffer stablecoin to burn;
/// for a take a live debt auction and a taker with stablecoin, for a close one past its TTL.
fn prepare_debt(w: &mut World, op: Op) {
    assert!(w.book_shortfall() > 0, "a shortfall is booked");
    if matches!(op, Op::SettleDebt | Op::CheckSolvency) {
        let buffer = w.buffer().account_id;
        w.donate_stable(buffer, SETTLE_BUFFER);
        return;
    }
    if op == Op::StartDebt {
        return;
    }
    w.start_debt(0).expect("the debt auction starts");
    if op == Op::TakeDebt {
        w.fund_bidder(TAKER, TAKER_STABLE);
    } else {
        past_ttl(w);
    }
}

/// `op`'s transaction at the world's current time.
fn transaction(w: &mut World, op: Op, t: &Target) -> Tx {
    let config = w.config.account_id;
    let now_ms = w.now_ms;
    match op {
        Op::Liquidate => liquidate_tx(w, t.position),
        Op::Bid => bid_tx(w, t),
        Op::SettleEmpty => settle_empty_tx(w, t.position),
        Op::ClaimLeftover => claim_tx(w, t.position),
        Op::ObserveFeed => observe_tx(w),
        Op::StartSurplus => system_tx(w, start_surplus(config, now_ms), None),
        Op::TakeSurplus => take_surplus_tx(w),
        Op::CloseSurplus => system_tx(w, close_surplus(config, now_ms), None),
        Op::StartDebt => system_tx(w, start_debt(config, now_ms), None),
        Op::TakeDebt => take_debt_tx(w),
        Op::CloseDebt => system_tx(w, close_debt(config, now_ms), None),
        Op::SettleDebt => {
            let msg = w.settle_debt_msg(SETTLE);
            system_tx(w, msg, None)
        }
        Op::CheckSolvency => {
            let msg = w.check_solvency_msg();
            system_tx(w, msg, None)
        }
        Op::SetCollateral => {
            let risk = w.collateral_state().ext.risk;
            admin_tx(w, w.set_collateral_msg_with(tier2_collateral_args(), risk))
        }
        Op::SetEngineParams => admin_tx(w, set_engine_params(w)),
        Op::SetSlack => admin_tx(w, w.set_slack_msg(NEW_SLACK_MS)),
        Op::SetPauseLevel => admin_tx(w, w.set_pause_level_msg(Level::NoLiquidations.as_u8())),
    }
}

fn liquidate_tx(w: &World, position: AccountId) -> Tx {
    let msg = w.liquidate_msg(position, 0, w.now_ms);
    Tx {
        root: Actor::new(w.keeper, w.engine),
        actors: w.proxy_actors(w.keeper, &msg),
        signers: vec![w.keeper_key.clone()],
        msg,
        evidence: Vec::new(),
    }
}

fn bid_tx(w: &World, t: &Target) -> Tx {
    let bidder = t.bidder.as_ref().expect("an auction row has a bidder");
    let msg = w.bid_msg(bidder.account, t.position, 0, (BID_IN, 0), w.now_ms);
    Tx {
        root: w.collateral,
        actors: w.bid_actors(bidder.account, t.position, 0),
        signers: vec![bidder.key.clone()],
        msg,
        evidence: Vec::new(),
    }
}

fn settle_empty_tx(w: &World, position: AccountId) -> Tx {
    Tx {
        root: w.config,
        actors: w.settle_empty_actors(position, 0),
        signers: Vec::new(),
        msg: w.settle_empty_msg(position, 0),
        evidence: Vec::new(),
    }
}

fn claim_tx(w: &World, position: AccountId) -> Tx {
    let owner = w.owner_of(position);
    let msg = w.claim_msg(position, 0);
    Tx {
        root: Actor::new(owner.account, w.engine),
        actors: w.proxy_actors(owner.account, &msg),
        signers: vec![owner.key.clone()],
        msg,
        evidence: Vec::new(),
    }
}

/// The permissionless `ObserveFeed` at the collateral actor (`World::observe_feed`).
fn observe_tx(w: &World) -> Tx {
    Tx {
        root: w.collateral,
        actors: vec![w.collateral, w.params, w.oracle, w.chain.clock()],
        signers: Vec::new(),
        msg: EngineMsg::ObserveFeed {
            config: w.config.account_id,
            collateral_def: w.collateral_def,
            now_ms: w.now_ms,
        },
        evidence: Vec::new(),
    }
}

fn take_surplus_tx(w: &mut World) -> Tx {
    let msg = EngineMsg::TakeSurplus {
        taker: w.chain.key(TAKER).1,
        config: w.config.account_id,
        nonce: 0,
        amount_coin: SURPLUS_TAKE,
        max_protocol_in: TAKER_GENESIS,
        now_ms: w.now_ms,
    };
    system_tx(w, msg, Some(TAKER))
}

fn take_debt_tx(w: &mut World) -> Tx {
    let msg = EngineMsg::TakeDebt {
        taker: w.chain.key(TAKER).1,
        config: w.config.account_id,
        nonce: 0,
        amount_coin: DEBT_TAKE,
        min_protocol_out: DEBT_MIN_OUT,
        now_ms: w.now_ms,
    };
    system_tx(w, msg, Some(TAKER))
}

fn start_surplus(config: AccountId, now_ms: u64) -> EngineMsg {
    EngineMsg::StartSurplus {
        config,
        nonce: 0,
        now_ms,
    }
}

fn close_surplus(config: AccountId, now_ms: u64) -> EngineMsg {
    EngineMsg::CloseSurplus {
        config,
        nonce: 0,
        now_ms,
    }
}

fn start_debt(config: AccountId, now_ms: u64) -> EngineMsg {
    EngineMsg::StartDebt {
        config,
        nonce: 0,
        now_ms,
    }
}

fn close_debt(config: AccountId, now_ms: u64) -> EngineMsg {
    EngineMsg::CloseDebt {
        config,
        nonce: 0,
        now_ms,
    }
}

/// The system root `msg` at its config, signed by the signer `signer` (if any), with the
/// buffer's evidence when it reads the buffer holding (`World::buffer_evidence`).
fn system_tx(w: &mut World, msg: EngineMsg, signer: Option<&str>) -> Tx {
    let actors = w.system_actors(&msg);
    let root = *actors.first().expect("the root config");
    let signers = signer
        .map(|label| w.chain.key(label).0)
        .into_iter()
        .collect();
    let holding = w.holding(w.buffer().account_id, w.stable_def);
    let evidence = if actors.contains(&holding) {
        w.buffer_evidence()
    } else {
        Vec::new()
    };
    Tx {
        root,
        actors,
        signers,
        msg,
        evidence,
    }
}

/// `SetEngineParams` keeping the protocol token on (as `World::init_system_with` set it), with a
/// new surplus lot.
fn set_engine_params(w: &World) -> EngineMsg {
    let params = EngineParamsArgs {
        protocol_token_enabled: true,
        surplus_lot: NEW_SURPLUS_LOT,
        ..World::engine_params()
    };
    w.set_engine_params_msg(params, None)
}

/// The admin message `msg` at the admin's proxy, signed by the admin.
fn admin_tx(w: &World, msg: EngineMsg) -> Tx {
    Tx {
        root: Actor::new(w.admin, w.engine),
        actors: w.proxy_actors(w.admin, &msg),
        signers: vec![w.admin_key.clone()],
        msg,
        evidence: Vec::new(),
    }
}
