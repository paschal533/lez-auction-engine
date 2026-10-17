#!/usr/bin/env bash
# One-command reproduction (spec §1.1, ADR-051). Inside Linux or WSL:
#   scripts/demo.sh [--sequencer-dev-mode 0|1] [--keep] [--preflight-only]
# The author's machine: scripts/wsl-env.sh scripts/demo.sh (shared target dir, S1 builds); run it that way.
# A clean machine first runs scripts/bootstrap-lez.sh (DEMO.md).
set -euo pipefail
seq_mode=1; keep=0; preflight_only=0
while (( $# )); do
  case "$1" in
    --sequencer-dev-mode)
      seq_mode="${2:-}"; shift
      [[ "$seq_mode" == 0 || "$seq_mode" == 1 ]] || { echo "usage: --sequencer-dev-mode takes 0 or 1" >&2; exit 2; }
      ;;
    --keep) keep=1 ;;
    --preflight-only) preflight_only=1 ;;
    *) echo "usage: scripts/demo.sh [--sequencer-dev-mode 0|1] [--keep] [--preflight-only]" >&2; exit 2 ;;
  esac
  shift
done
LN="${DEMO_RUN_DIR:-$HOME/lez-target/demo}"
# Always the throwaway localnet: ignore network settings left in the environment (scripts/lib/localnet.sh
# defaults). scripts/testnet-legacy.sh drives the same scenario (scripts/lib/demo-scenario.sh) remotely.
unset LN_NETWORK LN_SEQUENCER_URL LN_LOCAL_SEQUENCER LN_GENESIS LN_DEPLOY_GUESTS LN_SKIP_DEPLOYED \
  LN_RESET_WALLET LN_WALLET_LOG LN_WALLET_POLL_TIMEOUT LN_WALLET_POLL_MAX_BLOCKS DEMO_DEPLOY_PHASE DEMO_PUBLISHER
# shellcheck disable=SC1091
. "$(dirname "${BASH_SOURCE[0]}")/lib/localnet.sh"
ln_preflight
if (( preflight_only )); then echo "preflight ok"; exit 0; fi
export SEQ_RISC0_DEV_MODE="$seq_mode"
SUMMARY="$LN/demo-summary.txt"
# shellcheck disable=SC1091
. "$(dirname "${BASH_SOURCE[0]}")/lib/demo-scenario.sh"
on_exit() {
  local s=$?
  if (( keep == 0 )) && [[ -f "$LN/sequencer.pid" ]]; then ln_stop_sequencer || true; fi
  if (( s != 0 )); then echo "demo FAILED (exit $s); logs in $LN/logs, summary in $SUMMARY" >&2; fi
}

# Never wipe an arbitrary DEMO_RUN_DIR: only an empty dir, a previous demo run (marker or summary) may be reused.
if [[ -n "$(ls -A "$LN" 2>/dev/null)" && ! -f "$LN/.demo-run" && ! -f "$LN/demo-summary.txt" ]]; then
  echo "demo: $LN exists and is not a demo run directory; refusing to delete it" >&2
  exit 2
fi
rm -rf "${LN:?}"
mkdir -p "$LN/logs"
touch "$LN/.demo-run"
: > "$SUMMARY"
trap on_exit EXIT

phase "build auction-cli; committed guests (artifacts/guests, ADR-047)"
ln_build
"$REPO/scripts/disk-guard.sh" "${DEMO_MIN_C_GIB:-3}" "${DEMO_MIN_HOME_GIB:-20}" || exit 3
phase "standalone LEZ v0.2.4 sequencer (RISC0_DEV_MODE=$seq_mode)"
ln_start_sequencer
demo_scenario
