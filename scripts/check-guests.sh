#!/usr/bin/env bash
# Verifies (default) or writes (--write) artifacts/guests/manifest.json: sha256 and ImageID of every
# committed guest binary (ADR-047). --sha-only skips the ImageIDs (no cargo; used by demo preflight).
#   scripts/check-guests.sh [--write] [--sha-only] [dir=artifacts/guests]
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
write=0; sha_only=0
while [[ "${1:-}" == --* ]]; do
  case "$1" in --write) write=1 ;; --sha-only) sha_only=1 ;; *) echo "unknown flag $1" >&2; exit 2 ;; esac
  shift
done
dir="${1:-$repo/artifacts/guests}"
manifest="$dir/manifest.json"
ids=""
if (( sha_only == 0 )); then
  ids="$(cd "$repo" && cargo run -q --locked -p engine_integration_tests --example localnet_tool -- image-id "$dir"/*.bin)"
fi
python3 - "$dir" "$manifest" "$write" "$sha_only" "$ids" <<'PY'
import hashlib, json, os, sys
d, manifest, write, sha_only, ids = sys.argv[1], sys.argv[2], sys.argv[3] == "1", sys.argv[4] == "1", sys.argv[5]
image = dict(line.split() for line in ids.splitlines() if line.strip())
found = {}
for f in sorted(os.listdir(d)):
    if f.endswith(".bin"):
        name = f[:-4]
        found[name] = {"sha256": hashlib.sha256(open(os.path.join(d, f), "rb").read()).hexdigest()}
        if name in image:
            found[name]["image_id"] = image[name]
if write:
    m = json.load(open(manifest)) if os.path.exists(manifest) else {"builder_image": "", "guests": {}}
    for name, v in found.items():
        m["guests"].setdefault(name, {}).update(v)
    json.dump(m, open(manifest, "w"), indent=2, sort_keys=True)
    open(manifest, "a").write("\n")
    print(f"check-guests: wrote {manifest}")
    sys.exit(0)
m = json.load(open(manifest))
errors = []
for name, want in m["guests"].items():
    got = found.get(name)
    if got is None:
        errors.append(f"{name}: in the manifest but no {name}.bin")
        continue
    if got["sha256"] != want["sha256"]:
        errors.append(f"{name}: sha256 {got['sha256']} != manifest {want['sha256']}")
    if not sha_only and got.get("image_id") != want["image_id"]:
        errors.append(f"{name}: ImageID {got.get('image_id')} != manifest {want['image_id']}")
for name in found:
    if name not in m["guests"]:
        errors.append(f"{name}.bin is not in the manifest")
for e in errors:
    print("check-guests: " + e, file=sys.stderr)
sys.exit(1 if errors else 0)
PY
