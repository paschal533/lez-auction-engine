//! Spec §3.2/§5.10: IDL position == enum index for every engine instruction (Shared Reference R2).

const R2: [&str; 17] = [
    "init_config",
    "set_engine_params",
    "set_collateral_config",
    "observe_feed",
    "liquidate",
    "bid",
    "bid_with_settlement",
    "settle_empty",
    "claim_leftover",
    "settle_debt",
    "start_surplus_auction",
    "take_surplus",
    "close_surplus_auction",
    "start_debt_auction",
    "take_debt",
    "close_debt_auction",
    "check_solvency",
];

fn idl_names() -> Vec<String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../idl/auction_engine-idl.json"
    );
    let idl: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("engine IDL"))
            .expect("IDL json");
    idl["instructions"]
        .as_array()
        .expect("array")
        .iter()
        .map(|i| i["name"].as_str().expect("name").to_owned())
        .collect()
}

#[test]
fn engine_idl_is_a_prefix_of_the_reference_order() {
    let names = idl_names();
    assert!(!names.is_empty() && names.len() <= R2.len());
    for (i, name) in names.iter().enumerate() {
        assert_eq!(name, R2[i], "IDL position {i}");
    }
}

#[test]
fn engine_idl_is_complete() {
    assert_eq!(
        idl_names(),
        R2.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>()
    );
}

// ---- Stablecoin: IDL position != enum index for 8..=18 (spec section 3.2, known upstream fact) ----

/// One sample per `stablecoin_core::Instruction` variant, paired with its IDL name.
fn stablecoin_samples() -> Vec<(&'static str, stablecoin_core::Instruction)> {
    use stablecoin_core::Instruction as I;
    let account = lee_core::account::AccountId::new([1; 32]);
    vec![
        (
            "initialize_program",
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
        ),
        ("accrue_stability_fee", I::AccrueStabilityFee),
        ("update_redemption_rate", I::UpdateRedemptionRate),
        ("refresh_globals", I::RefreshGlobals),
        (
            "open_position",
            I::OpenPosition {
                position_nonce: 0,
                initial_collateral_amount: 0,
            },
        ),
        ("deposit_collateral", I::DepositCollateral { amount: 0 }),
        ("generate_debt", I::GenerateDebt { amount: 0 }),
        ("close_position", I::ClosePosition),
        ("withdraw_collateral", I::WithdrawCollateral { amount: 0 }),
        ("repay_debt", I::RepayDebt { amount: 0 }),
        (
            "set_stability_fee_per_millisecond",
            I::SetStabilityFeePerMillisecond { new_rate: 0 },
        ),
        (
            "set_minimum_collateralization_ratio",
            I::SetMinimumCollateralizationRatio { new_ratio: 0 },
        ),
        (
            "set_controller_gains",
            I::SetControllerGains {
                new_proportional_gain: 0,
                new_integral_gain: 0,
            },
        ),
        (
            "set_timing_parameters",
            I::SetTimingParameters {
                new_minimum_milliseconds_between_rate_updates: 0,
                new_maximum_oracle_price_age_milliseconds: 0,
            },
        ),
        (
            "set_admin",
            I::SetAdmin {
                new_admin_account_id: account,
            },
        ),
        (
            "set_freeze_authority",
            I::SetFreezeAuthority {
                new_freeze_authority_account_id: account,
            },
        ),
        ("set_market_price_oracle", I::SetMarketPriceOracle),
        ("freeze", I::Freeze),
        ("unfreeze", I::Unfreeze),
        (
            "set_liquidation_engine",
            I::SetLiquidationEngine {
                engine_program_id: [1; 8],
                engine_authority_id: account,
            },
        ),
        (
            "liquidate_position",
            I::LiquidatePosition {
                collateral_to_seize: 0,
                normalized_debt_to_cover: 0,
            },
        ),
        ("engine_freeze_new_debt", I::EngineFreezeNewDebt),
    ]
}

/// Plan 3 SDK encoders must key on the enum index, never on the IDL position: for IDL positions
/// 8..=18 the two differ (the vendored IDL follows the upstream guest function order, the guest
/// decodes the enum). `idl/stablecoin-idl.json` is pinned to upstream and is not reordered.
#[test]
fn stablecoin_idl_position_maps_to_enum_index() {
    // IDL position -> enum index, written out (docs/architecture/programs.md "Interfaces for Plan 3").
    const EXPECTED: [u32; 22] = [
        0, 1, 2, 3, 4, 5, 6, 7, 17, 18, 14, 8, 9, 11, 12, 13, 10, 15, 16, 19, 20, 21,
    ];
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../idl/stablecoin-idl.json");
    let idl: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("stablecoin IDL"))
            .expect("IDL json");
    let idl_names: Vec<String> = idl["instructions"]
        .as_array()
        .expect("array")
        .iter()
        .map(|i| i["name"].as_str().expect("name").to_owned())
        .collect();
    assert_eq!(idl_names.len(), EXPECTED.len());

    let samples = stablecoin_samples();
    assert_eq!(samples.len(), EXPECTED.len());
    // Enum index of each sample, as the first word of its risc0 serde encoding.
    let enum_index = |name: &str| -> u32 {
        let (_, ix) = samples
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("IDL instruction {name} has no enum variant"));
        risc0_zkvm::serde::to_vec(ix).expect("serializes")[0]
    };
    for (position, name) in idl_names.iter().enumerate() {
        assert_eq!(
            enum_index(name),
            EXPECTED[position],
            "IDL position {position} ({name}) -> enum index"
        );
    }
    // Every enum index 0..=21 is taken exactly once, so no variant is missing or duplicated.
    let mut seen: Vec<u32> = samples.iter().map(|(n, _)| enum_index(n)).collect();
    seen.sort_unstable();
    assert_eq!(seen, (0..22).collect::<Vec<u32>>());
}
