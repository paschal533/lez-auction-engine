//! Handler-level tests of `settle_empty`, `claim_leftover` and `settle_debt`: every guard is exercised
//! without the guest, so each can be mutation-checked in seconds.

use auction_engine_core::{
    pda::{
        accounting_pda, auction_pda, buffer_pda, buffer_seed, coll_config_pda, vault_pda,
        vault_seed,
    },
    state::*,
    EngineError,
};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::AccountPostState,
};
use token_core::TokenHolding;

use crate::{
    admin_tests::{
        coll_state, config_state, meta, rejected, ADMIN, COLLATERAL, ENGINE, STABLE, TOKEN,
    },
    collateral_tests::{cfg_id, engine_owned},
    settle::{
        buffer_burn, claim_leftover, implicit_settle, settle_debt, settle_empty,
        ClaimLeftoverAccounts, SettleDebtAccounts, SettleEmptyAccounts,
    },
    token_calls, EngineResult,
};

const POSITION: AccountId = AccountId::new([0x77; 32]);
const OWNER: AccountId = AccountId::new([0x81; 32]);
const OWNER_COLL: AccountId = AccountId::new([0x82; 32]);
const CALLER: AccountId = AccountId::new([0x83; 32]);
const ATTACKER: AccountId = AccountId::new([0x84; 32]);
const FOREIGN: AccountId = AccountId::new([0x85; 32]);

const RESERVED: u128 = 1_100;
const DEBT: u128 = 1_000;
const LEFTOVER: u128 = 163_241;

fn holding(
    id: AccountId,
    definition: AccountId,
    balance: u128,
    signed: bool,
) -> AccountWithMetadata {
    meta(
        id,
        Account {
            program_owner: TOKEN,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: definition,
                balance,
            }),
            ..Account::default()
        },
        signed,
    )
}

fn auction_id() -> AccountId {
    auction_pda(&ENGINE, &cfg_id(), &POSITION, 0)
}

fn vault_id() -> AccountId {
    vault_pda(&ENGINE, &auction_id())
}

/// Lot 0 after the keeper reward consumed the whole seizure (Task 10 underwater case).
fn empty_lot() -> AuctionState {
    AuctionState {
        tag: TAG_AUCTION,
        config: cfg_id(),
        position: POSITION,
        nonce: 0,
        owner: OWNER,
        keeper: ADMIN,
        settled: false,
        collateral_seized: 1_000,
        collateral_sold: 0,
        collateral_left: 0,
        keeper_reward: 1_000,
        leftover_claimable: 0,
        leftover_claimed: 0,
        debt_principal: DEBT,
        amount_to_raise: RESERVED,
        reserved_amount: RESERVED,
        debt_raised: 0,
        penalty_raised: 0,
        start_ms: 1,
        schedule: vec![SchedulePointRec {
            elapsed_ms: 0,
            discount_bps: 200,
        }],
        type_paused_total_at_start: 0,
    }
}

/// Target reached with a leftover of `LEFTOVER` collateral.
fn settled_with_leftover() -> AuctionState {
    AuctionState {
        settled: true,
        collateral_seized: 500_000,
        collateral_sold: 336_734,
        collateral_left: LEFTOVER,
        keeper_reward: 25,
        leftover_claimable: LEFTOVER,
        debt_principal: 600_000,
        amount_to_raise: 660_000,
        reserved_amount: 660_000,
        debt_raised: 600_000,
        penalty_raised: 60_000,
        ..empty_lot()
    }
}

fn auction_account(state: &AuctionState) -> AccountWithMetadata {
    meta(auction_id(), engine_owned(state.into()), false)
}

fn accounting_account(state: &AccountingState) -> AccountWithMetadata {
    meta(
        accounting_pda(&ENGINE, &cfg_id()),
        engine_owned(state.into()),
        false,
    )
}

fn reserved_config() -> AccountWithMetadata {
    let mut c = config_state();
    c.global_reserved = RESERVED + 9;
    meta(cfg_id(), engine_owned((&c).into()), false)
}

