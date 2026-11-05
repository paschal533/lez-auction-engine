//! The block scanner (ADR-066) against an in-memory chain: cursor, rewind, anchor and `collect`.

use std::collections::{BTreeMap, BTreeSet};

use borsh::BorshSerialize;
use engine_core::msg::EngineMsg;
use lee::{AccountId, Actor};
use sdk::SdkError;
use sdk::scan::{
    ANCHOR_BLOCK, BlockSource, BlockSummary, Cursor, Found, ScanSettings, ScanTargets, TxSummary,
    collect, scan, scan_page,
};

const ENGINE: AccountId = AccountId::new([0xE0; 32]);
const CONFIG: AccountId = AccountId::new([0xC0; 32]);
const OTHER_CONFIG: AccountId = AccountId::new([0xC1; 32]);
const POSITION_A: AccountId = AccountId::new([0xA1; 32]);
const POSITION_B: AccountId = AccountId::new([0xA2; 32]);
const COLLATERAL: AccountId = AccountId::new([0xD0; 32]);

fn targets() -> ScanTargets {
    ScanTargets {
        engine: ENGINE,
        config: CONFIG,
    }
}

fn digest(id: u64, salt: u8, prev: [u8; 32], txs: usize) -> [u8; 32] {
    let mut hash = prev;
    for (slot, byte) in hash.iter_mut().zip(id.to_le_bytes()) {
        *slot = byte;
    }
    hash[8] = salt;
    hash[9] = u8::try_from(txs).expect("few txs");
    hash[10] ^= 0x5A;
    hash
}

/// An in-memory chain; `hide_block` makes the node fail to return one block.
struct MemChain {
    blocks: Vec<BlockSummary>,
    hidden: BTreeSet<u64>,
}

impl MemChain {
    fn new(per_block_txs: Vec<Vec<TxSummary>>) -> Self {
        let mut chain = Self {
            blocks: Vec::new(),
            hidden: BTreeSet::new(),
        };
        for txs in per_block_txs {
            chain.push_block(txs);
        }
        chain
    }

    fn push_block(&mut self, txs: Vec<TxSummary>) {
        self.push_salted(txs, 0);
    }

    fn push_salted(&mut self, txs: Vec<TxSummary>, salt: u8) {
        let id = u64::try_from(self.blocks.len())
            .expect("small")
            .saturating_add(1);
        let prev_hash = self.blocks.last().map_or([0; 32], |b| b.hash);
        self.blocks.push(BlockSummary {
            id,
            hash: digest(id, salt, prev_hash, txs.len()),
            prev_hash,
            timestamp_ms: id.saturating_mul(1_000),
            txs,
        });
    }

    /// The node rebuilt every block from `from` on (same transactions, other hashes).
    fn rewrite_from(&mut self, from: u64, salt: u8) {
        let kept = usize::try_from(from.saturating_sub(1)).expect("small");
        let rebuilt: Vec<_> = self
            .blocks
            .split_off(kept)
            .into_iter()
            .map(|b| b.txs)
            .collect();
        for txs in rebuilt {
            self.push_salted(txs, salt);
        }
    }

    fn truncate(&mut self, head: u64) {
        self.blocks.truncate(usize::try_from(head).expect("small"));
    }

    fn hide_block(&mut self, id: u64) {
        self.hidden.insert(id);
    }

    fn hash(&self, id: u64) -> [u8; 32] {
        self.blocks.iter().find(|b| b.id == id).expect("block").hash
    }

    fn extend(&mut self, blocks: u64) {
        for _ in 0..blocks {
            self.push_block(vec![]);
        }
    }
}

impl BlockSource for MemChain {
    async fn last_block_id(&self) -> Result<u64, SdkError> {
        u64::try_from(self.blocks.len()).map_err(|e| SdkError::Rpc(e.to_string()))
    }

    async fn blocks(&self, from: u64, to: u64) -> Result<Vec<BlockSummary>, SdkError> {
        Ok(self
            .blocks
            .iter()
            .filter(|b| (from..=to).contains(&b.id) && !self.hidden.contains(&b.id))
            .cloned()
            .collect())
    }
}

