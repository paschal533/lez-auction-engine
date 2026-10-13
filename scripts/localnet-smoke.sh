#!/usr/bin/env bash
# Plan 3a Task 16 localnet smoke: the Plan 2 runbook with auction-cli for every program instruction (no spel).
# Run INSIDE WSL. Steps, in order (several may be given in one call):
#   step_build step_sequencer step_accounts step_auth step_deploy step_host step_engine step_bind
#   step_collateral step_holdings step_flow step_system step_check step_peak step_stop
# Requires the S1 builds (sequencer_service, wallet) and S1's Docker-built upstream guests (token, twap_oracle,
# token_mint_authority). The wallet binary still does wallet business: key creation, auth-transfer init and
# program deployment. Only the sequencer started by step_sequencer (pid file) is ever signalled.
# SEQ_RISC0_DEV_MODE=0 runs the sequencer with dev mode off (RFP S2).
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
S1="$HOME/lez-spikes/s1"
LN="$HOME/lez-target/plan3a-localnet"
W="$S1/lez/target/debug/wallet"
UP="$S1/lez-programs/target/guest"
CLI_BIN="${CARGO_TARGET_DIR:-$HOME/lez-target/plan2}/debug/auction-cli"
export LEE_WALLET_HOME_DIR="$LN/wallet-home"
# shellcheck disable=SC1091
. "$S1/env.sh"
mkdir -p "$LN/logs" "$LN/guest"
# Fixed by S1 (same upstream binaries -> same ids):
TOK=14b3bc6cc129d0359f9595ba0f60a13e4a8df08ffe8eb1a83251c47abecc67a8
TWAP=5681e27c27b3ece4a7c161bbf8e12e6ece6ca69925a87b77f9e193fa4a249b31
ONE=1000000000000000000000000000
DEP="$LN/deployment.json"
LABELS="admin freeze oracle-src coll-def owner owner-coll owner-stable engine-admin keeper keeper-coll caller protocol-def taker-protocol taker-stable"

cli() { "$CLI_BIN" --deployment "$DEP" --state-dir "$LN/cli-state" --wallet-home "$LEE_WALLET_HOME_DIR" "$@"; }
# Last field of the first `key  value` line starting with $1; reads all input (no SIGPIPE under pipefail).
field() {
  awk -v k="$1" 'index($0, k) == 1 && !done { print $NF; done = 1 }
    END { if (!done) { print "error: key \"" k "\" not found in auction-cli output" > "/dev/stderr"; exit 1 } }'
}
# Runs auction-cli, shows its output on stderr, prints the value of key $1. (Not `tee /dev/stderr`: reopening
# /dev/stderr truncates it when stderr is redirected to a log file.)
run_field() { local key="$1" out; shift; out="$(cli "$@")" || { printf '%s\n' "$out" >&2; return 1; }; printf '%s\n' "$out" >&2; printf '%s\n' "$out" | field "$key"; }
id_of() { cli dev resolve --label "$1" | field id; }
remember() { printf '%s=%s\n' "$1" "$2" >> "$LN/run.env"; }
recall() { sed -n "s/^$1=//p" "$LN/run.env" | tail -1; }

set_deployment_field() {
  python3 - "$DEP" "$1" "$2" <<'PY'
import json, sys
path, key, value = sys.argv[1:]
d = json.load(open(path, encoding="utf-8"))
d[key] = value
json.dump(d, open(path, "w", encoding="utf-8"), indent=2)
PY
}

