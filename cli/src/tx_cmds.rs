//! Transaction commands (spec §6.6). Each one: verify the config (I4) and announce its admin, quote
//! where a price is involved (the quote sets the slippage bound), refuse client-side what the engine
//! or the host would reject, build the plan, then lock, sign with the wallet, send and confirm
//! (spec §6.5). Nothing is ever resent.

use std::io::Write;

use auction_engine_core::state::AccountingState;
use auction_sdk::{
    chain::Chain,
    decode::Reader,
    error::SdkError,
    plan::{self, BidArgs, LiquidateArgs, TakeArgs},
    quote,
    verify::{verify_binding, VerifiedConfig},
};
use lee_core::account::AccountId;

use crate::{
    args::{BidCmd, LiquidateCmd, TakeCmd},
    env::Env,
    error::CliError,
    output::kv,
    tx_report::{Touch, TxMeta},
};

/// Read command that shows the outcome of an auction transaction whose result is unknown.
const CHECK_AUCTIONS: &str = "list-auctions";
/// Same for transactions that only move the engine's own accounts.
const CHECK_CONFIG: &str = "config";

/// I4 first, then the admin the user is about to sign against (before any signing).
fn begin<C: Chain>(env: &Env<C>, out: &mut dyn Write) -> Result<VerifiedConfig, CliError> {
    if env.at_ms.is_some() {
        return Err(CliError::Usage(
            "--at-ms is for read commands; transactions quote at the chain clock".into(),
        ));
    }
    let v = env.verified()?;
    env.announce_admin(&v, out)?;
    Ok(v)
}

fn accounting<C: Chain>(env: &Env<C>, v: &VerifiedConfig) -> Result<AccountingState, CliError> {
    Ok(Reader::new(&env.chain, v.engine.program).accounting(&v.engine.accounting)?)
}

/// The nonce of the latest auction, given the next nonce to be used.
fn latest(next_nonce: u64, what: &str) -> Result<u64, CliError> {
    next_nonce
        .checked_sub(1)
        .ok_or_else(|| CliError::Usage(format!("no {what} auction has been started")))
}

fn protocol_definition(v: &VerifiedConfig) -> Result<AccountId, CliError> {
    v.state.protocol_token.ok_or_else(|| {
        CliError::Usage("the config has no protocol token (token_not_configured)".into())
    })
}

fn send_with<C: Chain>(
    env: &Env<C>,
    v: &VerifiedConfig,
    p: &plan::Plan,
    (what, check): (&'static str, &'static str),
    touched: Vec<Touch>,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let meta = TxMeta {
        what,
        check,
        touched,
    };
    env.send(v, p, &meta, out)
}

pub fn liquidate<C: Chain>(
    env: &Env<C>,
    a: &LiquidateCmd,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let v = begin(env, out)?;
    verify_binding(&env.chain, &v)?;
    let position = env.account(&a.position)?;
    let keeper = env.account(&a.keeper)?;
    let holding = env.account(&a.keeper_holding)?;
    let q =
        quote::quote_liquidation(&env.chain, &v, &position, env.chain_now()?).map_err(
            |e| match e {
                SdkError::HostReject(m) => CliError::Usage(format!(
                    "refusing before signing: the host would reject this liquidation ({m})"
                )),
                other => other.into(),
            },
        )?;
    kv(out, "nonce", &q.nonce.to_string())?;
    kv(out, "auction", &q.auction.to_string())?;
    kv(out, "keeper reward", &q.quote.keeper_reward.to_string())?;
    let p = plan::liquidate(
        &v,
        &LiquidateArgs {
            keeper,
            keeper_collateral_holding: holding,
            keeper_holding_is_fresh: env.is_fresh(&holding)?,
            position,
            nonce: q.nonce,
            oracle: v.coll(&env.chain)?.oracle_id,
        },
    )?;
    let touched = vec![
        Touch::Auction(q.auction),
        Touch::Holding {
            label: "keeper",
            id: holding,
            definition: v.state.collateral_definition_id,
        },
    ];
    send_with(env, &v, &p, ("liquidate", CHECK_AUCTIONS), touched, out)
}

