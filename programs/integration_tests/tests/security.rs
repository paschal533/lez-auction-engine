//! Task 16: attack tests through the real lee, token, stablecoin and engine guests.
//!
//! Variant C (ADR-019): the host's engine-only hooks require BOTH `caller_program_id == bound engine
//! program` AND the bound engine-authority PDA authorised. Top-level-only binding (ADR-026).
//! Attacker programs live in `programs/test_programs` (`impostor`, `flawed_engine`).
//!
//! Laundering (ADR-019): lee propagates authorisation down a call chain, so an authority authorised by
//! one program stays authorised for every program it chains to. The REAL engine never authorises
//! `ENGINE_AUTH` for any program but `cfg.host_program_id`, and the host's hooks chain to nothing
//! (invariants (i), (ii)), so no real path exists. The laundering tests therefore bind the host to a
//! `flawed_engine` program that DOES authorise its PDA for a caller-chosen program (S2's
//! `call_untrusted`) and show that the immediate-caller check still rejects the forwarded call.

mod common;

use std::collections::HashMap;

use auction_engine_core::{
    args::{CollateralArgs, EngineParamsArgs},
    pda,
    state::EngineConfigState,
    Instruction as Ix,
};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use common::*;
use lee::{
    execute_and_prove, privacy_preserving_transaction::circuit::ProgramWithDependencies,
    program::Program,
};
use lee_core::{
    account::{AccountId, AccountWithMetadata},
    program::{PdaSeed, ProgramId},
    InputAccountIdentity,
};
use stablecoin_core::{LiquidationEngineState, Position};
use test_programs_core::{FlawedEngineInstruction, ImpostorInstruction};

const NOT_ENGINE: &str = "Caller program is not the liquidation engine";

fn auth_seed_bytes(config_id: &AccountId) -> [u8; 32] {
    pda::combine_seeds(&[&pda::seed_from_str("ENGINE_AUTH"), config_id.value()])
}

fn impostor_world() -> World {
    let mut w = World::with_engine();
    w.deploy(test_programs_methods::IMPOSTOR_ELF);
    w
}

/// An unsafe position, the keeper's collateral holding (the "custody" the hook would pay into) and the
/// ten `liquidate_position` accounts with `authority` first.
fn hook_accounts(w: &mut World, authority: AccountId) -> (AccountId, Vec<AccountId>) {
    let pos = w.unsafe_position();
    w.holding(k::KEEPER_COLL, collateral_def(), 0);
    let accounts = vec![
        authority,
        les(),
        pos,
        host_vault(pos),
        id(k::KEEPER_COLL),
        accumulator(),
        redemption_state(),
        oracle(),
        protocol_parameters(),
        CLOCK_01_PROGRAM_ACCOUNT_ID,
    ];
    (pos, accounts)
}

fn own_pda(program: ProgramId, seed: [u8; 32]) -> AccountId {
    AccountId::for_public_pda(&program, &PdaSeed::new(seed))
}

fn assert_host_untouched(w: &World, pos: Option<AccountId>) {
    assert!(!w.host_frozen(), "the host must not be frozen");
    if let Some(p) = pos {
        let position: Position = w.read(p);
        assert_eq!(
            position.collateral_amount, LIQ_COLLATERAL,
            "position untouched"
        );
    }
}

// ---- impostor: forged flag / own PDA (freeze) ----

#[test]
fn impostor_forging_the_authority_flag_is_rejected() {
    // The host's immediate-caller check fires first (S2 table, variant C, attack 3a); were it absent,
    // lee would still reject the forged flag (InvalidAccountAuthorization).
    let mut w = impostor_world();
    let r = w.submit(
        test_programs_methods::IMPOSTOR_ID,
        vec![engine_authority(), les(), protocol_parameters()],
        &[k::ATTACKER],
        ImpostorInstruction::AttackFreeze {
            host_program_id: host_program(),
            mode: 0,
            seed: [0; 32],
        },
    );
    assert_err(r, NOT_ENGINE);
    assert!(!w.host_frozen());
}

