#!/usr/bin/env bash
# Plan 2 Task 20 localnet smoke. Run INSIDE WSL, one step at a time:
#   scripts/localnet-smoke.sh step_sequencer | step_accounts | step_auth | step_deploy | step_host | step_engine |
#                             step_verify_and_bind | step_bind | step_collateral | step_holdings | step_flow | step_check | step_stop
# (make_engine_idl is run by step_deploy; it can also be run on its own.)
# Requires the S1 builds (sequencer_service, wallet, spel) and S1's Docker-built upstream guests.
# Only the sequencer started by step_sequencer (tracked in a pid file) is ever signalled.
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
S1="$HOME/lez-spikes/s1"
LN="$HOME/lez-target/plan2-localnet"
W="$S1/lez/target/debug/wallet"
SP="$S1/spel/target/debug/spel"
UP="$S1/lez-programs/target/guest"
export LEE_WALLET_HOME_DIR="$LN/wallet-home"
. "$S1/env.sh"
mkdir -p "$LN/logs"
# Fixed by S1 (same upstream binaries -> same ids):
TOK=14b3bc6cc129d0359f9595ba0f60a13e4a8df08ffe8eb1a83251c47abecc67a8
TWAP=5681e27c27b3ece4a7c161bbf8e12e6ece6ca69925a87b77f9e193fa4a249b31
CLOCK=4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU
FAUCET_PDA=GmAwyCjGaT7ocZ2ePDgGFmNw9Yv51ohG421dHMukZRQC
ONE=1000000000000000000000000000
SALT=1
env_file="$LN/localnet.env"   # written by hand after step_accounts and step_engine (see docs/architecture/localnet.md)
load() { local keep="${ORACLE:-}"; . "$env_file"; [[ -n "$keep" ]] && ORACLE="$keep"; return 0; }
spel_host() { "$SP" --idl "$LN/stablecoin-idl-u128.json" -p "$STABLE" -- "$@"; }
spel_engine() { "$SP" --idl "$LN/auction_engine-idl-spel.json" -p "$ENGINE" -- "$@"; }
spel_token() { "$SP" --idl "$S1/lez-programs/artifacts/token-idl.json" -p "$TOK" -- "$@"; }

step_sequencer() {
  if (ss -ltn 2>/dev/null || true) | grep -q ':3040 '; then
    echo "port 3040 is already in use; not starting a second sequencer" >&2
    exit 1
  fi
  rm -rf "$LN/seqhome" && mkdir -p "$LN/seqhome"
  (cd "$S1/lez" && RISC0_DEV_MODE=1 RUST_LOG=info setsid nohup target/debug/sequencer_service \
    lez/sequencer/service/configs/debug/sequencer_config.json --home "$LN/seqhome" --listen-address 127.0.0.1 -p 3040 \
    > "$LN/logs/sequencer.log" 2>&1 < /dev/null &)
  rm -f "$LN/sequencer.pid" "$LN/config-verified"
  # Poll for the listener (up to 60 s). setsid may fork, so $! is not reliable: record the pid of whatever listens
  # on our port (we just started it).
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
  curl -s -X POST localhost:3040 -H 'content-type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"getLastBlockId","params":{}}'
}

step_accounts() {
  mkdir -p "$LEE_WALLET_HOME_DIR" && cp "$S1/lez/lez/wallet/configs/debug/wallet_config.json" "$LEE_WALLET_HOME_DIR/"
  for l in admin freeze oracle-src coll-def coll-hold engine-admin keeper keeper-coll owner owner-coll owner-stable; do
    echo pw | "$W" account new public --label "$l"
  done
  # Keys that sign more than one transaction must be auth-transfer-initialised BEFORE first use (ADR-018).
  "$W" account list
  echo "Write $env_file now (format in docs/architecture/localnet.md), then run: $0 step_auth"
}

step_auth() {
  load
  for a in "$ADMIN" "$ENGINE_ADMIN" "$KEEPER" "$OWNER"; do "$W" auth-transfer init --account-id "Public/$a"; done
}

step_deploy() {
  load
  for p in token twap_oracle token_mint_authority; do "$W" deploy-program "$UP/$p.bin"; done
  "$W" deploy-program "$LN/guest/stablecoin.bin"
  "$W" deploy-program "$LN/guest/auction_engine.bin"
  make_host_idl
  make_engine_idl
}

