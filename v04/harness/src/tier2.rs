//! The Tier 2 test world (Plan 9 ruling R9-14): [`World::new_tier2`] configures the collateral
//! with [`tier2_collateral_args`] and the given `RiskArgs`, so the recommended preset passes
//! ruling R9-3 and the TWAP anchor and the stale floor can bind inside the band.

use auction_engine_params::args::CollateralArgs;
use engine_core::{msg::EngineMsg, state::CollateralState};
use oracle_core::OracleMsg;
use risk_core::RiskArgs;

use crate::world::{Forward, RAY, SALT, World};

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

impl World {
    /// [`World::new`] at redemption price `RAY / 2` with [`tier2_collateral_args`] and `risk`.
    #[must_use]
    pub fn new_tier2(risk: RiskArgs) -> World {
        World::new_with(RAY / 2, |_| {}, tier2_collateral_args(), risk)
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
