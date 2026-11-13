//! The reference-oracle and relist-delay parameters (Plan 10, E9): `ref_price` and
//! `debt_precheck` read the engine's stored `EngineExtArgs` instead of v0.2.4's constants.

use auction_core::price::{MAX_CONF_BPS, REF_ORACLE_CLAMP_BPS};
use auction_engine_params::{
    EngineError,
    args::{DEBT_RELIST_DELAY_MS, MAX_SYSTEM_TTL_MS},
};
use risk_core::{
    EngineExtArgs, LEGACY_SANITY_HI_BPS, LEGACY_SANITY_LO_BPS, REF_CONF_MAX_BPS,
    RELIST_DELAY_MAX_MS,
};

use crate::{
    ext::ConfigExt,
    state::{
        AccountingState, ConfigState, OracleReadingRec,
        fixtures::{actor, config, id},
    },
    system::{debt_precheck, ref_price},
    system_state::{RefReading, SystemCtx},
};

const RAY: u128 = 1_000_000_000_000_000_000_000_000_000;
const NOW: u64 = 10_000_000;

/// Admin price 2 RAY, a reference oracle configured, the engine parameters `ext`.
fn ctx(ext: EngineExtArgs) -> SystemCtx {
    let c = ConfigState {
        protocol_token_ref_price: 2 * RAY,
        protocol_token_oracle: Some(actor(30, 31)),
        ext: ConfigExt::new(ext),
        ..config()
    };
    SystemCtx::from_config(id(40), &c)
}

fn reading(price_raw: u128, confidence_raw: u128) -> Option<RefReading> {
    Some(RefReading::Valid(OracleReadingRec {
        price_raw,
        confidence_raw,
        timestamp_ms: NOW,
    }))
}

#[test]
fn ref_clamp_is_a_parameter() {
    let c = ctx(EngineExtArgs {
        ref_clamp_bps: 1_000,
        ..EngineExtArgs::legacy()
    });
    assert_eq!(
        ref_price(&c, reading(3 * RAY, 0), NOW),
        22 * RAY / 10,
        "clamped to +10 %"
    );
    assert_eq!(
        ref_price(&c, reading(RAY + RAY / 2, 0), NOW),
        18 * RAY / 10,
        "clamped to -10 %"
    );
}

#[test]
fn ref_conf_limit_is_a_parameter() {
    let price = 22 * RAY / 10;
    let five_percent = price / 20;
    let at = |ref_conf_bps| {
        let c = ctx(EngineExtArgs {
            ref_conf_bps,
            ..EngineExtArgs::legacy()
        });
        ref_price(&c, reading(price, five_percent), NOW)
    };
    assert_eq!(at(1_000), price, "5 % confidence is inside a 10 % limit");
    assert_eq!(at(100), 2 * RAY, "and outside a 1 % limit: the admin price");
}

#[test]
fn q64_reference_oracle_reads() {
    let q64 = |ext_scale| {
        ctx(EngineExtArgs {
            ref_scale: ext_scale,
            ..EngineExtArgs::legacy()
        })
    };
    let two = 2u128 << 64;
    assert_eq!(ref_price(&q64(1), reading(two, 0), NOW), 2 * RAY);
    // 2.2 in Q64x64 is read as about 2.2 RAY, which is not the admin price...
    let two_point_two = (11u128 << 64) / 5;
    let read = ref_price(&q64(1), reading(two_point_two, 0), NOW);
    assert!(read > 2 * RAY && read <= 22 * RAY / 10, "{read}");
    // ...while the same raw value at scale 0 is a tiny RAY price, out of the window.
    assert_eq!(ref_price(&q64(0), reading(two_point_two, 0), NOW), 2 * RAY);
}

#[test]
fn ref_sanity_window_is_a_parameter() {
    let at = |ref_sanity_hi_bps| {
        let c = ctx(EngineExtArgs {
            ref_sanity_hi_bps,
            ..EngineExtArgs::legacy()
        });
        ref_price(&c, reading(5 * RAY, 0), NOW)
    };
    assert_eq!(at(20_000), 2 * RAY, "5 RAY is above 2 x 2 RAY: fallback");
    assert_eq!(at(30_000), 24 * RAY / 10, "inside 3 x 2 RAY, then clamped");
}

#[test]
fn debt_relist_delay_is_a_parameter() {
    let delay = 60_000;
    let ended = AccountingState {
        last_debt_end_ms: NOW,
        ..AccountingState::new()
    };
    assert_eq!(
        debt_precheck(&ended, 0, NOW + delay - 1, delay),
        Err(EngineError::DelayNotElapsed)
    );
    assert_eq!(debt_precheck(&ended, 0, NOW + delay, delay), Ok(()));
}

#[test]
fn legacy_preset_equals_the_v024_constants() {
    let l = EngineExtArgs::legacy();
    assert_eq!(l.ref_conf_bps, MAX_CONF_BPS);
    assert_eq!(l.ref_clamp_bps, REF_ORACLE_CLAMP_BPS);
    assert_eq!(l.debt_relist_delay_ms, DEBT_RELIST_DELAY_MS);
    assert_eq!(l.ref_sanity_lo_bps, LEGACY_SANITY_LO_BPS);
    assert_eq!(l.ref_sanity_hi_bps, LEGACY_SANITY_HI_BPS);
    assert_eq!(l.ref_scale, 0);
    assert_eq!(RELIST_DELAY_MAX_MS, MAX_SYSTEM_TTL_MS);
    assert_eq!(REF_CONF_MAX_BPS, MAX_CONF_BPS);
}
