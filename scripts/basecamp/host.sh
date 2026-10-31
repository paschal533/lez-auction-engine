#!/usr/bin/env bash
# Host toolchain for the Auctions app (Plan 4 Task 1): Basecamp standalone + the lez_core wallet module.
#   scripts/basecamp/host.sh check   exit 2 if C: has less than MIN_C_GB (default 10) GB free; skipped without /mnt/c (CI)
#   scripts/basecamp/host.sh build   check, nix build Basecamp and lez_core, check again
#   scripts/basecamp/host.sh paths   print BASECAMP_BIN= and LEZ_CORE_ROOT= lines, shell-quoted (run-app.sh evals them)
# Runs inside WSL; from Git Bash it re-runs itself through wsl. Of the environment overrides only MIN_C_GB is
# forwarded across that re-exec (BASECAMP_OUT_DIR is not): set them inside WSL.
set -euo pipefail

BASECAMP_REV="e09962f5998792afd3bfed8c69c3b8f07f5825a5"
LEZ_CORE_REV="acf0cd501b262c4c15969e3735e85318297b85bf"
BASECAMP_FLAKE="github:logos-co/logos-basecamp/${BASECAMP_REV}#app-standalone"
LEZ_CORE_FLAKE="github:logos-blockchain/logos-execution-zone-module?rev=${LEZ_CORE_REV}#install"
OUT_DIR="${BASECAMP_OUT_DIR:-$HOME/lez-target/basecamp}"
MIN_C_GB="${MIN_C_GB:-10}"
# Where WSL mounts C: (overridable for tests). The WSL disk is a file on C:, so C: free space is what runs out.
C_MOUNT="${C_MOUNT:-/mnt/c}"

if ! grep -qi microsoft /proc/version 2>/dev/null && [[ ! -d /mnt/c ]] && command -v wsl >/dev/null 2>&1; then
  here="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
  win="$(cd "$here" && pwd -W)"
  wsl_here="/mnt/$(echo "${win:0:1}" | tr 'A-Z' 'a-z')${win:2}"
  # shellcheck disable=SC2016
  MSYS_NO_PATHCONV=1 exec wsl -d Ubuntu-20.04 -e bash -lc 'cd "$1" && shift && MIN_C_GB="$1" && shift && export MIN_C_GB && exec scripts/basecamp/host.sh "$@"' _ "$wsl_here" "$MIN_C_GB" "$@"
fi

# Whole GB, rounded down (df -B1G rounds up).
c_free_gb() { echo $(( $(df -k --output=avail "$C_MOUNT" | tail -1 | tr -dc '0-9') / 1048576 )); }

# On plain Linux (CI) there is no C: to protect, as in scripts/disk-guard.sh.
check() {
  local free
  if [[ ! -d "$C_MOUNT" ]]; then
    echo "no ${C_MOUNT} (not WSL): C: check skipped"
    return 0
  fi
  free="$(c_free_gb)"
  if (( free < MIN_C_GB )); then
    echo "refusing: C: has ${free} GB free, need ${MIN_C_GB}" >&2
    return 2
  fi
  echo "C: ${free} GB free (min ${MIN_C_GB}): ok"
}

build_one() { # <flake ref> <out link>
  check
  # Keeps the substituters (Logos cache) from ~/.config/nix/nix.conf; --accept-flake-config is not used.
  nix build "$1" --out-link "$2"
  check
}

cmd="${1:-}"
case "$cmd" in
  check) check ;;
  build)
    mkdir -p "$OUT_DIR"
    build_one "$BASECAMP_FLAKE" "$OUT_DIR/app"
    build_one "$LEZ_CORE_FLAKE" "$OUT_DIR/lez_core"
    ;;
  paths)
    printf 'BASECAMP_BIN=%q\n' "$OUT_DIR/app/bin/LogosBasecamp"
    printf 'LEZ_CORE_ROOT=%q\n' "$OUT_DIR/lez_core"
    ;;
  *) echo "usage: $0 check|build|paths" >&2; exit 64 ;;
esac
