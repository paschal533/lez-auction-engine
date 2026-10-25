#!/usr/bin/env bash
# Runs the Auctions app in Basecamp's standalone mode against a localnet (Plan 4 Task 6).
#   scripts/basecamp/run-app.sh [--setup] [--test FILE]
# --setup      scripts/demo.sh --setup-only --sequencer-dev-mode 1 first (fresh localnet, kept running)
# --test FILE  node FILE against the inspector (localhost:3768); exits with its code and stops what this
#              run started (Basecamp, and the sequencer with --setup)
# Without --setup it uses the deployment and wallet of the last setup run (AUCTIONS_DEPLOYMENT and
# LEE_WALLET_HOME_DIR override). Without --test Basecamp keeps running; the script prints how to stop it.
# Overrides: AUCTION_ENGINE_ROOT, AUCTIONS_ROOT (install roots; default: nix build of modules/auction_engine
# and apps/auctions #install), LOGOS_QT_MCP, NODE, DEMO_RUN_DIR (default ~/lez-target/demo).
# Runs inside WSL; from Git Bash it re-runs itself through wsl.
set -euo pipefail

BASECAMP_REV="e09962f5998792afd3bfed8c69c3b8f07f5825a5"
INSPECTOR_PORT=3768
INSPECTOR_WAIT_S=90

if ! grep -qi microsoft /proc/version 2>/dev/null && [[ ! -d /mnt/c ]] && command -v wsl >/dev/null 2>&1; then
  here="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
  win="$(cd "$here" && pwd -W)"
  wsl_here="/mnt/$(echo "${win:0:1}" | tr 'A-Z' 'a-z')${win:2}"
  MSYS_NO_PATHCONV=1 exec wsl -d Ubuntu-20.04 -e bash -lc 'cd "$1" && shift && exec scripts/basecamp/run-app.sh "$@"' _ "$wsl_here" "$@"
fi

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
# The author's machine: the shared cargo target dir and the S1 environment (libpcsclite for the wallet).
if [[ -z "${CARGO_TARGET_DIR:-}" && -f "$HOME/lez-spikes/s1/env.sh" ]]; then
  exec "$repo/scripts/wsl-env.sh" "$repo/scripts/basecamp/run-app.sh" "$@"
fi

usage() { echo "usage: scripts/basecamp/run-app.sh [--setup] [--test FILE]" >&2; exit 2; }
setup=0; test_file=""
while (( $# )); do
  case "$1" in
    --setup) setup=1 ;;
    --test) test_file="${2:-}"; [[ -n "$test_file" ]] || usage; shift ;;
    *) usage ;;
  esac
  shift
done
if [[ -n "$test_file" ]]; then test_file="$(cd "$(dirname "$test_file")" && pwd -P)/$(basename "$test_file")"; fi

LN="${DEMO_RUN_DIR:-$HOME/lez-target/demo}"
out="$HOME/lez-target/basecamp"
mkdir -p "$out"

# Builds one flake output to a result link under ~/lez-target/basecamp and prints the store path.
build() { # <flake ref> <link name>
  nix build "$1" --out-link "$out/$2" >&2
  readlink -f "$out/$2"
}

eval "$("$repo/scripts/basecamp/host.sh" paths)"
[[ -x "$BASECAMP_BIN" && -d "$LEZ_CORE_ROOT/modules" ]] || {
  echo "run-app: Basecamp or lez_core missing; run scripts/basecamp/host.sh build" >&2; exit 1; }
"$repo/scripts/basecamp/host.sh" check
AUCTION_ENGINE_ROOT="${AUCTION_ENGINE_ROOT:-$(build "$repo/modules/auction_engine#install" auction_engine)}"
AUCTIONS_ROOT="${AUCTIONS_ROOT:-$(build "$repo/apps/auctions#install" auctions)}"
"$repo/scripts/basecamp/host.sh" check

seq_started=0
basecamp_pid=""
cleanup() {
  local s=$?
  if [[ -n "$test_file" ]]; then
    if [[ -n "$basecamp_pid" ]] && kill -0 "$basecamp_pid" 2>/dev/null; then
      kill "$basecamp_pid" 2>/dev/null || true
      for _ in $(seq 1 20); do kill -0 "$basecamp_pid" 2>/dev/null || break; sleep 0.5; done
      kill -9 "$basecamp_pid" 2>/dev/null || true
      echo "run-app: stopped Basecamp (pid $basecamp_pid)"
    fi
    if (( seq_started )); then
      # shellcheck disable=SC1091
      (. "$repo/scripts/lib/localnet.sh" && ln_stop_sequencer) && echo "run-app: stopped the sequencer"
    fi
  fi
  exit "$s"
}
trap cleanup EXIT

