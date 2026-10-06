//! Borsh account states of the engine (Shared Reference R1) and their conversions to `auction_core`.
//! Every state starts with an 8-byte tag so one engine account type can never decode as another.

use auction_core::{
    accounting::Accounting,
    auction::{Auction, Status},
    liquidation::CollateralParams,
    pause::PauseClock,
    price::{FeedParams, OracleScale},
    schedule::{Schedule, SchedulePoint},
    system::{DebtAuction, SurplusAuction, SystemParams},
};
use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{
    account::{AccountId, Data},
    program::ProgramId,
};

use crate::EngineError;

pub const TAG_CONFIG: [u8; 8] = *b"AECONFIG";
pub const TAG_COLL: [u8; 8] = *b"AECOLLAT";
pub const TAG_ACCOUNTING: [u8; 8] = *b"AEACCTNG";
pub const TAG_LIQREC: [u8; 8] = *b"AELIQREC";
pub const TAG_AUCTION: [u8; 8] = *b"AEAUCTN_";
pub const TAG_SURPLUS: [u8; 8] = *b"AESURPLS";
pub const TAG_DEBT: [u8; 8] = *b"AEDEBT__";

macro_rules! account_data {
    ($t:ty, $tag:expr) => {
        impl TryFrom<&Data> for $t {
            type Error = std::io::Error;
            fn try_from(data: &Data) -> Result<Self, Self::Error> {
                let v = <$t>::try_from_slice(data.as_ref())?;
                if v.tag != $tag {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "wrong account tag",
                    ));
                }
                Ok(v)
            }
        }
        impl From<&$t> for Data {
            fn from(v: &$t) -> Self {
                let buf = borsh::to_vec(v).expect("engine state serialises");
                Data::try_from(buf).expect("engine state fits into Data")
            }
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SchedulePointRec {
    pub elapsed_ms: u64,
    pub discount_bps: u16,
}

#[must_use]
pub fn recs_from_schedule(s: &Schedule) -> Vec<SchedulePointRec> {
    s.points()
        .iter()
        .map(|p| SchedulePointRec {
            elapsed_ms: p.elapsed_ms,
            discount_bps: p.discount_bps,
        })
        .collect()
}

pub fn schedule_from_recs(recs: &[SchedulePointRec]) -> Result<Schedule, EngineError> {
    let pts: Vec<SchedulePoint> = recs
        .iter()
        .map(|r| SchedulePoint {
            elapsed_ms: r.elapsed_ms,
            discount_bps: r.discount_bps,
        })
        .collect();
    Ok(Schedule::new(&pts)?)
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SystemParamsRec {
    pub surplus_buffer_target: u128,
    pub surplus_lot: u128,
    pub surplus_delay_ms: u64,
    pub surplus_premium_bps: u16,
    pub surplus_max_discount_bps: u16,
    pub debt_lot: u128,
    pub debt_initial_discount_bps: u16,
    pub debt_growth: Vec<SchedulePointRec>,
    pub surplus_decay: Vec<SchedulePointRec>,
    pub max_mint_per_auction: u128,
    pub min_take: u128,
    pub system_ttl_ms: u64,
}

impl SystemParamsRec {
    #[must_use]
    pub fn from_core(s: &SystemParams) -> Self {
        Self {
            surplus_buffer_target: s.surplus_buffer_target,
            surplus_lot: s.surplus_lot,
            surplus_delay_ms: s.surplus_delay_ms,
            surplus_premium_bps: s.surplus_premium_bps,
            surplus_max_discount_bps: s.surplus_max_discount_bps,
            debt_lot: s.debt_lot,
            debt_initial_discount_bps: s.debt_initial_discount_bps,
            debt_growth: recs_from_schedule(&s.debt_growth),
            surplus_decay: recs_from_schedule(&s.surplus_decay),
            max_mint_per_auction: s.max_mint_per_auction,
            min_take: s.min_take,
            system_ttl_ms: s.system_ttl_ms,
        }
    }

    pub fn to_core(&self) -> Result<SystemParams, EngineError> {
        let s = SystemParams {
            surplus_buffer_target: self.surplus_buffer_target,
            surplus_lot: self.surplus_lot,
            surplus_delay_ms: self.surplus_delay_ms,
            surplus_premium_bps: self.surplus_premium_bps,
            surplus_max_discount_bps: self.surplus_max_discount_bps,
            debt_lot: self.debt_lot,
            debt_initial_discount_bps: self.debt_initial_discount_bps,
            debt_growth: schedule_from_recs(&self.debt_growth)?,
            surplus_decay: schedule_from_recs(&self.surplus_decay)?,
            max_mint_per_auction: self.max_mint_per_auction,
            min_take: self.min_take,
            system_ttl_ms: self.system_ttl_ms,
        };
        s.validate()?;
        Ok(s)
    }
}

/// `["CONFIG", admin, salt]`. Admin, host binding and token program are fixed at `init_config`
/// (the admin is part of the PDA; clients verify the stored program ids, I4).
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct EngineConfigState {
    pub tag: [u8; 8],
    pub salt: u64,
    pub admin: AccountId,
    pub host_program_id: ProgramId,
    pub host_protocol_parameters_id: AccountId,
    pub stablecoin_definition_id: AccountId,
    pub collateral_definition_id: AccountId,
    pub token_program_id: ProgramId,
    pub protocol_token: Option<AccountId>,
    pub protocol_token_ref_price: u128,
    pub protocol_token_oracle_id: Option<AccountId>,
    pub ref_oracle_max_age_ms: u64,
    pub global_cap: u128,
    pub global_reserved: u128,
    pub freeze_on_bad_debt: bool,
    pub bad_debt_freeze_threshold: u128,
    pub system: SystemParamsRec,
}
account_data!(EngineConfigState, TAG_CONFIG);

/// `["COLL", config, collateral_definition]`. One per engine instance (the host has one collateral type).
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CollateralConfigState {
    pub tag: [u8; 8],
    pub config: AccountId,
    pub collateral_definition_id: AccountId,
    pub oracle_id: AccountId,
    /// 0 = Ray, 1 = Q64x64.
    pub oracle_scale: u8,
    pub max_oracle_age_ms: u64,
    pub max_conf_bps: u16,
    pub band_lo_bps: u16,
    pub band_hi_bps: u16,
    pub penalty_bps: u16,
    pub tip: u128,
    pub chip_bps: u16,
    pub reward_cap: u128,
    pub debt_floor: u128,
    pub min_bid: u128,
    pub type_cap: u128,
    pub type_reserved: u128,
    pub schedule: Vec<SchedulePointRec>,
    pub admin_paused: bool,
    pub paused_since_ms: Option<u64>,
    pub paused_total_ms: u64,
    pub last_seen_oracle_ts: Option<u64>,
}
account_data!(CollateralConfigState, TAG_COLL);

impl CollateralConfigState {
    pub fn feed_params(&self) -> Result<FeedParams, EngineError> {
        let scale = match self.oracle_scale {
            0 => OracleScale::Ray,
            1 => OracleScale::Q64x64,
            _ => return Err(EngineError::InvalidConfig),
        };
        Ok(FeedParams {
            scale,
            max_age_ms: self.max_oracle_age_ms,
            max_conf_bps: self.max_conf_bps,
            band_lo_bps: self.band_lo_bps,
            band_hi_bps: self.band_hi_bps,
        })
    }

    pub fn collateral_params(&self) -> Result<CollateralParams, EngineError> {
        Ok(CollateralParams {
            penalty_bps: self.penalty_bps,
            tip: self.tip,
            chip_bps: self.chip_bps,
            reward_cap: self.reward_cap,
            debt_floor: self.debt_floor,
            min_bid: self.min_bid,
            schedule: schedule_from_recs(&self.schedule)?,
        })
    }

    #[must_use]
    pub fn pause_clock(&self) -> PauseClock {
        PauseClock {
            paused_since_ms: self.paused_since_ms,
            paused_total_ms: self.paused_total_ms,
            last_seen_oracle_ts: self.last_seen_oracle_ts,
        }
    }

    #[must_use]
    pub fn with_pause_clock(&self, c: PauseClock) -> Self {
        Self {
            paused_since_ms: c.paused_since_ms,
            paused_total_ms: c.paused_total_ms,
            last_seen_oracle_ts: c.last_seen_oracle_ts,
            ..self.clone()
        }
    }
}

/// `["ACCT", config]`: the core `Accounting` fields plus the program-layer system-auction state.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct AccountingState {
    pub tag: [u8; 8],
    pub unbacked_debt: u128,
    pub total_shortfall_booked: u128,
    pub total_penalty_collected: u128,
    pub total_debt_burned: u128,
    pub total_debt_settled: u128,
    pub surplus_nonce: u64,
    pub debt_nonce: u64,
    pub host_frozen_by_engine: bool,
    pub debt_on_auction: u128,
    pub total_debt_auctioned: u128,
    pub surplus_live: bool,
    pub debt_live: bool,
    pub last_surplus_end_ms: u64,
    pub total_protocol_burned: u128,
    pub total_protocol_minted: u128,
    pub total_surplus_lots_out: u128,
    pub total_surplus_returned: u128,
}
account_data!(AccountingState, TAG_ACCOUNTING);

impl AccountingState {
    #[must_use]
    pub fn new() -> Self {
        Self::with_core(&Self::empty(), &Accounting::default())
    }

    fn empty() -> Self {
        Self {
            tag: TAG_ACCOUNTING,
            unbacked_debt: 0,
            total_shortfall_booked: 0,
            total_penalty_collected: 0,
            total_debt_burned: 0,
            total_debt_settled: 0,
            surplus_nonce: 0,
            debt_nonce: 0,
            host_frozen_by_engine: false,
            debt_on_auction: 0,
            total_debt_auctioned: 0,
            surplus_live: false,
            debt_live: false,
            last_surplus_end_ms: 0,
            total_protocol_burned: 0,
            total_protocol_minted: 0,
            total_surplus_lots_out: 0,
            total_surplus_returned: 0,
        }
    }

    /// Test helper so other crates can build a tagged, empty state.
    #[must_use]
    pub fn default_for_tests() -> Self {
        Self::new()
    }

    #[must_use]
    pub fn core(&self) -> Accounting {
        Accounting {
            unbacked_debt: self.unbacked_debt,
            total_shortfall_booked: self.total_shortfall_booked,
            total_penalty_collected: self.total_penalty_collected,
            total_debt_burned: self.total_debt_burned,
            total_debt_settled: self.total_debt_settled,
            surplus_nonce: self.surplus_nonce,
            debt_nonce: self.debt_nonce,
            host_frozen_by_engine: self.host_frozen_by_engine,
            debt_on_auction: self.debt_on_auction,
            total_debt_auctioned: self.total_debt_auctioned,
        }
    }

    #[must_use]
    pub fn with_core(base: &Self, a: &Accounting) -> Self {
        Self {
            unbacked_debt: a.unbacked_debt,
            total_shortfall_booked: a.total_shortfall_booked,
            total_penalty_collected: a.total_penalty_collected,
            total_debt_burned: a.total_debt_burned,
            total_debt_settled: a.total_debt_settled,
            surplus_nonce: a.surplus_nonce,
            debt_nonce: a.debt_nonce,
            host_frozen_by_engine: a.host_frozen_by_engine,
            debt_on_auction: a.debt_on_auction,
            total_debt_auctioned: a.total_debt_auctioned,
            ..base.clone()
        }
    }
}

impl Default for AccountingState {
    fn default() -> Self {
        Self::new()
    }
}

/// `["LIQREC", config, position]`: the per-position auction counter (ADR-004, ADR-020).
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct LiquidationRecordState {
    pub tag: [u8; 8],
    pub position: AccountId,
    pub count: u64,
}
account_data!(LiquidationRecordState, TAG_LIQREC);

/// `["AUCTION", config, position, nonce]`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct AuctionState {
    pub tag: [u8; 8],
    pub config: AccountId,
    pub position: AccountId,
    pub nonce: u64,
    pub owner: AccountId,
    pub keeper: AccountId,
    pub settled: bool,
    pub collateral_seized: u128,
    pub collateral_sold: u128,
    pub collateral_left: u128,
    pub keeper_reward: u128,
    pub leftover_claimable: u128,
    pub leftover_claimed: u128,
    pub debt_principal: u128,
    pub amount_to_raise: u128,
    pub reserved_amount: u128,
    pub debt_raised: u128,
    pub penalty_raised: u128,
    pub start_ms: u64,
    pub schedule: Vec<SchedulePointRec>,
    pub type_paused_total_at_start: u64,
}
account_data!(AuctionState, TAG_AUCTION);

