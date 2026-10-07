//! Spec section 5.13 invariants that need accounts (Plan 1 left them to Plan 2): 2, 7, 9, 10, 11, 12, 17 and R3.
//! `check_all` returns the first violated invariant as `Err(text)` so negative tests can show that the
//! check itself catches a broken engine (invariant 10, ADR-036).

mod common;

use auction_engine_core::{
    args::{CollateralArgs, EngineParamsArgs},
    state::{
        AccountingState, AuctionState, CollateralConfigState, EngineConfigState,
        SurplusAuctionState,
    },
    Instruction as Ix,
};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID as CLOCK;
use common::*;
use lee_core::account::{Account, AccountId, Data};
use stablecoin_core::Position;

macro_rules! ensure {
    ($cond:expr, $($arg:tt)*) => {
        if !($cond) {
            return Err(format!($($arg)*));
        }
    };
}

/// What the host held at the moment a position was handed to the engine (invariant 10 baseline).
struct Liquidated {
    pos: AccountId,
    host_collateral: u128,
    host_debt: u128,
}

#[derive(Default)]
struct Tracked {
    liquidated: Vec<Liquidated>,
}

/// Records the host side of `pos`, then liquidates it (nonce 0).
fn liquidate_tracked(w: &mut World, t: &mut Tracked, pos: AccountId) {
    let host_collateral = w.balance(host_vault(pos));
    let host_debt = w.read::<Position>(pos).normalized_debt_amount;
    w.liquidate(pos, 0).expect("liquidate");
    t.liquidated.push(Liquidated {
        pos,
        host_collateral,
        host_debt,
    });
}

fn check_all(w: &World, t: &Tracked) -> Result<(), String> {
    let cfg: EngineConfigState = w.read(config());
    let coll: CollateralConfigState = w.read(coll_config());
    let acc: AccountingState = w.read(accounting());
    let (mut live_reserved, mut live_penalty, mut live_debt_open) = (0u128, 0u128, 0u128);
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
    ensure!(
        coll.type_reserved == cfg.global_reserved,
        "inv 7: global reserved"
    );
    // Invariant 9: the actual buffer covers the book (penalties + returned surplus - settled debt - lots out).
    // Signed: donations into the buffer are not in the book, so the book itself may be negative.
    let credits = acc.total_penalty_collected + live_penalty + acc.total_surplus_returned;
    let debits = acc.total_debt_settled + acc.total_surplus_lots_out;
    let book_buffer = i128::try_from(credits).unwrap() - i128::try_from(debits).unwrap();
    let buffer_balance = i128::try_from(w.balance(buffer())).unwrap();
    ensure!(
        buffer_balance >= book_buffer,
        "inv 9: buffer {buffer_balance} < book {book_buffer}"
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

fn assert_ok(r: Result<(), String>, label: &str) {
    if let Err(e) = r {
        panic!("{label}: {e}");
    }
}

fn lcg(seed: &mut u64) -> u64 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *seed >> 33
}

/// Rewrites the decoded state of `account` (test-local tampering, never used on a happy path).
fn tamper<T>(w: &mut World, account: AccountId, f: impl FnOnce(&mut T))
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
fn full_chain() -> (World, Tracked, AccountId) {
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

#[test]
fn collateral_invariants_hold_over_random_bids() {
    for mut seed in [1u64, 7, 42] {
        let mut w = World::with_system(false);
        let big = w.open_position(
            k::OWNER,
            k::OWNER_COLL,
            k::OWNER_STABLE,
            0,
            LIQ_COLLATERAL,
            LIQ_DEBT,
        );
        let small = w.open_small_position();
        w.set_min_cr(2 * ONE);
        let mut t = Tracked::default();
        for pos in [big, small] {
            liquidate_tracked(&mut w, &mut t, pos);
            assert_ok(check_all(&w, &t), "after liquidate");
        }
        let auctions: Vec<AccountId> = t.liquidated.iter().map(|l| l.pos).collect();
        w.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 10_000_000);
        let mut ok_bids = 0u32;
        for step in 0..12 {
            w.advance(lcg(&mut seed) % 600_000);
            w.set_oracle(R, w.now);
            let pos = auctions[(lcg(&mut seed) % 2) as usize];
            let coin = 25 + u128::from(lcg(&mut seed)) % 200_000;
            // Dust/min-bid/not-auctioning rejections are fine here; count the bids that landed.
            if w.bid_settle(pos, coin, 0).is_ok() {
                ok_bids += 1;
            }
            assert_ok(check_all(&w, &t), &format!("seed {seed} step {step}"));
        }
        assert!(
            ok_bids > 0,
            "seed {seed}: no random bid succeeded, the test exercised nothing"
        );
        for pos in &auctions {
            let _ = w.bid_settle(*pos, u128::MAX, 0);
        }
        for pos in &auctions {
            assert!(
                w.read::<AuctionState>(auction(*pos, 0)).settled,
                "seed {seed}: every auction settled"
            );
        }
        assert_ok(check_all(&w, &t), "after settling everything");
        // F5 on the collected penalties (invariant 9 with lots out and remainders back).
        w.start_surplus(0).unwrap();
        w.take_surplus(0, 250, u128::MAX).unwrap();
        w.advance(14_400_000);
        w.close_surplus(0).unwrap();
        assert_ok(check_all(&w, &t), "after surplus auction");
    }
}

#[test]
fn liveness_unsafe_position_is_liquidatable_within_caps_or_alone() {
    // Invariant 12: blocked only while another auction holds the capacity; liquidatable once it settles.
    let mut w = World::with_engine();
    let whale = w.open_position(
        k::OWNER,
        k::OWNER_COLL,
        k::OWNER_STABLE,
        0,
        LIQ_COLLATERAL,
        LIQ_DEBT,
    );
    let small = w.open_small_position();
    w.set_min_cr(2 * ONE);
    let cap = CollateralArgs {
        type_cap: 600_000,
        ..CollateralArgs::defaults_for_tests()
    };
    w.set_collateral_config(k::ENGINE_ADMIN, &cap).unwrap();
    w.liquidate(whale, 0).unwrap();
    assert_err(w.liquidate(small, 0), "CapacityExhausted");
    w.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 1_000_000);
    w.bid_settle(whale, u128::MAX, 0).unwrap();
    w.liquidate(small, 0)
        .expect("liquidatable once the reservation is released");
}

