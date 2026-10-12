//! The "standard world" shared by SDK, CLI and golden tests (numbers in Plan 3a Task 5). Accounts are
//! inserted directly (no programs run); every value is consistent with the engine's own state rules.

use auction_core::units::RAY;
use auction_engine_core::{
    args::{CollateralArgs, EngineParamsArgs},
    state::*,
    Instruction,
};
use lee_core::{account::AccountId, program::ProgramId};
use stablecoin_core::{
    LiquidationEngineState, Position, ProtocolParameters, RedemptionPriceState,
    StabilityFeeAccumulator,
};
use token_core::TokenDefinition;
use twap_oracle_core::OraclePriceAccount;

use super::{id, FixtureChain};
use crate::{
    addresses::{EngineAddresses, HostAddresses},
    chain::TxSummary,
    deployment::Deployment,
    encode::words,
};

pub const ENGINE: ProgramId = [0xE1; 8];
pub const HOST: ProgramId = [0xA1; 8];
pub const TOKEN: ProgramId = [0xB1; 8];
pub const TWAP: ProgramId = [0xC1; 8];
pub const ORACLE_OWNER: ProgramId = [9; 8];
pub const T0: u64 = 1_700_000_000_000;
pub const ONE: u128 = RAY;
pub const R: u128 = RAY / 2;
pub const SALT: u64 = 1;
pub const COLLATERAL: AccountId = AccountId::new([0x30; 32]);
pub const ORACLE: AccountId = AccountId::new([0x50; 32]);
pub const PROTOCOL: AccountId = AccountId::new([0x91; 32]);

/// Key seeds (same values as the integration harness's `common::k`).
pub mod k {
    pub const OWNER: u8 = 41;
    pub const OWNER_COLL: u8 = 42;
    pub const OWNER_STABLE: u8 = 43;
    pub const HOST_ADMIN: u8 = 44;
    pub const FREEZE: u8 = 45;
    pub const ENGINE_ADMIN: u8 = 50;
    pub const KEEPER: u8 = 51;
    pub const KEEPER_COLL: u8 = 52;
    pub const BIDDER_STABLE: u8 = 53;
    pub const BIDDER_COLL: u8 = 54;
    pub const TAKER_STABLE: u8 = 57;
    pub const TAKER_PROTOCOL: u8 = 58;
    pub const CALLER: u8 = 62;
    pub const OWNER2: u8 = 65;
}

pub struct Fixture {
    pub chain: FixtureChain,
    pub deployment: Deployment,
    pub engine: EngineAddresses,
    pub host: HostAddresses,
    pub unsafe_position: AccountId,
    pub auction_position: AccountId,
    pub live_auction: AccountId,
}

#[must_use]
pub fn config_id() -> AccountId {
    EngineAddresses::new(ENGINE, &id(k::ENGINE_ADMIN), SALT).config
}

#[must_use]
pub fn deployment() -> Deployment {
    Deployment {
        network: "fixture".into(),
        sequencer_url: String::new(),
        engine_program: ENGINE,
        host_program: HOST,
        token_program: TOKEN,
        twap_oracle_program: Some(TWAP),
        engine_admin: Some(id(k::ENGINE_ADMIN)),
        config: Some(config_id()),
    }
}

#[must_use]
pub fn oracle_state(price: u128, timestamp: u64, host: &HostAddresses) -> OraclePriceAccount {
    OraclePriceAccount {
        base_asset: host.stablecoin_definition,
        quote_asset: COLLATERAL,
        price,
        timestamp,
        source_id: ORACLE,
        confidence_interval: 0,
    }
}

fn schedule_recs() -> Vec<SchedulePointRec> {
    vec![
        SchedulePointRec {
            elapsed_ms: 0,
            discount_bps: 200,
        },
        SchedulePointRec {
            elapsed_ms: 2_700_000,
            discount_bps: 1_000,
        },
    ]
}

