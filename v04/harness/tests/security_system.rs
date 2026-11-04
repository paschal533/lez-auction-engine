//! Security tests in actor form for the system messages (design note §4.3 c): `settle_debt`, the
//! surplus and debt auctions and `check_solvency`/`Freeze` (rows 9-16).
//!
//! The attacker's code is the `impostor` guest (`harness::impostor`). The tests cover:
//! - guard 3, the program half of every sender check: each internal system message forged from
//!   `(K, Impostor)`, `(Acc, Impostor)`, `(S, Impostor)` and `(D, Impostor)`;
//! - guard 2: the same messages sent as root messages;
//! - the account half: an attacker's own config `K'` of the same engine runs every system root,
//!   and every message lands on the actors `K'` derives, never on the victim's;
//! - the Read blocks: a `StateReply` outside a pending read, a second reply from the holding the
//!   accounting asked (on a config whose token program the attacker scripts), reports for a nonce
//!   the sender is not, and token `Debit`s of an engine vault from anyone but its owner actor.
//!
//! `security_surplus.rs` holds the surplus- and reference-oracle-specific cases (ruling R-7a).
//! Every revert names the exact error text; the victim's states are compared byte for byte, and
//! `harness::invariants::check` runs on every world.

use auction_engine_params::{EngineError, args::EngineParamsArgs};
use engine_core::{
    SystemCtx, SystemOp,
    actors::{accounting_actor, auction_actor, buffer_holding, collateral_actor, config_actor},
    check::fail_text,
    msg::{EngineMsg, ProtocolGenesis},
    state::AccountingState,
};
use harness::{
    World, expect_revert,
    impostor::{encode, script},
    invariants,
    system::{TAKER, TAKER_GENESIS},
    world::{Forward, SALT},
};
use lee::{AccountId, Actor, PrivateKey};
use lee_core::program::StateReply;
use token_core::{DebitAction, Definition, TokenMsg};

/// The chain time the tests start at, ms.
const T0: u64 = 1_000_000;
/// `EngineParamsArgs::defaults_for_tests().system_ttl_ms`.
const TTL: u64 = 14_400_000;
/// The shortfall `World::book_shortfall` books.
const VICTIM_DEBT: u128 = 10_000;
/// The rogue config's own shortfall (a 3 / 7_000 scripted position settled empty).
const ROGUE_DEBT: u128 = 7_000;
/// What the rogue buffer holds beyond its debt: the surplus target 1_000 plus a 500 lot.
const ROGUE_SURPLUS: u128 = 1_500;

fn token_params() -> EngineParamsArgs {
    EngineParamsArgs {
        protocol_token_enabled: true,
        ..World::engine_params()
    }
}

fn wrong(name: &'static str) -> String {
    fail_text(EngineError::WrongAccount(name))
}

fn stable_supply(w: &World) -> u128 {
    w.chain
        .state::<Definition>(Actor::new(w.stable_def, w.token))
        .expect("the stablecoin definition")
        .supply
}

fn states(w: &World, actors: &[Actor]) -> Vec<Option<Vec<u8>>> {
    actors
        .iter()
        .map(|actor| w.chain.raw_state(*actor))
        .collect()
}

/// A token world at `T0` with surplus auction 0 live (lot 500, buffer 1_000).
fn surplus_live() -> World {
    let mut w = World::with_system(token_params());
    w.set_now_ms(T0);
    let buffer = w.buffer().account_id;
    w.donate_stable(buffer, 1_500);
    w.start_surplus(0).expect("surplus auction 0 starts");
    invariants::check(&w);
    w
}

/// [`surplus_live`], then the lot-3 shortfall booked and debt auction 0 started: its implicit
/// settle burns the buffer's 1_000, leaving 9_000 unbacked, 500 of it on auction. Both live.
fn both_live() -> World {
    let mut w = surplus_live();
    assert_eq!(w.book_shortfall(), VICTIM_DEBT);
    w.start_debt(0).expect("debt auction 0 starts");
    let a = w.accounting_state();
    assert!(a.surplus_live && a.debt_live, "both auctions live");
    invariants::check(&w);
    w
}

