//! Shared by the golden test crates: the fixture case, normalisation and golden-file check.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use auction_cli::{args::Cli, env::Env, run_with};
use auction_sdk::{
    confirm::ConfirmSettings,
    deployment::Deployment,
    testing::{
        standard::{self, Fixture},
        write_keystore, STANDARD_LABELS,
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
