#!/usr/bin/env bash
# The demo scenario (scripts/lib/demo-scenario.sh) on the public LEZ v0.2.4 legacy testnet
# (docs/architecture/testnet-legacy.md). Inside Linux or WSL:
#   scripts/testnet-legacy.sh --preflight-only     read-only checks, sends nothing
#   scripts/testnet-legacy.sh --send               deploy stablecoin + auction_engine, run the scenario
#   scripts/testnet-legacy.sh --resume RUN_DIR     continue an interrupted --send run after its last
#                                                  finished phase (never re-sends an applied step)
# Options: --state-dir DIR (default $HOME/lez-target/testnet-legacy, or TESTNET_STATE_DIR; not under
# /mnt), --url URL (default the public endpoint, or TESTNET_URL). TESTNET_GENESIS overrides the expected
# block-1 hash; KEEPER_REFRESH_BEFORE_MS (default 1_800_000) is how early a keeper round re-publishes
# the mock price. The author's machine: scripts/wsl-env.sh scripts/testnet-legacy.sh ...
# Every run gets its own directory under the state dir (keys, cursor, logs, summary), mode 700; nothing
# is deleted. Exit codes: 2 usage or state dir, 3 disk, 4 LEZ wallet or tools missing, 6 guests do not
# match the manifest, 7 node unreachable, 8 another chain (block 1), 9 the stablecoin host is already
# initialised, 10 a resume would risk sending a step twice (the message says what to check).
set -euo pipefail
# Keys, cursor and logs of a public-network run are private: 700 directories, 600 files.
umask 077
mode=""; resume_dir=""; url_arg=""
root="${TESTNET_STATE_DIR:-$HOME/lez-target/testnet-legacy}"
usage="usage: scripts/testnet-legacy.sh (--preflight-only | --send | --resume RUN_DIR) [--state-dir DIR] [--url URL]"
while (( $# )); do
  case "$1" in
    --preflight-only) mode=preflight ;;
    --send) mode=send ;;
    --resume) mode=resume; resume_dir="${2:?$usage}"; shift ;;
    --state-dir) root="${2:?$usage}"; shift ;;
    --url) url_arg="${2:?$usage}"; shift ;;
    *) echo "$usage" >&2; exit 2 ;;
  esac
  shift
