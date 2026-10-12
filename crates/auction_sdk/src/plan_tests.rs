use super::*;
use crate::testing::id;
use crate::testing::standard::{self, k, COLLATERAL, ORACLE, PROTOCOL};
use crate::testing::support::assert_matches_idl;
use crate::verify::verify_config;
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID as CLOCK;

const P: ProgramId = [7; 8];

#[test]
fn plan_rejects_duplicates_unlisted_signers_and_no_signer() {
    let (a, b) = (AccountId::new([1; 32]), AccountId::new([2; 32]));
    let ix = Instruction::ObserveFeed;
    assert!(matches!(
        Plan::new(P, vec![a, a], vec![a], &ix),
        Err(SdkError::Encode(m)) if m.contains("listed twice")
    ));
    assert!(matches!(
        Plan::new(P, vec![a], vec![b], &ix),
        Err(SdkError::Encode(m)) if m.contains("not one of")
    ));
    assert!(matches!(
        Plan::new(P, vec![a], vec![], &ix),
        Err(SdkError::Encode(_))
    ));
    let ok = Plan::new(P, vec![a, b], vec![a], &ix).expect("valid");
    assert_eq!(ok.variant_index(), Some(3));
}

fn v() -> (standard::Fixture, VerifiedConfig) {
    let fx = standard::standard();
    let v = verify_config(&fx.chain, &fx.deployment, &standard::config_id()).expect("verifies");
    (fx, v)
}

fn name_of(p: &Plan) -> &'static str {
    let ix: Instruction = risc0_zkvm::serde::from_slice(&p.instruction_words).expect("decodes");
    ix.name()
}

#[test]
fn liquidate_lists_the_seventeen_accounts_in_order() {
    let (fx, v) = v();
    let pos = fx.unsafe_position;
    let p = liquidate(
        &v,
        &LiquidateArgs {
            keeper: id(k::KEEPER),
            keeper_collateral_holding: id(k::KEEPER_COLL),
            keeper_holding_is_fresh: true,
            position: pos,
            nonce: 0,
            oracle: ORACLE,
        },
    )
    .expect("plan");
    let auction = v.engine.auction(&pos, 0);
    assert_eq!(
        p.account_ids,
        vec![
            id(k::KEEPER),
            id(k::KEEPER_COLL),
            v.config_id,
            v.coll_config_id(),
            pos,
            v.engine.record(&pos),
            auction,
            v.engine.auction_vault(&auction),
            v.engine.engine_authority,
            v.host.liquidation_engine_state,
            v.host.position_vault(&pos),
            COLLATERAL,
            v.host.stability_fee_accumulator,
            v.host.redemption_price_state,
            ORACLE,
            v.host.protocol_parameters,
            CLOCK,
        ]
    );
    assert_eq!(
        p.signing_requirements,
        vec![id(k::KEEPER), id(k::KEEPER_COLL)]
    );
    assert_eq!(name_of(&p), "liquidate");
}

#[test]
fn bid_switches_to_bid_with_settlement_and_appends_accounting() {
    let (fx, v) = v();
    let args = BidArgs {
        bidder_stable: id(k::BIDDER_STABLE),
        bidder_collateral: id(k::BIDDER_COLL),
        bidder_collateral_is_fresh: false,
        auction: fx.live_auction,
        coin_in_max: 100_000,
        min_collateral_out: 51_000,
        with_settlement: false,
        oracle: ORACLE,
    };
    let plain = bid(&v, &args).expect("bid");
    assert_eq!((name_of(&plain), plain.account_ids.len()), ("bid", 11));
    let settle = bid(
        &v,
        &BidArgs {
            with_settlement: true,
            ..args
        },
    )
    .expect("bid_with_settlement");
    assert_eq!(name_of(&settle), "bid_with_settlement");
    assert_eq!(settle.account_ids.last(), Some(&v.engine.accounting));
    assert_eq!(settle.signing_requirements, vec![id(k::BIDDER_STABLE)]);
}

