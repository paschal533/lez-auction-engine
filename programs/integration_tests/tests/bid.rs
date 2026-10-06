mod common;

use auction_engine_core::{
    args::CollateralArgs,
    state::{AccountingState, AuctionState, CollateralConfigState, EngineConfigState},
    Instruction as Ix,
};
use common::*;

fn auction_of(w: &World, pos: lee_core::account::AccountId) -> AuctionState {
    w.read(auction(pos, 0))
}

#[test]
fn partial_bid_burns_debt_first_and_leaves_accounting_alone() {
    let (mut w, pos) = liquidated_world();
    let supply_before = w.supply(stable_def());
    w.bid(pos, 100_000, 51_020).expect("partial bid");
    // floor(100_000 × 0.5 / 0.98) = 51_020; the whole charge is debt (debt first).
    let a = auction_of(&w, pos);
    assert_eq!(
        (
            a.debt_raised,
            a.penalty_raised,
            a.collateral_left,
            a.settled
        ),
        (100_000, 0, LIQ_LOT - 51_020, false)
    );
    assert_eq!(w.balance(id(k::BIDDER_COLL)), 51_020);
    assert_eq!(w.balance(id(k::BIDDER_STABLE)), 900_000);
    assert_eq!(w.supply(stable_def()), supply_before - 100_000);
    assert_eq!(w.balance(buffer()), 0);
    assert_eq!(
        w.read::<AccountingState>(accounting()),
        AccountingState::new(),
        "S6: no per-bid Accounting"
    );
}

#[test]
fn final_bid_reaches_target_and_settles_once() {
    let (mut w, pos) = liquidated_world();
    w.bid(pos, 100_000, 0).unwrap();
    w.bid_settle(pos, 100_000, 0)
        .expect("non-final bid_with_settlement is fine");
    assert_eq!(
        w.read::<AccountingState>(accounting()),
        AccountingState::new()
    );
    w.bid_settle(pos, 460_000, 0).expect("final bid");
    let a = auction_of(&w, pos);
    assert!(a.settled);
    assert_eq!((a.debt_raised, a.penalty_raised), (LIQ_DEBT, 60_000));
    assert_eq!(a.leftover_claimable, a.collateral_left);
    let acc: AccountingState = w.read(accounting());
    assert_eq!(
        (
            acc.total_debt_burned,
            acc.total_penalty_collected,
            acc.unbacked_debt
        ),
        (LIQ_DEBT, 60_000, 0)
    );
    assert_eq!(w.balance(buffer()), 60_000);
    assert_eq!(
        w.read::<CollateralConfigState>(coll_config()).type_reserved,
        0
    );
    assert_eq!(w.read::<EngineConfigState>(config()).global_reserved, 0);
    // A settled auction never settles again: later bids are rejected and Accounting stays as booked.
    assert_err(w.bid_settle(pos, 1_000, 0), "NotAuctioning");
    assert_err(w.bid(pos, 1_000, 0), "NotAuctioning");
    assert_eq!(w.read::<AccountingState>(accounting()), acc);
    assert_eq!(w.balance(buffer()), 60_000);
}

#[test]
fn shortfall_is_booked_once_and_a_settled_lot_binding_auction_rejects_bids() {
    let (mut w, pos) = underwater_world();
    w.bid_settle(pos, LIQ_TARGET, 0).expect("lot-binding bid");
    let booked: AccountingState = w.read(accounting());
    assert_eq!(booked.total_shortfall_booked, 355_049);
    assert_err(w.bid_settle(pos, LIQ_TARGET, 0), "NotAuctioning");
    assert_eq!(w.read::<AccountingState>(accounting()), booked);
}

#[test]
fn bid_that_turns_final_without_accounting_reverts() {
    // Review Focus 1 / ADR-029.
    let (mut w, pos) = liquidated_world();
    let before_auction = auction_of(&w, pos);
    let before_coll: CollateralConfigState = w.read(coll_config());
    assert_err(w.bid(pos, LIQ_TARGET, 0), "ChainEndAccountsMissing");
    assert_eq!(auction_of(&w, pos), before_auction, "nothing applied");
    assert_eq!(w.read::<CollateralConfigState>(coll_config()), before_coll);
    assert_eq!(
        w.balance(id(k::BIDDER_STABLE)),
        1_000_000,
        "nothing charged"
    );
    assert_eq!(w.balance(id(k::BIDDER_COLL)), 0);
    w.bid_settle(pos, LIQ_TARGET, 0)
        .expect("same bid with the chain-end account");
    assert!(auction_of(&w, pos).settled);
    let acc: AccountingState = w.read(accounting());
    assert_eq!(
        (acc.total_debt_burned, acc.total_penalty_collected),
        (LIQ_DEBT, 60_000),
        "folded exactly once"
    );
}

