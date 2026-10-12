//! Plans (spec §6.1): `{program_id, account_ids, signing_requirements, instruction_words}`. A plan is
//! everything a signer needs; nothing is signed here.

use std::collections::BTreeSet;

use lee_core::{account::AccountId, program::ProgramId};
use serde::Serialize;

use auction_engine_core::{
    args::{CollateralArgs, EngineParamsArgs},
    pda::{SLOT_PROTOCOL_TOKEN, SLOT_REF_ORACLE},
    Instruction,
};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID as CLOCK;

use crate::{
    addresses::{EngineAddresses, HostAddresses},
    encode::{init_config_ix, set_collateral_config_ix, set_engine_params_ix, words},
    error::{SdkError, SdkResult},
    verify::VerifiedConfig,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub program_id: ProgramId,
    pub account_ids: Vec<AccountId>,
    /// Accounts that must sign, in witness order; the message nonces follow the same order.
    pub signing_requirements: Vec<AccountId>,
    pub instruction_words: Vec<u32>,
}

impl Plan {
    /// Rejects what LEZ would reject anyway: a duplicate account id, a signer that is not one of the
    /// accounts, or no signer at all (a public transaction needs one).
    pub fn new<T: Serialize>(
        program_id: ProgramId,
        account_ids: Vec<AccountId>,
        signing_requirements: Vec<AccountId>,
        ix: &T,
    ) -> SdkResult<Self> {
        let mut seen = BTreeSet::new();
        for a in &account_ids {
            if !seen.insert(*a) {
                return Err(SdkError::Encode(format!(
                    "account {a} is listed twice (LEZ rejects duplicate account ids)"
                )));
            }
        }
        if signing_requirements.is_empty() {
            return Err(SdkError::Encode(
                "a public transaction needs a signer".into(),
            ));
        }
        for s in &signing_requirements {
            if !seen.contains(s) {
                return Err(SdkError::Encode(format!(
                    "signer {s} is not one of the plan's accounts"
                )));
            }
        }
        Ok(Self {
            program_id,
            account_ids,
            signing_requirements,
            instruction_words: words(ix)?,
        })
    }

    #[must_use]
    pub fn variant_index(&self) -> Option<u32> {
        self.instruction_words.first().copied()
    }
}

fn signers(primary: AccountId, holding: AccountId, holding_is_fresh: bool) -> Vec<AccountId> {
    if holding_is_fresh {
        vec![primary, holding]
    } else {
        vec![primary]
    }
}

/// The enabled flags must agree with the slot accounts passed (ADR-030, ADR-038).
fn check_slot_flags(
    p: &EngineParamsArgs,
    protocol_token: Option<AccountId>,
    ref_oracle: Option<AccountId>,
) -> SdkResult<()> {
    if p.protocol_token_enabled != protocol_token.is_some()
        || p.ref_oracle_enabled != ref_oracle.is_some()
    {
        return Err(SdkError::Encode(
            "protocol_token_enabled / ref_oracle_enabled must match the slot accounts passed"
                .into(),
        ));
    }
    Ok(())
}

/// `init_config` (no config exists yet, so there is nothing to verify; the CLI checks the host and
/// token owners before calling this). The enabled flags must agree with the slot accounts.
pub fn init_config(
    engine_program: ProgramId,
    host: &HostAddresses,
    admin: AccountId,
    salt: u64,
    p: &EngineParamsArgs,
    protocol_token: Option<AccountId>,
    ref_oracle: Option<AccountId>,
) -> SdkResult<Plan> {
    check_slot_flags(p, protocol_token, ref_oracle)?;
    let e = EngineAddresses::new(engine_program, &admin, salt);
    Plan::new(
        engine_program,
        vec![
            admin,
            e.config,
            e.accounting,
            e.buffer,
            host.protocol_parameters,
            host.stablecoin_definition,
            protocol_token.unwrap_or(e.unused_protocol_token),
            ref_oracle.unwrap_or(e.unused_ref_oracle),
        ],
        vec![admin],
        &init_config_ix(salt, p),
    )
}

/// The slots carry the NEW values (the handler checks them against the new enabled flags).
pub fn set_engine_params(
    v: &VerifiedConfig,
    admin: AccountId,
    p: &EngineParamsArgs,
    protocol_token: Option<AccountId>,
    ref_oracle: Option<AccountId>,
) -> SdkResult<Plan> {
    check_slot_flags(p, protocol_token, ref_oracle)?;
    Plan::new(
        v.engine.program,
        vec![
            admin,
            v.config_id,
            v.engine.accounting,
            v.coll_config_id(),
            protocol_token.unwrap_or(v.engine.unused_protocol_token),
            ref_oracle.unwrap_or(v.engine.unused_ref_oracle),
        ],
        vec![admin],
        &set_engine_params_ix(p),
    )
}

