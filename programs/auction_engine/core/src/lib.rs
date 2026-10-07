//! Shared types of the `auction_engine` program: instruction enum, account states, PDA seeds,
//! errors and argument parsing. Used by the guest, the host handlers, tests and (Plan 3) the SDK.
#![forbid(unsafe_code)]

pub mod args;
pub mod error;
pub mod instruction;
pub mod pda;
pub mod state;

pub use error::EngineError;
pub use instruction::Instruction;

#[cfg(test)]
mod tests {
    use super::*;
    use auction_core::schedule::SchedulePoint;
    use lee_core::account::{AccountId, Data};
    use pda::*;
    use state::*;

    const ENGINE: lee_core::program::ProgramId = [0xE1; 8];

    #[test]
    fn pdas_match_spel_derivation() {
        let config = AccountId::new([0x11; 32]);
        let position = AccountId::new([0x22; 32]);
        let auction = auction_pda(&ENGINE, &config, &position, 7);
        let spel = spel_framework::pda::compute_pda(
            &ENGINE,
            &[
                &seed_from_str("AUCTION"),
                config.value(),
                position.value(),
                &u64_seed(7),
            ],
        );
        assert_eq!(auction, spel);
        let admin = AccountId::new([0x33; 32]);
        assert_eq!(
            config_pda(&ENGINE, &admin, 42),
            spel_framework::pda::compute_pda(
                &ENGINE,
                &[&seed_from_str("CONFIG"), admin.value(), &u64_seed(42)]
            )
        );
        assert_ne!(
            config_pda(&ENGINE, &admin, 42),
            config_pda(&ENGINE, &position, 42),
            "admin is part of the seed"
        );
        assert_eq!(
            vault_pda(&ENGINE, &auction),
            spel_framework::pda::compute_pda(&ENGINE, &[&seed_from_str("VAULT"), auction.value()])
        );
        assert_eq!(
            engine_authority_pda(&ENGINE, &config),
            spel_framework::pda::compute_pda(
                &ENGINE,
                &[&seed_from_str("ENGINE_AUTH"), config.value()]
            )
        );
        assert_ne!(
            engine_authority_pda(&ENGINE, &config),
            engine_authority_pda(&ENGINE, &position)
        );
        assert_ne!(
            unused_slot_pda(&ENGINE, &config, 0),
            unused_slot_pda(&ENGINE, &config, 1)
        );
    }

    #[test]
    fn auction_state_round_trips_and_rejects_wrong_tag() {
        let s = AuctionState {
            tag: TAG_AUCTION,
            config: AccountId::new([1; 32]),
            position: AccountId::new([2; 32]),
            nonce: 3,
            owner: AccountId::new([4; 32]),
            keeper: AccountId::new([5; 32]),
            settled: false,
            collateral_seized: 10,
            collateral_sold: 0,
            collateral_left: 9,
            keeper_reward: 1,
            leftover_claimable: 0,
            leftover_claimed: 0,
            debt_principal: 100,
            amount_to_raise: 110,
            reserved_amount: 110,
            debt_raised: 0,
            penalty_raised: 0,
            start_ms: 1_000,
            schedule: vec![SchedulePointRec {
                elapsed_ms: 0,
                discount_bps: 200,
            }],
            type_paused_total_at_start: 0,
        };
        let data = Data::from(&s);
        assert_eq!(AuctionState::try_from(&data).unwrap(), s);
        let core = s.to_core().unwrap();
        assert_eq!(core.collateral_left, 9);
        assert_eq!(AuctionState::from_core(&s, &core), s);
        let wrong = AccountingState::default_for_tests();
        assert!(AuctionState::try_from(&Data::from(&wrong)).is_err());
    }

    #[test]
    fn schedule_words_parse_and_validate() {
        let s = args::schedule_from_words(&[0, 200, 2_700_000, 1000]).unwrap();
        assert_eq!(
            s.points()[1],
            SchedulePoint {
                elapsed_ms: 2_700_000,
                discount_bps: 1000
            }
        );
        assert_eq!(
            args::schedule_from_words(&[0, 200, 5]),
            Err(EngineError::ScheduleEncoding)
        );
        assert_eq!(
            args::schedule_from_words(&[]),
            Err(EngineError::ScheduleEncoding)
        );
        assert_eq!(
            args::schedule_from_words(&[0, 3_001]),
            Err(EngineError::InvalidConfig)
        );
        assert_eq!(
            args::schedule_from_words(&[0, 70_000]),
            Err(EngineError::BpsOutOfRange)
        );
    }

    #[test]
    fn engine_params_validation() {
        let ok = args::EngineParamsArgs::defaults_for_tests();
        assert!(ok.validate().is_ok());
        let zero_ref = args::EngineParamsArgs {
            protocol_token_ref_price: 0,
            ..ok.clone()
        };
        assert_eq!(zero_ref.validate(), Err(EngineError::ZeroRefPrice));
        let low_threshold = args::EngineParamsArgs {
            bad_debt_freeze_threshold: ok.debt_lot - 1,
            ..ok.clone()
        };
        assert_eq!(
            low_threshold.validate(),
            Err(EngineError::ThresholdBelowDebtLot)
        );
        let short_ttl = args::EngineParamsArgs {
            system_ttl_ms: 599_999,
            ..ok.clone()
        };
        assert_eq!(short_ttl.validate(), Err(EngineError::TtlOutOfRange));
        let zero_lot = args::EngineParamsArgs {
            surplus_lot: 0,
            ..ok.clone()
        };
        assert_eq!(zero_lot.validate(), Err(EngineError::InvalidConfig));
        let old_oracle = args::EngineParamsArgs {
            ref_oracle_max_age_ms: 86_400_001,
            ..ok.clone()
        };
        assert_eq!(old_oracle.validate(), Err(EngineError::InvalidConfig));
        let zero_cap = args::EngineParamsArgs {
            global_cap: 0,
            ..ok
        };
        assert_eq!(zero_cap.validate(), Err(EngineError::InvalidConfig));
    }

