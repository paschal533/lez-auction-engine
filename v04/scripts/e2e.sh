#!/usr/bin/env bash
# The liquidation slice end to end on a standalone LEZ v0.4.0-rc1 sequencer (spikes S8, S9):
#   1. build (or reuse) LEZ's sequencer_service (standalone) and wallet at v0.4.0-rc1,
#   2. build the v04 guests and the SDK example `slice`,
#   3. set up a fresh sequencer home from lez/sequencer/service/networks/local.json and start it,
#   4. import the public dev key A (genesis-funded) into a fresh wallet: the fee payer,
#   5. deploy token, oracle, mock_host, relay and engine with `wallet program-loader deploy`,
#   6. run `cargo run -p sdk --example slice` against it (expects "slice complete"),
#   7. stop the sequencer by its exact process name.
# `e2e.sh` (or `e2e.sh all`) runs every step; `up`, `deploy`, `slice`, `down` run them in stages.
# Exits non-zero on any failed step. Local only (no CI): it needs a few GB of LEZ build and
# about 15 minutes of 15 s blocks.
#
# Environment (all optional):
#   LEZ_DIR        LEZ checkout at v0.4.0-rc1      (default ~/lez-spikes/lez-v04, cloned if absent)
#   LEZ_TARGET     cargo target dir for LEZ        (default ~/lez-target/v04)
#   CARGO_TARGET_DIR  target dir for v04           (default ~/lez-target/v04-port)
#   E2E_HOME       sequencer home + wallet, wiped  (default ~/lez-e2e/v04)
#   ENV_SH         sourced first (libclang, libpcsclite sysroot; default ~/lez-spikes/s1/env.sh if present)
#   RUN_TAG, SLACK_MS  passed to the example
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
V04="$(cd "$HERE/.." && pwd)"
ENV_SH="${ENV_SH:-$HOME/lez-spikes/s1/env.sh}"
# shellcheck disable=SC1090
[[ -f "$ENV_SH" ]] && source "$ENV_SH"

LEZ_TAG="v0.4.0-rc1"
LEZ_DIR="${LEZ_DIR:-$HOME/lez-spikes/lez-v04}"
LEZ_TARGET="${LEZ_TARGET:-$HOME/lez-target/v04}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/lez-target/v04-port}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
E2E_HOME="${E2E_HOME:-$HOME/lez-e2e/v04}"
BIN="$LEZ_TARGET/release"
GUESTS="$CARGO_TARGET_DIR/riscv-guest/methods/v04-guests/riscv32im-risc0-zkvm-elf/release"
RPC_PORT=3040
export SEQUENCER_URL="http://127.0.0.1:$RPC_PORT"
# Funding key used by S8's setup; standalone mode never posts to Bedrock, any valid key works.
FUNDING_KEY="2e03b2eff5a45478e7e79668d2a146cf2c5c7925bce927f2b1c67f2ab4fc0d26"
# lez/testnet_initial_state PRIVATE_KEY_PUB_ACC_A, funded by local.json's genesis (S8).
DEV_KEY_A="10a26a9aec7d34b82364eeae45c5294dbb0a764b000b94eeb9b58511dc487c4d"
DEV_ACCOUNT_A="Public/6iArKUXxhUJqS7kCaPNhwMWt3ro71PDyBj7jwAyE2VQV"
export LEE_WALLET_HOME_DIR="$E2E_HOME/wallet"
export LEE_WALLET_PASSWORD=""

step() { echo; echo "== $*"; }
die() { echo "e2e: $*" >&2; exit 1; }
wallet() { "$BIN/wallet" "$@"; }

build_lez() {
  if [[ -x "$BIN/sequencer_service" && -x "$BIN/wallet" ]]; then
    echo "reusing $BIN/{sequencer_service,wallet}"; return
  fi
  if [[ ! -d "$LEZ_DIR" ]]; then
    git clone --depth 1 --branch "$LEZ_TAG" \
      https://github.com/logos-blockchain/logos-execution-zone.git "$LEZ_DIR"
  fi
  [[ "$(git -C "$LEZ_DIR" describe --tags)" == "$LEZ_TAG" ]] || die "$LEZ_DIR is not at $LEZ_TAG"
  ( cd "$LEZ_DIR" && CARGO_TARGET_DIR="$LEZ_TARGET" RISC0_SKIP_BUILD=1 \
      cargo build --release --features standalone -p sequencer_service \
    && CARGO_TARGET_DIR="$LEZ_TARGET" RISC0_SKIP_BUILD=1 cargo build --release -p wallet )
}

build_v04() {
  ( cd "$V04" && cargo build -p methods && cargo build -p sdk --example slice )
  for g in token oracle mock_host relay engine; do
    [[ -f "$GUESTS/$g.bin" ]] || die "missing guest $GUESTS/$g.bin"
  done
}

stop_sequencer() {
  # Exact process name (comm is truncated to 15 chars). Never `pkill -f`: it matches this shell.
  pkill -x sequencer_servi 2>/dev/null || true
}

