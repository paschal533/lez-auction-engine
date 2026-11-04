//! Security tests for the surplus auction's authorization surface and the system Read block's
//! reference-oracle read (design note §4.3 c; routed from the Task 3 and Task 4 reviews). Split
//! from `security_system.rs` by ruling R-7a.
//!
//! - a `StateReply` to a closing surplus auction from an actor that is not its vault holding;
//! - a take priced at the reference price snapshotted at start, not a later oracle value;
//! - `TakeSurplus` on a config without a protocol token;
//! - `NonceMismatch` raised before any read is issued;
//! - `System` / a system root sent from inside a read window;
//! - an undecodable reference reply, and a reference oracle that replies twice.
//!
//! A reference oracle the attacker controls is an `impostor` whose script runs when the
//! accounting's `ReadState` reaches it. Every revert names the exact error text; the victim's
//! states are compared byte for byte, and `harness::invariants::check` runs on every world.

use auction_engine_params::{EngineError, args::EngineParamsArgs};
use engine_core::{
    SystemCtx, SystemOp,
    actors::{accounting_actor, buffer_holding, surplus_actor, surplus_vault_holding},
    check::fail_text,
    msg::EngineMsg,
    system_state::{SurplusAuctionState, SystemPhase},
};
use harness::{
    World, expect_revert,
    impostor::{encode, script},
    invariants,
    system::{TAKER, TAKER_GENESIS},
    world::RAY,
};
use lee::{AccountId, Actor};
use lee_core::program::StateReply;
use token_core::Holding;

/// The chain time the tests start at, ms.
const T0: u64 = 1_000_000;
/// `EngineParamsArgs::defaults_for_tests().system_ttl_ms`.
const TTL: u64 = 14_400_000;

fn token_params() -> EngineParamsArgs {
    EngineParamsArgs {
        protocol_token_enabled: true,
        ..World::engine_params()
    }
}

fn ref_params() -> EngineParamsArgs {
    EngineParamsArgs {
        ref_oracle_enabled: true,
        ..token_params()
    }
}

fn err(e: EngineError) -> String {
    fail_text(e)
}

fn states(w: &World, actors: &[Actor]) -> Vec<Option<Vec<u8>>> {
    actors
        .iter()
        .map(|actor| w.chain.raw_state(*actor))
        .collect()
}

fn donate_buffer(w: &mut World, amount: u128) {
    let buffer = w.buffer().account_id;
    w.donate_stable(buffer, amount);
    invariants::check(w);
}

/// A `StateReply` carrying a token holding of `owner`'s stablecoin with `balance`.
fn holding_reply(w: &World, owner: AccountId, balance: u128) -> Vec<u8> {
    let holding = Holding {
        balance,
        ..Holding::new(owner, w.stable_def)
    };
    encode(&StateReply {
        state: encode(&holding).into(),
    })
}

/// A token world at `T0` whose reference oracle is an impostor at `"ref-impostor"` that, on any
/// message, sends what `script_of(&world)` lists; 1_500 in the buffer. The script can name the
/// world's actors because they are derived before `Init`.
fn impostor_oracle_world(script_of: impl Fn(&World) -> Vec<(Actor, Vec<u8>)>) -> (World, Actor) {
    let mut w = World::bare();
    let forwards = script_of(&w);
    let oracle = w.impostor_with_state("ref-impostor", script(&forwards));
    w.init_system(ref_params(), Some(oracle));
    w.set_now_ms(T0);
    donate_buffer(&mut w, 1_500);
    (w, oracle)
}