pub fn bid<C: Chain>(env: &Env<C>, a: &BidCmd, out: &mut dyn Write) -> Result<(), CliError> {
    let v = begin(env, out)?;
    let auction = env.account(&a.auction)?;
    let stable = env.account(&a.bidder_stable)?;
    let collateral = env.account(&a.bidder_collateral)?;
    let now = env.chain_now()?;
    let q = quote::quote_bid(&env.chain, &v, &auction, a.coin_in_max, 0, now)?;
    let min_out = a
        .min_out
        .unwrap_or_else(|| quote::min_out(q.fill.collateral_out, a.slippage_bps));
    if a.min_out.is_some() {
        // The engine's own check, run now: a bound above the quote would revert (Slippage).
        quote::quote_bid(&env.chain, &v, &auction, a.coin_in_max, min_out, now)?;
    }
    let with_settlement = q.needs_settlement(a.slippage_bps);
    kv(out, "quoted out", &q.fill.collateral_out.to_string())?;
    kv(out, "min out", &min_out.to_string())?;
    let p = plan::bid(
        &v,
        &BidArgs {
            bidder_stable: stable,
            bidder_collateral: collateral,
            bidder_collateral_is_fresh: env.is_fresh(&collateral)?,
            auction,
            coin_in_max: a.coin_in_max,
            min_collateral_out: min_out,
            with_settlement,
            oracle: v.coll(&env.chain)?.oracle_id,
        },
    )?;
    let what = if with_settlement {
        "bid_with_settlement"
    } else {
        "bid"
    };
    let touched = vec![
        Touch::Auction(auction),
        Touch::Holding {
            label: "bidder coll",
            id: collateral,
            definition: v.state.collateral_definition_id,
        },
        Touch::Holding {
            label: "bidder stable",
            id: stable,
            definition: v.state.stablecoin_definition_id,
        },
    ];
    send_with(env, &v, &p, (what, CHECK_AUCTIONS), touched, out)
}

pub fn claim_leftover<C: Chain>(
    env: &Env<C>,
    owner: &str,
    owner_holding: &str,
    auction: &str,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let v = begin(env, out)?;
    let holding = env.account(owner_holding)?;
    let auction = env.account(auction)?;
    let p = plan::claim_leftover(
        &v,
        env.account(owner)?,
        holding,
        env.is_fresh(&holding)?,
        auction,
    )?;
    let touched = vec![
        Touch::Auction(auction),
        Touch::Holding {
            label: "owner",
            id: holding,
            definition: v.state.collateral_definition_id,
        },
    ];
    send_with(
        env,
        &v,
        &p,
        ("claim_leftover", CHECK_AUCTIONS),
        touched,
        out,
    )
}

pub fn settle_empty<C: Chain>(
    env: &Env<C>,
    caller: &str,
    auction: &str,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let v = begin(env, out)?;
    let auction = env.account(auction)?;
    let p = plan::settle_empty(&v, env.account(caller)?, auction)?;
    let touched = vec![Touch::Auction(auction), Touch::Accounting];
    send_with(env, &v, &p, ("settle_empty", CHECK_AUCTIONS), touched, out)
}

/// A system transaction that only moves the engine's accounting.
fn accounting_tx<C: Chain>(
    env: &Env<C>,
    v: &VerifiedConfig,
    p: &plan::Plan,
    what: &'static str,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    send_with(
        env,
        v,
        p,
        (what, CHECK_CONFIG),
        vec![Touch::Accounting],
        out,
    )
}

pub fn settle_debt<C: Chain>(
    env: &Env<C>,
    caller: &str,
    amount: Option<u128>,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let v = begin(env, out)?;
    let p = plan::settle_debt(&v, env.account(caller)?, amount.unwrap_or(u128::MAX))?;
    accounting_tx(env, &v, &p, "settle_debt", out)
}

pub fn check_solvency<C: Chain>(
    env: &Env<C>,
    caller: &str,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let v = begin(env, out)?;
    let p = plan::check_solvency(&v, env.account(caller)?)?;
    accounting_tx(env, &v, &p, "check_solvency", out)
}

pub fn observe_feed<C: Chain>(
    env: &Env<C>,
    caller: &str,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let v = begin(env, out)?;
    let p = plan::observe_feed(&v, env.account(caller)?, v.coll(&env.chain)?.oracle_id)?;
    send_with(env, &v, &p, ("observe_feed", CHECK_CONFIG), Vec::new(), out)
}

pub fn start_surplus<C: Chain>(
    env: &Env<C>,
    caller: &str,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let v = begin(env, out)?;
    let acc = accounting(env, &v)?;
    if acc.surplus_live {
        return Err(CliError::Usage(format!(
            "a surplus auction is live (nonce {}); take it, or close it after its TTL, first",
            latest(acc.surplus_nonce, "surplus")?
        )));
    }
    let p = plan::start_surplus(&v, env.account(caller)?, acc.surplus_nonce)?;
    accounting_tx(env, &v, &p, "start_surplus_auction", out)
}

