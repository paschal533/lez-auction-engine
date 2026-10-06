//! World harness: the real token and stablecoin guests (and the auction_engine) in an in-process V03State
//! (risc0 dev mode). Host bootstrap uses the real `InitializeProgram`; positions use the real
//! `OpenPosition` + `GenerateDebt`. Fixture accounts are force-inserted like the upstream tests.
#![allow(dead_code)]

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee::{
    error::LeeError,
    program_deployment_transaction::{self, ProgramDeploymentTransaction},
    public_transaction, PrivateKey, PublicKey, PublicTransaction, V03State,
};
use lee_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use serde::Serialize;
use stablecoin_core::{
    compute_liquidation_engine_state_pda, compute_position_pda, compute_position_vault_pda,
    compute_protocol_parameters_pda, compute_redemption_price_state_pda,
    compute_stability_fee_accumulator_pda, compute_stablecoin_definition_pda,
    compute_stablecoin_master_holding_pda, math::FIXED_POINT_ONE, RedemptionPriceState,
};
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::OraclePriceAccount;

pub const T0: u64 = 1_700_000_000_000;
pub const ONE: u128 = FIXED_POINT_ONE; // == auction_core::units::RAY
pub const R: u128 = FIXED_POINT_ONE / 2; // initial redemption price: 0.5 collateral per coin

/// Key bytes: `PrivateKey::try_new([n; 32])`. Holdings sign with their own key.
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
    pub const BIDDER2_STABLE: u8 = 55;
    pub const BIDDER2_COLL: u8 = 56;
    pub const TAKER_STABLE: u8 = 57;
    pub const TAKER_PROTOCOL: u8 = 58;
    pub const PROTOCOL_DEF: u8 = 59;
    pub const PROTOCOL_SEED_HOLDING: u8 = 60;
    pub const ATTACKER: u8 = 61;
    pub const CALLER: u8 = 62;
    pub const DONOR_COLL: u8 = 63;
    pub const DONOR_STABLE: u8 = 64;
    pub const OWNER2: u8 = 65;
    pub const OWNER2_COLL: u8 = 66;
    pub const OWNER2_STABLE: u8 = 67;
}

pub fn key(n: u8) -> PrivateKey {
    PrivateKey::try_new([n; 32]).expect("valid private key")
}
pub fn id(n: u8) -> AccountId {
    AccountId::from(&PublicKey::new_from_private_key(&key(n)))
}
pub fn host_program() -> ProgramId {
    stablecoin_methods::STABLECOIN_ID
}
pub fn token_program() -> ProgramId {
    token_methods::TOKEN_ID
}
pub fn collateral_def() -> AccountId {
    AccountId::new([5; 32])
}
pub fn oracle() -> AccountId {
    AccountId::new([0x70; 32])
}
pub fn ref_oracle() -> AccountId {
    AccountId::new([0x71; 32])
}
pub fn protocol_token_def() -> AccountId {
    id(k::PROTOCOL_DEF)
}
pub fn stable_def() -> AccountId {
    compute_stablecoin_definition_pda(host_program())
}
pub fn protocol_parameters() -> AccountId {
    compute_protocol_parameters_pda(host_program())
}
pub fn accumulator() -> AccountId {
    compute_stability_fee_accumulator_pda(host_program())
}
pub fn redemption_state() -> AccountId {
    compute_redemption_price_state_pda(host_program())
}
pub fn les() -> AccountId {
    compute_liquidation_engine_state_pda(host_program())
}
pub fn position(owner: u8, nonce: u64) -> AccountId {
    compute_position_pda(host_program(), id(owner), nonce)
}
pub fn host_vault(pos: AccountId) -> AccountId {
    compute_position_vault_pda(host_program(), pos)
}

pub fn assert_err(result: Result<(), LeeError>, needle: &str) {
    let text = format!(
        "{:?}",
        result.expect_err("transaction was expected to fail")
    );
    assert!(text.contains(needle), "expected {needle:?} in: {text}");
}

pub struct World {
    pub state: V03State,
    pub now: u64,
    pub block: u64,
    /// Stablecoin force-minted into test holdings (supply invariant, Task 17).
    pub injected_stable: u128,
}

