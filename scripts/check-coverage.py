#!/usr/bin/env python3
"""Per-crate line coverage gate (spec §9, ADR-049).

usage: check-coverage.py COV_JSON [--root DIR] [--threshold 80]
COV_JSON is `cargo llvm-cov ... --json --summary-only --output-path COV_JSON`. Every crate below must reach
the threshold on its own; files of other crates (vendored code, dependencies) are ignored.
"""
import json
import os
import sys

CRATES = {
    "auction_core": "crates/auction_core/",
    "auction_engine_core": "programs/auction_engine/core/",
    "auction_engine_program": "programs/auction_engine/src/",
    "auction_sdk": "crates/auction_sdk/",
    "auction_cli": "cli/",
}
THRESHOLD = 80


def per_crate(export, root):
    totals = {c: [0, 0] for c in CRATES}
    for f in export["data"][0]["files"]:
        rel = os.path.relpath(f["filename"], root).replace(os.sep, "/")
        for crate, prefix in CRATES.items():
            if rel.startswith(prefix):
                totals[crate][0] += f["summary"]["lines"]["count"]
                totals[crate][1] += f["summary"]["lines"]["covered"]
    return totals


def check(totals, threshold=THRESHOLD):
    errors = []
    for crate, (count, covered) in totals.items():
        if count == 0:
            errors.append(f"{crate}: no coverage data (not built, or every file ignored)")
            continue
        pct = 100 * covered / count
        if pct < threshold:
            errors.append(f"{crate}: {pct:.1f} % of lines covered (< {threshold} %)")
    return errors


def main(argv):
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    root = argv[argv.index("--root") + 1] if "--root" in argv else os.getcwd()
    threshold = float(argv[argv.index("--threshold") + 1]) if "--threshold" in argv else THRESHOLD
    totals = per_crate(json.load(open(argv[1], encoding="utf-8")), root)
    for crate, (count, covered) in totals.items():
        if count:
            print(f"{crate}: {100 * covered / count:.1f} % ({covered}/{count} lines)")
    errors = check(totals, threshold)
    for e in errors:
        print("check-coverage: " + e, file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
