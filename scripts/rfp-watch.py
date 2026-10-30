#!/usr/bin/env python3
"""Watches for RFP-014 reopening and for an auction or liquidation λPrize.

usage: rfp-watch.py [--alert] [--rfp-readme FILE] [--prize-readme FILE]

Reads the logos-co/rfp and logos-co/lambda-prize READMEs (through `gh api`, or from files for tests)
and prints one line per finding. Nothing is printed while the RFP-014 row still says "Reopening soon"
and no prize mentions an auction or liquidation. A missing RFP-014 row is reported too, so a format
change upstream is never silent.

With --alert and at least one finding, opens one issue labelled `rfp-watch` in the current repository
(GitHub emails the owner), unless an open one already exists. Run by .github/workflows/rfp-watch.yml.
"""
import base64
import json
import re
import subprocess
import sys

RFP_REPO, PRIZE_REPO = "logos-co/rfp", "logos-co/lambda-prize"
RFP_URL = f"https://github.com/{RFP_REPO}#readme"
PRIZE_URL = f"https://github.com/{PRIZE_REPO}#readme"
LABEL = "rfp-watch"
WAITING = "reopening soon"
PRIZE_WORDS = re.compile(r"auction|liquidat", re.IGNORECASE)


def cells(line):
    return [c.strip() for c in line.strip().strip("|").split("|")]


def rfp_findings(readme):
    for line in readme.splitlines():
        if line.lstrip().startswith("| RFP-014 "):
            if WAITING in line.lower():
                return []
            return ["RFP-014 changed: " + " | ".join(cells(line)[2:])]
    return ["RFP-014 row not found in the RFP list (the README format may have changed)"]


def prize_findings(readme):
    return [f"λPrize mentions an auction: {' | '.join(cells(line))}"
            for line in readme.splitlines()
            if line.lstrip().startswith("|") and PRIZE_WORDS.search(line)]


def issue_body(findings):
    lines = "\n".join(f"- {f}" for f in findings)
    return (f"The RFP watcher found:\n\n{lines}\n\nSources: {RFP_URL} and {PRIZE_URL}\n\n"
            "Close this issue once handled; the watcher opens a new one on the next change.")


def gh(*args):
    return subprocess.run(["gh", *args], check=True, capture_output=True, text=True).stdout


def fetch_readme(repo):
    return base64.b64decode(json.loads(gh("api", f"repos/{repo}/readme"))["content"]).decode()


def read(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


def alert(findings):
    if json.loads(gh("issue", "list", "--label", LABEL, "--state", "open", "--json", "number")):
        print("an open rfp-watch issue already exists; not opening another")
        return
    gh("label", "create", LABEL, "--color", "D93F0B", "--force",
       "--description", "RFP-014 / λPrize watcher")
    gh("issue", "create", "--label", LABEL, "--title", "RFP watch: RFP-014 or λPrize changed",
       "--body", issue_body(findings))
    print("opened an rfp-watch issue")


def main(argv):
    args = dict(rfp=None, prize=None, alert=False)
    file_flags = {"--rfp-readme": "rfp", "--prize-readme": "prize"}
    it = iter(argv)
    for a in it:
        if a == "--alert":
            args["alert"] = True
        elif a in file_flags:
            args[file_flags[a]] = next(it)
        else:
            print(__doc__, file=sys.stderr)
            return 2
    rfp = read(args["rfp"]) if args["rfp"] else fetch_readme(RFP_REPO)
    prize = read(args["prize"]) if args["prize"] else fetch_readme(PRIZE_REPO)
    findings = rfp_findings(rfp) + prize_findings(prize)
    for f in findings:
        print(f)
    if not findings:
        print("no change: RFP-014 still 'Reopening soon', no auction λPrize")
    elif args["alert"]:
        alert(findings)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