#[test]
fn max_coin_in_charges_only_remaining() {
    // Review Focus 4.
    let (mut w, pos) = liquidated_world();
    w.bid_settle(pos, u128::MAX, 0)
        .expect("u128::MAX coin_in_max");
    assert_eq!(w.balance(id(k::BIDDER_STABLE)), 1_000_000 - LIQ_TARGET);
    assert!(auction_of(&w, pos).settled);
}

#[test]
fn lot_binds_charges_only_for_the_lot_and_books_the_shortfall() {
    let (mut w, pos) = underwater_world();
    w.bid_settle(pos, LIQ_TARGET, 0).expect("lot-binding bid");
    let a = auction_of(&w, pos);
    // charged = ceil(499_900 × 0.98 / 2) = 244_951; shortfall = 600_000 − 244_951.
    assert_eq!(
        (
            a.debt_raised,
            a.collateral_left,
            a.leftover_claimable,
            a.settled
        ),
        (244_951, 0, 0, true)
    );
    assert_eq!(w.balance(id(k::BIDDER_STABLE)), 1_000_000 - 244_951);
    assert_eq!(w.balance(id(k::BIDDER_COLL)), 499_900);
    let acc: AccountingState = w.read(accounting());
    assert_eq!(
        (acc.unbacked_debt, acc.total_shortfall_booked),
        (355_049, 355_049)
    );
    assert_eq!(
        w.read::<CollateralConfigState>(coll_config()).type_reserved,
        0
    );
    assert_eq!(w.read::<EngineConfigState>(config()).global_reserved, 0);
}

#[test]
fn lot_emptying_bid_sweeps_donation() {
    // Review Focus 3 / ADR-033.
    let (mut w, pos) = underwater_world();
    w.fund_collateral(k::DONOR_COLL, 7);
    w.transfer_tokens(k::DONOR_COLL, auction_vault(pos, 0), 7)
        .unwrap();
    w.bid_settle(pos, LIQ_TARGET, 0).unwrap();
    assert_eq!(w.balance(id(k::BIDDER_COLL)), 499_907);
    assert_eq!(w.balance(auction_vault(pos, 0)), 0);
}

#[test]
fn donation_never_blocks_a_partial_bid_and_is_swept_by_the_lot_emptying_one() {
    let (mut w, pos) = underwater_world();
    w.fund_collateral(k::DONOR_COLL, 7);
    w.transfer_tokens(k::DONOR_COLL, auction_vault(pos, 0), 7)
        .unwrap();
    // R = 2.0: floor(100_000 × 2 / 0.98) = 204_081 booked; the donation stays in the vault.
    w.bid(pos, 100_000, 204_081).expect("partial bid");
    assert_eq!(w.balance(id(k::BIDDER_COLL)), 204_081);
    assert_eq!(w.balance(auction_vault(pos, 0)), 499_900 - 204_081 + 7);
    w.bid_settle(pos, LIQ_TARGET, 0).expect("lot-emptying bid");
    assert_eq!(w.balance(id(k::BIDDER_COLL)), 499_907);
    assert_eq!(w.balance(auction_vault(pos, 0)), 0);
}

#[test]
fn target_reaching_bid_pays_the_booked_amount_and_leaves_donation_for_the_claim() {
    let (mut w, pos) = liquidated_world();
    w.fund_collateral(k::DONOR_COLL, 7);
    w.transfer_tokens(k::DONOR_COLL, auction_vault(pos, 0), 7)
        .unwrap();
    w.bid_settle(pos, LIQ_TARGET, 0).unwrap();
    // floor(660_000 × 0.5 / 0.98) = 336_734; the lot does not empty, so no sweep.
    assert_eq!(w.balance(id(k::BIDDER_COLL)), 336_734);
    assert_eq!(w.balance(auction_vault(pos, 0)), LIQ_LOT - 336_734 + 7);
}

#[test]
fn dust_min_bid_and_slippage_are_rejected() {
    let (mut w, pos) = liquidated_world();
    assert_err(w.bid(pos, 10, 0), "BelowMinBid");
    assert_err(w.bid(pos, LIQ_TARGET - 50, 0), "DustRemainder");
    assert_err(w.bid(pos, 100_000, 51_021), "Slippage");
    let a = auction_of(&w, pos);
    assert_eq!((a.debt_raised, a.collateral_left), (0, LIQ_LOT));
    assert_eq!(w.balance(id(k::BIDDER_STABLE)), 1_000_000);
}

#[test]
fn fresh_bidder_holding_must_sign() {
    // Review Focus 2.
    let (mut w, pos) = liquidated_world();
    w.fund_stable(k::BIDDER2_STABLE, 200_000);
    let accounts = w.bid_accounts(pos, 0, k::BIDDER2_STABLE, k::BIDDER2_COLL, false);
    let ix = || Ix::Bid {
        coin_in_max: 100_000,
        min_collateral_out: 0,
    };
    assert_err(
        w.engine(accounts.clone(), &[k::BIDDER2_STABLE], ix()),
        "RecipientMustSign",
    );
    w.engine(accounts, &[k::BIDDER2_STABLE, k::BIDDER2_COLL], ix())
        .expect("fresh holding signs");
    assert_eq!(w.balance(id(k::BIDDER2_COLL)), 51_020);
}

