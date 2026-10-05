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
  # shorthand used by step4/step5; the ids are the S1 run's, replace with yours
  cat > "$S1/s1env.sh" <<'XEOF'
. $HOME/lez-spikes/s1/env.sh
cd $HOME/lez-spikes/s1
export LEE_WALLET_HOME_DIR=$HOME/lez-spikes/s1/wallet-home
SP=$PWD/spel/target/debug/spel; W=lez/target/debug/wallet; A=$PWD/lez-programs/artifacts
TOK=14b3bc6cc129d0359f9595ba0f60a13e4a8df08ffe8eb1a83251c47abecc67a8
TWAP=5681e27c27b3ece4a7c161bbf8e12e6ece6ca69925a87b77f9e193fa4a249b31
STABLE=c655d477f2aa5a1684b4e22e50bab81b70218a0f99e44cd5959340f2b1c596ba
CLOCK=4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU
FAUCET_PDA=GmAwyCjGaT7ocZ2ePDgGFmNw9Yv51ohG421dHMukZRQC
ADMIN=ANrZnv2VFYpFNKiT5L6jUP4nb4wFx8q91iDxGYhoN14t; FREEZE=5xJg9LwMZwAxvxC2f8wVXTyy2AefRCk9DWS72Bg5wnkE
ORSRC=Gy2vucuTCPvxx7LVLjY3nnCtFv1ybjkwcVfTDPXxvuJf; COLLDEF=98zNtVZcjppaXxRf2kRXzveVq7po1tqPce9y8YCeNgFC
COLLHOLD=3L3JSx5rveVZeC8stfzpX7tbxvhhuVdDAXDRFy2tdRcu; ORACLE=63ETbn37pNdwKcYT4UxokjwKsNdAqumuzeus7gNRaYb6
PP=8jZQco4Py4ChAxMfE4AGn4N1j7kRwhS1hW62rDUGmpzV; SFA=7BuSzbPeGdbZQwxqkaPjecjLiRuAUXT9T4P8ifJecyyY
RPS=FbJ9RP5dpLzaC2bFYe6EkRFMnyGMiYcwBsD2vM3XZHXo; SDEF=8C4KgWQZYkBkZVo8P1mmrf5xkJNimtkjoMKE91vevrKb
SMASTER=4Xdmh1MFxBptfMn91TFiS2PTLDK1PDdirn43m81a2vpa
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
  for l in admin freeze oracle-src coll-def coll-hold user-hold; do echo pw | $W account new public --label $l; done
  for p in token twap_oracle token_mint_authority stablecoin; do $W deploy-program lez-programs/target/guest/$p.bin; done
  $SP -- program-id lez-programs/target/guest/stablecoin.bin       # ImageID + ProgramId limbs
  # PDAs: helper examples run inside the lez-programs workspace (RISC0_SKIP_BUILD=1 avoids rebuilding guests)
  cp "$REPO_S1_DIR/global_pdas.rs" lez-programs/programs/stablecoin/examples/global_pdas.rs
  # Ids come from the s1env.sh written in step0 (the S1 run's ids; substitute yours:
  # accounts via `$W account list`, PDAs via the global_pdas / mint_authority / twap_oracle_pdas examples).
  . "$S1/s1env.sh"
  # spel at this rev cannot encode i128 args: use an IDL copy with i128 -> u128 (valid only for gains >= 0; we used 0).
  sed 's/"i128"/"u128"/g' "$A/stablecoin-idl.json" > "$S1/stablecoin-idl-u128.json"
  $SP --idl $A/token-idl.json -p $TOK -- new-fungible-definition --name COLL --total-supply 1000000000000000000000000 \
    --mint-authority $FAUCET_PDA --definition-target-account $COLLDEF --holding-target-account $COLLHOLD
  $SP --idl $A/twap_oracle-idl.json -p $TWAP -- create-oracle-price-account --base-asset $SDEF --quote-asset $COLLDEF \
    --initial-price 1000000000000000000000000000 --window-duration 3600000 --oracle-price-account $ORACLE --price-source $ORSRC --clock $CLOCK
  $SP --idl "$S1/stablecoin-idl-u128.json" -p $STABLE -- initialize-program --freeze-authority-account-id $FREEZE \
    --initial-stability-fee-per-millisecond 1000000000000001000000000000 --initial-controller-proportional-gain 0 --initial-controller-integral-gain 0 \
    --initial-minimum-collateralization-ratio 1500000000000000000000000000 --minimum-milliseconds-between-rate-updates 60000 \
    --maximum-oracle-price-age-milliseconds 3600000 --initial-redemption-price 1000000000000000000000000000 --stablecoin-name SPIKE \
    --admin $ADMIN --protocol-parameters $PP --stability-fee-accumulator $SFA --redemption-price-state $RPS \
    --stablecoin-definition $SDEF --stablecoin-master-holding $SMASTER --collateral-definition $COLLDEF --market-price-oracle $ORACLE --clock $CLOCK
  # A key that signs more than one stablecoin tx must first run: $W auth-transfer init --account-id Public/<id>
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
  # One-time: trust the logos binary cache (from Basecamp's flake nixConfig), else Nix ignores its unsigned substitutes.
  #   ~/.config/nix/nix.conf: extra-substituters = https://cache.nix.logos.co/public
  #                           extra-trusted-public-keys = public:l4HrXgL4nw246+LBh2SOJyhz64BoGegOYLheT/iIAPU=
  # The S1 run used the floating ref (rev not recorded; bundle store path s4sk19kp...). master was 9697d110... when checked later.
  # To pin: github:logos-co/logos-basecamp/9697d11084b9538fdfdd373b611f05a5680a2c30#bin-bundle-dir-inspector (unverified to reproduce).
  nix build github:logos-co/logos-basecamp#bin-bundle-dir-inspector -o s7/result-bundle -L
  nix build 'github:logos-blockchain/logos-execution-zone-module/acf0cd501b262c4c15969e3735e85318297b85bf#install-portable' -o s7/lez-core-install -L
  mkdir -p s7/user/modules s7/user/plugins && cp -RL s7/lez-core-install/modules/. s7/user/modules/
  XDG_RUNTIME_DIR=/mnt/wslg/runtime-dir QT_QPA_PLATFORM=offscreen QML_DISABLE_DISK_CACHE=1 QML_INSPECTOR_PORT=3768 \
    LEE_WALLET_HOME_DIR="$S1/wallet-home" LD_LIBRARY_PATH="$PWD/s7/result-bundle/lib" setsid nohup s7/result-bundle/bin/LogosBasecamp \
    --user-dir "$PWD/s7/user" -platform offscreen > logs/basecamp.log 2>&1 < /dev/null &
}

"$@"
