//! The debt auction (design note §4.2 rows 13-15; v0.2.4 `debt.rs`, its numbers):
//!
//! - `StartDebt` is a system root at the config `K`: the accounting reads the buffer (and the
//!   reference oracle), settles what it can, reserves the lot (`debt_on_auction`) and opens
//!   `debt_actor(E, K, nonce)`.
//! - `TakeDebt` / `CloseDebt` are roots at `K` too, which probes the auction and hands it the
//!   config's context. A take burns the taker's stablecoin and mints protocol tokens through `K`,
//!   the protocol definition's minter; a close after the TTL releases the untaken reservation.
//!
//! The shortfall is Plan 5's lot-3 case ([`World::book_shortfall`], `U` = 10_000, where v0.2.4
//! had 355_049); the taker holds 1_000 stablecoin minted by the host and the protocol token's
//! genesis. `harness::invariants::check` after every send (ADR-024 pairing included).

use auction_engine_params::{
    EngineError,
    args::{DEBT_RELIST_DELAY_MS, EngineParamsArgs},
};
use engine_core::{
    actors::{config_actor, debt_actor},
    check::fail_text,
    msg::EngineMsg,
    system_state::SystemPhase,
};
use harness::{
    World, expect_revert, invariants,
    system::{TAKER, TAKER_GENESIS},
    world::{RAY, SALT, SLACK_MS},
};
use lee::{AccountId, Actor, error::LeeError};
use lee_core::program::TransactionEvent;
use token_core::{Definition, TokenMsg};

/// The chain time the tests start at, ms.
const T0: u64 = 1_000_000;
/// `EngineParamsArgs::defaults_for_tests().system_ttl_ms`.
const TTL: u64 = 14_400_000;
/// The taker's stablecoin, minted by the host.
const TAKER_STABLE: u128 = 1_000;

type Sent = Result<Vec<TransactionEvent>, LeeError>;

fn params() -> EngineParamsArgs {
    EngineParamsArgs {
        protocol_token_enabled: true,
        ..World::engine_params()
    }
}

/// `World::with_system(p)` at `T0` with the lot-3 shortfall booked and the taker funded; the
/// shortfall `U`.
fn world_with(p: EngineParamsArgs) -> (World, u128) {
    let mut w = World::with_system(p);
    w.set_now_ms(T0);
    let unbacked = w.book_shortfall();
    w.fund_bidder(TAKER, TAKER_STABLE);
    invariants::check(&w);
    (w, unbacked)
}

fn world() -> (World, u128) {
    world_with(params())
}

fn checked(w: &World, sent: Sent) -> Sent {
    invariants::check(w);
    sent
}

fn start(w: &mut World, nonce: u64) -> Sent {
    let sent = w.start_debt(nonce);
    checked(w, sent)
}

fn take(w: &mut World, nonce: u64, amount_coin: u128, min_protocol_out: u128) -> Sent {
    let sent = w.take_debt(nonce, amount_coin, min_protocol_out);
    checked(w, sent)
}

fn close(w: &mut World, nonce: u64) -> Sent {
    let sent = w.close_debt(nonce);
    checked(w, sent)
}

fn expect_err(sent: Sent, err: EngineError) {
    expect_revert(sent, &fail_text(err));
}

fn donate_buffer(w: &mut World, amount: u128) {
    let buffer = w.buffer().account_id;
    w.donate_stable(buffer, amount);
    invariants::check(w);
}

fn taker(w: &mut World) -> AccountId {
    w.chain.key(TAKER).1
}

fn protocol_balance(w: &mut World, label: &str) -> u128 {
    let (account, def) = (w.chain.key(label).1, w.protocol_def());
    w.balance(account, def)
}

fn supply(w: &World, definition: AccountId) -> u128 {
    w.chain
        .raw_state(Actor::new(definition, w.token))
        .and_then(|bytes| Definition::decode(&bytes))
        .expect("a token definition")
        .supply
}

