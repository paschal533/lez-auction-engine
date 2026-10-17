//! Golden tests of the admin, host and dev commands (spec §6.6, Task 15), against the SDK fixture.
//! FixtureChain does not execute programs: the "after" lines show the pre-state; the real post-state
//! is checked on localnet (Task 16).

mod common;
use auction_sdk::{
    chain::Chain,
    deployment::Deployment,
    testing::{id, standard},
};
use common::{check, value, Case};

fn params(file: &str) -> String {
    format!("{}/params/{file}", env!("CARGO_MANIFEST_DIR"))
}

fn sent_names(c: &Case) -> Vec<String> {
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

fn sent_host_debug(c: &Case) -> String {
    let sent = c.fx.chain.sent();
    let tx = sent.last().expect("sent");
    let ix: stablecoin_core::Instruction =
        risc0_zkvm::serde::from_slice(&tx.message().instruction_data)
            .expect("stablecoin instruction");
    format!("{ix:?}")
}

/// The sending dev commands act on a named dev network only (localnet, testnet-legacy), on the chain
/// the deployment file recorded (ADR-046).
fn localnet_case() -> Case {
    dev_network_case("localnet")
}

/// A deployment file naming `network` that records the fixture's own block 1.
fn dev_network_case(network: &str) -> Case {
    let c = Case::new();
    let genesis = hex::encode(c.fx.chain.blocks(1, 1).expect("block 1")[0].hash);
    Case {
        deployment: Some(Deployment {
            network: network.into(),
            chain_genesis: Some(genesis),
            ..standard::deployment()
        }),
        ..c
    }
}

fn wrong_host_case() -> Case {
    Case {
        deployment: Some(Deployment {
            host_program: [0xA2; 8],
            ..standard::deployment()
        }),
        ..Case::new()
    }
}

#[test]
fn admin_commands() {
    let protocol = standard::PROTOCOL.to_string();
    let engine_params = params("localnet-engine.json");
    let collateral_params = params("localnet-collateral.json");
    let cases: Vec<(&str, Vec<&str>, Option<&str>)> = vec![
        (
            "admin_addresses",
            vec![
                "admin",
                "addresses",
                "--admin",
                "engine-admin",
                "--salt",
                "1",
            ],
            None,
        ),
        ("admin_verify", vec!["admin", "verify"], None),
        (
            "admin_init_config",
            vec![
                "admin",
                "init-config",
                "--admin",
                "engine-admin",
                "--salt",
                "2",
                "--params",
                &engine_params,
                "--protocol-token",
                &protocol,
            ],
            Some("init_config"),
        ),
        (
            "admin_set_engine_params",
            vec![
                "admin",
                "set-engine-params",
                "--admin",
                "engine-admin",
                "--params",
                &engine_params,
                "--protocol-token",
                &protocol,
            ],
            Some("set_engine_params"),
        ),
        (
            "admin_set_collateral_config",
            vec![
                "admin",
                "set-collateral-config",
                "--admin",
                "engine-admin",
                "--params",
                &collateral_params,
            ],
            Some("set_collateral_config"),
        ),
    ];
    for (name, args, sent) in cases {
        let c = Case::new();
        let out = c.run(&args);
        assert!(out.ends_with("exit: 0\n"), "{name}: {out}");
        assert_eq!(
            sent_names(&c),
            sent.map(|s| vec![s.to_owned()]).unwrap_or_default(),
            "{name}"
        );
        check(name, &out);
    }
    let c = Case::new();
    let out = c.run(&[
        "admin",
        "addresses",
        "--admin",
        "engine-admin",
        "--salt",
        "1",
    ]);
    assert_eq!(value(&out, "config"), standard::config_id().to_string());
}

#[test]
fn admin_commands_on_an_existing_config_announce_the_admin() {
    let c = Case::new();
    let engine_params = params("localnet-engine.json");
    let protocol = standard::PROTOCOL.to_string();
    let out = c.run(&[
        "admin",
        "set-engine-params",
        "--admin",
        "engine-admin",
        "--params",
        &engine_params,
        "--protocol-token",
        &protocol,
    ]);
    assert_eq!(
        value(&out, "admin "),
        id(standard::k::ENGINE_ADMIN).to_string()
    );
    assert!(out.contains("admin source"), "{out}");
}

#[test]
fn set_engine_params_by_another_signer_is_refused_before_signing() {
    let c = Case::new();
    let engine_params = params("localnet-engine.json");
    let out = c.run(&[
        "admin",
        "set-engine-params",
        "--admin",
        "keeper",
        "--params",
        &engine_params,
    ]);
    assert!(out.contains("refusing before signing"), "{out}");
    assert!(out.ends_with("exit: 2\n"), "{out}");
    assert!(c.fx.chain.sent().is_empty());
}

#[test]
fn init_config_on_the_wrong_network_is_a_verification_failure() {
    let c = wrong_host_case();
    let engine_params = params("localnet-engine.json");
    let out = c.run(&[
        "admin",
        "init-config",
        "--admin",
        "engine-admin",
        "--salt",
        "2",
        "--params",
        &engine_params,
    ]);
    assert!(out.ends_with("exit: 2\n"), "{out}");
    assert!(c.fx.chain.sent().is_empty());
}

#[test]
fn host_bind() {
    let c = Case::new();
    let out = c.run(&["host", "bind", "--admin", "host-admin"]);
    assert!(out.contains("verified"), "{out}");
    assert_eq!(
        value(&out, "admin "),
        id(standard::k::ENGINE_ADMIN).to_string()
    );
    assert!(sent_host_debug(&c).starts_with("SetLiquidationEngine"));
    check("host_bind", &out);
}

#[test]
fn bind_wrong_host() {
    let c = wrong_host_case();
    let out = c.run(&["host", "bind", "--admin", "host-admin"]);
    assert!(
        out.contains("verification failed: host program id stored in the config"),
        "{out}"
    );
    assert!(out.ends_with("exit: 2\n"), "{out}");
    assert!(c.fx.chain.sent().is_empty(), "nothing is bound");
    check("bind_wrong_host", &out);
}

#[test]
fn bind_by_a_signer_that_is_not_the_host_admin_is_refused() {
    let c = Case::new();
    let out = c.run(&["host", "bind", "--admin", "keeper"]);
    assert!(out.contains("is not the host admin"), "{out}");
    assert!(out.ends_with("exit: 2\n"), "{out}");
    assert!(c.fx.chain.sent().is_empty());
}

#[test]
fn host_parameter_changes_by_a_signer_that_is_not_the_host_admin_are_refused() {
    let cases: [&[&str]; 2] = [
        &["host", "set-min-cr", "--admin", "keeper", "--ratio", "2"],
        &[
            "host",
            "set-controller-gains",
            "--admin",
            "keeper",
            "--kp",
            "1",
            "--ki",
            "1",
        ],
    ];
    for args in cases {
        let c = Case::new();
        let out = c.run(args);
        assert!(
            out.contains("verification failed: host admin (refusing before signing"),
            "{out}"
        );
        assert!(out.ends_with("exit: 2\n"), "{out}");
        assert!(c.fx.chain.sent().is_empty(), "nothing signed or sent");
    }
}

#[test]
fn host_set_controller_gains() {
    let c = Case::new();
    let out = c.run(&[
        "host",
        "set-controller-gains",
        "--admin",
        "host-admin",
        "--kp",
        "-1000000000000000000000000000",
        "--ki",
        "-1",
    ]);
    assert!(out.ends_with("exit: 0\n"), "{out}");
    let ix = sent_host_debug(&c);
    assert!(
        ix.contains("new_proportional_gain: -1000000000000000000000000000"),
        "{ix}"
    );
    assert!(ix.contains("new_integral_gain: -1"), "{ix}");
    check("host_set_controller_gains", &out);
}

#[test]
fn host_and_dev_commands() {
    let col = standard::COLLATERAL.to_string();
    let oracle = standard::ORACLE.to_string();
    let owner_coll_holding = id(standard::k::OWNER_COLL).to_string();
    let init = vec![
        "host",
        "init-program",
        "--admin",
        "host-admin",
        "--freeze",
        "freeze",
        "--collateral-def",
        &col,
        "--oracle",
        &oracle,
        "--kp",
        "-1",
    ];
    let cases: Vec<(&str, Vec<&str>)> = vec![
        ("host_addresses", vec!["host", "addresses"]),
        ("host_init_program", init.clone()),
        (
            "host_open_position",
            vec![
                "host",
                "open-position",
                "--owner",
                "owner",
                "--owner-collateral",
                "owner-coll",
                "--nonce",
                "3",
                "--amount",
                "500000",
            ],
        ),
        ("dev_resolve", vec!["dev", "resolve", "--label", "keeper"]),
        ("dev_chain_id", vec!["dev", "chain-id"]),
        (
            "dev_token_new",
            vec![
                "dev",
                "token-new",
                "--definition",
                "keeper",
                "--holding",
                "keeper-coll",
                "--name",
                "MOCK",
                "--supply",
                "10000",
            ],
        ),
        (
            "dev_oracle_create",
            vec![
                "dev",
                "oracle-create",
                "--price-source",
                "caller",
                "--base",
                &col,
                "--quote",
                &col,
                "--price",
                "1000",
            ],
        ),
        (
            "dev_balance",
            vec!["dev", "balance", "--account", &owner_coll_holding],
        ),
    ];
    for (name, args) in cases {
        let c = localnet_case();
        let out = c.run(&args);
        assert!(out.ends_with("exit: 0\n"), "{name}: {out}");
        check(name, &out);
    }
    let c = Case::new();
    c.run(&init);
    assert!(sent_host_debug(&c).contains("initial_controller_proportional_gain: -1"));
}

#[test]
fn sending_commands_refuse_at_ms_and_send_nothing() {
    let engine_params = params("localnet-engine.json");
    let cases: Vec<Vec<&str>> = vec![
        vec!["host", "bind", "--admin", "host-admin"],
        vec![
            "host",
            "set-min-cr",
            "--admin",
            "host-admin",
            "--ratio",
            "1",
        ],
        vec![
            "dev",
            "token-transfer",
            "--from",
            "keeper-coll",
            "--to",
            "owner-coll",
            "--amount",
            "1",
        ],
        vec![
            "admin",
            "set-engine-params",
            "--admin",
            "engine-admin",
            "--params",
            &engine_params,
        ],
    ];
    for args in cases {
        let c = Case::new();
        let mut argv = vec!["--at-ms", "5"];
        argv.extend(args.iter());
        let out = c.run(&argv);
        assert!(
            out.contains("--at-ms is for read commands"),
            "{args:?}: {out}"
        );
        assert!(out.ends_with("exit: 1\n"), "{out}");
        assert!(c.fx.chain.sent().is_empty());
    }
}

#[test]
fn set_engine_params_without_the_flag_on_a_config_with_a_token_is_refused() {
    let c = Case::new();
    let engine_params = params("localnet-engine.json");
    let out = c.run(&[
        "admin",
        "set-engine-params",
        "--admin",
        "engine-admin",
        "--params",
        &engine_params,
    ]);
    assert!(out.contains("would disable the protocol token"), "{out}");
    assert!(out.ends_with("exit: 1\n"), "{out}");
    assert!(c.fx.chain.sent().is_empty(), "nothing is sent");
}

#[test]
fn set_engine_params_can_disable_a_slot_only_when_asked_and_says_so() {
    let c = Case::new();
    let engine_params = params("localnet-engine.json");
    let out = c.run(&[
        "admin",
        "set-engine-params",
        "--admin",
        "engine-admin",
        "--params",
        &engine_params,
        "--disable-protocol-token",
    ]);
    assert!(
        out.ends_with(
            "exit: 0
"
        ),
        "{out}"
    );
    assert!(value(&out, "protocol token").ends_with("-> none"), "{out}");
    assert_eq!(sent_names(&c), vec!["set_engine_params"]);
}

#[test]
fn set_engine_params_announces_a_slot_change() {
    let c = Case::new();
    let engine_params = params("localnet-engine.json");
    let oracle = standard::ORACLE.to_string();
    let protocol = standard::PROTOCOL.to_string();
    let out = c.run(&[
        "admin",
        "set-engine-params",
        "--admin",
        "engine-admin",
        "--params",
        &engine_params,
        "--protocol-token",
        &protocol,
        "--ref-oracle",
        &oracle,
    ]);
    assert!(out.contains(&format!("none -> {oracle}")), "{out}");
    assert!(!out.contains("protocol token"), "unchanged slot: {out}");
}

#[test]
fn set_collateral_config_validates_before_signing() {
    let c = Case::new();
    let dir = tempfile::tempdir().expect("tmp");
    let file = dir.path().join("coll.json");
    let text = std::fs::read_to_string(params("localnet-collateral.json"))
        .expect("read")
        .replace("\"5000000\"", "\"50000000\"");
    std::fs::write(&file, text).expect("write");
    let out = c.run(&[
        "admin",
        "set-collateral-config",
        "--admin",
        "engine-admin",
        "--params",
        &file.display().to_string(),
    ]);
    assert!(out.contains("invalid collateral config"), "{out}");
    assert!(
        out.ends_with(
            "exit: 1
"
        ),
        "{out}"
    );
    assert!(c.fx.chain.sent().is_empty());
}

#[test]
fn a_protocol_token_that_is_not_a_fungible_definition_is_refused() {
    let engine_params = params("localnet-engine.json");
    let holding = id(standard::k::OWNER_COLL).to_string();
    let c = Case::new();
    for args in [
        vec![
            "admin",
            "init-config",
            "--admin",
            "engine-admin",
            "--salt",
            "2",
        ],
        vec!["admin", "set-engine-params", "--admin", "engine-admin"],
    ] {
        let mut argv = args.clone();
        argv.extend(["--params", &engine_params, "--protocol-token", &holding]);
        let out = c.run(&argv);
        assert!(out.contains("not a fungible token definition"), "{out}");
        assert!(
            out.ends_with(
                "exit: 1
"
            ),
            "{out}"
        );
    }
    assert!(c.fx.chain.sent().is_empty());
}

#[test]
fn init_config_by_a_signer_other_than_the_named_operator_is_exit_2() {
    let c = Case::new();
    let engine_params = params("localnet-engine.json");
    let out = c.run(&[
        "admin",
        "init-config",
        "--admin",
        "keeper",
        "--salt",
        "2",
        "--params",
        &engine_params,
    ]);
    assert!(out.contains("verification failed: engine admin"), "{out}");
    assert!(
        out.ends_with(
            "exit: 2
"
        ),
        "{out}"
    );
    assert!(c.fx.chain.sent().is_empty());
}

fn transfer_args() -> Vec<&'static str> {
    vec![
        "dev",
        "token-transfer",
        "--from",
        "owner-stable",
        "--to",
        "taker-stable",
        "--amount",
        "1",
    ]
}