/// Routed 4: inside a close's read window, a `StateReply` to the closing surplus auction from an
/// actor other than its vault holding is not taken as the vault's reading: the auction leaves it
/// to the dispatch (`engine: BadMessage`) and the close reverts. No attacker code runs in a close
/// of a real-token config, so the window is opened on the attacker's own config `K'` whose token
/// program is the impostor: its vault holding forwards to an accomplice, which replies to `S'`.
/// When only the vault holding replies, the same close goes through.
#[test]
fn state_reply_to_a_closing_surplus_from_another_actor_reverts() {
    let mut w = World::new();
    w.set_now_ms(T0);
    let victim = [w.config, w.accounting, w.collateral, w.params];
    let before = states(&w, &victim);
    let rogue = w.impostor_token_config("attacker", token_params());
    let (e, k, imp) = (w.engine, rogue.account_id, w.chain.program("impostor"));
    let acc = accounting_actor(e, k);
    // K' starts its surplus auction 0 on a scripted 1_500 buffer reading.
    let buffer = buffer_holding(e, imp, k);
    let buffer_reads = w.impostor_holding(buffer.account_id);
    let reading = holding_reply(&w, buffer.account_id, 1_500);
    w.impostor_at(buffer_reads.account_id, Some(script(&[(acc, reading)])));
    w.impostor_at(buffer.account_id, None);
    let s = surplus_actor(e, k, 0);
    let start = EngineMsg::StartSurplus {
        config: k,
        nonce: 0,
        now_ms: T0,
    };
    let actors = [rogue, acc, buffer_reads, buffer, w.chain.clock(), s];
    w.chain
        .send(rogue, &actors, &[], &start)
        .expect("K' starts its surplus auction");
    let live = w.chain.raw_state(s);

    w.set_now_ms(T0 + TTL);
    let vault = surplus_vault_holding(e, imp, s.account_id).account_id;
    let vault_reads = w.impostor_holding(vault);
    let empty = holding_reply(&w, vault, 0);
    let accomplice = w.impostor_with_state("accomplice", script(&[(s, empty.clone())]));
    let forged = script(&[(accomplice, Vec::new()), (s, empty.clone())]);
    w.impostor_at(vault_reads.account_id, Some(forged));
    let close = EngineMsg::CloseSurplus {
        config: k,
        nonce: 0,
        now_ms: T0 + TTL,
    };
    let actors = [rogue, w.chain.clock(), s, vault_reads, accomplice, acc];
    expect_revert(
        w.chain.send(rogue, &actors, &[], &close),
        "engine: BadMessage",
    );
    assert_eq!(
        w.chain.raw_state(s),
        live,
        "S' is still live, no close scratch"
    );

    w.impostor_at(vault_reads.account_id, Some(script(&[(s, empty)])));
    w.chain
        .send(rogue, &actors, &[], &close)
        .expect("the vault holding's own reply closes S'");
    let closed = w
        .chain
        .raw_state(s)
        .and_then(|bytes| SurplusAuctionState::decode(&bytes))
        .expect("S' state");
    assert_eq!((closed.phase, closed.close), (SystemPhase::Ended, None));
    assert_eq!(states(&w, &victim), before);
    invariants::check(&w);
}

/// Routed 7: a take is priced at the `ref_ray` snapshotted at start from the reference oracle
/// (3 RAY clipped to 2.4 RAY: 500 coin cost `ceil(ceil(500 × 1.05) / 2.4) = 219` tokens), not at
/// a later oracle value (1.6 RAY would cost 329).
#[test]
fn take_is_priced_at_the_snapshot_ref_ray() {
    let mut w = World::with_system(ref_params());
    w.set_now_ms(T0);
    w.set_ref_price(3 * RAY, T0);
    donate_buffer(&mut w, 1_500);
    w.start_surplus(0).expect("starts");
    assert_eq!(w.surplus_state(0).ref_ray, 24 * RAY / 10);
    // A later reading (chain time stays at the start, so the Dutch price has not decayed).
    w.set_ref_price(16 * RAY / 10, T0 + 1);
    invariants::check(&w);
    expect_revert(w.take_surplus(0, 500, 218), &err(EngineError::Slippage));
    w.take_surplus(0, 500, 219).expect("219 tokens buy the lot");
    let (taker, def) = (w.chain.key(TAKER).1, w.protocol_def());
    assert_eq!(w.balance(taker, def), TAKER_GENESIS - 219);
    assert_eq!(w.accounting_state().total_protocol_burned, 219);
    assert_eq!(w.surplus_state(0).ref_ray, 24 * RAY / 10);
    invariants::check(&w);
}

/// Routed 8: `TakeSurplus` (and `TakeDebt`) at a config without a protocol token fails
/// `TokenNotConfigured` at the config, before any probe; nothing moves.
#[test]
fn take_surplus_without_protocol_token_reverts() {
    let mut w = World::new();
    w.set_now_ms(T0);
    let victim = [w.config, w.accounting];
    let before = states(&w, &victim);
    let none = err(EngineError::TokenNotConfigured);
    expect_revert(w.take_surplus(0, 500, u128::MAX), &none);
    expect_revert(w.take_debt(0, 500, 0), &none);
    assert_eq!(states(&w, &victim), before);
    invariants::check(&w);
}

