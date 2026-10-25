#!/usr/bin/env bash
# The demo scenario (spec §1.1, ADR-051), shared by scripts/demo.sh (localnet) and
# scripts/testnet-legacy.sh (the public v0.2.4 legacy testnet). Source it after scripts/lib/localnet.sh
# with SUMMARY set (and DEMO_DEPLOY_PHASE / DEMO_PUBLISHER, if overridden); the node is already
# running and the deployment file written (ln_build).
# The scenario is a list of named phases (DEMO_PHASES); values one phase hands to a later one go
# through run.env (remember / recall), so a driver can resume after the last finished phase.
# Hooks a driver may redefine after sourcing (plain runs or no-ops here, so the localnet run is unchanged):
#   demo_run_phase NAME TITLE FUNC  runs one phase: prints TITLE (when not empty) and calls FUNC
#   demo_after_deploy               after the programs are deployed, before any program instruction
#   demo_before_positions           after the engine is configured, before the first position
#   demo_tick PRICE                 at the start of every scenario phase after setup (testnet keeper round)
: "${SUMMARY:?set SUMMARY (the summary file) before sourcing scripts/lib/demo-scenario.sh}"
DEMO_DEPLOY_PHASE="${DEMO_DEPLOY_PHASE:-accounts, auth-transfer init, deploy token, twap_oracle, token_mint_authority, stablecoin (+hooks), auction_engine}"
phase() { printf '\n=== %s\n' "$*" | tee -a "$SUMMARY"; }
# expect NAME ACTUAL eq|gt|ge|le EXPECTED (integers of any size); sub A B prints A - B
sub() { python3 -c 'import sys; print(int(sys.argv[1]) - int(sys.argv[2]))' "$1" "$2"; }
expect() {
  local name="$1" got="$2" op="$3" want="$4"
  if python3 -c 'import sys
a, op, b = int(sys.argv[1]), sys.argv[2], int(sys.argv[3])
sys.exit(0 if {"eq": a == b, "gt": a > b, "ge": a >= b, "le": a <= b}[op] else 1)' "$got" "$op" "$want"; then
    echo "check $name: $got $op $want: ok" | tee -a "$SUMMARY"
  else
    echo "check $name: $got is not $op $want" | tee -a "$SUMMARY" >&2
    exit 1
  fi
}
demo_run_phase() {
  if [[ -n "$2" ]]; then phase "$2"; fi
  "$3"
}
demo_after_deploy() { :; }
demo_before_positions() { :; }
demo_tick() { :; }
# The redemption price R and the moved market price (1.9 R), RAY.
DEMO_PRICE_R=1000000000000000000000000000
DEMO_PRICE_MOVED=1900000000000000000000000000

dp_accounts() { ln_accounts; }
# oracle-src signs twice (the first oracle, then the publisher's), so it is initialised too (ADR-018).
dp_auth() { ln_auth admin engine-admin keeper owner caller oracle-src; }
dp_deploy() { ln_deploy; demo_after_deploy; }
dp_host() { ln_host; }
dp_engine() { ln_engine "$REPO/cli/params/demo-engine.json"; }
dp_bind() { ln_bind; }
dp_collateral() { ln_collateral "$REPO/cli/params/demo-collateral.json"; }
dp_holdings() { ln_holdings; demo_before_positions; }

dp_positions() {
  local p0 p1 p2
  demo_tick "$DEMO_PRICE_R"
  p0="$(run_field position host open-position --owner owner --owner-collateral owner-coll --nonce 0 --amount 500000)"
  remember P0 "$p0"
  cli host generate-debt --owner owner --position "$p0" --owner-stable owner-stable --amount 300000
  p1="$(run_field position host open-position --owner owner --owner-collateral owner-coll --nonce 1 --amount 1000000)"
  remember P1 "$p1"
  cli host generate-debt --owner owner --position "$p1" --owner-stable owner-stable --amount 100000
  p2="$(run_field position host open-position --owner owner --owner-collateral owner-coll --nonce 2 --amount 500000)"
  remember P2 "$p2"
  cli host generate-debt --owner owner --position "$p2" --owner-stable owner-stable --amount 300000
  cli host set-min-cr --admin admin --ratio 2000000000000000000000000000
  cli list-at-risk | tee -a "$SUMMARY"
}

dp_f1f3() {
  local p0 a0 buffer_id
  p0="$(recall P0)"
  demo_tick "$DEMO_PRICE_R"
  cli quote-liquidation --position "$p0" | tee -a "$SUMMARY"
  a0="$(run_field auction liquidate --position "$p0" --keeper keeper --keeper-holding keeper-coll)"
  remember A0 "$a0"
  cli quote-bid --auction "$a0" --coin-in-max 100000 | tee -a "$SUMMARY"
  cli bid --auction "$a0" --coin-in-max 100000 --bidder-stable owner-stable --bidder-collateral owner-coll
  cli bid --auction "$a0" --coin-in-max 230000 --bidder-stable owner-stable --bidder-collateral owner-coll
  cli claim-leftover --owner owner --owner-holding owner-coll --auction "$a0"
  buffer_id="$(run_field buffer admin addresses --admin engine-admin --salt 1)"
  remember BUFFER "$buffer_id"
  expect "buffer holds the 10 % penalty" "$(balance_of "$buffer_id")" eq 30000
}