fn settle_all(w: &mut World) {
    let msg = w.settle_debt_msg(u128::MAX);
    let sent = w.send_system(&msg, "settler", &[]);
    checked(w, sent).expect("settles");
}

#[test]
fn debt_auction_mints_against_burned_coin() {
    let (mut w, unbacked) = world();
    let stable_supply = supply(&w, w.stable_def);
    start(&mut w, 0).expect("starts");
    let a = w.accounting_state();
    assert_eq!(
        (a.debt_on_auction, a.debt_live, a.debt_nonce),
        (500, true, 1)
    );
    let d = w.debt_state(0);
    assert_eq!(
        (d.lot_left, d.start_amount, d.start_ms, d.phase),
        (500, 250, T0, SystemPhase::Live)
    );
    // start_amount = floor(500 / 2) = 250 tokens for the whole 500-coin lot at t = 0.
    take(&mut w, 0, 500, 250).expect("takes the lot");
    let def = w.protocol_def();
    assert_eq!(protocol_balance(&mut w, TAKER), TAKER_GENESIS + 250);
    assert_eq!(supply(&w, def), TAKER_GENESIS + 250);
    let taker = taker(&mut w);
    assert_eq!(w.stable_balance(taker), TAKER_STABLE - 500);
    assert_eq!(supply(&w, w.stable_def), stable_supply - 500);
    let a = w.accounting_state();
    assert_eq!(
        (a.unbacked_debt, a.debt_on_auction, a.total_debt_auctioned),
        (unbacked - 500, 0, 500)
    );
    assert_eq!(
        (a.total_protocol_minted, a.debt_live, a.last_debt_end_ms),
        (250, false, T0)
    );
    assert_eq!(w.debt_state(0).phase, SystemPhase::Ended);
}

/// The taker never held the protocol token: the definition's `Credit` (HOLDING seed) admits its
/// fresh holding without any transaction evidence.
#[test]
fn take_debt_mints_into_a_fresh_holding() {
    let (mut w, _) = world();
    let fresh = "fresh-taker";
    let account = w.fund_bidder(fresh, TAKER_STABLE).account;
    let holding = w.holding(account, w.protocol_def());
    assert_eq!(w.chain.raw_state(holding), None, "no protocol holding yet");
    start(&mut w, 0).expect("starts");
    let sent = w.take_debt_as(fresh, 0, 500, 0);
    checked(&w, sent).expect("mints into the fresh holding");
    assert_eq!(protocol_balance(&mut w, fresh), 250);
    assert_eq!(w.stable_balance(account), TAKER_STABLE - 500);
}

#[test]
fn partial_take_then_close_releases_the_reservation() {
    let (mut w, unbacked) = world();
    start(&mut w, 0).expect("starts");
    take(&mut w, 0, 300, 150).expect("a partial take");
    let a = w.accounting_state();
    assert_eq!((a.debt_on_auction, a.unbacked_debt), (200, unbacked - 300));
    assert_eq!(protocol_balance(&mut w, TAKER), TAKER_GENESIS + 150);
    w.set_now_ms(T0 + TTL - 1);
    expect_err(close(&mut w, 0), EngineError::DelayNotElapsed);
    w.set_now_ms(T0 + TTL);
    close(&mut w, 0).expect("closes");
    let a = w.accounting_state();
    assert_eq!(
        (a.debt_on_auction, a.debt_live, a.last_debt_end_ms),
        (0, false, T0 + TTL)
    );
    assert_eq!(a.unbacked_debt, unbacked - 300, "close burns nothing");
    let d = w.debt_state(0);
    assert_eq!((d.phase, d.lot_left), (SystemPhase::Ended, 0));
    expect_err(close(&mut w, 0), EngineError::NotLive);
    expect_err(take(&mut w, 0, 200, 0), EngineError::NotLive);
}