fn reserved_coll() -> AccountWithMetadata {
    let c = CollateralConfigState {
        type_reserved: RESERVED + 4,
        ..coll_state(cfg_id(), 5_000_000)
    };
    meta(
        coll_config_pda(&ENGINE, &cfg_id(), &COLLATERAL),
        engine_owned((&c).into()),
        false,
    )
}

fn prior_accounting() -> AccountingState {
    AccountingState {
        unbacked_debt: 7,
        total_shortfall_booked: 11,
        total_debt_burned: 5,
        total_penalty_collected: 3,
        ..AccountingState::new()
    }
}

fn empty_fixture() -> SettleEmptyAccounts {
    SettleEmptyAccounts {
        caller: meta(CALLER, Account::default(), true),
        config: reserved_config(),
        coll_config: reserved_coll(),
        accounting: accounting_account(&prior_accounting()),
        auction: auction_account(&empty_lot()),
    }
}

fn run_empty(a: SettleEmptyAccounts) -> EngineResult {
    settle_empty(ENGINE, a)
}

const E_CALLER: usize = 0;
const E_CONFIG: usize = 1;
const E_COLL: usize = 2;
const E_ACCOUNTING: usize = 3;
const E_AUCTION: usize = 4;

fn auction_of(p: &AccountPostState) -> AuctionState {
    AuctionState::try_from(&p.account().data).expect("auction decodes")
}
fn accounting_of(p: &AccountPostState) -> AccountingState {
    AccountingState::try_from(&p.account().data).expect("accounting decodes")
}

#[test]
fn settle_empty_books_the_shortfall_releases_both_reservations_and_chains_nothing() {
    let (post, calls) = run_empty(empty_fixture()).expect("settle_empty");
    assert_eq!(post.len(), 5);
    assert!(calls.is_empty());
    let a = auction_of(&post[E_AUCTION]);
    assert!(a.settled);
    assert_eq!(
        (a.collateral_left, a.leftover_claimable, a.reserved_amount),
        (0, 0, RESERVED),
        "only the status changes"
    );
    let acc = accounting_of(&post[E_ACCOUNTING]);
    assert_eq!(
        (
            acc.unbacked_debt,
            acc.total_shortfall_booked,
            acc.total_debt_burned,
            acc.total_penalty_collected
        ),
        (7 + DEBT, 11 + DEBT, 5, 3)
    );
    let coll = CollateralConfigState::try_from(&post[E_COLL].account().data).unwrap();
    assert_eq!(coll.type_reserved, 4);
    let cfg = EngineConfigState::try_from(&post[E_CONFIG].account().data).unwrap();
    assert_eq!(cfg.global_reserved, 9);
    assert_eq!(post[E_CALLER].account(), &Account::default());
}

#[test]
fn settle_empty_folds_cumulative_raised_amounts_once() {
    let mut st = empty_lot();
    st.debt_raised = 300;
    st.penalty_raised = 20;
    let mut a = empty_fixture();
    a.auction = auction_account(&st);
    let (post, _) = run_empty(a).expect("settle_empty");
    let acc = accounting_of(&post[E_ACCOUNTING]);
    assert_eq!(
        (
            acc.total_debt_burned,
            acc.total_penalty_collected,
            acc.unbacked_debt,
            acc.total_shortfall_booked
        ),
        (5 + 300, 3 + 20, 7 + (DEBT - 300), 11 + (DEBT - 300))
    );
}

#[test]
fn settle_empty_applies_exactly_once() {
    let (post, _) = run_empty(empty_fixture()).expect("first settle_empty");
    let mut replay = empty_fixture();
    replay.auction = meta(auction_id(), post[E_AUCTION].account().clone(), false);
    replay.accounting = meta(
        accounting_pda(&ENGINE, &cfg_id()),
        post[E_ACCOUNTING].account().clone(),
        false,
    );
    assert_eq!(rejected(run_empty(replay)), EngineError::NotAuctioning);
}