impl World {
    /// Host bootstrapped with the real `InitializeProgram`: stability fee exactly 1.0 per ms (debt stays
    /// exact), min CR 1.5, redemption price 0.5, oracle fresh at 0.5.
    pub fn new() -> Self {
        let mut w = Self {
            state: V03State::new(),
            now: T0,
            block: 1,
            injected_stable: 0,
        };
        w.seed_clock();
        for elf in [
            token_methods::TOKEN_ELF,
            stablecoin_methods::STABLECOIN_ELF,
            auction_engine_methods::AUCTION_ENGINE_ELF,
        ] {
            w.deploy(elf);
        }
        for n in [
            k::HOST_ADMIN,
            k::FREEZE,
            k::OWNER,
            k::ENGINE_ADMIN,
            k::KEEPER,
            k::ATTACKER,
            k::CALLER,
        ] {
            w.ensure_signer(n);
        }
        w.state.force_insert_account(
            collateral_def(),
            Account {
                program_owner: token_program(),
                balance: 0,
                data: Data::from(&TokenDefinition::Fungible {
                    name: String::from("COLL"),
                    total_supply: 0,
                    metadata_id: None,
                    authority: None,
                }),
                nonce: Nonce(0),
            },
        );
        w.set_oracle(R, T0);
        let init = stablecoin_core::Instruction::InitializeProgram {
            freeze_authority_account_id: id(k::FREEZE),
            initial_stability_fee_per_millisecond: ONE,
            initial_controller_proportional_gain: 0,
            initial_controller_integral_gain: 0,
            initial_minimum_collateralization_ratio: ONE * 3 / 2,
            minimum_milliseconds_between_rate_updates: 300_000,
            maximum_oracle_price_age_milliseconds: 900_000,
            initial_redemption_price: R,
            stablecoin_name: String::from("LSC"),
        };
        w.host(
            vec![
                id(k::HOST_ADMIN),
                protocol_parameters(),
                accumulator(),
                redemption_state(),
                stable_def(),
                compute_stablecoin_master_holding_pda(host_program()),
                collateral_def(),
                oracle(),
                CLOCK_01_PROGRAM_ACCOUNT_ID,
            ],
            &[k::HOST_ADMIN],
            init,
        )
        .expect("initialize_program must succeed");
        w
    }

    pub fn deploy(&mut self, elf: &[u8]) {
        self.state
            .transition_from_program_deployment_transaction(&ProgramDeploymentTransaction::new(
                program_deployment_transaction::Message::new(elf.to_vec()),
            ))
            .expect("program deployment must succeed");
    }

    /// A signer owned by a placeholder non-default program, so it can sign many transactions
    /// (S1 problem 6; upstream fixtures do the same).
    pub fn ensure_signer(&mut self, n: u8) {
        if self.state.get_account_by_id(id(n)) == Account::default() {
            self.state.force_insert_account(
                id(n),
                Account {
                    program_owner: [7u32; 8],
                    ..Account::default()
                },
            );
        }
    }

    pub fn seed_clock(&mut self) {
        let data = ClockAccountData {
            block_id: self.block,
            timestamp: self.now,
        }
        .to_bytes();
        self.state.force_insert_account(
            CLOCK_01_PROGRAM_ACCOUNT_ID,
            Account {
                program_owner: [8u32; 8],
                data: Data::try_from(data).expect("clock data fits"),
                ..Account::default()
            },
        );
    }

    pub fn advance(&mut self, ms: u64) {
        self.now += ms;
        self.seed_clock();
    }

    pub fn set_oracle_full(&mut self, price: u128, timestamp: u64, confidence: u128) {
        self.state.force_insert_account(
            oracle(),
            Account {
                program_owner: [9u32; 8],
                balance: 0,
                data: Data::from(&OraclePriceAccount {
                    base_asset: stable_def(),
                    quote_asset: collateral_def(),
                    price,
                    timestamp,
                    source_id: oracle(),
                    confidence_interval: confidence,
                }),
                nonce: Nonce(0),
            },
        );
    }

    pub fn set_oracle(&mut self, price: u128, timestamp: u64) {
        self.set_oracle_full(price, timestamp, 0);
    }

    /// Re-anchors the host redemption price at `price` (rate 1.0) as of `now`.
    pub fn set_redemption_price(&mut self, price: u128) {
        let mut account = self.state.get_account_by_id(redemption_state());
        let mut s = RedemptionPriceState::try_from(&account.data).expect("redemption state");
        s.redemption_price_at_last_update = price;
        s.redemption_rate_per_millisecond = ONE;
        s.last_updated_at = self.now;
        account.data = Data::from(&s);
        self.state.force_insert_account(redemption_state(), account);
    }

