#!/usr/bin/env bash
# Hosted runners start with about 14 GB free; the risc0 guests, LEZ and our target need more.
# Removes preinstalled SDKs this repository never uses. CI only.
set -euo pipefail
df -h /
sudo rm -rf /usr/share/dotnet /usr/local/lib/android /opt/ghc /opt/hostedtoolcache/CodeQL /usr/local/share/boost
df -h /