pub fn set_collateral_config(
    v: &VerifiedConfig,
    admin: AccountId,
    oracle: AccountId,
    c: &CollateralArgs,
) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![
            admin,
            v.config_id,
            v.state.collateral_definition_id,
            v.coll_config_id(),
            oracle,
            v.host.protocol_parameters,
            v.host.redemption_price_state,
            CLOCK,
        ],
        vec![admin],
        &set_collateral_config_ix(c),
    )
}

pub fn observe_feed(v: &VerifiedConfig, caller: AccountId, oracle: AccountId) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![
            caller,
            v.config_id,
            v.coll_config_id(),
            v.host.redemption_price_state,
            oracle,
            CLOCK,
        ],
        vec![caller],
        &Instruction::ObserveFeed,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiquidateArgs {
    pub keeper: AccountId,
    pub keeper_collateral_holding: AccountId,
    /// A never-initialised reward holding must sign (`RecipientMustSign`).
    pub keeper_holding_is_fresh: bool,
    pub position: AccountId,
    /// `LiquidationRecord.count` (from `quote::quote_liquidation`).
    pub nonce: u64,
    /// The collateral config oracle (= the host market oracle, ADR-028).
    pub oracle: AccountId,
}

pub fn liquidate(v: &VerifiedConfig, a: &LiquidateArgs) -> SdkResult<Plan> {
    let auction = v.engine.auction(&a.position, a.nonce);
    Plan::new(
        v.engine.program,
        vec![
            a.keeper,
            a.keeper_collateral_holding,
            v.config_id,
            v.coll_config_id(),
            a.position,
            v.engine.record(&a.position),
            auction,
            v.engine.auction_vault(&auction),
            v.engine.engine_authority,
            v.host.liquidation_engine_state,
            v.host.position_vault(&a.position),
            v.state.collateral_definition_id,
            v.host.stability_fee_accumulator,
            v.host.redemption_price_state,
            a.oracle,
            v.host.protocol_parameters,
            CLOCK,
        ],
        signers(
            a.keeper,
            a.keeper_collateral_holding,
            a.keeper_holding_is_fresh,
        ),
        &Instruction::Liquidate { nonce: a.nonce },
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BidArgs {
    pub bidder_stable: AccountId,
    pub bidder_collateral: AccountId,
    pub bidder_collateral_is_fresh: bool,
    pub auction: AccountId,
    pub coin_in_max: u128,
    pub min_collateral_out: u128,
    /// ADR-029: `quote::BidQuote::needs_settlement`.
    pub with_settlement: bool,
    pub oracle: AccountId,
}

pub fn bid(v: &VerifiedConfig, a: &BidArgs) -> SdkResult<Plan> {
    let mut accounts = vec![
        a.bidder_stable,
        a.bidder_collateral,
        v.config_id,
        v.coll_config_id(),
        a.auction,
        v.engine.auction_vault(&a.auction),
        v.engine.buffer,
        v.state.stablecoin_definition_id,
        v.host.redemption_price_state,
        a.oracle,
        CLOCK,
    ];
    let ix = if a.with_settlement {
        accounts.push(v.engine.accounting);
        Instruction::BidWithSettlement {
            coin_in_max: a.coin_in_max,
            min_collateral_out: a.min_collateral_out,
        }
    } else {
        Instruction::Bid {
            coin_in_max: a.coin_in_max,
            min_collateral_out: a.min_collateral_out,
        }
    };
    Plan::new(
        v.engine.program,
        accounts,
        signers(
            a.bidder_stable,
            a.bidder_collateral,
            a.bidder_collateral_is_fresh,
        ),
        &ix,
    )
}

pub fn settle_empty(v: &VerifiedConfig, caller: AccountId, auction: AccountId) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![
            caller,
            v.config_id,
            v.coll_config_id(),
            v.engine.accounting,
            auction,
        ],
        vec![caller],
        &Instruction::SettleEmpty,
    )
}

