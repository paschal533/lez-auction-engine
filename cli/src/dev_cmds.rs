//! Dev commands: the dev-network setup steps (tokens, oracle, balances) that used to need spel.

use std::io::Write;

use auction_sdk::{
    chain::Chain,
    error::SdkResult,
    ops::{self, TokenTransferRequest, CHECK_BALANCE},
    plan_host,
};
use lee_core::account::{Account, AccountId};
use token_core::TokenHolding;

use crate::{
    admin_cmds::{optional, print_after},
    args::DevCommand,
    env::Env,
    error::CliError,
    output::kv,
};

/// Read-only dev commands run anywhere; the others sign, so they need a named dev network in the
/// deployment file AND the chain it recorded (ADR-046; Plan 3a M6: a label alone let `--rpc` aim them
/// at any node).
pub(crate) fn is_read_only(cmd: &DevCommand) -> bool {
    matches!(
        cmd,
        DevCommand::Resolve { .. } | DevCommand::Balance { .. } | DevCommand::ChainId
    )
}

pub fn dev<C: Chain>(env: &Env<C>, cmd: &DevCommand, out: &mut dyn Write) -> Result<(), CliError> {
    if !is_read_only(cmd) {
        env.refuse_at_ms()?;
        ops::require_dev_network(&env.chain, &env.deployment)?;
    }
    match cmd {
        DevCommand::ChainId => {
            let b = env.chain.blocks(1, 1)?;
            let first = b
                .first()
                .ok_or_else(|| CliError::Usage("the node returned no block 1".into()))?;
            kv(out, "genesis", &hex::encode(first.hash))
        }
        DevCommand::Resolve { label } => {
            kv(out, "id", &env.keystore()?.resolve(label)?.to_string())
        }
        DevCommand::TokenNew {
            definition,
            holding,
            name,
            supply,
            mint_authority,
        } => token_new(
            env,
            (definition, holding),
            (name, *supply, mint_authority.as_ref()),
            out,
        ),
        DevCommand::TokenInitAccount {
            definition,
            account,
        } => {
            let account = env.account(account)?;
            let plan = plan_host::token_initialize_account(
                env.deployment.token_program,
                env.account(definition)?,
                account,
            )?;
            env.send_plain(&plan, "initialize_account", CHECK_BALANCE, out)?;
            print_after(out, "account", describe(env, &account))
        }
        DevCommand::TokenTransfer { from, to, amount } => {
            let req = TokenTransferRequest {
                from: from.clone(),
                to: to.clone(),
                amount: *amount,
            };
            let p = env.with_accounts(|accounts| {
                ops::prepare_token_transfer(&env.deployment, &req, accounts)
            })?;
            env.send_steps(&p, out)?;
            print_after(out, "from", describe(env, &env.account(from)?))?;
            print_after(out, "to", describe(env, &env.account(to)?))
        }
        DevCommand::OracleCreate {
            price_source,
            base,
            quote,
            price,
            window_ms,
        } => oracle_create(env, (price_source, base, quote), (*price, *window_ms), out),
        DevCommand::Balance { account } => balance(env, account, out),
        DevCommand::OraclePublish(a) => crate::oracle_cmds::publish(env, a, out),
        DevCommand::Keeper {
            publish,
            interval_ms,
            iterations,
            refresh_before_ms,
        } => crate::oracle_cmds::keeper(
            env,
            publish,
            (*interval_ms, *iterations, *refresh_before_ms),
            out,
        ),
    }
}

fn token_new<C: Chain>(
    env: &Env<C>,
    (definition, holding): (&str, &str),
    (name, supply, mint_authority): (&str, u128, Option<&String>),
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let definition = env.account(definition)?;
    let holding = env.account(holding)?;
    let authority = optional(env, mint_authority)?;
    kv(out, "definition", &definition.to_string())?;
    let plan = plan_host::token_new_fungible(
        env.deployment.token_program,
        definition,
        holding,
        name,
        supply,
        authority,
    )?;
    env.send_plain(&plan, "new_fungible_definition", CHECK_BALANCE, out)?;
    print_after(out, "holding", describe(env, &holding))
}

fn oracle_create<C: Chain>(
    env: &Env<C>,
    (price_source, base, quote): (&str, &str, &str),
    (price, window_ms): (u128, u64),
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let twap = env.deployment.twap_oracle_program.ok_or_else(|| {
        CliError::Usage("the deployment file has no twap_oracle_program_id".into())
    })?;
    let (plan, oracle) = plan_host::create_oracle_price_account(
        twap,
        env.account(price_source)?,
        env.account(base)?,
        env.account(quote)?,
        price,
        window_ms,
    )?;
    kv(out, "oracle", &oracle.to_string())?;
    env.send_plain(&plan, "create_oracle_price_account", CHECK_BALANCE, out)?;
    let initialised = env.chain.account(&oracle).map(|a| {
        if a == Account::default() {
            "uninitialised".to_owned()
        } else {
            "initialised".to_owned()
        }
    });
    print_after(out, "oracle", initialised)
}

fn balance<C: Chain>(env: &Env<C>, account: &str, out: &mut dyn Write) -> Result<(), CliError> {
    let id = env.account(account)?;
    kv(out, "account", &id.to_string())?;
    let acc = env.chain.account(&id)?;
    if acc == Account::default() {
        return kv(out, "state", "uninitialised");
    }
    match TokenHolding::try_from(&acc.data) {
        Ok(TokenHolding::Fungible {
            definition_id,
            balance,
        }) => {
            kv(out, "definition", &definition_id.to_string())?;
            kv(out, "balance", &balance.to_string())
        }
        _ => kv(out, "state", "not a fungible token holding"),
    }
}

/// One-line description of a token holding, for the post-inclusion re-read.
fn describe<C: Chain>(env: &Env<C>, id: &AccountId) -> SdkResult<String> {
    let acc = env.chain.account(id)?;
    if acc == Account::default() {
        return Ok("uninitialised".to_owned());
    }
    Ok(match TokenHolding::try_from(&acc.data) {
        Ok(TokenHolding::Fungible {
            definition_id,
            balance,
        }) => format!("balance {balance} of {definition_id}"),
        _ => "not a fungible token holding".to_owned(),
    })
}
