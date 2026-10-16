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
export SEQ_RISC0_DEV_MODE="$seq_mode"
SUMMARY="$LN/demo-summary.txt"
phase() { printf '
=== %s
' "$*" | tee -a "$SUMMARY"; }
# expect NAME ACTUAL eq|gt|ge EXPECTED (integers of any size)
expect() {
  local name="$1" got="$2" op="$3" want="$4"
  if python3 -c 'import sys
a, op, b = int(sys.argv[1]), sys.argv[2], int(sys.argv[3])
sys.exit(0 if {"eq": a == b, "gt": a > b, "ge": a >= b}[op] else 1)' "$got" "$op" "$want"; then
    echo "check $name: $got $op $want: ok" | tee -a "$SUMMARY"
  else
    echo "check $name: $got is not $op $want" | tee -a "$SUMMARY" >&2
    exit 1
  fi
}
on_exit() {
  local s=$?
  if (( keep == 0 )) && [[ -f "$LN/sequencer.pid" ]]; then ln_stop_sequencer || true; fi
  if (( s != 0 )); then echo "demo FAILED (exit $s); logs in $LN/logs, summary in $SUMMARY" >&2; fi
}

rm -rf "${LN:?}"
mkdir -p "$LN/logs"
: > "$SUMMARY"
trap on_exit EXIT

phase "build auction-cli; committed guests (artifacts/guests, ADR-047)"
ln_build
"$REPO/scripts/disk-guard.sh" "${DEMO_MIN_C_GIB:-3}" "${DEMO_MIN_HOME_GIB:-20}" || exit 3
phase "standalone LEZ v0.2.4 sequencer (RISC0_DEV_MODE=$seq_mode)"
ln_start_sequencer
phase "accounts, auth-transfer init, deploy token, twap_oracle, token_mint_authority, stablecoin (+hooks), auction_engine"
ln_accounts
# oracle-src signs twice (the first oracle, then the publisher's), so it is initialised too (ADR-018).
ln_auth admin engine-admin keeper owner caller oracle-src
ln_deploy
phase "host: collateral token, oracle at R = 1, initialize_program"
ln_host
phase "engine: mock protocol token (mint authority = engine PDA), init_config (demo parameters), bind, collateral config"
ln_engine "$REPO/cli/params/demo-engine.json"
ln_bind
ln_collateral "$REPO/cli/params/demo-collateral.json"
ln_holdings

phase "positions p0, p1, p2; min collateral ratio 1.5 -> 2.0 makes p0 and p2 liquidatable"
p0="$(run_field position host open-position --owner owner --owner-collateral owner-coll --nonce 0 --amount 500000)"
cli host generate-debt --owner owner --position "$p0" --owner-stable owner-stable --amount 300000
p1="$(run_field position host open-position --owner owner --owner-collateral owner-coll --nonce 1 --amount 1000000)"
cli host generate-debt --owner owner --position "$p1" --owner-stable owner-stable --amount 100000
p2="$(run_field position host open-position --owner owner --owner-collateral owner-coll --nonce 2 --amount 500000)"
cli host generate-debt --owner owner --position "$p2" --owner-stable owner-stable --amount 300000
cli host set-min-cr --admin admin --ratio 2000000000000000000000000000
cli list-at-risk | tee -a "$SUMMARY"

phase "F1-F3: liquidate p0 (keeper reward), partial bid, final bid, leftover claimed"
cli quote-liquidation --position "$p0" | tee -a "$SUMMARY"
a0="$(run_field auction liquidate --position "$p0" --keeper keeper --keeper-holding keeper-coll)"
cli quote-bid --auction "$a0" --coin-in-max 100000 | tee -a "$SUMMARY"
cli bid --auction "$a0" --coin-in-max 100000 --bidder-stable owner-stable --bidder-collateral owner-coll
cli bid --auction "$a0" --coin-in-max 230000 --bidder-stable owner-stable --bidder-collateral owner-coll
cli claim-leftover --owner owner --owner-holding owner-coll --auction "$a0"
buffer_id="$(run_field buffer admin addresses --admin engine-admin --salt 1)"
expect "buffer holds the 10 % penalty" "$(balance_of "$buffer_id")" eq 30000

phase "F5: surplus auction sells buffer coin for protocol tokens, which are burned"
cli observe-feed --caller caller
cli settle-debt --caller caller
cli check-solvency --caller caller
cli start-surplus --caller caller
cli take-surplus --payer taker-protocol --receiver taker-stable --amount 500
expect "buffer after the surplus lot" "$(balance_of "$buffer_id")" eq 29500

phase "price move: the localnet publisher sets the market price to 1.9 R (ADR-050)"
cli dev oracle-publish --price-source oracle-src --price 1900000000000000000000000000 --host-admin admin --engine-admin engine-admin
cli host status | tee -a "$SUMMARY"

phase "shortfall: liquidate p2 at 1.9 R; the lot-binding bid raises less than the debt"
a2="$(run_field auction liquidate --position "$p2" --keeper keeper --keeper-holding keeper-coll)"
cli bid --auction "$a2" --coin-in-max 330000 --bidder-stable owner-stable --bidder-collateral owner-coll
cli list-auctions --all | tee -a "$SUMMARY"
u1="$(unbacked)"
expect "unbacked debt after the shortfall" "$u1" gt 0

phase "F4: settle against the buffer, start a debt auction, take one lot (stablecoin burned, protocol tokens minted)"
cli settle-debt --caller caller
u2="$(unbacked)"
expect "the buffer settled 29_500 of the bad debt" "$u2" eq "$((u1 - 29500))"
expect "enough bad debt for one lot" "$u2" ge 5000
cli start-debt --caller caller
cli dev token-transfer --from owner-stable --to taker-stable --amount 5000
tokens_before="$(balance_of taker-protocol)"
cli take-debt --payer taker-stable --receiver taker-protocol --amount 5000
expect "protocol tokens minted to the taker" "$(balance_of taker-protocol)" gt "$tokens_before"
u3="$(unbacked)"
expect "one lot of bad debt burned" "$u3" eq "$((u2 - 5000))"
expect "bad debt still above the 5_000 freeze threshold" "$u3" gt 5000

phase "freeze path: without a protocol token, bad debt above the threshold freezes new debt"
cli admin set-engine-params --admin engine-admin --params "$REPO/cli/params/demo-engine.json" --disable-protocol-token
cli check-solvency --caller caller
frozen="$(run_field frozen host status)"
[[ "$frozen" == yes ]] || { echo "check host frozen: got $frozen" | tee -a "$SUMMARY" >&2; exit 1; }
echo "check host frozen: yes" | tee -a "$SUMMARY"

phase "final check"
cli admin verify | tee -a "$SUMMARY"
ln_check_log | tee -a "$SUMMARY"
"$REPO/scripts/disk-guard.sh" "${DEMO_MIN_C_GIB:-3}" "${DEMO_MIN_HOME_GIB:-20}" | tee -a "$SUMMARY"
echo "demo complete" | tee -a "$SUMMARY"