#[test]
fn impostor_own_pda_is_rejected_by_the_immediate_caller_check() {
    let mut w = impostor_world();
    let seed = auth_seed_bytes(&config());
    let own = own_pda(test_programs_methods::IMPOSTOR_ID, seed);
    let r = w.submit(
        test_programs_methods::IMPOSTOR_ID,
        vec![own, les(), protocol_parameters()],
        &[k::ATTACKER],
        ImpostorInstruction::AttackFreeze {
            host_program_id: host_program(),
            mode: 1,
            seed,
        },
    );
    assert_err(r, NOT_ENGINE);
    assert!(!w.host_frozen());
}

// ---- impostor chaining liquidate_position / both hooks (Task 7 ruling, item A(b)) ----

/// Forged flag: the host keeps its real binding and the impostor claims the real engine authority.
fn forged_world() -> (World, u8, [u8; 32], AccountId) {
    (impostor_world(), 0, [0; 32], engine_authority())
}

/// Rebound: the host is bound to (real engine program id, the impostor's OWN PDA) so the authority id
/// matches and is legitimately authorised by the impostor's seed: only the immediate-caller check is
/// left standing.
fn rebound_world() -> (World, u8, [u8; 32], AccountId) {
    let seed = auth_seed_bytes(&config());
    let own = own_pda(test_programs_methods::IMPOSTOR_ID, seed);
    let mut w = impostor_world();
    w.bind_engine_to(engine_program(), own)
        .expect("admin rebinds");
    (w, 1, seed, own)
}

fn run_attack_liquidate(world: (World, u8, [u8; 32], AccountId)) {
    let (mut w, mode, seed, authority) = world;
    let (pos, accounts) = hook_accounts(&mut w, authority);
    let r = w.submit(
        test_programs_methods::IMPOSTOR_ID,
        accounts,
        &[k::ATTACKER],
        ImpostorInstruction::AttackLiquidate {
            host_program_id: host_program(),
            mode,
            seed,
            collateral_to_seize: LIQ_COLLATERAL,
            normalized_debt_to_cover: LIQ_DEBT,
        },
    );
    assert_err(r, NOT_ENGINE);
    assert_host_untouched(&w, Some(pos));
}

/// One transaction chains engine_freeze_new_debt AND liquidate_position; the first guard stops it.
fn run_attack_both(world: (World, u8, [u8; 32], AccountId)) {
    let (mut w, mode, seed, authority) = world;
    let (pos, accounts) = hook_accounts(&mut w, authority);
    let r = w.submit(
        test_programs_methods::IMPOSTOR_ID,
        accounts,
        &[k::ATTACKER],
        ImpostorInstruction::AttackBoth {
            host_program_id: host_program(),
            mode,
            seed,
            collateral_to_seize: LIQ_COLLATERAL,
            normalized_debt_to_cover: LIQ_DEBT,
        },
    );
    assert_err(r, NOT_ENGINE);
    assert_host_untouched(&w, Some(pos));
}

#[test]
fn impostor_chaining_liquidate_position_with_a_forged_flag_is_rejected() {
    run_attack_liquidate(forged_world());
}

#[test]
fn impostor_chaining_liquidate_position_with_a_rebound_own_pda_is_rejected() {
    run_attack_liquidate(rebound_world());
}

#[test]
fn impostor_chaining_both_hooks_with_a_forged_flag_is_rejected() {
    run_attack_both(forged_world());
}

#[test]
fn impostor_chaining_both_hooks_with_a_rebound_own_pda_is_rejected() {
    run_attack_both(rebound_world());
}

// ---- laundering (Task 7 ruling, item A(c)) ----

