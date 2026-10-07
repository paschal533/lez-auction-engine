//! ADR-036: every engine instruction fits the chained-call budget (Shared Reference R3), measured with the
//! risc0 executor on the real guests. Run with `--nocapture` to see the `CYCLES[...]` lines.

mod common;

use auction_engine_core::{args::CollateralArgs, pda::SLOT_REF_ORACLE, Instruction as Ix};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID as CLOCK;
use common::*;

/// Shared Reference R3: every instruction fits the private-path limit (10 executions).
#[test]
fn every_instruction_fits_the_chain_budget() {
    let mut w = World::with_system(false);
    let mut table: Vec<(&str, usize)> = Vec::new();
    let mut record = |name: &'static str, (n, r): (usize, Result<(), lee::error::LeeError>)| {
        r.unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert!(n <= 10, "{name}: {n} executions");
        table.push((name, n));
    };
    record(
        "observe_feed",
        w.measured(
            "observe_feed",
            vec![
                id(k::CALLER),
                config(),
                coll_config(),
                redemption_state(),
                oracle(),
                CLOCK,
            ],
            &[k::CALLER],
            Ix::ObserveFeed,
        ),
    );
    let c = CollateralArgs::defaults_for_tests();
    record(
        "set_collateral_config",
        w.measured(
            "set_collateral_config",
            vec![
                id(k::ENGINE_ADMIN),
                config(),
                collateral_def(),
                coll_config(),
                oracle(),
                protocol_parameters(),
                redemption_state(),
                CLOCK,
            ],
            &[k::ENGINE_ADMIN],
            collateral_ix(&c),
        ),
    );
    let pos = w.unsafe_position();
    w.holding(k::KEEPER_COLL, collateral_def(), 0);
    let accounts = w.liquidate_accounts(pos, 0, id(k::KEEPER_COLL));
    record(
        "liquidate",
        w.measured(
            "liquidate",
            accounts,
            &[k::KEEPER],
            Ix::Liquidate { nonce: 0 },
        ),
    );
    w.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 1_000_000);
    let accounts = w.bid_accounts(pos, 0, k::BIDDER_STABLE, k::BIDDER_COLL, false);
    record(
        "bid",
        w.measured(
            "bid",
            accounts,
            &[k::BIDDER_STABLE],
            Ix::Bid {
                coin_in_max: 100_000,
                min_collateral_out: 0,
            },
        ),
    );
    let accounts = w.bid_accounts(pos, 0, k::BIDDER_STABLE, k::BIDDER_COLL, true);
    record(
        "bid_with_settlement(final)",
        w.measured(
            "bid_final",
            accounts,
            &[k::BIDDER_STABLE],
            Ix::BidWithSettlement {
                coin_in_max: 560_000,
                min_collateral_out: 0,
            },
        ),
    );
    record(
        "claim_leftover",
        w.measured(
            "claim_leftover",
            vec![
                id(k::OWNER),
                id(k::OWNER_COLL),
                config(),
                auction(pos, 0),
                auction_vault(pos, 0),
            ],
            &[k::OWNER],
            Ix::ClaimLeftover,
        ),
    );
    w.donate_stable(buffer(), 1_500);
    record(
        "start_surplus_auction",
        w.measured(
            "start_surplus",
            vec![
                id(k::CALLER),
                config(),
                accounting(),
                buffer(),
                stable_def(),
                surplus_auction(0),
                surplus_vault(0),
                unused(SLOT_REF_ORACLE),
                CLOCK,
            ],
            &[k::CALLER],
            Ix::StartSurplusAuction { nonce: 0 },
        ),
    );
    record(
        "take_surplus",
        w.measured(
            "take_surplus",
            vec![
                id(k::TAKER_PROTOCOL),
                id(k::TAKER_STABLE),
                config(),
                accounting(),
                surplus_auction(0),
                surplus_vault(0),
                protocol_token_def(),
                CLOCK,
            ],
            &[k::TAKER_PROTOCOL],
            Ix::TakeSurplus {
                amount_coin: 250,
                max_protocol_in: u128::MAX,
            },
        ),
    );
    w.advance(14_400_000);
    record(
        "close_surplus_auction",
        w.measured(
            "close_surplus",
            vec![
                id(k::CALLER),
                config(),
                accounting(),
                surplus_auction(0),
                surplus_vault(0),
                buffer(),
                CLOCK,
            ],
            &[k::CALLER],
            Ix::CloseSurplusAuction,
        ),
    );
    record(
        "settle_debt",
        w.measured(
            "settle_debt",
            vec![
                id(k::CALLER),
                config(),
                accounting(),
                buffer(),
                stable_def(),
            ],
            &[k::CALLER],
            Ix::SettleDebt { amount: u128::MAX },
        ),
    );
    println!("CHAIN TABLE {table:?}");
    let get = |n: &str| {
        table
            .iter()
            .find(|(name, _)| *name == n)
            .map(|(_, c)| *c)
            .expect("measured")
    };
    assert_eq!(get("liquidate"), 5);
    assert_eq!(get("bid"), 3);
    assert_eq!(get("bid_with_settlement(final)"), 4);
    assert_eq!(get("take_surplus"), 3);
}