fn liquidate(config: AccountId, position: AccountId, nonce: u64) -> EngineMsg {
    EngineMsg::Liquidate {
        config,
        collateral_def: COLLATERAL,
        position,
        nonce,
        now_ms: 0,
    }
}

fn tx_to(program: AccountId, message: Vec<u8>, reverted: bool) -> TxSummary {
    TxSummary {
        hash: [7; 32],
        root_to: Some(Actor::new(CONFIG, program)),
        message,
        reverted,
    }
}

fn tx(msg: &EngineMsg) -> TxSummary {
    tx_to(ENGINE, borsh::to_vec(msg).expect("encodes"), false)
}

fn block_with(txs: Vec<TxSummary>) -> BlockSummary {
    BlockSummary {
        id: 9,
        hash: [9; 32],
        prev_hash: [8; 32],
        timestamp_ms: 0,
        txs,
    }
}

/// Block 1 empty; block 2 liquidates position A; block 3 liquidates position B.
fn standard() -> MemChain {
    MemChain::new(vec![
        vec![],
        vec![tx(&liquidate(CONFIG, POSITION_A, 0))],
        vec![tx(&liquidate(CONFIG, POSITION_B, 0))],
    ])
}

fn standard_found() -> Found {
    Found {
        auctions: BTreeSet::from([(POSITION_A, 0), (POSITION_B, 0)]),
        positions: BTreeSet::from([POSITION_A, POSITION_B]),
        ..Found::default()
    }
}

async fn full_scan(chain: &MemChain) -> Cursor {
    scan(
        chain,
        &Cursor::default(),
        &targets(),
        &ScanSettings::default(),
    )
    .await
    .expect("full scan")
    .cursor
}

#[tokio::test]
async fn finds_auctions_and_positions_incrementally() {
    let chain = standard();
    let c = full_scan(&chain).await;
    assert_eq!(c.last_block, 3);
    assert_eq!(c.found, standard_found());
    let again = scan(&chain, &c, &targets(), &ScanSettings::default())
        .await
        .expect("no new blocks");
    assert_eq!(again.cursor, c);
    assert!(!again.restarted);
    let small = ScanSettings {
        batch: 1,
        ..ScanSettings::default()
    };
    let one = scan(&chain, &Cursor::default(), &targets(), &small)
        .await
        .expect("batch 1");
    assert_eq!(one.cursor, c);
}

#[tokio::test]
async fn rewinds_on_changed_hash() {
    let mut chain = standard();
    let c = full_scan(&chain).await;
    chain.rewrite_from(3, 7);
    let after = scan(&chain, &c, &targets(), &ScanSettings::default())
        .await
        .expect("rescan");
    assert!(!after.restarted, "same chain: no restart");
    assert_ne!(after.cursor.last_hash, c.last_hash);
    assert_eq!(
        after.cursor.found, c.found,
        "nothing missed, nothing duplicated"
    );
    assert_eq!(after.cursor.last_block, 3);
}

#[tokio::test]
async fn rewind_arithmetic_is_exact() {
    let mut chain = standard();
    let c = full_scan(&chain).await;
    chain.rewrite_from(3, 7);
    let one = ScanSettings {
        reorg_depth: 1,
        ..ScanSettings::default()
    };
    let after = scan(&chain, &c, &targets(), &one)
        .await
        .expect("rescan from block 2")
        .cursor;
    assert_eq!(after.last_block, 3);
    assert_eq!(after.last_hash, Some(chain.hash(3)));
    assert_ne!(after.last_hash, c.last_hash);
    assert_eq!(after.found, c.found);
    assert_eq!(after.recent.keys().copied().collect::<Vec<_>>(), vec![2, 3]);
    // Rebuilt from block 1: the anchor (block 2) changed, so the whole chain is rescanned.
    chain.rewrite_from(1, 8);
    let deep = scan(&chain, &after, &targets(), &one)
        .await
        .expect("rebuilt");
    assert!(deep.restarted);
    assert_eq!(deep.cursor.last_hash, Some(chain.hash(3)));
    assert_eq!(deep.cursor.anchor, Some(chain.hash(ANCHOR_BLOCK)));
    assert_eq!(deep.cursor.found, c.found);
}