    pub fn submit<I: Serialize>(
        &mut self,
        program: ProgramId,
        accounts: Vec<AccountId>,
        signers: &[u8],
        instruction: I,
    ) -> Result<(), LeeError> {
        self.seed_clock();
        let keys: Vec<PrivateKey> = signers.iter().map(|n| key(*n)).collect();
        let nonces = signers
            .iter()
            .map(|n| self.state.get_account_by_id(id(*n)).nonce)
            .collect();
        let message = public_transaction::Message::try_new(program, accounts, nonces, instruction)
            .expect("message is valid");
        let refs: Vec<&PrivateKey> = keys.iter().collect();
        let witness = public_transaction::WitnessSet::for_message(&message, &refs);
        let result = self
            .state
            .transition_from_public_transaction(
                &PublicTransaction::new(message, witness),
                self.block,
                self.now,
            )
            .map(|_| ());
        if result.is_ok() {
            self.block += 1;
        }
        result
    }

    pub fn host<I: Serialize>(
        &mut self,
        accounts: Vec<AccountId>,
        signers: &[u8],
        ix: I,
    ) -> Result<(), LeeError> {
        self.submit(host_program(), accounts, signers, ix)
    }

    /// Writes holding `id(n)` of `definition` with `balance` (no supply change).
    pub fn holding(&mut self, n: u8, definition: AccountId, balance: u128) {
        self.state.force_insert_account(
            id(n),
            Account {
                program_owner: token_program(),
                balance: 0,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: definition,
                    balance,
                }),
                nonce: Nonce(0),
            },
        );
    }

    fn add_supply(&mut self, definition: AccountId, amount: u128) {
        let mut account = self.state.get_account_by_id(definition);
        let next = match TokenDefinition::try_from(&account.data).expect("definition") {
            TokenDefinition::Fungible {
                name,
                total_supply,
                metadata_id,
                authority,
            } => TokenDefinition::Fungible {
                name,
                total_supply: total_supply + amount,
                metadata_id,
                authority,
            },
            TokenDefinition::NonFungible { .. } => panic!("fungible definition expected"),
        };
        account.data = Data::from(&next);
        self.state.force_insert_account(definition, account);
    }

    /// Adds `amount` stablecoin to holding `id(n)` and to the stablecoin supply (books stay consistent).
    pub fn fund_stable(&mut self, n: u8, amount: u128) {
        let current = self.balance(id(n));
        self.holding(n, stable_def(), current + amount);
        self.add_supply(stable_def(), amount);
        self.injected_stable += amount;
    }

    pub fn fund_collateral(&mut self, n: u8, amount: u128) {
        let current = self.balance(id(n));
        self.holding(n, collateral_def(), current + amount);
        self.add_supply(collateral_def(), amount);
    }

    pub fn balance(&self, account: AccountId) -> u128 {
        match TokenHolding::try_from(&self.state.get_account_by_id(account).data) {
            Ok(TokenHolding::Fungible { balance, .. }) => balance,
            _ => 0,
        }
    }

    pub fn supply(&self, definition: AccountId) -> u128 {
        match TokenDefinition::try_from(&self.state.get_account_by_id(definition).data) {
            Ok(TokenDefinition::Fungible { total_supply, .. }) => total_supply,
            _ => 0,
        }
    }

    pub fn read<T: for<'a> TryFrom<&'a Data>>(&self, account: AccountId) -> T {
        T::try_from(&self.state.get_account_by_id(account).data)
            .unwrap_or_else(|_| panic!("account {account:?} does not decode"))
    }

    /// Real `OpenPosition` then `GenerateDebt` (debt minted to `stable_holding`). Returns the position id.
    pub fn open_position(
        &mut self,
        owner: u8,
        coll_holding: u8,
        stable_holding: u8,
        nonce: u64,
        collateral: u128,
        debt: u128,
    ) -> AccountId {
        self.ensure_signer(owner);
        self.fund_collateral(coll_holding, collateral);
        if self.state.get_account_by_id(id(stable_holding)) == Account::default() {
            self.holding(stable_holding, stable_def(), 0);
        }
        let pos = position(owner, nonce);
        self.host(
            vec![
                id(owner),
                pos,
                host_vault(pos),
                id(coll_holding),
                collateral_def(),
                protocol_parameters(),
                CLOCK_01_PROGRAM_ACCOUNT_ID,
            ],
            &[owner, coll_holding],
            stablecoin_core::Instruction::OpenPosition {
                position_nonce: nonce,
                initial_collateral_amount: collateral,
            },
        )
        .expect("open_position must succeed");
        if debt > 0 {
            self.host(
                vec![
                    id(owner),
                    pos,
                    stable_def(),
                    id(stable_holding),
                    accumulator(),
                    redemption_state(),
                    oracle(),
                    protocol_parameters(),
                    CLOCK_01_PROGRAM_ACCOUNT_ID,
                ],
                &[owner],
                stablecoin_core::Instruction::GenerateDebt { amount: debt },
            )
            .expect("generate_debt must succeed");
        }
        pos
    }

    /// Host admin retunes the minimum collateralisation ratio (used to make positions unsafe).
    pub fn set_min_cr(&mut self, ratio: u128) {
        self.host(
            vec![id(k::HOST_ADMIN), protocol_parameters()],
            &[k::HOST_ADMIN],
            stablecoin_core::Instruction::SetMinimumCollateralizationRatio { new_ratio: ratio },
        )
        .expect("set_minimum_collateralization_ratio must succeed");
    }

    /// Host admin binds an engine (program id, authority) through the real guest (ADR-026).
    pub fn bind_engine_to(
        &mut self,
        engine_program_id: ProgramId,
        engine_authority_id: AccountId,
    ) -> Result<(), LeeError> {
        self.host(
            vec![id(k::HOST_ADMIN), protocol_parameters(), les()],
            &[k::HOST_ADMIN],
            stablecoin_core::Instruction::SetLiquidationEngine {
                engine_program_id,
                engine_authority_id,
            },
        )
    }
}

