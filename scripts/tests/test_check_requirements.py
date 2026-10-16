"""Review Focus 5: the requirements checker must not be fooled (Plan 3b Task 15)."""
import importlib.util
import os
import pathlib
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "check_requirements", pathlib.Path(__file__).resolve().parents[1] / "check-requirements.py")
cr = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cr)

HEADER = "| ID | Requirement | Status | Evidence | Notes |\n|---|---|---|---|---|\n"


class CheckerTest(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.mkdtemp()
        self.write("crates/x/src/lib.rs", """
// fn only_in_a_comment() is not a test
#[test]
fn real_test() {}

#[test]
#[ignore = "real proof"]
fn slow_test() {}
""")
        self.write("cli/tests/golden/bid.txt", "exit: 0\n")
        self.write("cli/tests/golden.rs", 'check("bid", &out);\n')
        self.write(".github/workflows/core.yml", "jobs:\n  programs:\n    runs-on: x\n")
        self.write("README.md", "x")

    def write(self, rel, text):
        path = os.path.join(self.root, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8") as f:
            f.write(text)

    def errors(self, rows, required=("F1",)):
        return cr.check(HEADER + rows, self.root, required)

    def test_valid_rows_pass(self):
        rows = ("| F1 | x | PoC | `real_test`, `slow_test`[ignored], golden:bid, ci:core.yml/programs, doc:README.md | |\n")
        self.assertEqual(self.errors(rows), [])

    def test_name_only_in_a_comment_fails(self):
        self.assertEqual(self.errors("| F1 | x | PoC | `only_in_a_comment` | |\n"),
                         ["F1: no test `only_in_a_comment` (a #[test] fn of that name)"])

    def test_ignored_test_without_the_marker_fails(self):
        self.assertEqual(self.errors("| F1 | x | PoC | `slow_test` | |\n"),
                         ["F1: `slow_test` is #[ignore]d; write `slow_test`[ignored] and name the job that runs it"])

    def test_marker_on_a_running_test_fails(self):
        self.assertEqual(self.errors("| F1 | x | PoC | `real_test`[ignored] | |\n"),
                         ["F1: `real_test` is marked [ignored] but runs by default"])

    def test_renamed_golden_fails(self):
        self.assertEqual(self.errors("| F1 | x | PoC | golden:bid_old | |\n"),
                         ["F1: golden:bid_old has no cli/tests/golden/bid_old.txt"])

    def test_unknown_ci_job_fails(self):
        self.assertEqual(self.errors("| F1 | x | PoC | ci:core.yml/coverage | |\n"),
                         ["F1: ci:core.yml/coverage: no job coverage in .github/workflows/core.yml"])

    def test_poc_row_without_evidence_fails(self):
        self.assertEqual(self.errors("| F1 | x | PoC |  | |\n"), ["F1: status PoC needs evidence"])

    def test_milestone_row_needs_a_note(self):
        self.assertEqual(self.errors("| F1 | x | M4 |  |  |\n"), ["F1: status M4 needs a note (why, and where it is promised)"])

    def test_missing_required_id_fails(self):
        self.assertEqual(self.errors("| F1 | x | PoC | `real_test` | |\n", required=("F1", "F2")),
                         ["F2: missing from REQUIREMENTS.md"])

    def test_ci_key_outside_the_jobs_block_fails(self):
        self.write(".github/workflows/e2e.yml",
                   "on:\n  push:\n    branches: [main]\njobs:\n  localnet-e2e:\n    runs-on: x\nenv:\n  other:\n    a: b\n")
        self.assertEqual(self.errors("| F1 | x | PoC | ci:e2e.yml/push, ci:e2e.yml/other | |\n"),
                         ["F1: ci:e2e.yml/push: no job push in .github/workflows/e2e.yml",
                          "F1: ci:e2e.yml/other: no job other in .github/workflows/e2e.yml"])
        self.assertEqual(self.errors("| F1 | x | PoC | ci:e2e.yml/localnet-e2e | |\n"), [])

    def test_ignore_before_test_is_detected(self):
        self.write("crates/y/src/lib.rs", """
#[ignore = "slow"]
#[test]
fn early_ignored() {}

#[test]
#[should_panic]
#[ignore]
fn middle_ignored() {}
""")
        self.assertEqual(self.errors("| F1 | x | PoC | `early_ignored`, `middle_ignored` | |\n"),
                         ["F1: `early_ignored` is #[ignore]d; write `early_ignored`[ignored] and name the job that runs it",
                          "F1: `middle_ignored` is #[ignore]d; write `middle_ignored`[ignored] and name the job that runs it"])
        self.assertEqual(self.errors("| F1 | x | PoC | `early_ignored`[ignored], `middle_ignored`[ignored] | |\n"), [])

    def test_golden_must_be_checked_not_merely_quoted(self):
        self.write("cli/tests/golden/other.txt", "exit: 0\n")
        self.write("cli/tests/other.rs", 'assert_eq!(name, "other");\n')
        self.assertEqual(self.errors("| F1 | x | PoC | golden:other | |\n"),
                         ['F1: golden:other is not checked by any CLI test (check("other", ..))'])

    def test_duplicate_id_fails(self):
        row = "| F1 | x | PoC | `real_test` | |\n"
        self.assertEqual(self.errors(row + row), ["F1: listed twice"])


if __name__ == "__main__":
    unittest.main()
