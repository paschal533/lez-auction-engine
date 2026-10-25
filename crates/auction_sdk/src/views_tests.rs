//! Views against the standard fixture: the paged scan and its bid counts, wallet labels, CR, the
//! discount schedule, the lists the boards render, and the account picker rows.

use std::collections::BTreeMap;
use std::path::Path;

use auction_core::units::bps_to_ray;
use auction_engine_core::{state::AuctionState, Instruction};
use lee_core::account::AccountId;
use stablecoin_core::Position;

use super::*;
use crate::{
    chain::TxSummary,
    decode::Reader,
    encode::words,
    keystore::Keystore,
    oracle::check_new_price,
    quote::{self, discount_schedule},
    scan::{self, Cursor, Restart, ScanSettings, ScanTargets},
    testing::{
        id, keystore_json,
        standard::{self, k, R, T0},
        STANDARD_LABELS,
    },
    verify::{verify_config, VerifiedConfig},
};

struct Setup {
    fx: standard::Fixture,
    v: VerifiedConfig,
    t: ScanTargets,
}

fn setup() -> Setup {
    let fx = standard::standard();
    let v = verify_config(&fx.chain, &fx.deployment, &standard::config_id()).expect("verifies");
    let t = ScanTargets::of(&v);
    Setup { fx, v, t }
}

fn full_scan(s: &Setup) -> Cursor {
    scan::scan(
        &s.fx.chain,
        &s.t,
        &Cursor::default(),
        &ScanSettings::default(),
    )
    .expect("scan")
}

fn page(s: &Setup, cursor: &Cursor, max_blocks: u64) -> scan::ScanPage {
    scan::scan_page(
        &s.fx.chain,
        &s.t,
        cursor,
        &ScanSettings::default(),
        max_blocks,
    )
    .expect("page")
}

#[test]
fn scan_page_stops_after_max_blocks() {
    let s = setup();
    for _ in 0..297 {
        s.fx.chain.push_block(vec![]);
    }
    let first = page(&s, &Cursor::default(), 100);
    assert_eq!(
        (first.head, first.done, first.scanned.cursor.last_block),
        (300, false, 100)
    );
    assert_eq!(first.scanned.restarted, None);
    assert_eq!(
        first.scanned.cursor.auction_ids().expect("ids"),
        vec![s.fx.live_auction],
        "block 3 is in the first page"
    );
    let mut cursor = first.scanned.cursor;
    let mut pages = 1;
    loop {
        let next = page(&s, &cursor, 100);
        pages += 1;
        cursor = next.scanned.cursor;
        if next.done {
            break;
        }
    }
    assert_eq!(pages, 3);
    assert_eq!(
        cursor,
        full_scan(&s),
        "paging ends where one full scan ends"
    );
}

#[test]
fn scan_page_refuses_a_page_of_zero_blocks() {
    let s = setup();
    let e = scan::scan_page(
        &s.fx.chain,
        &s.t,
        &Cursor::default(),
        &ScanSettings::default(),
        0,
    )
    .expect_err("a 0-block page never advances");
    assert!(
        matches!(&e, crate::error::SdkError::Scan(m) if m.contains("max_blocks")),
        "{e}"
    );
}

#[test]
fn scan_page_reports_a_replaced_chain() {
    let s = setup();
    let old = full_scan(&s);
    assert!(!old.auctions.is_empty());
    s.fx.chain.rewrite_from(1, 9); // the localnet restarted: block 2 (the anchor) differs
    let p = page(&s, &old, 1);
    assert_eq!(p.scanned.restarted, Some(Restart::ChainReplaced));
    assert_eq!((p.scanned.cursor.last_block, p.done), (1, false));
    assert!(
        p.scanned.cursor.auctions.is_empty() && p.scanned.cursor.bids.is_empty(),
        "no id of the old chain survives"
    );
    let listed = auctions(&s.fx.chain, &s.v, &p.scanned.cursor, T0).expect("list");
    assert!(listed.items.is_empty(), "the list is empty, not stale");
}

#[test]
fn old_cursor_files_load_without_bids() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("cursor.json");
    std::fs::write(
        &path,
        r#"{"last_block":3,"last_hash":"ab","anchor":"cd","recent":{},"auctions":[],"positions":[]}"#,
    )
    .expect("write");
    let c = scan::load_cursor(&path).expect("an old cursor still loads");
    assert_eq!(c.last_block, 3);
    assert!(c.bids.is_empty());
}

