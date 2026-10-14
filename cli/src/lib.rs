//! `auction-cli` (spec §6.6): a thin front end over `auction_sdk`. `run` is generic over the chain so
//! golden tests drive every command against `FixtureChain`; `main.rs` plugs in `RpcChain`.
#![forbid(unsafe_code)]

mod admin_cmds;
pub mod args;
mod dev_cmds;
pub mod env;
pub mod error;
mod host_cmds;
mod oracle_cmds;
pub mod output;
mod params;
mod read_cmds;
mod tx_cmds;
mod tx_report;

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
            observe_with,
            scan,
        } => read_cmds::watch(
            env,
            *interval_ms,
            *iterations,
            observe_with.as_deref(),
            scan,
            out,
            err,
        ),
        Command::Liquidate(a) => tx_cmds::liquidate(env, a, out),
        Command::Bid(a) => tx_cmds::bid(env, a, out),
        Command::SettleDebt { caller, amount } => tx_cmds::settle_debt(env, caller, *amount, out),
        Command::CheckSolvency { caller } => tx_cmds::check_solvency(env, caller, out),
        Command::StartSurplus { caller } => tx_cmds::start_surplus(env, caller, out),
        Command::TakeSurplus(a) => tx_cmds::take_surplus(env, a, out),
        Command::StartDebt { caller } => tx_cmds::start_debt(env, caller, out),
        Command::TakeDebt(a) => tx_cmds::take_debt(env, a, out),
        Command::ObserveFeed { caller } => tx_cmds::observe_feed(env, caller, out),
        Command::ClaimLeftover {
            owner,
            owner_holding,
            auction,
        } => tx_cmds::claim_leftover(env, owner, owner_holding, auction, out),
        Command::SettleEmpty { caller, auction } => {
            tx_cmds::settle_empty(env, caller, auction, out)
        }
        Command::CloseSurplus { caller } => tx_cmds::close_surplus(env, caller, out),
        Command::CloseDebt { caller } => tx_cmds::close_debt(env, caller, out),
        Command::Admin(c) => admin_cmds::admin(env, c, out),
        Command::Host(c) => host_cmds::host(env, c, out),
        Command::Dev(c) => dev_cmds::dev(env, c, out),
    }
}
