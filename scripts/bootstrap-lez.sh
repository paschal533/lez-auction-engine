#!/usr/bin/env bash
# Builds the LEZ v0.2.4 sequencer and wallet the localnet scripts need (demo.sh, localnet-smoke.sh, CI).
# Idempotent: skips what exists. LEZ_DEPS defaults like scripts/lib/localnet.sh: the S1 builds when they
# exist (the author's machine; this script never modifies them), else $HOME/lez-deps. A clean machine's
# $LEZ_DEPS/lez/target is the one cargo target outside the shared plan2 directory; it needs several GB.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
if [[ -z "${LEZ_DEPS:-}" ]]; then
  if [[ -x "$HOME/lez-spikes/s1/lez/target/debug/sequencer_service" ]]; then LEZ_DEPS="$HOME/lez-spikes/s1"; else LEZ_DEPS="$HOME/lez-deps"; fi
fi
LEZ_REV=47eba256479f6f785acbd138834340703cd03401
SEQ_CONFIG=lez/sequencer/service/configs/debug/sequencer_config.json
WALLET_CONFIG=lez/wallet/configs/debug/wallet_config.json
# The binaries and both configs: a warm CI cache restores the binaries only, so the configs decide too.
if [[ -x "$LEZ_DEPS/lez/target/debug/sequencer_service" && -x "$LEZ_DEPS/lez/target/debug/wallet" \
  && -f "$LEZ_DEPS/lez/$SEQ_CONFIG" && -f "$LEZ_DEPS/lez/$WALLET_CONFIG" ]]; then
  echo "bootstrap-lez: sequencer_service, wallet and their configs already present under $LEZ_DEPS/lez"
  exit 0
fi
if [[ "$LEZ_DEPS" == "$HOME/lez-spikes/s1" ]]; then
  echo "bootstrap-lez: S1 is incomplete; set LEZ_DEPS to another directory (this script never modifies S1)" >&2
  exit 4
fi
need() {
  command -v "$1" >/dev/null || {
    echo "bootstrap-lez: missing $1. On Ubuntu: sudo apt-get install -y build-essential clang libclang-dev pkg-config libssl-dev libpcsclite-dev python3 git curl; Rust via rustup (rust-toolchain.toml pins 1.94.0)" >&2
    exit 4
  }
}
for t in git cargo clang pkg-config python3; do need "$t"; done
"$repo_root/scripts/disk-guard.sh" "${DEMO_MIN_C_GIB:-3}" "${DEMO_MIN_HOME_GIB:-20}"
mkdir -p "$LEZ_DEPS/lez"
cd "$LEZ_DEPS/lez"
# init + fetch works in a directory that already holds a cached target/ (CI restores the binaries only).
# Always run, before the build-skip tests below: it restores the configs a warm cache lacks.
if [[ ! -d .git ]]; then
  git init -q
  git remote add origin https://github.com/logos-blockchain/logos-execution-zone
fi
git fetch -q --depth 1 origin "$LEZ_REV"
git checkout -q FETCH_HEAD
test "$(git rev-parse HEAD)" = "$LEZ_REV"
# shellcheck disable=SC1091
if [[ -f "$LEZ_DEPS/env.sh" ]]; then . "$LEZ_DEPS/env.sh"; fi
[[ -x target/debug/sequencer_service ]] || env -u CARGO_TARGET_DIR cargo build --locked --features standalone -p sequencer_service
[[ -x target/debug/wallet ]] || env -u CARGO_TARGET_DIR cargo build --locked -p wallet
echo "bootstrap-lez: sequencer_service and wallet ready under $LEZ_DEPS/lez/target/debug"
