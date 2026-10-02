//! F4 debt auction and F5 surplus auction: instant Dutch auctions, plus the solvency decision (spec §5.11, ADR-017).
#![allow(clippy::arithmetic_side_effects)] // `BPS - x` with x <= 3000 (checked by SystemParams::validate); `lot_left - take` with take <= lot_left
use crate::accounting::Accounting;
use crate::error::ConfigError;
use crate::math::{mul_div_ceil, mul_div_floor};
use crate::schedule::Schedule;
use crate::units::{BPS, BPS_TO_RAY, RAY};

pub const MAX_MINT_HARD_CAP: u128 = 1_000_000_000_000_000_000_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemAuctionError {
    TokenNotConfigured,
    UnbackedDebtPresent,
    BufferBelowTarget,
    DelayNotElapsed,
    InsufficientBadDebt,
    MintCapConfig,
    MintAuthorityMissing,
    ProtocolTokenMismatch,
    DebtAuctionAlreadyLive,
    SurplusAuctionAlreadyLive,
    NotLive,
    Slippage,
    DustRemainder,
    ZeroFill,
    Math,
}
impl From<crate::math::MathError> for SystemAuctionError {
    fn from(_: crate::math::MathError) -> Self {
        Self::Math
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemParams {
    pub surplus_buffer_target: u128,
    pub surplus_lot: u128,
    pub surplus_delay_ms: u64,
    pub surplus_premium_bps: u16,
    pub surplus_max_discount_bps: u16,
    pub debt_lot: u128,
    pub debt_initial_discount_bps: u16,
    pub debt_growth: Schedule,
    pub surplus_decay: Schedule,
    pub max_mint_per_auction: u128,
    pub min_take: u128,
    pub system_ttl_ms: u64,
}

/// Maximum discount for system auctions, in bps (spec §5.12).
const MAX_SYSTEM_DISCOUNT_BPS: u16 = 3000;

impl SystemParams {
    /// Config checks that the program layer must run before storing params. The auction functions assume them.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.surplus_max_discount_bps > MAX_SYSTEM_DISCOUNT_BPS
            || self.debt_initial_discount_bps > MAX_SYSTEM_DISCOUNT_BPS
        {
            return Err(ConfigError::SystemDiscountAboveMax);
        }
        if self.surplus_lot == 0 || self.debt_lot == 0 {
            return Err(ConfigError::ZeroLot);
        }
        if self.min_take > self.surplus_lot || self.min_take > self.debt_lot {
            return Err(ConfigError::MinTakeAboveLot);
        }
        if self.max_mint_per_auction == 0 {
            return Err(ConfigError::MintCapZero);
        }
        if self.max_mint_per_auction > MAX_MINT_HARD_CAP {
            return Err(ConfigError::MintCapAboveHardCap);
        }
        if self.system_ttl_ms == 0 {
            return Err(ConfigError::ZeroTtl);
        }
        Ok(())
    }
}

/// Closes a live surplus auction after `system_ttl_ms`; returns the untaken coin to hand back to the buffer.
pub fn close_surplus(
    a: &SurplusAuction,
    s: &SystemParams,
    now_ms: u64,
) -> Result<(SurplusAuction, u128), SystemAuctionError> {
    if !a.live {
        return Err(SystemAuctionError::NotLive);
    }
    if now_ms < a.start_ms.saturating_add(s.system_ttl_ms) {
        return Err(SystemAuctionError::DelayNotElapsed);
    }
    Ok((
        SurplusAuction {
            lot_left: 0,
            live: false,
            ..*a
        },
        a.lot_left,
    ))
}

/// Closes a live debt auction after `system_ttl_ms`; nothing is minted for the untaken part. Pair the returned
/// released lot with `Accounting::record_debt_close`.
pub fn close_debt(
    a: &DebtAuction,
    s: &SystemParams,
    now_ms: u64,
) -> Result<(DebtAuction, u128), SystemAuctionError> {
    if !a.live {
        return Err(SystemAuctionError::NotLive);
    }
    if now_ms < a.start_ms.saturating_add(s.system_ttl_ms) {
        return Err(SystemAuctionError::DelayNotElapsed);
    }
    Ok((
        DebtAuction {
            lot_left: 0,
            live: false,
            ..*a
        },
        a.lot_left,
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurplusAuction {
    pub lot_left: u128,
    pub start_ms: u64,
    pub live: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DebtAuction {
    pub lot_left: u128,
    pub start_amount: u128,
    pub start_ms: u64,
    pub live: bool,
}

/// Multiplier in bps at time t: (1e4 + premium − decay(t)), floored at (1e4 − max_discount).
fn surplus_multiplier_bps(s: &SystemParams, t_ms: u64) -> u128 {
    let decay_bps = s.surplus_decay.discount_ray(t_ms) / BPS_TO_RAY;
    let start = BPS + s.surplus_premium_bps as u128;
    let floor = BPS - s.surplus_max_discount_bps as u128; // validated: x <= 3000 < BPS
    start.saturating_sub(decay_bps).max(floor)
}

/// Protocol tokens a taker pays for `coin` stablecoin: ceil(coin × mult / 1e4 × RAY / ref). Owed → round up.
pub fn surplus_required_tokens(
    coin: u128,
    ref_ray: u128,
    s: &SystemParams,
    t_ms: u64,
) -> Result<u128, SystemAuctionError> {
    let scaled = mul_div_ceil(coin, surplus_multiplier_bps(s, t_ms), BPS)?;
    Ok(mul_div_ceil(scaled, RAY, ref_ray)?)
}

/// floor(debt_lot × RAY / ref × (1e4 − init_discount) / 1e4); errors with MintCapConfig if above the cap.
pub fn debt_start_amount(ref_ray: u128, s: &SystemParams) -> Result<u128, SystemAuctionError> {
    let tokens = mul_div_floor(s.debt_lot, RAY, ref_ray)?;
    let amount = mul_div_floor(tokens, BPS - s.debt_initial_discount_bps as u128, BPS)?; // validated: x <= 3000 < BPS
    if amount > s.max_mint_per_auction || s.max_mint_per_auction > MAX_MINT_HARD_CAP {
        return Err(SystemAuctionError::MintCapConfig);
    }
    Ok(amount)
}

/// Offered tokens for the whole lot at t: floor(start × (1e4 + growth(t)) / 1e4), capped. Paid out → round down.
pub fn debt_offered_tokens(
    start_amount: u128,
    s: &SystemParams,
    t_ms: u64,
) -> Result<u128, SystemAuctionError> {
    let growth_bps = s.debt_growth.discount_ray(t_ms) / BPS_TO_RAY;
    Ok(mul_div_floor(start_amount, BPS + growth_bps, BPS)?.min(s.max_mint_per_auction))
}

pub fn start_surplus(
    acc: &Accounting,
    buffer: u128,
    token_configured: bool,
    last_end_ms: u64,
    now_ms: u64,
    s: &SystemParams,
    live: bool,
) -> Result<SurplusAuction, SystemAuctionError> {
    if live {
        return Err(SystemAuctionError::SurplusAuctionAlreadyLive);
    }
    if !token_configured {
        return Err(SystemAuctionError::TokenNotConfigured);
    }
    if acc.unbacked_debt > 0 {
        return Err(SystemAuctionError::UnbackedDebtPresent);
    }
    if buffer < s.surplus_buffer_target.saturating_add(s.surplus_lot) {
        return Err(SystemAuctionError::BufferBelowTarget);
    }
    if now_ms < last_end_ms.saturating_add(s.surplus_delay_ms) {
        return Err(SystemAuctionError::DelayNotElapsed);
    }
    Ok(SurplusAuction {
        lot_left: s.surplus_lot,
        start_ms: now_ms,
        live: true,
    })
}

/// The coin actually filled is `a.lot_left - new.lot_left` (<= requested `coin`); callers must transfer that amount.
pub fn take_surplus(
    a: &SurplusAuction,
    coin: u128,
    max_tokens_in: u128,
    ref_ray: u128,
    s: &SystemParams,
    now_ms: u64,
) -> Result<(SurplusAuction, u128), SystemAuctionError> {
    if !a.live {
        return Err(SystemAuctionError::NotLive);
    }
    let take = coin.min(a.lot_left);
    if take == 0 {
        return Err(SystemAuctionError::ZeroFill);
    }
    let left = a.lot_left - take;
    if left != 0 && left < s.min_take {
        return Err(SystemAuctionError::DustRemainder);
    }
    let tokens = surplus_required_tokens(take, ref_ray, s, now_ms.saturating_sub(a.start_ms))?;
    if tokens > max_tokens_in {
        return Err(SystemAuctionError::Slippage);
    }
    Ok((
        SurplusAuction {
            lot_left: left,
            live: left != 0,
            ..*a
        },
        tokens,
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn start_debt(
    acc: &Accounting,
    buffer: u128,
    token_configured: bool,
    mint_authority_ok: bool,
    token_ok: bool,
    ref_ray: u128,
    s: &SystemParams,
    now_ms: u64,
    live: bool,
) -> Result<DebtAuction, SystemAuctionError> {
    if live {
        return Err(SystemAuctionError::DebtAuctionAlreadyLive);
    }
    if !token_configured {
        return Err(SystemAuctionError::TokenNotConfigured);
    }
    if !token_ok {
        return Err(SystemAuctionError::ProtocolTokenMismatch);
    }
    if !mint_authority_ok {
        return Err(SystemAuctionError::MintAuthorityMissing);
    }
    if acc.unbacked_debt.saturating_sub(buffer) < s.debt_lot {
        return Err(SystemAuctionError::InsufficientBadDebt);
    }
    let start_amount = debt_start_amount(ref_ray, s)?;
    if start_amount == 0 {
        return Err(SystemAuctionError::MintCapConfig); // cannot mint usefully at this ref price
    }
    Ok(DebtAuction {
        lot_left: s.debt_lot,
        start_amount,
        start_ms: now_ms,
        live: true,
    })
}

/// The coin actually filled is `a.lot_left - new.lot_left` (<= requested `coin`); callers must burn that amount.
/// Taker pays `coin` stablecoin (burned, reduces unbacked debt); receives minted tokens pro rata, rounded down.
pub fn take_debt(
    a: &DebtAuction,
    coin: u128,
    min_tokens_out: u128,
    s: &SystemParams,
    now_ms: u64,
) -> Result<(DebtAuction, u128), SystemAuctionError> {
    if !a.live {
        return Err(SystemAuctionError::NotLive);
    }
    let take = coin.min(a.lot_left);
    if take == 0 {
        return Err(SystemAuctionError::ZeroFill);
    }
    let left = a.lot_left - take;
    if left != 0 && left < s.min_take {
        return Err(SystemAuctionError::DustRemainder);
    }
    let offered = debt_offered_tokens(a.start_amount, s, now_ms.saturating_sub(a.start_ms))?;
    let minted = mul_div_floor(offered, take, s.debt_lot)?;
    if minted == 0 {
        return Err(SystemAuctionError::ZeroFill);
    }
    if minted < min_tokens_out {
        return Err(SystemAuctionError::Slippage);
    }
    Ok((
        DebtAuction {
            lot_left: left,
            live: left != 0,
            ..*a
        },
        minted,
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolvencyDecision {
    Healthy,
    Freeze,
    RecoveryInProgress,
    NoAction,
}

/// spec §5.11 check_solvency. Callers pass `unbacked` AFTER settle_debt.
pub fn check_solvency(
    unbacked: u128,
    threshold: u128,
    token_configured: bool,
    freeze_on_bad_debt: bool,
    debt_auction_live: bool,
    start_check: Result<(), SystemAuctionError>,
) -> SolvencyDecision {
    use SystemAuctionError as E;
    if unbacked <= threshold {
        return SolvencyDecision::Healthy;
    }
    if !token_configured {
        return SolvencyDecision::Freeze;
    }
    if debt_auction_live {
        return SolvencyDecision::RecoveryInProgress;
    }
    if !freeze_on_bad_debt {
        return SolvencyDecision::NoAction;
    }
    match start_check {
        Err(E::MintCapConfig) | Err(E::MintAuthorityMissing) | Err(E::ProtocolTokenMismatch) => {
            SolvencyDecision::Freeze
        }
        Err(E::DebtAuctionAlreadyLive) => SolvencyDecision::RecoveryInProgress,
        _ => SolvencyDecision::NoAction,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::{Schedule, SchedulePoint};
    use crate::units::RAY;
    fn sched(end_bps: u16) -> Schedule {
        Schedule::new(&[
            SchedulePoint {
                elapsed_ms: 0,
                discount_bps: 0,
            },
            SchedulePoint {
                elapsed_ms: 1_000,
                discount_bps: end_bps,
            },
        ])
        .unwrap()
    }
    fn sp() -> SystemParams {
        SystemParams {
            surplus_buffer_target: 1_000,
            surplus_lot: 500,
            surplus_delay_ms: 0,
            surplus_premium_bps: 500,
            surplus_max_discount_bps: 2000,
            debt_lot: 500,
            debt_initial_discount_bps: 0,
            debt_growth: sched(3000),
            surplus_decay: sched(2500),
            max_mint_per_auction: 5_000,
            min_take: 10,
            system_ttl_ms: 14_400_000,
        }
    }
    const REF: u128 = 2 * RAY; // 2 coin per protocol token

    #[test]
    fn surplus_units() {
        // corrected units: lot / ref × (1 + premium − decay)
        assert_eq!(surplus_required_tokens(500, REF, &sp(), 0), Ok(263)); // ceil(250 × 1.05)
        assert_eq!(surplus_required_tokens(500, REF, &sp(), 1_000), Ok(200)); // floor of 0.80
    }
    #[test]
    fn debt_amounts_round_down_and_cap() {
        assert_eq!(debt_start_amount(REF, &sp()), Ok(250)); // floor(500 / 2)
        assert_eq!(debt_offered_tokens(250, &sp(), 1_000), Ok(325)); // floor(250 × 1.30)
        let tiny_ref = sp();
        assert_eq!(
            debt_start_amount(RAY / 100, &tiny_ref),
            Err(SystemAuctionError::MintCapConfig)
        ); // 50_000 > 5_000
    }
    #[test]
    fn mint_cap_holds_with_low_price() {
        let s = SystemParams {
            debt_growth: sched(3000),
            ..sp()
        };
        assert!(debt_offered_tokens(4_900, &s, 1_000).unwrap() <= s.max_mint_per_auction);
    }
    #[test]
    fn solvency_decisions() {
        use SolvencyDecision::*;
        assert_eq!(
            check_solvency(999, 1_000, false, true, false, Ok(())),
            Healthy
        ); // 1-unit style shortfall
        assert_eq!(
            check_solvency(1_001, 1_000, false, true, false, Ok(())),
            Freeze
        ); // no token
        assert_eq!(
            check_solvency(
                1_001,
                1_000,
                true,
                true,
                true,
                Err(SystemAuctionError::DebtAuctionAlreadyLive)
            ),
            RecoveryInProgress
        );
        assert_eq!(
            check_solvency(
                1_001,
                1_000,
                true,
                true,
                false,
                Err(SystemAuctionError::MintCapConfig)
            ),
            Freeze
        );
        assert_eq!(
            check_solvency(
                1_001,
                1_000,
                true,
                true,
                false,
                Err(SystemAuctionError::MintAuthorityMissing)
            ),
            Freeze
        );
        assert_eq!(
            check_solvency(
                1_001,
                1_000,
                true,
                true,
                false,
                Err(SystemAuctionError::ProtocolTokenMismatch)
            ),
            Freeze
        );
        assert_eq!(
            check_solvency(1_001, 1_000, true, true, false, Ok(())),
            NoAction
        ); // a keeper should start one
        assert_eq!(
            check_solvency(
                1_001,
                1_000,
                true,
                false,
                false,
                Err(SystemAuctionError::MintCapConfig)
            ),
            NoAction
        );
    }
    #[test]
    fn start_and_take_flows() {
        let acc = Accounting {
            unbacked_debt: 2_000,
            ..Accounting::default()
        };
        let d = start_debt(&acc, 0, true, true, true, REF, &sp(), 0, false).unwrap();
        let (d2, minted) = take_debt(&d, 250, 0, &sp(), 0).unwrap(); // half the lot at t=0 → 125 tokens
        assert_eq!((minted, d2.lot_left), (125, 250));
        assert_eq!(
            start_debt(&acc, 0, true, true, true, REF, &sp(), 0, true),
            Err(SystemAuctionError::DebtAuctionAlreadyLive)
        );
        let clean = Accounting::default();
        let s = start_surplus(&clean, 1_500, true, 0, 0, &sp(), false).unwrap();
        let (_, burn) = take_surplus(&s, 500, u128::MAX, REF, &sp(), 0).unwrap();
        assert_eq!(burn, 263);
        assert_eq!(
            start_surplus(&clean, 1_499, true, 0, 0, &sp(), false),
            Err(SystemAuctionError::BufferBelowTarget)
        );
        assert_eq!(
            start_surplus(&acc, 9_999, true, 0, 0, &sp(), false),
            Err(SystemAuctionError::UnbackedDebtPresent)
        );
    }
}

#[cfg(test)]
mod fix1_tests {
    use super::*;
    use crate::error::ConfigError;
    use crate::schedule::{Schedule, SchedulePoint};
    use crate::units::RAY;
    fn sched(end_bps: u16) -> Schedule {
        Schedule::new(&[
            SchedulePoint {
                elapsed_ms: 0,
                discount_bps: 0,
            },
            SchedulePoint {
                elapsed_ms: 1_000,
                discount_bps: end_bps,
            },
        ])
        .unwrap()
    }
    fn sp() -> SystemParams {
        SystemParams {
            surplus_buffer_target: 1_000,
            surplus_lot: 500,
            surplus_delay_ms: 0,
            surplus_premium_bps: 500,
            surplus_max_discount_bps: 2000,
            debt_lot: 500,
            debt_initial_discount_bps: 0,
            debt_growth: sched(3000),
            surplus_decay: sched(2500),
            max_mint_per_auction: 5_000,
            min_take: 10,
            system_ttl_ms: 14_400_000,
        }
    }
    const REF: u128 = 2 * RAY;

    #[test]
    fn validate_rules() {
        assert_eq!(sp().validate(), Ok(()));
        let bad = |s: SystemParams| s.validate();
        assert_eq!(
            bad(SystemParams {
                surplus_max_discount_bps: 3001,
                ..sp()
            }),
            Err(ConfigError::SystemDiscountAboveMax)
        );
        assert_eq!(
            bad(SystemParams {
                debt_initial_discount_bps: 3001,
                ..sp()
            }),
            Err(ConfigError::SystemDiscountAboveMax)
        );
        assert_eq!(
            bad(SystemParams {
                surplus_lot: 0,
                ..sp()
            }),
            Err(ConfigError::ZeroLot)
        );
        assert_eq!(
            bad(SystemParams {
                debt_lot: 0,
                ..sp()
            }),
            Err(ConfigError::ZeroLot)
        );
        assert_eq!(
            bad(SystemParams {
                min_take: 501,
                ..sp()
            }),
            Err(ConfigError::MinTakeAboveLot)
        );
        assert_eq!(
            bad(SystemParams {
                max_mint_per_auction: 0,
                ..sp()
            }),
            Err(ConfigError::MintCapZero)
        );
        assert_eq!(
            bad(SystemParams {
                max_mint_per_auction: MAX_MINT_HARD_CAP + 1,
                ..sp()
            }),
            Err(ConfigError::MintCapAboveHardCap)
        );
        assert_eq!(
            bad(SystemParams {
                system_ttl_ms: 0,
                ..sp()
            }),
            Err(ConfigError::ZeroTtl)
        );
    }

    #[test]
    fn close_surplus_rules() {
        let s = SurplusAuction {
            lot_left: 300,
            start_ms: 1_000,
            live: true,
        };
        let ttl = sp().system_ttl_ms;
        assert_eq!(
            close_surplus(&s, &sp(), 1_000 + ttl - 1),
            Err(SystemAuctionError::DelayNotElapsed)
        );
        let (c, back) = close_surplus(&s, &sp(), 1_000 + ttl).unwrap();
        assert_eq!((c.live, c.lot_left, back), (false, 0, 300));
        assert_eq!(
            close_surplus(&c, &sp(), 1_000 + ttl),
            Err(SystemAuctionError::NotLive)
        );
    }

    #[test]
    fn close_debt_rules() {
        let d = DebtAuction {
            lot_left: 200,
            start_amount: 250,
            start_ms: 0,
            live: true,
        };
        let ttl = sp().system_ttl_ms;
        assert_eq!(
            close_debt(&d, &sp(), ttl - 1),
            Err(SystemAuctionError::DelayNotElapsed)
        );
        let (c, released) = close_debt(&d, &sp(), ttl).unwrap();
        assert_eq!((c.live, c.lot_left, released), (false, 0, 200));
        assert_eq!(close_debt(&c, &sp(), ttl), Err(SystemAuctionError::NotLive));
    }

    #[test]
    fn take_debt_zero_mint_is_zero_fill() {
        // floor(250 * 1 / 500) = 0 tokens for 1 coin
        let d = DebtAuction {
            lot_left: 500,
            start_amount: 250,
            start_ms: 0,
            live: true,
        };
        assert_eq!(
            take_debt(&d, 1, 0, &sp(), 0),
            Err(SystemAuctionError::ZeroFill)
        );
    }

    #[test]
    fn start_debt_zero_start_amount_is_config_fault() {
        let acc = Accounting {
            unbacked_debt: 2_000,
            ..Accounting::default()
        };
        // floor(500 / 1000) = 0 tokens
        assert_eq!(
            start_debt(&acc, 0, true, true, true, 1_000 * RAY, &sp(), 0, false),
            Err(SystemAuctionError::MintCapConfig)
        );
    }

    #[test]
    fn multi_take_debt_over_time() {
        let acc = Accounting {
            unbacked_debt: 2_000,
            ..Accounting::default()
        };
        let d = start_debt(&acc, 0, true, true, true, REF, &sp(), 0, false).unwrap(); // start_amount 250
                                                                                      // t=0: offered 250, floor(250*200/500)=100
        let (d, m1) = take_debt(&d, 200, 0, &sp(), 0).unwrap();
        // t=500: growth 1500 bps, offered floor(250*1.15)=287, floor(287*200/500)=114
        let (d, m2) = take_debt(&d, 200, 0, &sp(), 500).unwrap();
        // t=1000: offered 325, floor(325*100/500)=65
        let (d, m3) = take_debt(&d, 100, 0, &sp(), 1_000).unwrap();
        assert_eq!((m1, m2, m3, d.lot_left, d.live), (100, 114, 65, 0, false));
        let total = m1 + m2 + m3; // 279
        assert!(total <= sp().max_mint_per_auction);
        assert!(total <= debt_offered_tokens(250, &sp(), 1_000).unwrap());
    }

    #[test]
    fn dust_slippage_notlive() {
        let acc = Accounting {
            unbacked_debt: 2_000,
            ..Accounting::default()
        };
        let s = SurplusAuction {
            lot_left: 500,
            start_ms: 0,
            live: true,
        };
        assert_eq!(
            take_surplus(&s, 495, u128::MAX, REF, &sp(), 0),
            Err(SystemAuctionError::DustRemainder)
        ); // left 5 < 10
        assert_eq!(
            take_surplus(&s, 500, 262, REF, &sp(), 0),
            Err(SystemAuctionError::Slippage)
        ); // needs 263
        let dead = SurplusAuction { live: false, ..s };
        assert_eq!(
            take_surplus(&dead, 500, u128::MAX, REF, &sp(), 0),
            Err(SystemAuctionError::NotLive)
        );
        let d = start_debt(&acc, 0, true, true, true, REF, &sp(), 0, false).unwrap();
        assert_eq!(
            take_debt(&d, 495, 0, &sp(), 0),
            Err(SystemAuctionError::DustRemainder)
        );
        assert_eq!(
            take_debt(&d, 250, 126, &sp(), 0),
            Err(SystemAuctionError::Slippage)
        ); // mints 125
        let dd = DebtAuction { live: false, ..d };
        assert_eq!(
            take_debt(&dd, 250, 0, &sp(), 0),
            Err(SystemAuctionError::NotLive)
        );
    }

    #[test]
    fn start_error_paths() {
        let clean = Accounting::default();
        let s = SystemParams {
            surplus_delay_ms: 100,
            ..sp()
        };
        assert_eq!(
            start_surplus(&clean, 1_500, true, 50, 100, &s, false),
            Err(SystemAuctionError::DelayNotElapsed)
        ); // 100 < 150
        assert_eq!(
            start_surplus(&clean, 1_500, false, 0, 0, &sp(), false),
            Err(SystemAuctionError::TokenNotConfigured)
        );
        let acc = Accounting {
            unbacked_debt: 2_000,
            ..Accounting::default()
        };
        assert_eq!(
            start_debt(&acc, 1_600, true, true, true, REF, &sp(), 0, false),
            Err(SystemAuctionError::InsufficientBadDebt)
        ); // 400 < 500
        assert_eq!(
            start_debt(&acc, 0, true, false, true, REF, &sp(), 0, false),
            Err(SystemAuctionError::MintAuthorityMissing)
        );
        assert_eq!(
            start_debt(&acc, 0, true, true, false, REF, &sp(), 0, false),
            Err(SystemAuctionError::ProtocolTokenMismatch)
        );
    }

    #[test]
    fn solvency_at_threshold_is_healthy() {
        assert_eq!(
            check_solvency(1_000, 1_000, true, true, false, Ok(())),
            SolvencyDecision::Healthy
        );
    }
}
