//! `view`: read-only questions about the deployment, the scan cursor and the engine, each a thin
//! mapping of an `auction_sdk` view or quote (the numbers the CLI prints). The quotes live in
//! `quotes.rs`. u128 values are decimal strings; account ids are base58.
//!
//! Every view but `deployment` verifies the config first (I4) and evaluates at the chain's
//! `CLOCK_01`. Lists come from the cursor under `state_dir` (the CLI's file); only `scan` moves it,
//! one page per call, so no call blocks on a full scan.

use std::path::PathBuf;

use auction_sdk::{
    chain::clock_ms,
    decode::Reader,
    keystore::Keystore,
    quote,
    scan::{self, Cursor, ScanSettings, ScanTargets},
    verify::{verify_chain_identity, VerifiedConfig},
    views::{self, AuctionEntry, Listed},
    SdkError, SdkResult,
};
use lee_core::account::AccountId;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    ctx::Ctx,
    envelope::{parse, FfiError},
    quotes,
};

/// Samples of the discount schedule the `auction` view returns (the chart's resolution).
const SCHEDULE_SAMPLES: usize = 48;

/// Largest `scan` page: 5 `getBlockRange` reads of `ScanSettings::default().batch` (100) blocks,
/// plus the head, anchor and rewind reads. Each read has `READ_TIMEOUT` (5 s), so a healthy node
/// answers a page well inside the call budget; a slow one is cut off by the deadline (no read starts
/// or runs past it, ctx.rs), never by the 20 s IPC timeout. Private: cbindgen would export a `pub const`.
const MAX_SCAN_PAGE_BLOCKS: u64 = 500;

/// Every view name `dispatch` answers.
const VIEWS: &[&str] = &[
    "deployment",
    "overview",
    "scan",
    "live",
    "settled",
    "at_risk",
    "auction",
    "position",
    "accounts",
    "quote_bid",
    "quote_liquidation",
    "quote_take_surplus",
    "quote_take_debt",
    "quote_settle_debt",
    "quote_price",
];

fn name(req: &Value) -> Result<&str, FfiError> {
    req.get("view")
        .and_then(Value::as_str)
        .ok_or_else(|| FfiError::bad("the request names no \"view\""))
}

/// The `view` op: an unknown view is refused before the context is opened.
pub fn handle(req: Value) -> Result<Value, FfiError> {
    let view = name(&req)?;
    if !VIEWS.contains(&view) {
        return Err(FfiError::bad(format!("unknown view {view}")));
    }
    dispatch(&Ctx::from_request(&req)?, &req)
}

/// Answers `req["view"]` over an open context.
pub fn dispatch(ctx: &Ctx, req: &Value) -> Result<Value, FfiError> {
    match name(req)? {
        "deployment" => deployment(ctx),
        "overview" => overview(ctx),
        "scan" => scan_page(ctx, req),
        "live" => auctions(ctx, true),
        "settled" => auctions(ctx, false),
        "at_risk" => at_risk(ctx),
        "auction" => auction(ctx, req),
        "position" => position(ctx, req),
        "accounts" => accounts(ctx),
        "quote_bid" => quotes::bid(ctx, req),
        "quote_liquidation" => quotes::liquidation(ctx, req),
        "quote_take_surplus" => quotes::take(ctx, req, true),
        "quote_take_debt" => quotes::take(ctx, req, false),
        "quote_settle_debt" => quotes::settle_debt(ctx, req),
        "quote_price" => quotes::price(ctx, req),
        other => Err(FfiError::bad(format!("unknown view {other}"))),
    }
}

/// `{network, dev, config, genesis_ok}`. `genesis_ok` is the chain-identity check (false when the
/// deployment records no genesis hash or the node's block 1 differs); `dev` needs a named dev
/// network as well. A node that cannot be read is an error, not `false`.
pub fn deployment(ctx: &Ctx) -> Result<Value, FfiError> {
    let genesis_ok = match verify_chain_identity(ctx.chain.as_ref(), &ctx.deployment) {
        Ok(()) => true,
        Err(SdkError::Mismatch { .. }) => false,
        Err(e) => return Err(e.into()),
    };
    Ok(json!({
        "network": ctx.deployment.network,
        "dev": ctx.is_dev_network() && genesis_ok,
        "config": ctx.config_id().map(|c| c.to_string()),
        "genesis_ok": genesis_ok,
    }))
}

/// A verified config and the chain clock: what every engine view starts from.
pub struct Read {
    pub v: VerifiedConfig,
    pub now: u64,
}

