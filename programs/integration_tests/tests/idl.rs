//! Spec §3.2/§5.10: IDL position == enum index for every engine instruction (Shared Reference S1), and
//! the pinned IDL-position -> enum-index table of the vendored stablecoin.

fn idl_names(file: &str) -> Vec<String> {
    let path = format!("{}/../../idl/{file}", env!("CARGO_MANIFEST_DIR"));
    let idl: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("IDL file")).expect("IDL json");
    idl["instructions"]
        .as_array()
        .expect("array")
        .iter()
        .map(|i| i["name"].as_str().expect("name").to_owned())
        .collect()
}

fn word0<T: serde::Serialize>(ix: &T) -> u32 {
    risc0_zkvm::serde::to_vec(ix).expect("serializes")[0]
}

#[test]
fn every_engine_variant_has_a_sample_and_an_idl_entry_at_its_enum_index() {
    let samples = auction_engine_core::instruction::instruction_samples();
    let names = idl_names("auction_engine-idl.json");
    assert_eq!(
        samples.len(),
        names.len(),
        "one sample per IDL instruction (regenerate the IDL after adding a variant)"
    );
    for (i, ix) in samples.iter().enumerate() {
        assert_eq!(
            usize::try_from(word0(ix)).expect("u32 fits usize"),
            i,
            "sample {i} is variant {i}"
        );
        assert_eq!(ix.name(), names[i], "IDL position {i}");
    }
}

/// Exhaustive: a new stablecoin variant does not compile until it is named here.
fn stablecoin_name(ix: &stablecoin_core::Instruction) -> &'static str {
    use stablecoin_core::Instruction as I;
    match ix {
        I::InitializeProgram { .. } => "initialize_program",
        I::AccrueStabilityFee => "accrue_stability_fee",
        I::UpdateRedemptionRate => "update_redemption_rate",
        I::RefreshGlobals => "refresh_globals",
        I::OpenPosition { .. } => "open_position",
        I::DepositCollateral { .. } => "deposit_collateral",
        I::GenerateDebt { .. } => "generate_debt",
        I::ClosePosition => "close_position",
        I::SetMinimumCollateralizationRatio { .. } => "set_minimum_collateralization_ratio",
        I::SetControllerGains { .. } => "set_controller_gains",
        I::SetMarketPriceOracle => "set_market_price_oracle",
        I::SetTimingParameters { .. } => "set_timing_parameters",
        I::SetAdmin { .. } => "set_admin",
        I::SetFreezeAuthority { .. } => "set_freeze_authority",
        I::SetStabilityFeePerMillisecond { .. } => "set_stability_fee_per_millisecond",
        I::Freeze => "freeze",
        I::Unfreeze => "unfreeze",
        I::WithdrawCollateral { .. } => "withdraw_collateral",
        I::RepayDebt { .. } => "repay_debt",
        I::SetLiquidationEngine { .. } => "set_liquidation_engine",
        I::LiquidatePosition { .. } => "liquidate_position",
        I::EngineFreezeNewDebt => "engine_freeze_new_debt",
    }
}

/// One sample per `stablecoin_core::Instruction` variant (any order).
fn stablecoin_samples() -> Vec<stablecoin_core::Instruction> {
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

/// SDK encoders key on the enum index, never on the IDL position: for IDL positions 8..=18 the two
/// differ (the vendored IDL follows the upstream guest function order). `idl/stablecoin-idl.json` is
/// pinned to upstream and is not reordered.
#[test]
fn stablecoin_idl_position_maps_to_enum_index() {
    // IDL position -> enum index (docs/architecture/programs.md "Interfaces for Plan 3").
    const EXPECTED: [u32; 22] = [
        0, 1, 2, 3, 4, 5, 6, 7, 17, 18, 14, 8, 9, 11, 12, 13, 10, 15, 16, 19, 20, 21,
    ];
    let idl = idl_names("stablecoin-idl.json");
    let samples = stablecoin_samples();
    let mut sample_names: Vec<&str> = samples.iter().map(stablecoin_name).collect();
    sample_names.sort_unstable();
    let mut idl_sorted: Vec<&str> = idl.iter().map(String::as_str).collect();
    idl_sorted.sort_unstable();
    assert_eq!(
        sample_names, idl_sorted,
        "every variant has an IDL entry and every IDL entry has a sample"
    );
    assert_eq!(idl.len(), EXPECTED.len());
    for (position, name) in idl.iter().enumerate() {
        let ix = samples
            .iter()
            .find(|s| stablecoin_name(s) == name)
            .expect("sample exists (checked above)");
        assert_eq!(
            word0(ix),
            EXPECTED[position],
            "IDL position {position} ({name})"
        );
    }
    let mut seen: Vec<u32> = samples.iter().map(word0).collect();
    seen.sort_unstable();
    assert_eq!(
        seen,
        (0..22).collect::<Vec<u32>>(),
        "enum indices 0..=21, each once"
    );
}
