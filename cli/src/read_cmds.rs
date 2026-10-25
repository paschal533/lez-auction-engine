//! Read-only commands: config, quotes, lists, rescan, watch. Every one verifies the config first (I4).

use std::io::Write;
use std::time::Duration;

use auction_sdk::{
    chain::Chain,
    deployment::program_id_hex,
    quote,
    scan::{self, Cursor, Restart, ScanSettings, ScanTargets},
    verify::VerifiedConfig,
    views::{self, AuctionEntry, Listed},
};

use crate::{
    args::ScanArgs,
    env::Env,
    error::CliError,
    output::{bps, kv, yes_no},
    tx_report,
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
    kv(out, "bad-debt freeze", yes_no(s.freeze_on_bad_debt))?;
    system_state(env, &v, out)
}

/// What a user needs to judge a system transaction whose outcome is unknown (take/start/close).
fn system_state<C: Chain>(
    env: &Env<C>,
    v: &VerifiedConfig,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let a = views::accounting(&env.chain, v)?;
    kv(out, "accounting", &tx_report::accounting_line(&a))?;
    if let Some(lot) = a.surplus_lot_left {
        kv(out, "surplus lot left", &lot.to_string())?;
    }
    if let Some(lot) = a.debt_lot_left {
        kv(out, "debt lot left", &lot.to_string())?;
    }
    Ok(())
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

/// One stderr line when a scan discarded the stored cursor (ADR-043, amended).
fn note_restart(restarted: Option<Restart>, err: &mut dyn Write) -> Result<(), CliError> {
    if let Some(r) = restarted {
        writeln!(err, "note: {r}")?;
    }
    Ok(())
}

/// Scans the new blocks and persists the cursor.
pub(crate) fn scan_now<C: Chain>(
    env: &Env<C>,
    v: &VerifiedConfig,
    args: &ScanArgs,
    err: &mut dyn Write,
) -> Result<Cursor, CliError> {
    let path = env.cursor_path(&v.config_id);
    let cursor = scan::load_cursor(&path)?;
    let next = scan::scan_report(
        &env.chain,
        &ScanTargets::of(v),
        &cursor,
        &scan_settings(args),
    )?;
    scan::save_cursor(&path, &next.cursor)?;
    note_restart(next.restarted, err)?;
    Ok(next.cursor)
}

fn auction_line(entry: &AuctionEntry) -> String {
    let l = match entry {
        AuctionEntry::Settled(s) => {
            return format!(
                "{}  settled  sold {}  raised {}/{}  leftover {} (claimed {})",
                s.id, s.sold, s.raised, s.target, s.leftover, s.claimed
            );
        }
        AuctionEntry::Live(l) => l,
    };
    let mut line = format!(
        "{}  live  lot {}  raised {}/{}  discount {}  t_eff {} ms",
        l.id,
        l.collateral_left,
        l.raised,
        l.target,
        bps(l.discount_ray),
        l.t_eff_ms
    );
    if l.held_at_max {
        line.push_str(&format!(
            "  held at max discount (keeps its cap reservation of {})",
            l.reserved
        ));
    }
    if let Some(p) = &l.paused {
        line.push_str(&format!("  bids revert now: {p} (run observe-feed)"));
    }
    line
}

/// Stale scanner candidates were dropped by the view; the ones it skipped get a warning on stderr,
/// so one bad id never hides the others and never hides itself either (ADR-043).
fn warn_skipped<T>(listed: &Listed<T>, err: &mut dyn Write) -> Result<(), CliError> {
    for s in &listed.skipped {
        writeln!(err, "warning: skipped {}: {}", s.id, s.reason)?;
    }
    Ok(())
}

pub fn list_auctions<C: Chain>(
    env: &Env<C>,
    all: bool,
    args: &ScanArgs,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    let v = env.verified()?;
    let cursor = scan_now(env, &v, args, err)?;
    let now = env.now()?;
    kv(out, "scanned to block", &cursor.last_block.to_string())?;
    let listed = views::auctions(&env.chain, &v, &cursor, now)?;
    warn_skipped(&listed, err)?;
    let shown: Vec<&AuctionEntry> = listed
        .items
        .iter()
        .filter(|e| all || matches!(e, AuctionEntry::Live(_)))
        .collect();
    for entry in &shown {
        writeln!(out, "{}", auction_line(entry))?;
    }
    if shown.is_empty() {
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
    let cursor = scan_now(env, &v, args, err)?;
    let now = env.now()?;
    kv(out, "scanned to block", &cursor.last_block.to_string())?;
    // Cursor order, as before the views existed; the app shows `views::at_risk` (lowest CR first).
    let listed = views::positions(&env.chain, &v, &cursor, now)?;
    warn_skipped(&listed, err)?;
    let mut shown = 0usize;
    for p in listed.items.iter().filter(|p| p.liquidatable) {
        shown += 1;
        writeln!(
            out,
            "{}  owner {}  collateral {}  debt {}  unsafe at min CR",
            p.position, p.owner, p.collateral, p.nominal_debt
        )?;
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
        let placed = scan::start_at(&env.chain, &cursor, from)?;
        note_restart(placed.restarted, err)?;
        let covered = if placed.restarted.is_some() {
            0
        } else {
            cursor.last_block
        };
        if from > covered.saturating_add(1) {
            writeln!(err, "note: blocks {}..{} skipped", covered + 1, from - 1)?;
        }
        cursor = placed.cursor;
    }
    let scanned = scan::scan_report(
        &env.chain,
        &ScanTargets::of(&v),
        &cursor,
        &scan_settings(args),
    )?;
    scan::save_cursor(&path, &scanned.cursor)?;
    note_restart(scanned.restarted, err)?;
    let next = scanned.cursor;
    kv(out, "scanned to block", &next.last_block.to_string())?;
    kv(out, "auctions", &next.auctions.len().to_string())?;
    kv(out, "positions", &next.positions.len().to_string())
}

/// Errors that end `watch`: the setup is wrong (unverifiable config, no wallet, bad arguments, a
/// closed output), so every later round would fail the same way. Anything else (a node hiccup, a
/// dropped or in-flight `observe_feed`, a scan gap) concerns one round only.
fn ends_watch(e: &CliError) -> bool {
    matches!(
        e,
        CliError::Verify(_) | CliError::Keystore(_) | CliError::Usage(_) | CliError::Output(_)
    )
}

/// The error a bounded `watch` exits with: the most severe so far (exit 3, dropped, over exit 1),
/// the first of equals (Plan 3a N3). Setup errors (2, 4) never reach here: they end `watch` at once.
pub fn keep_worst(worst: &mut Option<CliError>, e: CliError) {
    if worst
        .as_ref()
        .is_none_or(|prev| e.exit_code() > prev.exit_code())
    {
        *worst = Some(e);
    }
}

/// A step's failure: setup errors are returned (they end `watch`); any other is reported on stderr
/// and kept as the round's error, so the loop goes on.
fn watch_step(
    result: Result<(), CliError>,
    what: &str,
    round: u32,
    err: &mut dyn Write,
    worst_error: &mut Option<CliError>,
) -> Result<(), CliError> {
    match result {
        Ok(()) => Ok(()),
        Err(e) if ends_watch(&e) => Err(e),
        Err(e) => {
            writeln!(err, "watch: round {round}: {what} failed (continuing): {e}")?;
            keep_worst(worst_error, e);
            Ok(())
        }
    }
}

/// Repeats `observe_feed` (optional) and `list-auctions`. A failed step is reported on stderr and
/// the loop goes on (a failed `observe_feed` does not skip the listing); setup errors (`ends_watch`)
/// stop it at once. A run bounded by `--iterations` exits with the most severe step error if any step
/// failed (3 before 1, the first of equals); exit 0 only when every round succeeded (S4).
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
    let mut worst_error = None;
    loop {
        round = round.saturating_add(1);
        if let Some(caller) = observe_with {
            let observed = crate::tx_cmds::observe_feed(env, caller, out);
            watch_step(observed, "observe-feed", round, err, &mut worst_error)?;
        }
        let listed = list_auctions(env, false, args, out, err);
        watch_step(listed, "list-auctions", round, err, &mut worst_error)?;
        if iterations.is_some_and(|n| round >= n) {
            return worst_error.map_or(Ok(()), Err);
        }
        (env.sleep)(Duration::from_millis(interval_ms));
    }
}

#[cfg(test)]
mod watch_tests {
    use super::*;

    #[test]
    fn keep_worst_prefers_dropped_and_keeps_the_first_of_equals() {
        let mut worst = None;
        keep_worst(&mut worst, CliError::Dropped { height: 7 });
        keep_worst(&mut worst, CliError::Unknown("later".into()));
        assert!(matches!(worst, Some(CliError::Dropped { height: 7 })));
        let mut worst = Some(CliError::Unknown("first".into()));
        keep_worst(&mut worst, CliError::Usage("second".into()));
        assert!(matches!(&worst, Some(CliError::Unknown(m)) if m == "first"));
        keep_worst(&mut worst, CliError::Dropped { height: 9 });
        assert!(matches!(worst, Some(CliError::Dropped { height: 9 })));
    }
}
