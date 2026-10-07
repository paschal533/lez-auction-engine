//! Task 15: `check_solvency` (spec §5.11, ADR-017) end to end through the real engine, token and
//! stablecoin guests: implicit settle first, then Healthy / Freeze / RecoveryInProgress / NoAction.
//! A Freeze chains `stablecoin.engine_freeze_new_debt` signed by `["ENGINE_AUTH", config]` (Variant C).

mod common;

use auction_engine_core::{
    args::{CollateralArgs, EngineParamsArgs},
    pda::{SLOT_PROTOCOL_TOKEN, SLOT_REF_ORACLE},
    state::AccountingState,
};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use common::*;
use lee::error::LeeError;
use lee_core::account::AccountId;

/// Unbacked debt after `book_shortfall` (see `common`).
const BOOKED: u128 = 355_049;

fn token_params() -> EngineParamsArgs {
    EngineParamsArgs {
        protocol_token_enabled: true,
        ..EngineParamsArgs::defaults_for_tests()
    }
}

fn acc(w: &World) -> AccountingState {
    let a: AccountingState = w.read(accounting());
    assert!(
        a.debt_on_auction <= a.unbacked_debt,
        "debt_on_auction {} > unbacked_debt {}",
        a.debt_on_auction,
        a.unbacked_debt
    );
    a
}

/// Engine configured with `params`; the mock protocol token exists with mint authority `authority`
/// (or not at all); a shortfall of `BOOKED` is booked.
fn system_world(authority: Option<AccountId>, params: EngineParamsArgs) -> World {
    let mut w = World::new();
    w.init_engine_config(&params).unwrap();
    w.bind_engine().unwrap();
    w.set_collateral_config(k::ENGINE_ADMIN, &CollateralArgs::defaults_for_tests())
        .unwrap();
    if let Some(a) = authority {
        w.create_protocol_token(10_000, a).unwrap();
    }
    w.book_shortfall();
    w
}

