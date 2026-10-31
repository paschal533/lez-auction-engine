//! `dev oracle-publish` and `dev keeper` (ADR-050): the dev-network price publisher. The dev-command
//! guard (a named dev network + the recorded chain, ADR-046) has already run in `dev_cmds::dev`.

use std::io::Write;
use std::time::Duration;

use auction_sdk::{
    chain::Chain,
    decode::Reader,
    ops::{self, OraclePublishRequest},
};

use crate::{
    admin_cmds::print_after, args::OraclePublishCmd, env::Env, error::CliError, output::kv,
};

/// One move of the price (`ops::prepare_oracle_publish`): new price account, host rotation, engine
/// follow-up, each sent in turn.
pub fn publish<C: Chain>(
    env: &Env<C>,
    a: &OraclePublishCmd,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let v = env.verified()?;
    env.announce_admin(&v, out)?;
    let req = OraclePublishRequest {
        price_source: a.price_source.clone(),
        price: a.price,
        host_admin: a.host_admin.clone(),
        engine_admin: a.engine_admin.clone(),
        base_window_ms: a.base_window_ms,
    };
    let now = env.chain_now()?;
    let p = env.with_accounts(|accounts| {
        ops::prepare_oracle_publish(&env.chain, &env.deployment, &v, &req, accounts, now)
    })?;
    env.send_steps(&p, out)?;
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
