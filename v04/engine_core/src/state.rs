//! Borsh actor states of the engine (design note §3). Every state starts with an 8-byte tag, so an
//! encoding is never empty (an empty state deletes the actor state) and one engine state can never
//! decode as another. The tags are the v0.2.4 ones.

use auction_core::{
    accounting::Accounting,
    auction::{Auction, Status},
    liquidation::CollateralParams,
    pause::PauseClock,
    price::{FeedParams, OracleScale},
    schedule::{Schedule, SchedulePoint},
    system::SystemParams,
};
use auction_engine_params::EngineError;
use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::{AccountId, Actor};

pub use crate::scratch::{
    AuctionScratch, BidRequestRec, ObserveThen, OracleReadingRec, PendingObserve, QuoteCtx,
};

pub const TAG_CONFIG: [u8; 8] = *b"AECONFIG";
pub const TAG_COLL: [u8; 8] = *b"AECOLLAT";
pub const TAG_ACCOUNTING: [u8; 8] = *b"AEACCTNG";
pub const TAG_LIQREC: [u8; 8] = *b"AELIQREC";
pub const TAG_AUCTION: [u8; 8] = *b"AEAUCTN_";

/// `encode` / `decode` for a tagged state; `decode` is `None` unless the bytes are exactly that
/// state with its own tag.
macro_rules! tagged_state {
    ($t:ty, $tag:expr) => {
        impl $t {
            /// Borsh bytes of the state, never empty.
            #[must_use]
            pub fn encode(&self) -> Vec<u8> {
                borsh::to_vec(self).expect("engine state encodes")
            }

            /// Decodes the state, `None` if the bytes are not exactly this state.
            #[must_use]
            pub fn decode(bytes: &[u8]) -> Option<Self> {
                borsh::from_slice::<Self>(bytes)
                    .ok()
                    .filter(|state| state.tag == $tag)
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

/// Engine config at `(pda(E, ["CONFIG", admin, salt]), E)`. The admin, the host, the token program,
/// the oracle and the clock are fixed at `Init`; the program fields are program addresses.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ConfigState {
    pub tag: [u8; 8],
    pub salt: u64,
    pub admin: AccountId,
    /// The host's params actor `(params, H)`.
    pub host: Actor,
    /// The token program's address `T`.
    pub token: AccountId,
    pub stablecoin_definition_id: AccountId,
    /// The oracle actor read for collateral prices.
    pub oracle: Actor,
    /// The clock actor that `AssertTimestamp` goes to.
    pub clock: Actor,
    /// Slack of the claimed `now` against the clock, ms.
    pub slack_ms: u64,
    pub protocol_token: Option<AccountId>,
    pub protocol_token_ref_price: u128,
    pub protocol_token_oracle: Option<Actor>,
    pub ref_oracle_max_age_ms: u64,
    pub global_cap: u128,
    pub global_reserved: u128,
    pub freeze_on_bad_debt: bool,
    pub bad_debt_freeze_threshold: u128,
    pub system: SystemParamsRec,
    /// Read-block scratch (design note §4.1): the actor whose `StateReply` the config awaits.
    /// Set by the handler that sends the `ReadState`, cleared by the reply, and asserted `None`
    /// by the `Proceed` sentinel, so it is `None` in every committed state.
    pub pending_read: Option<Actor>,
}
tagged_state!(ConfigState, TAG_CONFIG);

/// Collateral config at `(pda(E, ["COLL", config, def]), E)`. One per collateral definition; the
/// oracle is the config's.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CollateralState {
    pub tag: [u8; 8],
    pub config: AccountId,
    pub collateral_definition_id: AccountId,
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
    /// The config's actors and slack the Observe block uses, copied by the config when it relays
    /// `SetCollateral` (all fixed at `Init`).
    pub ctx: ObserveCtx,
    /// Observe-block scratch (design note §4.1): the pending reads and what follows them. Set by
    /// the handler that starts an observation, filled by the replies, consumed and cleared by the
    /// `Proceed` sentinel, so it is `None` in every committed state.
    pub pending: Option<PendingObserve>,
}
tagged_state!(CollateralState, TAG_COLL);

/// What the collateral actor needs from its config to observe: the host params actor (redemption
/// price, ADR-028 oracle), the oracle, the clock and the clock slack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ObserveCtx {
    pub host: Actor,
    pub oracle: Actor,
    pub clock: Actor,
    pub slack_ms: u64,
}

impl CollateralState {
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

/// Accounting at `(pda(E, ["ACCT", config]), E)`: the core `Accounting` fields plus the
/// program-layer system-auction state.
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
    /// When the last debt auction ended (close or final take), ms; 0 before the first one (ADR-039).
    pub last_debt_end_ms: u64,
}
tagged_state!(AccountingState, TAG_ACCOUNTING);

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
            last_debt_end_ms: 0,
        }
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

