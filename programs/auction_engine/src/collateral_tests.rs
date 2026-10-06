//! Handler-level tests of `set_collateral_config`, `observe_feed` and `feed`: every guard is exercised
//! without the guest, so each can be mutation-checked in seconds.

use auction_core::{error::PauseReason, pause::PauseClock};
use auction_engine_core::{
    args::CollateralArgs,
    pda::{coll_config_pda, config_pda},
    state::*,
    EngineError,
};
use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::AccountPostState,
};
use stablecoin_core::{
    compute_protocol_parameters_pda, compute_redemption_price_state_pda, RedemptionPriceState,
};
use twap_oracle_core::OraclePriceAccount;

use crate::{
    admin::{set_collateral_config, SetCollateralConfigAccounts},
    admin_tests::{
        coll_state, config_state, fresh, host_params, meta, rejected, ADMIN, COLLATERAL, ENGINE,
        HOST, ONE, ORACLE, OTHER, SALT, STABLE,
    },
    feed,
    observe::{observe_feed, ObserveFeedAccounts},
    EngineResult,
};

pub(crate) const NOW: u64 = 1_700_000_000_000;
pub(crate) const R: u128 = ONE / 2;
pub(crate) const MAX_AGE: u64 = 3_600_000;

pub(crate) fn cfg_id() -> AccountId {
    config_pda(&ENGINE, &ADMIN, SALT)
}

pub(crate) fn clock_at(now: u64) -> AccountWithMetadata {
    let data = ClockAccountData {
        block_id: 1,
        timestamp: now,
    }
    .to_bytes();
    meta(
        CLOCK_01_PROGRAM_ACCOUNT_ID,
        Account {
            program_owner: [8; 8],
            data: Data::try_from(data).expect("clock data fits"),
            ..Account::default()
        },
        false,
    )
}

pub(crate) fn rps_with(price: u128, rate: u128, last_updated_at: u64) -> AccountWithMetadata {
    let state = RedemptionPriceState {
        redemption_price_at_last_update: price,
        redemption_rate_per_millisecond: rate,
        controller_integral_term: 0,
        last_updated_at,
    };
    meta(
        compute_redemption_price_state_pda(HOST),
        Account {
            program_owner: HOST,
            data: Data::from(&state),
            ..Account::default()
        },
        false,
    )
}

pub(crate) fn rps() -> AccountWithMetadata {
    rps_with(R, ONE, NOW)
}

pub(crate) fn oracle_with(
    id: AccountId,
    price: u128,
    timestamp: u64,
    confidence: u128,
) -> AccountWithMetadata {
    let state = OraclePriceAccount {
        base_asset: STABLE,
        quote_asset: COLLATERAL,
        price,
        timestamp,
        source_id: id,
        confidence_interval: confidence,
    };
    meta(
        id,
        Account {
            program_owner: [9; 8],
            data: Data::from(&state),
            ..Account::default()
        },
        false,
    )
}

pub(crate) fn oracle_fresh() -> AccountWithMetadata {
    oracle_with(ORACLE, R, NOW, 0)
}

pub(crate) fn engine_owned(data: Data) -> Account {
    Account {
        program_owner: ENGINE,
        data,
        ..Account::default()
    }
}

pub(crate) fn config_account() -> AccountWithMetadata {
    meta(cfg_id(), engine_owned((&config_state()).into()), false)
}

fn coll_account(existing: Option<&CollateralConfigState>) -> AccountWithMetadata {
    let account = existing.map_or_else(Account::default, |c| engine_owned(c.into()));
    meta(
        coll_config_pda(&ENGINE, &cfg_id(), &COLLATERAL),
        account,
        false,
    )
}

fn set_accounts(existing: Option<&CollateralConfigState>) -> SetCollateralConfigAccounts {
    SetCollateralConfigAccounts {
        admin: meta(ADMIN, Account::default(), true),
        config: config_account(),
        collateral_definition: meta(
            COLLATERAL,
            Account {
                program_owner: [5; 8],
                ..Account::default()
            },
            false,
        ),
        coll_config: coll_account(existing),
        oracle: oracle_fresh(),
        protocol_parameters: meta(
            compute_protocol_parameters_pda(HOST),
            Account {
                program_owner: HOST,
                data: (&host_params()).into(),
                ..Account::default()
            },
            false,
        ),
        redemption_price_state: rps(),
        clock: clock_at(NOW),
    }
}

fn args() -> CollateralArgs {
    CollateralArgs::defaults_for_tests()
}

