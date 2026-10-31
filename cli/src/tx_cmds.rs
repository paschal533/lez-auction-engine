//! Transaction commands (spec §6.6), printers over `auction_sdk::ops`. Each one: verify the config
//! (I4) and announce its admin, let `ops` quote, refuse and build the plan, then print its facts, lock,
//! sign with the wallet, send and confirm (spec §6.5), and print the touched accounts. Nothing is ever
//! resent.

use std::io::Write;

use auction_sdk::{
    chain::Chain,
    error::SdkResult,
    ops::{
        self, Accounts, BidRequest, CallerRequest, ClaimLeftoverRequest, LiquidateRequest,
        Prepared, SettleDebtRequest, SettleEmptyRequest, TakeRequest,
    },
    verify::VerifiedConfig,
};

use crate::{
    args::{BidCmd, LiquidateCmd, TakeCmd},
    env::Env,
    error::CliError,
};

/// I4 first, then the admin the user is about to sign against (before any signing), then `prepare`
/// at the chain clock, then the steps.
fn run<C: Chain>(
    env: &Env<C>,
    out: &mut dyn Write,
    prepare: impl FnOnce(&C, &VerifiedConfig, &Accounts<'_>, u64) -> SdkResult<Prepared>,
) -> Result<(), CliError> {
    env.refuse_at_ms()?;
    let v = env.verified()?;
    env.announce_admin(&v, out)?;
    let now = env.chain_now()?;
    let p = env.with_accounts(|accounts| prepare(&env.chain, &v, accounts, now))?;
    env.send_prepared(&v, &p, out)
}

fn caller(caller: &str) -> CallerRequest {
    CallerRequest {
        caller: caller.to_owned(),
    }
}

fn take(a: &TakeCmd) -> TakeRequest {
    TakeRequest {
        payer: a.payer.clone(),
        receiver: a.receiver.clone(),
        amount: a.amount,
        slippage_bps: a.slippage_bps,
    }
}

pub fn liquidate<C: Chain>(
    env: &Env<C>,
    a: &LiquidateCmd,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let req = LiquidateRequest {
        position: a.position.clone(),
        keeper: a.keeper.clone(),
        keeper_holding: a.keeper_holding.clone(),
    };
    run(env, out, |c, v, acc, now| {
        ops::prepare_liquidate(c, v, &req, acc, now)
    })
}

pub fn bid<C: Chain>(env: &Env<C>, a: &BidCmd, out: &mut dyn Write) -> Result<(), CliError> {
    let req = BidRequest {
        auction: a.auction.clone(),
        coin_in_max: a.coin_in_max,
        bidder_stable: a.bidder_stable.clone(),
        bidder_collateral: a.bidder_collateral.clone(),
        slippage_bps: a.slippage_bps,
        min_out: a.min_out,
    };
    run(env, out, |c, v, acc, now| {
        ops::prepare_bid(c, v, &req, acc, now)
    })
}

pub fn claim_leftover<C: Chain>(
    env: &Env<C>,
    owner: &str,
    owner_holding: &str,
    auction: &str,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let req = ClaimLeftoverRequest {
        owner: owner.to_owned(),
        owner_holding: owner_holding.to_owned(),
        auction: auction.to_owned(),
    };
    run(env, out, |c, v, acc, now| {
        ops::prepare_claim_leftover(c, v, &req, acc, now)
    })
}

pub fn settle_empty<C: Chain>(
    env: &Env<C>,
    caller: &str,
    auction: &str,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let req = SettleEmptyRequest {
        caller: caller.to_owned(),
        auction: auction.to_owned(),
    };
    run(env, out, |c, v, acc, now| {
        ops::prepare_settle_empty(c, v, &req, acc, now)
    })
}

pub fn settle_debt<C: Chain>(
    env: &Env<C>,
    caller: &str,
    amount: Option<u128>,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let req = SettleDebtRequest {
        caller: caller.to_owned(),
        amount,
    };
    run(env, out, |c, v, acc, now| {
        ops::prepare_settle_debt(c, v, &req, acc, now)
    })
}

pub fn check_solvency<C: Chain>(
    env: &Env<C>,
    who: &str,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    run(env, out, |c, v, acc, now| {
        ops::prepare_check_solvency(c, v, &caller(who), acc, now)
    })
}

pub fn observe_feed<C: Chain>(
    env: &Env<C>,
    who: &str,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    run(env, out, |c, v, acc, now| {
        ops::prepare_observe_feed(c, v, &caller(who), acc, now)
    })
}

pub fn start_surplus<C: Chain>(
    env: &Env<C>,
    who: &str,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    run(env, out, |c, v, acc, now| {
        ops::prepare_start_surplus(c, v, &caller(who), acc, now)
    })
}

pub fn take_surplus<C: Chain>(
    env: &Env<C>,
    a: &TakeCmd,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    run(env, out, |c, v, acc, now| {
        ops::prepare_take_surplus(c, v, &take(a), acc, now)
    })
}

pub fn close_surplus<C: Chain>(
    env: &Env<C>,
    who: &str,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    run(env, out, |c, v, acc, now| {
        ops::prepare_close_surplus(c, v, &caller(who), acc, now)
    })
}

pub fn start_debt<C: Chain>(env: &Env<C>, who: &str, out: &mut dyn Write) -> Result<(), CliError> {
    run(env, out, |c, v, acc, now| {
        ops::prepare_start_debt(c, v, &caller(who), acc, now)
    })
}

pub fn take_debt<C: Chain>(env: &Env<C>, a: &TakeCmd, out: &mut dyn Write) -> Result<(), CliError> {
    run(env, out, |c, v, acc, now| {
        ops::prepare_take_debt(c, v, &take(a), acc, now)
    })
}

pub fn close_debt<C: Chain>(env: &Env<C>, who: &str, out: &mut dyn Write) -> Result<(), CliError> {
    run(env, out, |c, v, acc, now| {
        ops::prepare_close_debt(c, v, &caller(who), acc, now)
    })
}
