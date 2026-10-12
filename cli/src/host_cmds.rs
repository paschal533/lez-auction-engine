//! Host (stablecoin) commands. `host bind` is the ADR-027 bind-time verification automated: it reads
//! and verifies the engine config it is about to bind and refuses (exit 2) on any mismatch, before
//! anything is signed or sent.

use std::io::Write;

use auction_sdk::{chain::Chain, deployment::program_id_hex, plan_host, verify::verify_binding};
use stablecoin_core::ProtocolParameters;

use crate::{
    admin_cmds::{host_addresses, print_after, protocol_parameters},
    args::{HostCommand, InitProgramCmd},
    env::Env,
    error::CliError,
    output::{kv, yes_no},
};

const CHECK_HOST: &str = "host addresses";
const CHECK_VERIFY: &str = "admin verify";

pub fn host<C: Chain>(
    env: &Env<C>,
    cmd: &HostCommand,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    if !matches!(cmd, HostCommand::Addresses) {
        env.refuse_at_ms()?;
    }
    match cmd {
        HostCommand::Addresses => addresses(env, out),
        HostCommand::Bind { admin } => bind(env, admin, out),
        HostCommand::InitProgram(a) => init_program(env, a, out),
        HostCommand::OpenPosition {
            owner,
            owner_collateral,
            nonce,
            amount,
        } => open_position(env, (owner, owner_collateral, *nonce), *amount, out),
        HostCommand::GenerateDebt {
            owner,
            position,
            owner_stable,
            amount,
        } => generate_debt(env, (owner, position, owner_stable), *amount, out),
        HostCommand::SetMinCr { admin, ratio } => {
            let host = host_addresses(env);
            let plan =
                plan_host::set_minimum_collateralization_ratio(&host, env.account(admin)?, *ratio)?;
            env.send_plain(
                &plan,
                "set_minimum_collateralization_ratio",
                CHECK_HOST,
                out,
            )?;
            print_params(env, out, |p| {
                format!(
                    "minimum collateralization ratio {}",
                    p.minimum_collateralization_ratio
                )
            })
        }
        HostCommand::SetControllerGains { admin, kp, ki } => {
            let host = host_addresses(env);
            let plan = plan_host::set_controller_gains(&host, env.account(admin)?, *kp, *ki)?;
            env.send_plain(&plan, "set_controller_gains", CHECK_HOST, out)?;
            print_params(env, out, |p| {
                format!(
                    "kp {}, ki {}",
                    p.controller_proportional_gain, p.controller_integral_gain
                )
            })
        }
    }
}

/// Re-reads `ProtocolParameters` after inclusion and prints one summary of it.
fn print_params<C: Chain>(
    env: &Env<C>,
    out: &mut dyn Write,
    summary: fn(&ProtocolParameters) -> String,
) -> Result<(), CliError> {
    let read = protocol_parameters(env, &host_addresses(env)).map(|p| summary(&p));
    print_after(out, "protocol parameters", read)
}

fn addresses<C: Chain>(env: &Env<C>, out: &mut dyn Write) -> Result<(), CliError> {
    let host = host_addresses(env);
    kv(
        out,
        "protocol parameters",
        &host.protocol_parameters.to_string(),
    )?;
    kv(
        out,
        "stablecoin definition",
        &host.stablecoin_definition.to_string(),
    )?;
    kv(
        out,
        "stablecoin master",
        &host.stablecoin_master_holding.to_string(),
    )?;
    kv(
        out,
        "stability fee acc",
        &host.stability_fee_accumulator.to_string(),
    )?;
    kv(
        out,
        "redemption state",
        &host.redemption_price_state.to_string(),
    )?;
    kv(
        out,
        "engine binding",
        &host.liquidation_engine_state.to_string(),
    )
}

fn bind<C: Chain>(env: &Env<C>, admin: &str, out: &mut dyn Write) -> Result<(), CliError> {
    // ADR-027: refuses (exit 2) unless verify_config accepts the config about to be bound.
    let v = env.verified()?;
    kv(
        out,
        "verified",
        "config owner, address, host and token program ids, admin, host ProtocolParameters",
    )?;
    env.announce_admin(&v, out)?;
    kv(out, "config", &v.config_id.to_string())?;
    kv(out, "binds engine", &program_id_hex(&v.engine.program))?;
    kv(
        out,
        "engine authority",
        &v.engine.engine_authority.to_string(),
    )?;
    let signer = env.account(admin)?;
    let host = host_addresses(env);
    let host_admin = protocol_parameters(env, &host)?.admin_account_id;
    if signer != host_admin {
        return Err(CliError::Usage(format!(
            "refusing before signing: {signer} is not the host admin {host_admin}"
        )));
    }
    let plan = plan_host::set_liquidation_engine(
        &host,
        signer,
        v.engine.program,
        v.engine.engine_authority,
    )?;
    env.send_plain(&plan, "set_liquidation_engine", CHECK_VERIFY, out)?;
    let bound = match verify_binding(&env.chain, &v) {
        Ok(()) => "yes".to_owned(),
        Err(e) => format!("no ({e})"),
    };
    kv(out, "after binding", &format!("bound by the host: {bound}"))
}

fn init_program<C: Chain>(
    env: &Env<C>,
    a: &InitProgramCmd,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let host = host_addresses(env);
    let args = plan_host::InitializeProgramArgs {
        admin: env.account(&a.admin)?,
        freeze_authority: env.account(&a.freeze)?,
        collateral_definition: env.account(&a.collateral_def)?,
        market_price_oracle: env.account(&a.oracle)?,
        stability_fee_per_ms: a.stability_fee,
        proportional_gain: a.kp,
        integral_gain: a.ki,
        min_collateralization_ratio: a.min_cr,
        min_ms_between_rate_updates: a.min_ms_between_updates,
        max_oracle_age_ms: a.max_oracle_age_ms,
        redemption_price: a.redemption_price,
        stablecoin_name: a.name.clone(),
    };
    let plan = plan_host::initialize_program(&host, &args)?;
    env.send_plain(&plan, "initialize_program", CHECK_HOST, out)?;
    print_params(env, out, |p| format!("frozen {}", yes_no(p.is_frozen)))
}

fn open_position<C: Chain>(
    env: &Env<C>,
    (owner, owner_collateral, nonce): (&str, &str, u64),
    amount: u128,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let host = host_addresses(env);
    let owner = env.account(owner)?;
    let collateral = protocol_parameters(env, &host)?.collateral_definition_id;
    kv(out, "position", &host.position(&owner, nonce).to_string())?;
    let holding = env.account(owner_collateral)?;
    let plan = plan_host::open_position(&host, owner, holding, collateral, nonce, amount)?;
    env.send_plain(&plan, "open_position", CHECK_HOST, out)
}

fn generate_debt<C: Chain>(
    env: &Env<C>,
    (owner, position, owner_stable): (&str, &str, &str),
    amount: u128,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let host = host_addresses(env);
    let oracle = protocol_parameters(env, &host)?.market_price_oracle_id;
    let plan = plan_host::generate_debt(
        &host,
        env.account(owner)?,
        env.account(position)?,
        env.account(owner_stable)?,
        oracle,
        amount,
    )?;
    env.send_plain(&plan, "generate_debt", CHECK_HOST, out)
}
