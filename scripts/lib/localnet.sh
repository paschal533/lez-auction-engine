#!/usr/bin/env bash
# Shared localnet helpers (Plan 3b Task 12) for scripts/localnet-smoke.sh, scripts/demo.sh and CI.
# Source it after setting LN (the run directory). Linux or WSL only. Requires the LEZ v0.2.4 sequencer
# and wallet (scripts/bootstrap-lez.sh), python3, ss, and the committed guests (artifacts/guests,
# ADR-047). Only the sequencer recorded in $LN/sequencer.pid is ever signalled. On the dev machine run the
# scripts through scripts/wsl-env.sh (shared target dir, S1 environment); ln_build refuses otherwise.
# Preflight thresholds: DEMO_MIN_C_GIB (default 3) and DEMO_MIN_HOME_GIB (default 20); CI lowers the latter.
# Network parameters (set before sourcing; the defaults are the throwaway localnet, behaviour unchanged):
#   LN_NETWORK         deployment-file network name (default localnet; auction-cli's sending dev commands
#                      accept localnet and testnet-legacy, always with the recorded genesis, ADR-046)
#   LN_SEQUENCER_URL   sequencer JSON-RPC URL (default http://127.0.0.1:$PORT)
#   LN_LOCAL_SEQUENCER 1 (default): this lib starts and stops its own sequencer; 0: a remote node (no
#                      sequencer binary, port check or sequencer log)
#   LN_GENESIS         block-1 hash ln_build records as chain_genesis_hash (default none: a local run
#                      records it in ln_start_sequencer)
#   LN_DEPLOY_GUESTS   guests ln_deploy deploys, in order (default all five)
#   LN_SKIP_DEPLOYED   1: ln_deploy skips a guest whose exact deployment transaction the node already
#                      has (scripts/lib/lez_rpc.py deployed); default 0
#   LN_RESET_WALLET    1 (default): ln_accounts deletes $LN/wallet-home first; 0: it refuses an existing one
#   LN_WALLET_LOG      when set, wallet key setup output (it prints the recovery phrase) goes to this file
#   LN_WALLET_POLL_TIMEOUT, LN_WALLET_POLL_MAX_BLOCKS: wallet inclusion polling (default: the LEZ config)
LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
REPO="$(cd "$LIB_DIR/../.." && pwd -P)"
: "${LN:?set LN (the run directory) before sourcing scripts/lib/localnet.sh}"
if [[ -z "${LEZ_DEPS:-}" ]]; then
  if [[ -x "$HOME/lez-spikes/s1/lez/target/debug/sequencer_service" ]]; then LEZ_DEPS="$HOME/lez-spikes/s1"; else LEZ_DEPS="$HOME/lez-deps"; fi
fi
LEZ="$LEZ_DEPS/lez"
SEQ_BIN="$LEZ/target/debug/sequencer_service"
W="$LEZ/target/debug/wallet"
# SEQ_CONFIG may point at a copy (e.g. a longer block_create_timeout for a rehearsal); default the LEZ file.
SEQ_CONFIG="${SEQ_CONFIG:-$LEZ/lez/sequencer/service/configs/debug/sequencer_config.json}"
WALLET_CONFIG="$LEZ/lez/wallet/configs/debug/wallet_config.json"
CLI_BIN="${CARGO_TARGET_DIR:-$REPO/target}/debug/auction-cli"
DEP="$LN/deployment.json"
PORT=3040
LN_NETWORK="${LN_NETWORK:-localnet}"
LN_SEQUENCER_URL="${LN_SEQUENCER_URL:-http://127.0.0.1:$PORT}"
LN_LOCAL_SEQUENCER="${LN_LOCAL_SEQUENCER:-1}"
LN_GENESIS="${LN_GENESIS:-}"
LN_DEPLOY_GUESTS="${LN_DEPLOY_GUESTS:-token twap_oracle token_mint_authority stablecoin auction_engine}"
LN_SKIP_DEPLOYED="${LN_SKIP_DEPLOYED:-0}"
LN_RESET_WALLET="${LN_RESET_WALLET:-1}"
LEZ_RPC="$LIB_DIR/lez_rpc.py"
ONE=1000000000000000000000000000
GUESTS_DIR="${GUESTS_DIR:-$REPO/artifacts/guests}"
LABELS="${LABELS:-admin freeze oracle-src coll-def owner owner-coll owner-stable engine-admin keeper keeper-coll caller protocol-def taker-protocol taker-stable}"
export LEE_WALLET_HOME_DIR="$LN/wallet-home"
# shellcheck disable=SC1091
if [[ -f "$LEZ_DEPS/env.sh" ]]; then . "$LEZ_DEPS/env.sh"; fi

