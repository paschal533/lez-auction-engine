//! Proxy block (design note §3, §4.1): a root message is sent to the signer's own engine actor
//! `(signer, E)`, where `is_authorized` holds only if the signer signed (a root message carries no
//! PDA seed). The stateless proxy forwards the message, and the receiver checks it came from
//! exactly `(signer, E)`:
//!
//! - `Init` / `SetCollateral` / `SetEngineParams` / `SetSlack` (admin) go to the signer's config
//!   `config_actor(E, signer, salt)`;
//! - `Liquidate` (keeper, row 4) goes to the named collateral actor
//!   `collateral_actor(E, config, def)`, which takes the keeper from the proxy's account;
//! - `ClaimLeftover` (owner, row 8) goes to the named auction, which accepts it only from
//!   `(owner, E)`;
//! - `SettleEmpty` (permissionless, row 7) is rooted at the named config itself, which forwards it
//!   to the auction.
//!
//! The system roots (`SettleDebt`, row 9; `StartSurplus`, row 10) are rooted at the named config
//! too, and handled by `system` (the config hands them to its accounting, not to an auction);
//! `TakeSurplus` / `CloseSurplus` (rows 11-12) by `surplus`, behind a seeded `Probe { Surplus }`;
//! `StartDebt` (row 13) by `debt` through `system`, and `TakeDebt` / `CloseDebt` (rows 14-15) by
//! `debt`, behind a seeded `Probe { Debt }`.
//!
//! Every forward to an auction goes behind a seeded `Probe { Auction }` (ruling R-9b).
//!
//! Seeded probes (ruling R-8b): LEZ hands a message's grants down to every message its receiver
//! sends, so a PDA seed goes only on a message whose handler sends nothing: the seeded `Init`
//! (which creates the config) or a seeded `Probe` (which asserts the PDA exists, so an absent one
//! fails with the engine's own error, R-7a). The message that continues the conversation follows
//! unseeded, as the next sibling; siblings share no grants.

use auction_engine_params::EngineError;
use engine_core::{
    actors::{
        auction_actor, auction_seed, collateral_actor, collateral_seed, config_actor, config_seed,
        debt_actor, debt_seed, surplus_actor, surplus_seed,
    },
    check::fail,
    msg::{EngineMsg, ProbeKind},
    state::{AuctionState, CollateralState, ConfigState},
    system_state::{DebtAuctionState, SurplusAuctionState},
};
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, SyncMessage},
};

/// A root message: the proxied ones are forwarded; anything else is left to the dispatch, whose
/// handlers refuse a message without a sender.
pub fn root(input: &ReceiveInput, message: &EngineMsg) -> Option<Response> {
    match message {
        EngineMsg::Init { salt, .. }
        | EngineMsg::SetCollateral { salt, .. }
        | EngineMsg::SetEngineParams { salt, .. }
        | EngineMsg::SetSlack { salt, .. } => Some(forward_admin(input, message, *salt)),
        EngineMsg::Liquidate {
            config,
            collateral_def,
            ..
        } => Some(forward_liquidate(input, message, *config, *collateral_def)),
        EngineMsg::ClaimLeftover {
            config,
            position,
            nonce,
        } => Some(forward_claim(input, message, *config, *position, *nonce)),
        EngineMsg::SettleEmpty {
            config,
            position,
            nonce,
        } => Some(forward_settle_empty(
            input, message, *config, *position, *nonce,
        )),
        _ => None,
    }
}

/// Forwards a root `Init` / `SetCollateral` / `SetEngineParams` / `SetSlack` to the signer's
/// config.
///
/// `Init`: the seeded `Init` creates the config (and sends nothing), then the unseeded `InitReads`
/// runs its reads and creates the accounting and the protocol definition. `SetCollateral` /
/// `SetEngineParams` / `SetSlack`: a seeded `Probe` makes a missing config fail with the engine's
/// v0.2.4 `WrongAccount("config")` rather than LEZ's admission error, then the unseeded message
/// follows.
fn forward_admin(input: &ReceiveInput, message: &EngineMsg, salt: u64) -> Response {
    if !input.is_authorized {
        fail(EngineError::NotAdmin);
    }
    let engine = input.receiver.program_account_id;
    let signer = input.receiver.account_id;
    let config = config_actor(engine, signer, salt);
    let seed = config_seed(signer, salt);
    let response = Response::keep_state();
    if matches!(message, EngineMsg::Init { .. }) {
        return response
            .send(SyncMessage::new(config, message).with_pda_seeds([seed]))
            .send_sync(config, &EngineMsg::InitReads);
    }
    let probe = EngineMsg::Probe {
        kind: ProbeKind::Config,
    };
    response
        .send(SyncMessage::new(config, &probe).with_pda_seeds([seed]))
        .send_sync(config, message)
}

