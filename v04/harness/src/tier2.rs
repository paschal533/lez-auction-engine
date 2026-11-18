//! The Tier 2 test world (Plan 9 ruling R9-14): [`World::new_tier2`] configures the collateral
//! with [`tier2_collateral_args`] and the given `RiskArgs`, so the recommended preset passes
//! ruling R9-3 and the TWAP anchor and the stale floor can bind inside the band.

use auction_engine_params::args::CollateralArgs;
use engine_core::{msg::EngineMsg, state::CollateralState};
use oracle_core::{OracleMsg, OracleState};
use risk_core::RiskArgs;

use crate::world::{Forward, MIN_CR_RAY, RAY, SALT, World};

/// The Tier 2 world's feed max age, ms. R9-3 needs it below the recommended preset's
/// `stale_bid_max_age_ms` (3_600_000) and window less one spacing (1_800_000 - 300_000).
pub const TIER2_MAX_AGE_MS: u64 = 600_000;

/// The Tier 2 world's collateral parameters: the test defaults with a [`TIER2_MAX_AGE_MS`] max
/// age and a 150 % upper band.
#[must_use]
pub fn tier2_collateral_args() -> CollateralArgs {
    CollateralArgs {
        max_oracle_age_ms: TIER2_MAX_AGE_MS,
        band_hi_bps: 15_000,
        ..CollateralArgs::defaults_for_tests()
    }
}

/// The step [`World::advance_observed`] observes at, ms: below the Tier 2 max age, so no two
/// valid observations are a grace-opening gap apart (ADR-075).
pub const OBSERVE_STEP_MS: u64 = 300_000;

/// The recommended preset with grace off, for Tier 2 fixtures that are not about grace. These
/// worlds jump time without observing (setup at t = 0, then straight to their start time), and by
/// spec §3.3 such a gap opens a grace period on the next valid observation, which would refuse
/// their `Liquidate` with `GracePeriod`. Grace tests use [`World::new_tier2_live`] instead.
#[must_use]
pub fn recommended_no_grace() -> RiskArgs {
    RiskArgs {
        grace_ms: 0,
        grace_min_gap_ms: 0,
        ..RiskArgs::recommended()
    }
}

impl World {
    /// [`World::new_tier2`] whose feed is valid from its first observation: the oracle publishes
    /// `RAY / 2` (inside the sanity window) at t = 0 before `SetCollateral` observes it. With
    /// [`World::advance_observed`] it reaches any time without an outage, so grace stays closed.
    #[must_use]
    pub fn new_tier2_live(risk: RiskArgs) -> World {
        let mut w = World::bare_with(RAY / 2, |_| {});
        w.set_price(RAY / 2);
        let key = w.admin_key.clone();
        let init = w.init_msg();
        w.send_engine(w.admin, &[&key], &init)
            .expect("the engine is initialised");
        let set = w.set_collateral_msg_with(tier2_collateral_args(), risk);
        w.send_engine(w.admin, &[&key], &set)
            .expect("the collateral is configured");
        w.bind_host().expect("the host binds the config actor");
        w
    }

    /// [`World::new_tier2_system`] whose feed is valid from its first observation, as
    /// [`World::new_tier2_live`].
    #[must_use]
    pub fn new_tier2_system_live(risk: RiskArgs) -> World {
        let mut w = World::bare_with(RAY / 2, |_| {});
        w.set_price(RAY / 2);
        w.init_system_with(World::engine_params(), None, tier2_collateral_args(), risk);
        w
    }

    /// Advances the world's time to `to_ms` in steps of at most [`OBSERVE_STEP_MS`], publishing
    /// `price_ray` and sending `ObserveFeed` at each, so the feed never has an outage.
    ///
    /// # Panics
    /// If an `ObserveFeed` reverts or the time overflows.
    pub fn advance_observed(&mut self, to_ms: u64, price_ray: u128) {
        while self.now_ms < to_ms {
            let step = self.now_ms.checked_add(OBSERVE_STEP_MS).expect("time fits");
            self.set_now_ms(step.min(to_ms));
            self.set_price(price_ray);
            self.observe_feed(self.now_ms)
                .expect("the feed is observed");
        }
    }

    /// [`World::new`] at redemption price `RAY / 2` with [`tier2_collateral_args`] and `risk`.
    #[must_use]
    pub fn new_tier2(risk: RiskArgs) -> World {
        World::new_tier2_at(RAY / 2, MIN_CR_RAY, risk)
    }

    /// [`World::new_tier2`] at redemption price `redemption_price_ray`, with the host's minimum
    /// collateral ratio `min_cr_ray`.
    #[must_use]
    pub fn new_tier2_at(redemption_price_ray: u128, min_cr_ray: u128, risk: RiskArgs) -> World {
        World::new_with(
            redemption_price_ray,
            |host| host.min_cr_ray = min_cr_ray,
            tier2_collateral_args(),
            risk,
        )
    }

