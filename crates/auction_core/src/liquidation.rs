//! Full-position liquidation quote (spec §5.6, §5.9, ADR-012). Reads only pre-state.
#![allow(clippy::arithmetic_side_effects)] // seized - reward_coll: reward_coll ≤ seized by min()

use crate::error::{AuctionError, ConfigError};
use crate::math::{mul_div_ceil, mul_div_floor, MathError};
use crate::schedule::Schedule;
use crate::units::{BPS, RAY};

pub const MAX_PENALTY_BPS: u16 = 2500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollateralParams {
    pub penalty_bps: u16,
    pub tip: u128,
    pub chip_bps: u16,
    pub reward_cap: u128,
    pub debt_floor: u128,
    pub min_bid: u128,
    pub schedule: Schedule,
}

impl CollateralParams {
    /// chost = ceil(debt_floor · (1e4 + penalty) / 1e4)
    pub fn chost(&self) -> Result<u128, MathError> {
        mul_div_ceil(self.debt_floor, BPS + self.penalty_bps as u128, BPS)
    }
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.penalty_bps > MAX_PENALTY_BPS {
            return Err(ConfigError::PenaltyAboveMax);
        }
        let chost = self.chost().map_err(|_| ConfigError::ChostBelowMinBid)?;
        if chost < self.min_bid {
            return Err(ConfigError::ChostBelowMinBid);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapState {
    pub type_cap: u128,
    pub type_reserved: u128,
    pub global_cap: u128,
    pub global_reserved: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiquidationInput {
    pub position_collateral: u128,
    pub vault_balance: u128,
    pub nominal_debt: u128,
    pub price_ray: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiquidationQuote {
    pub debt_principal: u128,
    pub amount_to_raise: u128,
    pub reserve: u128,
    pub keeper_reward: u128,
    pub lot: u128,
    pub seized: u128,
}

pub fn quote_liquidation(
    i: &LiquidationInput,
    p: &CollateralParams,
    caps: &CapState,
) -> Result<LiquidationQuote, AuctionError> {
    if i.nominal_debt == 0 {
        return Err(AuctionError::ZeroDebt);
    }
    if i.vault_balance < i.position_collateral {
        return Err(AuctionError::VaultBelowCollateral);
    }
    let d = i.nominal_debt;
    let amount_to_raise = mul_div_ceil(d, BPS + p.penalty_bps as u128, BPS)?;
    let fits = amount_to_raise <= caps.type_cap.saturating_sub(caps.type_reserved)
        && amount_to_raise <= caps.global_cap.saturating_sub(caps.global_reserved);
    let alone = caps.type_reserved == 0 && caps.global_reserved == 0;
    if !(fits || alone) {
        return Err(AuctionError::CapacityExhausted);
    }

    let seized = i.vault_balance; // sweep everything (≥, not =)
    let reward_coin = if d < p.debt_floor {
        0
    } else {
        let chip = mul_div_floor(d, p.chip_bps as u128, BPS)?;
        let penalty = mul_div_floor(d, p.penalty_bps as u128, BPS)?;
        p.tip.saturating_add(chip).min(p.reward_cap).min(penalty)
    };
    let reward_coll = mul_div_floor(reward_coin, i.price_ray, RAY)?.min(seized);
    Ok(LiquidationQuote {
        debt_principal: d,
        amount_to_raise,
        reserve: amount_to_raise,
        keeper_reward: reward_coll,
        lot: seized - reward_coll,
        seized,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::rai_default;
    use crate::units::RAY;

    const P: u128 = 752 * RAY / 1_000_000; // 1 LSC = 0.000752 wTKA; amounts in whole units for readability
    fn params() -> CollateralParams {
        CollateralParams {
            penalty_bps: 1000,
            tip: 5,
            chip_bps: 10,
            reward_cap: 50,
            debt_floor: 100,
            min_bid: 25,
            schedule: rai_default(),
        }
    }
    fn caps() -> CapState {
        CapState {
            type_cap: 60_000,
            type_reserved: 0,
            global_cap: 250_000,
            global_reserved: 0,
        }
    }

    #[test]
    fn nominal_quote() {
        // c = 125_000 base units, D = 15_900
        let i = LiquidationInput {
            position_collateral: 125_000,
            vault_balance: 125_000,
            nominal_debt: 15_900,
            price_ray: P,
        };
        let q = quote_liquidation(&i, &params(), &caps()).unwrap();
        assert_eq!(q.amount_to_raise, 17_490); // ceil(15_900 × 1.1)
        assert_eq!(q.reserve, 17_490);
        // reward_coin = min(5 + 15.9→15, 50, 1590) = 20 ; reward_coll = floor(20 × 0.000752) = 0
        assert_eq!(q.keeper_reward, 0);
        assert_eq!(q.lot, 125_000);
        assert_eq!(params().chost().unwrap(), 110);
    }
    #[test]
    fn donated_vault_is_swept() {
        let i = LiquidationInput {
            position_collateral: 1_000,
            vault_balance: 1_001,
            nominal_debt: 900,
            price_ray: P,
        };
        assert_eq!(
            quote_liquidation(&i, &params(), &caps()).unwrap().seized,
            1_001
        );
        let bad = LiquidationInput {
            vault_balance: 999,
            ..i
        };
        assert_eq!(
            quote_liquidation(&bad, &params(), &caps()),
            Err(AuctionError::VaultBelowCollateral)
        );
    }
    #[test]
    fn debt_below_floor_pays_no_reward_but_liquidates() {
        // Review Focus #4
        let i = LiquidationInput {
            position_collateral: 10,
            vault_balance: 10,
            nominal_debt: 99,
            price_ray: RAY,
        };
        let q = quote_liquidation(&i, &params(), &caps()).unwrap();
        assert_eq!(q.keeper_reward, 0);
        assert_eq!(q.amount_to_raise, 109);
    }
    #[test]
    fn reward_capped_at_lot() {
        // price 1 collateral per coin, tiny collateral: the reward in collateral would exceed c
        let i = LiquidationInput {
            position_collateral: 3,
            vault_balance: 3,
            nominal_debt: 10_000,
            price_ray: RAY,
        };
        let q = quote_liquidation(&i, &params(), &caps()).unwrap();
        assert_eq!(q.keeper_reward, 3);
        assert_eq!(q.lot, 0);
    }
    #[test]
    fn caps_and_oversized_alone() {
        let i = LiquidationInput {
            position_collateral: 1,
            vault_balance: 1,
            nominal_debt: 100_000,
            price_ray: P,
        };
        assert!(quote_liquidation(&i, &params(), &caps()).is_ok()); // oversized but alone
        let busy = CapState {
            type_reserved: 1,
            ..caps()
        };
        assert_eq!(
            quote_liquidation(&i, &params(), &busy),
            Err(AuctionError::CapacityExhausted)
        );
        let small = LiquidationInput {
            nominal_debt: 1_000,
            ..i
        };
        let nearly_full = CapState {
            type_reserved: 59_000,
            ..caps()
        };
        assert_eq!(
            quote_liquidation(&small, &params(), &nearly_full),
            Err(AuctionError::CapacityExhausted)
        ); // 1_100 > 1_000 room
    }
    #[test]
    fn zero_debt_rejected() {
        let i = LiquidationInput {
            position_collateral: 1,
            vault_balance: 1,
            nominal_debt: 0,
            price_ray: P,
        };
        assert_eq!(
            quote_liquidation(&i, &params(), &caps()),
            Err(AuctionError::ZeroDebt)
        );
    }
    #[test]
    fn params_validation() {
        assert_eq!(
            CollateralParams {
                penalty_bps: 2501,
                ..params()
            }
            .validate(),
            Err(ConfigError::PenaltyAboveMax)
        );
        assert_eq!(
            CollateralParams {
                min_bid: 200,
                ..params()
            }
            .validate(),
            Err(ConfigError::ChostBelowMinBid)
        );
    }
    #[test]
    fn reward_cap_binds() {
        // d=3000, tip=30, chip_bps=300: chip=floor(3000×300/10000)=90
        // penalty=floor(3000×2000/10000)=600
        // reward_coin = min(30+90, 50, 600) = min(120, 50, 600) = 50 (reward_cap binds)
        // At price_ray=RAY: reward_coll=floor(50×RAY/RAY)=50
        let i = LiquidationInput {
            position_collateral: 1_000,
            vault_balance: 1_000,
            nominal_debt: 3_000,
            price_ray: RAY,
        };
        let p = CollateralParams {
            penalty_bps: 2000,
            tip: 30,
            chip_bps: 300,
            reward_cap: 50,
            ..params()
        };
        let q = quote_liquidation(&i, &p, &caps()).unwrap();
        assert_eq!(q.keeper_reward, 50);
        assert_eq!(q.lot, 950);
    }
    #[test]
    fn penalty_binds_reward() {
        // d=1000, tip=5, chip_bps=3000: chip=floor(1000×3000/10000)=300
        // penalty=floor(1000×100/10000)=10
        // reward_coin = min(5+300, 100, 10) = min(305, 100, 10) = 10 (penalty binds)
        // At price_ray=RAY: reward_coll=floor(10×RAY/RAY)=10
        let i = LiquidationInput {
            position_collateral: 100,
            vault_balance: 100,
            nominal_debt: 1_000,
            price_ray: RAY,
        };
        let p = CollateralParams {
            penalty_bps: 100,
            tip: 5,
            chip_bps: 3000,
            reward_cap: 100,
            ..params()
        };
        let q = quote_liquidation(&i, &p, &caps()).unwrap();
        assert_eq!(q.keeper_reward, 10);
        assert_eq!(q.lot, 90);
    }
    #[test]
    fn reward_floors_to_positive_value() {
        // d=10000, tip=1, chip_bps=100: chip=floor(10000×100/10000)=100
        // penalty=floor(10000×1000/10000)=1000
        // reward_coin = min(1+100, 100, 1000) = min(101, 100, 1000) = 100
        // At price_ray=3×RAY/7≈0.4286: reward_coll=floor(100×(3×RAY/7)/RAY)=floor(100×3/7)=floor(42.857)=42
        let price_ray_num = 3u128 * RAY / 7;
        let i = LiquidationInput {
            position_collateral: 500,
            vault_balance: 500,
            nominal_debt: 10_000,
            price_ray: price_ray_num,
        };
        let p = CollateralParams {
            penalty_bps: 1000,
            tip: 1,
            chip_bps: 100,
            reward_cap: 100,
            ..params()
        };
        let q = quote_liquidation(&i, &p, &caps()).unwrap();
        assert_eq!(q.keeper_reward, 42);
        assert_eq!(q.lot, 458);
    }
    #[test]
    fn global_reservation_blocks_alone() {
        // Setup: type room ample, global_reserved=1 blocks even though type_reserved=0
        // amount_to_raise=100000 exceeds global room (100000 - 1 = 99999) but fits type
        // nomical_debt such that ceil(D×11000/10000)=100000 requires D>=90909
        let i = LiquidationInput {
            position_collateral: 100,
            vault_balance: 100,
            nominal_debt: 90_909,
            price_ray: P,
        };
        let caps = CapState {
            type_cap: 100_000,
            type_reserved: 0,
            global_cap: 100_000,
            global_reserved: 1,
        };
        assert_eq!(
            quote_liquidation(&i, &params(), &caps),
            Err(AuctionError::CapacityExhausted)
        );
    }
    #[test]
    fn global_room_exhausted() {
        // Type room ample (90000 > 30000), global room smaller than amount_to_raise
        // amount_to_raise=30000 requires D>=27272 such that ceil(D×11000/10000)=30000
        let i = LiquidationInput {
            position_collateral: 100,
            vault_balance: 100,
            nominal_debt: 27_272,
            price_ray: P,
        };
        let caps = CapState {
            type_cap: 100_000,
            type_reserved: 10_000,
            global_cap: 50_000,
            global_reserved: 30_000,
        };
        assert_eq!(
            quote_liquidation(&i, &params(), &caps),
            Err(AuctionError::CapacityExhausted)
        );
    }
}