fn set(a: SetCollateralConfigAccounts, c: &CollateralArgs) -> EngineResult {
    set_collateral_config(ENGINE, a, c)
}

fn decode(post: &AccountPostState) -> CollateralConfigState {
    CollateralConfigState::try_from(&post.account().data).expect("coll_config decodes")
}

fn set_ok(a: SetCollateralConfigAccounts, c: &CollateralArgs) -> Vec<AccountPostState> {
    set(a, c).expect("set_collateral_config succeeds").0
}

/// Index of `coll_config` in the `set_collateral_config` account list.
const SET_COLL: usize = 3;
/// Index of `coll_config` in the `observe_feed` account list.
const OBS_COLL: usize = 2;

fn set_with_feed(
    existing: Option<&CollateralConfigState>,
    oracle: AccountWithMetadata,
    rps: AccountWithMetadata,
) -> CollateralConfigState {
    let mut a = set_accounts(existing);
    a.oracle = oracle;
    a.redemption_price_state = rps;
    decode(&set_ok(a, &args())[SET_COLL])
}

#[test]
fn creates_the_collateral_config_and_claims_its_pda() {
    let post = set_ok(set_accounts(None), &args());
    assert_eq!(post.len(), 8);
    assert!(post[SET_COLL].required_claim().is_some());
    let c = decode(&post[SET_COLL]);
    assert_eq!(c.tag, TAG_COLL);
    assert_eq!(c.config, cfg_id());
    assert_eq!(c.collateral_definition_id, COLLATERAL);
    assert_eq!(c.oracle_id, ORACLE);
    assert_eq!(c.oracle_scale, 0);
    assert_eq!(c.max_oracle_age_ms, MAX_AGE);
    assert_eq!(
        (c.max_conf_bps, c.band_lo_bps, c.band_hi_bps, c.penalty_bps),
        (100, 9_000, 11_000, 1_000)
    );
    assert_eq!((c.tip, c.chip_bps, c.reward_cap), (5, 10, 50));
    assert_eq!((c.debt_floor, c.min_bid, c.type_cap), (100, 25, 5_000_000));
    assert_eq!((c.type_reserved, c.admin_paused), (0, false));
    assert_eq!(c.schedule.len(), 2);
    assert_eq!(
        (c.paused_since_ms, c.paused_total_ms, c.last_seen_oracle_ts),
        (None, 0, Some(NOW))
    );
}

#[test]
fn update_keeps_reserved_capacity_and_the_clock_history() {
    let existing = CollateralConfigState {
        type_reserved: 7,
        paused_total_ms: 99,
        last_seen_oracle_ts: Some(NOW - 10),
        ..coll_state(cfg_id(), 5_000_000)
    };
    let tighter = CollateralArgs {
        penalty_bps: 1_200,
        ..args()
    };
    let post = set_ok(set_accounts(Some(&existing)), &tighter);
    assert!(post[SET_COLL].required_claim().is_none());
    let c = decode(&post[SET_COLL]);
    assert_eq!(c.penalty_bps, 1_200);
    assert_eq!(c.type_reserved, 7);
    assert_eq!(c.paused_total_ms, 99);
    assert_eq!(c.last_seen_oracle_ts, Some(NOW));
}

#[test]
fn only_the_signing_admin_may_configure() {
    let mut a = set_accounts(None);
    a.admin = meta(OTHER, Account::default(), true);
    assert_eq!(rejected(set(a, &args())), EngineError::NotAdmin);
    let mut a = set_accounts(None);
    a.admin.is_authorized = false;
    assert_eq!(rejected(set(a, &args())), EngineError::NotAdmin);
}

#[test]
fn rejects_an_invalid_parameter_set() {
    let too_big = CollateralArgs {
        type_cap: 10_000_001,
        ..args()
    };
    assert_eq!(
        rejected(set(set_accounts(None), &too_big)),
        EngineError::TypeCapAboveGlobal
    );
    let at_global = CollateralArgs {
        type_cap: 10_000_000,
        ..args()
    };
    assert!(set(set_accounts(None), &at_global).is_ok());
    let bad_scale = CollateralArgs {
        oracle_scale: 2,
        ..args()
    };
    assert_eq!(
        rejected(set(set_accounts(None), &bad_scale)),
        EngineError::InvalidConfig
    );
}

#[test]
fn rejects_a_collateral_definition_that_is_not_the_configured_one() {
    let mut a = set_accounts(None);
    a.collateral_definition.account_id = OTHER;
    assert_eq!(
        rejected(set(a, &args())),
        EngineError::WrongAccount("collateral_definition")
    );
}