pub fn read(ctx: &Ctx) -> SdkResult<Read> {
    let v = ctx.verified()?;
    let now = clock_ms(ctx.chain.as_ref())?;
    Ok(Read { v, now })
}

/// The CLI's cursor file: `<state_dir>/<config>.cursor.json`.
#[must_use]
pub fn cursor_path(ctx: &Ctx, v: &VerifiedConfig) -> PathBuf {
    ctx.state_dir.join(format!("{}.cursor.json", v.config_id))
}

fn cursor(ctx: &Ctx, v: &VerifiedConfig) -> SdkResult<Cursor> {
    scan::load_cursor(&cursor_path(ctx, v))
}

fn to_json<T: Serialize>(v: &T) -> Result<Value, FfiError> {
    serde_json::to_value(v).map_err(|e| FfiError::Sdk(SdkError::Encode(e.to_string())))
}

/// A list view: its rows, the candidates it skipped, and the block the cursor reached.
fn listed<T: Serialize>(l: &Listed<T>, c: &Cursor) -> Result<Value, FfiError> {
    Ok(
        json!({ "items": to_json(&l.items)?, "skipped": to_json(&l.skipped)?, "scanned_to": c.last_block }),
    )
}

/// `{block, accounting, host, oracle, counts:{live, at_risk, settled}, scanned_to}`. The counts are
/// the row counts of the `live`, `at_risk` and `settled` lists.
pub fn overview(ctx: &Ctx) -> Result<Value, FfiError> {
    let chain = ctx.chain.as_ref();
    let Read { v, now } = read(ctx)?;
    let c = cursor(ctx, &v)?;
    let all = views::auctions(chain, &v, &c, now)?;
    let live = all
        .items
        .iter()
        .filter(|e| matches!(e, AuctionEntry::Live(_)))
        .count();
    Ok(json!({
        "block": chain.last_block_id()?,
        "accounting": to_json(&views::accounting(chain, &v)?)?,
        "host": to_json(&views::host(chain, &v.host)?)?,
        "oracle": to_json(&views::oracle(chain, &v, now)?)?,
        "counts": {
            "live": live,
            "at_risk": views::at_risk(chain, &v, &c, now)?.items.len(),
            "settled": all.items.len() - live,
        },
        "scanned_to": c.last_block,
    }))
}

#[derive(Deserialize)]
struct ScanRequest {
    max_blocks: Option<u64>,
}

/// `{max_blocks}` -> `{scanned_to, head, done, restarted, note}`: one `scan_page` of at most
/// `max_blocks` blocks, the cursor saved under `state_dir`. Call again until `done`.
pub fn scan_page(ctx: &Ctx, req: &Value) -> Result<Value, FfiError> {
    let max_blocks = match parse::<ScanRequest>(req)?.max_blocks {
        Some(n) if (1..=MAX_SCAN_PAGE_BLOCKS).contains(&n) => n,
        _ => {
            return Err(FfiError::bad(format!(
                "scan needs \"max_blocks\" from 1 to {MAX_SCAN_PAGE_BLOCKS}"
            )))
        }
    };
    let v = ctx.verified()?;
    let path = cursor_path(ctx, &v);
    let page = scan::scan_page(
        ctx.chain.as_ref(),
        &ScanTargets::of(&v),
        &scan::load_cursor(&path)?,
        &ScanSettings::default(),
        max_blocks,
    )?;
    scan::save_cursor(&path, &page.scanned.cursor)?;
    Ok(json!({
        "scanned_to": page.scanned.cursor.last_block,
        "head": page.head,
        "done": page.done,
        "restarted": page.scanned.restarted.is_some(),
        "note": page.scanned.restarted.map(|r| r.to_string()),
    }))
}

/// The smallest charge a bid that does not end the auction may make (`CollateralParams::min_bid`,
/// which the engine's `bid` checks: `BelowMinBid`). A final bid is exempt.
fn min_bid(ctx: &Ctx, v: &VerifiedConfig) -> Result<u128, FfiError> {
    let coll =
        Reader::new(ctx.chain.as_ref(), v.engine.program).coll_config(&v.coll_config_id())?;
    Ok(coll.collateral_params().map_err(SdkError::from)?.min_bid)
}

/// A live auction's JSON with `min_bid` (decimal string) added for the bid card.
fn with_min_bid(mut row: Value, min_bid: u128) -> Value {
    row["min_bid"] = Value::String(min_bid.to_string());
    row
}

