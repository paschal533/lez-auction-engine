//! `Liquidate` as an actor conversation (design note §4.2 row 4, §4.3 a): the keeper's proxy, the
//! collateral actor's Observe block, the config, the record, the auction, the host's seizure and
//! its `Seized` report, the keeper reward from the auction vault, the reservations and the
//! auction's `Finish` sentinel; `harness::invariants::check` after every step.
//!
//! Numbers: 500_000 collateral and 600_000 debt (v0.2.4 `liquidate_happy_path`'s `LIQ_*`).
//! `open_unsafe_position` publishes `600_000 × 1.5 / 500_000 / 2 = 0.9` RAY; with the host's
//! redemption price 1 RAY the band `[0.9, 1.1]` keeps it, so the bid price is 0.9 RAY. v0.2.4's
//! quote then gives target `ceil(600_000 × 1.1) = 660_000` and reward
//! `floor(min(5 + 600, 50, 60_000) × 0.9) = 45` collateral (v0.2.4's own run had price 0.5 and
//! reward 25; `engine_core::liquidation`'s unit test pins that case on the same code).

use auction_engine_params::{EngineError, args::CollateralArgs};
use engine_core::{
    check::fail_text,
    msg::EngineMsg,
    state::{AuctionPhase, AuctionState, CollateralState, ConfigState, LiquidationRecordState},
};
use harness::{
    World, expect_revert, invariants,
    world::{Forward, SLACK_MS},
};
use lee::{AccountId, Actor};
use mock_host_core::{Params, Position, TAG_PARAMS};
use token_core::TokenMsg;

const COLLATERAL: u128 = 500_000;
const DEBT: u128 = 600_000;
const TARGET: u128 = 660_000;
const REWARD: u128 = 45;
const LOT: u128 = COLLATERAL - REWARD;
/// The chain time the tests open positions at, ms.
const T: u64 = 1_000_000;
/// `CollateralArgs::defaults_for_tests().max_oracle_age_ms`.
const MAX_AGE_MS: u64 = 3_600_000;

/// `World::new()`, time `T`, and one unsafe position (the oracle publishes at `T`).
fn world_with_position() -> (World, AccountId) {
    let mut w = World::new();
    w.set_now_ms(T);
    let position = w.open_unsafe_position(COLLATERAL, DEBT);
    invariants::check(&w);
    (w, position)
}

fn auction_state(w: &World, position: AccountId, nonce: u64) -> Option<AuctionState> {
    w.chain
        .raw_state(w.auction(position, nonce))
        .map(|bytes| AuctionState::decode(&bytes).expect("an auction state"))
}

fn collateral_state(w: &World) -> CollateralState {
    w.chain
        .raw_state(w.collateral)
        .and_then(|bytes| CollateralState::decode(&bytes))
        .expect("a collateral state")
}

fn config_state(w: &World) -> ConfigState {
    w.chain
        .raw_state(w.config)
        .and_then(|bytes| ConfigState::decode(&bytes))
        .expect("a config state")
}

fn record_count(w: &World, position: AccountId) -> Option<u64> {
    w.chain
        .raw_state(w.record(position))
        .and_then(|bytes| LiquidationRecordState::decode(&bytes))
        .map(|record| record.count)
}

fn position_state(w: &World, position: AccountId) -> Position {
    w.chain
        .raw_state(Actor::new(position, w.host_prog))
        .and_then(|bytes| Position::decode(&bytes))
        .expect("a position")
}

/// The auction a liquidation of the world's position at `start_ms` must write, field by field.
fn expected_auction(w: &World, position: AccountId, start_ms: u64) -> AuctionState {
    AuctionState {
        tag: engine_core::state::TAG_AUCTION,
        config: w.config.account_id,
        position,
        nonce: 0,
        host: w.params,
        phase: AuctionPhase::Auctioning,
        owner: w.owner_of(position).account,
        keeper: w.keeper,
        collateral_seized: COLLATERAL,
        collateral_sold: 0,
        collateral_left: LOT,
        keeper_reward: REWARD,
        leftover_claimable: 0,
        leftover_claimed: 0,
        debt_principal: DEBT,
        amount_to_raise: TARGET,
        reserved_amount: TARGET,
        debt_raised: 0,
        penalty_raised: 0,
        start_ms,
        schedule: collateral_state(w).schedule,
        type_paused_total_at_start: 0,
        scratch: None,
    }
}

