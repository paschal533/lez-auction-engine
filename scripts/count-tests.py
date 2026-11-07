#!/usr/bin/env python3
"""Count the repo's tests by area, so published numbers cannot drift (Plan 8 Task 3).

usage: count-tests.py [--root DIR] [--markdown] [--check FILE]
  (no flag)     print one `area: n` line per area
  --markdown    print the table that goes between the README markers
  --check FILE  exit 1 if the block between <!-- test-counts:begin --> and <!-- test-counts:end -->
                in FILE differs from the --markdown output (exit 2 on bad usage or unreadable FILE)

Each Rust fn is counted once, by its attribute: #[test], #[tokio::test] (any #[path::test]) go to the
area of their directory (v04/, programs/, crates/, cli/, fuzz/); #[kani::proof] goes to fuzz_kani
wherever it is. Rust files elsewhere (tools/, spikes/) are not counted. QML: `function test_` in
apps/**/tests/qml/*.qml. C++: LOGOS_TEST( / TEST( / `void test` in modules/**/tests. Python: `def test_`
in scripts/tests/*.py. Directories named target, node_modules, .git, corpus, artifacts, .superpowers
are skipped.
"""
import os
import re
import sys

BEGIN, END = "<!-- test-counts:begin -->", "<!-- test-counts:end -->"
SKIP_DIRS = {"target", "node_modules", ".git", "corpus", "artifacts", ".superpowers"}
RUST_AREAS = (("v04", "v04"), ("programs", "v024_programs"), ("crates", "crates"),
              ("cli", "cli"), ("fuzz", "fuzz_kani"))
AREAS = [a for _, a in RUST_AREAS] + ["qml", "cpp", "python"]
LABELS = {
    "v04": "0.4 engine (v04/)", "v024_programs": "v0.2.4 programs", "crates": "Shared crates",
    "cli": "CLI", "fuzz_kani": "Fuzz + Kani proofs", "qml": "Basecamp QML",
    "cpp": "Core module C++", "python": "Python tooling",
}
RUST_TEST = re.compile(r"^[ \t]*#\[(?:[A-Za-z_]\w*::)*test(?:\s*\([^\]]*\))?\]", re.M)
KANI_PROOF = re.compile(r"^[ \t]*#\[kani::proof\b[^\]]*\]", re.M)
QML_TEST = re.compile(r"^\s*function\s+test_\w+\s*\(", re.M)
CPP_TEST = re.compile(r"^\s*(?:LOGOS_TEST\(\w+\)|TEST\w*\(\w+\s*,|void\s+test\w*\s*\()", re.M)
PY_TEST = re.compile(r"^\s*(?:async\s+)?def\s+test_\w+\s*\(", re.M)


def walk(root):
    """Yield (posix path relative to root, absolute path) for every file outside the skipped dirs."""
    for dirpath, dirnames, files in os.walk(root):
        dirnames[:] = sorted(d for d in dirnames if d not in SKIP_DIRS)
        for f in sorted(files):
            full = os.path.join(dirpath, f)
            yield os.path.relpath(full, root).replace(os.sep, "/"), full


def read(path):
    with open(path, encoding="utf-8-sig", errors="replace") as f:
        return f.read()


RAW_STRING_OPEN = re.compile(r'b?r(#*)"')


def blank(text, start, end):
    """text[start:end] with everything but newlines replaced by spaces."""
    return "".join(c if c == "\n" else " " for c in text[start:end])


def mask_rust(src):
    """Blank comments (// and nested /* */), string literals (plain and raw) and char literals,
    keeping every newline, so line-anchored attribute regexes only see real code."""
    out, i, n = [], 0, len(src)
    while i < n:
        c = src[i]
        j = i + 1
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
        elif src.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif src.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
        elif c in "rb" and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")) \
                and RAW_STRING_OPEN.match(src, i):
            m = RAW_STRING_OPEN.match(src, i)
            close = src.find('"' + m.group(1), m.end())
            j = n if close < 0 else close + 1 + len(m.group(1))
        elif c == '"':
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            j = min(j + 1, n)
        elif c == "'" and src.startswith("\\", i + 1):
            close = src.find("'", i + 3)
            j = n if close < 0 else close + 1
        elif c == "'" and src.startswith("'", i + 2):
            j = i + 3  # 'x'; anything else after ' is a lifetime, which is plain code
        else:
            out.append(c)
            i += 1
            continue
        out.append(blank(src, i, j))
        i = j
    return "".join(out)


def hits(pattern, path, rust=False):
    text = read(path)
    return len(pattern.findall(mask_rust(text) if rust else text))


def count(root):
    counts = dict.fromkeys(AREAS, 0)
    for rel, full in walk(root):
        parts = rel.split("/")
        if rel.endswith(".rs"):
            counts["fuzz_kani"] += hits(KANI_PROOF, full, True)
            area = next((a for d, a in RUST_AREAS if parts[0] == d), None)
            if area:
                counts[area] += hits(RUST_TEST, full, True)
        elif rel.endswith(".qml") and parts[0] == "apps" and "tests" in parts and parts[-2] == "qml":
            counts["qml"] += hits(QML_TEST, full)
        elif rel.endswith((".cpp", ".cc", ".h")) and parts[0] == "modules" and "tests" in parts[:-1]:
            counts["cpp"] += hits(CPP_TEST, full)
        elif rel.endswith(".py") and rel.startswith("scripts/tests/") and len(parts) == 3:
            counts["python"] += hits(PY_TEST, full)
    counts["total"] = sum(counts[a] for a in AREAS)
    return counts


def markdown(counts):
    rows = ["| Area | Tests |", "|---|---:|"]
    rows += [f"| {LABELS[a]} | {counts[a]} |" for a in AREAS]
    rows.append(f"| **Total** | {counts['total']} |")
    return "\n".join(rows) + "\n"


def block(text):
    """The text strictly between the markers, or None when either marker is missing."""
    start, end = text.find(BEGIN), text.find(END)
    if start < 0 or end < start:
        return None
    return text[start + len(BEGIN):end].strip() + "\n"


def check(path, counts):
    try:
        current = block(read(path))
    except OSError as e:
        print(f"count-tests: cannot read {path}: {e}", file=sys.stderr)
        return 2
    if current is None:
        print(f"count-tests: {path} lacks the {BEGIN} / {END} markers", file=sys.stderr)
        return 1
    if current != markdown(counts):
        print(f"count-tests: the test-count block in {path} is stale; paste `count-tests.py --markdown` "
              f"between the markers", file=sys.stderr)
        return 1
    print("count-tests: the test-count block is current")
    return 0


def main(argv):
    args = argv[1:]
    root, check_file, want_md = ".", None, False
    try:
        while args:
            a = args.pop(0)
            if a == "--root":
                root = args.pop(0)
            elif a == "--check":
                check_file = args.pop(0)
            elif a == "--markdown":
                want_md = True
            else:
                raise ValueError(a)
    except (IndexError, ValueError):
        print(__doc__, file=sys.stderr)
        return 2
    counts = count(root)
    if check_file:
        return check(check_file, counts)
    if want_md:
        print(markdown(counts), end="")
    else:
        for a in AREAS + ["total"]:
            print(f"{a}: {counts[a]}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
