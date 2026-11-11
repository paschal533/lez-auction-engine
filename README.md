# LEZ Auction Engine

A liquidation and auction engine for the Logos Execution Zone (LEZ), built as a proof of concept for RFP-014. It runs the auctions behind an over-collateralised stablecoin: collateral auctions for liquidated positions (descending discount, partial fills, keeper reward, no expiry), a surplus auction that sells protocol-buffer coin for a protocol token, and a debt auction that mints protocol tokens against unbacked debt. All decision logic lives in `auction_core`, a pure, `no_std`, dependency-free Rust crate that does not know about LEZ. Two engines are built on it:

- **The 0.4 engine (`v04/`, primary):** LEZ v0.4.0-rc1 actors in a separate cargo workspace ([v04/README.md](v04/README.md); design note [docs/v04-port-design.md](docs/v04-port-design.md)).
- **The v0.2.4 engine (root workspace):** LEZ v0.2.4 and SPEL `1ef0500f` ("Track B", ADR-001), with the stablecoin hooks, SDK, CLI and the Basecamp app.

## Status

**0.4 engine (LEZ v0.4.0-rc1).** Plans 5 to 7 are merged to `main` (PRs #4, #5 and #6).

- Engine: the liquidation slice, `set_engine_params`, `observe_feed`, `settle_debt`, the surplus and debt auctions (start, take, close) and `check_solvency` with the host freeze, as actor conversations (ADR-056 to ADR-063; status table in [docs/v04-port-design.md](docs/v04-port-design.md) §8). It runs against our own token and oracle actors and a mock CDP host that implements the F7 host interface, because lez-programs has no 0.4 branch yet.
- SDK: plans, simulate-then-send with a fitted gas limit (ADR-061), block-level confirmation and the scanner (ADR-066).
- CI: `v04.yml` runs fmt, clippy and `cargo test --workspace` on changes to `v04/` and the shared crates; green on `main`, run 42033888330.
- End to end: `v04-e2e.yml` on a standalone sequencer with `RISC0_DEV_MODE=0`: run 42015414886 at 56876f5, green in 59m30s (cold cache); run 42033888263 at 5913f0a, green in 41m51s (warm) ([ci.md](docs/architecture/ci.md)).
- Cycles and fees: [v04/CYCLES.md](v04/CYCLES.md). The most expensive instruction, `Liquidate`, uses under 9 % of the gas cap.
- Waiting on upstream: the IDL and SPEL entrypoint (no SPEL for 0.4), the real host hooks (no lez-programs 0.4), the CLI and Basecamp module on `lez_core` 0.6.0, and a public 0.4 testnet. See [Dependencies](#dependencies).

**v0.2.4 engine (LEZ v0.2.4, SPEL `1ef0500f`).** Plans 1 to 4 are merged to `main` (Plan 4 by PR #3).

- Programs: the `auction_engine` program, 17 instructions over `auction_core`, and three hooks appended to the vendored stablecoin (lez-programs `64c01210`), exported as a patch series for upstream ([programs.md](docs/architecture/programs.md), [hooks.md](docs/architecture/hooks.md)).
- `auction_sdk` and `auction-cli` ([sdk.md](docs/architecture/sdk.md), [cli/README.md](cli/README.md)).
- CI on hosted runners: program tests, IDL drift, hook patches, cycle gate, coverage gate, requirements check, reproducible guests, the `RISC0_DEV_MODE=0` e2e demo and the `real-proof-push` proof of `settle_debt`, all green on `main` after the Plan 4 merge ([ci.md](docs/architecture/ci.md), [real-proofs.md](docs/architecture/real-proofs.md)).
- Public v0.2.4 legacy testnet run (2026-10-17): `demo complete`, every check `ok` ([testnet-legacy.md](docs/architecture/testnet-legacy.md)).
- [CYCLES.md](CYCLES.md): cycle baseline, transaction sizes and the 10 % regression gate (ADR-048).

**Basecamp app (on the v0.2.4 engine).** `apps/auctions/tests/story.mjs` drives the whole demo as 172 UI steps, from `.lgx` packages in a clean Basecamp profile. It passed in Basecamp `e09962f5` and again in Basecamp master `c154375` (2026-11-06), every figure check `ok` against `auction-cli` ([basecamp-app.md](docs/architecture/basecamp-app.md)).

**Evidence.** [REQUIREMENTS.md](REQUIREMENTS.md) maps every hard requirement to evidence and is checked by `scripts/check-requirements.py`. Red-team pass: [docs/security/redteam-2026-10.md](docs/security/redteam-2026-10.md), 0 Critical, 0 High, 0 Medium; 6 Low (all off-chain) and 6 Info. Submission index with hashes: [SUBMISSION.md](SUBMISSION.md).

## Tests

Counted from the source by `scripts/count-tests.py`, one row per area. CI fails if this block drifts from the source; regenerate it with `python3 scripts/count-tests.py --markdown`.

<!-- test-counts:begin -->
| Area | Tests |
|---|---:|
| 0.4 engine (v04/) | 381 |
| v0.2.4 programs | 851 |
| Shared crates | 344 |
| CLI | 85 |
| Fuzz + Kani proofs | 6 |
| Basecamp QML | 162 |
| Core module C++ | 51 |
| Python tooling | 113 |
| **Total** | 1993 |
<!-- test-counts:end -->

Counts include `#[ignore]`d tests (run by named jobs) and the six Kani harnesses (two in `crates/auction_core`, four in `v04/risk_core`), which are written but have never been run: no Kani toolchain is installed and no CI job runs them (SOFT2, ADR-015).

## Dependencies

What the engines build against today, and what happens if a dependency is late or changes. "Not pinned" means nothing in the repository pins a version.

| Capability | Built against today (version/commit) | If late or changed | Where the interface is isolated |
|---|---|---|---|
| Price feed | 0.4: our oracle actor with a relay publisher (`v04/programs/oracle_core`). v0.2.4: the vendored `twap_oracle` core from lez-programs `64c01210` (`programs/UPSTREAM.md`) | The engine validates every reading (age, zero price, confidence, band around the reference price) and pauses the collateral type on a bad feed, with no fallback (ADR-005); another feed program needs only a new reader | `v04/engine_core/src/feed.rs` and the guest's `engine/observe.rs`; on v0.2.4 `programs/auction_engine/src/feed.rs` and `observe.rs` |
| Host CDP | 0.4: a mock host that implements the F7 interface (`v04/programs/mock_host_core`), because lez-programs has no 0.4 branch. v0.2.4: the vendored stablecoin, lez-programs `64c01210`, with our three hooks | Keep the mock until upstream ports the stablecoin, then replace it with upstream's port or our own (design note §8, task 12) | The F7 host messages (`v04/programs/mock_host_core/src/lib.rs`); on v0.2.4 the hook patch series `patches/stablecoin/` ([hooks.md](docs/architecture/hooks.md)) |
| Collateral and stable token programs | 0.4: our token actor port, one holding actor per owner and definition (`v04/programs/token_core`, ADR-062). v0.2.4: the vendored token program, lez-programs `64c01210` | Swap in upstream's 0.4 token program when it exists; on v0.2.4 the engine checks every definition it is given (`WrongDefinition`, `ProtocolTokenMismatch`); on 0.4 the definitions come from the config and a host that disagrees is refused (`HostMismatch`, `v04/engine_core/src/check.rs`) | `v04/programs/token_core/src/lib.rs`; on v0.2.4 `token_program_id` in the engine config |
| Admin authority (RFP-001) | Not pinned: no RFP-001 implementation is used. The engine admin and the host admin are user keys held by the operator ([THREAT_MODEL.md](THREAT_MODEL.md)) | Adopt the RFP-001 authority when it lands. Today a PDA or multisig host admin cannot call `set_liquidation_engine` (top-level only, ADR-026) | 0.4: the signer proxy `(admin, E)` in front of the config (design note §3); v0.2.4: the `admin` signer of the engine's admin instructions |
| Freeze authority (RFP-002) | Not pinned: no RFP-002 implementation is used. The host's existing freeze authority (a user key) unfreezes; the engine only freezes, through `engine_freeze_new_debt` | Route the freeze through the RFP-002 authority when it lands; the engine never unfreezes, so only that one call changes | The host freeze hook: `engine_freeze_new_debt` (v0.2.4), `EngineFreezeNewDebt` (0.4) |
| Runtime (LEZ) | 0.4: LEZ `v0.4.0-rc1` (`v04/Cargo.toml`), Rust 1.98.1, risc0 3.0.5. v0.2.4: LEZ `v0.2.4` (tag in the root `Cargo.toml`; commit `47eba256` in `scripts/submission-hashes.sh`), Rust 1.94.0, risc0 3.0.5 | Re-pin the tag and re-run `v04.yml` and `v04-e2e.yml`. No public 0.4 testnet exists yet (design note §8, task 14) | The `v04/` workspace pins LEZ in `v04/Cargo.toml` (the `*_core` crates repeat the tag); `auction_core` and `auction_engine_params`, shared with v0.2.4, do not depend on LEZ |
| Framework (SPEL) | v0.2.4: SPEL `1ef0500f` (the programs' `Cargo.toml` files). 0.4: none; SPEL has no 0.4 support, so the guest uses `actor_entrypoint!` directly | Move the 0.4 guest to the SPEL entrypoint, IDL and client when SPEL supports 0.4 (design note §8, task 15) | The engine guest's entrypoint; the decision code in `engine_core` and `auction_core` does not use SPEL |
| Wallet / core module (`lez_core`) | `lez_core` 0.4.1, `logos-execution-zone-module` `acf0cd50` ([basecamp-app.md](docs/architecture/basecamp-app.md)), used by the v0.2.4 Basecamp module. The 0.4 SDK signs with LEZ's own `wallet` crate at `v0.4.0-rc1` | `lez_core` 0.6.0 (upstream PR #72) changes `send_generic_public_transaction`; the CLI, FFI and module move to it once it is released (design note §8, task 11) | `modules/auction_engine/src/auction_engine_impl.cpp` and `lez_args.h`, where the module calls `send_generic_public_transaction` |

## Reproduce in one command

0.4 engine, end to end on a standalone LEZ v0.4.0-rc1 sequencer (stages `up`, `deploy`, `slice`, `system`, `down`; prerequisites and the build-and-test commands in [v04/README.md](v04/README.md)):

```bash
v04/scripts/e2e.sh
```

v0.2.4 engine:

```bash
scripts/bootstrap-lez.sh && scripts/demo.sh
```

`scripts/bootstrap-lez.sh` builds the LEZ v0.2.4 sequencer and wallet once (about 20 min cold, not yet timed on a clean machine). `scripts/demo.sh`
then starts a standalone sequencer, deploys the programs and runs liquidation, bids, the surplus and debt auctions,
a price move and the freeze path with checks. Prerequisites: Linux or WSL Ubuntu, the risc0 toolchain through
`rzup` (pinned versions in [DEMO.md](DEMO.md)), port 3040 free, at least 20 GB free disk (3 GB on `C:` under WSL).
On the author's machine (WSL, repository on `/mnt/c`) run it as `scripts/wsl-env.sh scripts/demo.sh`. Flags, a
recorded run and the exit codes are in [DEMO.md](DEMO.md). The same scenario on the public v0.2.4 legacy testnet:
`scripts/testnet-legacy.sh` ([testnet-legacy.md](docs/architecture/testnet-legacy.md)).

## Basecamp app

`apps/auctions` (a `ui_qml` app) and `modules/auction_engine` (its core module) put the engine in Basecamp. They
sign through the `lez_core` 0.4.1 wallet module (`acf0cd50`). To install them as `.lgx` packages into a clean
Basecamp profile and run the whole demo as UI actions in the full Basecamp (WSL, Nix):

```bash
scripts/basecamp/install-lgx.sh /tmp/auctions-profile --setup --test apps/auctions/tests/story.mjs
```

Without `--test`, Basecamp stays open on a fresh localnet. `scripts/basecamp/run-app.sh` runs the app in
Basecamp's standalone mode for development. A profile with a newer `lez_core` (0.5.x, from Basecamp's catalogue)
is unsupported. Details, pins and recorded runs: [docs/architecture/basecamp-app.md](docs/architecture/basecamp-app.md);
the GUI run in [DEMO.md](DEMO.md#gui-run-basecamp).

## Run the tests

The 0.4 engine's tests run from `v04/` with `cargo test --workspace` (Rust 1.98.1 and the risc0 toolchain; see [v04/README.md](v04/README.md)). The commands below are for the v0.2.4 root workspace.

Git Bash on Windows cannot resolve the LEZ and risc0 git dependencies of the workspace (`programs/UPSTREAM.md`), so the core checks run in Linux/WSL through the wrapper (ADR-025, `.github/workflows/core.yml`). Rust 1.94.0 (pinned by `rust-toolchain.toml`) and Python 3 are enough. Format is checked only on our crates with `-p` (vendored code keeps its upstream formatting). From the repository root in WSL:

```bash
scripts/wsl-cargo.sh fmt -p auction_core -p auction_engine_core -p auction_engine_program -p engine_integration_tests -p auction_sdk -p auction_cli -- --check
scripts/wsl-cargo.sh clippy --locked -p auction_core --all-targets -- -D warnings
PROPTEST_CASES=5000 scripts/wsl-cargo.sh test --locked -p auction_core
scripts/wsl-cargo.sh run --locked -q -p auction_core --example vectors > /tmp/vectors.json && python3 tools/differential/check.py /tmp/vectors.json
```

From Git Bash, wrap each one as shown under "Build and test the programs (WSL)" below (`MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh ...'`). The last line is the differential check: the example prints test vectors from the Rust core and `tools/differential/check.py` replays them against an independent Python model. CI additionally runs `PROPTEST_CASES=2000 cargo test --locked -p auction_core --test invariants --test s5_pause_onset`, and builds the guests with the pinned risc0 toolchain to run the program, SDK, CLI and vendored suites, the IDL drift check and the hook patch-series check. The job list, runner facts and local commands are in [docs/architecture/ci.md](docs/architecture/ci.md).

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
- 0.4 engine: [docs/v04-port-design.md](docs/v04-port-design.md) (design note and status table), [v04/README.md](v04/README.md), [v04/CYCLES.md](v04/CYCLES.md)
- Refusal codes (every `EngineError`, its wire code and the tests that prove it; generated): [docs/architecture/refusal-codes.md](docs/architecture/refusal-codes.md)
- F8 parameter audit (every constant in the economic code: init parameter or safety bound): [docs/architecture/f8-parameters.md](docs/architecture/f8-parameters.md)
- Pricing model (collateral auction pricing, bad feeds, extractable value, liquidation cost on 0.4): [docs/architecture/pricing-model.md](docs/architecture/pricing-model.md)
- Research notes: [liquidation engines on EVM chains](docs/research/2026-11-07-evm-engines.md), [on non-EVM chains](docs/research/2026-11-07-non-evm-engines.md), [the stablecoin CDP, oracle and hook design](docs/research/research-2-stablecoin.md), [Basecamp packaging](docs/research/research-4-basecamp-packaging.md), [open questions](docs/research/research-6-open-questions.md)
- Spikes: [docs/spikes/](docs/spikes/) ([S1](docs/spikes/S1.md), [S2](docs/spikes/S2.md), [S3](docs/spikes/S3.md), [S4](docs/spikes/S4.md), [S5](docs/spikes/S5.md), [S6](docs/spikes/S6.md), [S7](docs/spikes/S7.md)); spike code in [spikes/](spikes/)
- Architecture: [docs/architecture/core.md](docs/architecture/core.md) (auction state machine), [docs/architecture/math.md](docs/architecture/math.md) (fixed-point arithmetic), [docs/architecture/hooks.md](docs/architecture/hooks.md) (stablecoin hooks), [docs/architecture/programs.md](docs/architecture/programs.md) (engine program, Plan 3 interfaces), [docs/architecture/sdk.md](docs/architecture/sdk.md) (SDK), [docs/architecture/security-checklist.md](docs/architecture/security-checklist.md) (per-instruction checklist)
- Localnet smoke test: [docs/architecture/localnet.md](docs/architecture/localnet.md); legacy testnet run: [docs/architecture/testnet-legacy.md](docs/architecture/testnet-legacy.md); deployed program ids and reference-price log: [DEPLOYMENTS.md](DEPLOYMENTS.md)
- Threat model: [THREAT_MODEL.md](THREAT_MODEL.md)
- RFP-014 requirements mapped to evidence, milestones and descopes: [REQUIREMENTS.md](REQUIREMENTS.md) (checked by `scripts/check-requirements.py`)
- Submission record and hashes: [SUBMISSION.md](SUBMISSION.md); one-command demo: [DEMO.md](DEMO.md); cycles and transaction sizes: [CYCLES.md](CYCLES.md)
- Privacy stub: [docs/PRIVACY.md](docs/PRIVACY.md); LEZ v0.3 port plan (written before the 0.4 port, not started): [docs/v03-port-plan.md](docs/v03-port-plan.md)
- Upstream and outreach drafts (unposted, unfiled): [docs/outreach/](docs/outreach/)
- Plans: [docs/superpowers/plans/](docs/superpowers/plans/)

## Licence

MIT OR Apache-2.0.
