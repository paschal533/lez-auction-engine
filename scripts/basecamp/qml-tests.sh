#!/usr/bin/env bash
# QML unit tests for the Auctions app (Plan 4 Task 5).
#   scripts/basecamp/qml-tests.sh [dir-or-file ...]   default: every test in apps/auctions/tests/qml
# Runs qmltestrunner (Qt from the nixpkgs the pinned logos-module-builder uses) offscreen against the design
# system Basecamp master ships (88330d04). Runs inside WSL; from Git Bash it re-runs itself through wsl.
set -euo pipefail

DESIGN_SYSTEM_REV="88330d04968954be956311b0be429048a41da2ea"

if ! grep -qi microsoft /proc/version 2>/dev/null && [[ ! -d /mnt/c ]] && command -v wsl >/dev/null 2>&1; then
  here="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
  win="$(cd "$here" && pwd -W)"
  wsl_here="/mnt/$(echo "${win:0:1}" | tr 'A-Z' 'a-z')${win:2}"
  MSYS_NO_PATHCONV=1 exec wsl -d Ubuntu-20.04 -e bash -lc 'cd "$1" && shift && exec scripts/basecamp/qml-tests.sh "$@"' _ "$wsl_here" "$@"
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
cd "$root"

# Qt must be the one the app runs on: the nixpkgs the pinned logos-module-builder uses (not the root flake's).
nixpkgs_rev="$(python3 -c "
import json
d = json.load(open('apps/auctions/flake.lock'))
n = d['nodes']
def resolve(ref):
    if isinstance(ref, str):
        return ref
    cur = d['root']
    for key in ref:
        cur = resolve(n[cur]['inputs'][key])
    return cur
builder = resolve(n[d['root']]['inputs']['logos-module-builder'])
print(n[resolve(n[builder]['inputs']['nixpkgs'])]['locked']['rev'])")"

# The Qt builds below may download or build; never start them with C: below host.sh's floor.
scripts/basecamp/host.sh check >&2
qt_decl="$(nix build "github:NixOS/nixpkgs/${nixpkgs_rev}#qt6.qtdeclarative" --no-link --print-out-paths | head -1)"
qt_svg="$(nix build "github:NixOS/nixpkgs/${nixpkgs_rev}#qt6.qtsvg" --no-link --print-out-paths | head -1)"
design_src="$(nix flake prefetch "github:logos-co/logos-design-system/${DESIGN_SYSTEM_REV}" --json \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['storePath'])")"

export QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software QT_QUICK_CONTROLS_STYLE=Basic
export QT_PLUGIN_PATH="${qt_svg}/lib/qt-6/plugins"
# Tests read the generated JSON fixtures (tests/qml/Fixtures.js) through a file:// XMLHttpRequest.
export QML_XHR_ALLOW_FILE_READ=1

inputs=("$@")
[[ ${#inputs[@]} -gt 0 ]] || inputs=(apps/auctions/tests/qml)
args=()
for i in "${inputs[@]}"; do args+=(-input "$i"); done

exec "${qt_decl}/bin/qmltestrunner" \
  -import "${qt_decl}/lib/qt-6/qml" \
  -import "${design_src}/src/qml" \
  -import apps/auctions/qml \
  "${args[@]}"