#[test]
fn settle_empty_needs_an_empty_auctioning_lot() {
    let mut live = empty_fixture();
    live.auction = auction_account(&AuctionState {
        collateral_left: 1,
        ..empty_lot()
    });
    assert_eq!(rejected(run_empty(live)), EngineError::NotAuctioning);
    let mut settled = empty_fixture();
    settled.auction = auction_account(&AuctionState {
        settled: true,
        ..empty_lot()
    });
    assert_eq!(rejected(run_empty(settled)), EngineError::NotAuctioning);
}

#[test]
fn settle_empty_needs_a_signing_caller() {
    let mut a = empty_fixture();
    a.caller.is_authorized = false;
    assert_eq!(rejected(run_empty(a)), EngineError::WrongAccount("caller"));
}

#[test]
fn settle_empty_validates_every_engine_account() {
    let mut cfg = empty_fixture();
    cfg.config.account_id = FOREIGN;
    assert_eq!(
        rejected(run_empty(cfg)),
        EngineError::WrongAccount("config")
    );
    let mut coll = empty_fixture();
    coll.coll_config.account_id = FOREIGN;
    assert_eq!(
        rejected(run_empty(coll)),
        EngineError::WrongAccount("coll_config")
    );
    let mut acc_id = empty_fixture();
    acc_id.accounting.account_id = FOREIGN;
    assert_eq!(
        rejected(run_empty(acc_id)),
        EngineError::WrongAccount("accounting")
    );
    let mut acc_owner = empty_fixture();
    acc_owner.accounting.account.program_owner = TOKEN;
    assert_eq!(
        rejected(run_empty(acc_owner)),
        EngineError::WrongAccount("accounting")
    );
    let mut auction = empty_fixture();
    auction.auction.account_id = FOREIGN;
    assert_eq!(
        rejected(run_empty(auction)),
        EngineError::WrongAccount("auction")
    );
}

#[test]
fn settle_empty_cannot_release_more_than_is_reserved() {
    let mut coll = empty_fixture();
    let c = CollateralConfigState {
        type_reserved: RESERVED - 1,
        ..coll_state(cfg_id(), 5_000_000)
    };
    coll.coll_config = meta(
        coll.coll_config.account_id,
        engine_owned((&c).into()),
        false,
    );
    assert_eq!(rejected(run_empty(coll)), EngineError::Math);
    let mut g = config_state();
    g.global_reserved = RESERVED - 1;
    let mut cfg = empty_fixture();
    cfg.config = meta(cfg_id(), engine_owned((&g).into()), false);
    assert_eq!(rejected(run_empty(cfg)), EngineError::Math);
}

fn claim_fixture() -> ClaimLeftoverAccounts {
    ClaimLeftoverAccounts {
        owner: meta(OWNER, Account::default(), true),
        owner_collateral_holding: holding(OWNER_COLL, COLLATERAL, 0, false),
        config: meta(cfg_id(), engine_owned((&config_state()).into()), false),
        auction: auction_account(&settled_with_leftover()),
        auction_vault: holding(vault_id(), COLLATERAL, LEFTOVER, false),
    }
}

fn run_claim(a: ClaimLeftoverAccounts) -> EngineResult {
    claim_leftover(ENGINE, a)
}

fn sweep_call(vault_balance: u128, paid: u128) -> lee_core::program::ChainedCall {
    token_calls::transfer(
        TOKEN,
        token_calls::authorized(&holding(vault_id(), COLLATERAL, vault_balance, false)),
        holding(OWNER_COLL, COLLATERAL, 0, false),
        paid,
        vec![vault_seed(&auction_id())],
    )
}