/// Every victim state a forged system message could aim at: config, accounting, collateral,
/// surplus and debt auction 0, the surplus vault's and the buffer's stablecoin, the protocol and
/// stablecoin definitions, the taker's protocol holding and the host's params.
fn victim_actors(w: &mut World) -> Vec<Actor> {
    let (sdef, pdef) = (w.stable_def, w.protocol_def());
    let taker = w.chain.key(TAKER).1;
    vec![
        w.config,
        w.accounting,
        w.collateral,
        w.surplus(0),
        w.debt(0),
        w.holding(w.surplus_vault(0).account_id, sdef),
        w.holding(w.buffer().account_id, sdef),
        Actor::new(pdef, w.token),
        Actor::new(sdef, w.token),
        w.holding(taker, pdef),
        w.params,
    ]
}

/// The fifteen internal system messages an attacker would forge against the victim, each at the
/// receiver it is meant for, with a payload that would hurt if accepted, and the error its
/// receiver refuses any sender but the right one with.
fn forged(w: &World, attacker: AccountId) -> Vec<(Actor, EngineMsg, String)> {
    let k = w.config.account_id;
    let ctx = || Box::new(SystemCtx::from_config(k, &w.config_state()));
    let (acc, s, d) = (w.accounting, w.surplus(0), w.debt(0));
    let big = u128::MAX / 2;
    vec![
        // Burn the whole buffer.
        (
            acc,
            EngineMsg::System {
                op: SystemOp::SettleDebt { amount: u128::MAX },
                now_ms: T0,
                ctx: ctx(),
            },
            wrong("accounting"),
        ),
        // Rewrite the live auctions with huge lots.
        (
            s,
            EngineMsg::SurplusOpen {
                config: k,
                nonce: 0,
                lot: big,
                start_ms: T0,
                ref_ray: 1,
            },
            wrong("surplus_auction"),
        ),
        (
            d,
            EngineMsg::DebtOpen {
                config: k,
                nonce: 0,
                lot: big,
                start_amount: big,
                start_ms: T0,
            },
            wrong("debt_auction"),
        ),
        // Take or close without the config.
        (
            s,
            EngineMsg::SurplusTake {
                taker: attacker,
                amount_coin: u128::MAX,
                max_protocol_in: u128::MAX,
                now_ms: T0,
                ctx: ctx(),
            },
            wrong("config"),
        ),
        (
            s,
            EngineMsg::SurplusClose {
                now_ms: T0 + TTL,
                ctx: ctx(),
            },
            wrong("config"),
        ),
        (
            d,
            EngineMsg::DebtTake {
                taker: attacker,
                amount_coin: u128::MAX,
                min_protocol_out: 0,
                now_ms: T0,
                ctx: ctx(),
            },
            wrong("config"),
        ),
        (
            d,
            EngineMsg::DebtClose {
                now_ms: T0 + TTL,
                ctx: ctx(),
            },
            wrong("config"),
        ),
        // Book reports that never happened (routed 5: an ended take with a huge burn).
        (
            acc,
            EngineMsg::SurplusTaken {
                config: k,
                nonce: 0,
                protocol_burned: big,
                ended: true,
                now_ms: T0,
            },
            wrong("surplus_auction"),
        ),
        (
            acc,
            EngineMsg::SurplusReturned {
                config: k,
                nonce: 0,
                amount: big,
                now_ms: T0,
            },
            wrong("surplus_auction"),
        ),
        (
            acc,
            EngineMsg::DebtTaken {
                config: k,
                nonce: 0,
                filled: 500,
                minted: 0,
                ended: true,
                now_ms: T0,
            },
            wrong("debt_auction"),
        ),
        (
            acc,
            EngineMsg::DebtClosed {
                config: k,
                nonce: 0,
                lot_left: 500,
                now_ms: T0,
            },
            wrong("debt_auction"),
        ),
        // Mint protocol tokens to the attacker; freeze the host.
        (
            w.config,
            EngineMsg::Mint {
                nonce: 0,
                to: attacker,
                amount: big,
            },
            wrong("debt_auction"),
        ),
        (w.config, EngineMsg::Freeze, wrong("accounting")),
        // The admin's leaf assertions.
        (
            w.collateral,
            EngineMsg::AssertCapAtMost { global_cap: 0 },
            fail_text(EngineError::NotAdmin),
        ),
        (
            acc,
            EngineMsg::AssertSystemIdle,
            fail_text(EngineError::NotAdmin),
        ),
    ]
}

