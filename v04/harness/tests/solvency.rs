//! `check_solvency` (design note §4.2 row 16, §5; v0.2.4 `solvency.rs`, spec §5.11, ADR-017)
//! through the system Read block: a permissionless root `CheckSolvency` at the config `K`; the
//! accounting reads the buffer (and the reference oracle), settles what it can, then decides:
//! Healthy / NoAction apply only the settle, RecoveryInProgress reverts, Freeze sends `Freeze` to
//! `K`, which sends `EngineFreezeNewDebt` to its bound host.
//!
//! The shortfall is Plan 5's lot-3 case ([`World::book_shortfall`], `U` = 10_000, where v0.2.4
//! had 355_049); the default threshold is 1_000. `harness::invariants::check` after every send.

use auction_engine_params::{EngineError, args::EngineParamsArgs};
use engine_core::{actors::config_actor, check::fail_text, msg::EngineMsg};
use harness::{
    World,
    bidding::Bidder,
    expect_revert, invariants,
    world::{Forward, SALT},
};
use lee::{AccountId, Actor, error::LeeError};
use lee_core::program::TransactionEvent;
use mock_host_core::{HostMsg, position_id};
use token_core::{Definition, TokenMsg};

/// The shortfall [`World::book_shortfall`] books.
const U: u128 = 10_000;
/// `EngineParamsArgs::defaults_for_tests().bad_debt_freeze_threshold`.
const THRESHOLD: u128 = 1_000;
/// The chain time the token worlds start at, ms.
const T0: u64 = 1_000_000;

type Sent = Result<Vec<TransactionEvent>, LeeError>;

fn checked(w: &World, sent: Sent) -> Sent {
    invariants::check(w);
    sent
}

fn check(w: &mut World) -> Sent {
    let sent = w.check_solvency();
    checked(w, sent)
}

fn expect_err(sent: Sent, err: EngineError) {
    expect_revert(sent, &fail_text(err));
}

fn token_params() -> EngineParamsArgs {
    EngineParamsArgs {
        protocol_token_enabled: true,
        ..World::engine_params()
    }
}

/// A world without protocol token, initialised with `params`, the shortfall booked.
fn plain_world(params: EngineParamsArgs) -> World {
    let mut w = World::bare();
    let key = w.admin_key.clone();
    let init = w.init_msg_with(params);
    w.send_engine(w.admin, &[&key], &init)
        .expect("the engine is initialised");
    let set = w.set_collateral_msg(World::collateral_args());
    w.send_engine(w.admin, &[&key], &set)
        .expect("the collateral is configured");
    w.bind_host().expect("the host binds the config actor");
    assert_eq!(w.book_shortfall(), U);
    invariants::check(&w);
    w
}

/// The default world (no protocol token), the shortfall booked.
fn fresh() -> World {
    let mut w = World::new();
    assert_eq!(w.book_shortfall(), U);
    invariants::check(&w);
    w
}

/// `World::with_system(params)` (protocol token enabled) at `T0`, the shortfall booked.
fn token_world(params: EngineParamsArgs) -> World {
    let mut w = World::with_system(params);
    w.set_now_ms(T0);
    assert_eq!(w.book_shortfall(), U);
    invariants::check(&w);
    w
}

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

fn donate_buffer(w: &mut World, amount: u128) {
    let buffer = w.buffer().account_id;
    w.donate_stable(buffer, amount);
    invariants::check(w);
}

/// `funder` transfers `amount` of its stablecoin to the buffer (no host mint, so this works on a
/// frozen host too).
fn pay_buffer(w: &mut World, funder: &Bidder, amount: u128) {
    let (sdef, buffer) = (w.stable_def, w.buffer().account_id);
    let from = Actor::new(funder.account, w.token);
    let transfer = TokenMsg::Transfer {
        definition: sdef,
        to: buffer,
        amount,
    };
    let actors = [
        from,
        w.holding(funder.account, sdef),
        w.holding(buffer, sdef),
    ];
    w.chain
        .send(from, &actors, &[&funder.key], &transfer)
        .expect("the funder pays the buffer");
    invariants::check(w);
}

/// The admin sets the threshold to `threshold` (everything else the world's defaults).
fn set_threshold(w: &mut World, threshold: u128) {
    let params = EngineParamsArgs {
        bad_debt_freeze_threshold: threshold,
        ..World::engine_params()
    };
    let msg = w.set_engine_params_msg(params, None);
    let key = w.admin_key.clone();
    w.send_engine(w.admin, &[&key], &msg)
        .expect("the admin sets the threshold");
}

/// The signer `label`, owner of a host position, asks the host for `amount` more debt.
fn generate_debt(w: &mut World, label: &str, amount: u128) -> Sent {
    let (key, owner) = w.chain.key(label);
    let (host, token) = (w.host_prog, w.token);
    let proxy = Actor::new(owner, host);
    let msg = HostMsg::Proxy {
        params: w.params.account_id,
        message: Box::new(HostMsg::GenerateDebt {
            normalized_debt: amount,
        }),
    };
    let actors = [
        proxy,
        w.params,
        Actor::new(position_id(host, owner), host),
        Actor::new(w.stable_def, token),
        w.holding(owner, w.stable_def),
    ];
    w.chain.send(proxy, &actors, &[&key], &msg)
}

