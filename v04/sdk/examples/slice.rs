//! The liquidation slice end to end on a live LEZ v0.4.0-rc1 sequencer, through the SDK:
//! `init → set collateral → open an unsafe position (mock host) → liquidate → partial bid →
//! final bid → claim leftover`, printing each transaction's hash, simulated cycles, declared
//! `gas_limit` and the fee charged (the payer's balance before and after), and asserting the end
//! state (auction `Settled`, auction vault empty after the claim).
//!
//! Driven by `v04/scripts/e2e.sh`, which deploys the guests and sets:
//! - `SEQUENCER_URL` (default `http://127.0.0.1:3040`);
//! - `TOKEN_PROGRAM`, `ORACLE_PROGRAM`, `HOST_PROGRAM`, `RELAY_PROGRAM`, `ENGINE_PROGRAM`: the
//!   deployed program accounts (the loader's header accounts);
//! - `PAYER`: the fee payer, whose key the wallet (`LEE_WALLET_HOME_DIR`) holds;
//! - optional `RUN_TAG` (fresh actor keys per tag) and `SLACK_MS` (the config's clock slack).
//!
//! Every other actor (admin, host params, token definitions, owners, keeper, bidder) signs with a
//! key derived from `RUN_TAG` and its label; only the payer pays fees.

//!
//! The session, signers and token/oracle/host fixtures are `sdk::session`'s, shared with
//! `examples/system.rs`.

use auction_engine_params::args::{CollateralArgs, EngineParamsArgs};
use engine_core::{
    actors::record_actor,
    state::{AuctionPhase, AuctionState, LiquidationRecordState},
};
use lee::AccountId;
use mock_host_core::is_unsafe;
use sdk::{
    BidArgs, Deployment, InitArgs, LiquidateArgs,
    session::{Live, MIN_CR_RAY, Programs, RAY, Res, Setup, clock, print_summary},
};
use sequencer_service_rpc::RpcClient as _;

/// The oracle price the run starts at.
const SAFE_PRICE_RAY: u128 = 2 * RAY;
const SALT: u64 = 1;
/// The scenario of `harness/tests/cycles.rs` (CYCLES.md): collateral 500_000, debt 600_000,
/// redemption price and oracle 0.5.
const COLLATERAL: u128 = 500_000;
const DEBT: u128 = 600_000;
const PARTIAL: u128 = 100_000;
const TARGET: u128 = 660_000;
const FUNDS: u128 = 1_000_000;

/// Half the price at which the position would sit exactly at the minimum ratio.
fn unsafe_price(collateral: u128, debt: u128) -> Res<u128> {
    let price = debt
        .checked_mul(MIN_CR_RAY)
        .and_then(|needed| needed.checked_div(collateral))
        .and_then(|at_ratio| at_ratio.checked_div(2))
        .ok_or("unsafe price overflows")?
        .max(1);
    if !is_unsafe(collateral, debt, price, MIN_CR_RAY) {
        return Err("the position is not unsafe at the published price".into());
    }
    Ok(price)
}

/// `Init`, `SetCollateral` and the host bound to the config actor.
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
    let set = sdk::plan_set_collateral(
        &d,
        s.collateral_def,
        CollateralArgs::defaults_for_tests(),
        now,
    )?;
    live.run("engine: SetCollateral", &set).await?;
    s.bind(live, d.config).await?;
    Ok(d)
}

/// The record's auction count for `position`: the nonce the next liquidation must name.
async fn next_nonce(live: &Live, d: &Deployment, position: AccountId) -> Res<u64> {
    let record = record_actor(d.engine, d.config.account_id, position);
    Ok(match live.raw_state(record).await? {
        None => 0,
        Some(bytes) => {
            LiquidationRecordState::decode(&bytes)
                .ok_or("the record does not decode")?
                .count
        }
    })
}

async fn auction_state(
    live: &Live,
    d: &Deployment,
    position: AccountId,
    nonce: u64,
) -> Res<AuctionState> {
    let bytes = live
        .raw_state(d.auction(position, nonce))
        .await?
        .ok_or("no auction state")?;
    Ok(AuctionState::decode(&bytes).ok_or("the auction does not decode")?)
}