// ---- engine part (Task 8) ----
use auction_engine_core::{
    args::EngineParamsArgs,
    pda::{self, SLOT_PROTOCOL_TOKEN, SLOT_REF_ORACLE},
    Instruction as Ix,
};

pub const SALT: u64 = 1;

pub fn engine_program() -> ProgramId {
    auction_engine_methods::AUCTION_ENGINE_ID
}
pub fn config() -> AccountId {
    pda::config_pda(&engine_program(), &id(k::ENGINE_ADMIN), SALT)
}
pub fn coll_config() -> AccountId {
    pda::coll_config_pda(&engine_program(), &config(), &collateral_def())
}
pub fn accounting() -> AccountId {
    pda::accounting_pda(&engine_program(), &config())
}
pub fn buffer() -> AccountId {
    pda::buffer_pda(&engine_program(), &config())
}
pub fn engine_authority() -> AccountId {
    pda::engine_authority_pda(&engine_program(), &config())
}
pub fn mint_authority() -> AccountId {
    pda::mint_authority_pda(&engine_program(), &config())
}
pub fn unused(slot: u64) -> AccountId {
    pda::unused_slot_pda(&engine_program(), &config(), slot)
}
pub fn record(pos: AccountId) -> AccountId {
    pda::liqrec_pda(&engine_program(), &config(), &pos)
}
pub fn auction(pos: AccountId, nonce: u64) -> AccountId {
    pda::auction_pda(&engine_program(), &config(), &pos, nonce)
}
pub fn auction_vault(pos: AccountId, nonce: u64) -> AccountId {
    pda::vault_pda(&engine_program(), &auction(pos, nonce))
}

pub fn init_config_ix(salt: u64, p: &EngineParamsArgs) -> Ix {
    Ix::InitConfig {
        salt,
        global_cap: p.global_cap,
        freeze_on_bad_debt: p.freeze_on_bad_debt,
        bad_debt_freeze_threshold: p.bad_debt_freeze_threshold,
        protocol_token_enabled: p.protocol_token_enabled,
        protocol_token_ref_price: p.protocol_token_ref_price,
        ref_oracle_enabled: p.ref_oracle_enabled,
        ref_oracle_max_age_ms: p.ref_oracle_max_age_ms,
        surplus_buffer_target: p.surplus_buffer_target,
        surplus_lot: p.surplus_lot,
        surplus_delay_ms: p.surplus_delay_ms,
        surplus_premium_bps: p.surplus_premium_bps,
        surplus_max_discount_bps: p.surplus_max_discount_bps,
        debt_lot: p.debt_lot,
        debt_initial_discount_bps: p.debt_initial_discount_bps,
        debt_growth: p.debt_growth.clone(),
        surplus_decay: p.surplus_decay.clone(),
        max_mint_per_auction: p.max_mint_per_auction,
        min_take: p.min_take,
        system_ttl_ms: p.system_ttl_ms,
    }
}

