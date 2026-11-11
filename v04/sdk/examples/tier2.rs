//! The Tier 2 risk features end to end on a live LEZ v0.4.0-rc1 sequencer, through the SDK (Plan 9
//! Task 12), with the time-compressed values of Plan 9 ruling R9-13:
//!
//! 1. `Init` (legacy engine params), then `SetCollateral` with the R9-13 `RiskArgs` (window
//!    300_000 ms, spacing 10_000 ms, `min_obs` 3, breaker 2_000 bps, stale window 600_000 ms) and
//!    collateral (`max_oracle_age_ms` 180_000, `max_conf_bps` 100, `band_hi_bps` 15_000).
//! 2. A large unsafe position P1 and a tiny one P2 (the lot-3 case), and a funded bidder.
//! 3. `Liquidate` P1 is refused during the TWAP warm-up (6070, `TwapUnavailable`): simulated, not
//!    sent. A fresh price is published first, so the refusal is the warm-up and not a stale feed.
//! 4. Three prices at least one spacing apart, each with `confidence = price / 200` (50 bps) and
//!    each followed by `ObserveFeed`, so the ring holds three samples.
//! 5. `Liquidate` P1 and P2 right after the third `ObserveFeed`.
//! 6. `SetPauseLevel 1`, then a partial bid on P1 fills (level 1 stops liquidations, not bids).
//! 7. No more publishing until the feed is stale (`now + slack > ts + 180_000`); a final bid on P1
//!    fills as a D4 fallback bid at the last-good TWAP and settles the auction. R9-3 makes D4
//!    reachable: 300_000 > 180_000 + (3 - 2) · 10_000, and 600_000 > 180_000.
//! 8. `SetPauseLevel 3`: `ClaimLeftover` on P1 and `SettleEmpty` on P2 still run.
//!
//! Prints each transaction's hash, simulated cycles, `gas_limit` and fee, and `tier2 complete` on
//! success; any failed check exits non-zero. Driven by `v04/scripts/e2e.sh` (stage `tier2`, after
//! `system`), with the environment of `examples/slice.rs`. Its keys derive from `RUN_TAG` +
//! `/tier2`, so it never collides with the other stages' configs on the same sequencer.

use std::time::{Duration, Instant};

use auction_engine_params::args::{CollateralArgs, EngineParamsArgs};
use engine_core::{
    check::fail_text,
    state::{AuctionPhase, AuctionState, CollateralState},
};
use lee::AccountId;
use mock_host_core::is_unsafe;
use oracle_core::OracleState;
use risk_core::RiskError;
use sdk::{
    BidArgs, Deployment, InitArgs, LiquidateArgs, Plan, RiskArgs, SdkError,
    live::Wallet,
    session::{
        Live, MIN_CR_RAY, Programs, REDEMPTION_PRICE_RAY, Res, Setup, Signer, clock, print_summary,
    },
};
use sequencer_service_rpc::RpcClient as _;

/// Every publication's price: the redemption price, inside the band and the breaker.
const PRICE_RAY: u128 = REDEMPTION_PRICE_RAY;
/// `confidence = price / 200`: 50 bps, inside `max_conf_bps` 100.
const CONFIDENCE: u128 = PRICE_RAY / 200;
const SALT: u64 = 1;
/// The R9-13 feed max age, ms.
const MAX_AGE_MS: u64 = 180_000;
/// The R9-13 sample spacing, ms.
const SPACING_MS: u64 = 10_000;
/// P1: the scenario of `harness/tests/cycles.rs`, collateral 500_000, debt 600_000.
const COLLATERAL: u128 = 500_000;
const DEBT: u128 = 600_000;
/// P1's target: the debt plus the 10 % penalty.
const TARGET: u128 = 660_000;
const PENALTY: u128 = TARGET - DEBT;
/// The level 1 partial bid's coin in.
const PARTIAL: u128 = 100_000;
/// P2: Plan 5's lot-3 case, where the keeper reward takes the whole seizure.
const SMALL_COLLATERAL: u128 = 3;
const SMALL_DEBT: u128 = 10_000;
/// The bidder's stablecoin.
const FUNDS: u128 = 1_000_000;
/// How often a wait reads the head clock.
const POLL: Duration = Duration::from_secs(5);
/// The longest a wait for the head clock may take.
const WAIT_LIMIT: Duration = Duration::from_secs(900);

