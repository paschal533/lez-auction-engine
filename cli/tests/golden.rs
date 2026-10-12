//! Golden tests (spec §9 "CLI golden tests"): every command against the SDK's standard fixture.
//! Regenerate with `UPDATE_GOLDEN=1` and review the diff; never edit a golden file by hand to make a
//! test pass. Each test also asserts its key numbers independently of the golden file.

use std::path::{Path, PathBuf};
use std::time::Duration;

use auction_cli::{args::Cli, env::Env, run_with};
use auction_sdk::{
    confirm::ConfirmSettings,
    deployment::Deployment,
    testing::{
        standard::{self, Fixture},
        write_keystore, SendMode, STANDARD_LABELS,
    },
};
use clap::Parser;

pub struct Case {
    pub fx: Fixture,
    pub dir: tempfile::TempDir,
    pub with_wallet: bool,
    pub deployment: Option<Deployment>,
}

impl Default for Case {
    fn default() -> Self {
        Self::new()
    }
}

impl Case {
    pub fn new() -> Self {
        Self {
            fx: standard::standard(),
            dir: tempfile::tempdir().expect("tmp"),
            with_wallet: true,
            deployment: None,
        }
    }

    pub fn cursor_path(&self) -> PathBuf {
        self.dir
            .path()
            .join("state")
            .join(format!("{}.cursor.json", standard::config_id()))
    }

    pub fn write_cursor(&self, json: &str) {
        let path = self.cursor_path();
        std::fs::create_dir_all(path.parent().expect("parent")).expect("state dir");
        std::fs::write(path, json).expect("cursor");
    }

    pub fn run(&self, args: &[&str]) -> String {
        self.run_split(args).0
    }

    /// stdout (with the `exit:` line) and stderr, both normalised.
    pub fn run_split(&self, args: &[&str]) -> (String, String) {
        self.run_split_with(
            args,
            ConfirmSettings {
                poll_interval: Duration::ZERO,
                drop_after_blocks: 5,
                max_polls: 50,
            },
        )
    }

    /// `run_split` with explicit confirmation settings.
    pub fn run_split_with(&self, args: &[&str], confirm: ConfirmSettings) -> (String, String) {
        let wallet = self.dir.path().join("wallet");
        if self.with_wallet {
            write_keystore(&wallet, &STANDARD_LABELS).expect("wallet");
        }
        let mut argv = vec!["auction-cli"];
        argv.extend_from_slice(args);
        let cli = Cli::try_parse_from(argv).expect("arguments parse");
        let env = Env {
            chain: &self.fx.chain,
            deployment: self
                .deployment
                .clone()
                .unwrap_or_else(|| self.fx.deployment.clone()),
            config: cli.config.as_deref().map(|s| s.parse().expect("config id")),
            wallet_home: Some(wallet),
            state_dir: self.dir.path().join("state"),
            at_ms: cli.at_ms,
            confirm,
            sleep: |_| {},
        };
        let mut out = Vec::new();
        let mut err = Vec::new();
        let result = run_with(&cli.command, &env, &mut out, &mut err);
        let mut text = String::from_utf8(out).expect("utf-8");
        match result {
            Ok(()) => text.push_str("exit: 0\n"),
            Err(e) => text.push_str(&format!("error: {e}\nexit: {}\n", e.exit_code())),
        }
        let stderr = String::from_utf8(err).expect("utf-8");
        (
            normalise(&text, self.dir.path()),
            stderr.replace(&self.dir.path().display().to_string(), "<tmp>"),
        )
    }
}

fn normalise(text: &str, tmp: &Path) -> String {
    let replaced = text.replace(&tmp.display().to_string(), "<tmp>");
    let mut lines: Vec<String> = replaced
        .lines()
        .map(|l| {
            if l.starts_with("submitted") {
                format!("{:<17} <hash>", "submitted")
            } else {
                l.to_owned()
            }
        })
        .collect();
    lines.push(String::new());
    lines.join("\n")
}

pub fn check(name: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.txt"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("golden dir");
        std::fs::write(&path, actual).expect("write golden");
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing {}: run with UPDATE_GOLDEN=1 and review it",
            path.display()
        )
    });
    assert_eq!(actual, expected, "golden {name} changed");
}

pub fn value<'a>(text: &'a str, key: &str) -> &'a str {
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .map(str::trim)
        .unwrap_or_else(|| panic!("no line {key:?} in\n{text}"))
}

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

// ---- Task 14: transaction commands ----

fn sent_engine_names(c: &Case) -> Vec<String> {
    c.fx.chain
        .sent()
        .iter()
        .map(|tx| {
            let ix: auction_engine_core::Instruction =
                risc0_zkvm::serde::from_slice(&tx.message().instruction_data)
                    .expect("engine instruction");
            ix.name().to_owned()
        })
        .collect()
}