#[test]
fn holding_of_wrong_definition_is_rejected() {
    let (mut w, pos) = liquidated_world();
    w.holding(k::BIDDER2_COLL, stable_def(), 0);
    let accounts = w.bid_accounts(pos, 0, k::BIDDER_STABLE, k::BIDDER2_COLL, false);
    assert_err(
        w.engine(
            accounts,
            &[k::BIDDER_STABLE],
            Ix::Bid {
                coin_in_max: 100_000,
                min_collateral_out: 0,
            },
        ),
        "WrongDefinition",
    );
}

#[test]
fn unsigned_bidder_stable_holding_is_rejected() {
    let (mut w, pos) = liquidated_world();
    let accounts = w.bid_accounts(pos, 0, k::BIDDER_STABLE, k::BIDDER_COLL, false);
    // An otherwise-valid signer signs; the bidder's stablecoin holding does not.
    assert_err(
        w.engine(
            accounts,
            &[k::CALLER],
            Ix::Bid {
                coin_in_max: 100_000,
                min_collateral_out: 0,
            },
        ),
        "must be a signer",
    );
    assert_eq!(w.balance(id(k::BIDDER_STABLE)), 1_000_000);
}

#[test]
fn bidder_cannot_pay_more_than_the_holding_balance() {
    let (mut w, pos) = liquidated_world();
    w.holding(k::BIDDER_STABLE, stable_def(), 50_000);
    assert_err(w.bid(pos, 100_000, 0), "InsufficientBalance");
}

#[test]
fn discount_frozen_while_paused() {
    // Review Focus 5 / invariant 14: 1_000_000 ms of admin pause add no discount.
    let (mut w, pos) = liquidated_world();
    w.advance(1_350_000);
    let paused = CollateralArgs {
        admin_paused: true,
        ..CollateralArgs::defaults_for_tests()
    };
    w.set_collateral_config(k::ENGINE_ADMIN, &paused).unwrap();
    w.advance(1_000_000);
    assert_err(w.bid(pos, 100_000, 0), "Paused");
    w.set_collateral_config(k::ENGINE_ADMIN, &CollateralArgs::defaults_for_tests())
        .unwrap();
    w.bid(pos, 100_000, 0).unwrap();
    // t_eff = 1_350_000 → 6% → floor(100_000 × 0.5 / 0.94) = 53_191 (not 54_920 at 8.96%).
    assert_eq!(w.balance(id(k::BIDDER_COLL)), 53_191);
}

#[test]
fn bid_rejects_invalid_feed() {
    // Review Focus 5 (I6): stale, low-confidence and out-of-range feeds all stop bids; nothing is applied.
    for (needle, case) in [
        ("PriceFeedStale", 0u8),
        ("PriceFeedLowConfidence", 1),
        ("PriceFeedOutOfRange", 2),
    ] {
        let (mut w, pos) = liquidated_world();
        match case {
            0 => w.advance(3_600_001),
            1 => {
                w.advance(1_000);
                w.set_oracle_full(R, w.now, R / 50);
            }
            _ => {
                w.advance(1_000);
                w.set_oracle(3 * R, w.now);
            }
        }
        assert_err(w.bid_settle(pos, 100_000, 0), needle);
        let a = auction_of(&w, pos);
        assert_eq!(
            (a.debt_raised, a.collateral_left),
            (0, LIQ_LOT),
            "{needle}: auction unchanged"
        );
        assert_eq!(
            w.balance(id(k::BIDDER_STABLE)),
            1_000_000,
            "{needle}: nothing charged"
        );
    }
}

#[test]
fn unsold_auction_holds_max_discount_and_keeps_its_reservation() {
    let (mut w, pos) = liquidated_world();
    w.advance(36_000_000);
    w.set_oracle(R, w.now);
    w.bid(pos, 100_000, 0).unwrap();
    // No expiry: 10% max discount → floor(100_000 × 0.5 / 0.9) = 55_555.
    assert_eq!(w.balance(id(k::BIDDER_COLL)), 55_555);
    assert_eq!(
        w.read::<CollateralConfigState>(coll_config()).type_reserved,
        LIQ_TARGET
    );
    assert!(!auction_of(&w, pos).settled);
}

#[test]
fn schedule_change_affects_only_new_auctions() {
    let (mut w, pos) = liquidated_world();
    let flat = CollateralArgs {
        schedule: vec![0, 3_000],
        ..CollateralArgs::defaults_for_tests()
    };
    w.set_collateral_config(k::ENGINE_ADMIN, &flat).unwrap();
    w.bid(pos, 100_000, 0).unwrap();
    assert_eq!(
        w.balance(id(k::BIDDER_COLL)),
        51_020,
        "snapshot 2%, not 30%"
    );
}