if (( setup )); then
  setup_log="$(mktemp)"
  # Not a pipe: the subshell that launches the sequencer outlives demo.sh and would hold a pipe open.
  DEMO_RUN_DIR="$LN" "$repo/scripts/demo.sh" --setup-only --sequencer-dev-mode 1 > "$setup_log" 2>&1 &
  demo_pid=$!
  tail -n +1 -f --pid="$demo_pid" "$setup_log" &
  tail_pid=$!
  demo_status=0
  wait "$demo_pid" || demo_status=$?
  wait "$tail_pid" || true
  (( demo_status == 0 )) || { echo "run-app: demo.sh --setup-only failed (exit $demo_status)" >&2; exit "$demo_status"; }
  seq_started=1
  env_line="$(grep -E '^AUCTIONS_DEPLOYMENT=' "$setup_log" | tail -1)"
  rm -f "$setup_log"
  [[ -n "$env_line" ]] || { echo "run-app: demo.sh --setup-only printed no AUCTIONS_DEPLOYMENT line" >&2; exit 1; }
  read -r dep_kv wallet_kv <<<"$env_line"
  export AUCTIONS_DEPLOYMENT="${dep_kv#AUCTIONS_DEPLOYMENT=}" LEE_WALLET_HOME_DIR="${wallet_kv#LEE_WALLET_HOME_DIR=}"
fi
export AUCTIONS_DEPLOYMENT="${AUCTIONS_DEPLOYMENT:-$LN/deployment.json}"
export LEE_WALLET_HOME_DIR="${LEE_WALLET_HOME_DIR:-$LN/wallet-home}"
# Same in-flight locks as auction-cli on this run (scripts/lib/localnet.sh `cli`).
export AUCTIONS_STATE_DIR="${AUCTIONS_STATE_DIR:-$(dirname "$AUCTIONS_DEPLOYMENT")/cli-state}"
[[ -f "$AUCTIONS_DEPLOYMENT" && -d "$LEE_WALLET_HOME_DIR" ]] || {
  echo "run-app: no deployment at $AUCTIONS_DEPLOYMENT or wallet at $LEE_WALLET_HOME_DIR; run with --setup" >&2; exit 1; }
mkdir -p "$AUCTIONS_STATE_DIR"

if ss -ltn 2>/dev/null | grep -q ":$INSPECTOR_PORT "; then
  echo "run-app: port $INSPECTOR_PORT is in use (another Basecamp?); stop it first" >&2; exit 1
fi
user_dir="$(mktemp -d)"
log_dir="$(dirname "$AUCTIONS_DEPLOYMENT")/logs"
mkdir -p "$log_dir"
XDG_RUNTIME_DIR=/mnt/wslg/runtime-dir setsid nohup "$BASECAMP_BIN" --user-dir "$user_dir" \
  --modules-dir "$LEZ_CORE_ROOT" --modules-dir "$AUCTION_ENGINE_ROOT" --module "$AUCTIONS_ROOT" \
  > "$log_dir/basecamp.log" 2>&1 < /dev/null &
basecamp_pid=$!
echo "run-app: Basecamp pid $basecamp_pid, log $log_dir/basecamp.log"
for _ in $(seq 1 "$INSPECTOR_WAIT_S"); do
  kill -0 "$basecamp_pid" 2>/dev/null || { echo "run-app: Basecamp exited; see $log_dir/basecamp.log" >&2; exit 1; }
  ss -ltn 2>/dev/null | grep -q ":$INSPECTOR_PORT " && break
  sleep 1
done
ss -ltn 2>/dev/null | grep -q ":$INSPECTOR_PORT " || {
  echo "run-app: no inspector on port $INSPECTOR_PORT after ${INSPECTOR_WAIT_S} s" >&2; exit 1; }

if [[ -z "$test_file" ]]; then
  echo "run-app: running; stop Basecamp with: kill $basecamp_pid"
  if [[ -f "$LN/sequencer.pid" ]]; then echo "run-app: the sequencer (pid $(cat "$LN/sequencer.pid")) keeps running"; fi
  exit 0
fi

export LOGOS_QT_MCP="${LOGOS_QT_MCP:-$(build "github:logos-co/logos-basecamp/${BASECAMP_REV}#logos-qt-mcp" qt-mcp)}"
export AUCTION_CLI="${AUCTION_CLI:-${CARGO_TARGET_DIR:-$repo/target}/debug/auction-cli}"
if [[ -z "${NODE:-}" ]]; then
  # Node from the nixpkgs Basecamp pins (shares its store paths) when none is on PATH.
  NODE="$(command -v node || true)"
  if [[ -z "$NODE" ]]; then
    nix build --inputs-from "github:logos-co/logos-basecamp/${BASECAMP_REV}" nixpkgs#nodejs --out-link "$out/node" >&2
    NODE="$(readlink -f "$out/node")/bin/node"
  fi
fi
set +e
QML_INSPECTOR_PORT="$INSPECTOR_PORT" "$NODE" "$test_file"
status=$?
set -e
echo "run-app: $(basename "$test_file") exited $status"
exit "$status"
