//! R4/R5 (spec §5.4, §9): only the instance whose feed is invalid (or that the admin paused) stops.
//! Instance B = the same stablecoin source built with feature `instance-b` (distinct ImageID, S1).

mod common;

use auction_engine_core::{
    args::{CollateralArgs, EngineParamsArgs},
    pda, Instruction as Ix,
};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID as CLOCK;
use common::*;
use lee_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use stablecoin_core::{
    compute_liquidation_engine_state_pda, compute_position_pda, compute_position_vault_pda,
    compute_protocol_parameters_pda, compute_redemption_price_state_pda,
    compute_stability_fee_accumulator_pda, compute_stablecoin_definition_pda,
    compute_stablecoin_master_holding_pda,
};
use token_core::TokenDefinition;
use twap_oracle_core::OraclePriceAccount;

const KEEPER_B_COLL: u8 = 69;

fn host_b() -> ProgramId {
    stablecoin_methods_b::STABLECOIN_ID
}
fn coll_b() -> AccountId {
    AccountId::new([6; 32])
}
fn oracle_b() -> AccountId {
    AccountId::new([0x72; 32])
}
fn pp_b() -> AccountId {
    compute_protocol_parameters_pda(host_b())
}
fn acc_b() -> AccountId {
    compute_stability_fee_accumulator_pda(host_b())
}
fn rps_b() -> AccountId {
    compute_redemption_price_state_pda(host_b())
}
fn stable_b() -> AccountId {
    compute_stablecoin_definition_pda(host_b())
}
fn les_b() -> AccountId {
    compute_liquidation_engine_state_pda(host_b())
}
fn config_b() -> AccountId {
    pda::config_pda(&engine_program(), &id(k::ENGINE_ADMIN), 2)
}
fn coll_config_b() -> AccountId {
    pda::coll_config_pda(&engine_program(), &config_b(), &coll_b())
}

fn set_oracle_b(w: &mut World, price: u128, ts: u64) {
    w.state.force_insert_account(
        oracle_b(),
        Account {
            program_owner: [9u32; 8],
            balance: 0,
            data: Data::from(&OraclePriceAccount {
                base_asset: stable_b(),
                quote_asset: coll_b(),
                price,
                timestamp: ts,
                source_id: oracle_b(),
                confidence_interval: 0,
            }),
            nonce: Nonce(0),
        },
    );
}

