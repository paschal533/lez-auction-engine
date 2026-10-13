#!/usr/bin/env bash
# Measures every instruction's executor cycles and signed transaction size (chains.rs scenarios, ADR-036)
# into a JSON-lines file for scripts/check-cycles.py. Locally: scripts/wsl-env.sh scripts/cycles.sh OUT
set -euo pipefail
out="$(realpath -m "${1:?usage: cycles.sh OUT.jsonl}")"
cd "$(dirname "${BASH_SOURCE[0]}")/.."
rm -f "$out"
CYCLES_OUT="$out" cargo test --locked -p engine_integration_tests --test chains -- --test-threads 1
test -s "$out" || { echo "cycles.sh: no measurements were written to $out" >&2; exit 1; }
echo "cycles.sh: $(wc -l < "$out") measurements in $out"