#[test]
fn system_invariants_burn_and_mint_totals() {
    // Invariant 17: minted <= cap; unbacked falls by exactly the coin burned; the protocol-token supply
    // moves by exactly the minted and burned totals.
    let mut w = World::with_system(false);
    w.book_shortfall();
    w.fund_stable(k::TAKER_STABLE, 2_000);
    let protocol_supply = w.supply(protocol_token_def());
    let unbacked = w.read::<AccountingState>(accounting()).unbacked_debt;
    w.start_debt(0).unwrap();
    w.take_debt(
        0,
        k::TAKER_STABLE,
        k::TAKER_PROTOCOL,
        &[k::TAKER_STABLE],
        300,
        0,
    )
    .unwrap();
    w.take_debt(
        0,
        k::TAKER_STABLE,
        k::TAKER_PROTOCOL,
        &[k::TAKER_STABLE],
        u128::MAX,
        0,
    )
    .unwrap();
    let acc: AccountingState = w.read(accounting());
    assert_eq!(acc.unbacked_debt, unbacked - 500);
    assert!(acc.total_protocol_minted <= 5_000);
    assert_eq!(
        w.supply(protocol_token_def()),
        protocol_supply + acc.total_protocol_minted
    );
    // Surplus side: refill the buffer past target + lot after the debt is covered by donations.
    w.donate_stable(buffer(), acc.unbacked_debt + 1_500);
    w.start_surplus(0).unwrap();
    let before = w.supply(protocol_token_def());
    w.take_surplus(0, 500, u128::MAX).unwrap();
    let acc: AccountingState = w.read(accounting());
    assert_eq!(
        before - w.supply(protocol_token_def()),
        acc.total_protocol_burned
    );
    assert!(!w.read::<SurplusAuctionState>(surplus_auction(0)).live);
}

#[test]
fn r3_a_bid_touches_only_its_own_auction() {
    let mut w = World::with_engine();
    let a = w.open_position(
        k::OWNER,
        k::OWNER_COLL,
        k::OWNER_STABLE,
        0,
        LIQ_COLLATERAL,
        LIQ_DEBT,
    );
    let b = w.open_small_position();
    w.set_min_cr(2 * ONE);
    w.liquidate(a, 0).unwrap();
    w.liquidate(b, 0).unwrap();
    w.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 1_000_000);
    let (b_auction, b_vault, b_record, b_host, acct) = (
        w.raw(auction(b, 0)),
        w.raw(auction_vault(b, 0)),
        w.raw(record(b)),
        w.raw(b),
        w.raw(accounting()),
    );
    w.bid(a, 100_000, 0).unwrap();
    assert_eq!(
        w.raw(auction(b, 0)),
        b_auction,
        "R3: other auction untouched"
    );
    assert_eq!(
        w.raw(auction_vault(b, 0)),
        b_vault,
        "R3: other vault untouched"
    );
    assert_eq!(
        w.raw(record(b)),
        b_record,
        "R3: other liquidation record untouched"
    );
    assert_eq!(w.raw(b), b_host, "R3: other host position untouched");
    assert_eq!(
        w.raw(accounting()),
        acct,
        "S6: non-final bid does not write Accounting"
    );
}

