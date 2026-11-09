"""gen-refusal-codes.py: refusal-code catalogue generator (Plan 8 Task 4)."""
import importlib.util
import io
import os
import pathlib
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout

SPEC = importlib.util.spec_from_file_location(
    "gen_refusal_codes", pathlib.Path(__file__).resolve().parents[1] / "gen-refusal-codes.py")
g = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(g)

ERROR_RS = """
pub enum EngineError {
    NotAdmin,
    WrongAccount(&'static str),
    Paused,
}
impl EngineError {
    pub const fn code(&self) -> u32 {
        match self {
            Self::NotAdmin => 1,
            Self::WrongAccount(_) => 12,
            Self::Paused => 20,
        }
    }
}
"""


class Fixture(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.mkdtemp()
        self.write(g.ERROR_RS_PATH, ERROR_RS)
        self.write("programs/a/src/admin.rs", "fn f() { return Err(EngineError::NotAdmin); }\n")
        self.write("programs/a/src/admin_tests.rs",
                   "#[test]\nfn rejects_stranger() { assert_eq!(x, EngineError::NotAdmin); }\n")
        self.write("programs/it/tests/b.rs",
                   '#[test]\nfn wire_form() { assert_err(r, "Program error [6012]: Program error 12: x"); }\n')

    def write(self, rel, text):
        path = os.path.join(self.root, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8") as f:
            f.write(text)

    def run_main(self, *args):
        out, err = io.StringIO(), io.StringIO()
        with redirect_stdout(out), redirect_stderr(err):
            code = g.main(["gen", "--root", self.root, *args])
        return code, out.getvalue(), err.getvalue()

    def doc(self):
        with open(os.path.join(self.root, g.DOC_PATH), encoding="utf-8") as f:
            return f.read()


class ParseTest(unittest.TestCase):
    def test_parse_codes_handles_tuple_variants(self):
        self.assertIn(("WrongAccount", 12), g.parse_codes(ERROR_RS))
        self.assertEqual([("NotAdmin", 1), ("WrongAccount", 12), ("Paused", 20)], g.parse_codes(ERROR_RS))

    def test_codes_unique(self):
        dup = ERROR_RS.replace("Self::Paused => 20", "Self::Paused => 1")
        with self.assertRaises(ValueError):
            g.parse_codes(dup)


class FindUsesTest(Fixture):
    def test_raised_in_excludes_tests_and_error_rs(self):
        uses = g.find_uses(self.root, "NotAdmin")
        self.assertEqual(["programs/a/src/admin.rs"], uses["raised_in"])

    def test_test_matched_by_variant_path_and_wire_form(self):
        self.assertEqual(["rejects_stranger"], g.find_uses(self.root, "NotAdmin")["tests"])
        self.assertEqual(["wire_form"], g.find_uses(self.root, "WrongAccount", 12)["tests"])

    def test_test_attribute_in_comment_is_ignored(self):
        self.write("programs/a/src/c.rs", "// #[test]\n// fn ghost() { EngineError::Paused }\n")
        self.assertEqual([], g.find_uses(self.root, "Paused")["tests"])

    def test_bare_quoted_name_counts(self):
        self.write("programs/it/tests/d.rs", '#[test]\nfn paused() { assert_err(w.bid(), "Paused"); }\n')
        self.assertEqual(["paused"], g.find_uses(self.root, "Paused")["tests"])


class ProofRulesTest(Fixture):
    def test_mapping_only_tests_are_not_proof(self):
        self.write("programs/a/src/lib.rs",
                   "#[test]\nfn error_codes_are_stable() { assert_eq!(EngineError::Paused.code(), 20); }\n")
        self.assertEqual([], g.find_uses(self.root, "Paused", 20)["tests"])

    def test_sdk_only_tests_do_not_prove(self):
        self.write("v04/sdk/tests/q.rs", "#[test]\nfn predicts() { let _ = EngineError::Paused; }\n")
        uses = g.find_uses(self.root, "Paused", 20)
        self.assertEqual([], uses["tests"])
        self.assertEqual([], uses["labelled"])

    def test_tests_are_labelled_by_engine(self):
        self.write("v04/h/tests/x.rs", "#[test]\nfn on_04() { let _ = EngineError::NotAdmin; }\n")
        self.write("crates/auction_core/src/y.rs", "#[test]\nfn in_core() { let _ = EngineError::NotAdmin; }\n")
        labelled = g.find_uses(self.root, "NotAdmin", 1)["labelled"]
        self.assertEqual({"(v0.2.4) rejects_stranger", "(0.4) on_04", "(core) in_core"}, set(labelled))

    def test_from_only_variant_lists_core_raise_sites(self):
        text = ERROR_RS + "impl From<AuctionError> for EngineError { match e { AuctionError::Paused => Self::Paused } }\n"
        self.write("crates/auction_core/src/liq.rs", "fn q() { return Err(AuctionError::Paused); }\n")
        uses = g.find_uses(self.root, "Paused", 20, error_text=text)
        self.assertEqual(["crates/auction_core/src/liq.rs"], uses["raised_in"])

    RENAMED = ("impl From<PauseReason> for EngineError { fn from(r: PauseReason) -> Self { match r {"
               " PauseReason::Stale | PauseReason::Old => Self::Paused } } }\n")

    def test_renamed_from_mapping_is_followed(self):
        self.assertEqual({"Paused": ["PauseReason::Stale", "PauseReason::Old"]},
                         g.from_arms(ERROR_RS + self.RENAMED))
        self.write("crates/auction_core/src/feed.rs", "fn q() { return Err(PauseReason::Stale); }\n")
        uses = g.find_uses(self.root, "Paused", 20, error_text=ERROR_RS + self.RENAMED)
        self.assertEqual(["crates/auction_core/src/feed.rs"], uses["raised_in"])

    def test_from_only_variant_without_raise_site_fails_check(self):
        self.write(g.ERROR_RS_PATH, ERROR_RS + self.RENAMED)
        self.write("programs/it/tests/p.rs", '#[test]\nfn p() { assert_err(r, "Paused"); }\n')
        saved = dict(g.MEANINGS)
        try:
            g.MEANINGS.update(NotAdmin="a", WrongAccount="b", Paused="c")
            self.run_main()
            code, _, err = self.run_main("--check")
            self.assertEqual(1, code)
            self.assertIn("Paused: mapped by a From impl but no raise site", err)
        finally:
            g.MEANINGS.clear(); g.MEANINGS.update(saved)


class CheckTest(Fixture):
    def test_variant_without_test_fails_check(self):
        code, _, _ = self.run_main()
        self.assertEqual(1, code)
        self.assertRegex(self.doc(), r"\| `Paused` \|[^\n]*\*\*no test\*\*")
        saved = (dict(g.MEANINGS), set(g.KNOWN_GAPS))
        try:
            g.MEANINGS.update(NotAdmin="a", WrongAccount="b", Paused="c")
            g.KNOWN_GAPS.clear()
            self.assertEqual(1, self.run_main()[0])
            code, _, err = self.run_main("--check")
            self.assertEqual(1, code)
            self.assertIn("Paused", err)
            g.KNOWN_GAPS["Paused"] = "covered by a later plan"
            self.run_main()
            self.assertEqual(0, self.run_main("--check")[0])
        finally:
            g.MEANINGS.clear(); g.MEANINGS.update(saved[0])
            g.KNOWN_GAPS.clear(); g.KNOWN_GAPS.update({k: "" for k in saved[1]})

    def test_write_mode_writes_the_doc_then_prints_problems_and_exits_1(self):
        saved = (dict(g.MEANINGS), dict(g.KNOWN_GAPS))
        try:
            g.MEANINGS.update(NotAdmin="a", WrongAccount="b", Paused="c")
            g.KNOWN_GAPS.clear()
            code, _, err = self.run_main()
            self.assertEqual(1, code)
            self.assertIn("gen-refusal-codes: ", err)
            self.assertIn("Paused", err)
            self.assertRegex(self.doc(), r"\| `Paused` \|[^\n]*\*\*no test\*\*")
            g.KNOWN_GAPS["Paused"] = "later"
            code, _, err = self.run_main()
            self.assertEqual((0, ""), (code, err))
        finally:
            g.MEANINGS.clear(); g.MEANINGS.update(saved[0])
            g.KNOWN_GAPS.clear(); g.KNOWN_GAPS.update(saved[1])

    def test_check_detects_stale_doc(self):
        saved = (dict(g.MEANINGS), dict(g.KNOWN_GAPS))
        try:
            g.MEANINGS.update(NotAdmin="a", WrongAccount="b", Paused="c")
            g.KNOWN_GAPS["Paused"] = "later"
            self.run_main()
            self.assertEqual(0, self.run_main("--check")[0])
            self.write(g.DOC_PATH, self.doc() + "stale\n")
            code, _, err = self.run_main("--check")
            self.assertEqual(1, code)
            self.assertIn("stale", err)
        finally:
            g.MEANINGS.clear(); g.MEANINGS.update(saved[0])
            g.KNOWN_GAPS.clear(); g.KNOWN_GAPS.update(saved[1])

    def test_missing_meaning_fails_check(self):
        saved = dict(g.MEANINGS)
        try:
            g.MEANINGS.clear()
            self.run_main()
            self.assertEqual(1, self.run_main("--check")[0])
        finally:
            g.MEANINGS.update(saved)


if __name__ == "__main__":
    unittest.main()
