#!/usr/bin/env bash
# Builds our two guests reproducibly in Docker (ADR-047) into OUT (default $HOME/lez-target/plan3b-guests).
# Run inside WSL or Linux with Docker. The disk guard runs before (5 GiB, the Docker disk grows on C:)
# and after (3 GiB).
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
out="${1:-$HOME/lez-target/plan3b-guests}"
if [[ -x "$repo/scripts/disk-guard.sh" && -d /mnt/c ]]; then "$repo/scripts/disk-guard.sh" 5; fi
staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
DOCKER_BUILDKIT=1 docker build --output="$staging" -f "$repo/scripts/build-guests.Dockerfile" "$repo"
mkdir -p "$out"
install -m 0644 "$staging/stablecoin.bin" "$out/stablecoin.bin"
install -m 0644 "$staging/auction_engine.bin" "$out/auction_engine.bin"
sha256sum "$out"/*.bin
if [[ -x "$repo/scripts/disk-guard.sh" && -d /mnt/c ]]; then "$repo/scripts/disk-guard.sh"; fi
