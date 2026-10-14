# syntax=docker/dockerfile:1.7
# Reproducible guest build (ADR-047), the lez-programs 64c01210 recipe (scripts/build-guests.Dockerfile):
# same builder image (pinned by digest here), rustflags and packager; our two guests.
ARG BUILDER=risczero/risc0-guest-builder:r0.1.88.0@sha256:3e12f71bacd27527a61dea96fa0e53e468c99aa261d3a1019b593f6dbd943eb3
FROM ${BUILDER} AS build
WORKDIR /src
COPY . .
ENV CARGO_TARGET_DIR=/src/target/risc0-guests
ENV RISC0_FEATURE_bigint2=""
ENV CC_riscv32im_risc0_zkvm_elf=/root/.risc0/cpp/bin/riscv32-unknown-elf-gcc
ENV CFLAGS_riscv32im_risc0_zkvm_elf="-march=rv32im -nostdlib"
# Registry and git caches are shared with lez-programs' build (same ids) so the Docker disk on C: grows
# only by our target directory.
RUN --mount=type=cache,id=lez-programs-risc0-guests-cargo-git,sharing=locked,target=/root/.cargo/git \
    --mount=type=cache,id=lez-programs-risc0-guests-cargo-registry,sharing=locked,target=/root/.cargo/registry \
    --mount=type=cache,id=lez-auction-engine-guests-target,sharing=locked,target=/src/target/risc0-guests <<'EOT'
set -eu
target_triple="riscv32im-risc0-zkvm-elf"
unit_separator="$(printf '\037')"
guest_rustflags="-C${unit_separator}passes=lower-atomic${unit_separator}-C${unit_separator}link-arg=-Ttext=0x00200800${unit_separator}-C${unit_separator}link-arg=--fatal-warnings${unit_separator}-C${unit_separator}panic=abort${unit_separator}--cfg${unit_separator}getrandom_backend=\"custom\""
export CARGO_ENCODED_RUSTFLAGS="${guest_rustflags}"
for manifest in programs/stablecoin/methods/guest/Cargo.toml programs/auction_engine/methods/guest/Cargo.toml; do
    echo "==> Building ${manifest}"
    cargo +risc0 build --release --locked --target "${target_triple}" --manifest-path "${manifest}"
done
unset CARGO_ENCODED_RUSTFLAGS
cargo +risc0 build --locked --manifest-path tools/risc0-packager/Cargo.toml
packager="${CARGO_TARGET_DIR}/debug/risc0-packager"
mkdir -p /guest-output
"${packager}" "${CARGO_TARGET_DIR}/${target_triple}/release/stablecoin" /guest-output/stablecoin.bin
"${packager}" "${CARGO_TARGET_DIR}/${target_triple}/release/auction_engine" /guest-output/auction_engine.bin
EOT

FROM scratch AS export
COPY --from=build /guest-output /