#[test]
fn liquidate() {
    let c = Case::new();
    let p = c.fx.unsafe_position.to_string();
    let out = c.run(&[
        "liquidate",
        "--position",
        &p,
        "--keeper",
        "keeper",
        "--keeper-holding",
        "keeper-coll",
    ]);
    assert_eq!(
        value(&out, "plan"),
        "liquidate (17 accounts, 2 signer(s))",
        "fresh reward holding signs"
    );
    assert_eq!(value(&out, "included"), "signer nonce 1");
    assert_eq!(sent_engine_names(&c), vec!["liquidate"]);
    check("liquidate", &out);
}

#[test]
fn liquidate_refuses_a_position_that_is_safe() {
    let c = Case::new();
    let r = auction_sdk::decode::Reader::new(&c.fx.chain, standard::ENGINE);
    let mut pp: stablecoin_core::ProtocolParameters = r
        .host(
            &c.fx.host.protocol_parameters,
            &standard::HOST,
            "ProtocolParameters",
        )
        .expect("pp");
    pp.minimum_collateralization_ratio = standard::ONE * 3 / 2; // 500_000 >= 600_000 x 0.5 x 1.5
    c.fx.chain
        .put_state(c.fx.host.protocol_parameters, standard::HOST, &pp);
    let p = c.fx.unsafe_position.to_string();
    let out = c.run(&[
        "liquidate",
        "--position",
        &p,
        "--keeper",
        "keeper",
        "--keeper-holding",
        "keeper-coll",
    ]);
    assert!(out.contains("refusing before signing"), "{out}");
    assert!(out.ends_with("exit: 1\n"), "{out}");
    assert!(c.fx.chain.sent().is_empty());
    check("liquidate_safe_position", &out);
}

