#!/usr/bin/env bash
# One-command reproduction (spec §1.1, ADR-051). Inside Linux or WSL:
#   scripts/demo.sh [--sequencer-dev-mode 0|1] [--keep] [--preflight-only]
# The author's machine: scripts/wsl-env.sh scripts/demo.sh (shared target dir, S1 builds); run it that way.
# A clean machine first runs scripts/bootstrap-lez.sh (DEMO.md).
set -euo pipefail
seq_mode=1; keep=0; preflight_only=0
while (( $# )); do
  case "$1" in
    --sequencer-dev-mode) seq_mode="${2:?0 or 1}"; shift ;;
    --keep) keep=1 ;;
    --preflight-only) preflight_only=1 ;;
    *) echo "usage: scripts/demo.sh [--sequencer-dev-mode 0|1] [--keep] [--preflight-only]" >&2; exit 2 ;;
  esac
  shift
done
LN="${DEMO_RUN_DIR:-$HOME/lez-target/demo}"
# shellcheck disable=SC1091
. "$(dirname "${BASH_SOURCE[0]}")/lib/localnet.sh"
ln_preflight
if (( preflight_only )); then echo "preflight ok"; exit 0; fi
echo "the demo run is added in Plan 3b Task 13" >&2
exit 2
