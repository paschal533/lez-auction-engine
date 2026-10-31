//! The mock CDP host (design note §5, option B): the engine-facing interface of the stablecoin,
//! built only from the design's blocks (§4.1).
//!
//! - Owner actions use the **Proxy** block: root at `(owner, H)`, forwarded to the params actor
//!   `P` only if the owner signed; `P` takes the owner from the proxy's account.
//! - `P` **Relays** to positions (POSITION seed granted on creation); a position obeys only the
//!   `P` that created it. The position sweeps its vault holding with the VAULT seed.
//! - `LiquidatePosition` is accepted only from exactly the bound engine actor. `P` **Reads** the
//!   oracle (pending request in its own state), relays `Seize` on the reply, and ends with a
//!   **Sentinel** `Finish` that fails the transaction if the read never completed.
//! - `Seize`: the vault is swept to custody first, then `Seized` goes to `reply_to`.
//!
//! The mock sends nothing to caller-chosen code except the `Seized` report and `StateReply`.
//! Errors are panics `host: <reason>`.

use borsh::BorshDeserialize;
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, StateReply, SyncMessage},
};
use mock_host_core::{
    HostMsg, Params, PendingLiquidation, Position, SeizeOrder, Seized, TAG_PARAMS, TAG_POSITION,
    is_unsafe, position_id, position_seed, vault_id, vault_seed,
};
use oracle_core::{OracleMsg, OracleState};
use token_core::TokenMsg;

lee_core::actor_entrypoint!(raw handle_message);

fn encode<T: borsh::BorshSerialize>(state: &T) -> Vec<u8> {
    borsh::to_vec(state).expect("state encodes")
}

fn host_actor(input: &ReceiveInput, account: AccountId) -> Actor {
    Actor::new(account, input.receiver.program_account_id)
}

fn load_params(input: &ReceiveInput) -> Params {
    Params::decode(input.pre_state.as_ref()).expect("host: NotParams")
}

fn load_position(input: &ReceiveInput) -> Position {
    Position::decode(input.pre_state.as_ref()).expect("host: NoPosition")
}

fn handle_message(input: &ReceiveInput) -> Response {
    // At `P`, anything from the configured oracle is the reply to our read.
    if let Some(params) = Params::decode(input.pre_state.as_ref()) {
        if input.from == Some(params.oracle) {
            return on_oracle_reply(input, params);
        }
    }
    let message = HostMsg::try_from_slice(&input.message).expect("host: BadMessage");
    match message {
        HostMsg::Init {
            admin,
            token,
            stable_def,
            collateral_def,
            oracle,
            redemption_price_ray,
            min_cr_ray,
        } => {
            assert!(input.is_authorized, "host: Unauthorized");
            assert!(
                input.pre_state.as_ref().is_empty(),
                "host: AlreadyInitialized"
            );
            assert_eq!(admin, input.receiver, "host: AdminNotParams");
            Response::set_state(encode(&Params {
                tag: TAG_PARAMS,
                admin,
                token,
                stable_def,
                collateral_def,
                oracle,
                redemption_price_ray,
                min_cr_ray,
                engine_actor: None,
                pending: None,
            }))
        }
        HostMsg::SetLiquidationEngine { engine_actor } => {
            assert!(input.from.is_none(), "host: NotRoot");
            assert!(input.is_authorized, "host: Unauthorized");
            let mut params = load_params(input);
            params.engine_actor = Some(engine_actor);
            Response::set_state(encode(&params))
        }
        HostMsg::OpenPosition { owner, collateral } => open_position(input, owner, collateral),
        HostMsg::GenerateDebt { normalized_debt } => generate_debt(input, normalized_debt),
        HostMsg::LiquidatePosition {
            position,
            custody,
            reply_to,
        } => liquidate(input, position, custody, reply_to),
        HostMsg::ReadState => {
            let reader = input.from.expect("host: Unroutable");
            Response::keep_state().send_sync(reader, &StateReply::from(input))
        }
        HostMsg::Proxy { params, message } => proxy(input, params, *message),
        HostMsg::CreatePosition {
            owner,
            vault,
            collateral,
        } => create_position(input, owner, vault, collateral),
        HostMsg::AddDebt { normalized_debt } => {
            let mut position = load_position(input);
            assert_eq!(input.from, Some(position.params), "host: NotItsParams");
            position.normalized_debt = position
                .normalized_debt
                .checked_add(normalized_debt)
                .expect("host: DebtOverflow");
            Response::set_state(encode(&position))
        }
        HostMsg::Seize(order) => seize(input, &order),
        HostMsg::Finish => {
            assert_eq!(input.from, Some(input.receiver), "host: NotSelf");
            assert!(load_params(input).pending.is_none(), "host: ReadIncomplete");
            Response::keep_state()
        }
    }
}

/// Proxy block: the owner's root message at `(owner, H)`, forwarded only if the owner signed.
fn proxy(input: &ReceiveInput, params: AccountId, message: HostMsg) -> Response {
    assert!(input.from.is_none(), "host: NotRoot");
    assert!(input.is_authorized, "host: Unauthorized");
    let is_owner_action = matches!(
        message,
        HostMsg::OpenPosition { .. } | HostMsg::GenerateDebt { .. }
    );
    assert!(is_owner_action, "host: NotAnOwnerAction");
    Response::keep_state().send_sync(host_actor(input, params), &message)
}

