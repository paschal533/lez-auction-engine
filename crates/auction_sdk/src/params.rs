//! Parameter files of `admin init-config` / `set-engine-params` / `set-collateral-config` (the files
//! under `cli/params/`). `u128` values are decimal strings (JSON numbers cannot carry them safely); the
//! two optional-slot flags come from `--protocol-token` / `--ref-oracle`, never from the file, so they
//! cannot disagree with the slots.

use std::path::Path;

use auction_engine_core::args::{CollateralArgs, EngineParamsArgs};
use serde::{de::DeserializeOwned, Deserialize};

use crate::error::{SdkError, SdkResult};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineParamsFile {
    global_cap: String,
    freeze_on_bad_debt: bool,
    bad_debt_freeze_threshold: String,
    protocol_token_ref_price: String,
    ref_oracle_max_age_ms: u64,
    surplus_buffer_target: String,
    surplus_lot: String,
    surplus_delay_ms: u64,
    surplus_premium_bps: u32,
    surplus_max_discount_bps: u32,
    debt_lot: String,
    debt_initial_discount_bps: u32,
    debt_growth: Vec<u32>,
    surplus_decay: Vec<u32>,
    max_mint_per_auction: String,
    min_take: String,
    system_ttl_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CollateralFile {
    oracle_scale: u8,
    max_oracle_age_ms: u64,
    max_conf_bps: u32,
    band_lo_bps: u32,
    band_hi_bps: u32,
    penalty_bps: u32,
    tip: String,
    chip_bps: u32,
    reward_cap: String,
    debt_floor: String,
    min_bid: String,
    type_cap: String,
    schedule: Vec<u32>,
    admin_paused: bool,
}

fn read<T: DeserializeOwned>(path: &Path) -> SdkResult<T> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| SdkError::Refused(format!("{}: {e}", path.display())))?;
    serde_json::from_str(&text).map_err(|e| SdkError::Refused(format!("{}: {e}", path.display())))
}

fn big(s: &str, field: &str) -> SdkResult<u128> {
    s.parse::<u128>()
        .map_err(|_| SdkError::Refused(format!("{field} must be a decimal u128 string")))
}

pub fn engine_params(
    path: &Path,
    protocol_token_enabled: bool,
    ref_oracle_enabled: bool,
) -> SdkResult<EngineParamsArgs> {
    let f: EngineParamsFile = read(path)?;
    let p = EngineParamsArgs {
        global_cap: big(&f.global_cap, "global_cap")?,
        freeze_on_bad_debt: f.freeze_on_bad_debt,
        bad_debt_freeze_threshold: big(&f.bad_debt_freeze_threshold, "bad_debt_freeze_threshold")?,
        protocol_token_enabled,
        protocol_token_ref_price: big(&f.protocol_token_ref_price, "protocol_token_ref_price")?,
        ref_oracle_enabled,
        ref_oracle_max_age_ms: f.ref_oracle_max_age_ms,
        surplus_buffer_target: big(&f.surplus_buffer_target, "surplus_buffer_target")?,
        surplus_lot: big(&f.surplus_lot, "surplus_lot")?,
        surplus_delay_ms: f.surplus_delay_ms,
        surplus_premium_bps: f.surplus_premium_bps,
        surplus_max_discount_bps: f.surplus_max_discount_bps,
        debt_lot: big(&f.debt_lot, "debt_lot")?,
        debt_initial_discount_bps: f.debt_initial_discount_bps,
        debt_growth: f.debt_growth,
        surplus_decay: f.surplus_decay,
        max_mint_per_auction: big(&f.max_mint_per_auction, "max_mint_per_auction")?,
        min_take: big(&f.min_take, "min_take")?,
        system_ttl_ms: f.system_ttl_ms,
    };
    // Same rules the engine applies at config time (ADR-024): fail before signing anything.
    p.validate()
        .map_err(|e| SdkError::Refused(format!("invalid engine parameters: {e}")))?;
    Ok(p)
}

pub fn collateral_args(path: &Path) -> SdkResult<CollateralArgs> {
    let f: CollateralFile = read(path)?;
    Ok(CollateralArgs {
        oracle_scale: f.oracle_scale,
        max_oracle_age_ms: f.max_oracle_age_ms,
        max_conf_bps: f.max_conf_bps,
        band_lo_bps: f.band_lo_bps,
        band_hi_bps: f.band_hi_bps,
        penalty_bps: f.penalty_bps,
        tip: big(&f.tip, "tip")?,
        chip_bps: f.chip_bps,
        reward_cap: big(&f.reward_cap, "reward_cap")?,
        debt_floor: big(&f.debt_floor, "debt_floor")?,
        min_bid: big(&f.min_bid, "min_bid")?,
        type_cap: big(&f.type_cap, "type_cap")?,
        schedule: f.schedule,
        admin_paused: f.admin_paused,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_localnet_files_parse_and_validate() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cli/params");
        let p =
            engine_params(&dir.join("localnet-engine.json"), true, false).expect("engine params");
        assert_eq!(
            p.protocol_token_ref_price,
            2_000_000_000_000_000_000_000_000_000
        );
        assert!(p.protocol_token_enabled && !p.ref_oracle_enabled);
        let c = collateral_args(&dir.join("localnet-collateral.json")).expect("collateral");
        assert_eq!(c.validate(p.global_cap).expect("valid").0.min_bid, 25);
    }

    #[test]
    fn a_bad_number_names_its_field() {
        let dir = tempfile::tempdir().expect("tmp");
        let f = dir.path().join("p.json");
        let text = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cli/params/localnet-engine.json"),
        )
        .expect("read")
        .replace("\"10000000\"", "\"ten\"");
        std::fs::write(&f, text).expect("write");
        let e = engine_params(&f, false, false).expect_err("bad number");
        assert!(e.to_string().contains("global_cap"), "{e}");
    }

    #[test]
    fn the_demo_files_carry_the_pinned_scenario() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cli/params");
        let p = engine_params(&dir.join("demo-engine.json"), true, false).expect("engine params");
        assert_eq!(p, crate::demo::engine_params());
        p.validate().expect("valid");
        let c = collateral_args(&dir.join("demo-collateral.json")).expect("collateral args");
        assert_eq!(c, crate::demo::collateral_args());
        c.validate(p.global_cap).expect("valid");
    }
}
