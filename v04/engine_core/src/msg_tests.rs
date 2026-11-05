//! The wire-format pins of [`super::EngineMsg`] and its companions: every variant index is
//! part of the wire format (appended only), pinned one by one. A child module of `msg`.

use super::*;
use crate::state::fixtures::id;
use auction_engine_params::args::{CollateralArgs, EngineParamsArgs};

#[test]
fn root_variant_indices_are_fixed() {
    let params = Box::new(EngineParamsArgs::defaults_for_tests());
    let roots = [
        EngineMsg::Init {
            salt: 1,
            params,
            host: Actor::new(id(1), id(2)),
            token: id(3),
            stable_def: id(4),
            oracle: Actor::new(id(5), id(6)),
            clock: Actor::new(id(7), id(8)),
            slack_ms: 9,
            ref_oracle: Some(Actor::new(id(10), id(11))),
            protocol_genesis: Some(ProtocolGenesis {
                to: id(12),
                supply: 13,
            }),
        },
        EngineMsg::SetCollateral {
            salt: 1,
            collateral_def: id(1),
            args: CollateralArgs::defaults_for_tests(),
            now_ms: 2,
        },
        EngineMsg::Liquidate {
            config: id(1),
            collateral_def: id(2),
            position: id(3),
            nonce: 5,
            now_ms: 4,
        },
        EngineMsg::Bid {
            bidder: id(9),
            config: id(1),
            collateral_def: id(2),
            position: id(3),
            nonce: 4,
            max_debt_in: 5,
            min_collateral_out: 6,
            now_ms: 7,
        },
        EngineMsg::ClaimLeftover {
            config: id(1),
            position: id(2),
            nonce: 3,
        },
    ];
    assert_indices(&roots, 0);
}

/// Every message's first byte is its variant index, counted from `first`.
fn assert_indices<T: BorshSerialize + core::fmt::Debug>(messages: &[T], first: usize) {
    for (index, message) in (first..).zip(messages) {
        let bytes = borsh::to_vec(message).expect("encodes");
        assert_eq!(
            usize::from(*bytes.first().expect("non-empty")),
            index,
            "{message:?}"
        );
    }
}

/// The variants 5..=25 are wire format too (appended only): pinned one by one.
#[test]
fn internal_variant_indices_are_fixed() {
    use crate::state::fixtures::{actor, coll};
    let (position, nonce) = (id(1), 2);
    let internals = [
        EngineMsg::Seized {
            position,
            owner: id(2),
            collateral: 3,
            normalized_debt: 4,
            nominal_debt: 5,
        },
        EngineMsg::LiqCtx {
            position,
            nonce,
            keeper: id(3),
            now_ms: 4,
            price_ray: 5,
            collateral: Box::new(coll()),
        },
        EngineMsg::Bump { position, nonce },
        EngineMsg::Open {
            position,
            nonce,
            keeper: id(3),
            host: actor(4, 5),
            token: id(6),
            stable_def: id(7),
            now_ms: 8,
            price_ray: 9,
            collateral: Box::new(coll()),
            global_cap: 10,
            global_reserved: 11,
        },
        EngineMsg::Reserve {
            position,
            nonce,
            amount: 3,
        },
        EngineMsg::Release {
            position,
            nonce,
            amount: 3,
        },
        EngineMsg::Settled {
            config: id(3),
            position,
            nonce,
            debt_raised: 4,
            penalty_raised: 5,
            shortfall: 6,
            release: 7,
            leftover: 8,
        },
        EngineMsg::Finish,
        EngineMsg::BidCtx {
            bidder: id(3),
            position,
            nonce,
            max_debt_in: 4,
            min_collateral_out: 5,
            price_ray: 6,
            now_ms: 7,
            paused_total_ms: 8,
            min_bid: 9,
            chost: 10,
        },
        EngineMsg::Proceed,
        EngineMsg::Configure {
            collateral_def: id(3),
            args: CollateralArgs::defaults_for_tests(),
            ctx: ObserveCtx {
                host: actor(4, 5),
                oracle: actor(6, 7),
                clock: actor(8, 9),
                slack_ms: 10,
            },
        },
        EngineMsg::Probe {
            kind: ProbeKind::Config,
        },
        EngineMsg::InitReads,
        EngineMsg::Observe { now_ms: 3 },
        EngineMsg::SettleEmpty {
            config: id(3),
            position,
            nonce,
        },
        EngineMsg::SetEngineParams {
            salt: 1,
            params: Box::new(EngineParamsArgs::defaults_for_tests()),
            ref_oracle: Some(actor(4, 5)),
        },
        EngineMsg::ObserveFeed {
            config: id(3),
            collateral_def: id(4),
            now_ms: 5,
        },
        EngineMsg::AssertCapAtMost { global_cap: 3 },
        EngineMsg::AssertSystemIdle,
        EngineMsg::SettleDebt {
            config: id(3),
            amount: 4,
        },
        EngineMsg::System {
            op: SystemOp::CheckSolvency,
            now_ms: 5,
            ctx: Box::new(SystemCtx::from_config(
                id(3),
                &crate::state::fixtures::config(),
            )),
        },
    ];
    assert_eq!(internals.len(), 21, "variants 5..=25");
    assert_indices(&internals, usize::from(SEIZED_INDEX));
}

