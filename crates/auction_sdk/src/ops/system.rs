//! System transactions: debt settlement, solvency, the feed observation, and the surplus and debt
//! auctions (start refused while one is live, takes quoted for their slippage bound, closes).

use auction_engine_core::state::AccountingState;
use lee_core::account::AccountId;

use crate::{
    chain::Chain,
    decode::Reader,
    error::{SdkError, SdkResult},
    plan::{self, Plan, TakeArgs},
    quote,
    verify::VerifiedConfig,
};

use super::{fact, is_fresh, Accounts, CallerRequest, Prepared, Touch, CHECK_CONFIG};

/// The flags of `auction-cli settle-debt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettleDebtRequest {
    pub caller: String,
    /// Default: as much as possible.
    pub amount: Option<u128>,
}

/// The flags of `auction-cli take-surplus` and `take-debt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakeRequest {
    pub payer: String,
    pub receiver: String,
    pub amount: u128,
    pub slippage_bps: u32,
}

fn accounting<C: Chain + ?Sized>(chain: &C, v: &VerifiedConfig) -> SdkResult<AccountingState> {
    Reader::new(chain, v.engine.program).accounting(&v.engine.accounting)
}

/// The nonce of the latest auction, given the next nonce to be used.
fn latest(next_nonce: u64, what: &str) -> SdkResult<u64> {
    next_nonce
        .checked_sub(1)
        .ok_or_else(|| SdkError::Refused(format!("no {what} auction has been started")))
}

fn protocol_definition(v: &VerifiedConfig) -> SdkResult<AccountId> {
    v.state.protocol_token.ok_or_else(|| {
        SdkError::Refused("the config has no protocol token (token_not_configured)".into())
    })
}

/// A system transaction that only moves the engine's accounting.
fn accounting_tx(p: Plan, what: &'static str) -> Prepared {
    Prepared::one(p, what, CHECK_CONFIG).with_touched(vec![Touch::Accounting])
}

fn refuse_live(live: bool, next_nonce: u64, what: &str) -> SdkResult<()> {
    if live {
        return Err(SdkError::Refused(format!(
            "a {what} auction is live (nonce {}); take it, or close it after its TTL, first",
            latest(next_nonce, what)?
        )));
    }
    Ok(())
}

pub fn prepare_settle_debt<C: Chain + ?Sized>(
    _chain: &C,
    v: &VerifiedConfig,
    req: &SettleDebtRequest,
    accounts: &Accounts<'_>,
    _now_ms: u64,
) -> SdkResult<Prepared> {
    let p = plan::settle_debt(
        v,
        accounts.id(&req.caller)?,
        req.amount.unwrap_or(u128::MAX),
    )?;
    Ok(accounting_tx(p, "settle_debt"))
}

pub fn prepare_check_solvency<C: Chain + ?Sized>(
    _chain: &C,
    v: &VerifiedConfig,
    req: &CallerRequest,
    accounts: &Accounts<'_>,
    _now_ms: u64,
) -> SdkResult<Prepared> {
    let p = plan::check_solvency(v, accounts.id(&req.caller)?)?;
    Ok(accounting_tx(p, "check_solvency"))
}

pub fn prepare_observe_feed<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    req: &CallerRequest,
    accounts: &Accounts<'_>,
    _now_ms: u64,
) -> SdkResult<Prepared> {
    let p = plan::observe_feed(v, accounts.id(&req.caller)?, v.coll(chain)?.oracle_id)?;
    Ok(Prepared::one(p, "observe_feed", CHECK_CONFIG))
}

pub fn prepare_start_surplus<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    req: &CallerRequest,
    accounts: &Accounts<'_>,
    _now_ms: u64,
) -> SdkResult<Prepared> {
    let acc = accounting(chain, v)?;
    refuse_live(acc.surplus_live, acc.surplus_nonce, "surplus")?;
    let p = plan::start_surplus(v, accounts.id(&req.caller)?, acc.surplus_nonce)?;
    Ok(accounting_tx(p, "start_surplus_auction"))
}

