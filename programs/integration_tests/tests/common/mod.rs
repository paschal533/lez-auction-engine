//! World harness: the real token and stablecoin guests (and the auction_engine) in an in-process V03State
//! (risc0 dev mode). Host bootstrap uses the real `InitializeProgram`; positions use the real
//! `OpenPosition` + `GenerateDebt`. Fixture accounts are force-inserted like the upstream tests.
#![allow(dead_code)]

use auction_sdk::{
    addresses::HostAddresses,
    plan_host::{self, InitializeProgramArgs},
};
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
    math::FIXED_POINT_ONE, RedemptionPriceState,
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
    /// Stablecoin donated straight into the surplus buffer (invariant 9 book).
    pub donated_buffer: u128,
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
            donated_buffer: 0,
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
        let init = auction_sdk::plan_host::initialize_program(
            &HostAddresses::new(host_program()),
            &InitializeProgramArgs {
                admin: id(k::HOST_ADMIN),
                freeze_authority: id(k::FREEZE),
                collateral_definition: collateral_def(),
                market_price_oracle: oracle(),
                stability_fee_per_ms: ONE,
                proportional_gain: 0,
                integral_gain: 0,
                min_collateralization_ratio: ONE * 3 / 2,
                min_ms_between_rate_updates: 300_000,
                max_oracle_age_ms: 900_000,
                redemption_price: R,
                stablecoin_name: String::from("LSC"),
            },
        )
        .expect("initialize_program plan");
        w.run_plan(&init, &[k::HOST_ADMIN])
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
        self.put_oracle_at(oracle(), price, timestamp, confidence);
    }

    /// The one place a RAY-scale price account is built and stored (force-inserted like `set_oracle`:
    /// the twap guest is not part of the in-process World). `set_oracle_full` writes the default oracle;
    /// the localnet publisher's rotation writes the new one at another address.
    pub fn put_oracle_at(&mut self, at: AccountId, price: u128, timestamp: u64, confidence: u128) {
        self.state.force_insert_account(
            at,
            Account {
                program_owner: [9u32; 8],
                balance: 0,
                data: Data::from(&OraclePriceAccount {
                    base_asset: stable_def(),
                    quote_asset: collateral_def(),
                    price,
                    timestamp,
                    source_id: at,
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

    /// The one place a signed public transaction is built: `submit` sends it and `signed_tx_bytes`
    /// sizes it, so the reported size is the size of what is sent.
    fn signed_tx<I: Serialize>(
        &self,
        program: ProgramId,
        accounts: &[AccountId],
        signers: &[u8],
        instruction: &I,
    ) -> PublicTransaction {
        let keys: Vec<PrivateKey> = signers.iter().map(|n| key(*n)).collect();
        let nonces = signers
            .iter()
            .map(|n| self.state.get_account_by_id(id(*n)).nonce)
            .collect();
        let message =
            public_transaction::Message::try_new(program, accounts.to_vec(), nonces, instruction)
                .expect("message is valid");
        let refs: Vec<&PrivateKey> = keys.iter().collect();
        let witness = public_transaction::WitnessSet::for_message(&message, &refs);
        PublicTransaction::new(message, witness)
    }

    pub fn submit<I: Serialize>(
        &mut self,
        program: ProgramId,
        accounts: Vec<AccountId>,
        signers: &[u8],
        instruction: I,
    ) -> Result<(), LeeError> {
        self.seed_clock();
        let tx = self.signed_tx(program, &accounts, signers, &instruction);
        let result = self
            .state
            .transition_from_public_transaction(&tx, self.block, self.now)
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
        let host = HostAddresses::new(host_program());
        let open = plan_host::open_position(
            &host,
            id(owner),
            id(coll_holding),
            collateral_def(),
            nonce,
            collateral,
        )
        .expect("open_position plan");
        assert_eq!(
            open.account_ids[1], pos,
            "SDK position address == harness PDA"
        );
        self.run_plan(&open, &[owner, coll_holding])
            .expect("open_position must succeed");
        if debt > 0 {
            let generate =
                plan_host::generate_debt(&host, id(owner), pos, id(stable_holding), oracle(), debt)
                    .expect("generate_debt plan");
            self.run_plan(&generate, &[owner])
                .expect("generate_debt must succeed");
        }
        pos
    }

    /// Host admin retunes the minimum collateralisation ratio (used to make positions unsafe).
    pub fn set_min_cr(&mut self, ratio: u128) {
        let plan = plan_host::set_minimum_collateralization_ratio(
            &HostAddresses::new(host_program()),
            id(k::HOST_ADMIN),
            ratio,
        )
        .expect("set_minimum_collateralization_ratio plan");
        self.run_plan(&plan, &[k::HOST_ADMIN])
            .expect("set_minimum_collateralization_ratio must succeed");
    }

    /// Host admin binds an engine (program id, authority) through the real guest (ADR-026).
    pub fn bind_engine_to(
        &mut self,
        engine_program_id: ProgramId,
        engine_authority_id: AccountId,
    ) -> Result<(), LeeError> {
        let plan = plan_host::set_liquidation_engine(
            &HostAddresses::new(host_program()),
            id(k::HOST_ADMIN),
            engine_program_id,
            engine_authority_id,
        )
        .expect("set_liquidation_engine plan");
        self.run_plan(&plan, &[k::HOST_ADMIN])
    }
}

// ---- engine part (Task 8) ----
use auction_engine_core::args::CollateralArgs;
use auction_engine_core::{
    args::EngineParamsArgs,
    pda::{self, SLOT_PROTOCOL_TOKEN, SLOT_REF_ORACLE},
    Instruction as Ix,
};

mod engine;
mod sdk;
mod system;
#[allow(unused_imports)]
pub use engine::*;
#[allow(unused_imports)]
pub use sdk::*;
#[allow(unused_imports)]
pub use system::*;

// ---- chain measurement and raw account access (Task 17) ----
pub mod checks;
pub mod measure;
pub mod prove;

impl World {
    /// Measures the chain (executions, cycles; proofs under `AUCTION_PROVE=1`), then submits.
    pub fn measured_full(
        &mut self,
        label: &str,
        accounts: Vec<AccountId>,
        signers: &[u8],
        ix: Ix,
    ) -> (measure::Measured, Result<(), LeeError>) {
        self.seed_clock();
        let keys: Vec<PrivateKey> = signers.iter().map(|n| key(*n)).collect();
        let refs: Vec<&PrivateKey> = keys.iter().collect();
        let m = measure::measure(label, &self.state, engine_program(), &accounts, &refs, &ix);
        (m, self.engine(accounts, signers, ix))
    }

    /// `measured_full` for the ADR-036 tables: also records cycles and the signed transaction size.
    /// Returns (executions, result).
    pub fn measured(
        &mut self,
        label: &str,
        accounts: Vec<AccountId>,
        signers: &[u8],
        ix: Ix,
    ) -> (usize, Result<(), LeeError>) {
        let tx_bytes = self.signed_tx_bytes(&accounts, signers, &ix);
        let (m, r) = self.measured_full(label, accounts, signers, ix);
        measure::record(label, &m, tx_bytes);
        (m.per_exec.len(), r)
    }

    /// Borsh size of the signed engine transaction `World::engine` submits (spec 11 tx sizes). The RPC
    /// wire form adds a one-byte `LeeTransaction` variant tag and base64.
    pub fn signed_tx_bytes(&self, accounts: &[AccountId], signers: &[u8], ix: &Ix) -> usize {
        borsh::to_vec(&self.signed_tx(engine_program(), accounts, signers, ix))
            .expect("borsh")
            .len()
    }

    /// Raw account (R3 isolation checks).
    pub fn raw(&self, account: AccountId) -> Account {
        self.state.get_account_by_id(account)
    }
}
