//! The system instructions end to end on a live LEZ v0.4.0-rc1 sequencer, through the SDK (Plan 6
//! Task 9): `Init` with the protocol token enabled (genesis to the taker) → `SetCollateral` → a
//! lot-3 position liquidated and settled empty (a shortfall `U`) → `StartDebt` (the buffer
//! holding still absent: the SDK attaches its `Pda` evidence) → `TakeDebt` of the whole lot →
//! `U − 500` stablecoin donated to the buffer → `SettleDebt u128::MAX` → 1_500 donated →
//! `StartSurplus` → `TakeSurplus` of the whole lot → `CheckSolvency` (predicted `Healthy` by the
//! engine's own computation before the send).
//!
//! Prints each transaction's hash, simulated cycles, declared `gas_limit` and the fee charged,
//! and asserts the end state: `unbacked_debt` 0, both auctions `Ended`, the host not frozen, and
//! the protocol supply equal to genesis + minted − burned (all of it in the taker's holding).
//!
//! Driven by `v04/scripts/e2e.sh` (stage `system`, after `slice`), with the environment of
//! `examples/slice.rs`: `SEQUENCER_URL`, the five program accounts, `PAYER`, optional `RUN_TAG`
//! and `SLACK_MS`. Its keys derive from `RUN_TAG` + `/system`, so it never collides with the
//! slice's config on the same sequencer. The session, signers and token/oracle/host fixtures are
//! `sdk::session`'s, shared with `examples/slice.rs`.

use auction_engine_params::args::{CollateralArgs, EngineParamsArgs};
use engine_core::{
    SystemCtx,
    msg::ProtocolGenesis,
    solvency::SolvencyDecision,
    state::{AccountingState, ConfigState},
    system::{check_solvency_op, ref_price},
    system_state::{DebtAuctionState, SurplusAuctionState, SystemPhase},
};
use lee::{AccountId, Actor};
use mock_host_core::Params;
use sdk::{
    Deployment, InitArgs, LiquidateArgs, Snapshot, StateSource, TakeArgs,
    session::{Live, Programs, Res, Setup, Signer, clock, plan, print_summary},
};
use sequencer_service_rpc::RpcClient as _;
use token_core::{Definition, Holding, TokenMsg, holding_actor};

/// The oracle price, the mock host's redemption price: the lot-3 position is unsafe at it and
/// the feed is in band.
const PRICE_RAY: u128 = sdk::session::REDEMPTION_PRICE_RAY;
const SALT: u64 = 1;
/// Plan 5's lot-3 `SettleEmpty` case (`World::book_shortfall`): the keeper reward takes the whole
/// 3-unit seizure, so the empty lot books the debt as a shortfall.
const SHORTFALL_COLLATERAL: u128 = 3;
const SHORTFALL_DEBT: u128 = 10_000;
/// The protocol token's genesis supply, credited to the taker at `Init`.
const GENESIS: u128 = 10_000;
/// The second donation: `surplus_buffer_target` 1_000 + `surplus_lot` 500 of the test defaults.
const SURPLUS_DONATION: u128 = 1_500;
/// The stablecoin the donor mints: `U − debt_lot` for the settle, then [`SURPLUS_DONATION`].
const DONOR_FUNDS: u128 = 20_000;

/// What the system plans read: the config, the accounting and the buffer's stablecoin holding
/// (absent until the first credit).
async fn system_source(live: &Live, d: &Deployment) -> Res<Snapshot> {
    let buffer = d.holding_of(d.buffer().account_id, d.stable_def);
    live.snapshot(&[
        d.config.account_id,
        d.accounting.account_id,
        buffer.account_id,
    ])
    .await
}

/// The decision `CheckSolvency` takes on `source` at `now_ms`, by the engine's own pure
/// computation (`engine_core::system::check_solvency_op`, which the guest runs). The config has
/// no reference oracle, so the reference price is the admin's.
fn predicted_solvency(source: &Snapshot, d: &Deployment, now_ms: u64) -> Res<SolvencyDecision> {
    let config = source
        .actor_state(d.config)
        .and_then(|bytes| ConfigState::decode(&bytes))
        .ok_or("no config state")?;
    let acc = source
        .actor_state(d.accounting)
        .and_then(|bytes| AccountingState::decode(&bytes))
        .ok_or("no accounting state")?;
    let buffer = match source.actor_state(d.holding_of(d.buffer().account_id, d.stable_def)) {
        None => 0,
        Some(bytes) => Holding::decode(&bytes).ok_or("not a holding")?.balance,
    };
    let ctx = SystemCtx::from_config(d.config.account_id, &config);
    if ctx.ref_oracle.is_some() {
        return Err("the scenario's config has no reference oracle".into());
    }
    let reference = ref_price(&ctx, None, now_ms);
    let (_, _, decision) = check_solvency_op(&acc, &ctx, buffer, reference, now_ms)
        .map_err(|err| format!("CheckSolvency would revert: {err:?}"))?;
    Ok(decision)
}