    #[test]
    fn collateral_args_validation() {
        let ok = args::CollateralArgs::defaults_for_tests();
        assert!(ok.validate(10_000_000).is_ok());
        assert_eq!(
            ok.validate(ok.type_cap - 1),
            Err(EngineError::TypeCapAboveGlobal)
        );
        let bad_penalty = args::CollateralArgs {
            penalty_bps: 2_501,
            ..ok.clone()
        };
        assert_eq!(
            bad_penalty.validate(10_000_000),
            Err(EngineError::InvalidConfig)
        );
        let chip = args::CollateralArgs {
            chip_bps: 1_001,
            ..ok.clone()
        };
        assert_eq!(
            chip.validate(10_000_000),
            Err(EngineError::ChipAbovePenalty)
        );
        let band = args::CollateralArgs {
            band_lo_bps: 4_999,
            ..ok.clone()
        };
        assert_eq!(band.validate(10_000_000), Err(EngineError::InvalidConfig));
        let scale = args::CollateralArgs {
            oracle_scale: 2,
            ..ok
        };
        assert_eq!(scale.validate(10_000_000), Err(EngineError::InvalidConfig));
    }

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(EngineError::CapacityExhausted.code(), 33);
        assert_eq!(EngineError::ChainEndAccountsMissing.code(), 41);
        assert_eq!(EngineError::RecoveryInProgress.code(), 61);
        assert_eq!(EngineError::RefPriceStepTooLarge.code(), 16);
        assert_eq!(
            EngineError::from(auction_core::error::AuctionError::DustRemainder),
            EngineError::DustRemainder
        );
        assert_eq!(
            EngineError::from(auction_core::error::PauseReason::ZeroPrice),
            EngineError::PriceFeedOutOfRange
        );
    }

    pub(crate) fn idx(i: &Instruction) -> u32 {
        risc0_zkvm::serde::to_vec(i).unwrap()[0]
    }

    #[test]
    fn instruction_indices_follow_reference_order() {
        assert_eq!(
            idx(&Instruction::SetEngineParams {
                global_cap: 0,
                freeze_on_bad_debt: false,
                bad_debt_freeze_threshold: 0,
                protocol_token_enabled: false,
                protocol_token_ref_price: 0,
                ref_oracle_enabled: false,
                ref_oracle_max_age_ms: 0,
                surplus_buffer_target: 0,
                surplus_lot: 0,
                surplus_delay_ms: 0,
                surplus_premium_bps: 0,
                surplus_max_discount_bps: 0,
                debt_lot: 0,
                debt_initial_discount_bps: 0,
                debt_growth: vec![],
                surplus_decay: vec![],
                max_mint_per_auction: 0,
                min_take: 0,
                system_ttl_ms: 0,
            }),
            1
        );
        // Later tasks append one assertion per appended variant (Shared Reference R2 index).
    }

    #[test]
    fn collateral_and_observe_indices() {
        assert_eq!(idx(&Instruction::ObserveFeed), 3);
        let c = args::CollateralArgs::defaults_for_tests();
        assert_eq!(
            idx(&Instruction::SetCollateralConfig {
                oracle_scale: c.oracle_scale,
                max_oracle_age_ms: c.max_oracle_age_ms,
                max_conf_bps: c.max_conf_bps,
                band_lo_bps: c.band_lo_bps,
                band_hi_bps: c.band_hi_bps,
                penalty_bps: c.penalty_bps,
                tip: c.tip,
                chip_bps: c.chip_bps,
                reward_cap: c.reward_cap,
                debt_floor: c.debt_floor,
                min_bid: c.min_bid,
                type_cap: c.type_cap,
                schedule: c.schedule.clone(),
                admin_paused: c.admin_paused,
            }),
            2
        );
    }

    #[test]
    fn liquidate_index() {
        assert_eq!(idx(&Instruction::Liquidate { nonce: 0 }), 4);
    }

    #[test]
    fn settle_indices() {
        assert_eq!(idx(&Instruction::SettleEmpty), 7);
        assert_eq!(idx(&Instruction::ClaimLeftover), 8);
        assert_eq!(idx(&Instruction::SettleDebt { amount: 0 }), 9);
    }

    #[test]
    fn bid_indices() {
        assert_eq!(
            idx(&Instruction::Bid {
                coin_in_max: 0,
                min_collateral_out: 0
            }),
            5
        );
        assert_eq!(
            idx(&Instruction::BidWithSettlement {
                coin_in_max: 0,
                min_collateral_out: 0
            }),
            6
        );
    }

    #[test]
    fn surplus_indices() {
        assert_eq!(idx(&Instruction::StartSurplusAuction { nonce: 0 }), 10);
        assert_eq!(
            idx(&Instruction::TakeSurplus {
                amount_coin: 0,
                max_protocol_in: 0
            }),
            11
        );
        assert_eq!(idx(&Instruction::CloseSurplusAuction), 12);
    }

    #[test]
    fn debt_indices() {
        assert_eq!(idx(&Instruction::StartDebtAuction { nonce: 0 }), 13);
        assert_eq!(
            idx(&Instruction::TakeDebt {
                amount_coin: 0,
                min_protocol_out: 0
            }),
            14
        );
        assert_eq!(idx(&Instruction::CloseDebtAuction), 15);
    }
}
