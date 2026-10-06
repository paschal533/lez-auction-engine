# Vendored upstream code

| Path here | Upstream path | Repository | Revision |
|---|---|---|---|
| `programs/stablecoin/` | `programs/stablecoin/` | logos-blockchain/lez-programs | `64c01210adecd6b557dbd021a3cc94553f243fd8` |
| `programs/token/` | `programs/token/` | same | same |
| `programs/twap_oracle/core/` | `programs/twap_oracle/core/` | same | same |
| `programs/upstream_integration_tests/` | `programs/integration_tests/` (only `Cargo.toml`, `src/lib.rs`, `tests/stablecoin.rs`) | same | same |
| `tools/idl-gen/` | `tools/idl-gen/` | same | same |
| `idl/upstream/*.json` | `artifacts/*.json` | same | same |

The vendor commit has the subject `vendor: lez-programs 64c01210 (stablecoin, token, twap_oracle core, idl-gen)` and contains no local edits.

## Local adjustments (not offered upstream)

- `programs/upstream_integration_tests/Cargo.toml`: amm, ata and token_mint_authority dependencies removed (their tests are not vendored).
- Root `Cargo.toml` carries upstream's `[workspace.lints]` verbatim so `[lints] workspace = true` resolves.
- Upstream's root `clippy.toml` and `rustfmt.toml` are copied into `programs/stablecoin/`, `programs/token/` and `programs/twap_oracle/` (not the repo root, so our crates keep their own style). The stablecoin crates do not opt into `[workspace.lints]` upstream either; their clippy gate is `-D warnings` with this `clippy.toml`. No nightly toolchain is installed in WSL (`rustup toolchain list`: stable (default), 1.94.0 (active), risc0), so upstream's `cargo +nightly fmt` cannot run here; formatting is checked with stable rustfmt and this config against the Task 3 baseline below.
- `Cargo.lock` is seeded from upstream's lock at the pin, then `proptest` is pinned to `1.11.0` (auction_core dev-dependency); `ruint` resolves to `1.17.0`.

## Formatting baseline (stable rustfmt, vendored config)

`scripts/wsl-cargo.sh fmt -p stablecoin_core -p stablecoin_program -- --check` on the pristine stablecoin: exit status clean, 0 `Diff in` hunks, 21 output lines, all of them "can't set `<option>`, unstable features are only available in nightly channel" warnings (the baseline line count is 21). Hook tasks require that `fmt --check` reports no diff in any hook-added line.

## Hook patch series (offered upstream)

Every later commit touching `programs/stablecoin/` (except commits whose subject contains `[local]`) is one patch. Regenerate with `scripts/export-hook-patches.sh`; the files land in `patches/stablecoin/` at the repo root (outside `programs/stablecoin/`, so a patch never contains an earlier patch) and apply at the lez-programs root with `git am`; `scripts/check-hook-patches.sh` proves that on a pristine copy.

## Upstream suite baseline

`scripts/wsl-cargo.sh test -p stablecoin_core -p stablecoin_program -p token_program -p integration_tests`: 410 passed, 0 failed at the vendor commit (stablecoin_core 66, stablecoin_program 264, token_program 63, integration_tests `tests/stablecoin.rs` 17, doc-tests/lib 0). Every hook task re-runs it and must keep 0 failed.
