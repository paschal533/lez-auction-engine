#!/usr/bin/env bash
# Proves the exported hook series applies with `git am` to pristine lez-programs 64c01210 (ADR-025).
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
upstream="${LEZ_PROGRAMS:-$HOME/lez-spikes/s1/lez-programs}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
git -C "$upstream" archive 64c01210adecd6b557dbd021a3cc94553f243fd8 | tar -x -C "$tmp"
cd "$tmp"
git init -q && git add -A && git -c user.email=check@local -c user.name=check commit -qm pristine
shopt -s nullglob
patches=("$repo"/patches/stablecoin/*.patch)
if (( ${#patches[@]} == 0 )); then echo "check-hook-patches: no patches"; exit 0; fi
git -c user.email=check@local -c user.name=check am -q "${patches[@]}"
echo "check-hook-patches: ${#patches[@]} patch(es) apply cleanly"
# [local] commits are excluded from the exported series (export-hook-patches.sh) but their programs/stablecoin
# changes live in the repo tree, so replay them onto the temp tree before comparing (ADR-035). An un-exported
# change that is NOT in a [local] commit is still caught by the diff below.
paths=(programs/stablecoin ':(exclude)programs/stablecoin/methods_b' ':(exclude)programs/stablecoin/clippy.toml' ':(exclude)programs/stablecoin/rustfmt.toml')
base="$(git -C "$repo" log --format=%H -1 --grep='^vendor: lez-programs 64c01210')"
if [[ -z "$base" ]]; then echo "check-hook-patches: vendor commit not found" >&2; exit 1; fi
locals=0
for sha in $(git -C "$repo" rev-list --reverse "$base"..HEAD -- "${paths[@]}"); do
  git -C "$repo" log -1 --format=%s "$sha" | grep -q '\[local\]' || continue
  git -C "$repo" diff-tree -p --no-commit-id "$sha" -- "${paths[@]}" | git apply --index - \
    || { echo "check-hook-patches: [local] commit $sha does not apply" >&2; exit 1; }
  locals=$((locals + 1))
done
# Pre-commit verification only: CHECK_HOOK_PATCHES_STAGED=1 also replays the staged stablecoin diff.
if [[ "${CHECK_HOOK_PATCHES_STAGED:-0}" == 1 ]]; then
  git -C "$repo" diff --cached --binary -- "${paths[@]}" | git apply --index - \
    || { echo "check-hook-patches: staged diff does not apply" >&2; exit 1; }
  echo "check-hook-patches: also applied the staged stablecoin diff (pre-commit mode)"
fi
echo "check-hook-patches: replayed $locals [local] commit(s)"
# The patched pristine tree must equal the repo's stablecoin tree (catches a drifting export).
diff -r -x methods_b -x clippy.toml -x rustfmt.toml "$tmp/programs/stablecoin" "$repo/programs/stablecoin" \
  || { echo "check-hook-patches: patched tree differs from repo programs/stablecoin" >&2; exit 1; }
echo "check-hook-patches: patched tree matches programs/stablecoin"