    /// [`World::new_tier2`] with the protocol token on and its genesis credited to the
    /// [`crate::system::TAKER`] signer ([`World::init_system_with`]), so the surplus and debt
    /// auctions can run too. The collateral arguments, `risk` and the redemption price are
    /// [`World::new_tier2`]'s.
    #[must_use]
    pub fn new_tier2_system(risk: RiskArgs) -> World {
        let mut w = World::bare_with(RAY / 2, |_| {});
        w.init_system_with(World::engine_params(), None, tier2_collateral_args(), risk);
        w
    }

    /// `SetCollateral` of the world's collateral definition for salt [`SALT`] with `args` and
    /// `risk`, claiming the world's current time.
    #[must_use]
    pub fn set_collateral_msg_with(&self, args: CollateralArgs, risk: RiskArgs) -> EngineMsg {
        EngineMsg::SetCollateral {
            salt: SALT,
            collateral_def: self.collateral_def,
            args,
            now_ms: self.now_ms,
            risk: Box::new(risk),
        }
    }

    /// The publisher publishes `price_ray` with `confidence` (raw, the oracle's scale) at the
    /// world's current time.
    ///
    /// # Panics
    /// If the publish fails.
    pub fn set_price_with_confidence(&mut self, price_ray: u128, confidence: u128) {
        let (oracle, publisher) = (self.oracle, self.publisher);
        let publish = OracleMsg::Publish {
            price_ray,
            timestamp_ms: self.now_ms,
            confidence,
        };
        self.chain
            .send(
                publisher,
                &[publisher, oracle],
                &[],
                &Forward::new(oracle, &publish),
            )
            .expect("the publisher publishes");
    }

    /// `samples` times: advances the world's time by `spacing_ms`, publishes `price_ray` (zero
    /// confidence) and sends the permissionless `ObserveFeed`, so each valid publish is offered
    /// to the TWAP ring.
    ///
    /// # Panics
    /// If the time overflows or an `ObserveFeed` reverts.
    pub fn warm_twap(&mut self, price_ray: u128, samples: u8, spacing_ms: u64) {
        for _ in 0..samples {
            let now = self.now_ms.checked_add(spacing_ms).expect("time fits");
            self.set_now_ms(now);
            self.set_price(price_ray);
            self.observe_feed(now).expect("the feed is observed");
        }
    }

    /// Makes the world's feed stale without publishing: advances the world's time to 1 ms past
    /// `newest publish + max_oracle_age_ms - slack_ms`, the first claim whose age check (at
    /// `now + slack`) fails. The max age and the slack are the collateral actor's, so this works
    /// for any world's feed.
    ///
    /// # Panics
    /// If the oracle or the collateral actor holds no state, or the time does not fit.
    pub fn go_stale(&mut self) {
        let coll = self.collateral_state();
        let newest = self.oracle_state().timestamp_ms;
        let stale = newest
            .checked_add(coll.max_oracle_age_ms)
            .and_then(|t| t.checked_sub(coll.ctx.slack_ms))
            .and_then(|t| t.checked_add(1))
            .expect("time fits");
        assert!(stale > self.now_ms, "the feed is already stale");
        self.set_now_ms(stale);
    }

    /// Opens a grace period (ADR-075) through an observed outage: [`World::go_stale`], an
    /// `ObserveFeed` that records the outage, then `grace_min_gap_ms` later a fresh publish of
    /// the oracle's last price and confidence and an `ObserveFeed` that ends the outage. The
    /// world's time is then the start of the grace period.
    ///
    /// # Panics
    /// If an `ObserveFeed` reverts, the time does not fit, or no grace period opened (grace off,
    /// or the collateral risk parameters' minimum gap is 0).
    pub fn start_grace(&mut self) {
        self.go_stale();
        self.observe_feed(self.now_ms)
            .expect("the stale feed is observed");
        let min_gap = self.collateral_state().ext.risk.grace_min_gap_ms;
        let back = self.now_ms.checked_add(min_gap).expect("time fits");
        self.set_now_ms(back);
        let last = self.oracle_state();
        self.set_price_with_confidence(last.price_ray, last.confidence);
        self.observe_feed(back).expect("the fresh feed is observed");
        assert!(
            self.collateral_state().ext.grace_until_ms > back,
            "a grace period opened"
        );
    }

    /// The world's oracle state.
    ///
    /// # Panics
    /// If the oracle holds no oracle state.
    #[must_use]
    pub fn oracle_state(&self) -> OracleState {
        self.chain
            .raw_state(self.oracle)
            .and_then(|bytes| OracleState::decode(&bytes))
            .expect("an oracle state")
    }

    /// The world's collateral actor state.
    ///
    /// # Panics
    /// If the collateral actor holds no collateral state.
    #[must_use]
    pub fn collateral_state(&self) -> CollateralState {
        self.chain
            .raw_state(self.collateral)
            .and_then(|bytes| CollateralState::decode(&bytes))
            .expect("a collateral state")
    }
}