fn put_host(c: &FixtureChain, host: &HostAddresses, engine: &EngineAddresses) {
    c.put_state(
        host.protocol_parameters,
        HOST,
        &ProtocolParameters {
            admin_account_id: id(k::HOST_ADMIN),
            freeze_authority_account_id: id(k::FREEZE),
            stablecoin_definition_id: host.stablecoin_definition,
            collateral_definition_id: COLLATERAL,
            market_price_oracle_id: ORACLE,
            stability_fee_per_millisecond: ONE,
            controller_proportional_gain: 0,
            controller_integral_gain: 0,
            minimum_collateralization_ratio: 2 * ONE,
            minimum_milliseconds_between_rate_updates: 300_000,
            maximum_oracle_price_age_milliseconds: 900_000,
            is_frozen: false,
        },
    );
    c.put_state(
        host.stability_fee_accumulator,
        HOST,
        &StabilityFeeAccumulator {
            accumulated_rate_at_last_accrual: ONE,
            last_accrued_at: T0,
        },
    );
    c.put_state(
        host.redemption_price_state,
        HOST,
        &RedemptionPriceState {
            redemption_price_at_last_update: R,
            redemption_rate_per_millisecond: ONE,
            controller_integral_term: 0,
            last_updated_at: T0,
        },
    );
    c.put_state(
        host.liquidation_engine_state,
        HOST,
        &LiquidationEngineState {
            engine_program_id: ENGINE,
            engine_authority_id: engine.engine_authority,
        },
    );
    for (def, name, authority) in [
        (host.stablecoin_definition, "LSC", None),
        (COLLATERAL, "COLL", None),
        (PROTOCOL, "MOCK", Some(engine.mint_authority)),
    ] {
        c.put_state(
            def,
            TOKEN,
            &TokenDefinition::Fungible {
                name: name.into(),
                total_supply: 10_000_000,
                metadata_id: None,
                authority,
            },
        );
    }
    c.put_state(ORACLE, ORACLE_OWNER, &oracle_state(R, T0, host));
}

fn put_engine(c: &FixtureChain, host: &HostAddresses, e: &EngineAddresses) {
    let system = EngineParamsArgs::defaults_for_tests()
        .validate()
        .expect("valid defaults");
    c.put_state(
        e.config,
        ENGINE,
        &EngineConfigState {
            tag: TAG_CONFIG,
            salt: SALT,
            admin: id(k::ENGINE_ADMIN),
            host_program_id: HOST,
            host_protocol_parameters_id: host.protocol_parameters,
            stablecoin_definition_id: host.stablecoin_definition,
            collateral_definition_id: COLLATERAL,
            token_program_id: TOKEN,
            protocol_token: Some(PROTOCOL),
            protocol_token_ref_price: 2 * ONE,
            protocol_token_oracle_id: None,
            ref_oracle_max_age_ms: 3_600_000,
            global_cap: 10_000_000,
            global_reserved: 660_000,
            freeze_on_bad_debt: true,
            bad_debt_freeze_threshold: 1_000,
            system: SystemParamsRec::from_core(&system),
        },
    );
    let c_args = CollateralArgs::defaults_for_tests();
    c.put_state(
        e.coll_config(&COLLATERAL),
        ENGINE,
        &CollateralConfigState {
            tag: TAG_COLL,
            config: e.config,
            collateral_definition_id: COLLATERAL,
            oracle_id: ORACLE,
            oracle_scale: c_args.oracle_scale,
            max_oracle_age_ms: c_args.max_oracle_age_ms,
            max_conf_bps: 100,
            band_lo_bps: 9_000,
            band_hi_bps: 11_000,
            penalty_bps: 1_000,
            tip: c_args.tip,
            chip_bps: 10,
            reward_cap: c_args.reward_cap,
            debt_floor: c_args.debt_floor,
            min_bid: c_args.min_bid,
            type_cap: c_args.type_cap,
            type_reserved: 660_000,
            schedule: schedule_recs(),
            admin_paused: false,
            paused_since_ms: None,
            paused_total_ms: 0,
            last_seen_oracle_ts: Some(T0),
        },
    );
    c.put_state(
        e.accounting,
        ENGINE,
        &AccountingState {
            unbacked_debt: 1_000,
            debt_on_auction: 500,
            debt_live: true,
            debt_nonce: 1,
            surplus_live: true,
            surplus_nonce: 1,
            total_surplus_lots_out: 500,
            ..AccountingState::new()
        },
    );
    c.put_holding(e.buffer, TOKEN, host.stablecoin_definition, 2_000);
    c.put_state(
        e.surplus_auction(0),
        ENGINE,
        &SurplusAuctionState {
            tag: TAG_SURPLUS,
            config: e.config,
            nonce: 0,
            lot_left: 500,
            start_ms: T0 - 600_000,
            live: true,
            ref_ray: 2 * ONE,
        },
    );
    c.put_holding(e.surplus_vault(0), TOKEN, host.stablecoin_definition, 500);
    c.put_state(
        e.debt_auction(0),
        ENGINE,
        &DebtAuctionState {
            tag: TAG_DEBT,
            config: e.config,
            nonce: 0,
            lot_left: 500,
            start_amount: 250,
            start_ms: T0 - 600_000,
            live: true,
        },
    );
}