#[test]
fn dev_commands_refuse_any_network_but_the_named_dev_networks() {
    let c = Case::new();
    let out = c.run(&transfer_args());
    assert!(
        out.contains("dev commands that send are for the named dev networks only"),
        "{out}"
    );
    assert!(
        out.ends_with(
            "exit: 1
"
        ),
        "{out}"
    );
    assert!(c.fx.chain.sent().is_empty());
}

#[test]
fn dev_commands_send_on_the_localnet_and_the_legacy_testnet() {
    for network in ["localnet", "testnet-legacy"] {
        let c = dev_network_case(network);
        let out = c.run(&transfer_args());
        assert!(
            out.ends_with(
                "exit: 0
"
            ),
            "{network}: {out}"
        );
        assert_eq!(c.fx.chain.sent().len(), 1, "{network}: one transfer sent");
    }
}

#[test]
fn dev_commands_refuse_other_network_names_even_on_the_recorded_chain() {
    // A label is never enough, and it is matched exactly: the recorded genesis does not widen the list.
    for network in [
        "testnet",
        "mainnet",
        "fixture",
        "Localnet",
        "testnet-legacy ",
        "",
    ] {
        let c = dev_network_case(network);
        let out = c.run(&transfer_args());
        assert!(
            out.contains("dev commands that send are for the named dev networks only"),
            "{network:?}: {out}"
        );
        assert!(
            out.contains("localnet, testnet-legacy"),
            "{network:?}: {out}"
        );
        assert!(
            out.ends_with(
                "exit: 1
"
            ),
            "{network:?}: {out}"
        );
        assert!(c.fx.chain.sent().is_empty(), "{network:?}: nothing sent");
    }
}

