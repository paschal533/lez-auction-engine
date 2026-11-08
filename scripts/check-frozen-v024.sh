#!/usr/bin/env bash
# ADR-069: the v0.2.4 guest's source paths stay frozen. Exits 1 (stat on stderr) if any of them differ from BASE.
# Usage: scripts/check-frozen-v024.sh [BASE]   (BASE defaults to 23a57bb; env FROZEN_ROOT defaults to the repo)
set -euo pipefail

BASE="${1:-23a57bb}"
FROZEN_ROOT="${FROZEN_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"

stat="$(git -C "$FROZEN_ROOT" diff --stat "$BASE" -- \
  programs/ crates/auction_core/ crates/auction_engine_params/ Cargo.toml Cargo.lock)"

if [ -n "$stat" ]; then
  {
    echo "check-frozen-v024: frozen v0.2.4 paths changed since $BASE"
    echo "$stat"
  } >&2
  exit 1
fi