start_sequencer() {
  if pgrep -x sequencer_servi >/dev/null; then
    die "a sequencer is already running; stop it first (pkill -x sequencer_servi)"
  fi
  rm -rf "$E2E_HOME"; mkdir -p "$E2E_HOME"
  "$BIN/sequencer_service" setup "$E2E_HOME/seq_home" \
    --config "$LEZ_DIR/lez/sequencer/service/networks/local.json" \
    --funding-public-key "$FUNDING_KEY" --create-channel
  trap stop_sequencer EXIT
  ( cd "$E2E_HOME" && RUST_LOG=info,risc0_zkvm=warn,kameo=warn \
      setsid nohup "$BIN/sequencer_service" start seq_home >> sequencer.log 2>&1 < /dev/null & )
  for _ in $(seq 1 60); do
    if ss -ltn | grep -q ":$RPC_PORT "; then echo "sequencer up ($E2E_HOME/sequencer.log)"; return; fi
    sleep 2
  done
  die "the sequencer did not come up; see $E2E_HOME/sequencer.log"
}

new_public() { wallet account new public 2>&1 | grep -oE 'Public/[1-9A-HJ-NP-Za-km-z]+' | head -1; }

setup_wallet() {
  mkdir -p "$LEE_WALLET_HOME_DIR"
  cp "$LEZ_DIR/lez/wallet/configs/debug/wallet_config.json" "$LEE_WALLET_HOME_DIR/"
  wallet check-health
  wallet account import public --private-key "$DEV_KEY_A" | tail -2
  wallet account list 2>&1 | grep -q "$DEV_ACCOUNT_A" || die "dev key A did not import"
  export PAYER="$DEV_ACCOUNT_A"
  echo "payer $PAYER"
}

# deploy <guest>: prints the program account (the loader's header account).
deploy() {
  local elf="$GUESTS/$1.bin" n header segs=()
  # The loader chunks the ELF inside the .bin, so ask it (S8): a dry run with one segment fails
  # with "splits into N segment(s)" unless N is 1.
  n=$(wallet program-loader deploy --elf "$elf" --header "$(new_public)" --segments "$(new_public)" \
        --payer "$PAYER" --dry-run 2>&1 | grep -oE 'splits into [0-9]+' | grep -oE '[0-9]+' | head -1 || true)
  n=${n:-1}
  header=$(new_public)
  for _ in $(seq 1 "$n"); do segs+=("$(new_public)"); done
  echo "deploy $1: $(stat -c %s "$elf") bytes, $n segment(s), program $header" >&2
  wallet program-loader deploy --elf "$elf" --header "$header" --segments "${segs[@]}" \
    --payer "$PAYER" >&2 || die "deploying $1 failed"
  echo "${header#Public/}"
}

deploy_all() {
  TOKEN_PROGRAM=$(deploy token); ORACLE_PROGRAM=$(deploy oracle)
  HOST_PROGRAM=$(deploy mock_host); RELAY_PROGRAM=$(deploy relay)
  ENGINE_PROGRAM=$(deploy engine)
  export TOKEN_PROGRAM ORACLE_PROGRAM HOST_PROGRAM RELAY_PROGRAM ENGINE_PROGRAM
  printf 'TOKEN_PROGRAM=%s\nORACLE_PROGRAM=%s\nHOST_PROGRAM=%s\nRELAY_PROGRAM=%s\nENGINE_PROGRAM=%s\nPAYER=%s\n' \
    "$TOKEN_PROGRAM" "$ORACLE_PROGRAM" "$HOST_PROGRAM" "$RELAY_PROGRAM" "$ENGINE_PROGRAM" "$PAYER" \
    | tee "$E2E_HOME/programs.env"
}

run_slice() {
  local log="$E2E_HOME/slice.log"
  ( cd "$V04" && cargo run -q -p sdk --example slice ) 2>&1 | tee "$log"
  [[ ${PIPESTATUS[0]} -eq 0 ]] || die "the slice example failed (log $log)"
  grep -q '^slice complete$' "$log" || die "no 'slice complete' in $log"
}

load_env() {
  [[ -f "$E2E_HOME/programs.env" ]] || die "no $E2E_HOME/programs.env: run '$0 up' and '$0 deploy' first"
  set -a; source "$E2E_HOME/programs.env"; set +a
}

up() {
  step "LEZ $LEZ_TAG binaries"; build_lez
  step "v04 guests and the slice example"; build_v04
  step "standalone sequencer"; start_sequencer
  step "wallet and payer"; setup_wallet
}

case "${1:-all}" in
  all)
    up
    step "deploy the guests"; deploy_all
    step "the slice"; run_slice
    step "stop the sequencer"; stop_sequencer; trap - EXIT
    echo "e2e: slice complete" ;;
  # Stages, for a session that cannot hold one long command: the sequencer keeps running between
  # them (no EXIT trap); `down` stops it.
  up) up; trap - EXIT; printf 'PAYER=%s\n' "$PAYER" > "$E2E_HOME/programs.env" ;;
  deploy) load_env; step "deploy the guests"; deploy_all ;;
  slice) load_env; step "the slice"; run_slice; echo "e2e: slice complete" ;;
  down) step "stop the sequencer"; stop_sequencer ;;
  *) echo "usage: $0 [all|up|deploy|slice|down]" >&2; exit 2 ;;
esac