/// Guard 3, program half: impostors at the config's, the accounting's and both live auctions'
/// accounts (right account, wrong program) send every internal system message to its receiver;
/// each reverts with the receiver's sender error and no victim state moves (routed 1, 3, 5).
#[test]
fn impostor_at_engine_accounts_cannot_send_system_internals() {
    let mut w = both_live();
    let attacker = w.chain.key("attacker").1;
    let victim = victim_actors(&mut w);
    let before = states(&w, &victim);
    let cases = forged(&w, attacker);
    let senders =
        [w.config, w.accounting, w.surplus(0), w.debt(0)].map(|a| w.impostor(a.account_id));
    for from in senders {
        for (to, msg, needle) in &cases {
            expect_revert(w.speak(from, *to, msg), needle);
        }
    }
    assert_eq!(states(&w, &victim), before, "no forged message moved state");
    invariants::check(&w);
}

/// Guard 2: the same messages as the transaction's root at their receivers have no sender and
/// are refused — among them `System` at `Acc`, `SurplusTaken` at `Acc`, `Mint` and `Freeze` at
/// `K`, and `SurplusTake` at `S` (`from == None`).
#[test]
fn internal_system_messages_sent_as_root_revert() {
    let mut w = both_live();
    let attacker = w.chain.key("attacker").1;
    let victim = victim_actors(&mut w);
    let before = states(&w, &victim);
    for (to, msg, needle) in forged(&w, attacker) {
        expect_revert(w.chain.send(to, &[to], &[], &msg), &needle);
    }
    assert_eq!(states(&w, &victim), before);
    invariants::check(&w);
}

/// The victim: its shortfall booked, debt auction 0 live, 800 in its buffer.
fn victim_with_debt_auction() -> World {
    let mut w = World::with_system(token_params());
    w.set_now_ms(T0);
    assert_eq!(w.book_shortfall(), VICTIM_DEBT);
    w.start_debt(0).expect("the victim's debt auction 0");
    let buffer = w.buffer().account_id;
    w.donate_stable(buffer, 800);
    invariants::check(&w);
    w
}

/// The attacker's own config `K'` of the engine, with its own protocol token (genesis to the
/// attacker) and an impostor host, books its own `ROGUE_DEBT` shortfall: a scripted 3 / 7_000
/// position liquidated into `K'`'s auction and settled empty. Returns the attacker's key and `K'`.
fn rogue_with_shortfall(w: &mut World) -> (PrivateKey, AccountId, Actor) {
    let (key, attacker) = w.chain.key("attacker");
    w.chain.fund(attacker, 1);
    let (e, def) = (w.engine, w.collateral_def);
    let rogue = config_actor(e, attacker, SALT).account_id;
    let position = w.scripted_position("rogue-position", rogue, 3, ROGUE_DEBT);
    let host = w.impostor_host("rogue-host", Some(position));
    let genesis = ProtocolGenesis {
        to: attacker,
        supply: TAKER_GENESIS,
    };
    let init = w.init_msg_full(token_params(), None, Some(genesis));
    let config = w.configure_init_with_host(attacker, &key, host, init);
    let p = position.account_id;
    w.fund_vault(rogue, p, 3);
    w.liquidate_through(rogue, host, position)
        .expect("K' liquidates into its own auction");
    let rogue_acc = accounting_actor(e, rogue);
    let settle = EngineMsg::SettleEmpty {
        config: rogue,
        position: p,
        nonce: 0,
    };
    let actors = [
        config,
        auction_actor(e, rogue, p, 0),
        rogue_acc,
        collateral_actor(e, rogue, def),
    ];
    w.chain
        .send(config, &actors, &[], &settle)
        .expect("K' books its own shortfall");
    assert_eq!(rogue_books(w, rogue).unbacked_debt, ROGUE_DEBT);
    (key, attacker, config)
}

