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

## Prerequisites (Linux or WSL Ubuntu)

Tested on Ubuntu 24.04 under WSL. Ubuntu 22.04 is expected to work and is what the `e2e` workflow
(`.github/workflows/e2e.yml`) runs on; its first hosted run passed (run 39017229879 at 5c77daa).

- `sudo apt-get install -y build-essential clang libclang-dev pkg-config libssl-dev libpcsclite-dev python3 git curl`
- Rust through rustup; `rust-toolchain.toml` selects 1.94.0.
- risc0 components, pinned: `curl -L https://risczero.com/install | bash`, then
  `rzup install rust 1.94.1 && rzup install cpp 2024.1.5 && rzup install r0vm 3.0.5 && rzup install cargo-risczero 3.0.5`.
- `scripts/bootstrap-lez.sh` builds the LEZ v0.2.4 sequencer and wallet into `$HOME/lez-deps`, once
  (21m44s cold on a hosted `ubuntu-22.04` runner, run 39017229879 at 5c77daa).
- Port 3040 free; at least 20 GB free disk.

The spec asks for "a clean machine with Nix"; this PoC documents the prerequisites above instead of a Nix
flake (Open decision recorded in the Plan 3b plan and in SUBMISSION.md).

## Run

    scripts/demo.sh                          # dev-mode sequencer
    scripts/demo.sh --sequencer-dev-mode 0   # RISC0_DEV_MODE=0 sequencer (RFP S2)
    scripts/demo.sh --keep                   # leave the sequencer running to explore with auction-cli
    scripts/demo.sh --preflight-only

On the author's machine (WSL, repository on `/mnt/c`) run it through the wrapper, which sets the shared target
directory: `scripts/wsl-env.sh scripts/demo.sh`. The script refuses to delete a `DEMO_RUN_DIR` that is not
empty and not a previous demo run (exit 2).

With `--keep` the sequencer stays up after the run. Explore it with `auction-cli` using the run's files:

    auction-cli --deployment $HOME/lez-target/demo/deployment.json --state-dir $HOME/lez-target/demo/cli-state       --wallet-home $HOME/lez-target/demo/wallet-home host status

Stop it with `kill "$(cat "$HOME/lez-target/demo/sequencer.pid")"`. Without `--keep` the script stops it itself.
Preflight failures (exit 3 to 6) print their own `STOP`/`preflight:` reason and not `demo FAILED`; a failure
after the preflight prints `demo FAILED (exit N)` with the log paths.

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

## GUI run (Basecamp)

The same scenario runs as UI actions in the Auctions app inside Basecamp. It needs Nix inside WSL and the
prerequisites above, with at least 10 GB free on `C:`. Install the app as `.lgx` packages into a fresh profile
and run the story in the full Basecamp:

    scripts/basecamp/install-lgx.sh /tmp/auctions-profile --setup --test apps/auctions/tests/story.mjs

This builds lez_core 0.4.1, `auction_engine` and `auctions` as `.lgx` packages and installs them with `lgpm`,
core modules first. It then sets up a fresh localnet (`demo.sh --setup-only`), starts Basecamp with
`--user-dir /tmp/auctions-profile`, opens Auctions from the sidebar and drives all 172 steps. The figures on
screen are checked against the table above and against `auction-cli`.

Options:
- Leave out `--test …` to keep Basecamp and the sequencer running and click through by hand.
- Use `--install-only` to stop after the install.
- Use `scripts/basecamp/run-app.sh --setup --test apps/auctions/tests/story.mjs` for the standalone-mode
  variant, used while developing the app.

A profile with a lez_core newer than 0.4.1, for example one taken from Basecamp's catalogue, is unsupported
(exit 4). Use a fresh profile and do not accept a lez_core upgrade there. Details and the recorded run (1261 s
in total, 329 s for the story): [docs/architecture/basecamp-app.md](docs/architecture/basecamp-app.md).

## Public legacy testnet

`scripts/testnet-legacy.sh` runs the same scenario (`scripts/lib/demo-scenario.sh`) on the public LEZ v0.2.4
legacy testnet instead of a standalone sequencer: `--preflight-only` checks the node read-only, `--send` deploys
the stablecoin and the engine and runs the phases above on 60 s blocks. Why, how and the caveats (one run per
chain, possible resets, the mock oracle keeper): [testnet-legacy.md](docs/architecture/testnet-legacy.md).

## Preflight exit codes

3 low disk; 4 LEZ binaries or tools missing (run `scripts/bootstrap-lez.sh`); 5 port 3040 in use; 6 a
committed guest does not match `artifacts/guests/manifest.json`.

## Recorded runs

| Date | Commit | Sequencer | Result |
|---|---|---|---|
| 2026-10-16 | 28e31b0 | RISC0_DEV_MODE=1 | demo complete (971 s, 37 CLI transactions submitted; unbacked debt 42_289) |
| 2026-10-16 | 28e31b0 + review fixes | RISC0_DEV_MODE=1 | demo complete (987 s, 37 CLI transactions submitted; unbacked debt 42_275, every check including the R4 range 42_154 to 63_200) |
| 2026-10-16 | 28e31b0 + review fixes | RISC0_DEV_MODE=0 | demo complete (973 s, 37 CLI transactions submitted; unbacked debt 42_276, every check) |
| 2026-10-17 | 5c77daa (hosted, run 39017229879) | RISC0_DEV_MODE=0 | demo complete (demo step 21m00s on `ubuntu-22.04`; unbacked debt 42_274, every check including the R4 range and `frozen yes`) |

On CI the `e2e` workflow (job `localnet-e2e`) runs the demo with `RISC0_DEV_MODE=0` on every push to `main` and in
both modes weekly (Sunday). The `RISC0_DEV_MODE=0` run takes about as long as
the dev-mode one: LEZ v0.2.4's sequencer executes public transactions with the risc0 executor and never proves them
(see [real-proofs.md](docs/architecture/real-proofs.md)), so with either setting the sequencer only executes. Sampled
with `ps` early in that run, the sequencer's resident memory was about 84 MB and no `r0vm` process was running.

The table above quotes the first run. The shortfall figures vary slightly from run to run, because the auction price
decays with block time and the bid lands a few blocks earlier or later (42_289, 42_275 and 42_276 in the three runs; the
settle and lot steps then follow exactly: minus 29_500, minus 5_000). The buffer, keeper reward, token and freeze
results are the same in every run. The demo checks the shortfall against R4's range, not an exact value.