#[tokio::test]
async fn replaced_chain_rescans_from_genesis() {
    // Shorter: the cursor of an old, long chain; the node now serves 3 blocks whose block 2 differs.
    let mut chain = standard();
    let old = Cursor {
        last_block: 500,
        last_hash: Some([0xAA; 32]),
        anchor: Some([0xBB; 32]),
        recent: BTreeMap::from([(490, [0xCC; 32])]),
        ..Cursor::default()
    };
    let r = scan(&chain, &old, &targets(), &ScanSettings::default())
        .await
        .expect("restarts");
    assert!(r.restarted);
    assert_eq!(r.cursor, full_scan(&chain).await);
    assert_eq!(r.cursor.found, standard_found());
    // Longer: head 20 is above `last_block - reorg_depth` (15 - 10); without the anchor the scan
    // would rewind to block 5, take its hash from the new node and never see blocks 1..=5.
    chain.extend(17);
    let old = Cursor {
        last_block: 15,
        last_hash: Some([0xAA; 32]),
        anchor: Some([0xBB; 32]),
        ..Cursor::default()
    };
    let r = scan(&chain, &old, &targets(), &ScanSettings::default())
        .await
        .expect("restarts");
    assert!(r.restarted);
    assert_eq!(r.cursor.last_block, 20);
    assert_eq!(r.cursor.found, standard_found());
    // The same through a real rebuild: the node restarted and produced more blocks since.
    let c = r.cursor;
    chain.rewrite_from(1, 9);
    chain.extend(5);
    let r = scan(&chain, &c, &targets(), &ScanSettings::default())
        .await
        .expect("restarts");
    assert!(r.restarted);
    assert_eq!(r.cursor, full_scan(&chain).await);
}

#[tokio::test]
async fn matching_anchor_scans_incrementally() {
    let mut chain = standard();
    let c = full_scan(&chain).await;
    assert_eq!(c.anchor, Some(chain.hash(ANCHOR_BLOCK)));
    chain.extend(2);
    // Block 1 is unreadable now: only an incremental scan (blocks 4..=5) can succeed.
    chain.hide_block(1);
    let r = scan(&chain, &c, &targets(), &ScanSettings::default())
        .await
        .expect("incremental");
    assert!(!r.restarted);
    assert_eq!(r.cursor.last_block, 5);
    assert_eq!(r.cursor.anchor, c.anchor);
    assert_eq!(r.cursor.found, c.found);
}

#[tokio::test]
async fn anchorless_cursor_is_rebuilt_once() {
    let chain = standard();
    let s = ScanSettings::default();
    // A cursor that passed the anchor block without recording it: the chain is unknown.
    let old = Cursor {
        last_block: 3,
        last_hash: Some(chain.hash(3)),
        ..Cursor::default()
    };
    let r = scan(&chain, &old, &targets(), &s).await.expect("rebuilt");
    assert!(r.restarted);
    assert_eq!(
        r.cursor,
        full_scan(&chain).await,
        "a full rescan, anchor recorded"
    );
    let again = scan(&chain, &r.cursor, &targets(), &s).await.expect("ok");
    assert!(!again.restarted, "only once");
    // An anchorless cursor that recorded only genesis has nothing to lose: kept quietly.
    let genesis_only = Cursor {
        last_block: 1,
        last_hash: Some(chain.hash(1)),
        ..Cursor::default()
    };
    let r = scan(&chain, &genesis_only, &targets(), &s)
        .await
        .expect("ok");
    assert!(!r.restarted);
    assert_eq!(r.cursor.last_block, 3, "continues after block 1");
    assert_eq!(r.cursor.anchor, Some(chain.hash(ANCHOR_BLOCK)));
}

