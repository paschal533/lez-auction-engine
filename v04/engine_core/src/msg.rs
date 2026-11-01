//! The engine's message set (design note §4.2). Variant order is part of the wire format: the
//! root variants come first, in the order of the instruction table, then the internal ones. New
//! variants are only ever appended. Replies to `ReadState` arrive as LEZ `StateReply`s, not as
//! `EngineMsg`, so the Read block needs no variant of its own.

use auction_engine_params::args::{CollateralArgs, EngineParamsArgs};
use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::{AccountId, Actor};

use crate::state::{CollateralState, ObserveCtx};

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
    /// 1. Sets (or replaces) a collateral type's parameters, then observes the feed with the new
    ///    admin flag at the clock claim `now_ms` (row 2).
    SetCollateral {
        salt: u64,
        collateral_def: AccountId,
        args: CollateralArgs,
        now_ms: u64,
    },
    /// 2. Liquidates `position`; `nonce` is the expected auction number, `now_ms` the clock claim.
    Liquidate {
        config: AccountId,
        collateral_def: AccountId,
        position: AccountId,
        nonce: u64,
        now_ms: u64,
    },
    /// 3. A bid on the auction `(config, position, nonce)`, root at the collateral actor
    ///    `collateral_actor(E, config, collateral_def)`. `bidder` pays the charge from, and is paid
    ///    the collateral to, its own holding `(bidder, T)`, so it must sign (rows 5-6).
    Bid {
        bidder: AccountId,
        config: AccountId,
        collateral_def: AccountId,
        position: AccountId,
        nonce: u64,
        max_debt_in: u128,
        min_collateral_out: u128,
        now_ms: u64,
    },
    /// 4. The owner claims the leftover collateral of a settled auction, root at the owner's
    ///    proxy `(owner, E)` (row 8).
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
        nonce: u64,
        keeper: AccountId,
        now_ms: u64,
        /// The bid price `P` (v0.2.4 `bid_price`) of this observation.
        price_ray: u128,
        /// The collateral actor's state after this observation.
        collateral: Box<CollateralState>,
    },
    /// 7. Config to `position`'s record: auction number `nonce`; the record asserts
    ///    `nonce == count`, then increments.
    Bump { position: AccountId, nonce: u64 },
    /// 8. Config to auction (empty state): creates it in `AwaitingSeize`, bound to `host`, with
    ///    what the host's `Seized` report is quoted against.
    Open {
        position: AccountId,
        nonce: u64,
        keeper: AccountId,
        host: Actor,
        token: AccountId,
        stable_def: AccountId,
        now_ms: u64,
        price_ray: u128,
        collateral: Box<CollateralState>,
        global_cap: u128,
        global_reserved: u128,
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
    /// 11. Auction to accounting `accounting_actor(E, config)`: the auction `(config, position,
    ///     nonce)` settled with these cumulative totals and this chain end (v0.2.4
    ///     `record_settled`).
    Settled {
        config: AccountId,
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
    /// 13. Collateral actor to auction: a bid with its observation context (rows 5-6): the bid
    ///     price `P`, the clock claim, the type's pause total after this (live) observation, and
    ///     the type's current `min_bid` and `chost` (v0.2.4 quotes with the current parameters).
    BidCtx {
        bidder: AccountId,
        position: AccountId,
        nonce: u64,
        max_debt_in: u128,
        min_collateral_out: u128,
        price_ray: u128,
        now_ms: u64,
        paused_total_ms: u64,
        min_bid: u128,
        chost: u128,
    },
    /// 14. Sentinel of the Read block: the replies are in, consume them.
    Proceed,
    /// 15. Config to collateral actor (COLL seed): writes the validated `SetCollateral` with the
    ///     config's observe context, and sends nothing; `Observe` follows unseeded.
    Configure {
        collateral_def: AccountId,
        args: CollateralArgs,
        ctx: ObserveCtx,
    },
    /// 16. A seeded probe (ruling R-8b): its handler only asserts the receiver holds a `kind`
    ///     state and sends nothing, so the PDA seed it carries is inherited by no other message.
    ///     The real message follows it unseeded, as its next sibling.
    Probe { kind: ProbeKind },
    /// 17. After the seeded `Init` created the config: proxy to config, the config's reads and
    ///     the accounting's creation; config to accounting (ACCT seed), the creation itself.
    InitReads,
    /// 18. After the seeded `Configure` wrote the parameters: config to collateral actor, observe
    ///     at the clock claim `now_ms`.
    Observe { now_ms: u64 },
    /// 19. Root at the config `K = (config, E)`, permissionless (row 7; v0.2.4 `settle_empty`):
    ///     settles the auction `(config, position, nonce)` whose lot is empty. `K` sends a seeded
    ///     `Probe { Auction }`, then this message unseeded to the auction, which accepts it only
    ///     from `K` and ends with the same `Settled` / `Release` tail as a bid's chain end.
    SettleEmpty {
        config: AccountId,
        position: AccountId,
        nonce: u64,
    },
}

/// What a [`EngineMsg::Probe`] expects at its receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum ProbeKind {
    /// A config; otherwise `WrongAccount("config")`.
    Config,
    /// A collateral actor; otherwise `WrongAccount("coll_config")`.
    Collateral,
    /// An auction; otherwise `WrongAccount("auction")` (v0.2.4 `load_auction`).
    Auction,
}

/// The clock program's message set (`clock_core::Message` at LEZ v0.4.0-rc1), mirrored so the
/// guest needs no clock crate; only `AssertTimestamp` is sent. Pinned by a test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum ClockMsg {
    Tick {
        timestamp: u64,
        block_id: u64,
    },
    Record {
        block_id: u64,
        timestamp: u64,
    },
    /// The clock asserts its timestamp lies in `[at_least, at_most]`.
    AssertTimestamp {
        at_least: u64,
        at_most: u64,
    },
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
}
