//! Plain `key  value` output, one item per line, stable for golden tests.

use std::io::Write;

use auction_core::{math::mul_div_floor, units::BPS_TO_RAY};

use crate::error::CliError;

/// Writes `key` padded to 17 columns, then a space, then `value`: always at least one space, so a
/// script can take the last field of the line.
pub fn kv(out: &mut dyn Write, key: &str, value: &str) -> Result<(), CliError> {
    writeln!(out, "{key:<17} {value}")?;
    Ok(())
}

/// A RAY discount as basis points with two decimals (floor).
#[must_use]
pub fn bps(ray: u128) -> String {
    let whole = ray / BPS_TO_RAY;
    // `BPS_TO_RAY` is a non-zero constant and the remainder is below it, so this cannot fail.
    let hundredths = mul_div_floor(ray % BPS_TO_RAY, 100, BPS_TO_RAY).unwrap_or(0);
    format!("{whole}.{hundredths:02} bps")
}

#[must_use]
pub fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bps_formats_ray_discounts() {
        assert_eq!(bps(2 * BPS_TO_RAY * 100), "200.00 bps");
        assert_eq!(bps(37_777_777_777_777_777_777_777_777), "377.77 bps");
        assert_eq!(bps(0), "0.00 bps");
    }

    #[test]
    fn bps_floors_and_survives_the_maximum() {
        assert_eq!(bps(BPS_TO_RAY - 1), "0.99 bps");
        assert!(bps(u128::MAX).ends_with(" bps"));
    }

    #[test]
    fn kv_pads_the_key_and_keeps_one_space() {
        let mut out = Vec::new();
        kv(&mut out, "a", "1").unwrap();
        kv(&mut out, "a-very-long-key-over-17", "2").unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text, "a                 1\na-very-long-key-over-17 2\n");
    }
}
