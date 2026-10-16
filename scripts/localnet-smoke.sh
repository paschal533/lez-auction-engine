#!/usr/bin/env bash
# Plan 3a localnet smoke (Plan 3b Task 12: on scripts/lib/localnet.sh). Run INSIDE WSL, through
# scripts/wsl-env.sh on the dev machine. Steps, in order (several may be given in one call):
#   step_build step_sequencer step_accounts step_auth step_deploy step_host step_engine step_bind
#   step_collateral step_holdings step_flow step_system step_check step_peak step_stop
# SEQ_RISC0_DEV_MODE=0 runs the sequencer with dev mode off (RFP S2). Deploys the committed guests
# (artifacts/guests, ADR-047); needs the LEZ sequencer and wallet (S1 builds or scripts/bootstrap-lez.sh).
set -euo pipefail
LN="${SMOKE_RUN_DIR:-$HOME/lez-target/plan3a-localnet}"
mkdir -p "$LN/logs"
# shellcheck disable=SC1091
. "$(dirname "${BASH_SOURCE[0]}")/lib/localnet.sh"

step_build() { "$REPO/scripts/check-guests.sh" --sha-only "$GUESTS_DIR"; ln_build; }
step_sequencer() {
  if (ss -ltn 2>/dev/null || true) | grep -q ":$PORT "; then
    echo "port $PORT is already in use; not starting a second sequencer" >&2
    exit 1
  fi
  ln_start_sequencer
}
step_accounts() { ln_accounts; }
step_auth() { ln_auth admin engine-admin keeper owner caller; }
step_deploy() { ln_deploy; }
step_host() { ln_host; }
step_engine() { ln_engine "$REPO/cli/params/localnet-engine.json"; }
step_bind() {
  # Bind-time verification (ADR-027) is part of the command: it refuses (exit 2) unless the config verifies.
  ln_bind
}
step_collateral() { ln_collateral "$REPO/cli/params/localnet-collateral.json"; }
step_holdings() { ln_holdings; }

step_flow() {
  local p0 p1 a
  p0="$(run_field position host open-position --owner owner --owner-collateral owner-coll --nonce 0 --amount 500000)"
  cli host generate-debt --owner owner --position "$p0" --owner-stable owner-stable --amount 300000
  p1="$(run_field position host open-position --owner owner --owner-collateral owner-coll --nonce 1 --amount 1000000)"
  cli host generate-debt --owner owner --position "$p1" --owner-stable owner-stable --amount 100000
  cli host set-min-cr --admin admin --ratio 2000000000000000000000000000
  cli list-at-risk
  cli quote-liquidation --position "$p0"
  a="$(run_field auction liquidate --position "$p0" --keeper keeper --keeper-holding keeper-coll)"
  remember P0 "$p0"
  remember AUCTION "$a"
  cli list-auctions
  cli quote-bid --auction "$a" --coin-in-max 100000
  cli bid --auction "$a" --coin-in-max 100000 --bidder-stable owner-stable --bidder-collateral owner-coll
  cli bid --auction "$a" --coin-in-max 230000 --bidder-stable owner-stable --bidder-collateral owner-coll
  cli claim-leftover --owner owner --owner-holding owner-coll --auction "$a"
  cli list-auctions --all
}

step_system() {
  cli observe-feed --caller caller
  cli settle-debt --caller caller
  cli check-solvency --caller caller
  cli start-surplus --caller caller
  cli take-surplus --payer taker-protocol --receiver taker-stable --amount 500
  cli list-auctions --all
}

step_check() {
  cli admin verify
  local a
  for a in owner-coll owner-stable keeper-coll taker-stable taker-protocol; do cli dev balance --account "$a"; done
  cli dev balance --account "$(run_field buffer admin addresses --admin engine-admin --salt 1)"
  ln_check_log
}

step_peak() { ln_peak; }
step_stop() { ln_stop_sequencer || exit 1; }

for step in "$@"; do
  case "$step" in
    step_*) "$step" ;;
    *) echo "unknown step: $step" >&2; exit 2 ;;
  esac
done