fn bid_tx(s: &Setup, n: u8, config: AccountId, auction: AccountId, settle: bool) -> TxSummary {
    let (coin_in_max, min_collateral_out) = (100_000, 0);
    let ix = if settle {
        Instruction::BidWithSettlement {
            coin_in_max,
            min_collateral_out,
        }
    } else {
        Instruction::Bid {
            coin_in_max,
            min_collateral_out,
        }
    };
    TxSummary {
        hash: [n; 32],
        program_id: standard::ENGINE,
        account_ids: vec![
            id(k::BIDDER_STABLE),
            id(k::BIDDER_COLL),
            config,
            s.v.coll_config_id(),
            auction,
        ],
        instruction_words: words(&ix).expect("encodes"),
    }
}

#[test]
fn bids_are_counted_per_auction() {
    let s = setup();
    let a = s.fx.live_auction;
    let other = AccountId::new([0x6A; 32]);
    let foreign_config = AccountId::new([0x42; 32]);
    s.fx.chain.push_block(vec![
        bid_tx(&s, 0xB0, s.v.config_id, a, false),
        bid_tx(&s, 0xB1, s.v.config_id, other, false),
        bid_tx(&s, 0xB2, foreign_config, a, false),
    ]);
    s.fx.chain
        .push_block(vec![bid_tx(&s, 0xB3, s.v.config_id, a, true)]);
    let c = full_scan(&s);
    let want = BTreeMap::from([(a.to_string(), 2), (other.to_string(), 1)]);
    assert_eq!(
        c.bids, want,
        "bid and bid_with_settlement of our config only"
    );
    // A shallow reorg (blocks 4..=5 rebuilt with the same transactions) does not count them twice.
    s.fx.chain.rewrite_from(4, 7);
    let again = scan::scan(&s.fx.chain, &s.t, &c, &ScanSettings::default()).expect("rescan");
    assert_eq!(again.bids, want);
    // Neither does `rescan --from 1` on the same chain.
    let placed = scan::start_at(&s.fx.chain, &again, 1).expect("placed");
    let from_1 =
        scan::scan(&s.fx.chain, &s.t, &placed.cursor, &ScanSettings::default()).expect("scan");
    assert_eq!(from_1.bids, want);
    let listed = auctions(&s.fx.chain, &s.v, &from_1, T0).expect("list");
    match listed.items.as_slice() {
        [AuctionEntry::Live(l)] => assert_eq!(l.bids, 2),
        other => panic!("one live auction expected, got {other:?}"),
    }
}

#[test]
fn labels_lists_every_wallet_label() {
    let ks = Keystore::parse(
        &keystore_json(&STANDARD_LABELS),
        Path::new("/w/storage.json"),
    )
    .expect("parses");
    let listed: BTreeMap<String, AccountId> =
        ks.labels().map(|(l, i)| (l.to_owned(), *i)).collect();
    let want: BTreeMap<String, AccountId> = STANDARD_LABELS
        .iter()
        .map(|(l, n)| ((*l).to_owned(), id(*n)))
        .collect();
    assert_eq!(listed.len(), 14);
    assert_eq!(listed, want);
    // A label of a private account is not offered: it can never sign a public transaction.
    let mut json: serde_json::Value =
        serde_json::from_str(&keystore_json(&STANDARD_LABELS)).expect("json");
    json["labels"]["secret"] = serde_json::json!({ "Private": id(70).to_string() });
    let ks = Keystore::parse(&json.to_string(), Path::new("/w/storage.json")).expect("parses");
    assert!(ks.labels().all(|(l, _)| l != "secret"));
    assert_eq!(ks.labels().count(), 14);
}

fn put_debt(s: &Setup, position: AccountId, owner: AccountId, nonce: u64, coll: u128, debt: u128) {
    s.fx.chain.put_state(
        position,
        standard::HOST,
        &Position {
            owner_account_id: owner,
            position_nonce: nonce,
            vault_account_id: s.fx.host.position_vault(&position),
            collateral_amount: coll,
            normalized_debt_amount: debt,
            opened_at: T0 - 1_000_000,
        },
    );
}

