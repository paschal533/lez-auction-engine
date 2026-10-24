//! Shared fixtures: a `Ctx` over the standard fixture chain (no node).
#![allow(dead_code, reason = "each test binary uses a different part")]

use std::path::Path;

use auction_ffi::ctx::{Ctx, CtxJson};
use auction_sdk::testing::{standard, write_keystore, FixtureChain, STANDARD_LABELS};
use tempfile::TempDir;

pub struct World {
    pub chain: &'static FixtureChain,
    pub ctx: Ctx,
    pub json: CtxJson,
    pub dir: TempDir,
}

/// The standard fixture as a dev network of the given name, wallet with the standard labels.
pub fn world(network: &str) -> World {
    let dir = tempfile::tempdir().expect("tmp");
    let state_dir = dir.path().join("state");
    let wallet = dir.path().join("wallet");
    write_keystore(&wallet, &STANDARD_LABELS).expect("keystore");
    let fx = standard::standard();
    let genesis = fx.chain.blocks(1, 1).expect("block 1")[0].hash;
    let mut deployment = fx.deployment.clone();
    deployment.network = network.to_owned();
    deployment.chain_genesis = Some(hex::encode(genesis));
    let chain: &'static FixtureChain = Box::leak(Box::new(fx.chain));
    let json = CtxJson {
        deployment: dir.path().join("deployment.json"),
        state_dir,
        wallet_home: Some(wallet),
        config: None,
        drop_after_blocks: None,
    };
    let ctx = Ctx::with_chain(Box::new(chain), deployment, &json).expect("ctx");
    World {
        chain,
        ctx,
        json,
        dir,
    }
}

use auction_sdk::Chain as _;

pub fn lock_dir(state_dir: &Path) -> std::path::PathBuf {
    state_dir.join("locks")
}
