//! The token program (design note §6, ADR-062): holdings per (owner, definition) and definitions
//! as actor states.
//!
//! - The owner's token actor `(owner, T)` is a stateless proxy: `Transfer`/`Sweep`/`Burn` need its
//!   authorisation (a signature or the caller's PDA seed), then it sends an unseeded `Debit` to
//!   `holding_actor(T, owner, definition)`.
//! - A holding accepts `Debit` only from `(o, T)` whose re-derived holding is the receiver, and
//!   `Credit` only from the token program, at the holding the credit names. A credit carries the
//!   token program's `HOLDING` seed, so a fresh recipient holding is admitted without transaction
//!   evidence; `Credit` sends nothing (a leaf).
//! - A definition needs its account's authorisation, so nobody can claim a definition address;
//!   credits never write `(X, T)`, so nobody can pre-block one either.
//!
//! Credits are synchronous, so a whole conversation settles in one transaction. The program sends
//! nothing but `Debit`, `Credit`, `SupplyDown` and `StateReply`. Errors are panics
//! `token: <reason>`.

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, StateReply, SyncMessage},
};
use token_core::{
    DebitAction, Definition, Holding, TokenMsg, holding_actor, holding_seed, is_holding_owner,
};

lee_core::actor_entrypoint!(handle_message);

fn encode<T: borsh::BorshSerialize>(state: &T) -> Vec<u8> {
    borsh::to_vec(state).expect("state encodes")
}

fn token(input: &ReceiveInput) -> AccountId {
    input.receiver.program_account_id
}

fn load_definition(input: &ReceiveInput) -> Definition {
    Definition::decode(input.pre_state.as_ref()).expect("token: NotADefinition")
}

/// The receiver's holding of `owner` for `definition`; a holding with no state yet holds nothing.
/// An existing state must be that holding.
fn load_holding(input: &ReceiveInput, owner: AccountId, definition: AccountId) -> Holding {
    if input.pre_state.as_ref().is_empty() {
        return Holding::new(owner, definition);
    }
    let holding = Holding::decode(input.pre_state.as_ref()).expect("token: NotAHolding");
    assert!(
        holding.owner == owner && holding.definition == definition,
        "token: WrongHolding"
    );
    holding
}

/// A credit of `amount` of `definition` to `to`'s holding, granting the holding's seed.
fn credit(input: &ReceiveInput, to: AccountId, definition: AccountId, amount: u128) -> SyncMessage {
    let msg = TokenMsg::Credit {
        owner: to,
        definition,
        amount,
    };
    SyncMessage::new(holding_actor(token(input), to, definition), &msg)
        .with_pda_seeds([holding_seed(to, definition)])
}

fn handle_message(input: &ReceiveInput, message: TokenMsg) -> Response {
    match message {
        TokenMsg::NewDefinition {
            minter,
            initial_to,
            initial_supply,
        } => new_definition(input, minter, initial_to, initial_supply),
        TokenMsg::Transfer {
            definition,
            to,
            amount,
        } => forward_debit(
            input,
            definition,
            Some(to),
            DebitAction::Transfer { to, amount },
        ),
        TokenMsg::Sweep {
            definition,
            to,
            at_least,
        } => forward_debit(
            input,
            definition,
            Some(to),
            DebitAction::Sweep { to, at_least },
        ),
        TokenMsg::Burn { definition, amount } => {
            forward_debit(input, definition, None, DebitAction::Burn { amount })
        }
        TokenMsg::MintTo { to, amount } => {
            let mut definition = load_definition(input);
            let minter = definition.minter.expect("token: NoMinter");
            assert_eq!(input.from, Some(minter), "token: NotMinter");
            definition.supply = definition
                .supply
                .checked_add(amount)
                .expect("token: SupplyOverflow");
            let mint = credit(input, to, input.receiver.account_id, amount);
            Response::set_state(encode(&definition)).send(mint)
        }
        TokenMsg::Credit {
            owner,
            definition,
            amount,
        } => receive_credit(input, owner, definition, amount),
        TokenMsg::SupplyDown { amount } => {
            assert!(input.from_own_program(), "token: ForeignSupplyDown");
            let mut definition = load_definition(input);
            definition.supply = definition
                .supply
                .checked_sub(amount)
                .expect("token: SupplyUnderflow");
            Response::set_state(encode(&definition))
        }
        TokenMsg::ReadState => {
            let reader = input.from.expect("token: Unroutable");
            Response::keep_state().send_sync(reader, &StateReply::from(input))
        }
        TokenMsg::Debit { definition, action } => debit(input, definition, action),
    }
}

fn new_definition(
    input: &ReceiveInput,
    minter: Option<Actor>,
    initial_to: Option<AccountId>,
    initial_supply: u128,
) -> Response {
    assert!(input.is_authorized, "token: Unauthorized");
    assert!(input.pre_state.as_ref().is_empty(), "token: AlreadyExists");
    let state = Response::set_state(encode(&Definition::new(initial_supply, minter)));
    match initial_to {
        Some(to) => state.send(credit(input, to, input.receiver.account_id, initial_supply)),
        None => {
            assert_eq!(initial_supply, 0, "token: SupplyWithoutRecipient");
            state
        }
    }
}

/// At the owner's token actor: authorisation, no payment to itself, then an unseeded `Debit` to
/// the owner's holding of `definition`.
fn forward_debit(
    input: &ReceiveInput,
    definition: AccountId,
    to: Option<AccountId>,
    action: DebitAction,
) -> Response {
    assert!(input.is_authorized, "token: Unauthorized");
    let owner = input.receiver.account_id;
    assert_ne!(to, Some(owner), "token: SelfTransfer");
    let holding = holding_actor(token(input), owner, definition);
    Response::keep_state().send_sync(holding, &TokenMsg::Debit { definition, action })
}

/// At a holding, from exactly its owner's token actor: debits, then credits or lowers supply.
fn debit(input: &ReceiveInput, definition: AccountId, action: DebitAction) -> Response {
    assert!(
        is_holding_owner(input.receiver, input.from, definition),
        "token: NotHoldingOwner"
    );
    let owner = input.from.expect("token: NotHoldingOwner").account_id;
    let mut holding = load_holding(input, owner, definition);
    let amount = match action {
        DebitAction::Transfer { amount, .. } | DebitAction::Burn { amount } => amount,
        DebitAction::Sweep { at_least, .. } => {
            assert!(holding.balance >= at_least, "token: BelowAtLeast");
            holding.balance
        }
    };
    holding.balance = holding
        .balance
        .checked_sub(amount)
        .expect("token: InsufficientBalance");
    let state = Response::set_state(encode(&holding));
    match action {
        DebitAction::Transfer { to, .. } | DebitAction::Sweep { to, .. } => {
            state.send(credit(input, to, definition, amount))
        }
        DebitAction::Burn { .. } => state.send_sync(
            Actor::new(definition, token(input)),
            &TokenMsg::SupplyDown { amount },
        ),
    }
}

/// At the holding the credit names (checked first, so a credit aimed anywhere else reports
/// `WrongHolding`), from the token program.
fn receive_credit(
    input: &ReceiveInput,
    owner: AccountId,
    definition: AccountId,
    amount: u128,
) -> Response {
    assert!(
        input.receiver == holding_actor(token(input), owner, definition),
        "token: WrongHolding"
    );
    assert!(input.from_own_program(), "token: ForeignCredit");
    let mut holding = load_holding(input, owner, definition);
    holding.balance = holding
        .balance
        .checked_add(amount)
        .expect("token: BalanceOverflow");
    Response::set_state(encode(&holding))
}
