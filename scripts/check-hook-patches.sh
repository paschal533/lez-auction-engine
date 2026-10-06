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
