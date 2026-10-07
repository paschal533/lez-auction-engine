//! Account-level invariant checks shared by the invariant tests (Task 17). `check_all` returns the first
//! violated invariant as text so negative tests can prove the check itself catches a broken engine.

use super::*;
use auction_engine_core::state::{
    AccountingState, AuctionState, CollateralConfigState, EngineConfigState,
};
use lee_core::account::{AccountId, Data};
use stablecoin_core::Position;

macro_rules! ensure {
    ($cond:expr, $($arg:tt)*) => {
        if !($cond) {
            return Err(format!($($arg)*));
        }
    };
}

/// What the host held at the moment a position was handed to the engine (invariant 10 baseline).
pub struct Liquidated {
    pub pos: AccountId,
    pub host_collateral: u128,
    pub host_debt: u128,
}

#[derive(Default)]
pub struct Tracked {
    pub liquidated: Vec<Liquidated>,
}

/// Records the host side of `pos`, then liquidates it (nonce 0).
pub fn liquidate_tracked(w: &mut World, t: &mut Tracked, pos: AccountId) {
    let host_collateral = w.balance(host_vault(pos));
    let host_debt = w.read::<Position>(pos).normalized_debt_amount;
    w.liquidate(pos, 0).expect("liquidate");
    t.liquidated.push(Liquidated {
        pos,
        host_collateral,
        host_debt,
    });
}

pub fn check_all(w: &World, t: &Tracked) -> Result<(), String> {
    let cfg: EngineConfigState = w.read(config());
    let coll: CollateralConfigState = w.read(coll_config());
    let acc: AccountingState = w.read(accounting());
    let (mut live_reserved, mut live_penalty, mut live_debt_open) = (0u128, 0u128, 0u128);
    let mut live_count = 0usize;
    let (mut settled_principal, mut settled_count) = (0u128, 0usize);
    for l in &t.liquidated {
        let a: AuctionState = w.read(auction(l.pos, 0));
        let vault = w.balance(auction_vault(l.pos, 0));
        // Invariant 2: the vault holds at least the book (a donation may raise it, never lower it).
        let book = if a.settled {
            a.leftover_claimable - a.leftover_claimed
        } else {
            a.collateral_left
        };
        ensure!(vault >= book, "inv 2: vault {vault} < book {book}");
        // Collateral conservation inside the auction book.
        ensure!(
            a.collateral_seized == a.keeper_reward + a.collateral_sold + a.collateral_left,
            "inv 2: seized {} != reward {} + sold {} + left {}",
            a.collateral_seized,
            a.keeper_reward,
            a.collateral_sold,
            a.collateral_left
        );
        // Invariant 10, per position: the engine seized exactly what the host held and recorded exactly the
        // debt the host cleared (the host bounds only the MAXIMUM seizure, so the engine must match).
        let p: Position = w.read(l.pos);
        ensure!(
            (p.collateral_amount, p.normalized_debt_amount) == (0, 0),
            "inv 10 host position: ({}, {}) left on the host",
            p.collateral_amount,
            p.normalized_debt_amount
        );
        ensure!(
            a.collateral_seized == l.host_collateral,
            "inv 10 collateral: seized {} != host held {}",
            a.collateral_seized,
            l.host_collateral
        );
        ensure!(
            a.debt_principal == l.host_debt,
            "inv 10 debt: principal {} != host cleared {}",
            a.debt_principal,
            l.host_debt
        );
        if a.settled {
            settled_principal += a.debt_principal;
            settled_count += 1;
        } else {
            live_count += 1;
            live_reserved += a.reserved_amount;
            live_penalty += a.penalty_raised;
            live_debt_open += a.debt_principal - a.debt_raised;
        }
    }
    // Invariant 10, across the chain: every cleared debt unit was either burned by a bidder or booked as
    // shortfall once its auction settled (liquidate -> bid -> settle).
    if settled_count > 0 {
        ensure!(
            settled_principal == acc.total_debt_burned + acc.total_shortfall_booked,
            "inv 10 chain: settled principal {settled_principal} != burned {} + shortfall {}",
            acc.total_debt_burned,
            acc.total_shortfall_booked
        );
    }
    // Invariant 7: sum of live reservations == type_reserved == global_reserved (one type per instance).
    ensure!(live_reserved == coll.type_reserved, "inv 7: type reserved");
    // Invariant 7 (caps): only a single auction may exceed a cap on its own (liquidity of last resort).
    ensure!(
        live_count <= 1 || live_reserved <= coll.type_cap,
        "inv 7: {live_count} live auctions reserve {live_reserved} > type cap {}",
        coll.type_cap
    );
    ensure!(
        live_count <= 1 || cfg.global_reserved <= cfg.global_cap,
        "inv 7: {live_count} live auctions exceed the global cap"
    );
    ensure!(
        coll.type_reserved == cfg.global_reserved,
        "inv 7: global reserved"
    );
    // Invariant 9, exact: the buffer holds the penalties collected (settled and live), the surplus returned by
    // closed auctions (donations into a surplus vault included) and the donations made to the buffer itself,
    // minus the debt settled from it and the surplus lots moved out.
    let credits =
        acc.total_penalty_collected + live_penalty + acc.total_surplus_returned + w.donated_buffer;
    let debits = acc.total_debt_settled + acc.total_surplus_lots_out;
    let book_buffer = credits.checked_sub(debits);
    ensure!(
        book_buffer == Some(w.balance(buffer())),
        "inv 9: buffer {} != book {book_buffer:?}",
        w.balance(buffer())
    );
    // Invariant 11 (accumulator stays 1.0 in the harness, so nominal == normalized): supply is backed by
    // position debt, open auction debt, unbacked debt, or stablecoin the harness injected.
    let positions_debt: u128 = [position(k::OWNER, 0), position(k::OWNER2, 0)]
        .iter()
        .map(|p| {
            // A position the scenario never opened simply carries no debt.
            Position::try_from(&w.raw(*p).data).map_or(0, |pos| pos.normalized_debt_amount)
        })
        .sum();
    ensure!(
        w.supply(stable_def())
            <= positions_debt + live_debt_open + acc.unbacked_debt + w.injected_stable,
        "inv 11: supply {} unbacked",
        w.supply(stable_def())
    );
    Ok(())
}