#[tokio::test]
async fn cursor_above_head_rewinds_only_when_provable() {
    let mut chain = standard();
    chain.extend(15);
    let s = ScanSettings::default();
    let c = full_scan(&chain).await; // block 18, recent 8..=18
    // Within reach of the recorded hashes: rewinds to block 8 (provably unchanged).
    chain.truncate(12);
    let r = scan(&chain, &c, &targets(), &s).await.expect("rewinds");
    assert!(!r.restarted);
    assert_eq!(
        (r.cursor.last_block, r.cursor.last_hash),
        (12, Some(chain.hash(12)))
    );
    assert_eq!(r.cursor.found, c.found);
    // Out of reach: the rewind would land on block 5, which the cursor has no record of.
    chain.truncate(5);
    let e = scan(&chain, &c, &targets(), &s)
        .await
        .expect_err("cannot prove");
    assert_eq!(e, SdkError::ScanFork { at: 5 });
    // A cursor within `reorg_depth` of genesis rewinds to genesis, which is always complete.
    let short = scan(&chain, &Cursor::default(), &targets(), &s)
        .await
        .expect("ok")
        .cursor;
    chain.truncate(3);
    let r = scan(&chain, &short, &targets(), &s)
        .await
        .expect("to genesis");
    assert_eq!(r.cursor, full_scan(&chain).await);
}

#[tokio::test]
async fn fork_deeper_than_reorg_depth_fails() {
    let mut chain = standard();
    chain.extend(15);
    let s = ScanSettings::default();
    let c = full_scan(&chain).await;
    chain.rewrite_from(10, 7); // within reach: block 8 unchanged
    let shallow = scan(&chain, &c, &targets(), &s)
        .await
        .expect("shallow reorg")
        .cursor;
    assert_eq!(shallow.last_hash, Some(chain.hash(18)));
    chain.rewrite_from(5, 8); // block 8 changed too, block 2 did not
    let e = scan(&chain, &shallow, &targets(), &s)
        .await
        .expect_err("too deep");
    assert_eq!(e, SdkError::ScanFork { at: 8 });
}

#[tokio::test]
async fn missing_cursor_block_is_an_error() {
    let mut chain = standard();
    let c = full_scan(&chain).await;
    chain.hide_block(3);
    let e = scan(&chain, &c, &targets(), &ScanSettings::default())
        .await
        .expect_err("gap");
    assert_eq!(e, SdkError::MissingBlock(3));
}

#[tokio::test]
async fn cursor_unchanged_on_missing_block() {
    let mut chain = standard();
    chain.hide_block(2);
    let start = Cursor::default();
    let e = scan(&chain, &start, &targets(), &ScanSettings::default())
        .await
        .expect_err("gap");
    assert_eq!(e, SdkError::MissingBlock(2));
    assert_eq!(start, Cursor::default(), "not advanced");
}

#[tokio::test]
async fn scan_page_stops_at_max_blocks_and_reports_done() {
    let mut chain = standard();
    chain.extend(2); // head 5
    let s = ScanSettings::default();
    let p1 = scan_page(&chain, &Cursor::default(), &targets(), &s, 2)
        .await
        .expect("page 1");
    assert_eq!(
        (p1.scanned.cursor.last_block, p1.head, p1.done),
        (2, 5, false)
    );
    let p2 = scan_page(&chain, &p1.scanned.cursor, &targets(), &s, 2)
        .await
        .expect("page 2");
    assert_eq!((p2.scanned.cursor.last_block, p2.done), (4, false));
    let p3 = scan_page(&chain, &p2.scanned.cursor, &targets(), &s, 2)
        .await
        .expect("page 3");
    assert_eq!((p3.scanned.cursor.last_block, p3.done), (5, true));
    assert_eq!(
        p3.scanned.cursor,
        full_scan(&chain).await,
        "paging equals one scan"
    );
}

