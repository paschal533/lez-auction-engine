"""count-tests.py: per-area test counts and the README block check (Plan 8 Task 3)."""
import importlib.util
import io
import os
import pathlib
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout

SPEC = importlib.util.spec_from_file_location(
    "count_tests", pathlib.Path(__file__).resolve().parents[1] / "count-tests.py")
ct = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ct)

BEGIN, END = "<!-- test-counts:begin -->", "<!-- test-counts:end -->"


class CountTest(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.mkdtemp()
        self.write("v04/a/src/lib.rs", "#[test]\nfn one() {}\n\n#[tokio::test]\nasync fn two() {}\n")
        self.write("crates/x/src/lib.rs", "#[test]\nfn three() {}\n#[cfg(test)]\nmod m {}\n")
        self.write("crates/x/src/proofs.rs", "#[kani::proof]\nfn proof() {}\n")
        self.write("apps/auctions/tests/qml/tst_a.qml", "TestCase {\n  function test_a() {}\n  function helper() {}\n}\n")
        self.write("scripts/tests/test_x.py", "def test_x(): pass\ndef helper(): pass\n")

    def write(self, rel, text):
        path = os.path.join(self.root, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8") as f:
            f.write(text)

    def run_main(self, *args):
        out, err = io.StringIO(), io.StringIO()
        with redirect_stdout(out), redirect_stderr(err):
            code = ct.main(["count-tests.py", "--root", self.root, *args])
        return code, out.getvalue(), err.getvalue()

    def readme(self, body):
        self.write("README.md", f"intro\n{BEGIN}\n{body}\n{END}\noutro\n")
        return os.path.join(self.root, "README.md")

    def test_counts_by_area(self):
        self.assertEqual(ct.count(self.root), {
            "v04": 2, "v024_programs": 0, "crates": 1, "cli": 0, "fuzz_kani": 1,
            "qml": 1, "cpp": 0, "python": 1, "total": 6})

    def test_skips_target_dirs(self):
        self.write("crates/x/target/debug/gen.rs", "#[test]\nfn nope() {}\n")
        self.write("v04/node_modules/q.rs", "#[test]\nfn nope() {}\n")
        self.write("v04/.superpowers/q.rs", "#[test]\nfn nope() {}\n")
        self.assertEqual(ct.count(self.root)["total"], 6)

    def test_other_dirs_not_counted_but_kani_anywhere_is(self):
        self.write("tools/t.rs", "#[test]\nfn t() {}\n#[kani::proof]\nfn p() {}\n")
        counts = ct.count(self.root)
        self.assertEqual((counts["fuzz_kani"], counts["total"]), (2, 7))

    def test_proptest_fn_with_test_attr_counted_once(self):
        self.write("crates/x/src/prop.rs", "proptest! {\n    #[test]\n    fn p(x: u8) {}\n}\n")
        self.assertEqual(ct.count(self.root)["crates"], 2)

    def test_tests_in_comments_are_not_counted(self):
        self.write("crates/x/src/c.rs", "/* old\n#[test]\nfn a() {}\n#[kani::proof]\nfn b() {}\n*/\n"
                   "// #[test]\n/// #[test]\n//! #[test]\n#[test]\nfn real() {}\n")
        counts = ct.count(self.root)
        self.assertEqual((counts["crates"], counts["fuzz_kani"]), (2, 1))

    def test_bom_and_crlf_files_count(self):
        with open(os.path.join(self.root, "crates/x/src/b.rs"), "wb") as f:
            f.write(b"\xef\xbb\xbf#[test]\r\nfn a() {}\r\n#[test]\r\nfn b() {}\r\n")
        with open(os.path.join(self.root, "scripts/tests/test_b.py"), "wb") as f:
            f.write(b"\xef\xbb\xbfdef test_b(): pass\r\n")
        counts = ct.count(self.root)
        self.assertEqual((counts["crates"], counts["python"]), (3, 2))

    def test_cpp_macros(self):
        self.write("modules/m/tests/a_test.cpp", "LOGOS_TEST(one) {}\nLOGOS_TEST(two) {}\n")
        self.write("modules/m/tests/harness.h", "#define LOGOS_TEST(name) void name()\n")
        self.assertEqual(ct.count(self.root)["cpp"], 2)

    def test_check_detects_stale_block(self):
        fresh = ct.markdown(ct.count(self.root))
        stale = fresh.replace("| 2 |", "| 9 |")
        code, _, err = self.run_main("--check", self.readme(stale))
        self.assertEqual(code, 1)
        self.assertIn("stale", err)

    def test_check_passes_on_fresh_block(self):
        code, _, _ = self.run_main("--check", self.readme(ct.markdown(ct.count(self.root))))
        self.assertEqual(code, 0)

    def test_check_fails_without_markers(self):
        self.write("README.md", "nothing here\n")
        code, _, _ = self.run_main("--check", os.path.join(self.root, "README.md"))
        self.assertEqual(code, 1)

    def test_markdown_flag_prints_table(self):
        code, out, _ = self.run_main("--markdown")
        self.assertEqual(code, 0)
        self.assertTrue(out.startswith("| Area | Tests |"))
        self.assertIn("| **Total** | 6 |", out)
        self.assertNotIn(BEGIN, out)


if __name__ == "__main__":
    unittest.main()
