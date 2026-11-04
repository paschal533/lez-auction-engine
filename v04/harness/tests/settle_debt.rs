//! `SettleDebt` (design note §4.2 row 9; v0.2.4 `settle_debt`) through the system Read block at
//! the accounting actor: a permissionless root at the config `K`, which hands `System` to its
//! accounting; the accounting reads the buffer's stablecoin holding and, at `Proceed`, burns
//! `min(amount, buffer, unbacked - debt_on_auction)` from it under the `BUFFER` seed.
//! `harness::invariants::check` after every send.
//!
//! The shortfall is Plan 5's lot-3 case ([`World::book_shortfall`]): 10_000 of debt booked by
//! `SettleEmpty`, with no bid ever paying a penalty into the buffer.

use auction_engine_params::EngineError;
use engine_core::{
    SystemCtx, SystemOp,
    actors::{accounting_actor, buffer_holding, config_actor},
    check::fail_text,
    msg::EngineMsg,
    state::ConfigState,
};
use harness::{
    World, expect_revert, invariants,
    world::{Forward, SALT},
};
use lee::{Actor, PublicAccountEvidence};
use token_core::{Definition, holding_seed};

/// The shortfall [`World::book_shortfall`] books.
const SHORTFALL: u128 = 10_000;
/// What the tests donate to the buffer.
const DONATION: u128 = 300;

fn stable_supply(w: &World) -> u128 {
    w.chain
        .raw_state(Actor::new(w.stable_def, w.token))
        .and_then(|bytes| Definition::decode(&bytes))
        .expect("the stablecoin definition")
        .supply
}

fn buffer_balance(w: &World) -> u128 {
    w.stable_balance(w.buffer().account_id)
}

/// A world whose accounting carries the lot-3 shortfall.
fn with_shortfall() -> World {
    let mut w = World::new();
    assert_eq!(w.book_shortfall(), SHORTFALL);
    invariants::check(&w);
    w
}

#[test]
fn settle_debt_burns_min_of_amount_buffer_and_settleable() {
    let mut w = with_shortfall();
    let buffer = w.buffer().account_id;
    w.donate_stable(buffer, DONATION);
    assert_eq!(buffer_balance(&w), DONATION);
    let supply = stable_supply(&w);
    assert!(w.buffer_evidence().is_empty(), "the buffer holding exists");

    let msg = w.settle_debt_msg(100);
    w.send_system(&msg, "settler", &[]).expect("settles 100");
    invariants::check(&w);
    let acc = w.accounting_state();
    assert_eq!(
        (acc.unbacked_debt, acc.total_debt_settled),
        (SHORTFALL - 100, 100)
    );
    assert_eq!(buffer_balance(&w), DONATION - 100);
    assert_eq!(stable_supply(&w), supply - 100);

    // The rest of the buffer, whatever the amount asked.
    let msg = w.settle_debt_msg(u128::MAX);
    w.send_system(&msg, "", &[]).expect("settles the buffer");
    invariants::check(&w);
    let acc = w.accounting_state();
    assert_eq!(
        (acc.unbacked_debt, acc.total_debt_settled),
        (SHORTFALL - DONATION, DONATION)
    );
    assert_eq!(buffer_balance(&w), 0);
    assert_eq!(stable_supply(&w), supply - DONATION);

    // An empty buffer burns nothing and changes nothing.
    let before = w.chain.raw_state(w.accounting);
    w.send_system(&msg, "", &[]).expect("burns 0");
    invariants::check(&w);
    assert_eq!(w.chain.raw_state(w.accounting), before);
    assert_eq!(stable_supply(&w), supply - DONATION);
}

/// Review Focus 1: no bid has paid a penalty, so the buffer holding was never created. With the
/// `Pda` evidence for it the read sees 0 and nothing burns; without it the transaction cannot
/// admit the holding.
#[test]
fn settle_debt_on_a_fresh_config_reads_an_absent_buffer_as_zero() {
    let mut w = with_shortfall();
    let holding = w.holding(w.buffer().account_id, w.stable_def);
    assert_eq!(w.chain.raw_state(holding), None, "no buffer holding yet");
    let (accounting, supply) = (w.chain.raw_state(w.accounting), stable_supply(&w));
    let msg = w.settle_debt_msg(u128::MAX);

    expect_revert(w.send_system(&msg, "", &[]), "UnadmittedPublicActor");
    assert_eq!(w.chain.raw_state(w.accounting), accounting);

    let evidence = w.buffer_evidence();
    assert_eq!(evidence.len(), 1, "Pda evidence for the absent holding");
    w.send_system(&msg, "", &evidence)
        .expect("reads the absent buffer as 0");
    invariants::check(&w);
    assert_eq!(w.chain.raw_state(w.accounting), accounting, "burns 0");
    assert_eq!(w.chain.raw_state(holding), None, "the read creates nothing");
    assert_eq!(stable_supply(&w), supply);
}