#[test]
fn liquidate_opens_an_auction_and_pays_the_keeper() {
    let (mut w, position) = world_with_position();
    let host_vault = w.host_vault(position).account_id;
    assert_eq!(w.collateral_balance(host_vault), COLLATERAL);

    w.liquidate(position, 0).expect("the keeper liquidates");
    invariants::check(&w);

    assert_eq!(
        auction_state(&w, position, 0),
        Some(expected_auction(&w, position, T))
    );
    let seized = position_state(&w, position);
    assert_eq!((seized.collateral, seized.normalized_debt), (0, 0));
    assert_eq!(w.collateral_balance(host_vault), 0);
    let vault = w.vault(position, 0).account_id;
    assert_eq!(w.collateral_balance(vault), LOT);
    assert_eq!(w.collateral_balance(w.keeper), REWARD);
    let collateral = collateral_state(&w);
    assert_eq!(collateral.type_reserved, TARGET);
    assert_eq!(
        collateral.last_seen_oracle_ts,
        Some(T),
        "observed this publish"
    );
    assert_eq!(collateral.paused_since_ms, None);
    assert_eq!(config_state(&w).global_reserved, TARGET);
    assert_eq!(record_count(&w, position), Some(1));
}

#[test]
fn liquidate_safe_position_reverts() {
    let (mut w, position) = world_with_position();
    w.set_price(harness::world::SAFE_PRICE_RAY);
    expect_revert(w.liquidate(position, 0), "host: PositionSafe");
    invariants::check(&w);
    assert_eq!(auction_state(&w, position, 0), None);
    assert_eq!(record_count(&w, position), None);
    assert_eq!(position_state(&w, position).collateral, COLLATERAL);
}

#[test]
fn liquidate_twice_reverts() {
    let (mut w, position) = world_with_position();
    w.liquidate(position, 0).expect("the first liquidation");
    invariants::check(&w);
    let first = auction_state(&w, position, 0);

    // The same nonce: the record has moved on (v0.2.4's stale-nonce error).
    expect_revert(
        w.liquidate(position, 0),
        &fail_text(EngineError::NonceMismatch),
    );
    invariants::check(&w);
    // The next nonce: the host finds the seized (empty) position safe.
    expect_revert(w.liquidate(position, 1), "host: PositionSafe");
    invariants::check(&w);
    assert_eq!(auction_state(&w, position, 0), first);
    assert_eq!(auction_state(&w, position, 1), None);
    assert_eq!(record_count(&w, position), Some(1));
    assert_eq!(collateral_state(&w).type_reserved, TARGET);
}

#[test]
fn stale_feed_pauses_and_reverts_liquidate() {
    let (mut w, position) = world_with_position();
    let before = collateral_state(&w);
    // The feed age is judged at now + slack: one ms past the max age.
    let stale_at = T + MAX_AGE_MS - SLACK_MS + 1;
    w.set_now_ms(stale_at);
    expect_revert(
        w.liquidate(position, 0),
        &fail_text(EngineError::PriceFeedStale),
    );
    invariants::check(&w);
    // The revert drops the observation (v0.2.4 ADR-032: policy (b) recovers the onset later).
    assert_eq!(collateral_state(&w), before);
    assert_eq!(auction_state(&w, position, 0), None);
    // Claiming one ms earlier (inside the clock window) puts the feed exactly at its max age.
    w.liquidate_at(position, 0, stale_at - 1)
        .expect("a feed exactly max-age old is live");
    invariants::check(&w);
}

#[test]
fn liquidate_with_now_outside_the_clock_window_reverts() {
    let (mut w, position) = world_with_position();
    for now_ms in [T - SLACK_MS - 1, T + 1] {
        expect_revert(w.liquidate_at(position, 0, now_ms), "Clock timestamp");
        invariants::check(&w);
    }
    assert_eq!(auction_state(&w, position, 0), None);
    // The earliest claim the window admits: the auction starts at the claim.
    w.liquidate_at(position, 0, T - SLACK_MS)
        .expect("a claim slack ms early is admitted");
    invariants::check(&w);
    assert_eq!(
        auction_state(&w, position, 0).map(|a| a.start_ms),
        Some(T - SLACK_MS)
    );
}

