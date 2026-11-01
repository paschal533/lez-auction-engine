//! The token program (design note §6): holdings and definitions as actor states.
//!
//! A holding keeps a balance per definition, and a definition needs its account's authorisation,
//! so nobody can lock an account's token slot with a junk token. Credits are synchronous, so a
//! whole conversation settles in one transaction. The program sends nothing but `Credit`,
//! `SupplyDown` and `StateReply`. Errors are panics `token: <reason>`.

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, StateReply},
};
use token_core::{Definition, Holding, TokenMsg};

lee_core::actor_entrypoint!(handle_message);

fn encode<T: borsh::BorshSerialize>(state: &T) -> Vec<u8> {
    borsh::to_vec(state).expect("state encodes")
}

fn token_actor(input: &ReceiveInput, account: AccountId) -> Actor {
    Actor::new(account, input.receiver.program_account_id)
}

/// The receiver's holding; an account with no state yet holds nothing.
fn load_holding(input: &ReceiveInput) -> Holding {
    if input.pre_state.as_ref().is_empty() {
        Holding::empty()
    } else {
        Holding::decode(input.pre_state.as_ref()).expect("token: NotAHolding")
    }
}

fn load_definition(input: &ReceiveInput) -> Definition {
    Definition::decode(input.pre_state.as_ref()).expect("token: NotADefinition")
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
        } => {
            let mut holding = load_holding(input);
            debit(input, &mut holding, definition, amount);
            assert_ne!(to, input.receiver.account_id, "token: SelfTransfer");
            send_credit(input, &holding, definition, to, amount)
        }
        TokenMsg::Sweep {
            definition,
            to,
            at_least,
        } => {
            let mut holding = load_holding(input);
            let amount = holding.balance(definition);
            assert!(amount >= at_least, "token: BelowAtLeast");
            debit(input, &mut holding, definition, amount);
            assert_ne!(to, input.receiver.account_id, "token: SelfTransfer");
            send_credit(input, &holding, definition, to, amount)
        }
        TokenMsg::Burn { definition, amount } => {
            let mut holding = load_holding(input);
            debit(input, &mut holding, definition, amount);
            Response::set_state(encode(&holding)).send_sync(
                token_actor(input, definition),
                &TokenMsg::SupplyDown { amount },
            )
        }
        TokenMsg::MintTo { to, amount } => {
            let mut definition = load_definition(input);
            let minter = definition.minter.expect("token: NoMinter");
            assert_eq!(input.from, Some(minter), "token: NotMinter");
            definition.supply = definition
                .supply
                .checked_add(amount)
                .expect("token: SupplyOverflow");
            let credit = TokenMsg::Credit {
                definition: input.receiver.account_id,
                amount,
            };
            Response::set_state(encode(&definition)).send_sync(token_actor(input, to), &credit)
        }
        TokenMsg::Credit { definition, amount } => receive_credit(input, definition, amount),
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
        Some(to) => {
            let credit = TokenMsg::Credit {
                definition: input.receiver.account_id,
                amount: initial_supply,
            };
            state.send_sync(token_actor(input, to), &credit)
        }
        None => {
            assert_eq!(initial_supply, 0, "token: SupplyWithoutRecipient");
            state
        }
    }
}

/// Checks authorisation and the balance, then debits `definition`'s entry of `holding` in place.
fn debit(input: &ReceiveInput, holding: &mut Holding, definition: AccountId, amount: u128) {
    assert!(input.is_authorized, "token: Unauthorized");
    let balance = holding
        .balance(definition)
        .checked_sub(amount)
        .expect("token: InsufficientBalance");
    holding.set_balance(definition, balance);
}

fn send_credit(
    input: &ReceiveInput,
    holding: &Holding,
    definition: AccountId,
    to: AccountId,
    amount: u128,
) -> Response {
    let credit = TokenMsg::Credit { definition, amount };
    Response::set_state(encode(holding)).send_sync(token_actor(input, to), &credit)
}

fn receive_credit(input: &ReceiveInput, definition: AccountId, amount: u128) -> Response {
    assert!(input.from_own_program(), "token: ForeignCredit");
    let mut holding = load_holding(input);
    let balance = holding
        .balance(definition)
        .checked_add(amount)
        .expect("token: BalanceOverflow");
    holding.set_balance(definition, balance);
    Response::set_state(encode(&holding))
}