# Host IDL copy for spel: (1) i128 -> u128 (spel rev 1ef0500 cannot encode i128; valid for gains >= 0, we use 0);
# (2) instructions re-sorted into `stablecoin_core::Instruction` enum order. spel encodes the IDL *position* as the
# instruction index, and idl/stablecoin-idl.json differs from the enum order from index 8 on up to 18 (indices 0-7
# and the hooks 19-21 agree), so e.g. set_minimum_collateralization_ratio (IDL 11, enum 8) would execute
# set_timing_parameters (found by this smoke; see docs/architecture/localnet.md).
make_host_idl() {
  python3 - "$REPO/programs/stablecoin/core/src/lib.rs" "$REPO/idl/stablecoin-idl.json" "$LN/stablecoin-idl-u128.json" <<'PY'
import json, re, sys
src = open(sys.argv[1], encoding="utf-8").read()
body = src[src.index("pub enum Instruction"):]
body = body[:body.index("\n}\n")]
variants = re.findall(r"^    ([A-Z][A-Za-z]+)\s*(?:\{|,)", body, re.M)
order = [re.sub(r"(?<!^)(?=[A-Z])", "_", v).lower() for v in variants]
idl = json.load(open(sys.argv[2], encoding="utf-8"))
by_name = {i["name"]: i for i in idl["instructions"]}
assert sorted(order) == sorted(by_name), sorted(set(order) ^ set(by_name))
idl["instructions"] = [by_name[n] for n in order]
open(sys.argv[3], "w", encoding="utf-8").write(json.dumps(idl, indent=2).replace('"i128"', '"u128"'))
PY
}

# spel (rev 1ef0500) cannot resolve a PDA whose seed is another PDA of the same instruction: it demands
# --config / --auction for the seed and then rejects the flag as "unknown argument" because those accounts are
# PDAs. This copy drops the `pda` marking from `config` (init_config) and `auction` (liquidate) only, so they are
# passed explicitly; their addresses are the ones `localnet_tool pdas` prints. Wire format and account order are unchanged.
make_engine_idl() {
  python3 - "$REPO/idl/auction_engine-idl.json" "$LN/auction_engine-idl-spel.json" <<'PY'
import json, sys
idl = json.load(open(sys.argv[1], encoding="utf-8"))
for ins in idl["instructions"]:
    for acc in ins["accounts"]:
        if (ins["name"], acc["name"]) in {("init_config", "config"), ("liquidate", "auction")}:
            acc.pop("pda", None)
json.dump(idl, open(sys.argv[2], "w", encoding="utf-8"), indent=2)
PY
}

step_host() {
  load
  spel_token new-fungible-definition --name COLL --total-supply 100000000000 --mint-authority "$FAUCET_PDA" \
    --definition-target-account "$COLLDEF" --holding-target-account "$OWNER_COLL"
  "$SP" --idl "$S1/lez-programs/artifacts/twap_oracle-idl.json" -p "$TWAP" -- create-oracle-price-account \
    --base-asset "$SDEF" --quote-asset "$COLLDEF" --initial-price "$ONE" --window-duration 3600000 \
    --oracle-price-account "$ORACLE" --price-source "$ORSRC" --clock "$CLOCK"
  spel_host initialize-program --freeze-authority-account-id "$FREEZE" --initial-stability-fee-per-millisecond "$ONE" \
    --initial-controller-proportional-gain 0 --initial-controller-integral-gain 0 \
    --initial-minimum-collateralization-ratio 1500000000000000000000000000 --minimum-milliseconds-between-rate-updates 60000 \
    --maximum-oracle-price-age-milliseconds 3600000 --initial-redemption-price "$ONE" --stablecoin-name LSC \
    --admin "$ADMIN" --protocol-parameters "$PP" --stability-fee-accumulator "$SFA" --redemption-price-state "$RPS" \
    --stablecoin-definition "$SDEF" --stablecoin-master-holding "$SMASTER" --collateral-definition "$COLLDEF" \
    --market-price-oracle "$ORACLE" --clock "$CLOCK"
}

step_engine() {
  load
  spel_engine init-config --salt "$SALT" --global-cap 10000000 --freeze-on-bad-debt true --bad-debt-freeze-threshold 1000 \
    --protocol-token-enabled false --protocol-token-ref-price 2000000000000000000000000000 --ref-oracle-enabled false \
    --ref-oracle-max-age-ms 3600000 --surplus-buffer-target 1000 --surplus-lot 500 --surplus-delay-ms 0 \
    --surplus-premium-bps 500 --surplus-max-discount-bps 2000 --debt-lot 500 --debt-initial-discount-bps 0 \
    --debt-growth "0,0,1000,3000" --surplus-decay "0,0,1000,2500" --max-mint-per-auction 5000 --min-take 10 \
    --system-ttl-ms 14400000 \
    --admin "$ENGINE_ADMIN" --config "$CONFIG" --protocol-parameters "$PP" \
    --stablecoin-definition "$SDEF" --protocol-token-definition "$UNUSED1" --ref-oracle "$UNUSED0"
}

