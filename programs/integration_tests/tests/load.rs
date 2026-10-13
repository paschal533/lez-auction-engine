//! P2 / R3 load test (spec §11): N concurrent auctions with interleaved partial bids. With no on-chain
//! list (ADR-008) and per-auction PDAs, a bid's cost should not depend on how many auctions are live.
//! Two assertions, nothing more is claimed:
//! - a first-round bid with 6 live auctions costs within 5 % of the same bid with exactly 1 live auction;
//! - the 18 bids (6 auctions, 3 rounds) are uniform, within 5 % of each other.
//!
//! Every auction also settles with the reservations released.

mod common;

use auction_engine_core::{state::CollateralConfigState, Instruction as Ix};
use common::*;
use lee_core::account::AccountId;

const N: u64 = 6;
const PARTIAL: u128 = 30_000;
const ROUNDS: usize = 3;
/// Target of a 100_000 / 120_000 position with a 10 % penalty.
const TARGET: u128 = 132_000;

/// A world with `count` liquidated positions (live auctions) and a funded bidder.
fn world_with_live_auctions(count: u64) -> (World, Vec<AccountId>) {
    let mut w = World::with_engine();
    let positions: Vec<_> = (0..count)
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
    (w, positions)
}

/// One partial bid on `position`'s auction; returns its total executor cycles.
fn partial_bid_cycles(w: &mut World, position: AccountId) -> u64 {
    let accounts = w.bid_accounts(position, 0, k::BIDDER_STABLE, k::BIDDER_COLL, false);
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
    m.total
}

#[test]
fn load_concurrent_auctions_with_interleaved_bids_stay_isolated() {
    let (mut reference_world, reference_positions) = world_with_live_auctions(1);
    let reference = partial_bid_cycles(&mut reference_world, reference_positions[0]);

    let (mut w, positions) = world_with_live_auctions(N);
    let mut cycles = Vec::new();
    for _ in 0..ROUNDS {
        for p in &positions {
            cycles.push(partial_bid_cycles(&mut w, *p));
        }
    }
    let first_round = &cycles[..positions.len()];
    let (f_min, f_max) = (
        *first_round.iter().min().expect("bids"),
        *first_round.iter().max().expect("bids"),
    );
    let (min, max) = (
        *cycles.iter().min().expect("bids"),
        *cycles.iter().max().expect("bids"),
    );
    println!(
        "LOAD reference_1_live_auction_cycles={reference} first_round_N={N} min_cycles={f_min} max_cycles={f_max}"
    );
    println!(
        "LOAD N={N} bids={} min_cycles={min} max_cycles={max} spread_percent={:.2}",
        cycles.len(),
        100.0 * (max - min) as f64 / min as f64
    );
    for c in first_round {
        assert!(
            c * 100 <= reference * 105 && reference * 100 <= c * 105,
            "a first-round bid with {N} live auctions ({c} cycles) must stay within 5 % of the bid with 1 live auction ({reference} cycles)"
        );
    }
    assert!(
        max * 100 <= min * 105,
        "bids must be uniform across auctions and rounds: {min}..{max}"
    );
    for p in &positions {
        w.bid_settle(*p, TARGET, 0).expect("final bid");
    }
    let coll: CollateralConfigState = w.read(coll_config());
    assert_eq!(coll.type_reserved, 0, "every reservation released once");
}
