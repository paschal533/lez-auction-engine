//! `auction-cli`: parse arguments, load the deployment file, connect to the sequencer, run.
//! Every failure goes through `CliError::exit_code` (S4); a deployment file that cannot be loaded is
//! an ordinary error (1), because 2 is reserved for verification failures.
#![forbid(unsafe_code)]

use std::process::ExitCode;

use auction_cli::{
    args::{deployment_note, usage_exit_code, Cli},
    env::Env,
    error::CliError,
    run,
};
use auction_sdk::{confirm::ConfirmSettings, deployment::Deployment, rpc::RpcChain};
use clap::Parser;
use lee_core::account::AccountId;

fn fail(e: &CliError) -> ExitCode {
    eprintln!("error: {e}");
    ExitCode::from(e.exit_code())
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return ExitCode::from(usage_exit_code(&e));
        }
    };
    let deployment = match Deployment::load(&cli.deployment) {
        Ok(d) => d,
        Err(e) => return fail(&e.into()),
    };
    if let Some(note) = deployment_note(&cli.command, &cli.deployment, &deployment.network) {
        eprintln!("{note}");
    }
    let config = match cli
        .config
        .as_deref()
        .map(str::parse::<AccountId>)
        .transpose()
    {
        Ok(c) => c,
        Err(_) => {
            return fail(&CliError::Usage(
                "--config is not a base58 account id".into(),
            ))
        }
    };
    let url = cli
        .rpc
        .clone()
        .unwrap_or_else(|| deployment.sequencer_url.clone());
    let chain = match RpcChain::connect(&url) {
        Ok(c) => c,
        Err(e) => return fail(&e.into()),
    };
    let env = Env {
        chain,
        deployment,
        config,
        wallet_home: cli.wallet_home.clone(),
        state_dir: cli.state_dir.clone(),
        at_ms: cli.at_ms,
        confirm: ConfirmSettings::default(),
        sleep: std::thread::sleep,
    };
    let mut out = std::io::stdout().lock();
    match run(&cli.command, &env, &mut out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(&e),
    }
}