/// A `SettleDebt` whose root is not the named config, or names an account that holds no config,
/// fails with v0.2.4's `WrongAccount("config")`.
#[test]
fn settle_debt_at_a_foreign_config_reverts() {
    let mut w = with_shortfall();
    let buffer = w.buffer().account_id;
    w.donate_stable(buffer, DONATION);
    let actors = w.system_actors(&w.settle_debt_msg(1));
    let before = (w.chain.raw_state(w.accounting), buffer_balance(&w));
    // Rooted at the world's config, naming another account.
    let foreign = EngineMsg::SettleDebt {
        config: w.accounting.account_id,
        amount: 1,
    };
    expect_revert(
        w.chain.send(w.config, &actors, &[], &foreign),
        &fail_text(EngineError::WrongAccount("config")),
    );
    // Rooted at the named account, which holds an accounting, not a config.
    let mut at_accounting = actors.clone();
    at_accounting.swap(0, 1);
    expect_revert(
        w.chain.send(w.accounting, &at_accounting, &[], &foreign),
        &fail_text(EngineError::WrongAccount("config")),
    );
    assert_eq!(
        (w.chain.raw_state(w.accounting), buffer_balance(&w)),
        before
    );
    invariants::check(&w);
}

/// An attacker's own config `(pda(E, CONFIG, attacker, salt), E)` runs `SettleDebt` on its own
/// accounting only; it cannot root one at the victim's config, and a `System` delivered to the
/// victim's accounting from any actor but the victim's config is refused.
#[test]
fn system_message_from_a_rogue_config_cannot_reach_the_victim_accounting() {
    let mut w = with_shortfall();
    let buffer = w.buffer().account_id;
    w.donate_stable(buffer, DONATION);
    let victim = (w.chain.raw_state(w.accounting), buffer_balance(&w));
    let (key, attacker) = w.chain.key("attacker");
    w.chain.fund(attacker, 1);
    let init = w.init_msg();
    w.send_engine(attacker, &[&key], &init)
        .expect("the attacker's own config");
    let (e, t, sdef) = (w.engine, w.token, w.stable_def);
    let rogue = config_actor(e, attacker, SALT);
    let rogue_buffer = buffer_holding(e, t, rogue.account_id).account_id;

    // The rogue config's `System` lands on its own accounting, the victim's declared beside it.
    let msg = EngineMsg::SettleDebt {
        config: rogue.account_id,
        amount: u128::MAX,
    };
    let mut actors = w.system_actors(&msg);
    actors.push(w.accounting);
    let evidence = [PublicAccountEvidence::Pda {
        program: t,
        seed: holding_seed(rogue_buffer, sdef),
    }];
    w.chain
        .send_with_evidence(rogue, &actors, &[], &evidence, &msg)
        .expect("K' settles on its own accounting");
    assert!(!w.chain.changed_actors().contains(&w.accounting));
    assert_eq!(
        (w.chain.raw_state(w.accounting), buffer_balance(&w)),
        victim
    );

    // Rooted at the rogue config, naming the victim's.
    let at_victim = w.settle_debt_msg(u128::MAX);
    expect_revert(
        w.chain.send(rogue, &actors, &[], &at_victim),
        &fail_text(EngineError::WrongAccount("config")),
    );

    // A forged `System` naming the victim's config, from a relay actor.
    let config = w
        .chain
        .raw_state(w.config)
        .and_then(|bytes| ConfigState::decode(&bytes))
        .expect("the victim's config");
    let forged = EngineMsg::System {
        op: SystemOp::SettleDebt { amount: u128::MAX },
        now_ms: 0,
        ctx: Box::new(SystemCtx::from_config(w.config.account_id, &config)),
    };
    let relay = Actor::new(attacker, w.chain.program("relay"));
    let victim_buffer = w.holding(buffer, sdef);
    expect_revert(
        w.chain.send(
            relay,
            &[relay, w.accounting, victim_buffer],
            &[],
            &Forward::new(w.accounting, &forged),
        ),
        &fail_text(EngineError::WrongAccount("accounting")),
    );
    assert_eq!(accounting_actor(e, w.config.account_id), w.accounting);
    assert_eq!(
        (w.chain.raw_state(w.accounting), buffer_balance(&w)),
        victim
    );
    invariants::check(&w);
}
