//! In-memory `Chain` for SDK and CLI tests (feature `testing`). Not a ledger: it never runs programs.
//! `send` records the transaction and, in `SendMode::Include`, bumps every signer's nonce and appends a
//! block holding it, which is all the confirmation and scanner logic observe.

pub mod standard;
#[doc(hidden)]
pub mod support;
mod wallet;
pub use wallet::{keystore_json, write_keystore, TestKeys, STANDARD_LABELS};

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee::{PrivateKey, PublicKey, PublicTransaction};
use lee_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use token_core::TokenHolding;

use crate::chain::{BlockSummary, Chain, TxSummary};
use crate::error::{SdkError, SdkResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendMode {
    /// Applied: signer nonces +1 and a new block with the transaction.
    Include,
    /// Accepted by `send`, never applied (spec §3.2 silent drop).
    Drop,
    /// `send` itself fails.
    Reject,
}

/// Test key `n`: `PrivateKey::try_new([n; 32])` (the integration harness uses the same scheme).
#[must_use]
pub fn key(n: u8) -> PrivateKey {
    PrivateKey::try_new([n; 32]).expect("valid test key")
}

#[must_use]
pub fn id(n: u8) -> AccountId {
    AccountId::from(&PublicKey::new_from_private_key(&key(n)))
}

fn block_hash(id: u64, generation: u8) -> [u8; 32] {
    let mut h = [0u8; 32];
    h[..8].copy_from_slice(&id.to_le_bytes());
    h[31] = generation;
    h
}

#[derive(Debug)]
pub struct FixtureChain {
    accounts: RefCell<BTreeMap<AccountId, Account>>,
    blocks: RefCell<Vec<BlockSummary>>,
    hidden: RefCell<BTreeSet<u64>>,
    height: Cell<u64>,
    height_step: Cell<u64>,
    mode: Cell<SendMode>,
    sent: RefCell<Vec<PublicTransaction>>,
}

impl Default for FixtureChain {
    fn default() -> Self {
        Self::new()
    }
}

impl FixtureChain {
    #[must_use]
    pub fn new() -> Self {
        Self {
            accounts: RefCell::new(BTreeMap::new()),
            blocks: RefCell::new(Vec::new()),
            hidden: RefCell::new(BTreeSet::new()),
            height: Cell::new(0),
            height_step: Cell::new(0),
            mode: Cell::new(SendMode::Include),
            sent: RefCell::new(Vec::new()),
        }
    }

    pub fn put(&self, id: AccountId, account: Account) {
        self.accounts.borrow_mut().insert(id, account);
    }

    pub fn put_state<T>(&self, id: AccountId, owner: ProgramId, state: &T)
    where
        Data: for<'a> From<&'a T>,
    {
        self.put(
            id,
            Account {
                program_owner: owner,
                data: Data::from(state),
                ..Account::default()
            },
        );
    }

    pub fn put_holding(
        &self,
        id: AccountId,
        token: ProgramId,
        definition: AccountId,
        balance: u128,
    ) {
        self.put_state(
            id,
            token,
            &TokenHolding::Fungible {
                definition_id: definition,
                balance,
            },
        );
    }

    pub fn set_clock(&self, ms: u64) {
        let data = ClockAccountData {
            block_id: u64::try_from(self.blocks.borrow().len()).expect("small"),
            timestamp: ms,
        }
        .to_bytes();
        self.put(
            CLOCK_01_PROGRAM_ACCOUNT_ID,
            Account {
                program_owner: [8; 8],
                data: Data::try_from(data).expect("clock data fits"),
                ..Account::default()
            },
        );
    }

    pub fn set_nonce(&self, id: AccountId, nonce: u128) {
        self.accounts.borrow_mut().entry(id).or_default().nonce = Nonce(nonce);
    }

    /// Appends a block (ids start at 1, like LEZ's genesis block id) and returns it.
    pub fn push_block(&self, txs: Vec<TxSummary>) -> BlockSummary {
        let mut blocks = self.blocks.borrow_mut();
        let id = u64::try_from(blocks.len()).expect("small") + 1;
        let prev_hash = blocks.last().map_or([0; 32], |b| b.hash);
        let b = BlockSummary {
            id,
            hash: block_hash(id, 0),
            prev_hash,
            timestamp_ms: 0,
            txs,
        };
        blocks.push(b.clone());
        b
    }

    /// Re-hashes block `id` and every later block (a node that rebuilt them, e.g. a restarted localnet).
    pub fn rewrite_from(&self, id: u64, generation: u8) {
        let mut blocks = self.blocks.borrow_mut();
        let start = usize::try_from(id).expect("small").saturating_sub(1);
        let mut prev = if start == 0 {
            [0; 32]
        } else {
            blocks[start - 1].hash
        };
        for b in blocks.iter_mut().skip(start) {
            b.prev_hash = prev;
            b.hash = block_hash(b.id, generation);
            prev = b.hash;
        }
    }

    /// Keeps blocks `1..=len` only (a node that lost its later blocks, or a shorter replacement).
    pub fn truncate(&self, len: u64) {
        self.blocks
            .borrow_mut()
            .truncate(usize::try_from(len).expect("small"));
    }

    /// The node answers "not found" for block `id`.
    pub fn hide_block(&self, id: u64) {
        self.hidden.borrow_mut().insert(id);
    }

    pub fn set_send_mode(&self, mode: SendMode) {
        self.mode.set(mode);
    }

    /// Every `last_block_id` call advances the height by `step` (simulates blocks during polling).
    pub fn set_height_step(&self, step: u64) {
        self.height_step.set(step);
    }

    #[must_use]
    pub fn sent(&self) -> Vec<PublicTransaction> {
        self.sent.borrow().clone()
    }
}

impl Chain for FixtureChain {
    fn account(&self, id: &AccountId) -> SdkResult<Account> {
        Ok(self.accounts.borrow().get(id).cloned().unwrap_or_default())
    }

    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<Nonce>> {
        let accounts = self.accounts.borrow();
        Ok(ids
            .iter()
            .map(|i| accounts.get(i).map_or(Nonce(0), |a| a.nonce))
            .collect())
    }

    fn last_block_id(&self) -> SdkResult<u64> {
        let next = self.height.get() + self.height_step.get();
        self.height.set(next);
        Ok(next.max(u64::try_from(self.blocks.borrow().len()).expect("small")))
    }

    fn blocks(&self, from: u64, to: u64) -> SdkResult<Vec<BlockSummary>> {
        let blocks = self.blocks.borrow();
        let hidden = self.hidden.borrow();
        (from..=to)
            .map(|i| {
                let found = if hidden.contains(&i) || i == 0 {
                    None
                } else {
                    blocks.get(usize::try_from(i - 1).expect("small")).cloned()
                };
                found.ok_or_else(|| SdkError::Chain(format!("block {i} not found")))
            })
            .collect()
    }

    fn send(&self, tx: &PublicTransaction) -> SdkResult<[u8; 32]> {
        self.sent.borrow_mut().push(tx.clone());
        let hash = tx.hash();
        match self.mode.get() {
            SendMode::Reject => Err(SdkError::Chain("sequencer rejected the transaction".into())),
            SendMode::Drop => Ok(hash),
            SendMode::Include => {
                for (_, pk) in tx.witness_set().signatures_and_public_keys() {
                    let mut accounts = self.accounts.borrow_mut();
                    let acc = accounts.entry(AccountId::from(pk)).or_default();
                    acc.nonce = Nonce(acc.nonce.0 + 1);
                }
                let m = tx.message();
                self.push_block(vec![TxSummary {
                    hash,
                    program_id: m.program_id,
                    account_ids: m.account_ids.clone(),
                    instruction_words: m.instruction_data.clone(),
                }]);
                Ok(hash)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_chain_blocks_clock_and_missing_blocks() {
        let c = FixtureChain::new();
        c.set_clock(42);
        assert_eq!(crate::chain::clock_ms(&c).expect("clock"), 42);
        let b1 = c.push_block(vec![]);
        let b2 = c.push_block(vec![]);
        assert_eq!((b1.id, b2.prev_hash), (1, b1.hash));
        assert_eq!(c.blocks(1, 2).expect("two blocks").len(), 2);
        c.hide_block(2);
        assert!(matches!(c.blocks(1, 2), Err(SdkError::Chain(m)) if m.contains("block 2")));
        c.rewrite_from(1, 9);
        assert_ne!(c.blocks(1, 1).expect("block 1")[0].hash, b1.hash);
        assert_eq!(c.last_block_id().expect("height"), 2);
    }
}
