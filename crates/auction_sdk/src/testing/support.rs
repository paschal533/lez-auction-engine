//! Single source of the IDL-name extraction and the stablecoin instruction samples. Used by the SDK's
//! encoder tests and by `programs/integration_tests/tests/idl.rs`; not a stable API.

/// Instruction names, in IDL order, of `idl/<file>` at the repository root.
#[must_use]
pub fn idl_instruction_names(file: &str) -> Vec<String> {
    let path = format!("{}/../../idl/{file}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("IDL file {path}: {e}"));
    let idl: serde_json::Value = serde_json::from_str(&text).expect("IDL json");
    idl["instructions"]
        .as_array()
        .expect("instructions array")
        .iter()
        .map(|i| i["name"].as_str().expect("instruction name").to_owned())
        .collect()
}

/// One sample per `stablecoin_core::Instruction` variant (any order).
#[must_use]
pub fn stablecoin_samples() -> Vec<stablecoin_core::Instruction> {
    use stablecoin_core::Instruction as I;
    let account = lee_core::account::AccountId::new([1; 32]);
    vec![
        I::InitializeProgram {
            freeze_authority_account_id: account,
            initial_stability_fee_per_millisecond: 0,
            initial_controller_proportional_gain: 0,
            initial_controller_integral_gain: 0,
            initial_minimum_collateralization_ratio: 0,
            minimum_milliseconds_between_rate_updates: 0,
            maximum_oracle_price_age_milliseconds: 0,
            initial_redemption_price: 0,
            stablecoin_name: String::new(),
        },
        I::AccrueStabilityFee,
        I::UpdateRedemptionRate,
        I::RefreshGlobals,
        I::OpenPosition {
            position_nonce: 0,
            initial_collateral_amount: 0,
        },
        I::DepositCollateral { amount: 0 },
        I::GenerateDebt { amount: 0 },
        I::ClosePosition,
        I::WithdrawCollateral { amount: 0 },
        I::RepayDebt { amount: 0 },
        I::SetStabilityFeePerMillisecond { new_rate: 0 },
        I::SetMinimumCollateralizationRatio { new_ratio: 0 },
        I::SetControllerGains {
            new_proportional_gain: 0,
            new_integral_gain: 0,
        },
        I::SetTimingParameters {
            new_minimum_milliseconds_between_rate_updates: 0,
            new_maximum_oracle_price_age_milliseconds: 0,
        },
        I::SetAdmin {
            new_admin_account_id: account,
        },
        I::SetFreezeAuthority {
            new_freeze_authority_account_id: account,
        },
        I::SetMarketPriceOracle,
        I::Freeze,
        I::Unfreeze,
        I::SetLiquidationEngine {
            engine_program_id: [1; 8],
            engine_authority_id: account,
        },
        I::LiquidatePosition {
            collateral_to_seize: 0,
            normalized_debt_to_cover: 0,
        },
        I::EngineFreezeNewDebt,
    ]
}
