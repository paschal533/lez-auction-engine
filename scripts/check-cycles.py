#!/usr/bin/env python3
"""Cycle regression check (spec §11, ADR-048).

usage: check-cycles.py BASELINE CURRENT_JSONL [--write-baseline] [--write-md CYCLES.md]

CURRENT_JSONL is written by scripts/cycles.sh (one JSON object per measured instruction). Fails with
exit 1 when an instruction's total executor cycles grew by more than 10 % over the baseline, when a
label is in only one of the two sets, when a chain has more than 10 executions, or when one execution
exceeds the 32 Mi cycle session limit.
"""
import json
import sys

LIMIT_PERCENT = 10
MAX_EXECUTIONS = 10
SESSION_LIMIT = 32 * 1024 * 1024
BEGIN, END = "<!-- cycles:begin -->", "<!-- cycles:end -->"


def load_current(path):
    rows = {}
    with open(path, encoding="utf-8") as f:
        for line in f:
            if not line.strip():
                continue
            r = json.loads(line)
            if r["label"] in rows and rows[r["label"]] != r:
                raise SystemExit(f"check-cycles: label {r['label']} measured twice with different results; rename one in chains.rs")
            rows[r["label"]] = r
    return rows


def compare(baseline, current):
    errors = []
    for label in sorted(set(baseline) | set(current)):
        if label not in current:
            errors.append(f"{label}: in the baseline but not measured (removed or renamed? update cycles/baseline.json in the same commit)")
            continue
        if label not in baseline:
            errors.append(f"{label}: measured but has no baseline (run check-cycles.py with --write-baseline and commit cycles/baseline.json)")
            continue
        old, new = baseline[label]["total_cycles"], current[label]["total_cycles"]
        if new * 100 > old * (100 + LIMIT_PERCENT):
            errors.append(f"{label}: {new} cycles is {100 * (new - old) / old:.1f} % above the baseline {old} (limit {LIMIT_PERCENT} %)")
        if current[label]["executions"] > MAX_EXECUTIONS:
            errors.append(f"{label}: {current[label]['executions']} executions (limit {MAX_EXECUTIONS}, ADR-036)")
        for e in current[label]["per_exec"]:
            if e["cycles"] > SESSION_LIMIT:
                errors.append(f"{label}: one {e['program']} execution takes {e['cycles']} cycles, over 32 Mi")
    return errors


def table(current):
    lines = ["| Instruction (scenario label) | Executions | Total cycles | Largest execution | % of 32 Mi | Signed tx bytes |",
             "|---|---|---|---|---|---|"]
    for label in sorted(current):
        r = current[label]
        largest = max(e["cycles"] for e in r["per_exec"])
        lines.append(f"| {label} | {r['executions']} | {r['total_cycles']:,} | {largest:,} | {100 * largest / SESSION_LIMIT:.1f} | {r['tx_bytes']} |")
    return "\n".join(lines)


def write_md(path, current):
    text = open(path, encoding="utf-8").read()
    head, rest = text.split(BEGIN, 1)
    _, tail = rest.split(END, 1)
    open(path, "w", encoding="utf-8", newline="\n").write(f"{head}{BEGIN}\n{table(current)}\n{END}{tail}")


def main(argv):
    if len(argv) < 3:
        print(__doc__, file=sys.stderr)
        return 2
    current = load_current(argv[2])
    if "--write-baseline" in argv:
        with open(argv[1], "w", encoding="utf-8", newline="\n") as f:
            json.dump(current, f, indent=2, sort_keys=True)
            f.write("\n")
        print(f"check-cycles: wrote {argv[1]} ({len(current)} labels)")
    if "--write-md" in argv:
        write_md(argv[argv.index("--write-md") + 1], current)
    baseline = json.load(open(argv[1], encoding="utf-8"))
    errors = compare(baseline, current)
    for e in errors:
        print("check-cycles: " + e, file=sys.stderr)
    if not errors:
        print(f"check-cycles: {len(current)} instructions within {LIMIT_PERCENT} % of the baseline")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
