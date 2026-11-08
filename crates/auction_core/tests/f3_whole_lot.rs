//! F3 (RFP-014, "a bid may purchase any portion of the remaining lot"): a bid that takes the whole
//! remaining lot, or that reaches the remaining target, is accepted even when it is below `min_bid`
//! and would leave less than `chost` behind. Pins `quote_bid`'s `ends_auction` exemption from
//! `BelowMinBid` and `DustRemainder`. Integration-test file: never compiled into a guest.
use auction_core::{
    auction::{apply_bid, quote_bid, Auction, BidRequest},
    liquidation::LiquidationQuote,
    schedule::rai_default,
    units::RAY,
};

const MIN_BID: u128 = 25;
const CHOST: u128 = 110;

fn quote(lot: u128, debt: u128, target: u128) -> LiquidationQuote {
    LiquidationQuote {
        debt_principal: debt,
        amount_to_raise: target,
        reserve: target,
        keeper_reward: 0,
        lot,
        seized: lot,
    }
}

/// An auction whose remaining target is below both `min_bid` and `chost`: target 990, lot 2_000,
/// and a first bid of 980 coin at price 1 (RAY, 2 % discount at t = 0) that buys 1_000 collateral
/// and leaves 10 coin to raise and 1_000 collateral in the lot.
fn auction_with_small_remainder() -> Auction {
    let a = Auction::open(&quote(2_000, 900, 990), 0, rai_default(), 0);
    let first = quote_bid(
        &a,
        &BidRequest {
            coin_in_max: 980,
            min_collateral_out: 0,
        },
        RAY,
        0,
        MIN_BID,
        0, // chost 0 for the set-up bid only, so it may leave the 10-coin remainder
    )
    .expect("set-up bid");
    let (a, end) = apply_bid(&a, &first);
    assert!(end.is_none(), "set-up bid must not end the auction");
    assert!(a.remaining_target() < MIN_BID && a.remaining_target() < CHOST);
    a
}

#[test]
fn whole_remaining_lot_bid_is_accepted_below_min_bid() {
    // A lot smaller than the remaining coin can buy: the 10 coin left to raise would buy 10
    // collateral (floor of 10 / 0.98), but only 5 remain, so the bid empties the lot for 5 coin
    // and leaves 5 coin unraised (< chost): the lot-empties path, not the target-reaching one.
    let base = auction_with_small_remainder();
    let a = Auction {
        collateral_sold: base.collateral_sold + base.collateral_left - 5,
        collateral_left: 5,
        ..base
    };
    let fill = quote_bid(
        &a,
        &BidRequest {
            coin_in_max: a.remaining_target(),
            min_collateral_out: 0,
        },
        RAY,
        0,
        MIN_BID,
        CHOST,
    )
    .expect("a bid for the whole remaining lot is accepted below min_bid and chost");
    assert_eq!(fill.collateral_out, a.collateral_left);
    assert!(fill.charged < MIN_BID);
    assert!(a.remaining_target() - fill.charged < CHOST);
    assert!(fill.ends_auction);
}

#[test]
fn target_reaching_bid_is_accepted_below_min_bid() {
    let a = auction_with_small_remainder();
    let remaining = a.remaining_target();
    assert!(remaining < MIN_BID);
    let fill = quote_bid(
        &a,
        &BidRequest {
            coin_in_max: remaining,
            min_collateral_out: 0,
        },
        RAY,
        0,
        MIN_BID,
        CHOST,
    )
    .expect("a target-reaching bid is accepted below min_bid");
    assert_eq!(fill.charged, remaining);
    assert!(fill.ends_auction);
}
