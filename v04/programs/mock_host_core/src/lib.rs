//! Mock CDP host types (design note §5, option B): the minimal stand-in for the stablecoin until
//! upstream ports it to LEZ 0.4. Its engine-facing messages (`SetLiquidationEngine`,
//! `LiquidatePosition`, the `Seized` report) are the interface the upstream port must provide.
//! The handler lives in the guest (`programs/methods/guest/src/bin/mock_host.rs`).
//!
//! Actors, with `H` the host program and `T` the token program:
//! - params `P = (params, H)`: [`Params`]; the params account is the mock's admin and signs
//!   `Init` and `SetLiquidationEngine`.
//! - owner proxy `(owner, H)`: stateless; forwards an owner action to `P` only if the owner signed.
//! - position `(pda(H, ["POSITION", owner]), H)`: [`Position`], one per owner.
//! - vault `(pda(H, ["VAULT", owner]), T)`: the position's collateral, a token holding.
//!
//! Each state starts with an 8-byte tag, so its encoding is never empty (empty state deletes an
//! actor state) and params can never be mistaken for a position.

use auction_engine_params::seeds::{combine_seeds, seed_from_str};
use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{
    account::{AccountId, Actor},
    program::PdaSeed,
};

/// Tag of an encoded [`Params`].
pub const TAG_PARAMS: [u8; 8] = *b"MCKPAR01";
/// Tag of an encoded [`Position`].
pub const TAG_POSITION: [u8; 8] = *b"MCKPOS01";
/// Borsh variant index of the engine's `EngineMsg::Seized` (ruling R-P1); see [`Seized`].
pub const SEIZED_INDEX: u8 = 5;
/// Seed literal of a position PDA, `pda(H, ["POSITION", owner])`.
pub const POSITION_SEED: &str = "POSITION";
/// Seed literal of a vault PDA, `pda(H, ["VAULT", owner])`, held under the token program.
pub const VAULT_SEED: &str = "VAULT";

/// What the host accepts. Variant order is part of the wire format: variants 0-5 are the
/// interface; the variants after them are the owner proxy and host-internal messages.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum HostMsg {
    /// At `P` (empty state); `P` signs and `admin` must be `P` itself.
    Init {
        admin: Actor,
        token: AccountId,
        stable_def: AccountId,
        collateral_def: AccountId,
        oracle: Actor,
        redemption_price_ray: u128,
        min_cr_ray: u128,
    },
    /// At `P`, root only; `P` signs. Binds (or rebinds) the engine actor.
    SetLiquidationEngine { engine_actor: Actor },
    /// At `P`, forwarded by the owner's proxy. Creates the position and moves `collateral` from
    /// the owner's holding to the vault. `owner` must be the proxy's account.
    OpenPosition { owner: AccountId, collateral: u128 },
    /// At `P`, forwarded by the owner's proxy. Adds debt and mints that much stablecoin to the
    /// owner (accumulator = 1, so nominal = normalized).
    GenerateDebt { normalized_debt: u128 },
    /// At `P`, only from exactly the bound engine actor. Reads the oracle; if the position is
    /// unsafe at `min_cr_ray`, sweeps the whole vault to `(custody, T)`, zeroes the position and
    /// then reports [`Seized`] to `reply_to`.
    LiquidatePosition {
        position: AccountId,
        custody: AccountId,
        reply_to: Actor,
    },
    /// At any host actor; replies `StateReply` to `from`.
    ReadState,
    /// Root at the owner's proxy `(owner, H)`; the owner signs. Forwards `message`
    /// (`OpenPosition` or `GenerateDebt`) to `(params, H)`.
    Proxy {
        params: AccountId,
        message: Box<HostMsg>,
    },
    /// `P` → a fresh position (POSITION seed granted). Creates it, bound to the sending `P`.
    CreatePosition {
        owner: AccountId,
        vault: AccountId,
        collateral: u128,
    },
    /// `P` → its position. Adds normalized debt.
    AddDebt { normalized_debt: u128 },
    /// `P` → its position, after the oracle read. Refuses a safe position; else sweeps the vault
    /// to `custody` (VAULT seed), zeroes the position, then sends `Seized` to `reply_to`.
    Seize(SeizeOrder),
    /// `P` → `P`, the last message of `LiquidatePosition` (Sentinel): the oracle read completed.
    Finish,
}

