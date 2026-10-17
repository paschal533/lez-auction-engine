//! `dev oracle-publish` and `dev keeper` (ADR-050): the dev-network price publisher. The dev-command
//! guard (a named dev network + the recorded chain, ADR-046) has already run in `dev_cmds::dev`.

use std::io::Write;
use std::time::Duration;

use auction_sdk::{chain::Chain, decode::Reader, oracle, plan, plan_host};

use crate::{
    admin_cmds::{begin, host_addresses, print_after, protocol_parameters},
    args::OraclePublishCmd,
    env::Env,
    error::CliError,
    host_cmds::require_host_admin,
    output::kv,
};

const CHECK: &str = "host status";

/// One move of the price: new price account, host rotation, engine follow-up. Both admins and the
/// price are checked before anything is signed.
pub fn publish<C: Chain>(
    env: &Env<C>,
    a: &OraclePublishCmd,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let (v, engine_admin) = begin(env, &a.engine_admin, out)?;
    let twap = env.deployment.twap_oracle_program.ok_or_else(|| {
        CliError::Usage("the deployment file has no twap_oracle_program_id".into())
    })?;
    let host = host_addresses(env);
    let params = protocol_parameters(env, &host)?;
    let host_admin = require_host_admin(env, &params, &a.host_admin)?;
    let source = env.account(&a.price_source)?;
    let coll = v.coll(&env.chain)?;
    let old = Reader::new(&env.chain, v.engine.program).oracle(&coll.oracle_id)?;
    let now = env.chain_now()?;
    let (window, new_oracle) =
        oracle::free_oracle_window(&env.chain, twap, source, a.base_window_ms)?;
    oracle::check_new_price(&env.chain, &v, new_oracle, source, a.price, now).map_err(|e| {
        CliError::Usage(format!(
            "refusing before signing: price {} would make the feed invalid for the engine ({e:?})",
            a.price
        ))
    })?;
    kv(out, "price", &format!("{} -> {}", old.price, a.price))?;
    kv(out, "new oracle", &new_oracle.to_string())?;
    kv(out, "window ms", &window.to_string())?;
    let (create, created) = plan_host::create_oracle_price_account(
        twap,
        source,
        params.stablecoin_definition_id,
        params.collateral_definition_id,
        a.price,
        window,
    )?;
    if created != new_oracle {
        return Err(CliError::Usage(
            "oracle address derivation disagrees".into(),
        ));
    }
    env.send_plain(&create, "create_oracle_price_account", CHECK, out)?;
    let rotate = plan_host::set_market_price_oracle(&host, host_admin, new_oracle)?;
    env.send_plain(&rotate, "set_market_price_oracle", CHECK, out)?;
    let args = oracle::collateral_args_from_state(&coll)?;
    let follow = plan::set_collateral_config(&v, engine_admin, new_oracle, &args)?;
    env.send_plain(&follow, "set_collateral_config", CHECK, out)?;
    print_after(
        out,
        "engine oracle",
        v.coll(&env.chain).map(|c| c.oracle_id.to_string()),
    )
}

/// Re-publishes when the oracle's age plus `refresh_before_ms` reaches the configured maximum age.
/// Runs `iterations` rounds (forever when `None`), sleeping `interval_ms` between them.
pub fn keeper<C: Chain>(
    env: &Env<C>,
    a: &OraclePublishCmd,
    (interval_ms, iterations, refresh_before_ms): (u64, Option<u32>, u64),
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let mut round = 0u32;
    loop {
        round = round.saturating_add(1);
        let v = env.verified()?;
        let coll = v.coll(&env.chain)?;
        let ts = Reader::new(&env.chain, v.engine.program)
            .oracle(&coll.oracle_id)?
            .timestamp;
        let age = env.chain_now()?.saturating_sub(ts);
        if age.saturating_add(refresh_before_ms) >= coll.max_oracle_age_ms {
            kv(
                out,
                "keeper",
                &format!("round {round}: oracle age {age} ms, publishing"),
            )?;
            publish(env, a, out)?;
        } else {
            kv(
                out,
                "keeper",
                &format!("round {round}: oracle age {age} ms, fresh"),
            )?;
        }
        if iterations.is_some_and(|n| round >= n) {
            return Ok(());
        }
        (env.sleep)(Duration::from_millis(interval_ms));
    }
}