#[test]
fn cr_bps_agrees_with_the_host_check() {
    let s = setup();
    let p = s.fx.unsafe_position;
    // Collateral 500_000 at R = 0.5 and min CR 2.0: the host's boundary is a debt of 500_000.
    for debt in [
        1,
        250_000,
        333_333,
        499_999,
        500_000,
        500_001,
        600_000,
        1_000_000_007,
        u128::MAX,
    ] {
        put_debt(&s, p, id(k::OWNER), 0, 500_000, debt);
        let h = quote::position_health(&s.fx.chain, &s.v, &p, T0).expect("health");
        let cr = h.cr_bps.expect("debt > 0 has a CR");
        assert_eq!(h.min_cr_bps, 20_000);
        assert_eq!(
            cr < h.min_cr_bps,
            h.unsafe_at_min_cr,
            "debt {debt}: cr {cr} bps vs the host's verdict"
        );
    }
    put_debt(&s, p, id(k::OWNER), 0, 500_000, 600_000);
    let h = quote::position_health(&s.fx.chain, &s.v, &p, T0).expect("health");
    assert_eq!(
        h.cr_bps,
        Some(16_666),
        "500_000 / (600_000 x 0.5) = 166.66 %"
    );
    let zero =
        quote::position_health(&s.fx.chain, &s.v, &s.fx.auction_position, T0).expect("health");
    assert_eq!(zero.cr_bps, None, "no debt, no CR");
}

#[test]
fn discount_schedule_matches_the_engine() {
    // The standard fixture's schedule is the demo's (cli/params/demo-collateral.json: 2 % at 0,
    // 10 % at 45 min).
    let s = setup();
    let st: AuctionState = Reader::new(&s.fx.chain, standard::ENGINE)
        .auction(&s.fx.live_auction)
        .expect("auction");
    let engine = st.to_core().expect("core").schedule;
    let points = discount_schedule(&st, 48).expect("schedule");
    assert!(points.len() >= 48);
    assert_eq!(points.first(), Some(&(0, bps_to_ray(200))));
    assert!(
        points.contains(&(2_700_000, bps_to_ray(1_000))),
        "the knee is a sample"
    );
    assert_eq!(points.last(), Some(&(7_200_000, bps_to_ray(1_000))));
    assert!(points.windows(2).all(|w| w[0].0 < w[1].0), "time increases");
    for (t, d) in &points {
        assert_eq!(*d, engine.discount_ray(*t), "t = {t}");
        if *t >= 2_700_000 {
            assert_eq!(*d, bps_to_ray(1_000), "flat after 45 min, t = {t}");
        }
    }
}

#[test]
fn at_risk_is_sorted_lowest_cr_first() {
    let s = setup();
    let host = &s.fx.host;
    let low = host.position(&id(k::OWNER), 1);
    let safe = host.position(&id(k::OWNER2), 1);
    put_debt(&s, low, id(k::OWNER), 1, 500_000, 900_000); // 111.11 %
    put_debt(&s, safe, id(k::OWNER2), 1, 500_000, 300_000); // 333.33 %
    let mut cursor = full_scan(&s);
    cursor.positions.insert(low.to_string());
    cursor.positions.insert(safe.to_string());

    let in_cursor_order = positions(&s.fx.chain, &s.v, &cursor, T0).expect("positions");
    let order: Vec<AccountId> = in_cursor_order.items.iter().map(|p| p.position).collect();
    let mut expected: Vec<AccountId> = cursor
        .position_ids()
        .expect("ids")
        .into_iter()
        .filter(|p| *p != s.fx.auction_position) // no debt: not at risk
        .collect();
    assert_eq!(order, expected, "positions() keeps the cursor order");

    let sorted = at_risk(&s.fx.chain, &s.v, &cursor, T0).expect("at risk");
    let rows: Vec<(AccountId, u64, bool)> = sorted
        .items
        .iter()
        .map(|p| (p.position, p.cr_bps, p.liquidatable))
        .collect();
    assert_eq!(
        rows,
        vec![
            (low, 11_111, true),
            (s.fx.unsafe_position, 16_666, true),
            (safe, 33_333, false)
        ]
    );
    assert!(sorted.items.iter().all(|p| p.min_cr_bps == 20_000));
    expected.sort();
    let mut same: Vec<AccountId> = sorted.items.iter().map(|p| p.position).collect();
    same.sort();
    assert_eq!(same, expected, "the same rows in both orders");
}