#[test]
fn claim_leftover_pays_the_leftover_once_and_marks_it_claimed() {
    let a = claim_fixture();
    let config_before = a.config.account.clone();
    let (post, calls) = run_claim(a).expect("owner claims");
    assert_eq!(post.len(), 5);
    assert_eq!(calls, vec![sweep_call(LEFTOVER, LEFTOVER)]);
    let s = auction_of(&post[3]);
    assert_eq!(
        (s.leftover_claimed, s.leftover_claimable),
        (LEFTOVER, LEFTOVER)
    );
    assert!(s.settled);
    assert_eq!(post[2].account(), &config_before, "config is only read");

    let mut again = claim_fixture();
    again.auction = meta(auction_id(), post[3].account().clone(), false);
    assert_eq!(rejected(run_claim(again)), EngineError::NothingToClaim);
}

#[test]
fn claim_leftover_sweeps_the_whole_vault_including_donations() {
    let mut a = claim_fixture();
    a.auction_vault = holding(vault_id(), COLLATERAL, LEFTOVER + 3, false);
    let (_, calls) = run_claim(a).expect("owner claims");
    assert_eq!(calls, vec![sweep_call(LEFTOVER + 3, LEFTOVER + 3)]);
}

#[test]
fn claim_leftover_pays_only_the_unclaimed_part_but_sweeps_the_balance() {
    let mut st = settled_with_leftover();
    st.leftover_claimed = 40;
    let mut a = claim_fixture();
    a.auction = auction_account(&st);
    a.auction_vault = holding(vault_id(), COLLATERAL, LEFTOVER - 40, false);
    let (post, calls) = run_claim(a).expect("owner claims the rest");
    assert_eq!(calls, vec![sweep_call(LEFTOVER - 40, LEFTOVER - 40)]);
    assert_eq!(auction_of(&post[3]).leftover_claimed, LEFTOVER);
}

#[test]
fn claim_leftover_vault_must_hold_at_least_the_claim() {
    let mut a = claim_fixture();
    a.auction_vault = holding(vault_id(), COLLATERAL, LEFTOVER - 1, false);
    assert_eq!(rejected(run_claim(a)), EngineError::VaultBelowBook);
    run_claim(claim_fixture()).expect("exactly the claim is enough (>=)");
}

#[test]
fn claim_leftover_is_owner_only() {
    let mut other = claim_fixture();
    other.owner = meta(ATTACKER, Account::default(), true);
    assert_eq!(rejected(run_claim(other)), EngineError::NotOwner);
    let mut unsigned = claim_fixture();
    unsigned.owner.is_authorized = false;
    assert_eq!(rejected(run_claim(unsigned)), EngineError::NotOwner);
}

#[test]
fn claim_leftover_needs_a_settled_auction_with_something_left() {
    let mut live = claim_fixture();
    live.auction = auction_account(&AuctionState {
        settled: false,
        ..settled_with_leftover()
    });
    assert_eq!(rejected(run_claim(live)), EngineError::NothingToClaim);
    let mut none = claim_fixture();
    none.auction = auction_account(&AuctionState {
        leftover_claimable: 0,
        ..settled_with_leftover()
    });
    assert_eq!(rejected(run_claim(none)), EngineError::NothingToClaim);
}

#[test]
fn claim_leftover_checks_the_vault_and_the_recipient() {
    let mut vault_id_wrong = claim_fixture();
    vault_id_wrong.auction_vault.account_id = FOREIGN;
    assert_eq!(
        rejected(run_claim(vault_id_wrong)),
        EngineError::WrongAccount("auction_vault")
    );
    let mut vault_def = claim_fixture();
    vault_def.auction_vault = holding(vault_id(), STABLE, LEFTOVER, false);
    assert_eq!(rejected(run_claim(vault_def)), EngineError::WrongDefinition);
    let mut recipient_def = claim_fixture();
    recipient_def.owner_collateral_holding = holding(OWNER_COLL, STABLE, 0, false);
    assert_eq!(
        rejected(run_claim(recipient_def)),
        EngineError::WrongDefinition
    );
    let mut to_vault = claim_fixture();
    to_vault.owner_collateral_holding = holding(vault_id(), COLLATERAL, LEFTOVER, false);
    assert_eq!(
        rejected(run_claim(to_vault)),
        EngineError::WrongAccount("owner_collateral_holding")
    );
}

