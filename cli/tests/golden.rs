//! Golden tests (spec §9 "CLI golden tests"): every command against the SDK's standard fixture.
//! Regenerate with `UPDATE_GOLDEN=1` and review the diff; never edit a golden file by hand to make a
//! test pass. Each test also asserts its key numbers independently of the golden file.

mod common;
use auction_sdk::{
    deployment::Deployment,
    testing::{standard, STANDARD_LABELS},
};
use common::*;

#[test]
fn quote_liquidation() {
    let c = Case::new();
    let p = c.fx.unsafe_position.to_string();
    let out = c.run(&["quote-liquidation", "--position", &p]);
    assert_eq!(value(&out, "unsafe (min CR)"), "yes");
    assert_eq!(value(&out, "amount to raise"), "660000");
    assert_eq!(value(&out, "keeper reward"), "25");
    assert_eq!(value(&out, "lot "), "499975");
    assert_eq!(value(&out, "nonce"), "0");
    assert!(out.ends_with("exit: 0\n"));
    check("quote_liquidation", &out);
}

#[test]
fn quote_bid() {
    let c = Case::new();
    let a = c.fx.live_auction.to_string();
    let out = c.run(&["quote-bid", "--auction", &a, "--coin-in-max", "100000"]);
    assert_eq!(value(&out, "t_eff (ms)"), "600000");
    assert_eq!(value(&out, "collateral out"), "51963");
    assert_eq!(value(&out, "min out (50 bps)"), "51703");
    assert_eq!(value(&out, "instruction"), "bid");
    check("quote_bid", &out);
}

#[test]
fn quote_bid_of_a_non_auction_account_is_refused() {
    // A host-owned account where an engine Auction is expected: the decoder's owner check is a
    // mismatch, reported like any verification failure (exit 2), before anything is quoted.
    let c = Case::new();
    let p = c.fx.unsafe_position.to_string();
    let out = c.run(&["quote-bid", "--auction", &p, "--coin-in-max", "1"]);
    assert!(out.contains("verification failed: Auction"), "{out}");
    assert!(out.ends_with("exit: 2\n"), "{out}");
    check("quote_bid_not_an_auction", &out);
}

#[test]
fn list_auctions() {
    let c = Case::new();
    let out = c.run(&["list-auctions"]);
    assert_eq!(value(&out, "scanned to block"), "3");
    assert!(out.contains(&format!("{}  live  lot 475000", c.fx.live_auction)));
    assert!(!out.contains("held at max discount"));
    check("list_auctions", &out);
}

#[test]
fn list_auctions_shows_an_auction_held_at_max_discount() {
    let c = Case::new();
    let later = (standard::T0 + 2_100_000).to_string();
    let out = c.run(&["--at-ms", &later, "list-auctions"]);
    assert!(
        out.contains("held at max discount (keeps its cap reservation of 660000)"),
        "{out}"
    );
    check("list_auctions_held_at_max", &out);
}

#[test]
fn list_at_risk() {
    let c = Case::new();
    let out = c.run(&["list-at-risk"]);
    assert!(out.contains(&format!("{}  owner", c.fx.unsafe_position)));
    assert!(
        !out.contains(&c.fx.auction_position.to_string()),
        "zero-debt position not listed"
    );
    check("list_at_risk", &out);
}

#[test]
fn rescan_from_the_first_block() {
    let c = Case::new();
    let out = c.run(&["rescan", "--from", "1"]);
    assert_eq!(value(&out, "scanned to block"), "3");
    assert_eq!(value(&out, "auctions"), "1");
    assert_eq!(value(&out, "positions"), "2");
    check("rescan", &out);
}

#[test]
fn watch_two_rounds() {
    let c = Case::new();
    let out = c.run(&["watch", "--iterations", "2", "--interval-ms", "0"]);
    assert_eq!(out.matches("scanned to block").count(), 2);
    check("watch", &out);
}

#[test]
fn config_names_the_admin_and_where_it_comes_from() {
    let c = Case::new();
    let out = c.run(&["config"]);
    assert_eq!(
        value(&out, "admin "),
        auction_sdk::testing::id(standard::k::ENGINE_ADMIN).to_string()
    );
    assert!(
        value(&out, "admin source").starts_with("deployment file"),
        "{out}"
    );
    check("config", &out);
}

