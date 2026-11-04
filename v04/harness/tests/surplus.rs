//! The surplus auction (design note §4.2 rows 10-12; v0.2.4 `surplus.rs`, its numbers):
//!
//! - `StartSurplus` is a system root at the config `K`: the accounting reads the buffer (and the
//!   reference oracle), settles the debt it can, then moves the lot from the buffer into the new
//!   auction's vault and opens `surplus_actor(E, K, nonce)`.
//! - `TakeSurplus` / `CloseSurplus` are roots at `K` too, which probes the auction and hands it
//!   the config's context; a take burns the taker's protocol tokens for the filled stablecoin, a
//!   close after the TTL sweeps the vault back to the buffer and books what it returned.
//!
//! `harness::invariants::check` after every send.

use auction_engine_params::{EngineError, args::EngineParamsArgs};
use engine_core::{check::fail_text, state::ConfigState, system_state::SystemPhase};
use harness::{
    World, expect_revert,
    impostor::{encode, script},
    invariants,
    world::{RAY, SLACK_MS},
};
use lee::{AccountId, Actor, error::LeeError};
use lee_core::program::{StateReply, TransactionEvent};
use token_core::Definition;

/// The protocol token's genesis supply, held by the taker.
const GENESIS: u128 = 10_000;
/// The chain time the tests start at, ms.
const T0: u64 = 1_000_000;
/// `EngineParamsArgs::defaults_for_tests().system_ttl_ms`.
const TTL: u64 = 14_400_000;

type Sent = Result<Vec<TransactionEvent>, LeeError>;

/// The world's engine parameters with the protocol token enabled.
fn params() -> EngineParamsArgs {
    EngineParamsArgs {
        protocol_token_enabled: true,
        ..World::engine_params()
    }
}

/// `World::with_system(p)` at `T0`.
fn world_with(p: EngineParamsArgs) -> World {
    let mut w = World::with_system(p);
    w.set_now_ms(T0);
    invariants::check(&w);
    w
}

fn world() -> World {
    world_with(params())
}

fn checked(w: &World, sent: Sent) -> Sent {
    invariants::check(w);
    sent
}

fn start(w: &mut World, nonce: u64) -> Sent {
    let sent = w.start_surplus(nonce);
    checked(w, sent)
}

fn take(w: &mut World, nonce: u64, amount_coin: u128, max_protocol_in: u128) -> Sent {
    let sent = w.take_surplus(nonce, amount_coin, max_protocol_in);
    checked(w, sent)
}

