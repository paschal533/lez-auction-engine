//! S3: create a PDA on first use with an argument-derived seed.
//!
//! `["LIQREC", config, position]` and `["AUCTION", config, position, nonce]` are created by the
//! `caller` program with SPEL `#[account(.. pda = [literal(..), account(..), arg(..)])]`.

mod common;

use common::*;
use lee::V03State;
use lee_core::account::AccountId;
use spike_core::{
    auction_pda, liqrec_pda, seed_from_str, u64_seed, Auction, CallerInstruction, LiqRecord,
    AUCTION_TAG, LIQREC_TAG,
};

const USER: u8 = 31;

fn config_id() -> AccountId {
    AccountId::new([0xC0; 32])
}

fn position(n: u8) -> [u8; 32] {
    [n; 32]
}

fn user_id() -> AccountId {
    id_of(&key(USER))
}

fn read_record(state: &V03State, id: AccountId) -> (lee_core::account::Account, LiqRecord) {
    let account = state.get_account_by_id(id);
    let rec = LiqRecord::try_from(&account.data).expect("record decodes");
    (account, rec)
}

fn run(
    label: &str,
    state: &mut V03State,
    accounts: Vec<AccountId>,
    instruction: CallerInstruction,
    expect_ok: bool,
) -> Result<(), lee::error::LeeError> {
    let signer = key(USER);
    if expect_ok {
        let m = measure(label, state, caller_id(), &accounts, &[&signer], &instruction);
        println!("S3RESULT {label} caller_cycles={}", m.total);
    }
    submit(state, caller_id(), accounts, &[&signer], instruction)
}

fn open_record(label: &str, state: &mut V03State, pos: [u8; 32], ok: bool) -> Result<(), lee::error::LeeError> {
    let accounts = vec![
        user_id(),
        config_id(),
        liqrec_pda(caller_id(), &config_id(), &pos),
    ];
    run(label, state, accounts, CallerInstruction::OpenRecord { position: pos }, ok)
}

fn open_auction(
    label: &str,
    state: &mut V03State,
    pos: [u8; 32],
    nonce: u64,
    ok: bool,
) -> Result<(), lee::error::LeeError> {
    let accounts = vec![
        user_id(),
        config_id(),
        liqrec_pda(caller_id(), &config_id(), &pos),
        auction_pda(caller_id(), &config_id(), &pos, nonce),
    ];
    run(
        label,
        state,
        accounts,
        CallerInstruction::OpenAuction { position: pos, nonce },
        ok,
    )
}

fn open_auction_existing(
    label: &str,
    state: &mut V03State,
    pos: [u8; 32],
    nonce: u64,
    ok: bool,
) -> Result<(), lee::error::LeeError> {
    let accounts = vec![
        user_id(),
        config_id(),
        liqrec_pda(caller_id(), &config_id(), &pos),
        auction_pda(caller_id(), &config_id(), &pos, nonce),
    ];
    run(
        label,
        state,
        accounts,
        CallerInstruction::OpenAuctionExisting { position: pos, nonce },
        ok,
    )
}

fn expect_fail(label: &str, res: Result<(), lee::error::LeeError>, needle: &str) {
    let text = err_text(res);
    println!("FAIL[{label}] {text}");
    assert!(text.contains(needle), "{label}: unexpected failure: {text}");
}

#[test]
fn open_record_first_call_creates_the_record() {
    let mut state = base_state();
    let pos = position(1);
    let id = liqrec_pda(caller_id(), &config_id(), &pos);
    assert_eq!(state.get_account_by_id(id), lee_core::account::Account::default());

    open_record("S3 open_record first", &mut state, pos, true).expect("first call creates");

    let (account, rec) = read_record(&state, id);
    assert_eq!(account.program_owner, caller_id(), "claimed by the caller program");
    assert_eq!(rec, LiqRecord { position: pos, count: 1 });
}

#[test]
fn open_record_second_call_increments_without_reclaiming() {
    let mut state = base_state();
    let pos = position(2);
    open_record("S3 open_record first", &mut state, pos, true).unwrap();
    open_record("S3 open_record second", &mut state, pos, true).expect("re-entry works");
    let (account, rec) = read_record(&state, liqrec_pda(caller_id(), &config_id(), &pos));
    assert_eq!(account.program_owner, caller_id());
    assert_eq!(rec.count, 2);
}

#[test]
fn open_auction_first_call_creates_record_and_auction() {
    let mut state = base_state();
    let pos = position(3);
    open_auction("S3 open_auction nonce0 (first use)", &mut state, pos, 0, true)
        .expect("first call creates record + auction");

    let (rec_acc, rec) = read_record(&state, liqrec_pda(caller_id(), &config_id(), &pos));
    assert_eq!(rec_acc.program_owner, caller_id());
    assert_eq!(rec.count, 1);

    let auction = state.get_account_by_id(auction_pda(caller_id(), &config_id(), &pos, 0));
    assert_eq!(auction.program_owner, caller_id());
    assert_eq!(
        Auction::try_from(&auction.data).unwrap(),
        Auction { position: pos, nonce: 0 }
    );
}