/// The R9-13 `RiskArgs`.
const fn risk_args() -> RiskArgs {
    RiskArgs {
        twap_window_ms: 300_000,
        twap_min_spacing_ms: SPACING_MS,
        twap_min_obs: 3,
        max_spot_twap_dev_bps: 2_000,
        stale_bid_max_age_ms: 600_000,
    }
}

/// The R9-13 collateral parameters: the test defaults with a 180 s feed max age, a 100 bps
/// confidence bound and a 150 % upper band.
fn collateral_args() -> CollateralArgs {
    CollateralArgs {
        max_oracle_age_ms: MAX_AGE_MS,
        max_conf_bps: 100,
        band_hi_bps: 15_000,
        ..CollateralArgs::defaults_for_tests()
    }
}

/// The live session with its fixtures, the deployment and a wallet to simulate refusals with.
struct Run {
    live: Live,
    s: Setup,
    d: Deployment,
    wallet: Wallet,
    slack_ms: u64,
}

/// A position and the signer that owns it.
struct Pos {
    owner: Signer,
    position: AccountId,
}

/// `Init` with the legacy engine params, `SetCollateral` with the R9-13 values and the host
/// bound to the config actor.
async fn engine_up(live: &mut Live, s: &Setup, slack_ms: u64) -> Res<Deployment> {
    let admin = live.signer("admin");
    let args = InitArgs {
        salt: SALT,
        params: EngineParamsArgs::defaults_for_tests(),
        host: s.params,
        token: s.p.token,
        stable_def: s.stable_def,
        oracle: s.oracle,
        clock: clock(),
        slack_ms,
        ref_oracle: None,
        protocol_genesis: None,
    };
    let init = sdk::plan_init(s.p.engine, admin.account, args)?;
    live.run("engine: Init", &init).await?;

    let config = engine_core::actors::config_actor(s.p.engine, admin.account, SALT);
    let snapshot = live.snapshot(&[config.account_id]).await?;
    let d = Deployment::load(&snapshot, s.p.engine, config.account_id)?;
    let now = live.now_ms().await?;
    let set =
        sdk::plan_set_collateral_with(&d, s.collateral_def, collateral_args(), risk_args(), now)?;
    live.run("engine: SetCollateral (R9-13)", &set).await?;
    s.bind(live, d.config).await?;
    Ok(d)
}

async fn auction(run: &Run, p: &Pos) -> Res<AuctionState> {
    let actor = run.d.auction(p.position, 0);
    run.live.state(actor, "auction", AuctionState::decode).await
}

async fn collateral(run: &Run) -> Res<CollateralState> {
    let actor = run.d.collateral(run.s.collateral_def);
    run.live
        .state(actor, "collateral", CollateralState::decode)
        .await
}

/// The oracle's newest publish time, ms.
async fn oracle_ts(run: &Run) -> Res<u64> {
    let oracle = run.live.state(run.s.oracle, "oracle", OracleState::decode);
    Ok(oracle.await?.timestamp_ms)
}

/// Waits until the head clock reaches `at_ms`; returns it.
async fn wait_for_head(run: &Run, at_ms: u64, what: &str) -> Res<u64> {
    let started = Instant::now();
    loop {
        let now = run.live.now_ms().await?;
        if now >= at_ms {
            return Ok(now);
        }
        if started.elapsed() > WAIT_LIMIT {
            return Err(format!("{what}: the head clock {now} did not reach {at_ms}").into());
        }
        tokio::time::sleep(POLL).await;
    }
}

/// `label` opens a position unsafe at [`PRICE_RAY`].
async fn open_unsafe(run: &mut Run, label: &str, collateral: u128, debt: u128) -> Res<Pos> {
    if !is_unsafe(collateral, debt, PRICE_RAY, MIN_CR_RAY) {
        return Err(format!("{label}'s position is not unsafe at the price").into());
    }
    let owner = run.live.signer(label);
    let position = run
        .s
        .open_position(&mut run.live, &owner, collateral, debt)
        .await?;
    Ok(Pos { owner, position })
}

