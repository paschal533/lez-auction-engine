mod common;

use auction_engine_core::{
    args::{CollateralArgs, EngineParamsArgs, DEBT_RELIST_DELAY_MS},
    state::{AccountingState, DebtAuctionState},
};
use common::*;
use lee_core::account::AccountId;

const TAKE_ALL: u128 = u128::MAX;

fn acc(w: &World) -> AccountingState {
    let a: AccountingState = w.read(accounting());
    // ADR-024: a reservation can never exceed the debt it reserves, after every step.
    assert!(
        a.debt_on_auction <= a.unbacked_debt,
        "debt_on_auction {} > unbacked_debt {}",
        a.debt_on_auction,
        a.unbacked_debt
    );
    a
}

/// Protocol token configured; the mock token is created with `authority` (or not at all); shortfall
/// booked (unbacked 355_049); the taker holds 1_000 stablecoin.
fn debt_world(authority: Option<AccountId>, params: EngineParamsArgs) -> World {
    let mut w = World::new();
    // ADR-038: an enabled protocol token must already be a token definition when the config names it.
    w.create_protocol_token(10_000, authority.unwrap_or_else(mint_authority))
        .unwrap();
    w.init_engine_config(&params).unwrap();
    w.bind_engine().unwrap();
    w.set_collateral_config(k::ENGINE_ADMIN, &CollateralArgs::defaults_for_tests())
        .unwrap();
    if authority.is_none() {
        // Defensive path only: a validated slot cannot stop being a token definition on-chain, so the
        // use-time ProtocolTokenMismatch is reached by force-inserting an empty account over it.
        w.state
            .force_insert_account(protocol_token_def(), lee_core::account::Account::default());
    }
    w.book_shortfall();
    w.fund_stable(k::TAKER_STABLE, 1_000);
    w
}

fn token_params() -> EngineParamsArgs {
    EngineParamsArgs {
        protocol_token_enabled: true,
        ..EngineParamsArgs::defaults_for_tests()
    }
}

/// Reference price 1/9 coin per token: the whole lot starts at 4_500 tokens (cap 5_000).
fn cheap_token_params() -> EngineParamsArgs {
    EngineParamsArgs {
        protocol_token_ref_price: ONE / 9,
        ..token_params()
    }
}

fn take(w: &mut World, coin: u128, min_out: u128) -> Result<(), lee::error::LeeError> {
    w.take_debt(
        0,
        k::TAKER_STABLE,
        k::TAKER_PROTOCOL,
        &[k::TAKER_STABLE],
        coin,
        min_out,
    )
}

#[test]
fn debt_auction_mints_against_burned_coin() {
    let mut w = debt_world(Some(mint_authority()), token_params());
    let stable_supply = w.supply(stable_def());
    w.start_debt(0).expect("start_debt_auction");
    let a = acc(&w);
    assert_eq!(
        (a.debt_on_auction, a.debt_live, a.debt_nonce),
        (500, true, 1)
    );
    // start_amount = floor(500 / 2) = 250 tokens for the whole 500-coin lot at t = 0.
    take(&mut w, 500, 250).expect("take_debt");
    assert_eq!(w.balance(id(k::TAKER_PROTOCOL)), 10_250);
    assert_eq!(w.supply(protocol_token_def()), 10_250);
    assert_eq!(w.balance(id(k::TAKER_STABLE)), 500);
    assert_eq!(w.supply(stable_def()), stable_supply - 500);
    let a = acc(&w);
    assert_eq!(
        (a.unbacked_debt, a.debt_on_auction, a.total_debt_auctioned),
        (355_049 - 500, 0, 500)
    );
    assert_eq!((a.total_protocol_minted, a.debt_live), (250, false));
    assert!(!w.read::<DebtAuctionState>(debt_auction(0)).live);
}