#[test]
fn start_needs_the_next_nonce_and_a_single_live_auction() {
    let (mut w, _) = world();
    expect_err(start(&mut w, 1), EngineError::NonceMismatch);
    start(&mut w, 0).expect("starts");
    expect_err(start(&mut w, 1), EngineError::DebtAuctionAlreadyLive);
    // v0.2.4 refused a second `create` of nonce 0; here the spent nonce fails first.
    expect_err(start(&mut w, 0), EngineError::NonceMismatch);
    assert_eq!(w.accounting_state().debt_on_auction, 500, "one reservation");
    take(&mut w, 0, u128::MAX, 0).expect("takes the lot");
    w.set_now_ms(T0 + DEBT_RELIST_DELAY_MS);
    start(&mut w, 1).expect("a new auction after the first ended");
    let a = w.accounting_state();
    assert_eq!((a.debt_on_auction, a.debt_nonce), (500, 2));
}

#[test]
fn start_is_blocked_when_the_buffer_covers_the_debt() {
    let (mut w, unbacked) = world();
    donate_buffer(&mut w, 400_000);
    expect_err(start(&mut w, 0), EngineError::InsufficientBadDebt);
    assert_eq!(
        w.accounting_state().unbacked_debt,
        unbacked,
        "a failed start settles nothing"
    );
    assert_eq!(w.stable_balance(w.buffer().account_id), 400_000);
}

#[test]
fn settle_debt_excludes_the_reservation_of_a_live_debt_auction() {
    let (mut w, _) = world();
    start(&mut w, 0).expect("starts");
    donate_buffer(&mut w, 400_000);
    settle_all(&mut w);
    let a = w.accounting_state();
    assert_eq!(
        (a.unbacked_debt, a.debt_on_auction),
        (500, 500),
        "settle_debt burns only unbacked - debt_on_auction"
    );
    assert!(
        w.stable_balance(w.buffer().account_id) > 0,
        "the rest stays"
    );
    take(&mut w, 0, 500, 0).expect("takes the lot");
    let a = w.accounting_state();
    assert_eq!((a.unbacked_debt, a.debt_on_auction), (0, 0));
}

/// Reference price 1/9 coin per token: the lot starts at 4_500 tokens (cap 5_000); growth 30 %
/// at 1_000 ms would offer 5_850, capped at 5_000.
#[test]
fn mint_never_exceeds_the_absolute_cap() {
    let (mut w, _) = world_with(EngineParamsArgs {
        protocol_token_ref_price: RAY / 9,
        ..params()
    });
    start(&mut w, 0).expect("starts");
    assert_eq!(w.debt_state(0).start_amount, 4_500);
    w.set_now_ms(T0 + 1_000);
    take(&mut w, 0, 500, 0).expect("takes the lot");
    assert_eq!(w.accounting_state().total_protocol_minted, 5_000);
    let def = w.protocol_def();
    assert_eq!(supply(&w, def), TAKER_GENESIS + 5_000);
}

/// `AssertSystemIdle` end to end on the debt side: the protocol token cannot be switched off while
/// a debt auction is live; the reference price can (the auction snapshotted its start amount).
#[test]
fn protocol_token_locked_while_debt_auction_live() {
    let (mut w, _) = world();
    start(&mut w, 0).expect("starts");
    let key = w.admin_key.clone();
    let off = w.set_engine_params_msg(
        EngineParamsArgs {
            protocol_token_enabled: false,
            ..params()
        },
        None,
    );
    let sent = w.send_engine(w.admin, &[&key], &off);
    expect_err(checked(&w, sent), EngineError::ProtocolTokenLocked);
    let price = w.set_engine_params_msg(
        EngineParamsArgs {
            protocol_token_ref_price: 3 * RAY,
            ..params()
        },
        None,
    );
    let sent = w.send_engine(w.admin, &[&key], &price);
    checked(&w, sent).expect("the reference price may change");
    take(&mut w, 0, 500, 250).expect("the snapshotted start amount");
    let sent = w.send_engine(w.admin, &[&key], &off);
    checked(&w, sent).expect("unlocked once the auction ended");
    assert_eq!(w.config_state().protocol_token, None);
}