/// The owner of a forwarded owner action: the account of the host-program proxy that sent it.
fn proxied_owner(input: &ReceiveInput) -> AccountId {
    let from = input
        .from
        .filter(|_| input.from_own_program())
        .expect("host: NotProxied");
    from.account_id
}

fn open_position(input: &ReceiveInput, owner: AccountId, collateral: u128) -> Response {
    let params = load_params(input);
    assert_eq!(proxied_owner(input), owner, "host: OwnerMismatch");
    let host = input.receiver.program_account_id;
    let vault = vault_id(host, owner);
    let create = HostMsg::CreatePosition {
        owner,
        vault,
        collateral,
    };
    let deposit = TokenMsg::Transfer {
        definition: params.collateral_def,
        to: vault,
        amount: collateral,
    };
    Response::keep_state()
        .send(
            SyncMessage::new(host_actor(input, position_id(host, owner)), &create)
                .with_pda_seeds([position_seed(owner)]),
        )
        .send_sync(Actor::new(owner, params.token), &deposit)
}

fn generate_debt(input: &ReceiveInput, normalized_debt: u128) -> Response {
    let params = load_params(input);
    let owner = proxied_owner(input);
    let host = input.receiver.program_account_id;
    let add = HostMsg::AddDebt { normalized_debt };
    // Accumulator = 1: the minted (nominal) debt equals the normalized debt.
    let mint = TokenMsg::MintTo {
        to: owner,
        amount: normalized_debt,
    };
    Response::keep_state()
        .send(
            SyncMessage::new(host_actor(input, position_id(host, owner)), &add)
                .with_pda_seeds([position_seed(owner)]),
        )
        .send_sync(Actor::new(params.stable_def, params.token), &mint)
}

fn create_position(
    input: &ReceiveInput,
    owner: AccountId,
    vault: AccountId,
    collateral: u128,
) -> Response {
    let params = input
        .from
        .filter(|_| input.from_own_program())
        .expect("host: NotFromHost");
    assert!(input.pre_state.as_ref().is_empty(), "host: PositionExists");
    let host = input.receiver.program_account_id;
    assert_eq!(
        input.receiver.account_id,
        position_id(host, owner),
        "host: NotThePosition"
    );
    assert_eq!(vault, vault_id(host, owner), "host: WrongVault");
    Response::set_state(encode(&Position {
        tag: TAG_POSITION,
        params,
        owner,
        collateral,
        normalized_debt: 0,
        vault,
    }))
}

/// Read block, first half: only the bound engine actor; store the request, read the oracle,
/// then the Sentinel.
fn liquidate(
    input: &ReceiveInput,
    position: AccountId,
    custody: AccountId,
    reply_to: Actor,
) -> Response {
    let mut params = load_params(input);
    assert!(
        params.engine_actor.is_some() && input.from == params.engine_actor,
        "host: NotEngine"
    );
    assert!(params.pending.is_none(), "host: Busy");
    params.pending = Some(PendingLiquidation {
        position,
        custody,
        reply_to,
    });
    let oracle = params.oracle;
    Response::set_state(encode(&params))
        .send_sync(oracle, &OracleMsg::ReadState)
        .send_sync(input.receiver, &HostMsg::Finish)
}

/// Read block, second half: the oracle's `StateReply`, only while a request is pending.
fn on_oracle_reply(input: &ReceiveInput, mut params: Params) -> Response {
    let pending = params.pending.take().expect("host: UnexpectedReply");
    let reply = StateReply::try_from_slice(&input.message).expect("host: BadReply");
    let oracle = OracleState::decode(reply.state.as_ref()).expect("host: NotAnOracle");
    assert!(oracle.price_ray > 0, "host: NoPrice");
    let seize = HostMsg::Seize(SeizeOrder {
        custody: pending.custody,
        reply_to: pending.reply_to,
        token: params.token,
        collateral_def: params.collateral_def,
        price_ray: oracle.price_ray,
        min_cr_ray: params.min_cr_ray,
    });
    let position = host_actor(input, pending.position);
    Response::set_state(encode(&params)).send_sync(position, &seize)
}

/// At the position: refuse a safe position; sweep the whole vault to custody first, zero the
/// position, then report `Seized` to `reply_to` (Transfer first, then Seized).
fn seize(input: &ReceiveInput, order: &SeizeOrder) -> Response {
    let position = load_position(input);
    assert_eq!(input.from, Some(position.params), "host: NotItsParams");
    assert!(
        is_unsafe(
            position.collateral,
            position.normalized_debt,
            order.price_ray,
            order.min_cr_ray
        ),
        "host: PositionSafe"
    );
    let seized = Seized {
        position: input.receiver.account_id,
        owner: position.owner,
        collateral: position.collateral,
        normalized_debt: position.normalized_debt,
        nominal_debt: position.normalized_debt,
    };
    let sweep = TokenMsg::Sweep {
        definition: order.collateral_def,
        to: order.custody,
        at_least: position.collateral,
    };
    let emptied = Position {
        collateral: 0,
        normalized_debt: 0,
        ..position
    };
    Response::set_state(encode(&emptied))
        .send(
            SyncMessage::new(Actor::new(position.vault, order.token), &sweep)
                .with_pda_seeds([vault_seed(position.owner)]),
        )
        .send(SyncMessage {
            to: order.reply_to,
            message: seized.encode(),
            pda_seeds: Default::default(),
        })
}
