# Demo: one-command reproduction (spec §1.1)

`scripts/demo.sh` starts a standalone LEZ v0.2.4 sequencer and deploys the stablecoin (with our hooks), the
auction engine and a mock protocol token. It then runs, with checks:
1. a position made liquidatable, liquidated, bid on in two parts and settled (F1-F3);
2. a surplus auction (F5);
3. a market price move that leaves a later auction short (localnet publisher, ADR-050);
4. that shortfall driven into a debt auction (F4);
5. the "no protocol token configured, so the host freezes new debt" path.

All transactions are public, sent by `auction-cli` (own encoders; no spel). The guests deployed are the
committed reproducible builds in `artifacts/guests/` (ADR-047).

## Prerequisites (clean Ubuntu 22.04 or WSL Ubuntu)

- `sudo apt-get install -y build-essential clang libclang-dev pkg-config libssl-dev libpcsclite-dev python3 git curl`
- Rust through rustup; `rust-toolchain.toml` selects 1.94.0.
- risc0 components, pinned: `curl -L https://risczero.com/install | bash`, then
  `rzup install rust 1.94.1 && rzup install cpp 2024.1.5 && rzup install r0vm 3.0.5 && rzup install cargo-risczero 3.0.5`.
- `scripts/bootstrap-lez.sh` builds the LEZ v0.2.4 sequencer and wallet into `$HOME/lez-deps`, once
  (about 20 min cold).
- Port 3040 free; at least 20 GB free disk.

The spec asks for "a clean machine with Nix"; this PoC documents the prerequisites above instead of a Nix
flake (Open decision recorded in the Plan 3b plan and in SUBMISSION.md).

## Run

    scripts/demo.sh                          # dev-mode sequencer
    scripts/demo.sh --sequencer-dev-mode 0   # RISC0_DEV_MODE=0 sequencer (RFP S2)
    scripts/demo.sh --keep                   # leave the sequencer running to explore with auction-cli
    scripts/demo.sh --preflight-only

A run takes about 16 minutes (blocks every 15 s; 971 s measured, warm build). The summary is written to
`$HOME/lez-target/demo/demo-summary.txt` (`DEMO_RUN_DIR` overrides the directory).

## What it shows (localnet values, R = 1)

| Phase | Action | On chain (recorded run) |
|---|---|---|
| positions | p0 nonce 0: 500_000 coll / 300_000 debt; p1 nonce 1: 1_000_000 / 100_000; p2 nonce 2: 500_000 / 300_000; then min CR 1.5 -> 2.0 | `list-at-risk` shows p0 and p2 |
| F1-F3 | liquidate p0 (keeper reward 50); bid 100_000; bid 230_000 (final); claim leftover | buffer 30_000 |
| F5 | observe, settle (nothing), check (healthy), start surplus, take 500 | buffer 29_500 |
| price move | `dev oracle-publish --price 1.9e27` | the host's market oracle is the new account |
| shortfall | liquidate p2; bid 330_000 (`bid_with_settlement`, the lot binds: raised 257_711 of 330_000) | unbacked debt 42_289 |
| F4 | settle-debt (burns 29_500); start-debt; transfer 5_000 to taker-stable; take-debt 5_000 | unbacked debt 12_789, then 7_789 (exactly 5_000 less, still above 5_000); taker protocol tokens 9_800 -> 13_050 |
| freeze | `admin set-engine-params --disable-protocol-token`; check-solvency | `host status` -> `frozen yes` |

The run ends with `admin verify`, a scan of the sequencer log (`no failed execution check`) and `demo complete`.

Parameters: `cli/params/demo-engine.json`, `cli/params/demo-collateral.json` (the defaults except a
`band_hi_bps` of 20_000, a debt lot and freeze threshold of 5_000; ADR-051). The same economics run in-process in
`programs/integration_tests/tests/demo_scenario.rs`.

## Preflight exit codes

3 low disk; 4 LEZ binaries or tools missing (run `scripts/bootstrap-lez.sh`); 5 port 3040 in use; 6 a
committed guest does not match `artifacts/guests/manifest.json`.

## Recorded runs

| Date | Commit | Sequencer | Result |
|---|---|---|---|
| 2026-10-16 | 21f5449 + Task 13 | RISC0_DEV_MODE=1 | demo complete (971 s, 37 CLI transactions submitted) |
| | | RISC0_DEV_MODE=0 | not yet run locally (host memory); runs in CI (`e2e` workflow) |