#[test]
fn donation_to_the_host_vault_does_not_block_liquidation() {
    let (mut w, position) = world_with_position();
    let donation = 1_000;
    let (whale, host_vault) = (Actor::new(w.whale, w.token), w.host_vault(position));
    let donate = TokenMsg::Transfer {
        definition: w.collateral_def,
        to: host_vault.account_id,
        amount: donation,
    };
    let key = w.whale_key.clone();
    w.chain
        .send(whale, &[whale, host_vault], &[&key], &donate)
        .expect("the whale donates into the host vault");

    w.liquidate(position, 0)
        .expect("the donation does not block");
    invariants::check(&w);
    // The auction books the reported collateral; the surplus stays in the vault, unbooked.
    assert_eq!(
        auction_state(&w, position, 0),
        Some(expected_auction(&w, position, T))
    );
    let vault = w.vault(position, 0).account_id;
    assert_eq!(w.collateral_balance(vault), LOT + donation);
    assert_eq!(w.collateral_balance(host_vault.account_id), 0);
}

#[test]
fn admin_pause_toggle_runs_observe() {
    let (mut w, position) = world_with_position();
    let key = w.admin_key.clone();
    let paused_at = T + 1_000;
    w.set_now_ms(paused_at);
    let pause = w.set_collateral_msg(CollateralArgs {
        admin_paused: true,
        ..World::collateral_args()
    });
    w.send_engine(w.admin, &[&key], &pause)
        .expect("the admin pauses");
    invariants::check(&w);
    let paused = collateral_state(&w);
    assert!(paused.admin_paused);
    assert_eq!(
        (paused.paused_since_ms, paused.paused_total_ms),
        (Some(paused_at), 0)
    );
    expect_revert(w.liquidate(position, 0), &fail_text(EngineError::Paused));
    invariants::check(&w);

    let resumed_at = paused_at + 10_000;
    w.set_now_ms(resumed_at);
    let resume = w.set_collateral_msg(World::collateral_args());
    w.send_engine(w.admin, &[&key], &resume)
        .expect("the admin resumes");
    let resumed = collateral_state(&w);
    assert_eq!(
        (resumed.paused_since_ms, resumed.paused_total_ms),
        (None, 10_000)
    );
    w.liquidate(position, 0).expect("the type is live again");
    invariants::check(&w);
    assert_eq!(
        auction_state(&w, position, 0).map(|a| a.type_paused_total_at_start),
        Some(10_000)
    );
}

#[test]
fn forged_seized_and_finish_are_refused() {
    let (mut w, position) = world_with_position();
    w.liquidate(position, 0).expect("the keeper liquidates");
    let auction = w.auction(position, 0);
    let before = auction_state(&w, position, 0);
    let owner = w.owner_of(position).account;
    let seized = EngineMsg::Seized {
        position,
        owner,
        collateral: 1,
        normalized_debt: 1,
        nominal_debt: 1,
    };
    // A root `Seized` straight at the auction, and one relayed by a foreign actor.
    expect_revert(
        w.chain.send(auction, &[auction], &[], &seized),
        &fail_text(EngineError::HostMismatch),
    );
    let relay = w.publisher;
    expect_revert(
        w.chain.send(
            relay,
            &[relay, auction],
            &[],
            &Forward::new(auction, &seized),
        ),
        &fail_text(EngineError::HostMismatch),
    );
    // `Finish` from anyone but the auction's config.
    expect_revert(
        w.chain.send(
            relay,
            &[relay, auction],
            &[],
            &Forward::new(auction, &EngineMsg::Finish),
        ),
        &fail_text(EngineError::WrongAccount("finish")),
    );
    assert_eq!(auction_state(&w, position, 0), before);
    invariants::check(&w);
}