impl AuctionState {
    pub fn to_core(&self) -> Result<Auction, EngineError> {
        Ok(Auction {
            status: if self.settled {
                Status::Settled
            } else {
                Status::Auctioning
            },
            collateral_seized: self.collateral_seized,
            collateral_sold: self.collateral_sold,
            collateral_left: self.collateral_left,
            keeper_reward: self.keeper_reward,
            leftover_claimable: self.leftover_claimable,
            leftover_claimed: self.leftover_claimed,
            debt_principal: self.debt_principal,
            amount_to_raise: self.amount_to_raise,
            reserved_amount: self.reserved_amount,
            debt_raised: self.debt_raised,
            penalty_raised: self.penalty_raised,
            start_ms: self.start_ms,
            schedule: schedule_from_recs(&self.schedule)?,
            type_paused_total_at_start: self.type_paused_total_at_start,
        })
    }

    /// Keeps the identity fields of `base` and copies every core field of `a`.
    #[must_use]
    pub fn from_core(base: &Self, a: &Auction) -> Self {
        Self {
            settled: a.status == Status::Settled,
            collateral_seized: a.collateral_seized,
            collateral_sold: a.collateral_sold,
            collateral_left: a.collateral_left,
            keeper_reward: a.keeper_reward,
            leftover_claimable: a.leftover_claimable,
            leftover_claimed: a.leftover_claimed,
            debt_principal: a.debt_principal,
            amount_to_raise: a.amount_to_raise,
            reserved_amount: a.reserved_amount,
            debt_raised: a.debt_raised,
            penalty_raised: a.penalty_raised,
            start_ms: a.start_ms,
            schedule: recs_from_schedule(&a.schedule),
            type_paused_total_at_start: a.type_paused_total_at_start,
            ..base.clone()
        }
    }
}

