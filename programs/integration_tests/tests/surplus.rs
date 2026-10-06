mod common;

use auction_engine_core::{
    args::EngineParamsArgs,
    state::{AccountingState, EngineConfigState, SurplusAuctionState},
    Instruction as Ix,
};
use common::*;
use lee_core::account::{Account, Data, Nonce};
use token_core::TokenDefinition;

fn acc(w: &World) -> AccountingState {
    w.read(accounting())
}

fn system_params() -> EngineParamsArgs {
    EngineParamsArgs {
        protocol_token_enabled: true,
        ..EngineParamsArgs::defaults_for_tests()
    }
}

#[test]
fn surplus_auction_burns_protocol_tokens() {
    let mut w = World::with_system(false);
    w.donate_stable(buffer(), 1_500);
    w.start_surplus(0).expect("start_surplus_auction");
    let sa: SurplusAuctionState = w.read(surplus_auction(0));
    assert_eq!(
        (sa.lot_left, sa.live, sa.ref_ray),
        (500, true, 2 * ONE),
        "admin price, no oracle"
    );
    assert_eq!(
        (w.balance(buffer()), w.balance(surplus_vault(0))),
        (1_000, 500)
    );
    assert_eq!(
        (
            acc(&w).surplus_live,
            acc(&w).surplus_nonce,
            acc(&w).total_surplus_lots_out
        ),
        (true, 1, 500)
    );
    // tokens = ceil(ceil(500 x 1.05) x RAY / 2 RAY) = ceil(525 / 2) = 263, burned.
    w.take_surplus(0, 500, 263).expect("take");
    assert_eq!(w.balance(id(k::TAKER_PROTOCOL)), 10_000 - 263);
    assert_eq!(w.supply(protocol_token_def()), 10_000 - 263);
    assert_eq!(w.balance(id(k::TAKER_STABLE)), 500);
    assert_eq!(w.balance(surplus_vault(0)), 0);
    let a = acc(&w);
    assert_eq!(
        (
            a.surplus_live,
            a.total_protocol_burned,
            a.last_surplus_end_ms
        ),
        (false, 263, w.now)
    );
}

#[test]
fn take_surplus_max_amount_fills_lot() {
    // Review Focus 4: only the filled amount moves; the tokens are computed by the core (ceil) and burned.
    let mut w = World::with_system(false);
    w.donate_stable(buffer(), 1_500);
    w.start_surplus(0).unwrap();
    w.take_surplus(0, u128::MAX, u128::MAX).expect("max take");
    assert_eq!(w.balance(id(k::TAKER_STABLE)), 500);
    assert_eq!(w.balance(buffer()), 1_000, "nothing else moved");
    assert_eq!(w.balance(id(k::TAKER_PROTOCOL)), 10_000 - 263);
    assert_eq!(w.supply(protocol_token_def()), 10_000 - 263);
    assert_eq!(acc(&w).total_protocol_burned, 263);
    assert!(!w.read::<SurplusAuctionState>(surplus_auction(0)).live);
}

#[test]
fn partial_take_dust_and_slippage() {
    let mut w = World::with_system(false);
    w.donate_stable(buffer(), 1_500);
    w.start_surplus(0).unwrap();
    assert_err(w.take_surplus(0, 495, u128::MAX), "DustRemainder");
    assert_err(w.take_surplus(0, 250, 1), "Slippage");
    assert_err(w.take_surplus(0, 0, u128::MAX), "ZeroFill");
    w.take_surplus(0, 250, u128::MAX).unwrap();
    let sa: SurplusAuctionState = w.read(surplus_auction(0));
    assert_eq!((sa.lot_left, sa.live), (250, true));
    // 250 coin: ceil(250 x 1.05) = 263 (262.5 up), then ceil(263 / 2) = 132 tokens.
    assert_eq!(w.balance(id(k::TAKER_PROTOCOL)), 10_000 - 132);
    assert_eq!(w.balance(surplus_vault(0)), 250);
    assert_eq!(acc(&w).total_protocol_burned, 132);
    assert!(
        acc(&w).surplus_live,
        "a partial take leaves the auction live"
    );
}

#[test]
fn start_rejections() {
    let mut w = World::with_engine();
    w.donate_stable(buffer(), 1_500);
    assert_err(w.start_surplus(0), "TokenNotConfigured");

    let mut w = World::with_system(false);
    w.donate_stable(buffer(), 1_499);
    assert_err(w.start_surplus(0), "BufferBelowTarget");
    w.donate_stable(buffer(), 1);
    assert_err(w.start_surplus(1), "NonceMismatch");
    w.start_surplus(0).unwrap();
    w.donate_stable(buffer(), 1_500);
    assert_err(w.start_surplus(1), "SurplusAuctionAlreadyLive");
}

