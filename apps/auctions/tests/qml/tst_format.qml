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
}
