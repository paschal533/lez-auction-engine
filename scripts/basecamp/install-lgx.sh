#!/usr/bin/env bash
# Installs the Auctions app as .lgx packages into a clean Basecamp profile and runs it in the full Basecamp
# (Plan 4 Task 17): the way a user gets it, instead of standalone mode (run-app.sh without --profile).
#   scripts/basecamp/install-lgx.sh PROFILE_DIR [--install-only] [run-app.sh options, e.g. --setup --test FILE]
# Builds the .#lgx packages of lez_core acf0cd50, modules/auction_engine and apps/auctions, the full Basecamp
# (.#app at BASECAMP_REV) and lgpm (the package manager Basecamp pins). Installs the core modules first
# (lgpm --modules-dir PROFILE_DIR/modules) and the UI plugin second (lgpm --ui-plugins-dir PROFILE_DIR/plugins),
# then hands over to `run-app.sh --profile PROFILE_DIR …`, which starts the full Basecamp with
# --user-dir PROFILE_DIR against the localnet. --install-only stops after the install.
# Exit 4: the profile has a lez_core other than 0.4.1, e.g. a newer one from Basecamp's catalogue. The app is
# built against lez_core 0.4.1 (LEZ v0.2.4); that profile is unsupported, use a fresh PROFILE_DIR.
# Use a Linux path for PROFILE_DIR (e.g. /tmp/auctions-profile), not one under /mnt/c.
# Exit 4 also when PROFILE_DIR/modules/lez_core/manifest.json exists but has no readable version (fails closed).
# Overrides: BASECAMP_OUT_DIR (result links, default ~/lez-target/basecamp; run-app.sh honours it too), MIN_C_GB,
# RUN_APP.
# Runs inside WSL; from Git Bash it re-runs itself through wsl. Environment overrides are not forwarded across
# that re-exec, so with STUB_LOG or RUN_APP set (scripts/tests/install-lgx.sh) it refuses instead.
set -euo pipefail

BASECAMP_REV="e09962f5998792afd3bfed8c69c3b8f07f5825a5"
# logos-package-manager as locked by Basecamp at BASECAMP_REV (its lgpm writes what that Basecamp reads).
PACKAGE_MANAGER_REV="30b3a3f8a4f05d99e6b377fb40e02018c101882b"
LEZ_CORE_REV="acf0cd501b262c4c15969e3735e85318297b85bf"
LEZ_CORE_VERSION="0.4.1"

if ! grep -qi microsoft /proc/version 2>/dev/null && [[ ! -d /mnt/c ]] && command -v wsl >/dev/null 2>&1; then
  if [[ -n "${STUB_LOG:-}" || -n "${RUN_APP:-}" ]]; then
    echo "install-lgx: STUB_LOG or RUN_APP is set, and the WSL re-exec would drop it; run inside WSL" >&2
    exit 2
  fi
  here="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
  win="$(cd "$here" && pwd -W)"
  wsl_here="/mnt/$(echo "${win:0:1}" | tr 'A-Z' 'a-z')${win:2}"
  # shellcheck disable=SC2016
  MSYS_NO_PATHCONV=1 exec wsl -d Ubuntu-20.04 -e bash -lc 'cd "$1" && shift && exec scripts/basecamp/install-lgx.sh "$@"' _ "$wsl_here" "$@"
