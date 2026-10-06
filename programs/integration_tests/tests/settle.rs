mod common;

use auction_engine_core::state::{
    AccountingState, AuctionState, CollateralConfigState, EngineConfigState,
};
use common::*;
use lee_core::account::{Account, AccountId, Data};

/// Reward consumes the whole seizure (Task 10 underwater case): lot 0, debt 1_000.
fn empty_lot_world() -> (World, AccountId) {
    let mut w = World::with_engine();
    let pos = w.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 0, 1_000, 1_000);
    w.set_redemption_price(250 * ONE);
    w.set_oracle(250 * ONE, w.now);
    w.liquidate(pos, 0).unwrap();
    w.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 10_000);
    (w, pos)
}

#[test]
fn settle_empty_books_the_full_shortfall_and_releases_reservations() {
    let (mut w, pos) = empty_lot_world();
    assert_eq!(
        w.read::<CollateralConfigState>(coll_config()).type_reserved,
        1_100,
        "the lot-0 auction still holds its reservation"
    );
    assert_err(w.bid_settle(pos, 1_100, 0), "ZeroFill");
    w.settle_empty(pos).expect("settle_empty");
    let a: AuctionState = w.read(auction(pos, 0));
    assert!(a.settled);
    let acc: AccountingState = w.read(accounting());
    assert_eq!(
        (
            acc.unbacked_debt,
            acc.total_shortfall_booked,
            acc.total_debt_burned
        ),
        (1_000, 1_000, 0)
    );
    assert_eq!(
        w.read::<CollateralConfigState>(coll_config()).type_reserved,
        0
    );
    assert_eq!(w.read::<EngineConfigState>(config()).global_reserved, 0);
}

#[test]
fn settle_empty_applies_once_and_blocks_further_bids() {
    let (mut w, pos) = empty_lot_world();
    w.settle_empty(pos).expect("settle_empty");
    let acc: AccountingState = w.read(accounting());
    assert_err(w.settle_empty(pos), "NotAuctioning");
    assert_err(w.bid_settle(pos, 1_100, 0), "NotAuctioning");
    assert_eq!(
        w.read::<AccountingState>(accounting()),
        acc,
        "no second fold"
    );
}

#[test]
fn settle_empty_on_a_live_lot_is_rejected() {
    let (mut w, pos) = liquidated_world();
    assert_err(w.settle_empty(pos), "NotAuctioning");
    assert_eq!(
        w.read::<AccountingState>(accounting()),
        AccountingState::new()
    );
}

#[test]
fn claim_leftover_pays_the_owner_once_and_sweeps() {
    let (mut w, pos) = liquidated_world();
    w.bid_settle(pos, LIQ_TARGET, 0).unwrap();
    // out = floor(660_000 x 0.5 / 0.98) = 336_734 -> leftover = 499_975 - 336_734 = 163_241.
    w.fund_collateral(k::DONOR_COLL, 3);
    w.transfer_tokens(k::DONOR_COLL, auction_vault(pos, 0), 3)
        .unwrap();
    assert_err(
        w.claim_leftover(k::ATTACKER, k::OWNER_COLL, pos),
        "NotOwner",
    );
    let before = w.balance(id(k::OWNER_COLL));
    w.claim_leftover(k::OWNER, k::OWNER_COLL, pos)
        .expect("owner claims");
    assert_eq!(w.balance(id(k::OWNER_COLL)), before + 163_244);
    assert_eq!(w.balance(auction_vault(pos, 0)), 0);
    let a: AuctionState = w.read(auction(pos, 0));
    assert_eq!(a.leftover_claimed, a.leftover_claimable);
    assert_err(
        w.claim_leftover(k::OWNER, k::OWNER_COLL, pos),
        "NothingToClaim",
    );
}

#[test]
fn claim_leftover_needs_a_settled_auction_with_leftover() {
    let (mut w, pos) = liquidated_world();
    assert_err(
        w.claim_leftover(k::OWNER, k::OWNER_COLL, pos),
        "NothingToClaim",
    );
    let (mut u, upos) = underwater_world();
    u.bid_settle(upos, LIQ_TARGET, 0).unwrap();
    assert_eq!(
        u.read::<AuctionState>(auction(upos, 0)).leftover_claimable,
        0
    );
    assert_err(
        u.claim_leftover(k::OWNER, k::OWNER_COLL, upos),
        "NothingToClaim",
    );
}

