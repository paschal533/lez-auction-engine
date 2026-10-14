//! Host (stablecoin) commands. `host bind` is the ADR-027 bind-time verification automated: it reads
//! and verifies the engine config it is about to bind and refuses (exit 2) on any mismatch, before
//! anything is signed or sent.

use std::io::Write;

use auction_sdk::{
    addresses::HostAddresses, chain::Chain, decode::Reader, deployment::program_id_hex,
    error::SdkError, plan_host, verify::verify_binding,
};
use lee_core::account::{Account, AccountId};
use stablecoin_core::{LiquidationEngineState, ProtocolParameters};

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
    if !matches!(cmd, HostCommand::Addresses | HostCommand::Status) {
        env.refuse_at_ms()?;
    }
    match cmd {
        HostCommand::Addresses => addresses(env, out),
        HostCommand::Status => status(env, out),
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
            let now = protocol_parameters(env, &host)?;
            kv(
                out,
                "min collateral ratio",
                &format!("{} -> {ratio}", now.minimum_collateralization_ratio),
            )?;
            let signer = require_host_admin(env, &now, admin)?;
            let plan = plan_host::set_minimum_collateralization_ratio(&host, signer, *ratio)?;
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
            let now = protocol_parameters(env, &host)?;
            let (old_kp, old_ki) = (
                now.controller_proportional_gain,
                now.controller_integral_gain,
            );
            kv(out, "kp", &format!("{old_kp} -> {kp}"))?;
            kv(out, "ki", &format!("{old_ki} -> {ki}"))?;
            let signer = require_host_admin(env, &now, admin)?;
            let plan = plan_host::set_controller_gains(&host, signer, *kp, *ki)?;
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

/// The signer, refused before signing (exit 2) unless it is the host admin recorded in
/// `ProtocolParameters`: the host would drop the transaction, and resending could not help.
pub(crate) fn require_host_admin<C: Chain>(
    env: &Env<C>,
    params: &ProtocolParameters,
    signer: &str,
) -> Result<AccountId, CliError> {
    let signer = env.account(signer)?;
    if signer != params.admin_account_id {
        return Err(CliError::Verify(SdkError::Mismatch {
            what: "host admin (refusing before signing: the signer is not the host admin)",
            expected: params.admin_account_id.to_string(),
            found: signer.to_string(),
        }));
    }
    Ok(signer)
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

/// Read-only view of the host's `ProtocolParameters` (the freeze flag is the current state; the
/// engine's `host_frozen_by_engine` only means "has ever frozen", THREAT_MODEL runbook).
fn status<C: Chain>(env: &Env<C>, out: &mut dyn Write) -> Result<(), CliError> {
    let p = protocol_parameters(env, &host_addresses(env))?;
    kv(out, "frozen", yes_no(p.is_frozen))?;
    kv(out, "admin", &p.admin_account_id.to_string())?;
    kv(
        out,
        "freeze authority",
        &p.freeze_authority_account_id.to_string(),
    )?;
    kv(out, "market oracle", &p.market_price_oracle_id.to_string())?;
    kv(
        out,
        "min collateral ratio",
        &p.minimum_collateralization_ratio.to_string(),
    )?;
    kv(
        out,
        "max oracle age ms",
        &p.maximum_oracle_price_age_milliseconds.to_string(),
    )
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

/// The engine the host is bound to now (a re-bind away from another config must be visible).
fn current_binding<C: Chain>(env: &Env<C>, host: &HostAddresses) -> String {
    let state = match env.chain.account(&host.liquidation_engine_state) {
        Ok(acc) if acc == Account::default() => return "none".to_owned(),
        Ok(_) => Reader::new(&env.chain, env.deployment.engine_program)
            .host::<LiquidationEngineState>(
                &host.liquidation_engine_state,
                &host.program,
                "LiquidationEngineState",
            ),
        Err(e) => return format!("unreadable: {e}"),
    };
    match state {
        Ok(s) => format!(
            "engine {}, authority {}",
            program_id_hex(&s.engine_program_id),
            s.engine_authority_id
        ),
        Err(e) => format!("unreadable: {e}"),
    }
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
    let host = host_addresses(env);
    let signer = require_host_admin(env, &protocol_parameters(env, &host)?, admin)?;
    kv(out, "currently bound", &current_binding(env, &host))?;
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
    let current = match protocol_parameters(env, &host) {
        Ok(p) => format!("already initialised (admin {})", p.admin_account_id),
        Err(_) => "uninitialised".to_owned(),
    };
    kv(out, "host state", &current)?;
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