#[test]
fn dev_commands_on_the_legacy_testnet_refuse_a_node_with_another_genesis() {
    let mut c = dev_network_case("testnet-legacy");
    c.deployment = Some(Deployment {
        chain_genesis: Some("ab".repeat(32)),
        ..c.deployment.clone().expect("deployment")
    });
    let out = c.run(&transfer_args());
    assert!(out.contains("chain identity"), "{out}");
    assert!(
        out.ends_with(
            "exit: 2
"
        ),
        "{out}"
    );
    assert!(c.fx.chain.sent().is_empty(), "nothing signed or sent");
}

#[test]
fn dev_commands_on_the_legacy_testnet_refuse_a_file_without_a_genesis() {
    let mut c = dev_network_case("testnet-legacy");
    c.deployment = Some(Deployment {
        chain_genesis: None,
        ..c.deployment.clone().expect("deployment")
    });
    let out = c.run(&transfer_args());
    assert!(out.contains("records no chain_genesis_hash"), "{out}");
    assert!(
        out.ends_with(
            "exit: 2
"
        ),
        "{out}"
    );
    assert!(c.fx.chain.sent().is_empty(), "nothing signed or sent");
}

#[test]
fn dev_commands_refuse_a_node_with_another_genesis() {
    let mut c = localnet_case();
    c.deployment = Some(Deployment {
        chain_genesis: Some("ab".repeat(32)),
        ..c.deployment.clone().expect("deployment")
    });
    let out = c.run(&transfer_args());
    assert!(out.contains("chain identity"), "{out}");
    assert!(
        out.ends_with(
            "exit: 2
"
        ),
        "{out}"
    );
    assert!(c.fx.chain.sent().is_empty(), "nothing signed or sent");
}