pub fn prepare_take_surplus<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    req: &TakeRequest,
    accounts: &Accounts<'_>,
    now_ms: u64,
) -> SdkResult<Prepared> {
    let nonce = latest(accounting(chain, v)?.surplus_nonce, "surplus")?;
    let q = quote::quote_take_surplus(chain, v, nonce, req.amount, now_ms)?;
    let limit = quote::max_in(q.tokens, req.slippage_bps);
    let facts = vec![
        fact("quoted tokens", q.tokens),
        fact("max protocol in", limit),
    ];
    take(chain, v, req, accounts, (nonce, limit), true).map(|p| p.with_facts(facts))
}

pub fn prepare_close_surplus<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    req: &CallerRequest,
    accounts: &Accounts<'_>,
    _now_ms: u64,
) -> SdkResult<Prepared> {
    let nonce = latest(accounting(chain, v)?.surplus_nonce, "surplus")?;
    let p = plan::close_surplus(v, accounts.id(&req.caller)?, nonce)?;
    Ok(accounting_tx(p, "close_surplus_auction"))
}

pub fn prepare_start_debt<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    req: &CallerRequest,
    accounts: &Accounts<'_>,
    now_ms: u64,
) -> SdkResult<Prepared> {
    let acc = accounting(chain, v)?;
    refuse_live(acc.debt_live, acc.debt_nonce, "debt")?;
    if let Some(earliest) = quote::earliest_debt_start(&acc) {
        if now_ms < earliest {
            return Err(SdkError::Refused(format!(
                "relist delay (ADR-039): the next debt auction may start at {earliest} ms"
            )));
        }
    }
    let p = plan::start_debt(v, accounts.id(&req.caller)?, acc.debt_nonce)?;
    Ok(accounting_tx(p, "start_debt_auction"))
}

pub fn prepare_take_debt<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    req: &TakeRequest,
    accounts: &Accounts<'_>,
    now_ms: u64,
) -> SdkResult<Prepared> {
    let nonce = latest(accounting(chain, v)?.debt_nonce, "debt")?;
    let q = quote::quote_take_debt(chain, v, nonce, req.amount, now_ms)?;
    let limit = quote::min_out(q.tokens, req.slippage_bps);
    let facts = vec![
        fact("quoted tokens", q.tokens),
        fact("min protocol out", limit),
    ];
    take(chain, v, req, accounts, (nonce, limit), false).map(|p| p.with_facts(facts))
}

pub fn prepare_close_debt<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    req: &CallerRequest,
    accounts: &Accounts<'_>,
    _now_ms: u64,
) -> SdkResult<Prepared> {
    let nonce = latest(accounting(chain, v)?.debt_nonce, "debt")?;
    let p = plan::close_debt(v, accounts.id(&req.caller)?, nonce)?;
    Ok(accounting_tx(p, "close_debt_auction"))
}

/// The plan of a quoted take and the holdings it moves: surplus pays protocol tokens for stablecoin,
/// debt pays stablecoin for protocol tokens.
fn take<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    req: &TakeRequest,
    accounts: &Accounts<'_>,
    (nonce, limit): (u64, u128),
    surplus: bool,
) -> SdkResult<Prepared> {
    let (payer, receiver) = (accounts.id(&req.payer)?, accounts.id(&req.receiver)?);
    let args = TakeArgs {
        payer,
        receiver,
        receiver_is_fresh: is_fresh(chain, &receiver)?,
        nonce,
        amount_coin: req.amount,
        limit,
    };
    let (p, what) = if surplus {
        (plan::take_surplus(v, &args)?, "take_surplus")
    } else {
        (plan::take_debt(v, &args)?, "take_debt")
    };
    let protocol = protocol_definition(v)?;
    let stable = v.state.stablecoin_definition_id;
    let (paid, received) = if surplus {
        (protocol, stable)
    } else {
        (stable, protocol)
    };
    Ok(Prepared::one(p, what, CHECK_CONFIG).with_touched(vec![
        Touch::Accounting,
        Touch::Holding {
            label: "payer".into(),
            id: payer,
            definition: paid,
        },
        Touch::Holding {
            label: "receiver".into(),
            id: receiver,
            definition: received,
        },
    ]))
}
