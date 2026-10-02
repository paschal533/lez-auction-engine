//! Spike S5: an adversary controls oracle publish times and observation times.
//! Ground truth: an auction's discount should advance only while the feed is fresh (age ≤ max_age).
use auction_core::pause::{Observation, OnsetPolicy, PauseClock};
use proptest::prelude::*;

const MAX_AGE: u64 = 100;
const BLOCK: u64 = 10; // one block of tolerance

/// Admin-paused intervals [a, b), sorted and non-overlapping.
type Admin = Vec<(u64, u64)>;

fn admin_at(admin: &[(u64, u64)], t: u64) -> bool {
    admin.iter().any(|(a, b)| *a <= t && t < *b)
}

/// Ground truth: ms in [start, end) where the feed is fresh (age <= max_age) and the admin has not paused.
fn fresh_time(publishes: &[u64], admin: &[(u64, u64)], start: u64, end: u64) -> u64 {
    (start..end)
        .filter(|t| {
            !admin_at(admin, *t)
                && publishes
                    .iter()
                    .rev()
                    .find(|p| **p <= *t)
                    .is_some_and(|p| t - p <= MAX_AGE)
        })
        .count() as u64
}

fn run(
    policy: OnsetPolicy,
    publishes: &[u64],
    admin: &[(u64, u64)],
    observes: &[u64],
    end: u64,
) -> (u64, u64) {
    let start = publishes[0];
    let mut c = PauseClock::default().observed(
        Observation {
            now_ms: start,
            feed_valid: true,
            admin_paused: false,
            oracle_ts: start,
            max_age_ms: MAX_AGE,
        },
        policy,
    );
    let total_at_start = c.paused_total_ms;
    for &t in observes.iter().filter(|t| **t > start && **t <= end) {
        let last = *publishes.iter().rev().find(|p| **p <= t).unwrap();
        c = c.observed(
            Observation {
                now_ms: t,
                feed_valid: t - last <= MAX_AGE,
                admin_paused: admin_at(admin, t),
                oracle_ts: last,
                max_age_ms: MAX_AGE,
            },
            policy,
        );
    }
    (
        c.effective_elapsed(start, total_at_start, end),
        fresh_time(publishes, admin, start, end),
    )
}

type Scenario = (Vec<u64>, Admin, Vec<u64>, u64);

fn scenario() -> impl Strategy<Value = Scenario> {
    (
        prop::collection::vec(1u64..400, 1..12),
        prop::collection::vec(1u64..2_000, 0..40),
        prop::collection::vec(1u64..2_000, 0..6),
        100u64..2_000,
    )
        .prop_map(|(gaps, mut obs, mut toggles, end)| {
            let mut t = 0;
            let mut pubs = vec![0];
            for g in gaps {
                t += g;
                pubs.push(t);
            }
            obs.sort_unstable();
            // Admin toggles inside (0, end), paired into sorted non-overlapping [a, b) intervals.
            toggles.retain(|x| *x < end);
            toggles.sort_unstable();
            toggles.dedup();
            if toggles.len() % 2 == 1 {
                toggles.push(end);
            }
            let admin = toggles.chunks(2).map(|c| (c[0], c[1])).collect();
            (pubs, admin, obs, end)
        })
}

proptest! {
    /// Policy (b): the adversary never gains more than one block of discount time over the fresh,
    /// non-admin-paused time, provided observations happen at every publish and every admin toggle
    /// (the engine observes on liquidate/bid; keepers on refresh; the admin tx carries the clock).
    #[test]
    fn derived_policy_bounded_when_publishes_observed((pubs, admin, mut obs, end) in scenario()) {
        // t_eff is always evaluated right after an observation at the same instant (liquidate/bid observe first)
        obs.extend(pubs.iter().copied());
        obs.extend(admin.iter().flat_map(|(a, b)| [*a, *b]));
        obs.push(end);
        obs.sort_unstable();
        let (t_eff, truth) = run(OnsetPolicy::DerivedFromOracle, &pubs, &admin, &obs, end);
        prop_assert!(t_eff <= truth + BLOCK, "t_eff={t_eff} truth={truth}");
    }
}

/// Policy (a) documents its residual: without observations, the stale time counts as elapsed.
#[test]
fn at_observation_policy_overcounts_unobserved_staleness() {
    let pubs = [0u64, 1_000]; // stale from 100 to 1000
    let (t_eff, fresh) = run(OnsetPolicy::AtObservation, &pubs, &[], &[1_000], 1_100);
    assert!(
        t_eff > fresh + BLOCK,
        "policy (a) residual: t_eff={t_eff} fresh={fresh}"
    );
}
