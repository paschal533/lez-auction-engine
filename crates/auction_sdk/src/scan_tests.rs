//! Scanner tests against the standard fixture (moved out of `scan.rs` to keep it focused).

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
    fx.chain.rewrite_from(3, 7); // the node rebuilt block 3 (a shallow reorg)
    let after = scan_report(&fx.chain, &t, &c, &ScanSettings::default()).expect("rescan");
    assert_eq!(after.restarted, None, "same chain: no restart");
    let after = after.cursor;
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

fn extend(fx: &standard::Fixture, blocks: u64) {
    for _ in 0..blocks {
        fx.chain.push_block(vec![]);
    }
}

fn full_scan(fx: &standard::Fixture, t: &ScanTargets) -> Cursor {
    scan(&fx.chain, t, &Cursor::default(), &ScanSettings::default()).expect("full scan")
}

fn is_rescan_from_1(e: &SdkError) -> bool {
    matches!(e, SdkError::Scan(m) if m.contains("run `rescan --from 1`"))
}

#[test]
fn rewind_arithmetic_is_exact_and_follows_the_rebuilt_chain() {
    let (fx, t) = setup();
    let c = full_scan(&fx, &t);
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
    assert_eq!(after.recent.keys().copied().collect::<Vec<_>>(), vec![2, 3]);
    // Rebuilt from block 1: the anchor (block 2) changed, so the whole chain is rescanned.
    fx.chain.rewrite_from(1, 8);
    let deep = scan_report(&fx.chain, &t, &after, &one).expect("rebuilt");
    assert_eq!(deep.restarted, Some(Restart::ChainReplaced));
    assert_eq!(deep.cursor.last_hash, hash_hex(&fx, 3));
    assert_eq!(deep.cursor.anchor, hash_hex(&fx, ANCHOR_BLOCK));
    assert_eq!(deep.cursor.auctions, c.auctions);
}

#[test]
fn a_replaced_chain_shorter_than_the_cursor_is_rescanned_from_block_1() {
    // The cursor of an old, long chain; the node now serves a new chain of 3 blocks whose
    // block 2 differs and which holds a liquidation the old cursor never saw.
    let (fx, t) = setup();
    let old = Cursor {
        last_block: 500,
        last_hash: "aa".repeat(32),
        anchor: "bb".repeat(32),
        recent: BTreeMap::from([(490, "cc".repeat(32))]),
        ..Cursor::default()
    };
    let r = scan_report(&fx.chain, &t, &old, &ScanSettings::default()).expect("restarts");
    assert_eq!(r.restarted, Some(Restart::ChainReplaced));
    assert_eq!(r.cursor, full_scan(&fx, &t));
    assert_eq!(r.cursor.auction_ids().expect("ids"), vec![fx.live_auction]);
}

#[test]
fn a_replaced_chain_longer_than_the_cursor_is_rescanned_from_block_1() {
    // Head 20 is above `last_block - reorg_depth` (15 - 10): without the anchor the scan would
    // rewind to block 5, take its hash from the new node and never see blocks 1..=5.
    let (fx, t) = setup();
    extend(&fx, 17);
    let old = Cursor {
        last_block: 15,
        last_hash: "aa".repeat(32),
        anchor: "bb".repeat(32),
        ..Cursor::default()
    };
    let r = scan_report(&fx.chain, &t, &old, &ScanSettings::default()).expect("restarts");
    assert_eq!(r.restarted, Some(Restart::ChainReplaced));
    assert_eq!(r.cursor.last_block, 20);
    assert_eq!(r.cursor.auction_ids().expect("ids"), vec![fx.live_auction]);
    // The same through a real rebuild: the node restarted and produced more blocks since.
    let c = r.cursor;
    fx.chain.rewrite_from(1, 9);
    extend(&fx, 5);
    let r = scan_report(&fx.chain, &t, &c, &ScanSettings::default()).expect("restarts");
    assert_eq!(r.restarted, Some(Restart::ChainReplaced));
    assert_eq!(r.cursor, full_scan(&fx, &t));
}

#[test]
fn a_matching_anchor_scans_incrementally() {
    let (fx, t) = setup();
    let c = full_scan(&fx, &t);
    assert_eq!(c.anchor, hash_hex(&fx, ANCHOR_BLOCK));
    extend(&fx, 2);
    // Block 1 is unreadable now: only an incremental scan (blocks 4..=5) can succeed.
    fx.chain.hide_block(1);
    let r = scan_report(&fx.chain, &t, &c, &ScanSettings::default()).expect("incremental");
    assert_eq!(r.restarted, None);
    assert_eq!(r.cursor.last_block, 5);
    assert_eq!(r.cursor.anchor, c.anchor);
    assert_eq!(r.cursor.auctions, c.auctions);
}