/// `donor` transfers `amount` of stablecoin to the buffer's holding (credits are permissionless;
/// a fresh holding self-admits through the token's `HOLDING` seed).
async fn donate(
    live: &mut Live,
    s: &Setup,
    d: &Deployment,
    donor: &Signer,
    amount: u128,
) -> Res<()> {
    let t = s.p.token;
    let buffer = d.buffer().account_id;
    let from = Actor::new(donor.account, t);
    let actors = vec![
        from,
        holding_actor(t, donor.account, s.stable_def),
        d.holding_of(buffer, s.stable_def),
    ];
    let transfer = TokenMsg::Transfer {
        definition: s.stable_def,
        to: buffer,
        amount,
    };
    let pl = plan(from, actors, vec![donor.account], vec![], &transfer)?;
    live.run(&format!("token: donate {amount}"), &pl).await
}

/// `Init` with the protocol token enabled and [`GENESIS`] to `taker`, `SetCollateral` and the
/// host bound to the config actor.
async fn engine_up(live: &mut Live, s: &Setup, taker: AccountId, slack_ms: u64) -> Res<Deployment> {
    let admin = live.signer("admin");
    let args = InitArgs {
        salt: SALT,
        params: EngineParamsArgs {
            protocol_token_enabled: true,
            ..EngineParamsArgs::defaults_for_tests()
        },
        host: s.params,
        token: s.p.token,
        stable_def: s.stable_def,
        oracle: s.oracle,
        clock: clock(),
        slack_ms,
        ref_oracle: None,
        protocol_genesis: Some(ProtocolGenesis {
            to: taker,
            supply: GENESIS,
        }),
    };
    let init = sdk::plan_init(s.p.engine, admin.account, args)?;
    live.run("engine: Init", &init).await?;

    let config = engine_core::actors::config_actor(s.p.engine, admin.account, SALT);
    let snapshot = live.snapshot(&[config.account_id]).await?;
    let d = Deployment::load(&snapshot, s.p.engine, config.account_id)?;
    let now = live.now_ms().await?;
    let collateral = CollateralArgs::defaults_for_tests();
    let set = sdk::plan_set_collateral(&d, s.collateral_def, collateral, now)?;
    live.run("engine: SetCollateral", &set).await?;
    s.bind(live, d.config).await?;
    Ok(d)
}

async fn accounting(live: &Live, d: &Deployment) -> Res<AccountingState> {
    live.state(d.accounting, "accounting", AccountingState::decode)
        .await
}

/// The lot-3 position liquidated and settled empty; returns the shortfall `U` booked.
async fn book_shortfall(live: &mut Live, s: &Setup, d: &Deployment) -> Res<u128> {
    let owner = live.signer("owner");
    let position = s
        .open_position(live, &owner, SHORTFALL_COLLATERAL, SHORTFALL_DEBT)
        .await?;
    let keeper = live.signer("keeper");
    let args = LiquidateArgs {
        keeper: keeper.account,
        collateral_def: s.collateral_def,
        position,
        nonce: 0,
        now_ms: live.now_ms().await?,
    };
    let liq = sdk::plan_liquidate(d, &args, &s.seized_actors(owner.account))?;
    live.run("engine: Liquidate (lot 3)", &liq).await?;
    let settle = sdk::plan_settle_empty(d, s.collateral_def, position, 0)?;
    live.run("engine: SettleEmpty", &settle).await?;
    let unbacked = accounting(live, d).await?.unbacked_debt;
    println!("shortfall booked: unbacked_debt {unbacked}");
    Ok(unbacked)
}

/// `StartDebt 0` on the absent buffer holding, then `taker` takes the whole lot.
async fn debt_auction(live: &mut Live, d: &Deployment, taker: AccountId) -> Res<()> {
    let source = system_source(live, d).await?;
    if sdk::buffer_evidence(&source, d).len() != 1 {
        return Err("the buffer holding exists before any credit".into());
    }
    println!("buffer holding absent: StartDebt carries its Pda evidence");
    let start = sdk::plan_start_debt(&source, d, 0, live.now_ms().await?)?;
    live.run("engine: StartDebt", &start).await?;
    let auction = live
        .state(d.debt(0), "debt auction", DebtAuctionState::decode)
        .await?;
    let take = TakeArgs {
        taker,
        nonce: 0,
        amount_coin: auction.lot_left,
        limit: auction.start_amount,
        now_ms: live.now_ms().await?,
    };
    live.run("engine: TakeDebt (whole)", &sdk::plan_take_debt(d, take)?)
        .await
}

/// `StartSurplus 0`, then `taker` takes the whole lot, paying at most its protocol balance.
async fn surplus_auction(live: &mut Live, d: &Deployment, taker: AccountId) -> Res<()> {
    let source = system_source(live, d).await?;
    let start = sdk::plan_start_surplus(&source, d, 0, live.now_ms().await?)?;
    live.run("engine: StartSurplus", &start).await?;
    let auction = live
        .state(d.surplus(0), "surplus auction", SurplusAuctionState::decode)
        .await?;
    let take = TakeArgs {
        taker,
        nonce: 0,
        amount_coin: auction.lot_left,
        limit: live.balance(d.holding_of(taker, d.protocol_def)).await?,
        now_ms: live.now_ms().await?,
    };
    live.run(
        "engine: TakeSurplus (whole)",
        &sdk::plan_take_surplus(d, take)?,
    )
    .await
}

