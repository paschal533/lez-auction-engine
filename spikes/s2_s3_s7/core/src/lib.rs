//! Shared types for the S2/S3/S7 spike programs (`callee`, `caller`, `impostor`).
//!
//! * `callee`   - owns a stored engine config and exposes three guards (S2 variants A, B, C).
//! * `caller`   - plays the engine: chains to `callee`, creates arg-seeded PDAs (S3), mints/burns
//!   through the token program under a PDA mint authority (S7).
//! * `impostor` - a third program that tries to pass the callee guards.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{
    account::{AccountId, Data},
    program::{PdaSeed, ProgramId},
};
use risc0_zkvm::sha::{Impl, Sha256 as _};
use serde::{Deserialize, Serialize};

/// S2 guard variants.
pub const VARIANT_A: u8 = 0; // caller_program_id == stored engine program id
pub const VARIANT_B: u8 = 1; // stored engine authority PDA is authorised in the call
pub const VARIANT_C: u8 = 2; // both

pub const CALLEE_CFG_SEED: &str = "CALLEE_CFG";
pub const ENGINE_AUTH_SEED: &str = "ENGINE_AUTH";
pub const MINT_AUTH_SEED: &str = "MINT_AUTH";
pub const LIQREC_TAG: &str = "LIQREC";
pub const AUCTION_TAG: &str = "AUCTION";

/// SPEL `literal("..")` / `const("..")` seed: UTF-8 bytes, zero-padded to 32 (max 32 bytes).
#[must_use]
pub fn seed_from_str(s: &str) -> [u8; 32] {
    let src = s.as_bytes();
    assert!(src.len() <= 32, "seed string exceeds 32 bytes");
    let mut out = [0u8; 32];
    out[..src.len()].copy_from_slice(src);
    out
}

/// SPEL `arg("..")` seed for a `u64`: little-endian bytes zero-padded to 32.
#[must_use]
pub fn u64_seed(n: u64) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[..8].copy_from_slice(&n.to_le_bytes());
    out
}

/// SPEL multi-seed combination: a single seed is used as is; several are hashed,
/// `SHA-256(seed_1 || seed_2 || ...)`.
#[must_use]
pub fn combine_seeds(seeds: &[&[u8; 32]]) -> [u8; 32] {
    assert!(!seeds.is_empty(), "PDA requires at least one seed");
    if seeds.len() == 1 {
        return *seeds[0];
    }
    let mut buf = Vec::with_capacity(seeds.len() * 32);
    for s in seeds {
        buf.extend_from_slice(*s);
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(Impl::hash_bytes(&buf).as_bytes());
    out
}

#[must_use]
pub fn named_pda_seed(name: &str) -> PdaSeed {
    PdaSeed::new(seed_from_str(name))
}

#[must_use]
pub fn named_pda(program_id: ProgramId, name: &str) -> AccountId {
    AccountId::for_public_pda(&program_id, &named_pda_seed(name))
}

/// Seed of `["LIQREC", config, position]`.
#[must_use]
pub fn liqrec_seed(config: &AccountId, position: &[u8; 32]) -> PdaSeed {
    PdaSeed::new(combine_seeds(&[
        &seed_from_str(LIQREC_TAG),
        config.value(),
        position,
    ]))
}

#[must_use]
pub fn liqrec_pda(program_id: ProgramId, config: &AccountId, position: &[u8; 32]) -> AccountId {
    AccountId::for_public_pda(&program_id, &liqrec_seed(config, position))
}

/// Seed of `["AUCTION", config, position, nonce]`.
#[must_use]
pub fn auction_seed(config: &AccountId, position: &[u8; 32], nonce: u64) -> PdaSeed {
    PdaSeed::new(combine_seeds(&[
        &seed_from_str(AUCTION_TAG),
        config.value(),
        position,
        &u64_seed(nonce),
    ]))
}

#[must_use]
pub fn auction_pda(
    program_id: ProgramId,
    config: &AccountId,
    position: &[u8; 32],
    nonce: u64,
) -> AccountId {
    AccountId::for_public_pda(&program_id, &auction_seed(config, position, nonce))
}

macro_rules! borsh_data {
    ($t:ty, $name:literal) => {
        impl TryFrom<&Data> for $t {
            type Error = std::io::Error;
            fn try_from(data: &Data) -> Result<Self, Self::Error> {
                Self::try_from_slice(data.as_ref())
            }
        }
        impl From<&$t> for Data {
            fn from(state: &$t) -> Self {
                let mut buf = Vec::new();
                BorshSerialize::serialize(state, &mut buf).expect(concat!($name, " serialises"));
                Self::try_from(buf).expect(concat!($name, " fits into Data"))
            }
        }
    };
}

/// Stored by `callee` (a PDA of callee, seed `CALLEE_CFG`): who is allowed to call the guards.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CalleeConfig {
    pub engine_program_id: ProgramId,
    pub engine_authority: AccountId,
}
borsh_data!(CalleeConfig, "CalleeConfig");

/// `["LIQREC", config, position]`, owned by `caller`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct LiqRecord {
    pub position: [u8; 32],
    pub count: u64,
}
borsh_data!(LiqRecord, "LiqRecord");

/// `["AUCTION", config, position, nonce]`, owned by `caller`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Auction {
    pub position: [u8; 32],
    pub nonce: u64,
}
borsh_data!(Auction, "Auction");

/// Callee instructions: one guard per S2 variant (no args; the expected caller is stored).
#[derive(Debug, Serialize, Deserialize)]
pub enum CalleeInstruction {
    GuardedA,
    GuardedB,
    GuardedC,
}

impl CalleeInstruction {
    #[must_use]
    pub fn from_variant(variant: u8) -> Self {
        match variant {
            VARIANT_A => Self::GuardedA,
            VARIANT_B => Self::GuardedB,
            VARIANT_C => Self::GuardedC,
            _ => panic!("unknown guard variant"),
        }
    }
}

/// Caller (engine stand-in) instructions.
#[derive(Debug, Serialize, Deserialize)]
pub enum CallerInstruction {
    /// S2: chained call to `callee.guarded_{a,b,c}`.
    CallGuarded {
        variant: u8,
        callee_program_id: ProgramId,
    },
    /// S3: create `["LIQREC", config, position]` if uninitialised; `count += 1`.
    OpenRecord { position: [u8; 32] },
    /// S3: create the record if needed, assert `nonce == count`, create
    /// `["AUCTION", config, position, nonce]`, `count += 1` (explicit claims).
    OpenAuction { position: [u8; 32], nonce: u64 },
    /// S3: like `OpenAuction` but the record must already exist and the auction claim is the
    /// one the SPEL macro generates from `#[account(init, pda = [...])]`.
    OpenAuctionExisting { position: [u8; 32], nonce: u64 },
    /// S7: chained `Token::MintWithAuthority` under the `MINT_AUTH` PDA.
    MintTo { amount_to_mint: u128 },
    /// S7: chained `Token::Burn`, authorised by the holder's top-level signature.
    BurnFrom { amount_to_burn: u128 },
}

/// Impostor instructions.
#[derive(Debug, Serialize, Deserialize)]
pub enum ImpostorInstruction {
    /// Chain to `callee.guarded_*`. `mode`: 0 = forge `is_authorized` on the given authority
    /// account, no seeds; 1 = forge the flag and also claim PDA seeds `ENGINE_AUTH` (derives the
    /// wrong address); 2 = same call but the authority account passed is the impostor's own
    /// `ENGINE_AUTH` PDA (legitimately authorised under the impostor's seeds).
    Attack {
        callee_program_id: ProgramId,
        variant: u8,
        mode: u8,
    },
}
