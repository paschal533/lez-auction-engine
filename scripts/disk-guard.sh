#!/usr/bin/env bash
# Stops a task before it fills a disk (Plan 3b Global Constraints). Run inside WSL.
#   scripts/disk-guard.sh [min_c_gib=3] [min_wsl_gib=20]
# Exit 0: enough space. Exit 3: stop (says which disk and what is safe to delete).
# Also used by the localnet scripts on plain Linux (no /mnt/c). The WSL ext4 disk and the Docker Desktop disk are files on C:, so C: free space is what runs out.
set -euo pipefail
min_c_gib="${1:-3}"
min_wsl_gib="${2:-20}"
# Whole GiB, rounded down (df -BG rounds up, which would pass 2.1 GiB as 3).
avail_gib() { echo $(( $(df -k --output=avail "$1" | tail -1 | tr -dc '0-9') / 1048576 )); }
# On plain Linux (CI) there is no /mnt/c: only the home disk is checked.
c="$min_c_gib"
if [[ -d /mnt/c ]]; then c="$(avail_gib /mnt/c)"; fi
w="$(avail_gib "$HOME")"
echo "disk-guard: C: ${c} GiB free (min ${min_c_gib}), home ${w} GiB free (min ${min_wsl_gib})"
if (( c < min_c_gib )); then
  echo "disk-guard: STOP: C: has ${c} GiB free (< ${min_c_gib} GiB); the WSL and Docker disks grow into it." >&2
  echo "disk-guard: safe to delete: \$HOME/lez-target/plan2/llvm-cov-target, \$HOME/lez-target/plan2/fuzz," \
    "\$HOME/lez-target/plan3b-proofs/*.log, docker build cache of this repo" \
    "(docker builder prune --filter type=exec.cachemount). Never delete \$HOME/lez-spikes/s1." >&2
  exit 3
fi
if (( w < min_wsl_gib )); then
  echo "disk-guard: STOP: the WSL home has ${w} GiB free (< ${min_wsl_gib} GiB)." >&2
  exit 3
fi