#[test]
fn liquidate_needs_the_keeper_signature_and_a_configured_collateral() {
    let (mut w, position) = world_with_position();
    let root = Actor::new(w.keeper, w.engine);
    let msg = w.liquidate_msg(position, 0, T);
    let actors = w.proxy_actors(w.keeper, &msg);
    let evidence = w.liquidate_evidence(position, 0);
    // Unsigned at the keeper's proxy (v0.2.4: the keeper must sign).
    expect_revert(
        w.chain
            .send_with_evidence(root, &actors, &[], &evidence, &msg),
        &fail_text(EngineError::WrongAccount("keeper")),
    );
    // A collateral type the config never configured: the engine's own error, not LEZ's
    // admission error, because the proxy grants the COLL seed (ruling R-7a).
    let unconfigured = w.stable_def;
    let other = engine_core::actors::collateral_actor(w.engine, w.config.account_id, unconfigured);
    let msg = EngineMsg::Liquidate {
        config: w.config.account_id,
        collateral_def: unconfigured,
        position,
        nonce: 0,
        now_ms: T,
    };
    let mut actors = actors;
    actors.push(other);
    let key = w.keeper_key.clone();
    expect_revert(
        w.chain
            .send_with_evidence(root, &actors, &[&key], &evidence, &msg),
        &fail_text(EngineError::WrongAccount("coll_config")),
    );
    assert_eq!(w.chain.raw_state(other), None);
    assert_eq!(auction_state(&w, position, 0), None);
    invariants::check(&w);
}

/// Design note §4.3 c, guard 5 (ruling R-8b): the collateral actor's PDA grant must not be
/// inherited into the host's subtree. The engine is bound to a `grant_probe` host stand-in whose
/// `params.admin` is the probe actor at the collateral actor's account; on `LiquidatePosition` the
/// stand-in pings it, and the probe panics `probe: authorized` if that account is authorized
/// there. The stand-in never seizes, so without a leak the conversation's `Finish` sentinel
/// fails with `NotAuctioning` (which also shows a host that never reports cannot commit).
#[test]
fn collateral_grant_does_not_reach_the_host() {
    let mut w = World::bare();
    let probe_prog = w.chain.program("grant_probe");
    let (host_key, host_account) = w.chain.key("probe-host");
    w.chain.fund(host_account, 1);
    let host = Actor::new(host_account, probe_prog);
    let target = Actor::new(w.collateral.account_id, probe_prog);
    let params = Params {
        tag: TAG_PARAMS,
        admin: target,
        token: w.token,
        stable_def: w.stable_def,
        collateral_def: w.collateral_def,
        oracle: w.oracle,
        redemption_price_ray: harness::world::RAY,
        min_cr_ray: harness::world::MIN_CR_RAY,
        engine_actor: Some(w.config),
        pending: None,
    };
    w.chain
        .send(host, &[host], &[&host_key], &params)
        .expect("the stand-in host stores its params");

    let admin_key = w.admin_key.clone();
    let mut init = w.init_msg();
    if let EngineMsg::Init { host: bound, .. } = &mut init {
        *bound = host;
    }
    w.send_engine(w.admin, &[&admin_key], &init)
        .expect("the engine binds the stand-in host");
    let set = w.set_collateral_msg(World::collateral_args());
    let actors: Vec<Actor> = w
        .proxy_actors(w.admin, &set)
        .into_iter()
        .map(|actor| if actor == w.params { host } else { actor })
        .collect();
    let root = Actor::new(w.admin, w.engine);
    w.chain
        .send(root, &actors, &[&admin_key], &set)
        .expect("the collateral observes through the stand-in host");

    let position = AccountId::new([7; 32]);
    let msg = w.liquidate_msg(position, 0, w.now_ms);
    let keeper_root = Actor::new(w.keeper, w.engine);
    let actors = [
        keeper_root,
        w.collateral,
        host,
        w.oracle,
        w.chain.clock(),
        w.config,
        w.record(position),
        w.auction(position, 0),
        target,
    ];
    let keeper_key = w.keeper_key.clone();
    let evidence = w.liquidate_evidence(position, 0);
    expect_revert(
        w.chain
            .send_with_evidence(keeper_root, &actors, &[&keeper_key], &evidence, &msg),
        &fail_text(EngineError::NotAuctioning),
    );
}