/// Liquidation record at `(pda(E, ["LIQREC", config, position]), E)`: the per-position auction
/// counter (ADR-004, ADR-020). `count` is incremented only by auction creation (`Bump`).
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct LiquidationRecordState {
    pub tag: [u8; 8],
    pub position: AccountId,
    pub count: u64,
}
tagged_state!(LiquidationRecordState, TAG_LIQREC);

/// Where an auction is in its life. Replaces v0.2.4's `settled` flag (design note §4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum AuctionPhase {
    /// Opened, waiting for the host's `Seized` report.
    AwaitingSeize,
    Auctioning,
    Settled,
}

/// Auction at `(pda(E, ["AUCTION", config, position, nonce]), E)`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct AuctionState {
    pub tag: [u8; 8],
    pub config: AccountId,
    /// The requested position, as the liquidation named it.
    pub position: AccountId,
    pub nonce: u64,
    /// The host params actor bound at `Open`; `Seized` is accepted only from
    /// `Actor(position, host.program_account_id)`.
    pub host: Actor,
    /// The token program `T`, the collateral definition and the stablecoin definition, bound at
    /// `Open` by the config: the vault is `(pda(E, ["VAULT", auction]), T)`, the bid's `BidCtx`
    /// is accepted only from `collateral_actor(E, config, collateral_def)`.
    pub token: AccountId,
    pub collateral_def: AccountId,
    pub stable_def: AccountId,
    pub phase: AuctionPhase,
    pub owner: AccountId,
    pub keeper: AccountId,
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
    /// Liquidation scratch (design note §4.3 a): the quote inputs between `Open` and `Seized`,
    /// then the vault check between `Seized` and its `StateReply`. Cleared before the `Finish`
    /// sentinel, so it is `None` in every committed state.
    pub scratch: Option<AuctionScratch>,
}
tagged_state!(AuctionState, TAG_AUCTION);