step_build() {
  (cd "$REPO" && scripts/wsl-cargo.sh build -p auction_cli)
  (cd "$REPO" && scripts/wsl-cargo.sh run -q -p engine_integration_tests --example localnet_tool -- export-guests "$LN/guest") \
    | tee "$LN/guest/ids.txt"
  local engine host
  engine="$(awk '/^AUCTION_ENGINE_ID/ { print $2 }' "$LN/guest/ids.txt")"
  host="$(awk '/^STABLECOIN_ID/ { print $2 }' "$LN/guest/ids.txt")"
  python3 - "$DEP" "$engine" "$host" "$TOK" "$TWAP" <<'PY'
import json, sys
path, engine, host, tok, twap = sys.argv[1:]
json.dump({"network": "localnet", "sequencer_url": "http://127.0.0.1:3040", "engine_program_id": engine,
           "host_program_id": host, "token_program_id": tok, "twap_oracle_program_id": twap,
           "engine_admin": None, "config": None, "chain_genesis_hash": None}, open(path, "w", encoding="utf-8"), indent=2)
PY
  echo "deployment file: $DEP"
}

step_sequencer() {
  if (ss -ltn 2>/dev/null || true) | grep -q ':3040 '; then
    echo "port 3040 is already in use; not starting a second sequencer" >&2
    exit 1
  fi
  rm -rf "${LN:?}/seqhome" "${LN:?}/cli-state" && mkdir -p "$LN/seqhome"
  (cd "$S1/lez" && RISC0_DEV_MODE="${SEQ_RISC0_DEV_MODE:-1}" RUST_LOG=info setsid nohup target/debug/sequencer_service \
    lez/sequencer/service/configs/debug/sequencer_config.json --home "$LN/seqhome" --listen-address 127.0.0.1 -p 3040 \
    > "$LN/logs/sequencer.log" 2>&1 < /dev/null &)
  rm -f "$LN/sequencer.pid"
  local pid="" i
  for i in $(seq 1 30); do
    pid="$(ss -ltnp 2>/dev/null | sed -n 's/.*:3040 .*pid=\([0-9]*\).*/\1/p' | head -1)"
    [[ -n "$pid" ]] && break
    sleep 2
  done
  if [[ -z "$pid" ]]; then
    echo "sequencer did not open port 3040 within 60 s; see $LN/logs/sequencer.log" >&2
    exit 1
  fi
  echo "$pid" > "$LN/sequencer.pid"
  # ADR-046: the state-changing dev commands require the chain this deployment file was recorded on.
  local genesis=""
  for i in $(seq 1 15); do
    genesis="$("$CLI_BIN" --deployment "$DEP" dev chain-id 2>/dev/null | awk '$1 == "genesis" { print $2 }')" && [[ -n "$genesis" ]] && break
    sleep 2
  done
  [[ -n "$genesis" ]] || { echo "could not read block 1 from the new sequencer" >&2; exit 1; }
  set_deployment_field chain_genesis_hash "$genesis"
}

step_accounts() {
  # Fresh wallet for every full run (duplicate labels would make id_of ambiguous). Throwaway localnet wallet only.
  rm -rf "${LN:?}/wallet-home"
  mkdir -p "$LEE_WALLET_HOME_DIR" && cp "$S1/lez/lez/wallet/configs/debug/wallet_config.json" "$LEE_WALLET_HOME_DIR/"
  local l
  for l in $LABELS; do echo pw | "$W" account new public --label "$l"; done
  : > "$LN/run.env"
  # New keys mean a new engine admin and config address: forget the previous run's (step_engine records them).
  if [[ -f "$DEP" ]]; then
    python3 - "$DEP" <<'PY'
import json, sys
d = json.load(open(sys.argv[1], encoding="utf-8"))
d["engine_admin"] = None
d["config"] = None
json.dump(d, open(sys.argv[1], "w", encoding="utf-8"), indent=2)
PY
  fi
}

step_auth() {
  # Keys that sign more than one transaction must be auth-transfer-initialised before first use (ADR-018).
  local l
  for l in admin engine-admin keeper owner caller; do "$W" auth-transfer init --account-id "Public/$(id_of "$l")"; done
}

step_deploy() {
  local p
  for p in token twap_oracle token_mint_authority; do "$W" deploy-program "$UP/$p.bin"; done
  "$W" deploy-program "$LN/guest/stablecoin.bin"
  "$W" deploy-program "$LN/guest/auction_engine.bin"
}

