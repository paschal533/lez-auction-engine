#!/usr/bin/env bash
# Proves the whole ADR-036 suite locally, one chains.rs test at a time (NIGHTLY tier C, ADR-045).
# Run INSIDE WSL from the repo root; never next to a cargo build or another proving job.
#   scripts/prove-suite.sh <po2> <timeout_seconds_per_test> [test ...]
# With no test names it runs all three chains.rs tests; with names it runs only those (and appends to the
# existing report, so a long suite can be split across calls). Logs, the report and summary.txt go to
# $HOME/lez-target/plan3b-proofs/suite-po2-<po2>/.
set -euo pipefail
po2="${1:?po2}"; limit="${2:?timeout per test}"; shift 2
tests=("$@")
if (( ${#tests[@]} == 0 )); then
  tests=(every_instruction_fits_the_chain_budget debt_and_solvency_chains_fit_the_budget implicit_burn_variants_fit_the_budget)
  fresh=1
fi
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
out="$HOME/lez-target/plan3b-proofs/suite-po2-$po2"
mkdir -p "$out"
[[ -z "${fresh:-}" ]] || rm -f "$out/proofs.jsonl"
"$repo/scripts/disk-guard.sh"
# Build once, outside the timed runs (dev mode: the build never proves).
exe="$("$repo/scripts/wsl-cargo.sh" test --locked -p engine_integration_tests --test chains --no-run --message-format=json-render-diagnostics 2>"$out/build.log" \
  | python3 -c 'import json, sys
for line in sys.stdin:
    m = json.loads(line)
    if m.get("reason") == "compiler-artifact" and m.get("executable") and m["target"]["name"] == "chains":
        print(m["executable"])' | tail -1)" || true
# A failed build prints nothing here (|| true above); rustc diagnostics are in build.log.
[[ -n "$exe" && -x "$exe" ]] || { echo "prove-suite: chains test binary not found (build output: $out/build.log)" >&2; exit 2; }
# shellcheck disable=SC1090
. "${LEZ_ENV_FILE:-$HOME/lez-spikes/s1/env.sh}"
for t in "${tests[@]}"; do
  "$repo/scripts/disk-guard.sh"
  ( cd "$repo" && AUCTION_PROVE=1 RISC0_DEV_MODE=0 AUCTION_PROVE_PO2="$po2" PROOF_REPORT="$out/proofs.jsonl" \
      /usr/bin/time -v timeout --kill-after=30 "$limit" "$exe" --exact "$t" --test-threads 1 --nocapture ) \
    > "$out/$t.log" 2>&1 || {
      # timeout kills the test's whole process group (r0vm included); this also catches a test that
      # failed on its own and left its prover child behind. Safe: one proving job at a time (see above).
      pkill -x r0vm || true
      echo "prove-suite: $t failed, see $out/$t.log" >&2; exit 1; }
  echo "prove-suite: $t done: $(sed -n 's/.*Elapsed (wall clock) time (h:mm:ss or m:ss): //p' "$out/$t.log")"
done
# A partial run (test names given) cannot expect every label; run the check again without names to finish.
expect=(--expect 'liquidate,take_debt,check_solvency(freeze)')
[[ -n "${fresh:-}" ]] || expect=()
python3 "$repo/scripts/check-proof-report.py" "$out/proofs.jsonl" "${expect[@]}" | tee "$out/summary.txt"
