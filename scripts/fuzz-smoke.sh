#!/usr/bin/env bash
# Runs every cargo-fuzz target for SECONDS each (spec §10). Needs `rustup toolchain install nightly` and
# cargo-fuzz. Builds go to $CARGO_TARGET_DIR/fuzz (WSL: $HOME/lez-target/plan2/fuzz, deleted after use).
#   scripts/fuzz-smoke.sh [SECONDS=60]
set -euo pipefail
secs="${1:-60}"
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
tdir="${CARGO_TARGET_DIR:-$repo/target}/fuzz"
cd "$repo"
for t in $(cargo +nightly fuzz list); do
  echo "== fuzz $t for $secs s"
  cargo +nightly fuzz run --target-dir "$tdir" "$t" -- -max_total_time="$secs" -rss_limit_mb=4096
done
echo "fuzz-smoke: every target ran $secs s without a crash"
