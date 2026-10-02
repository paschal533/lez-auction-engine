//! Emits deterministic JSON vectors for the Python differential model.
use auction_core::{
    auction::*, liquidation::LiquidationQuote, math::*, schedule::rai_default, units::RAY,
};

fn lcg(s: &mut u128) -> u128 {
    *s = s
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *s
}

/// One bid config: (kind tag, quote, coin range start, coin range span).
struct BidCfg {
    kind: &'static str,
    q: LiquidationQuote,
    coin_base: u128,
    coin_span: u128,
    count: u128,
}

fn emit_bids(out: &mut Vec<String>, s: &mut u128, cfg: &BidCfg, price: u128) {
    let a = Auction::open(&cfg.q, 0, rai_default(), 0);
    for i in 0..cfg.count {
        let coin = cfg.coin_base + (lcg(s) % cfg.coin_span);
        let t = (i as u64) * 6_000;
        let req = BidRequest {
            coin_in_max: coin,
            min_collateral_out: 0,
        };
        if let Ok(f) = quote_bid(&a, &req, price, t, 1, 1) {
            out.push(format!(
                r#"{{"k":"{}","coin":"{coin}","t":{t},"lot":"{}","remaining":"{}","out":"{}","charged":"{}"}}"#,
                cfg.kind, cfg.q.lot, cfg.q.amount_to_raise, f.collateral_out, f.charged
            ));
        }
    }
}

fn opt(v: Result<u128, impl Sized>) -> String {
    v.map_or("null".into(), |v| format!("\"{v}\""))
}

fn md_vector(x: u128, y: u128, d: u128) -> String {
    format!(
        r#"{{"k":"md","x":"{x}","y":"{y}","d":"{d}","f":{},"c":{}}}"#,
        opt(mul_div_floor(x, y, d)),
        opt(mul_div_ceil(x, y, d))
    )
}

/// Small-divisor and overflow-boundary operands, computed (not hand-typed).
fn md_edge_cases() -> Vec<(u128, u128, u128)> {
    let m = u128::MAX;
    // 7 * (2*(MAX/7)+1) == 2^129 - 1 == 2*MAX + 1: with d = 2 floor == MAX (fits), ceil == MAX + 1 (overflows).
    let y7 = 2 * (m / 7) + 1;
    let mut v = vec![
        (m, m, 1),
        (m, m, m),
        (m, 1, 1),
        (m, 2, 2),
        (1 << 127, 1, 1),
        (1 << 127, 2, 1),
        (1 << 127, 3, 3),
        (7, y7, 2),
        (7, y7, 1),
        (7, y7, 3),
        (m, 1, m),
        (m, 2, m),
        (m, m - 1, m),
    ];
    for d in [1u128, 2, 3, 7] {
        for (x, y) in [
            (0, 5),
            (5, 0),
            (0, 0),
            (1, 1),
            (m, 1),
            (m, 3),
            (3, m),
            (1 << 64, 1 << 64),
            (m, m),
        ] {
            v.push((x, y, d));
        }
    }
    v
}

fn main() {
    let mut s = 42u128;
    let mut out = Vec::new();
    for _ in 0..3_000 {
        let (x, y, d) = (
            lcg(&mut s) >> (lcg(&mut s) % 128),
            lcg(&mut s) >> 40,
            (lcg(&mut s) >> 60).max(1),
        );
        out.push(md_vector(x, y, d));
    }
    for (x, y, d) in md_edge_cases() {
        out.push(md_vector(x, y, d));
    }
    for _ in 0..200 {
        let (x, y, d) = (
            lcg(&mut s) >> (lcg(&mut s) % 128),
            lcg(&mut s) >> (lcg(&mut s) % 128),
            (lcg(&mut s) >> (lcg(&mut s) % 128)).max(1),
        );
        out.push(md_vector(x, y, d));
    }
    let sch = rai_default();
    for t in (0..3_000_000u64).step_by(7_919) {
        out.push(format!(
            r#"{{"k":"sch","t":{t},"d":"{}"}}"#,
            sch.discount_ray(t)
        ));
    }
    let p = RAY * 752 / 1_000_000;
    let quote = |debt, raise, lot| LiquidationQuote {
        debt_principal: debt,
        amount_to_raise: raise,
        reserve: raise,
        keeper_reward: 0,
        lot,
        seized: lot,
    };
    let cfgs = [
        // Plain floor path: lot far above any fill.
        BidCfg {
            kind: "bid",
            q: quote(900_000, 990_000, 1_000_000_000),
            coin_base: 25,
            coin_span: 900_000,
            count: 500,
        },
        // Lot-binds path: small lot, coins large enough that the floor fill reaches the lot.
        BidCfg {
            kind: "bid_lot",
            q: quote(900_000, 990_000, 500),
            coin_base: 620_000,
            coin_span: 600_000,
            count: 500,
        },
        // Final-fill path: tiny target, plain floor fill is 0, coins reach the remaining target.
        BidCfg {
            kind: "bid_ff",
            q: quote(900, 990, 1_000_000),
            coin_base: 990,
            coin_span: 1_000,
            count: 500,
        },
    ];
    for cfg in &cfgs {
        emit_bids(&mut out, &mut s, cfg, p);
    }
    println!("[{}]", out.join(","));
}
