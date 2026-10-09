//! Spec section 5.13 invariants that need accounts (Plan 1 left them to Plan 2): 2, 7, 9, 10, 11, 12, 17 and R3.
//! `check_all` returns the first violated invariant as `Err(text)` so negative tests can show that the
//! check itself catches a broken engine (invariant 10, ADR-036).

mod common;

use auction_engine_core::{
    args::{CollateralArgs, EngineParamsArgs},
    instruction::instruction_samples,
    state::{AccountingState, AuctionState, EngineConfigState, SurplusAuctionState},
    Instruction as Ix,
};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID as CLOCK;
use common::checks::*;
use common::*;
use lee_core::account::{Account, AccountId};
use stablecoin_core::Position;

fn lcg(seed: &mut u64) -> u64 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *seed >> 33
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

    // Within the caps, both auctions run side by side (660_000 + 132_000 <= 800_000).
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
    let roomy = CollateralArgs {
        type_cap: 800_000,
        ..CollateralArgs::defaults_for_tests()
    };
    w.set_collateral_config(k::ENGINE_ADMIN, &roomy).unwrap();
    let mut t = Tracked::default();
    liquidate_tracked(&mut w, &mut t, whale);
    liquidate_tracked(&mut w, &mut t, small);
    assert_ok(check_all(&w, &t), "two auctions within the cap");
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
    let cfg: EngineConfigState = w.read(config());
    assert!(acc.total_protocol_minted <= cfg.system.max_mint_per_auction);
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
    let (a_auction, config_before, coll_before) =
        (w.raw(auction(a, 0)), w.raw(config()), w.raw(coll_config()));
    w.bid(a, 100_000, 0).unwrap();
    assert_ne!(
        w.raw(auction(a, 0)),
        a_auction,
        "the bid did change its own auction (the isolation check is not vacuous)"
    );
    assert_eq!(
        w.raw(config()),
        config_before,
        "R3: a non-final bid leaves the shared config untouched"
    );
    assert_eq!(
        w.raw(coll_config()),
        coll_before,
        "R3: a non-final bid leaves the collateral config untouched"
    );
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

/// Exhaustive on purpose: adding an engine instruction forces a decision about who may call it.
/// `Some(name)` = admin instruction.
fn admin_name(ix: &Ix) -> Option<&'static str> {
    match ix {
        Ix::InitConfig { .. } => Some("init_config"),
        Ix::SetEngineParams { .. } => Some("set_engine_params"),
        Ix::SetCollateralConfig { .. } => Some("set_collateral_config"),
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
        | Ix::CheckSolvency => None,
    }
}

/// Derived from one sample per enum variant, so a new admin instruction is picked up by `admin_name`.
fn admin_instructions() -> Vec<&'static str> {
    instruction_samples()
        .iter()
        .filter_map(admin_name)
        .collect()
}

struct Attempt {
    name: &'static str,
    ix: Ix,
    accounts: Vec<AccountId>,
    /// `None`: must succeed; `Some(text)`: must be rejected and the error must contain `text`.
    expect_err: Option<&'static str>,
}

fn fund_snapshot(w: &World, pos: AccountId, with_surplus: bool) -> Vec<Account> {
    let mut ids = vec![
        buffer(),
        auction_vault(pos, 0),
        accounting(),
        auction(pos, 0),
        stable_def(),
    ];
    if with_surplus {
        ids.extend([surplus_vault(0), surplus_auction(0)]);
    }
    ids.iter().map(|a| w.raw(*a)).collect()
}

/// Submits the attempt as the engine admin and checks the verdict and that no fund or book moved.
fn run_attempt(w: &mut World, pos: AccountId, with_surplus: bool, a: &Attempt) {
    let funds_before = fund_snapshot(w, pos, with_surplus);
    let result = w.engine(a.accounts.clone(), &[k::ENGINE_ADMIN], a.ix.clone());
    match a.expect_err {
        None => result.unwrap_or_else(|e| panic!("{}: {e:?}", a.name)),
        Some(text) => {
            println!("ADMIN[{}] rejected: {:?}", a.name, result.as_ref().err());
            assert_err(result, text);
        }
    }
    assert_eq!(
        funds_before,
        fund_snapshot(w, pos, with_surplus),
        "{}: funds or books moved",
        a.name
    );
}

fn admin_params() -> EngineParamsArgs {
    EngineParamsArgs {
        protocol_token_enabled: true,
        surplus_lot: 400,
        ..EngineParamsArgs::defaults_for_tests()
    }
}

fn params_attempt(
    name: &'static str,
    p: &EngineParamsArgs,
    slots: [AccountId; 2],
    acct: AccountId,
    expect_err: Option<&'static str>,
) -> Attempt {
    Attempt {
        name,
        ix: set_engine_params_ix(p),
        accounts: vec![
            id(k::ENGINE_ADMIN),
            config(),
            acct,
            coll_config(),
            slots[0],
            slots[1],
        ],
        expect_err,
    }
}

fn coll_attempt(
    name: &'static str,
    coll: AccountId,
    orc: AccountId,
    def: AccountId,
    expect_err: Option<&'static str>,
) -> Attempt {
    let c = CollateralArgs {
        admin_paused: true,
        ..CollateralArgs::defaults_for_tests()
    };
    Attempt {
        name,
        ix: collateral_ix(&c),
        accounts: vec![
            id(k::ENGINE_ADMIN),
            config(),
            def,
            coll,
            orc,
            protocol_parameters(),
            redemption_state(),
            CLOCK,
        ],
        expect_err,
    }
}

