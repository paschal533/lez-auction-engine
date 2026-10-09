//! The SDK's only view of the network. Production: `rpc::RpcChain` (feature `rpc`, Task 12). Tests:
//! `testing::FixtureChain` and the integration harness's `WorldChain` (real guests). Synchronous on
//! purpose: the CLI and the Plan 4 FFI (`char* op(const char* json)`) are synchronous.

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee::PublicTransaction;
use lee_core::{
    account::{Account, AccountId, Nonce},
    program::ProgramId,
};

use crate::error::{SdkError, SdkResult};

/// One public transaction of a block, reduced to what discovery needs (spec §6.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxSummary {
    pub hash: [u8; 32],
    pub program_id: ProgramId,
    pub account_ids: Vec<AccountId>,
    pub instruction_words: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockSummary {
    pub id: u64,
    pub hash: [u8; 32],
    pub prev_hash: [u8; 32],
    pub timestamp_ms: u64,
    pub txs: Vec<TxSummary>,
}

pub trait Chain {
    /// The account at `id`; an uninitialised account is `Account::default()`, never an error.
    fn account(&self, id: &AccountId) -> SdkResult<Account>;
    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<Nonce>>;
    fn last_block_id(&self) -> SdkResult<u64>;
    /// Blocks `from..=to`. A block the node cannot return is an error, never silently skipped.
    fn blocks(&self, from: u64, to: u64) -> SdkResult<Vec<BlockSummary>>;
    /// Hands the transaction to the sequencer. Acceptance is not execution (spec §3.2): confirm with
    /// `confirm::confirm`.
    fn send(&self, tx: &PublicTransaction) -> SdkResult<[u8; 32]>;
}

/// Lets a caller lend a chain (`Env { chain: &fixture, .. }` in the CLI tests) and keep using it.
impl<T: Chain + ?Sized> Chain for &T {
    fn account(&self, id: &AccountId) -> SdkResult<Account> {
        (**self).account(id)
    }
    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<Nonce>> {
        (**self).nonces(ids)
    }
    fn last_block_id(&self) -> SdkResult<u64> {
        (**self).last_block_id()
    }
    fn blocks(&self, from: u64, to: u64) -> SdkResult<Vec<BlockSummary>> {
        (**self).blocks(from, to)
    }
    fn send(&self, tx: &PublicTransaction) -> SdkResult<[u8; 32]> {
        (**self).send(tx)
    }
}

/// `CLOCK_01` timestamp in ms: the previous block's timestamp (spec §3.2). Quotes default to it.
pub fn clock_ms<C: Chain + ?Sized>(chain: &C) -> SdkResult<u64> {
    let acc = chain.account(&CLOCK_01_PROGRAM_ACCOUNT_ID)?;
    if acc == Account::default() {
        return Err(SdkError::Missing {
            what: "CLOCK_01",
            id: CLOCK_01_PROGRAM_ACCOUNT_ID.to_string(),
        });
    }
    borsh::from_slice::<ClockAccountData>(acc.data.as_ref())
        .map(|c| c.timestamp)
        .map_err(|e| SdkError::Decode {
            what: "CLOCK_01",
            id: CLOCK_01_PROGRAM_ACCOUNT_ID.to_string(),
            reason: e.to_string(),
        })
}