/// `["SURP", config, nonce]`. `ref_ray` is snapshotted at start, so takes need no oracle account.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SurplusAuctionState {
    pub tag: [u8; 8],
    pub config: AccountId,
    pub nonce: u64,
    pub lot_left: u128,
    pub start_ms: u64,
    pub live: bool,
    pub ref_ray: u128,
}
account_data!(SurplusAuctionState, TAG_SURPLUS);

impl SurplusAuctionState {
    #[must_use]
    pub fn core(&self) -> SurplusAuction {
        SurplusAuction {
            lot_left: self.lot_left,
            start_ms: self.start_ms,
            live: self.live,
        }
    }
    #[must_use]
    pub fn with_core(&self, a: &SurplusAuction) -> Self {
        Self {
            lot_left: a.lot_left,
            start_ms: a.start_ms,
            live: a.live,
            ..self.clone()
        }
    }
}

/// `["DEBT", config, nonce]`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct DebtAuctionState {
    pub tag: [u8; 8],
    pub config: AccountId,
    pub nonce: u64,
    pub lot_left: u128,
    pub start_amount: u128,
    pub start_ms: u64,
    pub live: bool,
}
account_data!(DebtAuctionState, TAG_DEBT);

impl DebtAuctionState {
    #[must_use]
    pub fn core(&self) -> DebtAuction {
        DebtAuction {
            lot_left: self.lot_left,
            start_amount: self.start_amount,
            start_ms: self.start_ms,
            live: self.live,
        }
    }
    #[must_use]
    pub fn with_core(&self, a: &DebtAuction) -> Self {
        Self {
            lot_left: a.lot_left,
            start_amount: a.start_amount,
            start_ms: a.start_ms,
            live: a.live,
            ..self.clone()
        }
    }
}
