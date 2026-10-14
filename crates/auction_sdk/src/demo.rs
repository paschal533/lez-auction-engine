//! The parameters of the one-command demo (`scripts/demo.sh`, ADR-051), shared by the CLI's
//! parameter-file test and the in-process scenario test (`demo_scenario.rs`), so the numbers the
//! demo relies on are pinned on the real guests.

use auction_engine_core::args::{CollateralArgs, EngineParamsArgs};

/// The demo moves the market price to `PRICE_MOVE_NUM / PRICE_MOVE_DEN` of the redemption price R:
/// 1.9 R is inside the validity range `[R/2, 2R]` (no pause) and makes the lot worth less than the debt.
pub const PRICE_MOVE_NUM: u128 = 19;
pub const PRICE_MOVE_DEN: u128 = 10;
/// The band must reach the moved price, otherwise the clamp at 1.1 R hides the move (spec §5.2 allows
/// `band_hi_bps` up to 20_000).
pub const BAND_HI_BPS: u32 = 20_000;
/// One debt-auction lot (stablecoin base units) and the freeze threshold (spec §5.12: threshold >= lot).
pub const DEBT_LOT: u128 = 5_000;
pub const FREEZE_THRESHOLD: u128 = 5_000;

/// Engine parameters with the protocol token enabled (the demo disables it for the freeze path).
#[must_use]
pub fn engine_params() -> EngineParamsArgs {
    EngineParamsArgs {
        protocol_token_enabled: true,
        debt_lot: DEBT_LOT,
        bad_debt_freeze_threshold: FREEZE_THRESHOLD,
        ..EngineParamsArgs::defaults_for_tests()
    }
}

#[must_use]
pub fn collateral_args() -> CollateralArgs {
    CollateralArgs {
        band_hi_bps: BAND_HI_BPS,
        ..CollateralArgs::defaults_for_tests()
    }
}