#[test]
fn claim_leftover_to_a_fresh_holding_must_sign() {
    let mut fresh = claim_fixture();
    fresh.owner_collateral_holding = meta(OWNER_COLL, Account::default(), false);
    assert_eq!(rejected(run_claim(fresh)), EngineError::RecipientMustSign);
    let mut signed = claim_fixture();
    signed.owner_collateral_holding = meta(OWNER_COLL, Account::default(), true);
    run_claim(signed).expect("fresh holding that signs");
}

#[test]
fn claim_leftover_binds_the_auction_to_this_config() {
    let mut a = claim_fixture();
    a.auction.account_id = FOREIGN;
    assert_eq!(rejected(run_claim(a)), EngineError::WrongAccount("auction"));
    let mut cfg = claim_fixture();
    cfg.config.account_id = FOREIGN;
    assert_eq!(
        rejected(run_claim(cfg)),
        EngineError::WrongAccount("config")
    );
}

const BUFFER_BALANCE: u128 = 5_000;

fn buffer_id() -> AccountId {
    buffer_pda(&ENGINE, &cfg_id())
}

fn debt_accounting() -> AccountingState {
    AccountingState {
        unbacked_debt: DEBT,
        debt_on_auction: 200,
        ..AccountingState::new()
    }
}

fn debt_fixture(buffer_balance: u128) -> SettleDebtAccounts {
    SettleDebtAccounts {
        caller: meta(CALLER, Account::default(), true),
        config: meta(cfg_id(), engine_owned((&config_state()).into()), false),
        accounting: accounting_account(&debt_accounting()),
        buffer: holding(buffer_id(), STABLE, buffer_balance, false),
        stablecoin_definition: meta(
            STABLE,
            Account {
                program_owner: TOKEN,
                ..Account::default()
            },
            false,
        ),
    }
}

fn run_debt(a: SettleDebtAccounts, amount: u128) -> EngineResult {
    settle_debt(ENGINE, a, amount)
}

fn burn_call(balance: u128, amount: u128) -> lee_core::program::ChainedCall {
    let f = debt_fixture(balance);
    token_calls::burn(
        TOKEN,
        f.stablecoin_definition,
        token_calls::authorized(&f.buffer),
        amount,
        vec![buffer_seed(&cfg_id())],
    )
}

const D_ACCOUNTING: usize = 2;

#[test]
fn settle_debt_burns_under_the_buffer_seed_and_excludes_the_auctioned_debt() {
    let (post, calls) = run_debt(debt_fixture(BUFFER_BALANCE), u128::MAX).expect("settle_debt");
    assert_eq!(post.len(), 5);
    assert_eq!(calls, vec![burn_call(BUFFER_BALANCE, DEBT - 200)]);
    let acc = accounting_of(&post[D_ACCOUNTING]);
    assert_eq!(
        (
            acc.unbacked_debt,
            acc.total_debt_settled,
            acc.debt_on_auction
        ),
        (200, DEBT - 200, 200)
    );
}

#[test]
fn settle_debt_burns_the_smallest_of_amount_balance_and_settleable() {
    let (_, by_amount) = run_debt(debt_fixture(BUFFER_BALANCE), 500).unwrap();
    assert_eq!(by_amount, vec![burn_call(BUFFER_BALANCE, 500)]);
    let (_, by_balance) = run_debt(debt_fixture(300), u128::MAX).unwrap();
    assert_eq!(by_balance, vec![burn_call(300, 300)]);
    let (_, by_debt) = run_debt(debt_fixture(BUFFER_BALANCE), DEBT * 3).unwrap();
    assert_eq!(by_debt, vec![burn_call(BUFFER_BALANCE, DEBT - 200)]);
}