#[test]
fn claim_leftover_to_a_fresh_holding_must_sign() {
    let (mut w, pos) = liquidated_world();
    w.bid_settle(pos, LIQ_TARGET, 0).unwrap();
    assert_eq!(
        w.state.get_account_by_id(id(k::OWNER2_COLL)),
        Account::default()
    );
    assert_err(
        w.claim_leftover(k::OWNER, k::OWNER2_COLL, pos),
        "RecipientMustSign",
    );
    w.claim_leftover_signed(k::OWNER, k::OWNER2_COLL, pos, &[k::OWNER, k::OWNER2_COLL])
        .expect("fresh holding that signs");
    assert_eq!(w.balance(id(k::OWNER2_COLL)), 163_241);
}

#[test]
fn settle_debt_uses_actual_buffer_balance() {
    // Review Focus 3 + S7 prerequisite: burn from an engine-PDA-held holding with seeds.
    let (mut w, pos) = underwater_world();
    w.bid_settle(pos, LIQ_TARGET, 0).unwrap();
    assert_eq!(
        w.read::<AccountingState>(accounting()).unbacked_debt,
        355_049
    );
    w.settle_debt(u128::MAX)
        .expect("nothing to burn is a no-op");
    assert_eq!(
        w.read::<AccountingState>(accounting()).unbacked_debt,
        355_049
    );
    w.donate_stable(buffer(), 1_000);
    let supply = w.supply(stable_def());
    w.settle_debt(500).expect("partial settle");
    assert_eq!(w.balance(buffer()), 500);
    assert_eq!(w.supply(stable_def()), supply - 500);
    let acc: AccountingState = w.read(accounting());
    assert_eq!((acc.unbacked_debt, acc.total_debt_settled), (354_549, 500));
}

#[test]
fn settle_debt_max_amount() {
    // Review Focus 4: u128::MAX burns min(buffer, unbacked - on_auction) and nothing more.
    let (mut w, pos) = underwater_world();
    w.bid_settle(pos, LIQ_TARGET, 0).unwrap();
    w.donate_stable(buffer(), 1_000);
    w.settle_debt(u128::MAX).unwrap();
    assert_eq!(w.balance(buffer()), 0);
    assert_eq!(
        w.read::<AccountingState>(accounting()).unbacked_debt,
        354_049
    );
}

#[test]
fn settle_debt_never_burns_more_than_the_unbacked_debt() {
    let (mut w, pos) = underwater_world();
    w.bid_settle(pos, LIQ_TARGET, 0).unwrap();
    w.donate_stable(buffer(), 400_000);
    w.settle_debt(u128::MAX).unwrap();
    assert_eq!(w.balance(buffer()), 400_000 - 355_049);
    let acc: AccountingState = w.read(accounting());
    assert_eq!((acc.unbacked_debt, acc.total_debt_settled), (0, 355_049));
}

#[test]
fn settle_debt_excludes_the_debt_a_live_debt_auction_sells() {
    // ADR-024 reservation: no engine path sets debt_on_auction before Task 14, so write it directly.
    let (mut w, pos) = underwater_world();
    w.bid_settle(pos, LIQ_TARGET, 0).unwrap();
    let mut account = w.state.get_account_by_id(accounting());
    let mut acc: AccountingState = w.read(accounting());
    acc.debt_on_auction = acc.unbacked_debt - 100;
    account.data = Data::from(&acc);
    w.state.force_insert_account(accounting(), account);
    w.donate_stable(buffer(), 1_000);
    w.settle_debt(u128::MAX).unwrap();
    assert_eq!(w.balance(buffer()), 900);
    let after: AccountingState = w.read(accounting());
    assert_eq!(
        (after.unbacked_debt, after.debt_on_auction),
        (acc.unbacked_debt - 100, acc.debt_on_auction)
    );
}
