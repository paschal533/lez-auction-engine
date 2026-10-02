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

fn main() {
    let mut s = 42u128;
    let mut out = Vec::new();
    for _ in 0..3_000 {
        let (x, y, d) = (
            lcg(&mut s) >> (lcg(&mut s) % 128),
            lcg(&mut s) >> 40,
            (lcg(&mut s) >> 60).max(1),
        );
        out.push(format!(
            r#"{{"k":"md","x":"{x}","y":"{y}","d":"{d}","f":{},"c":{}}}"#,
            mul_div_floor(x, y, d).map_or("null".into(), |v| format!("\"{v}\"")),
            mul_div_ceil(x, y, d).map_or("null".into(), |v| format!("\"{v}\""))
        ));
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
