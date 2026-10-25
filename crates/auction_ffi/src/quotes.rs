//! The quote views (`quote_*`): `auction_sdk::quote` and the engine's own functions, mapped to JSON.
//! Nothing here prices anything itself. Amounts in and out are decimal strings.

use auction_engine_program::feed;
use auction_sdk::{
    decode::Reader,
    oracle::check_new_price,
    quote::{self, LiquidationQuoteView},
    scan, views, SdkError,
};
use lee_core::account::AccountId;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    ctx::Ctx,
    envelope::{parse, FfiError},
    view::{cursor_path, read, Read},
};

/// The CLI's default `--slippage-bps`.
pub(crate) const DEFAULT_SLIPPAGE_BPS: u32 = 50;
/// Slippage must stay below 100 % (10 000 bps would remove the bound), as in the CLI.
pub(crate) const MAX_SLIPPAGE_BPS: u32 = 9_999;

#[must_use]
pub fn default_slippage() -> u32 {
    DEFAULT_SLIPPAGE_BPS
}

/// `slippage_bps` as the CLI accepts it.
pub fn slippage(bps: u32) -> Result<u32, FfiError> {
    if bps > MAX_SLIPPAGE_BPS {
        return Err(FfiError::bad(format!(
            "slippage_bps must be below 10000, got {bps}"
        )));
    }
    Ok(bps)
}

fn dec(v: u128) -> Value {
    Value::String(v.to_string())
}

#[derive(Deserialize)]
struct BidRequest {
    auction: AccountId,
    #[serde(with = "auction_sdk::dec")]
    coin_in_max: u128,
    #[serde(default = "default_slippage")]
    slippage_bps: u32,
}

/// `{auction, coin_in_max, slippage_bps}` -> what `auction-cli quote-bid` prints, plus the bound
/// and the instruction `prepare bid` would use.
pub fn bid(ctx: &Ctx, req: &Value) -> Result<Value, FfiError> {
    let r: BidRequest = parse(req)?;
    let bps = slippage(r.slippage_bps)?;
    let Read { v, now } = read(ctx)?;
    let q = quote::quote_bid(ctx.chain.as_ref(), &v, &r.auction, r.coin_in_max, 0, now)?;
    let instruction = if q.needs_settlement(bps) {
        "bid_with_settlement"
    } else {
        "bid"
    };
    Ok(json!({
        "auction": q.auction.to_string(),
        "now_ms": q.now_ms,
        "t_eff_ms": q.t_eff_ms,
        "discount_ray": dec(q.fill.discount_ray),
        "redemption_ray": dec(q.redemption_ray),
        "price_ray": dec(q.price_ray),
        "remaining_target": dec(q.remaining_target),
        "collateral_left": dec(q.collateral_left),
        "coin_in_max": dec(q.coin_in_max),
        "charged": dec(q.fill.charged),
        "collateral_out": dec(q.fill.collateral_out),
        "slippage_bps": bps,
        "min_out": dec(quote::min_out(q.fill.collateral_out, bps)),
        "debt_part": dec(q.fill.debt_part),
        "penalty_part": dec(q.fill.penalty_part),
        "ends_auction": q.fill.ends_auction,
        "instruction": instruction,
    }))
}

/// A liquidation quote as `auction-cli quote-liquidation` shows it, every field of the SDK's view.
#[must_use]
pub fn liquidation_json(q: &LiquidationQuoteView) -> Value {
    json!({
        "position": q.health.position.to_string(),
        "owner": q.health.owner.to_string(),
        "liquidatable": q.health.unsafe_at_min_cr,
        "nonce": q.nonce,
        "auction": q.auction.to_string(),
        "now_ms": q.now_ms,
        "collateral": dec(q.health.collateral),
        "nominal_debt": dec(q.health.nominal_debt),
        "vault_balance": dec(q.vault_balance),
        "redemption_ray": dec(q.redemption_ray),
        "market_ray": dec(q.market_ray),
        "price_ray": dec(q.price_ray),
        "debt_principal": dec(q.quote.debt_principal),
        "amount_to_raise": dec(q.quote.amount_to_raise),
        "reserve": dec(q.quote.reserve),
        "keeper_reward": dec(q.quote.keeper_reward),
        "lot": dec(q.quote.lot),
        "seized": dec(q.quote.seized),
    })
}

#[derive(Deserialize)]
struct PositionRequest {
    position: AccountId,
}

/// `{position}` -> `liquidation_json` (an error when the engine or the host would refuse).
pub fn liquidation(ctx: &Ctx, req: &Value) -> Result<Value, FfiError> {
    let PositionRequest { position } = parse(req)?;
    let Read { v, now } = read(ctx)?;
    let q = quote::quote_liquidation(ctx.chain.as_ref(), &v, &position, now)?;
    Ok(liquidation_json(&q))
}

#[derive(Deserialize)]
struct TakeRequest {
    #[serde(with = "auction_sdk::dec")]
    amount: u128,
    #[serde(default = "default_slippage")]
    slippage_bps: u32,
}