#[test]
fn nonce_cannot_be_replayed_after_the_auction_ends() {
    let mut w = World::with_system(false);
    w.donate_stable(buffer(), 3_000);
    w.start_surplus(0).unwrap();
    w.take_surplus(0, u128::MAX, u128::MAX).unwrap();
    // The ended auction's account still exists, so the replay fails (before the nonce check even runs).
    assert_err(w.start_surplus(0), "AccountAlreadyInitialized");
    assert_err(w.start_surplus(2), "NonceMismatch");
    w.start_surplus(1).expect("the next nonce starts");
    assert_eq!(acc(&w).surplus_nonce, 2);
}

#[test]
fn restart_waits_for_the_configured_delay() {
    let mut w = World::with_system(false);
    w.donate_stable(buffer(), 3_000);
    let p = EngineParamsArgs {
        surplus_delay_ms: 60_000,
        ..system_params()
    };
    w.set_engine_params(k::ENGINE_ADMIN, &p).expect("set delay");
    w.start_surplus(0).unwrap();
    w.take_surplus(0, u128::MAX, u128::MAX).unwrap();
    assert_err(w.start_surplus(1), "DelayNotElapsed");
    w.advance(60_000);
    w.start_surplus(1).expect("delay elapsed");
}

#[test]
fn implicit_settle_runs_first_and_unbacked_debt_blocks_start() {
    let mut w = World::with_system(false);
    w.book_shortfall();
    w.donate_stable(buffer(), 1_500);
    assert_err(w.start_surplus(0), "UnbackedDebtPresent");
    assert_eq!(
        w.balance(buffer()),
        1_500,
        "the implicit burn reverted with the transaction"
    );
}

#[test]
fn implicit_settle_burns_the_debt_before_the_lot_moves() {
    // The buffer covers the booked shortfall first (implicit settle_debt); the remainder funds the lot.
    let mut w = World::with_system(false);
    w.book_shortfall();
    let debt = acc(&w).unbacked_debt;
    assert_eq!(debt, 355_049);
    let held = w.balance(buffer());
    w.donate_stable(buffer(), debt + 1_500);
    w.start_surplus(0).expect("start after implicit settle");
    let a = acc(&w);
    assert_eq!((a.unbacked_debt, a.total_debt_settled), (0, debt));
    assert_eq!(
        (w.balance(buffer()), w.balance(surplus_vault(0))),
        (held + 1_500 - 500, 500)
    );
}

#[test]
fn close_after_ttl_returns_the_remainder() {
    let mut w = World::with_system(false);
    w.donate_stable(buffer(), 1_500);
    w.start_surplus(0).unwrap();
    w.take_surplus(0, 250, u128::MAX).unwrap();
    assert_err(w.close_surplus(0), "DelayNotElapsed");
    w.advance(EngineParamsArgs::defaults_for_tests().system_ttl_ms);
    w.close_surplus(0).expect("close");
    assert_eq!(w.balance(buffer()), 1_250);
    assert_eq!(w.balance(surplus_vault(0)), 0);
    let a = acc(&w);
    assert_eq!(
        (
            a.surplus_live,
            a.total_surplus_returned,
            a.last_surplus_end_ms
        ),
        (false, 250, w.now)
    );
    assert_err(w.close_surplus(0), "NotLive");
    assert_err(w.take_surplus(0, 250, u128::MAX), "NotLive");
}

#[test]
fn reference_oracle_overrides_within_twenty_percent() {
    // spec 5.11 / ADR-017: valid oracle at 3.0 -> clamped to 2.4; 1.9 -> 1.9; stale -> admin 2.0.
    for (price, age, expected) in [
        (3 * ONE, 0, 24 * ONE / 10),
        (19 * ONE / 10, 0, 19 * ONE / 10),
        (3 * ONE, 3_600_001, 2 * ONE),
    ] {
        let mut w = World::with_system(true);
        w.advance(3_600_001);
        w.set_ref_oracle(price, w.now - age);
        w.set_oracle(R, w.now);
        w.donate_stable(buffer(), 1_500);
        w.start_surplus(0).unwrap();
        assert_eq!(
            w.read::<SurplusAuctionState>(surplus_auction(0)).ref_ray,
            expected,
            "oracle {price} age {age}"
        );
    }
}