# Compares decode-config output against everything the host is about to trust. Exits non-zero on any mismatch.
check_decoded_config() {
  local decoded="$1"
  grep -qx "admin=$ENGINE_ADMIN salt=$SALT" <<<"$decoded" || { echo "admin/salt mismatch: do NOT bind" >&2; return 1; }
  grep -qx "host_program_id=$STABLE token_program_id=$TOK" <<<"$decoded" || { echo "program id mismatch: do NOT bind" >&2; return 1; }
  grep -qx "host_protocol_parameters_id=$PP stablecoin_definition_id=$SDEF" <<<"$decoded" \
    || { echo "protocol parameters / stablecoin definition mismatch: do NOT bind" >&2; return 1; }
}

step_verify_and_bind() {
  load
  # Bind-time verification (ADR-027, I7): read the config the host is about to bind and compare.
  # (`spel inspect` needs an IDL type and the engine IDL has none, so the raw data comes from `wallet account get`.)
  # step_bind refuses to run unless this wrote the marker for the same config id.
  rm -f "$LN/config-verified"
  local data decoded
  data="$("$W" account get --account-id "Public/$CONFIG" | grep '^{' | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"])')"
  decoded="$(cd "$REPO" && scripts/wsl-cargo.sh run -q -p engine_integration_tests --example localnet_tool -- decode-config "$data")"
  echo "$decoded" | head -3 | tee "$LN/config-decoded.txt"
  check_decoded_config "$decoded" || exit 1
  printf '%s\n' "$CONFIG" > "$LN/config-verified"
  echo "config verified (admin, salt, host/token program ids, protocol parameters, stablecoin definition); next: $0 step_bind"
}

step_bind() {
  load
  if [[ ! -f "$LN/config-verified" ]] || [[ "$(cat "$LN/config-verified")" != "$CONFIG" ]]; then
    echo "refusing to bind: run step_verify_and_bind first (marker missing or for another config)" >&2
    exit 1
  fi
  spel_host set-liquidation-engine --engine-program-id "$ENGINE_LIMBS" --engine-authority-id "$ENGINE_AUTH" \
    --admin "$ADMIN" --protocol-parameters "$PP" --liquidation-engine-state "$LES"
}

step_collateral() {
  load
  spel_engine set-collateral-config --oracle-scale 0 --max-oracle-age-ms 3600000 --max-conf-bps 100 --band-lo-bps 9000 \
    --band-hi-bps 11000 --penalty-bps 1000 --tip 5 --chip-bps 10 --reward-cap 50 --debt-floor 100 --min-bid 25 \
    --type-cap 5000000 --schedule "0,200,2700000,1000" --admin-paused false \
    --admin "$ENGINE_ADMIN" --config "$CONFIG" --collateral-definition "$COLLDEF" \
    --oracle "$ORACLE" --protocol-parameters "$PP" --redemption-price-state "$RPS" --clock "$CLOCK"
}

step_holdings() {
  load
  # generate_debt and the keeper reward need initialised destination holdings.
  spel_token initialize-account --definition-account "$SDEF" --account-to-initialize "$OWNER_STABLE"
  spel_token initialize-account --definition-account "$COLLDEF" --account-to-initialize "$KEEPER_COLL"
}

flow_open() {
  load
  # Owner: an unsafe position (nonce 0: 500_000 coll, 300_000 debt at R = 1) and a healthy one (nonce 1) for bidding funds.
  spel_host open-position --position-nonce 0 --initial-collateral-amount 500000 --owner "$OWNER" --position "$POSITION" \
    --vault "$HOST_VAULT" --user-collateral-holding "$OWNER_COLL" --collateral-definition "$COLLDEF" --protocol-parameters "$PP" --clock "$CLOCK"
  spel_host generate-debt --amount 300000 --owner "$OWNER" --position "$POSITION" --stablecoin-definition "$SDEF" \
    --user-stablecoin-holding "$OWNER_STABLE" --stability-fee-accumulator "$SFA" --redemption-price-state "$RPS" \
    --market-price-oracle "$ORACLE" --protocol-parameters "$PP" --clock "$CLOCK"
  spel_host open-position --position-nonce 1 --initial-collateral-amount 1000000 --owner "$OWNER" --position "$POSITION1" \
    --vault "$HOST_VAULT1" --user-collateral-holding "$OWNER_COLL" --collateral-definition "$COLLDEF" --protocol-parameters "$PP" --clock "$CLOCK"
  spel_host generate-debt --amount 100000 --owner "$OWNER" --position "$POSITION1" --stablecoin-definition "$SDEF" \
    --user-stablecoin-holding "$OWNER_STABLE" --stability-fee-accumulator "$SFA" --redemption-price-state "$RPS" \
    --market-price-oracle "$ORACLE" --protocol-parameters "$PP" --clock "$CLOCK"
}