#[test]
fn rejects_a_coll_config_at_another_address() {
    let mut a = set_accounts(None);
    a.coll_config.account_id = OTHER;
    assert_eq!(
        rejected(set(a, &args())),
        EngineError::WrongAccount("coll_config")
    );
}

#[test]
fn rejects_an_existing_coll_config_with_a_wrong_owner_tag_or_config() {
    let existing = coll_state(cfg_id(), 5_000_000);
    let mut a = set_accounts(Some(&existing));
    a.coll_config.account.program_owner = HOST;
    assert_eq!(
        rejected(set(a, &args())),
        EngineError::WrongAccount("coll_config")
    );
    let mut a = set_accounts(Some(&existing));
    a.coll_config.account.data = (&config_state()).into();
    assert_eq!(
        rejected(set(a, &args())),
        EngineError::WrongAccount("coll_config")
    );
    let foreign = CollateralConfigState {
        config: OTHER,
        ..existing
    };
    assert_eq!(
        rejected(set(set_accounts(Some(&foreign)), &args())),
        EngineError::WrongAccount("coll_config")
    );
}

#[test]
fn rejects_protocol_parameters_that_are_not_the_hosts() {
    let mut a = set_accounts(None);
    a.protocol_parameters.account_id = OTHER;
    assert_eq!(
        rejected(set(a, &args())),
        EngineError::WrongAccount("protocol_parameters")
    );
    let mut a = set_accounts(None);
    a.protocol_parameters.account.program_owner = TOKEN_LIKE;
    assert_eq!(rejected(set(a, &args())), EngineError::HostMismatch);
    let mut a = set_accounts(None);
    a.protocol_parameters.account.data = Data::try_from(vec![1u8, 2, 3]).expect("fits");
    assert_eq!(rejected(set(a, &args())), EngineError::HostMismatch);
}

const TOKEN_LIKE: [u32; 8] = [0xB1; 8];

#[test]
fn the_oracle_must_be_the_hosts_market_oracle() {
    let mut a = set_accounts(None);
    a.oracle = oracle_with(OTHER, R, NOW, 0);
    assert_eq!(rejected(set(a, &args())), EngineError::OracleMismatch);
}

#[test]
fn rejects_a_clock_that_is_not_clock_01() {
    let mut a = set_accounts(None);
    a.clock.account_id = OTHER;
    assert_eq!(rejected(set(a, &args())), EngineError::WrongClock);
}

#[test]
fn admin_pause_starts_and_ends_exactly_at_its_transaction() {
    let paused = CollateralArgs {
        admin_paused: true,
        ..args()
    };
    let c = decode(&set_ok(set_accounts(None), &paused)[SET_COLL]);
    assert!(c.admin_paused);
    assert_eq!((c.paused_since_ms, c.paused_total_ms), (Some(NOW), 0));

    let later = NOW + 10_000;
    let mut a = set_accounts(Some(&c));
    a.clock = clock_at(later);
    a.oracle = oracle_with(ORACLE, R, later, 0);
    a.redemption_price_state = rps_with(R, ONE, later);
    let c = decode(&set_ok(a, &args())[SET_COLL]);
    assert!(!c.admin_paused);
    assert_eq!((c.paused_since_ms, c.paused_total_ms), (None, 10_000));
}

#[test]
fn a_stale_feed_pauses_from_the_derived_onset() {
    let ts = NOW - MAX_AGE - 1_000;
    let c = set_with_feed(None, oracle_with(ORACLE, R, ts, 0), rps());
    assert_eq!(c.paused_since_ms, Some(ts + MAX_AGE));
}

#[test]
fn a_feed_exactly_at_max_age_is_valid() {
    let c = set_with_feed(None, oracle_with(ORACLE, R, NOW - MAX_AGE, 0), rps());
    assert_eq!(c.paused_since_ms, None);
}

#[test]
fn low_confidence_pauses_from_now_and_exactly_the_limit_is_valid() {
    let c = set_with_feed(None, oracle_with(ORACLE, R, NOW, R / 50), rps());
    assert_eq!(c.paused_since_ms, Some(NOW));
    let c = set_with_feed(None, oracle_with(ORACLE, R, NOW, R / 100), rps());
    assert_eq!(c.paused_since_ms, None);
}