pub fn take_surplus<C: Chain>(
    env: &Env<C>,
    a: &TakeCmd,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let v = begin(env, out)?;
    let nonce = latest(accounting(env, &v)?.surplus_nonce, "surplus")?;
    let q = quote::quote_take_surplus(&env.chain, &v, nonce, a.amount, env.chain_now()?)?;
    let limit = quote::max_in(q.tokens, a.slippage_bps);
    kv(out, "quoted tokens", &q.tokens.to_string())?;
    kv(out, "max protocol in", &limit.to_string())?;
    let (payer, receiver) = (env.account(&a.payer)?, env.account(&a.receiver)?);
    let p = plan::take_surplus(
        &v,
        &TakeArgs {
            payer,
            receiver,
            receiver_is_fresh: env.is_fresh(&receiver)?,
            nonce,
            amount_coin: a.amount,
            limit,
        },
    )?;
    let touched = take_touched(&v, payer, receiver, protocol_definition(&v)?, true);
    send_with(env, &v, &p, ("take_surplus", CHECK_CONFIG), touched, out)
}

pub fn close_surplus<C: Chain>(
    env: &Env<C>,
    caller: &str,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let v = begin(env, out)?;
    let nonce = latest(accounting(env, &v)?.surplus_nonce, "surplus")?;
    let p = plan::close_surplus(&v, env.account(caller)?, nonce)?;
    accounting_tx(env, &v, &p, "close_surplus_auction", out)
}

pub fn start_debt<C: Chain>(
    env: &Env<C>,
    caller: &str,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let v = begin(env, out)?;
    let acc = accounting(env, &v)?;
    if acc.debt_live {
        return Err(CliError::Usage(format!(
            "a debt auction is live (nonce {}); take it, or close it after its TTL, first",
            latest(acc.debt_nonce, "debt")?
        )));
    }
    if let Some(earliest) = quote::earliest_debt_start(&acc) {
        if env.chain_now()? < earliest {
            return Err(CliError::Usage(format!(
                "relist delay (ADR-039): the next debt auction may start at {earliest} ms"
            )));
        }
    }
    let p = plan::start_debt(&v, env.account(caller)?, acc.debt_nonce)?;
    accounting_tx(env, &v, &p, "start_debt_auction", out)
}

pub fn take_debt<C: Chain>(env: &Env<C>, a: &TakeCmd, out: &mut dyn Write) -> Result<(), CliError> {
    let v = begin(env, out)?;
    let nonce = latest(accounting(env, &v)?.debt_nonce, "debt")?;
    let q = quote::quote_take_debt(&env.chain, &v, nonce, a.amount, env.chain_now()?)?;
    let limit = quote::min_out(q.tokens, a.slippage_bps);
    kv(out, "quoted tokens", &q.tokens.to_string())?;
    kv(out, "min protocol out", &limit.to_string())?;
    let (payer, receiver) = (env.account(&a.payer)?, env.account(&a.receiver)?);
    let p = plan::take_debt(
        &v,
        &TakeArgs {
            payer,
            receiver,
            receiver_is_fresh: env.is_fresh(&receiver)?,
            nonce,
            amount_coin: a.amount,
            limit,
        },
    )?;
    let touched = take_touched(&v, payer, receiver, protocol_definition(&v)?, false);
    send_with(env, &v, &p, ("take_debt", CHECK_CONFIG), touched, out)
}

pub fn close_debt<C: Chain>(
    env: &Env<C>,
    caller: &str,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let v = begin(env, out)?;
    let nonce = latest(accounting(env, &v)?.debt_nonce, "debt")?;
    let p = plan::close_debt(&v, env.account(caller)?, nonce)?;
    accounting_tx(env, &v, &p, "close_debt_auction", out)
}

/// The payer and receiver holdings of a take, with the token each is of: surplus pays protocol
/// tokens for stablecoin, debt pays stablecoin for protocol tokens.
fn take_touched(
    v: &VerifiedConfig,
    payer: AccountId,
    receiver: AccountId,
    protocol: AccountId,
    surplus: bool,
) -> Vec<Touch> {
    let stable = v.state.stablecoin_definition_id;
    let (paid, received) = if surplus {
        (protocol, stable)
    } else {
        (stable, protocol)
    };
    vec![
        Touch::Accounting,
        Touch::Holding {
            label: "payer",
            id: payer,
            definition: paid,
        },
        Touch::Holding {
            label: "receiver",
            id: receiver,
            definition: received,
        },
    ]
}
