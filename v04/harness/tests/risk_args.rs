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
    let bad = RiskArgs {
        twap_window_ms: 1,
        ..RiskArgs::legacy()
    };
    let set = w.set_collateral_msg_with(CollateralArgs::defaults_for_tests(), bad);
    let key = w.admin_key.clone();
    expect_revert(
        w.send_engine(w.admin, &[&key], &set),
        &fail_text(EngineError::InvalidConfig),
    );
    assert_eq!(w.collateral_state(), before);
}

#[test]
fn set_collateral_writes_the_risk_args() {
    let w = World::new_tier2(RiskArgs::recommended());
    let ext = w.collateral_state().ext;
    assert_eq!(ext.risk, RiskArgs::recommended());
    assert_eq!(ext.pause_level, 0);
    assert_eq!(ext.ring, TwapRing::default());
}
