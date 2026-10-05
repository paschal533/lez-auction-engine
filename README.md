# LEZ Auction Engine

A liquidation and auction engine for the Logos Execution Zone (LEZ), built as a proof of concept for RFP-014. It runs the auctions behind an over-collateralised stablecoin: collateral auctions for liquidated positions (descending discount, partial fills, keeper reward, no expiry), a surplus auction that sells protocol-buffer coin for a protocol token, and a debt auction that mints protocol tokens against unbacked debt. All decision logic lives in `auction_core`, a pure, `no_std`, dependency-free Rust crate that does not know about LEZ; the on-chain programs, SDK, CLI and GUI are thin layers over it (planned in Plans 2 to 4). The work targets LEZ v0.2.4 and SPEL `1ef0500f` ("Track B", ADR-001).

## Status

**Plan 1 complete: core + spikes.**

- `crates/auction_core`: collateral, surplus and debt logic with property tests, a differential model in Python, and Kani proof harnesses.
- Spikes S1 to S7 are recorded, with decisions in the design log (ADR-018 to ADR-023): toolchain and multi-instance build (S1), host authentication of the engine (S2), arg-seeded PDAs created on first use (S3), piecewise-linear discount kept (S4), pause-onset policy (S5), `Accounting` updated at chain end (S6), protocol token minted from an engine PDA (S7).
- Next: Plan 2 (programs), Plan 3 (SDK, CLI, CI and real proofs), Plan 4 (thin GUI).

## Run the tests

Rust 1.94.0 (pinned by `rust-toolchain.toml`) and Python 3 are enough for the core. From the repository root (Git Bash on Windows, or any POSIX shell):

```bash
cargo fmt --all -- --check
cargo clippy -p auction_core --all-targets -- -D warnings
PROPTEST_CASES=5000 cargo test -p auction_core
cargo run -q -p auction_core --example vectors > "$TEMP/vectors.json" && python tools/differential/check.py "$TEMP/vectors.json"
```

The last line is the differential check: the example prints test vectors from the Rust core and `tools/differential/check.py` replays them against an independent Python model. CI (`.github/workflows/core.yml`, Ubuntu) uses `/tmp/vectors.json` and `python3` instead of `$TEMP` and `python`.

The spikes under `spikes/` need the risc0 toolchain (WSL or Linux) and are not part of the workspace; see the per-spike documents for the exact commands.

## Documents

- Spec: [docs/superpowers/specs/2026-10-01-rfp014-auction-engine-poc-design.md](docs/superpowers/specs/2026-10-01-rfp014-auction-engine-poc-design.md)
- Design log (ADRs): [docs/design-log.md](docs/design-log.md)
- Spikes: [docs/spikes/](docs/spikes/) ([S1](docs/spikes/S1.md), [S2](docs/spikes/S2.md), [S3](docs/spikes/S3.md), [S4](docs/spikes/S4.md), [S5](docs/spikes/S5.md), [S6](docs/spikes/S6.md), [S7](docs/spikes/S7.md)); spike code in [spikes/](spikes/)
- Architecture: [docs/architecture/core.md](docs/architecture/core.md) (auction state machine), [docs/architecture/math.md](docs/architecture/math.md) (fixed-point arithmetic)
- Plans: [docs/superpowers/plans/](docs/superpowers/plans/)

## Licence

MIT OR Apache-2.0.
