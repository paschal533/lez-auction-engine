//! S4/S6 cycle bench. One guest, many operations: the host sends `[op, reps, ...]` and the guest runs
//! operation `op` `reps` times with a per-rep varied input (so nothing is hoisted), then commits a
//! checksum. Marginal cost per operation = (cycles(reps=R) - cycles(reps=1)) / (R - 1).
#![no_main]
use auction_core::accounting::Accounting;
use auction_core::auction::{apply_bid, quote_bid, Auction, BidFill, BidRequest};
use auction_core::liquidation::{quote_liquidation, CapState, CollateralParams, LiquidationInput};
use auction_core::math::mul_div_floor;
use auction_core::schedule::{rai_default, Schedule, SchedulePoint};
use auction_core::system::{take_debt, DebtAuction, SystemParams};
use auction_core::units::RAY;
use borsh::{BorshDeserialize, BorshSerialize};
use core::hint::black_box;
use risc0_zkvm::guest::env;
use nssa_core::account::AccountWithMetadata;
use nssa_core::program::AccountPostState;

risc0_zkvm::guest::entry!(main);

const OP_NOOP: u128 = 0;
const OP_MUL_DIV_WORST: u128 = 1;
const OP_MUL_DIV_SMALL: u128 = 2;
const OP_DISCOUNT_2: u128 = 3;
const OP_DISCOUNT_8: u128 = 4;
const OP_QUOTE_LIQ: u128 = 5;
const OP_BID: u128 = 6;
const OP_TAKE_DEBT: u128 = 7;
const OP_EXP_DISCOUNT: u128 = 8;
const OP_SCHEDULE_NEW_8: u128 = 9;
const OP_ACC_SERIALISE: u128 = 10;
const OP_ACC_RMW: u128 = 11;
const OP_ACCOUNT_IN: u128 = 20;
const OP_ACCOUNT_IN_OUT: u128 = 21;

/// Borsh mirror of `auction_core::accounting::Accounting` (all fields). auction_core has no serialiser dependency.
#[derive(BorshSerialize, BorshDeserialize)]
struct AccountingWire {
    unbacked_debt: u128,
    total_shortfall_booked: u128,
    total_penalty_collected: u128,
    total_debt_burned: u128,
    total_debt_settled: u128,
    surplus_nonce: u64,
    debt_nonce: u64,
    host_frozen_by_engine: bool,
    debt_on_auction: u128,
    total_debt_auctioned: u128,
}

impl From<Accounting> for AccountingWire {
    fn from(a: Accounting) -> Self {
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
        }
    }
}

impl From<AccountingWire> for Accounting {
    fn from(w: AccountingWire) -> Self {
        Self {
            unbacked_debt: w.unbacked_debt,
            total_shortfall_booked: w.total_shortfall_booked,
            total_penalty_collected: w.total_penalty_collected,
            total_debt_burned: w.total_debt_burned,
            total_debt_settled: w.total_debt_settled,
            surplus_nonce: w.surplus_nonce,
            debt_nonce: w.debt_nonce,
            host_frozen_by_engine: w.host_frozen_by_engine,
            debt_on_auction: w.debt_on_auction,
            total_debt_auctioned: w.total_debt_auctioned,
        }
    }
}

fn pt(elapsed_ms: u64, discount_bps: u16) -> SchedulePoint {
    SchedulePoint {
        elapsed_ms,
        discount_bps,
    }
}

fn schedule_8() -> [SchedulePoint; 8] {
    [
        pt(0, 100),
        pt(300_000, 250),
        pt(600_000, 400),
        pt(900_000, 600),
        pt(1_200_000, 800),
        pt(1_800_000, 1_000),
        pt(2_400_000, 1_400),
        pt(3_600_000, 2_000),
    ]
}

/// RAY * (1 - r)^t by binary exponentiation in RAY fixed point (the S4 exponential variant); returns the discount.
fn exp_discount(r_ray: u128, t_s: u32) -> u128 {
    let mut result = RAY;
    let mut base = RAY - r_ray;
    let mut e = t_s;
    while e > 0 {
        if e & 1 == 1 {
            result = mul_div_floor(result, base, RAY).unwrap_or(0);
        }
        e >>= 1;
        if e > 0 {
            base = mul_div_floor(base, base, RAY).unwrap_or(0);
        }
    }
    RAY - result
}

fn collateral_params() -> CollateralParams {
    CollateralParams {
        penalty_bps: 1000,
        tip: 5_000_000,
        chip_bps: 10,
        reward_cap: 50_000_000,
        debt_floor: 100_000_000,
        min_bid: 1_000_000,
        schedule: rai_default(),
    }
}

fn system_params() -> SystemParams {
    SystemParams {
        surplus_buffer_target: 1_000_000,
        surplus_lot: 100_000,
        surplus_delay_ms: 60_000,
        surplus_premium_bps: 0,
        surplus_max_discount_bps: 1_000,
        debt_lot: 1_000_000,
        debt_initial_discount_bps: 500,
        debt_growth: rai_default(),
        surplus_decay: rai_default(),
        max_mint_per_auction: 1_000_000_000_000,
        min_take: 1_000,
        system_ttl_ms: 3_600_000,
    }
}

const D: u128 = 10_000_000_000;
const P: u128 = 752 * RAY / 1_000_000;

fn liquidation_input(i: u128) -> LiquidationInput {
    LiquidationInput {
        position_collateral: 11_000_000,
        vault_balance: 12_000_000,
        nominal_debt: D + i,
        price_ray: P + i,
    }
}

