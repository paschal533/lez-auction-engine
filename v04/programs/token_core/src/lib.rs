//! Token actor types (design note §6): the message set and the two actor states the token
//! program keeps. The handler lives in the guest (`programs/methods/guest/src/bin/token.rs`).
//!
//! A holding is the token program's actor state on an account, `{ definition, balance }`; a
//! definition is its actor state on the definition account, `{ supply, minter }`. Each state
//! starts with an 8-byte tag, so its encoding is never empty (empty state deletes an actor state)
//! and a holding can never be mistaken for a definition.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::{AccountId, Actor};

/// Tag of an encoded [`Definition`].
pub const TAG_DEFINITION: [u8; 8] = *b"TOKDEF01";
/// Tag of an encoded [`Holding`].
pub const TAG_HOLDING: [u8; 8] = *b"TOKHLD01";

/// What the token actor accepts. Variant order is part of the wire format.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum TokenMsg {
    /// At the definition account (empty state). Mints `initial_supply` to `initial_to`.
    NewDefinition {
        minter: Option<Actor>,
        initial_to: Option<AccountId>,
        initial_supply: u128,
    },
    /// At a holding; authorised. Debits, then credits `(to, T)` synchronously.
    Transfer { to: AccountId, amount: u128 },
    /// At a holding; authorised. Moves the whole balance, which must be at least `at_least`.
    Sweep { to: AccountId, at_least: u128 },
    /// At a holding; authorised. Debits and lowers the definition's supply.
    Burn { amount: u128 },
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

/// One account's balance of one token.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Holding {
    pub tag: [u8; 8],
    pub definition: AccountId,
    pub balance: u128,
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
    pub fn new(definition: AccountId, balance: u128) -> Self {
        Self {
            tag: TAG_HOLDING,
            definition,
            balance,
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
        let holding = Holding::new(account(1), 0);
        let encoded = borsh::to_vec(&holding).expect("encodes");
        assert!(!encoded.is_empty());
        assert_eq!(Holding::decode(&encoded), Some(holding));

        let definition = Definition::new(0, None);
        let encoded = borsh::to_vec(&definition).expect("encodes");
        assert_eq!(Definition::decode(&encoded), Some(definition));
    }

    #[test]
    fn a_holding_does_not_decode_as_a_definition_or_back() {
        let holding = borsh::to_vec(&Holding::new(account(1), 5)).expect("encodes");
        let definition = borsh::to_vec(&Definition::new(5, None)).expect("encodes");
        assert_eq!(Definition::decode(&holding), None);
        assert_eq!(Holding::decode(&definition), None);
        assert_eq!(Holding::decode(&[]), None);
    }
}