fn settle(s: &Setup, debt_raised: u128, penalty_raised: u128, leftover: u128) {
    let r = Reader::new(&s.fx.chain, standard::ENGINE);
    let st = r.auction(&s.fx.live_auction).expect("auction");
    s.fx.chain.put_state(
        s.fx.live_auction,
        standard::ENGINE,
        &AuctionState {
            settled: true,
            collateral_sold: 475_000 - leftover,
            collateral_left: leftover,
            leftover_claimable: leftover,
            debt_raised,
            penalty_raised,
            ..st
        },
    );
}

fn principal(s: &Setup) -> u128 {
    Reader::new(&s.fx.chain, standard::ENGINE)
        .auction(&s.fx.live_auction)
        .expect("auction")
        .debt_principal
}

fn the_settled(s: &Setup) -> SettledAuctionView {
    let listed = auctions(&s.fx.chain, &s.v, &full_scan(s), T0).expect("list");
    match listed.items.as_slice() {
        [AuctionEntry::Settled(v)] => v.clone(),
        other => panic!("one settled auction expected, got {other:?}"),
    }
}

#[test]
fn settled_view_splits_debt_penalty_shortfall() {
    let s = setup();
    settle(&s, 500_000, 50_000, 0); // ended short: 550_000 of 660_000
    let v = the_settled(&s);
    let principal = principal(&s);
    assert_eq!(
        v.debt_repaid + v.shortfall,
        principal,
        "the shortfall is the debt left unrepaid"
    );
    assert_eq!(
        (v.debt_repaid, v.penalty),
        (500_000, 50_000),
        "the split of what was raised"
    );
    assert_eq!(
        (v.raised, v.target, v.shortfall, v.leftover),
        (550_000, 660_000, 100_000, 0)
    );
    assert_eq!((v.lot, v.sold), (475_000, 475_000));
    assert_eq!(
        (v.owner, v.position),
        (id(k::OWNER2), s.fx.auction_position)
    );
    let json = serde_json::to_value(&v).expect("json");
    assert_eq!(json["raised"], "550000", "u128 crosses as a decimal string");
    assert_eq!(json["shortfall"], "100000");

    settle(&s, 600_000, 60_000, 1_000); // target reached, collateral left over for the owner
    let v = the_settled(&s);
    assert_eq!(v.debt_repaid + v.shortfall, principal);
    assert_eq!((v.debt_repaid, v.penalty), (600_000, 60_000));
    assert_eq!(
        (v.raised, v.shortfall, v.leftover, v.claimed),
        (660_000, 0, 1_000, 0)
    );
}

#[test]
fn live_view_carries_the_board_figures() {
    let s = setup();
    let listed = auctions(&s.fx.chain, &s.v, &full_scan(&s), T0).expect("list");
    assert!(listed.skipped.is_empty());
    let l = match listed.items.as_slice() {
        [AuctionEntry::Live(l)] => l.clone(),
        other => panic!("one live auction expected, got {other:?}"),
    };
    let engine = AuctionState::to_core(
        &Reader::new(&s.fx.chain, standard::ENGINE)
            .auction(&s.fx.live_auction)
            .expect("auction"),
    )
    .expect("core")
    .schedule;
    assert_eq!(
        (l.lot, l.raised, l.target, l.collateral_left, l.reserved),
        (475_000, 0, 660_000, 475_000, 660_000)
    );
    assert_eq!((l.t_eff_ms, l.started_at_ms), (600_000, T0 - 600_000));
    assert_eq!(l.discount_ray, engine.discount_ray(600_000));
    assert_eq!(
        l.rate_per_min_ray,
        engine.discount_ray(660_000) - engine.discount_ray(600_000)
    );
    assert_eq!(l.max_at_ms, T0 + 2_100_000, "45 min after the start");
    assert_eq!(l.ends_at_ms, None, "no expiry (ADR-012)");
    assert!(!l.held_at_max && l.paused.is_none());
    assert_eq!((l.bids, l.position), (0, s.fx.auction_position));
    let json = serde_json::to_value(&l).expect("json");
    assert_eq!(
        json["discount_ray"],
        engine.discount_ray(600_000).to_string()
    );
}