#[test]
fn take_debt_mints_into_fresh_holding() {
    // Review Focus 2 + S7 prerequisite: mint into a taker's never-initialised holding.
    let mut w = debt_world(Some(mint_authority()), token_params());
    w.start_debt(0).unwrap();
    assert_err(
        w.take_debt(
            0,
            k::TAKER_STABLE,
            TAKER2_PROTOCOL,
            &[k::TAKER_STABLE],
            500,
            0,
        ),
        "RecipientMustSign",
    );
    assert_eq!(
        acc(&w).debt_on_auction,
        500,
        "a failed take changes nothing"
    );
    w.take_debt(
        0,
        k::TAKER_STABLE,
        TAKER2_PROTOCOL,
        &[k::TAKER_STABLE, TAKER2_PROTOCOL],
        500,
        0,
    )
    .expect("fresh signed holding");
    assert_eq!(w.balance(id(TAKER2_PROTOCOL)), 250);
}

#[test]
fn take_debt_max_amount_fills_lot() {
    // Review Focus 4: only the filled 500 coin is burned and recorded.
    let mut w = debt_world(Some(mint_authority()), token_params());
    let unbacked = acc(&w).unbacked_debt;
    w.start_debt(0).unwrap();
    take(&mut w, TAKE_ALL, 0).unwrap();
    assert_eq!(w.balance(id(k::TAKER_STABLE)), 500);
    let a = acc(&w);
    assert_eq!(
        (a.unbacked_debt, a.total_debt_auctioned, a.debt_on_auction),
        (unbacked - 500, 500, 0)
    );
    assert_eq!(w.balance(id(k::TAKER_PROTOCOL)), 10_250);
}

#[test]
fn partial_take_then_close_releases_the_reservation() {
    let mut w = debt_world(Some(mint_authority()), token_params());
    w.start_debt(0).unwrap();
    take(&mut w, 300, 150).unwrap();
    assert_eq!(acc(&w).debt_on_auction, 200);
    assert_eq!(acc(&w).unbacked_debt, 355_049 - 300);
    assert_err(w.close_debt(0), "DelayNotElapsed");
    w.advance(token_params().system_ttl_ms);
    w.close_debt(0).expect("close");
    let a = acc(&w);
    assert_eq!((a.debt_on_auction, a.debt_live), (0, false));
    assert_eq!(a.unbacked_debt, 355_049 - 300, "close burns nothing");
    assert!(!w.read::<DebtAuctionState>(debt_auction(0)).live);
    assert_err(w.close_debt(0), "NotLive");
    assert_err(take(&mut w, 200, 0), "NotLive");
}

#[test]
fn start_rejections_map_to_the_freeze_list() {
    let mut w = debt_world(Some(id(k::ATTACKER)), token_params());
    assert_err(w.start_debt(0), "MintAuthorityMissing");
    let mut w = debt_world(None, token_params());
    assert_err(w.start_debt(0), "ProtocolTokenMismatch");
    let mut w = debt_world(
        Some(mint_authority()),
        EngineParamsArgs {
            protocol_token_ref_price: ONE / 100,
            ..token_params()
        },
    );
    assert_err(w.start_debt(0), "MintCapConfig");
    let mut w = World::with_system(false);
    assert_err(w.start_debt(0), "InsufficientBadDebt");
}

#[test]
fn start_needs_the_next_nonce_and_a_single_live_auction() {
    let mut w = debt_world(Some(mint_authority()), token_params());
    assert_err(w.start_debt(1), "NonceMismatch");
    w.start_debt(0).unwrap();
    assert_err(w.start_debt(1), "DebtAuctionAlreadyLive");
    assert_err(w.start_debt(0), "AccountAlreadyInitialized");
    assert_eq!(acc(&w).debt_on_auction, 500, "only one reservation");
    take(&mut w, TAKE_ALL, 0).unwrap();
    w.advance(DEBT_RELIST_DELAY_MS); // ADR-039 relist delay
    w.start_debt(1)
        .expect("a new auction after the first ended");
    assert_eq!((acc(&w).debt_on_auction, acc(&w).debt_nonce), (500, 2));
}

#[test]
fn start_is_blocked_when_the_buffer_covers_the_debt() {
    let mut w = debt_world(Some(mint_authority()), token_params());
    w.donate_stable(buffer(), 400_000);
    assert_err(w.start_debt(0), "InsufficientBadDebt");
    assert_eq!(
        acc(&w).unbacked_debt,
        355_049,
        "a failed start settles nothing"
    );
}