cli() { "$CLI_BIN" --deployment "$DEP" --state-dir "$LN/cli-state" --wallet-home "$LEE_WALLET_HOME_DIR" "$@"; }
# Last field of the first `key  value` line starting with $1; reads all input (no SIGPIPE under pipefail).
field() {
  awk -v k="$1" 'index($0, k) == 1 && !done { print $NF; done = 1 }
    END { if (!done) { print "error: key \"" k "\" not found in auction-cli output" > "/dev/stderr"; exit 1 } }'
}
# Runs auction-cli, shows its output on stderr, prints the value of key $1.
run_field() { local key="$1" out; shift; out="$(cli "$@")" || { printf '%s\n' "$out" >&2; return 1; }; printf '%s\n' "$out" >&2; printf '%s\n' "$out" | field "$key"; }
id_of() { cli dev resolve --label "$1" | field id; }
remember() { printf '%s=%s\n' "$1" "$2" >> "$LN/run.env"; }
recall() { sed -n "s/^$1=//p" "$LN/run.env" | tail -1; }
unbacked() { cli config | sed -n 's/^accounting *unbacked debt \([0-9]*\),.*/\1/p'; }
balance_of() { run_field balance dev balance --account "$1"; }

set_deployment_field() {
  python3 - "$DEP" "$1" "$2" <<'PY'
import json, sys
path, key, value = sys.argv[1:]
d = json.load(open(path, encoding="utf-8"))
d[key] = value
json.dump(d, open(path, "w", encoding="utf-8"), indent=2)
PY
}

ln_preflight() {
  "$REPO/scripts/disk-guard.sh" "${DEMO_MIN_C_GIB:-3}" "${DEMO_MIN_HOME_GIB:-20}" || exit 3
  local b t missing="" bins=("$SEQ_BIN" "$W") configs=("$SEQ_CONFIG" "$WALLET_CONFIG") tools=(python3 ss awk)
  # A remote node needs only the wallet (keys, auth-transfer init, deploy-program) and its config.
  if [[ "$LN_LOCAL_SEQUENCER" != 1 ]]; then bins=("$W"); configs=("$WALLET_CONFIG"); tools=(python3 awk); fi
  for b in "${bins[@]}"; do [[ -x "$b" ]] || missing+=" $b"; done
  # A warm CI cache restores the binaries only: the sequencer and wallet configs come from the LEZ checkout.
  for b in "${configs[@]}"; do [[ -f "$b" ]] || missing+=" $b"; done
  if [[ -n "$missing" ]]; then
    echo "preflight: missing LEZ binaries:$missing; run scripts/bootstrap-lez.sh (LEZ_DEPS=$LEZ_DEPS)" >&2
    exit 4
  fi
  for t in "${tools[@]}"; do
    command -v "$t" >/dev/null || { echo "preflight: missing tool $t" >&2; exit 4; }
  done
  if [[ "$LN_LOCAL_SEQUENCER" == 1 ]]; then ln_preflight_port; fi
  "$REPO/scripts/check-guests.sh" --sha-only "$GUESTS_DIR" \
    || { echo "preflight: the guests in $GUESTS_DIR do not match their manifest" >&2; exit 6; }
}

ln_preflight_port() {
  # Capture once: `ss | grep -q` under pipefail can read a SIGPIPE (141) as "port free".
  local listeners holder
  listeners="$(ss -ltnp 2>/dev/null || true)"
  if grep -q ":$PORT " <<<"$listeners"; then
    holder="$(sed -n "/:$PORT /s/.*pid=\([0-9]*\).*/pid \1/p" <<<"$listeners" | head -1)"
    echo "preflight: port $PORT is in use (${holder:-unknown process}); stop it first (for a smoke run: scripts/localnet-smoke.sh step_stop)" >&2
    exit 5
  fi
}

ln_build() {
  # Never build into $REPO/target on /mnt/c (Plan 3b Global Constraints): the wrapper sets CARGO_TARGET_DIR.
  if [[ -d /mnt/c && -z "${CARGO_TARGET_DIR:-}" ]]; then
    echo "ln_build: CARGO_TARGET_DIR is unset under WSL; run through scripts/wsl-env.sh" >&2
    exit 1
  fi
  ln_build_cli
  ln_write_deployment
}

ln_build_cli() { (cd "$REPO" && cargo build --locked -p auction_cli); }