#[test]
fn collect_finds_liquidate_bid_surplus_debt() {
    let bid = EngineMsg::Bid {
        bidder: POSITION_B,
        config: CONFIG,
        collateral_def: COLLATERAL,
        position: POSITION_B,
        nonce: 3,
        max_debt_in: 1,
        min_collateral_out: 1,
        now_ms: 0,
    };
    let claim = EngineMsg::ClaimLeftover {
        config: CONFIG,
        position: POSITION_B,
        nonce: 4,
    };
    let empty = EngineMsg::SettleEmpty {
        config: CONFIG,
        position: POSITION_B,
        nonce: 5,
    };
    let surplus = EngineMsg::StartSurplus {
        config: CONFIG,
        nonce: 6,
        now_ms: 0,
    };
    let debt = EngineMsg::StartDebt {
        config: CONFIG,
        nonce: 7,
        now_ms: 0,
    };
    let block = block_with(vec![
        tx(&liquidate(CONFIG, POSITION_A, 2)),
        tx(&bid),
        tx(&claim),
        tx(&empty),
        tx(&surplus),
        tx(&debt),
    ]);
    let before = Found::default();
    let found = collect(&before, &block, &targets());
    assert_eq!(before, Found::default(), "the input is not mutated");
    assert_eq!(
        found.auctions,
        BTreeSet::from([
            (POSITION_A, 2),
            (POSITION_B, 3),
            (POSITION_B, 4),
            (POSITION_B, 5)
        ])
    );
    assert_eq!(
        found.positions,
        BTreeSet::from([POSITION_A]),
        "only liquidate names a position"
    );
    assert_eq!(found.surplus, BTreeSet::from([6]));
    assert_eq!(found.debt, BTreeSet::from([7]));
}

#[test]
fn collect_ignores_other_config() {
    let block = block_with(vec![
        tx(&liquidate(OTHER_CONFIG, POSITION_A, 0)),
        tx(&EngineMsg::StartDebt {
            config: OTHER_CONFIG,
            nonce: 1,
            now_ms: 0,
        }),
    ]);
    assert_eq!(
        collect(&Found::default(), &block, &targets()),
        Found::default()
    );
}

/// `SetSlack` and `Init` carry no config id (they name it by salt), so `collect` leaves them to the
/// wildcard arm: neither is a discovery event for the target config.
#[test]
fn collect_ignores_set_slack_and_init_root_calls() {
    let init = harness::World::bare().init_msg();
    let block = block_with(vec![
        tx(&EngineMsg::SetSlack {
            salt: 0,
            slack_ms: 30_000,
        }),
        tx(&init),
    ]);
    assert_eq!(
        collect(&Found::default(), &block, &targets()),
        Found::default()
    );
}

#[test]
fn collect_skips_reverted_tx() {
    let message = borsh::to_vec(&liquidate(CONFIG, POSITION_A, 0)).expect("encodes");
    let block = block_with(vec![tx_to(ENGINE, message, true)]);
    assert_eq!(
        collect(&Found::default(), &block, &targets()),
        Found::default()
    );
}

#[test]
fn collect_ignores_other_program() {
    let message = borsh::to_vec(&liquidate(CONFIG, POSITION_A, 0)).expect("encodes");
    let mut no_root = tx_to(ENGINE, message.clone(), false);
    no_root.root_to = None;
    let block = block_with(vec![
        tx_to(AccountId::new([0x77; 32]), message, false),
        no_root,
    ]);
    assert_eq!(
        collect(&Found::default(), &block, &targets()),
        Found::default()
    );
}

#[test]
fn collect_skips_undecodable_root_message() {
    let good = tx(&liquidate(CONFIG, POSITION_A, 1));
    let block = block_with(vec![
        tx_to(ENGINE, vec![0xFF, 1, 2], false),
        tx_to(ENGINE, vec![], false),
        good,
    ]);
    let found = collect(&Found::default(), &block, &targets());
    assert_eq!(
        found.auctions,
        BTreeSet::from([(POSITION_A, 1)]),
        "the bad ones are skipped"
    );
}

#[test]
fn cursor_round_trips_through_borsh() {
    let cursor = Cursor {
        last_block: 12,
        last_hash: Some([3; 32]),
        anchor: Some([4; 32]),
        recent: BTreeMap::from([(11, [5; 32]), (12, [3; 32])]),
        found: standard_found(),
    };
    let bytes = {
        let mut out = Vec::new();
        cursor.serialize(&mut out).expect("encodes");
        out
    };
    assert_eq!(
        borsh::from_slice::<Cursor>(&bytes).expect("decodes"),
        cursor
    );
    let fresh = borsh::to_vec(&Cursor::default()).expect("encodes");
    assert_eq!(
        borsh::from_slice::<Cursor>(&fresh).expect("decodes"),
        Cursor::default()
    );
}
