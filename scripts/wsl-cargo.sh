#!/usr/bin/env bash
# Runs one cargo command inside WSL for this repository (Plan 2 build convention).
# From Git Bash:
#   MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc \
#     'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_core'
# The target directory lives on the WSL-native disk; reading sources over /mnt/c is slow, writing targets there is slower.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
env_file="${LEZ_ENV_FILE:-$HOME/lez-spikes/s1/env.sh}"
if [[ ! -f "$env_file" ]]; then
  echo "wsl-cargo.sh: missing $env_file (see docs/spikes/S1.md, 'Missing system packages')" >&2
  exit 2
fi
# shellcheck disable=SC1090
. "$env_file"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/lez-target/plan2}"
export RISC0_DEV_MODE="${RISC0_DEV_MODE:-1}"
mkdir -p "$CARGO_TARGET_DIR"
cd "$repo_root"
exec cargo "$@"
