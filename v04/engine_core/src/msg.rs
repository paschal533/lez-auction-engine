//! The engine's message set (design note §4.2). Variant order is part of the wire format: the
//! root variants come first, in the order of the instruction table, then the internal ones. New
//! variants are only ever appended. Replies to `ReadState` arrive as LEZ `StateReply`s, not as
//! `EngineMsg`, so the Read block needs no variant of its own.

use auction_engine_params::args::{CollateralArgs, EngineParamsArgs};
use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::{AccountId, Actor};

use crate::state::CollateralState;

/// Borsh variant index of [`EngineMsg::Seized`] (ruling R-P1); the mock host encodes the same.
pub const SEIZED_INDEX: u8 = 5;

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum EngineMsg {
    // Root variants (0-4): sent by a transaction, at a proxy `(signer, E)` or at the actor itself.
    /// 0. Creates the config at the config actor, then `Accounting`.
    Init {
        salt: u64,
        params: Box<EngineParamsArgs>,
        host: Actor,
        token: AccountId,
        stable_def: AccountId,
        oracle: Actor,
        clock: Actor,
        slack_ms: u64,
    },
    /// 1. Sets (or replaces) a collateral type's parameters.
    SetCollateral {
        salt: u64,
        collateral_def: AccountId,
        args: CollateralArgs,
    },
    /// 2. Starts the liquidation of `position`; `now_ms` is the caller's clock claim.
    Liquidate {
        config: AccountId,
        collateral_def: AccountId,
        position: AccountId,
        now_ms: u64,
    },
    /// 3. A bid on the auction `(config, position, nonce)`.
    Bid {
        config: AccountId,
        collateral_def: AccountId,
        position: AccountId,
        nonce: u64,
        max_debt_in: u128,
        min_collateral_out: u128,
        now_ms: u64,
    },
    /// 4. The owner claims the leftover collateral of a settled auction.
    ClaimLeftover {
        config: AccountId,
        position: AccountId,
        nonce: u64,
    },

    // Internal variants (5 and up).
    /// 5. Host to auction: the position was seized (ruling R-P1). From the actor `(requested position, bound host program)` only.
    Seized {
        position: AccountId,
        owner: AccountId,
        collateral: u128,
        normalized_debt: u128,
        nominal_debt: u128,
    },
    /// 6. Collateral actor to config: observation done, the liquidation may go ahead (row 4 context).
    LiqCtx {
        position: AccountId,
        keeper: AccountId,
        now_ms: u64,
        price_ray: u128,
        paused_total_ms: u64,
        collateral: Box<CollateralState>,
    },
    /// 7. Config to record: the next auction of the position is number `nonce`.
    Bump { nonce: u64 },
    /// 8. Config to auction (empty state): creates it in `AwaitingSeize`, bound to `host`.
    Open {
        position: AccountId,
        nonce: u64,
        keeper: AccountId,
        host: Actor,
        now_ms: u64,
        price_ray: u128,
        paused_total_ms: u64,
        collateral: Box<CollateralState>,
    },
    /// 9. Auction to collateral actor and config: reserve `amount` of cap for the auction.
    Reserve {
        position: AccountId,
        nonce: u64,
        amount: u128,
    },
    /// 10. Auction to collateral actor and config: release `amount` of cap at chain end.
    Release {
        position: AccountId,
        nonce: u64,
        amount: u128,
    },
    /// 11. Auction to accounting: the auction settled with these totals.
    Settled {
        position: AccountId,
        nonce: u64,
        debt_raised: u128,
        penalty_raised: u128,
        shortfall: u128,
        release: u128,
        leftover: u128,
    },
    /// 12. Sentinel, the last message of a liquidation: the conversation must have reached its end.
    Finish,
    /// 13. Collateral actor to auction: a bid with its observation context (rows 5-6).
    BidCtx {
        position: AccountId,
        nonce: u64,
        max_debt_in: u128,
        min_collateral_out: u128,
        price_ray: u128,
        redemption_ray: u128,
        now_ms: u64,
        paused_total_ms: u64,
    },
    /// 14. Sentinel of the Read block: the replies are in, consume them.
    Proceed,
}

#[cfg(test)]
mod tests {
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
            },
            EngineMsg::SetCollateral {
                salt: 1,
                collateral_def: id(1),
                args: CollateralArgs::defaults_for_tests(),
            },
            EngineMsg::Liquidate {
                config: id(1),
                collateral_def: id(2),
                position: id(3),
                now_ms: 4,
            },
            EngineMsg::Bid {
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
        for (index, message) in roots.iter().enumerate() {
            let bytes = borsh::to_vec(message).expect("encodes");
            assert_eq!(
                usize::from(*bytes.first().expect("non-empty")),
                index,
                "{message:?}"
            );
        }
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
}
