//! Engine-side World helpers: config, collateral, liquidate, bid, settle (moved from mod.rs, Plan 3a Task 1).
use super::*;

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

// Instruction encoders are the SDK's (Plan 3a Task 8): the harness no longer keeps its own copies.
pub use auction_sdk::encode::{
    init_config_ix, set_collateral_config_ix as collateral_ix, set_engine_params_ix,
};

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

    /// A plain `Token::Transfer` signed by holding `id(from)` (donations in tests), built by the SDK.
    pub fn transfer_tokens(
        &mut self,
        from: u8,
        to: AccountId,
        amount: u128,
    ) -> Result<(), LeeError> {
        let plan = auction_sdk::plan_host::token_transfer(token_program(), id(from), to, amount)
            .expect("transfer plan");
        self.run_plan(&plan, &[from])
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
        if to == buffer() {
            self.donated_buffer += amount;
        }
    }
}