#[test]
fn open_auction_same_nonce_again_fails_and_next_nonce_succeeds() {
    let mut state = base_state();
    let pos = position(4);
    open_auction("S3 open_auction nonce0", &mut state, pos, 0, true).unwrap();

    // The second call with the same nonce fails. The `init` check on the (already claimed)
    // auction PDA fires first; the `nonce == count` assertion would reject it as well (count is 1).
    let res = open_auction("S3 replay nonce0", &mut state, pos, 0, false);
    expect_fail("replay nonce 0", res, "AccountAlreadyInitialized");

    // The next nonce succeeds.
    open_auction("S3 open_auction nonce1", &mut state, pos, 1, true).expect("nonce 1 succeeds");
    let (_, rec) = read_record(&state, liqrec_pda(caller_id(), &config_id(), &pos));
    assert_eq!(rec.count, 2);
    assert_eq!(
        Auction::try_from(
            &state
                .get_account_by_id(auction_pda(caller_id(), &config_id(), &pos, 1))
                .data
        )
        .unwrap()
        .nonce,
        1
    );

    // A skipped nonce fails too.
    let res = open_auction("S3 skip nonce", &mut state, pos, 5, false);
    expect_fail("skipped nonce", res, "auction nonce must equal the record count");
}

#[test]
fn open_auction_existing_uses_the_macro_generated_claim() {
    let mut state = base_state();
    let pos = position(5);

    // Without a record the strict variant refuses.
    let res = open_auction_existing("S3 existing without record", &mut state, pos, 0, false);
    expect_fail("existing w/o record", res, "record must be opened before the auction");

    open_record("S3 open_record", &mut state, pos, true).unwrap(); // count = 1
    open_auction_existing("S3 open_auction_existing nonce1", &mut state, pos, 1, true)
        .expect("macro auto-claim of an arg-seeded 4-seed PDA works");
    let auction = state.get_account_by_id(auction_pda(caller_id(), &config_id(), &pos, 1));
    assert_eq!(auction.program_owner, caller_id());

    let res = open_auction_existing("S3 existing replay", &mut state, pos, 1, false);
    expect_fail("existing replay", res, "AccountAlreadyInitialized");

    // A fresh auction address with a stale nonce hits the nonce assertion instead.
    let res = open_auction_existing("S3 existing stale nonce", &mut state, pos, 0, false);
    expect_fail("existing stale nonce", res, "auction nonce must equal the record count");
}

#[test]
fn offchain_derivation_equals_onchain_derivation() {
    let mut state = base_state();
    let pos = position(6);
    let cfg = config_id();

    // Independent re-derivation with SPEL's own host function (what the on-chain validation
    // calls): compute_pda(self_program_id, [seed bytes of each part]).
    let spel_rec = spel_framework::pda::compute_pda(
        &caller_id(),
        &[&seed_from_str(LIQREC_TAG), cfg.value(), &pos],
    );
    assert_eq!(liqrec_pda(caller_id(), &cfg, &pos), spel_rec);
    for nonce in [0u64, 1, 2, u64::MAX] {
        let spel_auc = spel_framework::pda::compute_pda(
            &caller_id(),
            &[&seed_from_str(AUCTION_TAG), cfg.value(), &pos, &u64_seed(nonce)],
        );
        assert_eq!(auction_pda(caller_id(), &cfg, &pos, nonce), spel_auc);
        // SPEL's ToSeed for u64 is the same little-endian zero padding.
        assert_eq!(
            <u64 as spel_framework::pda::ToSeed>::to_seed(&nonce),
            u64_seed(nonce)
        );
    }

    // On-chain: the accounts the program really claimed sit at exactly those addresses
    // (the program would have returned PdaMismatch otherwise, the claim would be rejected).
    open_auction("S3 offchain==onchain", &mut state, pos, 0, true).unwrap();
    open_auction("S3 offchain==onchain n1", &mut state, pos, 1, true).unwrap();
    for (id, label) in [
        (spel_rec, "record"),
        (auction_pda(caller_id(), &cfg, &pos, 0), "auction 0"),
        (auction_pda(caller_id(), &cfg, &pos, 1), "auction 1"),
    ] {
        assert_eq!(state.get_account_by_id(id).program_owner, caller_id(), "{label}");
    }

    // Distinct inputs give distinct PDAs.
    let other_pos = position(7);
    let other_cfg = AccountId::new([0xC1; 32]);
    assert_ne!(liqrec_pda(caller_id(), &cfg, &pos), liqrec_pda(caller_id(), &cfg, &other_pos));
    assert_ne!(liqrec_pda(caller_id(), &cfg, &pos), liqrec_pda(caller_id(), &other_cfg, &pos));
    assert_ne!(
        auction_pda(caller_id(), &cfg, &pos, 0),
        auction_pda(caller_id(), &cfg, &pos, 1)
    );
}

#[test]
fn wrong_pda_address_is_rejected() {
    let mut state = base_state();
    let pos = position(8);
    // Record account for a different position: the macro's PDA check must refuse.
    let accounts = vec![
        user_id(),
        config_id(),
        liqrec_pda(caller_id(), &config_id(), &position(9)),
    ];
    let res = run(
        "S3 wrong pda",
        &mut state,
        accounts,
        CallerInstruction::OpenRecord { position: pos },
        false,
    );
    expect_fail("wrong pda", res, "PdaMismatch");
}
