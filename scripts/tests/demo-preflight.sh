#!/usr/bin/env bash
# Review Focus 1: scripts/demo.sh refuses a machine that is not clean, in preflight, with a named reason
# and a distinct exit code, before any sequencer starts. Stubs `df` and `ss` on PATH; fake LEZ binaries.
# Stub `df` answers in KiB, the unit scripts/disk-guard.sh asks for (`df -k --output=avail`).
set -uo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fails=0
kib_100g=104857600
kib_2g=2097152
mkdir -p "$tmp/deps/lez/target/debug" "$tmp/deps/lez/lez/sequencer/service/configs/debug" \
  "$tmp/deps/lez/lez/wallet/configs/debug" "$tmp/bin" "$tmp/empty"
for b in sequencer_service wallet; do
  printf '#!/bin/sh\nexit 0\n' > "$tmp/deps/lez/target/debug/$b"
  chmod +x "$tmp/deps/lez/target/debug/$b"
done
echo '{}' > "$tmp/deps/lez/lez/sequencer/service/configs/debug/sequencer_config.json"
echo '{}' > "$tmp/deps/lez/lez/wallet/configs/debug/wallet_config.json"
stub() { printf '#!/bin/sh\n%s\n' "$2" > "$tmp/bin/$1"; chmod +x "$tmp/bin/$1"; }
# DEMO_ARGS: extra scripts/demo.sh flags for the next case (default none).
run_case() {
  local name="$1" want="$2" text="$3" out got
  shift 3
  # shellcheck disable=SC2086
  out="$(env "$@" "$repo/scripts/demo.sh" ${DEMO_ARGS:-} --preflight-only 2>&1)"
  got=$?
  if [[ "$got" != "$want" || "$out" != *"$text"* ]]; then
    echo "FAIL $name: exit $got (want $want), output: $out"
    fails=$((fails + 1))
  else
    echo "ok   $name"
  fi
}
stub df "echo Avail; echo $kib_100g"
stub ss 'exit 0'
base=(PATH="$tmp/bin:$PATH" LEZ_DEPS="$tmp/deps" DEMO_RUN_DIR="$tmp/run")
run_case clean 0 "preflight ok" "${base[@]}"
# --setup-only (scripts/basecamp/run-app.sh) goes through the same preflight; a bad flag value is refused.
DEMO_ARGS="--setup-only --sequencer-dev-mode 1" run_case setup_only 0 "preflight ok" "${base[@]}"
DEMO_ARGS="--setup-only --sequencer-dev-mode 2" run_case setup_only_bad_mode 2 "takes 0 or 1" "${base[@]}"
DEMO_ARGS="--setup" run_case unknown_flag_usage 2 "[--setup-only]" "${base[@]}"
stub df "echo Avail; echo $kib_2g"
run_case low_disk 3 "STOP" "${base[@]}"
stub df "echo Avail; echo $kib_100g"
run_case no_lez_binaries 4 "bootstrap-lez.sh" PATH="$tmp/bin:$PATH" LEZ_DEPS="$tmp/empty" DEMO_RUN_DIR="$tmp/run"
stub ss 'echo "LISTEN 0 128 127.0.0.1:3040 0.0.0.0:* users:((\"sequencer_servi\",pid=4242,fd=9))"'
run_case port_in_use 5 "port 3040 is in use" "${base[@]}"
stub ss 'exit 0'
mkdir -p "$tmp/guests"
cp "$repo"/artifacts/guests/* "$tmp/guests/"
printf 'x' >> "$tmp/guests/auction_engine.bin"
run_case guest_mismatch 6 "do not match" "${base[@]}" GUESTS_DIR="$tmp/guests"
if (( fails == 0 )); then echo "demo-preflight: all cases pass"; else echo "demo-preflight: $fails case(s) failed"; exit 1; fi