#[test]
fn debt_and_solvency_chains_fit_the_budget() {
    let mut w = World::with_system(false);
    w.book_shortfall();
    w.fund_stable(k::TAKER_STABLE, 1_000);
    let (n, r) = w.measured(
        "start_debt",
        vec![
            id(k::CALLER),
            config(),
            accounting(),
            buffer(),
            stable_def(),
            debt_auction(0),
            protocol_token_def(),
            unused(SLOT_REF_ORACLE),
            CLOCK,
        ],
        &[k::CALLER],
        Ix::StartDebtAuction { nonce: 0 },
    );
    r.unwrap();
    assert!(n <= 10);
    let (n, r) = w.measured(
        "take_debt",
        vec![
            id(k::TAKER_STABLE),
            id(k::TAKER_PROTOCOL),
            config(),
            accounting(),
            debt_auction(0),
            stable_def(),
            protocol_token_def(),
            mint_authority(),
            CLOCK,
        ],
        &[k::TAKER_STABLE],
        Ix::TakeDebt {
            amount_coin: 500,
            min_protocol_out: 0,
        },
    );
    r.unwrap();
    assert_eq!(n, 3);
    let mut w = World::with_engine();
    w.book_shortfall();
    let (n, r) = w.measured(
        "check_solvency(freeze)",
        vec![
            id(k::CALLER),
            config(),
            accounting(),
            buffer(),
            stable_def(),
            unused(1),
            unused(0),
            engine_authority(),
            les(),
            protocol_parameters(),
            CLOCK,
        ],
        &[k::CALLER],
        Ix::CheckSolvency,
    );
    r.unwrap();
    assert_eq!(
        n, 2,
        "engine + freeze hook (no implicit burn: empty buffer)"
    );
}

/// The variants with an implicit buffer burn (a non-empty buffer next to unbacked debt) add one execution.
#[test]
fn implicit_burn_variants_fit_the_budget() {
    let mut w = World::with_system(false);
    w.book_shortfall();
    w.donate_stable(buffer(), 100);
    let (n, r) = w.measured(
        "settle_debt(burn)",
        vec![
            id(k::CALLER),
            config(),
            accounting(),
            buffer(),
            stable_def(),
        ],
        &[k::CALLER],
        Ix::SettleDebt { amount: u128::MAX },
    );
    r.unwrap();
    assert_eq!(n, 2, "settle_debt: engine + burn");

    let mut w = World::with_system(false);
    w.book_shortfall();
    w.donate_stable(buffer(), 100);
    let (n, r) = w.measured(
        "start_debt(burn)",
        vec![
            id(k::CALLER),
            config(),
            accounting(),
            buffer(),
            stable_def(),
            debt_auction(0),
            protocol_token_def(),
            unused(SLOT_REF_ORACLE),
            CLOCK,
        ],
        &[k::CALLER],
        Ix::StartDebtAuction { nonce: 0 },
    );
    r.unwrap();
    assert_eq!(n, 2, "start_debt: engine + implicit burn");

    let mut w = World::with_system(false);
    w.book_shortfall();
    let unbacked = w
        .read::<auction_engine_core::state::AccountingState>(accounting())
        .unbacked_debt;
    w.donate_stable(buffer(), unbacked + 1_500);
    let accounts = w.start_surplus_accounts(0);
    let (n, r) = w.measured(
        "start_surplus(burn)",
        accounts,
        &[k::CALLER],
        Ix::StartSurplusAuction { nonce: 0 },
    );
    r.unwrap();
    assert_eq!(
        n, 4,
        "start_surplus: engine + init vault + implicit burn + lot transfer"
    );

    let mut w = World::with_engine();
    w.book_shortfall();
    w.donate_stable(buffer(), 100);
    let (n, r) = w.measured(
        "check_solvency(burn+freeze)",
        vec![
            id(k::CALLER),
            config(),
            accounting(),
            buffer(),
            stable_def(),
            unused(1),
            unused(0),
            engine_authority(),
            les(),
            protocol_parameters(),
            CLOCK,
        ],
        &[k::CALLER],
        Ix::CheckSolvency,
    );
    r.unwrap();
    assert_eq!(n, 3, "check_solvency: engine + implicit burn + freeze hook");
}
