//! ADR-039 (THREAT_MODEL finding 8): a debt auction cannot be relisted until DEBT_RELIST_DELAY_MS after
//! the previous one ended, which gives the admin a window with no live debt auction.

use auction_engine_core::{args::DEBT_RELIST_DELAY_MS, state::*, EngineError};

use crate::{
    admin_tests::{fresh, rejected, ENGINE},
    debt::close_debt_auction,
    debt_tests::*,
    surplus_tests::{accounting_account, accounting_of, NOW},
};

fn ended_at(ms: u64) -> AccountingState {
    AccountingState {
        last_debt_end_ms: ms,
        debt_nonce: 1,
        ..start_accounting()
    }
}

fn start_after_end_at(ms: u64) -> crate::EngineResult {
    let mut f = start_fixture();
    f.accounting = accounting_account(&ended_at(ms));
    f.debt_auction = fresh(debt_id(1));
    run_start(f, 1)
}

#[test]
fn start_waits_for_the_relist_delay_after_the_previous_auction_ended() {
    assert_eq!(
        rejected(start_after_end_at(NOW - DEBT_RELIST_DELAY_MS + 1)),
        EngineError::DelayNotElapsed
    );
    assert_eq!(
        rejected(start_after_end_at(NOW)),
        EngineError::DelayNotElapsed,
        "ended in this very block"
    );
    assert!(
        start_after_end_at(NOW - DEBT_RELIST_DELAY_MS).is_ok(),
        "the delay has elapsed exactly"
    );
}

#[test]
fn an_end_time_near_u64_max_blocks_without_panicking() {
    assert_eq!(
        rejected(start_after_end_at(u64::MAX)),
        EngineError::DelayNotElapsed
    );
}

#[test]
fn the_first_debt_auction_has_no_delay() {
    let (post, _) = run_start(start_fixture(), 0).expect("no debt auction has ended yet");
    assert_eq!(accounting_of(&post[S_ACCOUNTING]).last_debt_end_ms, 0);
}

#[test]
fn start_does_not_touch_the_end_time() {
    let (post, _) = start_after_end_at(NOW - DEBT_RELIST_DELAY_MS).expect("relist");
    assert_eq!(
        accounting_of(&post[S_ACCOUNTING]).last_debt_end_ms,
        NOW - DEBT_RELIST_DELAY_MS
    );
}

#[test]
fn close_records_the_end_time() {
    let (post, _) =
        close_debt_auction(ENGINE, close_fixture(LOT, NOW + TTL)).expect("close after the ttl");
    assert_eq!(accounting_of(&post[2]).last_debt_end_ms, NOW + TTL);
}

#[test]
fn the_last_take_records_the_end_time_and_a_partial_take_does_not() {
    let (post, _) = run_take(take_fixture(), LOT, 0).expect("full take ends the auction");
    assert_eq!(accounting_of(&post[T_ACCOUNTING]).last_debt_end_ms, NOW);
    let (post, _) = run_take(take_fixture(), LOT / 2, 0).expect("partial take");
    assert_eq!(accounting_of(&post[T_ACCOUNTING]).last_debt_end_ms, 0);
}