#[test]
fn slots_follow_the_config_and_init_config_checks_the_flags() {
    let (fx, v) = v();
    let p = start_debt(&v, id(k::CALLER), 1).expect("plan");
    assert_eq!(p.account_ids[6], PROTOCOL, "configured protocol token");
    assert_eq!(
        p.account_ids[7], v.engine.unused_ref_oracle,
        "no reference oracle: placeholder"
    );
    let params = auction_engine_core::args::EngineParamsArgs::defaults_for_tests();
    let e = init_config(
        standard::ENGINE,
        &fx.host,
        id(k::ENGINE_ADMIN),
        1,
        &params,
        Some(PROTOCOL),
        None,
    )
    .expect_err("flag says disabled, slot says enabled");
    assert!(matches!(e, SdkError::Encode(_)));
    let ok = init_config(
        standard::ENGINE,
        &fx.host,
        id(k::ENGINE_ADMIN),
        1,
        &params,
        None,
        None,
    )
    .expect("plan");
    assert_eq!(ok.account_ids[1], standard::config_id());
    assert_eq!(ok.account_ids[6], v.engine.unused_protocol_token);
}

fn take_args() -> TakeArgs {
    TakeArgs {
        payer: id(k::TAKER_PROTOCOL),
        receiver: id(k::TAKER_STABLE),
        receiver_is_fresh: false,
        nonce: 0,
        amount_coin: 500,
        limit: 200,
    }
}

/// Every engine builder once, paired with the instruction name it must encode (also the IDL name).
fn all_plans(fx: &standard::Fixture, v: &VerifiedConfig) -> Vec<(SdkResult<Plan>, &'static str)> {
    let caller = id(k::CALLER);
    let admin = id(k::ENGINE_ADMIN);
    let take = take_args();
    let params = EngineParamsArgs::defaults_for_tests();
    vec![
        (
            init_config(standard::ENGINE, &fx.host, admin, 1, &params, None, None),
            "init_config",
        ),
        (
            set_engine_params(v, admin, &params, None, None),
            "set_engine_params",
        ),
        (
            set_collateral_config(v, admin, ORACLE, &CollateralArgs::defaults_for_tests()),
            "set_collateral_config",
        ),
        (observe_feed(v, caller, ORACLE), "observe_feed"),
        (settle_empty(v, caller, fx.live_auction), "settle_empty"),
        (
            claim_leftover(v, id(k::OWNER2), id(k::OWNER_COLL), false, fx.live_auction),
            "claim_leftover",
        ),
        (settle_debt(v, caller, u128::MAX), "settle_debt"),
        (start_surplus(v, caller, 1), "start_surplus_auction"),
        (take_surplus(v, &take), "take_surplus"),
        (close_surplus(v, caller, 0), "close_surplus_auction"),
        (start_debt(v, caller, 1), "start_debt_auction"),
        (
            take_debt(
                v,
                &TakeArgs {
                    payer: id(k::TAKER_STABLE),
                    receiver: id(k::TAKER_PROTOCOL),
                    ..take
                },
            ),
            "take_debt",
        ),
        (close_debt(v, caller, 0), "close_debt_auction"),
        (check_solvency(v, caller), "check_solvency"),
    ]
}

#[test]
fn every_engine_builder_encodes_its_own_variant() {
    let (fx, v) = v();
    for (plan, name) in all_plans(&fx, &v) {
        let plan = plan.expect(name);
        assert_eq!(name_of(&plan), name);
    }
}

#[test]
fn every_engine_builder_matches_its_idl_entry() {
    let (fx, v) = v();
    for (plan, name) in all_plans(&fx, &v) {
        assert_matches_idl("auction_engine-idl.json", name, &plan.expect(name), false);
    }
    let pos = fx.unsafe_position;
    let liq = liquidate(
        &v,
        &LiquidateArgs {
            keeper: id(k::KEEPER),
            keeper_collateral_holding: id(k::KEEPER_COLL),
            keeper_holding_is_fresh: true,
            position: pos,
            nonce: 0,
            oracle: ORACLE,
        },
    )
    .expect("plan");
    assert_matches_idl("auction_engine-idl.json", "liquidate", &liq, true);
    let args = BidArgs {
        bidder_stable: id(k::BIDDER_STABLE),
        bidder_collateral: id(k::BIDDER_COLL),
        bidder_collateral_is_fresh: true,
        auction: fx.live_auction,
        coin_in_max: 1,
        min_collateral_out: 1,
        with_settlement: false,
        oracle: ORACLE,
    };
    assert_matches_idl(
        "auction_engine-idl.json",
        "bid",
        &bid(&v, &args).expect("plan"),
        true,
    );
    let settle = bid(
        &v,
        &BidArgs {
            with_settlement: true,
            ..args
        },
    )
    .expect("plan");
    assert_matches_idl(
        "auction_engine-idl.json",
        "bid_with_settlement",
        &settle,
        true,
    );
}

