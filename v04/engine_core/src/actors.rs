//! Actor and PDA helpers (design note §3). Seed bytes come from `auction_engine_params::seeds`
//! with the v0.2.4 literals (ADR-020); `engine` is the deployed engine program's address, in place
//! of the v0.2.4 image id. A `*_seed` is what the engine must grant when it moves or creates the
//! actor; an actor is the derived account under the engine (or, for holdings, the token program).

use auction_engine_params::seeds::{combine_seeds, seed_from_str, u64_seed};
use lee_core::{
    account::{AccountId, Actor},
    program::PdaSeed,
};

fn seed(parts: &[&[u8; 32]]) -> PdaSeed {
    PdaSeed::new(combine_seeds(parts))
}

/// The actor at `pda(engine, seed)` that belongs to `program`.
fn actor_at(program: AccountId, engine: AccountId, seed: &PdaSeed) -> Actor {
    Actor::new(AccountId::for_public_pda(&engine, seed), program)
}

/// `["CONFIG", admin, salt]`.
#[must_use]
pub fn config_seed(admin: AccountId, salt: u64) -> PdaSeed {
    seed(&[&seed_from_str("CONFIG"), admin.value(), &u64_seed(salt)])
}

/// `["COLL", config, collateral_def]`.
#[must_use]
pub fn collateral_seed(config: AccountId, collateral_def: AccountId) -> PdaSeed {
    seed(&[
        &seed_from_str("COLL"),
        config.value(),
        collateral_def.value(),
    ])
}

/// `["ACCT", config]`.
#[must_use]
pub fn accounting_seed(config: AccountId) -> PdaSeed {
    seed(&[&seed_from_str("ACCT"), config.value()])
}

/// `["BUFFER", config]`.
#[must_use]
pub fn buffer_seed(config: AccountId) -> PdaSeed {
    seed(&[&seed_from_str("BUFFER"), config.value()])
}

/// `["LIQREC", config, position]`.
#[must_use]
pub fn record_seed(config: AccountId, position: AccountId) -> PdaSeed {
    seed(&[&seed_from_str("LIQREC"), config.value(), position.value()])
}

/// `["AUCTION", config, position, nonce]`.
#[must_use]
pub fn auction_seed(config: AccountId, position: AccountId, nonce: u64) -> PdaSeed {
    seed(&[
        &seed_from_str("AUCTION"),
        config.value(),
        position.value(),
        &u64_seed(nonce),
    ])
}

/// `["VAULT", auction]`.
#[must_use]
pub fn vault_seed(auction: AccountId) -> PdaSeed {
    seed(&[&seed_from_str("VAULT"), auction.value()])
}

/// Config actor `(pda(E, CONFIG), E)`.
#[must_use]
pub fn config_actor(engine: AccountId, admin: AccountId, salt: u64) -> Actor {
    actor_at(engine, engine, &config_seed(admin, salt))
}

/// Collateral actor `(pda(E, COLL), E)`.
#[must_use]
pub fn collateral_actor(engine: AccountId, config: AccountId, collateral_def: AccountId) -> Actor {
    actor_at(engine, engine, &collateral_seed(config, collateral_def))
}

/// Accounting actor `(pda(E, ACCT), E)`.
#[must_use]
pub fn accounting_actor(engine: AccountId, config: AccountId) -> Actor {
    actor_at(engine, engine, &accounting_seed(config))
}

/// Liquidation-record actor `(pda(E, LIQREC), E)`.
#[must_use]
pub fn record_actor(engine: AccountId, config: AccountId, position: AccountId) -> Actor {
    actor_at(engine, engine, &record_seed(config, position))
}

/// Auction actor `(pda(E, AUCTION), E)`.
#[must_use]
pub fn auction_actor(
    engine: AccountId,
    config: AccountId,
    position: AccountId,
    nonce: u64,
) -> Actor {
    actor_at(engine, engine, &auction_seed(config, position, nonce))
}

/// An auction's vault `(pda(E, VAULT), T)`: a token holding at an engine-derived account.
#[must_use]
pub fn vault_holding(engine: AccountId, token: AccountId, auction: AccountId) -> Actor {
    actor_at(token, engine, &vault_seed(auction))
}

/// The surplus buffer `(pda(E, BUFFER), T)`.
#[must_use]
pub fn buffer_holding(engine: AccountId, token: AccountId, config: AccountId) -> Actor {
    actor_at(token, engine, &buffer_seed(config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixtures::id;
    use sha2::{Digest, Sha256};

    /// Literal: UTF-8, zero-padded to 32 bytes.
    fn lit(s: &str) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (slot, byte) in out.iter_mut().zip(s.bytes()) {
            *slot = byte;
        }
        out
    }

    /// u64: little-endian, zero-padded to 32 bytes.
    fn le(n: u64) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (slot, byte) in out.iter_mut().zip(n.to_le_bytes()) {
            *slot = byte;
        }
        out
    }

    /// v0.2.4 `pda.rs`: SHA-256 of the 32-byte parts, computed here without the shared helper.
    fn sha(parts: &[&[u8; 32]]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        for part in parts {
            hasher.update(part);
        }
        hasher.finalize().into()
    }

    #[test]
    fn pda_seeds_match_v024_literals() {
        let (admin, config, def, position, auction) = (id(1), id(2), id(3), id(4), id(5));
        let cases = [
            (
                config_seed(admin, 9),
                sha(&[&lit("CONFIG"), admin.value(), &le(9)]),
            ),
            (
                collateral_seed(config, def),
                sha(&[&lit("COLL"), config.value(), def.value()]),
            ),
            (
                accounting_seed(config),
                sha(&[&lit("ACCT"), config.value()]),
            ),
            (buffer_seed(config), sha(&[&lit("BUFFER"), config.value()])),
            (
                record_seed(config, position),
                sha(&[&lit("LIQREC"), config.value(), position.value()]),
            ),
            (
                auction_seed(config, position, 7),
                sha(&[&lit("AUCTION"), config.value(), position.value(), &le(7)]),
            ),
            (vault_seed(auction), sha(&[&lit("VAULT"), auction.value()])),
        ];
        for (seed, expected) in cases {
            assert_eq!(seed.as_bytes(), &expected);
        }
    }

    #[test]
    fn actors_are_the_derived_account_under_the_right_program() {
        let (engine, token, admin, config, def, position) =
            (id(9), id(8), id(1), id(2), id(3), id(4));
        let derive = |seed: PdaSeed| AccountId::for_public_pda(&engine, &seed);
        let k = config_actor(engine, admin, 1);
        assert_eq!(k, Actor::new(derive(config_seed(admin, 1)), engine));
        assert_eq!(
            collateral_actor(engine, config, def),
            Actor::new(derive(collateral_seed(config, def)), engine)
        );
        assert_eq!(
            accounting_actor(engine, config),
            Actor::new(derive(accounting_seed(config)), engine)
        );
        assert_eq!(
            record_actor(engine, config, position),
            Actor::new(derive(record_seed(config, position)), engine)
        );
        let a = auction_actor(engine, config, position, 3);
        assert_eq!(
            a,
            Actor::new(derive(auction_seed(config, position, 3)), engine)
        );
        assert_ne!(a, auction_actor(engine, config, position, 4));
        assert_eq!(
            vault_holding(engine, token, a.account_id),
            Actor::new(derive(vault_seed(a.account_id)), token)
        );
        assert_eq!(
            buffer_holding(engine, token, config),
            Actor::new(derive(buffer_seed(config)), token)
        );
    }
}