#[test]
fn system_views_read_the_fixture() {
    let s = setup();
    let a = accounting(&s.fx.chain, &s.v).expect("accounting");
    assert_eq!(
        (
            a.buffer,
            a.unbacked_debt,
            a.debt_on_auction,
            a.freeze_threshold
        ),
        (2_000, 1_000, 500, 1_000)
    );
    assert_eq!(
        (a.surplus_lot_left, a.debt_lot_left),
        (Some(500), Some(500))
    );
    assert_eq!(
        (a.surplus_live, a.surplus_nonce, a.debt_live, a.debt_nonce),
        (true, 1, true, 1)
    );
    assert_eq!(a.earliest_debt_start_ms, None);
    assert_eq!(serde_json::to_value(&a).expect("json")["buffer"], "2000");

    let h = host(&s.fx.chain, &s.fx.host).expect("host");
    assert_eq!(
        (h.frozen, h.admin, h.market_oracle, h.min_cr_ray),
        (
            false,
            id(k::HOST_ADMIN),
            standard::ORACLE,
            2 * standard::ONE
        )
    );
    assert_eq!(h.max_oracle_age_ms, 900_000);

    let o = oracle(&s.fx.chain, &s.v, T0).expect("oracle");
    assert_eq!((o.price_ray, o.age_ms, o.fresh), (R, 0, true));
    assert_eq!((o.range_lo_ray, o.range_hi_ray), (R / 2, 2 * R));
    let coll = s.v.coll(&s.fx.chain).expect("coll");
    assert_eq!(o.max_age_ms, coll.max_oracle_age_ms);
    // The range is the engine's own: its ends pass the feed check, one unit outside does not.
    let probe = |price: u128| {
        check_new_price(
            &s.fx.chain,
            &s.v,
            AccountId::new([0x74; 32]),
            id(k::CALLER),
            price,
            T0,
        )
    };
    probe(o.range_lo_ray).expect("low end is valid");
    probe(o.range_hi_ray).expect("high end is valid");
    probe(o.range_lo_ray - 1).expect_err("below the range");
    probe(o.range_hi_ray + 1).expect_err("above the range");
    let stale = oracle(&s.fx.chain, &s.v, T0 + coll.max_oracle_age_ms + 1).expect("oracle");
    assert!(!stale.fresh);
}

fn row<'a>(rows: &'a [AccountView], label: &str) -> &'a AccountView {
    rows.iter()
        .find(|r| r.label == label)
        .unwrap_or_else(|| panic!("no row {label}"))
}

#[test]
fn accounts_attach_holdings_to_roles() {
    let s = setup();
    let labels: Vec<(String, AccountId)> = STANDARD_LABELS
        .iter()
        .map(|(l, n)| ((*l).to_owned(), id(*n)))
        .collect();
    let rows = accounts(&s.fx.chain, &s.v, &labels).expect("accounts");
    let names: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "owner",
            "keeper",
            "caller",
            "taker",
            "engine-admin",
            "bidder",
            "freeze",
            "host-admin",
            "owner2"
        ],
        "roles first, in the picker's order; holdings never listed alone"
    );
    let owner = row(&rows, "owner");
    assert_eq!(
        (owner.role, owner.id, owner.holding.as_deref()),
        (Some("owner"), id(k::OWNER), Some("owner-stable"))
    );
    assert_eq!((owner.stable_balance, owner.coll_balance), (None, Some(0)));
    let keeper = row(&rows, "keeper");
    assert_eq!(keeper.holding.as_deref(), Some("keeper-coll"));
    let taker = row(&rows, "taker");
    assert_eq!(
        (taker.role, taker.id, taker.stable_balance),
        (Some("taker"), id(k::TAKER_STABLE), Some(1_000))
    );
    assert_eq!(taker.holding.as_deref(), Some("taker-stable"));
    let caller = row(&rows, "caller");
    assert_eq!(
        (caller.role, caller.holding.as_deref()),
        (Some("caller"), None)
    );
    let bidder = row(&rows, "bidder");
    assert_eq!(
        (bidder.role, bidder.stable_balance),
        (None, Some(1_000_000))
    );
    assert_eq!(row(&rows, "host-admin").role, None);
    let json = serde_json::to_value(row(&rows, "taker")).expect("json");
    assert_eq!(json["stable_balance"], "1000");
    assert_eq!(json["coll_balance"], serde_json::Value::Null);
}