pub fn set_engine_params_ix(p: &EngineParamsArgs) -> Ix {
    Ix::SetEngineParams {
        global_cap: p.global_cap,
        freeze_on_bad_debt: p.freeze_on_bad_debt,
        bad_debt_freeze_threshold: p.bad_debt_freeze_threshold,
        protocol_token_enabled: p.protocol_token_enabled,
        protocol_token_ref_price: p.protocol_token_ref_price,
        ref_oracle_enabled: p.ref_oracle_enabled,
        ref_oracle_max_age_ms: p.ref_oracle_max_age_ms,
        surplus_buffer_target: p.surplus_buffer_target,
        surplus_lot: p.surplus_lot,
        surplus_delay_ms: p.surplus_delay_ms,
        surplus_premium_bps: p.surplus_premium_bps,
        surplus_max_discount_bps: p.surplus_max_discount_bps,
        debt_lot: p.debt_lot,
        debt_initial_discount_bps: p.debt_initial_discount_bps,
        debt_growth: p.debt_growth.clone(),
        surplus_decay: p.surplus_decay.clone(),
        max_mint_per_auction: p.max_mint_per_auction,
        min_take: p.min_take,
        system_ttl_ms: p.system_ttl_ms,
    }
}

pub fn slot_account(slot: u64, enabled: bool) -> AccountId {
    match (slot, enabled) {
        (SLOT_PROTOCOL_TOKEN, true) => protocol_token_def(),
        (SLOT_REF_ORACLE, true) => ref_oracle(),
        _ => unused(slot),
    }
}

impl World {
    pub fn engine(
        &mut self,
        accounts: Vec<AccountId>,
        signers: &[u8],
        ix: Ix,
    ) -> Result<(), LeeError> {
        self.submit(engine_program(), accounts, signers, ix)
    }

    pub fn init_engine_config(&mut self, p: &EngineParamsArgs) -> Result<(), LeeError> {
        self.engine(
            vec![
                id(k::ENGINE_ADMIN),
                config(),
                accounting(),
                buffer(),
                protocol_parameters(),
                stable_def(),
                slot_account(SLOT_PROTOCOL_TOKEN, p.protocol_token_enabled),
                slot_account(SLOT_REF_ORACLE, p.ref_oracle_enabled),
            ],
            &[k::ENGINE_ADMIN],
            init_config_ix(SALT, p),
        )
    }

    pub fn set_engine_params(&mut self, signer: u8, p: &EngineParamsArgs) -> Result<(), LeeError> {
        self.engine(
            vec![
                id(signer),
                config(),
                accounting(),
                coll_config(),
                slot_account(SLOT_PROTOCOL_TOKEN, p.protocol_token_enabled),
                slot_account(SLOT_REF_ORACLE, p.ref_oracle_enabled),
            ],
            &[signer],
            set_engine_params_ix(p),
        )
    }

    /// Host admin binds `config()`'s engine authority (ADR-026/027).
    pub fn bind_engine(&mut self) -> Result<(), LeeError> {
        self.bind_engine_to(engine_program(), engine_authority())
    }
}

// ---- collateral config and feed observation (Task 9) ----
use auction_engine_core::args::CollateralArgs;

pub fn collateral_ix(c: &CollateralArgs) -> Ix {
    Ix::SetCollateralConfig {
        oracle_scale: c.oracle_scale,
        max_oracle_age_ms: c.max_oracle_age_ms,
        max_conf_bps: c.max_conf_bps,
        band_lo_bps: c.band_lo_bps,
        band_hi_bps: c.band_hi_bps,
        penalty_bps: c.penalty_bps,
        tip: c.tip,
        chip_bps: c.chip_bps,
        reward_cap: c.reward_cap,
        debt_floor: c.debt_floor,
        min_bid: c.min_bid,
        type_cap: c.type_cap,
        schedule: c.schedule.clone(),
        admin_paused: c.admin_paused,
    }
}

impl World {
    pub fn set_collateral_config(
        &mut self,
        signer: u8,
        c: &CollateralArgs,
    ) -> Result<(), LeeError> {
        self.engine(
            vec![
                id(signer),
                config(),
                collateral_def(),
                coll_config(),
                oracle(),
                protocol_parameters(),
                redemption_state(),
                CLOCK_01_PROGRAM_ACCOUNT_ID,
            ],
            &[signer],
            collateral_ix(c),
        )
    }

    pub fn observe_feed(&mut self) -> Result<(), LeeError> {
        self.engine(
            vec![
                id(k::CALLER),
                config(),
                coll_config(),
                redemption_state(),
                oracle(),
                CLOCK_01_PROGRAM_ACCOUNT_ID,
            ],
            &[k::CALLER],
            Ix::ObserveFeed,
        )
    }