#[test]
fn read_only_dev_commands_work_on_any_network() {
    let c = Case::new();
    let holding = id(standard::k::OWNER_COLL).to_string();
    for args in [
        vec!["dev", "resolve", "--label", "keeper"],
        vec!["dev", "balance", "--account", &holding],
        vec!["dev", "chain-id"],
    ] {
        let out = c.run(&args);
        assert!(
            out.ends_with(
                "exit: 0
"
            ),
            "{args:?}: {out}"
        );
    }
    let out = localnet_case().run(&transfer_args());
    assert!(
        out.ends_with(
            "exit: 0
"
        ),
        "the recorded chain: {out}"
    );
}

fn publish_args(price: u128) -> Vec<String> {
    [
        "dev",
        "oracle-publish",
        "--price-source",
        "caller",
        "--price",
        &price.to_string(),
        "--host-admin",
        "host-admin",
        "--engine-admin",
        "engine-admin",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

fn run_strings(c: &Case, args: &[String]) -> String {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    c.run(&refs)
}

#[test]
fn dev_oracle_publish_creates_rotates_and_follows() {
    let c = localnet_case();
    let out = run_strings(&c, &publish_args(standard::R * 19 / 10));
    assert!(out.ends_with("exit: 0\n"), "{out}");
    assert_eq!(
        c.fx.chain.sent().len(),
        3,
        "price account, host rotation, engine follow-up"
    );
    check("dev_oracle_publish", &out);
}

#[test]
fn dev_oracle_publish_refuses_an_invalid_price_before_signing() {
    let c = localnet_case();
    let out = run_strings(&c, &publish_args(standard::R * 3));
    assert!(out.contains("PriceFeedOutOfRange"), "{out}");
    assert!(out.ends_with("exit: 1\n"), "{out}");
    assert!(c.fx.chain.sent().is_empty(), "nothing signed");
}

#[test]
fn dev_keeper_publishes_only_near_the_age_limit() {
    let c = localnet_case();
    let mut args = publish_args(standard::R);
    args[1] = "keeper".into();
    args.extend(["--iterations", "1", "--interval-ms", "0"].map(String::from));
    let out = run_strings(&c, &args);
    assert!(out.contains("fresh"), "{out}");
    assert!(c.fx.chain.sent().is_empty(), "a fresh oracle is left alone");
    args.extend(["--refresh-before-ms", "999999999"].map(String::from));
    let out = run_strings(&c, &args);
    assert!(out.contains("publishing"), "{out}");
    assert_eq!(c.fx.chain.sent().len(), 3);
}

#[test]
fn host_status_reads_the_protocol_parameters() {
    let c = Case::new();
    let out = c.run(&["host", "status"]);
    assert!(out.contains("frozen            no"), "{out}");
    check("host_status", &out);
}

#[test]
fn dev_oracle_publish_refuses_signers_that_are_not_the_admins_before_signing() {
    let c = localnet_case();
    for (flag, label) in [("--host-admin", "caller"), ("--engine-admin", "caller")] {
        let mut args = publish_args(standard::R);
        let at = args.iter().position(|a| a == flag).expect("flag") + 1;
        args[at] = label.into();
        let out = run_strings(&c, &args);
        assert!(out.contains("refusing before signing"), "{flag}: {out}");
        assert!(out.ends_with("exit: 2\n"), "{flag}: {out}");
    }
    assert!(c.fx.chain.sent().is_empty(), "nothing signed or sent");
}

#[test]
fn dev_oracle_publish_needs_the_twap_program_in_the_deployment_file() {
    let mut c = localnet_case();
    let d = c.deployment.take().expect("localnet deployment");
    c.deployment = Some(Deployment {
        twap_oracle_program: None,
        ..d
    });
    let out = run_strings(&c, &publish_args(standard::R));
    assert!(out.contains("twap_oracle_program_id"), "{out}");
    assert!(out.ends_with("exit: 1\n"), "{out}");
    assert!(c.fx.chain.sent().is_empty(), "nothing signed or sent");
}