dp_f5() {
  demo_tick "$DEMO_PRICE_R"
  cli observe-feed --caller caller
  cli settle-debt --caller caller
  cli check-solvency --caller caller
  cli start-surplus --caller caller
  cli take-surplus --payer taker-protocol --receiver taker-stable --amount 500
  expect "buffer after the surplus lot" "$(balance_of "$(recall BUFFER)")" eq 29500
}

dp_price_move() {
  cli dev oracle-publish --price-source oracle-src --price "$DEMO_PRICE_MOVED" --host-admin admin --engine-admin engine-admin
  cli host status | tee -a "$SUMMARY"
}

dp_shortfall() {
  local a2 u1
  demo_tick "$DEMO_PRICE_MOVED"
  a2="$(run_field auction liquidate --position "$(recall P2)" --keeper keeper --keeper-holding keeper-coll)"
  remember A2 "$a2"
  cli bid --auction "$a2" --coin-in-max 330000 --bidder-stable owner-stable --bidder-collateral owner-coll
  cli list-auctions --all | tee -a "$SUMMARY"
  u1="$(unbacked)"
  remember U1 "$u1"
  expect "unbacked debt after the shortfall (R4 range, lower)" "$u1" ge 42154
  expect "unbacked debt after the shortfall (R4 range, upper)" "$u1" le 63200
}

dp_f4() {
  local u1 u2 u3 tokens_before
  u1="$(recall U1)"
  demo_tick "$DEMO_PRICE_MOVED"
  cli settle-debt --caller caller
  u2="$(unbacked)"
  expect "the buffer settled 29_500 of the bad debt" "$u2" eq "$(sub "$u1" 29500)"
  expect "enough bad debt for one lot" "$u2" ge 5000
  cli start-debt --caller caller
  cli dev token-transfer --from owner-stable --to taker-stable --amount 5000
  tokens_before="$(balance_of taker-protocol)"
  cli take-debt --payer taker-stable --receiver taker-protocol --amount 5000
  expect "protocol tokens minted to the taker" "$(balance_of taker-protocol)" gt "$tokens_before"
  u3="$(unbacked)"
  expect "one lot of bad debt burned" "$u3" eq "$(sub "$u2" 5000)"
  expect "bad debt still above the 5_000 freeze threshold" "$u3" gt 5000
}

dp_freeze() {
  local frozen
  demo_tick "$DEMO_PRICE_MOVED"
  cli admin set-engine-params --admin engine-admin --params "$REPO/cli/params/demo-engine.json" --disable-protocol-token
  cli check-solvency --caller caller
  frozen="$(run_field frozen host status)"
  [[ "$frozen" == yes ]] || { echo "check host frozen: got $frozen" | tee -a "$SUMMARY" >&2; exit 1; }
  echo "check host frozen: yes" | tee -a "$SUMMARY"
}

dp_final() {
  cli admin verify | tee -a "$SUMMARY"
  if [[ "$LN_LOCAL_SEQUENCER" == 1 ]]; then ln_check_log | tee -a "$SUMMARY"; fi
  "$REPO/scripts/disk-guard.sh" "${DEMO_MIN_C_GIB:-3}" "${DEMO_MIN_HOME_GIB:-20}" | tee -a "$SUMMARY"
  echo "demo complete" | tee -a "$SUMMARY"
}

# name|title|function, in order; an empty title prints no header (the step belongs to the phase above).
DEMO_PHASES=(
  "accounts|$DEMO_DEPLOY_PHASE|dp_accounts"
  "auth||dp_auth"
  "deploy||dp_deploy"
  "host|host: collateral token, oracle at R = 1, initialize_program|dp_host"
  "engine|engine: mock protocol token (mint authority = engine PDA), init_config (demo parameters), bind, collateral config|dp_engine"
  "bind||dp_bind"
  "collateral||dp_collateral"
  "holdings||dp_holdings"
  "positions|positions p0, p1, p2; min collateral ratio 1.5 -> 2.0 makes p0 and p2 liquidatable|dp_positions"
  "f1f3|F1-F3: liquidate p0 (keeper reward), partial bid, final bid, leftover claimed|dp_f1f3"
  "f5|F5: surplus auction sells buffer coin for protocol tokens, which are burned|dp_f5"
  "price_move|price move: the ${DEMO_PUBLISHER:-localnet} publisher sets the market price to 1.9 R (ADR-050)|dp_price_move"
  "shortfall|shortfall: liquidate p2 at 1.9 R; the lot-binding bid raises less than the debt|dp_shortfall"
  "f4|F4: settle against the buffer, start a debt auction, take one lot (stablecoin burned, protocol tokens minted)|dp_f4"
  "freeze|freeze path: without a protocol token, bad debt above the threshold freezes new debt|dp_freeze"
  "final|final check|dp_final"
)

demo_scenario() {
  local entry name title fn
  for entry in "${DEMO_PHASES[@]}"; do
    IFS='|' read -r name title fn <<<"$entry"
    demo_run_phase "$name" "$title" "$fn"
  done
}

# Runs the phases in order up to and including LAST (scripts/demo.sh --setup-only); an unknown name fails.
demo_scenario_until() {
  local last="$1" entry name title fn
  if [[ " ${DEMO_PHASES[*]%%|*} " != *" $last "* ]]; then
    echo "demo_scenario_until: no phase named $last" >&2
    return 1
  fi
  for entry in "${DEMO_PHASES[@]}"; do
    IFS='|' read -r name title fn <<<"$entry"
    demo_run_phase "$name" "$title" "$fn"
    if [[ "$name" == "$last" ]]; then return 0; fi
  done
}
