//! Discovery by block scan (spec §6.4, ADR-008, ADR-043): no account-listing RPC and no events exist
//! on v0.2.4. The scan keeps engine `liquidate` transactions that name our config (auction and
//! position ids) and host `open_position` transactions that name our `ProtocolParameters`
//! (position ids), and persists a cursor `{last_block, last_hash, auctions, positions}`.

use std::collections::BTreeSet;
use std::path::Path;

use auction_engine_core::Instruction;
use lee_core::{account::AccountId, program::ProgramId};
use serde::{Deserialize, Serialize};

use crate::{
    chain::{BlockSummary, Chain},
    encode::{variant_index, words},
    error::{SdkError, SdkResult},
    verify::VerifiedConfig,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub last_block: u64,
    /// Hex hash of `last_block` ("" before the first scan).
    pub last_hash: String,
    pub auctions: BTreeSet<String>,
    pub positions: BTreeSet<String>,
}

fn parse_ids(set: &BTreeSet<String>) -> SdkResult<Vec<AccountId>> {
    set.iter()
        .map(|s| {
            s.parse::<AccountId>()
                .map_err(|_| SdkError::Scan(format!("cursor holds an invalid account id {s}")))
        })
        .collect()
}

impl Cursor {
    pub fn auction_ids(&self) -> SdkResult<Vec<AccountId>> {
        parse_ids(&self.auctions)
    }

