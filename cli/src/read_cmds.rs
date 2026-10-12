//! Read-only commands: config, quotes, lists, rescan, watch. Every one verifies the config first (I4).

use std::io::Write;
use std::time::Duration;

use lee_core::account::AccountId;

use auction_sdk::{
    chain::Chain,
    deployment::program_id_hex,
    error::SdkError,
    quote::{self, AuctionView},
    scan::{self, Cursor, ScanSettings, ScanTargets},
    verify::VerifiedConfig,
};

use crate::{
    args::ScanArgs,
    env::Env,
    error::CliError,
    output::{bps, kv, yes_no},
};

pub fn config<C: Chain>(env: &Env<C>, out: &mut dyn Write) -> Result<(), CliError> {
    let v = env.verified()?;
    let s = &v.state;
    kv(out, "config", &v.config_id.to_string())?;
    env.announce_admin(&v, out)?;
    kv(out, "engine program", &program_id_hex(&v.engine.program))?;
    kv(out, "host program", &program_id_hex(&v.host.program))?;
    kv(out, "token program", &program_id_hex(&s.token_program_id))?;
    kv(out, "stablecoin", &s.stablecoin_definition_id.to_string())?;
    kv(out, "collateral", &s.collateral_definition_id.to_string())?;
    let protocol_token = s
        .protocol_token
        .map_or_else(|| "none".to_owned(), |t| t.to_string());
    kv(out, "protocol token", &protocol_token)?;
    kv(out, "global cap", &s.global_cap.to_string())?;
    kv(out, "global reserved", &s.global_reserved.to_string())?;
    kv(out, "bad-debt freeze", yes_no(s.freeze_on_bad_debt))
}

pub fn quote_liquidation<C: Chain>(
    env: &Env<C>,
    position: &str,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let v = env.verified()?;
    let pos = env.account(position)?;
    let now = env.now()?;
    let q = quote::quote_liquidation(&env.chain, &v, &pos, now)?;
    kv(out, "position", &pos.to_string())?;
    kv(out, "owner", &q.health.owner.to_string())?;
    kv(out, "unsafe (min CR)", yes_no(q.health.unsafe_at_min_cr))?;
    kv(out, "nonce", &q.nonce.to_string())?;
    kv(out, "auction", &q.auction.to_string())?;
    kv(out, "collateral", &q.health.collateral.to_string())?;
    kv(out, "vault balance", &q.vault_balance.to_string())?;
    kv(out, "nominal debt", &q.health.nominal_debt.to_string())?;
    kv(out, "amount to raise", &q.quote.amount_to_raise.to_string())?;
    kv(out, "keeper reward", &q.quote.keeper_reward.to_string())?;
    kv(out, "lot", &q.quote.lot.to_string())?;
    kv(out, "price (RAY)", &q.price_ray.to_string())?;
    kv(out, "clock (ms)", &now.to_string())
}

pub fn quote_bid<C: Chain>(
    env: &Env<C>,
    auction: &str,
    coin_in_max: u128,
    slippage_bps: u32,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let v = env.verified()?;
    let a = env.account(auction)?;
    let now = env.now()?;
    let q = quote::quote_bid(&env.chain, &v, &a, coin_in_max, 0, now)?;
    kv(out, "auction", &a.to_string())?;
    kv(out, "clock (ms)", &now.to_string())?;
    kv(out, "t_eff (ms)", &q.t_eff_ms.to_string())?;
    kv(out, "discount", &bps(q.fill.discount_ray))?;
    kv(out, "price (RAY)", &q.price_ray.to_string())?;
    kv(out, "remaining target", &q.remaining_target.to_string())?;
    kv(out, "collateral left", &q.collateral_left.to_string())?;
    kv(out, "charged", &q.fill.charged.to_string())?;
    kv(out, "collateral out", &q.fill.collateral_out.to_string())?;
    kv(
        out,
        &format!("min out ({slippage_bps} bps)"),
        &quote::min_out(q.fill.collateral_out, slippage_bps).to_string(),
    )?;
    kv(out, "debt part", &q.fill.debt_part.to_string())?;
    kv(out, "penalty part", &q.fill.penalty_part.to_string())?;
    kv(out, "ends auction", yes_no(q.fill.ends_auction))?;
    let ix = if q.needs_settlement(slippage_bps) {
        "bid_with_settlement"
    } else {
        "bid"
    };
    kv(out, "instruction", ix)
}

fn scan_settings(args: &ScanArgs) -> ScanSettings {
    let default = ScanSettings::default();
    ScanSettings {
        batch: args.batch.unwrap_or(default.batch),
        ..default
    }
}

/// Scans the new blocks and persists the cursor.
pub(crate) fn scan_now<C: Chain>(
    env: &Env<C>,
    v: &VerifiedConfig,
    args: &ScanArgs,
) -> Result<Cursor, CliError> {
    let path = env.cursor_path(&v.config_id);
    let cursor = scan::load_cursor(&path)?;
    let next = scan::scan(
        &env.chain,
        &ScanTargets::of(v),
        &cursor,
        &scan_settings(args),
    )?;
    scan::save_cursor(&path, &next)?;
    Ok(next)
}

fn auction_line(view: &AuctionView) -> String {
    let s = &view.state;
    let raised = s.debt_raised + s.penalty_raised;
    if s.settled {
        return format!(
            "{}  settled  sold {}  raised {}/{}  leftover {} (claimed {})",
            view.id,
            s.collateral_sold,
            raised,
            s.amount_to_raise,
            s.leftover_claimable,
            s.leftover_claimed
        );
    }
    let mut line = format!(
        "{}  live  lot {}  raised {}/{}  discount {}  t_eff {} ms",
        view.id,
        s.collateral_left,
        raised,
        s.amount_to_raise,
        bps(view.discount_ray),
        view.t_eff_ms
    );
    if view.held_at_max {
        line.push_str(&format!(
            "  held at max discount (keeps its cap reservation of {})",
            s.reserved_amount
        ));
    }
    if let Some(p) = &view.paused {
        line.push_str(&format!("  bids revert now: {p} (run observe-feed)"));
    }
    line
}