/// Step 2: P1 (large) and P2 (lot 3), and the funded bidder.
async fn open_positions(run: &mut Run) -> Res<(Pos, Pos, Signer)> {
    let p1 = open_unsafe(run, "owner-large", COLLATERAL, DEBT).await?;
    let p2 = open_unsafe(run, "owner-small", SMALL_COLLATERAL, SMALL_DEBT).await?;
    let bidder = run.live.signer("bidder");
    run.s
        .open_position(&mut run.live, &bidder, FUNDS, FUNDS)
        .await?;
    Ok((p1, p2, bidder))
}

/// The keeper's `Liquidate` of `p`'s auction 0 at the head clock.
async fn liquidate_plan(run: &mut Run, p: &Pos) -> Res<Plan> {
    let keeper = run.live.signer("keeper");
    let args = LiquidateArgs {
        keeper: keeper.account,
        collateral_def: run.s.collateral_def,
        position: p.position,
        nonce: 0,
        now_ms: run.live.now_ms().await?,
    };
    let host_actors = run.s.seized_actors(p.owner.account);
    Ok(sdk::plan_liquidate(&run.d, &args, &host_actors)?)
}

/// Simulates `plan` (nothing is sent) and requires it to revert with `err`'s text.
async fn expect_refusal(run: &Run, label: &str, plan: &Plan, err: RiskError) -> Res<()> {
    let needle = fail_text(err);
    match sdk::simulate(&run.live.client, &run.wallet, plan, run.live.payer).await {
        Err(SdkError::WouldRevert(reason)) if reason.contains(&needle) => {
            println!("{label}: refused as expected: {needle}");
            Ok(())
        }
        Err(other) => Err(format!("{label}: expected {needle:?}, got {other}").into()),
        Ok(cycles) => {
            Err(format!("{label}: expected {needle:?}, but it simulates ({cycles} cycles)").into())
        }
    }
}

/// Step 3: with a fresh feed and fewer than `min_obs` weighted samples, `Liquidate` P1 is refused
/// with 6070.
async fn warm_up_refused(run: &mut Run, p1: &Pos) -> Res<()> {
    let label = "oracle: fresh price (warm-up check)";
    run.s
        .publish_with_confidence(&mut run.live, label, PRICE_RAY, CONFIDENCE)
        .await?;
    let liq = liquidate_plan(run, p1).await?;
    let label = "engine: Liquidate P1 (warm-up)";
    expect_refusal(run, label, &liq, RiskError::TwapUnavailable).await
}

/// Step 4: three publications at least [`SPACING_MS`] apart, each observed; checks the ring's
/// newest three samples are exactly those publications. Returns the newest publish time.
async fn warm_twap(run: &mut Run) -> Res<u64> {
    let mut published: Vec<u64> = Vec::new();
    for (publish, observe) in [
        ("oracle: sample 1", "engine: ObserveFeed 1"),
        ("oracle: sample 2", "engine: ObserveFeed 2"),
        ("oracle: sample 3", "engine: ObserveFeed 3"),
    ] {
        if let Some(prev) = published.last() {
            let at = prev.checked_add(SPACING_MS).ok_or("time overflows")?;
            wait_for_head(run, at, "the sample spacing").await?;
        }
        run.s
            .publish_with_confidence(&mut run.live, publish, PRICE_RAY, CONFIDENCE)
            .await?;
        published.push(oracle_ts(run).await?);
        let now = run.live.now_ms().await?;
        let plan = sdk::plan_observe_feed(&run.d, run.s.collateral_def, now)?;
        run.live.run(observe, &plan).await?;
    }
    let ring = collateral(run).await?.ext.ring;
    let newest: Vec<u64> = ring.samples.iter().rev().take(3).map(|s| s.ts_ms).collect();
    let expected: Vec<u64> = published.iter().rev().copied().collect();
    println!(
        "ring: {} samples, newest {newest:?}, last confidence {} bps",
        ring.samples.len(),
        ring.last_conf_bps
    );
    if newest != expected {
        return Err(format!("the ring's newest samples {newest:?}, expected {expected:?}").into());
    }
    published
        .last()
        .copied()
        .ok_or_else(|| "nothing was published".into())
}