    pub fn position_ids(&self) -> SdkResult<Vec<AccountId>> {
        parse_ids(&self.positions)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanTargets {
    pub engine_program: ProgramId,
    pub config: AccountId,
    pub host_program: ProgramId,
    pub protocol_parameters: AccountId,
}

impl ScanTargets {
    #[must_use]
    pub fn of(v: &VerifiedConfig) -> Self {
        Self {
            engine_program: v.engine.program,
            config: v.config_id,
            host_program: v.host.program,
            protocol_parameters: v.host.protocol_parameters,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanSettings {
    pub batch: u64,
    pub reorg_depth: u64,
}

impl Default for ScanSettings {
    fn default() -> Self {
        Self {
            batch: 100,
            reorg_depth: 10,
        }
    }
}

fn index_of<T: serde::Serialize>(ix: &T) -> SdkResult<u32> {
    variant_index(&words(ix)?).ok_or_else(|| SdkError::Encode("empty instruction".into()))
}

fn collect(t: &ScanTargets, b: &BlockSummary, next: &mut Cursor, liquidate: u32, open: u32) {
    for tx in &b.txs {
        let kind = variant_index(&tx.instruction_words);
        if tx.program_id == t.engine_program
            && kind == Some(liquidate)
            && tx.account_ids.get(2) == Some(&t.config)
        {
            if let Some(a) = tx.account_ids.get(6) {
                next.auctions.insert(a.to_string());
            }
            if let Some(p) = tx.account_ids.get(4) {
                next.positions.insert(p.to_string());
            }
        }
        if tx.program_id == t.host_program
            && kind == Some(open)
            && tx.account_ids.contains(&t.protocol_parameters)
        {
            if let Some(p) = tx.account_ids.get(1) {
                next.positions.insert(p.to_string());
            }
        }
    }
}

fn hash_at<C: Chain + ?Sized>(chain: &C, id: u64) -> SdkResult<String> {
    if id == 0 {
        return Ok(String::new());
    }
    let b = chain.blocks(id, id)?;
    b.first()
        .map(|b| hex::encode(b.hash))
        .ok_or_else(|| SdkError::Chain(format!("block {id} not found")))
}

/// Where to rewind to, or `None` when the cursor block is still on the node with the stored hash.
/// A cursor above the node's head (a restarted localnet with fewer blocks) rewinds to
/// `min(last_block - reorg_depth, head)`. A cursor block within the head that the node cannot
/// return is a transient gap: the scan fails and the cursor is not advanced.
fn rewind_target<C: Chain + ?Sized>(
    chain: &C,
    head: u64,
    cursor: &Cursor,
    s: &ScanSettings,
) -> SdkResult<Option<u64>> {
    let n = cursor.last_block;
    if n == 0 {
        return Ok(None);
    }
    let target = n.saturating_sub(s.reorg_depth).min(head);
    if n > head {
        return Ok(Some(target));
    }
    let on_node = hash_at(chain, n).map_err(|e| {
        SdkError::Scan(format!(
            "cursor block {n} is not on the node ({e}); run `rescan`"
        ))
    })?;
    Ok((on_node != cursor.last_hash).then_some(target))
}

pub fn scan<C: Chain + ?Sized>(
    chain: &C,
    t: &ScanTargets,
    cursor: &Cursor,
    s: &ScanSettings,
) -> SdkResult<Cursor> {
    let liquidate = index_of(&Instruction::Liquidate { nonce: 0 })?;
    let open = index_of(&stablecoin_core::Instruction::OpenPosition {
        position_nonce: 0,
        initial_collateral_amount: 0,
    })?;
    let head = chain.last_block_id()?;
    let mut next = cursor.clone();
    if let Some(target) = rewind_target(chain, head, cursor, s)? {
        // The node no longer has the block we stopped at: rewind and rescan (sets never duplicate).
        next.last_block = target;
        next.last_hash = hash_at(chain, target).map_err(|e| {
            SdkError::Scan(format!(
                "cannot rewind to block {target}: {e}; run `rescan`"
            ))
        })?;
    }
    let batch = s.batch.max(1);
    let mut from = next.last_block + 1;
    while from <= head {
        let to = head.min(from.saturating_add(batch - 1));
        let blocks = chain.blocks(from, to)?;
        let expected =
            usize::try_from(to - from + 1).map_err(|_| SdkError::Scan("range too large".into()))?;
        if blocks.len() != expected {
            return Err(SdkError::Scan(format!(
                "the node returned {} blocks for {from}..={to}",
                blocks.len()
            )));
        }
        for (i, b) in blocks.iter().enumerate() {
            let want = from + u64::try_from(i).map_err(|_| SdkError::Scan("index".into()))?;
            if b.id != want {
                return Err(SdkError::Scan(format!(
                    "expected block {want}, got {}",
                    b.id
                )));
            }
            if !next.last_hash.is_empty() && hex::encode(b.prev_hash) != next.last_hash {
                return Err(SdkError::Scan(format!(
                    "block {} does not extend the scanned chain; run `rescan`",
                    b.id
                )));
            }
            collect(t, b, &mut next, liquidate, open);
            next.last_block = b.id;
            next.last_hash = hex::encode(b.hash);
        }
        from = to + 1;
    }
    Ok(next)
}

pub fn load_cursor(path: &Path) -> SdkResult<Cursor> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Cursor::default()),
        Err(e) => {
            return Err(SdkError::Io {
                path: path.display().to_string(),
                reason: e.to_string(),
            })
        }
    };
    serde_json::from_str(&text).map_err(|e| SdkError::Io {
        path: path.display().to_string(),
        reason: format!("corrupt cursor ({e}); delete it or run `rescan --from 1`"),
    })
}

/// Per-process, per-call suffix so concurrent writers never share a temporary file.
fn unique_suffix() -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{}-{nanos}-{n}", std::process::id())
}

/// Atomic: write a unique temp file next to `path`, then rename over `path`.
pub fn save_cursor(path: &Path, c: &Cursor) -> SdkResult<()> {
    let io = |e: std::io::Error| SdkError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let tmp = path.with_extension(format!("json.{}.tmp", unique_suffix()));
    let text = serde_json::to_string_pretty(c).map_err(|e| SdkError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    let written = std::fs::File::create(&tmp).and_then(|mut f| {
        std::io::Write::write_all(&mut f, text.as_bytes())?;
        f.sync_all()
    });
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(io(e));
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io(e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::standard;
    use crate::verify::verify_config;

    fn setup() -> (standard::Fixture, ScanTargets) {
        let fx = standard::standard();
        let v = verify_config(&fx.chain, &fx.deployment, &standard::config_id()).expect("verifies");
        (fx, ScanTargets::of(&v))
    }

    #[test]
    fn scanner_finds_the_standard_auction_and_positions_incrementally() {
        let (fx, t) = setup();
        let c = scan(&fx.chain, &t, &Cursor::default(), &ScanSettings::default()).expect("scan");
        assert_eq!(c.last_block, 3);
        assert_eq!(c.auction_ids().expect("ids"), vec![fx.live_auction]);
        let mut positions = c.position_ids().expect("ids");
        positions.sort();
        let mut expected = vec![fx.unsafe_position, fx.auction_position];
        expected.sort();
        assert_eq!(positions, expected);
        let again = scan(&fx.chain, &t, &c, &ScanSettings::default()).expect("no new blocks");
        assert_eq!(again, c);
        let small = ScanSettings {
            batch: 1,
            ..ScanSettings::default()
        };
        assert_eq!(
            scan(&fx.chain, &t, &Cursor::default(), &small).expect("batch 1"),
            c
        );
    }

    #[test]
    fn scanner_rewinds_when_the_cursor_block_hash_changed() {
        let (fx, t) = setup();
        let c = scan(&fx.chain, &t, &Cursor::default(), &ScanSettings::default()).expect("scan");
        fx.chain.rewrite_from(2, 7); // the node rebuilt blocks 2..=3 (e.g. a restarted localnet)
        let after = scan(&fx.chain, &t, &c, &ScanSettings::default()).expect("rescan");
        assert_ne!(after.last_hash, c.last_hash);
        assert_eq!(
            after.auctions, c.auctions,
            "nothing missed, nothing duplicated"
        );
        assert_eq!(after.last_block, 3);
    }

    fn hash_hex(fx: &standard::Fixture, id: u64) -> String {
        hex::encode(fx.chain.blocks(id, id).expect("block")[0].hash)
    }

    #[test]
    fn rewind_arithmetic_is_exact_and_follows_the_rebuilt_chain() {
        let (fx, t) = setup();
        let c = scan(&fx.chain, &t, &Cursor::default(), &ScanSettings::default()).expect("scan");
        fx.chain.rewrite_from(3, 7);
        let one = ScanSettings {
            reorg_depth: 1,
            ..ScanSettings::default()
        };
        let after = scan(&fx.chain, &t, &c, &one).expect("rescan from block 2");
        assert_eq!(after.last_block, 3);
        assert_eq!(after.last_hash, hash_hex(&fx, 3));
        assert_ne!(after.last_hash, c.last_hash);
        assert_eq!(after.auctions, c.auctions);
        // A fork below the rewind point is not detected: the rewind point is re-read from the node,
        // so the scan stays consistent but blocks at or below it are not rescanned (ADR-043).
        fx.chain.rewrite_from(1, 8);
        let deep = scan(&fx.chain, &t, &after, &one).expect("consistent on the new chain");
        assert_eq!(deep.last_hash, hash_hex(&fx, 3));
    }

    #[test]
    fn a_cursor_above_the_node_head_rewinds_to_the_head_or_below() {
        let (fx, t) = setup();
        let full = scan(&fx.chain, &t, &Cursor::default(), &ScanSettings::default()).expect("scan");
        let ahead = Cursor {
            last_block: 10,
            last_hash: "ab".into(),
            ..Cursor::default()
        };
        let shallow = ScanSettings {
            reorg_depth: 2,
            ..ScanSettings::default()
        };
        let a = scan(&fx.chain, &t, &ahead, &shallow).expect("rewinds to the head");
        assert_eq!((a.last_block, a.last_hash.clone()), (3, hash_hex(&fx, 3)));
        let deep =
            scan(&fx.chain, &t, &ahead, &ScanSettings::default()).expect("rewinds to genesis");
        assert_eq!(deep, full, "a full rescan finds everything");
    }

    #[test]
    fn a_missing_cursor_block_within_the_head_is_an_actionable_error() {
        let (fx, t) = setup();
        let c = scan(&fx.chain, &t, &Cursor::default(), &ScanSettings::default()).expect("scan");
        fx.chain.hide_block(3);
        let e = scan(&fx.chain, &t, &c, &ScanSettings::default()).expect_err("gap");
        assert!(
            matches!(&e, SdkError::Scan(m) if m.contains("cursor block 3") && m.contains("rescan"))
        );
    }

    #[test]
    fn scanner_leaves_the_cursor_unchanged_on_a_missing_block() {
        let (fx, t) = setup();
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("cursor.json");
        save_cursor(&path, &Cursor::default()).expect("save");
        fx.chain.hide_block(2);
        let start = load_cursor(&path).expect("load");
        let e = scan(&fx.chain, &t, &start, &ScanSettings::default()).expect_err("gap");
        assert!(matches!(e, SdkError::Chain(m) if m.contains("block 2")));
        assert_eq!(
            load_cursor(&path).expect("load"),
            Cursor::default(),
            "not advanced"
        );
    }

    #[test]
    fn cursor_write_is_atomic_and_a_corrupt_cursor_is_an_error() {
        let (fx, t) = setup();
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("cursor.json");
        assert_eq!(
            load_cursor(&path).expect("missing = genesis"),
            Cursor::default()
        );
        let c = scan(&fx.chain, &t, &Cursor::default(), &ScanSettings::default()).expect("scan");
        save_cursor(&path, &c).expect("save");
        assert_eq!(load_cursor(&path).expect("round trip"), c);
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .expect("dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec!["cursor.json".to_owned()],
            "no temporary file left behind"
        );
        std::fs::write(dir.path().join("cursor.json.tmp"), "garbage").expect("stray temp");
        save_cursor(&path, &c).expect("a stray temp file is harmless");
        assert_eq!(load_cursor(&path).expect("reload"), c);
        std::fs::remove_file(dir.path().join("cursor.json.tmp")).expect("rm stray");
        std::fs::write(&path, "{ half").expect("write");
        let e = load_cursor(&path).expect_err("corrupt");
        assert!(matches!(e, SdkError::Io { ref path, .. } if path.ends_with("cursor.json")));
    }

    #[test]
    fn a_failed_save_leaves_no_temporary_file() {
        // The target is a non-empty directory, so the final rename fails (portable stand-in for a
        // failure after the temp file was written; read-only directories are unreliable on /mnt/c).
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("cursor.json");
        std::fs::create_dir(&path).expect("dir");
        std::fs::write(path.join("keep"), "x").expect("file");
        let e = save_cursor(&path, &Cursor::default()).expect_err("rename fails");
        assert!(matches!(e, SdkError::Io { .. }));
        assert!(
            path.join("keep").exists(),
            "the existing target is untouched"
        );
        let names: Vec<_> = std::fs::read_dir(dir.path()).expect("dir").collect();
        assert_eq!(names.len(), 1, "no temporary file left behind");
    }
}
