//! PDA seeds of the engine (Shared Reference R1). Encoding = SPEL's (ADR-020, verified in S3):
//! literal → UTF-8 zero-padded to 32 bytes, account → 32-byte id, u64 → little-endian zero-padded,
//! several seeds → SHA-256 of the concatenation.

pub use auction_engine_params::seeds::{combine_seeds, seed_from_str, u64_seed};
use lee_core::{
    account::AccountId,
    program::{PdaSeed, ProgramId},
};

pub const SLOT_REF_ORACLE: u64 = 0;
pub const SLOT_PROTOCOL_TOKEN: u64 = 1;

fn seed(parts: &[&[u8; 32]]) -> PdaSeed {
    PdaSeed::new(combine_seeds(parts))
}

fn pda(program_id: &ProgramId, s: &PdaSeed) -> AccountId {
    AccountId::for_public_pda(program_id, s)
}

macro_rules! pda_fns {
    ($seed_fn:ident, $pda_fn:ident, ($($arg:ident : $ty:ty),*), $body:expr) => {
        #[must_use]
        pub fn $seed_fn($($arg: $ty),*) -> PdaSeed { $body }
        #[must_use]
        pub fn $pda_fn(program_id: &ProgramId, $($arg: $ty),*) -> AccountId { pda(program_id, &$seed_fn($($arg),*)) }
    };
}

pda_fns!(config_seed, config_pda, (admin: &AccountId, salt: u64),
    seed(&[&seed_from_str("CONFIG"), admin.value(), &u64_seed(salt)]));
pda_fns!(coll_config_seed, coll_config_pda, (config: &AccountId, collateral_definition: &AccountId),
    seed(&[&seed_from_str("COLL"), config.value(), collateral_definition.value()]));
pda_fns!(accounting_seed, accounting_pda, (config: &AccountId), seed(&[&seed_from_str("ACCT"), config.value()]));
pda_fns!(buffer_seed, buffer_pda, (config: &AccountId), seed(&[&seed_from_str("BUFFER"), config.value()]));
pda_fns!(liqrec_seed, liqrec_pda, (config: &AccountId, position: &AccountId),
    seed(&[&seed_from_str("LIQREC"), config.value(), position.value()]));
pda_fns!(auction_seed, auction_pda, (config: &AccountId, position: &AccountId, nonce: u64),
    seed(&[&seed_from_str("AUCTION"), config.value(), position.value(), &u64_seed(nonce)]));
pda_fns!(vault_seed, vault_pda, (auction: &AccountId), seed(&[&seed_from_str("VAULT"), auction.value()]));
pda_fns!(surplus_auction_seed, surplus_auction_pda, (config: &AccountId, nonce: u64),
    seed(&[&seed_from_str("SURP"), config.value(), &u64_seed(nonce)]));
pda_fns!(surplus_vault_seed, surplus_vault_pda, (surplus_auction: &AccountId),
    seed(&[&seed_from_str("SURPVAULT"), surplus_auction.value()]));
pda_fns!(debt_auction_seed, debt_auction_pda, (config: &AccountId, nonce: u64),
    seed(&[&seed_from_str("DEBT"), config.value(), &u64_seed(nonce)]));
pda_fns!(engine_authority_seed, engine_authority_pda, (config: &AccountId),
    seed(&[&seed_from_str("ENGINE_AUTH"), config.value()]));
pda_fns!(mint_authority_seed, mint_authority_pda, (config: &AccountId),
    seed(&[&seed_from_str("MINT_AUTH"), config.value()]));
pda_fns!(unused_slot_seed, unused_slot_pda, (config: &AccountId, slot: u64),
    seed(&[&seed_from_str("UNUSED"), config.value(), &u64_seed(slot)]));