#[test]
fn start_settles_implicitly_before_checking_eligibility() {
    let mut w = debt_world(Some(mint_authority()), token_params());
    let before = acc(&w);
    // The buffer is topped up to cover all but 600 of the debt; the implicit settle burns it,
    // then 600 >= debt_lot (500) still allows the start.
    let held = w.balance(buffer());
    w.donate_stable(buffer(), 355_049 - 600 - held);
    w.start_debt(0).expect("start after implicit settle");
    let a = acc(&w);
    assert_eq!((a.unbacked_debt, a.debt_on_auction), (600, 500));
    assert_eq!(
        a.total_debt_settled,
        before.total_debt_settled + 355_049 - 600
    );
    assert_eq!(w.balance(buffer()), 0);
}

#[test]
fn settle_debt_excludes_the_reservation_of_a_live_debt_auction() {
    // Task 12 review carry-forward: `debt_on_auction` is set by a REAL start, not force-written.
    let mut w = debt_world(Some(mint_authority()), token_params());
    w.start_debt(0).unwrap();
    w.donate_stable(buffer(), 400_000);
    w.settle_debt(u128::MAX).unwrap();
    let a = acc(&w);
    assert_eq!(
        (a.unbacked_debt, a.debt_on_auction),
        (500, 500),
        "settle_debt burns only unbacked - debt_on_auction"
    );
    assert!(w.balance(buffer()) > 0, "the rest of the buffer stays");
    // A full take burns the reserved coin; nothing is left to settle.
    take(&mut w, 500, 0).unwrap();
    assert_eq!((acc(&w).unbacked_debt, acc(&w).debt_on_auction), (0, 0));
}

#[test]
fn the_reserved_amount_becomes_settleable_again_after_close() {
    let mut w = debt_world(Some(mint_authority()), token_params());
    w.start_debt(0).unwrap();
    w.donate_stable(buffer(), 400_000);
    w.settle_debt(u128::MAX).unwrap();
    take(&mut w, 300, 0).unwrap();
    assert_eq!((acc(&w).unbacked_debt, acc(&w).debt_on_auction), (200, 200));
    w.settle_debt(u128::MAX).unwrap();
    assert_eq!(acc(&w).unbacked_debt, 200, "still reserved");
    w.advance(token_params().system_ttl_ms);
    w.close_debt(0).unwrap();
    assert_eq!((acc(&w).unbacked_debt, acc(&w).debt_on_auction), (200, 0));
    w.settle_debt(u128::MAX).unwrap();
    assert_eq!(
        acc(&w).unbacked_debt,
        0,
        "the released 200 is settled from the buffer"
    );
}

#[test]
fn mint_never_exceeds_the_absolute_cap() {
    // start = floor(500 x 9) = 4_500 <= 5_000; growth 30% would offer 5_850 -> capped at 5_000.
    let mut w = debt_world(Some(mint_authority()), cheap_token_params());
    w.start_debt(0).unwrap();
    w.advance(1_000);
    take(&mut w, 500, 0).unwrap();
    assert_eq!(acc(&w).total_protocol_minted, 5_000);
    assert_eq!(w.supply(protocol_token_def()), 15_000);
}

#[test]
fn multi_take_over_time_never_mints_more_than_the_cap() {
    let mut w = debt_world(Some(mint_authority()), cheap_token_params());
    w.start_debt(0).unwrap();
    // t = 0: floor(4_500 x 100 / 500) = 900.
    take(&mut w, 100, 0).unwrap();
    assert_eq!(acc(&w).total_protocol_minted, 900);
    // t = 1_000 ms: offered = min(5_850, 5_000); floor(5_000 x 400 / 500) = 4_000.
    w.advance(1_000);
    take(&mut w, 400, 0).unwrap();
    let a = acc(&w);
    assert_eq!(a.total_protocol_minted, 4_900);
    assert!(a.total_protocol_minted <= token_params().max_mint_per_auction);
    assert_eq!(w.supply(protocol_token_def()), 10_000 + 4_900);
    assert_eq!(w.balance(id(k::TAKER_PROTOCOL)), 10_000 + 4_900);
}