step_host() {
  local sdef oracle
  cli dev token-new --definition coll-def --holding owner-coll --name COLL --supply 100000000000
  sdef="$(run_field "stablecoin definition" host addresses)"
  oracle="$(run_field oracle dev oracle-create --price-source oracle-src --base "$sdef" --quote coll-def \
    --price "$ONE" --window-ms 3600000)"
  remember SDEF "$sdef"
  remember ORACLE "$oracle"
  cli host init-program --admin admin --freeze freeze --collateral-def coll-def --oracle "$oracle" \
    --min-cr 1500000000000000000000000000 --redemption-price "$ONE"
}

step_engine() {
  local mint config admin
  mint="$(run_field "mint authority" admin addresses --admin engine-admin --salt 1)"
  config="$(run_field config admin addresses --admin engine-admin --salt 1)"
  admin="$(id_of engine-admin)"
  # ADR-038: the protocol token must exist (mint authority = this config's MINT_AUTH PDA) before the config names it.
  cli dev token-new --definition protocol-def --holding taker-protocol --name MOCK --supply 10000 --mint-authority "$mint"
  cli admin init-config --admin engine-admin --salt 1 --params "$REPO/cli/params/localnet-engine.json" --protocol-token protocol-def
  set_deployment_field config "$config"
  set_deployment_field engine_admin "$admin"
}

step_bind() {
  # Bind-time verification (ADR-027) is part of the command: it refuses (exit 2) unless the config verifies.
  cli host bind --admin admin
}

step_collateral() {
  cli admin set-collateral-config --admin engine-admin --params "$REPO/cli/params/localnet-collateral.json"
}

step_holdings() {
  local sdef
  sdef="$(recall SDEF)"
  cli dev token-init-account --definition "$sdef" --account owner-stable
  cli dev token-init-account --definition coll-def --account keeper-coll
  cli dev token-init-account --definition "$sdef" --account taker-stable
}

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
  if [[ ! -f "$LN/logs/sequencer.log" ]]; then
    echo "sequencer log $LN/logs/sequencer.log not found; cannot check for failed transactions" >&2
    exit 1
  fi
  if grep -qE "failed execution check" "$LN/logs/sequencer.log"; then
    grep -E "failed execution check" "$LN/logs/sequencer.log" | tail -20
    echo "step_check: the sequencer log contains failed execution checks" >&2
    exit 1
  fi
  echo "no failed execution check"
}

step_peak() {
  # Peak RSS of the running sequencer (VmHWM), for the ADR-045 measurement.
  local pid
  pid="$(cat "$LN/sequencer.pid")"
  echo "sequencer peak RSS: $(sed -n 's/^VmHWM:[[:space:]]*//p' "/proc/$pid/status") (RISC0_DEV_MODE=${SEQ_RISC0_DEV_MODE:-1})"
}

step_stop() {
  local pid
  pid="$(cat "$LN/sequencer.pid" 2>/dev/null || true)"
  if [[ ! "$pid" =~ ^[0-9]+$ ]]; then
    echo "no sequencer pid recorded in $LN/sequencer.pid; nothing stopped (check manually, do not pkill by name)" >&2
    exit 1
  fi
  # Only signal the process if it is still our sequencer (the pid may have been reused).
  if ! tr '\000' ' ' < "/proc/$pid/cmdline" 2>/dev/null | grep -q "sequencer_service"; then
    echo "pid $pid is not a running sequencer_service; not signalling it" >&2
  else
    kill "$pid" 2>/dev/null || echo "pid $pid was not running" >&2
    # Wait for it to exit so a following step_sequencer finds port 3040 free.
    local i
    for i in $(seq 1 30); do
      kill -0 "$pid" 2>/dev/null || break
      sleep 1
    done
    if kill -0 "$pid" 2>/dev/null; then
      echo "sequencer pid $pid did not exit within 30 s" >&2
      exit 1
    fi
  fi
  rm -f "$LN/sequencer.pid"
}

for step in "$@"; do
  case "$step" in
    step_*) "$step" ;;
    *) echo "unknown step: $step" >&2; exit 2 ;;
  esac
done