#[test]
fn live_take_ignores_later_ref_price_and_oracle_max_age_changes() {
    // Carry-forward 2 (Task 8 review, ADR-031): the reference price is snapshotted at start, so a
    // later admin price change or oracle max-age change does not alter a live auction's price.
    let mut w = World::with_system(true);
    w.donate_stable(buffer(), 1_500);
    w.start_surplus(0).unwrap();
    let p = EngineParamsArgs {
        ref_oracle_enabled: true,
        protocol_token_ref_price: 4 * ONE,
        ref_oracle_max_age_ms: 1,
        ..system_params()
    };
    w.set_engine_params(k::ENGINE_ADMIN, &p)
        .expect("ref price and max age are not locked while an auction is live");
    let cfg: EngineConfigState = w.read(config());
    assert_eq!(
        (cfg.protocol_token_ref_price, cfg.ref_oracle_max_age_ms),
        (4 * ONE, 1)
    );
    // After the 1_000 ms decay the multiplier is 1.05 - 0.25 = 0.80: 400 coin-eq / snapshotted 2.0 = 200
    // tokens (a refreshed 4.0 would give 100; the stale oracle is never consulted).
    w.advance(5_000);
    assert_err(w.take_surplus(0, 500, 199), "Slippage");
    w.take_surplus(0, 500, 200)
        .expect("still priced at the snapshotted 2.0");
    assert_eq!(w.balance(id(k::TAKER_PROTOCOL)), 10_000 - 200);
}

#[test]
fn take_requires_the_configured_protocol_token_definition() {
    // Carry-forward 1 (Task 8 review): id, owner and type are checked at use time.
    let mut w = World::with_system(false);
    w.donate_stable(buffer(), 1_500);
    w.start_surplus(0).unwrap();
    let ix = || Ix::TakeSurplus {
        amount_coin: 500,
        max_protocol_in: u128::MAX,
    };

    // Swapped: a different account in the definition slot.
    let mut accounts = w.take_surplus_accounts(0);
    accounts[6] = id(k::PROTOCOL_SEED_HOLDING);
    assert_err(
        w.engine(accounts, &[k::TAKER_PROTOCOL], ix()),
        "WrongAccount",
    );

    // Foreign-owned: the configured id, but not owned by the token program.
    let real = w.state.get_account_by_id(protocol_token_def());
    w.state.force_insert_account(
        protocol_token_def(),
        Account {
            program_owner: [0x33u32; 8],
            ..real.clone()
        },
    );
    let accounts = w.take_surplus_accounts(0);
    assert_err(
        w.engine(accounts, &[k::TAKER_PROTOCOL], ix()),
        "ProtocolTokenMismatch",
    );

    // Not a fungible definition.
    w.state.force_insert_account(
        protocol_token_def(),
        Account {
            data: Data::from(&TokenDefinition::NonFungible {
                name: String::from("NFT"),
                printable_supply: 1,
                metadata_id: id(k::PROTOCOL_SEED_HOLDING),
            }),
            nonce: Nonce(0),
            ..real.clone()
        },
    );
    let accounts = w.take_surplus_accounts(0);
    assert_err(
        w.engine(accounts, &[k::TAKER_PROTOCOL], ix()),
        "ProtocolTokenMismatch",
    );

    // Restored: the take works.
    w.state.force_insert_account(protocol_token_def(), real);
    w.take_surplus(0, 500, u128::MAX).expect("take");
}

#[test]
fn take_pins_the_vault_and_the_holdings() {
    let mut w = World::with_system(false);
    w.donate_stable(buffer(), 1_500);
    w.start_surplus(0).unwrap();
    let ix = || Ix::TakeSurplus {
        amount_coin: 500,
        max_protocol_in: u128::MAX,
    };
    // A protocol holding of the wrong definition.
    w.holding(k::PROTOCOL_SEED_HOLDING, stable_def(), 10_000);
    w.ensure_signer(k::PROTOCOL_SEED_HOLDING);
    let mut accounts = w.take_surplus_accounts(0);
    accounts[0] = id(k::PROTOCOL_SEED_HOLDING);
    assert_err(
        w.engine(accounts, &[k::PROTOCOL_SEED_HOLDING], ix()),
        "WrongDefinition",
    );
    // The buffer in the vault slot is refused by the vault PDA.
    let mut accounts = w.take_surplus_accounts(0);
    accounts[5] = buffer();
    assert!(w.engine(accounts, &[k::TAKER_PROTOCOL], ix()).is_err());
}
