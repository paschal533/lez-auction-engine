#!/usr/bin/env bash
# Regenerates crates/auction_ffi/include/auction_ffi.h (build.rs runs cbindgen) from a deleted header
# and fails on any difference from the committed one. Locally: scripts/wsl-env.sh scripts/ci/ffi-header-drift.sh
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
export RISC0_SKIP_BUILD=1
header=crates/auction_ffi/include/auction_ffi.h
saved="$(mktemp)"
trap 'rm -f "$saved"' EXIT
cp "$header" "$saved"
rm -f "$header"
cargo build --locked -q -p auction_ffi
if [ ! -f "$header" ]; then
  echo "ffi-header-drift: the build did not regenerate $header" >&2
  exit 1
fi
if ! diff -u "$saved" "$header"; then
  echo "ffi-header-drift: the committed header differs from the one cbindgen generates; build auction_ffi and commit crates/auction_ffi/include/" >&2
  exit 1
fi
echo "ffi-header-drift: the committed header matches"
