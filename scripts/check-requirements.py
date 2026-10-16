#!/usr/bin/env python3
"""REQUIREMENTS.md checker (spec §1.4, §9; RFP S4).

usage: check-requirements.py REQUIREMENTS.md [--root DIR]
Every required RFP-014 id must have one row. A PoC or PoC (partial) row needs evidence; every evidence
token must resolve:
  `name`            a Rust fn preceded by #[test] or #[kani::proof] attributes (not #[ignore]d)
  `name`[ignored]   the same, #[ignore]d on purpose (a named CI job runs it)
  golden:NAME       cli/tests/golden/NAME.txt exists and a CLI test names "NAME"
  script:PATH       an existing script
  ci:FILE/JOB       job JOB exists in .github/workflows/FILE
  doc:PATH          an existing file or directory
Plan 4, M1-M7 and Descoped rows need a note.
"""
import os
import re
import sys

REQUIRED = (
    [f"F{i}" for i in range(1, 8)] + [f"U{i}" for i in range(1, 7)] + [f"R{i}" for i in range(1, 7)]
    + [f"P{i}" for i in range(1, 4)] + [f"S{i}" for i in range(1, 6)] + [f"PRIV{i}" for i in range(1, 4)]
    + [f"PA{i}" for i in range(1, 5)] + [f"SOFT{i}" for i in range(1, 4)]
)
STATUSES = {"PoC", "PoC (partial)", "Plan 4", "Descoped"} | {f"M{i}" for i in range(1, 8)}
NEEDS_EVIDENCE = {"PoC", "PoC (partial)"}
TOKEN = re.compile(r"`([A-Za-z0-9_]+)`(\[ignored\])?|(golden|script|ci|doc):([^\s,|]+)")
SOURCE_DIRS = ("crates", "programs", "cli", "fuzz")


def rust_sources(root):
    texts = []
    for d in SOURCE_DIRS:
        for dirpath, dirnames, files in os.walk(os.path.join(root, d)):
            dirnames[:] = [x for x in dirnames if x != "target"]
            texts += [open(os.path.join(dirpath, f), encoding="utf-8", errors="replace").read()
                      for f in files if f.endswith(".rs")]
    return "\n".join(texts)


def find_test(sources, name):
    """None if absent, else True when #[ignore]d."""
    m = re.search(r"#\[(?:test|kani::proof)\]((?:[^{])*?)\bfn\s+" + re.escape(name) + r"\s*[(<]", sources)
    if not m:
        return None
    return "#[ignore" in m.group(1)


def resolve(token, root, sources):
    name, ignored, kind, value = token
    if name:
        state = find_test(sources, name)
        if state is None:
            return f"no test `{name}` (a #[test] fn of that name)"
        if state and not ignored:
            return f"`{name}` is #[ignore]d; write `{name}`[ignored] and name the job that runs it"
        if ignored and not state:
            return f"`{name}` is marked [ignored] but runs by default"
        return None
    path = os.path.join(root, value)
    if kind == "golden":
        if not os.path.exists(os.path.join(root, "cli/tests/golden", value + ".txt")):
            return f"golden:{value} has no cli/tests/golden/{value}.txt"
        tests = "".join(open(os.path.join(root, "cli/tests", f), encoding="utf-8").read()
                        for f in os.listdir(os.path.join(root, "cli/tests")) if f.endswith(".rs"))
        return None if f'"{value}"' in tests else f'golden:{value} is not named by any CLI test'
    if kind in ("script", "doc"):
        return None if os.path.exists(path) else f"{kind}:{value} does not exist"
    wf, _, job = value.partition("/")
    wf_path = os.path.join(root, ".github/workflows", wf)
    if not os.path.exists(wf_path):
        return f"ci:{value}: no workflow {wf}"
    if not re.search(rf"^  {re.escape(job)}:\s*$", open(wf_path, encoding="utf-8").read(), re.M):
        return f"ci:{value}: no job {job} in .github/workflows/{wf}"
    return None


def check(text, root, required=REQUIRED):
    errors, seen = [], set()
    sources = rust_sources(root)
    for line in text.splitlines():
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 5 or not re.fullmatch(r"[A-Z]+[0-9]+", cells[0]):
            continue
        rid, _, status, evidence, notes = cells[:5]
        if rid in seen:
            errors.append(f"{rid}: listed twice")
            continue
        seen.add(rid)
        if status not in STATUSES:
            errors.append(f"{rid}: unknown status {status!r}")
            continue
        tokens = TOKEN.findall(evidence)
        if status in NEEDS_EVIDENCE and not tokens:
            errors.append(f"{rid}: status {status} needs evidence")
        if status not in NEEDS_EVIDENCE and not notes:
            errors.append(f"{rid}: status {status} needs a note (why, and where it is promised)")
        for t in tokens:
            e = resolve(t, root, sources)
            if e:
                errors.append(f"{rid}: {e}")
    errors += [f"{rid}: missing from REQUIREMENTS.md" for rid in required if rid not in seen]
    return errors


def main(argv):
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    root = argv[argv.index("--root") + 1] if "--root" in argv else os.path.dirname(os.path.abspath(argv[1]))
    errors = check(open(argv[1], encoding="utf-8").read(), root)
    for e in errors:
        print("check-requirements: " + e, file=sys.stderr)
    if not errors:
        print("check-requirements: every requirement maps to existing evidence, a milestone or a descope")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
