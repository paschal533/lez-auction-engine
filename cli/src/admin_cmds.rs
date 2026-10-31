//! Admin commands (spec §6.6) and the helpers the host and dev commands share. `init-config` has no
//! config to verify yet (it checks the network instead); every command acting on an existing config
//! verifies it (I4), announces its admin and checks the signer is that admin before signing anything.

use std::io::Write;
use std::path::{Path, PathBuf};

use auction_sdk::{
    addresses::{EngineAddresses, HostAddresses},
    chain::Chain,
    decode::Reader,
    deployment::program_id_hex,
    error::{SdkError, SdkResult},
    ops::{self, SetEngineParamsRequest, CHECK_VERIFY},
    params, plan,
    verify::{verify_binding, verify_config, VerifiedConfig},
};
use lee_core::account::AccountId;
use stablecoin_core::ProtocolParameters;

use crate::{
    args::AdminCommand,
    env::Env,
    error::CliError,
    output::{kv, yes_no},
};

pub(crate) fn host_addresses<C: Chain>(env: &Env<C>) -> HostAddresses {
    HostAddresses::new(env.deployment.host_program)
}

pub(crate) fn protocol_parameters<C: Chain>(
    env: &Env<C>,
    host: &HostAddresses,
) -> SdkResult<ProtocolParameters> {
    Reader::new(&env.chain, env.deployment.engine_program).host(
        &host.protocol_parameters,
        &host.program,
        "ProtocolParameters",
    )
}

pub(crate) fn optional<C: Chain>(
    env: &Env<C>,
    arg: Option<&String>,
) -> Result<Option<AccountId>, CliError> {
    arg.map(|s| env.account(s)).transpose()
}

/// What the command re-read from the chain after inclusion; a failed read is reported, never fatal.
pub(crate) fn print_after(
    out: &mut dyn Write,
    label: &str,
    read: SdkResult<String>,
) -> Result<(), CliError> {
    let value = read.unwrap_or_else(|e| format!("unreadable: {e}"));
    kv(out, &format!("after {label}"), &value)
}

/// Before a config exists the host and the token program on this node are the trusted ones.
fn check_network<C: Chain>(env: &Env<C>, host: &HostAddresses) -> Result<(), CliError> {
    let d = &env.deployment;
    let pp = env.chain.account(&host.protocol_parameters)?;
    if pp.program_owner != d.host_program {
        return Err(CliError::Verify(SdkError::Mismatch {
            what: "owner of the host ProtocolParameters (wrong network?)",
            expected: program_id_hex(&d.host_program),
            found: program_id_hex(&pp.program_owner),
        }));
    }
    let sdef = env.chain.account(&host.stablecoin_definition)?;
    if sdef.program_owner != d.token_program {
        return Err(CliError::Verify(SdkError::Mismatch {
            what: "token program (owner of the stablecoin definition)",
            expected: program_id_hex(&d.token_program),
            found: program_id_hex(&sdef.program_owner),
        }));
    }
    Ok(())
}

/// I4, then the admin the user is about to sign against, then the signer check (all before signing).
pub(crate) fn begin<C: Chain>(
    env: &Env<C>,
    signer: &str,
    out: &mut dyn Write,
) -> Result<(VerifiedConfig, AccountId), CliError> {
    let v = env.verified()?;
    env.announce_admin(&v, out)?;
    let signer = env.account(signer)?;
    ops::require_engine_admin(&v, signer)?;
    Ok((v, signer))
}

pub fn admin<C: Chain>(
    env: &Env<C>,
    cmd: &AdminCommand,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    if !matches!(cmd, AdminCommand::Addresses { .. } | AdminCommand::Verify) {
        env.refuse_at_ms()?;
    }
    match cmd {
        AdminCommand::Addresses { admin, salt } => addresses(env, admin, *salt, out),
        AdminCommand::InitConfig {
            admin,
            salt,
            params: file,
            protocol_token,
            ref_oracle,
        } => init_config(
            env,
            (admin, *salt),
            (file, protocol_token.as_ref(), ref_oracle.as_ref()),
            out,
        ),
        AdminCommand::SetEngineParams {
            admin,
            params: file,
            protocol_token,
            ref_oracle,
            disable_protocol_token,
            disable_ref_oracle,
        } => {
            let req = SetEngineParamsRequest {
                admin: admin.clone(),
                params: file.clone(),
                protocol_token: protocol_token.clone(),
                ref_oracle: ref_oracle.clone(),
                disable_protocol_token: *disable_protocol_token,
                disable_ref_oracle: *disable_ref_oracle,
            };
            set_engine_params(env, &req, out)
        }
        AdminCommand::SetCollateralConfig {
            admin,
            params: file,
        } => set_collateral_config(env, admin, file, out),
        AdminCommand::Verify => verify(env, out),
    }
}

fn addresses<C: Chain>(
    env: &Env<C>,
    admin: &str,
    salt: u64,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let e = EngineAddresses::new(env.deployment.engine_program, &env.account(admin)?, salt);
    kv(out, "config", &e.config.to_string())?;
    kv(out, "accounting", &e.accounting.to_string())?;
    kv(out, "buffer", &e.buffer.to_string())?;
    kv(out, "engine authority", &e.engine_authority.to_string())?;
    kv(out, "mint authority", &e.mint_authority.to_string())?;
    kv(out, "unused slot 0", &e.unused_ref_oracle.to_string())?;
    kv(out, "unused slot 1", &e.unused_protocol_token.to_string())
}