/// `{amount, slippage_bps}` -> the take of the latest surplus (or debt) auction: `filled`,
/// `tokens` (protocol tokens paid, or minted), `lot_left_after`, and `limit`, the bound `prepare`
/// sends (`max protocol in`, or `min protocol out`).
pub fn take(ctx: &Ctx, req: &Value, surplus: bool) -> Result<Value, FfiError> {
    let r: TakeRequest = parse(req)?;
    let bps = slippage(r.slippage_bps)?;
    let chain = ctx.chain.as_ref();
    let Read { v, now } = read(ctx)?;
    let acc = Reader::new(chain, v.engine.program).accounting(&v.engine.accounting)?;
    let (next_nonce, what) = if surplus {
        (acc.surplus_nonce, "surplus")
    } else {
        (acc.debt_nonce, "debt")
    };
    let nonce = next_nonce
        .checked_sub(1)
        .ok_or_else(|| SdkError::Refused(format!("no {what} auction has been started")))?;
    let (q, limit) = if surplus {
        let q = quote::quote_take_surplus(chain, &v, nonce, r.amount, now)?;
        (q, quote::max_in(q.tokens, bps))
    } else {
        let q = quote::quote_take_debt(chain, &v, nonce, r.amount, now)?;
        (q, quote::min_out(q.tokens, bps))
    };
    Ok(json!({
        "nonce": q.nonce,
        "now_ms": q.now_ms,
        "filled": dec(q.filled),
        "tokens": dec(q.tokens),
        "lot_left_after": dec(q.lot_left_after),
        "slippage_bps": bps,
        "limit": dec(limit),
    }))
}

#[derive(Deserialize)]
struct SettleDebtRequest {
    #[serde(default, with = "auction_sdk::dec::opt")]
    amount: Option<u128>,
}

/// `{amount?}` -> what `settle_debt` would burn now: the engine's own `Accounting::settle_debt`
/// (`min(amount, buffer, unbacked - debt on auction)`) on the accounting and the buffer balance,
/// with the values before and after.
pub fn settle_debt(ctx: &Ctx, req: &Value) -> Result<Value, FfiError> {
    let SettleDebtRequest { amount } = parse(req)?;
    let chain = ctx.chain.as_ref();
    let v = ctx.verified()?;
    let acc = Reader::new(chain, v.engine.program).accounting(&v.engine.accounting)?;
    let buffer = views::accounting(chain, &v)?.buffer;
    let (next, burned) = acc.core().settle_debt(amount.unwrap_or(u128::MAX), buffer);
    Ok(json!({
        "amount": dec(burned),
        "buffer_before": dec(buffer),
        "buffer_after": dec(buffer.saturating_sub(burned)),
        "unbacked_before": dec(acc.unbacked_debt),
        "unbacked_after": dec(next.unbacked_debt),
        "debt_on_auction": dec(acc.debt_on_auction),
    }))
}

#[derive(Deserialize)]
struct PriceRequest {
    #[serde(with = "auction_sdk::dec")]
    price: u128,
}

/// `{price}` -> the engine's verdict on publishing `price` now (`check_new_price`: `valid`, and the
/// engine's `reason` when not) and, per scanned position with debt, its CR now and after.
///
/// CR is the host's min-CR ratio under the redemption price R (R17), never the market price. R is
/// `feed::redemption_price` over the host's `RedemptionPriceState`; none of the publish's three
/// transactions (new price account, `SetMarketPriceOracle`, `set_collateral_config`) writes that
/// account, so R after the publish is the same function of the same state at the same clock, and
/// `cr_bps_after` equals `cr_bps_now` (R19).
pub fn price(ctx: &Ctx, req: &Value) -> Result<Value, FfiError> {
    let PriceRequest { price } = parse(req)?;
    let chain = ctx.chain.as_ref();
    let Read { v, now } = read(ctx)?;
    let r = Reader::new(chain, v.engine.program);
    let coll = v.coll(chain)?;
    let source = r.oracle(&coll.oracle_id)?.source_id;
    let (valid, reason) = match check_new_price(chain, &v, coll.oracle_id, source, price, now) {
        Ok(()) => (true, None),
        Err(e @ SdkError::Engine { .. }) => (false, Some(e.to_string())),
        Err(e) => return Err(e.into()),
    };
    let rps = r.meta(&v.host.redemption_price_state)?;
    let redemption =
        feed::redemption_price(&r.config(&v.config_id)?, &rps, now).map_err(SdkError::from)?;
    let cursor = scan::load_cursor(&cursor_path(ctx, &v))?;
    let positions: Vec<Value> = views::positions(chain, &v, &cursor, now)?
        .items
        .iter()
        .map(|p| {
            json!({
                "position": p.position.to_string(),
                "cr_bps_now": p.cr_bps,
                "cr_bps_after": p.cr_bps,
            })
        })
        .collect();
    Ok(json!({
        "price": dec(price),
        "valid": valid,
        "reason": reason,
        "redemption_ray": dec(redemption),
        "redemption_ray_after": dec(redemption),
        "positions": positions,
    }))
}