#[test]
fn a_price_outside_half_to_double_the_redemption_price_pauses() {
    let c = set_with_feed(None, oracle_with(ORACLE, 2 * R + 1, NOW, 0), rps());
    assert_eq!(c.paused_since_ms, Some(NOW));
    let c = set_with_feed(None, oracle_with(ORACLE, 2 * R, NOW, 0), rps());
    assert_eq!(c.paused_since_ms, None);
    let c = set_with_feed(None, oracle_with(ORACLE, R / 2 - 1, NOW, 0), rps());
    assert_eq!(c.paused_since_ms, Some(NOW));
    let c = set_with_feed(None, oracle_with(ORACLE, R / 2, NOW, 0), rps());
    assert_eq!(c.paused_since_ms, None);
}

#[test]
fn validity_uses_the_redemption_price_projected_to_now() {
    // 1e-6 per ms for 1e6 ms is a factor of about e, so R(now) is about 1.36e27 and the oracle price
    // R = 0.5e27 is below R(now)/2: out of range now, though valid against the stale anchor.
    let drifting = rps_with(R, ONE + ONE / 1_000_000, NOW - 1_000_000);
    let c = set_with_feed(None, oracle_fresh(), drifting);
    assert_eq!(c.paused_since_ms, Some(NOW));
}

fn observe_accounts(existing: &CollateralConfigState) -> ObserveFeedAccounts {
    ObserveFeedAccounts {
        caller: meta(OTHER, Account::default(), true),
        config: config_account(),
        coll_config: coll_account(Some(existing)),
        redemption_price_state: rps(),
        oracle: oracle_fresh(),
        clock: clock_at(NOW),
    }
}

fn observe_ok(a: ObserveFeedAccounts) -> Vec<AccountPostState> {
    let (post, calls) = observe_feed(ENGINE, a).expect("observe_feed succeeds");
    assert!(calls.is_empty());
    post
}

fn existing_coll() -> CollateralConfigState {
    CollateralConfigState {
        last_seen_oracle_ts: Some(NOW),
        ..coll_state(cfg_id(), 5_000_000)
    }
}

#[test]
fn observe_feed_never_fails_on_a_stale_feed_and_persists_the_onset() {
    let existing = existing_coll();
    let mut a = observe_accounts(&existing);
    a.clock = clock_at(NOW + MAX_AGE + 5_000);
    a.redemption_price_state = rps_with(R, ONE, NOW + MAX_AGE + 5_000);
    let post = observe_ok(a);
    assert_eq!(post.len(), 6);
    let c = decode(&post[OBS_COLL]);
    assert_eq!(c.paused_since_ms, Some(NOW + MAX_AGE));
    // Only the pause clock moves.
    assert_eq!(
        c,
        existing.with_pause_clock(PauseClock {
            paused_since_ms: Some(NOW + MAX_AGE),
            paused_total_ms: 0,
            last_seen_oracle_ts: Some(NOW),
        })
    );
}

#[test]
fn observe_feed_closes_the_pause_on_recovery() {
    let paused = CollateralConfigState {
        paused_since_ms: Some(NOW - 5_000),
        ..existing_coll()
    };
    let c = decode(&observe_ok(observe_accounts(&paused))[OBS_COLL]);
    assert_eq!((c.paused_since_ms, c.paused_total_ms), (None, 5_000));
}

#[test]
fn observe_feed_keeps_an_admin_pause_open() {
    let admin_paused = CollateralConfigState {
        admin_paused: true,
        paused_since_ms: Some(NOW - 5_000),
        ..existing_coll()
    };
    let c = decode(&observe_ok(observe_accounts(&admin_paused))[OBS_COLL]);
    assert_eq!(c, admin_paused);
}

#[test]
fn observe_feed_echoes_every_other_account() {
    let existing = existing_coll();
    let a = observe_accounts(&existing);
    let before: Vec<Account> = [
        &a.caller,
        &a.config,
        &a.redemption_price_state,
        &a.oracle,
        &a.clock,
    ]
    .iter()
    .map(|m| m.account.clone())
    .collect();
    let post = observe_ok(a);
    let after: Vec<Account> = [0, 1, 3, 4, 5]
        .iter()
        .map(|i| post[*i].account().clone())
        .collect();
    assert_eq!(before, after);
}

#[test]
fn observe_feed_requires_a_signing_caller() {
    let mut a = observe_accounts(&existing_coll());
    a.caller.is_authorized = false;
    assert_eq!(
        rejected(observe_feed(ENGINE, a)),
        EngineError::WrongAccount("caller")
    );
}

#[test]
fn observe_feed_rejects_a_wrong_oracle() {
    let mut a = observe_accounts(&existing_coll());
    a.oracle = oracle_with(OTHER, R, NOW, 0);
    assert_eq!(
        rejected(observe_feed(ENGINE, a)),
        EngineError::OracleMismatch
    );
}

