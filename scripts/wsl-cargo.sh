#!/usr/bin/env bash
# Runs one cargo command inside WSL for this repository (Plan 2 build convention).
# From Git Bash:
#   MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc \
#     'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_core'
# The target directory lives on the WSL-native disk; reading sources over /mnt/c is slow, writing targets there is slower.
set -euo pipefail
exec "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)/wsl-env.sh" cargo "$@"