/// Step 5: both liquidations, then P1 auctioning with a stored stale cap (D4 on) and P2's lot
/// fully taken by the keeper reward.
async fn liquidate_both(run: &mut Run, p1: &Pos, p2: &Pos) -> Res<()> {
    let liq = liquidate_plan(run, p1).await?;
    run.live.run("engine: Liquidate P1", &liq).await?;
    let liq = liquidate_plan(run, p2).await?;
    run.live.run("engine: Liquidate P2 (lot 3)", &liq).await?;
    let a1 = auction(run, p1).await?;
    let a2 = auction(run, p2).await?;
    println!(
        "P1: {:?}, lot {}, stale cap {}; P2: {:?}, lot {}",
        a1.phase, a1.collateral_left, a1.stale_cap_ray, a2.phase, a2.collateral_left
    );
    if a1.phase != AuctionPhase::Auctioning || a1.stale_cap_ray == 0 {
        return Err("P1 is not auctioning with a stale cap".into());
    }
    if a2.collateral_left != 0 {
        return Err("P2's lot is not empty".into());
    }
    Ok(())
}

/// The admin sets the pause level, which must be stored.
async fn set_level(run: &mut Run, level: u8) -> Res<()> {
    let now = run.live.now_ms().await?;
    let plan = sdk::plan_set_pause_level(&run.d, run.s.collateral_def, level, now)?;
    let label = format!("engine: SetPauseLevel {level}");
    run.live.run(&label, &plan).await?;
    let stored = collateral(run).await?.ext.pause_level;
    if stored != level {
        return Err(format!("pause level {stored} stored, expected {level}").into());
    }
    Ok(())
}

/// `bidder` bids up to `max_debt_in` on P1's auction 0 at the head clock.
async fn bid(run: &mut Run, bidder: &Signer, p1: &Pos, max_debt_in: u128, label: &str) -> Res<()> {
    let args = BidArgs {
        bidder: bidder.account,
        collateral_def: run.s.collateral_def,
        position: p1.position,
        nonce: 0,
        max_debt_in,
        min_collateral_out: 0,
        now_ms: run.live.now_ms().await?,
    };
    let plan = sdk::plan_bid(&run.d, &args)?;
    run.live.run(label, &plan).await
}

/// Step 6: at level 1, a partial bid on P1 fills on the live feed.
async fn level_1_partial(run: &mut Run, p1: &Pos, bidder: &Signer, newest: u64) -> Res<()> {
    set_level(run, 1).await?;
    let now = run.live.now_ms().await?;
    let stale_at = newest.checked_add(MAX_AGE_MS).ok_or("time overflows")?;
    if now.saturating_add(run.slack_ms) > stale_at {
        return Err(format!(
            "the feed went stale before the level 1 bid (head {now}, newest publish {newest}): the blocks ran slower than 15 s"
        )
        .into());
    }
    bid(run, bidder, p1, PARTIAL, "engine: Bid P1 (partial, live)").await?;
    let a = auction(run, p1).await?;
    let raised = a.debt_raised.checked_add(a.penalty_raised);
    println!("P1 after the partial bid: {:?}, raised {raised:?}", a.phase);
    if a.phase != AuctionPhase::Auctioning || raised != Some(PARTIAL) {
        return Err("the partial bid did not fill as expected".into());
    }
    Ok(())
}

