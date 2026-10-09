//! Handler tests of `take_surplus` (moved from surplus_tests.rs).

use auction_engine_core::{
    pda::{surplus_vault_seed, SLOT_PROTOCOL_TOKEN},
    state::*,
    EngineError,
};
use lee_core::{
    account::{Account, Data},
    program::ChainedCall,
};
use token_core::TokenDefinition;

use crate::{
    admin_tests::{config_state, fresh, meta, ENGINE, ONE, STABLE, TOKEN},
    collateral_tests::clock_at,
    surplus::*,
    token_calls, EngineResult,
};

use crate::surplus_tests::*;

pub(crate) fn take_fixture() -> TakeSurplusAccounts {
    TakeSurplusAccounts {
        taker_protocol_holding: holding(TAKER_PROTOCOL, PROTOCOL, 10_000, true),
        taker_stable_holding: holding(TAKER_STABLE, STABLE, 0, false),
        config: config_account(&protocol_config(false)),
        accounting: accounting_account(&live_accounting()),
        surplus_auction: surplus_account(&live_auction(LOT)),
        surplus_vault: holding(vault_id(0), STABLE, LOT, false),
        protocol_token_definition: protocol_definition(),
        clock: clock_at(NOW),
    }
}

pub(crate) fn run_take(a: TakeSurplusAccounts, coin: u128, max_in: u128) -> EngineResult {
    take_surplus(ENGINE, a, coin, max_in)
}

pub(crate) fn take_err(a: TakeSurplusAccounts) -> EngineError {
    run_take(a, LOT, u128::MAX).expect_err("take must be rejected")
}

pub(crate) const T_ACCOUNTING: usize = 3;
pub(crate) const T_AUCTION: usize = 4;

pub(crate) fn burn_and_pay(tokens: u128, filled: u128) -> Vec<ChainedCall> {
    let f = take_fixture();
    vec![
        token_calls::burn(
            TOKEN,
            f.protocol_token_definition,
            f.taker_protocol_holding,
            tokens,
            vec![],
        ),
        token_calls::transfer(
            TOKEN,
            token_calls::authorized(&f.surplus_vault),
            f.taker_stable_holding,
            filled,
            vec![surplus_vault_seed(&auction_id(0))],
        ),
    ]
}

#[test]
pub(crate) fn full_take_burns_the_ceil_price_from_the_taker_and_pays_the_lot() {
    // 500 coin: ceil(500 x 10500 / 10000) = 525 coin-equivalents, ceil(525 / 2) = 263 tokens.
    let (post, calls) = run_take(take_fixture(), LOT, 263).expect("take");
    assert_eq!(post.len(), 8);
    assert_eq!(calls, burn_and_pay(263, LOT));
    let acc = accounting_of(&post[T_ACCOUNTING]);
    assert_eq!(
        (
            acc.surplus_live,
            acc.total_protocol_burned,
            acc.last_surplus_end_ms
        ),
        (false, 7 + 263, NOW)
    );
    let sa = surplus_of(&post[T_AUCTION]);
    assert_eq!((sa.lot_left, sa.live, sa.ref_ray), (0, false, ADMIN_REF));
}

#[test]
pub(crate) fn the_burn_is_authorised_by_the_taker_not_by_a_pda() {
    let (_, calls) = run_take(take_fixture(), LOT, u128::MAX).expect("take");
    assert!(calls[0].pda_seeds.is_empty(), "no engine seed on the burn");
    assert!(
        calls[0].pre_states[1].is_authorized,
        "the taker's signed holding"
    );
    assert_eq!(calls[0].pre_states[1].account_id, TAKER_PROTOCOL);
    assert_eq!(calls[0].pre_states[0].account_id, PROTOCOL);
}