fn caps() -> CapState {
    CapState {
        type_cap: u128::MAX / 4,
        type_reserved: 0,
        global_cap: u128::MAX / 4,
        global_reserved: 0,
    }
}

fn main() {
    let v: Vec<u128> = env::read();
    let accounts: Vec<AccountWithMetadata> = env::read();
    let (op, reps) = (v[0], v[1] as u32);
    let mut acc: u128 = 0;

    // Fixtures are built outside the repeated section; their cost sits in the reps=1 baseline, not the marginal.
    let sched2 = rai_default();
    let sched8 = Schedule::new(&schedule_8()).expect("valid");
    let params = collateral_params();
    let quote = quote_liquidation(&liquidation_input(0), &params, &caps()).expect("quote");
    let auction = Auction::open(&quote, 0, rai_default(), 0);
    let chost = params.chost().expect("chost");
    let sys = system_params();
    let debt_auction = DebtAuction {
        lot_left: sys.debt_lot,
        start_amount: 500_000_000,
        start_ms: 0,
        live: true,
    };
    let accounting = Accounting {
        unbacked_debt: 5_000_000,
        total_shortfall_booked: 4_000_000,
        total_penalty_collected: 3_000_000,
        total_debt_burned: 2_000_000,
        total_debt_settled: 1_000_000,
        surplus_nonce: 7,
        debt_nonce: 9,
        host_frozen_by_engine: false,
        debt_on_auction: 100_000,
        total_debt_auctioned: 50_000,
    };
    let stored = borsh::to_vec(&AccountingWire::from(accounting)).expect("ser");
    let fill = BidFill {
        charged: 3_000_000_000,
        collateral_out: 2_500_000,
        debt_part: 3_000_000_000,
        penalty_part: 0,
        discount_ray: 0,
        ends_auction: false,
    };

    for i in 0..reps {
        let i = black_box(i as u128);
        match op {
            OP_NOOP => acc = acc.wrapping_add(i),
            OP_MUL_DIV_WORST => {
                // hi != 0 and a full 128-bit divisor: the widest path of mul_div_floor.
                let r = mul_div_floor(black_box(u128::MAX - i), black_box(u128::MAX - 1), u128::MAX);
                acc = acc.wrapping_add(r.expect("fits"));
            }
            OP_MUL_DIV_SMALL => {
                let r = mul_div_floor(black_box(1_000_000 + i), black_box(RAY), black_box(RAY));
                acc = acc.wrapping_add(r.expect("fits"));
            }
            OP_DISCOUNT_2 => {
                let t = black_box(1_350_000 + i as u64);
                acc = acc.wrapping_add(black_box(&sched2).discount_ray(t));
            }
            OP_DISCOUNT_8 => {
                // last segment: scans every window.
                let t = black_box(3_000_000 + i as u64);
                acc = acc.wrapping_add(black_box(&sched8).discount_ray(t));
            }
            OP_QUOTE_LIQ => {
                let q = quote_liquidation(&liquidation_input(i), &params, &caps()).expect("quote");
                acc = acc.wrapping_add(black_box(q).keeper_reward);
            }
            OP_BID => {
                let req = BidRequest {
                    coin_in_max: 3_000_000_000 + i,
                    min_collateral_out: 0,
                };
                let t = 1_350_000 + i as u64;
                let f = quote_bid(&auction, &req, P, t, params.min_bid, chost).expect("bid");
                let (next, end) = apply_bid(&auction, &f);
                assert!(end.is_none());
                acc = acc.wrapping_add(black_box(next).collateral_left);
            }
            OP_TAKE_DEBT => {
                let now = 1_350_000 + i as u64;
                let (a2, minted) = take_debt(&debt_auction, 400_000 + i, 0, &sys, now).expect("take");
                acc = acc.wrapping_add(minted).wrapping_add(black_box(a2).lot_left);
            }
            OP_EXP_DISCOUNT => {
                // v[2] = t in seconds, v[3] = r in RAY per second (varied by i so the loop is not hoisted).
                acc = acc.wrapping_add(exp_discount(black_box(v[3] + i), black_box(v[2] as u32)));
            }
            OP_SCHEDULE_NEW_8 => {
                let s = Schedule::new(black_box(&schedule_8())).expect("valid");
                acc = acc.wrapping_add(black_box(s).max_discount_ray());
            }
            OP_ACC_SERIALISE => {
                let mut a = accounting;
                a.unbacked_debt += i;
                let bytes = borsh::to_vec(&AccountingWire::from(black_box(a))).expect("ser");
                acc = acc.wrapping_add(black_box(bytes).len() as u128);
            }
            OP_ACC_RMW => {
                // The per-bid Accounting update: decode the stored bytes, apply the fill, encode the new bytes.
                let old: Accounting = borsh::from_slice::<AccountingWire>(black_box(&stored))
                    .expect("de")
                    .into();
                let new = old.record_bid(&fill);
                let out = borsh::to_vec(&AccountingWire::from(black_box(new))).expect("ser");
                acc = acc.wrapping_add(black_box(out).len() as u128 + i);
            }
            OP_ACCOUNT_IN | OP_ACCOUNT_IN_OUT => {} // the cost is the env::read / env::commit of `accounts` below
            _ => panic!("unknown op"),
        }
    }
    black_box(&accounts);
    env::commit(&acc);
    if op == OP_ACCOUNT_IN_OUT {
        // The shape of a program output: pre-states echoed, post-states committed.
        env::commit(&accounts);
        let posts: Vec<AccountPostState> = accounts
            .iter()
            .map(|a| AccountPostState::new(a.account.clone()))
            .collect();
        env::commit(&posts);
    }
}
