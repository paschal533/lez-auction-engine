//! The harness runs a real guest in-process and keeps the clock actor at `now`.

use clock_core::Message as ClockMsg;
use harness::Chain;
use lee::Actor;

fn echo_chain() -> Chain {
    Chain::new(&[("echo", methods::ECHO_ELF)])
}

#[test]
fn echo_guest_writes_its_message() {
    let mut chain = echo_chain();
    let (key, target) = chain.key("target");
    chain.fund(target, 1_000);
    let echo = Actor::new(target, chain.program("echo"));
    let msg = b"hi".to_vec();

    let (cycles, simulated) = chain.cycles(echo, &[echo], &[&key], &msg);
    assert!(simulated.is_ok(), "simulation reverted: {simulated:?}");
    assert!(cycles > 0);

    chain
        .send(echo, &[echo], &[&key], &msg)
        .expect("echo applies");

    assert_eq!(chain.raw_state(echo).as_deref(), Some(&b"hi"[..]));
}

#[test]
fn state_is_none_for_an_actor_without_state() {
    let mut chain = echo_chain();
    let (_key, target) = chain.key("target");
    let echo = Actor::new(target, chain.program("echo"));
    assert_eq!(chain.state::<Vec<u8>>(echo), None);
}

#[test]
fn signer_nonces_advance_between_sends() {
    let mut chain = echo_chain();
    let (key, target) = chain.key("target");
    chain.fund(target, 1_000);
    let echo = Actor::new(target, chain.program("echo"));

    chain
        .send(echo, &[echo], &[&key], &b"a".to_vec())
        .expect("first send");
    chain
        .send(echo, &[echo], &[&key], &b"b".to_vec())
        .expect("second send");

    assert_eq!(chain.raw_state(echo).as_deref(), Some(&b"ab"[..]));
}

#[test]
fn keys_are_deterministic_per_label() {
    let mut a = echo_chain();
    let mut b = echo_chain();
    assert_eq!(a.key("alice").1, b.key("alice").1);
    assert_ne!(a.key("alice").1, a.key("bob").1);
}

#[test]
fn clock_actor_holds_now_after_set_now_ms() {
    let mut chain = echo_chain();
    chain.set_now_ms(1_700_000_000_000);
    let clock = chain.clock();

    let inside = ClockMsg::AssertTimestamp {
        at_least: 1_700_000_000_000,
        at_most: 1_700_000_000_500,
    };
    chain
        .send(clock, &[clock], &[], &inside)
        .expect("now is inside the window");

    let behind = ClockMsg::AssertTimestamp {
        at_least: 1_700_000_000_001,
        at_most: 1_700_000_000_500,
    };
    assert!(
        chain.send(clock, &[clock], &[], &behind).is_err(),
        "now is before the window"
    );

    let ahead = ClockMsg::AssertTimestamp {
        at_least: 1_699_999_999_000,
        at_most: 1_699_999_999_999,
    };
    assert!(
        chain.send(clock, &[clock], &[], &ahead).is_err(),
        "now is after the window"
    );

    chain.set_now_ms(1_700_000_000_501);
    assert!(
        chain.send(clock, &[clock], &[], &inside).is_err(),
        "time moved past the window"
    );
}
