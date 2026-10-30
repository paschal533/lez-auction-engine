"""host.sh check: the C: floor applies in WSL only; on plain Linux (CI) it is skipped, not a crash.

Regression: 913c58c made qml-tests.sh call `host.sh check`, which ran `df /mnt/c` on the hosted runner
and failed the basecamp-module job with an arithmetic syntax error.
"""
import os
import pathlib
import subprocess
import sys
import unittest

HOST = pathlib.Path(__file__).resolve().parents[1] / "basecamp" / "host.sh"


def check(c_mount, min_gb):
    env = dict(os.environ, C_MOUNT=c_mount, MIN_C_GB=str(min_gb))
    return subprocess.run(["bash", str(HOST), "check"], env=env, capture_output=True, text=True)


@unittest.skipUnless(sys.platform.startswith("linux"), "host.sh re-runs itself in WSL from Windows")
class HostCheckTest(unittest.TestCase):
    def test_no_windows_drive_skips_the_check(self):
        r = check("/nonexistent-c-drive", 10)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("skipped", r.stdout)

    def test_enough_space_passes(self):
        r = check("/", 0)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("ok", r.stdout)

    def test_too_little_space_refuses_with_exit_2(self):
        r = check("/", 10**9)
        self.assertEqual(r.returncode, 2)
        self.assertIn("refusing", r.stderr)


if __name__ == "__main__":
    unittest.main()