fn rogue_books(w: &World, rogue: AccountId) -> AccountingState {
    w.chain
        .raw_state(accounting_actor(w.engine, rogue))
        .and_then(|bytes| AccountingState::decode(&bytes))
        .expect("the rogue accounting")
}

/// The account half (routed 2, 3, 10): the attacker's config `K'` runs `SettleDebt`,
/// `StartSurplus`, `TakeSurplus`, `CloseSurplus` and `CheckSolvency` with the victim's actors
/// declared beside its own. Everything lands on `K'`'s accounting, auction, vault and buffer:
/// `K'`'s settle burns exactly its own 7_000 from its own buffer (donated first, so a misrouted
/// read or burn would show), the victim's buffer is not burned and its accounting, auctions,
/// protocol supply and host stay byte-identical. Rooted at `K'` naming the victim's config, every
/// root fails `WrongAccount("config")`.
#[test]
fn rogue_config_conversations_land_only_on_its_own_actors() {
    let mut w = victim_with_debt_auction();
    let mut victim = victim_actors(&mut w);
    // The stablecoin definition is shared: K's burns lower its supply; checked separately.
    let sdef_actor = Actor::new(w.stable_def, w.token);
    victim.retain(|actor| *actor != sdef_actor);
    let before = states(&w, &victim);
    let victim_buffer = w.stable_balance(w.buffer().account_id);
    let (key, attacker, rogue) = rogue_with_shortfall(&mut w);
    assert_eq!(states(&w, &victim), before);
    let r = rogue.account_id;
    let rogue_buffer = buffer_holding(w.engine, w.token, r).account_id;
    w.donate_stable(rogue_buffer, ROGUE_DEBT + ROGUE_SURPLUS);
    let supply = stable_supply(&w);

    let run = |w: &mut World, msg: EngineMsg, signers: &[&PrivateKey]| {
        let mut actors = w.system_actors(&msg);
        let extra: Vec<Actor> = victim
            .iter()
            .copied()
            .filter(|actor| !actors.contains(actor))
            .collect();
        actors.extend(extra);
        w.chain
            .send(rogue, &actors, signers, &msg)
            .unwrap_or_else(|err| panic!("K' runs {msg:?}: {err:?}"));
        let changed = w.chain.changed_actors();
        assert!(
            victim.iter().all(|actor| !changed.contains(actor)),
            "{msg:?} changed a victim actor: {changed:?}"
        );
        assert_eq!(states(w, &victim), before, "after {msg:?}");
        invariants::check(w);
    };

    run(
        &mut w,
        EngineMsg::SettleDebt {
            config: r,
            amount: u128::MAX,
        },
        &[],
    );
    let books = rogue_books(&w, r);
    assert_eq!(
        (books.unbacked_debt, books.total_debt_settled),
        (0, ROGUE_DEBT)
    );
    assert_eq!(w.stable_balance(rogue_buffer), ROGUE_SURPLUS);
    assert_eq!(
        stable_supply(&w),
        supply - ROGUE_DEBT,
        "only K's debt burned"
    );
    assert_eq!(w.stable_balance(w.buffer().account_id), victim_buffer);
    assert_eq!(w.accounting_state().unbacked_debt, VICTIM_DEBT);

    let now = w.now_ms;
    run(
        &mut w,
        EngineMsg::StartSurplus {
            config: r,
            nonce: 0,
            now_ms: now,
        },
        &[],
    );
    assert_eq!(w.stable_balance(rogue_buffer), ROGUE_SURPLUS - 500);
    run(
        &mut w,
        EngineMsg::TakeSurplus {
            taker: attacker,
            config: r,
            nonce: 0,
            amount_coin: 250,
            max_protocol_in: u128::MAX,
            now_ms: now,
        },
        &[&key],
    );
    assert_eq!(w.stable_balance(attacker), 250);
    w.set_now_ms(T0 + TTL);
    let now = w.now_ms;
    run(
        &mut w,
        EngineMsg::CloseSurplus {
            config: r,
            nonce: 0,
            now_ms: now,
        },
        &[],
    );
    assert_eq!(rogue_books(&w, r).total_surplus_returned, 250);
    run(
        &mut w,
        EngineMsg::CheckSolvency {
            config: r,
            now_ms: now,
        },
        &[],
    );
    assert!(!w.host_frozen(), "the victim's host is not frozen");

    // Rooted at K', naming the victim's config.
    let k = w.config.account_id;
    let at_victim = [
        EngineMsg::SettleDebt {
            config: k,
            amount: u128::MAX,
        },
        EngineMsg::StartSurplus {
            config: k,
            nonce: 0,
            now_ms: now,
        },
        EngineMsg::TakeSurplus {
            taker: attacker,
            config: k,
            nonce: 0,
            amount_coin: 1,
            max_protocol_in: u128::MAX,
            now_ms: now,
        },
        EngineMsg::CloseSurplus {
            config: k,
            nonce: 0,
            now_ms: now,
        },
        EngineMsg::StartDebt {
            config: k,
            nonce: 1,
            now_ms: now,
        },
        EngineMsg::TakeDebt {
            taker: attacker,
            config: k,
            nonce: 0,
            amount_coin: 1,
            min_protocol_out: 0,
            now_ms: now,
        },
        EngineMsg::CloseDebt {
            config: k,
            nonce: 0,
            now_ms: now,
        },
        EngineMsg::CheckSolvency {
            config: k,
            now_ms: now,
        },
    ];
    let mut actors = vec![rogue];
    actors.extend(victim.iter().copied());
    for msg in at_victim {
        expect_revert(
            w.chain.send(rogue, &actors, &[&key], &msg),
            &wrong("config"),
        );
    }
    // A `System` forged by a relay at K's own accounting, naming the victim's context.
    let ctx = SystemCtx::from_config(k, &w.config_state());
    let forged = EngineMsg::System {
        op: SystemOp::SettleDebt { amount: u128::MAX },
        now_ms: now,
        ctx: Box::new(ctx),
    };
    let relay = Actor::new(attacker, w.chain.program("relay"));
    let rogue_acc = accounting_actor(w.engine, r);
    expect_revert(
        w.chain.send(
            relay,
            &[relay, rogue_acc, w.accounting],
            &[],
            &Forward::new(rogue_acc, &forged),
        ),
        &wrong("accounting"),
    );
    assert_eq!(states(&w, &victim), before);
    invariants::check(&w);
}

