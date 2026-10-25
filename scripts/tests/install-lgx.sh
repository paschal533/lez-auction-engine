#!/usr/bin/env bash
# Plan 4 Task 17: scripts/basecamp/install-lgx.sh builds the three .lgx packages, the full Basecamp and lgpm,
# installs core modules first and the UI plugin second into PROFILE_DIR, refuses a profile whose lez_core is not
# the pinned 0.4.1 (design call 3), and hands the launch to run-app.sh --profile. Stubs `nix`, `df` and lgpm
# (the stub nix writes an lgpm script into the lgpm out link); RUN_APP points at a stub run-app.sh.
set -uo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fails=0
kib_100g=104857600
kib_2g=2097152
mkdir -p "$tmp/bin"
stub() { printf '#!/usr/bin/env bash\n%s\n' "$2" > "$tmp/bin/$1"; chmod +x "$tmp/bin/$1"; }

# nix build <ref> [flags] --out-link <link>: logs the ref; an lgx ref gets a fake .lgx, the app a fake binary,
# the package manager an lgpm that logs its arguments and writes a manifest like the real one.
cat > "$tmp/bin/nix" <<'EOF'
#!/usr/bin/env bash
ref="$2"; link=""
while (( $# )); do [[ "$1" == "--out-link" ]] && link="$2"; shift; done
echo "$ref" >> "$STUB_LOG/nix.log"
mkdir -p "$link"
case "$ref" in
  *logos-execution-zone-module*'#lgx') touch "$link/logos-lez_core-module-lib.lgx" ;;
  *auction_engine'#lgx') touch "$link/logos-auction_engine-module-lib.lgx" ;;
  *apps/auctions'#lgx') touch "$link/logos-auctions-module.lgx" ;;
  *logos-basecamp*'#app') mkdir -p "$link/bin"; printf '#!/bin/sh\n' > "$link/bin/LogosBasecamp"; chmod +x "$link/bin/LogosBasecamp" ;;
  *logos-package-manager*'#cli')
    mkdir -p "$link/bin"
    cat > "$link/bin/lgpm" <<'LGPM'
#!/usr/bin/env bash
echo "$*" >> "$STUB_LOG/lgpm.log"
dir=""; file=""
while (( $# )); do
  case "$1" in --modules-dir|--ui-plugins-dir) dir="$2"; shift ;; --file) file="$2"; shift ;; esac
  shift
done
name="$(basename "$file" .lgx)"; name="${name#logos-}"; name="${name%-module-lib}"; name="${name%-module}"
version=0.1.0
[[ "$name" == lez_core ]] && version="${STUB_LEZ_VERSION:-0.4.1}"
mkdir -p "$dir/$name"
printf '{\n  "manifestVersion": "0.3.0",\n  "name": "%s",\n  "version": "%s"\n}\n' "$name" "$version" > "$dir/$name/manifest.json"
LGPM
    chmod +x "$link/bin/lgpm" ;;
  *) echo "stub nix: unexpected ref $ref" >&2; exit 1 ;;
esac
EOF
chmod +x "$tmp/bin/nix"
cat > "$tmp/run-app.sh" <<'EOF'
#!/usr/bin/env bash
echo "BASECAMP_FULL_BIN=$BASECAMP_FULL_BIN $*" >> "$STUB_LOG/run-app.log"
EOF
chmod +x "$tmp/run-app.sh"

# run_case <name> <want exit> <text in output> <env...> -- <install-lgx args...>
run_case() {
  local name="$1" want="$2" text="$3" out got envs=()
  shift 3
  while (( $# )) && [[ "$1" != "--" ]]; do envs+=("$1"); shift; done
  shift
  rm -rf "$tmp/log" "$tmp/out"; mkdir -p "$tmp/log"
  out="$(env PATH="$tmp/bin:$PATH" STUB_LOG="$tmp/log" BASECAMP_OUT_DIR="$tmp/out" RUN_APP="$tmp/run-app.sh" \
    "${envs[@]}" "$repo/scripts/basecamp/install-lgx.sh" "$@" 2>&1)"
  got=$?
  if [[ "$got" != "$want" || "$out" != *"$text"* ]]; then
    echo "FAIL $name: exit $got (want $want), output: $out"
    fails=$((fails + 1))
    return 1
  fi
  echo "ok   $name"
}
expect_log() { # <name> <file> <expected content>
  local got
  got="$(cat "$tmp/log/$2" 2>/dev/null)"
  if [[ "$got" != "$3" ]]; then
    echo "FAIL $1: $2 was:"; echo "$got"; echo "want:"; echo "$3"
    fails=$((fails + 1))
  else
    echo "ok   $1"
  fi
}

stub df "echo Avail; echo $kib_100g"
run_case usage 2 "usage: scripts/basecamp/install-lgx.sh PROFILE_DIR" --

p="$tmp/profile"
run_case install_only 0 "installed into $p" -- "$p" --install-only
out="$tmp/out"
expect_log builds_pinned_refs nix.log "github:logos-co/logos-package-manager/30b3a3f8a4f05d99e6b377fb40e02018c101882b#cli
github:logos-blockchain/logos-execution-zone-module?rev=acf0cd501b262c4c15969e3735e85318297b85bf#lgx
$repo/modules/auction_engine#lgx
$repo/apps/auctions#lgx
github:logos-co/logos-basecamp/e09962f5998792afd3bfed8c69c3b8f07f5825a5#app"
expect_log core_first_then_ui lgpm.log "--modules-dir $p/modules install --file $out/lez_core-lgx/logos-lez_core-module-lib.lgx
--modules-dir $p/modules install --file $out/auction_engine-lgx/logos-auction_engine-module-lib.lgx
--ui-plugins-dir $p/plugins install --file $out/auctions-lgx/logos-auctions-module.lgx"
[[ ! -e "$tmp/log/run-app.log" ]] && echo "ok   install_only_does_not_launch" || { echo "FAIL install_only_does_not_launch"; fails=$((fails + 1)); }

# Re-running on the same profile (lez_core 0.4.1 already there) is fine and launches through run-app.sh.
run_case reinstall_launches 0 "" -- "$p" --setup --test story.mjs
expect_log launch_through_run_app run-app.log "BASECAMP_FULL_BIN=$out/app-full/bin/LogosBasecamp --profile $p --setup --test story.mjs"

# Design call 3: a profile that already has a newer lez_core (e.g. from the catalogue) is unsupported.
q="$tmp/newer"
mkdir -p "$q/modules/lez_core"
printf '{\n  "name": "lez_core",\n  "version": "0.5.2"\n}\n' > "$q/modules/lez_core/manifest.json"
run_case newer_lez_core_refused 4 "lez_core 0.5.2" -- "$q" --install-only
[[ ! -e "$tmp/log/lgpm.log" ]] && echo "ok   newer_lez_core_installs_nothing" || { echo "FAIL newer_lez_core_installs_nothing"; fails=$((fails + 1)); }

# If lgpm leaves a different lez_core in place, the install is refused before launching.
run_case installed_version_checked 4 "lez_core 0.5.0" STUB_LEZ_VERSION=0.5.0 -- "$tmp/fresh" --setup

stub df "echo Avail; echo $kib_2g"
run_case low_disk 2 "refusing: C: has 2 GB free" -- "$tmp/low" --install-only
[[ ! -e "$tmp/log/nix.log" ]] && echo "ok   low_disk_builds_nothing" || { echo "FAIL low_disk_builds_nothing"; fails=$((fails + 1)); }

if (( fails == 0 )); then echo "install-lgx: all cases pass"; else echo "install-lgx: $fails case(s) failed"; exit 1; fi
