//! Plans for the vendored stablecoin, token and twap_oracle programs (Shared Reference S2). Encoded by
//! ENUM index through `encode::words` (never the IDL position, spec §3.2); `i128` gains encode.

use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID as CLOCK;
use lee_core::{account::AccountId, program::ProgramId};

use crate::{addresses::HostAddresses, error::SdkResult, plan::Plan};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitializeProgramArgs {
    pub admin: AccountId,
    pub freeze_authority: AccountId,
    pub collateral_definition: AccountId,
    pub market_price_oracle: AccountId,
    pub stability_fee_per_ms: u128,
    pub proportional_gain: i128,
    pub integral_gain: i128,
    pub min_collateralization_ratio: u128,
    pub min_ms_between_rate_updates: u64,
    pub max_oracle_age_ms: u64,
    pub redemption_price: u128,
    pub stablecoin_name: String,
}

pub fn initialize_program(host: &HostAddresses, a: &InitializeProgramArgs) -> SdkResult<Plan> {
    Plan::new(
        host.program,
        vec![
            a.admin,
            host.protocol_parameters,
            host.stability_fee_accumulator,
            host.redemption_price_state,
            host.stablecoin_definition,
            host.stablecoin_master_holding,
            a.collateral_definition,
            a.market_price_oracle,
            CLOCK,
        ],
        vec![a.admin],
        &stablecoin_core::Instruction::InitializeProgram {
            freeze_authority_account_id: a.freeze_authority,
            initial_stability_fee_per_millisecond: a.stability_fee_per_ms,
            initial_controller_proportional_gain: a.proportional_gain,
            initial_controller_integral_gain: a.integral_gain,
            initial_minimum_collateralization_ratio: a.min_collateralization_ratio,
            minimum_milliseconds_between_rate_updates: a.min_ms_between_rate_updates,
            maximum_oracle_price_age_milliseconds: a.max_oracle_age_ms,
            initial_redemption_price: a.redemption_price,
            stablecoin_name: a.stablecoin_name.clone(),
        },
    )
}

pub fn set_liquidation_engine(
    host: &HostAddresses,
    admin: AccountId,
    engine_program: ProgramId,
    engine_authority: AccountId,
) -> SdkResult<Plan> {
    Plan::new(
        host.program,
        vec![
            admin,
            host.protocol_parameters,
            host.liquidation_engine_state,
        ],
        vec![admin],
        &stablecoin_core::Instruction::SetLiquidationEngine {
            engine_program_id: engine_program,
            engine_authority_id: engine_authority,
        },
    )
}

pub fn open_position(
    host: &HostAddresses,
    owner: AccountId,
    owner_collateral_holding: AccountId,
    collateral_definition: AccountId,
    nonce: u64,
    amount: u128,
) -> SdkResult<Plan> {
    let position = host.position(&owner, nonce);
    Plan::new(
        host.program,
        vec![
            owner,
            position,
            host.position_vault(&position),
            owner_collateral_holding,
            collateral_definition,
            host.protocol_parameters,
            CLOCK,
        ],
        vec![owner, owner_collateral_holding],
        &stablecoin_core::Instruction::OpenPosition {
            position_nonce: nonce,
            initial_collateral_amount: amount,
        },
    )
}

pub fn generate_debt(
    host: &HostAddresses,
    owner: AccountId,
    position: AccountId,
    owner_stable_holding: AccountId,
    market_price_oracle: AccountId,
    amount: u128,
) -> SdkResult<Plan> {
    Plan::new(
        host.program,
        vec![
            owner,
            position,
            host.stablecoin_definition,
            owner_stable_holding,
            host.stability_fee_accumulator,
            host.redemption_price_state,
            market_price_oracle,
            host.protocol_parameters,
            CLOCK,
        ],
        vec![owner],
        &stablecoin_core::Instruction::GenerateDebt { amount },
    )
}

pub fn set_minimum_collateralization_ratio(
    host: &HostAddresses,
    admin: AccountId,
    ratio: u128,
) -> SdkResult<Plan> {
    Plan::new(
        host.program,
        vec![admin, host.protocol_parameters],
        vec![admin],
        &stablecoin_core::Instruction::SetMinimumCollateralizationRatio { new_ratio: ratio },
    )
}

/// Signed gains (spel at `1ef0500f` cannot encode `i128`, ADR-018).
pub fn set_controller_gains(
    host: &HostAddresses,
    admin: AccountId,
    proportional: i128,
    integral: i128,
) -> SdkResult<Plan> {
    Plan::new(
        host.program,
        vec![admin, host.protocol_parameters],
        vec![admin],
        &stablecoin_core::Instruction::SetControllerGains {
            new_proportional_gain: proportional,
            new_integral_gain: integral,
        },
    )
}

pub fn token_new_fungible(
    token: ProgramId,
    definition: AccountId,
    holding: AccountId,
    name: &str,
    supply: u128,
    mint_authority: Option<AccountId>,
) -> SdkResult<Plan> {
    Plan::new(
        token,
        vec![definition, holding],
        vec![definition, holding],
        &token_core::Instruction::NewFungibleDefinition {
            name: name.to_owned(),
            total_supply: supply,
            mint_authority,
        },
    )
}

