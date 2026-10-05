#!/usr/bin/env bash
# Usage: build.sh <logname> <cargo args...>   (runs inside WSL with the S1 environment)
cd "$HOME/lez-spikes/s2_s3_s7" || exit 1
. "$HOME/lez-spikes/s1/env.sh"
export RISC0_DEV_MODE=1
log="logs/$1.log"; shift
cargo "$@" > "$log" 2>&1
echo "EXIT $?" >> "$log"
