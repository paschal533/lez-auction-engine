#!/usr/bin/env bash
# Regenerates the QML test fixtures of the Auctions app (Plan 4 Task 10).
#   scripts/basecamp/gen-qml-fixtures.sh
# Runs the ignored auction_ffi test `fixtures`, which writes the JSON of every view on the standard fixture
# (the values the auction_engine module returns to QML) into apps/auctions/tests/qml/fixtures/.
# Runs inside WSL; from Git Bash it re-runs itself through wsl.
set -euo pipefail

if ! grep -qi microsoft /proc/version 2>/dev/null && [[ ! -d /mnt/c ]] && command -v wsl >/dev/null 2>&1; then
  here="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
  win="$(cd "$here" && pwd -W)"
  wsl_here="/mnt/$(echo "${win:0:1}" | tr 'A-Z' 'a-z')${win:2}"
  MSYS_NO_PATHCONV=1 exec wsl -d Ubuntu-20.04 -e bash -lc 'cd "$1" && shift && exec scripts/basecamp/gen-qml-fixtures.sh "$@"' _ "$wsl_here" "$@"
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
cd "$root"

# No guest ELF is needed; two jobs keep memory use low on small machines.
export RISC0_SKIP_BUILD="${RISC0_SKIP_BUILD:-1}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"

scripts/wsl-cargo.sh test -p auction_ffi --test fixtures -- --ignored
echo "fixtures written to apps/auctions/tests/qml/fixtures:"
ls apps/auctions/tests/qml/fixtures
