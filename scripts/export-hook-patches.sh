#!/usr/bin/env bash
# Exports the stablecoin hook patch series (ADR-025): one patch per commit after the vendor commit that
# touches programs/stablecoin/, skipping commits whose subject contains "[local]". Paths are identical to
# upstream's, so the patches apply with `git am` at the lez-programs root.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
base="$(git log --format=%H -1 --grep='^vendor: lez-programs 64c01210')"
if [[ -z "$base" ]]; then echo "export-hook-patches: vendor commit not found" >&2; exit 1; fi
out=patches/stablecoin
mkdir -p "$out"; rm -f "$out"/*.patch
n=0
for sha in $(git rev-list --reverse "$base"..HEAD -- programs/stablecoin); do
  if git log -1 --format=%s "$sha" | grep -q '\[local\]'; then continue; fi
  n=$((n + 1))
  git format-patch -1 "$sha" --start-number "$n" -o "$out" --quiet -- programs/stablecoin \
    ':(exclude)programs/stablecoin/methods_b' ':(exclude)programs/stablecoin/clippy.toml' ':(exclude)programs/stablecoin/rustfmt.toml'
done
echo "export-hook-patches: $n patch(es) in $out"
