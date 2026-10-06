mod common;

use auction_engine_core::{
    args::CollateralArgs,
    state::{AuctionState, CollateralConfigState, EngineConfigState, LiquidationRecordState},
    Instruction as Ix,
};
use common::*;
use lee_core::account::{Account, Data};
use stablecoin_core::Position;
use token_core::TokenHolding;

fn coll(w: &World) -> CollateralConfigState {
    w.read(coll_config())
}

#[test]
fn liquidate_happy_path() {
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    w.liquidate(pos, 0).expect("liquidate");
    let a: AuctionState = w.read(auction(pos, 0));
    assert_eq!(
        (
            a.collateral_seized,
            a.keeper_reward,
            a.collateral_left,
            a.debt_principal,
            a.amount_to_raise,
            a.reserved_amount
        ),
        (
            LIQ_COLLATERAL,
            LIQ_REWARD,
            LIQ_LOT,
            LIQ_DEBT,
            LIQ_TARGET,
            LIQ_TARGET
        )
    );
    assert_eq!(
        (a.owner, a.keeper, a.settled, a.start_ms),
        (id(k::OWNER), id(k::KEEPER), false, w.now)
    );
    let p: Position = w.read(pos);
    assert_eq!((p.collateral_amount, p.normalized_debt_amount), (0, 0));
    assert_eq!(w.balance(host_vault(pos)), 0);
    assert_eq!(w.balance(auction_vault(pos, 0)), LIQ_LOT);
    assert_eq!(w.balance(id(k::KEEPER_COLL)), LIQ_REWARD);
    assert_eq!(coll(&w).type_reserved, LIQ_TARGET);
    assert_eq!(
        w.read::<EngineConfigState>(config()).global_reserved,
        LIQ_TARGET
    );
    assert_eq!(w.read::<LiquidationRecordState>(record(pos)).count, 1);
}

#[test]
fn replayed_and_skipped_nonces_are_rejected() {
    let mut w = World::with_engine();
    let pos = w.open_position(
        k::OWNER,
        k::OWNER_COLL,
        k::OWNER_STABLE,
        0,
        LIQ_COLLATERAL,
        LIQ_DEBT,
    );
    let small = w.open_small_position();
    w.set_min_cr(2 * ONE);
    w.liquidate(pos, 0).unwrap();
    assert_err(w.liquidate(pos, 0), "AccountAlreadyInitialized");
    assert_err(w.liquidate(small, 1), "NonceMismatch");
}

#[test]
fn refilled_position_is_liquidated_again_with_next_nonce() {
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    w.liquidate(pos, 0).unwrap();
    // Owner refills the same position: deposit 500_000, borrow 400_000 (safe at CR 2.0), then CR 3.0.
    w.fund_collateral(k::OWNER_COLL, 500_000);
    w.host(
        vec![
            id(k::OWNER),
            pos,
            host_vault(pos),
            id(k::OWNER_COLL),
            protocol_parameters(),
        ],
        &[k::OWNER, k::OWNER_COLL],
        stablecoin_core::Instruction::DepositCollateral { amount: 500_000 },
    )
    .expect("deposit");
    w.host(
        vec![
            id(k::OWNER),
            pos,
            stable_def(),
            id(k::OWNER_STABLE),
            accumulator(),
            redemption_state(),
            oracle(),
            protocol_parameters(),
            clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        &[k::OWNER],
        stablecoin_core::Instruction::GenerateDebt { amount: 400_000 },
    )
    .expect("borrow");
    w.set_min_cr(3 * ONE);
    w.liquidate(pos, 1).expect("second liquidation");
    assert_eq!(w.read::<LiquidationRecordState>(record(pos)).count, 2);
    assert_eq!(coll(&w).type_reserved, LIQ_TARGET + 440_000);
}

#[test]
fn oversized_auction_blocks_others_and_small_auction_blocks_whale() {
    let cap = CollateralArgs {
        type_cap: 600_000,
        ..CollateralArgs::defaults_for_tests()
    };
    // Direction 1: the oversized (660_000 > 600_000) liquidation goes alone, then blocks the small one.
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
    w.set_collateral_config(k::ENGINE_ADMIN, &cap).unwrap();
    w.liquidate(whale, 0).expect("oversized alone");
    assert_err(w.liquidate(small, 0), "CapacityExhausted");
    // Direction 2: a live small auction (132_000) makes the whale wait (660_000 > 600_000 - 132_000).
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
    w.set_collateral_config(k::ENGINE_ADMIN, &cap).unwrap();
    w.liquidate(small, 0).expect("small fits");
    assert_err(w.liquidate(whale, 0), "CapacityExhausted");
}

#[test]
fn underwater_reward_is_capped_at_the_seizure() {
    // Spec §5.9 / invariant 16: reward_coll = min(floor(6 coin × 250 collateral/coin), c = 1_000) = 1_000.
    let mut w = World::with_engine();
    let pos = w.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 0, 1_000, 1_000);
    w.set_redemption_price(250 * ONE);
    w.set_oracle(250 * ONE, w.now);
    w.liquidate(pos, 0).expect("liquidate underwater");
    let a: AuctionState = w.read(auction(pos, 0));
    assert_eq!((a.keeper_reward, a.collateral_left), (1_000, 0));
    assert_eq!(w.balance(id(k::KEEPER_COLL)), 1_000);
}

