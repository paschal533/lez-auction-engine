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

/// Slippage below 100% (10000 bps would remove the bound).
fn slippage_bps_parser() -> clap::builder::RangedI64ValueParser<u32> {
    clap::value_parser!(u32).range(0..10_000)
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
        #[arg(long, default_value_t = 50, value_parser = slippage_bps_parser())]
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
    /// Refresh periodically: scan and list auctions; with --observe-with, send observe_feed first
    /// each round (keepers and apps refresh the pause clock, THREAT_MODEL S5 residual).
    Watch {
        #[arg(long, default_value_t = 15_000)]
        interval_ms: u64,
        #[arg(long)]
        iterations: Option<u32>,
        #[arg(long)]
        observe_with: Option<String>,
        #[command(flatten)]
        scan: ScanArgs,
    },
    /// Liquidate an unsafe position (quotes first; refuses a safe position).
    Liquidate(LiquidateCmd),
    /// Bid; picks bid or bid_with_settlement from the quote (ADR-029).
    Bid(BidCmd),
    /// Burn buffer coin against unbacked debt (default: as much as possible).
    SettleDebt {
        #[arg(long)]
        caller: String,
        #[arg(long)]
        amount: Option<u128>,
    },
    /// Freeze new debt if bad debt exceeds the threshold and no working protocol token exists.
    CheckSolvency {
        #[arg(long)]
        caller: String,
    },
    /// Start a surplus auction (refused while one is live).
    StartSurplus {
        #[arg(long)]
        caller: String,
    },
    /// Pay protocol tokens (--payer), receive stablecoin (--receiver).
    TakeSurplus(TakeCmd),
    /// Start a debt auction (refused while one is live or inside the relist delay).
    StartDebt {
        #[arg(long)]
        caller: String,
    },
    /// Pay stablecoin (--payer), receive minted protocol tokens (--receiver).
    TakeDebt(TakeCmd),
    /// Persist a pause-clock observation (never reverts on a bad feed).
    ObserveFeed {
        #[arg(long)]
        caller: String,
    },
    /// Claim the leftover collateral of a settled auction.
    ClaimLeftover {
        #[arg(long)]
        owner: String,
        #[arg(long)]
        owner_holding: String,
        #[arg(long)]
        auction: String,
    },
    /// Settle an auction whose lot is already 0.
    SettleEmpty {
        #[arg(long)]
        caller: String,
        #[arg(long)]
        auction: String,
    },
    /// Close the latest surplus auction after its TTL.
    CloseSurplus {
        #[arg(long)]
        caller: String,
    },
    /// Close the latest debt auction after its TTL.
    CloseDebt {
        #[arg(long)]
        caller: String,
    },
    /// Engine admin commands: config creation and parameters.
    #[command(subcommand)]
    Admin(AdminCommand),
    /// Host (stablecoin) commands: bind, setup and parameters.
    #[command(subcommand)]
    Host(HostCommand),
    /// Localnet setup helpers (tokens, oracle, balances).
    #[command(subcommand)]
    Dev(DevCommand),
}

#[derive(Debug, Args)]
pub struct LiquidateCmd {
    #[arg(long)]
    pub position: String,
    #[arg(long)]
    pub keeper: String,
    /// Collateral holding that receives the keeper reward (signs too if it is fresh).
    #[arg(long)]
    pub keeper_holding: String,
}

#[derive(Debug, Args)]
pub struct BidCmd {
    #[arg(long)]
    pub auction: String,
    #[arg(long)]
    pub coin_in_max: u128,
    #[arg(long)]
    pub bidder_stable: String,
    #[arg(long)]
    pub bidder_collateral: String,
    #[arg(long, default_value_t = 50, value_parser = slippage_bps_parser())]
    pub slippage_bps: u32,
    /// Explicit minimum collateral out (replaces the bound derived from --slippage-bps; the choice
    /// of bid vs bid_with_settlement still uses --slippage-bps for its near-empty margin).
    #[arg(long)]
    pub min_out: Option<u128>,
}

#[derive(Debug, Args)]
pub struct TakeCmd {
    #[arg(long)]
    pub payer: String,
    #[arg(long)]
    pub receiver: String,
    #[arg(long)]
    pub amount: u128,
    #[arg(long, default_value_t = 50, value_parser = slippage_bps_parser())]
    pub slippage_bps: u32,
}

/// Exit code for an argument-parsing outcome. clap's own default for a usage error is 2, which S4
/// reserves for verification failures, so usage errors exit 1 and `--help`/`--version` exit 0.
#[must_use]
pub fn usage_exit_code(e: &clap::Error) -> u8 {
    u8::from(e.use_stderr())
}

#[derive(Debug, Subcommand)]
pub enum AdminCommand {
    /// Print the PDAs of the config `["CONFIG", admin, salt]` (no transaction).
    Addresses {
        #[arg(long)]
        admin: String,
        #[arg(long, default_value_t = 1)]
        salt: u64,
    },
    /// init_config: the signer becomes the config's admin (ADR-027).
    InitConfig {
        #[arg(long)]
        admin: String,
        #[arg(long, default_value_t = 1)]
        salt: u64,
        #[arg(long)]
        params: PathBuf,
        #[arg(long)]
        protocol_token: Option<String>,
        #[arg(long)]
        ref_oracle: Option<String>,
    },
    SetEngineParams {
        #[arg(long)]
        admin: String,
        #[arg(long)]
        params: PathBuf,
        #[arg(long)]
        protocol_token: Option<String>,
        #[arg(long)]
        ref_oracle: Option<String>,
        /// Allow the change to disable the protocol token the config currently has.
        #[arg(long, conflicts_with = "protocol_token")]
        disable_protocol_token: bool,
        /// Allow the change to disable the reference oracle the config currently has.
        #[arg(long, conflicts_with = "ref_oracle")]
        disable_ref_oracle: bool,
    },
    SetCollateralConfig {
        #[arg(long)]
        admin: String,
        #[arg(long)]
        params: PathBuf,
    },
    /// Verify the config against the deployment file and print what a host admin must check (ADR-027).
    Verify,
}