pub fn assert_ok(r: Result<(), String>, label: &str) {
    if let Err(e) = r {
        panic!("{label}: {e}");
    }
}

/// Rewrites the decoded state of `account` (test-local tampering, never used on a happy path).
pub fn tamper<T>(w: &mut World, account: AccountId, f: impl FnOnce(&mut T))
where
    T: for<'a> TryFrom<&'a Data>,
    for<'a> Data: From<&'a T>,
{
    let mut raw = w.raw(account);
    let mut value: T = w.read(account);
    f(&mut value);
    raw.data = Data::from(&value);
    w.state.force_insert_account(account, raw);
}

/// The standard unsafe position, liquidated, then bid down (non-final), finished (final) and claimed.
pub fn full_chain() -> (World, Tracked, AccountId) {
    let mut w = World::with_engine();
    let mut t = Tracked::default();
    let pos = w.unsafe_position();
    liquidate_tracked(&mut w, &mut t, pos);
    assert_ok(check_all(&w, &t), "after liquidate");
    w.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 1_000_000);
    w.bid(pos, 100_000, 0).expect("partial bid");
    assert_ok(check_all(&w, &t), "after partial bid");
    w.bid_settle(pos, u128::MAX, 0).expect("final bid");
    assert!(w.read::<AuctionState>(auction(pos, 0)).settled);
    assert_ok(check_all(&w, &t), "after settlement");
    w.claim_leftover(k::OWNER, k::OWNER_COLL, pos)
        .expect("claim leftover");
    assert_ok(check_all(&w, &t), "after claim");
    (w, t, pos)
}
