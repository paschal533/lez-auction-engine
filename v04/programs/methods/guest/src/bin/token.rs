//! The token program (design note §6): holdings and definitions as actor states.
//!
//! Credits are synchronous, so a whole conversation settles in one transaction, and a fresh
//! holding needs no signature: only this program can write its actor states. The program sends
//! nothing but `Credit`, `SupplyDown` and `StateReply`. Errors are panics `token: <reason>`.

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

fn load_holding(input: &ReceiveInput) -> Holding {
    Holding::decode(input.pre_state.as_ref()).expect("token: NotAHolding")
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
        TokenMsg::Transfer { to, amount } => {
            let mut holding = load_holding(input);
            debit(input, &mut holding, to, amount);
            send_credit(input, holding, to, amount)
        }
        TokenMsg::Sweep { to, at_least } => {
            let mut holding = load_holding(input);
            assert!(holding.balance >= at_least, "token: BelowAtLeast");
            let amount = holding.balance;
            debit(input, &mut holding, to, amount);
            send_credit(input, holding, to, amount)
        }
        TokenMsg::Burn { amount } => {
            let mut holding = load_holding(input);
            assert!(input.is_authorized, "token: Unauthorized");
            holding.balance = holding
                .balance
                .checked_sub(amount)
                .expect("token: InsufficientBalance");
            let definition = token_actor(input, holding.definition);
            Response::set_state(encode(&holding))
                .send_sync(definition, &TokenMsg::SupplyDown { amount })
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

/// Checks authorisation, the recipient and the balance, then debits `holding` in place.
fn debit(input: &ReceiveInput, holding: &mut Holding, to: AccountId, amount: u128) {
    assert!(input.is_authorized, "token: Unauthorized");
    assert_ne!(to, input.receiver.account_id, "token: SelfTransfer");
    holding.balance = holding
        .balance
        .checked_sub(amount)
        .expect("token: InsufficientBalance");
}

fn send_credit(input: &ReceiveInput, holding: Holding, to: AccountId, amount: u128) -> Response {
    let credit = TokenMsg::Credit {
        definition: holding.definition,
        amount,
    };
    Response::set_state(encode(&holding)).send_sync(token_actor(input, to), &credit)
}

fn receive_credit(input: &ReceiveInput, definition: AccountId, amount: u128) -> Response {
    assert!(input.from_own_program(), "token: ForeignCredit");
    let holding = if input.pre_state.as_ref().is_empty() {
        Holding::new(definition, amount)
    } else {
        let mut holding = load_holding(input);
        assert_eq!(holding.definition, definition, "token: WrongDefinition");
        holding.balance = holding
            .balance
            .checked_add(amount)
            .expect("token: BalanceOverflow");
        holding
    };
    Response::set_state(encode(&holding))
}