#[test]
pub(crate) fn max_amount_fills_at_most_the_lot_and_moves_only_the_filled_amount() {
    let (post, calls) = run_take(take_fixture(), u128::MAX, u128::MAX).expect("max take");
    assert_eq!(calls, burn_and_pay(263, LOT));
    assert!(!surplus_of(&post[T_AUCTION]).live);
    // The vault holds a donation on top; only the filled amount moves (ADR-033).
    let mut f = take_fixture();
    f.surplus_vault = holding(vault_id(0), STABLE, LOT + 900, false);
    let (_, calls) = run_take(f, u128::MAX, u128::MAX).expect("donated vault");
    let mut expected = burn_and_pay(263, LOT);
    expected[1] = token_calls::transfer(
        TOKEN,
        token_calls::authorized(&holding(vault_id(0), STABLE, LOT + 900, false)),
        holding(TAKER_STABLE, STABLE, 0, false),
        LOT,
        vec![surplus_vault_seed(&auction_id(0))],
    );
    assert_eq!(calls, expected);
}

#[test]
pub(crate) fn partial_take_keeps_the_auction_live_and_prices_the_filled_part_only() {
    // 250 coin: ceil(250 x 1.05) = 263, ceil(263 / 2) = 132.
    let (post, calls) = run_take(take_fixture(), 250, 132).expect("partial");
    assert_eq!(calls, burn_and_pay(132, 250));
    let sa = surplus_of(&post[T_AUCTION]);
    assert_eq!((sa.lot_left, sa.live), (250, true));
    let acc = accounting_of(&post[T_ACCOUNTING]);
    assert_eq!(
        (
            acc.surplus_live,
            acc.total_protocol_burned,
            acc.last_surplus_end_ms
        ),
        (true, 7 + 132, 0)
    );
}

#[test]
pub(crate) fn take_price_follows_the_snapshot_and_the_decay_not_the_config_price() {
    // The config price changes after start (4.0): the take still uses the snapshotted 2.0.
    let mut cfg = protocol_config(false);
    cfg.protocol_token_ref_price = 4 * ONE;
    let mut f = take_fixture();
    f.config = config_account(&cfg);
    assert_eq!(
        run_take(f, LOT, 262).expect_err("slippage"),
        EngineError::Slippage
    );
    let mut f = take_fixture();
    f.config = config_account(&cfg);
    run_take(f, LOT, 263).expect("snapshot price");
    // Decay: after 1_000 ms the multiplier is 10500 - 2500 = 8000 -> 400 coin-eq -> 200 tokens.
    let mut f = take_fixture();
    f.clock = clock_at(NOW + 1_000);
    run_take(f, LOT, 200).expect("decayed");
    let mut f = take_fixture();
    f.clock = clock_at(NOW + 1_000);
    assert_eq!(
        run_take(f, LOT, 199).expect_err("slippage"),
        EngineError::Slippage
    );
}

#[test]
pub(crate) fn take_core_rejections() {
    assert_eq!(
        run_take(take_fixture(), 495, u128::MAX).expect_err("dust"),
        EngineError::DustRemainder
    );
    assert_eq!(
        run_take(take_fixture(), 250, 131).expect_err("slippage"),
        EngineError::Slippage
    );
    assert_eq!(
        run_take(take_fixture(), 0, u128::MAX).expect_err("zero"),
        EngineError::ZeroFill
    );
    let mut f = take_fixture();
    f.surplus_auction = surplus_account(&SurplusAuctionState {
        live: false,
        ..live_auction(0)
    });
    assert_eq!(take_err(f), EngineError::NotLive);
}

#[test]
pub(crate) fn take_needs_the_taker_signature_and_enough_protocol_tokens() {
    let mut f = take_fixture();
    f.taker_protocol_holding.is_authorized = false;
    assert_eq!(
        take_err(f),
        EngineError::WrongAccount("taker_protocol_holding")
    );
    let mut f = take_fixture();
    f.taker_protocol_holding = holding(TAKER_PROTOCOL, PROTOCOL, 262, true);
    assert_eq!(take_err(f), EngineError::InsufficientBalance);
    let mut f = take_fixture();
    f.taker_protocol_holding = holding(TAKER_PROTOCOL, PROTOCOL, 263, true);
    run_take(f, LOT, u128::MAX).expect("exactly enough");
    let mut f = take_fixture();
    f.taker_protocol_holding = holding(TAKER_PROTOCOL, STABLE, 10_000, true);
    assert_eq!(take_err(f), EngineError::WrongDefinition);
}