/// The surplus auction's variants 26..=33 (Plan 6 Task 4), pinned one by one.
#[test]
fn surplus_variant_indices_are_fixed() {
    let (config, nonce, now_ms) = (id(1), 2, 3);
    let ctx = || {
        Box::new(SystemCtx::from_config(
            config,
            &crate::state::fixtures::config(),
        ))
    };
    let surplus = [
        EngineMsg::StartSurplus {
            config,
            nonce,
            now_ms,
        },
        EngineMsg::TakeSurplus {
            taker: id(4),
            config,
            nonce,
            amount_coin: 5,
            max_protocol_in: 6,
            now_ms,
        },
        EngineMsg::CloseSurplus {
            config,
            nonce,
            now_ms,
        },
        EngineMsg::SurplusOpen {
            config,
            nonce,
            lot: 4,
            start_ms: 5,
            ref_ray: 6,
        },
        EngineMsg::SurplusTake {
            taker: id(4),
            amount_coin: 5,
            max_protocol_in: 6,
            now_ms,
            ctx: ctx(),
        },
        EngineMsg::SurplusClose { now_ms, ctx: ctx() },
        EngineMsg::SurplusTaken {
            config,
            nonce,
            protocol_burned: 4,
            ended: true,
            now_ms,
        },
        EngineMsg::SurplusReturned {
            config,
            nonce,
            amount: 4,
            now_ms,
        },
    ];
    assert_indices(&surplus, 26);
}

#[test]
fn probe_kind_indices_are_fixed() {
    assert_indices(
        &[
            ProbeKind::Config,
            ProbeKind::Collateral,
            ProbeKind::Auction,
            ProbeKind::Surplus,
            ProbeKind::Debt,
        ],
        0,
    );
}