flow_raise_ratio() {
  load
  spel_host set-minimum-collateralization-ratio --new-ratio 2000000000000000000000000000 --admin "$ADMIN" --protocol-parameters "$PP"
}

flow_liquidate() {
  load
  spel_engine liquidate --nonce 0 --keeper "$KEEPER" --keeper-collateral-holding "$KEEPER_COLL" --config "$CONFIG" \
    --coll-config "$COLL_CONFIG" --position "$POSITION" --auction "$AUCTION" \
    --liquidation-engine-state "$LES" --host-vault "$HOST_VAULT" \
    --collateral-definition "$COLLDEF" --stability-fee-accumulator "$SFA" --redemption-price-state "$RPS" \
    --oracle "$ORACLE" --protocol-parameters "$PP" --clock "$CLOCK"
}

flow_bid() {
  load
  spel_engine bid-with-settlement --coin-in-max 330000 --min-collateral-out 0 --bidder-stable-holding "$OWNER_STABLE" \
    --bidder-collateral-holding "$OWNER_COLL" --config "$CONFIG" --coll-config "$COLL_CONFIG" --auction "$AUCTION" \
    --stablecoin-definition "$SDEF" --redemption-price-state "$RPS" \
    --oracle "$ORACLE" --clock "$CLOCK"
}

flow_claim() {
  load
  spel_engine claim-leftover --owner "$OWNER" --owner-collateral-holding "$OWNER_COLL" --config "$CONFIG" \
    --auction "$AUCTION"
}

step_flow() {
  flow_open
  flow_raise_ratio
  flow_liquidate
  flow_bid
  flow_claim
}

# The test oracle has no publisher: its price goes stale after 1 h (host and engine both enforce 3_600_000 ms), which
# the slow part of this runbook can exceed (error PriceFeedStale). This swaps in a freshly created oracle and rebinds
# the host (set_market_price_oracle) and the engine (set_collateral_config). Needs ORSRC2 and ORACLE2 in localnet.env
# (new "oracle-src2" public account; ORACLE2 from twap_oracle_pdas as in step 4 of the docs); then ORACLE=$ORACLE2.
step_refresh_oracle() {
  load
  "$SP" --idl "$S1/lez-programs/artifacts/twap_oracle-idl.json" -p "$TWAP" -- create-oracle-price-account     --base-asset "$SDEF" --quote-asset "$COLLDEF" --initial-price "$ONE" --window-duration 3600000     --oracle-price-account "$ORACLE2" --price-source "$ORSRC2" --clock "$CLOCK"
  spel_host set-market-price-oracle --admin "$ADMIN" --protocol-parameters "$PP" --new-oracle "$ORACLE2"
  ORACLE="$ORACLE2" step_collateral
}

step_stop() {
  # Stop only the sequencer this script started (pid file), never anything matched by name.
  local pid
  pid="$(cat "$LN/sequencer.pid" 2>/dev/null || true)"
  if [[ ! "$pid" =~ ^[0-9]+$ ]]; then
    echo "no sequencer pid recorded in $LN/sequencer.pid; nothing stopped (check manually, do not pkill by name)" >&2
    exit 1
  fi
  kill "$pid" 2>/dev/null || echo "pid $pid was not running" >&2
  rm -f "$LN/sequencer.pid"
}

step_check() {
  load
  "$SP" inspect "$POSITION" --idl "$LN/stablecoin-idl-u128.json" --type Position
  for a in "$AUCTION_VAULT" "$BUFFER" "$OWNER_COLL" "$OWNER_STABLE" "$KEEPER_COLL"; do "$SP" inspect "$a" --idl "$S1/lez-programs/artifacts/token-idl.json" --type TokenHolding; done
  grep -E "failed execution check|Transaction with hash" "$LN/logs/sequencer.log" | tail -20
}

"$@"
