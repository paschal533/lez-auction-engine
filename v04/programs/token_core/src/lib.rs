//! Token actor types (design note §6, ADR-062): the message set, the two actor states the token
//! program keeps and where a holding lives. The handler lives in the guest
//! (`programs/methods/guest/src/bin/token.rs`).
//!
//! A holding is one actor per (owner, definition): the token actor at
//! `pda(T, ["HOLDING", owner, definition])` ([`holding_actor`]), state `{ owner, definition,
//! balance }`. Junk credited to an account lands in other actors, so it never changes what a debit
//! of the real token costs. The owner's own token actor `(owner, T)` is a stateless proxy: it
//! authorises `Transfer`/`Sweep`/`Burn` and forwards a [`TokenMsg::Debit`] to the holding. A
//! definition is the actor state on the definition account, `{ supply, minter }`; nothing but
//! `NewDefinition` writes `(X, T)`. Each state starts with an 8-byte tag, so its encoding is never
//! empty (empty state deletes an actor state) and a holding can never be mistaken for a definition.

use auction_engine_params::seeds::{combine_seeds, seed_from_str};
use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{
    account::{AccountId, Actor},
    program::PdaSeed,
};

/// Tag of an encoded [`Definition`].
pub const TAG_DEFINITION: [u8; 8] = *b"TOKDEF01";
/// Tag of an encoded [`Holding`].
pub const TAG_HOLDING: [u8; 8] = *b"TOKHLD02";
/// Seed literal of a holding PDA, `pda(T, ["HOLDING", owner, definition])`.
pub const HOLDING_SEED: &str = "HOLDING";

/// What the token actor accepts. Variant order is part of the wire format.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum TokenMsg {
    /// At the definition account (empty state); authorised. Mints `initial_supply` to
    /// `initial_to`'s holding.
    NewDefinition {
        minter: Option<Actor>,
        initial_to: Option<AccountId>,
        initial_supply: u128,
    },
    /// At the owner's token actor `(owner, T)`; authorised. Debits the owner's holding of
    /// `definition`, which credits `to`'s holding synchronously.
    Transfer {
        definition: AccountId,
        to: AccountId,
        amount: u128,
    },
    /// At the owner's token actor; authorised. Moves the holding's whole balance, which must be at
    /// least `at_least`.
    Sweep {
        definition: AccountId,
        to: AccountId,
        at_least: u128,
    },
    /// At the owner's token actor; authorised. Debits the holding and lowers that definition's
    /// supply.
    Burn { definition: AccountId, amount: u128 },
    /// At the definition; only from its minter actor. Raises supply and credits `to`'s holding.
    MintTo { to: AccountId, amount: u128 },
    /// At `holding_actor(T, owner, definition)`; only from an actor of the token program, with
    /// the token program's `HOLDING` seed (a leaf). Adds `amount`.
    Credit {
        owner: AccountId,
        definition: AccountId,
        amount: u128,
    },
    /// At the definition; only from an actor of the token program. Lowers supply.
    SupplyDown { amount: u128 },
    /// At any token actor; replies `StateReply` to `from`.
    ReadState,
    /// At `holding_actor(T, o, definition)`; only from the owner's token actor `(o, T)`, unseeded.
    Debit {
        definition: AccountId,
        action: DebitAction,
    },
}

/// What a holding does with a [`TokenMsg::Debit`].
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum DebitAction {
    /// Debits `amount` and credits `to`'s holding.
    Transfer { to: AccountId, amount: u128 },
    /// Debits the whole balance, at least `at_least`, and credits `to`'s holding.
    Sweep { to: AccountId, at_least: u128 },
    /// Debits `amount` and lowers the definition's supply.
    Burn { amount: u128 },
}

/// A token definition.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Definition {
    pub tag: [u8; 8],
    pub supply: u128,
    pub minter: Option<Actor>,
}

/// `owner`'s balance of `definition`. A holding debited to zero keeps its state.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Holding {
    pub tag: [u8; 8],
    pub owner: AccountId,
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
    /// An empty holding of `owner` for `definition`.
    pub fn new(owner: AccountId, definition: AccountId) -> Self {
        Self {
            tag: TAG_HOLDING,
            owner,
            definition,
            balance: 0,
        }
    }

    /// Decodes a holding, `None` if the bytes are not exactly an encoded holding.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        borsh::from_slice::<Self>(bytes)
            .ok()
            .filter(|state| state.tag == TAG_HOLDING)
    }
}