fn open_position_raw(w: &mut World, owner: u8, coll: u8, nonce: u64) -> Result<(), LeeError> {
    let pos = position(owner, nonce);
    w.host(
        vec![
            id(owner),
            pos,
            host_vault(pos),
            id(coll),
            collateral_def(),
            protocol_parameters(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        &[owner, coll],
        stablecoin_core::Instruction::OpenPosition {
            position_nonce: nonce,
            initial_collateral_amount: 1_000,
        },
    )
}

fn placeholders(w: &World) -> Vec<AccountId> {
    w.check_solvency_accounts(unused(SLOT_PROTOCOL_TOKEN), unused(SLOT_REF_ORACLE))
}

// ---- decision table (ADR-017), one test per row ----

#[test]
fn no_protocol_token_freezes_above_threshold_and_only_the_freeze_authority_unfreezes() {
    let mut w = World::with_engine();
    w.book_shortfall();
    w.check_solvency().expect("check_solvency");
    assert!(w.host_frozen());
    assert!(acc(&w).host_frozen_by_engine);
    // Plan review M6: the real host now rejects new positions.
    w.ensure_signer(k::OWNER2);
    w.fund_collateral(k::OWNER2_COLL, 1_000);
    assert_err(
        open_position_raw(&mut w, k::OWNER2, k::OWNER2_COLL, 0),
        "Protocol is frozen",
    );
    // The host's own freeze authority unfreezes; the engine has no unfreeze path.
    w.host(
        vec![id(k::FREEZE), protocol_parameters()],
        &[k::FREEZE],
        stablecoin_core::Instruction::Unfreeze,
    )
    .expect("freeze authority unfreezes");
    assert!(!w.host_frozen());
    open_position_raw(&mut w, k::OWNER2, k::OWNER2_COLL, 0).expect("works again after unfreeze");
}

#[test]
fn frozen_host_rejects_generate_debt() {
    // Plan review M6: GenerateDebt on an existing position is blocked too.
    let mut w = World::with_engine();
    w.book_shortfall();
    w.holding(k::OWNER2_STABLE, stable_def(), 0);
    let pos = w.open_position(k::OWNER2, k::OWNER2_COLL, k::OWNER2_STABLE, 0, 1_000_000, 0);
    w.check_solvency().expect("freeze");
    assert!(w.host_frozen());
    let r = w.host(
        vec![
            id(k::OWNER2),
            pos,
            stable_def(),
            id(k::OWNER2_STABLE),
            accumulator(),
            redemption_state(),
            oracle(),
            protocol_parameters(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        &[k::OWNER2],
        stablecoin_core::Instruction::GenerateDebt { amount: 1_000 },
    );
    assert_err(r, "Protocol is frozen");
}

#[test]
fn shortfall_at_threshold_does_not_freeze() {
    // A small shortfall (1_000 = threshold) must not freeze the stablecoin (spec §5.11 "why the threshold").
    let mut w = World::with_engine();
    let pos = w.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 0, 1_000, 1_000);
    w.set_redemption_price(250 * ONE);
    w.set_oracle(250 * ONE, w.now);
    w.liquidate(pos, 0).unwrap();
    w.settle_empty(pos).unwrap();
    assert_eq!(acc(&w).unbacked_debt, 1_000);
    w.check_solvency().expect("healthy");
    assert!(!w.host_frozen());
    assert!(!acc(&w).host_frozen_by_engine);
}

#[test]
fn threshold_boundary_equality_is_healthy_and_one_unit_above_freezes() {
    // No protocol token: the only thing separating Healthy from Freeze is `unbacked <= threshold`.
    for (threshold, freezes) in [(BOOKED + 1, false), (BOOKED, false), (BOOKED - 1, true)] {
        let p = EngineParamsArgs {
            bad_debt_freeze_threshold: threshold,
            ..EngineParamsArgs::defaults_for_tests()
        };
        let mut w = system_world(None, p);
        assert_eq!(acc(&w).unbacked_debt, BOOKED);
        w.check_solvency().expect("check_solvency");
        assert_eq!(w.host_frozen(), freezes, "threshold {threshold}");
    }
}

#[test]
fn live_debt_auction_reverts_with_recovery_in_progress() {
    let mut w = system_world(Some(mint_authority()), token_params());
    w.start_debt(0).unwrap();
    assert_err(w.check_solvency(), "RecoveryInProgress");
    assert!(!w.host_frozen());
    assert!(!acc(&w).host_frozen_by_engine);
}

#[test]
fn live_debt_auction_is_recovery_in_progress_even_when_freeze_is_disabled() {
    let p = EngineParamsArgs {
        freeze_on_bad_debt: false,
        ..token_params()
    };
    let mut w = system_world(Some(mint_authority()), p);
    w.start_debt(0).unwrap();
    assert_err(w.check_solvency(), "RecoveryInProgress");
    assert!(!w.host_frozen());
}

#[test]
fn unstartable_debt_auction_freezes() {
    // Debt lot 500 at ref price p: start amount = 500 * RAY / p.
    let cap_exceeded = EngineParamsArgs {
        protocol_token_ref_price: ONE / 100, // 50_000 tokens > max_mint_per_auction (5_000)
        ..token_params()
    };
    let zero_start = EngineParamsArgs {
        protocol_token_ref_price: 1_000 * ONE, // 500 * RAY / (1_000 * RAY) rounds to 0 tokens
        ..token_params()
    };
    for (name, authority, params) in [
        (
            "MintAuthorityMissing",
            Some(id(k::ATTACKER)),
            token_params(),
        ),
        ("ProtocolTokenMismatch", None, token_params()),
        (
            "MintCapConfig (start_amount above max_mint_per_auction, ADR-024)",
            Some(mint_authority()),
            cap_exceeded,
        ),
        (
            "MintCapConfig (start_amount == 0, ADR-024)",
            Some(mint_authority()),
            zero_start,
        ),
    ] {
        let mut w = system_world(authority, params);
        w.check_solvency().expect("check_solvency");
        assert!(w.host_frozen(), "{name}");
        assert!(acc(&w).host_frozen_by_engine, "{name}");
    }
}

#[test]
fn working_protocol_token_never_freezes() {
    let mut w = system_world(Some(mint_authority()), token_params());
    w.check_solvency()
        .expect("NoAction: a keeper should start a debt auction");
    assert!(!w.host_frozen());
    assert!(!acc(&w).host_frozen_by_engine);
}

#[test]
fn freeze_disabled_takes_no_action_with_a_broken_token() {
    let p = EngineParamsArgs {
        freeze_on_bad_debt: false,
        ..token_params()
    };
    let mut w = system_world(Some(id(k::ATTACKER)), p);
    w.check_solvency().unwrap();
    assert!(!w.host_frozen());
}

#[test]
fn freeze_disabled_still_freezes_without_a_protocol_token() {
    // ADR-017: with no protocol token there is no recovery path, so the flag does not apply.
    let p = EngineParamsArgs {
        freeze_on_bad_debt: false,
        ..EngineParamsArgs::defaults_for_tests()
    };
    let mut w = system_world(None, p);
    w.check_solvency().unwrap();
    assert!(w.host_frozen());
}

// ---- implicit settle ----

#[test]
fn implicit_settle_runs_before_the_decision() {
    let mut w = World::with_engine();
    w.book_shortfall();
    w.donate_stable(buffer(), 355_000);
    w.check_solvency()
        .expect("healthy after the implicit settle");
    assert!(!w.host_frozen());
    assert_eq!(w.balance(buffer()), 0);
    assert_eq!(acc(&w).unbacked_debt, 49);
}

#[test]
fn implicit_settle_then_freeze_burns_and_freezes_in_one_transaction() {
    let mut w = World::with_engine();
    w.book_shortfall();
    w.donate_stable(buffer(), 1_000);
    let supply = w.supply(stable_def());
    w.check_solvency().expect("burn + freeze chain");
    assert!(w.host_frozen());
    assert_eq!(w.balance(buffer()), 0);
    assert_eq!(acc(&w).unbacked_debt, BOOKED - 1_000);
    assert_eq!(w.supply(stable_def()), supply - 1_000);
}

#[test]
fn implicit_settle_keeps_the_debt_auction_reservation_excluded() {
    // Healthy (threshold = booked debt) while a debt auction is live: the implicit settle may only
    // burn `unbacked - debt_on_auction` (Task 14 carry-forward).
    let p = EngineParamsArgs {
        bad_debt_freeze_threshold: BOOKED,
        ..token_params()
    };
    let mut w = system_world(Some(mint_authority()), p);
    w.start_debt(0).unwrap();
    let reserved = acc(&w).debt_on_auction;
    assert!(reserved > 0);
    w.donate_stable(buffer(), 400_000);
    w.check_solvency().expect("healthy");
    let a = acc(&w);
    assert_eq!(a.debt_on_auction, reserved);
    assert_eq!(
        a.unbacked_debt, reserved,
        "only the unreserved debt was burned"
    );
    assert_eq!(w.balance(buffer()), 400_000 - (BOOKED - reserved));
    assert!(!w.host_frozen());
}

// ---- idempotence ----

#[test]
fn second_check_on_a_frozen_host_is_a_harmless_refreeze() {
    let mut w = World::with_engine();
    w.book_shortfall();
    w.check_solvency().expect("first");
    assert!(w.host_frozen());
    w.check_solvency()
        .expect("second: the host's freeze is idempotent");
    assert!(w.host_frozen());
    assert_eq!(acc(&w).unbacked_debt, BOOKED);
    assert!(acc(&w).host_frozen_by_engine);
}

#[test]
fn engine_flag_records_the_freeze_and_a_later_healthy_check_does_not_unfreeze() {
    let mut w = World::with_engine();
    w.book_shortfall();
    w.check_solvency().expect("freeze");
    w.donate_stable(buffer(), 400_000);
    w.check_solvency().expect("healthy now");
    assert_eq!(acc(&w).unbacked_debt, 0);
    assert!(w.host_frozen(), "the engine never unfreezes");
    assert!(acc(&w).host_frozen_by_engine);
}

// ---- permissionless, no caller-forced decisions (ADR-030, Variant C) ----

#[test]
fn any_signer_may_call_but_an_unsigned_caller_may_not() {
    let mut w = World::with_engine();
    w.book_shortfall();
    w.ensure_signer(k::ATTACKER);
    let mut accounts = placeholders(&w);
    accounts[0] = id(k::ATTACKER);
    assert_err(
        w.check_solvency_raw(accounts.clone(), &[]),
        "must be a signer",
    );
    assert!(!w.host_frozen());
    w.check_solvency_raw(accounts, &[k::ATTACKER])
        .expect("anyone");
    assert!(w.host_frozen());
}

#[test]
fn caller_cannot_substitute_the_protocol_token_account() {
    // ADR-030: a wrong slot account cannot turn a working token into "ProtocolTokenMismatch" -> freeze.
    let mut w = system_world(Some(mint_authority()), token_params());
    assert_err(
        w.check_solvency_with(id(k::ATTACKER), unused(SLOT_REF_ORACLE)),
        "WrongAccount",
    );
    assert!(!w.host_frozen());
}

#[test]
fn caller_cannot_inject_accounts_into_unconfigured_slots() {
    // Unconfigured token: the placeholder is the only acceptable slot-1 account; a real definition
    // there (or a made-up oracle in slot 0) is rejected, so the "no token" Freeze cannot be steered.
    let mut w = system_world(
        Some(mint_authority()),
        EngineParamsArgs::defaults_for_tests(),
    );
    assert_err(
        w.check_solvency_with(protocol_token_def(), unused(SLOT_REF_ORACLE)),
        "WrongAccount",
    );
    assert_err(
        w.check_solvency_with(unused(SLOT_PROTOCOL_TOKEN), ref_oracle()),
        "WrongAccount",
    );
    assert!(!w.host_frozen());
    // Configured, working token without a ref oracle: a made-up ref slot is rejected too.
    let mut w = system_world(Some(mint_authority()), token_params());
    assert_err(
        w.check_solvency_with(protocol_token_def(), ref_oracle()),
        "WrongAccount",
    );
    assert!(!w.host_frozen());
}

#[test]
fn forwarded_accounts_must_be_the_engine_validated_ones() {
    // Positions in the R2 #16 list: 3 buffer, 4 stablecoin_definition, 7 engine_authority,
    // 8 liquidation_engine_state, 9 protocol_parameters.
    // The guest's PDA annotations reject buffer / engine_authority first (`PdaMismatch`); the handler
    // re-checks every id (`WrongAccount`) for the rest.
    for (index, what, needle) in [
        (3, "buffer", "PdaMismatch"),
        (4, "stablecoin_definition", "WrongAccount"),
        (7, "engine_authority", "PdaMismatch"),
        (8, "liquidation_engine_state", "WrongAccount"),
        (9, "protocol_parameters", "WrongAccount"),
    ] {
        let mut w = World::with_engine();
        w.book_shortfall();
        let mut accounts = placeholders(&w);
        accounts[index] = id(k::ATTACKER);
        assert_err(w.check_solvency_raw(accounts, &[k::CALLER]), needle);
        assert!(!w.host_frozen(), "{what}");
    }
}

#[test]
fn wrong_clock_is_rejected() {
    let mut w = World::with_engine();
    w.book_shortfall();
    let mut accounts = placeholders(&w);
    accounts[10] = id(k::ATTACKER);
    assert_err(w.check_solvency_raw(accounts, &[k::CALLER]), "WrongClock");
    assert!(!w.host_frozen());
}

// ---- configured protocol-token reference oracle (Task 15 review I2) ----

/// Token configured with a reference oracle; a mint cap that fits the start amount at the admin price
/// (500 * RAY / (2 * RAY) = 250 tokens <= 300) but not at the lowest price a valid oracle may report
/// (1.6 after the 20% clamp: 312 tokens > 300), so an oracle that is actually USED freezes the host.
fn oracle_world() -> World {
    let p = EngineParamsArgs {
        ref_oracle_enabled: true,
        max_mint_per_auction: 300,
        ..token_params()
    };
    let mut w = system_world(Some(mint_authority()), p);
    w.advance(3_600_001);
    w
}

#[test]
fn valid_ref_oracle_is_used_by_the_decision() {
    // Control: a fresh oracle at 1.6 overrides the admin price and the start amount exceeds the cap.
    let mut w = oracle_world();
    w.set_ref_oracle(16 * ONE / 10, w.now);
    w.check_solvency().expect("check_solvency");
    assert!(w.host_frozen());
}

#[test]
fn stale_ref_oracle_falls_back_to_the_admin_price() {
    // The same 1.6 reading, one millisecond past `ref_oracle_max_age_ms`: ignored, no freeze.
    let mut w = oracle_world();
    w.set_ref_oracle(16 * ONE / 10, w.now - 3_600_001);
    w.check_solvency().expect("NoAction");
    assert!(!w.host_frozen());
    assert!(!acc(&w).host_frozen_by_engine);
}

#[test]
fn garbage_ref_oracle_falls_back_to_the_admin_price() {
    // Undecodable data, a zero price and an out-of-range price are all treated as "no oracle".
    for (name, price) in [("zero price", 0), ("far below the admin price", 1)] {
        let mut w = oracle_world();
        w.set_ref_oracle(price, w.now);
        w.check_solvency().expect("NoAction");
        assert!(!w.host_frozen(), "{name}");
    }
    let mut w = oracle_world();
    w.state.force_insert_account(
        ref_oracle(),
        lee_core::account::Account {
            program_owner: [9u32; 8],
            data: lee_core::account::Data::try_from(vec![0xAB; 7]).expect("data fits"),
            ..lee_core::account::Account::default()
        },
    );
    w.check_solvency().expect("NoAction");
    assert!(!w.host_frozen(), "undecodable oracle data");
    assert!(!acc(&w).host_frozen_by_engine);
}

#[test]
fn wrong_ref_oracle_id_is_rejected() {
    // A configured ref oracle is pinned to its id: an attacker-made oracle cannot steer the decision.
    let mut w = oracle_world();
    w.set_ref_oracle(16 * ONE / 10, w.now);
    let token = protocol_token_def();
    assert_err(
        w.check_solvency_with(token, id(k::ATTACKER)),
        "WrongAccount",
    );
    assert_err(
        w.check_solvency_with(token, unused(SLOT_REF_ORACLE)),
        "WrongAccount",
    );
    assert!(!w.host_frozen());
}