# The deployment file (I4): the manifest's ImageIDs, the network name, URL and recorded genesis.
ln_write_deployment() {
  local engine host tok twap
  read -r engine host tok twap < <(python3 - "$GUESTS_DIR/manifest.json" <<'PY'
import json, sys
g = json.load(open(sys.argv[1]))["guests"]
print(g["auction_engine"]["image_id"], g["stablecoin"]["image_id"], g["token"]["image_id"], g["twap_oracle"]["image_id"])
PY
)
  python3 - "$DEP" "$engine" "$host" "$tok" "$twap" "$LN_NETWORK" "$LN_SEQUENCER_URL" "$LN_GENESIS" <<'PY'
import json, sys
path, engine, host, tok, twap, network, url, genesis = sys.argv[1:]
json.dump({"network": network, "sequencer_url": url, "engine_program_id": engine,
           "host_program_id": host, "token_program_id": tok, "twap_oracle_program_id": twap,
           "engine_admin": None, "config": None, "chain_genesis_hash": genesis or None},
          open(path, "w", encoding="utf-8"), indent=2)
PY
  echo "deployment file: $DEP"
}

ln_start_sequencer() {
  rm -rf "${LN:?}/seqhome" "${LN:?}/cli-state" && mkdir -p "$LN/seqhome" "$LN/logs"
  (cd "$LEZ" && RISC0_DEV_MODE="${SEQ_RISC0_DEV_MODE:-1}" RUST_LOG=info setsid nohup "$SEQ_BIN" \
    "$SEQ_CONFIG" --home "$LN/seqhome" --listen-address 127.0.0.1 -p "$PORT" \
    > "$LN/logs/sequencer.log" 2>&1 < /dev/null &)
  rm -f "$LN/sequencer.pid"
  local pid="" i genesis=""
  for i in $(seq 1 30); do
    pid="$(ss -ltnp 2>/dev/null | sed -n "s/.*:$PORT .*pid=\([0-9]*\).*/\1/p" | head -1)"
    [[ -n "$pid" ]] && break
    sleep 2
  done
  [[ -n "$pid" ]] || { echo "sequencer did not open port $PORT within 60 s; see $LN/logs/sequencer.log" >&2; exit 1; }
  echo "$pid" > "$LN/sequencer.pid"
  # ADR-046: state-changing dev commands require the chain this deployment file was recorded on.
  for i in $(seq 1 15); do
    genesis="$(cli dev chain-id 2>/dev/null | awk '$1 == "genesis" { print $2 }')" && [[ -n "$genesis" ]] && break
    sleep 2
  done
  [[ -n "$genesis" ]] || { echo "could not read block 1 from the new sequencer" >&2; exit 1; }
  set_deployment_field chain_genesis_hash "$genesis"
}