/// What `P` tells a position to seize: where to, whom to report to, and the reading to judge by.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SeizeOrder {
    pub custody: AccountId,
    pub reply_to: Actor,
    pub token: AccountId,
    pub collateral_def: AccountId,
    pub price_ray: u128,
    pub min_cr_ray: u128,
}

/// A liquidation waiting for the oracle's `StateReply` (Read block scratch).
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PendingLiquidation {
    pub position: AccountId,
    pub custody: AccountId,
    pub reply_to: Actor,
}

/// The host's parameters, at `P`. The redemption price is read by the engine via `ReadState`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Params {
    pub tag: [u8; 8],
    pub admin: Actor,
    pub token: AccountId,
    pub stable_def: AccountId,
    pub collateral_def: AccountId,
    pub oracle: Actor,
    pub redemption_price_ray: u128,
    pub min_cr_ray: u128,
    pub engine_actor: Option<Actor>,
    pub pending: Option<PendingLiquidation>,
}

/// One owner's position. `params` is the `P` that created it, the only actor it obeys.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Position {
    pub tag: [u8; 8],
    pub params: Actor,
    pub owner: AccountId,
    pub collateral: u128,
    pub normalized_debt: u128,
    pub vault: AccountId,
}

/// The host's report to the engine after a seizure: the engine's `EngineMsg::Seized`, encoded
/// as borsh variant [`SEIZED_INDEX`] followed by these fields in this order (ruling R-P1).
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Seized {
    pub position: AccountId,
    pub owner: AccountId,
    pub collateral: u128,
    pub normalized_debt: u128,
    pub nominal_debt: u128,
}

impl Params {
    /// Decodes params, `None` if the bytes are not exactly encoded params.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        borsh::from_slice::<Self>(bytes)
            .ok()
            .filter(|state| state.tag == TAG_PARAMS)
    }
}

impl Position {
    /// Decodes a position, `None` if the bytes are not exactly an encoded position.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        borsh::from_slice::<Self>(bytes)
            .ok()
            .filter(|state| state.tag == TAG_POSITION)
    }
}

impl Seized {
    /// The engine message bytes: `SEIZED_INDEX`, then the borsh fields.
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = vec![SEIZED_INDEX];
        self.serialize(&mut bytes).expect("Seized encodes");
        bytes
    }

    /// Decodes engine message bytes, `None` unless they are exactly an encoded `Seized`.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        match bytes.split_first() {
            Some((&SEIZED_INDEX, fields)) => borsh::from_slice(fields).ok(),
            _ => None,
        }
    }
}

fn owner_seed(literal: &str, owner: AccountId) -> PdaSeed {
    PdaSeed::new(combine_seeds(&[&seed_from_str(literal), owner.value()]))
}

/// The seed of `owner`'s position PDA under the host program.
pub fn position_seed(owner: AccountId) -> PdaSeed {
    owner_seed(POSITION_SEED, owner)
}

/// The seed of `owner`'s vault PDA under the host program.
pub fn vault_seed(owner: AccountId) -> PdaSeed {
    owner_seed(VAULT_SEED, owner)
}

/// `owner`'s position account under host program `host`.
pub fn position_id(host: AccountId, owner: AccountId) -> AccountId {
    AccountId::for_public_pda(&host, &position_seed(owner))
}

/// `owner`'s vault account under host program `host` (its holding lives under the token program).
pub fn vault_id(host: AccountId, owner: AccountId) -> AccountId {
    AccountId::for_public_pda(&host, &vault_seed(owner))
}

/// Whether a position is unsafe: `collateral × price < min_cr × debt` in RAY units, with the
/// accumulator at one (nominal = normalized debt). Exact 256-bit products, no rounding.
pub fn is_unsafe(
    collateral: u128,
    normalized_debt: u128,
    price_ray: u128,
    min_cr_ray: u128,
) -> bool {
    mul_wide(collateral, price_ray) < mul_wide(min_cr_ray, normalized_debt)
}