/// Host bound to the flawed engine (program id AND its own authority PDA); the flawed engine authorises
/// that PDA for the impostor, which forwards the received flags to the host.
fn laundering_world() -> (World, [u8; 32], AccountId) {
    let mut w = World::with_engine();
    w.deploy(test_programs_methods::IMPOSTOR_ELF);
    w.deploy(test_programs_methods::FLAWED_ENGINE_ELF);
    let seed = auth_seed_bytes(&config());
    let authority = own_pda(test_programs_methods::FLAWED_ENGINE_ID, seed);
    w.bind_engine_to(test_programs_methods::FLAWED_ENGINE_ID, authority)
        .expect("admin binds the flawed engine");
    (w, seed, authority)
}

#[test]
fn laundering_freeze_through_an_intermediate_program_is_rejected() {
    // flawed_engine (bound, authorises its PDA) -> impostor (forwards flags) -> host.engine_freeze_new_debt.
    // The authority id matches and is authorised (lee propagates it); only the immediate caller (the
    // impostor, not the bound engine) is wrong.
    let (mut w, seed, authority) = laundering_world();
    let r = w.submit(
        test_programs_methods::FLAWED_ENGINE_ID,
        vec![authority, les(), protocol_parameters()],
        &[k::ATTACKER],
        FlawedEngineInstruction::LaunderFreeze {
            target_program_id: test_programs_methods::IMPOSTOR_ID,
            host_program_id: host_program(),
            seed,
        },
    );
    assert_err(r, NOT_ENGINE);
    assert!(!w.host_frozen());
}

#[test]
fn laundering_liquidate_through_an_intermediate_program_is_rejected() {
    let (mut w, seed, authority) = laundering_world();
    let (pos, accounts) = hook_accounts(&mut w, authority);
    let r = w.submit(
        test_programs_methods::FLAWED_ENGINE_ID,
        accounts,
        &[k::ATTACKER],
        FlawedEngineInstruction::LaunderLiquidate {
            target_program_id: test_programs_methods::IMPOSTOR_ID,
            host_program_id: host_program(),
            seed,
            collateral_to_seize: LIQ_COLLATERAL,
            normalized_debt_to_cover: LIQ_DEBT,
        },
    );
    assert_err(r, NOT_ENGINE);
    assert_host_untouched(&w, Some(pos));
}

// ---- set_liquidation_engine is top-level only (ADR-026; Task 7 ruling, item A(d)) ----

#[test]
fn set_liquidation_engine_through_a_chained_call_is_rejected() {
    // The admin signs the impostor transaction; the impostor forwards the genuine signature to
    // set_liquidation_engine. The host requires caller_program_id == 0 and rejects it.
    let mut w = impostor_world();
    let r = w.submit(
        test_programs_methods::IMPOSTOR_ID,
        vec![id(k::HOST_ADMIN), protocol_parameters(), les()],
        &[k::HOST_ADMIN],
        ImpostorInstruction::AttackSetEngine {
            host_program_id: host_program(),
            engine_program_id: test_programs_methods::IMPOSTOR_ID,
            engine_authority_id: id(k::ATTACKER),
        },
    );
    assert_err(r, "set_liquidation_engine must be a top-level call");
    let bound: LiquidationEngineState = w.read(les());
    assert_eq!(bound.engine_program_id, engine_program(), "binding intact");
    assert_eq!(bound.engine_authority_id, engine_authority());
}

// ---- rogue config, program-id surface, fake host ----