#[test]
fn config_without_a_named_operator_prints_the_chain_admin_and_says_so() {
    let mut c = Case::new();
    c.deployment = Some(Deployment {
        engine_admin: None,
        ..c.fx.deployment.clone()
    });
    let out = c.run(&["config"]);
    assert_eq!(
        value(&out, "admin "),
        auction_sdk::testing::id(standard::k::ENGINE_ADMIN).to_string()
    );
    assert!(
        value(&out, "admin source").starts_with("chain only"),
        "{out}"
    );
    check("config_no_operator", &out);
}

#[test]
fn a_config_on_another_engine_is_refused_with_exit_2() {
    let mut c = Case::new();
    c.deployment = Some(Deployment {
        engine_program: [1; 8],
        ..c.fx.deployment.clone()
    });
    let p = c.fx.unsafe_position.to_string();
    let out = c.run(&["quote-liquidation", "--position", &p]);
    assert!(out.contains("verification failed: engine program"), "{out}");
    assert!(out.ends_with("exit: 2\n"), "{out}");
    check("verify_wrong_engine", &out);
}

#[test]
fn a_missing_keystore_is_exit_4() {
    let mut c = Case::new();
    c.with_wallet = false;
    // "nolabel" is not a base58 id, so it needs the keystore, which does not exist.
    let out = c.run(&["quote-liquidation", "--position", "nolabel"]);
    assert!(out.contains("error: keystore: "), "{out}");
    assert!(out.ends_with("exit: 4\n"), "{out}");
    check("keystore_missing", &out);
}

#[test]
fn wallet_labels_resolve_to_accounts() {
    let c = Case::new();
    let label = STANDARD_LABELS[0];
    let by_label = c.run(&["quote-liquidation", "--position", label.0]);
    // Whatever the label is, it resolves (no keystore error); an unrelated account then fails to
    // decode as a position, which is a verification-class refusal, not a keystore error.
    assert!(!by_label.contains("keystore"), "{by_label}");
}

#[test]
fn rescan_from_an_unknown_block_is_exit_1() {
    let c = Case::new();
    let out = c.run(&["rescan", "--from", "99"]);
    assert!(out.contains("error: "), "{out}");
    assert!(out.ends_with("exit: 1\n"), "{out}");
    check("rescan_unknown_block", &out);
}

#[test]
fn a_corrupt_cursor_is_an_error_and_rescan_from_1_repairs_it() {
    let c = Case::new();
    c.write_cursor("{not json");
    let broken = c.run(&["list-auctions"]);
    assert!(broken.contains("rescan --from 1"), "{broken}");
    assert!(broken.ends_with("exit: 1\n"), "{broken}");
    let repaired = c.run(&["rescan", "--from", "1"]);
    assert_eq!(value(&repaired, "auctions"), "1");
    assert!(repaired.ends_with("exit: 0\n"), "{repaired}");
    // A rescan from later on must not hide the corruption.
    c.write_cursor("{not json");
    let later = c.run(&["rescan", "--from", "2"]);
    assert!(later.ends_with("exit: 1\n"), "{later}");
    check("cursor_corrupt", &broken);
}

#[test]
fn candidates_that_no_longer_decode_are_dropped_from_the_lists() {
    let c = Case::new();
    let bogus = auction_cli_test_id(0x77);
    let not_an_auction = c.fx.unsafe_position.to_string();
    c.write_cursor(&format!(
        r#"{{"last_block":0,"last_hash":"","auctions":["{bogus}","{not_an_auction}"],"positions":["{bogus}"]}}"#
    ));
    let out = c.run(&["list-auctions"]);
    assert!(
        out.contains(&format!("{}  live", c.fx.live_auction)),
        "{out}"
    );
    assert!(!out.contains(&bogus), "{out}");
    assert!(!out.contains(&format!("{not_an_auction}  live")), "{out}");
    let risk = c.run(&["list-at-risk"]);
    assert!(!risk.contains(&bogus), "{risk}");
    assert!(risk.ends_with("exit: 0\n"), "{risk}");
}

fn auction_cli_test_id(byte: u8) -> String {
    lee_core::account::AccountId::new([byte; 32]).to_string()
}