// ---- Binding addition 1 (Task 5): invariant 10, the debt cleared must match the seizure ----

#[test]
fn inv10_debt_clear_matches_seizure_over_full_chain() {
    let (w, t, pos) = full_chain();
    let l = &t.liquidated[0];
    let a: AuctionState = w.read(auction(pos, 0));
    // The engine recorded what the host actually released, and the books close: principal = burned + shortfall.
    assert_eq!(
        (l.host_collateral, l.host_debt),
        (a.collateral_seized, a.debt_principal)
    );
    let acc: AccountingState = w.read(accounting());
    assert_eq!(
        a.debt_principal,
        acc.total_debt_burned + acc.total_shortfall_booked
    );
    // Every collateral unit went somewhere recorded: keeper, bidder, owner (the vault is empty after the claim).
    assert_eq!(w.balance(id(k::KEEPER_COLL)), a.keeper_reward);
    assert_eq!(w.balance(id(k::BIDDER_COLL)), a.collateral_sold);
    assert_eq!(w.balance(id(k::OWNER_COLL)), a.leftover_claimed);
    assert_eq!(
        a.collateral_seized,
        a.keeper_reward + a.collateral_sold + a.leftover_claimed
    );
    assert_eq!(w.balance(auction_vault(pos, 0)), 0);
}

type Breaker = fn(&mut World, &Tracked);

#[test]
fn inv10_check_catches_seizure_without_debt_clear() {
    // Each case starts from the real, passing chain and breaks one side in a test-local way, simulating an
    // engine path that seizes collateral without clearing the matching debt (or records a different amount).
    let cases: [(&str, &str, Breaker); 5] = [
        (
            "seized but the host debt was not cleared",
            "inv 10 host position",
            |w, t| {
                let l = &t.liquidated[0];
                tamper::<Position>(w, l.pos, |p| p.normalized_debt_amount = l.host_debt);
            },
        ),
        (
            "debt recorded below what the host cleared",
            "inv 10 debt",
            |w, t| {
                tamper::<AuctionState>(w, auction(t.liquidated[0].pos, 0), |a| {
                    a.debt_principal -= 1
                });
            },
        ),
        (
            "debt recorded above what the host cleared",
            "inv 10 debt",
            |w, t| {
                tamper::<AuctionState>(w, auction(t.liquidated[0].pos, 0), |a| {
                    a.debt_principal += 1
                });
            },
        ),
        (
            "collateral recorded above what the host released",
            "inv 10 collateral",
            |w, t| {
                tamper::<AuctionState>(w, auction(t.liquidated[0].pos, 0), |a| {
                    a.collateral_seized += 1;
                    a.collateral_left += 1; // keeps the book balanced so only the host comparison can catch it
                });
            },
        ),
        (
            "debt cleared but never burned nor booked as shortfall",
            "inv 10 chain",
            |w, _| {
                tamper::<AccountingState>(w, accounting(), |s| s.total_debt_burned -= 1);
            },
        ),
    ];
    for (name, expect, break_it) in cases {
        let (mut w, t, _) = full_chain();
        assert_ok(check_all(&w, &t), "baseline");
        break_it(&mut w, &t);
        let err = check_all(&w, &t).expect_err(name);
        assert!(
            err.contains(expect),
            "{name}: expected {expect:?} in {err:?}"
        );
    }
}

// ---- Binding addition 2 (Task 13): the surplus book uses >=, never == ----

