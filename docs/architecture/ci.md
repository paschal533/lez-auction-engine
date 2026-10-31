# Continuous integration

Workflows: `.github/workflows/core.yml` (every push and pull request), `fuzz.yml` (weekly), `guests.yml` (every push and pull request that touches a build input of the guests, weekly on Sunday at 04:05 UTC and on dispatch), `real-proofs.yml` and `e2e.yml` (pushes to `main`, tags where noted, schedule and dispatch), `basecamp-module.yml` (every push and pull request that touches `crates/auction_ffi`, `modules/`, `apps/` or the root flake, and on dispatch; Plan 4). Everything the hosted jobs run can be run locally in WSL; the commands are listed per job below.

Schedules are weekly, on Sunday (UTC), not nightly: the repository is private, and its 2,000 free Actions minutes a month would not cover the estimated 100 to 190 minutes a night of the scheduled jobs. The job, artifact and variable names (`fuzz-nightly`, `real-proof-nightly`, `real-proofs-nightly`, `NIGHTLY_TIER`, `PPE_NIGHTLY`) keep the word "nightly" so that `REQUIREMENTS.md` and the evidence keep pointing at the same jobs; read it as "scheduled". Every scheduled job can also be dispatched by hand.

## Jobs

| Job / action | Where | What it covers |
|--------------|-------|----------------|
| `core` | `core.yml` | Format of our crates, clippy and tests of `auction_core`, the property tests with `PROPTEST_CASES=2000`, the Rust/Python differential check, the preflight script tests of `scripts/demo.sh` and `scripts/testnet-legacy.sh` (stubbed, no network). No risc0 toolchain. |
| `programs` | `core.yml` | The guests built with the pinned risc0 toolchain; workspace clippy; the engine, real-guest integration, SDK, CLI and `auction_ffi` suites; the vendored stablecoin and token suites; the FFI header drift check (R9); the cycle regression check (ADR-048). |
| `requirements` | `core.yml` | `scripts/check-requirements.py REQUIREMENTS.md`: every RFP-014 requirement has one row, and every test, golden file, script, CI job and document it names exists (an `#[ignore]`d test must be marked `[ignored]`); then the checker's own tests. It checks that evidence exists, not that a hosted job has run. |
| `idl-drift` | `core.yml` | `tools/idl-gen` against both guest sources, diffed against the committed IDLs. |
| `hook-patches` | `core.yml` | The exported hook patch series applied to pristine lez-programs, compared with `programs/stablecoin`. |
| `coverage` | `core.yml` | `cargo llvm-cov` over our Rust crates (five in Plan 3, plus `auction_ffi` from Plan 4), then `scripts/check-coverage.py`: at least 80 % of lines per crate (ADR-049). |
| `module-and-qml-tests` | `basecamp-module.yml` | Plan 4: the `auction_engine` core module's unit tests (`nix build ./modules/auction_engine#unit-tests`, LOGOS_CMOCK with a mocked `auction_ffi` and `lez_core`) and the Auctions app's QML tests (`scripts/basecamp/qml-tests.sh`, offscreen, design system `88330d04`, ADR-054). Nix with the Logos binary caches. The story e2e is not a CI job (spec, designer's call 6). |
| `fuzz-nightly` | `fuzz.yml` | Weekly (Sunday 01:33 UTC) and on dispatch: `scripts/fuzz-smoke.sh 300` runs the five `cargo-fuzz` targets of `fuzz/` for 300 s each on the nightly toolchain. |
| `guest-repro` | `guests.yml` | Reproducible guests (ADR-047): our two guests rebuilt in Docker and compared byte for byte with `artifacts/guests`; every committed binary checked against `manifest.json`; the three upstream guests rebuilt only on the scheduled (weekly) and manual runs. |
| `real-proof-push` | `real-proofs.yml` | Every push to `main`: `prove_settle_debt` proved with the risc0 prover (`RISC0_DEV_MODE=0`, po2 19) and its receipt verified; `scripts/check-proof-report.py` fails the job if the proof report is missing, empty, unverified or lacks `settle_debt` (ADR-045, tier B). |
| `real-proof-nightly` | `real-proofs.yml` | Weekly (Sunday 02:17 UTC), on dispatch and on `v*` / `rfp014-*` tags, on a self-hosted runner only: the whole ADR-036 suite proved, every receipt verified. Off unless the repository variable `REAL_PROOF_SELF_HOSTED` is `true` (ADR-045, tier C). |
| `localnet-e2e` | `e2e.yml` | `scripts/bootstrap-lez.sh` then `scripts/demo.sh --sequencer-dev-mode N` against a standalone LEZ v0.2.4 sequencer: every push to `main` with `RISC0_DEV_MODE=0` (RFP S2), weekly (Sunday 03:41 UTC) and on dispatch with both modes (matrix `sequencer_dev_mode`). Artifact `demo-<mode>`: `demo-summary.txt` and the sequencer log. |
| `install-risc0` | `.github/actions/install-risc0` | Composite action used by `programs`, `coverage`, `guest-repro`, both real-proof jobs and `localnet-e2e`: rzup and the pinned components, cached in `~/.risc0`. |

## What each job runs

`core` (`ubuntu-22.04`, Rust 1.94.0):

```bash
cargo fmt -p auction_core -p auction_engine_core -p auction_engine_program -p engine_integration_tests -p auction_sdk -p auction_cli -p auction_ffi -- --check
cargo clippy --locked -p auction_core --all-targets -- -D warnings
cargo test --locked -p auction_core
PROPTEST_CASES=2000 cargo test --locked -p auction_core --test invariants --test s5_pause_onset
cargo run --locked -q -p auction_core --example vectors > /tmp/vectors.json && python3 tools/differential/check.py /tmp/vectors.json
scripts/tests/demo-preflight.sh
scripts/tests/testnet-legacy-preflight.sh
```

`programs` (`ubuntu-22.04`, 150 minute limit, `RISC0_DEV_MODE=1`). It runs `scripts/ci/free-disk.sh`, installs clang, libclang, pkg-config and libssl, installs Rust 1.94.0 and the pinned risc0 components, then:

```bash
RISC0_SKIP_BUILD=1 cargo clippy --locked --workspace --all-targets --exclude integration_tests -- -D warnings
cargo test --locked -p auction_engine_core -p auction_engine_program -p engine_integration_tests -p auction_sdk -p auction_cli -p auction_ffi --features auction_sdk/testing,auction_sdk/rpc
cargo test --locked -p stablecoin_core -p stablecoin_program -p token_program -p integration_tests
RISC0_SKIP_BUILD=1 scripts/ci/ffi-header-drift.sh
scripts/cycles.sh "$RUNNER_TEMP/cycles.jsonl" && python3 scripts/check-cycles.py cycles/baseline.json "$RUNNER_TEMP/cycles.jsonl"
python3 -m unittest discover -s scripts/tests -v
```

`scripts/ci/ffi-header-drift.sh` regenerates `crates/auction_ffi/include/auction_ffi.h` with cbindgen and fails with the diff if the committed header differs. It runs in `programs` rather than `core`, because building `auction_ffi` pulls in the risc0 graph, which `core` does not have (R9).

The last two commands are the cycle gate (ADR-048): `scripts/cycles.sh` runs the `chains.rs` scenarios with `CYCLES_OUT` set, `scripts/check-cycles.py` compares the result with `cycles/baseline.json` (more than 10 % growth, an added or removed label, more than 10 executions or an execution over 32 Mi fails), and the unit tests cover the checker itself. The step uploads `cycles.jsonl` as the `cycles` artifact. Regenerate the baseline and `CYCLES.md` with `python3 scripts/check-cycles.py cycles/baseline.json /tmp/c.jsonl --write-baseline --write-md CYCLES.md`.

The vendored upstream `integration_tests` crate is excluded from clippy (it keeps upstream style) and runs in the last command with our hooks appended (spec section 1.5).

`guest-repro` (`ubuntu-22.04`, 180 minute limit; triggers: push and pull request touching a build input of the guests (the path filter lists the root manifest and lock, `programs/**`, `crates/auction_core`, `patches/`, the build and check scripts, `.dockerignore`, the packager, `artifacts/guests/**` and the workflow itself), schedule `5 4 * * *`, and `workflow_dispatch`). It runs `scripts/ci/free-disk.sh`, installs Rust 1.94.0 and the pinned risc0 components, restores the `programs` cache, then:

```bash
scripts/build-guests.sh "$RUNNER_TEMP/guests"
# scheduled (weekly) and dispatch only: rebuild token, twap_oracle and token_mint_authority from lez-programs 64c01210
make -C "$RUNNER_TEMP/lez-programs" build-programs
# push and pull request: copy those three binaries from artifacts/guests (hash-checked only)
cp artifacts/guests/manifest.json "$RUNNER_TEMP/guests/" && scripts/check-guests.sh "$RUNNER_TEMP/guests"
scripts/check-guests.sh
```

The first check requires the Docker rebuild of our two guests (and, on the scheduled run, of the three upstream guests) to equal `artifacts/guests` in sha256 and ImageID; the second checks every committed `*.bin` against `manifest.json`. Hosted runs: 38558398627 at 0dec410 (cold 13m59s) and 38982905129 at 863f0fb, both green; this is the cross-machine evidence that the Docker build is reproducible (it was bit-identical locally too). The scheduled upstream rebuild (`make build-programs`, six guests, about 70 min cold on 8 cores in S1) has not run in CI yet; watch the first scheduled run against the 180 minute limit. Local: `scripts/build-guests.sh` then `scripts/check-guests.sh`. The job restores the `programs` rust-cache but never saves it (`save-if: false`): see "Runner facts".

`idl-drift` (`ubuntu-22.04`): `scripts/ci/idl-drift.sh` regenerates `idl/auction_engine-idl.json` and `idl/stablecoin-idl.json` with `idl-gen` and exits 1 with the diff if either differs from the committed file. It also catches an enum variant whose guest function was never added.

`hook-patches` (`ubuntu-22.04`): clones lez-programs, checks out `64c01210adecd6b557dbd021a3cc94553f243fd8`, then runs `LEZ_PROGRAMS=<clone> scripts/check-hook-patches.sh`.

`coverage` (`ubuntu-22.04`, 120 minute limit, `RISC0_DEV_MODE=1`). It runs `scripts/ci/free-disk.sh`, installs clang, libclang, pkg-config and libssl, Rust 1.94.0 with `llvm-tools-preview`, the pinned risc0 components and `cargo-llvm-cov`, then:

```bash
IGNORE='(/tests/|_tests\.rs$|/testing/|/methods/|/examples/)'
cargo llvm-cov --locked -p auction_core -p auction_engine_core -p auction_engine_program -p auction_sdk -p auction_cli -p auction_ffi --features auction_sdk/testing,auction_sdk/rpc --ignore-filename-regex "$IGNORE" --json --summary-only --output-path "$RUNNER_TEMP/cov.json"
python3 scripts/check-coverage.py "$RUNNER_TEMP/cov.json" --root "$GITHUB_WORKSPACE"
```

The gate is per crate (ADR-049): the checker prints each crate's line coverage and exits 1 naming every crate under 80 % or without data. Test code, fixtures, the guest wrappers and examples are matched by `IGNORE` and not counted. Run it locally inside WSL with the same two commands (prefix the first with `scripts/wsl-env.sh`), then `scripts/wsl-env.sh cargo llvm-cov clean --workspace` to delete the instrumented build.

`fuzz-nightly` (`ubuntu-22.04`, 90 minute limit, schedule `33 1 * * 0` and `workflow_dispatch`; it never runs on push or pull request). The job sets `RISC0_SKIP_BUILD=1`: the fuzzers never prove, so the guest ELFs and the risc0 toolchain (`install-risc0`) are not needed, and the SDK's guest-building dependencies compile without them (checked locally with the variable set on a fresh build). It runs `scripts/ci/free-disk.sh`, installs clang, libclang, pkg-config and libssl, the nightly Rust toolchain and `cargo-fuzz`, then:

```bash
scripts/fuzz-smoke.sh 300
```

A crash fails the job and uploads `fuzz/artifacts` as the `fuzz-crashes` artifact (the procedure is in [sdk.md](sdk.md), "Fuzzing"). A schedule or dispatch trigger only works from the default branch, so the first hosted run of `fuzz-nightly` happens after the merge to `main`; until then the targets have only been run locally (60 s each, inside WSL).

`real-proof-push` (`ubuntu-22.04`, 90 minute limit, `RISC0_DEV_MODE=0`, `AUCTION_PROVE=1`, `AUCTION_PROVE_PO2=19`). It runs `scripts/ci/free-disk.sh`, installs clang, libclang, pkg-config and libssl, Rust 1.94.0 and the pinned risc0 components, builds the test binary with `RISC0_DEV_MODE=1` (building never proves), then:

```bash
PROOF_REPORT="$RUNNER_TEMP/proofs.jsonl" cargo test --locked -p engine_integration_tests --test real_proofs -- --ignored --exact prove_settle_debt --test-threads 1 --nocapture
python3 scripts/check-proof-report.py "$RUNNER_TEMP/proofs.jsonl" --expect settle_debt
```

The proving step has its own 80 minute limit (340 for the nightly job), shorter than the job limit, so the `if: always()` checker and artifact upload still run when it times out. The checker step runs `if: always()`, so a prover killed for memory or by the step timeout fails with "the prover did not finish" instead of an empty log. `proofs.jsonl` is uploaded as `real-proofs-push`. Only the cheapest instruction fits a hosted runner (ADR-045); the end-to-end real-proof requirement (RFP S2) is the `localnet-e2e` job with a `RISC0_DEV_MODE=0` sequencer (`e2e.yml`).

`real-proof-nightly` (`[self-hosted, linux, x64, risc0-prover]`, 350 minute limit). Same steps as above with `--test chains -- --test-threads 1` (all three chain-budget tests under `AUCTION_PROVE=1`) and `--expect 'liquidate,take_debt,check_solvency(freeze)'`; uploads `real-proofs-nightly`. The runner is provisioned as described in [real-proofs.md](real-proofs.md); it has no `free-disk.sh` or apt step. The job is skipped, and CI stays green, while `REAL_PROOF_SELF_HOSTED` is unset. `scripts/prove-suite.sh` is the local equivalent. `.github/actionlint.yaml` declares the `risc0-prover` runner label so `actionlint` accepts it. Neither job was observed on a hosted runner in Plan 3b: dispatch and schedule need the workflow on the default branch (the push job runs on the first push to `main`), so its first hosted timing is recorded in [real-proofs.md](real-proofs.md) after the merge.

`localnet-e2e` (`ubuntu-22.04`, 240 minute limit, matrix `sequencer_dev_mode`: `["0"]` on push, `["0","1"]` on schedule and dispatch). It runs `scripts/ci/free-disk.sh`, installs the build packages (including `libpcsclite-dev` for the wallet), Rust 1.94.0 and the pinned risc0 components, restores a cache of the two LEZ binaries (`sequencer_service`, `wallet`; key `lez-<LEZ_REV>-ubuntu-22.04-bin-v1`, where `<LEZ_REV>` is the full 40-character commit derived from `scripts/bootstrap-lez.sh`), then:

```bash
scripts/bootstrap-lez.sh
scripts/demo.sh --sequencer-dev-mode "$N"
```

`DEMO_MIN_HOME_GIB=5` (the default of 20 protects a WSL disk that grows into C:; a hosted runner is expected to have well over 5 GiB free after `free-disk.sh`, which prints `df` before and after, so the first hosted run gives the measured figure). `bootstrap-lez.sh` always runs: it fetches the pinned LEZ commit, which restores the sequencer and wallet configs that a cache hit lacks, and builds only the binary that is missing, so a hit never skips the configs and a miss builds everything. The cache is saved right after a successful bootstrap, so a failing demo keeps the cold build. Bootstrap has a 120 minute step limit and the demo 90, both below the job limit so the `if: always()` summary and artifact steps still run. The artifact holds `demo-summary.txt` and the sequencer log only; the run directory is never uploaded because `wallet-home` holds the throwaway keys, and the step log of `wallet account new` (which prints the throwaway recovery phrase) is not an artifact. Local equivalent: `scripts/wsl-env.sh scripts/demo.sh --sequencer-dev-mode 0` after `scripts/bootstrap-lez.sh`. The cache key carries `LEZ_REV`, read from `scripts/bootstrap-lez.sh` in a step before the restore, so bumping the pin never reuses stale binaries; a `concurrency` group keeps one run per ref at a time. First hosted run (run 39017229879 at 5c77daa, push, `RISC0_DEV_MODE=0`, cold LEZ cache): green in 46m48s; `free-disk.sh` took the root filesystem from 14 GB to 32 GB free; bootstrap 21m44s cold (well inside its 120 minute limit); demo 21m00s, every check `ok`, unbacked debt 42_274. The warm-cache time and the first both-modes scheduled run are still to be recorded.

If a later `localnet-e2e` run fails, what to check (the first hosted run passed on every point below):

1. The `free-disk.sh` `df` output against `DEMO_MIN_HOME_GIB` (5). A disk-guard exit 3 or `No space left on device`: add `/opt/microsoft /usr/local/.ghcup` to `scripts/ci/free-disk.sh`, or set `DEMO_MIN_HOME_GIB` from the measured free space.
2. The bootstrap step time against its 120 minute limit. Over the limit: re-run once, because the binaries are cached only after a successful bootstrap; if it still exceeds the limit, apply the plan's split rule: add a separate job `lez-bootstrap` (timeout 350) that only builds and caches, and make `localnet-e2e` depend on it with `needs: lez-bootstrap`.
3. "sequencer did not open port 3040 within 60 s": read the sequencer log in the `demo-<mode>` artifact and consider a longer wait in `ln_start_sequencer`.

Record new timings in this file and in [real-proofs.md](real-proofs.md).

`requirements` (`ubuntu-22.04`): `python3 scripts/check-requirements.py REQUIREMENTS.md`, then the checker tests. No toolchain is installed; the job takes seconds.

`module-and-qml-tests` (`basecamp-module.yml`, `ubuntu-22.04`, 120 minute limit, Plan 4). It runs `scripts/ci/free-disk.sh`, installs Nix (`cachix/install-nix-action@v31`, flakes on) with the Logos binary caches `cache.nix.logos.co/public` and `logos-co.cachix.org` as extra substituters, then:

```bash
nix build ./modules/auction_engine#unit-tests -L --no-link
scripts/basecamp/qml-tests.sh
```

- The first command builds and runs the core module's C++ tests against LOGOS_CMOCK, with `auction_ffi` and `lez_core` mocked. The pinned builder names this output `unit-tests`; there is no `#tests` output.
- The second runs every `apps/auctions/tests/qml/tst_*.qml` with `qmltestrunner`, offscreen, on the builder's Qt 6.9.2 and design system `88330d04` (ADR-054).
- Without a cache hit, the builder's Qt and logos-cpp-sdk graph builds from source, so the first run's time depends on the caches.
- Hosted run: green on `feat/plan-4` at be82e74 (run 41172947166): 51 module tests and 182 QML tests. Before e9f104a the job failed on hosted runners because `host.sh check` read `/mnt/c`, which exists only in WSL; the C: check is now skipped there.
- The live story (`apps/auctions/tests/story.mjs`) is not a CI job (spec, designer's call 6). It runs locally through `scripts/basecamp/run-app.sh` or `scripts/basecamp/install-lgx.sh` ([basecamp-app.md](basecamp-app.md)).

Plan 4 CI status: the Plan 4 changes to `core.yml` (`auction_ffi` in `core`, `programs` and `coverage`, the header drift step) and `basecamp-module.yml` ran green on hosted runners on `feat/plan-4` at be82e74: `core` 41172947093 (core, programs, coverage, idl-drift, hook-patches, requirements) and `basecamp-module` 41172947166 (51 module tests, 182 QML tests). After the merge they ran green on `main` at 1405ea3 (the Plan 4 merge, PR #3): `core` 41187263086 (core, programs, coverage, idl-drift, hook-patches, requirements), `basecamp-module` 41187263071 (51 module tests, 182 QML tests), `guests` 41187263101, `e2e` 41187263076 and `real-proofs` 41187263065.

## Pinned versions

Never floated, and there is no bare `rzup install` anywhere (it would float the components).

| Component | Version |
|-----------|---------|
| Rust | 1.94.0 |
| rzup: rust | 1.94.1 |
| rzup: cpp | 2024.1.5 |
| rzup: r0vm | 3.0.5 |
| rzup: cargo-risczero | 3.0.5 |
| lez-programs | `64c01210adecd6b557dbd021a3cc94553f243fd8` |

The action ends with `r0vm --version | grep -q 3.0.5`, so a drifted prover fails the job. Its cache key encodes the component versions; change the key when a version changes.

## Runner facts

- Hosted runner `ubuntu-22.04`: 2 vCPU, 7 GiB RAM, about 14 GB free disk.
- Rust cache sharing: `guest-repro` and both `real-proofs.yml` jobs use `shared-key: programs` with `save-if: false`. They restore the `programs` cache but never write it, because cache keys are immutable and a shorter job finishing first on `main` would otherwise save a partial target under the key `programs` needs, leaving `programs` partly cold on every later run. A separate key was not chosen: it would cost a second cold build and cache quota for no gain.
- Job limit 360 minutes; 2000 free minutes a month (Free plan, private repository).
- The heavy job therefore runs `scripts/ci/free-disk.sh` first (removes dotnet, the Android SDK, ghc, the CodeQL toolcache and boost). If `programs` still fails with `No space left on device`, add `/opt/microsoft /usr/local/.ghcup` to that script.

## Running a job locally

The host is Windows; cargo runs in WSL through the wrappers. From Git Bash wrap each command as `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && <command>'`. Run `scripts/disk-guard.sh` before and after anything long.

| Job | Command (inside WSL, repository root) |
|-----|---------------------------------------|
| `programs`, clippy | `RISC0_SKIP_BUILD=1 scripts/wsl-cargo.sh clippy --locked --workspace --all-targets --exclude integration_tests -- -D warnings` |
| `programs`, engine/SDK/CLI | `scripts/wsl-cargo.sh test --locked -p auction_engine_core -p auction_engine_program -p engine_integration_tests -p auction_sdk -p auction_cli --features auction_sdk/testing,auction_sdk/rpc` |
| `programs`, vendored | `scripts/wsl-cargo.sh test --locked -p stablecoin_core -p stablecoin_program -p token_program -p integration_tests` |
| `programs`, cycle gate | `scripts/wsl-env.sh scripts/cycles.sh /tmp/c.jsonl && python3 scripts/check-cycles.py cycles/baseline.json /tmp/c.jsonl` |
| `programs`, checker tests | `python3 -m unittest discover -s scripts/tests -v` |
| `real-proof-push` | `scripts/measure-proof.sh prove_settle_debt 19 3600` (times it and records peak RSS) |
| `real-proof-nightly` | `scripts/prove-suite.sh 19 21600` (hours; the disk guard runs before each test) |
| `requirements` | `python3 scripts/check-requirements.py REQUIREMENTS.md && python3 -m unittest discover -s scripts/tests -p 'test_check_requirements.py' -v` |
| `idl-drift` | `scripts/wsl-env.sh scripts/ci/idl-drift.sh` |
| `hook-patches` | `scripts/wsl-env.sh env LEZ_PROGRAMS=$HOME/lez-spikes/s1/lez-programs scripts/check-hook-patches.sh` |
| `fuzz-nightly` | `scripts/wsl-env.sh scripts/fuzz-smoke.sh 60`, then `rm -rf "$HOME/lez-target/plan2/fuzz"` |
| `programs`, FFI header drift | `RISC0_SKIP_BUILD=1 scripts/wsl-env.sh ./scripts/ci/ffi-header-drift.sh` |
| `module-and-qml-tests`, module | `scripts/basecamp/host.sh check && nix build ./modules/auction_engine#unit-tests -L --no-link` (needs at least 10 GB free on C:) |
| `module-and-qml-tests`, QML | `scripts/basecamp/qml-tests.sh` (from Git Bash it re-runs itself through WSL) |

Validate the workflow files with `actionlint` 1.7.7: `actionlint` (it reads every workflow and `.github/actionlint.yaml`).

## Wall times

Local, WSL dev machine (warm target directory `~/lez-target/plan2`):

| Command | Wall time |
|---------|-----------|
| `programs`, clippy (`--locked`, workspace) | 19 s |
| `programs`, engine/SDK/CLI suites | 814 s (about 13.6 min) |
| `programs`, vendored stablecoin and token suites | 53 s |
| `idl-drift` (`scripts/ci/idl-drift.sh`) | 15 s |
| `hook-patches` | 4 s |

Hosted wall times, `ubuntu-22.04` (first hosted runs of `feat/plan-3b`, cold cache and warm cache):

| Job | Cold | Warm |
|-----|------|------|
| `programs` | 34m35s | 23m48s |
| `core` | 1m32s | 1m05s |
| `idl-drift` | 2m37s | 55s |
| `hook-patches` | 9s | 6s |
| `coverage` | 10m07s | 4m39s (run 38558398381) |

First hosted runs on `main` (5c77daa): `real-proof-push` 23m56s (run 39017229866 at 5c77daa: test build 11m17s, proving step 9m10s, `settle_debt: 1 executions, 2 segments, 543 s, all receipts verified`); `localnet-e2e` 46m48s (run 39017229879, cold LEZ cache); `guests` 22m51s (run 39017229906); `core` 44m51s (run 39017229894, cold `main` cache). `real-proof-nightly` was skipped (`REAL_PROOF_SELF_HOSTED` unset).