/// Forwards the keeper's root `Liquidate` to the collateral actor (v0.2.4: the keeper must sign,
/// `WrongAccount("keeper")`), behind a seeded `Probe` so that an unconfigured collateral type
/// fails with the engine's `WrongAccount("coll_config")` (R-7a) without the COLL grant reaching
/// the liquidation's subtree (R-8b).
fn forward_liquidate(
    input: &ReceiveInput,
    message: &EngineMsg,
    config: AccountId,
    collateral_def: AccountId,
) -> Response {
    if !input.is_authorized {
        fail(EngineError::WrongAccount("keeper"));
    }
    let engine = input.receiver.program_account_id;
    let collateral = collateral_actor(engine, config, collateral_def);
    let probe = EngineMsg::Probe {
        kind: ProbeKind::Collateral,
    };
    Response::keep_state()
        .send(
            SyncMessage::new(collateral, &probe)
                .with_pda_seeds([collateral_seed(config, collateral_def)]),
        )
        .send_sync(collateral, message)
}

/// The seeded leaf `Probe { Auction }` for `auction_actor(engine, config, position, nonce)`
/// (ruling R-9b): an absent auction fails with v0.2.4's `WrongAccount("auction")`, not LEZ's
/// admission error. The caller sends the real message unseeded as the next sibling.
pub fn auction_probe(
    engine: AccountId,
    config: AccountId,
    position: AccountId,
    nonce: u64,
) -> (Actor, SyncMessage) {
    let auction = auction_actor(engine, config, position, nonce);
    let probe = EngineMsg::Probe {
        kind: ProbeKind::Auction,
    };
    let seeded =
        SyncMessage::new(auction, &probe).with_pda_seeds([auction_seed(config, position, nonce)]);
    (auction, seeded)
}

/// The seeded leaf `Probe { Surplus }` for `surplus_actor(engine, config, nonce)`: an absent
/// surplus auction fails with v0.2.4's `WrongAccount("surplus_auction")`. The caller sends the
/// real message unseeded as the next sibling.
pub fn surplus_probe(engine: AccountId, config: AccountId, nonce: u64) -> (Actor, SyncMessage) {
    let surplus = surplus_actor(engine, config, nonce);
    let probe = EngineMsg::Probe {
        kind: ProbeKind::Surplus,
    };
    let seeded = SyncMessage::new(surplus, &probe).with_pda_seeds([surplus_seed(config, nonce)]);
    (surplus, seeded)
}

/// The seeded leaf `Probe { Debt }` for `debt_actor(engine, config, nonce)`: an absent debt
/// auction fails with v0.2.4's `WrongAccount("debt_auction")`. The caller sends the real message
/// unseeded as the next sibling.
pub fn debt_probe(engine: AccountId, config: AccountId, nonce: u64) -> (Actor, SyncMessage) {
    let debt = debt_actor(engine, config, nonce);
    let probe = EngineMsg::Probe {
        kind: ProbeKind::Debt,
    };
    let seeded = SyncMessage::new(debt, &probe).with_pda_seeds([debt_seed(config, nonce)]);
    (debt, seeded)
}

/// Forwards the owner's root `ClaimLeftover` to the auction (v0.2.4: the owner must sign,
/// `NotOwner`), behind the seeded auction probe; the auction checks the sender is exactly
/// `(owner, E)`.
fn forward_claim(
    input: &ReceiveInput,
    message: &EngineMsg,
    config: AccountId,
    position: AccountId,
    nonce: u64,
) -> Response {
    if !input.is_authorized {
        fail(EngineError::NotOwner);
    }
    let engine = input.receiver.program_account_id;
    let (auction, probe) = auction_probe(engine, config, position, nonce);
    Response::keep_state()
        .send(probe)
        .send_sync(auction, message)
}

/// A root `SettleEmpty` at the named config `(config, E)` (permissionless, row 7): the receiver
/// must be that config; then the seeded auction probe, then the message unseeded to the auction,
/// which accepts it only from this config.
fn forward_settle_empty(
    input: &ReceiveInput,
    message: &EngineMsg,
    config: AccountId,
    position: AccountId,
    nonce: u64,
) -> Response {
    let engine = input.receiver.program_account_id;
    if input.receiver != Actor::new(config, engine)
        || ConfigState::decode(input.pre_state.as_ref()).is_none()
    {
        fail(EngineError::WrongAccount("config"));
    }
    let (auction, probe) = auction_probe(engine, config, position, nonce);
    Response::keep_state()
        .send(probe)
        .send_sync(auction, message)
}

/// A seeded `Probe`: the receiver must hold a `kind` state. Writes and sends nothing.
pub fn probe(input: &ReceiveInput, kind: ProbeKind) -> Response {
    let state = input.pre_state.as_ref();
    match kind {
        ProbeKind::Config if ConfigState::decode(state).is_none() => {
            fail(EngineError::WrongAccount("config"))
        }
        ProbeKind::Collateral if CollateralState::decode(state).is_none() => {
            fail(EngineError::WrongAccount("coll_config"))
        }
        ProbeKind::Auction if AuctionState::decode(state).is_none() => {
            fail(EngineError::WrongAccount("auction"))
        }
        ProbeKind::Surplus if SurplusAuctionState::decode(state).is_none() => {
            fail(EngineError::WrongAccount("surplus_auction"))
        }
        ProbeKind::Debt if DebtAuctionState::decode(state).is_none() => {
            fail(EngineError::WrongAccount("debt_auction"))
        }
        ProbeKind::Config
        | ProbeKind::Collateral
        | ProbeKind::Auction
        | ProbeKind::Surplus
        | ProbeKind::Debt => Response::keep_state(),
    }
}
