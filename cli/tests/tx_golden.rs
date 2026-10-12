//! Golden tests of the transaction commands (spec §6.6, Task 14), against the SDK fixture.
//! FixtureChain does not execute programs: the "after" lines show the pre-state; the real
//! post-state is checked on localnet (Task 16).

use std::time::Duration;

use auction_sdk::{
    confirm::ConfirmSettings,
    testing::{standard, SendMode},
};

mod common;
use common::{check, value, Case};

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

#[test]
fn a_tx_command_refuses_at_ms() {
    // --at-ms invents a clock; slippage bounds and pre-checks must come from the chain clock.
    let c = Case::new();
    let mut args = vec!["--at-ms".to_owned(), "1".to_owned()];
    args.extend(bid_args(&c.fx.live_auction.to_string(), "100000"));
    let out = run_owned(&c, &args);
    assert!(out.contains("--at-ms is for read commands"), "{out}");
    assert!(
        out.ends_with(
            "exit: 1
"
        ),
        "{out}"
    );
    assert!(c.fx.chain.sent().is_empty(), "nothing is sent");
    check("bid_at_ms_refused", &out);
}