done
[[ -n "$mode" ]] || { echo "$usage" >&2; exit 2; }
DEFAULT_URL="https://testnet-legacy.lez.logos.co/"
# Block 1 of the legacy testnet (LEZ build v0.2.4-lez-legacy), recorded 2026-10-17.
DEFAULT_GENESIS="c8e3bb9ec8f246e5c2759e0e8152840aeb7df29948a9ad12c9c302a641230011"
KEEPER_REFRESH_BEFORE_MS="${KEEPER_REFRESH_BEFORE_MS:-1800000}"
# Blocks without inclusion before auction-cli calls a transaction dropped (60 s blocks; CLI default 5).
TN_DROP_AFTER_BLOCKS=10
ROOT_MARKER=".testnet-legacy-state"
RUN_MARKER=".testnet-run"
under_mnt() { case "$(realpath -m "$1")" in /mnt/*) return 0 ;; *) return 1 ;; esac; }

if [[ "$mode" == resume ]]; then
  LN="$(realpath -m "$resume_dir")"
  if [[ ! -f "$LN/$RUN_MARKER" || ! -f "$LN/deployment.json" || ! -f "$LN/run.env" ]]; then
    echo "testnet-legacy: $LN is not a testnet-legacy run directory (no $RUN_MARKER, deployment.json or run.env)" >&2
    exit 2
  fi
  root="$(dirname "$LN")"
  # The run's own node and chain, as recorded when it started.
  read -r url genesis < <(python3 -c 'import json, sys
d = json.load(open(sys.argv[1], encoding="utf-8"))
print(d["sequencer_url"], d["chain_genesis_hash"])' "$LN/deployment.json")
  if [[ -n "$url_arg" && "$url_arg" != "$url" ]]; then
    echo "testnet-legacy: --url $url_arg differs from the run's node $url; refusing to resume against another node" >&2
    exit 2
  fi
else
  url="${url_arg:-${TESTNET_URL:-$DEFAULT_URL}}"
  genesis="${TESTNET_GENESIS:-$DEFAULT_GENESIS}"
  if [[ -z "$url_arg" && -n "${TESTNET_URL:-}" ]]; then echo "testnet-legacy: WARNING: node URL from TESTNET_URL: $url" >&2; fi
  if [[ -n "${TESTNET_GENESIS:-}" ]]; then echo "testnet-legacy: WARNING: expected block 1 from TESTNET_GENESIS: $genesis" >&2; fi
fi
# /mnt/* is a Windows drive under WSL: no Unix permissions, so the keys would be world-readable.
if under_mnt "$root"; then
  echo "testnet-legacy: the state dir $root is under /mnt (no Unix permissions for the keys); use a WSL-native path" >&2
  exit 2
fi
# The state dir holds earlier runs' keys: never reuse a directory that is not ours.
if [[ "$mode" != resume && -n "$(ls -A "$root" 2>/dev/null)" && ! -f "$root/$ROOT_MARKER" ]]; then
  echo "testnet-legacy: $root exists and is not a testnet-legacy state directory; refusing to use it" >&2
  exit 2
fi
if [[ "$mode" == preflight ]]; then
  LN="$(mktemp -d)"
  trap 'rm -rf "$LN"' EXIT
elif [[ "$mode" == send ]]; then
  LN="$root/run-$(date -u +%Y%m%dT%H%M%SZ)"
  [[ ! -e "$LN" ]] || { echo "testnet-legacy: $LN already exists" >&2; exit 2; }
fi

export LN_NETWORK=testnet-legacy LN_SEQUENCER_URL="$url" LN_LOCAL_SEQUENCER=0 LN_GENESIS="$genesis"
export LN_DEPLOY_GUESTS="stablecoin auction_engine" LN_SKIP_DEPLOYED=1 LN_RESET_WALLET=0
# The LEZ wallet poller sleeps seq_poll_timeout (max_blocks - 1) times: 39 x 30 s = 19.5 min per transaction.
export LN_WALLET_LOG="$LN/logs/wallet-accounts.log" LN_WALLET_POLL_TIMEOUT=30s LN_WALLET_POLL_MAX_BLOCKS=40
DEMO_DEPLOY_PHASE="accounts (fresh keys), auth-transfer init, deploy stablecoin (+hooks) and auction_engine unless already deployed"
DEMO_PUBLISHER="mock (self-signed)"
# shellcheck disable=SC1091
. "$(dirname "${BASH_SOURCE[0]}")/lib/localnet.sh"

# auction-cli with the testnet drop window and, during a run, a record of every submitted transaction
# (command | instruction | hash) in $TXS.
cli() {
  local out s=0
  out="$("$CLI_BIN" --deployment "$DEP" --state-dir "$LN/cli-state" --wallet-home "$LEE_WALLET_HOME_DIR" \
    --drop-after-blocks "$TN_DROP_AFTER_BLOCKS" "$@")" || s=$?
  printf '%s\n' "$out"
  if [[ -n "${TXS:-}" ]]; then
    awk -v cmd="$*" '$1 == "plan" { p = $2 } $1 == "submitted" { print cmd " | " p " | " $2 }' <<<"$out" >> "$TXS"
  fi
  return "$s"
}

# Read-only checks against the node: reachable, the expected chain, which guests it has.
tn_preflight() {
  local last found g at
  last="$(python3 "$LEZ_RPC" last-block "$url")" || { echo "preflight: $url is not reachable" >&2; exit 7; }
  found="$(python3 "$LEZ_RPC" genesis "$url")" || { echo "preflight: could not read block 1 from $url" >&2; exit 7; }
  echo "node              $url (last block $last)"
  if [[ "$found" != "$genesis" ]]; then
    echo "preflight: block 1 of $url is $found, expected $genesis (reset or another chain)" >&2
    exit 8
  fi
  echo "chain identity    block 1 $found (expected)"
  for g in token twap_oracle token_mint_authority stablecoin auction_engine; do
    at="$(python3 "$LEZ_RPC" deployed "$url" "$GUESTS_DIR/$g.bin")" || exit 7
    if [[ "$at" != no ]]; then
      echo "guest $g: deployed in block $at"
    elif [[ " $LN_DEPLOY_GUESTS " == *" $g "* ]]; then
      echo "guest $g: not deployed yet (this run deploys it)"
    else
      echo "guest $g: no deployment of these exact bytes found; assumed deployed with the manifest ImageID (see docs/architecture/testnet-legacy.md)"
    fi
  done
}

# The stablecoin host is one instance per program (its ProtocolParameters PDA has no seed of ours):
# the scenario needs it uninitialised.
tn_host_free() {
  local pp state
  pp="$(cli host addresses | field "protocol parameters")"
  state="$(cli dev balance --account "$pp" | awk '$1 == "state" { print $2 }')"
  if [[ "$state" != uninitialised ]]; then
    echo "preflight: the stablecoin host on this chain is already initialised (ProtocolParameters $pp)." >&2
    echo "preflight: the scenario initialises its own host, so it can run once per chain (see docs/architecture/testnet-legacy.md)" >&2
    exit 9
  fi
  echo "stablecoin host   ProtocolParameters $pp uninitialised (free)"
}

ln_preflight
tn_preflight
if [[ "$mode" == preflight ]]; then
  if [[ -x "$CLI_BIN" ]]; then
    mkdir -p "$LN/logs"
    ln_write_deployment > /dev/null
    tn_host_free
  else
    echo "stablecoin host   not checked ($CLI_BIN not built yet; --send checks it after building)"
  fi
  echo "preflight ok (read-only; nothing was sent)"
  exit 0
fi

# --send: a new run directory under the state dir; --resume: the given one. Everything is logged.
if [[ "$mode" == send ]]; then
  mkdir -p "$root" "$LN/logs"
  touch "$root/$ROOT_MARKER" "$LN/$RUN_MARKER"
fi
mkdir -p "$LN/logs"
# One process per run directory: a --resume started while the original run is still alive would
# otherwise pass the nonce check between two transactions and send the same phase twice.
exec 9>"$LN/.lock"
if ! flock -n 9; then
  echo "testnet-legacy: run directory $LN is in use by another testnet-legacy.sh; wait until it has exited" >&2
  exit 2
fi
SUMMARY="$LN/testnet-summary.txt"
TXS="$LN/transactions.txt"
touch "$SUMMARY" "$TXS" "$LN/run.env"
# tee ignores INT and TERM (a Ctrl-C reaches the whole process group), so the failure banner written
# by the EXIT trap still reaches the console and run.log.
exec > >(trap '' INT TERM; exec tee -a "$LN/logs/run.log") 2>&1
tee_pid=$!

# shellcheck disable=SC1091
. "$(dirname "${BASH_SOURCE[0]}")/lib/demo-scenario.sh"

tn_evidence() {
  local l
  echo
  echo "=== evidence ($mode, exit $1, $(date -u +%Y-%m-%dT%H:%M:%SZ))"
  echo "last block        $(python3 "$LEZ_RPC" last-block "$url" 2>/dev/null || echo unreadable)"
  echo "deployment file   $DEP"
  cat "$DEP" 2>/dev/null; echo
  echo "accounts (label id)"
  for l in $LABELS; do echo "  $l $(id_of "$l" 2>/dev/null || echo '?')"; done
  echo "scenario ids and phases (run.env)"
  sed 's/^/  /' "$LN/run.env" 2>/dev/null || true
  echo "program deployments and auth-transfer init (wallet: transaction hash, inclusion block)"
  grep -E "^Transaction (hash is|is included in block)" "$LN/logs/run.log" | sed 's/^/  /' || true
  echo "auction-cli transactions (command | instruction | hash): $(wc -l < "$TXS")"
  sed 's/^/  /' "$TXS"
}

on_exit() {
  local s=$?
  set +e
  tn_evidence "$s" >> "$SUMMARY" 2>/dev/null
  if (( s != 0 )); then
    {
      echo "testnet-legacy FAILED (exit $s); log $LN/logs/run.log, summary $SUMMARY"
      echo "host may be claimed; admin keys are in $LN/wallet-home; resume with: scripts/wsl-env.sh scripts/testnet-legacy.sh --resume $LN; check nonces before acting"
    } | tee -a "$SUMMARY" >&2
  fi
  # Let tee write the last lines before the shell exits.
  exec 1>&- 2>&-
  wait "$tee_pid" 2>/dev/null
  exit "$s"
}
trap on_exit EXIT
# A signal ends the run through the EXIT trap (128 + signal), like any other failure.
trap 'exit 130' INT
trap 'exit 143' TERM

phase_done() { [[ "$(recall "PHASE_$1_done")" == 1 ]]; }
phase_started() { [[ "$(recall "PHASE_$1_started")" == 1 ]]; }
# The nonce of every label, in $LABELS order (one getAccountsNonces call). The ids are resolved once,
# after the accounts phase (the only phase that changes them, and it takes no snapshot).
# tn_ids fills the cache in this shell; call it before $(tn_nonces), which runs in a subshell.
TN_IDS=()
tn_ids() {
  local l
  if (( ${#TN_IDS[@]} == 0 )); then
    for l in $LABELS; do TN_IDS+=("$(id_of "$l")"); done
  fi
}
tn_nonces() { tn_ids; python3 "$LEZ_RPC" nonces "$url" "${TN_IDS[@]}"; }

# A phase interrupted part-way may be re-run only if none of its transactions applied: no in-flight
# lock (an unknown outcome) and every signer nonce as recorded when the phase started.
tn_restart_check() {
  local name="$1" before now locks i l
  case "$name" in
    accounts)
      # Sends nothing: keys made by the interrupted attempt were never used. Kept aside, not deleted.
      if [[ -e "$LEE_WALLET_HOME_DIR" ]]; then
        mv "$LEE_WALLET_HOME_DIR" "$LEE_WALLET_HOME_DIR.incomplete-$(date -u +%Y%m%dT%H%M%SZ)"
      fi
      echo "phase accounts: interrupted before any transaction; starting it again"
      return 0 ;;
    deploy)
      echo "phase deploy: interrupted; deployed guests are detected and skipped"
      return 0 ;;
  esac
  locks="$(ls "$LN/cli-state/locks"/*.lock 2>/dev/null || true)"
  if [[ -n "$locks" ]]; then
    echo "resume: phase $name has a transaction with an unknown outcome (in-flight lock: $locks)." >&2
    echo "resume: check the signer's nonce before acting (auction-cli dev balance / host status / list-auctions --all with this run's --deployment $DEP --state-dir $LN/cli-state). If the transaction landed, finish the phase by hand; if it was dropped, remove the stale lock file ($locks) and resume again." >&2
    exit 10
  fi
  before="$(recall "PHASE_${name}_nonces")"
  tn_ids
  now="$(tn_nonces)"
  if [[ -n "$before" && "$before" == "$now" ]]; then
    echo "phase $name: interrupted before any of its transactions applied (signer nonces unchanged); running it again"
    return 0
  fi
  echo "resume: phase $name was interrupted after some of its transactions applied; not re-running it (a step would be sent twice)." >&2
  read -ra b <<<"$before"; read -ra n <<<"$now"; i=0
  for l in $LABELS; do
    if [[ "${b[$i]:-?}" != "${n[$i]:-?}" ]]; then echo "resume:   $l nonce ${b[$i]:-?} -> ${n[$i]:-?}" >&2; fi
    i=$((i + 1))
  done
  echo "resume: finish phase $name by hand from $LN/logs/run.log (check each nonce before acting), then add PHASE_${name}_done=1 to $LN/run.env and resume again." >&2
  exit 10
}

# One guarded step: skipped when done; checked when interrupted; the nonce snapshot is taken first.
tn_guarded() {
  local name="$1" fn="$2"
  if phase_done "$name"; then echo "phase $name: done earlier, skipped"; return 0; fi
  if phase_started "$name"; then tn_restart_check "$name"; fi
  if [[ "$name" != accounts ]]; then tn_ids; remember "PHASE_${name}_nonces" "$(tn_nonces)"; fi
  remember "PHASE_${name}_started" 1
  "$fn"
  remember "PHASE_${name}_done" 1
}

# One keeper round (ADR-050): re-publish the current mock price when it is within
# KEEPER_REFRESH_BEFORE_MS of the collateral's maximum oracle age (1 h, demo-collateral.json). Run in
# line, before a phase: a concurrent keeper would sign with the same admins as the scenario.
tn_tick() {
  cli dev keeper --price-source oracle-src --price "$1" --host-admin admin --engine-admin engine-admin \
    --iterations 1 --interval-ms 0 --refresh-before-ms "$KEEPER_REFRESH_BEFORE_MS"
}
tn_tick_r() { tn_tick "$DEMO_PRICE_R"; }
tn_tick_moved() { tn_tick "$DEMO_PRICE_MOVED"; }

# Every phase of demo_scenario goes through here. The host check, the scan seed and the keeper rounds
# run outside the scenario phases, so an interruption in them never blocks a resume.
demo_run_phase() {
  local name="$1" title="$2" fn="$3"
  if phase_done "$name"; then echo "phase $name: done earlier, skipped"; return 0; fi
  if [[ -n "$title" ]]; then phase "$title"; fi
  case "$name" in
    host)
      if ! phase_started host; then tn_host_free | tee -a "$SUMMARY"; fi
      [[ -n "$(recall SCAN_FROM)" ]] || remember SCAN_FROM "$(python3 "$LEZ_RPC" last-block "$url")" ;;
    positions)
      # The scanner starts at our own deployment, not at block 1 of a long public chain.
      cli rescan --from "$(recall SCAN_FROM)" ;;
  esac
  case "$name" in
    positions | f1f3 | f5) tn_guarded "tick_$name" tn_tick_r ;;
    shortfall | f4 | freeze) tn_guarded "tick_$name" tn_tick_moved ;;
  esac
  tn_guarded "$name" "$fn"
}

if [[ "$mode" == send ]]; then
  phase "testnet-legacy: $url, block 1 $genesis, run directory $LN"
  tn_preflight | tee -a "$SUMMARY"
  phase "build auction-cli; committed guests (artifacts/guests, ADR-047); deployment file (network testnet-legacy)"
  ln_build
  ln_check_genesis | tee -a "$SUMMARY"
  if ! phase_started host; then tn_host_free | tee -a "$SUMMARY"; fi
else
  phase "testnet-legacy resume: $url, block 1 $genesis, run directory $LN"
  tn_preflight | tee -a "$SUMMARY"
  phase "build auction-cli (the run's deployment file is kept)"
  ln_build_cli
  ln_check_genesis | tee -a "$SUMMARY"
fi
demo_scenario
echo "testnet-legacy complete; summary $SUMMARY"
echo "back up $LN (wallet-home holds the only copy of the admin keys)"