/// Step 7: waits until the feed is stale at the head (`now + slack > newest + max_age`), then a
/// final bid on P1 fills at the D4 fallback price and settles the auction.
async fn fallback_final(run: &mut Run, p1: &Pos, bidder: &Signer, newest: u64) -> Res<()> {
    let stale_from = newest
        .checked_add(MAX_AGE_MS)
        .and_then(|t| t.checked_add(1))
        .map(|t| t.saturating_sub(run.slack_ms))
        .ok_or("time overflows")?;
    let now = wait_for_head(run, stale_from, "the stale feed").await?;
    println!(
        "feed stale at head {now} (newest publish {newest}, slack {})",
        run.slack_ms
    );
    bid(run, bidder, p1, TARGET, "engine: Bid P1 (final, D4)").await?;
    let a = auction(run, p1).await?;
    let coll = collateral(run).await?;
    println!(
        "P1 after the fallback bid: {:?}, debt {}, penalty {}, leftover {}; pause open since {:?}",
        a.phase, a.debt_raised, a.penalty_raised, a.leftover_claimable, coll.paused_since_ms
    );
    let settled = a.phase == AuctionPhase::Settled
        && a.debt_raised == DEBT
        && a.penalty_raised == PENALTY
        && a.leftover_claimable > 0;
    if !settled || coll.paused_since_ms.is_none() {
        return Err("the fallback bid did not settle P1 on a stale feed".into());
    }
    Ok(())
}

/// Step 8: at level 3, P1's owner claims the leftover and P2 settles empty.
async fn level_3_settles(run: &mut Run, p1: &Pos, p2: &Pos) -> Res<()> {
    set_level(run, 3).await?;
    let cdef = run.s.collateral_def;
    let claimable = auction(run, p1).await?.leftover_claimable;
    let claim = sdk::plan_claim_leftover(&run.d, cdef, p1.owner.account, p1.position, 0)?;
    run.live.run("engine: ClaimLeftover P1", &claim).await?;
    let settle = sdk::plan_settle_empty(&run.d, cdef, p2.position, 0)?;
    run.live.run("engine: SettleEmpty P2", &settle).await?;

    let vault = run.d.vault(p1.position, 0).account_id;
    let vault = run.live.balance(run.d.holding_of(vault, cdef)).await?;
    let owner = run.d.holding_of(p1.owner.account, cdef);
    let owner_got = run.live.balance(owner).await?;
    let claimed = auction(run, p1).await?.leftover_claimed;
    let p2_phase = auction(run, p2).await?.phase;
    println!(
        "level 3: P1 leftover {claimed} of {claimable} claimed, vault {vault}, owner holds {owner_got}; P2 {p2_phase:?}"
    );
    if vault != 0 || claimed != claimable || owner_got != claimable {
        return Err("the leftover claim did not empty the vault to the owner".into());
    }
    if p2_phase != AuctionPhase::Settled {
        return Err("P2 did not settle empty".into());
    }
    Ok(())
}

/// Steps 2 to 8.
async fn scenario(run: &mut Run) -> Res<()> {
    let (p1, p2, bidder) = open_positions(run).await?;
    warm_up_refused(run, &p1).await?;
    let newest = warm_twap(run).await?;
    liquidate_both(run, &p1, &p2).await?;
    level_1_partial(run, &p1, &bidder, newest).await?;
    fallback_final(run, &p1, &bidder, newest).await?;
    level_3_settles(run, &p1, &p2).await
}

#[tokio::main]
async fn main() -> Res<()> {
    risk_args()
        .validate(MAX_AGE_MS)
        .map_err(|err| format!("R9-3 refuses the stage's RiskArgs at {}", err.field))?;
    let programs = Programs::from_env()?;
    let slack_ms = sdk::session::slack_ms_from_env()?;
    let run_tag = std::env::var("RUN_TAG").unwrap_or_else(|_| "e2e".into());
    let mut live = Live::connect(format!("{run_tag}/tier2"))?;
    let wallet = Wallet::from_env_with_client(live.client.clone())?;
    println!(
        "payer {} balance {}, head clock {} ms, slack {slack_ms} ms",
        live.payer,
        live.client.get_account_balance(live.payer).await?,
        live.now_ms().await?
    );
    let s = Setup::deploy(&mut live, programs, ("oracle: price 0.5", PRICE_RAY)).await?;
    let d = match engine_up(&mut live, &s, slack_ms).await {
        Ok(d) => d,
        Err(err) => {
            print_summary(&live.rows);
            return Err(err);
        }
    };
    let mut run = Run {
        live,
        s,
        d,
        wallet,
        slack_ms,
    };
    let outcome = scenario(&mut run).await;
    print_summary(&run.live.rows);
    outcome?;
    println!("tier2 complete");
    Ok(())
}
