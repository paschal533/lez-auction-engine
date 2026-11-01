//! `seeds` is a copy of the v0.2.4 engine core's seed helpers (the core's `pda.rs` stays untouched
//! so the committed v0.2.4 guest is unchanged, ruling R-F1). This pins the copy to the original.

use auction_engine_core::pda;
use auction_engine_params::seeds;

const LITERALS: [&str; 5] = [
    "",
    "config",
    "collateral",
    "auction",
    "abcdefghijklmnopqrstuvwxyz012345",
];
const NUMBERS: [u64; 5] = [0, 1, 255, 1 << 40, u64::MAX];

#[test]
fn seed_from_str_matches_core() {
    for s in LITERALS {
        assert_eq!(seeds::seed_from_str(s), pda::seed_from_str(s), "{s:?}");
    }
}

#[test]
fn u64_seed_matches_core() {
    for n in NUMBERS {
        assert_eq!(seeds::u64_seed(n), pda::u64_seed(n), "{n}");
    }
}

#[test]
fn combine_seeds_matches_core() {
    let a = seeds::seed_from_str("config");
    let b = seeds::u64_seed(7);
    let c = [0xab; 32];
    for parts in [vec![&a], vec![&a, &b], vec![&a, &b, &c], vec![&c, &c]] {
        assert_eq!(seeds::combine_seeds(&parts), pda::combine_seeds(&parts));
    }
}