#[test]
fn liquidate_rejects_stale_feed_and_admin_pause() {
    // Review Focus 5.
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    w.advance(3_600_001);
    assert_err(w.liquidate(pos, 0), "PriceFeedStale");
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    let paused = CollateralArgs {
        admin_paused: true,
        ..CollateralArgs::defaults_for_tests()
    };
    w.set_collateral_config(k::ENGINE_ADMIN, &paused).unwrap();
    assert_err(w.liquidate(pos, 0), "Paused");
    for (needle, price, confidence) in [
        ("PriceFeedLowConfidence", R, R / 50),
        ("PriceFeedOutOfRange", 3 * R, 0),
    ] {
        let mut w = World::with_engine();
        let pos = w.unsafe_position();
        w.set_oracle_full(price, w.now, confidence);
        assert_err(w.liquidate(pos, 0), needle);
    }
}

#[test]
fn healthy_position_is_rejected_by_the_host() {
    let mut w = World::with_engine();
    let pos = w.open_position(
        k::OWNER,
        k::OWNER_COLL,
        k::OWNER_STABLE,
        0,
        LIQ_COLLATERAL,
        300_000,
    );
    assert_err(w.liquidate(pos, 0), "Position is not liquidatable");
    assert_eq!(coll(&w).type_reserved, 0, "the whole transaction reverted");
}

#[test]
fn keeper_reward_to_fresh_holding() {
    // Review Focus 2: a never-initialised keeper holding works when it signs, fails clearly when not.
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    let accounts = w.liquidate_accounts(pos, 0, id(k::KEEPER_COLL));
    assert_err(
        w.engine(accounts.clone(), &[k::KEEPER], Ix::Liquidate { nonce: 0 }),
        "RecipientMustSign",
    );
    w.engine(
        accounts,
        &[k::KEEPER, k::KEEPER_COLL],
        Ix::Liquidate { nonce: 0 },
    )
    .expect("fresh signed holding");
    assert_eq!(w.balance(id(k::KEEPER_COLL)), LIQ_REWARD);
}

#[test]
fn donation_into_host_vault_is_seized_too() {
    // Review Focus 3: the hook sweeps the whole vault; the lot includes the donation.
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    w.fund_collateral(k::DONOR_COLL, 10);
    w.transfer_tokens(k::DONOR_COLL, host_vault(pos), 10)
        .expect("donation");
    w.liquidate(pos, 0).unwrap();
    let a: AuctionState = w.read(auction(pos, 0));
    assert_eq!(
        (a.collateral_seized, a.collateral_left),
        (LIQ_COLLATERAL + 10, LIQ_LOT + 10)
    );
    assert_eq!(w.balance(auction_vault(pos, 0)), LIQ_LOT + 10);
}

