#!/usr/bin/env bash
# Regenerates both IDLs from the guest sources with tools/idl-gen and fails on any difference from the
# committed files (spec §9 drift checks). Also catches an enum variant whose guest function was never
# added (the Plan 3a Task 1 sample gap). Locally: scripts/wsl-env.sh scripts/ci/idl-drift.sh
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
export RISC0_SKIP_BUILD=1
cargo run --locked -q -p idl-gen -- programs/auction_engine/methods/guest/src/bin/auction_engine.rs > idl/auction_engine-idl.json
cargo run --locked -q -p idl-gen -- programs/stablecoin/methods/guest/src/bin/stablecoin.rs > idl/stablecoin-idl.json
if ! git diff --exit-code -- idl/; then
  echo "idl-drift: a committed IDL differs from its guest source; run scripts/ci/idl-drift.sh and commit idl/" >&2
  exit 1
fi
echo "idl-drift: both IDLs match their guest sources"