pub fn token_initialize_account(
    token: ProgramId,
    definition: AccountId,
    account: AccountId,
) -> SdkResult<Plan> {
    Plan::new(
        token,
        vec![definition, account],
        vec![account],
        &token_core::Instruction::InitializeAccount,
    )
}

pub fn token_transfer(
    token: ProgramId,
    from: AccountId,
    to: AccountId,
    amount: u128,
) -> SdkResult<Plan> {
    Plan::new(
        token,
        vec![from, to],
        vec![from],
        &token_core::Instruction::Transfer {
            amount_to_transfer: amount,
        },
    )
}

/// Returns the plan and the oracle account it creates (`compute_oracle_price_account_pda`).
pub fn create_oracle_price_account(
    twap: ProgramId,
    price_source: AccountId,
    base: AccountId,
    quote: AccountId,
    initial_price: u128,
    window_ms: u64,
) -> SdkResult<(Plan, AccountId)> {
    let oracle = twap_oracle_core::compute_oracle_price_account_pda(twap, price_source, window_ms);
    let plan = Plan::new(
        twap,
        vec![oracle, price_source, CLOCK],
        vec![price_source],
        &twap_oracle_core::Instruction::CreateOraclePriceAccount {
            base_asset: base,
            quote_asset: quote,
            initial_price,
            window_duration: window_ms,
        },
    )?;
    Ok((plan, oracle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::{stablecoin_name, variant_index};
    use crate::testing::id;
    use crate::testing::standard::{self, k, COLLATERAL, ORACLE, TOKEN, TWAP};
    use crate::testing::support::assert_matches_idl;

    fn decode<T: serde::de::DeserializeOwned>(p: &Plan) -> T {
        risc0_zkvm::serde::from_slice(&p.instruction_words).expect("the guest decodes it")
    }

    #[test]
    fn host_builders_use_enum_indices_and_the_idl_account_lists() {
        let host = HostAddresses::new(standard::HOST);
        let p = set_controller_gains(&host, id(k::HOST_ADMIN), -1, -2).expect("plan");
        assert_eq!(
            variant_index(&p.instruction_words),
            Some(9),
            "enum 9, IDL position 12"
        );
        assert_eq!(
            p.account_ids,
            vec![id(k::HOST_ADMIN), host.protocol_parameters]
        );
        let p = set_minimum_collateralization_ratio(&host, id(k::HOST_ADMIN), 2).expect("plan");
        assert_eq!(
            variant_index(&p.instruction_words),
            Some(8),
            "enum 8, IDL position 11"
        );
        let p = open_position(
            &host,
            id(k::OWNER),
            id(k::OWNER_COLL),
            COLLATERAL,
            0,
            500_000,
        )
        .expect("plan");
        assert_eq!(
            p.signing_requirements,
            vec![id(k::OWNER), id(k::OWNER_COLL)]
        );
        assert_eq!(p.account_ids[1], host.position(&id(k::OWNER), 0));
        let p = generate_debt(
            &host,
            id(k::OWNER),
            host.position(&id(k::OWNER), 0),
            id(k::OWNER_STABLE),
            ORACLE,
            1,
        )
        .expect("plan");
        assert_eq!(
            (variant_index(&p.instruction_words), p.account_ids.len()),
            (Some(6), 9)
        );
        let p = set_liquidation_engine(&host, id(k::HOST_ADMIN), standard::ENGINE, id(k::KEEPER))
            .expect("plan");
        assert_eq!(variant_index(&p.instruction_words), Some(19));
    }

    #[test]
    fn every_stablecoin_builder_matches_its_idl_entry() {
        let host = HostAddresses::new(standard::HOST);
        let admin = id(k::HOST_ADMIN);
        let position = host.position(&id(k::OWNER), 0);
        let init = InitializeProgramArgs {
            admin,
            freeze_authority: id(k::FREEZE),
            collateral_definition: COLLATERAL,
            market_price_oracle: ORACLE,
            stability_fee_per_ms: 1,
            proportional_gain: -3,
            integral_gain: 4,
            min_collateralization_ratio: 5,
            min_ms_between_rate_updates: 6,
            max_oracle_age_ms: 7,
            redemption_price: 8,
            stablecoin_name: "USDL".into(),
        };
        let plans = vec![
            initialize_program(&host, &init),
            set_liquidation_engine(&host, admin, standard::ENGINE, id(k::KEEPER)),
            open_position(&host, id(k::OWNER), id(k::OWNER_COLL), COLLATERAL, 0, 9),
            generate_debt(
                &host,
                id(k::OWNER),
                position,
                id(k::OWNER_STABLE),
                ORACLE,
                1,
            ),
            set_minimum_collateralization_ratio(&host, admin, 2),
            set_controller_gains(&host, admin, -1, 1),
        ];
        for plan in plans {
            let plan = plan.expect("plan");
            let ix: stablecoin_core::Instruction = decode(&plan);
            assert_matches_idl("stablecoin-idl.json", stablecoin_name(&ix), &plan);
            assert_eq!(plan.program_id, standard::HOST);
        }
    }

    #[test]
    fn signed_controller_gains_round_trip_through_the_builder() {
        let host = HostAddresses::new(standard::HOST);
        let (pg, ig) = (-5_000_000_000_000_000_000_000_000_000_i128, i128::MIN);
        let p = set_controller_gains(&host, id(k::HOST_ADMIN), pg, ig).expect("plan");
        let ix: stablecoin_core::Instruction = decode(&p);
        match ix {
            stablecoin_core::Instruction::SetControllerGains {
                new_proportional_gain,
                new_integral_gain,
            } => assert_eq!((new_proportional_gain, new_integral_gain), (pg, ig)),
            other => panic!("unexpected {other:?}"),
        }
        let init = InitializeProgramArgs {
            admin: id(k::HOST_ADMIN),
            freeze_authority: id(k::FREEZE),
            collateral_definition: COLLATERAL,
            market_price_oracle: ORACLE,
            stability_fee_per_ms: 1,
            proportional_gain: -7,
            integral_gain: i128::MAX,
            min_collateralization_ratio: 5,
            min_ms_between_rate_updates: 6,
            max_oracle_age_ms: 7,
            redemption_price: 8,
            stablecoin_name: "USDL".into(),
        };
        let ix: stablecoin_core::Instruction =
            decode(&initialize_program(&host, &init).expect("plan"));
        let text = format!("{ix:?}");
        assert!(
            text.contains("-7") && text.contains(&i128::MAX.to_string()),
            "{text}"
        );
    }

    #[test]
    fn token_and_oracle_builders() {
        let p = token_new_fungible(TOKEN, id(1), id(2), "MOCK", 10, Some(id(3))).expect("plan");
        assert_eq!(
            (
                variant_index(&p.instruction_words),
                p.signing_requirements.len()
            ),
            (Some(1), 2)
        );
        let p = token_initialize_account(TOKEN, id(1), id(2)).expect("plan");
        assert_eq!(
            (
                variant_index(&p.instruction_words),
                p.signing_requirements.clone()
            ),
            (Some(3), vec![id(2)])
        );
        let p = token_transfer(TOKEN, id(2), id(4), 5).expect("plan");
        assert_eq!(variant_index(&p.instruction_words), Some(0));
        let (p, oracle) =
            create_oracle_price_account(TWAP, id(5), id(6), id(7), 1, 3_600_000).expect("plan");
        assert_eq!(p.account_ids[0], oracle);
        assert_eq!(
            oracle,
            twap_oracle_core::compute_oracle_price_account_pda(TWAP, id(5), 3_600_000)
        );
        assert_eq!(variant_index(&p.instruction_words), Some(1));
    }

    #[test]
    fn token_and_oracle_instructions_round_trip() {
        let p = token_transfer(TOKEN, id(2), id(4), 5).expect("plan");
        let ix: token_core::Instruction = decode(&p);
        assert!(matches!(
            ix,
            token_core::Instruction::Transfer {
                amount_to_transfer: 5
            }
        ));
        assert_eq!(
            (p.account_ids, p.signing_requirements),
            (vec![id(2), id(4)], vec![id(2)])
        );

        let p = token_new_fungible(TOKEN, id(1), id(2), "MOCK", 10, Some(id(3))).expect("plan");
        let ix: token_core::Instruction = decode(&p);
        match ix {
            token_core::Instruction::NewFungibleDefinition {
                name,
                total_supply,
                mint_authority,
            } => assert_eq!(
                (name.as_str(), total_supply, mint_authority),
                ("MOCK", 10, Some(id(3)))
            ),
            _ => panic!("unexpected variant"),
        }
        assert_eq!(p.account_ids, vec![id(1), id(2)]);

        let p = token_initialize_account(TOKEN, id(1), id(2)).expect("plan");
        assert!(matches!(
            decode::<token_core::Instruction>(&p),
            token_core::Instruction::InitializeAccount
        ));
        assert_eq!(p.account_ids, vec![id(1), id(2)]);

        let (p, _) =
            create_oracle_price_account(TWAP, id(5), id(6), id(7), 11, 3_600_000).expect("plan");
        match decode::<twap_oracle_core::Instruction>(&p) {
            twap_oracle_core::Instruction::CreateOraclePriceAccount {
                base_asset,
                quote_asset,
                initial_price,
                window_duration,
            } => assert_eq!(
                (base_asset, quote_asset, initial_price, window_duration),
                (id(6), id(7), 11, 3_600_000)
            ),
            _ => panic!("unexpected variant"),
        }
        assert_eq!(
            p.account_ids[1..],
            [id(5), clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID]
        );
        assert_eq!(p.signing_requirements, vec![id(5)]);
    }
}