#[test]
pub(crate) fn take_checks_the_protocol_token_definition_id_owner_and_type() {
    // Carry-forward 1 (Task 8 review).
    let mut f = take_fixture();
    f.protocol_token_definition.account_id = FOREIGN;
    assert_eq!(take_err(f), EngineError::WrongAccount("slot"));
    let mut f = take_fixture();
    f.protocol_token_definition.account.program_owner = OTHER_PROGRAM;
    assert_eq!(take_err(f), EngineError::ProtocolTokenMismatch);
    let mut f = take_fixture();
    f.protocol_token_definition.account.data = Data::from(&TokenDefinition::NonFungible {
        name: String::from("N"),
        printable_supply: 1,
        metadata_id: FOREIGN,
    });
    assert_eq!(take_err(f), EngineError::ProtocolTokenMismatch);
    let mut f = take_fixture();
    f.config = config_account(&config_state());
    f.protocol_token_definition = placeholder(SLOT_PROTOCOL_TOKEN);
    assert_eq!(take_err(f), EngineError::TokenNotConfigured);
}

#[test]
pub(crate) fn take_pins_vault_recipient_and_book() {
    let mut f = take_fixture();
    f.surplus_vault.account_id = vault_id(1);
    assert_eq!(take_err(f), EngineError::WrongAccount("surplus_vault"));
    let mut f = take_fixture();
    f.surplus_vault = holding(vault_id(0), PROTOCOL, LOT, false);
    assert_eq!(take_err(f), EngineError::WrongDefinition);
    let mut f = take_fixture();
    f.surplus_vault = holding(vault_id(0), STABLE, LOT - 1, false);
    assert_eq!(take_err(f), EngineError::VaultBelowBook);
    let mut f = take_fixture();
    f.taker_stable_holding = holding(vault_id(0), STABLE, LOT, false);
    assert_eq!(
        take_err(f),
        EngineError::WrongAccount("taker_stable_holding")
    );
    let mut f = take_fixture();
    f.taker_stable_holding = holding(TAKER_STABLE, PROTOCOL, 0, false);
    assert_eq!(take_err(f), EngineError::WrongDefinition);
    // A fresh stablecoin holding must sign; signed it is accepted.
    let mut f = take_fixture();
    f.taker_stable_holding = fresh(TAKER_STABLE);
    assert_eq!(take_err(f), EngineError::RecipientMustSign);
    let mut f = take_fixture();
    f.taker_stable_holding = meta(TAKER_STABLE, Account::default(), true);
    run_take(f, LOT, u128::MAX).expect("fresh signed recipient");
    // The auction belongs to this config, at its own PDA, owned by the engine.
    let mut f = take_fixture();
    f.surplus_auction = surplus_account(&SurplusAuctionState {
        config: FOREIGN,
        ..live_auction(LOT)
    });
    assert_eq!(take_err(f), EngineError::WrongAccount("surplus_auction"));
    let mut f = take_fixture();
    f.surplus_auction.account_id = auction_id(3);
    assert_eq!(take_err(f), EngineError::WrongAccount("surplus_auction"));
    let mut f = take_fixture();
    f.surplus_auction.account.program_owner = OTHER_PROGRAM;
    assert_eq!(take_err(f), EngineError::WrongAccount("surplus_auction"));
    let mut f = take_fixture();
    f.accounting.account_id = FOREIGN;
    assert_eq!(take_err(f), EngineError::WrongAccount("accounting"));
}
