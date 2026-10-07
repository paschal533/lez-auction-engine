# LEZ Auction Engine

A liquidation and auction engine for the Logos Execution Zone (LEZ), built as a proof of concept for RFP-014. It runs the auctions behind an over-collateralised stablecoin: collateral auctions for liquidated positions (descending discount, partial fills, keeper reward, no expiry), a surplus auction that sells protocol-buffer coin for a protocol token, and a debt auction that mints protocol tokens against unbacked debt. All decision logic lives in `auction_core`, a pure, `no_std`, dependency-free Rust crate that does not know about LEZ; the on-chain programs, SDK, CLI and GUI are thin layers over it (planned in Plans 2 to 4). The work targets LEZ v0.2.4 and SPEL `1ef0500f` ("Track B", ADR-001).

## Status

**Plan 2 complete: on-chain programs.**

- Stablecoin hooks: the vendored stablecoin (lez-programs `64c01210`) gains `set_liquidation_engine`, `liquidate_position` and `engine_freeze_new_debt`, exported as a patch series for upstream (ADR-025) and guarded by Variant C, caller program id plus the bound engine-authority PDA (ADR-026, [hooks](docs/architecture/hooks.md)).
- Engine: the `auction_engine` program, 17 instructions over `auction_core` (liquidation, bids, settlement, F5 surplus and F4 debt auctions, `check_solvency`), with config-seeded PDAs, optional-account slots, sweep rules, the measured chained-call budget and a second host instance (ADR-027 to ADR-036, [programs](docs/architecture/programs.md)). Threat model and security review: [THREAT_MODEL.md](THREAT_MODEL.md), [per-instruction checklist](docs/architecture/security-checklist.md).
- Tests: handler unit tests (`auction_engine_program`), and `engine_integration_tests`, which runs the real token, stablecoin and engine guests in-process (hooks, config, liquidation, bids, settlement, system auctions, solvency, security and laundering attacks, account invariants, chain budgets, two instances). Run them with the WSL command under "Build and test the programs" below (`scripts/wsl-cargo.sh test -p engine_integration_tests`).
- Next: Plan 3 (SDK, CLI, CI and real proofs), then Plan 4 (thin GUI).

**Plan 1 complete: core + spikes.**

- `crates/auction_core`: collateral, surplus and debt logic with property tests, a differential model in Python, and Kani proof harnesses.
- Spikes S1 to S7 are recorded, with decisions in the design log: toolchain and multi-instance build (S1, ADR-018), host authentication of the engine (S2, ADR-019), arg-seeded PDAs created on first use (S3, ADR-020), protocol token minted from an engine PDA (S7, ADR-021), piecewise-linear discount kept (S4, ADR-022), `Accounting` updated at chain end (S6, ADR-023). The S5 pause-onset policy is ADR-007 (amended).

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

## Build and test the programs (WSL)

The on-chain programs build only inside WSL (risc0 toolchain, `~/lez-spikes/s1/env.sh` from spike S1). Run every cargo command through the wrapper; it keeps the target directory on the WSL disk (`$HOME/lez-target/plan2`) and sets `RISC0_DEV_MODE=1`:

```bash
MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc   'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p engine_integration_tests'
```

`MSYS_NO_PATHCONV=1` stops Git Bash from rewriting `/mnt/...` paths. Commit from Git Bash on Windows (GPG signing), not from WSL. The repository stores text with LF line endings (`.gitattributes`).

## Documents

- Spec: [docs/superpowers/specs/2026-10-01-rfp014-auction-engine-poc-design.md](docs/superpowers/specs/2026-10-01-rfp014-auction-engine-poc-design.md)
- Design log (ADRs): [docs/design-log.md](docs/design-log.md)
- Spikes: [docs/spikes/](docs/spikes/) ([S1](docs/spikes/S1.md), [S2](docs/spikes/S2.md), [S3](docs/spikes/S3.md), [S4](docs/spikes/S4.md), [S5](docs/spikes/S5.md), [S6](docs/spikes/S6.md), [S7](docs/spikes/S7.md)); spike code in [spikes/](spikes/)
- Architecture: [docs/architecture/core.md](docs/architecture/core.md) (auction state machine), [docs/architecture/math.md](docs/architecture/math.md) (fixed-point arithmetic), [docs/architecture/hooks.md](docs/architecture/hooks.md) (stablecoin hooks), [docs/architecture/programs.md](docs/architecture/programs.md) (engine program, Plan 3 interfaces), [docs/architecture/security-checklist.md](docs/architecture/security-checklist.md) (per-instruction checklist)
- Threat model: [THREAT_MODEL.md](THREAT_MODEL.md)
- Plans: [docs/superpowers/plans/](docs/superpowers/plans/)

## Licence

MIT OR Apache-2.0.