/// A `StateReply` that arrives when no read is pending is not a reading: at an accounting with
/// `pending == None` and at a closed surplus auction it is left to the message dispatch, where it
/// is not an `EngineMsg` (`engine: BadMessage`). The token program answers `ReadState` only to its
/// reader, so the real holdings cannot be made to send these; the reply carries the holdings'
/// real state bytes, sent from impostors at the holdings' own accounts and at the attacker's.
#[test]
fn state_reply_outside_a_pending_read_reverts() {
    let mut w = surplus_live();
    w.take_surplus(0, 250, u128::MAX).expect("a partial take");
    w.set_now_ms(T0 + TTL);
    w.close_surplus(0).expect("surplus auction 0 closes");
    invariants::check(&w);
    let sdef = w.stable_def;
    let buffer = w.holding(w.buffer().account_id, sdef);
    let vault = w.holding(w.surplus_vault(0).account_id, sdef);
    let reply_of = |w: &World, holding: Actor| StateReply {
        state: w.chain.raw_state(holding).expect("a holding").into(),
    };
    let attacker = w.chain.key("attacker").1;
    let at_attacker = w.impostor_at(attacker, None);
    let at_buffer = w.impostor_at(buffer.account_id, None);
    let at_vault = w.impostor_at(vault.account_id, None);
    let (acc, s) = (w.accounting, w.surplus(0));
    let victim = victim_actors(&mut w);
    let before = states(&w, &victim);
    let cases = [
        (at_buffer, acc, reply_of(&w, buffer)),
        (at_attacker, acc, reply_of(&w, buffer)),
        (at_vault, s, reply_of(&w, vault)),
        (at_attacker, s, reply_of(&w, vault)),
    ];
    for (from, to, reply) in cases {
        expect_revert(w.speak(from, to, &reply), "engine: BadMessage");
    }
    assert_eq!(states(&w, &victim), before);
    invariants::check(&w);
}