#[derive(Debug, Subcommand)]
pub enum HostCommand {
    /// Print the host PDAs (no transaction).
    Addresses,
    /// set_liquidation_engine, only after verify_config accepts the config (ADR-027).
    Bind {
        #[arg(long)]
        admin: String,
    },
    InitProgram(InitProgramCmd),
    OpenPosition {
        #[arg(long)]
        owner: String,
        #[arg(long)]
        owner_collateral: String,
        #[arg(long, default_value_t = 0)]
        nonce: u64,
        #[arg(long)]
        amount: u128,
    },
    GenerateDebt {
        #[arg(long)]
        owner: String,
        #[arg(long)]
        position: String,
        #[arg(long)]
        owner_stable: String,
        #[arg(long)]
        amount: u128,
    },
    SetMinCr {
        #[arg(long)]
        admin: String,
        #[arg(long)]
        ratio: u128,
    },
    /// Signed gains (spel cannot encode i128, ADR-018).
    SetControllerGains {
        #[arg(long)]
        admin: String,
        #[arg(long, allow_hyphen_values = true)]
        kp: i128,
        #[arg(long, allow_hyphen_values = true)]
        ki: i128,
    },
}

#[derive(Debug, Args)]
pub struct InitProgramCmd {
    #[arg(long)]
    pub admin: String,
    #[arg(long)]
    pub freeze: String,
    #[arg(long)]
    pub collateral_def: String,
    #[arg(long)]
    pub oracle: String,
    #[arg(long, allow_hyphen_values = true, default_value_t = 0)]
    pub kp: i128,
    #[arg(long, allow_hyphen_values = true, default_value_t = 0)]
    pub ki: i128,
    #[arg(long, default_value = "1500000000000000000000000000")]
    pub min_cr: u128,
    #[arg(long, default_value = "1000000000000000000000000000")]
    pub redemption_price: u128,
    #[arg(long, default_value = "1000000000000000000000000000")]
    pub stability_fee: u128,
    #[arg(long, default_value_t = 60_000)]
    pub min_ms_between_updates: u64,
    #[arg(long, default_value_t = 3_600_000)]
    pub max_oracle_age_ms: u64,
    #[arg(long, default_value = "LSC")]
    pub name: String,
}

#[derive(Debug, Subcommand)]
pub enum DevCommand {
    /// Print the base58 id of a wallet label.
    Resolve {
        #[arg(long)]
        label: String,
    },
    TokenNew {
        #[arg(long)]
        definition: String,
        #[arg(long)]
        holding: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        supply: u128,
        #[arg(long)]
        mint_authority: Option<String>,
    },
    TokenInitAccount {
        #[arg(long)]
        definition: String,
        #[arg(long)]
        account: String,
    },
    TokenTransfer {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u128,
    },
    /// twap_oracle CreateOraclePriceAccount (needs `twap_oracle_program_id` in the deployment file).
    OracleCreate {
        #[arg(long)]
        price_source: String,
        #[arg(long)]
        base: String,
        #[arg(long)]
        quote: String,
        #[arg(long)]
        price: u128,
        #[arg(long, default_value_t = 3_600_000)]
        window_ms: u64,
    },
    /// Token-holding balance (no transaction).
    Balance {
        #[arg(long)]
        account: String,
    },
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
    fn slippage_must_stay_below_100_percent() {
        let args = |bps: &str| {
            Cli::try_parse_from([
                "auction-cli",
                "take-debt",
                "--payer",
                "a",
                "--receiver",
                "b",
                "--amount",
                "1",
                "--slippage-bps",
                bps,
            ])
        };
        assert!(args("9999").is_ok());
        assert!(args("10000").is_err());
        assert!(Cli::try_parse_from([
            "auction-cli",
            "quote-bid",
            "--auction",
            "a",
            "--coin-in-max",
            "1",
            "--slippage-bps",
            "10000",
        ])
        .is_err());
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

    #[test]
    fn controller_gains_accept_negative_values() {
        for (kp, ki) in [("-5", "-1"), ("=-5", "=-1")] {
            let mut argv = vec![
                "auction-cli".to_owned(),
                "host".into(),
                "set-controller-gains".into(),
                "--admin".into(),
                "a".into(),
            ];
            if kp.starts_with('=') {
                argv.push(format!("--kp{kp}"));
                argv.push(format!("--ki{ki}"));
            } else {
                argv.extend(["--kp".into(), kp.into(), "--ki".into(), ki.into()]);
            }
            match Cli::try_parse_from(argv).unwrap().command {
                Command::Host(HostCommand::SetControllerGains { kp, ki, .. }) => {
                    assert_eq!((kp, ki), (-5, -1));
                }
                other => panic!("unexpected {other:?}"),
            }
        }
        let min = i128::MIN.to_string();
        let cli = Cli::try_parse_from([
            "auction-cli",
            "host",
            "set-controller-gains",
            "--admin",
            "a",
            "--kp",
            &min,
            "--ki",
            "0",
        ])
        .unwrap();
        match cli.command {
            Command::Host(HostCommand::SetControllerGains { kp, .. }) => assert_eq!(kp, i128::MIN),
            other => panic!("unexpected {other:?}"),
        }
    }
}
