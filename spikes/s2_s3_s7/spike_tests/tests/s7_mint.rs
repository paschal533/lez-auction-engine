//! S7: mint from an engine PDA, then burn.
//!
//! The token definition's mint authority is the `caller` program's `MINT_AUTH` PDA (the same
//! model as `token_mint_authority`). `caller.mint_to` chains `Token::MintWithAuthority` under the
//! PDA seed; `caller.burn_from` chains `Token::Burn`, authorised by the holder's signature.

mod common;

use common::*;
use lee::V03State;
use lee_core::account::{Account, AccountId, Data, Nonce};
use spike_core::{named_pda, CallerInstruction, MINT_AUTH_SEED};
use token_core::{TokenDefinition, TokenHolding};

const OPERATOR: u8 = 31;
const HOLDER: u8 = 32;
const UNIT: u128 = 1_000_000_000_000_000_000;

fn definition_id() -> AccountId {
    AccountId::new([0xD0; 32])
}

fn mint_authority() -> AccountId {
    named_pda(caller_id(), MINT_AUTH_SEED)
}

fn holder_id() -> AccountId {
    id_of(&key(HOLDER))
}

fn operator_id() -> AccountId {
    id_of(&key(OPERATOR))
}

fn definition_init() -> Account {
    Account {
        program_owner: token_id(),
        balance: 0,
        data: Data::from(&TokenDefinition::Fungible {
            name: String::from("SPIKE-MINT"),
            total_supply: 0,
            metadata_id: None,
            authority: Some(mint_authority()),
        }),
        nonce: Nonce(0),
    }
}

fn holding_init(balance: u128) -> Account {
    Account {
        program_owner: token_id(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: definition_id(),
            balance,
        }),
        nonce: Nonce(0),
    }
}

fn token_state() -> V03State {
    let mut state = base_state();
    state.force_insert_account(definition_id(), definition_init());
    state.force_insert_account(holder_id(), holding_init(0));
    state
}

fn balance(state: &V03State) -> u128 {
    match TokenHolding::try_from(&state.get_account_by_id(holder_id()).data).unwrap() {
        TokenHolding::Fungible { balance, .. } => balance,
        _ => panic!("fungible expected"),
    }
}

fn supply(state: &V03State) -> u128 {
    match TokenDefinition::try_from(&state.get_account_by_id(definition_id()).data).unwrap() {
        TokenDefinition::Fungible { total_supply, .. } => total_supply,
        _ => panic!("fungible expected"),
    }
}

fn mint_accounts() -> Vec<AccountId> {
    vec![operator_id(), definition_id(), holder_id(), mint_authority()]
}

/// Mints through `caller.mint_to`, recording cycles.
fn mint_via_pda(state: &mut V03State, label: &str, amount: u128) -> Result<(), lee::error::LeeError> {
    let operator = key(OPERATOR);
    let instruction = CallerInstruction::MintTo { amount_to_mint: amount };
    let m = measure(label, state, caller_id(), &mint_accounts(), &[&operator], &instruction);
    println!(
        "S7RESULT {label} caller_cycles={} token_cycles={} total_cycles={} executions={}",
        m.cycles_of("caller"),
        m.cycles_of("token"),
        m.total,
        m.per_exec.len()
    );
    submit(state, caller_id(), mint_accounts(), &[&operator], instruction)
}

#[test]
fn mint_by_the_pda_succeeds() {
    let mut state = token_state();
    mint_via_pda(&mut state, "S7 mint_to (via PDA)", 1_000 * UNIT).expect("PDA mint succeeds");
    assert_eq!(balance(&state), 1_000 * UNIT);
    assert_eq!(supply(&state), 1_000 * UNIT);

    // A second mint stacks.
    mint_via_pda(&mut state, "S7 mint_to again", 5 * UNIT).unwrap();
    assert_eq!(balance(&state), 1_005 * UNIT);
    assert_eq!(supply(&state), 1_005 * UNIT);
}

#[test]
fn mint_by_a_user_key_fails() {
    let mut state = token_state();
    let operator = key(OPERATOR);

    // (a) A user calls Token::MintWithAuthority directly with their own key as the authority.
    let res = submit(
        &mut state,
        token_id(),
        vec![definition_id(), holder_id(), operator_id()],
        &[&operator],
        token_core::Instruction::MintWithAuthority { amount_to_mint: UNIT },
    );
    let text = err_text(res);
    println!("FAIL[S7 user key as authority] {text}");
    assert!(text.contains("signer is not the current authority"), "{text}");

    // (b) The PDA is listed as the authority but nobody can authorise it at top level.
    let res = submit(
        &mut state,
        token_id(),
        vec![definition_id(), holder_id(), mint_authority()],
        &[],
        token_core::Instruction::MintWithAuthority { amount_to_mint: UNIT },
    );
    let text = err_text(res);
    println!("FAIL[S7 PDA listed, not authorised] {text}");
    assert!(text.contains("must be a signer"), "{text}");

    // (c) The definition account itself as self-authority (Token::Mint) is not the stored authority.
    let res = submit(
        &mut state,
        token_id(),
        vec![definition_id(), holder_id()],
        &[],
        token_core::Instruction::Mint { amount_to_mint: UNIT },
    );
    let text = err_text(res);
    println!("FAIL[S7 definition as self authority] {text}");
    assert!(text.contains("must be a signer"), "{text}");

    assert_eq!(balance(&state), 0);
    assert_eq!(supply(&state), 0);
}

#[test]
fn burn_by_the_holder_succeeds() {
    let mut state = token_state();
    mint_via_pda(&mut state, "S7 mint_to (setup)", 1_000 * UNIT).unwrap();

    let holder = key(HOLDER);
    let accounts = vec![definition_id(), holder_id()];
    let instruction = CallerInstruction::BurnFrom { amount_to_burn: 400 * UNIT };
    let m = measure("S7 burn_from", &state, caller_id(), &accounts, &[&holder], &instruction);
    println!(
        "S7RESULT S7 burn_from caller_cycles={} token_cycles={} total_cycles={} executions={}",
        m.cycles_of("caller"),
        m.cycles_of("token"),
        m.total,
        m.per_exec.len()
    );
    submit(&mut state, caller_id(), accounts, &[&holder], instruction).expect("holder burn works");
    assert_eq!(balance(&state), 600 * UNIT);
    assert_eq!(supply(&state), 600 * UNIT);
}

#[test]
fn burn_without_the_holders_signature_fails() {
    let mut state = token_state();
    mint_via_pda(&mut state, "S7 mint_to (setup)", 1_000 * UNIT).unwrap();

    // Nobody signs: the holder's holding is only listed.
    let res = submit(
        &mut state,
        caller_id(),
        vec![definition_id(), holder_id()],
        &[],
        CallerInstruction::BurnFrom { amount_to_burn: UNIT },
    );
    let text = err_text(res);
    println!("FAIL[S7 burn without holder signature] {text}");
    assert!(text.contains("must be a signer"), "{text}");
    assert_eq!(balance(&state), 1_000 * UNIT);
    assert_eq!(supply(&state), 1_000 * UNIT);
}
