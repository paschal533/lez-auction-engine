#!/usr/bin/env bash
# LEZ v0.4.0-rc1 spike: against the running standalone sequencer (127.0.0.1:3040), import the public dev key A,
# deploy hello_world, create a fresh public account and send one fee-paying public transaction to it.
#   v04-spike.sh rpc <method> [json-params]   one JSON-RPC call
#   v04-spike.sh run                          the whole flow
set -euo pipefail
source "$HOME/lez-spikes/s1/env.sh"
BIN="$HOME/lez-target/v04/release"
LEZ="$HOME/lez-spikes/lez-v04"
GUESTS="$HOME/lez-target/v04/riscv-guest/example_program_deployment_methods/example_program_deployment_programs/riscv32im-risc0-zkvm-elf/release"
export LEE_WALLET_HOME_DIR="$HOME/lez-spikes/v04-run/wallet"
export LEE_WALLET_PASSWORD=""
# Public test key from lez/testnet_initial_state (PRIVATE_KEY_PUB_ACC_A), funded by networks/local.json genesis.
DEV_KEY_A=$(python3 -c 'print(bytes([16,162,106,154,236,125,52,184,35,100,238,174,69,197,41,77,187,10,118,75,0,11,148,238,185,181,133,17,220,72,124,77]).hex())')

rpc() {
  local params="${2:-[]}"
  curl -s -m 10 -X POST -H 'Content-Type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$1\",\"params\":$params}" http://127.0.0.1:3040
  echo
}

wallet() { "$BIN/wallet" "$@"; }

step() { echo; echo "== $*"; }

run() {
  mkdir -p "$LEE_WALLET_HOME_DIR"
  [[ -f "$LEE_WALLET_HOME_DIR/wallet_config.json" ]] || cp "$LEZ/lez/wallet/configs/debug/wallet_config.json" "$LEE_WALLET_HOME_DIR/"
  step "sequencer"; rpc getLastBlockId; rpc getFeeState
  step "wallet health"; wallet check-health 2>&1 | tail -3
  step "import dev key A"; wallet account import public --private-key "$DEV_KEY_A" 2>&1 | tail -2
  step "accounts"; wallet account list 2>&1 | head -10
  step "deploy hello_world ($(stat -c %s "$GUESTS/hello_world.bin") bytes)"
  ( cd "$LEZ/examples/program_deployment" && time wallet deploy-program "$GUESTS/hello_world.bin" ) 2>&1 | tail -8
  step "new public account"; wallet account new public 2>&1 | tail -2
}

PAYER="Public/6iArKUXxhUJqS7kCaPNhwMWt3ro71PDyBj7jwAyE2VQV"   # dev key A, imported by `run`

new_public() { wallet account new public 2>&1 | grep -oE 'Public/[1-9A-HJ-NP-Za-km-z]+' | head -1; }

deploy() {
  local elf="$GUESTS/${1:-hello_world}.bin" size n header segs=()
  size=$(stat -c %s "$elf")
  # Ask the loader how many chunks it makes (it chunks the ELF inside the .bin, not the file): a dry run with
  # one throwaway segment fails with "splits into N segment(s)".
  n=$(wallet program-loader deploy --elf "$elf" --header "$(new_public)" --segments "$(new_public)" \
        --payer "$PAYER" --dry-run 2>&1 | grep -oE 'splits into [0-9]+' | grep -oE '[0-9]+' | head -1 || true)
  n=${n:-1}
  step "deploy $(basename "$elf"): $size bytes -> $n segments"
  header=$(new_public); echo "header  $header"
  for _ in $(seq 1 "$n"); do segs+=("$(new_public)"); done; echo "segments ${segs[*]}"
  step "dry run"; wallet program-loader deploy --elf "$elf" --header "$header" --segments "${segs[@]}" --payer "$PAYER" --dry-run 2>&1 | tail -12
  step "deploy"; time wallet program-loader deploy --elf "$elf" --header "$header" --segments "${segs[@]}" --payer "$PAYER" 2>&1 | tail -12
  echo "$header" > "$LEE_WALLET_HOME_DIR/${1:-hello_world}.header"
  step "payer after"; wallet account get --account-id "$PAYER" 2>&1 | head -8 || true
  rpc getLastBlockId
}

call() {
  local target
  target=$(new_public); echo "greeting target $target"
  step "run_hello_world (deploys again, then sends the call with no fee declaration)"
  ( cd "$LEZ/examples/program_deployment" && time "$BIN/run_hello_world" "$GUESTS/hello_world.bin" "${target#Public/}" "${PAYER#Public/}" ) 2>&1 | tail -6
  echo "exit ${PIPESTATUS[0]}"
  for _ in 1 2 3 4 5 6; do sleep 5; done   # let a block or two pass
  step "target account after"
  wallet account get --account-id "$target" 2>&1 | head -12
  rpc getAccount "[\"${target#Public/}\"]" | cut -c1-600
}

call2() {
  local program target
  program=$(cat "$LEE_WALLET_HOME_DIR/hello_world.header"); program=${program#Public/}
  for mode in ${MODES:-nofee fee}; do
    target=$(new_public); target=${target#Public/}
    step "$mode -> target $target (program $program)"
    "$BIN/spike_call_hello" "$program" "$target" "${PAYER#Public/}" "$mode" 2>&1 | tail -3 | cut -c1-300
    for _ in 1 2 3 4 5 6 7 8; do sleep 5; done
    echo "target account:"; rpc getAccount "[\"$target\"]" | cut -c1-500
  done
  step "payer after"; wallet account get --account-id "$PAYER" 2>&1 | head -3
  rpc getLastBlockId
}

case "${1:-}" in
  rpc) shift; rpc "$@" ;;
  call) call ;;
  call2) call2 ;;
  run) run ;;
  deploy) shift; deploy "$@" ;;
  *) echo "usage: $0 rpc <method> [params] | run" >&2; exit 2 ;;
esac