pub fn claim_leftover(
    v: &VerifiedConfig,
    owner: AccountId,
    owner_holding: AccountId,
    owner_holding_is_fresh: bool,
    auction: AccountId,
) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![
            owner,
            owner_holding,
            v.config_id,
            auction,
            v.engine.auction_vault(&auction),
        ],
        signers(owner, owner_holding, owner_holding_is_fresh),
        &Instruction::ClaimLeftover,
    )
}

pub fn settle_debt(v: &VerifiedConfig, caller: AccountId, amount: u128) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![
            caller,
            v.config_id,
            v.engine.accounting,
            v.engine.buffer,
            v.state.stablecoin_definition_id,
        ],
        vec![caller],
        &Instruction::SettleDebt { amount },
    )
}

pub fn start_surplus(v: &VerifiedConfig, caller: AccountId, nonce: u64) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![
            caller,
            v.config_id,
            v.engine.accounting,
            v.engine.buffer,
            v.state.stablecoin_definition_id,
            v.engine.surplus_auction(nonce),
            v.engine.surplus_vault(nonce),
            v.slot(SLOT_REF_ORACLE),
            CLOCK,
        ],
        vec![caller],
        &Instruction::StartSurplusAuction { nonce },
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TakeArgs {
    /// Signs and pays: protocol-token holding (surplus) or stablecoin holding (debt).
    pub payer: AccountId,
    /// Receives: stablecoin holding (surplus) or protocol-token holding (debt).
    pub receiver: AccountId,
    pub receiver_is_fresh: bool,
    pub nonce: u64,
    pub amount_coin: u128,
    /// `max_protocol_in` (surplus) or `min_protocol_out` (debt).
    pub limit: u128,
}

pub fn take_surplus(v: &VerifiedConfig, a: &TakeArgs) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![
            a.payer,
            a.receiver,
            v.config_id,
            v.engine.accounting,
            v.engine.surplus_auction(a.nonce),
            v.engine.surplus_vault(a.nonce),
            v.slot(SLOT_PROTOCOL_TOKEN),
            CLOCK,
        ],
        signers(a.payer, a.receiver, a.receiver_is_fresh),
        &Instruction::TakeSurplus {
            amount_coin: a.amount_coin,
            max_protocol_in: a.limit,
        },
    )
}

pub fn close_surplus(v: &VerifiedConfig, caller: AccountId, nonce: u64) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![
            caller,
            v.config_id,
            v.engine.accounting,
            v.engine.surplus_auction(nonce),
            v.engine.surplus_vault(nonce),
            v.engine.buffer,
            CLOCK,
        ],
        vec![caller],
        &Instruction::CloseSurplusAuction,
    )
}

pub fn start_debt(v: &VerifiedConfig, caller: AccountId, nonce: u64) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![
            caller,
            v.config_id,
            v.engine.accounting,
            v.engine.buffer,
            v.state.stablecoin_definition_id,
            v.engine.debt_auction(nonce),
            v.slot(SLOT_PROTOCOL_TOKEN),
            v.slot(SLOT_REF_ORACLE),
            CLOCK,
        ],
        vec![caller],
        &Instruction::StartDebtAuction { nonce },
    )
}

pub fn take_debt(v: &VerifiedConfig, a: &TakeArgs) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![
            a.payer,
            a.receiver,
            v.config_id,
            v.engine.accounting,
            v.engine.debt_auction(a.nonce),
            v.state.stablecoin_definition_id,
            v.slot(SLOT_PROTOCOL_TOKEN),
            v.engine.mint_authority,
            CLOCK,
        ],
        signers(a.payer, a.receiver, a.receiver_is_fresh),
        &Instruction::TakeDebt {
            amount_coin: a.amount_coin,
            min_protocol_out: a.limit,
        },
    )
}

pub fn close_debt(v: &VerifiedConfig, caller: AccountId, nonce: u64) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![
            caller,
            v.config_id,
            v.engine.accounting,
            v.engine.debt_auction(nonce),
            CLOCK,
        ],
        vec![caller],
        &Instruction::CloseDebtAuction,
    )
}

pub fn check_solvency(v: &VerifiedConfig, caller: AccountId) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![
            caller,
            v.config_id,
            v.engine.accounting,
            v.engine.buffer,
            v.state.stablecoin_definition_id,
            v.slot(SLOT_PROTOCOL_TOKEN),
            v.slot(SLOT_REF_ORACLE),
            v.engine.engine_authority,
            v.host.liquidation_engine_state,
            v.host.protocol_parameters,
            CLOCK,
        ],
        vec![caller],
        &Instruction::CheckSolvency,
    )
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
