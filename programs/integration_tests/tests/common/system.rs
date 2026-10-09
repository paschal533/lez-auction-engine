//! System-side World helpers: protocol token, surplus, debt, solvency (moved from mod.rs, Plan 3a Task 1).
use super::*;

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

// ---- F4 debt auction (Task 14) ----

/// A second taker protocol-token holding that is never initialised (S7: fresh holding).
pub const TAKER2_PROTOCOL: u8 = 68;

pub fn debt_auction(nonce: u64) -> AccountId {
    pda::debt_auction_pda(&engine_program(), &config(), nonce)
}

impl World {
    pub fn start_debt(&mut self, nonce: u64) -> Result<(), LeeError> {
        let cfg: auction_engine_core::state::EngineConfigState = self.read(config());
        let token_slot = cfg
            .protocol_token
            .unwrap_or_else(|| unused(SLOT_PROTOCOL_TOKEN));
        let ref_slot = self.ref_slot();
        self.engine(
            vec![
                id(k::CALLER),
                config(),
                accounting(),
                buffer(),
                stable_def(),
                debt_auction(nonce),
                token_slot,
                ref_slot,
                CLOCK_01_PROGRAM_ACCOUNT_ID,
            ],
            &[k::CALLER],
            Ix::StartDebtAuction { nonce },
        )
    }

    /// `take_debt` by `stable` (signer) receiving tokens in `protocol`; `signers` must include `stable`.
    pub fn take_debt(
        &mut self,
        nonce: u64,
        stable: u8,
        protocol: u8,
        signers: &[u8],
        amount_coin: u128,
        min_out: u128,
    ) -> Result<(), LeeError> {
        self.engine(
            vec![
                id(stable),
                id(protocol),
                config(),
                accounting(),
                debt_auction(nonce),
                stable_def(),
                protocol_token_def(),
                mint_authority(),
                CLOCK_01_PROGRAM_ACCOUNT_ID,
            ],
            signers,
            Ix::TakeDebt {
                amount_coin,
                min_protocol_out: min_out,
            },
        )
    }

    pub fn close_debt(&mut self, nonce: u64) -> Result<(), LeeError> {
        self.engine(
            vec![
                id(k::CALLER),
                config(),
                accounting(),
                debt_auction(nonce),
                CLOCK_01_PROGRAM_ACCOUNT_ID,
            ],
            &[k::CALLER],
            Ix::CloseDebtAuction,
        )
    }
}

// ---- check_solvency and the host freeze chain (Task 15) ----

impl World {
    /// The eleven `check_solvency` accounts (Shared Reference R2 #16) with explicit slot accounts.
    pub fn check_solvency_accounts(
        &self,
        token_slot: AccountId,
        ref_slot: AccountId,
    ) -> Vec<AccountId> {
        vec![
            id(k::CALLER),
            config(),
            accounting(),
            buffer(),
            stable_def(),
            token_slot,
            ref_slot,
            engine_authority(),
            les(),
            protocol_parameters(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ]
    }

    /// Slot accounts exactly as the config demands (the configured id or the `UNUSED` placeholder).
    pub fn check_solvency(&mut self) -> Result<(), LeeError> {
        let cfg: auction_engine_core::state::EngineConfigState = self.read(config());
        let token_slot = cfg
            .protocol_token
            .unwrap_or_else(|| unused(SLOT_PROTOCOL_TOKEN));
        let ref_slot = self.ref_slot();
        self.check_solvency_with(token_slot, ref_slot)
    }

    pub fn check_solvency_with(
        &mut self,
        token_slot: AccountId,
        ref_slot: AccountId,
    ) -> Result<(), LeeError> {
        let accounts = self.check_solvency_accounts(token_slot, ref_slot);
        self.check_solvency_raw(accounts, &[k::CALLER])
    }

    /// Fully caller-controlled account list and signer set.
    pub fn check_solvency_raw(
        &mut self,
        accounts: Vec<AccountId>,
        signers: &[u8],
    ) -> Result<(), LeeError> {
        self.engine(accounts, signers, Ix::CheckSolvency)
    }

    pub fn host_frozen(&self) -> bool {
        self.read::<stablecoin_core::ProtocolParameters>(protocol_parameters())
            .is_frozen
    }
}
