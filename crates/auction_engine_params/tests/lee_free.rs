//! `auction_engine_params` must stay usable from the LEZ v0.4 workspace, so it may not depend on
//! the v0.2.4 `lee` / `lee_core` crates.

use std::process::Command;

#[test]
fn crate_has_no_lee_dependency() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    let out = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
            manifest,
        ])
        .output()
        .expect("cargo metadata runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).expect("metadata is JSON");
    let pkg = meta["packages"]
        .as_array()
        .expect("packages array")
        .iter()
        .find(|p| p["name"] == "auction_engine_params")
        .expect("auction_engine_params package");
    let deps: Vec<&str> = pkg["dependencies"]
        .as_array()
        .expect("dependencies array")
        .iter()
        .map(|d| d["name"].as_str().expect("dep name"))
        .collect();
    for banned in ["lee", "lee_core"] {
        assert!(!deps.contains(&banned), "depends on {banned}: {deps:?}");
    }
}