/// Routed 11: a second reply from exactly the holding the accounting asked, inside the read
/// window, fails `WrongAccount("pending")`. The real token program answers once, so the holding
/// is replayed on the attacker's own config `K'` whose token program is the impostor: its
/// buffer holding replies twice. With one reply the same settle goes through.
#[test]
fn duplicate_buffer_reply_reverts() {
    let mut w = World::new();
    let victim = [w.config, w.accounting, w.collateral, w.params];
    let before = states(&w, &victim);
    let rogue = w.impostor_token_config("attacker", World::engine_params());
    let (e, imp) = (w.engine, w.chain.program("impostor"));
    let acc = accounting_actor(e, rogue.account_id);
    let buffer = buffer_holding(e, imp, rogue.account_id).account_id;
    let holding = w.impostor_holding(buffer);
    let reply = encode(&StateReply {
        state: Vec::new().into(),
    });
    let twice = script(&[(acc, reply.clone()), (acc, reply.clone())]);
    w.impostor_at(holding.account_id, Some(twice));
    let settle = EngineMsg::SettleDebt {
        config: rogue.account_id,
        amount: 1,
    };
    let actors = [rogue, acc, holding];
    let books = w.chain.raw_state(acc);
    expect_revert(
        w.chain.send(rogue, &actors, &[], &settle),
        &wrong("pending"),
    );
    assert_eq!(w.chain.raw_state(acc), books);
    w.impostor_at(holding.account_id, Some(script(&[(acc, reply)])));
    w.chain
        .send(rogue, &actors, &[], &settle)
        .expect("one reply completes the read");
    assert_eq!(states(&w, &victim), before);
    invariants::check(&w);
}

/// Routed 2, 6: reports for nonce 1 cannot book against auction 0's or 1's slot. The accounting
/// re-derives the sender from the reported nonce (`surplus_actor(E, K, 1)`), so `SurplusTaken` /
/// `SurplusReturned` for nonce 1 from `S(0)`'s account, and `DebtTaken` / `DebtClosed` / `Mint`
/// for nonce 1 from `D(0)`'s, revert; so do impostors at the never-started `S(1)` / `D(1)`. (The
/// real `S(0)` / `D(0)` only ever report their own nonce, so the engine-program sender at `S(0)`
/// with nonce 1 cannot be produced in a transaction.)
#[test]
fn system_messages_for_another_nonce_cannot_book() {
    let mut w = both_live();
    let attacker = w.chain.key("attacker").1;
    let victim = victim_actors(&mut w);
    let before = states(&w, &victim);
    let k = w.config.account_id;
    let (s1, d1) = (
        engine_core::actors::surplus_actor(w.engine, k, 1),
        engine_core::actors::debt_actor(w.engine, k, 1),
    );
    let surplus_reports = [
        EngineMsg::SurplusTaken {
            config: k,
            nonce: 1,
            protocol_burned: u128::MAX / 2,
            ended: true,
            now_ms: T0,
        },
        EngineMsg::SurplusReturned {
            config: k,
            nonce: 1,
            amount: u128::MAX / 2,
            now_ms: T0,
        },
    ];
    let debt_reports = [
        EngineMsg::DebtTaken {
            config: k,
            nonce: 1,
            filled: 500,
            minted: 0,
            ended: true,
            now_ms: T0,
        },
        EngineMsg::DebtClosed {
            config: k,
            nonce: 1,
            lot_left: 500,
            now_ms: T0,
        },
    ];
    let mint = EngineMsg::Mint {
        nonce: 1,
        to: attacker,
        amount: u128::MAX / 2,
    };
    for at in [w.surplus(0).account_id, s1.account_id] {
        let from = w.impostor_at(at, None);
        for msg in &surplus_reports {
            expect_revert(w.speak(from, w.accounting, msg), &wrong("surplus_auction"));
        }
    }
    for at in [w.debt(0).account_id, d1.account_id] {
        let from = w.impostor_at(at, None);
        for msg in &debt_reports {
            expect_revert(w.speak(from, w.accounting, msg), &wrong("debt_auction"));
        }
        expect_revert(w.speak(from, w.config, &mint), &wrong("debt_auction"));
    }
    assert_eq!(states(&w, &victim), before);
    assert_eq!(w.accounting_state().surplus_nonce, 1);
    invariants::check(&w);
}