#[test]
fn a_cursor_without_an_anchor_loads_and_is_rebuilt_once() {
    let (fx, t) = setup();
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("cursor.json");
    // Written before anchors existed: no `anchor`, no `recent`.
    std::fs::write(
        &path,
        format!(
            r#"{{"last_block":3,"last_hash":"{}","auctions":[],"positions":[]}}"#,
            hash_hex(&fx, 3)
        ),
    )
    .expect("write");
    let old = load_cursor(&path).expect("old cursors still load");
    assert_eq!((old.anchor.as_str(), old.recent.len()), ("", 0));
    let r = scan_report(&fx.chain, &t, &old, &ScanSettings::default()).expect("rebuilt");
    assert_eq!(r.restarted, Some(Restart::NoAnchor));
    assert_eq!(
        r.cursor,
        full_scan(&fx, &t),
        "a full rescan, anchor recorded"
    );
    let again = scan_report(&fx.chain, &t, &r.cursor, &ScanSettings::default()).expect("ok");
    assert_eq!(again.restarted, None, "only once");
    // A cursor without an anchor that recorded only genesis has nothing to lose: kept quietly.
    let genesis_only = Cursor {
        last_block: 1,
        last_hash: hash_hex(&fx, 1),
        ..Cursor::default()
    };
    let r = scan_report(&fx.chain, &t, &genesis_only, &ScanSettings::default()).expect("ok");
    assert_eq!(r.restarted, None);
    assert_eq!(r.cursor.last_block, 3, "continues after block 1");
    assert_eq!(r.cursor.anchor, hash_hex(&fx, ANCHOR_BLOCK));
}

#[test]
fn a_cursor_above_the_head_of_the_same_chain_rewinds_only_when_provable() {
    let (fx, t) = setup();
    extend(&fx, 15);
    let c = full_scan(&fx, &t); // block 18, recent 8..=18
                                // Within reach of the recorded hashes: rewinds to block 8 (provably unchanged).
    fx.chain.truncate(12);
    let r = scan_report(&fx.chain, &t, &c, &ScanSettings::default()).expect("rewinds");
    assert_eq!(r.restarted, None);
    assert_eq!(
        (r.cursor.last_block, r.cursor.last_hash.clone()),
        (12, hash_hex(&fx, 12))
    );
    assert_eq!(r.cursor.auctions, c.auctions);
    // Out of reach: the rewind would land on block 5, which the cursor has no record of.
    fx.chain.truncate(5);
    let e = scan(&fx.chain, &t, &c, &ScanSettings::default()).expect_err("cannot prove");
    assert!(is_rescan_from_1(&e), "{e}");
    assert!(e.to_string().contains("head is 5"), "{e}");
    // A cursor within `reorg_depth` of genesis rewinds to genesis, which is always complete.
    let short = scan(&fx.chain, &t, &Cursor::default(), &ScanSettings::default()).expect("ok");
    fx.chain.truncate(3);
    let r = scan(&fx.chain, &t, &short, &ScanSettings::default()).expect("to genesis");
    assert_eq!(r, full_scan(&fx, &t));
}

#[test]
fn a_fork_deeper_than_reorg_depth_on_the_same_chain_fails() {
    let (fx, t) = setup();
    extend(&fx, 15);
    let c = full_scan(&fx, &t);
    fx.chain.rewrite_from(10, 7); // within reach: block 8 unchanged
    let shallow = scan(&fx.chain, &t, &c, &ScanSettings::default()).expect("shallow reorg");
    assert_eq!(shallow.last_hash, hash_hex(&fx, 18));
    fx.chain.rewrite_from(5, 8); // block 8 changed too, block 2 did not
    let e = scan(&fx.chain, &t, &shallow, &ScanSettings::default()).expect_err("too deep");
    assert!(is_rescan_from_1(&e), "{e}");
}

#[test]
fn start_at_discards_a_cursor_of_another_chain() {
    let (fx, t) = setup();
    let c = full_scan(&fx, &t);
    let same = start_at(&fx.chain, &c, 3).expect("positioned");
    assert_eq!(same.restarted, None);
    assert_eq!(same.cursor.auctions, c.auctions, "keeps its discoveries");
    assert_eq!(same.cursor.recent, BTreeMap::from([(2, hash_hex(&fx, 2))]));
    let foreign = Cursor {
        anchor: "bb".repeat(32),
        ..c.clone()
    };
    let other = start_at(&fx.chain, &foreign, 3).expect("positioned");
    assert_eq!(other.restarted, Some(Restart::ChainReplaced));
    assert!(other.cursor.auctions.is_empty());
    assert_eq!(other.cursor.anchor, c.anchor);
    let resumed = scan(&fx.chain, &t, &other.cursor, &ScanSettings::default()).expect("scan");
    assert_eq!(resumed.last_block, 3);
}

#[test]
fn start_at_resets_an_anchorless_cursor_past_the_anchor_block() {
    // Plan 3a N2: an old-format cursor may belong to a replaced chain; `rescan --from N` must
    // treat it like `scan` does (NoAnchor reset), not keep its ids and hide the skipped blocks.
    let (fx, t) = setup();
    let c = full_scan(&fx, &t);
    let legacy = Cursor {
        anchor: String::new(),
        ..c.clone()
    };
    let placed = start_at(&fx.chain, &legacy, 3).expect("positioned");
    assert_eq!(placed.restarted, Some(Restart::NoAnchor));
    assert!(
        placed.cursor.auctions.is_empty(),
        "ids of an unknown chain dropped"
    );
    let early = Cursor {
        last_block: 1,
        ..Cursor::default()
    };
    assert_eq!(
        start_at(&fx.chain, &early, 2)
            .expect("positioned")
            .restarted,
        None,
        "an anchorless cursor that saw only block 1 cannot have missed anything"
    );
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