#[test]
fn a_smaller_scan_batch_gives_the_same_lists() {
    let c = Case::new();
    let default = c.run(&["list-auctions"]);
    let c2 = Case::new();
    let small = c2.run(&["list-auctions", "--batch", "1"]);
    assert_eq!(default, small);
    let rescan = c2.run(&["rescan", "--from", "1", "--batch", "2"]);
    assert_eq!(value(&rescan, "scanned to block"), "3");
    let risk = c2.run(&["list-at-risk", "--batch", "1"]);
    assert!(risk.contains(&c2.fx.unsafe_position.to_string()), "{risk}");
}

fn put_auction_copy(c: &Case, at: lee_core::account::AccountId) {
    use auction_sdk::chain::Chain;
    let acc = c.fx.chain.account(&c.fx.live_auction).expect("auction");
    c.fx.chain.put(at, acc);
}

#[test]
fn a_mismatched_candidate_is_warned_about_on_stderr_and_the_rest_is_listed() {
    let c = Case::new();
    let not_an_auction = c.fx.unsafe_position.to_string();
    c.write_cursor(&format!(
        r#"{{"last_block":0,"last_hash":"","auctions":["{not_an_auction}"],"positions":[]}}"#
    ));
    let (out, err) = c.run_split(&["list-auctions"]);
    assert!(
        out.contains(&format!("{}  live", c.fx.live_auction)),
        "{out}"
    );
    assert!(!out.contains("warning"), "stdout stays clean: {out}");
    assert!(
        err.contains(&format!("warning: skipped {not_an_auction}: ")) && err.contains("Auction"),
        "{err}"
    );
    assert!(out.ends_with("exit: 0\n"), "{out}");
}

#[test]
fn an_engine_rejected_candidate_is_warned_about_on_stderr_and_the_rest_is_listed() {
    let c = Case::new();
    // The live auction's account copied to an id it does not derive from: the engine's account
    // check refuses it (WrongAccount), which is an Engine error, not a stale leftover.
    let wrong = lee_core::account::AccountId::new([0x66; 32]);
    put_auction_copy(&c, wrong);
    c.write_cursor(&format!(
        r#"{{"last_block":0,"last_hash":"","auctions":["{wrong}"],"positions":[]}}"#
    ));
    let (out, err) = c.run_split(&["list-auctions"]);
    assert!(
        out.contains(&format!("{}  live", c.fx.live_auction)),
        "{out}"
    );
    assert!(
        err.contains(&format!(
            "warning: skipped {wrong}: the engine would reject this: "
        )),
        "{err}"
    );
    assert!(!out.contains(&wrong.to_string()), "{out}");
}

#[test]
fn stale_leftovers_are_dropped_without_a_warning() {
    let c = Case::new();
    let gone = auction_cli_test_id(0x77);
    c.write_cursor(&format!(
        r#"{{"last_block":0,"last_hash":"","auctions":["{gone}"],"positions":["{gone}"]}}"#
    ));
    let (_, err) = c.run_split(&["list-auctions"]);
    assert_eq!(err, "");
    let (_, err) = c.run_split(&["list-at-risk"]);
    assert_eq!(err, "");
}

#[test]
fn rescan_from_beyond_the_cursor_says_which_blocks_it_skips() {
    let c = Case::new();
    let (out, err) = c.run_split(&["rescan", "--from", "3"]);
    assert!(out.ends_with("exit: 0\n"), "{out}");
    assert_eq!(err, "note: blocks 1..2 skipped\n");
    // Continuing from where the cursor is now needs no note.
    let (_, err) = c.run_split(&["rescan", "--from", "3"]);
    assert_eq!(err, "");
}

#[test]
fn rescan_from_1_over_a_corrupt_cursor_says_it_is_rebuilding() {
    let c = Case::new();
    c.write_cursor("{not json");
    let (out, err) = c.run_split(&["rescan", "--from", "1"]);
    assert!(out.ends_with("exit: 0\n"), "{out}");
    assert_eq!(err, "cursor file unreadable; rebuilding from block 1\n");
    let (_, err) = c.run_split(&["rescan", "--from", "1"]);
    assert_eq!(err, "");
}