/// Cross-check of Task 1 through the engine: a `Debit` of the live surplus vault's stablecoin
/// holding from anyone but the vault's own token actor fails `token: NotHoldingOwner` — impostors
/// at the attacker's, the vault's and the auction's accounts, and a root message. (`(attacker,
/// T)` itself only ever debits its own holding, so it cannot address the vault's; that branch is
/// `token_core::tests::only_the_owner_actor_may_debit`.)
#[test]
fn holding_debit_not_from_its_owner_actor() {
    let mut w = surplus_live();
    let attacker = w.chain.key("attacker").1;
    let sdef = w.stable_def;
    let vault = w.surplus_vault(0).account_id;
    let holding = w.holding(vault, sdef);
    let buffer = w.holding(w.buffer().account_id, sdef);
    let victim = victim_actors(&mut w);
    let before = states(&w, &victim);
    let actions = [
        DebitAction::Transfer {
            to: attacker,
            amount: 500,
        },
        DebitAction::Sweep {
            to: attacker,
            at_least: 0,
        },
        DebitAction::Burn { amount: 500 },
    ];
    let senders = [attacker, vault, w.surplus(0).account_id].map(|at| w.impostor_at(at, None));
    for action in actions {
        let debit = TokenMsg::Debit {
            definition: sdef,
            action,
        };
        for target in [holding, buffer] {
            for from in senders {
                expect_revert(w.speak(from, target, &debit), "token: NotHoldingOwner");
            }
            expect_revert(
                w.chain.send(target, &[target], &[], &debit),
                "token: NotHoldingOwner",
            );
        }
    }
    assert_eq!(states(&w, &victim), before);
    assert_eq!(w.stable_balance(vault), 500);
    invariants::check(&w);
}

/// Routed 14, the `ProbeKind::Debt` arm end to end: `TakeDebt` / `CloseDebt` on a nonce that was
/// never started fail with v0.2.4's `WrongAccount("debt_auction")`, before and after auction 0
/// starts, and move nothing.
#[test]
fn take_or_close_debt_on_a_never_started_nonce_reverts() {
    let mut w = World::with_system(token_params());
    w.set_now_ms(T0);
    assert_eq!(w.book_shortfall(), VICTIM_DEBT);
    w.fund_bidder(TAKER, 1_000);
    invariants::check(&w);
    let absent = wrong("debt_auction");
    let books = w.chain.raw_state(w.accounting);
    expect_revert(w.take_debt(0, 500, 0), &absent);
    expect_revert(w.close_debt(0), &absent);
    assert_eq!(w.chain.raw_state(w.accounting), books);
    w.start_debt(0).expect("debt auction 0 starts");
    let victim = victim_actors(&mut w);
    let before = states(&w, &victim);
    expect_revert(w.take_debt(1, 500, 0), &absent);
    expect_revert(w.close_debt(1), &absent);
    assert_eq!(states(&w, &victim), before);
    assert_eq!(w.debt_state(0).lot_left, 500);
    invariants::check(&w);
}
