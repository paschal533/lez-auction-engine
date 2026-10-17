#!/usr/bin/env bash
# scripts/testnet-legacy.sh refuses, before sending anything, with a named reason and a distinct exit code:
# no mode, a state dir that is not ours or is under /mnt, a resume of a directory that is not a run, an
# unreachable node, another chain (block 1); a genesis taken from the environment is announced. Hermetic: a stub
# JSON-RPC node on 127.0.0.1, stub `df`, fake LEZ wallet. Nothing reaches a public network.
set -uo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
tmp="$(mktemp -d)"
node_pid=""
cleanup() { [[ -n "$node_pid" ]] && kill "$node_pid" 2>/dev/null; rm -rf "$tmp"; }
trap cleanup EXIT
fails=0
mkdir -p "$tmp/deps/lez/target/debug" "$tmp/deps/lez/lez/wallet/configs/debug" "$tmp/bin" "$tmp/foreign"
printf '#!/bin/sh\nexit 0\n' > "$tmp/deps/lez/target/debug/wallet"
chmod +x "$tmp/deps/lez/target/debug/wallet"
echo '{}' > "$tmp/deps/lez/lez/wallet/configs/debug/wallet_config.json"
printf '#!/bin/sh\necho Avail; echo 104857600\n' > "$tmp/bin/df"
chmod +x "$tmp/bin/df"
touch "$tmp/foreign/precious"

# Stub node: block 1 has hash 11..11; every guest is "not deployed".
cat > "$tmp/node.py" <<'PY'
import base64, http.server, json, struct, sys
BLOCK1 = base64.b64encode(struct.pack("<Q", 1) + bytes(32) + bytes([0x11]) * 32 + bytes(16)).decode()
class H(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        req = json.loads(self.rfile.read(int(self.headers["content-length"])))
        result = {"getLastBlockId": 7, "getBlock": BLOCK1, "getTransaction": None}[req["method"]]
        body = json.dumps({"jsonrpc": "2.0", "id": req["id"], "result": result}).encode()
        self.send_response(200); self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body))); self.end_headers(); self.wfile.write(body)
    def log_message(self, *a): pass
s = http.server.HTTPServer(("127.0.0.1", 0), H)
print(s.server_address[1], flush=True)
s.serve_forever()
PY
python3 "$tmp/node.py" > "$tmp/port" &
node_pid=$!
for _ in $(seq 1 50); do [[ -s "$tmp/port" ]] && break; sleep 0.1; done
node="http://127.0.0.1:$(cat "$tmp/port")/"

# run_case NAME EXIT TEXT [VAR=VALUE ...] -- [script arguments]
run_case() {
  local name="$1" want="$2" text="$3" out got vars=()
  shift 3
  while (( $# )) && [[ "$1" != -- ]]; do vars+=("$1"); shift; done
  shift
  out="$(env PATH="$tmp/bin:$PATH" LEZ_DEPS="$tmp/deps" CARGO_TARGET_DIR="$tmp/target" \
    TESTNET_STATE_DIR="$tmp/state" "${vars[@]}" "$repo/scripts/testnet-legacy.sh" "$@" 2>&1)"
  got=$?
  if [[ "$got" != "$want" || "$out" != *"$text"* ]]; then
    echo "FAIL $name: exit $got (want $want), output: $out"
    fails=$((fails + 1))
  else
    echo "ok   $name"
  fi
}
run_case no_mode 2 "usage:" --
run_case foreign_state_dir 2 "not a testnet-legacy state directory" -- --preflight-only --state-dir "$tmp/foreign" --url "$node"
[[ -f "$tmp/foreign/precious" ]] || { echo "FAIL foreign_state_dir: a file was deleted"; fails=$((fails + 1)); }
run_case state_dir_under_mnt 2 "under /mnt" -- --preflight-only --state-dir /mnt/c/testnet-legacy-state --url "$node"
run_case resume_not_a_run 2 "not a testnet-legacy run directory" -- --resume "$tmp/foreign"
run_case unreachable 7 "is not reachable" -- --preflight-only --url "http://127.0.0.1:9/"
run_case another_chain 8 "expected c8e3bb9e" -- --preflight-only --url "$node"
run_case genesis_from_env_warns 0 "WARNING: expected block 1 from TESTNET_GENESIS" \
  TESTNET_GENESIS=1111111111111111111111111111111111111111111111111111111111111111 -- --preflight-only --url "$node"
run_case stub_chain_ok 0 "preflight ok (read-only" \
  TESTNET_GENESIS=1111111111111111111111111111111111111111111111111111111111111111 -- --preflight-only --url "$node"
if (( fails == 0 )); then echo "testnet-legacy-preflight: all cases pass"; else echo "testnet-legacy-preflight: $fails case(s) failed"; exit 1; fi
