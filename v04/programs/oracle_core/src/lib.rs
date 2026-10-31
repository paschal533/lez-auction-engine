//! Oracle actor types (design note §6): the minimal price feed the engine reads. One oracle
//! account holds `{ publisher, price_ray, timestamp_ms, confidence }`; only the publisher actor,
//! compared as a whole `(account, program)` pair, may update it. The handler lives in the guest
//! (`programs/methods/guest/src/bin/oracle.rs`).
//!
//! Prices are RAY-scaled `u128` (RAY = 1e27). The state starts with an 8-byte tag, so its
//! encoding is never empty (empty state deletes an actor state).

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::Actor;

/// Tag of an encoded [`OracleState`].
pub const TAG_ORACLE: [u8; 8] = *b"ORACLE01";

/// What the oracle actor accepts. Variant order is part of the wire format.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum OracleMsg {
    /// At the oracle account (empty state); authorised. Fixes the publisher; price starts at zero.
    Init { publisher: Actor },
    /// At the oracle; only from exactly `publisher`. Replaces the whole publication.
    Publish {
        price_ray: u128,
        timestamp_ms: u64,
        confidence: u128,
    },
    /// At the oracle; replies `StateReply` to `from`.
    ReadState,
}

/// The oracle's last publication. Before the first `Publish` all three values are zero.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct OracleState {
    pub tag: [u8; 8],
    pub publisher: Actor,
    pub price_ray: u128,
    pub timestamp_ms: u64,
    pub confidence: u128,
}

impl OracleState {
    pub fn new(publisher: Actor) -> Self {
        Self {
            tag: TAG_ORACLE,
            publisher,
            price_ray: 0,
            timestamp_ms: 0,
            confidence: 0,
        }
    }

    /// Decodes an oracle state, `None` if the bytes are not exactly an encoded one.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        borsh::from_slice::<Self>(bytes)
            .ok()
            .filter(|state| state.tag == TAG_ORACLE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lee_core::account::AccountId;

    #[test]
    fn state_round_trips_and_rejects_a_wrong_tag() {
        let publisher = Actor::new(AccountId::new([1; 32]), AccountId::new([2; 32]));
        let state = OracleState::new(publisher);
        let mut encoded = borsh::to_vec(&state).expect("encodes");
        assert_eq!(OracleState::decode(&encoded), Some(state));
        assert_eq!(OracleState::decode(&[]), None);
        if let Some(first) = encoded.first_mut() {
            *first ^= 1;
        }
        assert_eq!(OracleState::decode(&encoded), None);
    }
}