fn tx(program_id: ProgramId, account_ids: Vec<AccountId>, words: Vec<u32>, n: u8) -> TxSummary {
    TxSummary {
        hash: [n; 32],
        program_id,
        account_ids,
        instruction_words: words,
    }
}

#[must_use]
pub fn standard() -> Fixture {
    let chain = FixtureChain::new();
    let host = HostAddresses::new(HOST);
    let engine = EngineAddresses::new(ENGINE, &id(k::ENGINE_ADMIN), SALT);
    chain.set_clock(T0);
    put_host(&chain, &host, &engine);
    put_engine(&chain, &host, &engine);

    let p0 = host.position(&id(k::OWNER), 0);
    chain.put_state(
        p0,
        HOST,
        &Position {
            owner_account_id: id(k::OWNER),
            position_nonce: 0,
            vault_account_id: host.position_vault(&p0),
            collateral_amount: 500_000,
            normalized_debt_amount: 600_000,
            opened_at: T0 - 1_000_000,
        },
    );
    chain.put_holding(host.position_vault(&p0), TOKEN, COLLATERAL, 500_000);

    let p1 = host.position(&id(k::OWNER2), 0);
    chain.put_state(
        p1,
        HOST,
        &Position {
            owner_account_id: id(k::OWNER2),
            position_nonce: 0,
            vault_account_id: host.position_vault(&p1),
            collateral_amount: 0,
            normalized_debt_amount: 0,
            opened_at: T0 - 2_000_000,
        },
    );
    chain.put_state(
        engine.record(&p1),
        ENGINE,
        &LiquidationRecordState {
            tag: TAG_LIQREC,
            position: p1,
            count: 1,
        },
    );
    // The liquidated position's host vault stays initialised (empty), as after a real liquidation.
    chain.put_holding(host.position_vault(&p1), TOKEN, COLLATERAL, 0);
    let a = engine.auction(&p1, 0);
    chain.put_state(
        a,
        ENGINE,
        &AuctionState {
            tag: TAG_AUCTION,
            config: engine.config,
            position: p1,
            nonce: 0,
            owner: id(k::OWNER2),
            keeper: id(k::KEEPER),
            settled: false,
            collateral_seized: 500_000,
            collateral_sold: 0,
            collateral_left: 475_000,
            keeper_reward: 25,
            leftover_claimable: 0,
            leftover_claimed: 0,
            debt_principal: 600_000,
            amount_to_raise: 660_000,
            reserved_amount: 660_000,
            debt_raised: 0,
            penalty_raised: 0,
            start_ms: T0 - 600_000,
            schedule: schedule_recs(),
            type_paused_total_at_start: 0,
        },
    );
    chain.put_holding(engine.auction_vault(&a), TOKEN, COLLATERAL, 475_000);

    chain.put_holding(
        id(k::BIDDER_STABLE),
        TOKEN,
        host.stablecoin_definition,
        1_000_000,
    );
    chain.put_holding(id(k::BIDDER_COLL), TOKEN, COLLATERAL, 0);
    chain.put_holding(id(k::OWNER_COLL), TOKEN, COLLATERAL, 0);
    chain.put_holding(
        id(k::TAKER_STABLE),
        TOKEN,
        host.stablecoin_definition,
        1_000,
    );
    chain.put_holding(id(k::TAKER_PROTOCOL), TOKEN, PROTOCOL, 10_000);

    let open = |owner: u8, pos: AccountId| {
        let ix = stablecoin_core::Instruction::OpenPosition {
            position_nonce: 0,
            initial_collateral_amount: 500_000,
        };
        tx(
            HOST,
            vec![
                id(owner),
                pos,
                host.position_vault(&pos),
                AccountId::new([owner; 32]),
                COLLATERAL,
                host.protocol_parameters,
            ],
            words(&ix).expect("encodes"),
            owner,
        )
    };
    chain.push_block(vec![open(k::OWNER2, p1)]);
    chain.push_block(vec![open(k::OWNER, p0)]);
    let liq_accounts = vec![
        id(k::KEEPER),
        id(k::KEEPER_COLL),
        engine.config,
        engine.coll_config(&COLLATERAL),
        p1,
        engine.record(&p1),
        a,
        engine.auction_vault(&a),
    ];
    chain.push_block(vec![tx(
        ENGINE,
        liq_accounts,
        words(&Instruction::Liquidate { nonce: 0 }).expect("encodes"),
        3,
    )]);

    Fixture {
        chain,
        deployment: deployment(),
        engine,
        host,
        unsafe_position: p0,
        auction_position: p1,
        live_auction: a,
    }
}