/// The debt auction's variants 34..=42 (Plan 6 Task 5), pinned one by one.
#[test]
fn debt_variant_indices_are_fixed() {
    let (config, nonce, now_ms) = (id(1), 2, 3);
    let ctx = || {
        Box::new(SystemCtx::from_config(
            config,
            &crate::state::fixtures::config(),
        ))
    };
    let (taker, amount_coin, min_protocol_out) = (id(4), 5, 6);
    let debt = [
        EngineMsg::StartDebt {
            config,
            nonce,
            now_ms,
        },
        EngineMsg::TakeDebt {
            taker,
            config,
            nonce,
            amount_coin,
            min_protocol_out,
            now_ms,
        },
        EngineMsg::CloseDebt {
            config,
            nonce,
            now_ms,
        },
        EngineMsg::DebtOpen {
            config,
            nonce,
            lot: 4,
            start_amount: 5,
            start_ms: 6,
        },
        EngineMsg::DebtTake {
            taker,
            amount_coin,
            min_protocol_out,
            now_ms,
            ctx: ctx(),
        },
        EngineMsg::DebtClose { now_ms, ctx: ctx() },
        EngineMsg::DebtTaken {
            config,
            nonce,
            filled: 4,
            minted: 5,
            ended: true,
            now_ms,
        },
        EngineMsg::DebtClosed {
            config,
            nonce,
            lot_left: 4,
            now_ms,
        },
        EngineMsg::Mint {
            nonce,
            to: id(4),
            amount: 5,
        },
    ];
    assert_indices(&debt, 34);
}

/// `check_solvency`'s variants 43..=44 (Plan 6 Task 6), pinned one by one.
#[test]
fn solvency_variant_indices_are_fixed() {
    let solvency = [
        EngineMsg::CheckSolvency {
            config: id(1),
            now_ms: 2,
        },
        EngineMsg::Freeze,
    ];
    assert_indices(&solvency, 43);
    assert_eq!(borsh::to_vec(&EngineMsg::Freeze).expect("encodes"), [44]);
}

#[test]
fn system_op_indices_are_fixed() {
    assert_indices(
        &[
            SystemOp::SettleDebt { amount: 1 },
            SystemOp::StartSurplus { nonce: 2 },
            SystemOp::StartDebt { nonce: 3 },
            SystemOp::CheckSolvency,
        ],
        0,
    );
}

#[test]
fn clock_assert_matches_the_clock_program() {
    let ours = ClockMsg::AssertTimestamp {
        at_least: 5,
        at_most: 9,
    };
    let theirs = clock_core::Message::AssertTimestamp {
        at_least: 5,
        at_most: 9,
    };
    assert_eq!(
        borsh::to_vec(&ours).expect("encodes"),
        borsh::to_vec(&theirs).expect("encodes")
    );
}

#[test]
fn seized_index_matches_the_host_encoding() {
    let (position, owner) = (id(1), id(2));
    let ours = EngineMsg::Seized {
        position,
        owner,
        collateral: 100,
        normalized_debt: 60,
        nominal_debt: 61,
    };
    let mut host = vec![mock_host_core::SEIZED_INDEX];
    host.extend(
        borsh::to_vec(&mock_host_core::Seized {
            position,
            owner,
            collateral: 100,
            normalized_debt: 60,
            nominal_debt: 61,
        })
        .expect("encodes"),
    );
    assert_eq!(SEIZED_INDEX, mock_host_core::SEIZED_INDEX);
    assert_eq!(borsh::to_vec(&ours).expect("encodes"), host);
    assert_eq!(EngineMsg::try_from_slice(&host).expect("decodes"), ours);
}

/// `SetSlack` (Plan 7 Task 2) is appended after `Freeze`: variant 45.
#[test]
fn set_slack_round_trips_at_index_45() {
    let msg = EngineMsg::SetSlack {
        salt: 7,
        slack_ms: 30_000,
    };
    let bytes = borsh::to_vec(&msg).expect("encodes");
    assert_eq!(bytes.first(), Some(&45));
    assert_eq!(
        borsh::from_slice::<EngineMsg>(&bytes).expect("decodes"),
        msg
    );
}

/// `SlackUpdate` (Plan 7 fix round 1) is appended after `SetSlack`: variant 46.
#[test]
fn slack_update_round_trips_at_index_46() {
    let msg = EngineMsg::SlackUpdate { slack_ms: 30_000 };
    let bytes = borsh::to_vec(&msg).expect("encodes");
    assert_eq!(bytes.first(), Some(&46));
    assert_eq!(
        borsh::from_slice::<EngineMsg>(&bytes).expect("decodes"),
        msg
    );
}
