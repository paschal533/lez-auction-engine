//! `auction-cli` (spec §6.6): a thin front end over `auction_sdk`. `run` is generic over the chain so
//! golden tests drive every command against `FixtureChain`; `main.rs` plugs in `RpcChain`.
#![forbid(unsafe_code)]

pub mod args;
pub mod env;
pub mod error;
pub mod output;
mod read_cmds;

use std::io::Write;

use auction_sdk::chain::Chain;

pub use args::{Cli, Command};
pub use env::Env;
pub use error::CliError;

/// Runs `cmd`, writing results to `out` and warnings/notes to stderr.
pub fn run<C: Chain>(cmd: &Command, env: &Env<C>, out: &mut dyn Write) -> Result<(), CliError> {
    run_with(cmd, env, out, &mut std::io::stderr())
}

/// `run` with an explicit warning stream (tests capture it).
pub fn run_with<C: Chain>(
    cmd: &Command,
    env: &Env<C>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    match cmd {
        Command::Config => read_cmds::config(env, out),
        Command::QuoteLiquidation { position } => read_cmds::quote_liquidation(env, position, out),
        Command::QuoteBid {
            auction,
            coin_in_max,
            slippage_bps,
        } => read_cmds::quote_bid(env, auction, *coin_in_max, *slippage_bps, out),
        Command::ListAuctions { all, scan } => read_cmds::list_auctions(env, *all, scan, out, err),
        Command::ListAtRisk { scan } => read_cmds::list_at_risk(env, scan, out, err),
        Command::Rescan { from, scan } => read_cmds::rescan(env, *from, scan, out, err),
        Command::Watch {
            interval_ms,
            iterations,
            scan,
        } => read_cmds::watch(env, *interval_ms, *iterations, scan, out, err),
    }
}