#[test]
fn surplus_returned_ge_untaken_lot_with_vault_donation() {
    let mut w = World::with_system(false);
    w.donate_stable(buffer(), 1_500);
    w.start_surplus(0).unwrap();
    let lots_out = w
        .read::<AccountingState>(accounting())
        .total_surplus_lots_out;
    assert_eq!(lots_out, 500);
    w.take_surplus(0, 250, u128::MAX).unwrap();
    let taken = w.balance(id(k::TAKER_STABLE));
    assert_eq!(taken, 250);
    // A third party donates into the live surplus vault: the vault covers the book with room to spare.
    w.donate_stable(surplus_vault(0), 77);
    let live: SurplusAuctionState = w.read(surplus_auction(0));
    assert!(live.live);
    assert!(
        w.balance(surplus_vault(0)) >= live.lot_left,
        "live vault covers lot_left"
    );
    assert_ne!(
        w.balance(surplus_vault(0)),
        live.lot_left,
        "an == check on the live vault would fail"
    );
    w.advance(14_400_000);
    w.close_surplus(0).unwrap();
    let acc: AccountingState = w.read(accounting());
    let untaken = lots_out - taken;
    // Closing returns the WHOLE vault, donation included: returned >= untaken lot, and == would be wrong.
    assert!(
        acc.total_surplus_returned >= untaken,
        "returned {} < untaken {untaken}",
        acc.total_surplus_returned
    );
    assert_eq!(acc.total_surplus_returned, untaken + 77);
    assert_ne!(
        acc.total_surplus_returned, untaken,
        "an == invariant would reject a correct run"
    );
    // The buffer got everything back and the vault is empty.
    assert_eq!(
        w.balance(buffer()),
        1_500 - lots_out + acc.total_surplus_returned
    );
    assert_eq!(w.balance(surplus_vault(0)), 0);
    assert_ok(
        check_all(&w, &Tracked::default()),
        "after donated surplus auction",
    );
}

// ---- Binding addition 3 (Task 13): no admin instruction moves surplus or buffer funds ----

#[derive(Debug, PartialEq, Eq)]
enum Caller {
    Admin,
    Permissionless,
}

/// Exhaustive on purpose: adding an engine instruction forces a decision about who may call it.
fn caller_of(ix: &Ix) -> Caller {
    match ix {
        Ix::InitConfig { .. } | Ix::SetEngineParams { .. } | Ix::SetCollateralConfig { .. } => {
            Caller::Admin
        }
        Ix::ObserveFeed
        | Ix::Liquidate { .. }
        | Ix::Bid { .. }
        | Ix::BidWithSettlement { .. }
        | Ix::SettleEmpty
        | Ix::ClaimLeftover
        | Ix::SettleDebt { .. }
        | Ix::StartSurplusAuction { .. }
        | Ix::TakeSurplus { .. }
        | Ix::CloseSurplusAuction
        | Ix::StartDebtAuction { .. }
        | Ix::TakeDebt { .. }
        | Ix::CloseDebtAuction
        | Ix::CheckSolvency => Caller::Permissionless,
    }
}

struct Attempt {
    name: &'static str,
    ix: Ix,
    accounts: Vec<AccountId>,
    must_succeed: bool,
}

fn fund_snapshot_without_surplus(w: &World, pos: AccountId) -> Vec<Account> {
    [
        buffer(),
        auction_vault(pos, 0),
        accounting(),
        auction(pos, 0),
        stable_def(),
    ]
    .iter()
    .map(|a| w.raw(*a))
    .collect()
}

fn fund_snapshot(w: &World, pos: AccountId) -> Vec<Account> {
    [
        buffer(),
        surplus_vault(0),
        auction_vault(pos, 0),
        accounting(),
        auction(pos, 0),
        surplus_auction(0),
        stable_def(),
    ]
    .iter()
    .map(|a| w.raw(*a))
    .collect()
}