/// The whole system scenario; asserts the end state.
async fn system(live: &mut Live, s: &Setup, d: &Deployment, taker: &Signer) -> Res<()> {
    let shortfall = book_shortfall(live, s, d).await?;
    let lot = EngineParamsArgs::defaults_for_tests().debt_lot;
    let settle_donation = shortfall
        .checked_sub(lot)
        .filter(|amount| *amount > 0 && amount.saturating_add(SURPLUS_DONATION) <= DONOR_FUNDS)
        .ok_or_else(|| format!("unexpected shortfall {shortfall}"))?;
    s.open_position(live, taker, lot, lot).await?;
    debt_auction(live, d, taker.account).await?;

    let donor = live.signer("donor");
    s.open_position(live, &donor, DONOR_FUNDS, DONOR_FUNDS)
        .await?;
    donate(live, s, d, &donor, settle_donation).await?;
    let source = system_source(live, d).await?;
    let settle = sdk::plan_settle_debt(&source, d, u128::MAX)?;
    live.run("engine: SettleDebt (max)", &settle).await?;
    let settled = accounting(live, d).await?;
    if settled.unbacked_debt != 0 {
        return Err(format!("unbacked {} after the settle", settled.unbacked_debt).into());
    }
    donate(live, s, d, &donor, SURPLUS_DONATION).await?;
    surplus_auction(live, d, taker.account).await?;

    let source = system_source(live, d).await?;
    let now_ms = live.now_ms().await?;
    let decision = predicted_solvency(&source, d, now_ms)?;
    if decision != SolvencyDecision::Healthy {
        return Err(format!("CheckSolvency would decide {decision:?}, expected Healthy").into());
    }
    println!("CheckSolvency predicted Healthy");
    let check = sdk::plan_check_solvency(&source, d, now_ms)?;
    live.run("engine: CheckSolvency", &check).await?;
    check_end_state(live, s, d, taker.account).await
}

/// `unbacked_debt` 0 (so `CheckSolvency` could only have been `Healthy`: unbacked at most the
/// threshold is `auction_core::check_solvency`'s first branch), both auctions `Ended`, the host not
/// frozen, and the protocol supply equal to genesis + minted − burned, all held by the taker.
async fn check_end_state(live: &Live, s: &Setup, d: &Deployment, taker: AccountId) -> Res<()> {
    let acc = accounting(live, d).await?;
    let debt = live
        .state(d.debt(0), "debt auction", DebtAuctionState::decode)
        .await?;
    let surplus = live
        .state(d.surplus(0), "surplus auction", SurplusAuctionState::decode)
        .await?;
    let host = live.state(s.params, "host params", Params::decode).await?;
    let definition = Actor::new(d.protocol_def, d.token);
    let supply = live
        .state(definition, "protocol definition", Definition::decode)
        .await?
        .supply;
    let held = live.balance(d.holding_of(taker, d.protocol_def)).await?;
    let expected = GENESIS
        .checked_add(acc.total_protocol_minted)
        .and_then(|sum| sum.checked_sub(acc.total_protocol_burned))
        .ok_or("protocol totals overflow")?;
    println!(
        "end: unbacked {}, debt auction {:?}, surplus auction {:?}, host frozen {}, protocol supply {supply} = genesis {GENESIS} + minted {} - burned {}, taker holds {held}",
        acc.unbacked_debt,
        debt.phase,
        surplus.phase,
        host.is_frozen,
        acc.total_protocol_minted,
        acc.total_protocol_burned,
    );
    let ok = acc.unbacked_debt == 0
        && debt.phase == SystemPhase::Ended
        && surplus.phase == SystemPhase::Ended
        && !host.is_frozen
        && !acc.host_frozen_by_engine
        && acc.total_protocol_minted > 0
        && acc.total_protocol_burned > 0
        && supply == expected
        && held == supply;
    if !ok {
        return Err("the end state does not match the scenario".into());
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Res<()> {
    let programs = Programs::from_env()?;
    let slack_ms = sdk::session::slack_ms_from_env()?;
    let run_tag = std::env::var("RUN_TAG").unwrap_or_else(|_| "e2e".into());
    let mut live = Live::connect(format!("{run_tag}/system"))?;
    println!(
        "payer {} balance {}, head clock {} ms, slack {slack_ms} ms",
        live.payer,
        live.client.get_account_balance(live.payer).await?,
        live.now_ms().await?
    );
    let setup = Setup::deploy(&mut live, programs, ("oracle: price 0.5", PRICE_RAY)).await?;
    let taker = live.signer("taker");
    let outcome = match engine_up(&mut live, &setup, taker.account, slack_ms).await {
        Ok(d) => system(&mut live, &setup, &d, &taker).await,
        Err(err) => Err(err),
    };
    print_summary(&live.rows);
    outcome?;
    println!("system complete");
    Ok(())
}
