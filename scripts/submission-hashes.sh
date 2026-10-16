#!/usr/bin/env bash
# Prints the hashes SUBMISSION.md records (spec §1.2). Run from the repository root.
set -euo pipefail
echo "## Pins"
echo "- LEZ v0.2.4 47eba256479f6f785acbd138834340703cd03401; SPEL 1ef0500f8fc8ce3ddf95523726ae159db83f744f; risc0-zkvm 3.0.5; Rust 1.94.0; lez-programs 64c01210adecd6b557dbd021a3cc94553f243fd8"
echo
echo "## Deployable guests (artifacts/guests/manifest.json)"
python3 - <<'PY'
import json
m = json.load(open("artifacts/guests/manifest.json"))
print(f"- builder: `{m['builder_image']}`")
for name, g in sorted(m["guests"].items()):
    print(f"- {name}: ImageID `{g['image_id']}`, sha256 `{g['sha256']}`")
PY
echo
echo "## Stablecoin hook patch series (patches/stablecoin)"
for p in patches/stablecoin/*.patch; do echo "- $(basename "$p"): sha256 \`$(sha256sum "$p" | cut -d' ' -f1)\`"; done
echo
echo "## Evidence files"
for f in REQUIREMENTS.md CYCLES.md cycles/baseline.json DEMO.md THREAT_MODEL.md docs/security/redteam-2026-10.md docs/architecture/real-proofs.md; do
  echo "- $f: sha256 \`$(sha256sum "$f" | cut -d' ' -f1)\`"
done
if compgen -G "docs/evidence/*.md" > /dev/null; then
  for f in docs/evidence/*.md; do echo "- $f: sha256 \`$(sha256sum "$f" | cut -d' ' -f1)\`"; done
fi
