//! The operation x pause-level matrix (spec §3.2.2, ADR-073, Plan 9 Task 10): every operation at
//! every pause level with a live feed, and every operation at level 0 with a stale feed, D4 on
//! (the recommended preset) and off (legacy). Each case is a fresh `harness::levels::fixture`
//! in which the operation's own preconditions hold; `harness::levels::expected` derives the
//! outcome from the rule table `risk_core::pause::allowed`.
//!
//! A case that runs is simulated (`Chain::cycles`), not sent, so one fixture serves exactly one
//! case; a refusal is sent, so the revert's rollback is observed: `World::snapshot_all` is
//! unchanged and `harness::invariants::check` holds (Plan 9 ruling).
//!
//! Every mismatch is collected before the test fails, so one run reports the whole matrix.

use harness::{
    invariants,
    levels::{Fixture, expected, fixture, stale_fixture},
};
use lee::PrivateKey;
use risk_core::{
    RiskArgs,
    pause::{FeedCase, Level, Op},
};

/// 17 operations x 4 levels.
const LEVEL_CASES: usize = 68;
/// 17 operations x {D4 on, D4 off}.
const STALE_CASES: usize = 34;

/// The risk-reducing operations, which must run end to end even at level 3 (spec §3.2.1).
const ALWAYS_RUN: [Op; 7] = [
    Op::SettleEmpty,
    Op::ClaimLeftover,
    Op::SettleDebt,
    Op::CheckSolvency,
    Op::CloseSurplus,
    Op::CloseDebt,
    Op::ObserveFeed,
];

/// (L + 1) % 4 as an exhaustive match (no unchecked arithmetic): Normal -> NoLiquidations ->
/// NoSystemStarts -> Halted -> Normal.
fn next(level: Level) -> Level {
    match level {
        Level::Normal => Level::NoLiquidations,
        Level::NoLiquidations => Level::NoSystemStarts,
        Level::NoSystemStarts => Level::Halted,
        Level::Halted => Level::Normal,
    }
}

/// Runs `f`'s transaction against `want` and returns the mismatch, if any: `None` must simulate
/// successfully; `Some(text)` must revert mentioning `text` and change nothing the world knows
/// of.
fn mismatch(f: &mut Fixture, want: Option<&str>, label: &str) -> Option<String> {
    let keys: Vec<&PrivateKey> = f.signers.iter().collect();
    let Some(text) = want else {
        let (_, outcome) =
            f.world
                .chain
                .cycles_with_evidence(f.root, &f.actors, &keys, &f.evidence, &f.msg);
        return outcome
            .err()
            .map(|err| format!("{label}: expected to run, refused: {err:?}"));
    };
    let before = f.world.snapshot_all();
    let sent = f
        .world
        .chain
        .send_with_evidence(f.root, &f.actors, &keys, &f.evidence, &f.msg);
    let problem = match sent {
        Ok(_) => Some(format!("{label}: expected {text:?}, but it applied")),
        Err(err) => {
            let got = format!("{err} {err:?}");
            (!got.contains(text)).then(|| format!("{label}: expected {text:?}, got {err:?}"))
        }
    };
    if problem.is_none() {
        assert_eq!(f.world.snapshot_all(), before, "{label} moved nothing");
        invariants::check(&f.world);
    }
    problem
}

/// Panics listing every mismatch, unless there are none.
fn report(mismatches: &[String]) {
    assert!(
        mismatches.is_empty(),
        "{} matrix mismatches:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

#[test]
fn every_operation_at_every_pause_level() {
    let risk = RiskArgs::legacy();
    let (mut cases, mut mismatches) = (0_usize, Vec::new());
    for op in Op::ALL {
        for level in Level::ALL {
            let mut f = fixture(op, risk);
            f.world.set_pause_level(level.as_u8()).expect("level set");
            // SetPauseLevel's own row moves L to the next level, so raising and lowering are
            // both covered.
            if op == Op::SetPauseLevel {
                f.msg = f.world.set_pause_level_msg(next(level).as_u8());
            }
            let want = expected(op, level, FeedCase::Live, &risk);
            let label = format!("{op:?} at {level:?}");
            mismatches.extend(mismatch(&mut f, want.as_deref(), &label));
            cases = cases.checked_add(1).expect("few cases");
        }
    }
    assert_eq!(cases, LEVEL_CASES);
    report(&mismatches);
}

/// The matrix exercises the risk-reducing operations end to end at level 3: `expected` gives
/// `None` for each, so each is simulated and must succeed there (Task 8 review L2).
#[test]
fn risk_reducing_operations_run_at_level_3() {
    for op in ALWAYS_RUN {
        for feed in FeedCase::ALL {
            for risk in [RiskArgs::legacy(), RiskArgs::recommended()] {
                assert_eq!(expected(op, Level::Halted, feed, &risk), None, "{op:?}");
            }
        }
    }
}

#[test]
fn every_operation_with_a_stale_feed() {
    let presets = [
        ("Stale-ok", RiskArgs::recommended()),
        ("Stale-no", RiskArgs::legacy()),
    ];
    let (mut cases, mut mismatches) = (0_usize, Vec::new());
    for (name, risk) in presets {
        for op in Op::ALL {
            let mut f = stale_fixture(op, risk);
            let want = expected(op, Level::Normal, FeedCase::Stale, &risk);
            let label = format!("{op:?} {name}");
            mismatches.extend(mismatch(&mut f, want.as_deref(), &label));
            cases = cases.checked_add(1).expect("few cases");
        }
    }
    assert_eq!(cases, STALE_CASES);
    report(&mismatches);
}

/// The stale column as the spec states it: a bid runs with D4 on and is `PriceFeedStale` with it
/// off; a liquidation is `PriceFeedStale` either way; every other operation runs.
#[test]
fn stale_expectations_match_the_spec() {
    let stale = engine_core::check::fail_text(auction_engine_params::EngineError::PriceFeedStale);
    let (on, off) = (RiskArgs::recommended(), RiskArgs::legacy());
    let at = |op, risk: &RiskArgs| expected(op, Level::Normal, FeedCase::Stale, risk);
    assert!(on.d4_on() && !off.d4_on());
    assert_eq!(at(Op::Bid, &on), None);
    assert_eq!(at(Op::Bid, &off), Some(stale.clone()));
    for risk in [on, off] {
        assert_eq!(at(Op::Liquidate, &risk), Some(stale.clone()));
        for op in Op::ALL {
            if op != Op::Bid && op != Op::Liquidate {
                assert_eq!(at(op, &risk), None, "{op:?}");
            }
        }
    }
}
