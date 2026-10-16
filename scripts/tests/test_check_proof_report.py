"""Review Focus (resource exhaustion): a killed prover must fail loudly (Plan 3b Task 7)."""
import importlib.util
import json
import os
import pathlib
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "check_proof_report", pathlib.Path(__file__).resolve().parents[1] / "check-proof-report.py")
cpr = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cpr)


def write(lines):
    f = tempfile.NamedTemporaryFile("w", delete=False, suffix=".jsonl")
    for line in lines:
        f.write(json.dumps(line) + "\n")
    f.close()
    return f.name


def proof(label, verified=True):
    return {"label": label, "program": "auction_engine", "user_cycles": 1, "total_cycles": 2,
            "segments": 1, "seconds": 1.0, "seal_bytes": 10, "po2": 20, "verified": verified}


class ReportTest(unittest.TestCase):
    def test_missing_report_says_the_prover_did_not_finish(self):
        errors = cpr.check("/nonexistent/proofs.jsonl", ["liquidate"])
        self.assertIn("did not finish", errors[0])

    def test_empty_report_fails(self):
        errors = cpr.check(write([]), [])
        self.assertIn("empty", errors[0])

    def test_unverified_receipt_fails(self):
        errors = cpr.check(write([proof("liquidate", verified=False)]), ["liquidate"])
        self.assertIn("not verified", errors[0])

    def test_missing_expected_label_fails(self):
        errors = cpr.check(write([proof("liquidate")]), ["liquidate", "bid_with_settlement"])
        self.assertEqual(errors, ["bid_with_settlement: no proof in the report (killed or skipped?)"])

    def test_malformed_json_is_an_error_not_a_traceback(self):
        f = tempfile.NamedTemporaryFile("w", delete=False, suffix=".jsonl")
        f.write('{"label": "liquidate"' + chr(10))
        f.close()
        errors = cpr.check(f.name, [])
        self.assertIn("cannot be read", errors[0])
        os.unlink(f.name)

    def test_verified_must_be_the_boolean_true(self):
        row = proof("liquidate")
        row["verified"] = "false"
        errors = cpr.check(write([row]), ["liquidate"])
        self.assertIn("not verified", errors[0])

    def test_complete_report_passes(self):
        path = write([proof("liquidate"), proof("bid_with_settlement")])
        self.assertEqual(cpr.check(path, ["liquidate", "bid_with_settlement"]), [])
        os.unlink(path)


if __name__ == "__main__":
    unittest.main()