fi

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
usage() { echo "usage: scripts/basecamp/install-lgx.sh PROFILE_DIR [--install-only] [run-app.sh options]" >&2; exit 2; }
[[ $# -ge 1 && -n "$1" && "$1" != -* ]] || usage
profile="$1"; shift
mkdir -p "$profile"
profile="$(cd "$profile" && pwd -P)"
install_only=0
if [[ "${1:-}" == "--install-only" ]]; then install_only=1; shift; fi

out="${BASECAMP_OUT_DIR:-$HOME/lez-target/basecamp}"
mkdir -p "$out"
export BASECAMP_OUT_DIR="$out"

# The top-level "version" of a manifest.json; prints nothing when none can be read. python3 reads the JSON;
# without it, a regex over the whole file, accepted only when every "version" in it agrees.
manifest_version() { # <manifest.json>
  if command -v python3 >/dev/null 2>&1; then
    python3 -c 'import json, sys
try:
    v = json.load(open(sys.argv[1])).get("version")
except Exception:
    v = None
print(v if isinstance(v, str) else "")' "$1"
    return 0
  fi
  local all
  all="$(tr -d '\n' < "$1" | grep -oE '"version"[[:space:]]*:[[:space:]]*"[^"]*"' \
    | sed -E 's/.*"([^"]*)"$/\1/' | sort -u)"
  [[ -n "$all" && "$all" != *$'\n'* ]] && echo "$all"
  return 0
}
# Fails closed: a lez_core whose manifest has no readable version is refused like a newer one.
require_pinned_lez_core() {
  local m="$profile/modules/lez_core/manifest.json" v
  [[ -e "$m" ]] || return 0
  v="$(manifest_version "$m")"
  [[ "$v" == "$LEZ_CORE_VERSION" ]] && return 0
  if [[ -z "$v" ]]; then
    echo "install-lgx: $m has no readable version; the Auctions app needs lez_core $LEZ_CORE_VERSION" \
      "(rev ${LEZ_CORE_REV:0:8}). That profile is unsupported: use a fresh PROFILE_DIR." >&2
    exit 4
  fi
  echo "install-lgx: $profile has lez_core $v; the Auctions app needs lez_core $LEZ_CORE_VERSION" \
    "(rev ${LEZ_CORE_REV:0:8}, LEZ v0.2.4). That profile is unsupported: use a fresh PROFILE_DIR." >&2
  exit 4
}
require_pinned_lez_core

# One flake output to a result link under $out, left in $built. C: is checked before each build; build runs at
# the top level (not in a command substitution, where set -e is off), so a refused check stops the script.
build() { # <flake ref> <link name>
  "$repo/scripts/basecamp/host.sh" check
  nix build "$1" --max-jobs 1 --cores 2 --out-link "$out/$2" >&2
  built="$out/$2"
}
# The one .lgx file a package output holds.
lgx_in() {
  local f
  f="$(find -L "$1" -maxdepth 1 -name '*.lgx' | head -1)"
  [[ -n "$f" ]] || { echo "install-lgx: no .lgx in $1" >&2; exit 1; }
  echo "$f"
}

build "github:logos-co/logos-package-manager/${PACKAGE_MANAGER_REV}#cli" lgpm
lgpm="$built/bin/lgpm"
build "github:logos-blockchain/logos-execution-zone-module?rev=${LEZ_CORE_REV}#lgx" lez_core-lgx
lez_core_lgx="$(lgx_in "$built")"
build "$repo/modules/auction_engine#lgx" auction_engine-lgx
engine_lgx="$(lgx_in "$built")"
build "$repo/apps/auctions#lgx" auctions-lgx
auctions_lgx="$(lgx_in "$built")"
build "github:logos-co/logos-basecamp/${BASECAMP_REV}#app" app-full
basecamp_bin="$built/bin/LogosBasecamp"
"$repo/scripts/basecamp/host.sh" check

# Core modules first (the UI plugin depends on both), then the UI plugin. The packages are unsigned dev builds.
"$lgpm" --modules-dir "$profile/modules" install --file "$lez_core_lgx"
"$lgpm" --modules-dir "$profile/modules" install --file "$engine_lgx"
"$lgpm" --ui-plugins-dir "$profile/plugins" install --file "$auctions_lgx"
require_pinned_lez_core
echo "install-lgx: installed into $profile (lez_core $LEZ_CORE_VERSION, auction_engine, auctions)"

(( install_only )) && exit 0
export BASECAMP_FULL_BIN="$basecamp_bin"
exec "${RUN_APP:-$repo/scripts/basecamp/run-app.sh}" --profile "$profile" "$@"