fn init_attempt(
    name: &'static str,
    salt: u64,
    accounts: [AccountId; 3],
    expect_err: Option<&'static str>,
) -> Attempt {
    Attempt {
        name,
        ix: init_config_ix(salt, &admin_params()),
        accounts: vec![
            id(k::ENGINE_ADMIN),
            accounts[0],
            accounts[1],
            accounts[2],
            protocol_parameters(),
            stable_def(),
            protocol_token_def(),
            unused(0),
        ],
        expect_err,
    }
}

fn assert_covers_every_admin_instruction(attempts: &[&Attempt]) {
    let mut seen: Vec<&str> = attempts.iter().filter_map(|a| admin_name(&a.ix)).collect();
    seen.sort_unstable();
    seen.dedup();
    let mut all = admin_instructions();
    all.sort_unstable();
    assert_eq!(
        all.len(),
        3,
        "init_config, set_engine_params, set_collateral_config"
    );
    assert_eq!(seen, all, "every admin instruction is attempted");
}

#[test]
fn no_admin_instruction_moves_surplus_or_buffer_funds() {
    let mut w = World::with_system(false);
    let pos = w.unsafe_position();
    w.liquidate(pos, 0).unwrap();
    w.donate_stable(buffer(), 1_500);
    let p = admin_params();
    let (protocol, no_ref) = (protocol_token_def(), unused(0));

    // Legitimate calls while the buffer and an auction vault are funded (set_engine_params is refused for a
    // protocol-token or system-parameter change once a system auction is live, so it runs before one starts).
    let legit_params = params_attempt(
        "set_engine_params (legit)",
        &p,
        [protocol, no_ref],
        accounting(),
        None,
    );
    let legit_coll = coll_attempt(
        "set_collateral_config (legit)",
        coll_config(),
        oracle(),
        collateral_def(),
        None,
    );
    run_attempt(&mut w, pos, false, &legit_params);
    run_attempt(&mut w, pos, false, &legit_coll);

    w.start_surplus(0).unwrap();
    assert!(
        w.balance(buffer()) > 0
            && w.balance(surplus_vault(0)) > 0
            && w.balance(auction_vault(pos, 0)) > 0,
        "all three fund accounts hold a balance"
    );
    let before = fund_snapshot(&w, pos, true);
    let new_salt = 99;
    let other_config =
        auction_engine_core::pda::config_pda(&engine_program(), &id(k::ENGINE_ADMIN), new_salt);
    let other_acct = auction_engine_core::pda::accounting_pda(&engine_program(), &other_config);
    let locked = EngineParamsArgs {
        surplus_lot: 300,
        ..p.clone()
    };
    let attempts = vec![
        params_attempt("set_engine_params (live system auction: system-parameter lock)", &locked, [protocol, no_ref], accounting(), Some("ProtocolTokenLocked")),
        params_attempt("set_engine_params (buffer as ref-oracle slot)", &p, [protocol, buffer()], accounting(), Some("unused slot")),
        params_attempt("set_engine_params (surplus vault as protocol-token slot: rejected by the live-auction lock)", &p, [surplus_vault(0), no_ref], accounting(), Some("ProtocolTokenLocked")),
        params_attempt("set_engine_params (buffer as accounting)", &p, [protocol, no_ref], buffer(), Some(r#"WrongAccount(\"accounting\")"#)),
        coll_attempt("set_collateral_config (buffer as coll_config)", buffer(), oracle(), collateral_def(), Some(r#"PdaMismatch { account_name: \"coll_config\""#)),
        coll_attempt("set_collateral_config (surplus vault as oracle)", coll_config(), surplus_vault(0), collateral_def(), Some("OracleMismatch")),
        coll_attempt("set_collateral_config (auction vault as collateral definition)", coll_config(), oracle(), auction_vault(pos, 0), Some(r#"PdaMismatch { account_name: \"coll_config\""#)),
        init_attempt("init_config again (same salt, existing accounts)", SALT, [config(), accounting(), buffer()], Some("AccountAlreadyInitialized")),
        init_attempt("init_config (new salt, new config and accounting, existing funded buffer)", new_salt, [other_config, other_acct, buffer()], Some(r#"PdaMismatch { account_name: \"buffer\""#)),
    ];
    let covered: Vec<&Attempt> = attempts
        .iter()
        .chain([&legit_params, &legit_coll])
        .collect();
    assert_covers_every_admin_instruction(&covered);
    for a in &attempts {
        run_attempt(&mut w, pos, true, a);
    }
    assert_eq!(
        before,
        fund_snapshot(&w, pos, true),
        "no fund account, book or the stablecoin supply changed"
    );
}

/// ADR-038 (THREAT_MODEL finding 4): the protocol-token slot is validated at config time, so the admin
/// can no longer point it at a fund account; the config and every fund stay unchanged.
#[test]
fn admin_cannot_point_the_protocol_token_slot_at_a_fund_account() {
    let mut w = World::with_system(false);
    let pos = w.unsafe_position();
    w.liquidate(pos, 0).unwrap();
    w.donate_stable(buffer(), 1_500);
    let before = fund_snapshot(&w, pos, false);
    let a = params_attempt(
        "set_engine_params (buffer as protocol-token slot, no live auction)",
        &admin_params(),
        [buffer(), unused(0)],
        accounting(),
        Some("ProtocolTokenMismatch"),
    );
    run_attempt(&mut w, pos, false, &a);
    let cfg: EngineConfigState = w.read(config());
    assert_eq!(
        cfg.protocol_token,
        Some(protocol_token_def()),
        "the configured token is unchanged"
    );
    assert_eq!(
        before,
        fund_snapshot(&w, pos, false),
        "no fund or book moved"
    );
}
