import QtQuick
import QtTest

import "../../qml/common/Format.js" as Format

TestCase {
    name: "Format"

    // u128 values travel as decimal strings; no digit may be lost to a double.
    function test_amount_keeps_all_digits() {
        compare(Format.amount("2000000000000000000000000000", 0),
                "2,000,000,000,000,000,000,000,000,000")
        compare(Format.amount("340282366920938463463374607431768211455", 0),
                "340,282,366,920,938,463,463,374,607,431,768,211,455")
    }

    function test_amount_with_decimals() {
        compare(Format.amount("1234567", 3), "1,234.567")
        compare(Format.amount("1500", 3), "1.5")
        compare(Format.amount("5", 3), "0.005")
        compare(Format.amount("1000000", 6), "1")
    }

    function test_amount_returns_invalid_input_unchanged() {
        compare(Format.amount("-100", 0), "-100")
        compare(Format.amount("12a4", 2), "12a4")
        compare(Format.amount("1.5", 0), "1.5")
        compare(Format.amount("1000", -1), "1000")
        compare(Format.amount("1000", 1.5), "1000")
    }

    function test_amount_small_and_empty() {
        compare(Format.amount("0", 0), "0")
        compare(Format.amount("999", 0), "999")
        compare(Format.amount("1000", 0), "1,000")
        compare(Format.amount("", 0), "")
    }

    // Account ids are base58; the header and picker show the first and last four characters.
    function test_short_id() {
        compare(Format.shortId("7wx93ACHrg2tR8vHRBcKnVkRJTrtT7zcP3UsxcViopKg"), "7wx9…opKg")
        compare(Format.shortId("abcdefghij"), "abcdefghij")
        compare(Format.shortId(""), "")
        compare(Format.shortId(null), "")
    }

    // Ray values (1e27 = 1) are u128 strings: percentages and prices come from string arithmetic.
    function test_ray_percent_rounds_without_a_double() {
        compare(Format.rayPercent("37777777777777777777777777", 1), "3.8")
        compare(Format.rayPercent("1777777777777777777777778", 2), "0.18")
        compare(Format.rayPercent("100000000000000000000000000", 1), "10.0")
        compare(Format.rayPercent("0", 1), "0.0")
        compare(Format.rayPercent("994999999999999999999999999", 0), "99")
        compare(Format.rayPercent("995000000000000000000000000", 0), "100")
        compare(Format.rayPercent("2000000000000000000000000000000", 0), "200,000")
    }

    function test_ray_price_trims_trailing_zeros() {
        compare(Format.rayPrice("500000000000000000000000000"), "0.5")
        compare(Format.rayPrice("752000000000000000000000"), "0.000752")
        compare(Format.rayPrice("2000000000000000000000000000"), "2")
    }

    function test_fixed_rounds_half_up_and_carries() {
        compare(Format.fixed("12345", 2, 1), "123.5")
        compare(Format.fixed("99995", 4, 2), "10.00")
        compare(Format.fixed("5", 3, 0), "0")
        compare(Format.fixed("500", 3, 0), "1")
        compare(Format.fixed("7", 0, 2), "7.00")
        compare(Format.fixed("abc", 2, 1), "abc")
    }

    function test_decimal_compare_and_subtract_beyond_2_pow_53() {
        compare(Format.cmp("660000", "660000"), 0)
        compare(Format.cmp("700000", "660000"), 1)
        compare(Format.cmp("0099", "100"), -1)
        compare(Format.cmp("340282366920938463463374607431768211455", "340282366920938463463374607431768211454"), 1)
        compare(Format.subtract("660000", "100000"), "560000")
        compare(Format.subtract("100000000000000000000000000001", "1"), "100000000000000000000000000000")
        compare(Format.subtract("1000", "1"), "999")
        compare(Format.subtract("5", "7"), "0")
        compare(Format.subtract("5", "5"), "0")
    }

    function test_ratio_of_large_strings() {
        compare(Format.ratio("0", "660000"), 0)
        compare(Format.ratio("330000", "660000"), 0.5)
        compare(Format.ratio("900", "600"), 1)
        compare(Format.ratio("5", "0"), 0)
        fuzzyCompare(Format.ratio("1000000000000000000000000000", "3000000000000000000000000000"), 1 / 3, 1e-9)
    }

    function test_percent_to_bps() {
        compare(Format.percentToBps("0.5"), 50)
        compare(Format.percentToBps("1"), 100)
        compare(Format.percentToBps("2.25"), 225)
        compare(Format.percentToBps("99.99"), 9999)
        compare(Format.percentToBps("100"), -1)
        compare(Format.percentToBps("0.505"), -1)
        compare(Format.percentToBps(""), -1)
        compare(Format.percentToBps("x"), -1)
    }
}
