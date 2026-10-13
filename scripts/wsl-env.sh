#!/usr/bin/env bash
# Runs one command inside WSL with the Plan 2 build environment: S1's env.sh (risc0 toolchain, libclang,
# sysroot), the shared target directory and dev mode unless the caller set it. scripts/wsl-cargo.sh is
# this wrapper applied to cargo. Never point CARGO_TARGET_DIR anywhere else (Plan 3b Global Constraints).
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
env_file="${LEZ_ENV_FILE:-$HOME/lez-spikes/s1/env.sh}"
if [[ ! -f "$env_file" ]]; then
  echo "wsl-env.sh: missing $env_file (see docs/spikes/S1.md, 'Missing system packages')" >&2
  exit 2
fi
# shellcheck disable=SC1090
. "$env_file"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/lez-target/plan2}"
export RISC0_DEV_MODE="${RISC0_DEV_MODE:-1}"
mkdir -p "$CARGO_TARGET_DIR"
cd "$repo_root"
exec "$@"