#[test]
fn rogue_config_with_the_same_salt_cannot_liquidate_the_bound_host() {
    // ADR-027 (I7): anyone may create a config on the same host, even with the bound config's salt; it is a
    // different PDA (the admin is in the seed) and only the bound config's authority passes the hook.
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    let e = engine_program();
    let rogue = pda::config_pda(&e, &id(k::ATTACKER), SALT);
    assert_ne!(rogue, config());
    let rogue_params = EngineParamsArgs {
        global_cap: 100_000_000,
        ..EngineParamsArgs::defaults_for_tests()
    };
    w.engine(
        vec![
            id(k::ATTACKER),
            rogue,
            pda::accounting_pda(&e, &rogue),
            pda::buffer_pda(&e, &rogue),
            protocol_parameters(),
            stable_def(),
            pda::unused_slot_pda(&e, &rogue, 1),
            pda::unused_slot_pda(&e, &rogue, 0),
        ],
        &[k::ATTACKER],
        init_config_ix(SALT, &rogue_params),
    )
    .expect("anyone can create a config");
    let greedy = CollateralArgs {
        penalty_bps: 2_500,
        tip: 1_000,
        chip_bps: 2_500,
        reward_cap: 1_000_000,
        ..CollateralArgs::defaults_for_tests()
    };
    w.engine(
        vec![
            id(k::ATTACKER),
            rogue,
            collateral_def(),
            pda::coll_config_pda(&e, &rogue, &collateral_def()),
            oracle(),
            protocol_parameters(),
            redemption_state(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        &[k::ATTACKER],
        collateral_ix(&greedy),
    )
    .expect("rogue collateral config");
    w.holding(k::KEEPER_COLL, collateral_def(), 0);
    let auction_id = pda::auction_pda(&e, &rogue, &pos, 0);
    let r = w.engine(
        vec![
            id(k::KEEPER),
            id(k::KEEPER_COLL),
            rogue,
            pda::coll_config_pda(&e, &rogue, &collateral_def()),
            pos,
            pda::liqrec_pda(&e, &rogue, &pos),
            auction_id,
            pda::vault_pda(&e, &auction_id),
            pda::engine_authority_pda(&e, &rogue),
            les(),
            host_vault(pos),
            collateral_def(),
            accumulator(),
            redemption_state(),
            oracle(),
            protocol_parameters(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        &[k::KEEPER],
        Ix::Liquidate { nonce: 0 },
    );
    assert_err(
        r,
        "Engine authority account is not the bound engine authority",
    );
    let p: Position = w.read(pos);
    assert_eq!(p.collateral_amount, LIQ_COLLATERAL, "position untouched");
}

#[test]
fn engine_takes_no_program_id_argument() {
    // Documentation only (M5): records that no instruction argument is a program id. Invariant (i) itself
    // is enforced in code (`cfg.host_program_id` / `cfg.token_program_id` only) and reviewed in Task 19.
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../idl/auction_engine-idl.json"
    );
    let idl = std::fs::read_to_string(path).expect("engine IDL");
    assert!(
        !idl.contains("program_id"),
        "an engine instruction takes a program id argument"
    );
}

#[test]
fn fake_host_config_is_accepted_so_clients_must_verify_program_ids() {
    // I4: init_config takes the host program id from the ProtocolParameters owner. A config built on a
    // fake "host" is accepted; every later engine call would chain to that program (and to its token
    // program) with the signer flags the user gives. The defence is client-side: the SDK/CLI refuses any
    // config whose host_program_id / token_program_id differ from the ImageIDs in DEPLOYMENTS.md.
    let mut w = World::with_engine();
    w.deploy(test_programs_methods::IMPOSTOR_ELF);
    let fake_host = test_programs_methods::IMPOSTOR_ID;
    let fake_pp = stablecoin_core::compute_protocol_parameters_pda(fake_host);
    let mut pp = w.state.get_account_by_id(protocol_parameters());
    pp.program_owner = fake_host;
    w.state.force_insert_account(fake_pp, pp);
    let e = engine_program();
    let fake_cfg = pda::config_pda(&e, &id(k::ATTACKER), 7);
    w.engine(
        vec![
            id(k::ATTACKER),
            fake_cfg,
            pda::accounting_pda(&e, &fake_cfg),
            pda::buffer_pda(&e, &fake_cfg),
            fake_pp,
            stable_def(),
            pda::unused_slot_pda(&e, &fake_cfg, 1),
            pda::unused_slot_pda(&e, &fake_cfg, 0),
        ],
        &[k::ATTACKER],
        init_config_ix(7, &EngineParamsArgs::defaults_for_tests()),
    )
    .expect("the engine cannot tell a fake host from a real one");
    let cfg: EngineConfigState = w.read(fake_cfg);
    assert_eq!(cfg.host_program_id, fake_host);
    assert_ne!(
        cfg.host_program_id,
        host_program(),
        "a client checking DEPLOYMENTS.md refuses this config"
    );
}

#[test]
fn engine_authority_cannot_be_signed_at_top_level() {
    // A user transaction listing the engine authority cannot authorise it (no key exists for a PDA).
    let mut w = World::with_engine();
    let r = w.host(
        vec![engine_authority(), les(), protocol_parameters()],
        &[],
        stablecoin_core::Instruction::EngineFreezeNewDebt,
    );
    assert_err(r, "must be a signer"); // SPEL's `#[account(signer)]` check, before the handler runs
}

// ---- private path (ADR-034) ----

fn with_deps(main_elf: &[u8]) -> ProgramWithDependencies {
    ProgramWithDependencies::new(
        Program::new(main_elf.to_vec().into()).expect("valid ELF"),
        HashMap::from([
            (
                host_program(),
                Program::new(stablecoin_methods::STABLECOIN_ELF.to_vec().into())
                    .expect("stablecoin ELF"),
            ),
            (
                token_program(),
                Program::new(token_methods::TOKEN_ELF.to_vec().into()).expect("token ELF"),
            ),
        ]),
    )
}

fn public_pre(w: &World, ids: &[AccountId], signers: &[AccountId]) -> Vec<AccountWithMetadata> {
    ids.iter()
        .map(|i| AccountWithMetadata::new(w.state.get_account_by_id(*i), signers.contains(i), *i))
        .collect()
}

#[test]
fn private_path_impostor_is_rejected_by_the_immediate_caller_check() {
    // ADR-019 open item: caller_program_id is enforced per call in the privacy circuit too.
    let w = World::with_engine();
    let seed = auth_seed_bytes(&config());
    let own = own_pda(test_programs_methods::IMPOSTOR_ID, seed);
    let ids = [own, les(), protocol_parameters()];
    let result = execute_and_prove(
        public_pre(&w, &ids, &[]),
        Program::serialize_instruction(ImpostorInstruction::AttackFreeze {
            host_program_id: host_program(),
            mode: 1,
            seed,
        })
        .expect("instruction"),
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
        ],
        &with_deps(test_programs_methods::IMPOSTOR_ELF),
    );
    let text = format!(
        "{:?}",
        result.expect_err("the impostor must be rejected on the private path")
    );
    println!("PRIVATE-PATH impostor: {text}");
    assert!(text.contains(NOT_ENGINE), "{text}");
}

#[test]
fn private_path_liquidate_is_not_expressible_on_v0_2_4() {
    // ADR-034: the auction vault is authorised by its seed in InitializeAccount and passed unauthorised to
    // the hook; the circuit's transaction-wide authorisation set rejects that, as it rejects upstream
    // open_position (programs/upstream_integration_tests/tests/stablecoin.rs:775-850).
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    w.holding(k::KEEPER_COLL, collateral_def(), 0);
    let ids = w.liquidate_accounts(pos, 0, id(k::KEEPER_COLL));
    let identities = ids.iter().map(|_| InputAccountIdentity::Public).collect();
    let result = execute_and_prove(
        public_pre(&w, &ids, &[id(k::KEEPER)]),
        Program::serialize_instruction(Ix::Liquidate { nonce: 0 }).expect("instruction"),
        identities,
        &with_deps(auction_engine_methods::AUCTION_ENGINE_ELF),
    );
    let text = format!(
        "{:?}",
        result.expect_err("liquidate is not private-path capable on v0.2.4")
    );
    println!("PRIVATE-PATH liquidate: {text}");
    assert!(
        text.contains("Inconsistent authorization for account"),
        "{text}"
    );
}