    /// Host bootstrapped, engine config created with defaults, bound by the host admin, collateral
    /// configured with defaults (Ray scale, 60 min max age, band 9000-11000, penalty 10%).
    pub fn with_engine() -> Self {
        let mut w = Self::new();
        w.init_engine_config(&EngineParamsArgs::defaults_for_tests())
            .expect("init_config");
        w.bind_engine().expect("set_liquidation_engine");
        w.set_collateral_config(k::ENGINE_ADMIN, &CollateralArgs::defaults_for_tests())
            .expect("set_collateral_config");
        w
    }
}

// ---- liquidate (Task 10) ----

/// Standard unsafe position (Task 10): 500_000 collateral, 600_000 debt at R = 0.5 (safe at min CR 1.5,
/// unsafe at 2.0). Penalty 10% → target 660_000; reward min(5 + 600, 50, 60_000) = 50 coin = 25 collateral.
pub const LIQ_COLLATERAL: u128 = 500_000;
pub const LIQ_DEBT: u128 = 600_000;
pub const LIQ_TARGET: u128 = 660_000;
pub const LIQ_REWARD: u128 = 25;
pub const LIQ_LOT: u128 = LIQ_COLLATERAL - LIQ_REWARD;

impl World {
    pub fn unsafe_position(&mut self) -> AccountId {
        let pos = self.open_position(
            k::OWNER,
            k::OWNER_COLL,
            k::OWNER_STABLE,
            0,
            LIQ_COLLATERAL,
            LIQ_DEBT,
        );
        self.set_min_cr(2 * ONE);
        pos
    }

    /// A second, smaller position (100_000 collateral, 120_000 debt → target 132_000); open it BEFORE
    /// `set_min_cr(2 * ONE)`.
    pub fn open_small_position(&mut self) -> AccountId {
        self.open_position(
            k::OWNER2,
            k::OWNER2_COLL,
            k::OWNER2_STABLE,
            0,
            100_000,
            120_000,
        )
    }

    pub fn liquidate_accounts(
        &self,
        pos: AccountId,
        nonce: u64,
        keeper_holding: AccountId,
    ) -> Vec<AccountId> {
        vec![
            id(k::KEEPER),
            keeper_holding,
            config(),
            coll_config(),
            pos,
            record(pos),
            auction(pos, nonce),
            auction_vault(pos, nonce),
            engine_authority(),
            les(),
            host_vault(pos),
            collateral_def(),
            accumulator(),
            redemption_state(),
            oracle(),
            protocol_parameters(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ]
    }

    /// Liquidates with the keeper's initialised collateral holding `id(k::KEEPER_COLL)`.
    pub fn liquidate(&mut self, pos: AccountId, nonce: u64) -> Result<(), LeeError> {
        if self.state.get_account_by_id(id(k::KEEPER_COLL)) == Account::default() {
            self.holding(k::KEEPER_COLL, collateral_def(), 0);
        }
        let accounts = self.liquidate_accounts(pos, nonce, id(k::KEEPER_COLL));
        self.engine(accounts, &[k::KEEPER], Ix::Liquidate { nonce })
    }

    /// A plain `Token::Transfer` signed by holding `id(from)` (donations in tests).
    pub fn transfer_tokens(
        &mut self,
        from: u8,
        to: AccountId,
        amount: u128,
    ) -> Result<(), LeeError> {
        self.submit(
            token_program(),
            vec![id(from), to],
            &[from],
            token_core::Instruction::Transfer {
                amount_to_transfer: amount,
            },
        )
    }
}

// ---- bid (Task 11) ----

impl World {
    /// Funds `stable` with stablecoin and makes sure `coll` exists as an empty collateral holding.
    pub fn bidder(&mut self, stable: u8, coll: u8, amount: u128) {
        self.fund_stable(stable, amount);
        if self.state.get_account_by_id(id(coll)) == Account::default() {
            self.holding(coll, collateral_def(), 0);
        }
    }

