#!/usr/bin/env bash
# Spike S1 - reproducible command record. Run INSIDE WSL (Ubuntu 24.04, distro named Ubuntu-20.04).
# Sources live in the WSL-native filesystem, NOT in this repo (the repo is on /mnt/c: slow 9P + CRLF).
# This script is a runbook: run the sections one at a time (steps 3-4 need a running sequencer, and
# the guest build takes ~70 min on a cold Docker cache). Nothing here changes a pinned SHA/version.
set -euo pipefail

S1="$HOME/lez-spikes/s1"
REPO_S1_DIR="${REPO_S1_DIR:-/mnt/c/Users/user/Documents/basecamp/lez-auction-engine/spikes/s1}"
LEZ_SHA=47eba256479f6f785acbd138834340703cd03401        # logos-execution-zone == tag v0.2.4
PROGRAMS_SHA=64c01210adecd6b557dbd021a3cc94553f243fd8   # lez-programs
SPEL_SHA=1ef0500f8fc8ce3ddf95523726ae159db83f744f       # spel, the rev the stablecoin guest pins

step0_toolchain() {
  # rzup (risc0 installer), user-local. Versions copied from lez/.github/docker/ci.Dockerfile.
  curl -sSL https://risczero.com/install | bash
  export PATH="$HOME/.risc0/bin:$PATH"
  rzup install rust 1.94.1 && rzup install cpp 2024.1.5 && rzup install r0vm 3.0.5 && rzup install cargo-risczero 3.0.5
  # No root: system libs are fetched as .deb and unpacked into $S1/sysroot (see env.sh below).
  mkdir -p "$S1/sysroot/debs" && (cd "$S1/sysroot/debs" && apt-get download libpcsclite-dev libpcsclite1 pkgconf pkgconf-bin libpkgconf3)
  for d in "$S1"/sysroot/debs/*.deb; do dpkg-deb -x "$d" "$S1/sysroot/root"; done
  ln -sf pkgconf "$S1/sysroot/root/usr/bin/pkg-config"
  pip install --user --break-system-packages libclang   # bindgen needs a libclang compatible with host glibc
  cat > "$S1/env.sh" <<'XEOF'
export PATH="$HOME/.risc0/bin:$HOME/.cargo/bin:$PATH"
export LIBCLANG_PATH=$HOME/.local/lib/python3.12/site-packages/clang/native
export BINDGEN_EXTRA_CLANG_ARGS="-isystem /usr/lib/gcc/x86_64-linux-gnu/13/include"
export LIBRARY_PATH=$HOME/lez-spikes/s1/sysroot/root/usr/lib/x86_64-linux-gnu${LIBRARY_PATH:+:$LIBRARY_PATH}
export PATH=$HOME/lez-spikes/s1/sysroot/root/usr/bin:$PATH
export PKG_CONFIG_PATH=$HOME/lez-spikes/s1/sysroot/root/usr/lib/x86_64-linux-gnu/pkgconfig
export PKG_CONFIG_SYSROOT_DIR=$HOME/lez-spikes/s1/sysroot/root
export LD_LIBRARY_PATH=$HOME/lez-spikes/s1/sysroot/root/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
XEOF
}

step1_clone() {
  mkdir -p "$S1/logs" && cd "$S1"
  git clone https://github.com/logos-blockchain/logos-execution-zone lez && git -C lez checkout "$LEZ_SHA"
  git clone https://github.com/logos-blockchain/lez-programs && git -C lez-programs checkout "$PROGRAMS_SHA"
  git clone https://github.com/logos-co/spel spel && git -C spel checkout "$SPEL_SHA"
}

step2_guests_and_tests() {
  . "$S1/env.sh"; cd "$S1/lez-programs"
  docker info >/dev/null                 # Docker Desktop WSL integration must be on
  make build-programs                     # output: target/guest/*.bin (NOT .../docker/ as the runbook says)
  make test                               # RISC0_DEV_MODE=1 cargo test --workspace (949 tests)
}

step3_sequencer() {
  . "$S1/env.sh"; cd "$S1/lez"
  cargo build --features standalone -p sequencer_service
  mkdir -p "$S1/seqhome"
  RISC0_DEV_MODE=1 RUST_LOG=info setsid nohup target/debug/sequencer_service \
    lez/sequencer/service/configs/debug/sequencer_config.json --home "$S1/seqhome" --listen-address 127.0.0.1 -p 3040 \
    > "$S1/logs/sequencer.log" 2>&1 < /dev/null &
  sleep 20
  curl -s -X POST localhost:3040 -H 'content-type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"getLastBlockId","params":{}}'    # grows by 1 every 15 s
  # stop with: pkill -f sequencer_service
}

step4_deploy_and_init() {
  . "$S1/env.sh"; cd "$S1"
  (cd lez && cargo build -p wallet) && (cd spel && cargo build -p spel)
  export LEE_WALLET_HOME_DIR="$S1/wallet-home"; mkdir -p "$LEE_WALLET_HOME_DIR"
  cp lez/lez/wallet/configs/debug/wallet_config.json "$LEE_WALLET_HOME_DIR/"     # sequencer_addr 127.0.0.1:3040
  W=lez/target/debug/wallet; SP=spel/target/debug/spel
  for l in admin freeze oracle-src coll-def coll-hold; do echo pw | $W account new public --label $l; done
  for p in token twap_oracle token_mint_authority stablecoin; do $W deploy-program lez-programs/target/guest/$p.bin; done
  $SP -- program-id lez-programs/target/guest/stablecoin.bin       # ImageID + ProgramId limbs
  # PDAs: helper examples run inside the lez-programs workspace (RISC0_SKIP_BUILD=1 avoids rebuilding guests)
  cp "$REPO_S1_DIR/global_pdas.rs" lez-programs/programs/stablecoin/examples/global_pdas.rs
  echo "see docs/spikes/S1.md section Commands for the exact spel invocations (new-fungible-definition,"
  echo "create-oracle-price-account, initialize-program) with the concrete account ids used."
  # NOTE: spel at this rev cannot encode i128 args; the run used a copy of stablecoin-idl.json with
  # "i128" -> "u128" (sed 's/"i128"/"u128"/g'); safe only for gain values 0 or positive.
}

step5_second_build() {
  cd "$S1"
  git clone lez-programs lez-programs-b && git -C lez-programs-b checkout "$PROGRAMS_SHA"
  git -C lez-programs-b apply "$REPO_S1_DIR/inst-b01.patch"          # adds #[used] static HOST_INSTANCE_TAG = *b"INST-B01"
  (cd lez-programs-b && make build-programs)                           # ~4 min with a warm Docker cache
  sha256sum lez-programs/target/guest/*.bin lez-programs-b/target/guest/*.bin
}

step6_chain_count() {
  . "$S1/env.sh"; cd "$S1/lez-programs/programs/integration_tests"
  cat tests/stablecoin.rs "$REPO_S1_DIR/chain_count.rs" > tests/chain_count.rs
  grep -q '^risc0-zkvm' Cargo.toml || echo 'risc0-zkvm = { workspace = true }' >> Cargo.toml
  cd ../.. && RISC0_DEV_MODE=1 cargo test -p integration_tests --test chain_count chain_count_liquidate_shaped -- --nocapture
}

step7_basecamp_offscreen() {
  cd "$S1" && mkdir -p s7
  nix build github:logos-co/logos-basecamp#bin-bundle-dir-inspector -o s7/result-bundle -L
  nix build 'github:logos-blockchain/logos-execution-zone-module/acf0cd501b262c4c15969e3735e85318297b85bf#install-portable' -o s7/lez-core-install -L
  mkdir -p s7/user/modules s7/user/plugins && cp -RL s7/lez-core-install/modules/. s7/user/modules/
  XDG_RUNTIME_DIR=/mnt/wslg/runtime-dir QT_QPA_PLATFORM=offscreen QML_INSPECTOR_PORT=3768 \
    LD_LIBRARY_PATH="$PWD/s7/result-bundle/lib" setsid nohup s7/result-bundle/bin/LogosBasecamp \
    --user-dir "$PWD/s7/user" -platform offscreen > logs/basecamp.log 2>&1 < /dev/null &
}

"$@"