/// Review Focus 1: no bid has paid a penalty, so the buffer holding was never created. Without
/// the `Pda` evidence for it the transaction cannot admit the holding; with it the read sees 0,
/// the debt stays above the threshold and, with no protocol token, the host is frozen. A frozen
/// host refuses new debt.
#[test]
fn fresh_config_without_protocol_token_freezes_above_threshold() {
    let mut w = fresh();
    w.open_position("borrower", 1_000, 100);
    let holding = w.holding(w.buffer().account_id, w.stable_def);
    assert_eq!(w.chain.raw_state(holding), None, "no buffer holding yet");
    let msg = w.check_solvency_msg();
    expect_revert(w.send_system(&msg, "", &[]), "UnadmittedPublicActor");
    assert!(!w.host_frozen());

    check(&mut w).expect("freezes");
    assert!(w.host_frozen());
    let acc = w.accounting_state();
    assert!(acc.host_frozen_by_engine);
    assert_eq!((acc.unbacked_debt, acc.total_debt_settled), (U, 0));
    assert_eq!(w.chain.raw_state(holding), None, "the read creates nothing");

    expect_revert(generate_debt(&mut w, "borrower", 1), "host: Frozen");
}

/// No protocol token: `unbacked ≤ threshold` is healthy, one unit above freezes.
#[test]
fn threshold_boundary_equality_is_healthy_and_one_unit_above_freezes() {
    let mut w = fresh();
    set_threshold(&mut w, U);
    check(&mut w).expect("healthy at the threshold");
    assert!(!w.host_frozen());
    assert!(!w.accounting_state().host_frozen_by_engine);

    set_threshold(&mut w, U - 1);
    check(&mut w).expect("freezes one unit above");
    assert!(w.host_frozen());
    assert!(w.accounting_state().host_frozen_by_engine);
}

/// A live debt auction is recovery in progress: the check reverts and freezes nothing, also with
/// the freeze disabled.
#[test]
fn live_debt_auction_reverts_with_recovery_in_progress() {
    for freeze_on_bad_debt in [true, false] {
        let mut w = token_world(EngineParamsArgs {
            freeze_on_bad_debt,
            ..token_params()
        });
        let sent = w.start_debt(0);
        checked(&w, sent).expect("the debt auction starts");
        let before = w.chain.raw_state(w.accounting);
        expect_err(check(&mut w), EngineError::RecoveryInProgress);
        assert!(!w.host_frozen(), "freeze_on_bad_debt {freeze_on_bad_debt}");
        assert_eq!(w.chain.raw_state(w.accounting), before);
    }
}

/// The debt lot's start amount is 500 × RAY / 2 RAY = 250 tokens: a mint cap of 250 can start an
/// auction (no action), 249 cannot (`MintCapConfig`), so the host is frozen.
#[test]
fn unstartable_debt_auction_freezes() {
    for (max_mint_per_auction, freezes) in [(250, false), (249, true)] {
        let mut w = token_world(EngineParamsArgs {
            max_mint_per_auction,
            ..token_params()
        });
        check(&mut w).expect("decides");
        assert_eq!(w.host_frozen(), freezes, "cap {max_mint_per_auction}");
        assert_eq!(w.accounting_state().host_frozen_by_engine, freezes);
    }
}

/// With the freeze disabled an unstartable auction takes no action (v0.2.4
/// `freeze_disabled_takes_no_action_with_a_broken_token`).
#[test]
fn freeze_disabled_takes_no_action_with_an_unstartable_auction() {
    let mut w = token_world(EngineParamsArgs {
        freeze_on_bad_debt: false,
        max_mint_per_auction: 249,
        ..token_params()
    });
    check(&mut w).expect("no action");
    assert!(!w.host_frozen());
    assert!(!w.accounting_state().host_frozen_by_engine);
}

#[test]
fn working_protocol_token_never_freezes() {
    let mut w = token_world(token_params());
    let before = w.chain.raw_state(w.accounting);
    check(&mut w).expect("NoAction: a keeper should start a debt auction");
    assert!(!w.host_frozen());
    assert_eq!(w.chain.raw_state(w.accounting), before, "nothing to settle");
}

/// ADR-017: with no protocol token there is no recovery path, so the flag does not apply.
#[test]
fn freeze_disabled_still_freezes_without_a_protocol_token() {
    let mut w = plain_world(EngineParamsArgs {
        freeze_on_bad_debt: false,
        ..World::engine_params()
    });
    check(&mut w).expect("freezes");
    assert!(w.host_frozen());
    assert!(w.accounting_state().host_frozen_by_engine);
}

#[test]
fn implicit_settle_runs_before_the_decision() {
    let mut w = fresh();
    donate_buffer(&mut w, U - THRESHOLD);
    check(&mut w).expect("healthy after the implicit settle");
    assert!(!w.host_frozen());
    assert_eq!(buffer_balance(&w), 0);
    assert_eq!(w.accounting_state().unbacked_debt, THRESHOLD);
}