/// `live` (`LiveAuctionView` rows plus `min_bid`) or `settled` (`SettledAuctionView` rows), in
/// cursor order.
pub fn auctions(ctx: &Ctx, live: bool) -> Result<Value, FfiError> {
    let Read { v, now } = read(ctx)?;
    let c = cursor(ctx, &v)?;
    let all = views::auctions(ctx.chain.as_ref(), &v, &c, now)?;
    let floor = if live { min_bid(ctx, &v)? } else { 0 };
    let items: Vec<Value> = all
        .items
        .iter()
        .filter_map(|e| match (e, live) {
            (AuctionEntry::Live(a), true) => Some(to_json(a).map(|row| with_min_bid(row, floor))),
            (AuctionEntry::Settled(a), false) => Some(to_json(a)),
            _ => None,
        })
        .collect::<Result<_, _>>()?;
    listed(
        &Listed {
            items,
            skipped: all.skipped,
        },
        &c,
    )
}

/// `AtRiskView` rows, lowest CR first.
pub fn at_risk(ctx: &Ctx) -> Result<Value, FfiError> {
    let Read { v, now } = read(ctx)?;
    let c = cursor(ctx, &v)?;
    listed(&views::at_risk(ctx.chain.as_ref(), &v, &c, now)?, &c)
}

#[derive(Deserialize)]
struct IdRequest {
    id: AccountId,
}

/// `{id}` -> the auction's `LiveAuctionView` (plus `min_bid`) or `SettledAuctionView` (`kind` says
/// which) plus `schedule: [[t_ms, discount_ray], ...]` from `quote::discount_schedule`.
pub fn auction(ctx: &Ctx, req: &Value) -> Result<Value, FfiError> {
    let IdRequest { id } = parse(req)?;
    let chain = ctx.chain.as_ref();
    let Read { v, now } = read(ctx)?;
    let bids = cursor(ctx, &v)?
        .bids
        .get(&id.to_string())
        .copied()
        .unwrap_or(0);
    let entry = views::auction_entry(chain, &v, &id, bids, now)?;
    let mut out = to_json(&entry)?;
    if matches!(entry, AuctionEntry::Live(_)) {
        out = with_min_bid(out, min_bid(ctx, &v)?);
    }
    let state = Reader::new(chain, v.engine.program).auction(&id)?;
    let schedule: Vec<Value> = quote::discount_schedule(&state, SCHEDULE_SAMPLES)?
        .into_iter()
        .map(|(t, d)| json!([t, d.to_string()]))
        .collect();
    out["schedule"] = Value::Array(schedule);
    Ok(out)
}

/// `{id}` -> the position's health (the `AtRiskView` fields; `cr_bps` null without debt) plus its
/// liquidation quote, or `quote: null` and `refused` with the reason the engine or the host would
/// give (a safe position is an answer here, not an error).
pub fn position(ctx: &Ctx, req: &Value) -> Result<Value, FfiError> {
    let IdRequest { id } = parse(req)?;
    let chain = ctx.chain.as_ref();
    let Read { v, now } = read(ctx)?;
    let h = quote::position_health(chain, &v, &id, now)?;
    let (quote, refused) = match quote::quote_liquidation(chain, &v, &id, now) {
        Ok(q) => (quotes::liquidation_json(&q), None),
        Err(e @ (SdkError::HostReject(_) | SdkError::Engine { .. })) => {
            (Value::Null, Some(e.to_string()))
        }
        Err(e) => return Err(e.into()),
    };
    Ok(json!({
        "position": h.position.to_string(),
        "owner": h.owner.to_string(),
        "collateral": h.collateral.to_string(),
        "nominal_debt": h.nominal_debt.to_string(),
        "cr_bps": h.cr_bps,
        "min_cr_bps": h.min_cr_bps,
        "liquidatable": h.unsafe_at_min_cr,
        "quote": quote,
        "refused": refused,
    }))
}

/// The account picker: `views::accounts` over the wallet's labels.
pub fn accounts(ctx: &Ctx) -> Result<Value, FfiError> {
    let v = ctx.verified()?;
    let path = Keystore::default_path(ctx.wallet_home.as_deref()).map_err(SdkError::from)?;
    let ks = Keystore::open(&path).map_err(SdkError::from)?;
    let labels: Vec<(String, AccountId)> = ks
        .labels()
        .map(|(label, id)| (label.to_owned(), *id))
        .collect();
    to_json(&views::accounts(ctx.chain.as_ref(), &v, &labels)?)
}