fn two_instances() -> (World, AccountId, AccountId) {
    let mut w = World::with_engine();
    w.deploy(stablecoin_methods_b::STABLECOIN_ELF);
    w.state.force_insert_account(
        coll_b(),
        Account {
            program_owner: token_program(),
            balance: 0,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("COLLB"),
                total_supply: 0,
                metadata_id: None,
                authority: None,
            }),
            nonce: Nonce(0),
        },
    );
    let now = w.now;
    set_oracle_b(&mut w, R, now);
    w.submit(
        host_b(),
        vec![
            id(k::HOST_ADMIN),
            pp_b(),
            acc_b(),
            rps_b(),
            stable_b(),
            compute_stablecoin_master_holding_pda(host_b()),
            coll_b(),
            oracle_b(),
            CLOCK,
        ],
        &[k::HOST_ADMIN],
        stablecoin_core::Instruction::InitializeProgram {
            freeze_authority_account_id: id(k::FREEZE),
            initial_stability_fee_per_millisecond: ONE,
            initial_controller_proportional_gain: 0,
            initial_controller_integral_gain: 0,
            initial_minimum_collateralization_ratio: ONE * 3 / 2,
            minimum_milliseconds_between_rate_updates: 300_000,
            maximum_oracle_price_age_milliseconds: 900_000,
            initial_redemption_price: R,
            stablecoin_name: String::from("LSCB"),
        },
    )
    .expect("initialize host B");
    let e = engine_program();
    w.engine(
        vec![
            id(k::ENGINE_ADMIN),
            config_b(),
            pda::accounting_pda(&e, &config_b()),
            pda::buffer_pda(&e, &config_b()),
            pp_b(),
            stable_b(),
            pda::unused_slot_pda(&e, &config_b(), 1),
            pda::unused_slot_pda(&e, &config_b(), 0),
        ],
        &[k::ENGINE_ADMIN],
        init_config_ix(2, &EngineParamsArgs::defaults_for_tests()),
    )
    .expect("engine config B");
    w.submit(
        host_b(),
        vec![id(k::HOST_ADMIN), pp_b(), les_b()],
        &[k::HOST_ADMIN],
        stablecoin_core::Instruction::SetLiquidationEngine {
            engine_program_id: e,
            engine_authority_id: pda::engine_authority_pda(&e, &config_b()),
        },
    )
    .expect("bind B");
    w.engine(
        vec![
            id(k::ENGINE_ADMIN),
            config_b(),
            coll_b(),
            coll_config_b(),
            oracle_b(),
            pp_b(),
            rps_b(),
            CLOCK,
        ],
        &[k::ENGINE_ADMIN],
        collateral_ix(&CollateralArgs::defaults_for_tests()),
    )
    .expect("collateral B");

    let pos_a = w.open_position(
        k::OWNER,
        k::OWNER_COLL,
        k::OWNER_STABLE,
        0,
        LIQ_COLLATERAL,
        LIQ_DEBT,
    );
    w.ensure_signer(k::OWNER2);
    w.holding(k::OWNER2_COLL, coll_b(), LIQ_COLLATERAL);
    w.holding(k::OWNER2_STABLE, stable_b(), 0);
    let pos_b = compute_position_pda(host_b(), id(k::OWNER2), 0);
    w.submit(
        host_b(),
        vec![
            id(k::OWNER2),
            pos_b,
            compute_position_vault_pda(host_b(), pos_b),
            id(k::OWNER2_COLL),
            coll_b(),
            pp_b(),
            CLOCK,
        ],
        &[k::OWNER2, k::OWNER2_COLL],
        stablecoin_core::Instruction::OpenPosition {
            position_nonce: 0,
            initial_collateral_amount: LIQ_COLLATERAL,
        },
    )
    .expect("open B");
    w.submit(
        host_b(),
        vec![
            id(k::OWNER2),
            pos_b,
            stable_b(),
            id(k::OWNER2_STABLE),
            acc_b(),
            rps_b(),
            oracle_b(),
            pp_b(),
            CLOCK,
        ],
        &[k::OWNER2],
        stablecoin_core::Instruction::GenerateDebt { amount: LIQ_DEBT },
    )
    .expect("borrow B");
    w.set_min_cr(2 * ONE);
    w.submit(
        host_b(),
        vec![id(k::HOST_ADMIN), pp_b()],
        &[k::HOST_ADMIN],
        stablecoin_core::Instruction::SetMinimumCollateralizationRatio { new_ratio: 2 * ONE },
    )
    .expect("CR B");
    (w, pos_a, pos_b)
}

fn liquidate_b(w: &mut World, pos_b: AccountId) -> Result<(), lee::error::LeeError> {
    let e = engine_program();
    w.holding(KEEPER_B_COLL, coll_b(), 0);
    let auction_b = pda::auction_pda(&e, &config_b(), &pos_b, 0);
    w.engine(
        vec![
            id(k::KEEPER),
            id(KEEPER_B_COLL),
            config_b(),
            coll_config_b(),
            pos_b,
            pda::liqrec_pda(&e, &config_b(), &pos_b),
            auction_b,
            pda::vault_pda(&e, &auction_b),
            pda::engine_authority_pda(&e, &config_b()),
            les_b(),
            compute_position_vault_pda(host_b(), pos_b),
            coll_b(),
            acc_b(),
            rps_b(),
            oracle_b(),
            pp_b(),
            CLOCK,
        ],
        &[k::KEEPER],
        Ix::Liquidate { nonce: 0 },
    )
}

#[test]
fn instances_have_distinct_programs_and_state() {
    let (w, _, _) = two_instances();
    assert_ne!(host_b(), host_program());
    assert_ne!(pp_b(), protocol_parameters());
    assert_ne!(config_b(), config());
    let _ = w;
}

#[test]
fn stale_feed_on_a_does_not_pause_b() {
    let (mut w, pos_a, pos_b) = two_instances();
    w.advance(3_600_001);
    let now = w.now;
    set_oracle_b(&mut w, R, now);
    assert_err(w.liquidate(pos_a, 0), "PriceFeedStale");
    liquidate_b(&mut w, pos_b).expect("instance B keeps liquidating");
}

#[test]
fn admin_pause_on_a_does_not_pause_b() {
    let (mut w, pos_a, pos_b) = two_instances();
    let paused = CollateralArgs {
        admin_paused: true,
        ..CollateralArgs::defaults_for_tests()
    };
    w.set_collateral_config(k::ENGINE_ADMIN, &paused).unwrap();
    assert_err(w.liquidate(pos_a, 0), "Paused");
    liquidate_b(&mut w, pos_b).expect("instance B unaffected");
}
