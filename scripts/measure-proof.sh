#!/usr/bin/env bash
# Times one real-proof test (ADR-045) and records its peak RSS. Run INSIDE WSL from the repo root:
#   scripts/measure-proof.sh <test_name> <po2> <timeout_seconds>
# Builds the test binary first (so compilation is not measured), then runs exactly that test with
# AUCTION_PROVE=1 RISC0_DEV_MODE=0 under /usr/bin/time -v. The prover is the external r0vm (a child
# of the test process); GNU time reports the largest RSS in the process tree.
set -euo pipefail
test_name="${1:?test name}"; po2="${2:?po2}"; limit="${3:?timeout seconds}"
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
"$repo/scripts/disk-guard.sh"
out_dir="$HOME/lez-target/plan3b-proofs"
mkdir -p "$out_dir"
log="$out_dir/${test_name}-po2-${po2}.log"
# shellcheck disable=SC1090
. "${LEZ_ENV_FILE:-$HOME/lez-spikes/s1/env.sh}"
r0vm --version | grep -q "3.0.5" || { echo "measure-proof: r0vm 3.0.5 required (rzup install r0vm 3.0.5)" >&2; exit 2; }
exe="$("$repo/scripts/wsl-cargo.sh" test -p engine_integration_tests --test real_proofs --no-run --message-format=json 2>"$log.build" \
  | python3 -c 'import json, sys
for line in sys.stdin:
    m = json.loads(line)
    if m.get("reason") == "compiler-artifact" and m.get("executable") and m["target"]["name"] == "real_proofs":
        print(m["executable"])' | tail -1)"
[[ -x "$exe" ]] || { echo "measure-proof: real_proofs test binary not found (build output: $log.build)" >&2; exit 2; }
set +e
AUCTION_PROVE=1 RISC0_DEV_MODE=0 AUCTION_PROVE_PO2="$po2" \
  /usr/bin/time -v timeout "$limit" "$exe" --ignored --exact "$test_name" --nocapture --test-threads 1 > "$log" 2>&1
status=$?
set -e
wall="$(sed -n 's/.*Elapsed (wall clock) time (h:mm:ss or m:ss): //p' "$log")"
rss_kb="$(sed -n 's/.*Maximum resident set size (kbytes): //p' "$log")"
# Not anchored: the first PROOF line follows libtest's "test <name> ... " on the same line.
grep -oE 'PROOF\[.*' "$log" || true
echo "RESULT test=$test_name po2=$po2 status=$status wall=$wall peak_rss_mib=$(( ${rss_kb:-0} / 1024 )) log=$log"
"$repo/scripts/disk-guard.sh" || echo "disk-guard: STOP after run" >&2
exit "$status"
