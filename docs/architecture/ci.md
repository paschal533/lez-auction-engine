# Continuous integration

Workflow: `.github/workflows/core.yml` (runs on every push and pull request). Everything the hosted jobs run can be run locally in WSL; the commands are listed per job below.

## Jobs

| Job / action | Where | What it covers |
|--------------|-------|----------------|
| `core` | `core.yml` | Format of our crates, clippy and tests of `auction_core`, the property tests with `PROPTEST_CASES=2000`, the Rust/Python differential check. No risc0 toolchain. |
| `programs` | `core.yml` | The guests built with the pinned risc0 toolchain; workspace clippy; the engine, real-guest integration, SDK and CLI suites; the vendored stablecoin and token suites; the cycle regression check (ADR-048). |
| `idl-drift` | `core.yml` | `tools/idl-gen` against both guest sources, diffed against the committed IDLs. |
| `hook-patches` | `core.yml` | The exported hook patch series applied to pristine lez-programs, compared with `programs/stablecoin`. |
| `coverage` | `core.yml` | `cargo llvm-cov` over the five Rust crates, then `scripts/check-coverage.py`: at least 80 % of lines per crate (ADR-049). |
| `fuzz-nightly` | `fuzz.yml` | Nightly (01:33 UTC) and on dispatch: `scripts/fuzz-smoke.sh 300` runs the five `cargo-fuzz` targets of `fuzz/` for 300 s each on the nightly toolchain. |
| `install-risc0` | `.github/actions/install-risc0` | Composite action used by `programs`: rzup and the pinned components, cached in `~/.risc0`. |

## What each job runs

`core` (`ubuntu-22.04`, Rust 1.94.0):

```bash
cargo fmt -p auction_core -p auction_engine_core -p auction_engine_program -p engine_integration_tests -p auction_sdk -p auction_cli -- --check
cargo clippy --locked -p auction_core --all-targets -- -D warnings
cargo test --locked -p auction_core
PROPTEST_CASES=2000 cargo test --locked -p auction_core --test invariants --test s5_pause_onset
cargo run --locked -q -p auction_core --example vectors > /tmp/vectors.json && python3 tools/differential/check.py /tmp/vectors.json
```

`programs` (`ubuntu-22.04`, 150 minute limit, `RISC0_DEV_MODE=1`). It runs `scripts/ci/free-disk.sh`, installs clang, libclang, pkg-config and libssl, installs Rust 1.94.0 and the pinned risc0 components, then:

```bash
RISC0_SKIP_BUILD=1 cargo clippy --locked --workspace --all-targets --exclude integration_tests -- -D warnings
cargo test --locked -p auction_engine_core -p auction_engine_program -p engine_integration_tests -p auction_sdk -p auction_cli --features auction_sdk/testing,auction_sdk/rpc
cargo test --locked -p stablecoin_core -p stablecoin_program -p token_program -p integration_tests
scripts/cycles.sh "$RUNNER_TEMP/cycles.jsonl" && python3 scripts/check-cycles.py cycles/baseline.json "$RUNNER_TEMP/cycles.jsonl"
python3 -m unittest discover -s scripts/tests -v
```

The last two commands are the cycle gate (ADR-048): `scripts/cycles.sh` runs the `chains.rs` scenarios with `CYCLES_OUT` set, `scripts/check-cycles.py` compares the result with `cycles/baseline.json` (more than 10 % growth, an added or removed label, more than 10 executions or an execution over 32 Mi fails), and the unit tests cover the checker itself. The step uploads `cycles.jsonl` as the `cycles` artifact. Regenerate the baseline and `CYCLES.md` with `python3 scripts/check-cycles.py cycles/baseline.json /tmp/c.jsonl --write-baseline --write-md CYCLES.md`.

The vendored upstream `integration_tests` crate is excluded from clippy (it keeps upstream style) and runs in the last command with our hooks appended (spec section 1.5).

`idl-drift` (`ubuntu-22.04`): `scripts/ci/idl-drift.sh` regenerates `idl/auction_engine-idl.json` and `idl/stablecoin-idl.json` with `idl-gen` and exits 1 with the diff if either differs from the committed file. It also catches an enum variant whose guest function was never added.

`hook-patches` (`ubuntu-22.04`): clones lez-programs, checks out `64c01210adecd6b557dbd021a3cc94553f243fd8`, then runs `LEZ_PROGRAMS=<clone> scripts/check-hook-patches.sh`.

`coverage` (`ubuntu-22.04`, 120 minute limit, `RISC0_DEV_MODE=1`). It runs `scripts/ci/free-disk.sh`, installs clang, libclang, pkg-config and libssl, Rust 1.94.0 with `llvm-tools-preview`, the pinned risc0 components and `cargo-llvm-cov`, then:

```bash
IGNORE='(/tests/|_tests\.rs$|/testing/|/methods/|/examples/)'
cargo llvm-cov --locked -p auction_core -p auction_engine_core -p auction_engine_program -p auction_sdk -p auction_cli --features auction_sdk/testing,auction_sdk/rpc --ignore-filename-regex "$IGNORE" --json --summary-only --output-path "$RUNNER_TEMP/cov.json"
python3 scripts/check-coverage.py "$RUNNER_TEMP/cov.json" --root "$GITHUB_WORKSPACE"
```

The gate is per crate (ADR-049): the checker prints each crate's line coverage and exits 1 naming every crate under 80 % or without data. Test code, fixtures, the guest wrappers and examples are matched by `IGNORE` and not counted. Run it locally inside WSL with the same two commands (prefix the first with `scripts/wsl-env.sh`), then `scripts/wsl-env.sh cargo llvm-cov clean --workspace` to delete the instrumented build.

`fuzz-nightly` (`ubuntu-22.04`, 90 minute limit, schedule `33 1 * * *` and `workflow_dispatch`; it never runs on push or pull request). It runs `scripts/ci/free-disk.sh`, installs clang, libclang, pkg-config and libssl, the nightly Rust toolchain and `cargo-fuzz`, then:

```bash
scripts/fuzz-smoke.sh 300
```

A crash fails the job and uploads `fuzz/artifacts` as the `fuzz-crashes` artifact (the procedure is in [sdk.md](sdk.md), "Fuzzing"). A schedule or dispatch trigger only works from the default branch, so the first hosted run of `fuzz-nightly` happens after the merge to `main`; until then the targets have only been run locally (60 s each, inside WSL).

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
| `idl-drift` | `scripts/wsl-env.sh scripts/ci/idl-drift.sh` |
| `hook-patches` | `scripts/wsl-env.sh env LEZ_PROGRAMS=$HOME/lez-spikes/s1/lez-programs scripts/check-hook-patches.sh` |
| `fuzz-nightly` | `scripts/wsl-env.sh scripts/fuzz-smoke.sh 60`, then `rm -rf "$HOME/lez-target/plan2/fuzz"` |

Validate the workflow file with `actionlint` 1.7.7: `actionlint .github/workflows/core.yml`.

## Wall times

Local, WSL dev machine (warm target directory `~/lez-target/plan2`):

| Command | Wall time |
|---------|-----------|
| `programs`, clippy (`--locked`, workspace) | 19 s |
| `programs`, engine/SDK/CLI suites | 814 s (about 13.6 min) |
| `programs`, vendored stablecoin and token suites | 53 s |
| `idl-drift` (`scripts/ci/idl-drift.sh`) | 15 s |
| `hook-patches` | 4 s |

Hosted wall times (cold and warm): recorded after the first hosted run of this branch.
