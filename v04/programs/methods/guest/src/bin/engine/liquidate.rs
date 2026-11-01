//! `liquidate` past the Observe block (design note §4.2 row 4; v0.2.4 `liquidate.rs`).
//!
//! The config `K`, on `LiqCtx` from exactly its collateral actor, sends in this order:
//! 1. `Bump { position, nonce }` to the record (LIQREC seed): `nonce == count`, then `count + 1`;
//! 2. `Open` to the auction `auction_actor(E, K, position, nonce)` (AUCTION seed);
//! 3. `LiquidatePosition { custody: auction vault, reply_to: auction }` to the host (which checks
//!    `from == (K, E)`, sweeps the position vault to custody, then reports `Seized`);
//! 4. `Finish` to the auction: depth-first order runs it after every earlier subtree, so it sees
//!    the whole conversation (Sentinel).
//!
//! `Reserve` (after the quote) and `Release` (at the chain end of a bid) from the auction land at
//! the collateral actor and the config, each accepting only the auction re-derived from its own
//! address and the message's `(position, nonce)`.

use auction_engine_params::EngineError;
use engine_core::{
    actors::{
        auction_actor, auction_seed, collateral_actor, record_actor, record_seed, vault_holding,
    },
    check::{expect_from, fail},
    msg::EngineMsg,
    state::{CollateralState, ConfigState, LiquidationRecordState, TAG_LIQREC},
};
use lee_core::{
    account::AccountId,
    program::{ReceiveInput, Response, SyncMessage},
};
use mock_host_core::HostMsg;

use crate::admin::{engine, engine_sender};

/// `LiqCtx` without the collateral state.
pub struct LiqCtx {
    pub position: AccountId,
    pub nonce: u64,
    pub keeper: AccountId,
    pub now_ms: u64,
    pub price_ray: u128,
}

/// At the config: the observed liquidation from exactly `collateral_actor(E, K, def)`.
pub fn on_liq_ctx(input: &ReceiveInput, c: &LiqCtx, collateral: CollateralState) -> Response {
    let (e, k) = (engine(input), input.receiver);
    let from_collateral = collateral_actor(e, k.account_id, collateral.collateral_definition_id);
    expect_from(
        input,
        from_collateral,
        EngineError::WrongAccount("coll_config"),
    );
    let cfg = ConfigState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("config")));
    if cfg.pending_read.is_some() || collateral.config != k.account_id {
        fail(EngineError::WrongAccount("config"));
    }
    let auction = auction_actor(e, k.account_id, c.position, c.nonce);
    let vault = vault_holding(e, cfg.token, auction.account_id);
    let bump = SyncMessage::new(
        record_actor(e, k.account_id, c.position),
        &EngineMsg::Bump {
            position: c.position,
            nonce: c.nonce,
        },
    )
    .with_pda_seeds([record_seed(k.account_id, c.position)]);
    let open = EngineMsg::Open {
        position: c.position,
        nonce: c.nonce,
        keeper: c.keeper,
        host: cfg.host,
        token: cfg.token,
        stable_def: cfg.stablecoin_definition_id,
        now_ms: c.now_ms,
        price_ray: c.price_ray,
        collateral: Box::new(collateral),
        global_cap: cfg.global_cap,
        global_reserved: cfg.global_reserved,
    };
    let open = SyncMessage::new(auction, &open).with_pda_seeds([auction_seed(
        k.account_id,
        c.position,
        c.nonce,
    )]);
    let seize = HostMsg::LiquidatePosition {
        position: c.position,
        custody: vault.account_id,
        reply_to: auction,
    };
    Response::keep_state()
        .send(bump)
        .send(open)
        .send_sync(cfg.host, &seize)
        .send_sync(auction, &EngineMsg::Finish)
}

/// At `record_actor(E, K, position)`, from its config `K` (LIQREC seed): created on first use;
/// `nonce == count` (v0.2.4 `NonceMismatch`), then `count + 1` (ADR-004, ADR-020).
pub fn on_bump(input: &ReceiveInput, position: AccountId, nonce: u64) -> Response {
    let config = engine_sender(input, EngineError::WrongAccount("record"));
    if input.receiver != record_actor(engine(input), config, position) {
        fail(EngineError::WrongAccount("record"));
    }
    let current = if input.pre_state.as_ref().is_empty() {
        if !input.is_authorized {
            fail(EngineError::WrongAccount("record"));
        }
        LiquidationRecordState {
            tag: TAG_LIQREC,
            position,
            count: 0,
        }
    } else {
        LiquidationRecordState::decode(input.pre_state.as_ref())
            .filter(|record| record.position == position)
            .unwrap_or_else(|| fail(EngineError::WrongAccount("record")))
    };
    if nonce != current.count {
        fail(EngineError::NonceMismatch);
    }
    let count = current
        .count
        .checked_add(1)
        .unwrap_or_else(|| fail(EngineError::Math));
    let next = LiquidationRecordState { count, ..current };
    Response::set_state(next.encode())
}

/// Which way a reservation message moves the caps.
#[derive(Clone, Copy)]
pub enum Reservation {
    /// `Reserve` (after the liquidation quote).
    Reserve,
    /// `Release` (at the auction's chain end); more than is reserved is v0.2.4's `Math`.
    Release,
}

/// `Reserve` / `Release` at the collateral actor (type reservation) or the config (global
/// reservation), from exactly the auction `(config, position, nonce)` of the receiver's config.
pub fn on_reservation(
    input: &ReceiveInput,
    (position, nonce): (AccountId, u64),
    amount: u128,
    way: Reservation,
) -> Response {
    let e = engine(input);
    let apply = |reserved: u128| {
        match way {
            Reservation::Reserve => reserved.checked_add(amount),
            Reservation::Release => reserved.checked_sub(amount),
        }
        .unwrap_or_else(|| fail(EngineError::Math))
    };
    if let Some(coll) = CollateralState::decode(input.pre_state.as_ref()) {
        let auction = auction_actor(e, coll.config, position, nonce);
        expect_from(input, auction, EngineError::WrongAccount("auction"));
        if coll.pending.is_some() {
            fail(EngineError::WrongAccount("pending"));
        }
        let next = CollateralState {
            type_reserved: apply(coll.type_reserved),
            ..coll
        };
        return Response::set_state(next.encode());
    }
    let cfg = ConfigState::decode(input.pre_state.as_ref())
        .unwrap_or_else(|| fail(EngineError::WrongAccount("reserve")));
    let auction = auction_actor(e, input.receiver.account_id, position, nonce);
    expect_from(input, auction, EngineError::WrongAccount("auction"));
    if cfg.pending_read.is_some() {
        fail(EngineError::WrongAccount("pending"));
    }
    let next = ConfigState {
        global_reserved: apply(cfg.global_reserved),
        ..cfg
    };
    Response::set_state(next.encode())
}
