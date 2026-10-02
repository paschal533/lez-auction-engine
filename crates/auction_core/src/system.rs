//! F4 debt auction and F5 surplus auction: instant Dutch auctions, plus the solvency decision (spec §5.11, ADR-017).
#![allow(clippy::arithmetic_side_effects)] // ordered subtractions noted inline
use crate::accounting::Accounting;
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
    let floor = BPS - s.surplus_max_discount_bps as u128;
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
    let amount = mul_div_floor(tokens, BPS - s.debt_initial_discount_bps as u128, BPS)?;
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
    Ok(DebtAuction {
        lot_left: s.debt_lot,
        start_amount,
        start_ms: now_ms,
        live: true,
    })
}

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