#[test]
fn implicit_settle_then_freeze_burns_and_freezes_in_one_transaction() {
    let mut w = fresh();
    donate_buffer(&mut w, 1_000);
    let supply = stable_supply(&w);
    check(&mut w).expect("burn + freeze");
    assert!(w.host_frozen());
    assert_eq!(buffer_balance(&w), 0);
    let acc = w.accounting_state();
    assert_eq!(
        (acc.unbacked_debt, acc.total_debt_settled),
        (U - 1_000, 1_000)
    );
    assert!(acc.host_frozen_by_engine);
    assert_eq!(stable_supply(&w), supply - 1_000);
}

#[test]
fn second_check_on_a_frozen_host_is_a_harmless_refreeze() {
    let mut w = fresh();
    check(&mut w).expect("first");
    assert!(w.host_frozen());
    let before = w.chain.raw_state(w.accounting);
    check(&mut w).expect("second: the host's freeze is idempotent");
    assert!(w.host_frozen());
    assert_eq!(w.chain.raw_state(w.accounting), before);
}

/// The engine never unfreezes: a later healthy check keeps the host frozen and the flag set.
#[test]
fn a_later_healthy_check_does_not_unfreeze() {
    let mut w = fresh();
    let funder = w.fund_bidder("funder", U);
    check(&mut w).expect("freeze");
    pay_buffer(&mut w, &funder, U);
    check(&mut w).expect("healthy now");
    let acc = w.accounting_state();
    assert_eq!(acc.unbacked_debt, 0);
    assert!(acc.host_frozen_by_engine);
    assert!(w.host_frozen(), "the engine never unfreezes");
}

/// Review Focus 4: the host was rebound to another config before the freeze. The host refuses
/// the old config's `EngineFreezeNewDebt`, and the whole check reverts with it, implicit settle
/// included: no burn, the accounting byte-identical.
#[test]
fn freeze_after_the_host_was_rebound_reverts_atomically() {
    let mut w = fresh();
    donate_buffer(&mut w, 300);
    let (rival_key, rival) = w.chain.key("rival-admin");
    w.chain.fund(rival, 1);
    let init = w.init_msg();
    w.send_engine(rival, &[&rival_key], &init)
        .expect("the rival's own config");
    let params = w.params;
    let rebind = HostMsg::SetLiquidationEngine {
        engine_actor: config_actor(w.engine, rival, SALT),
    };
    let params_key = w.params_key.clone();
    w.chain
        .send(params, &[params], &[&params_key], &rebind)
        .expect("the host rebinds");

    let before = (
        w.chain.raw_state(w.accounting),
        buffer_balance(&w),
        stable_supply(&w),
    );
    expect_revert(check(&mut w), "host: NotEngine");
    assert_eq!(
        (
            w.chain.raw_state(w.accounting),
            buffer_balance(&w),
            stable_supply(&w)
        ),
        before
    );
    assert!(!w.host_frozen());
    assert!(!w.accounting_state().host_frozen_by_engine);
}

/// `K` freezes only for `accounting_actor(E, K)`: a root `Freeze`, a relay, impostor programs at
/// the accounting's, `K`'s and a rival accounting's accounts all revert. (An engine actor other
/// than the accounting sending `Freeze` is unreachable in a transaction; that half is unit-tested
/// on `engine_core::assertions::freeze_sender`.) The host refuses `EngineFreezeNewDebt` from
/// anyone but its bound engine actor.
#[test]
fn freeze_from_anything_but_this_configs_accounting_reverts() {
    let mut w = fresh();
    let (rival_key, rival) = w.chain.key("rival-admin");
    w.chain.fund(rival, 1);
    let init = w.init_msg();
    w.send_engine(rival, &[&rival_key], &init)
        .expect("the rival's own config");
    let rival_accounting = engine_core::actors::accounting_actor(
        w.engine,
        config_actor(w.engine, rival, SALT).account_id,
    );
    let refused = fail_text(EngineError::WrongAccount("accounting"));
    let (config, params) = (w.config, w.params);
    expect_revert(
        w.chain
            .send(config, &[config, params], &[], &EngineMsg::Freeze),
        &refused,
    );
    let (_, attacker) = w.chain.key("attacker");
    w.chain.fund(attacker, 1);
    let relay = Actor::new(attacker, w.chain.program("relay"));
    expect_revert(
        w.chain.send(
            relay,
            &[relay, config, params],
            &[],
            &Forward::new(config, &EngineMsg::Freeze),
        ),
        &refused,
    );
    let ats: [AccountId; 3] = [
        w.accounting.account_id,
        config.account_id,
        rival_accounting.account_id,
    ];
    for at in ats {
        let impostor = w.impostor(at);
        expect_revert(w.speak(impostor, config, &EngineMsg::Freeze), &refused);
    }
    let impostor = w.impostor(config.account_id);
    expect_revert(
        w.speak(impostor, params, &HostMsg::EngineFreezeNewDebt),
        "host: NotEngine",
    );
    assert!(!w.host_frozen());
    invariants::check(&w);
}
