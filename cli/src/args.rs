//! Command line (spec §6.6). Account arguments take a base58 id or a wallet label.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "auction-cli",
    version,
    about = "RFP-014 auction engine CLI (LEZ v0.2.4, public transactions only)"
)]
pub struct Cli {
    /// Deployment file with the trusted ImageIDs (I4).
    #[arg(long, global = true, default_value = "deployments/localnet.json")]
    pub deployment: PathBuf,
    /// Engine config account (overrides the deployment file).
    #[arg(long, global = true)]
    pub config: Option<String>,
    /// Sequencer URL (overrides the deployment file).
    #[arg(long, global = true)]
    pub rpc: Option<String>,
    /// Wallet home holding storage.json (default: $LEE_WALLET_HOME_DIR).
    #[arg(long, global = true)]
    pub wallet_home: Option<PathBuf>,
    /// Directory for the scan cursor and the in-flight locks.
    #[arg(long, global = true, default_value = ".auction-cli")]
    pub state_dir: PathBuf,
    /// Evaluate quotes at this clock (ms) instead of the chain's CLOCK_01.
    #[arg(long, global = true)]
    pub at_ms: Option<u64>,
    #[command(subcommand)]
    pub command: Command,
}

/// Options of every command that scans blocks.
#[derive(Debug, Clone, Copy, Default, Args)]
pub struct ScanArgs {
    /// Blocks per getBlockRange request (default 100). Lower it when 100 blocks exceed the node's
    /// 16 MiB response cap.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub batch: Option<u64>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show the verified engine config, including its admin (and where that admin comes from).
    Config,
    /// Quote the liquidation of a position (no transaction).
    QuoteLiquidation {
        #[arg(long)]
        position: String,
    },
    /// Quote a bid (no transaction).
    QuoteBid {
        #[arg(long)]
        auction: String,
        #[arg(long)]
        coin_in_max: u128,
        #[arg(long, default_value_t = 50)]
        slippage_bps: u32,
    },
    /// Scan new blocks, then list live auctions (settled ones too with --all).
    ListAuctions {
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        scan: ScanArgs,
    },
    /// Scan new blocks, then list positions unsafe at the host's minimum ratio.
    ListAtRisk {
        #[command(flatten)]
        scan: ScanArgs,
    },
    /// Rescan from a block (default: continue from the cursor). `--from 1` rebuilds everything and
    /// also repairs a corrupt cursor file. Blocks before N are not scanned unless the cursor
    /// already covers them.
    Rescan {
        #[arg(long)]
        from: Option<u64>,
        #[command(flatten)]
        scan: ScanArgs,
    },
    /// Refresh periodically: scan and list auctions.
    Watch {
        #[arg(long, default_value_t = 15_000)]
        interval_ms: u64,
        #[arg(long)]
        iterations: Option<u32>,
        #[command(flatten)]
        scan: ScanArgs,
    },
}

/// Exit code for an argument-parsing outcome. clap's own default for a usage error is 2, which S4
/// reserves for verification failures, so usage errors exit 1 and `--help`/`--version` exit 0.
#[must_use]
pub fn usage_exit_code(e: &clap::Error) -> u8 {
    u8::from(e.use_stderr())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_errors_exit_1_and_help_exits_0() {
        let bad = Cli::try_parse_from(["auction-cli", "--no-such-flag"]).unwrap_err();
        assert_eq!(usage_exit_code(&bad), 1);
        let missing = Cli::try_parse_from(["auction-cli"]).unwrap_err();
        assert_eq!(usage_exit_code(&missing), 1);
        let help = Cli::try_parse_from(["auction-cli", "--help"]).unwrap_err();
        assert_eq!(usage_exit_code(&help), 0);
    }

    #[test]
    fn batch_must_be_positive() {
        assert!(Cli::try_parse_from(["auction-cli", "rescan", "--batch", "0"]).is_err());
        let ok = Cli::try_parse_from(["auction-cli", "list-auctions", "--batch", "5"]).unwrap();
        match ok.command {
            Command::ListAuctions { scan, .. } => assert_eq!(scan.batch, Some(5)),
            other => panic!("unexpected {other:?}"),
        }
    }
}