    pub fn bid_accounts(
        &self,
        pos: AccountId,
        nonce: u64,
        stable: u8,
        coll: u8,
        with_accounting: bool,
    ) -> Vec<AccountId> {
        let mut v = vec![
            id(stable),
            id(coll),
            config(),
            coll_config(),
            auction(pos, nonce),
            auction_vault(pos, nonce),
            buffer(),
            stable_def(),
            redemption_state(),
            oracle(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ];
        if with_accounting {
            v.push(accounting());
        }
        v
    }

    /// `bid` by the default bidder (no Accounting account; must not end the auction).
    pub fn bid(
        &mut self,
        pos: AccountId,
        coin_in_max: u128,
        min_out: u128,
    ) -> Result<(), LeeError> {
        let accounts = self.bid_accounts(pos, 0, k::BIDDER_STABLE, k::BIDDER_COLL, false);
        self.engine(
            accounts,
            &[k::BIDDER_STABLE],
            Ix::Bid {
                coin_in_max,
                min_collateral_out: min_out,
            },
        )
    }

    /// `bid_with_settlement` by the default bidder (lists Accounting; may end the auction).
    pub fn bid_settle(
        &mut self,
        pos: AccountId,
        coin_in_max: u128,
        min_out: u128,
    ) -> Result<(), LeeError> {
        let accounts = self.bid_accounts(pos, 0, k::BIDDER_STABLE, k::BIDDER_COLL, true);
        self.engine(
            accounts,
            &[k::BIDDER_STABLE],
            Ix::BidWithSettlement {
                coin_in_max,
                min_collateral_out: min_out,
            },
        )
    }
}

/// Engine configured, the standard unsafe position liquidated (nonce 0), bidder funded with 1_000_000.
pub fn liquidated_world() -> (World, AccountId) {
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    w.liquidate(pos, 0).expect("liquidate");
    w.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 1_000_000);
    (w, pos)
}

/// Underwater variant: position 500_000 / 600_000 opened at R = 0.5, then R re-anchored to 2.0
/// (oracle 2.0). Reward 50 coin = 100 collateral, lot 499_900; the lot binds at 244_951 coin.
pub fn underwater_world() -> (World, AccountId) {
    let mut w = World::with_engine();
    let pos = w.open_position(
        k::OWNER,
        k::OWNER_COLL,
        k::OWNER_STABLE,
        0,
        LIQ_COLLATERAL,
        LIQ_DEBT,
    );
    w.set_redemption_price(2 * ONE);
    w.set_oracle(2 * ONE, w.now);
    w.liquidate(pos, 0).expect("liquidate");
    w.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 1_000_000);
    (w, pos)
}

// ---- settle_empty, claim_leftover, settle_debt (Task 12) ----

impl World {
    pub fn settle_empty(&mut self, pos: AccountId) -> Result<(), LeeError> {
        self.engine(
            vec![
                id(k::CALLER),
                config(),
                coll_config(),
                accounting(),
                auction(pos, 0),
            ],
            &[k::CALLER],
            Ix::SettleEmpty,
        )
    }

    pub fn claim_leftover(
        &mut self,
        signer: u8,
        holding: u8,
        pos: AccountId,
    ) -> Result<(), LeeError> {
        self.claim_leftover_signed(signer, holding, pos, &[signer])
    }

    /// `claim_leftover` with an explicit signer list (a fresh recipient holding must sign too).
    pub fn claim_leftover_signed(
        &mut self,
        signer: u8,
        holding: u8,
        pos: AccountId,
        signers: &[u8],
    ) -> Result<(), LeeError> {
        self.engine(
            vec![
                id(signer),
                id(holding),
                config(),
                auction(pos, 0),
                auction_vault(pos, 0),
            ],
            signers,
            Ix::ClaimLeftover,
        )
    }

    pub fn settle_debt(&mut self, amount: u128) -> Result<(), LeeError> {
        self.engine(
            vec![
                id(k::CALLER),
                config(),
                accounting(),
                buffer(),
                stable_def(),
            ],
            &[k::CALLER],
            Ix::SettleDebt { amount },
        )
    }

    /// A third party raises `to`'s balance (stablecoin donation).
    pub fn donate_stable(&mut self, to: AccountId, amount: u128) {
        self.fund_stable(k::DONOR_STABLE, amount);
        self.transfer_tokens(k::DONOR_STABLE, to, amount)
            .expect("donation");
    }
}

// ---- mock protocol token and the F5 surplus auction (Task 13) ----

pub fn surplus_auction(nonce: u64) -> AccountId {
    pda::surplus_auction_pda(&engine_program(), &config(), nonce)
}
pub fn surplus_vault(nonce: u64) -> AccountId {
    pda::surplus_vault_pda(&engine_program(), &surplus_auction(nonce))
}

impl World {
    /// Real `Token::NewFungibleDefinition` of the mock protocol token: definition `id(k::PROTOCOL_DEF)`,
    /// initial supply to the taker's holding `id(k::TAKER_PROTOCOL)`, mint authority `authority`.
    pub fn create_protocol_token(
        &mut self,
        initial_supply: u128,
        authority: AccountId,
    ) -> Result<(), LeeError> {
        self.submit(
            token_program(),
            vec![protocol_token_def(), id(k::TAKER_PROTOCOL)],
            &[k::PROTOCOL_DEF, k::TAKER_PROTOCOL],
            token_core::Instruction::NewFungibleDefinition {
                name: String::from("MOCK"),
                total_supply: initial_supply,
                mint_authority: Some(authority),
            },
        )
    }