#[test]
fn no_admin_instruction_moves_surplus_or_buffer_funds() {
    let mut w = World::with_system(false);
    let pos = w.unsafe_position();
    w.liquidate(pos, 0).unwrap();
    w.donate_stable(buffer(), 1_500);
    let admin = id(k::ENGINE_ADMIN);

    let p = EngineParamsArgs {
        protocol_token_enabled: true,
        surplus_lot: 400,
        ..EngineParamsArgs::defaults_for_tests()
    };
    let c = CollateralArgs {
        admin_paused: true,
        ..CollateralArgs::defaults_for_tests()
    };
    // A legitimate set_engine_params while the buffer and an auction vault are funded (it is refused once a
    // system auction is live, ProtocolTokenLocked, so it runs before the surplus auction starts).
    let funds_before = fund_snapshot_without_surplus(&w, pos);
    w.engine(
        vec![
            admin,
            config(),
            accounting(),
            coll_config(),
            protocol_token_def(),
            unused(0),
        ],
        &[k::ENGINE_ADMIN],
        set_engine_params_ix(&p),
    )
    .expect("legit set_engine_params");
    assert_eq!(
        funds_before,
        fund_snapshot_without_surplus(&w, pos),
        "legit set_engine_params moved funds"
    );

    w.start_surplus(0).unwrap();
    assert!(
        w.balance(buffer()) > 0
            && w.balance(surplus_vault(0)) > 0
            && w.balance(auction_vault(pos, 0)) > 0,
        "all three fund accounts hold a balance"
    );
    let before = fund_snapshot(&w, pos);

    let new_salt = 99;
    let other_config =
        auction_engine_core::pda::config_pda(&engine_program(), &id(k::ENGINE_ADMIN), new_salt);
    let coll_accounts = |coll: AccountId, orc: AccountId, def: AccountId| {
        vec![
            admin,
            config(),
            def,
            coll,
            orc,
            protocol_parameters(),
            redemption_state(),
            CLOCK,
        ]
    };
    let init_accounts = |cfg: AccountId, acct: AccountId, buf: AccountId| {
        vec![
            admin,
            cfg,
            acct,
            buf,
            protocol_parameters(),
            stable_def(),
            protocol_token_def(),
            unused(0),
        ]
    };
    // The legitimate set_collateral_config must succeed (proves the test is live); every other attempt puts a
    // fund account into a slot the instruction accepts, or reuses existing accounts.
    let attempts = vec![
        Attempt {
            name: "set_engine_params (live system auction: ProtocolTokenLocked)",
            ix: set_engine_params_ix(&EngineParamsArgs {
                surplus_lot: 300,
                ..p.clone()
            }),
            accounts: vec![
                admin,
                config(),
                accounting(),
                coll_config(),
                protocol_token_def(),
                unused(0),
            ],
            must_succeed: false,
        },
        Attempt {
            name: "set_engine_params (buffer as ref-oracle slot)",
            ix: set_engine_params_ix(&p),
            accounts: vec![
                admin,
                config(),
                accounting(),
                coll_config(),
                protocol_token_def(),
                buffer(),
            ],
            must_succeed: false,
        },
        Attempt {
            name: "set_engine_params (surplus vault as protocol-token slot)",
            ix: set_engine_params_ix(&p),
            accounts: vec![
                admin,
                config(),
                accounting(),
                coll_config(),
                surplus_vault(0),
                unused(0),
            ],
            must_succeed: false,
        },
        Attempt {
            name: "set_engine_params (buffer as accounting)",
            ix: set_engine_params_ix(&p),
            accounts: vec![
                admin,
                config(),
                buffer(),
                coll_config(),
                protocol_token_def(),
                unused(0),
            ],
            must_succeed: false,
        },
        Attempt {
            name: "set_collateral_config (legit)",
            ix: collateral_ix(&c),
            accounts: coll_accounts(coll_config(), oracle(), collateral_def()),
            must_succeed: true,
        },
        Attempt {
            name: "set_collateral_config (buffer as coll_config)",
            ix: collateral_ix(&c),
            accounts: coll_accounts(buffer(), oracle(), collateral_def()),
            must_succeed: false,
        },
        Attempt {
            name: "set_collateral_config (surplus vault as oracle)",
            ix: collateral_ix(&c),
            accounts: coll_accounts(coll_config(), surplus_vault(0), collateral_def()),
            must_succeed: false,
        },
        Attempt {
            name: "set_collateral_config (auction vault as collateral definition)",
            ix: collateral_ix(&c),
            accounts: coll_accounts(coll_config(), oracle(), auction_vault(pos, 0)),
            must_succeed: false,
        },
        Attempt {
            name: "init_config again (same salt, existing accounts)",
            ix: init_config_ix(SALT, &p),
            accounts: init_accounts(config(), accounting(), buffer()),
            must_succeed: false,
        },
        Attempt {
            name: "init_config (new salt, existing funded buffer)",
            ix: init_config_ix(new_salt, &p),
            accounts: init_accounts(other_config, accounting(), buffer()),
            must_succeed: false,
        },
    ];
    for a in attempts {
        assert_eq!(
            caller_of(&a.ix),
            Caller::Admin,
            "{}: only admin instructions are attempted",
            a.name
        );
        let funds_before = fund_snapshot(&w, pos);
        let result = w.engine(a.accounts, &[k::ENGINE_ADMIN], a.ix);
        if a.must_succeed {
            result.unwrap_or_else(|e| panic!("{}: {e:?}", a.name));
        } else {
            assert!(
                result.is_err(),
                "{}: malformed admin call was accepted",
                a.name
            );
        }
        assert_eq!(
            funds_before,
            fund_snapshot(&w, pos),
            "{}: funds or books moved",
            a.name
        );
    }
    assert_eq!(
        before,
        fund_snapshot(&w, pos),
        "no fund account, book or the stablecoin supply changed"
    );
}
