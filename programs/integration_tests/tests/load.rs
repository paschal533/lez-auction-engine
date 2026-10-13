//! P2 / R3 load test (spec §11): N concurrent auctions with interleaved partial bids. With no on-chain
//! list (ADR-008) and per-auction PDAs, a bid's cost must not depend on how many auctions are live,
//! and every auction settles with the reservations released.

mod common;

use auction_engine_core::{state::CollateralConfigState, Instruction as Ix};
use common::*;

const N: u64 = 6;
const PARTIAL: u128 = 30_000;
const ROUNDS: usize = 3;
/// Target of a 100_000 / 120_000 position with a 10 % penalty.
const TARGET: u128 = 132_000;

#[test]
fn load_concurrent_auctions_with_interleaved_bids_stay_isolated() {
    let mut w = World::with_engine();
    let positions: Vec<_> = (0..N)
        .map(|n| {
            w.open_position(
                k::OWNER,
                k::OWNER_COLL,
                k::OWNER_STABLE,
                n,
                100_000,
                120_000,
            )
        })
        .collect();
    w.set_min_cr(2 * ONE);
    for p in &positions {
        w.liquidate(*p, 0).expect("liquidate");
    }
    w.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 10_000_000);
    let mut cycles = Vec::new();
    for _ in 0..ROUNDS {
        for p in &positions {
            let accounts = w.bid_accounts(*p, 0, k::BIDDER_STABLE, k::BIDDER_COLL, false);
            let (m, r) = w.measured_full(
                "load_bid",
                accounts,
                &[k::BIDDER_STABLE],
                Ix::Bid {
                    coin_in_max: PARTIAL,
                    min_collateral_out: 0,
                },
            );
            r.expect("partial bid");
            cycles.push(m.total);
        }
    }
    let (min, max) = (
        *cycles.iter().min().expect("bids"),
        *cycles.iter().max().expect("bids"),
    );
    println!(
        "LOAD N={N} bids={} min_cycles={min} max_cycles={max} spread_percent={:.2}",
        cycles.len(),
        100.0 * (max - min) as f64 / min as f64
    );
    assert!(
        max * 100 <= min * 105,
        "a bid's cost must not grow with the number of live auctions: {min}..{max}"
    );
    for p in &positions {
        w.bid_settle(*p, TARGET, 0).expect("final bid");
    }
    let coll: CollateralConfigState = w.read(coll_config());
    assert_eq!(coll.type_reserved, 0, "every reservation released once");
}