/// `["HOLDING", owner, definition]`: the seed the token program grants on a `Credit`.
#[must_use]
pub fn holding_seed(owner: AccountId, definition: AccountId) -> PdaSeed {
    PdaSeed::new(combine_seeds(&[
        &seed_from_str(HOLDING_SEED),
        owner.value(),
        definition.value(),
    ]))
}

/// `owner`'s holding of `definition`: `(pda(T, holding_seed(owner, definition)), T)`.
#[must_use]
pub fn holding_actor(token: AccountId, owner: AccountId, definition: AccountId) -> Actor {
    Actor::new(
        AccountId::for_public_pda(&token, &holding_seed(owner, definition)),
        token,
    )
}

/// Whether a `Debit` of `definition` from `from` may debit `receiver`: only the owner's own token
/// actor `(o, T)` debits, and `receiver` must be `o`'s holding of `definition` (whole-actor `from`
/// check plus re-derivation, ADR-057).
#[must_use]
pub fn is_holding_owner(receiver: Actor, from: Option<Actor>, definition: AccountId) -> bool {
    let token = receiver.program_account_id;
    from.is_some_and(|from| {
        from.program_account_id == token
            && receiver == holding_actor(token, from.account_id, definition)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn account(byte: u8) -> AccountId {
        AccountId::new([byte; 32])
    }

    /// Literal: UTF-8, zero-padded to 32 bytes.
    fn lit(s: &str) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (slot, byte) in out.iter_mut().zip(s.bytes()) {
            *slot = byte;
        }
        out
    }

    #[test]
    fn holding_seed_matches_its_literal() {
        let (owner, definition) = (account(1), account(2));
        let mut hasher = Sha256::new();
        hasher.update(lit("HOLDING"));
        hasher.update(owner.value());
        hasher.update(definition.value());
        let expected: [u8; 32] = hasher.finalize().into();
        assert_eq!(holding_seed(owner, definition).as_bytes(), &expected);
        assert_ne!(
            holding_seed(definition, owner),
            holding_seed(owner, definition)
        );
    }

    #[test]
    fn holding_actor_is_the_token_pda() {
        let (token, owner, definition) = (account(9), account(1), account(2));
        let actor = holding_actor(token, owner, definition);
        assert_eq!(actor.program_account_id, token);
        assert_eq!(
            actor.account_id,
            AccountId::for_public_pda(&token, &holding_seed(owner, definition))
        );
    }

    #[test]
    fn only_the_owner_actor_may_debit() {
        let (token, other, owner, c, definition) =
            (account(9), account(8), account(1), account(3), account(2));
        let holding = holding_actor(token, owner, definition);
        assert!(is_holding_owner(
            holding,
            Some(Actor::new(owner, token)),
            definition
        ));
        for from in [
            None,
            Some(Actor::new(c, token)),
            Some(Actor::new(owner, other)),
            Some(holding),
        ] {
            assert!(!is_holding_owner(holding, from, definition), "{from:?}");
        }
        // The right owner, but another definition's holding.
        assert!(!is_holding_owner(
            holding,
            Some(Actor::new(owner, token)),
            account(4)
        ));
    }

    #[test]
    fn states_round_trip_and_are_never_empty() {
        let holding = Holding::new(account(1), account(2));
        assert_eq!(holding.balance, 0);
        let encoded = borsh::to_vec(&holding).expect("encodes");
        assert!(!encoded.is_empty());
        assert_eq!(Holding::decode(&encoded), Some(holding));

        let definition = Definition::new(0, None);
        let encoded = borsh::to_vec(&definition).expect("encodes");
        assert_eq!(Definition::decode(&encoded), Some(definition));
    }

    #[test]
    fn a_holding_does_not_decode_as_a_definition_or_back() {
        let holding = borsh::to_vec(&Holding::new(account(1), account(2))).expect("encodes");
        let definition = borsh::to_vec(&Definition::new(5, None)).expect("encodes");
        assert_eq!(Definition::decode(&holding), None);
        assert_eq!(Holding::decode(&definition), None);
        assert_eq!(Holding::decode(&[]), None);
    }

    #[test]
    fn token_msg_variant_indices_are_fixed() {
        let index = |msg: &TokenMsg| borsh::to_vec(msg).expect("encodes").first().copied();
        let credit = TokenMsg::Credit {
            owner: account(1),
            definition: account(2),
            amount: 3,
        };
        let debit = TokenMsg::Debit {
            definition: account(1),
            action: DebitAction::Burn { amount: 1 },
        };
        assert_eq!(index(&credit), Some(5));
        assert_eq!(index(&TokenMsg::ReadState), Some(7));
        assert_eq!(index(&debit), Some(8));
    }
}
