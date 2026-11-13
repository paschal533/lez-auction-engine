"""Tests for scripts/check-frozen-v024.sh (ADR-069: the v0.2.4 guest source paths stay frozen)."""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "check-frozen-v024.sh"


def git(root, *args):
    return subprocess.run(
        ["git", "-C", str(root), *args],
        check=True, capture_output=True, text=True,
    ).stdout.strip()


class CheckFrozenV024(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)
        git(self.root, "init", "-q")
        git(self.root, "config", "user.email", "t@example.invalid")
        git(self.root, "config", "user.name", "t")
        (self.root / "programs").mkdir()
        (self.root / "v04").mkdir()
        (self.root / "programs" / "a.rs").write_text("fn a() {}\n")
        (self.root / "v04" / "b.rs").write_text("fn b() {}\n")
        git(self.root, "add", "-A")
        self.tree = git(self.root, "write-tree")

    def run_script(self):
        env = {**os.environ, "FROZEN_ROOT": str(self.root)}
        return subprocess.run(
            ["bash", str(SCRIPT), self.tree],
            capture_output=True, text=True, env=env,
        )

    def test_clean_tree_passes(self):
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(r.stdout, "")
        self.assertEqual(r.stderr, "")

    def test_frozen_file_change_fails(self):
        (self.root / "programs" / "a.rs").write_text("fn a() { changed }\n")
        r = self.run_script()
        self.assertEqual(r.returncode, 1)
        self.assertIn("programs/a.rs", r.stderr)
        self.assertIn("frozen v0.2.4 paths changed since", r.stderr)

    def test_v04_change_passes(self):
        (self.root / "v04" / "b.rs").write_text("fn b() { changed }\n")
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr)

    def test_root_manifest_change_fails(self):
        (self.root / "Cargo.toml").write_text("[workspace]\n")
        git(self.root, "add", "Cargo.toml")
        r = self.run_script()
        self.assertEqual(r.returncode, 1)
        self.assertIn("Cargo.toml", r.stderr)


if __name__ == "__main__":
    unittest.main()