#[test]
fn take_debt_guards() {
    let mut w = debt_world(Some(mint_authority()), token_params());
    w.start_debt(0).unwrap();
    assert_err(take(&mut w, 500, 251), "Slippage");
    assert_err(take(&mut w, 495, 0), "DustRemainder");
    assert_err(take(&mut w, 0, 0), "ZeroFill");
    w.holding(k::TAKER_STABLE, stable_def(), 100);
    assert_err(take(&mut w, 500, 0), "InsufficientBalance");
    assert_err(
        w.take_debt(0, k::TAKER_STABLE, k::TAKER_PROTOCOL, &[], 100, 0),
        "must be a signer",
    );
    assert_eq!(acc(&w).debt_on_auction, 500, "failed takes change nothing");
}

#[test]
fn protocol_token_locked_while_debt_auction_live() {
    let mut w = debt_world(Some(mint_authority()), token_params());
    w.start_debt(0).unwrap();
    let off = EngineParamsArgs {
        protocol_token_enabled: false,
        ..token_params()
    };
    assert_err(
        w.set_engine_params(k::ENGINE_ADMIN, &off),
        "ProtocolTokenLocked",
    );
    let new_price = EngineParamsArgs {
        protocol_token_ref_price: 3 * ONE,
        ..token_params()
    };
    w.set_engine_params(k::ENGINE_ADMIN, &new_price)
        .expect("ref price may change: auctions snapshot it");
    // The live auction is unaffected: its start amount was snapshotted.
    take(&mut w, 500, 250).unwrap();
    w.set_engine_params(k::ENGINE_ADMIN, &off)
        .expect("unlocked once the auction ended");
}

#[test]
fn a_closed_debt_auction_cannot_be_relisted_before_the_delay_so_the_admin_can_swap_the_token() {
    let mut w = debt_world(Some(mint_authority()), token_params());
    w.start_debt(0).unwrap();
    w.advance(token_params().system_ttl_ms);
    w.close_debt(0).expect("anyone closes after the ttl");
    assert_err(w.start_debt(1), "DelayNotElapsed");
    let off = EngineParamsArgs {
        protocol_token_enabled: false,
        ..token_params()
    };
    w.set_engine_params(k::ENGINE_ADMIN, &off)
        .expect("no debt auction is live during the relist delay: the admin may change the token");
    w.advance(DEBT_RELIST_DELAY_MS);
    assert_err(w.start_debt(1), "TokenNotConfigured");
}

#[test]
fn a_fully_taken_debt_auction_also_starts_the_relist_delay() {
    let mut w = debt_world(Some(mint_authority()), token_params());
    w.start_debt(0).unwrap();
    take(&mut w, 500, 0).expect("take the whole lot");
    assert_err(w.start_debt(1), "DelayNotElapsed");
    w.advance(DEBT_RELIST_DELAY_MS - 1);
    assert_err(w.start_debt(1), "DelayNotElapsed");
    w.advance(1);
    w.start_debt(1).expect("relist after the delay");
    assert!(acc(&w).debt_live);
}

/// The griefing loop of finding 8: start, wait the ttl, close, relist. The relist is refused for the
/// delay, so the admin changes a locked system parameter in the window; after the delay the griefer's
/// next auction locks the parameters again, and its close opens the next window.
#[test]
fn the_griefing_loop_leaves_the_admin_a_window_after_every_close() {
    let mut w = debt_world(Some(mint_authority()), token_params());
    let ttl = token_params().system_ttl_ms;
    w.start_debt(0).expect("the griefer lists");
    w.advance(ttl);
    w.close_debt(0).expect("and closes after the ttl");
    assert_err(w.start_debt(1), "DelayNotElapsed");
    let longer = EngineParamsArgs {
        system_ttl_ms: ttl + 1_000,
        ..token_params()
    };
    w.set_engine_params(k::ENGINE_ADMIN, &longer)
        .expect("a locked system parameter changes inside the window");

    w.advance(DEBT_RELIST_DELAY_MS);
    w.start_debt(1)
        .expect("the griefer relists after the delay");
    assert_err(
        w.set_engine_params(k::ENGINE_ADMIN, &token_params()),
        "ProtocolTokenLocked",
    );
    w.advance(ttl + 1_000);
    w.close_debt(1).expect("close again");
    w.set_engine_params(k::ENGINE_ADMIN, &token_params())
        .expect("the next close opens the next window");
}