/// A parameter file with the optional protocol-token and reference-oracle slots.
type ParamsArgs<'a> = (&'a PathBuf, Option<&'a String>, Option<&'a String>);

fn init_config<C: Chain>(
    env: &Env<C>,
    (admin, salt): (&str, u64),
    (file, protocol_token, ref_oracle): ParamsArgs<'_>,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let host = host_addresses(env);
    check_network(env, &host)?;
    let admin = env.account(admin)?;
    if let Some(expected) = env.deployment.engine_admin {
        if expected != admin {
            return Err(CliError::Verify(SdkError::Mismatch {
                what: "engine admin (the signer is not the operator named by the deployment file)",
                expected: expected.to_string(),
                found: admin.to_string(),
            }));
        }
    }
    let token = optional(env, protocol_token)?;
    let oracle = optional(env, ref_oracle)?;
    if let Some(t) = token {
        ops::check_protocol_token(&env.chain, env.deployment.token_program, &t)?;
    }
    let p = params::engine_params(file, token.is_some(), oracle.is_some())?;
    let e = EngineAddresses::new(env.deployment.engine_program, &admin, salt);
    kv(out, "config", &e.config.to_string())?;
    kv(out, "admin", &admin.to_string())?;
    kv(
        out,
        "ref price (RAY)",
        &p.protocol_token_ref_price.to_string(),
    )?;
    kv(
        out,
        "record",
        "the initial reference price in DEPLOYMENTS.md (ADR-017)",
    )?;
    let engine = env.deployment.engine_program;
    let plan = plan::init_config(engine, &host, admin, salt, &p, token, oracle)?;
    env.send_plain(&plan, "init_config", CHECK_VERIFY, out)?;
    let read = verify_config(&env.chain, &env.deployment, &e.config)
        .map(|v| format!("config verified, admin {}", v.state.admin));
    print_after(out, "config", read)
}

fn set_engine_params<C: Chain>(
    env: &Env<C>,
    req: &SetEngineParamsRequest,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let v = env.verified()?;
    env.announce_admin(&v, out)?;
    let now = env.chain_now()?;
    let p = env.with_accounts(|accounts| {
        ops::prepare_set_engine_params(&env.chain, &v, req, accounts, now)
    })?;
    env.send_steps(&p, out)?;
    let read = verify_config(&env.chain, &env.deployment, &v.config_id).map(|a| {
        format!(
            "global cap {}, ref price {}",
            a.state.global_cap, a.state.protocol_token_ref_price
        )
    });
    print_after(out, "config", read)
}

fn set_collateral_config<C: Chain>(
    env: &Env<C>,
    admin: &str,
    file: &Path,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let (v, admin) = begin(env, admin, out)?;
    let c = params::collateral_args(file)?;
    // The engine's own rules, run now so a bad file fails before anything is signed.
    c.validate(v.state.global_cap)
        .map_err(|e| CliError::Usage(format!("invalid collateral config: {e}")))?;
    let oracle = protocol_parameters(env, &v.host)?.market_price_oracle_id; // ADR-028
    let plan = plan::set_collateral_config(&v, admin, oracle, &c)?;
    env.send_plain(&plan, "set_collateral_config", CHECK_VERIFY, out)?;
    let read = v.coll(&env.chain).map(|s| {
        format!(
            "min bid {}, type cap {}, paused {}",
            s.min_bid,
            s.type_cap,
            yes_no(s.admin_paused)
        )
    });
    print_after(out, "collateral config", read)
}

fn verify<C: Chain>(env: &Env<C>, out: &mut dyn Write) -> Result<(), CliError> {
    let v = env.verified()?;
    kv(
        out,
        "verified",
        "config owner, address, host and token program ids, admin, host ProtocolParameters",
    )?;
    kv(out, "config", &v.config_id.to_string())?;
    kv(out, "admin", &v.state.admin.to_string())?;
    kv(out, "salt", &v.state.salt.to_string())?;
    kv(
        out,
        "host program",
        &program_id_hex(&v.state.host_program_id),
    )?;
    kv(
        out,
        "token program",
        &program_id_hex(&v.state.token_program_id),
    )?;
    let token = v
        .state
        .protocol_token
        .map_or_else(|| "none".to_owned(), |t| t.to_string());
    kv(out, "protocol token", &token)?;
    kv(
        out,
        "ref price (RAY)",
        &v.state.protocol_token_ref_price.to_string(),
    )?;
    kv(
        out,
        "freeze threshold",
        &v.state.bad_debt_freeze_threshold.to_string(),
    )?;
    kv(
        out,
        "freeze on bad debt",
        yes_no(v.state.freeze_on_bad_debt),
    )?;
    kv(out, "global cap", &v.state.global_cap.to_string())?;
    let bound = match verify_binding(&env.chain, &v) {
        Ok(()) => "yes".to_owned(),
        Err(e) => format!("no ({e})"),
    };
    kv(out, "bound by the host", &bound)
}
