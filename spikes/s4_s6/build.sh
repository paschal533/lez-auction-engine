#!/usr/bin/env bash
# Usage: build.sh <logname> <cargo args...>   (runs inside WSL with the S1 environment)
# Expects the repo layout (crates/auction_core next to spikes/s4_s6); the WSL copy lives at ~/lez-spikes/s4_s6/spikes/s4_s6.
cd "$(dirname "$0")" || exit 1
. "$HOME/lez-spikes/s1/env.sh"
export RISC0_DEV_MODE=1
mkdir -p logs
log="logs/$1.log"; shift
cargo "$@" > "$log" 2>&1
echo "EXIT $?" >> "$log"