#[test]
fn observe_feed_rejects_a_wrong_clock() {
    let mut a = observe_accounts(&existing_coll());
    a.clock.account_id = OTHER;
    assert_eq!(rejected(observe_feed(ENGINE, a)), EngineError::WrongClock);
}

#[test]
fn observe_feed_rejects_a_foreign_config_or_coll_config() {
    let mut a = observe_accounts(&existing_coll());
    a.config.account.program_owner = HOST;
    assert_eq!(
        rejected(observe_feed(ENGINE, a)),
        EngineError::WrongAccount("config")
    );
    let mut a = observe_accounts(&existing_coll());
    a.coll_config.account_id = OTHER;
    assert_eq!(
        rejected(observe_feed(ENGINE, a)),
        EngineError::WrongAccount("coll_config")
    );
    let mut a = observe_accounts(&existing_coll());
    a.coll_config = fresh(coll_config_pda(&ENGINE, &cfg_id(), &COLLATERAL));
    assert_eq!(
        rejected(observe_feed(ENGINE, a)),
        EngineError::WrongAccount("coll_config")
    );
}

#[test]
fn observe_feed_rejects_a_foreign_redemption_price_state() {
    let mut a = observe_accounts(&existing_coll());
    a.redemption_price_state.account_id = OTHER;
    assert_eq!(
        rejected(observe_feed(ENGINE, a)),
        EngineError::WrongAccount("redemption_price_state")
    );
    let mut a = observe_accounts(&existing_coll());
    a.redemption_price_state.account.program_owner = TOKEN_LIKE;
    assert_eq!(
        rejected(observe_feed(ENGINE, a)),
        EngineError::WrongAccount("redemption_price_state")
    );
    let mut a = observe_accounts(&existing_coll());
    a.redemption_price_state.account.data = Data::try_from(vec![1u8]).expect("fits");
    assert_eq!(
        rejected(observe_feed(ENGINE, a)),
        EngineError::WrongAccount("redemption_price_state")
    );
}

// ---- feed.rs ----

#[test]
fn u512_to_u128_accepts_exactly_the_u128_range() {
    use alloy_primitives::U512;
    assert_eq!(feed::u512_to_u128(U512::from(u128::MAX)), Ok(u128::MAX));
    assert_eq!(
        feed::u512_to_u128(U512::from(u128::MAX) + U512::from(1u8)),
        Err(EngineError::Math)
    );
}

#[test]
fn redemption_price_is_the_hosts_projection_at_now() {
    let cfg = config_state();
    assert_eq!(feed::redemption_price(&cfg, &rps(), NOW), Ok(R));
    let drifting = rps_with(R, ONE + ONE / 1_000_000, NOW);
    let later = feed::redemption_price(&cfg, &drifting, NOW + 1_000).expect("projects");
    assert!(later > R);
}

#[test]
fn require_live_reports_the_pause_reason() {
    let live = existing_coll();
    let ok = feed::Observed {
        clock: live.pause_clock(),
        feed: Ok(42),
    };
    assert_eq!(feed::require_live(&live, &ok), Ok(42));
    for (reason, error) in [
        (PauseReason::Stale, EngineError::PriceFeedStale),
        (
            PauseReason::LowConfidence,
            EngineError::PriceFeedLowConfidence,
        ),
        (PauseReason::OutOfRange, EngineError::PriceFeedOutOfRange),
        (PauseReason::ZeroPrice, EngineError::PriceFeedOutOfRange),
    ] {
        let bad = feed::Observed {
            clock: live.pause_clock(),
            feed: Err(reason),
        };
        assert_eq!(feed::require_live(&live, &bad), Err(error));
    }
    let admin_paused = CollateralConfigState {
        admin_paused: true,
        ..live
    };
    assert_eq!(
        feed::require_live(&admin_paused, &ok),
        Err(EngineError::Paused)
    );
}

#[test]
fn observe_rejects_an_undecodable_oracle_and_a_bad_scale() {
    let coll = existing_coll();
    let mut garbage = oracle_fresh();
    garbage.account.data = Data::try_from(vec![1u8, 2]).expect("fits");
    assert_eq!(
        feed::observe(&coll, &garbage, R, NOW).err(),
        Some(EngineError::WrongAccount("oracle"))
    );
    let bad_scale = CollateralConfigState {
        oracle_scale: 2,
        ..coll
    };
    assert_eq!(
        feed::observe(&bad_scale, &oracle_fresh(), R, NOW).err(),
        Some(EngineError::InvalidConfig)
    );
}