    /// The protocol-token reference oracle at `ref_oracle()` (coin per protocol token, RAY).
    pub fn set_ref_oracle(&mut self, price: u128, timestamp: u64) {
        self.state.force_insert_account(
            ref_oracle(),
            Account {
                program_owner: [9u32; 8],
                balance: 0,
                data: Data::from(&OraclePriceAccount {
                    base_asset: protocol_token_def(),
                    quote_asset: stable_def(),
                    price,
                    timestamp,
                    source_id: ref_oracle(),
                    confidence_interval: 0,
                }),
                nonce: Nonce(0),
            },
        );
    }

    /// Engine with the protocol token configured (ref price 2 coin/token), optionally with the
    /// reference oracle; the mock token exists with mint authority `["MINT_AUTH", config]` and 10_000
    /// tokens in the taker's holding; the taker's stablecoin holding exists (empty).
    pub fn with_system(ref_oracle_enabled: bool) -> Self {
        let mut w = Self::new();
        let p = EngineParamsArgs {
            protocol_token_enabled: true,
            ref_oracle_enabled,
            ..EngineParamsArgs::defaults_for_tests()
        };
        if ref_oracle_enabled {
            w.set_ref_oracle(2 * ONE, w.now);
        }
        w.init_engine_config(&p).expect("init_config");
        w.bind_engine().expect("bind");
        w.set_collateral_config(k::ENGINE_ADMIN, &CollateralArgs::defaults_for_tests())
            .expect("collateral");
        w.create_protocol_token(10_000, mint_authority())
            .expect("mock protocol token");
        w.holding(k::TAKER_STABLE, stable_def(), 0);
        w
    }

    /// Books a shortfall on a configured world: underwater position (R → 2.0), liquidated, lot sold.
    /// Leaves `unbacked_debt == 355_049` and the bidder with 1_000_000 − 244_951 stablecoin.
    pub fn book_shortfall(&mut self) -> AccountId {
        let pos = self.open_position(
            k::OWNER,
            k::OWNER_COLL,
            k::OWNER_STABLE,
            0,
            LIQ_COLLATERAL,
            LIQ_DEBT,
        );
        self.set_redemption_price(2 * ONE);
        self.set_oracle(2 * ONE, self.now);
        self.liquidate(pos, 0).expect("liquidate");
        self.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 1_000_000);
        self.bid_settle(pos, LIQ_TARGET, 0)
            .expect("lot-binding bid");
        pos
    }

    fn ref_slot(&self) -> AccountId {
        let cfg: auction_engine_core::state::EngineConfigState = self.read(config());
        cfg.protocol_token_oracle_id
            .unwrap_or_else(|| unused(SLOT_REF_ORACLE))
    }

    pub fn start_surplus_accounts(&self, nonce: u64) -> Vec<AccountId> {
        vec![
            id(k::CALLER),
            config(),
            accounting(),
            buffer(),
            stable_def(),
            surplus_auction(nonce),
            surplus_vault(nonce),
            self.ref_slot(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ]
    }

    pub fn start_surplus(&mut self, nonce: u64) -> Result<(), LeeError> {
        let accounts = self.start_surplus_accounts(nonce);
        self.engine(accounts, &[k::CALLER], Ix::StartSurplusAuction { nonce })
    }

    pub fn take_surplus_accounts(&self, nonce: u64) -> Vec<AccountId> {
        vec![
            id(k::TAKER_PROTOCOL),
            id(k::TAKER_STABLE),
            config(),
            accounting(),
            surplus_auction(nonce),
            surplus_vault(nonce),
            protocol_token_def(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ]
    }

    pub fn take_surplus(
        &mut self,
        nonce: u64,
        amount_coin: u128,
        max_protocol_in: u128,
    ) -> Result<(), LeeError> {
        let accounts = self.take_surplus_accounts(nonce);
        self.engine(
            accounts,
            &[k::TAKER_PROTOCOL],
            Ix::TakeSurplus {
                amount_coin,
                max_protocol_in,
            },
        )
    }

    pub fn close_surplus(&mut self, nonce: u64) -> Result<(), LeeError> {
        self.engine(
            vec![
                id(k::CALLER),
                config(),
                accounting(),
                surplus_auction(nonce),
                surplus_vault(nonce),
                buffer(),
                CLOCK_01_PROGRAM_ACCOUNT_ID,
            ],
            &[k::CALLER],
            Ix::CloseSurplusAuction,
        )
    }
}
