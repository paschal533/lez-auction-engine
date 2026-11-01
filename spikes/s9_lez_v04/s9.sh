#!/usr/bin/env bash
# S9 driver: deploy s9_engine + s9_host on the local LEZ v0.4.0-rc1 standalone sequencer and run the
# liquidation conversation (engine -> host -> engine -> auction PDA). Uses ../s8_lez_v04/v04-spike.sh (S8) helpers.
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SPIKE="$HERE/../s8_lez_v04/v04-spike.sh"
source "$HOME/lez-spikes/s1/env.sh"
export LEE_WALLET_HOME_DIR="$HOME/lez-spikes/v04-run/wallet" LEE_WALLET_PASSWORD=""
export S9_PAYER="Public/6iArKUXxhUJqS7kCaPNhwMWt3ro71PDyBj7jwAyE2VQV"
BIN="$HOME/lez-target/v04/release"
W="$BIN/wallet"
R="$BIN/spike_s9"
STATE="$LEE_WALLET_HOME_DIR/s9.env"

step() { echo; echo "== $*"; }
blocks() { for _ in $(seq 1 "${1:-4}"); do sleep 5; done; }   # let blocks pass
new_public() { "$W" account new public 2>&1 | grep -oE 'Public/[1-9A-HJ-NP-Za-km-z]+' | head -1; }
balance() { "$W" account get --account-id "$S9_PAYER" 2>&1 | head -1; }

start_sequencer() {
  if ss -ltn | grep -q ':3040 '; then echo "sequencer already up"; return; fi
  ( cd "$HOME/lez-spikes/v04-run" && RUST_LOG=info,risc0_zkvm=warn,kameo=warn \
      setsid nohup "$BIN/sequencer_service" start seq_home >> sequencer.log 2>&1 < /dev/null & )
  for _ in $(seq 1 60); do ss -ltn | grep -q ':3040 ' && { echo "sequencer up"; return; }; sleep 2; done
  echo "sequencer did not come up"; exit 1
}

setup() {
  start_sequencer
  step "deploy s9_engine and s9_host"
  bash "$SPIKE" deploy s9_engine 2>&1 | grep -E "segments|Program deployed|Error" | tail -3
  bash "$SPIKE" deploy s9_host 2>&1 | grep -E "segments|Program deployed|Error" | tail -3
  ENGINE=$(cat "$LEE_WALLET_HOME_DIR/s9_engine.header"); HOST_PROG=$(cat "$LEE_WALLET_HOME_DIR/s9_host.header")
  CONFIG=$(new_public); HOST=$(new_public)
  printf 'ENGINE=%s\nHOST_PROG=%s\nCONFIG=%s\nHOST=%s\n' "$ENGINE" "$HOST_PROG" "$CONFIG" "$HOST" | tee "$STATE"
  step "init engine config and host (collateral 1000)"
  "$R" init-engine "$ENGINE" "$CONFIG"; blocks 4
  "$R" init-host "$HOST_PROG" "$HOST" "$ENGINE" "$CONFIG" 1000; blocks 4
  "$R" show "$CONFIG" | cut -c1-300; "$R" show "$HOST" | cut -c1-300
}

tests() {
  source "$STATE"
  step "simulate liquidate: PDA without evidence, then with evidence"
  "$R" liquidate "$ENGINE" "$CONFIG" "$HOST_PROG" "$HOST" 7 250 noevidence sim
  "$R" liquidate "$ENGINE" "$CONFIG" "$HOST_PROG" "$HOST" 7 250 evidence sim
  step "spoof: Seized sent straight to the config (must revert)"
  "$R" spoof "$ENGINE" "$CONFIG" 250
  step "send liquidate with evidence (gas from the simulation + margin)"
  echo "payer before: $(balance)"
  SPIKE_GAS=${SPIKE_GAS:-200000} "$R" liquidate "$ENGINE" "$CONFIG" "$HOST_PROG" "$HOST" 7 250 evidence send; blocks 4
  echo "payer after:  $(balance)"
  step "states after"
  "$R" show "$CONFIG" | cut -c1-300; "$R" show "$HOST" | cut -c1-300
  PDA=$("$R" liquidate "$ENGINE" "$CONFIG" "$HOST_PROG" "$HOST" 7 250 evidence sim 2>&1 | grep -oE 'auction [1-9A-HJ-NP-Za-km-z]+' | head -1 | cut -d' ' -f2)
  echo "auction PDA $PDA"; "$R" show "$PDA" | cut -c1-300
  step "replay the same seed (auction exists: must revert in simulation)"
  "$R" liquidate "$ENGINE" "$CONFIG" "$HOST_PROG" "$HOST" 7 250 evidence sim
}

case "${1:-}" in
  setup) setup ;;
  tests) tests ;;
  all) setup; tests ;;
  *) echo "usage: $0 setup|tests|all" >&2; exit 2 ;;
esac