#[test]
fn settle_debt_uses_the_actual_balance_not_a_booked_one() {
    // A donation raises the balance; the burn follows it up to the settleable debt.
    let (post, calls) = run_debt(debt_fixture(BUFFER_BALANCE * 100), u128::MAX).unwrap();
    assert_eq!(calls, vec![burn_call(BUFFER_BALANCE * 100, DEBT - 200)]);
    assert_eq!(
        accounting_of(&post[D_ACCOUNTING]).total_debt_settled,
        DEBT - 200
    );
}

#[test]
fn settle_debt_with_nothing_to_burn_chains_nothing_and_changes_nothing() {
    let before = debt_accounting();
    for (balance, amount) in [(0, u128::MAX), (BUFFER_BALANCE, 0)] {
        let (post, calls) = run_debt(debt_fixture(balance), amount).unwrap();
        assert!(calls.is_empty());
        assert_eq!(accounting_of(&post[D_ACCOUNTING]), before);
    }
    let mut all_reserved = debt_fixture(BUFFER_BALANCE);
    all_reserved.accounting = accounting_account(&AccountingState {
        debt_on_auction: DEBT,
        ..debt_accounting()
    });
    let (_, calls) = run_debt(all_reserved, u128::MAX).unwrap();
    assert!(
        calls.is_empty(),
        "debt on auction is never settled from the buffer"
    );
}

#[test]
fn settle_debt_needs_a_signing_caller() {
    let mut a = debt_fixture(BUFFER_BALANCE);
    a.caller.is_authorized = false;
    assert_eq!(
        rejected(run_debt(a, 1)),
        EngineError::WrongAccount("caller")
    );
}

#[test]
fn settle_debt_binds_buffer_definition_and_accounting() {
    let mut buffer = debt_fixture(BUFFER_BALANCE);
    buffer.buffer.account_id = FOREIGN;
    assert_eq!(
        rejected(run_debt(buffer, 1)),
        EngineError::WrongAccount("buffer")
    );
    let mut def = debt_fixture(BUFFER_BALANCE);
    def.stablecoin_definition.account_id = FOREIGN;
    assert_eq!(
        rejected(run_debt(def, 1)),
        EngineError::WrongAccount("stablecoin_definition")
    );
    let mut wrong_buffer_def = debt_fixture(BUFFER_BALANCE);
    wrong_buffer_def.buffer = holding(buffer_id(), COLLATERAL, BUFFER_BALANCE, false);
    assert_eq!(
        rejected(run_debt(wrong_buffer_def, 1)),
        EngineError::WrongDefinition
    );
    let mut acc = debt_fixture(BUFFER_BALANCE);
    acc.accounting.account_id = FOREIGN;
    assert_eq!(
        rejected(run_debt(acc, 1)),
        EngineError::WrongAccount("accounting")
    );
    let mut cfg = debt_fixture(BUFFER_BALANCE);
    cfg.config.account_id = FOREIGN;
    assert_eq!(
        rejected(run_debt(cfg, 1)),
        EngineError::WrongAccount("config")
    );
}

#[test]
fn implicit_settle_burns_up_to_the_settleable_debt_and_buffer_balance() {
    let (next, burned) = implicit_settle(&debt_accounting(), BUFFER_BALANCE);
    assert_eq!(burned, DEBT - 200);
    assert_eq!(
        (next.unbacked_debt, next.total_debt_settled),
        (200, DEBT - 200)
    );
    let (_, small) = implicit_settle(&debt_accounting(), 10);
    assert_eq!(small, 10);
}

#[test]
fn buffer_burn_is_none_for_zero_and_seeded_otherwise() {
    let f = debt_fixture(BUFFER_BALANCE);
    let cfg = config_state();
    assert!(buffer_burn(&cfg, &cfg_id(), &f.stablecoin_definition, &f.buffer, 0).is_none());
    assert_eq!(
        buffer_burn(&cfg, &cfg_id(), &f.stablecoin_definition, &f.buffer, 9),
        Some(burn_call(BUFFER_BALANCE, 9))
    );
}
