#!/usr/bin/env python3
"""Checks a real-proof report (PROOF_REPORT JSON lines, ADR-045).

usage: check-proof-report.py REPORT [--expect label1,label2]
Fails when the report is missing (the prover was killed: out of memory or the job timeout), empty, has an
unverified receipt, or lacks an expected label. Prints a per-label summary otherwise.
"""
import json
import os
import sys


REQUIRED = ("label", "program", "segments", "seconds")


def load(path):
    with open(path, encoding="utf-8") as f:
        return [json.loads(line) for line in f if line.strip()]


def check(path, expect):
    if not os.path.exists(path):
        return [f"no proof report at {path}: the prover did not finish (killed for memory, or the job timed out); see the job log"]
    try:
        rows = load(path)
    except (OSError, ValueError) as e:  # json.JSONDecodeError and UnicodeDecodeError are ValueErrors
        return [f"{path} cannot be read as a proof report: {e}"]
    if not rows:
        return [f"{path} is empty: nothing was proved"]
    for i, r in enumerate(rows, 1):
        missing = [k for k in REQUIRED if not isinstance(r, dict) or k not in r]
        if missing:
            return [f"{path} line {i} is not a proof row (missing {', '.join(missing)})"]
    errors = [f"{r['label']}: {r['program']} receipt not verified" for r in rows if r.get("verified") is not True]
    labels = {r["label"] for r in rows}
    errors += [f"{label}: no proof in the report (killed or skipped?)" for label in expect if label not in labels]
    return errors


def parse_expect(argv):
    """Returns the expected labels, or None when --expect has no value."""
    if "--expect" not in argv:
        return []
    i = argv.index("--expect")
    if i + 1 >= len(argv):
        return None
    return [label for label in argv[i + 1].split(",") if label]


def main(argv):
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    expect = parse_expect(argv)
    if expect is None:
        print("check-proof-report: --expect needs a comma-separated label list", file=sys.stderr)
        return 2
    errors = check(argv[1], expect)
    for e in errors:
        print("check-proof-report: " + e, file=sys.stderr)
    if errors:
        return 1
    rows = load(argv[1])
    for label in sorted({r["label"] for r in rows}):
        rs = [r for r in rows if r["label"] == label]
        print(f"{label}: {len(rs)} executions, {sum(r['segments'] for r in rs)} segments, "
              f"{sum(r['seconds'] for r in rs):.0f} s, all receipts verified")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