impl AuctionState {
    /// The core view; an auction still `AwaitingSeize` has no core state yet.
    pub fn to_core(&self) -> Result<Auction, EngineError> {
        let status = match self.phase {
            AuctionPhase::AwaitingSeize => return Err(EngineError::NotAuctioning),
            AuctionPhase::Auctioning => Status::Auctioning,
            AuctionPhase::Settled => Status::Settled,
        };
        Ok(Auction {
            status,
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

    /// Keeps the identity fields of `base` and copies every core field of `a`; the phase follows
    /// the core status.
    #[must_use]
    pub fn from_core(base: &Self, a: &Auction) -> Self {
        Self {
            phase: if a.status == Status::Settled {
                AuctionPhase::Settled
            } else {
                AuctionPhase::Auctioning
            },
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

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;

    pub fn id(b: u8) -> AccountId {
        AccountId::new([b; 32])
    }

    pub fn actor(a: u8, p: u8) -> Actor {
        Actor::new(id(a), id(p))
    }

    pub fn point() -> SchedulePointRec {
        SchedulePointRec {
            elapsed_ms: 0,
            discount_bps: 200,
        }
    }

    pub fn sys() -> SystemParamsRec {
        SystemParamsRec {
            surplus_buffer_target: 1_000,
            surplus_lot: 500,
            surplus_delay_ms: 0,
            surplus_premium_bps: 500,
            surplus_max_discount_bps: 2_000,
            debt_lot: 500,
            debt_initial_discount_bps: 0,
            debt_growth: vec![point()],
            surplus_decay: vec![point()],
            max_mint_per_auction: 5_000,
            min_take: 10,
            system_ttl_ms: 14_400_000,
        }
    }

    pub fn config() -> ConfigState {
        ConfigState {
            tag: TAG_CONFIG,
            salt: 9,
            admin: id(1),
            host: actor(2, 7),
            token: id(8),
            stablecoin_definition_id: id(3),
            oracle: actor(10, 11),
            clock: actor(12, 13),
            slack_ms: 6_000,
            protocol_token: Some(id(5)),
            protocol_token_ref_price: 42,
            protocol_token_oracle: None,
            ref_oracle_max_age_ms: 3_600_000,
            global_cap: 10,
            global_reserved: 1,
            freeze_on_bad_debt: true,
            bad_debt_freeze_threshold: 600,
            system: sys(),
            pending_read: None,
        }
    }

    pub fn coll() -> CollateralState {
        CollateralState {
            tag: TAG_COLL,
            config: id(1),
            collateral_definition_id: id(4),
            oracle_scale: 0,
            max_oracle_age_ms: 3_600_000,
            max_conf_bps: 100,
            band_lo_bps: 9_000,
            band_hi_bps: 11_000,
            penalty_bps: 1_000,
            tip: 5,
            chip_bps: 10,
            reward_cap: 50,
            debt_floor: 100,
            min_bid: 25,
            type_cap: 5,
            type_reserved: 0,
            schedule: vec![point()],
            admin_paused: false,
            paused_since_ms: Some(5),
            paused_total_ms: 6,
            last_seen_oracle_ts: Some(7),
            ctx: ObserveCtx {
                host: actor(2, 7),
                oracle: actor(10, 11),
                clock: actor(12, 13),
                slack_ms: 6_000,
            },
            pending: None,
        }
    }

    pub fn liqrec() -> LiquidationRecordState {
        LiquidationRecordState {
            tag: TAG_LIQREC,
            position: id(2),
            count: 3,
        }
    }

    pub fn auction(phase: AuctionPhase) -> AuctionState {
        AuctionState {
            tag: TAG_AUCTION,
            config: id(1),
            position: id(2),
            nonce: 0,
            host: actor(2, 7),
            token: id(8),
            collateral_def: id(4),
            stable_def: id(3),
            phase,
            owner: id(20),
            keeper: id(21),
            collateral_seized: 100,
            collateral_sold: 0,
            collateral_left: 100,
            keeper_reward: 5,
            leftover_claimable: 0,
            leftover_claimed: 0,
            debt_principal: 60,
            amount_to_raise: 66,
            reserved_amount: 100,
            debt_raised: 0,
            penalty_raised: 0,
            start_ms: 1_000,
            schedule: vec![point()],
            type_paused_total_at_start: 0,
            scratch: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fixtures::*, *};

    #[test]
    fn every_state_encodes_non_empty() {
        let encodings = [
            config().encode(),
            coll().encode(),
            AccountingState::new().encode(),
            liqrec().encode(),
            auction(AuctionPhase::AwaitingSeize).encode(),
        ];
        for bytes in &encodings {
            assert!(bytes.len() > 8, "tag plus fields");
        }
        let tags = [
            TAG_CONFIG,
            TAG_COLL,
            TAG_ACCOUNTING,
            TAG_LIQREC,
            TAG_AUCTION,
        ];
        for (bytes, tag) in encodings.iter().zip(tags) {
            assert_eq!(bytes.get(..8), Some(&tag[..]), "tag is the 8-byte prefix");
        }
    }

    #[test]
    fn every_state_round_trips_and_rejects_foreign_tags() {
        let (c, k, a, l, u) = (
            config(),
            coll(),
            AccountingState::new(),
            liqrec(),
            auction(AuctionPhase::Auctioning),
        );
        assert_eq!(ConfigState::decode(&c.encode()), Some(c.clone()));
        assert_eq!(CollateralState::decode(&k.encode()), Some(k.clone()));
        assert_eq!(AccountingState::decode(&a.encode()), Some(a.clone()));
        assert_eq!(LiquidationRecordState::decode(&l.encode()), Some(l.clone()));
        assert_eq!(AuctionState::decode(&u.encode()), Some(u.clone()));
        // A correctly sized buffer with the wrong tag is rejected by the tag check, not by length.
        let wrong = LiquidationRecordState {
            tag: TAG_AUCTION,
            ..l
        };
        assert_eq!(LiquidationRecordState::decode(&wrong.encode()), None);
        let wrong = ConfigState {
            tag: TAG_COLL,
            ..c.clone()
        };
        assert_eq!(ConfigState::decode(&wrong.encode()), None);
        // Cross-kind and empty decoding fail.
        assert_eq!(ConfigState::decode(&k.encode()), None);
        assert_eq!(AuctionState::decode(&c.encode()), None);
        assert_eq!(ConfigState::decode(&[]), None);
    }

    #[test]
    fn pause_clock_round_trips() {
        let k = coll();
        let clock = k.pause_clock();
        assert_eq!(clock.paused_since_ms, Some(5));
        assert_eq!(clock.paused_total_ms, 6);
        assert_eq!(clock.last_seen_oracle_ts, Some(7));
        let moved = k.with_pause_clock(PauseClock {
            paused_since_ms: None,
            paused_total_ms: 60,
            last_seen_oracle_ts: Some(70),
        });
        assert_eq!(moved.paused_since_ms, None);
        assert_eq!(moved.paused_total_ms, 60);
        assert_eq!(moved.last_seen_oracle_ts, Some(70));
        assert_eq!(moved.penalty_bps, k.penalty_bps, "other fields untouched");
        assert_eq!(k.with_pause_clock(clock), k);
    }

    #[test]
    fn system_params_rec_converts_and_validates() {
        let rec = sys();
        let core = rec.to_core().expect("valid");
        assert_eq!(core.debt_lot, 500);
        assert_eq!(SystemParamsRec::from_core(&core), rec);
        let bad = SystemParamsRec {
            surplus_lot: 0,
            ..rec.clone()
        };
        assert_eq!(bad.to_core(), Err(EngineError::InvalidConfig));
        let empty = SystemParamsRec {
            debt_growth: vec![],
            ..rec
        };
        assert!(empty.to_core().is_err());
    }

    #[test]
    fn core_views_round_trip() {
        let a = AccountingState::new();
        assert_eq!(AccountingState::with_core(&a, &a.core()), a);
        let k = coll();
        assert_eq!(k.collateral_params().expect("valid").penalty_bps, 1_000);
        assert!(k.feed_params().is_ok());
        let bad = CollateralState {
            oracle_scale: 2,
            ..k
        };
        assert_eq!(bad.feed_params(), Err(EngineError::InvalidConfig));
    }

    #[test]
    fn auction_core_view_follows_the_phase() {
        let live = auction(AuctionPhase::Auctioning);
        let core = live.to_core().expect("auctioning has a core view");
        assert_eq!(core.status, Status::Auctioning);
        assert_eq!(AuctionState::from_core(&live, &core), live);
        let settled = Auction {
            status: Status::Settled,
            ..core
        };
        let after = AuctionState::from_core(&live, &settled);
        assert_eq!(after.phase, AuctionPhase::Settled);
        assert_eq!(after.host, live.host, "identity fields are kept");
        assert_eq!(
            auction(AuctionPhase::AwaitingSeize).to_core(),
            Err(EngineError::NotAuctioning)
        );
    }
}