#[test]
fn fresh_recipient_holdings_sign_and_initialised_ones_do_not() {
    let (fx, v) = v();
    let (payer, receiver) = (id(k::TAKER_PROTOCOL), id(k::TAKER_STABLE));
    let fresh = TakeArgs {
        receiver_is_fresh: true,
        ..take_args()
    };
    assert_eq!(
        take_surplus(&v, &fresh).expect("plan").signing_requirements,
        vec![payer, receiver]
    );
    assert_eq!(
        take_surplus(&v, &take_args())
            .expect("plan")
            .signing_requirements,
        vec![payer]
    );
    let claim =
        claim_leftover(&v, id(k::OWNER2), id(k::OWNER_COLL), true, fx.live_auction).expect("plan");
    assert_eq!(
        claim.signing_requirements,
        vec![id(k::OWNER2), id(k::OWNER_COLL)]
    );
}

#[test]
fn set_engine_params_slots_carry_the_new_values() {
    let (_, v) = v();
    let mut params = EngineParamsArgs::defaults_for_tests();
    params.protocol_token_enabled = true;
    assert!(set_engine_params(&v, id(k::ENGINE_ADMIN), &params, None, None).is_err());
    let p =
        set_engine_params(&v, id(k::ENGINE_ADMIN), &params, Some(PROTOCOL), None).expect("plan");
    assert_eq!(p.account_ids[4], PROTOCOL);
    assert_eq!(p.account_ids[5], v.engine.unused_ref_oracle);
}

#[test]
fn a_spurious_signer_at_index_one_does_not_pass_the_idl_check() {
    let (_, v) = v();
    let plan = take_surplus(&v, &take_args()).expect("plan");
    let mut spurious = plan.clone();
    spurious.signing_requirements.push(plan.account_ids[1]);
    let caught = std::panic::catch_unwind(|| {
        assert_matches_idl("auction_engine-idl.json", "take_surplus", &spurious, false);
    });
    assert!(caught.is_err(), "extra signer at index 1 must be refused");
    assert_matches_idl("auction_engine-idl.json", "take_surplus", &spurious, true);
}

#[test]
fn system_and_admin_builders_list_their_accounts_in_the_documented_order() {
    let (_, v) = v();
    let (caller, admin) = (id(k::CALLER), id(k::ENGINE_ADMIN));
    let take = take_args();
    let p = take_surplus(&v, &take).expect("plan");
    assert_eq!(
        p.account_ids,
        vec![
            take.payer,
            take.receiver,
            v.config_id,
            v.engine.accounting,
            v.engine.surplus_auction(0),
            v.engine.surplus_vault(0),
            PROTOCOL,
            CLOCK,
        ]
    );
    let p = take_debt(
        &v,
        &TakeArgs {
            payer: id(k::TAKER_STABLE),
            receiver: id(k::TAKER_PROTOCOL),
            ..take
        },
    )
    .expect("plan");
    assert_eq!(
        p.account_ids,
        vec![
            id(k::TAKER_STABLE),
            id(k::TAKER_PROTOCOL),
            v.config_id,
            v.engine.accounting,
            v.engine.debt_auction(0),
            v.state.stablecoin_definition_id,
            PROTOCOL,
            v.engine.mint_authority,
            CLOCK,
        ]
    );
    let p = check_solvency(&v, caller).expect("plan");
    assert_eq!(
        p.account_ids,
        vec![
            caller,
            v.config_id,
            v.engine.accounting,
            v.engine.buffer,
            v.state.stablecoin_definition_id,
            PROTOCOL,
            v.engine.unused_ref_oracle,
            v.engine.engine_authority,
            v.host.liquidation_engine_state,
            v.host.protocol_parameters,
            CLOCK,
        ]
    );
    let mut params = EngineParamsArgs::defaults_for_tests();
    params.protocol_token_enabled = true;
    let p = set_engine_params(&v, admin, &params, Some(PROTOCOL), None).expect("plan");
    assert_eq!(
        p.account_ids,
        vec![
            admin,
            v.config_id,
            v.engine.accounting,
            v.coll_config_id(),
            PROTOCOL,
            v.engine.unused_ref_oracle,
        ]
    );
}