ln_accounts() {
  # Fresh wallet for every run (duplicate labels would make id_of ambiguous). Throwaway localnet wallet.
  if [[ "$LN_RESET_WALLET" == 1 ]]; then
    rm -rf "${LN:?}/wallet-home"
  elif [[ -e "$LEE_WALLET_HOME_DIR" ]]; then
    echo "ln_accounts: $LEE_WALLET_HOME_DIR exists and LN_RESET_WALLET=0; not replacing its keys" >&2
    exit 1
  fi
  mkdir -p "$LEE_WALLET_HOME_DIR" && cp "$WALLET_CONFIG" "$LEE_WALLET_HOME_DIR/"
  ln_wallet_config "$LEE_WALLET_HOME_DIR/$(basename "$WALLET_CONFIG")"
  local l
  if [[ -n "${LN_WALLET_LOG:-}" ]]; then
    (umask 077 && : >> "$LN_WALLET_LOG")
    for l in $LABELS; do
      echo pw | "$W" account new public --label "$l" >> "$LN_WALLET_LOG" 2>&1 || {
        echo "ln_accounts: wallet account new failed for $l; last error lines of $LN_WALLET_LOG:" >&2
        ln_wallet_log_errors >&2
        exit 1
      }
    done
  else
    for l in $LABELS; do echo pw | "$W" account new public --label "$l"; done
  fi
  : > "$LN/run.env"
  # New keys mean a new engine admin and config address: forget the previous run's (ln_engine records them).
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

# Error lines of $LN_WALLET_LOG without the recovery phrase: the phrase line is indented lowercase words
# (BIP39 words include "error"), so such lines are never printed.
ln_wallet_log_errors() {
  grep -iE "error|failed|panicked" "$LN_WALLET_LOG" | grep -vE '^[[:space:]]+([a-z]+[[:space:]]+){11,}[a-z]+[[:space:]]*$'     | grep -vi "recovery phrase" | tail -5
}

# Points the copied wallet config at $LN_SEQUENCER_URL and applies the polling overrides. A no-op with
# the defaults, so the localnet wallet config stays the LEZ file byte for byte.
ln_wallet_config() {
  if [[ "$LN_SEQUENCER_URL" == "http://127.0.0.1:$PORT" && -z "${LN_WALLET_POLL_TIMEOUT:-}" \
        && -z "${LN_WALLET_POLL_MAX_BLOCKS:-}" ]]; then
    return 0
  fi
  python3 - "$1" "$LN_SEQUENCER_URL" "${LN_WALLET_POLL_TIMEOUT:-}" "${LN_WALLET_POLL_MAX_BLOCKS:-}" <<'PY'
import json, sys
path, url, timeout, max_blocks = sys.argv[1:]
c = json.load(open(path, encoding="utf-8"))
c["sequencers"] = [{"sequencer_addr": url}]
if timeout:
    c["seq_poll_timeout"] = timeout
if max_blocks:
    c["seq_tx_poll_max_blocks"] = int(max_blocks)
json.dump(c, open(path, "w", encoding="utf-8"), indent=4)
PY
}

ln_auth() {
  # Keys that sign more than one transaction must be auth-transfer-initialised before first use (ADR-018).
  local l
  for l in "$@"; do "$W" auth-transfer init --account-id "Public/$(id_of "$l")"; done
}

ln_deploy() {
  local p at
  for p in $LN_DEPLOY_GUESTS; do
    if [[ "$LN_SKIP_DEPLOYED" == 1 ]]; then
      at="$(python3 "$LEZ_RPC" deployed "$LN_SEQUENCER_URL" "$GUESTS_DIR/$p.bin")" || exit 1
      if [[ "$at" != no ]]; then echo "$p: already deployed (block $at); skipped"; continue; fi
    fi
    "$W" deploy-program "$GUESTS_DIR/$p.bin"
  done
}

# ADR-046 for a remote node: its block 1 must be $LN_GENESIS, the hash ln_build recorded.
ln_check_genesis() {
  local found
  found="$(cli dev chain-id | awk '$1 == "genesis" { print $2 }')"
  if [[ -z "$LN_GENESIS" || "$found" != "$LN_GENESIS" ]]; then
    echo "chain identity: block 1 of $LN_SEQUENCER_URL is ${found:-unreadable}, expected ${LN_GENESIS:-<unset>}" >&2
    exit 1
  fi
  echo "chain identity: block 1 $found"
}

ln_host() {
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

ln_engine() {
  local params="$1" mint config admin
  mint="$(run_field "mint authority" admin addresses --admin engine-admin --salt 1)"
  config="$(run_field config admin addresses --admin engine-admin --salt 1)"
  admin="$(id_of engine-admin)"
  # ADR-038: the protocol token must exist (mint authority = this config's MINT_AUTH PDA) before the config names it.
  cli dev token-new --definition protocol-def --holding taker-protocol --name MOCK --supply 10000 --mint-authority "$mint"
  cli admin init-config --admin engine-admin --salt 1 --params "$params" --protocol-token protocol-def
  set_deployment_field config "$config"
  set_deployment_field engine_admin "$admin"
}

ln_bind() { cli host bind --admin admin; }
ln_collateral() { cli admin set-collateral-config --admin engine-admin --params "$1"; }

ln_holdings() {
  local sdef
  sdef="$(recall SDEF)"
  cli dev token-init-account --definition "$sdef" --account owner-stable
  cli dev token-init-account --definition coll-def --account keeper-coll
  cli dev token-init-account --definition "$sdef" --account taker-stable
}

ln_check_log() {
  [[ -f "$LN/logs/sequencer.log" ]] || { echo "sequencer log $LN/logs/sequencer.log not found" >&2; exit 1; }
  if grep -qE "failed execution check" "$LN/logs/sequencer.log"; then
    grep -E "failed execution check" "$LN/logs/sequencer.log" | tail -20
    echo "the sequencer log contains failed execution checks" >&2
    exit 1
  fi
  echo "no failed execution check"
}

ln_peak() {
  local pid
  pid="$(cat "$LN/sequencer.pid")"
  echo "sequencer peak RSS: $(sed -n 's/^VmHWM:[[:space:]]*//p' "/proc/$pid/status") (RISC0_DEV_MODE=${SEQ_RISC0_DEV_MODE:-1})"
}

ln_stop_sequencer() {
  local pid i
  pid="$(cat "$LN/sequencer.pid" 2>/dev/null || true)"
  if [[ ! "$pid" =~ ^[0-9]+$ ]]; then
    echo "no sequencer pid recorded in $LN/sequencer.pid; nothing stopped (check manually, do not pkill by name)" >&2
    return 1
  fi
  if ! tr '\000' ' ' < "/proc/$pid/cmdline" 2>/dev/null | grep -q "sequencer_service"; then
    echo "pid $pid is not a running sequencer_service; not signalling it" >&2
  else
    kill "$pid" 2>/dev/null || echo "pid $pid was not running" >&2
    for i in $(seq 1 30); do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
    if kill -0 "$pid" 2>/dev/null; then echo "sequencer pid $pid did not exit within 30 s" >&2; return 1; fi
  fi
  rm -f "$LN/sequencer.pid"
}