/// Scanner candidates can be stale (ADR-043). An id whose account is gone or no longer decodes is an
/// expected leftover and is dropped silently. An id that decodes but is the wrong account
/// (`Mismatch`) or that the engine rejects (`Engine`) is skipped with a warning on stderr, so one
/// bad id never hides the others and never hides itself either. Anything else fails the command.
fn skip_stale(id: &AccountId, e: SdkError, err: &mut dyn Write) -> Result<(), CliError> {
    match e {
        SdkError::Missing { .. } | SdkError::Decode { .. } => Ok(()),
        SdkError::Mismatch { .. } | SdkError::Engine { .. } => {
            writeln!(err, "warning: skipped {id}: {e}")?;
            Ok(())
        }
        other => Err(other.into()),
    }
}

pub fn list_auctions<C: Chain>(
    env: &Env<C>,
    all: bool,
    args: &ScanArgs,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    let v = env.verified()?;
    let cursor = scan_now(env, &v, args)?;
    let now = env.now()?;
    kv(out, "scanned to block", &cursor.last_block.to_string())?;
    let mut shown = 0usize;
    for id in cursor.auction_ids()? {
        let view = match quote::auction_view(&env.chain, &v, &id, now) {
            Ok(view) => view,
            Err(e) => {
                skip_stale(&id, e, err)?;
                continue;
            }
        };
        if view.state.settled && !all {
            continue;
        }
        shown += 1;
        writeln!(out, "{}", auction_line(&view))?;
    }
    if shown == 0 {
        writeln!(out, "no auctions")?;
    }
    Ok(())
}

pub fn list_at_risk<C: Chain>(
    env: &Env<C>,
    args: &ScanArgs,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    let v = env.verified()?;
    let cursor = scan_now(env, &v, args)?;
    let now = env.now()?;
    kv(out, "scanned to block", &cursor.last_block.to_string())?;
    let mut shown = 0usize;
    for id in cursor.position_ids()? {
        let h = match quote::position_health(&env.chain, &v, &id, now) {
            Ok(h) => h,
            Err(e) => {
                skip_stale(&id, e, err)?;
                continue;
            }
        };
        if h.unsafe_at_min_cr && h.normalized_debt > 0 {
            shown += 1;
            writeln!(
                out,
                "{}  owner {}  collateral {}  debt {}  unsafe at min CR",
                h.position, h.owner, h.collateral, h.nominal_debt
            )?;
        }
    }
    if shown == 0 {
        writeln!(out, "no position is unsafe")?;
    }
    Ok(())
}

/// A corrupt cursor is repaired by `rescan --from 1`, so that one invocation starts from an empty
/// cursor instead of failing on the file it is about to replace.
fn cursor_for_rescan(
    path: &std::path::Path,
    from: Option<u64>,
    err: &mut dyn Write,
) -> Result<Cursor, CliError> {
    match scan::load_cursor(path) {
        Ok(c) => Ok(c),
        Err(_) if from.is_some_and(|f| f <= 1) => {
            writeln!(err, "cursor file unreadable; rebuilding from block 1")?;
            Ok(Cursor::default())
        }
        Err(e) => Err(e.into()),
    }
}

/// The cursor positioned so that the next scan starts at block `from`.
fn cursor_at<C: Chain>(env: &Env<C>, cursor: Cursor, from: u64) -> Result<Cursor, CliError> {
    let start = from.saturating_sub(1);
    let last_hash = if start == 0 {
        String::new()
    } else {
        let blocks = env.chain.blocks(start, start)?;
        let block = blocks
            .first()
            .ok_or_else(|| CliError::Usage(format!("block {start} not found")))?;
        hex::encode(block.hash)
    };
    Ok(Cursor {
        last_block: start,
        last_hash,
        ..cursor
    })
}

pub fn rescan<C: Chain>(
    env: &Env<C>,
    from: Option<u64>,
    args: &ScanArgs,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    let v = env.verified()?;
    let path = env.cursor_path(&v.config_id);
    let mut cursor = cursor_for_rescan(&path, from, err)?;
    if let Some(from) = from {
        let covered = cursor.last_block;
        if from > covered.saturating_add(1) {
            writeln!(err, "note: blocks {}..{} skipped", covered + 1, from - 1)?;
        }
        cursor = cursor_at(env, cursor, from)?;
    }
    let next = scan::scan(
        &env.chain,
        &ScanTargets::of(&v),
        &cursor,
        &scan_settings(args),
    )?;
    scan::save_cursor(&path, &next)?;
    kv(out, "scanned to block", &next.last_block.to_string())?;
    kv(out, "auctions", &next.auctions.len().to_string())?;
    kv(out, "positions", &next.positions.len().to_string())
}

pub fn watch<C: Chain>(
    env: &Env<C>,
    interval_ms: u64,
    iterations: Option<u32>,
    observe_with: Option<&str>,
    args: &ScanArgs,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    let mut round = 0u32;
    loop {
        if let Some(caller) = observe_with {
            crate::tx_cmds::observe_feed(env, caller, out)?;
        }
        list_auctions(env, false, args, out, err)?;
        round = round.saturating_add(1);
        if iterations.is_some_and(|n| round >= n) {
            return Ok(());
        }
        (env.sleep)(Duration::from_millis(interval_ms));
    }
}