#[test]
fn foreign_account_as_position_is_rejected() {
    // Confused deputy (iii): nothing but the bound host's real position and its own vault reaches the hook.
    let mut w = World::with_engine();
    let pos = w.open_position(
        k::OWNER,
        k::OWNER_COLL,
        k::OWNER_STABLE,
        0,
        LIQ_COLLATERAL,
        LIQ_DEBT,
    );
    let small = w.open_small_position();
    w.set_min_cr(2 * ONE);
    // Not owned by the host.
    assert_err(w.liquidate(accounting(), 0), "HostMismatch");
    // Host-owned but not a position.
    let fake = lee_core::account::AccountId::new([0x99; 32]);
    w.state.force_insert_account(
        fake,
        lee_core::account::Account {
            program_owner: host_program(),
            data: lee_core::account::Data::try_from(vec![1u8, 2, 3]).expect("data"),
            ..lee_core::account::Account::default()
        },
    );
    assert_err(w.liquidate(fake, 0), "WrongAccount");
    // A real position paired with another position's vault (index 10 = host_vault).
    w.holding(k::KEEPER_COLL, collateral_def(), 0);
    let mut accounts = w.liquidate_accounts(pos, 0, id(k::KEEPER_COLL));
    accounts[10] = host_vault(small);
    assert_err(
        w.engine(accounts, &[k::KEEPER], Ix::Liquidate { nonce: 0 }),
        "WrongAccount",
    );
}

#[test]
fn custody_is_this_auctions_own_vault() {
    // Carry-forward 1: the host trusts the engine for the custody destination; a caller cannot
    // substitute another account (its own holding, another nonce's vault) for `["VAULT", auction]`;
    // the guest's PDA attribute rejects it before the handler's own `expect_id` backstop.
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    w.holding(k::KEEPER_COLL, collateral_def(), 0);
    w.holding(k::DONOR_COLL, collateral_def(), 0);
    let mut accounts = w.liquidate_accounts(pos, 0, id(k::KEEPER_COLL));
    accounts[7] = id(k::DONOR_COLL);
    assert_err(
        w.engine(accounts, &[k::KEEPER], Ix::Liquidate { nonce: 0 }),
        "PdaMismatch",
    );
    let mut accounts = w.liquidate_accounts(pos, 0, id(k::KEEPER_COLL));
    accounts[7] = auction_vault(pos, 1);
    assert_err(
        w.engine(accounts, &[k::KEEPER], Ix::Liquidate { nonce: 0 }),
        "PdaMismatch",
    );
    // On success the whole sweep lands in exactly this auction's vault.
    w.liquidate(pos, 0).expect("liquidate");
    assert_eq!(w.balance(auction_vault(pos, 0)), LIQ_LOT);
    assert_eq!(w.balance(id(k::KEEPER_COLL)), LIQ_REWARD);
}

#[test]
fn forwarded_amounts_clear_the_whole_position() {
    // Carry-forward 2: the engine forwards the position's own collateral and normalised debt, so the
    // debt it seizes for is cleared (the host only bounds the maximum seize).
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    let before: Position = w.read(pos);
    assert_eq!(
        (before.collateral_amount, before.normalized_debt_amount),
        (LIQ_COLLATERAL, LIQ_DEBT)
    );
    w.liquidate(pos, 0).unwrap();
    let after: Position = w.read(pos);
    assert_eq!(
        (after.collateral_amount, after.normalized_debt_amount),
        (0, 0)
    );
    assert_eq!(w.balance(host_vault(pos)), 0);
    // A donation changes what the sweep moves, not what is forwarded for the position.
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    w.fund_collateral(k::DONOR_COLL, 7);
    w.transfer_tokens(k::DONOR_COLL, host_vault(pos), 7)
        .unwrap();
    w.liquidate(pos, 0).unwrap();
    let after: Position = w.read(pos);
    assert_eq!(
        (after.collateral_amount, after.normalized_debt_amount),
        (0, 0)
    );
    assert_eq!(w.balance(host_vault(pos)), 0);
}