fn bid_args(a: &str, coin: &str) -> Vec<String> {
    [
        "bid",
        "--auction",
        a,
        "--coin-in-max",
        coin,
        "--bidder-stable",
        "bidder-stable",
        "--bidder-collateral",
        "bidder-coll",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

fn run_owned(c: &Case, args: &[String]) -> String {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    c.run(&refs)
}

#[test]
fn bid_partial() {
    let c = Case::new();
    let out = run_owned(&c, &bid_args(&c.fx.live_auction.to_string(), "100000"));
    assert_eq!(value(&out, "quoted out"), "51963");
    assert_eq!(value(&out, "min out"), "51703");
    assert_eq!(value(&out, "plan"), "bid (11 accounts, 1 signer(s))");
    check("bid_partial", &out);
}

#[test]
fn bid_final() {
    let c = Case::new();
    let out = run_owned(&c, &bid_args(&c.fx.live_auction.to_string(), "660000"));
    assert_eq!(
        value(&out, "plan"),
        "bid_with_settlement (12 accounts, 1 signer(s))"
    );
    assert_eq!(sent_engine_names(&c), vec!["bid_with_settlement"]);
    check("bid_final", &out);
}

#[test]
fn bid_with_a_min_out_above_the_quote_is_refused_before_signing() {
    let c = Case::new();
    let mut args = bid_args(&c.fx.live_auction.to_string(), "100000");
    args.extend(["--min-out".to_owned(), "51964".to_owned()]);
    let out = run_owned(&c, &args);
    assert!(out.contains("Slippage"), "{out}");
    assert!(out.ends_with("exit: 1\n"), "{out}");
    assert!(c.fx.chain.sent().is_empty());
}

#[test]
fn bid_dropped() {
    let c = Case::new();
    c.fx.chain.set_send_mode(SendMode::Drop);
    c.fx.chain.set_height_step(1);
    let out = run_owned(&c, &bid_args(&c.fx.live_auction.to_string(), "100000"));
    assert!(
        out.contains("nothing was applied; re-quote and resend"),
        "{out}"
    );
    assert!(out.ends_with("exit: 3\n"), "{out}");
    assert_eq!(c.fx.chain.sent().len(), 1, "never resent");
    let locks = c.dir.path().join("state/locks");
    assert!(
        std::fs::read_dir(&locks).map_or(true, |mut d| d.next().is_none()),
        "lock released"
    );
    check("bid_dropped", &out);
}

#[test]
fn bid_without_a_verdict_keeps_the_lock_and_says_how_to_check() {
    // Not a golden: the point is the lock, the exit code and that nothing is resent.
    let c = Case::new();
    c.fx.chain.set_send_mode(SendMode::Drop);
    let args = bid_args(&c.fx.live_auction.to_string(), "100000");
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let few_polls = ConfirmSettings {
        poll_interval: Duration::ZERO,
        drop_after_blocks: 5,
        max_polls: 2,
    };
    let out = c.run_split_with(&refs, few_polls).0;
    assert!(out.contains("outcome unknown, lock kept"), "{out}");
    assert!(out.contains("auction-cli list-auctions"), "{out}");
    assert!(out.ends_with("exit: 1\n"), "{out}");
    assert_eq!(c.fx.chain.sent().len(), 1, "never resent");
    let locks = c.dir.path().join("state/locks");
    assert!(
        std::fs::read_dir(&locks).is_ok_and(|mut d| d.next().is_some()),
        "lock kept"
    );
}

#[test]
fn a_tx_command_without_a_keystore_is_exit_4_and_sends_nothing() {
    let c = Case {
        with_wallet: false,
        ..Case::new()
    };
    let out = run_owned(&c, &bid_args(&c.fx.live_auction.to_string(), "100000"));
    assert!(
        out.contains("keystore: wallet storage not found at <tmp>/wallet/storage.json"),
        "{out}"
    );
    assert!(out.ends_with("exit: 4\n"), "{out}");
    assert!(c.fx.chain.sent().is_empty());
    check("keystore_missing_tx", &out);
}

#[test]
fn system_and_maintenance_commands() {
    let a = standard::standard().live_auction.to_string();
    let cases: Vec<(&str, Vec<&str>, Option<&str>)> = vec![
        (
            "settle_debt",
            vec!["settle-debt", "--caller", "caller"],
            Some("settle_debt"),
        ),
        (
            "check_solvency",
            vec!["check-solvency", "--caller", "caller"],
            Some("check_solvency"),
        ),
        (
            "observe_feed",
            vec!["observe-feed", "--caller", "caller"],
            Some("observe_feed"),
        ),
        (
            "start_surplus_while_live",
            vec!["start-surplus", "--caller", "caller"],
            None,
        ),
        (
            "take_surplus",
            vec![
                "take-surplus",
                "--payer",
                "taker-protocol",
                "--receiver",
                "taker-stable",
                "--amount",
                "500",
            ],
            Some("take_surplus"),
        ),
        (
            "take_debt",
            vec![
                "take-debt",
                "--payer",
                "taker-stable",
                "--receiver",
                "taker-protocol",
                "--amount",
                "500",
            ],
            Some("take_debt"),
        ),
        (
            "close_surplus",
            vec!["close-surplus", "--caller", "caller"],
            Some("close_surplus_auction"),
        ),
        (
            "close_debt",
            vec!["close-debt", "--caller", "caller"],
            Some("close_debt_auction"),
        ),
        (
            "claim_leftover",
            vec![
                "claim-leftover",
                "--owner",
                "owner2",
                "--owner-holding",
                "owner-coll",
                "--auction",
                &a,
            ],
            Some("claim_leftover"),
        ),
        (
            "settle_empty",
            vec!["settle-empty", "--caller", "caller", "--auction", &a],
            Some("settle_empty"),
        ),
    ];
    for (name, args, sent) in cases {
        let c = Case::new();
        let out = c.run(&args);
        match sent {
            Some(ix) => {
                assert!(out.ends_with("exit: 0\n"), "{name}: {out}");
                assert_eq!(sent_engine_names(&c), vec![ix], "{name}");
            }
            None => {
                assert!(out.ends_with("exit: 1\n"), "{name}: {out}");
                assert!(c.fx.chain.sent().is_empty(), "{name}");
            }
        }
        check(name, &out);
    }
}

#[test]
fn every_tx_command_announces_the_admin_before_it_signs() {
    let c = Case::new();
    let out = c.run(&["observe-feed", "--caller", "caller"]);
    let admin = out.find("admin ").expect("admin line");
    let plan = out.find("plan ").expect("plan line");
    assert!(admin < plan, "{out}");
    assert!(out.contains("admin source"), "{out}");
}

#[test]
fn take_quotes_set_the_slippage_bounds() {
    let c = Case::new();
    let out = c.run(&[
        "take-surplus",
        "--payer",
        "taker-protocol",
        "--receiver",
        "taker-stable",
        "--amount",
        "500",
    ]);
    assert_eq!(value(&out, "quoted tokens"), "200");
    assert_eq!(value(&out, "max protocol in"), "201");
    let c = Case::new();
    let out = c.run(&[
        "take-debt",
        "--payer",
        "taker-stable",
        "--receiver",
        "taker-protocol",
        "--amount",
        "500",
    ]);
    assert_eq!(value(&out, "quoted tokens"), "325");
    assert_eq!(value(&out, "min protocol out"), "323");
}

#[test]
fn watch_observe() {
    let c = Case::new();
    let out = c.run(&[
        "watch",
        "--iterations",
        "1",
        "--interval-ms",
        "0",
        "--observe-with",
        "caller",
    ]);
    assert_eq!(sent_engine_names(&c), vec!["observe_feed"]);
    check("watch_observe", &out);
}
