"""The coverage gate is per crate (Plan 3b Task 8)."""
import importlib.util
import pathlib
import unittest

SPEC = importlib.util.spec_from_file_location(
    "check_coverage", pathlib.Path(__file__).resolve().parents[1] / "check-coverage.py")
cov = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cov)


def export(files):
    return {"data": [{"files": [
        {"filename": f"/repo/{name}", "summary": {"lines": {"count": count, "covered": covered}}}
        for name, count, covered in files]}]}


ALL_GOOD = [("crates/auction_core/src/math.rs", 100, 90),
            ("programs/auction_engine/core/src/state.rs", 100, 90),
            ("programs/auction_engine/src/bid.rs", 100, 90),
            ("crates/auction_sdk/src/quote.rs", 100, 90),
            ("cli/src/env.rs", 100, 90)]


class CoverageTest(unittest.TestCase):
    def test_every_crate_above_the_threshold_passes(self):
        self.assertEqual(cov.check(cov.per_crate(export(ALL_GOOD), "/repo")), [])

    def test_one_weak_crate_fails_even_when_the_total_passes(self):
        files = ALL_GOOD[:-1] + [("cli/src/env.rs", 100, 70)]
        self.assertEqual(cov.check(cov.per_crate(export(files), "/repo")),
                         ["auction_cli: 70.0 % of lines covered (< 80 %)"])

    def test_a_crate_without_data_fails(self):
        errors = cov.check(cov.per_crate(export(ALL_GOOD[1:]), "/repo"))
        self.assertEqual(errors, ["auction_core: no coverage data (not built, or every file ignored)"])

    def test_vendored_files_do_not_count(self):
        files = ALL_GOOD + [("programs/stablecoin/src/lib.rs", 1000, 0)]
        self.assertEqual(cov.check(cov.per_crate(export(files), "/repo")), [])


if __name__ == "__main__":
    unittest.main()