#[test]
fn lowered_type_cap_below_reserved_blocks_non_alone_liquidation() {
    // Carry-forward 3: an admin lowers type_cap under type_reserved; a non-alone liquidation is
    // rejected with CapacityExhausted (no underflow) and nothing changes.
    let mut w = World::with_engine();
    let small = w.open_small_position();
    let whale = w.open_position(
        k::OWNER,
        k::OWNER_COLL,
        k::OWNER_STABLE,
        0,
        LIQ_COLLATERAL,
        LIQ_DEBT,
    );
    w.set_min_cr(2 * ONE);
    w.liquidate(small, 0)
        .expect("small fits under the default cap");
    assert_eq!(coll(&w).type_reserved, 132_000);
    let lowered = CollateralArgs {
        type_cap: 100_000,
        ..CollateralArgs::defaults_for_tests()
    };
    w.set_collateral_config(k::ENGINE_ADMIN, &lowered)
        .expect("lowering below reserved is allowed");
    assert!(coll(&w).type_reserved > coll(&w).type_cap);
    assert_err(w.liquidate(whale, 0), "CapacityExhausted");
    assert_eq!(coll(&w).type_reserved, 132_000);
}

#[test]
fn liquidate_writes_no_accounting() {
    // Carry-forward 5: Accounting is written only at chain end (ADR-023), never by liquidate.
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    let before = w.state.get_account_by_id(accounting());
    w.liquidate(pos, 0).unwrap();
    assert_eq!(w.state.get_account_by_id(accounting()), before);
}

#[test]
fn unsigned_keeper_is_rejected() {
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    w.holding(k::KEEPER_COLL, collateral_def(), 0);
    let accounts = w.liquidate_accounts(pos, 0, id(k::KEEPER_COLL));
    assert!(w.engine(accounts, &[], Ix::Liquidate { nonce: 0 }).is_err());
}

#[test]
fn debt_below_floor_pays_no_reward_and_skips_the_transfer() {
    // d < debt_floor → reward 0; the unsafe position still liquidates and the keeper holding is untouched.
    let mut w = World::with_engine();
    let floor = CollateralArgs {
        debt_floor: 1_000_000,
        ..CollateralArgs::defaults_for_tests()
    };
    w.set_collateral_config(k::ENGINE_ADMIN, &floor).unwrap();
    let pos = w.unsafe_position();
    w.liquidate(pos, 0).expect("liquidate");
    let a: AuctionState = w.read(auction(pos, 0));
    assert_eq!((a.keeper_reward, a.collateral_left), (0, LIQ_COLLATERAL));
    assert_eq!(w.balance(id(k::KEEPER_COLL)), 0);
}

#[test]
fn rotated_host_oracle_blocks_liquidation_until_the_engine_follows() {
    // ADR-028: the engine re-checks its stored oracle against the live host ProtocolParameters.
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    let new_oracle = lee_core::account::AccountId::new([0x72; 32]);
    let copy = w.state.get_account_by_id(oracle());
    w.state.force_insert_account(new_oracle, copy);
    w.host(
        vec![id(k::HOST_ADMIN), protocol_parameters(), new_oracle],
        &[k::HOST_ADMIN],
        stablecoin_core::Instruction::SetMarketPriceOracle,
    )
    .expect("host rotates its oracle");
    assert_err(w.liquidate(pos, 0), "OracleMismatch");
}

#[test]
fn preexisting_auction_vault_is_rejected() {
    // The auction vault must be fresh (the engine creates it in this transaction).
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    w.state.force_insert_account(
        auction_vault(pos, 0),
        Account {
            program_owner: token_program(),
            data: Data::from(&TokenHolding::Fungible {
                definition_id: collateral_def(),
                balance: 0,
            }),
            ..Account::default()
        },
    );
    assert_err(w.liquidate(pos, 0), "WrongAccount");
}