/// The exact product `a × b` as `(high, low)` 128-bit halves, which compare as the product does.
fn mul_wide(a: u128, b: u128) -> (u128, u128) {
    const LOW: u128 = 0xFFFF_FFFF_FFFF_FFFF;
    let (a_hi, a_lo) = (a.wrapping_shr(64), a & LOW);
    let (b_hi, b_lo) = (b.wrapping_shr(64), b & LOW);
    // Each factor is below 2^64, so no partial product overflows.
    let lo_lo = a_lo.wrapping_mul(b_lo);
    let lo_hi = a_lo.wrapping_mul(b_hi);
    let hi_lo = a_hi.wrapping_mul(b_lo);
    let hi_hi = a_hi.wrapping_mul(b_hi);
    // Three terms below 2^64 each: the sum is below 2^66.
    let middle = lo_lo
        .wrapping_shr(64)
        .wrapping_add(lo_hi & LOW)
        .wrapping_add(hi_lo & LOW);
    let low = (lo_lo & LOW) | (middle & LOW).wrapping_shl(64);
    // The full product is below 2^256, so the high half cannot overflow.
    let high = hi_hi
        .wrapping_add(lo_hi.wrapping_shr(64))
        .wrapping_add(hi_lo.wrapping_shr(64))
        .wrapping_add(middle.wrapping_shr(64));
    (high, low)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAY: u128 = 1_000_000_000_000_000_000_000_000_000;

    fn account(byte: u8) -> AccountId {
        AccountId::new([byte; 32])
    }

    #[test]
    fn seized_encoding_is_variant_five_then_the_fields_in_order() {
        let seized = Seized {
            position: account(1),
            owner: account(2),
            collateral: 3,
            normalized_debt: 4,
            nominal_debt: 5,
        };
        let bytes = seized.encode();
        let mut expected = vec![5_u8];
        expected.extend_from_slice(&[1; 32]);
        expected.extend_from_slice(&[2; 32]);
        for field in [3_u128, 4, 5] {
            expected.extend_from_slice(&field.to_le_bytes());
        }
        assert_eq!(SEIZED_INDEX, 5);
        assert_eq!(bytes, expected);
        assert_eq!(Seized::decode(&bytes), Some(seized));
        assert_eq!(Seized::decode(&[4]), None);
    }

    #[test]
    fn states_round_trip_and_do_not_decode_as_each_other() {
        let actor = Actor::new(account(1), account(2));
        let params = Params {
            tag: TAG_PARAMS,
            admin: actor,
            token: account(3),
            stable_def: account(4),
            collateral_def: account(5),
            oracle: actor,
            redemption_price_ray: RAY,
            min_cr_ray: RAY,
            engine_actor: None,
            pending: None,
        };
        let position = Position {
            tag: TAG_POSITION,
            params: actor,
            owner: account(6),
            collateral: 7,
            normalized_debt: 8,
            vault: account(9),
        };
        let params_bytes = borsh::to_vec(&params).expect("encodes");
        let position_bytes = borsh::to_vec(&position).expect("encodes");
        assert_eq!(Params::decode(&params_bytes), Some(params));
        assert_eq!(Position::decode(&position_bytes), Some(position));
        assert_eq!(Params::decode(&position_bytes), None);
        assert_eq!(Position::decode(&params_bytes), None);
        assert_eq!(Params::decode(&[]), None);
    }

    #[test]
    fn mul_wide_is_exact() {
        assert_eq!(mul_wide(0, u128::MAX), (0, 0));
        assert_eq!(mul_wide(6, 7), (0, 42));
        assert_eq!(mul_wide(1 << 64, 1 << 64), (1, 0));
        // (2^128 - 1)^2 = 2^256 - 2^129 + 1.
        assert_eq!(mul_wide(u128::MAX, u128::MAX), (u128::MAX - 1, 1));
    }

    #[test]
    fn unsafe_below_the_minimum_ratio_only() {
        let min_cr = RAY * 3 / 2;
        // 1_000 collateral at price 2: value 2_000 against required 1.5 × 1_000.
        assert!(!is_unsafe(1_000, 1_000, 2 * RAY, min_cr));
        // Exactly at the ratio is safe.
        assert!(!is_unsafe(1_500, 1_000, RAY, min_cr));
        assert!(is_unsafe(1_499, 1_000, RAY, min_cr));
        // No debt is always safe; products beyond u128 are compared exactly.
        assert!(!is_unsafe(0, 0, 0, min_cr));
        assert!(is_unsafe(u128::MAX / 2, u128::MAX, RAY, min_cr));
    }

    #[test]
    fn position_and_vault_differ_per_owner_and_seed() {
        let host = account(7);
        assert_ne!(position_id(host, account(1)), vault_id(host, account(1)));
        assert_ne!(position_id(host, account(1)), position_id(host, account(2)));
        assert_eq!(
            position_id(host, account(1)),
            AccountId::for_public_pda(&host, &position_seed(account(1)))
        );
    }
}