fn close(w: &mut World, nonce: u64) -> Sent {
    let sent = w.close_surplus(nonce);
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

fn buffer_balance(w: &World) -> u128 {
    w.stable_balance(w.buffer().account_id)
}

fn vault_balance(w: &World, nonce: u64) -> u128 {
    w.stable_balance(w.surplus_vault(nonce).account_id)
}

fn taker(w: &mut World) -> AccountId {
    w.chain.key("taker").1
}

fn protocol_balance(w: &mut World) -> u128 {
    let (taker, def) = (taker(w), w.protocol_def());
    w.balance(taker, def)
}

fn supply(w: &World, definition: AccountId) -> u128 {
    w.chain
        .raw_state(Actor::new(definition, w.token))
        .and_then(|bytes| Definition::decode(&bytes))
        .expect("a token definition")
        .supply
}

#[test]
fn surplus_auction_burns_protocol_tokens() {
    let mut w = world();
    donate_buffer(&mut w, 1_500);
    start(&mut w, 0).expect("starts");
    let s = w.surplus_state(0);
    assert_eq!(
        (s.lot_left, s.phase, s.ref_ray, s.close),
        (500, SystemPhase::Live, 2 * RAY, None),
        "admin price, no oracle"
    );
    assert_eq!((buffer_balance(&w), vault_balance(&w, 0)), (1_000, 500));
    let a = w.accounting_state();
    assert_eq!(
        (a.surplus_live, a.surplus_nonce, a.total_surplus_lots_out),
        (true, 1, 500)
    );
    // tokens = ceil(ceil(500 x 1.05) x RAY / 2 RAY) = ceil(525 / 2) = 263, burned.
    take(&mut w, 0, 500, 263).expect("takes the lot");
    let def = w.protocol_def();
    assert_eq!(protocol_balance(&mut w), GENESIS - 263);
    assert_eq!(supply(&w, def), GENESIS - 263);
    let taker = taker(&mut w);
    assert_eq!(w.stable_balance(taker), 500);
    assert_eq!(vault_balance(&w, 0), 0);
    let a = w.accounting_state();
    assert_eq!(
        (
            a.surplus_live,
            a.total_protocol_burned,
            a.last_surplus_end_ms
        ),
        (false, 263, T0)
    );
    assert_eq!(w.surplus_state(0).phase, SystemPhase::Ended);
}

#[test]
fn partial_take_dust_and_slippage() {
    let mut w = world();
    donate_buffer(&mut w, 1_500);
    start(&mut w, 0).expect("starts");
    expect_err(take(&mut w, 0, 495, u128::MAX), EngineError::DustRemainder);
    expect_err(take(&mut w, 0, 250, 1), EngineError::Slippage);
    expect_err(take(&mut w, 0, 0, u128::MAX), EngineError::ZeroFill);
    take(&mut w, 0, 250, u128::MAX).expect("a partial take");
    let s = w.surplus_state(0);
    assert_eq!((s.lot_left, s.phase), (250, SystemPhase::Live));
    // 250 coin: ceil(250 x 1.05) = 263 (262.5 up), then ceil(263 / 2) = 132 tokens.
    assert_eq!(protocol_balance(&mut w), GENESIS - 132);
    assert_eq!(vault_balance(&w, 0), 250);
    let a = w.accounting_state();
    assert_eq!(a.total_protocol_burned, 132);
    assert!(a.surplus_live, "a partial take leaves the auction live");
    // v0.2.4 `take_surplus_max_amount_fills_lot`: only the rest of the lot moves.
    take(&mut w, 0, u128::MAX, u128::MAX).expect("fills the rest");
    let taker = taker(&mut w);
    assert_eq!(w.stable_balance(taker), 500);
    assert_eq!((buffer_balance(&w), vault_balance(&w, 0)), (1_000, 0));
    assert_eq!(protocol_balance(&mut w), GENESIS - 132 - 132);
    assert!(!w.accounting_state().surplus_live);
}

#[test]
fn start_rejections() {
    let mut w = World::new();
    donate_buffer(&mut w, 1_500);
    expect_err(start(&mut w, 0), EngineError::TokenNotConfigured);

    let mut w = world();
    donate_buffer(&mut w, 1_499);
    expect_err(start(&mut w, 0), EngineError::BufferBelowTarget);
    donate_buffer(&mut w, 1);
    expect_err(start(&mut w, 1), EngineError::NonceMismatch);
    start(&mut w, 0).expect("starts");
    donate_buffer(&mut w, 1_500);
    expect_err(start(&mut w, 1), EngineError::SurplusAuctionAlreadyLive);
    // An ended auction's nonce is spent.
    take(&mut w, 0, u128::MAX, u128::MAX).expect("takes the lot");
    expect_err(start(&mut w, 0), EngineError::NonceMismatch);
    start(&mut w, 1).expect("the next nonce starts");
    assert_eq!(w.accounting_state().surplus_nonce, 2);
}

#[test]
fn restart_waits_for_the_configured_delay() {
    let mut w = world_with(EngineParamsArgs {
        surplus_delay_ms: 60_000,
        ..params()
    });
    donate_buffer(&mut w, 3_000);
    start(&mut w, 0).expect("starts");
    take(&mut w, 0, u128::MAX, u128::MAX).expect("takes the lot");
    expect_err(start(&mut w, 1), EngineError::DelayNotElapsed);
    w.set_now_ms(T0 + 59_999);
    expect_err(start(&mut w, 1), EngineError::DelayNotElapsed);
    w.set_now_ms(T0 + 60_000);
    start(&mut w, 1).expect("delay elapsed");
}

#[test]
fn implicit_settle_burns_the_debt_before_the_lot_moves() {
    let mut w = world();
    let debt = w.book_shortfall();
    invariants::check(&w);
    assert_eq!(debt, 10_000);
    // v0.2.4 `implicit_settle_runs_first_and_unbacked_debt_blocks_start`.
    donate_buffer(&mut w, 1_500);
    expect_err(start(&mut w, 0), EngineError::UnbackedDebtPresent);
    assert_eq!(
        buffer_balance(&w),
        1_500,
        "the implicit burn reverted with the transaction"
    );
    // The buffer covers the booked shortfall first; the remainder funds the lot.
    donate_buffer(&mut w, debt);
    let stable = supply(&w, w.stable_def);
    start(&mut w, 0).expect("starts after the implicit settle");
    let a = w.accounting_state();
    assert_eq!((a.unbacked_debt, a.total_debt_settled), (0, debt));
    assert_eq!((buffer_balance(&w), vault_balance(&w, 0)), (1_000, 500));
    assert_eq!(supply(&w, w.stable_def), stable - debt);
}

#[test]
fn close_after_ttl_returns_the_remainder() {
    let mut w = world();
    donate_buffer(&mut w, 1_500);
    start(&mut w, 0).expect("starts");
    take(&mut w, 0, 250, u128::MAX).expect("a partial take");
    w.set_now_ms(T0 + TTL - 1);
    expect_err(close(&mut w, 0), EngineError::DelayNotElapsed);
    w.set_now_ms(T0 + TTL);
    close(&mut w, 0).expect("closes");
    assert_eq!((buffer_balance(&w), vault_balance(&w, 0)), (1_250, 0));
    let a = w.accounting_state();
    assert_eq!(
        (
            a.surplus_live,
            a.total_surplus_returned,
            a.last_surplus_end_ms
        ),
        (false, 250, T0 + TTL)
    );
    let s = w.surplus_state(0);
    assert_eq!(
        (s.phase, s.lot_left, s.close),
        (SystemPhase::Ended, 0, None)
    );
    expect_err(close(&mut w, 0), EngineError::NotLive);
    expect_err(take(&mut w, 0, 250, u128::MAX), EngineError::NotLive);
}

/// spec 5.11 / ADR-017: a valid reference reading overrides the admin price 2.0 within ±20 %;
/// the age is measured against `now + slack` (ADR-058).
#[test]
fn reference_oracle_overrides_within_twenty_percent() {
    let max_age = World::engine_params().ref_oracle_max_age_ms;
    let oldest = max_age - SLACK_MS;
    for (price, age, expected) in [
        (3 * RAY, 0, 24 * RAY / 10),
        (19 * RAY / 10, 0, 19 * RAY / 10),
        (3 * RAY, oldest, 24 * RAY / 10),
        (3 * RAY, oldest + 1, 2 * RAY),
        (3 * RAY, max_age + 1, 2 * RAY),
    ] {
        let mut w = world_with(EngineParamsArgs {
            ref_oracle_enabled: true,
            ..params()
        });
        let now = 4_000_000;
        w.set_now_ms(now);
        w.set_ref_price(price, now - age);
        donate_buffer(&mut w, 1_500);
        start(&mut w, 0).expect("starts");
        assert_eq!(
            w.surplus_state(0).ref_ray,
            expected,
            "oracle {price} age {age}"
        );
    }
}

/// Review Focus 3: a configured reference oracle that never published (an oracle state at zero),
/// an oracle actor with no state at all (an empty reply), and a foreign actor whose reply is not
/// an oracle state all price at the admin price, never a panic.
#[test]
fn unpublished_reference_oracle_falls_back_to_the_admin_price() {
    let enabled = EngineParamsArgs {
        ref_oracle_enabled: true,
        ..params()
    };
    let mut w = world_with(enabled.clone());
    let oracle = w.config_state().protocol_token_oracle.expect("configured");
    assert_eq!(
        w.chain
            .state::<oracle_core::OracleState>(oracle)
            .map(|o| o.price_ray),
        Some(0)
    );
    donate_buffer(&mut w, 1_500);
    start(&mut w, 0).expect("starts");
    assert_eq!(w.surplus_state(0).ref_ray, 2 * RAY, "never published");

    let mut w = World::bare();
    let (_, account) = w.chain.key("stateless-oracle");
    w.chain.fund(account, 1);
    let stateless = Actor::new(account, w.oracle.program_account_id);
    w.init_system(enabled.clone(), Some(stateless));
    assert_eq!(w.chain.raw_state(stateless), None);
    donate_buffer(&mut w, 1_500);
    start(&mut w, 0).expect("starts");
    assert_eq!(w.surplus_state(0).ref_ray, 2 * RAY, "an empty reply");

    // An impostor whose reply carries a state that is not an oracle's.
    let mut w = World::bare();
    let reply = StateReply {
        state: b"NOT AN ORACLE STATE".to_vec().into(),
    };
    let impostor = w.impostor_with_state("ref-impostor", script(&[(w.accounting, encode(&reply))]));
    w.init_system(enabled, Some(impostor));
    donate_buffer(&mut w, 1_500);
    start(&mut w, 0).expect("starts");
    assert_eq!(w.surplus_state(0).ref_ray, 2 * RAY, "a foreign state");
}

/// Review Focus 2: a donation to the predictable vault of nonce 0 before the start stays there;
/// a take moves only the filled amount; the close returns the whole balance and books it.
#[test]
fn donation_to_a_future_surplus_vault_is_returned_and_booked() {
    let mut w = world();
    let vault = w.surplus_vault(0).account_id;
    w.donate_stable(vault, 7);
    invariants::check(&w);
    donate_buffer(&mut w, 1_500);
    start(&mut w, 0).expect("starts");
    assert_eq!(vault_balance(&w, 0), 507);
    take(&mut w, 0, 250, u128::MAX).expect("a partial take");
    let taker = taker(&mut w);
    assert_eq!((w.stable_balance(taker), vault_balance(&w, 0)), (250, 257));
    w.set_now_ms(T0 + TTL);
    close(&mut w, 0).expect("closes");
    assert_eq!((buffer_balance(&w), vault_balance(&w, 0)), (1_000 + 257, 0));
    assert_eq!(w.accounting_state().total_surplus_returned, 257);
}

#[test]
fn take_on_an_absent_surplus_reverts_with_engine_error() {
    let mut w = world();
    donate_buffer(&mut w, 1_500);
    let before = w.chain.raw_state(w.accounting);
    let absent = EngineError::WrongAccount("surplus_auction");
    expect_err(take(&mut w, 0, 500, u128::MAX), absent);
    expect_err(close(&mut w, 0), absent);
    assert_eq!(w.chain.raw_state(w.accounting), before);
    start(&mut w, 0).expect("starts");
    expect_err(take(&mut w, 1, 500, u128::MAX), absent);
    assert_eq!(
        w.surplus_state(0).lot_left,
        500,
        "the live auction is untouched"
    );
}

/// `AssertSystemIdle` end to end: a locked field cannot change while a surplus auction is live;
/// an unlocked one can, and the locked one can again once the auction ended.
#[test]
fn locked_fields_revert_while_a_surplus_auction_is_live() {
    let mut w = world();
    donate_buffer(&mut w, 1_500);
    start(&mut w, 0).expect("starts");
    let key = w.admin_key.clone();
    let lot = w.set_engine_params_msg(
        EngineParamsArgs {
            surplus_lot: 400,
            ..params()
        },
        None,
    );
    let sent = w.send_engine(w.admin, &[&key], &lot);
    expect_err(checked(&w, sent), EngineError::ProtocolTokenLocked);
    assert_eq!(w.config_state().system.surplus_lot, 500);
    let cap = w.set_engine_params_msg(
        EngineParamsArgs {
            global_cap: 9_000_000,
            ..params()
        },
        None,
    );
    let sent = w.send_engine(w.admin, &[&key], &cap);
    checked(&w, sent).expect("global_cap is not locked");
    assert_eq!(w.config_state().global_cap, 9_000_000);
    take(&mut w, 0, u128::MAX, u128::MAX).expect("takes the lot");
    let sent = w.send_engine(w.admin, &[&key], &lot);
    checked(&w, sent).expect("no system auction is live");
    let ConfigState { system, .. } = w.config_state();
    assert_eq!(system.surplus_lot, 400);
}