/// Review Focus 5: the relist delay is measured at the claimed time. With the chain at
/// `end + delay + slack − 1`, a keeper claiming `chain − slack = end + delay − 1` (which the clock
/// accepts) is refused; claiming `end + delay` starts.
#[test]
fn relist_delay_boundary_uses_the_claimed_time() {
    let (mut w, _) = world();
    start(&mut w, 0).expect("starts");
    take(&mut w, 0, u128::MAX, 0).expect("takes the lot");
    assert_eq!(w.accounting_state().last_debt_end_ms, T0);
    let relist = T0 + DEBT_RELIST_DELAY_MS;
    w.set_now_ms(relist + SLACK_MS - 1);
    let sent = w.start_debt_at(1, relist - 1);
    expect_err(checked(&w, sent), EngineError::DelayNotElapsed);
    let sent = w.start_debt_at(1, relist);
    checked(&w, sent).expect("the delay elapsed at the claimed time");
    assert_eq!(w.debt_state(1).start_ms, relist);
}

/// `K` mints only for `debt_actor(E, K, nonce)`: a root `Mint`, an impostor program at `D`'s
/// account and at a rogue config's `D'` account all revert, and nothing is minted. (An engine
/// actor other than `D` sending `Mint` to `K` is unreachable in a transaction; that half is
/// unit-tested on `engine_core::assertions::mint_sender`.) The token refuses a `MintTo` from
/// anyone but its minter `K`.
#[test]
fn mint_from_anything_but_this_configs_debt_actor_reverts() {
    let (mut w, _) = world();
    start(&mut w, 0).expect("starts");
    let (rogue_key, rogue) = w.chain.key("rogue-admin");
    w.chain.fund(rogue, 1);
    let init = w.init_msg();
    w.send_engine(rogue, &[&rogue_key], &init)
        .expect("the rogue admin's own config");
    let k2 = config_actor(w.engine, rogue, SALT).account_id;
    let def = w.protocol_def();
    let to = taker(&mut w);
    let mint = EngineMsg::Mint {
        nonce: 0,
        to,
        amount: 1_000,
    };
    let refused = fail_text(EngineError::WrongAccount("debt_auction"));
    expect_revert(w.chain.send(w.config, &[w.config], &[], &mint), &refused);
    // `D'` was never started: an account must exist there for an actor to sit at it.
    let rogue_debt = debt_actor(w.engine, k2, 0).account_id;
    w.chain.fund(rogue_debt, 1);
    for at in [w.debt(0).account_id, rogue_debt] {
        let impostor = w.impostor(at);
        expect_revert(w.speak(impostor, w.config, &mint), &refused);
    }
    let impostor = w.impostor(w.debt(0).account_id);
    let mint_to = TokenMsg::MintTo { to, amount: 1_000 };
    expect_revert(
        w.speak(impostor, Actor::new(def, w.token), &mint_to),
        "token: NotMinter",
    );
    assert_eq!(supply(&w, def), TAKER_GENESIS, "nothing was minted");
    invariants::check(&w);
}

/// Final Minor 6: the invariants check protocol-token conservation (`supply == genesis +
/// total_protocol_minted − total_protocol_burned`), so a supply change the accounting did not
/// book is caught. Here the taker burns 1 of its own protocol tokens straight through the token
/// program, which the engine never sees.
#[test]
#[should_panic(expected = "protocol supply")]
fn invariants_catch_an_unbooked_protocol_supply_change() {
    let (mut w, _) = world();
    let (key, taker) = w.chain.key(TAKER);
    let def = w.protocol_def();
    let owner = Actor::new(taker, w.token);
    let actors = [owner, w.holding(taker, def), Actor::new(def, w.token)];
    let burn = TokenMsg::Burn {
        definition: def,
        amount: 1,
    };
    w.chain
        .send(owner, &actors, &[&key], &burn)
        .expect("a holder burns its own protocol token");
    invariants::check(&w);
}