/// Routed 9: `NonceMismatch` is raised at `System`, before the reads go out. The reference oracle
/// is an impostor that answers any read with a forged `Mint` to the config
/// (`WrongAccount("debt_auction")`): a start with the right nonce reaches it, so a start with the
/// wrong nonce that fails `NonceMismatch` issued no read. Likewise for the debt auction.
#[test]
fn nonce_mismatch_is_raised_before_any_read() {
    let (mut w, _) = impostor_oracle_world(|w| {
        let mint = EngineMsg::Mint {
            nonce: 0,
            to: w.admin,
            amount: 1,
        };
        vec![(w.config, encode(&mint))]
    });
    let books = w.chain.raw_state(w.accounting);
    expect_revert(w.start_surplus(1), &err(EngineError::NonceMismatch));
    expect_revert(
        w.start_surplus(0),
        &err(EngineError::WrongAccount("debt_auction")),
    );
    assert_eq!(w.book_shortfall(), 10_000);
    let books_after = w.chain.raw_state(w.accounting);
    expect_revert(w.start_debt(1), &err(EngineError::NonceMismatch));
    expect_revert(
        w.start_debt(0),
        &err(EngineError::WrongAccount("debt_auction")),
    );
    assert_eq!(w.chain.raw_state(w.accounting), books_after);
    assert_ne!(books, books_after, "only the shortfall was booked");
    invariants::check(&w);
}

/// Routed 12: inside a read window (after the oracle's reply, before `Proceed`) an accomplice of
/// the attacker's reference oracle sends `System` to the accounting, refused on its sender
/// (`WrongAccount("accounting")`); and the oracle sends a system root to the config
/// (`WrongAccount("root message expected")`). The start reverts and the accounting is untouched.
/// Any message from the configured oracle itself is its one reading (an undecodable one prices
/// at the admin price), so `System` must come from another actor to reach the dispatch. (The
/// `pending.is_some()` guard needs a second `System` from the config in one transaction; the
/// config sends `System` only for its root, so no transaction reaches it.)
#[test]
fn system_during_a_read_window_reverts() {
    let (mut w, oracle) = impostor_oracle_world(|_| Vec::new());
    let (k, acc) = (w.config.account_id, w.accounting);
    let system = EngineMsg::System {
        op: SystemOp::SettleDebt { amount: u128::MAX },
        now_ms: T0,
        ctx: Box::new(SystemCtx::from_config(k, &w.config_state())),
    };
    let root = EngineMsg::SettleDebt {
        config: k,
        amount: u128::MAX,
    };
    let reply = encode(&StateReply {
        state: Vec::new().into(),
    });
    let accomplice = w.impostor_with_state("accomplice", script(&[(acc, encode(&system))]));
    let books = w.chain.raw_state(acc);
    let cases = [
        (
            vec![(acc, reply), (accomplice, Vec::new())],
            EngineError::WrongAccount("accounting"),
        ),
        (
            vec![(w.config, encode(&root))],
            EngineError::WrongAccount("root message expected"),
        ),
    ];
    for (forwards, expected) in cases {
        w.impostor_at(oracle.account_id, Some(script(&forwards)));
        let msg = EngineMsg::StartSurplus {
            config: k,
            nonce: 0,
            now_ms: T0,
        };
        let mut actors = w.system_actors(&msg);
        actors.push(accomplice);
        let evidence = w.buffer_evidence();
        let root_at = w.config;
        let sent = w
            .chain
            .send_with_evidence(root_at, &actors, &[], &evidence, &msg);
        expect_revert(sent, &err(expected));
        assert_eq!(w.chain.raw_state(acc), books);
    }
    invariants::check(&w);
}

/// Routed 13: a reference oracle that replies twice fails `WrongAccount("pending")`; a reply that
/// is not even a `StateReply` is `Undecodable` and prices at the admin price 2 RAY.
#[test]
fn reference_oracle_double_and_undecodable_replies() {
    let (mut w, oracle) = impostor_oracle_world(|_| Vec::new());
    let acc = w.accounting;
    let reply = encode(&StateReply {
        state: Vec::new().into(),
    });
    w.impostor_at(
        oracle.account_id,
        Some(script(&[(acc, reply.clone()), (acc, reply)])),
    );
    let books = w.chain.raw_state(acc);
    expect_revert(
        w.start_surplus(0),
        &err(EngineError::WrongAccount("pending")),
    );
    assert_eq!(w.chain.raw_state(acc), books);
    let garbage = vec![0xFF, 0x01, 0x02];
    w.impostor_at(oracle.account_id, Some(script(&[(acc, garbage)])));
    w.start_surplus(0)
        .expect("an undecodable reply prices at the admin price");
    assert_eq!(w.surplus_state(0).ref_ray, 2 * RAY);
    invariants::check(&w);
}
