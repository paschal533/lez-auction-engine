//! S2: how does `callee` authenticate that it was invoked by the engine program?
//!
//! Variant A: `ctx.caller_program_id == stored engine program id`.
//! Variant B: the stored engine authority PDA is authorised in the call.
//! Variant C: both.
//!
//! For each variant: (1) legitimate chained call succeeds, (2) direct top-level call fails,
//! (3) a third program (`impostor`) chaining to `callee` fails, (4) a transaction that lists the
//! PDA account but is signed by a user key fails.

mod common;

use common::*;
use spike_core::{
    CalleeInstruction, CallerInstruction, ImpostorInstruction, VARIANT_A, VARIANT_B, VARIANT_C,
};

const USER: u8 = 31;

fn name(variant: u8) -> &'static str {
    match variant {
        VARIANT_A => "A",
        VARIANT_B => "B",
        _ => "C",
    }
}

fn user_id() -> lee_core::account::AccountId {
    id_of(&key(USER))
}

fn assert_fail(label: &str, res: Result<(), lee::error::LeeError>, needles: &[&str]) {
    let text = err_text(res);
    println!("FAIL[{label}] {text}");
    assert!(
        needles.iter().any(|n| text.contains(n)),
        "{label}: failed for an unexpected reason: {text}"
    );
}

/// (1) legitimate call from the engine program succeeds; also records cycles.
fn legit(variant: u8) {
    let mut state = base_state();
    let accounts = vec![callee_config_id(), engine_authority(), user_id()];
    let instruction = CallerInstruction::CallGuarded {
        variant,
        callee_program_id: callee_id(),
    };
    let signer = key(USER);
    let m = measure(
        &format!("S2 variant {} legit", name(variant)),
        &state,
        caller_id(),
        &accounts,
        &[&signer],
        &instruction,
    );
    assert_eq!(m.per_exec.len(), 2, "caller + callee");
    println!(
        "S2RESULT variant={} caller_cycles={} callee_cycles={} total_cycles={}",
        name(variant),
        m.cycles_of("caller"),
        m.cycles_of("callee"),
        m.total
    );
    submit(&mut state, caller_id(), accounts, &[&signer], instruction)
        .expect("legitimate chained call must succeed");
}

/// (2) a top-level transaction calling `callee.guarded_*` directly (no signature at all).
fn direct(variant: u8, needles: &[&str]) {
    let mut state = base_state();
    let res = submit(
        &mut state,
        callee_id(),
        vec![callee_config_id(), engine_authority(), user_id()],
        &[],
        CalleeInstruction::from_variant(variant),
    );
    assert_fail(&format!("variant {} direct", name(variant)), res, needles);
}

/// (3) the impostor program chains to `callee`, in all three attack modes.
fn impostor(variant: u8, mode: u8, needles: &[&str]) {
    let mut state = base_state();
    // Mode 2 passes the impostor's own ENGINE_AUTH PDA as the authority.
    let authority = if mode == 2 {
        spike_core::named_pda(impostor_id(), spike_core::ENGINE_AUTH_SEED)
    } else {
        engine_authority()
    };
    let signer = key(USER);
    let res = submit(
        &mut state,
        impostor_id(),
        vec![user_id(), callee_config_id(), authority],
        &[&signer],
        ImpostorInstruction::Attack {
            callee_program_id: callee_id(),
            variant,
            mode,
        },
    );
    assert_fail(
        &format!("variant {} impostor mode {mode}", name(variant)),
        res,
        needles,
    );
}

/// (4) the PDA account is listed and the transaction is signed by a user key.
fn user_signed(variant: u8, needles: &[&str]) {
    let mut state = base_state();
    let signer = key(USER);
    let res = submit(
        &mut state,
        callee_id(),
        vec![callee_config_id(), engine_authority(), user_id()],
        &[&signer],
        CalleeInstruction::from_variant(variant),
    );
    assert_fail(
        &format!("variant {} user-signed", name(variant)),
        res,
        needles,
    );
}

const GUARD_A: &[&str] = &["guard A"];
const GUARD_B: &[&str] = &["guard B"];
// The guest of variant B accepts a forged `is_authorized` flag, lee rejects it after execution.
const LEE_REJECTS_AUTH: &[&str] = &["InvalidAccountAuthorization"];

#[test]
fn variant_a_legit_chained_call_succeeds() {
    legit(VARIANT_A);
}
#[test]
fn variant_a_direct_call_fails() {
    direct(VARIANT_A, GUARD_A);
}
#[test]
fn variant_a_impostor_fails() {
    for mode in 0..3 {
        impostor(VARIANT_A, mode, GUARD_A);
    }
}
#[test]
fn variant_a_user_signed_fails() {
    user_signed(VARIANT_A, GUARD_A);
}

#[test]
fn variant_b_legit_chained_call_succeeds() {
    legit(VARIANT_B);
}
#[test]
fn variant_b_direct_call_fails() {
    direct(VARIANT_B, GUARD_B);
}
#[test]
fn variant_b_impostor_fails() {
    impostor(VARIANT_B, 0, LEE_REJECTS_AUTH);
    impostor(VARIANT_B, 1, LEE_REJECTS_AUTH);
    impostor(VARIANT_B, 2, GUARD_B);
}
#[test]
fn variant_b_user_signed_fails() {
    user_signed(VARIANT_B, GUARD_B);
}

#[test]
fn variant_c_legit_chained_call_succeeds() {
    legit(VARIANT_C);
}
#[test]
fn variant_c_direct_call_fails() {
    direct(VARIANT_C, GUARD_A);
}
#[test]
fn variant_c_impostor_fails() {
    for mode in 0..3 {
        impostor(VARIANT_C, mode, GUARD_A);
    }
}
#[test]
fn variant_c_user_signed_fails() {
    user_signed(VARIANT_C, GUARD_A);
}

/// (5) Laundering (lee propagates authorisation down the chain, validated_state_diff/mod.rs:262-290):
/// engine -> X (authority PDA authorised by the engine) -> callee.guarded_*.
fn laundered(variant: u8) -> Result<(), lee::error::LeeError> {
    let mut state = base_state();
    let signer = key(USER);
    let res = submit(
        &mut state,
        caller_id(),
        vec![callee_config_id(), engine_authority(), user_id()],
        &[&signer],
        CallerInstruction::CallUntrusted {
            target_program_id: impostor_id(),
            callee_program_id: callee_id(),
            variant,
        },
    );
    println!("LAUNDER[variant {}] {:?}", name(variant), res);
    res
}

#[test]
fn laundering_variant_a_rejects() {
    assert_fail("variant A laundered", laundered(VARIANT_A), GUARD_A);
}
#[test]
fn laundering_variant_b_accepts_the_bypass() {
    laundered(VARIANT_B).expect("variant B accepts the laundered call (the bypass)");
}
#[test]
fn laundering_variant_c_rejects() {
    assert_fail("variant C laundered", laundered(VARIANT_C), GUARD_A);
}
