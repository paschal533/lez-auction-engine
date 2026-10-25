#!/usr/bin/env bash
# Prints the hashes SUBMISSION.md records (spec §1.2). Run from the repository root.
# Fails (non-zero, "submission-hashes: ...") if a hashed file is missing or a committed guest binary does not
# match artifacts/guests/manifest.json.
set -euo pipefail
shopt -s nullglob

fail() { echo "submission-hashes: $*" >&2; exit 1; }
# Prints the sha256 of one file; callers assign it to a variable first so a failure aborts under set -e.
sha() { local out; out="$(sha256sum "$1")" || fail "cannot hash $1"; printf '%s' "${out%% *}"; }

guest_bins=(artifacts/guests/*.bin)
patches=(patches/stablecoin/*.patch)
evidence=(REQUIREMENTS.md CYCLES.md cycles/baseline.json DEMO.md THREAT_MODEL.md docs/security/redteam-2026-10.md docs/architecture/real-proofs.md docs/architecture/basecamp-app.md)
evidence_extra=(docs/evidence/*.md)

[[ -f artifacts/guests/manifest.json ]] || fail "missing artifacts/guests/manifest.json"
(( ${#guest_bins[@]} > 0 )) || fail "missing artifacts/guests/*.bin"
(( ${#patches[@]} > 0 )) || fail "missing patches/stablecoin/*.patch"
for f in "${evidence[@]}"; do [[ -f $f ]] || fail "missing $f"; done

echo "## Pins"
echo "- LEZ v0.2.4 47eba256479f6f785acbd138834340703cd03401; SPEL 1ef0500f8fc8ce3ddf95523726ae159db83f744f; risc0-zkvm 3.0.5; Rust 1.94.0; lez-programs 64c01210adecd6b557dbd021a3cc94553f243fd8"
echo
echo "## Deployable guests (artifacts/guests/manifest.json)"
# The sha256 of every committed binary is recomputed here and compared with the manifest; ImageIDs are echoed
# from the manifest (the CI guest-repro job rebuilds the binaries and checks them).
python3 - "${guest_bins[@]}" <<'PY'
import hashlib, json, os, sys
m = json.load(open("artifacts/guests/manifest.json"))
bins = {os.path.splitext(os.path.basename(p))[0]: p for p in sys.argv[1:]}
if set(bins) != set(m["guests"]):
    sys.exit(f"submission-hashes: binaries {sorted(bins)} do not match manifest guests {sorted(m['guests'])}")
for name, g in sorted(m["guests"].items()):
    actual = hashlib.sha256(open(bins[name], "rb").read()).hexdigest()
    if actual != g["sha256"]:
        sys.exit(f"submission-hashes: {bins[name]} sha256 {actual} differs from the manifest {g['sha256']}")
print(f"- builder: `{m['builder_image']}`")
print("- binaries verified against the manifest: sha256 recomputed for every artifacts/guests/*.bin; ImageIDs are echoed from the manifest")
for name, g in sorted(m["guests"].items()):
    print(f"- {name}: ImageID `{g['image_id']}`, sha256 `{g['sha256']}`")
PY
echo
echo "## Stablecoin hook patch series (patches/stablecoin)"
for p in "${patches[@]}"; do h="$(sha "$p")"; echo "- $(basename "$p"): sha256 \`$h\`"; done
echo
echo "## Evidence files"
for f in "${evidence[@]}" "${evidence_extra[@]}"; do h="$(sha "$f")"; echo "- $f: sha256 \`$h\`"; done
