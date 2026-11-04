//! The engine's message set (design note §4.2). Variant order is part of the wire format: the
//! root variants come first, in the order of the instruction table, then the internal ones. New
//! variants are only ever appended. Replies to `ReadState` arrive as LEZ `StateReply`s, not as
//! `EngineMsg`, so the Read block needs no variant of its own.

use auction_engine_params::args::{CollateralArgs, EngineParamsArgs};
use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::{AccountId, Actor};

use crate::{
    state::{CollateralState, ObserveCtx},
    system_state::{SystemCtx, SystemOp},
};

/// Borsh variant index of [`EngineMsg::Seized`] (ruling R-P1); the mock host encodes the same.
pub const SEIZED_INDEX: u8 = 5;

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum EngineMsg {
    // Root variants (0-4): sent by a transaction, at a proxy `(signer, E)` or at the actor itself.
    /// 0. Creates the config at the config actor, then `Accounting` and the protocol token's
    ///    definition `(protocol_def(E, K), T)` with minter `K`, its genesis supply credited to
    ///    `protocol_genesis.to` (none without a genesis). The config names that definition only
    ///    while `params.protocol_token_enabled`; `ref_oracle` fills the reference-oracle slot,
    ///    present iff `params.ref_oracle_enabled`.
    Init {
        salt: u64,
        params: Box<EngineParamsArgs>,
        host: Actor,
        token: AccountId,
        stable_def: AccountId,
        oracle: Actor,
        clock: Actor,
        slack_ms: u64,
        ref_oracle: Option<Actor>,
        protocol_genesis: Option<ProtocolGenesis>,
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
    /// 20. Root at the admin's proxy (row 1; v0.2.4 `set_engine_params`): a seeded
    ///     `Probe { Config }`, then this message unseeded to the config, which validates and writes
    ///     the parameters, then asks the collateral actor of its recorded type (if any, behind a
    ///     seeded `Probe { Collateral }`) for `AssertCapAtMost`, and, if the protocol token, the
    ///     reference oracle or the system parameters change, the accounting for
    ///     `AssertSystemIdle`. `slack_ms` and the actors fixed at `Init` are not settable.
    SetEngineParams {
        salt: u64,
        params: Box<EngineParamsArgs>,
        ref_oracle: Option<Actor>,
    },
    /// 21. Root at `collateral_actor(E, config, collateral_def)`, permissionless (row 3; v0.2.4
    ///     `observe_feed`): observes the feed at the clock claim `now_ms` and records the pause
    ///     clock; a stale feed is recorded, not refused.
    ObserveFeed {
        config: AccountId,
        collateral_def: AccountId,
        now_ms: u64,
    },
    /// 22. Config to collateral actor: the type's cap must not exceed `global_cap`
    ///     (`TypeCapAboveGlobal`). Sends nothing.
    AssertCapAtMost { global_cap: u128 },
    /// 23. Config to accounting: no surplus or debt auction may be live
    ///     (`ProtocolTokenLocked`). Sends nothing.
    AssertSystemIdle,
    /// 24. Root at the config `K = (config, E)`, permissionless (row 9; v0.2.4 `settle_debt`):
    ///     `K` sends `System { SettleDebt }` to its accounting, which reads the buffer and burns
    ///     `min(amount, buffer, unbacked - debt_on_auction)` from it.
    SettleDebt { config: AccountId, amount: u128 },
    /// 25. Config to its accounting `accounting_actor(E, config)`: run `op` with the config's
    ///     context at the clock claim `now_ms` (asserted by `K` for every op but `SettleDebt`).
    ///     The accounting reads the buffer (and the reference oracle), then acts at `Proceed`.
    System {
        op: SystemOp,
        now_ms: u64,
        ctx: Box<SystemCtx>,
    },
    /// 26. Root at the config `K`, permissionless (row 10; v0.2.4 `start_surplus_auction`): `K`
    ///     asserts the clock claim and sends `System { StartSurplus { nonce } }` to its
    ///     accounting, which reads the buffer (and the reference oracle), settles, moves the lot
    ///     into the auction's vault and opens `surplus_actor(E, config, nonce)`.
    StartSurplus {
        config: AccountId,
        nonce: u64,
        now_ms: u64,
    },
    /// 27. Root at `K` (row 11; v0.2.4 `take_surplus`): `taker` pays protocol tokens from its own
    ///     holding (so it signs) for up to `amount_coin` stablecoin. `K` asserts the clock,
    ///     probes the auction and sends it `SurplusTake`.
    TakeSurplus {
        taker: AccountId,
        config: AccountId,
        nonce: u64,
        amount_coin: u128,
        max_protocol_in: u128,
        now_ms: u64,
    },
    /// 28. Root at `K`, permissionless (row 12; v0.2.4 `close_surplus_auction`): after the TTL
    ///     the vault's whole balance returns to the buffer. `K` asserts the clock, probes the
    ///     auction and sends it `SurplusClose`.
    CloseSurplus {
        config: AccountId,
        nonce: u64,
        now_ms: u64,
    },
    /// 29. Accounting to `surplus_actor(E, config, nonce)` (SURP seed, empty state; a leaf):
    ///     creates the live auction.
    SurplusOpen {
        config: AccountId,
        nonce: u64,
        lot: u128,
        start_ms: u64,
        ref_ray: u128,
    },
    /// 30. Config to its surplus auction: a take with the config's context.
    SurplusTake {
        taker: AccountId,
        amount_coin: u128,
        max_protocol_in: u128,
        now_ms: u64,
        ctx: Box<SystemCtx>,
    },
    /// 31. Config to its surplus auction: a close with the config's context.
    SurplusClose { now_ms: u64, ctx: Box<SystemCtx> },
    /// 32. Surplus auction to its accounting: `protocol_burned` burned; `ended` if the lot is
    ///     gone.
    SurplusTaken {
        config: AccountId,
        nonce: u64,
        protocol_burned: u128,
        ended: bool,
        now_ms: u64,
    },
    /// 33. Surplus auction to its accounting: closed, `amount` returned to the buffer.
    SurplusReturned {
        config: AccountId,
        nonce: u64,
        amount: u128,
        now_ms: u64,
    },
    /// 34. Root at `K`, permissionless (row 13; v0.2.4 `start_debt_auction`): `K` asserts the
    ///     clock and sends `System { StartDebt { nonce } }`; the accounting reads the buffer (and
    ///     the reference oracle), settles, reserves the lot and opens `debt_actor(E, K, nonce)`.
    StartDebt {
        config: AccountId,
        nonce: u64,
        now_ms: u64,
    },
    /// 35. Root at `K` (row 14; v0.2.4 `take_debt`): `taker` burns up to `amount_coin` of its own
    ///     stablecoin (so it signs) for at least `min_protocol_out` freshly minted protocol tokens.
    TakeDebt {
        taker: AccountId,
        config: AccountId,
        nonce: u64,
        amount_coin: u128,
        min_protocol_out: u128,
        now_ms: u64,
    },
    /// 36. Root at `K`, permissionless (row 15; v0.2.4 `close_debt_auction`): after the TTL the
    ///     untaken lot's reservation is released.
    CloseDebt {
        config: AccountId,
        nonce: u64,
        now_ms: u64,
    },
    /// 37. Accounting to `debt_actor(E, config, nonce)` (DEBT seed, empty state; a leaf).
    DebtOpen {
        config: AccountId,
        nonce: u64,
        lot: u128,
        start_amount: u128,
        start_ms: u64,
    },
    /// 38. Config to its debt auction: a take with the config's context.
    DebtTake {
        taker: AccountId,
        amount_coin: u128,
        min_protocol_out: u128,
        now_ms: u64,
        ctx: Box<SystemCtx>,
    },
    /// 39. Config to its debt auction: a close with the config's context.
    DebtClose { now_ms: u64, ctx: Box<SystemCtx> },
    /// 40. Debt auction to its accounting: `filled` burned, `minted` minted; `ended` if the lot
    ///     is gone.
    DebtTaken {
        config: AccountId,
        nonce: u64,
        filled: u128,
        minted: u128,
        ended: bool,
        now_ms: u64,
    },
    /// 41. Debt auction to its accounting: closed, `lot_left` released.
    DebtClosed {
        config: AccountId,
        nonce: u64,
        lot_left: u128,
        now_ms: u64,
    },
    /// 42. Debt auction `nonce` to its config: mint `amount` to `to`. `K` accepts it only from
    ///     `debt_actor(E, K, nonce)` and sends `MintTo` to its protocol definition (minter `K`).
    Mint {
        nonce: u64,
        to: AccountId,
        amount: u128,
    },
    /// 43. Root at `K`, permissionless (row 16; v0.2.4 `check_solvency`): `K` asserts the clock
    ///     claim and sends `System { CheckSolvency }`; the accounting reads the buffer (and the
    ///     reference oracle), settles what it can, then refuses while a debt auction is live
    ///     (`RecoveryInProgress`) or, on a `Freeze` decision, sends [`EngineMsg::Freeze`].
    CheckSolvency { config: AccountId, now_ms: u64 },
    /// 44. Accounting to its config `K`: freeze new debt at the host. `K` accepts it only from
    ///     `accounting_actor(E, K)` and sends `EngineFreezeNewDebt` to its bound host.
    Freeze,
}

/// The protocol token's genesis at `Init`: `supply` credited to `to`'s holding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ProtocolGenesis {
    pub to: AccountId,
    pub supply: u128,
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
    /// A surplus auction; otherwise `WrongAccount("surplus_auction")` (v0.2.4 `load_surplus`).
    Surplus,
    /// A debt auction; otherwise `WrongAccount("debt_auction")` (v0.2.4 `load_debt`).
    Debt,
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
#[path = "msg_tests.rs"]
mod tests;
