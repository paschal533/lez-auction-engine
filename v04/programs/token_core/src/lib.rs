//! Token actor types (design note §6): the message set and the two actor states the token
//! program keeps. The handler lives in the guest (`programs/methods/guest/src/bin/token.rs`).
//!
//! A holding is the token program's actor state on an account: a balance per definition, so
//! crediting a junk token can never lock an account out of the real one. A
//! definition is its actor state on the definition account, `{ supply, minter }`. Each state
//! starts with an 8-byte tag, so its encoding is never empty (empty state deletes an actor state)
//! and a holding can never be mistaken for a definition.

use std::collections::BTreeMap;

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::{AccountId, Actor};

/// Tag of an encoded [`Definition`].
pub const TAG_DEFINITION: [u8; 8] = *b"TOKDEF01";
/// Tag of an encoded [`Holding`].
pub const TAG_HOLDING: [u8; 8] = *b"TOKHLD01";

/// What the token actor accepts. Variant order is part of the wire format.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum TokenMsg {
    /// At the definition account (empty state); authorised. Mints `initial_supply` to `initial_to`.
    NewDefinition {
        minter: Option<Actor>,
        initial_to: Option<AccountId>,
        initial_supply: u128,
    },
    /// At a holding; authorised. Debits `definition`'s entry, then credits `(to, T)` synchronously.
    Transfer {
        definition: AccountId,
        to: AccountId,
        amount: u128,
    },
    /// At a holding; authorised. Moves `definition`'s whole entry, which must be at least `at_least`.
    Sweep {
        definition: AccountId,
        to: AccountId,
        at_least: u128,
    },
    /// At a holding; authorised. Debits `definition`'s entry and lowers that definition's supply.
    Burn { definition: AccountId, amount: u128 },
    /// At the definition; only from its minter actor. Raises supply and credits `(to, T)`.
    MintTo { to: AccountId, amount: u128 },
    /// At a holding; only from an actor of the token program. Adds `amount` of `definition`.
    Credit { definition: AccountId, amount: u128 },
    /// At the definition; only from an actor of the token program. Lowers supply.
    SupplyDown { amount: u128 },
    /// At any token actor; replies `StateReply` to `from`.
    ReadState,
}

/// A token definition.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Definition {
    pub tag: [u8; 8],
    pub supply: u128,
    pub minter: Option<Actor>,
}

/// One account's balances, keyed by token definition. An entry at zero is removed.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Holding {
    pub tag: [u8; 8],
    pub balances: BTreeMap<AccountId, u128>,
}

impl Definition {
    pub fn new(supply: u128, minter: Option<Actor>) -> Self {
        Self {
            tag: TAG_DEFINITION,
            supply,
            minter,
        }
    }

    /// Decodes a definition, `None` if the bytes are not exactly an encoded definition.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        borsh::from_slice::<Self>(bytes)
            .ok()
            .filter(|state| state.tag == TAG_DEFINITION)
    }
}

impl Holding {
    pub fn empty() -> Self {
        Self {
            tag: TAG_HOLDING,
            balances: BTreeMap::new(),
        }
    }

    /// The balance of `definition`, zero if there is no entry.
    pub fn balance(&self, definition: AccountId) -> u128 {
        self.balances.get(&definition).copied().unwrap_or(0)
    }

    /// Sets `definition`'s balance, removing the entry at zero.
    pub fn set_balance(&mut self, definition: AccountId, balance: u128) {
        if balance == 0 {
            self.balances.remove(&definition);
        } else {
            self.balances.insert(definition, balance);
        }
    }

    /// Decodes a holding, `None` if the bytes are not exactly an encoded holding.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        borsh::from_slice::<Self>(bytes)
            .ok()
            .filter(|state| state.tag == TAG_HOLDING)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(byte: u8) -> AccountId {
        AccountId::new([byte; 32])
    }

    #[test]
    fn states_round_trip_and_are_never_empty() {
        let mut holding = Holding::empty();
        assert_eq!(holding.balance(account(1)), 0);
        holding.set_balance(account(1), 0);
        let encoded = borsh::to_vec(&holding).expect("encodes");
        assert!(!encoded.is_empty());
        assert_eq!(Holding::decode(&encoded), Some(holding.clone()));
        holding.set_balance(account(1), 4);
        holding.set_balance(account(1), 0);
        assert!(holding.balances.is_empty());

        let definition = Definition::new(0, None);
        let encoded = borsh::to_vec(&definition).expect("encodes");
        assert_eq!(Definition::decode(&encoded), Some(definition));
    }

    #[test]
    fn a_holding_does_not_decode_as_a_definition_or_back() {
        let holding = borsh::to_vec(&Holding::empty()).expect("encodes");
        let definition = borsh::to_vec(&Definition::new(5, None)).expect("encodes");
        assert_eq!(Definition::decode(&holding), None);
        assert_eq!(Holding::decode(&definition), None);
        assert_eq!(Holding::decode(&[]), None);
    }
}
