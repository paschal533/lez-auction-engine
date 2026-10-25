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

    // Ray values (1e27 = 1) are u128 strings: percentages come from string arithmetic and truncate, as the
    // CLI's bps() floors (R32): 37777…7 ray is 377.77 bps there, 3.7777% here.
    function test_ray_percent_truncates_like_the_cli() {
        compare(Format.rayPercent("37777777777777777777777777", 1), "3.7")
        compare(Format.rayPercent("37777777777777777777777777", 4), "3.7777")
        compare(Format.rayPercent("1777777777777777777777778", 2), "0.17")
        compare(Format.rayPercent("100000000000000000000000000", 1), "10.0")
        compare(Format.rayPercent("0", 1), "0.0")
        compare(Format.rayPercent("994999999999999999999999999", 0), "99")
        compare(Format.rayPercent("999999999999999999999999999", 0), "99")
        compare(Format.rayPercent("9999999999999999999999999", 2), "0.99")
        compare(Format.rayPercent("2000000000000000000000000000000", 0), "200,000")
    }

    function test_ray_price_trims_trailing_zeros() {
        compare(Format.rayPrice("500000000000000000000000000"), "0.5")
        compare(Format.rayPrice("752000000000000000000000"), "0.000752")
        compare(Format.rayPrice("2000000000000000000000000000"), "2")
    }

    function test_fixed_truncates() {
        compare(Format.fixed("12345", 2, 1), "123.4")
        compare(Format.fixed("99995", 4, 2), "9.99")
        compare(Format.fixed("5", 3, 0), "0")
        compare(Format.fixed("999", 3, 0), "0")
        compare(Format.fixed("1234567", 3, 0), "1,234")
        compare(Format.fixed("7", 0, 2), "7.00")
        compare(Format.fixed("abc", 2, 1), "abc")
    }

    function test_leading_zeros_are_ignored() {
        compare(Format.fixed("0012345", 2, 1), "123.4")
        compare(Format.fixed("000", 2, 2), "0.00")
        compare(Format.fixed("0005", 3, 3), "0.005")
        compare(Format.subtract("000660000", "0100000"), "560000")
        compare(Format.subtract("0005", "005"), "0")
        compare(Format.subtract("0010", "0001"), "9")
        compare(Format.cmp("000700000", "660000"), 1)
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
        // A part far longer than the whole clamps to 1; leading zeros do not count as digits.
        compare(Format.ratio("340282366920938463463374607431768211455", "7"), 1)
        compare(Format.ratio("000330000", "0660000"), 0.5)
        compare(Format.ratio("1", "340282366920938463463374607431768211455"), 0)
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

    // Products of u128 decimal strings (for comparisons such as lot × RAY < target × price), no double.
    function test_mul_keeps_all_digits() {
        compare(Format.mul("0", "123"), "0")
        compare(Format.mul("12", "12"), "144")
        compare(Format.mul("499975", "1000000000000000000000000000"), "499975000000000000000000000000000")
        compare(Format.mul("340282366920938463463374607431768211455", "2"),
                "680564733841876926926749214863536422910")
        compare(Format.mul("007", "006"), "42")
        compare(Format.mul("1.5", "2"), "", "not digits: no product")
    }

    // Debt value: nominal_debt × redemption_ray / RAY, floored to whole units.
    function test_mul_then_fixed_floors_a_ray_product() {
        compare(Format.fixed(Format.mul("600001", "500000000000000000000000000"), Format.RAY_DECIMALS, 0), "300,000")
        compare(Format.fixed(Format.mul("15900", "752000000000000000000000"), Format.RAY_DECIMALS, 3), "11.956")
    }

    // Sums of u128 decimal strings (the settled bar checks debt_repaid + penalty against raised), no double.
    function test_add_keeps_all_digits() {
        compare(Format.add("0", "0"), "0")
        compare(Format.add("600000", "60000"), "660000")
        compare(Format.add("999", "1"), "1000")
        compare(Format.add("340282366920938463463374607431768211455", "1"),
                "340282366920938463463374607431768211456")
        compare(Format.add("007", "05"), "12")
        compare(Format.add("1.5", "2"), "", "not digits: no sum")
    }

    // Dev tools: a typed price becomes a RAY string digit by digit; too many places or junk gives "".
    function test_to_units_makes_ray_prices() {
        compare(Format.toUnits("1.9", 27), "1900000000000000000000000000")
        compare(Format.toUnits("0.5", 27), "500000000000000000000000000")
        compare(Format.toUnits("2", 27), "2000000000000000000000000000")
        compare(Format.toUnits("1.", 3), "1000")
        compare(Format.toUnits("0", 27), "0")
        compare(Format.toUnits("1.2345", 3), "")
        compare(Format.toUnits("-1", 3), "")
        compare(Format.toUnits("", 3), "")
        compare(Format.toUnits("1e3", 3), "")
    }

    function test_duration_floors_to_seconds_then_minutes() {
        compare(Format.duration(0), "0s")
        compare(Format.duration(59999), "59s")
        compare(Format.duration(150000), "2m")
        compare(Format.duration(3600000), "60m")
        compare(Format.duration(-1), "")
    }

    // Lists in replies cross the remote-objects replica as array-likes; Format.list makes them JS arrays.
    function test_list_accepts_array_likes() {
        compare(JSON.stringify(Format.list([1, 2])), "[1,2]")
        compare(JSON.stringify(Format.list({ length: 2, 0: "a", 1: "b" })), '["a","b"]')
        compare(JSON.stringify(Format.list({ 1: "b", 0: "a" })), '["a","b"]')
        compare(Format.list(null).length, 0)
        compare(Format.list({ error: "x" }).length, 0)
        compare(Format.list("abc").length, 0)
    }
}
