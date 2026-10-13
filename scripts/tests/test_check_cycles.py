"""Review Focus 4: the cycle gate's own blind spots (Plan 3b Task 6)."""
import importlib.util
import pathlib
import unittest

SPEC = importlib.util.spec_from_file_location(
    "check_cycles", pathlib.Path(__file__).resolve().parents[1] / "check-cycles.py")
cc = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cc)


def row(total, executions=1):
    return {"label": "x", "executions": executions, "total_cycles": total,
            "per_exec": [{"program": "auction_engine", "cycles": total}], "tx_bytes": 100}


class CompareTest(unittest.TestCase):
    def test_exactly_ten_percent_passes(self):
        self.assertEqual(cc.compare({"a": row(1000)}, {"a": row(1100)}), [])

    def test_more_than_ten_percent_fails(self):
        errors = cc.compare({"a": row(1000)}, {"a": row(1101)})
        self.assertEqual(len(errors), 1)
        self.assertIn("a: 1101 cycles", errors[0])

    def test_new_label_without_baseline_fails(self):
        errors = cc.compare({}, {"new": row(10)})
        self.assertIn("no baseline", errors[0])

    def test_removed_label_fails(self):
        errors = cc.compare({"gone": row(10)}, {})
        self.assertIn("not measured", errors[0])

    def test_more_than_ten_executions_fails(self):
        errors = cc.compare({"a": row(1000, 11)}, {"a": row(1000, 11)})
        self.assertTrue(any("11 executions" in e for e in errors), errors)

    def test_an_execution_over_32_mi_cycles_fails(self):
        big = 32 * 1024 * 1024 + 1
        errors = cc.compare({"a": row(big)}, {"a": row(big)})
        self.assertTrue(any("32 Mi" in e for e in errors), errors)

    def test_shrinking_passes(self):
        self.assertEqual(cc.compare({"a": row(1000)}, {"a": row(10)}), [])


if __name__ == "__main__":
    unittest.main()
