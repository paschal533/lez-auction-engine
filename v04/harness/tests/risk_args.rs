//! `SetCollateral`'s Tier 2 risk parameters (spec §3.1.6, §3.2.4, ADR-082): the config refuses
//! out-of-bound ones as `InvalidConfig`, and writes valid ones to the collateral actor.

use auction_engine_params::{EngineError, args::CollateralArgs};
use engine_core::check::fail_text;
use harness::{World, expect_revert};
use risk_core::{RiskArgs, TwapRing};

#[test]
fn set_collateral_rejects_out_of_bound_risk_args() {
    let mut w = World::new();
    let before = w.collateral_state();
    let band_lo = CollateralArgs::defaults_for_tests().band_lo_bps;
    let bad_ones = [
        RiskArgs {
            twap_window_ms: 1,
            ..RiskArgs::legacy()
        },
        // A sanity window that excludes the type's own lower band edge.
        RiskArgs {
            sanity_lo_bps: band_lo + 1,
            ..RiskArgs::legacy()
        },
        // A grace period that could never end an outage gap.
        RiskArgs {
            grace_ms: 1,
            grace_min_gap_ms: 0,
            ..RiskArgs::legacy()
        },
    ];
    for bad in bad_ones {
        let set = w.set_collateral_msg_with(CollateralArgs::defaults_for_tests(), bad);
        let key = w.admin_key.clone();
        expect_revert(
            w.send_engine(w.admin, &[&key], &set),
            &fail_text(EngineError::InvalidConfig),
        );
        assert_eq!(w.collateral_state(), before);
    }
}

#[test]
fn set_collateral_writes_the_risk_args() {
    let w = World::new_tier2(RiskArgs::recommended());
    let ext = w.collateral_state().ext;
    assert_eq!(ext.risk, RiskArgs::recommended());
    let risk = ext.risk;
    assert_eq!((risk.sanity_lo_bps, risk.sanity_hi_bps), (5_000, 20_000));
    assert_eq!((risk.feed_min_raw, risk.feed_max_raw), (0, u128::MAX));
    assert_eq!((risk.grace_ms, risk.grace_min_gap_ms), (1_800_000, 600_000));
    assert_eq!(ext.pause_level, 0);
    assert_eq!(ext.ring, TwapRing::default());
}