/// Liquidate, two bids and the leftover claim; asserts the end state.
async fn auction(live: &mut Live, s: &Setup, d: &Deployment) -> Res<()> {
    let owner = live.signer("owner");
    let position = s.open_position(live, &owner, COLLATERAL, DEBT).await?;
    let unsafe_price = unsafe_price(COLLATERAL, DEBT)?;
    s.publish(live, "oracle: unsafe price", unsafe_price)
        .await?;
    s.publish(live, "oracle: price 0.5", RAY / 2).await?;

    let keeper = live.signer("keeper");
    let nonce = next_nonce(live, d, position).await?;
    let args = LiquidateArgs {
        keeper: keeper.account,
        collateral_def: s.collateral_def,
        position,
        nonce,
        now_ms: live.now_ms().await?,
    };
    let host_actors = s.seized_actors(owner.account);
    let liq = sdk::plan_liquidate(d, &args, &host_actors)?;
    live.run("engine: Liquidate", &liq).await?;

    let bidder = live.signer("bidder");
    s.open_position(live, &bidder, FUNDS, FUNDS).await?;
    for (label, max_debt_in) in [
        ("engine: Bid (partial)", PARTIAL),
        ("engine: Bid (final)", TARGET),
    ] {
        let args = BidArgs {
            bidder: bidder.account,
            collateral_def: s.collateral_def,
            position,
            nonce,
            max_debt_in,
            min_collateral_out: 0,
            now_ms: live.now_ms().await?,
        };
        let bid = sdk::plan_bid(d, &args)?;
        live.run(label, &bid).await?;
    }
    let settled = auction_state(live, d, position, nonce).await?;
    if settled.phase != AuctionPhase::Settled {
        return Err(format!("auction phase {:?}, expected Settled", settled.phase).into());
    }
    if settled.leftover_claimable == 0 {
        return Err("no leftover to claim".into());
    }

    let claim = sdk::plan_claim_leftover(d, s.collateral_def, owner.account, position, nonce)?;
    live.run("engine: ClaimLeftover", &claim).await?;
    let vault = d.vault(position, nonce).account_id;
    let vault = live.balance(d.holding_of(vault, s.collateral_def)).await?;
    let owner_got = live
        .balance(d.holding_of(owner.account, s.collateral_def))
        .await?;
    let claimed = auction_state(live, d, position, nonce).await?;
    println!(
        "auction: Settled, debt_raised {}, penalty_raised {}, collateral_sold {}, leftover {} claimed by the owner; vault {vault}",
        claimed.debt_raised,
        claimed.penalty_raised,
        claimed.collateral_sold,
        claimed.leftover_claimed
    );
    if vault != 0
        || owner_got != settled.leftover_claimable
        || claimed.leftover_claimed != settled.leftover_claimable
    {
        return Err(format!(
            "after the claim: vault {vault}, owner {owner_got}, claimed {} of {}",
            claimed.leftover_claimed, settled.leftover_claimable
        )
        .into());
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Res<()> {
    let programs = Programs::from_env()?;
    let slack_ms = sdk::session::slack_ms_from_env()?;
    let tag = std::env::var("RUN_TAG").unwrap_or_else(|_| "slice".into());
    let mut live = Live::connect(tag)?;
    println!(
        "payer {} balance {}",
        live.payer,
        live.client.get_account_balance(live.payer).await?
    );
    println!(
        "head clock {} ms, slack {slack_ms} ms",
        live.now_ms().await?
    );
    let first_price = ("oracle: safe price", SAFE_PRICE_RAY);
    let setup = Setup::deploy(&mut live, programs, first_price).await?;
    let d = engine_up(&mut live, &setup, slack_ms).await?;
    let outcome = auction(&mut live, &setup, &d).await;
    print_summary(&live.rows);
    outcome?;
    println!("slice complete");
    Ok(())
}
