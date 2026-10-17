# Plan 3b: CI, Real Proofs, Reproducible Builds and Evidence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the Plan 3a software into submission evidence. The work covers CI over every Rust crate with the risc0 toolchain, real proofs tiered to what runners can carry, reproducible Docker guest builds with refreshed ImageIDs, `CYCLES.md` with a regression gate, coverage and fuzzing, a localnet price publisher, a one-command demo that reaches the F4 debt auction and the freeze path, a CI end-to-end run with `RISC0_DEV_MODE=0`, `REQUIREMENTS.md` with its checker, a red-team pass, and draft upstream and outreach texts.

**Architecture:**
- **Measure first (Task 1).** A cheap, local, timed measurement of real proofs and of a sequencer run with `RISC0_DEV_MODE=0` decides how real proofs are tiered in CI (ADR-045). On LEZ v0.2.4 the sequencer executes public transactions without proving them (`lee/state_machine/src/program/mod.rs` uses `default_executor`; only the privacy circuit and the wallet's private path call the prover). This gives two kinds of real-proof evidence:
  - (a) RFP S2 / spec §9: end-to-end runs against a standalone sequencer with `RISC0_DEV_MODE=0` (Tasks 1 and 14).
  - (b) Real risc0 receipts, produced and verified, for every execution of a transaction's chained calls. These are the receipts a proving pipeline (the v0.3 port, the private path) must produce. The harness is the existing `measure.rs` executor replay switched to the prover by `AUCTION_PROVE=1` (Tasks 1 and 7).
- **CI is layered by cost.** `core.yml` runs on every push and pull request: the `auction_core`, `programs`, `idl-drift`, `hook-patches`, `coverage` and `requirements` jobs. Heavy workflows run on push to `main`, nightly and on tags: `real-proofs.yml`, `guests.yml`, `fuzz.yml` and `e2e.yml`. Everything runs on GitHub-hosted runners. A self-hosted runner appears only as a documented, disabled-by-default fallback, if Task 1's ruling requires one.
- **Reproducible guests (Task 5).** Our two guests (stablecoin with hooks, auction_engine) are built in the upstream builder image with the upstream recipe. The five deployable `.bin` files are committed under `artifacts/guests/` with a manifest of sha256 values and ImageIDs. `guests.yml` rebuilds them and requires byte equality. Deployment files and `DEPLOYMENTS.md` carry the Docker ImageIDs. In-process tests keep using the native `*_methods` ELFs (ADR-047).
- **Localnet evidence.** These pieces share one bash library (`scripts/lib/localnet.sh`):
  - the publisher (`auction-cli dev oracle-publish`, rotating the RAY-scale oracle, ADR-050);
  - the demo parameters, pinned on the real guests (`demo_scenario.rs`, ADR-051);
  - the one-command `scripts/demo.sh`;
  - the CI job that runs it.
- **Parked Plan 3a items fold into their owners:**
  - N1, N2, N4 and the panic-after-send test go to Task 2.
  - N3, M6 and the read-only `dev` exemption go to Task 3.
  - The `idl-gen` diff and the uncovered crates go to Task 4.
  - M7 (re-running the smoke) goes to Task 12.

**Tech Stack:**
- Rust 1.94.0.
- LEZ v0.2.4 (`lee`, `lee_core`, `clock_core`, `common`, `sequencer_service_rpc`).
- risc0-zkvm `=3.0.5`, with external prover `r0vm` 3.0.5 from rzup (rust 1.94.1, cpp 2024.1.5, cargo-risczero 3.0.5).
- SPEL `1ef0500f` (`idl-gen` only).
- clap 4, serde/serde_json, borsh 1.5.
- Docker with `risczero/risc0-guest-builder:r0.1.88.0`.
- GitHub Actions on `ubuntu-22.04`.
- Python 3 for the checkers, with `unittest`.
- cargo-llvm-cov, cargo-fuzz (nightly toolchain) and cargo-audit.
- WSL Ubuntu-20.04 for every local build and test.

**Spec:** `docs/superpowers/specs/2026-10-01-rfp014-auction-engine-poc-design.md` (v5), as amended by `docs/design-log.md` ADR-018 to ADR-044. Before any task, read:
- spec §1.1, §1.3, §1.4, §3.2, §5.2, §5.11, §9, §10, §11, §12 and §16;
- ADR-018, ADR-023, ADR-027, ADR-028, ADR-036, ADR-037 and ADR-040 to ADR-044;
- `THREAT_MODEL.md`, `DEPLOYMENTS.md`;
- `docs/architecture/{programs,sdk,localnet}.md`;
- the "Plan 3b scope (next plan)" section at the end of `docs/superpowers/plans/2026-10-09-plan-3a-engine-hardening-sdk-cli.md`;
- the parked items in `.superpowers/sdd/2026-10-09-plan-3a-engine-hardening-sdk-cli/{progress.md,final-review.md,final-rereview.md}`.

## Global Constraints

- **Pins** (spec §3.1):
  - LEZ v0.2.4 `47eba256479f6f785acbd138834340703cd03401`.
  - SPEL `1ef0500f8fc8ce3ddf95523726ae159db83f744f`.
  - risc0-zkvm `=3.0.5`, Rust `1.94.0`.
  - lez-programs (vendored stablecoin) `64c01210adecd6b557dbd021a3cc94553f243fd8`.
  - rzup components: rust `1.94.1`, cpp `2024.1.5`, r0vm `3.0.5`, cargo-risczero `3.0.5`. These are the versions of `lez/.github/docker/ci.Dockerfile`, and a bare `rzup install` would float them, so never run it.
  - Builder image `risczero/risc0-guest-builder:r0.1.88.0`, pinned by digest in Task 5.
- **Dev machine** (Windows 11 + WSL Ubuntu-20.04, 8 cores, 11 GiB RAM, 16 GiB swap):
  - The C: drive has about 6 GB free. The WSL disk and Docker Desktop's 44 GB vhdx both live on C: and grow into it.
  - **Every task runs `scripts/disk-guard.sh` (created in Task 1) before and after each build, Docker, proof, coverage or fuzz step, and stops if it exits 3** (C: under 3 GiB free, or the WSL home under 20 GiB). Docker steps use `scripts/disk-guard.sh 5` before they start.
  - Never create a new cargo target directory. Every cargo command goes through `scripts/wsl-cargo.sh` or `scripts/wsl-env.sh` (Task 1), which set `CARGO_TARGET_DIR=$HOME/lez-target/plan2`. Subdirectories that cargo-llvm-cov (`llvm-cov-target`) and cargo-fuzz (`fuzz`) create inside it are allowed and are deleted after use.
  - Never delete or modify `$HOME/lez-spikes/s1`: it holds the S1 sequencer and wallet builds, the S1 Docker-built upstream guests and `env.sh`.
  - Never run two proving jobs at once, or a proving job next to a cargo build.
  - Run local proofs only through `scripts/measure-proof.sh`, which adds a timeout and records peak RSS.
- **Command form** (from Git Bash; identical to Plan 3a):
  `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh <cargo args>'`
  - Scripts that call cargo themselves run as `scripts/wsl-env.sh <script> <args>` inside the same `bash -lc '...'`.
  - Long steps (first guest builds, proofs, coverage, fuzzing, Docker, localnet runs) use the Bash tool's `run_in_background` with a 3600000 ms timeout. Proofs use the timeout that `measure-proof.sh` is given.
- **Lint gate** (every task, before its commit step):
  `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && RISC0_SKIP_BUILD=1 scripts/wsl-cargo.sh clippy --workspace --all-targets --exclude integration_tests -- -D warnings'`
- **Format gate** (our crates only; vendored code keeps upstream formatting):
  `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh fmt -p auction_core -p auction_engine_core -p auction_engine_program -p engine_integration_tests -p auction_sdk -p auction_cli -- --check'`
- **Encoding** (spec §3.2, ADR-018, ADR-040):
  - Every encoder uses the program's own instruction enum: first word = enum index, never the IDL position.
  - `idl/stablecoin-idl.json` differs from `stablecoin_core::Instruction` for IDL positions 8..=18; the pinned position→enum table is `0,1,2,3,4,5,6,7,17,18,14,8,9,11,12,13,10,15,16,19,20,21`.
  - New host plan: `SetMarketPriceOracle`, enum index **10** (IDL position 16).
- **Vendored code:**
  - No task edits `programs/stablecoin/**`, `programs/token/**`, `programs/twap_oracle/**` or `programs/upstream_integration_tests/**`.
  - If a red-team fix (Task 16) must touch `programs/stablecoin/**`, follow ADR-025: a hook commit touches only what upstream needs; run `scripts/export-hook-patches.sh` and then `scripts/check-hook-patches.sh`. Repo-only changes are `[local]` commits.
  - Never reformat vendored code.
- **Guest changes refresh the ImageIDs.** After Task 5, any change under `programs/auction_engine/**` or `programs/stablecoin/**` that changes a guest must rerun `scripts/build-guests.sh` and `scripts/check-guests.sh --write` in the same task, and update `DEPLOYMENTS.md` and `deployments/localnet.json`.
- **Secrets:** keystore contents and keys are never printed, logged or committed. The localnet wallet under `$HOME/lez-target/*/wallet-home` is throwaway and never leaves WSL. No network access in unit or golden tests; live-node tests stay `#[ignore]`d.
- **No external posting.**
  - Outreach and upstream items are drafts in `docs/outreach/`.
  - No `gh issue create`, `gh pr create` against any other repository, no posts to Discord or forums, no releases, no visibility changes.
  - Pushing the feature branch to the private `origin` to observe CI is allowed only per Open decision 1. Nothing else is pushed.
- **ADR numbering.** ADRs continue after ADR-044, in task order:
  - ADR-045: real proofs on v0.2.4 and the CI tiers (Task 1, finalised in Task 7).
  - ADR-046: chain identity for dev commands (Task 3).
  - ADR-047: reproducible guest builds (Task 5).
  - ADR-048: cycle baseline and regression rule (Task 6).
  - ADR-049: coverage gate scope (Task 8).
  - ADR-050: localnet price publisher by rotation (Task 10).
  - ADR-051: demo scenario parameters (Task 11).
  - Amendments go under the amended ADR as `**Amended (Plan 3b, Task N)**`. ADRs are appended at the end of `docs/design-log.md` in the existing format: `## ADR-0NN: Title`, then **Context**, **Options**, **Decision**, **Trade-offs**, **Status**, then `---`.
- **Files:**
  - Every source file stays under 800 lines (`docs/design-log.md` and plan documents are append-only and exempt).
  - `#![forbid(unsafe_code)]` in every Rust crate this plan creates.
  - UTF-8 + LF (`.gitattributes` enforces `eol=lf`; `*.bin binary`).
  - Python scripts use the standard library only.
- **Docs are updated in the same task as the code they describe.** Each task's last code step lists the doc edits.
- **Commits:**
  - Conventional format `<type>: <description>` (feat, fix, refactor, docs, test, chore, perf, ci).
  - **No `Co-Authored-By` line and no AI attribution of any kind.**
  - Every commit step is "Commit (controller, GPG signing on Windows)": the implementer stages the listed files and reports; the controller commits from Git Bash on Windows (GPG-signed), never from WSL.
- **Ruling points.** Task 1 Step 9 and Task 16 Step 3 end with an explicit ruling by the controller, recorded in the ledger as `Ruling: ...`. A task that depends on a ruling (Task 7 on Task 1; Task 16's fix steps on its findings) does not start before that ruling exists.

## Review Focus

These are the inputs and conditions, implied by the spec and the 3b deliverables, most likely to bite a person who runs the demo, operates the localnet or maintains CI, ordered most likely first. A named test in the owning task pins each one.

1. **The demo on a machine that is not clean.**
   - Condition: port 3040 is held by a previous sequencer, the C: drive is low, the LEZ binaries were never built, or a committed guest binary does not match its manifest.
   - Expected: `scripts/demo.sh` stops in preflight with a named reason and a distinct exit code (3 disk, 4 missing tools, 5 port or live sequencer, 6 guest mismatch), before it starts a sequencer or sends anything.
   - Pinned in Task 12: `scripts/tests/demo-preflight.sh` (cases `port_in_use`, `low_disk`, `no_lez_binaries`, `guest_mismatch`).
2. **A price move the engine would reject, or a rotation while an auction is live.**
   - Condition: an operator publishes a price outside `[R/2, 2R]`, or rotates the oracle while bids are coming in.
   - Expected: the CLI refuses before signing, names the engine error and sends nothing. A rotation keeps live auctions biddable on the new feed, and the old feed is rejected (`OracleMismatch`).
   - Pinned in Task 10: `check_new_price_refuses_a_price_outside_the_validity_range` (SDK), `dev_oracle_publish_refuses_an_invalid_price_before_signing` (CLI golden) and `rotating_the_oracle_mid_auction_keeps_bids_on_the_new_feed` (real guests).
3. **Dev commands pointed at the wrong node** (Plan 3a M6).
   - Condition: `--rpc` names a node that is not the localnet the deployment file was recorded on.
   - Expected: every state-changing `dev` command refuses with a chain-identity mismatch (exit 2) before signing. The read-only `dev resolve`, `dev balance` and `dev chain-id` work anywhere.
   - Pinned in Task 3: `dev_commands_refuse_a_node_with_another_genesis`, `read_only_dev_commands_work_on_any_network` and `verify_chain_identity_compares_block_1`.
4. **The cycle gate's own blind spots.**
   - Condition: an instruction is added or renamed without a baseline, a measured instruction is missing, growth sits at exactly 10 %, or a chain exceeds 10 executions.
   - Expected: a new or removed label fails with an explanation, exactly +10 % passes, +10.1 % fails, and 11 executions fail.
   - Pinned in Task 6: `scripts/tests/test_check_cycles.py`.
5. **The requirements checker fooled.**
   - Condition: a test name appears only in a comment or a doc, the test is `#[ignore]`d without saying so, a golden file or CI job was renamed, or a PoC row names no test.
   - Expected: each case fails the checker with the row id and the reason.
   - Pinned in Task 15: `scripts/tests/test_check_requirements.py`.

---

## Shared Reference

### R1. Facts the plan designs around

| Fact | Value | Consequence |
|---|---|---|
| Public transactions on LEZ v0.2.4 | executed by the sequencer with the risc0 executor, never proved | "real proof" = (a) sequencer run with `RISC0_DEV_MODE=0` (RFP S2) + (b) prover receipts per execution (ADR-045) |
| risc0-zkvm features in this workspace | default (`client`, `bonsai`), no `prove` | `default_prover()` is the external `r0vm` 3.0.5; proving runs in that child process, so the test binary can stay in the dev profile |
| Hosted runner (private repo) | `ubuntu-22.04`, 2 vCPU, 7 GiB RAM, about 14 GB free disk, 360 min job limit, 2000 free minutes a month (Free plan) | proof ceiling 5.5 GiB RSS; heavy jobs only on push to `main`, nightly and tags; `scripts/ci/free-disk.sh` first |
| Dev machine | 8 cores, 11 GiB RAM + 16 GiB swap, C: about 6 GB free | measurement order po2 18, 19, 20 (cheapest memory first); disk guard |
| Docker in WSL | Docker Desktop with WSL integration; it was off at plan time (`docker` not found in Ubuntu-20.04) | Task 5 Step 1 stops until `docker info` works |
| Localnet oracle | twap_oracle `OraclePriceAccount` created with a RAY price; twap `PublishPrice` writes a Q64.64 TWAP of ticks | the publisher rotates oracles (ADR-050); ADR-028: the engine must follow the host's oracle |
| Localnet finding 2 | the oracle goes stale after 1 h, and only a price move gives a shortfall | publisher + keeper (Task 10); demo price move to 1.9 R (Task 11) |
| Genesis block | built with `timestamp: 0` from the sequencer config; block 2 is the first wall-clock block | block 1 hash identifies the localnet configuration (ADR-046); the scanner anchor stays at block 2 (ADR-043) |

### R2. Workflow and job inventory (final state after Task 15)

| Workflow | Job | Trigger | Owner task |
|---|---|---|---|
| `.github/workflows/core.yml` | `core` (auction_core, differential, fmt of all our crates) | push, pull_request | existing; fmt list extended in Task 4 |
| `core.yml` | `programs` (clippy, engine + integration + SDK + CLI tests, upstream suites, cycle regression check) | push, pull_request | Tasks 4, 6 |
| `core.yml` | `idl-drift` | push, pull_request | Task 4 |
| `core.yml` | `hook-patches` | push, pull_request | Task 4 |
| `core.yml` | `coverage` | push, pull_request | Task 8 |
| `core.yml` | `requirements` | push, pull_request | Task 15 |
| `.github/workflows/real-proofs.yml` | `real-proof-push`, `real-proof-nightly` | push to `main` / schedule, tags, dispatch | Task 7 (shape set by the Task 1 ruling) |
| `.github/workflows/guests.yml` | `guest-repro` | push touching `programs/**`, `artifacts/guests/**`, `scripts/build-guests*`; schedule; dispatch | Task 5 |
| `.github/workflows/fuzz.yml` | `fuzz-nightly` | schedule, dispatch | Task 9 |
| `.github/workflows/e2e.yml` | `localnet-e2e` | push to `main` (`RISC0_DEV_MODE=0`), schedule (`0` and `1`), dispatch | Task 14 |
| composite action | `.github/actions/install-risc0` | used by `programs`, `coverage`, `real-proof-*`, `localnet-e2e` | Task 4 |

### R3. CLI exit codes (unchanged, ADR-044)

`0` success; `1` usage or other; `2` verification (I4, and from Task 3 a chain-identity mismatch); `3` transaction dropped; `4` keystore.

### R4. Demo scenario (ADR-051; localnet values, R = 1 RAY)

| Phase | Action | Expected on chain |
|---|---|---|
| positions | p0 nonce 0: 500_000 coll / 300_000 debt; p1 nonce 1: 1_000_000 / 100_000; p2 nonce 2: 500_000 / 300_000; then min CR 1.5 → 2.0 | `list-at-risk` shows p0 and p2 |
| F1–F3 | liquidate p0; bid 100_000; bid 230_000 (final); claim leftover | buffer 30_000 |
| F5 | observe, settle (nothing), check (healthy), start surplus, take 500 | buffer 29_500 |
| price move | `dev oracle-publish --price 1.9e27` | engine oracle = new account |
| shortfall | liquidate p2 (reward 95 coll at P = 1.9); bid 330_000 → `bid_with_settlement`, lot binds | `unbacked debt` between 42_154 and 63_200 (depends on the discount at inclusion) |
| F4 | settle-debt (burns 29_500); start-debt; transfer 5_000 to taker-stable; take-debt 5_000 | unbacked debt falls by exactly 5_000 and stays > 5_000 |
| freeze | `admin set-engine-params --disable-protocol-token`; check-solvency | `host status` → `frozen yes`; `host frozen by engine` set |

---

## File structure (this plan)

```
scripts/disk-guard.sh scripts/wsl-env.sh scripts/measure-proof.sh                (Task 1)
programs/integration_tests/tests/common/{measure.rs,prove.rs,mod.rs}             (Tasks 1, 6, 10)
programs/integration_tests/tests/{real_proofs.rs}                                (Tasks 1, 7)
crates/auction_sdk/src/{confirm.rs,confirm_tests.rs,scan.rs}                     (Task 2)
crates/auction_sdk/src/{deployment.rs,verify.rs}, cli/src/{dev_cmds.rs,read_cmds.rs,args.rs}  (Task 3)
.github/actions/install-risc0/action.yml .github/workflows/core.yml scripts/ci/{idl-drift.sh,free-disk.sh}  (Task 4)
scripts/{build-guests.sh,build-guests.Dockerfile,check-guests.sh} .dockerignore tools/risc0-packager/
artifacts/guests/{*.bin,manifest.json} .github/workflows/guests.yml               (Task 5)
cycles/baseline.json CYCLES.md scripts/{cycles.sh,check-cycles.py} programs/integration_tests/tests/load.rs  (Task 6)
.github/workflows/real-proofs.yml scripts/check-proof-report.py docs/evidence/     (Task 7)
scripts/check-coverage.py                                                        (Task 8)
fuzz/ .github/workflows/fuzz.yml scripts/fuzz-smoke.sh                            (Task 9)
crates/auction_sdk/src/{oracle.rs,plan_host.rs} cli/src/{oracle_cmds.rs,host_cmds.rs}
programs/integration_tests/tests/publisher.rs                                     (Task 10)
crates/auction_sdk/src/demo.rs cli/params/demo-{engine,collateral}.json
programs/integration_tests/tests/demo_scenario.rs                                 (Task 11)
scripts/lib/localnet.sh scripts/bootstrap-lez.sh scripts/localnet-smoke.sh scripts/tests/demo-preflight.sh  (Task 12)
scripts/demo.sh DEMO.md                                                          (Task 13)
.github/workflows/e2e.yml                                                        (Task 14)
REQUIREMENTS.md scripts/check-requirements.py                                    (Task 15)
docs/security/redteam-2026-10.md                                                 (Task 16)
docs/outreach/2026-10-13-*.md docs/v03-port-plan.md docs/PRIVACY.md SUBMISSION.md scripts/submission-hashes.sh  (Task 17)
scripts/tests/test_*.py                                                          (Tasks 6, 7, 8, 15)
docs/architecture/{real-proofs.md,ci.md} (new), {programs,sdk,localnet}.md, THREAT_MODEL.md, DEPLOYMENTS.md, README.md, cli/README.md, docs/design-log.md
```

---

### Task 1: Real-proof feasibility measurement and the CI tier ruling (ADR-045)

Nothing else depends on proving until this task has measured it. The task adds the proving switch to the existing chain replay. It then measures one cheap and one expensive instruction locally at three segment sizes, measures the privacy circuit on one instruction, and runs the Plan 3a localnet smoke with a `RISC0_DEV_MODE=0` sequencer. A fixed decision rule then turns the numbers into the CI tiers that Task 7 builds.

**Files:**
- Create: `scripts/disk-guard.sh`, `scripts/wsl-env.sh`, `scripts/measure-proof.sh`
- Create: `programs/integration_tests/tests/common/prove.rs`, `programs/integration_tests/tests/real_proofs.rs`
- Modify: `programs/integration_tests/tests/common/measure.rs` (make `elf_for` public; add `first_call`; switch to proving under `AUCTION_PROVE=1`; `Measured.proofs`)
- Modify: `programs/integration_tests/tests/common/mod.rs` (`pub mod prove;`)
- Modify: `scripts/localnet-smoke.sh` (`SEQ_RISC0_DEV_MODE`, `step_peak`)
- Create: `docs/architecture/real-proofs.md`; Modify: `docs/design-log.md` (ADR-045)

**Interfaces:**
- Consumes: `World::measured(label, accounts, signers, ix) -> (usize, Result<(), LeeError>)` (Plan 2 Task 17), `World::with_engine`, `liquidated_world`, `World::liquidate_accounts`, `World::bid_accounts`, `World::unsafe_position`.
- Produces:
  - `scripts/disk-guard.sh [min_c_gib=3] [min_wsl_gib=20]`: exit 0 when there is enough space, exit 3 to stop.
  - `scripts/wsl-env.sh <cmd> [args...]`: sources `$HOME/lez-spikes/s1/env.sh`, exports `CARGO_TARGET_DIR=$HOME/lez-target/plan2` and `RISC0_DEV_MODE=${RISC0_DEV_MODE:-1}`, then execs.
  - `scripts/measure-proof.sh <test_name> <po2> <timeout_seconds>`: writes `$HOME/lez-target/plan3b-proofs/<test>-po2-<po2>.log` and prints `RESULT test=... po2=... status=... wall=... peak_rss_mib=... log=...`.
  - `common::prove::{proving_enabled() -> bool, ProveSettings { po2: u32, succinct: bool }, ProveSettings::from_env(), ProvedExec { program, user_cycles, total_cycles, segments, seconds, seal_bytes, po2 }, prove_one(&ChainedCall, Option<ProgramId>, &ProveSettings) -> (&'static str, ProgramOutput, ProvedExec), report(label, &ProvedExec)}`.
  - Environment: `AUCTION_PROVE=1` (prove instead of execute), `AUCTION_PROVE_PO2` (default 20), `AUCTION_PROVE_KIND=composite|succinct` (default composite), `PROOF_REPORT=<path>` (JSON lines).
  - `common::measure::{elf_for, first_call, measure, Measured { per_exec, total, proofs: Vec<ProvedExec> }}`.
  - Ignored tests in `real_proofs.rs`: `prove_settle_debt`, `prove_liquidate`, `prove_bid`, `prove_bid_with_settlement`, `prove_ppe_observe_feed`.
  - `scripts/localnet-smoke.sh`: `SEQ_RISC0_DEV_MODE` (default 1) and `step_peak`.
  - ADR-045 ruling values: `PO2` ∈ {18, 19, 20, none}, `PUSH_TIER` ∈ {A, B, C}, `NIGHTLY_TIER` ∈ {A1, A3, C}, `PPE_NIGHTLY` ∈ {yes, no}.

- [ ] **Step 1: Preflight**

Run inside WSL (Git Bash form): `nproc; free -m; df -h /mnt/c "$HOME"; . ~/lez-spikes/s1/env.sh && r0vm --version`
Expected: 8 cores, about 11 GiB RAM, C: at least 3 GiB free, `risc0-r0vm 3.0.5`. If C: has less than 3 GiB free, STOP and report.

- [ ] **Step 2: Add the disk guard and the environment wrapper**

Create `scripts/disk-guard.sh`:

```bash
#!/usr/bin/env bash
# Stops a task before it fills a disk (Plan 3b Global Constraints). Run inside WSL.
#   scripts/disk-guard.sh [min_c_gib=3] [min_wsl_gib=20]
# Exit 0: enough space. Exit 3: stop (says which disk and what is safe to delete).
# Also used by the localnet scripts on plain Linux (no /mnt/c). The WSL ext4 disk and the Docker Desktop disk are files on C:, so C: free space is what runs out.
set -euo pipefail
min_c_gib="${1:-3}"
min_wsl_gib="${2:-20}"
avail_gib() { df -BG --output=avail "$1" | tail -1 | tr -dc '0-9'; }
# On plain Linux (CI) there is no /mnt/c: only the home disk is checked.
c="$min_c_gib"
if [[ -d /mnt/c ]]; then c="$(avail_gib /mnt/c)"; fi
w="$(avail_gib "$HOME")"
echo "disk-guard: C: ${c} GiB free (min ${min_c_gib}), home ${w} GiB free (min ${min_wsl_gib})"
if (( c < min_c_gib )); then
  echo "disk-guard: STOP: C: has ${c} GiB free (< ${min_c_gib} GiB); the WSL and Docker disks grow into it." >&2
  echo "disk-guard: safe to delete: \$HOME/lez-target/plan2/llvm-cov-target, \$HOME/lez-target/plan2/fuzz," \
    "\$HOME/lez-target/plan3b-proofs/*.log, docker build cache of this repo" \
    "(docker builder prune --filter type=exec.cachemount). Never delete \$HOME/lez-spikes/s1." >&2
  exit 3
fi
if (( w < min_wsl_gib )); then
  echo "disk-guard: STOP: the WSL home has ${w} GiB free (< ${min_wsl_gib} GiB)." >&2
  exit 3
fi
```

Create `scripts/wsl-env.sh`:

```bash
#!/usr/bin/env bash
# Runs one command inside WSL with the Plan 2 build environment: S1's env.sh (risc0 toolchain, libclang,
# sysroot), the shared target directory and dev mode unless the caller set it. scripts/wsl-cargo.sh is
# this wrapper applied to cargo. Never point CARGO_TARGET_DIR anywhere else (Plan 3b Global Constraints).
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
env_file="${LEZ_ENV_FILE:-$HOME/lez-spikes/s1/env.sh}"
if [[ ! -f "$env_file" ]]; then
  echo "wsl-env.sh: missing $env_file (see docs/spikes/S1.md, 'Missing system packages')" >&2
  exit 2
fi
# shellcheck disable=SC1090
. "$env_file"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/lez-target/plan2}"
export RISC0_DEV_MODE="${RISC0_DEV_MODE:-1}"
mkdir -p "$CARGO_TARGET_DIR"
cd "$repo_root"
exec "$@"
```

Replace the body of `scripts/wsl-cargo.sh` after its header comment with:

```bash
set -euo pipefail
exec "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)/wsl-env.sh" cargo "$@"
```

Run: `chmod +x scripts/disk-guard.sh scripts/wsl-env.sh && git update-index --chmod=+x scripts/disk-guard.sh scripts/wsl-env.sh && scripts/disk-guard.sh && scripts/wsl-cargo.sh --version`
Expected: a `disk-guard:` line with exit 0, then `cargo 1.94.0`.

- [ ] **Step 3: Write the proving module**

Create `programs/integration_tests/tests/common/prove.rs`:

```rust
//! Real proofs (Plan 3b, ADR-045). With `AUCTION_PROVE=1` and `RISC0_DEV_MODE=0`, `measure::measure`
//! proves every execution of a transaction's chain with the risc0 prover instead of executing it.
//! The prover is the external `r0vm` 3.0.5 (risc0-zkvm default features), so proving cost lives in
//! that child process and the test binary can stay in the dev profile.
//!
//! On LEZ v0.2.4 the sequencer executes public transactions without proving them; these receipts are
//! the ones a proving pipeline (v0.3 port, private path) must produce for the same executions.
//! Every receipt is verified against the program id of its call (= its ImageID) before it is counted.

use std::io::Write as _;
use std::time::Instant;

use lee_core::program::{ChainedCall, ProgramId, ProgramOutput};
use risc0_zkvm::{default_prover, ExecutorEnv, ProverOpts};

pub struct ProvedExec {
    pub program: &'static str,
    pub user_cycles: u64,
    pub total_cycles: u64,
    pub segments: usize,
    pub seconds: f64,
    pub seal_bytes: usize,
    pub po2: u32,
}

pub struct ProveSettings {
    pub po2: u32,
    pub succinct: bool,
}

impl ProveSettings {
    /// `AUCTION_PROVE_PO2` (segment limit, default 20), `AUCTION_PROVE_KIND` (`composite` default, or
    /// `succinct`).
    pub fn from_env() -> Self {
        let po2 = std::env::var("AUCTION_PROVE_PO2")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(20);
        let succinct = std::env::var("AUCTION_PROVE_KIND").as_deref() == Ok("succinct");
        Self { po2, succinct }
    }
}

pub fn proving_enabled() -> bool {
    std::env::var("AUCTION_PROVE").as_deref() == Ok("1")
}

/// Proves one call with the same inputs `measure::execute_one` gives the executor, verifies the
/// receipt against the call's program id, and returns the program output from the journal.
pub fn prove_one(
    call: &ChainedCall,
    caller: Option<ProgramId>,
    s: &ProveSettings,
) -> (&'static str, ProgramOutput, ProvedExec) {
    assert!(
        !risc0_zkvm::is_dev_mode(),
        "AUCTION_PROVE=1 needs RISC0_DEV_MODE=0: a dev-mode receipt proves nothing"
    );
    let (name, elf) = super::measure::elf_for(call.program_id);
    let mut builder = ExecutorEnv::builder();
    builder.session_limit(Some(32 * 1024 * 1024));
    builder.segment_limit_po2(s.po2);
    builder.write(&call.program_id).unwrap();
    builder.write(&caller).unwrap();
    builder.write(&call.pre_states).unwrap();
    builder.write(&call.instruction_data).unwrap();
    let opts = if s.succinct {
        ProverOpts::succinct()
    } else {
        ProverOpts::composite()
    };
    let start = Instant::now();
    let info = default_prover()
        .prove_with_opts(builder.build().unwrap(), elf, &opts)
        .unwrap_or_else(|e| panic!("proving {name} failed: {e}"));
    let seconds = start.elapsed().as_secs_f64();
    info.receipt
        .verify(call.program_id)
        .unwrap_or_else(|e| panic!("{name}: the receipt does not verify against its ImageID: {e}"));
    let out: ProgramOutput = info.receipt.journal.decode().unwrap();
    let p = ProvedExec {
        program: name,
        user_cycles: info.stats.user_cycles,
        total_cycles: info.stats.total_cycles,
        segments: info.stats.segments,
        seconds,
        seal_bytes: info.receipt.seal_size(),
        po2: s.po2,
    };
    (name, out, p)
}

/// Appends one JSON line per proved execution to `$PROOF_REPORT` (CI evidence; Task 7's checker).
pub fn report(label: &str, p: &ProvedExec) {
    let Ok(path) = std::env::var("PROOF_REPORT") else {
        return;
    };
    let line = serde_json::json!({
        "label": label,
        "program": p.program,
        "user_cycles": p.user_cycles,
        "total_cycles": p.total_cycles,
        "segments": p.segments,
        "seconds": p.seconds,
        "seal_bytes": p.seal_bytes,
        "po2": p.po2,
        "verified": true,
    });
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .unwrap_or_else(|e| panic!("PROOF_REPORT {path}: {e}"));
    writeln!(f, "{line}").unwrap();
}
```

- [ ] **Step 4: Route `measure` through the prover when asked**

In `programs/integration_tests/tests/common/measure.rs`:
- change `fn elf_for` to `pub fn elf_for`;
- rename `run_one` to `execute_one`;
- replace the module doc, `Measured` and `measure` with the following. `execute_one` keeps its body.

```rust
//! Replays a transaction's whole chain with the risc0 executor (S1 step 6 / S2 method) and prints one
//! line per program execution: executor cycles of natively built guests in dev mode, not proving
//! cost. With `AUCTION_PROVE=1` (and `RISC0_DEV_MODE=0`) every execution is proved instead
//! (`prove.rs`, ADR-045) and a `PROOF[...]` line is printed per execution.

pub struct Measured {
    pub per_exec: Vec<(&'static str, u64)>,
    pub total: u64,
    /// Empty unless `AUCTION_PROVE=1`.
    pub proofs: Vec<super::prove::ProvedExec>,
}

/// The top-level call of a transaction, as the sequencer builds it from the message.
pub fn first_call<I: Serialize>(
    state: &V03State,
    program_id: ProgramId,
    accounts: &[AccountId],
    signers: &[&PrivateKey],
    instruction: &I,
) -> ChainedCall {
    let signer_ids: Vec<AccountId> = signers
        .iter()
        .map(|k| AccountId::from(&PublicKey::new_from_private_key(k)))
        .collect();
    ChainedCall {
        program_id,
        instruction_data: Program::serialize_instruction(instruction).unwrap(),
        pre_states: accounts
            .iter()
            .map(|id| {
                AccountWithMetadata::new(state.get_account_by_id(*id), signer_ids.contains(id), *id)
            })
            .collect(),
        pda_seeds: vec![],
    }
}

pub fn measure<I: Serialize>(
    label: &str,
    state: &V03State,
    program_id: ProgramId,
    accounts: &[AccountId],
    signers: &[&PrivateKey],
    instruction: &I,
) -> Measured {
    let first = first_call(state, program_id, accounts, signers, instruction);
    let prove = super::prove::proving_enabled();
    let settings = super::prove::ProveSettings::from_env();
    let mut queue = VecDeque::from([(first, None, 0usize)]);
    let (mut per_exec, mut total, mut proofs) = (Vec::new(), 0u64, Vec::new());
    while let Some((call, caller, depth)) = queue.pop_front() {
        let n = per_exec.len() + 1;
        let (name, out, cycles) = if prove {
            let (name, out, p) = super::prove::prove_one(&call, caller, &settings);
            println!(
                "PROOF[{label}] exec#{n} depth={depth} program={name} user_cycles={} total_cycles={} segments={} seconds={:.1} seal_bytes={} po2={}",
                p.user_cycles, p.total_cycles, p.segments, p.seconds, p.seal_bytes, p.po2
            );
            super::prove::report(label, &p);
            let cycles = p.user_cycles;
            proofs.push(p);
            (name, out, cycles)
        } else {
            let (name, out, cycles) = execute_one(&call, caller);
            println!("CYCLES[{label}] exec#{n} depth={depth} program={name} cycles={cycles}");
            (name, out, cycles)
        };
        per_exec.push((name, cycles));
        total += cycles;
        for c in out.chained_calls {
            queue.push_back((c, Some(call.program_id), depth + 1));
        }
    }
    println!(
        "CYCLES[{label}] TOTAL executions={} cycles={total}",
        per_exec.len()
    );
    Measured {
        per_exec,
        total,
        proofs,
    }
}
```

In `programs/integration_tests/tests/common/mod.rs`, next to `pub mod measure;`, add `pub mod prove;`.

- [ ] **Step 5: Write the ignored real-proof tests**

Create `programs/integration_tests/tests/real_proofs.rs`:

```rust
//! Real proofs (ADR-045). Every test is `#[ignore]`d: run one at a time through
//! `scripts/measure-proof.sh` (sets `AUCTION_PROVE=1`, `RISC0_DEV_MODE=0`, a timeout, and records peak
//! RSS) or the `real-proofs.yml` jobs. The scenarios are the ADR-036 ones (`chains.rs`).

mod common;

use std::collections::HashMap;
use std::time::Instant;

use auction_engine_core::Instruction as Ix;
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID as CLOCK;
use common::*;
use lee::{
    execute_and_prove, privacy_preserving_transaction::circuit::ProgramWithDependencies,
    program::Program,
};
use lee_core::{account::AccountWithMetadata, InputAccountIdentity};

fn require_proving() {
    assert!(
        prove::proving_enabled() && !risc0_zkvm::is_dev_mode(),
        "run through scripts/measure-proof.sh (AUCTION_PROVE=1, RISC0_DEV_MODE=0)"
    );
}

/// Cheapest instruction of ADR-036: `settle_debt` on an empty buffer (1 execution, 591,029 cycles).
#[test]
#[ignore = "real proof: scripts/measure-proof.sh"]
fn prove_settle_debt() {
    require_proving();
    let mut w = World::with_engine();
    let (n, r) = w.measured(
        "settle_debt",
        vec![id(k::CALLER), config(), accounting(), buffer(), stable_def()],
        &[k::CALLER],
        Ix::SettleDebt { amount: u128::MAX },
    );
    r.expect("settle_debt");
    assert_eq!(n, 1);
}

/// Most expensive instruction of ADR-036: `liquidate` (5 executions, 3,151,610 cycles).
#[test]
#[ignore = "real proof: scripts/measure-proof.sh"]
fn prove_liquidate() {
    require_proving();
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    w.holding(k::KEEPER_COLL, collateral_def(), 0);
    let accounts = w.liquidate_accounts(pos, 0, id(k::KEEPER_COLL));
    let (n, r) = w.measured("liquidate", accounts, &[k::KEEPER], Ix::Liquidate { nonce: 0 });
    r.expect("liquidate");
    assert_eq!(n, 5);
}

/// Non-final bid (3 executions); the ADR-023 re-measure needs it (Task 7).
#[test]
#[ignore = "real proof: scripts/measure-proof.sh"]
fn prove_bid() {
    require_proving();
    let (mut w, pos) = liquidated_world();
    let accounts = w.bid_accounts(pos, 0, k::BIDDER_STABLE, k::BIDDER_COLL, false);
    let (n, r) = w.measured(
        "bid",
        accounts,
        &[k::BIDDER_STABLE],
        Ix::Bid {
            coin_in_max: 100_000,
            min_collateral_out: 0,
        },
    );
    r.expect("bid");
    assert_eq!(n, 3);
}

/// The final bid of the end-to-end path liquidate -> bid -> settle (4 executions).
#[test]
#[ignore = "real proof: scripts/measure-proof.sh"]
fn prove_bid_with_settlement() {
    require_proving();
    let (mut w, pos) = liquidated_world();
    let accounts = w.bid_accounts(pos, 0, k::BIDDER_STABLE, k::BIDDER_COLL, true);
    let (n, r) = w.measured(
        "bid_with_settlement",
        accounts,
        &[k::BIDDER_STABLE],
        Ix::BidWithSettlement {
            coin_in_max: LIQ_TARGET,
            min_collateral_out: 0,
        },
    );
    r.expect("final bid");
    assert_eq!(n, 4);
}

/// The LEZ privacy circuit (execute_and_prove) over `observe_feed` with all-public identities: the
/// proving path a private transaction takes (ADR-034). Measured for the record (M4 sizing).
#[test]
#[ignore = "real proof: scripts/measure-proof.sh"]
fn prove_ppe_observe_feed() {
    assert!(
        !risc0_zkvm::is_dev_mode(),
        "run through scripts/measure-proof.sh (RISC0_DEV_MODE=0)"
    );
    let w = World::with_engine();
    let ids = [
        id(k::CALLER),
        config(),
        coll_config(),
        redemption_state(),
        oracle(),
        CLOCK,
    ];
    let pre = ids
        .iter()
        .map(|i| AccountWithMetadata::new(w.state.get_account_by_id(*i), *i == id(k::CALLER), *i))
        .collect();
    let program = ProgramWithDependencies::new(
        Program::new(auction_engine_methods::AUCTION_ENGINE_ELF.to_vec().into()).expect("ELF"),
        HashMap::new(),
    );
    let start = Instant::now();
    let result = execute_and_prove(
        pre,
        Program::serialize_instruction(Ix::ObserveFeed).expect("instruction"),
        ids.iter().map(|_| InputAccountIdentity::Public).collect(),
        &program,
    );
    println!(
        "PROOF[ppe_observe_feed] seconds={:.1} ok={}",
        start.elapsed().as_secs_f64(),
        result.is_ok()
    );
    result.expect("the privacy circuit proves observe_feed");
}
```

Run: `scripts/wsl-cargo.sh test -p engine_integration_tests --test real_proofs` and `scripts/wsl-cargo.sh test -p engine_integration_tests --test chains`
Expected: `real_proofs`: `0 passed; 0 failed; 5 ignored`. `chains`: `3 passed` with the same `CYCLES[...]` totals as ADR-036, which shows that executor mode is unchanged.

- [ ] **Step 6: Add the measurement script and the sequencer switch**

Create `scripts/measure-proof.sh`:

```bash
#!/usr/bin/env bash
# Times one real-proof test (ADR-045) and records its peak RSS. Run INSIDE WSL from the repo root:
#   scripts/measure-proof.sh <test_name> <po2> <timeout_seconds>
# Builds the test binary first (so compilation is not measured), then runs exactly that test with
# AUCTION_PROVE=1 RISC0_DEV_MODE=0 under /usr/bin/time -v. The prover is the external r0vm (a child
# of the test process); GNU time reports the largest RSS in the process tree.
set -euo pipefail
test_name="${1:?test name}"; po2="${2:?po2}"; limit="${3:?timeout seconds}"
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
"$repo/scripts/disk-guard.sh"
out_dir="$HOME/lez-target/plan3b-proofs"
mkdir -p "$out_dir"
log="$out_dir/${test_name}-po2-${po2}.log"
# shellcheck disable=SC1090
. "${LEZ_ENV_FILE:-$HOME/lez-spikes/s1/env.sh}"
r0vm --version | grep -q "3.0.5" || { echo "measure-proof: r0vm 3.0.5 required (rzup install r0vm 3.0.5)" >&2; exit 2; }
exe="$("$repo/scripts/wsl-cargo.sh" test -p engine_integration_tests --test real_proofs --no-run --message-format=json 2>/dev/null \
  | python3 -c 'import json, sys
for line in sys.stdin:
    m = json.loads(line)
    if m.get("reason") == "compiler-artifact" and m.get("executable") and m["target"]["name"] == "real_proofs":
        print(m["executable"])' | tail -1)"
[[ -x "$exe" ]] || { echo "measure-proof: real_proofs test binary not found" >&2; exit 2; }
set +e
AUCTION_PROVE=1 RISC0_DEV_MODE=0 AUCTION_PROVE_PO2="$po2" \
  /usr/bin/time -v timeout "$limit" "$exe" --ignored --exact "$test_name" --nocapture --test-threads 1 > "$log" 2>&1
status=$?
set -e
wall="$(sed -n 's/.*Elapsed (wall clock) time (h:mm:ss or m:ss): //p' "$log")"
rss_kb="$(sed -n 's/.*Maximum resident set size (kbytes): //p' "$log")"
grep -E '^PROOF\[' "$log" || true
echo "RESULT test=$test_name po2=$po2 status=$status wall=$wall peak_rss_mib=$(( ${rss_kb:-0} / 1024 )) log=$log"
"$repo/scripts/disk-guard.sh"
exit "$status"
```

In `scripts/localnet-smoke.sh`, `step_sequencer`, replace `RISC0_DEV_MODE=1 RUST_LOG=info setsid nohup` with `RISC0_DEV_MODE="${SEQ_RISC0_DEV_MODE:-1}" RUST_LOG=info setsid nohup`. Add to the header comment: `SEQ_RISC0_DEV_MODE=0 runs the sequencer with dev mode off (RFP S2).` Add before `step_stop`:

```bash
step_peak() {
  # Peak RSS of the running sequencer (VmHWM), for the ADR-045 measurement.
  local pid
  pid="$(cat "$LN/sequencer.pid")"
  echo "sequencer peak RSS: $(sed -n 's/^VmHWM:[[:space:]]*//p' "/proc/$pid/status") (RISC0_DEV_MODE=${SEQ_RISC0_DEV_MODE:-1})"
}
```

Run: `chmod +x scripts/measure-proof.sh && git update-index --chmod=+x scripts/measure-proof.sh && bash -n scripts/measure-proof.sh scripts/localnet-smoke.sh && echo SYNTAX-OK`
Expected: `SYNTAX-OK`.

- [ ] **Step 7: Measure (one at a time, in the background)**

Run these in order, each as its own background command (timeout 5700000 ms) from Git Bash, in the form `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/measure-proof.sh <test> <po2> 5400'`:
1. `prove_settle_debt 18`
2. `prove_liquidate 18`
3. If run 2 has `status=0` and `peak_rss_mib <= 5632`: `prove_liquidate 19`.
4. If run 3 has `status=0` and `peak_rss_mib <= 5632`: `prove_liquidate 20`.
5. With `PO2` = the largest po2 among 18, 19 and 20 whose `prove_liquidate` run had `status=0` and `peak_rss_mib <= 5632`: `prove_settle_debt <PO2>` (skip if PO2 is 18; run 1 already covers it) and `prove_bid_with_settlement <PO2>`.
6. `prove_ppe_observe_feed 20` (timeout 3600).

Stop the sequence early when a run exits 124 (timeout) or 137 (OOM kill): that po2 and every larger one fail the RSS/time test. Record every `RESULT` line verbatim.

Then run the sequencer measurement (background, timeout 3600000), all inside one `bash -lc`:
`export SEQ_RISC0_DEV_MODE=0 RISC0_DEV_MODE=0; scripts/localnet-smoke.sh step_build step_sequencer step_accounts step_auth step_deploy step_host step_engine step_bind step_collateral step_holdings step_flow step_system step_check step_peak step_stop 2>&1 | tee $HOME/lez-target/plan3b-proofs/smoke-dev-mode-0.log; echo EXIT ${PIPESTATUS[0]}`, wrapped in `/usr/bin/time -f "WALL %e s" ...`.
Expected: `no failed execution check`, a `sequencer peak RSS:` line, and `EXIT 0`. A non-zero exit is not a resource result: it is a correctness failure of the S2 path. STOP and report it with the log.

- [ ] **Step 8: Apply the decision rule**

Constants: `RSS_CEILING_MIB = 5632` (5.5 GiB of the hosted runner's 7 GiB); `SLOWDOWN = nproc_local / 2` (8 / 2 = 4: proving parallelises across cores, and the hosted runner has 2 vCPUs); `PUSH_BUDGET_S = 2700` (45 min); `NIGHTLY_BUDGET_S = 19800` (330 min of the 360 min job limit). Write `T(test, po2)` for the measured wall seconds of a `status=0` run, and `SUITE_FACTOR = 1.5 × 21,260,696 / 3,151,610 = 10.12`. The cycle ratio is the ADR-036 sum over all 17 rows divided by `liquidate`; the 1.5 covers per-execution padding.

- **R1 (S2):** the smoke run at `RISC0_DEV_MODE=0` is green → the per-push-to-main `localnet-e2e` job runs with `RISC0_DEV_MODE=0` (Task 14). This rule is not optional.
- **R2 (segment size):** `PO2` = the largest po2 ∈ {20, 19, 18} whose `prove_liquidate` run had `status=0` and `peak_rss_mib ≤ 5632`; `none` if there is no such po2.
- **R3 (per push):**
  - Tier A: `PO2 ≠ none` and `SLOWDOWN × (T(liquidate) + T(bid_with_settlement)) ≤ 2700` → `real-proof-push` proves `prove_liquidate` and `prove_bid_with_settlement` on the hosted runner.
  - Tier B: otherwise, `PO2 ≠ none` and `SLOWDOWN × T(settle_debt) ≤ 2700` → `real-proof-push` proves `prove_settle_debt` only.
  - Tier C: otherwise, there is no hosted proving per push; the per-push real-proof requirement rests on R1 alone, and ADR-045 says so.
- **R4 (nightly and tags):**
  - Tier A1: `PO2 ≠ none` and `SLOWDOWN × T(liquidate) × SUITE_FACTOR ≤ 19800` → one hosted job proves the whole `chains.rs` suite.
  - Tier A3: otherwise, `PO2 ≠ none` and the same estimate `≤ 3 × 19800` → a hosted matrix of the three `chains.rs` tests.
  - Tier C: otherwise, a self-hosted job, documented and disabled by default (`vars.REAL_PROOF_SELF_HOSTED == 'true'`), plus one recorded local full run committed as evidence (Task 7).
- **R5 (privacy circuit):** `PPE_NIGHTLY = yes` iff `prove_ppe_observe_feed` had `status=0`, `peak_rss_mib ≤ 5632` and `SLOWDOWN × T ≤ 3600`.

- [ ] **Step 9: Record the measurement and STOP for the ruling**

Create `docs/architecture/real-proofs.md`:

```markdown
# Real proofs on LEZ v0.2.4 (ADR-045)

## What "real proof" means here

LEZ v0.2.4's sequencer executes public transactions with the risc0 executor and never proves them
(`lee/state_machine/src/program/mod.rs`); only the privacy circuit (`execute_and_prove`) and the wallet's
private path call the prover. The PoC submits public transactions only (spec §8). So:

1. **RFP S2 / spec §9 end to end:** the localnet flow driven by `auction-cli` against a standalone
   sequencer started with `RISC0_DEV_MODE=0` (`scripts/demo.sh --sequencer-dev-mode 0`, CI job
   `localnet-e2e`). Nothing in that path accepts a dev-mode receipt.
2. **Receipts:** every execution of an instruction's chained calls proved with the risc0 prover
   (`AUCTION_PROVE=1 RISC0_DEV_MODE=0`, external `r0vm` 3.0.5) and verified against its ImageID
   (`programs/integration_tests/tests/common/prove.rs`). These are the receipts the v0.3 port or the
   private path must produce. The guests proved are the native builds of the same source; the deployed
   Docker builds have other ImageIDs (ADR-047).

## Measurement (Plan 3b Task 1, <date of the run>)

Machine: WSL Ubuntu-20.04, `nproc` = <n>, 11 GiB RAM + 16 GiB swap, r0vm 3.0.5, composite receipts.

| Test | po2 | Status | Wall | Peak RSS (MiB) | Executions / segments (from PROOF lines) |
|---|---|---|---|---|---|
<one row per RESULT line of Step 7, copied verbatim>

Sequencer with `RISC0_DEV_MODE=0` (Plan 3a smoke, 34 transactions): <WALL> s, peak RSS <VmHWM>, `no failed execution check`.

## Tiers

<the ADR-045 Decision text, repeated>

## Running a proof locally

`scripts/measure-proof.sh <prove_settle_debt|prove_liquidate|prove_bid|prove_bid_with_settlement|prove_ppe_observe_feed> <po2> <timeout_s>`
(inside WSL; one at a time; disk guard before and after; logs in `$HOME/lez-target/plan3b-proofs/`).
```

Fill the `<...>` cells from the `RESULT` lines, the `PROOF[...]` lines and the smoke log (all measured values; none are invented). Then append ADR-045 to `docs/design-log.md`. Keep exactly one sentence of each `R3`/`R4` alternative, the one the rule selected, and delete the others:

```markdown
## ADR-045: Real Proofs on LEZ v0.2.4 and Their CI Tiers

**Context**
Spec §1.3 and §9 ask for one real-proof (`RISC0_DEV_MODE=0`) end-to-end job per push to `main` and the full real-proof suite nightly and on tags; RFP S2 asks for end-to-end tests against a standalone sequencer with `RISC0_DEV_MODE=0`. LEZ v0.2.4 never proves public transactions (the sequencer uses the executor), and hosted runners for this private repository have 2 vCPUs, 7 GiB RAM and a 360-minute job limit.

**Options**
- (A) Treat a `RISC0_DEV_MODE=0` sequencer run as the whole requirement.
- (B) Prove every execution of each instruction's chain with the risc0 prover, in CI where it fits, plus (A).
- (C) Prove only locally and commit the logs.

**Decision**
(B), tiered by the Plan 3b Task 1 measurement (`docs/architecture/real-proofs.md`):
- RFP S2: the `localnet-e2e` job runs `scripts/demo.sh` against a sequencer with `RISC0_DEV_MODE=0` on every push to `main` (R1).
- Segment size `AUCTION_PROVE_PO2 = <PO2>` (R2).
- Per push to `main`: <Tier A: `real-proof-push` proves `liquidate` and `bid_with_settlement` on a hosted runner | Tier B: `real-proof-push` proves `settle_debt` on a hosted runner; the liquidate-bid path is proved nightly | Tier C: no hosted proving per push; the per-push real-proof job is R1's `RISC0_DEV_MODE=0` sequencer run> (R3).
- Nightly and on tags: <Tier A1: one hosted job proves the whole `chains.rs` suite | Tier A3: a hosted matrix proves the three `chains.rs` tests | Tier C: a self-hosted job (disabled unless `vars.REAL_PROOF_SELF_HOSTED == 'true'`) plus a committed local full run in `docs/evidence/`> (R4).
- Privacy circuit (`observe_feed`): <nightly | measured once, not scheduled> (R5).

**Trade-offs**
The proved guests are native builds of the same source (the deployed Docker builds have different ImageIDs, ADR-047): a receipt shows the source is provable within the limits, not that a particular deployed binary was proved. Hosted timing scales by an assumed factor of 4 from an 8-core machine; Task 7 records the first CI timings and moves a tier if they exceed the budgets.

**Status**
Accepted (Plan 3b, Task 1); finalised in Task 7.

---
```

Then report every `RESULT` line, the smoke result and the selected `PO2`, `PUSH_TIER`, `NIGHTLY_TIER` and `PPE_NIGHTLY` to the controller. **STOP: the controller records `Ruling: ADR-045 tiers PO2=<..> PUSH=<..> NIGHTLY=<..> PPE=<..>` in the ledger before Task 7 starts.** Tasks 2 to 6 do not depend on the ruling and may proceed.

- [ ] **Step 10: Gates**

Run the lint gate and the format gate. Expected: clean.

- [ ] **Step 11: Commit (controller, GPG signing on Windows)**

```bash
git add scripts/disk-guard.sh scripts/wsl-env.sh scripts/wsl-cargo.sh scripts/measure-proof.sh scripts/localnet-smoke.sh \
  programs/integration_tests/tests/common/prove.rs programs/integration_tests/tests/common/measure.rs \
  programs/integration_tests/tests/common/mod.rs programs/integration_tests/tests/real_proofs.rs \
  docs/architecture/real-proofs.md docs/design-log.md
git commit -m "feat(proofs): measure real proofs and record the CI tier ruling (ADR-045)"
```

---
### Task 2: SDK lock and rescan hardening (Plan 3a N1, N2, N4, panic after send)

This task closes four findings parked at the Plan 3a merge. None of them can lose funds or keys, but each leaves a documented contract untested or slightly wrong.

**Files:**
- Modify: `crates/auction_sdk/src/confirm.rs` (`lock_with_current_baseline`: rebase margin)
- Modify: `crates/auction_sdk/src/confirm_tests.rs` (three tests)
- Modify: `crates/auction_sdk/src/scan.rs` (`start_at` treats an anchorless cursor like `on_this_chain`; one test)
- Modify: `cli/tests/golden.rs` (one test)
- Modify: `docs/design-log.md` (amend ADR-042 and ADR-043), `docs/architecture/sdk.md`

**Interfaces:**
- Consumes: `submit`, `InFlightLock`, `ConfirmSettings`, `LOCK_ATTEMPTS` (confirm.rs); `start_at`, `on_this_chain`, `Restart::{NoAnchor, ChainReplaced}`, `Scanned` (scan.rs); `standard::standard()`, `TestKeys`, `SendMode` (testing).
- Produces: `const REBASE_MARGIN_BLOCKS: u64 = 1` (confirm.rs, private). There are no signature changes; behaviour changes as follows:
  - a baseline re-read at `h0 + drop - 1` or later retakes the lock instead of rebasing;
  - `start_at` on an anchorless cursor with `last_block >= ANCHOR_BLOCK` returns `restarted = Some(Restart::NoAnchor)` and an empty base.

- [ ] **Step 1: Write the failing tests**

Append to `crates/auction_sdk/src/confirm_tests.rs`:

```rust
/// Height 10 at the first read and 14 afterwards, drop window 5: the re-read under the lock is one
/// block short of the window end (15). The record still says 10 until `rebase` renames it, so a
/// block landing in between would let another process judge it stale and take it over (Plan 3a N1).
/// The lock is re-taken from a fresh baseline instead of being rebased so close to the edge.
struct NearWindowEnd<'a> {
    fx: &'a crate::testing::FixtureChain,
    reads: std::cell::Cell<u32>,
    reads_at_send: std::cell::Cell<u32>,
}

impl Chain for NearWindowEnd<'_> {
    fn account(&self, i: &AccountId) -> SdkResult<lee_core::account::Account> {
        self.fx.account(i)
    }
    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<lee_core::account::Nonce>> {
        self.fx.nonces(ids)
    }
    fn last_block_id(&self) -> SdkResult<u64> {
        self.reads.set(self.reads.get() + 1);
        Ok(if self.reads.get() == 1 { 10 } else { 14 })
    }
    fn blocks(&self, a: u64, b: u64) -> SdkResult<Vec<crate::chain::BlockSummary>> {
        self.fx.blocks(a, b)
    }
    fn send(&self, _: &lee::PublicTransaction) -> SdkResult<[u8; 32]> {
        self.reads_at_send.set(self.reads.get());
        Err(SdkError::SendUncertain("connection reset".into()))
    }
}

#[test]
fn a_re_read_one_block_short_of_the_window_end_retakes_the_lock() {
    let fx = standard::standard();
    let dir = tempfile::tempdir().expect("tmp");
    let chain = NearWindowEnd {
        fx: &fx.chain,
        reads: std::cell::Cell::new(0),
        reads_at_send: std::cell::Cell::new(0),
    };
    let e = submit(
        &chain,
        &TestKeys::of(&[k::CALLER]),
        &observe_plan(&fx),
        dir.path(),
        &fast(),
        &|_| {},
    )
    .expect_err("unknown result");
    assert!(matches!(e, SdkError::SendUncertain(_)));
    assert_eq!(
        chain.reads_at_send.get(),
        4,
        "baseline 10, re-read 14 (too close to 15): retry with baseline 14, re-read 14"
    );
    let r = read_record(&dir.path().join(format!("{}.lock", id(k::CALLER))));
    assert_eq!(r.height, 14);
}

/// The signer's nonce moves on every read: every attempt sees a stale baseline under the lock.
struct NonceAlwaysMoves<'a>(
    &'a crate::testing::FixtureChain,
    std::cell::Cell<u128>,
);

impl Chain for NonceAlwaysMoves<'_> {
    fn account(&self, i: &AccountId) -> SdkResult<lee_core::account::Account> {
        self.0.account(i)
    }
    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<lee_core::account::Nonce>> {
        let n = self.1.get();
        self.1.set(n + 1);
        Ok(ids.iter().map(|_| lee_core::account::Nonce(n)).collect())
    }
    fn last_block_id(&self) -> SdkResult<u64> {
        self.0.last_block_id()
    }
    fn blocks(&self, a: u64, b: u64) -> SdkResult<Vec<crate::chain::BlockSummary>> {
        self.0.blocks(a, b)
    }
    fn send(&self, _: &lee::PublicTransaction) -> SdkResult<[u8; 32]> {
        panic!("nothing may be sent without a current baseline")
    }
}

#[test]
fn a_nonce_that_moves_on_every_read_exhausts_the_retries_and_leaves_no_lock() {
    let fx = standard::standard();
    let dir = tempfile::tempdir().expect("tmp");
    let chain = NonceAlwaysMoves(&fx.chain, std::cell::Cell::new(0));
    let e = submit(
        &chain,
        &TestKeys::of(&[k::CALLER]),
        &observe_plan(&fx),
        dir.path(),
        &fast(),
        &|_| {},
    )
    .expect_err("in flight");
    assert!(matches!(e, SdkError::InFlight { .. }), "{e}");
    assert_eq!(chain.1.get(), 6, "LOCK_ATTEMPTS = 3, two nonce reads each");
    assert!(is_empty_dir(dir.path()), "every attempt released its lock");
}

/// The transaction was sent, then the node client panics while polling: the transaction may still
/// land, so unwinding must keep the lock (keep_on_drop is set before `send`).
struct PanicsAfterSend<'a>(&'a crate::testing::FixtureChain, std::cell::Cell<bool>);

impl Chain for PanicsAfterSend<'_> {
    fn account(&self, i: &AccountId) -> SdkResult<lee_core::account::Account> {
        self.0.account(i)
    }
    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<lee_core::account::Nonce>> {
        assert!(!self.1.get(), "node client crashed while polling");
        self.0.nonces(ids)
    }
    fn last_block_id(&self) -> SdkResult<u64> {
        self.0.last_block_id()
    }
    fn blocks(&self, a: u64, b: u64) -> SdkResult<Vec<crate::chain::BlockSummary>> {
        self.0.blocks(a, b)
    }
    fn send(&self, tx: &lee::PublicTransaction) -> SdkResult<[u8; 32]> {
        self.1.set(true);
        self.0.send(tx)
    }
}

#[test]
fn a_panic_after_the_send_keeps_the_lock() {
    let fx = standard::standard();
    let dir = tempfile::tempdir().expect("tmp");
    let chain = PanicsAfterSend(&fx.chain, std::cell::Cell::new(false));
    let plan = observe_plan(&fx);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        submit(
            &chain,
            &TestKeys::of(&[k::CALLER]),
            &plan,
            dir.path(),
            &fast(),
            &|_| {},
        )
    }));
    assert!(caught.is_err(), "the poll panicked");
    let lock = dir.path().join(format!("{}.lock", id(k::CALLER)));
    assert!(lock.exists(), "unwinding after the send keeps the lock");
    assert_eq!(fx.chain.sent().len(), 1);
}
```

Append inside `mod tests` of `crates/auction_sdk/src/scan.rs`, after `start_at_discards_a_cursor_of_another_chain`:

```rust
    #[test]
    fn start_at_resets_an_anchorless_cursor_past_the_anchor_block() {
        // Plan 3a N2: an old-format cursor may belong to a replaced chain; `rescan --from N` must
        // treat it like `scan` does (NoAnchor reset), not keep its ids and hide the skipped blocks.
        let (fx, t) = setup();
        let c = full_scan(&fx, &t);
        let legacy = Cursor {
            anchor: String::new(),
            ..c.clone()
        };
        let placed = start_at(&fx.chain, &legacy, 3).expect("positioned");
        assert_eq!(placed.restarted, Some(Restart::NoAnchor));
        assert!(placed.cursor.auctions.is_empty(), "ids of an unknown chain dropped");
        let early = Cursor {
            last_block: 1,
            ..Cursor::default()
        };
        assert_eq!(
            start_at(&fx.chain, &early, 2).expect("positioned").restarted,
            None,
            "an anchorless cursor that saw only block 1 cannot have missed anything"
        );
    }
```

Append to `cli/tests/golden.rs`:

```rust
#[test]
fn rescan_from_on_a_cursor_without_anchor_notes_the_reset_and_the_gap() {
    let c = Case::new();
    c.write_cursor(&format!(
        "{{\"last_block\":3,\"last_hash\":\"{}\",\"auctions\":[],\"positions\":[]}}",
        "00".repeat(32)
    ));
    let (out, err) = c.run_split(&["rescan", "--from", "3"]);
    assert!(out.ends_with("exit: 0\n"), "{out}");
    assert!(err.contains("note: blocks 1..2 skipped"), "{err}");
}
```

- [ ] **Step 2: Run them to verify they fail as intended**

Run: `scripts/wsl-cargo.sh test -p auction_sdk --features testing a_re_read_one_block_short start_at_resets_an_anchorless a_nonce_that_moves a_panic_after_the_send`
Expected:
- `a_re_read_one_block_short_of_the_window_end_retakes_the_lock` FAILS (`left: 2, right: 4`).
- `start_at_resets_an_anchorless_cursor_past_the_anchor_block` FAILS (`left: None`).
- The other two PASS: they pin behaviour that has no test yet (N4 and the panic case).

Run: `scripts/wsl-cargo.sh test -p auction_cli --features auction_sdk/testing --test golden rescan_from_on_a_cursor_without_anchor`
Expected: FAIL (`err` has no `note: blocks 1..2 skipped`).

- [ ] **Step 3: Fix the rebase margin and `start_at`**

In `crates/auction_sdk/src/confirm.rs`, add after `LOCK_ATTEMPTS`:

```rust
/// Blocks that must remain in the drop window when the baseline is re-read under the lock. Until
/// `rebase` renames its record into place, the lock still says `h0`, and a reader judges it stale at
/// `h0 + drop_after_blocks`. With one block of margin a takeover would need two blocks between the
/// re-read and the rename (microseconds apart; blocks are about 15 s), so it cannot happen.
const REBASE_MARGIN_BLOCKS: u64 = 1;
```

and in `lock_with_current_baseline` replace the condition with:

```rust
        if current == nonces
            && h1.saturating_add(REBASE_MARGIN_BLOCKS) < h0.saturating_add(s.drop_after_blocks)
        {
```

Extend that function's doc comment with: `A re-read within REBASE_MARGIN_BLOCKS of the window end also retries (Plan 3a N1).`

In `crates/auction_sdk/src/scan.rs`, `start_at`, replace

```rust
    let (base, restarted) = if !cursor.anchor.is_empty() && cursor.anchor != anchor {
        (Cursor::default(), Some(Restart::ChainReplaced))
    } else {
        (cursor.clone(), None)
    };
```

with

```rust
    let (base, restarted) = on_this_chain(cursor, &anchor);
```

and change its doc's last sentence to: `A cursor built on another chain, or an anchorless one past the anchor block, is discarded first (its discoveries may belong to another chain), reported in restarted.` The CLI needs no change: `rescan` already counts nothing as covered when `restarted` is set, so the gap note now appears.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `scripts/wsl-cargo.sh test -p auction_sdk --features testing` and `scripts/wsl-cargo.sh test -p auction_cli --features auction_sdk/testing`
Expected: all pass (SDK count +4, CLI golden count +1); no golden file changes.

- [ ] **Step 5: Update the docs**

Append to ADR-042 in `docs/design-log.md`:

```markdown
**Amended (Plan 3b, Task 2)**
- Rebase margin (Plan 3a N1): the baseline re-read under the lock must leave at least one block of the drop window (`h1 + 1 < h0 + drop`), otherwise the lock is re-taken from a fresh baseline; a takeover between the re-read and the rename would need two blocks within microseconds. Test `a_re_read_one_block_short_of_the_window_end_retakes_the_lock`.
- Pinned behaviour: three stale attempts end in `InFlight` with no lock file left (`a_nonce_that_moves_on_every_read_exhausts_the_retries_and_leaves_no_lock`, N4); a panic after the send keeps the lock (`a_panic_after_the_send_keeps_the_lock`).
```

Append to ADR-043:

```markdown
**Amended (Plan 3b, Task 2)**
`rescan --from N` applies the same rule as `scan` to an anchorless cursor past block 2: it is discarded (`NoAnchor`), so its ids are dropped and the CLI notes the skipped blocks 1..N-1 (Plan 3a N2; `start_at_resets_an_anchorless_cursor_past_the_anchor_block`, golden test `rescan_from_on_a_cursor_without_anchor_notes_the_reset_and_the_gap`).
```

In `docs/architecture/sdk.md`, confirmation section, add one sentence on the rebase margin; scanner section, add one sentence that `start_at` resets anchorless cursors like `scan`.

- [ ] **Step 6: Gates**

Run the lint gate and the format gate. Expected: clean.

- [ ] **Step 7: Commit (controller, GPG signing on Windows)**

```bash
git add crates/auction_sdk/src/confirm.rs crates/auction_sdk/src/confirm_tests.rs crates/auction_sdk/src/scan.rs \
  cli/tests/golden.rs docs/design-log.md docs/architecture/sdk.md
git commit -m "fix(sdk): keep a rebase margin, reset anchorless cursors on rescan, pin lock edge cases"
```

---

### Task 3: CLI hardening: the most severe exit for bounded `watch`, chain identity for dev commands (ADR-046)

This task closes three items parked from Plan 3a:
- **N3:** a bounded `watch` exits with the last error, not the most severe one.
- **M6:** the dev-command localnet guard trusts a label, so `--rpc` can point it at any node.
- The read-only `dev resolve` and `dev balance` are blocked off localnet for no reason.

**Files:**
- Modify: `crates/auction_sdk/src/deployment.rs` (`chain_genesis_hash` / `Deployment.chain_genesis`)
- Modify: `crates/auction_sdk/src/verify.rs` (`verify_chain_identity` + tests), `crates/auction_sdk/src/testing/standard.rs`, `programs/integration_tests/tests/common/sdk.rs` (new field)
- Modify: `cli/src/args.rs` (`DevCommand::ChainId`), `cli/src/dev_cmds.rs` (guard split, `chain-id`), `cli/src/read_cmds.rs` (`keep_worst`)
- Modify: `cli/tests/admin_golden.rs` (`localnet_case`, new tests), `cli/tests/golden.rs` (struct literals); Create: `cli/tests/golden/dev_chain_id.txt`
- Modify: `deployments/localnet.json` (`"chain_genesis_hash": null`), `scripts/localnet-smoke.sh` (record the genesis in `step_sequencer`, add the field in `step_build`)
- Modify: `docs/design-log.md` (ADR-046; amend ADR-044), `cli/README.md`, `docs/architecture/sdk.md`

**Interfaces:**
- Consumes: `Chain::blocks`, `SdkError::Mismatch`, `Env::refuse_at_ms`, `CliError::exit_code`.
- Produces:
  - Deployment file key `chain_genesis_hash` (64 lowercase hex characters or null) and field `Deployment.chain_genesis: Option<String>`.
  - `auction_sdk::verify::verify_chain_identity<C: Chain + ?Sized>(chain: &C, d: &Deployment) -> SdkResult<()>`: `Mismatch` (CLI exit 2) when the file has no genesis or block 1 differs.
  - CLI `auction-cli dev chain-id`, printing `genesis <hex of block 1>`.
  - Guard rule: the state-changing dev commands (`token-new`, `token-init-account`, `token-transfer`, `oracle-create`, and from Task 10 `oracle-publish`, `keeper`) require `network == "localnet"` and `verify_chain_identity`. `resolve`, `balance` and `chain-id` run on any network.
  - `cli::read_cmds::keep_worst(worst: &mut Option<CliError>, e: CliError)`.
  - Smoke and demo scripts record `chain_genesis_hash` right after the sequencer starts (helper `record_genesis`; it moves to `scripts/lib/localnet.sh` in Task 12).

- [ ] **Step 1: Write the failing tests**

In `crates/auction_sdk/src/verify.rs` `mod tests`, add:

```rust
    #[test]
    fn verify_chain_identity_compares_block_1() {
        let fx = standard::standard();
        let genesis = hex::encode(fx.chain.blocks(1, 1).expect("block 1")[0].hash);
        let none = Deployment {
            chain_genesis: None,
            ..standard::deployment()
        };
        let e = verify_chain_identity(&fx.chain, &none).expect_err("no genesis recorded");
        assert!(matches!(e, SdkError::Mismatch { .. }), "{e}");
        let other = Deployment {
            chain_genesis: Some("ab".repeat(32)),
            ..standard::deployment()
        };
        let e = verify_chain_identity(&fx.chain, &other).expect_err("another chain");
        assert!(e.to_string().contains(&genesis), "{e}");
        let same = Deployment {
            chain_genesis: Some(genesis),
            ..standard::deployment()
        };
        verify_chain_identity(&fx.chain, &same).expect("the recorded chain");
    }
```

and in the same module a parse test:

```rust
    #[test]
    fn chain_genesis_hash_must_be_32_bytes_of_hex() {
        let base = include_str!("../../../deployments/localnet.json");
        let bad = base.replace(
            "\"chain_genesis_hash\": null",
            "\"chain_genesis_hash\": \"abc\"",
        );
        let e = Deployment::from_json(&bad, "d.json").expect_err("short hash");
        assert!(e.to_string().contains("chain_genesis_hash"), "{e}");
    }
```

In `cli/tests/admin_golden.rs`:
- replace `localnet_case` with a version that records the fixture's genesis;
- replace `dev_commands_refuse_any_network_but_the_localnet` with the three tests below.

Add `use auction_sdk::chain::Chain;` to the imports.

```rust
/// The dev commands act on the localnet only, on the chain the deployment file recorded (ADR-046).
fn localnet_case() -> Case {
    let c = Case::new();
    let genesis = hex::encode(c.fx.chain.blocks(1, 1).expect("block 1")[0].hash);
    Case {
        deployment: Some(Deployment {
            network: "localnet".into(),
            chain_genesis: Some(genesis),
            ..standard::deployment()
        }),
        ..c
    }
}

fn transfer_args() -> Vec<&'static str> {
    vec![
        "dev",
        "token-transfer",
        "--from",
        "owner-stable",
        "--to",
        "taker-stable",
        "--amount",
        "1",
    ]
}

#[test]
fn dev_commands_refuse_any_network_but_the_localnet() {
    let c = Case::new();
    let out = c.run(&transfer_args());
    assert!(out.contains("dev commands that send are for the localnet only"), "{out}");
    assert!(out.ends_with("exit: 1\n"), "{out}");
    assert!(c.fx.chain.sent().is_empty());
}

#[test]
fn dev_commands_refuse_a_node_with_another_genesis() {
    let mut c = localnet_case();
    c.deployment = Some(Deployment {
        chain_genesis: Some("ab".repeat(32)),
        ..c.deployment.clone().expect("deployment")
    });
    let out = c.run(&transfer_args());
    assert!(out.contains("chain identity"), "{out}");
    assert!(out.ends_with("exit: 2\n"), "{out}");
    assert!(c.fx.chain.sent().is_empty(), "nothing signed or sent");
}

#[test]
fn read_only_dev_commands_work_on_any_network() {
    let c = Case::new();
    for args in [
        vec!["dev", "resolve", "--label", "keeper"],
        vec!["dev", "chain-id"],
    ] {
        let out = c.run(&args);
        assert!(out.ends_with("exit: 0\n"), "{args:?}: {out}");
    }
    let out = localnet_case().run(&transfer_args());
    assert!(out.ends_with("exit: 0\n"), "the recorded chain: {out}");
}
```

In `host_and_dev_commands`, add `("dev_chain_id", vec!["dev", "chain-id"])` to `cases`.

In `cli/src/read_cmds.rs`, add a test module (or extend the existing one):

```rust
#[cfg(test)]
mod watch_tests {
    use super::*;

    #[test]
    fn keep_worst_prefers_dropped_and_keeps_the_first_of_equals() {
        let mut worst = None;
        keep_worst(&mut worst, CliError::Dropped { height: 7 });
        keep_worst(&mut worst, CliError::Unknown("later".into()));
        assert!(matches!(worst, Some(CliError::Dropped { height: 7 })));
        let mut worst = Some(CliError::Unknown("first".into()));
        keep_worst(&mut worst, CliError::Usage("second".into()));
        assert!(matches!(&worst, Some(CliError::Unknown(m)) if m == "first"));
        keep_worst(&mut worst, CliError::Dropped { height: 9 });
        assert!(matches!(worst, Some(CliError::Dropped { height: 9 })));
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/wsl-cargo.sh test -p auction_sdk --features testing verify_chain_identity chain_genesis_hash` and `scripts/wsl-cargo.sh test -p auction_cli --features auction_sdk/testing`
Expected: compile errors: `no field chain_genesis`, `cannot find function verify_chain_identity`, `no variant ChainId`, `cannot find function keep_worst`.

- [ ] **Step 3: Implement**

`crates/auction_sdk/src/deployment.rs`:
- add to `DeploymentFile`: `#[serde(default)] pub chain_genesis_hash: Option<String>,`
- add to `Deployment`: `/// Hex hash of block 1 of the chain this file was recorded on (ADR-046); state-changing dev commands require it. pub chain_genesis: Option<String>,`
- in `from_json`, validate it:

```rust
            chain_genesis: f
                .chain_genesis_hash
                .as_deref()
                .map(|h| match hex::decode(h) {
                    Ok(b) if b.len() == 32 && h == h.to_lowercase() => Ok(h.to_owned()),
                    _ => Err(bad(
                        "chain_genesis_hash must be 64 lowercase hex characters".into(),
                    )),
                })
                .transpose()?,
```

`crates/auction_sdk/src/verify.rs`, add:

```rust
/// ADR-046: the node's block 1 must be the one the deployment file recorded. The LEZ v0.2.4 genesis
/// block is built from the sequencer config with timestamp 0, so it identifies the chain's
/// configuration: two localnets started from the same config share it (both throwaway), a testnet
/// or another configuration does not. Used by every state-changing dev command.
pub fn verify_chain_identity<C: Chain + ?Sized>(chain: &C, d: &Deployment) -> SdkResult<()> {
    let Some(expected) = d.chain_genesis.as_deref() else {
        return Err(SdkError::Mismatch {
            what: "chain identity (the deployment file records no chain_genesis_hash; run `auction-cli dev chain-id` against the intended node and record it)",
            expected: "a chain_genesis_hash".into(),
            found: "none".into(),
        });
    };
    let found = chain
        .blocks(1, 1)?
        .first()
        .map(|b| hex::encode(b.hash))
        .ok_or_else(|| SdkError::Chain("the node returned no block 1".into()))?;
    if found != expected {
        return Err(SdkError::Mismatch {
            what: "chain identity (block 1 hash: this node is not the chain the deployment file was recorded on)",
            expected: expected.to_owned(),
            found,
        });
    }
    Ok(())
}
```

Add `chain_genesis: None,` to the `Deployment` literals in `crates/auction_sdk/src/testing/standard.rs` (`deployment()`), `programs/integration_tests/tests/common/sdk.rs` (`sdk_deployment()`) and every other full literal the compiler reports (`cli/tests/golden.rs` uses `..` struct update and needs nothing). Add `"chain_genesis_hash": null` to `deployments/localnet.json` after `"config"`.

`cli/src/args.rs`, in `DevCommand`, add:

```rust
    /// Print the hash of block 1 of the node (`chain_genesis_hash` for the deployment file, ADR-046).
    ChainId,
```

and change the `Resolve`/`Balance` doc comments to say they are read-only and work on any network.

`cli/src/dev_cmds.rs`: replace the head of `dev` (up to `match cmd {`) with:

```rust
/// Read-only dev commands run anywhere; the others sign, so they need the localnet named by the
/// deployment file AND the chain it recorded (ADR-046; Plan 3a M6: a label alone let `--rpc` aim them
/// at any node).
fn is_read_only(cmd: &DevCommand) -> bool {
    matches!(
        cmd,
        DevCommand::Resolve { .. } | DevCommand::Balance { .. } | DevCommand::ChainId
    )
}

pub fn dev<C: Chain>(env: &Env<C>, cmd: &DevCommand, out: &mut dyn Write) -> Result<(), CliError> {
    if !is_read_only(cmd) {
        env.refuse_at_ms()?;
        if env.deployment.network != LOCALNET {
            return Err(CliError::Usage(format!(
                "dev commands that send are for the localnet only (the deployment file names network {:?})",
                env.deployment.network
            )));
        }
        verify_chain_identity(&env.chain, &env.deployment)?;
    }
    match cmd {
        DevCommand::ChainId => {
            let b = env.chain.blocks(1, 1)?;
            let first = b
                .first()
                .ok_or_else(|| CliError::Usage("the node returned no block 1".into()))?;
            kv(out, "genesis", &hex::encode(first.hash))
        }
```

(The remaining match arms are unchanged; add `use auction_sdk::verify::verify_chain_identity;`.)

`cli/src/read_cmds.rs`: add

```rust
/// The error a bounded `watch` exits with: the most severe so far (exit 3, dropped, over exit 1),
/// the first of equals (Plan 3a N3). Setup errors (2, 4) never reach here: they end `watch` at once.
pub fn keep_worst(worst: &mut Option<CliError>, e: CliError) {
    if worst
        .as_ref()
        .map_or(true, |prev| e.exit_code() > prev.exit_code())
    {
        *worst = Some(e);
    }
}
```

In `watch_step`, replace `*last_error = Some(e);` with `keep_worst(last_error, e);`. In `watch`, rename `last_error` to `worst_error`, and change the doc sentence to: `A run bounded by --iterations exits with the most severe step error if any step failed (3 before 1, the first of equals).`

`scripts/localnet-smoke.sh`:
- in the `step_build` python, add `"chain_genesis_hash": None` to the dict;
- at the end of `step_sequencer`, after writing the pid file, add:

```bash
  # ADR-046: the state-changing dev commands require the chain this deployment file was recorded on.
  local genesis="" i
  for i in $(seq 1 15); do
    genesis="$("$CLI_BIN" --deployment "$DEP" dev chain-id 2>/dev/null | awk '$1 == "genesis" { print $2 }')" && [[ -n "$genesis" ]] && break
    sleep 2
  done
  [[ -n "$genesis" ]] || { echo "could not read block 1 from the new sequencer" >&2; exit 1; }
  set_deployment_field chain_genesis_hash "$genesis"
```

- [ ] **Step 4: Run the tests and refresh the one new golden file**

Run: `scripts/wsl-cargo.sh test -p auction_sdk --features testing` then `UPDATE_GOLDEN=1 scripts/wsl-cargo.sh test -p auction_cli --features auction_sdk/testing --test admin_golden host_and_dev_commands` then `git status --short cli/tests/golden` then `scripts/wsl-cargo.sh test -p auction_cli --features auction_sdk/testing`
Expected: all pass. The only new or changed golden file is `cli/tests/golden/dev_chain_id.txt`, with content `genesis           <64 hex chars>` and `exit: 0`. Any other changed golden is a regression: inspect it and revert with `git checkout cli/tests/golden/<file>`.

Run: `scripts/wsl-cargo.sh test -p engine_integration_tests --test sdk_plans`
Expected: pass (`sdk_deployment` compiles with the new field).

- [ ] **Step 5: Update the docs**

Append ADR-046 to `docs/design-log.md`:

```markdown
## ADR-046: Chain Identity for State-Changing Dev Commands

**Context**
Plan 3a M6: the dev commands (mock tokens, mock oracles) were guarded by the deployment file's `network` label only, so `--rpc` could aim them at any node with a file that says `localnet`. The read-only `dev resolve` and `dev balance` were blocked off localnet for no reason.

**Options**
- (A) Refuse `--rpc` for dev commands.
- (B) Refuse non-loopback RPC URLs.
- (C) Record the chain in the deployment file and compare it with the node before signing.

**Decision**
(C). The deployment file gains `chain_genesis_hash`, the hash of block 1. LEZ v0.2.4 builds block 1 from the sequencer config with timestamp 0, so it identifies a chain's configuration: a restarted localnet keeps it, a testnet or another configuration does not. `verify_chain_identity` compares it before any state-changing dev command signs (`Mismatch`, exit 2). The scripts record it right after starting the sequencer (`auction-cli dev chain-id`). The read-only `resolve`, `balance` and `chain-id` run on any network. The committed `deployments/localnet.json` keeps `null` (the value is recorded per run), so state-changing dev commands refuse with the committed file until a run records it.

**Trade-offs**
Two localnets started from the same config share block 1; both are throwaway, so this is accepted. The scanner keeps its own anchor at block 2 (ADR-043), which also changes across restarts.

**Status**
Accepted (Plan 3b, Task 3)

---
```

Append to ADR-044 under its existing amendments: `**Amended (Plan 3b, Task 3)** A bounded watch exits with the most severe step error (3 before 1), the first of equals (Plan 3a N3, keep_worst). State-changing dev commands also require the deployment file's chain_genesis_hash to match the node (ADR-046, exit 2); dev resolve, dev balance and the new dev chain-id work on any network.`

In `cli/README.md`, dev section: document `dev chain-id`, the `chain_genesis_hash` field, and which dev commands are read-only. In `docs/architecture/sdk.md`, verification section: one paragraph on `verify_chain_identity`.

- [ ] **Step 6: Gates**

Run the lint gate and the format gate. Expected: clean.

- [ ] **Step 7: Commit (controller, GPG signing on Windows)**

```bash
git add crates/auction_sdk/src/deployment.rs crates/auction_sdk/src/verify.rs crates/auction_sdk/src/testing/standard.rs \
  programs/integration_tests/tests/common/sdk.rs cli/src/args.rs cli/src/dev_cmds.rs cli/src/read_cmds.rs \
  cli/tests/admin_golden.rs cli/tests/golden/dev_chain_id.txt deployments/localnet.json scripts/localnet-smoke.sh \
  docs/design-log.md cli/README.md docs/architecture/sdk.md
git commit -m "fix(cli): require the recorded chain for dev commands and exit with the worst watch error"
```

---

### Task 4: Program CI with the risc0 toolchain (`programs`, `idl-drift`, `hook-patches`)

This is Plan 3b scope item 1. Until now `core.yml` built `auction_core` only. This task adds jobs that build the guests with the pinned risc0 toolchain and run all of the following:
- the engine, integration (including `sdk_plans` and `sdk_quotes`), SDK and CLI suites;
- the vendored stablecoin and token suites, unchanged;
- clippy over the workspace;
- an `idl-gen` diff against both committed IDLs, which also closes the Plan 3a Task 1 `name()` sample gap;
- the hook patch-series check.

**Files:**
- Create: `.github/actions/install-risc0/action.yml`, `scripts/ci/free-disk.sh`, `scripts/ci/idl-drift.sh`, `docs/architecture/ci.md`
- Modify: `.github/workflows/core.yml` (fmt list; jobs `programs`, `idl-drift`, `hook-patches`)
- Modify: `README.md` (CI section), `docs/architecture/programs.md` (carry-over list)

**Interfaces:**
- Consumes: `scripts/check-hook-patches.sh` (reads `LEZ_PROGRAMS`), `tools/idl-gen`, `scripts/wsl-env.sh` (Task 1).
- Produces:
  - Job ids `programs`, `idl-drift`, `hook-patches` in `core.yml`. Task 6 adds steps to `programs`; Tasks 8 and 15 add the jobs `coverage` and `requirements`.
  - Composite action `./.github/actions/install-risc0` (puts `~/.risc0/bin` and `~/.cargo/bin` on `PATH`; installs the pinned components).
  - `scripts/ci/free-disk.sh`.
  - `scripts/ci/idl-drift.sh`: exit 0 when unchanged, exit 1 with the diff.

- [ ] **Step 1: Write the IDL drift script and run it locally first**

Create `scripts/ci/idl-drift.sh`:

```bash
#!/usr/bin/env bash
# Regenerates both IDLs from the guest sources with tools/idl-gen and fails on any difference from the
# committed files (spec §9 drift checks). Also catches an enum variant whose guest function was never
# added (the Plan 3a Task 1 sample gap). Locally: scripts/wsl-env.sh scripts/ci/idl-drift.sh
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
export RISC0_SKIP_BUILD=1
cargo run --locked -q -p idl-gen -- programs/auction_engine/methods/guest/src/bin/auction_engine.rs > idl/auction_engine-idl.json
cargo run --locked -q -p idl-gen -- programs/stablecoin/methods/guest/src/bin/stablecoin.rs > idl/stablecoin-idl.json
if ! git diff --exit-code -- idl/; then
  echo "idl-drift: a committed IDL differs from its guest source; run scripts/ci/idl-drift.sh and commit idl/" >&2
  exit 1
fi
echo "idl-drift: both IDLs match their guest sources"
```

Run: `chmod +x scripts/ci/idl-drift.sh && git update-index --chmod=+x scripts/ci/idl-drift.sh && scripts/wsl-env.sh scripts/ci/idl-drift.sh`
Expected: `idl-drift: both IDLs match their guest sources`.

Decision rule if it reports a diff on the first local run:
- If only `idl/stablecoin-idl.json` differs, run `scripts/wsl-cargo.sh test -p engine_integration_tests --test idl` and `scripts/wsl-cargo.sh test -p auction_sdk --features testing` against the regenerated file. If both pass, keep the regenerated file (it is now the guest's truth) and name the diff (sections and counts) in the commit body.
- If they fail, or the engine IDL differs, `git checkout idl/` and STOP. Report the diff: a stale committed IDL that tests depend on needs a ruling.

- [ ] **Step 2: Add the runner helpers**

Create `scripts/ci/free-disk.sh`:

```bash
#!/usr/bin/env bash
# Hosted runners start with about 14 GB free; the risc0 guests, LEZ and our target need more.
# Removes preinstalled SDKs this repository never uses. CI only.
set -euo pipefail
df -h /
sudo rm -rf /usr/share/dotnet /usr/local/lib/android /opt/ghc /opt/hostedtoolcache/CodeQL /usr/local/share/boost
df -h /
```

Create `.github/actions/install-risc0/action.yml`:

```yaml
name: Install risc0 (pinned)
description: rzup with the component versions of lez/.github/docker/ci.Dockerfile (never a bare `rzup install`, which floats them)
runs:
  using: composite
  steps:
    - name: Cache the rzup components
      uses: actions/cache@v4
      with:
        path: ~/.risc0
        key: risc0-rust-1.94.1-cpp-2024.1.5-r0vm-3.0.5-cargo-risczero-3.0.5-v1
    - name: Install rzup and the pinned components
      shell: bash
      run: |
        set -euo pipefail
        if [[ ! -x "$HOME/.risc0/bin/rzup" ]]; then curl -L https://risczero.com/install | bash; fi
        "$HOME/.risc0/bin/rzup" install rust 1.94.1
        "$HOME/.risc0/bin/rzup" install cpp 2024.1.5
        "$HOME/.risc0/bin/rzup" install r0vm 3.0.5
        "$HOME/.risc0/bin/rzup" install cargo-risczero 3.0.5
        echo "$HOME/.risc0/bin" >> "$GITHUB_PATH"
        echo "$HOME/.cargo/bin" >> "$GITHUB_PATH"
    - name: Check the prover version
      shell: bash
      run: r0vm --version | grep -q "3.0.5"
```

Run: `chmod +x scripts/ci/free-disk.sh && git update-index --chmod=+x scripts/ci/free-disk.sh && bash -n scripts/ci/free-disk.sh && echo OK`
Expected: `OK`.

- [ ] **Step 3: Extend `core.yml`**

In `.github/workflows/core.yml`:
- add `-p auction_sdk -p auction_cli` to the `core` job's `cargo fmt` line;
- append these jobs:

```yaml
  programs:
    runs-on: ubuntu-22.04
    timeout-minutes: 150
    env:
      RISC0_DEV_MODE: "1"
    steps:
      - uses: actions/checkout@v4
      - run: scripts/ci/free-disk.sh
      - run: sudo apt-get update && sudo apt-get install -y --no-install-recommends clang libclang-dev pkg-config libssl-dev
      - uses: dtolnay/rust-toolchain@1.94.0
        with: { components: "rustfmt, clippy" }
      - uses: ./.github/actions/install-risc0
      - uses: Swatinem/rust-cache@v2
        with: { shared-key: programs }
      - name: clippy (workspace; the vendored upstream integration_tests crate is excluded, as in every task)
        run: RISC0_SKIP_BUILD=1 cargo clippy --locked --workspace --all-targets --exclude integration_tests -- -D warnings
      - name: engine, real-guest integration (incl. sdk_plans, sdk_quotes, chains, idl), SDK and CLI
        run: cargo test --locked -p auction_engine_core -p auction_engine_program -p engine_integration_tests -p auction_sdk -p auction_cli --features auction_sdk/testing,auction_sdk/rpc
      - name: vendored stablecoin and token suites with our hooks appended (spec §1.5)
        run: cargo test --locked -p stablecoin_core -p stablecoin_program -p token_program -p integration_tests

  idl-drift:
    runs-on: ubuntu-22.04
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.94.0
      - uses: Swatinem/rust-cache@v2
        with: { shared-key: idl }
      - run: scripts/ci/idl-drift.sh

  hook-patches:
    runs-on: ubuntu-22.04
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - name: pristine lez-programs 64c01210
        run: |
          git clone --filter=blob:none https://github.com/logos-blockchain/lez-programs "$RUNNER_TEMP/lez-programs"
          git -C "$RUNNER_TEMP/lez-programs" checkout -q 64c01210adecd6b557dbd021a3cc94553f243fd8
      - run: LEZ_PROGRAMS="$RUNNER_TEMP/lez-programs" scripts/check-hook-patches.sh
```

- [ ] **Step 4: Validate the workflow locally**

Run (inside WSL):

```bash
mkdir -p "$HOME/.local/bin" && cd "$HOME/.local/bin" && \
  bash <(curl -sSL https://raw.githubusercontent.com/rhysd/actionlint/v1.7.7/scripts/download-actionlint.bash) 1.7.7 && \
  cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && "$HOME/.local/bin/actionlint" .github/workflows/core.yml
```

Expected: no output, exit 0.

Then run each new job's command locally through the wrapper, in the background (timeout 3600000), in this order:
1. `RISC0_SKIP_BUILD=1 scripts/wsl-cargo.sh clippy --locked --workspace --all-targets --exclude integration_tests -- -D warnings`
2. `scripts/wsl-cargo.sh test --locked -p auction_engine_core -p auction_engine_program -p engine_integration_tests -p auction_sdk -p auction_cli --features auction_sdk/testing,auction_sdk/rpc`
3. `scripts/wsl-cargo.sh test --locked -p stablecoin_core -p stablecoin_program -p token_program -p integration_tests`
4. `scripts/wsl-env.sh env LEZ_PROGRAMS=$HOME/lez-spikes/s1/lez-programs scripts/check-hook-patches.sh`

Expected: every `test result: ok`; `check-hook-patches: patched tree matches programs/stablecoin`.

- [ ] **Step 5: Observe the CI run (controller; per Open decision 1)**

The controller pushes `feat/plan-3b` to `origin` (`git push -u origin feat/plan-3b`) after the commit step and runs `gh run watch --exit-status $(gh run list --branch feat/plan-3b --workflow core --limit 1 --json databaseId --jq '.[0].databaseId')`.
Expected: jobs `core`, `programs`, `idl-drift` and `hook-patches` succeed. If `programs` fails on disk space (`No space left on device`), add `/opt/microsoft /usr/local/.ghcup` to `free-disk.sh` and re-run. If it exceeds 150 minutes cold, re-run once: the cache makes the second run shorter. Record the wall times of the cold and warm runs in `docs/architecture/ci.md`.

If Open decision 1 is declined, Steps 4's local runs are the verification, and the first CI run happens when the user opens the pull request.

- [ ] **Step 6: Update the docs**

Create `docs/architecture/ci.md` with:
- the job table (Shared Reference R2, the rows that exist after this task);
- what each job runs (the commands above);
- the runner facts (Shared Reference R1);
- how to run each job locally through `scripts/wsl-env.sh` / `scripts/wsl-cargo.sh`;
- the cold and warm wall times from Step 5.

In `README.md`, replace the CI sentence with a pointer to `docs/architecture/ci.md` and the job list. In `docs/architecture/programs.md`, "Plan 3 carry-overs", change the first bullet to `Done (Plan 3b Task 4): program CI with the risc0 toolchain (jobs programs, idl-drift, hook-patches).`

- [ ] **Step 7: Gates**

Run the lint gate and the format gate. Expected: clean.

- [ ] **Step 8: Commit (controller, GPG signing on Windows)**

```bash
git add .github/actions/install-risc0/action.yml .github/workflows/core.yml scripts/ci/free-disk.sh scripts/ci/idl-drift.sh \
  docs/architecture/ci.md README.md docs/architecture/programs.md
git commit -m "ci: build the guests with risc0 and run the program, SDK, CLI, upstream, IDL and patch checks"
```

(If Step 1 kept a regenerated `idl/stablecoin-idl.json`, also stage it and name the diff in the commit body.)

---
### Task 5: Reproducible Docker guest builds and the ImageID refresh (ADR-047)

This is Plan 3b scope item 3. It closes the I4 "native build" residual. Our two guests are built with the upstream recipe: the same builder image (pinned by digest), the same rustflags and the same packager. The five deployable binaries are committed with a manifest, and the published ImageIDs become the Docker ones. In-process tests keep the native ELFs.

**Files:**
- Create: `scripts/build-guests.Dockerfile`, `scripts/build-guests.sh`, `scripts/check-guests.sh`, `.dockerignore`
- Create: `tools/risc0-packager/Cargo.toml`, `tools/risc0-packager/Cargo.lock`, `tools/risc0-packager/src/main.rs` (copied from lez-programs `64c01210` `tools/risc0-packager`, standalone workspace)
- Modify: `Cargo.toml` (`exclude` += `"tools/risc0-packager"`)
- Create: `artifacts/guests/{stablecoin,auction_engine,token,twap_oracle,token_mint_authority}.bin`, `artifacts/guests/manifest.json`
- Modify: `programs/integration_tests/examples/localnet_tool.rs` (`image-id <file>...`)
- Modify: `crates/auction_sdk/src/verify.rs` (test `guest_manifest_matches_the_localnet_deployment`)
- Create: `.github/workflows/guests.yml`
- Modify: `DEPLOYMENTS.md`, `deployments/localnet.json`, `THREAT_MODEL.md`, `scripts/localnet-smoke.sh` (`step_build`, `step_deploy` use `artifacts/guests`), `docs/architecture/localnet.md`, `docs/design-log.md` (ADR-047)

**Interfaces:**
- Consumes: `scripts/disk-guard.sh` (Task 1), Docker Desktop with WSL integration, S1's Docker-built upstream guests in `$HOME/lez-spikes/s1/lez-programs/target/guest/` (read only).
- Produces:
  - `scripts/build-guests.sh [out_dir=$HOME/lez-target/plan3b-guests]`: writes `stablecoin.bin` and `auction_engine.bin`.
  - `scripts/check-guests.sh [--write] [--sha-only] [dir=artifacts/guests]`: verifies or writes `manifest.json`; exit 1 on mismatch.
  - `localnet_tool image-id <file.bin>...`: prints `<name> <imageid-hex>` lines.
  - `artifacts/guests/manifest.json`, with schema `{"builder_image": "...@sha256:...", "guests": {"<name>": {"image_id": "<64 hex>", "sha256": "<64 hex>", "source": "..."}}}`.
  - Workflow job `guest-repro`.
  - Deployment ImageIDs in `DEPLOYMENTS.md` and `deployments/localnet.json` equal to the manifest's.

- [ ] **Step 1: Preflight Docker and disk**

Run inside WSL: `docker info --format '{{.ServerVersion}} {{.MemTotal}}' && scripts/disk-guard.sh 5 && docker image ls risczero/risc0-guest-builder`
Expected: a server version line, the guard passes with C: at 5 GiB or more, and the `r0.1.88.0` image is listed (S1 pulled it).
- If `docker` is not found in Ubuntu-20.04, STOP: the controller asks the user to start Docker Desktop and enable WSL integration for Ubuntu-20.04 (Settings → Resources → WSL integration). Nothing else unblocks this task.
- If the image is not listed and C: has less than 7 GiB free (the pull is about 1.4 GB into the Docker disk on C:), STOP and report.

Record the builder digest: `docker image inspect --format '{{index .RepoDigests 0}}' risczero/risc0-guest-builder:r0.1.88.0` (when absent, run `docker pull risczero/risc0-guest-builder:r0.1.88.0` first, then the guard again).

- [ ] **Step 2: Vendor the packager as its own workspace**

Copy `$HOME/lez-spikes/s1/lez-programs/tools/risc0-packager/src/main.rs` unchanged to `tools/risc0-packager/src/main.rs`, adding `#![forbid(unsafe_code)]` as its first line and the line `// Copied from lez-programs 64c01210 tools/risc0-packager (MIT OR Apache-2.0).` after it. Create `tools/risc0-packager/Cargo.toml`:

```toml
[package]
name = "risc0-packager"
version = "0.1.0"
edition = "2021"
license = "MIT OR Apache-2.0"
publish = false
description = "Wraps a guest ELF with the risc0 kernel into the deployable .bin (from lez-programs 64c01210)."

# A workspace of its own: the Docker build compiles it with the builder image's toolchain without
# resolving this repository's workspace.
[workspace]

[dependencies]
anyhow = "1"
risc0-binfmt = "=3.0.4"
risc0-build = "=3.0.5"
```

Add `"tools/risc0-packager",` to `exclude` in the root `Cargo.toml`. Run: `scripts/wsl-env.sh cargo generate-lockfile --manifest-path tools/risc0-packager/Cargo.toml && scripts/wsl-env.sh cargo build --locked --manifest-path tools/risc0-packager/Cargo.toml`
Expected: `Finished`; `tools/risc0-packager/Cargo.lock` exists.

- [ ] **Step 3: Write the Docker build**

Create `.dockerignore`:

```
.git
target
**/target
spikes
docs
.superpowers
artifacts
```

(Every workspace member stays in the context: guest path dependencies inherit `workspace = true` keys from the root manifest, which names all members.)

Create `scripts/build-guests.Dockerfile`, replacing `<DIGEST>` with the digest recorded in Step 1. This is a recorded value, written once.

```dockerfile
# syntax=docker/dockerfile:1.7
# Reproducible guest build (ADR-047), the lez-programs 64c01210 recipe (scripts/build-guests.Dockerfile):
# same builder image (pinned by digest here), rustflags and packager; our two guests.
ARG BUILDER=risczero/risc0-guest-builder:r0.1.88.0@<DIGEST>
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
    --mount=type=cache,id=lez-auction-engine-guests-target,sharing=locked,target=/src/target/risc0-guests <<'EOF'
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
EOF

FROM scratch AS export
COPY --from=build /guest-output /
```

Create `scripts/build-guests.sh`:

```bash
#!/usr/bin/env bash
# Builds our two guests reproducibly in Docker (ADR-047) into OUT (default $HOME/lez-target/plan3b-guests).
# Run inside WSL or Linux with Docker. The disk guard runs before (5 GiB, the Docker disk grows on C:)
# and after (3 GiB).
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
out="${1:-$HOME/lez-target/plan3b-guests}"
if [[ -x "$repo/scripts/disk-guard.sh" && -d /mnt/c ]]; then "$repo/scripts/disk-guard.sh" 5; fi
staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
DOCKER_BUILDKIT=1 docker build --output="$staging" -f "$repo/scripts/build-guests.Dockerfile" "$repo"
mkdir -p "$out"
install -m 0644 "$staging/stablecoin.bin" "$out/stablecoin.bin"
install -m 0644 "$staging/auction_engine.bin" "$out/auction_engine.bin"
sha256sum "$out"/*.bin
if [[ -x "$repo/scripts/disk-guard.sh" && -d /mnt/c ]]; then "$repo/scripts/disk-guard.sh"; fi
```

Make it executable: `chmod +x scripts/build-guests.sh && git update-index --chmod=+x scripts/build-guests.sh`.

- [ ] **Step 4: Build twice and require identical bytes**

Run (background, timeout 3600000) inside WSL: `scripts/build-guests.sh $HOME/lez-target/plan3b-guests/a && scripts/build-guests.sh $HOME/lez-target/plan3b-guests/b && cmp $HOME/lez-target/plan3b-guests/a/stablecoin.bin $HOME/lez-target/plan3b-guests/b/stablecoin.bin && cmp $HOME/lez-target/plan3b-guests/a/auction_engine.bin $HOME/lez-target/plan3b-guests/b/auction_engine.bin && echo REPRODUCIBLE`
Expected: `REPRODUCIBLE`; the disk guard passes after each build.

Decision rule: if `cmp` reports a difference, run `diffoscope`-free triage: `strings` both files and look for embedded absolute paths or timestamps. The recipe pins `WORKDIR /src`, `--locked` and the image digest, so a difference means a non-deterministic build input. STOP and report it with the two sha256 values and the first differing strings; do not commit binaries that do not reproduce.

- [ ] **Step 5: Add `image-id` to the localnet tool and the manifest checker**

In `programs/integration_tests/examples/localnet_tool.rs`:
- extend `USAGE` with `| image-id <file.bin>...`;
- add a match arm:

```rust
        Some("image-id") if args.len() >= 2 => {
            for path in &args[1..] {
                let bytes = fs::read(path).unwrap_or_else(|e| fail(&format!("{path}: {e}")));
                let digest = risc0_zkvm::compute_image_id(&bytes)
                    .unwrap_or_else(|e| fail(&format!("{path}: not a risc0 program binary: {e}")));
                let mut id: ProgramId = [0; 8];
                for (limb, w) in id.iter_mut().zip(digest.as_words()) {
                    *limb = *w;
                }
                let name = std::path::Path::new(path)
                    .file_stem()
                    .map_or_else(|| path.clone(), |s| s.to_string_lossy().into_owned());
                println!("{name} {}", hex_of(&id));
            }
        }
```

Sanity check: the native ELF's computed id must equal its constant. Run: `scripts/wsl-cargo.sh run -q -p engine_integration_tests --example localnet_tool -- export-guests /tmp/native && scripts/wsl-cargo.sh run -q -p engine_integration_tests --example localnet_tool -- image-id /tmp/native/auction_engine.bin`
Expected: the printed id equals the `AUCTION_ENGINE_ID` hex printed by `export-guests`.

Create `scripts/check-guests.sh`:

```bash
#!/usr/bin/env bash
# Verifies (default) or writes (--write) artifacts/guests/manifest.json: sha256 and ImageID of every
# committed guest binary (ADR-047). --sha-only skips the ImageIDs (no cargo; used by demo preflight).
#   scripts/check-guests.sh [--write] [--sha-only] [dir=artifacts/guests]
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
write=0; sha_only=0
while [[ "${1:-}" == --* ]]; do
  case "$1" in --write) write=1 ;; --sha-only) sha_only=1 ;; *) echo "unknown flag $1" >&2; exit 2 ;; esac
  shift
done
dir="${1:-$repo/artifacts/guests}"
manifest="$dir/manifest.json"
ids=""
if (( sha_only == 0 )); then
  ids="$(cd "$repo" && cargo run -q --locked -p engine_integration_tests --example localnet_tool -- image-id "$dir"/*.bin)"
fi
python3 - "$dir" "$manifest" "$write" "$sha_only" "$ids" <<'PY'
import hashlib, json, os, sys
d, manifest, write, sha_only, ids = sys.argv[1], sys.argv[2], sys.argv[3] == "1", sys.argv[4] == "1", sys.argv[5]
image = dict(line.split() for line in ids.splitlines() if line.strip())
found = {}
for f in sorted(os.listdir(d)):
    if f.endswith(".bin"):
        name = f[:-4]
        found[name] = {"sha256": hashlib.sha256(open(os.path.join(d, f), "rb").read()).hexdigest()}
        if name in image:
            found[name]["image_id"] = image[name]
if write:
    m = json.load(open(manifest)) if os.path.exists(manifest) else {"builder_image": "", "guests": {}}
    for name, v in found.items():
        m["guests"].setdefault(name, {}).update(v)
    json.dump(m, open(manifest, "w"), indent=2, sort_keys=True)
    open(manifest, "a").write("\n")
    print(f"check-guests: wrote {manifest}")
    sys.exit(0)
m = json.load(open(manifest))
errors = []
for name, want in m["guests"].items():
    got = found.get(name)
    if got is None:
        errors.append(f"{name}: in the manifest but no {name}.bin")
        continue
    if got["sha256"] != want["sha256"]:
        errors.append(f"{name}: sha256 {got['sha256']} != manifest {want['sha256']}")
    if not sha_only and got.get("image_id") != want["image_id"]:
        errors.append(f"{name}: ImageID {got.get('image_id')} != manifest {want['image_id']}")
for name in found:
    if name not in m["guests"]:
        errors.append(f"{name}.bin is not in the manifest")
for e in errors:
    print("check-guests: " + e, file=sys.stderr)
sys.exit(1 if errors else 0)
PY
```

Make it executable (`chmod +x` and `git update-index --chmod=+x`).

- [ ] **Step 6: Commit the binaries and write the manifest**

Run inside WSL:

```bash
mkdir -p artifacts/guests
cp $HOME/lez-target/plan3b-guests/a/stablecoin.bin $HOME/lez-target/plan3b-guests/a/auction_engine.bin artifacts/guests/
cp $HOME/lez-spikes/s1/lez-programs/target/guest/{token,twap_oracle,token_mint_authority}.bin artifacts/guests/
scripts/wsl-env.sh scripts/check-guests.sh --write
```

Then edit `artifacts/guests/manifest.json`:
- set `builder_image` to `risczero/risc0-guest-builder:r0.1.88.0@<DIGEST>`;
- give each guest a `source`. Ours: `"this repository, scripts/build-guests.sh (programs/stablecoin = lez-programs 64c01210 + patches/stablecoin)"` and `"this repository, scripts/build-guests.sh"`. Upstream: `"lez-programs 64c01210, make build-programs (S1)"`.

Run: `scripts/wsl-env.sh scripts/check-guests.sh`
Expected: exit 0. The `token` and `twap_oracle` ImageIDs equal `14b3bc6c...67a8` and `5681e27c...9b31`, the values already in `DEPLOYMENTS.md`.

- [ ] **Step 7: Pin the deployment to the manifest (failing test first)**

In `crates/auction_sdk/src/verify.rs` `mod tests`, add:

```rust
    #[test]
    fn guest_manifest_matches_the_localnet_deployment() {
        // ADR-047: the deployed ImageIDs are the reproducible Docker ones in artifacts/guests.
        let d = Deployment::from_json(
            include_str!("../../../deployments/localnet.json"),
            "deployments/localnet.json",
        )
        .expect("parses");
        let m: serde_json::Value =
            serde_json::from_str(include_str!("../../../artifacts/guests/manifest.json"))
                .expect("manifest parses");
        let id = |name: &str| {
            m["guests"][name]["image_id"]
                .as_str()
                .unwrap_or_else(|| panic!("manifest has no {name}"))
                .to_owned()
        };
        assert_eq!(program_id_hex(&d.engine_program), id("auction_engine"));
        assert_eq!(program_id_hex(&d.host_program), id("stablecoin"));
        assert_eq!(program_id_hex(&d.token_program), id("token"));
        let twap = d.twap_oracle_program.expect("localnet names the twap oracle");
        assert_eq!(program_id_hex(&twap), id("twap_oracle"));
    }
```

Run: `scripts/wsl-cargo.sh test -p auction_sdk --features testing guest_manifest_matches`
Expected: FAIL: the engine and stablecoin ids in `deployments/localnet.json` are still the native ones.

Update `deployments/localnet.json` (`engine_program_id` and `host_program_id` = the manifest values) and the `DEPLOYMENTS.md` table rows for stablecoin and auction_engine, with Source `artifacts/guests (Docker, ADR-047)`. Replace the "NATIVE build" paragraph with:

```markdown
All five ImageIDs come from reproducible Docker builds (ADR-047): the binaries are committed in
`artifacts/guests/` with their sha256 and ImageIDs in `artifacts/guests/manifest.json`, and CI job
`guest-repro` rebuilds them from source and requires identical bytes. Anyone can rebuild with
`scripts/build-guests.sh` (our two guests) and lez-programs `make build-programs` at 64c01210 (token,
twap_oracle, token_mint_authority) and compare with `scripts/check-guests.sh`. The in-process test suites
run natively built ELFs of the same source, whose ImageIDs differ; they are never deployed.
```

Run: `scripts/wsl-cargo.sh test -p auction_sdk --features testing`
Expected: all pass, including `localnet_json_matches_deployments_md` and `guest_manifest_matches_the_localnet_deployment`.

- [ ] **Step 8: Deploy the committed binaries in the smoke script**

In `scripts/localnet-smoke.sh`:
- `step_build`: drop the `export-guests` call, and read the ids from the manifest instead:

```bash
step_build() {
  (cd "$REPO" && scripts/wsl-cargo.sh build -p auction_cli)
  "$REPO/scripts/check-guests.sh" --sha-only
  local engine host tok twap
  read -r engine host tok twap < <(python3 - "$REPO/artifacts/guests/manifest.json" <<'PY'
import json, sys
g = json.load(open(sys.argv[1]))["guests"]
print(g["auction_engine"]["image_id"], g["stablecoin"]["image_id"], g["token"]["image_id"], g["twap_oracle"]["image_id"])
PY
)
  python3 - "$DEP" "$engine" "$host" "$tok" "$twap" <<'PY'
import json, sys
path, engine, host, tok, twap = sys.argv[1:]
json.dump({"network": "localnet", "sequencer_url": "http://127.0.0.1:3040", "engine_program_id": engine,
           "host_program_id": host, "token_program_id": tok, "twap_oracle_program_id": twap,
           "engine_admin": None, "config": None, "chain_genesis_hash": None},
          open(path, "w", encoding="utf-8"), indent=2)
PY
  echo "deployment file: $DEP"
}
```

- `step_deploy`: deploy `"$REPO/artifacts/guests/$p.bin"` for `p` in `token twap_oracle token_mint_authority stablecoin auction_engine`;
- remove the `TOK`, `TWAP` and `UP` variables (now unused);
- update the header comment: the S1 Docker-built guests are no longer needed, only the S1 sequencer and wallet.

- [ ] **Step 9: Add the CI reproducibility job**

Create `.github/workflows/guests.yml`:

```yaml
name: guests
on:
  push:
    paths: ["programs/**", "artifacts/guests/**", "scripts/build-guests*", "tools/risc0-packager/**", "patches/**"]
  schedule:
    - cron: "5 4 * * *"
  workflow_dispatch:
jobs:
  guest-repro:
    runs-on: ubuntu-22.04
    timeout-minutes: 180
    steps:
      - uses: actions/checkout@v4
      - run: scripts/ci/free-disk.sh
      - uses: dtolnay/rust-toolchain@1.94.0
      - uses: ./.github/actions/install-risc0
      - uses: Swatinem/rust-cache@v2
        with: { shared-key: programs }
      - name: rebuild our guests in Docker
        run: scripts/build-guests.sh "$RUNNER_TEMP/guests"
      - name: rebuild the upstream guests (lez-programs 64c01210)
        if: github.event_name != 'push'
        run: |
          git clone --filter=blob:none https://github.com/logos-blockchain/lez-programs "$RUNNER_TEMP/lez-programs"
          git -C "$RUNNER_TEMP/lez-programs" checkout -q 64c01210adecd6b557dbd021a3cc94553f243fd8
          make -C "$RUNNER_TEMP/lez-programs" build-programs
          cp "$RUNNER_TEMP/lez-programs/target/guest/"{token,twap_oracle,token_mint_authority}.bin "$RUNNER_TEMP/guests/"
      - name: on push, the upstream binaries come from the repository
        if: github.event_name == 'push'
        run: cp artifacts/guests/{token,twap_oracle,token_mint_authority}.bin "$RUNNER_TEMP/guests/"
      - name: byte-for-byte equal to artifacts/guests (sha256 and ImageID)
        run: cp artifacts/guests/manifest.json "$RUNNER_TEMP/guests/" && scripts/check-guests.sh "$RUNNER_TEMP/guests"
```

Run: `"$HOME/.local/bin/actionlint" .github/workflows/guests.yml`
Expected: no output.

- [ ] **Step 10: Re-run the smoke on the Docker binaries**

Run (background, timeout 3600000) inside WSL: `scripts/localnet-smoke.sh step_build step_sequencer step_accounts step_auth step_deploy step_host step_engine step_bind step_collateral step_holdings step_flow step_system step_check step_stop`
Expected: `no failed execution check`, and the final balances of `docs/architecture/localnet.md` (owner-coll 99_998_999_950, owner-stable 70_000, keeper-coll 50, taker-stable 500, taker-protocol 9_800, buffer 29_500). `host bind` prints the Docker ImageIDs.

- [ ] **Step 11: Update the docs**

- Append ADR-047 to `docs/design-log.md`:

```markdown
## ADR-047: Reproducible Docker Guest Builds; Committed Binaries; Native ELFs Stay in Tests

**Context**
I4: clients compare a config's program ids with `DEPLOYMENTS.md`, which until now held native-build ImageIDs that nobody else could reproduce (THREAT_MODEL I4 residual). The upstream guests are Docker-built and their ImageIDs match lez-programs' own DEPLOYMENTS.md (S1).

**Options**
- (A) Keep native builds.
- (B) `RISC0_USE_DOCKER` for every test build.
- (C) Build the deployable guests with the upstream Docker recipe, commit the binaries with a manifest, check reproduction in CI; tests keep native ELFs.

**Decision**
(C). `scripts/build-guests.sh` runs lez-programs' recipe (builder image pinned by digest, same rustflags, the vendored `risc0-packager`) for stablecoin (with hooks) and auction_engine. The two binaries and the three upstream ones (token, twap_oracle, token_mint_authority from lez-programs `make build-programs`) are committed in `artifacts/guests/` with `manifest.json` (sha256, ImageID, source). `deployments/localnet.json` and `DEPLOYMENTS.md` carry these ImageIDs (`guest_manifest_matches_the_localnet_deployment`). CI job `guest-repro` rebuilds and requires identical bytes. The localnet scripts deploy the committed binaries, so a demo needs no Docker.

**Trade-offs**
About 2.3 MB of binaries in git. Two ImageID sets exist: the deployed Docker one and the native one of the in-process tests and of the Task 7 receipts (ADR-045). Every later guest change must rebuild and refresh the manifest in the same commit (Plan 3b Global Constraints).

**Status**
Accepted (Plan 3b, Task 5)

---
```

- `THREAT_MODEL.md`: replace the "Published ImageIDs are not yet reproducible (I4)" bullet with `**Published ImageIDs are reproducible (I4, ADR-047).** ... (keep the RPC-trust sentence)`. Add a review-log entry: `2026-10-<day>, Plan 3b Task 5: I4 native-build residual closed; the RPC endpoint is still trusted.`
- `docs/architecture/localnet.md`: add a "Plan 3b Task 5 re-run" paragraph with the result of Step 10 and the new ImageIDs.
- `docs/architecture/programs.md`: change the "Reproducible Docker builds" carry-over bullet to `Done (Plan 3b Task 5, ADR-047)`.

- [ ] **Step 12: Gates**

Run the lint gate and the format gate, then `scripts/disk-guard.sh`. Expected: clean, and the guard passes.

- [ ] **Step 13: Commit (controller, GPG signing on Windows)**

```bash
git add .dockerignore scripts/build-guests.Dockerfile scripts/build-guests.sh scripts/check-guests.sh tools/risc0-packager Cargo.toml \
  artifacts/guests programs/integration_tests/examples/localnet_tool.rs crates/auction_sdk/src/verify.rs \
  .github/workflows/guests.yml DEPLOYMENTS.md deployments/localnet.json THREAT_MODEL.md scripts/localnet-smoke.sh \
  docs/architecture/localnet.md docs/architecture/programs.md docs/design-log.md
git commit -m "build: reproducible Docker guests, committed binaries and refreshed ImageIDs (ADR-047)"
```

---

### Task 6: `CYCLES.md`, transaction sizes, the P2 load test and the cycle regression check (ADR-048)

This is Plan 3b scope item 4 and spec §11. Every instruction's executor cycles and signed transaction size are written to a JSON-lines file by the existing `chains.rs` scenarios. They are compared with a committed baseline, which fails on more than 10 % growth, a missing or new label, or more than 10 executions. `CYCLES.md` is generated from the same data. The load test measures N concurrent auctions with interleaved bids.

**Files:**
- Modify: `programs/integration_tests/Cargo.toml` (`borsh`), `programs/integration_tests/tests/common/measure.rs` (`record`), `programs/integration_tests/tests/common/mod.rs` (`signed_tx_bytes`, `measured_full`, `measured` records)
- Create: `programs/integration_tests/tests/load.rs`
- Create: `scripts/cycles.sh`, `scripts/check-cycles.py`, `scripts/tests/test_check_cycles.py`, `cycles/baseline.json`, `CYCLES.md`
- Modify: `.github/workflows/core.yml` (`programs` job: the cycle steps), `docs/architecture/ci.md`, `docs/architecture/programs.md`, `docs/design-log.md` (ADR-048)

**Interfaces:**
- Consumes: `measure::measure`, `Measured` (Task 1), `World::measured`, the `chains.rs` tests.
- Produces:
  - `measure::record(label: &str, m: &Measured, tx_bytes: usize)`: prints `TXSIZE[label] bytes=N` and appends a JSON line to `$CYCLES_OUT` when that is set.
  - `World::signed_tx_bytes(&self, accounts: &[AccountId], signers: &[u8], ix: &Ix) -> usize`.
  - `World::measured_full(label, accounts, signers, ix) -> (Measured, Result<(), LeeError>)`.
  - `scripts/cycles.sh OUT.jsonl`.
  - `scripts/check-cycles.py BASELINE CURRENT [--write-baseline] [--write-md CYCLES.md]`: exit 1 on a violation.
  - `cycles/baseline.json`.
  - `CYCLES.md`, with a generated table between `<!-- cycles:begin -->` and `<!-- cycles:end -->`.
  - Test `load_concurrent_auctions_with_interleaved_bids_stay_isolated`.

- [ ] **Step 1: Write the checker's failing tests**

Create `scripts/tests/test_check_cycles.py`:

```python
"""Review Focus 4: the cycle gate's own blind spots (Plan 3b Task 6)."""
import importlib.util
import pathlib
import unittest

SPEC = importlib.util.spec_from_file_location(
    "check_cycles", pathlib.Path(__file__).resolve().parents[1] / "check-cycles.py")
cc = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cc)


def row(total, executions=1):
    return {"label": "x", "executions": executions, "total_cycles": total,
            "per_exec": [{"program": "auction_engine", "cycles": total}], "tx_bytes": 100}


class CompareTest(unittest.TestCase):
    def test_exactly_ten_percent_passes(self):
        self.assertEqual(cc.compare({"a": row(1000)}, {"a": row(1100)}), [])

    def test_more_than_ten_percent_fails(self):
        errors = cc.compare({"a": row(1000)}, {"a": row(1101)})
        self.assertEqual(len(errors), 1)
        self.assertIn("a: 1101 cycles", errors[0])

    def test_new_label_without_baseline_fails(self):
        errors = cc.compare({}, {"new": row(10)})
        self.assertIn("no baseline", errors[0])

    def test_removed_label_fails(self):
        errors = cc.compare({"gone": row(10)}, {})
        self.assertIn("not measured", errors[0])

    def test_more_than_ten_executions_fails(self):
        errors = cc.compare({"a": row(1000, 11)}, {"a": row(1000, 11)})
        self.assertTrue(any("11 executions" in e for e in errors), errors)

    def test_an_execution_over_32_mi_cycles_fails(self):
        big = 32 * 1024 * 1024 + 1
        errors = cc.compare({"a": row(big)}, {"a": row(big)})
        self.assertTrue(any("32 Mi" in e for e in errors), errors)

    def test_shrinking_passes(self):
        self.assertEqual(cc.compare({"a": row(1000)}, {"a": row(10)}), [])


if __name__ == "__main__":
    unittest.main()
```

Run: `python3 -m unittest discover -s scripts/tests -p 'test_check_cycles.py' -v`
Expected: ERROR, with `FileNotFoundError` (or `No such file`) for `check-cycles.py`.

- [ ] **Step 2: Write the checker**

Create `scripts/check-cycles.py`:

```python
#!/usr/bin/env python3
"""Cycle regression check (spec §11, ADR-048).

usage: check-cycles.py BASELINE CURRENT_JSONL [--write-baseline] [--write-md CYCLES.md]

CURRENT_JSONL is written by scripts/cycles.sh (one JSON object per measured instruction). Fails with
exit 1 when an instruction's total executor cycles grew by more than 10 % over the baseline, when a
label is in only one of the two sets, when a chain has more than 10 executions, or when one execution
exceeds the 32 Mi cycle session limit.
"""
import json
import sys

LIMIT_PERCENT = 10
MAX_EXECUTIONS = 10
SESSION_LIMIT = 32 * 1024 * 1024
BEGIN, END = "<!-- cycles:begin -->", "<!-- cycles:end -->"


def load_current(path):
    rows = {}
    with open(path, encoding="utf-8") as f:
        for line in f:
            if not line.strip():
                continue
            r = json.loads(line)
            if r["label"] in rows and rows[r["label"]] != r:
                raise SystemExit(f"check-cycles: label {r['label']} measured twice with different results; rename one in chains.rs")
            rows[r["label"]] = r
    return rows


def compare(baseline, current):
    errors = []
    for label in sorted(set(baseline) | set(current)):
        if label not in current:
            errors.append(f"{label}: in the baseline but not measured (removed or renamed? update cycles/baseline.json in the same commit)")
            continue
        if label not in baseline:
            errors.append(f"{label}: measured but has no baseline (run check-cycles.py with --write-baseline and commit cycles/baseline.json)")
            continue
        old, new = baseline[label]["total_cycles"], current[label]["total_cycles"]
        if new * 100 > old * (100 + LIMIT_PERCENT):
            errors.append(f"{label}: {new} cycles is {100 * (new - old) / old:.1f} % above the baseline {old} (limit {LIMIT_PERCENT} %)")
        if current[label]["executions"] > MAX_EXECUTIONS:
            errors.append(f"{label}: {current[label]['executions']} executions (limit {MAX_EXECUTIONS}, ADR-036)")
        for e in current[label]["per_exec"]:
            if e["cycles"] > SESSION_LIMIT:
                errors.append(f"{label}: one {e['program']} execution takes {e['cycles']} cycles, over 32 Mi")
    return errors


def table(current):
    lines = ["| Instruction (scenario label) | Executions | Total cycles | Largest execution | % of 32 Mi | Signed tx bytes |",
             "|---|---|---|---|---|---|"]
    for label in sorted(current):
        r = current[label]
        largest = max(e["cycles"] for e in r["per_exec"])
        lines.append(f"| {label} | {r['executions']} | {r['total_cycles']:,} | {largest:,} | {100 * largest / SESSION_LIMIT:.1f} | {r['tx_bytes']} |")
    return "\n".join(lines)


def write_md(path, current):
    text = open(path, encoding="utf-8").read()
    head, rest = text.split(BEGIN, 1)
    _, tail = rest.split(END, 1)
    open(path, "w", encoding="utf-8").write(f"{head}{BEGIN}\n{table(current)}\n{END}{tail}")


def main(argv):
    if len(argv) < 3:
        print(__doc__, file=sys.stderr)
        return 2
    current = load_current(argv[2])
    if "--write-baseline" in argv:
        json.dump(current, open(argv[1], "w", encoding="utf-8"), indent=2, sort_keys=True)
        print(f"check-cycles: wrote {argv[1]} ({len(current)} labels)")
    if "--write-md" in argv:
        write_md(argv[argv.index("--write-md") + 1], current)
    baseline = json.load(open(argv[1], encoding="utf-8"))
    errors = compare(baseline, current)
    for e in errors:
        print("check-cycles: " + e, file=sys.stderr)
    if not errors:
        print(f"check-cycles: {len(current)} instructions within {LIMIT_PERCENT} % of the baseline")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
```

Run: `python3 -m unittest discover -s scripts/tests -p 'test_check_cycles.py' -v`
Expected: 7 tests OK.

- [ ] **Step 3: Record cycles and transaction sizes from `chains.rs`**

Add `borsh = { workspace = true }` to `[dependencies]` of `programs/integration_tests/Cargo.toml`.

Append to `programs/integration_tests/tests/common/measure.rs`:

```rust
/// Prints the signed transaction size and, when `CYCLES_OUT` is set, appends one JSON line for the
/// cycle regression check (`scripts/check-cycles.py`, ADR-048).
pub fn record(label: &str, m: &Measured, tx_bytes: usize) {
    println!("TXSIZE[{label}] bytes={tx_bytes}");
    let Ok(path) = std::env::var("CYCLES_OUT") else {
        return;
    };
    let line = serde_json::json!({
        "label": label,
        "executions": m.per_exec.len(),
        "total_cycles": m.total,
        "per_exec": m.per_exec.iter().map(|(p, c)| serde_json::json!({"program": p, "cycles": c})).collect::<Vec<_>>(),
        "tx_bytes": tx_bytes,
    });
    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .unwrap_or_else(|e| panic!("CYCLES_OUT {path}: {e}"));
    writeln!(f, "{line}").unwrap();
}
```

In `programs/integration_tests/tests/common/mod.rs`, replace `World::measured` with:

```rust
    /// Measures the chain (executions, cycles; proofs under `AUCTION_PROVE=1`), then submits.
    pub fn measured_full(
        &mut self,
        label: &str,
        accounts: Vec<AccountId>,
        signers: &[u8],
        ix: Ix,
    ) -> (measure::Measured, Result<(), LeeError>) {
        self.seed_clock();
        let keys: Vec<PrivateKey> = signers.iter().map(|n| key(*n)).collect();
        let refs: Vec<&PrivateKey> = keys.iter().collect();
        let m = measure::measure(label, &self.state, engine_program(), &accounts, &refs, &ix);
        (m, self.engine(accounts, signers, ix))
    }

    /// `measured_full` for the ADR-036 tables: also records cycles and the signed transaction size.
    /// Returns (executions, result).
    pub fn measured(
        &mut self,
        label: &str,
        accounts: Vec<AccountId>,
        signers: &[u8],
        ix: Ix,
    ) -> (usize, Result<(), LeeError>) {
        let tx_bytes = self.signed_tx_bytes(&accounts, signers, &ix);
        let (m, r) = self.measured_full(label, accounts, signers, ix);
        measure::record(label, &m, tx_bytes);
        (m.per_exec.len(), r)
    }

    /// Borsh size of the signed engine transaction `World::engine` would submit (spec §11 tx sizes).
    /// The RPC wire form adds a one-byte `LeeTransaction` variant tag and base64.
    pub fn signed_tx_bytes(&self, accounts: &[AccountId], signers: &[u8], ix: &Ix) -> usize {
        let keys: Vec<PrivateKey> = signers.iter().map(|n| key(*n)).collect();
        let refs: Vec<&PrivateKey> = keys.iter().collect();
        let nonces = signers
            .iter()
            .map(|n| self.state.get_account_by_id(id(*n)).nonce)
            .collect();
        let message =
            public_transaction::Message::try_new(engine_program(), accounts.to_vec(), nonces, ix)
                .expect("message is valid");
        let witness = public_transaction::WitnessSet::for_message(&message, &refs);
        borsh::to_vec(&PublicTransaction::new(message, witness))
            .expect("borsh")
            .len()
    }
```

Create `scripts/cycles.sh`:

```bash
#!/usr/bin/env bash
# Measures every instruction's executor cycles and signed transaction size (chains.rs scenarios, ADR-036)
# into a JSON-lines file for scripts/check-cycles.py. Locally: scripts/wsl-env.sh scripts/cycles.sh OUT
set -euo pipefail
out="$(realpath -m "${1:?usage: cycles.sh OUT.jsonl}")"
cd "$(dirname "${BASH_SOURCE[0]}")/.."
rm -f "$out"
CYCLES_OUT="$out" cargo test --locked -p engine_integration_tests --test chains -- --test-threads 1
test -s "$out" || { echo "cycles.sh: no measurements were written to $out" >&2; exit 1; }
echo "cycles.sh: $(wc -l < "$out") measurements in $out"
```

Make it executable. Run: `scripts/wsl-env.sh scripts/cycles.sh /tmp/cycles.jsonl && python3 -c "import json; [json.loads(l) for l in open('/tmp/cycles.jsonl')]; print('JSONL-OK')"`
Expected: `cycles.sh: <n> measurements`, with one line per `w.measured` call in `chains.rs`, then `JSONL-OK`.

If `check-cycles.py` later reports `label ... measured twice with different results`, rename the second occurrence in `chains.rs` by appending `_2` to its label string, and re-run.

- [ ] **Step 4: Write `CYCLES.md` and the baseline**

Create `CYCLES.md`:

```markdown
# Cycles and transaction sizes (spec §11, PEND10)

Measured by `scripts/cycles.sh` (the ADR-036 scenarios in `programs/integration_tests/tests/chains.rs`):
every program execution of each transaction's chain replayed with the risc0 executor on the natively built
guests, in dev mode. These are executor cycles, not proving cost; proving numbers are in
`docs/architecture/real-proofs.md` (ADR-045). Limits: 32 Mi (33,554,432) cycles per execution, at most 10
executions per transaction (11 with the top level on the public path, 10 on the private path, ADR-036).

CI (`core.yml`, job `programs`) re-measures on every push and fails when an instruction grows by more than
10 % over `cycles/baseline.json`, when an instruction is added or removed without updating the baseline, or
when a limit is exceeded (`scripts/check-cycles.py`, ADR-048). Reproduce:
`scripts/wsl-env.sh scripts/cycles.sh /tmp/c.jsonl && python3 scripts/check-cycles.py cycles/baseline.json /tmp/c.jsonl`.

<!-- cycles:begin -->
<!-- cycles:end -->

## P2 load test (N concurrent auctions, interleaved bids)

`load_concurrent_auctions_with_interleaved_bids_stay_isolated` (`tests/load.rs`): <filled from the test's LOAD line in Step 5>.

## Real proofs

See `docs/architecture/real-proofs.md` and the evidence linked from ADR-045 (Task 7).
```

Run: `python3 scripts/check-cycles.py cycles/baseline.json /tmp/cycles.jsonl --write-baseline --write-md CYCLES.md`
Expected: `wrote cycles/baseline.json`, then `... instructions within 10 % of the baseline`; the table now appears in `CYCLES.md`. Compare its totals with ADR-036; they must be equal (same scenarios, same guests).

- [ ] **Step 5: Write the load test**

Create `programs/integration_tests/tests/load.rs`:

```rust
//! P2 / R3 load test (spec §11): N concurrent auctions with interleaved partial bids. With no on-chain
//! list (ADR-008) and per-auction PDAs, a bid's cost must not depend on how many auctions are live,
//! and every auction settles with the reservations released.

mod common;

use auction_engine_core::{state::CollateralConfigState, Instruction as Ix};
use common::*;

const N: u64 = 6;
const PARTIAL: u128 = 30_000;
const ROUNDS: usize = 3;
/// Target of a 100_000 / 120_000 position with a 10 % penalty.
const TARGET: u128 = 132_000;

#[test]
fn load_concurrent_auctions_with_interleaved_bids_stay_isolated() {
    let mut w = World::with_engine();
    let positions: Vec<_> = (0..N)
        .map(|n| w.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, n, 100_000, 120_000))
        .collect();
    w.set_min_cr(2 * ONE);
    for p in &positions {
        w.liquidate(*p, 0).expect("liquidate");
    }
    w.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 10_000_000);
    let mut cycles = Vec::new();
    for _ in 0..ROUNDS {
        for p in &positions {
            let accounts = w.bid_accounts(*p, 0, k::BIDDER_STABLE, k::BIDDER_COLL, false);
            let (m, r) = w.measured_full(
                "load_bid",
                accounts,
                &[k::BIDDER_STABLE],
                Ix::Bid {
                    coin_in_max: PARTIAL,
                    min_collateral_out: 0,
                },
            );
            r.expect("partial bid");
            cycles.push(m.total);
        }
    }
    let (min, max) = (
        *cycles.iter().min().expect("bids"),
        *cycles.iter().max().expect("bids"),
    );
    println!(
        "LOAD N={N} bids={} min_cycles={min} max_cycles={max} spread_percent={:.2}",
        cycles.len(),
        100.0 * (max - min) as f64 / min as f64
    );
    assert!(
        max * 100 <= min * 105,
        "a bid's cost must not grow with the number of live auctions: {min}..{max}"
    );
    for p in &positions {
        w.bid_settle(*p, TARGET, 0).expect("final bid");
    }
    let coll: CollateralConfigState = w.read(coll_config());
    assert_eq!(coll.type_reserved, 0, "every reservation released once");
}
```

Run: `scripts/wsl-cargo.sh test -p engine_integration_tests --test load -- --nocapture`
Expected: PASS and a `LOAD N=6 bids=18 ...` line. Paste that line into the P2 section of `CYCLES.md`.

- [ ] **Step 6: Wire the check into CI**

In `.github/workflows/core.yml`, job `programs`, append:

```yaml
      - name: cycle regression check (ADR-048)
        run: scripts/cycles.sh "$RUNNER_TEMP/cycles.jsonl" && python3 scripts/check-cycles.py cycles/baseline.json "$RUNNER_TEMP/cycles.jsonl"
      - name: checker unit tests
        run: python3 -m unittest discover -s scripts/tests -v
      - uses: actions/upload-artifact@v4
        if: always()
        with: { name: cycles, path: "${{ runner.temp }}/cycles.jsonl", if-no-files-found: ignore }
```

Run: `"$HOME/.local/bin/actionlint" .github/workflows/core.yml`
Expected: no output.

Decision rule after the first CI run (Open decision 1): if CI's `cycles.jsonl` differs from the local baseline, CI's native build may differ from the local one.
- If every label is within 10 %, the gate passed; record the largest relative difference in ADR-048.
- If a label is over 10 %, download the artifact. Replace `cycles/baseline.json` with CI's values only if the scenario and guests are unchanged (`git diff` empty under `programs/`), with a commit body that names the toolchain difference; otherwise treat it as a real regression.

- [ ] **Step 7: Update the docs**

Append ADR-048 to `docs/design-log.md`:

```markdown
## ADR-048: Cycle Baseline, Transaction Sizes and the 10 % Regression Rule

**Context**
Spec §11: `CYCLES.md` records cycles per instruction against 32 Mi and transaction sizes; CI fails on a regression above 10 %. ADR-036 measured the chains once.

**Options**
- (A) Assert hard-coded numbers in tests.
- (B) A committed baseline compared by a script, with the table generated from the same data.
- (C) Real-proof cycles as the baseline.

**Decision**
(B). `chains.rs` records every measured instruction (`CYCLES_OUT`, executor cycles and borsh size of the signed transaction); `scripts/check-cycles.py` fails on growth above 10 % (exactly 10 % passes), on a label added or removed without a baseline change, on more than 10 executions and on any execution over 32 Mi; `CYCLES.md` is generated from the same file. The baseline is the local native build; CI measures its own native build. Executor cycles are deterministic for one ELF and input, and toolchain differences between native builds stay well inside 10 %. The P2 load test (6 live auctions, 18 interleaved bids) requires a bid's cost to stay within 5 % whatever the number of live auctions.

**Trade-offs**
Executor cycles, not proving time: proving is tiered separately (ADR-045). Refreshing the baseline is a reviewed change in the same commit as its cause.

**Status**
Accepted (Plan 3b, Task 6)

---
```

- `docs/architecture/ci.md`: add the cycle steps.
- `docs/architecture/programs.md`: carry-over `CYCLES.md` → `Done (Plan 3b Task 6; real-proof re-measure Task 7)`.

- [ ] **Step 8: Gates**

Run the lint gate and the format gate. Expected: clean.

- [ ] **Step 9: Commit (controller, GPG signing on Windows)**

```bash
git add programs/integration_tests/Cargo.toml Cargo.lock programs/integration_tests/tests/common/measure.rs \
  programs/integration_tests/tests/common/mod.rs programs/integration_tests/tests/load.rs \
  scripts/cycles.sh scripts/check-cycles.py scripts/tests/test_check_cycles.py cycles/baseline.json CYCLES.md \
  .github/workflows/core.yml docs/architecture/ci.md docs/architecture/programs.md docs/design-log.md
git commit -m "feat(bench): CYCLES.md, transaction sizes, P2 load test and the 10 % cycle gate (ADR-048)"
```

---

### Task 7: Real-proof CI tiers and the ADR-023 re-measure

This is Plan 3b scope item 2, spec §1.3 and §9 "Real proofs". The task builds exactly the tiers the Task 1 ruling selected; the text below gives the YAML for every branch. It also adds a report checker, so an out-of-memory kill fails with a clear message instead of an empty log, and it re-measures ADR-023's cost argument with real proofs.

**Precondition:** the ledger contains `Ruling: ADR-045 tiers PO2=<..> PUSH=<..> NIGHTLY=<..> PPE=<..>` (Task 1 Step 9).

**Files:**
- Create: `.github/workflows/real-proofs.yml`, `scripts/check-proof-report.py`, `scripts/tests/test_check_proof_report.py`
- Create (NIGHTLY = C only): `docs/evidence/real-proofs-2026-10.md`, `scripts/prove-suite.sh`
- Modify: `docs/architecture/real-proofs.md`, `docs/architecture/ci.md`, `CYCLES.md` (real-proof section), `docs/design-log.md` (finalise ADR-045; amend ADR-036)

**Interfaces:**
- Consumes: the ignored tests `prove_settle_debt`, `prove_liquidate`, `prove_bid`, `prove_bid_with_settlement`, `prove_ppe_observe_feed` (Task 1); `chains.rs` (`every_instruction_fits_the_chain_budget`, `debt_and_solvency_chains_fit_the_budget`, `implicit_burn_variants_fit_the_budget`) under `AUCTION_PROVE=1`; `PROOF_REPORT` lines (Task 1); `scripts/measure-proof.sh`.
- Produces:
  - Workflow jobs `real-proof-push` and `real-proof-nightly` (`real-proofs.yml`).
  - `scripts/check-proof-report.py REPORT [--expect label1,label2]`: exit 1 with a reason.
  - For NIGHTLY = C: `scripts/prove-suite.sh` and `docs/evidence/real-proofs-2026-10.md`.

- [ ] **Step 1: Write the report checker's failing tests**

Create `scripts/tests/test_check_proof_report.py`:

```python
"""Review Focus (resource exhaustion): a killed prover must fail loudly (Plan 3b Task 7)."""
import importlib.util
import json
import os
import pathlib
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "check_proof_report", pathlib.Path(__file__).resolve().parents[1] / "check-proof-report.py")
cpr = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cpr)


def write(lines):
    f = tempfile.NamedTemporaryFile("w", delete=False, suffix=".jsonl")
    for line in lines:
        f.write(json.dumps(line) + "\n")
    f.close()
    return f.name


def proof(label, verified=True):
    return {"label": label, "program": "auction_engine", "user_cycles": 1, "total_cycles": 2,
            "segments": 1, "seconds": 1.0, "seal_bytes": 10, "po2": 20, "verified": verified}


class ReportTest(unittest.TestCase):
    def test_missing_report_says_the_prover_did_not_finish(self):
        errors = cpr.check("/nonexistent/proofs.jsonl", ["liquidate"])
        self.assertIn("did not finish", errors[0])

    def test_empty_report_fails(self):
        errors = cpr.check(write([]), [])
        self.assertIn("empty", errors[0])

    def test_unverified_receipt_fails(self):
        errors = cpr.check(write([proof("liquidate", verified=False)]), ["liquidate"])
        self.assertIn("not verified", errors[0])

    def test_missing_expected_label_fails(self):
        errors = cpr.check(write([proof("liquidate")]), ["liquidate", "bid_with_settlement"])
        self.assertEqual(errors, ["bid_with_settlement: no proof in the report (killed or skipped?)"])

    def test_complete_report_passes(self):
        path = write([proof("liquidate"), proof("bid_with_settlement")])
        self.assertEqual(cpr.check(path, ["liquidate", "bid_with_settlement"]), [])
        os.unlink(path)


if __name__ == "__main__":
    unittest.main()
```

Run: `python3 -m unittest discover -s scripts/tests -p 'test_check_proof_report.py'`
Expected: ERROR (the script does not exist).

- [ ] **Step 2: Write the report checker**

Create `scripts/check-proof-report.py`:

```python
#!/usr/bin/env python3
"""Checks a real-proof report (PROOF_REPORT JSON lines, ADR-045).

usage: check-proof-report.py REPORT [--expect label1,label2]
Fails when the report is missing (the prover was killed: out of memory or the job timeout), empty, has an
unverified receipt, or lacks an expected label. Prints a per-label summary otherwise.
"""
import json
import os
import sys


def check(path, expect):
    if not os.path.exists(path):
        return [f"no proof report at {path}: the prover did not finish (killed for memory, or the job timed out); see the job log"]
    rows = [json.loads(line) for line in open(path, encoding="utf-8") if line.strip()]
    if not rows:
        return [f"{path} is empty: nothing was proved"]
    errors = [f"{r['label']}: {r['program']} receipt not verified" for r in rows if not r.get("verified")]
    labels = {r["label"] for r in rows}
    errors += [f"{label}: no proof in the report (killed or skipped?)" for label in expect if label not in labels]
    return errors


def main(argv):
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    expect = argv[argv.index("--expect") + 1].split(",") if "--expect" in argv else []
    errors = check(argv[1], expect)
    for e in errors:
        print("check-proof-report: " + e, file=sys.stderr)
    if errors:
        return 1
    rows = [json.loads(line) for line in open(argv[1], encoding="utf-8") if line.strip()]
    for label in sorted({r["label"] for r in rows}):
        rs = [r for r in rows if r["label"] == label]
        print(f"{label}: {len(rs)} executions, {sum(r['segments'] for r in rs)} segments, "
              f"{sum(r['seconds'] for r in rs):.0f} s, all receipts verified")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
```

Run: `python3 -m unittest discover -s scripts/tests -p 'test_check_proof_report.py' -v`
Expected: 5 tests OK.

- [ ] **Step 3: Write `real-proofs.yml` for the ruled tiers**

Create `.github/workflows/real-proofs.yml` from the blocks below. Use the header and the shared setup, then exactly one `real-proof-push` block (A, B, or none for C) and exactly one `real-proof-nightly` block (A1, A3 or C). Replace `<PO2>` with the ruled `PO2`.

Header and shared setup (YAML anchors are not supported in Actions, so the setup steps are repeated in each job):

```yaml
name: real-proofs
on:
  push:
    branches: [main]
    tags: ["v*", "rfp014-*"]
  schedule:
    - cron: "17 2 * * *"
  workflow_dispatch:
env:
  RISC0_DEV_MODE: "0"
  AUCTION_PROVE: "1"
  AUCTION_PROVE_PO2: "<PO2>"
jobs:
```

`real-proof-push`, tier A:

```yaml
  real-proof-push:
    if: github.event_name == 'push' && github.ref == 'refs/heads/main'
    runs-on: ubuntu-22.04
    timeout-minutes: 90
    steps:
      - uses: actions/checkout@v4
      - run: scripts/ci/free-disk.sh
      - run: sudo apt-get update && sudo apt-get install -y --no-install-recommends clang libclang-dev pkg-config libssl-dev
      - uses: dtolnay/rust-toolchain@1.94.0
      - uses: ./.github/actions/install-risc0
      - uses: Swatinem/rust-cache@v2
        with: { shared-key: programs }
      - run: RISC0_DEV_MODE=1 cargo test --locked -p engine_integration_tests --test real_proofs --no-run
      - name: prove the end-to-end path (liquidate, final bid) and verify every receipt
        run: PROOF_REPORT="$RUNNER_TEMP/proofs.jsonl" cargo test --locked -p engine_integration_tests --test real_proofs -- --ignored --exact prove_liquidate prove_bid_with_settlement --test-threads 1 --nocapture
      - if: always()
        run: python3 scripts/check-proof-report.py "$RUNNER_TEMP/proofs.jsonl" --expect liquidate,bid_with_settlement
      - uses: actions/upload-artifact@v4
        if: always()
        with: { name: real-proofs-push, path: "${{ runner.temp }}/proofs.jsonl", if-no-files-found: warn }
```

`real-proof-push`, tier B: identical to tier A except the two proving steps:

```yaml
      - name: prove the cheapest instruction and verify its receipt
        run: PROOF_REPORT="$RUNNER_TEMP/proofs.jsonl" cargo test --locked -p engine_integration_tests --test real_proofs -- --ignored --exact prove_settle_debt --test-threads 1 --nocapture
      - if: always()
        run: python3 scripts/check-proof-report.py "$RUNNER_TEMP/proofs.jsonl" --expect settle_debt
```

`real-proof-push`, tier C: omit the job. The per-push real-proof requirement is `localnet-e2e` with `RISC0_DEV_MODE=0` (Task 14), as ADR-045 records.

`real-proof-nightly`, tier A1:

```yaml
  real-proof-nightly:
    if: github.event_name != 'push' || startsWith(github.ref, 'refs/tags/')
    runs-on: ubuntu-22.04
    timeout-minutes: 350
    steps:
      - uses: actions/checkout@v4
      - run: scripts/ci/free-disk.sh
      - run: sudo apt-get update && sudo apt-get install -y --no-install-recommends clang libclang-dev pkg-config libssl-dev
      - uses: dtolnay/rust-toolchain@1.94.0
      - uses: ./.github/actions/install-risc0
      - uses: Swatinem/rust-cache@v2
        with: { shared-key: programs }
      - run: RISC0_DEV_MODE=1 cargo test --locked -p engine_integration_tests --test chains --no-run
      - name: prove every instruction of the ADR-036 suite
        run: PROOF_REPORT="$RUNNER_TEMP/proofs.jsonl" cargo test --locked -p engine_integration_tests --test chains -- --test-threads 1 --nocapture
      - if: always()
        run: python3 scripts/check-proof-report.py "$RUNNER_TEMP/proofs.jsonl" --expect 'liquidate,take_debt,check_solvency(freeze)'
      - uses: actions/upload-artifact@v4
        if: always()
        with: { name: real-proofs-nightly, path: "${{ runner.temp }}/proofs.jsonl", if-no-files-found: warn }
```

`real-proof-nightly`, tier A3: as A1, plus

```yaml
    strategy:
      fail-fast: false
      matrix:
        test: [every_instruction_fits_the_chain_budget, debt_and_solvency_chains_fit_the_budget, implicit_burn_variants_fit_the_budget]
```

with the proving step's test filter set to `-- --exact ${{ matrix.test }} --test-threads 1 --nocapture`, the `--expect` label list dropped, and the artifact name `real-proofs-nightly-${{ matrix.test }}`.

`real-proof-nightly`, tier C: as A1, but with `runs-on: [self-hosted, linux, x64, risc0-prover]`, `if: (github.event_name != 'push' || startsWith(github.ref, 'refs/tags/')) && vars.REAL_PROOF_SELF_HOSTED == 'true'`, and no `free-disk.sh`/`apt-get` steps (the runner is provisioned per `docs/architecture/real-proofs.md`).

If `PPE_NIGHTLY = yes`, append a step to `real-proof-nightly` (the A1 or C job, or a new single job for A3):

```yaml
      - name: privacy circuit over observe_feed (ADR-034 path)
        run: cargo test --locked -p engine_integration_tests --test real_proofs -- --ignored --exact prove_ppe_observe_feed --nocapture
```

Run: `"$HOME/.local/bin/actionlint" .github/workflows/real-proofs.yml`
Expected: no output.

Before the first observed run, check that the test names inside `real-proofs.yml` all exist: `grep -o 'prove_[a-z_]*\|[a-z_]*_fits_the_[a-z_]*' .github/workflows/real-proofs.yml | sort -u` gives a list, and every entry must have a `fn` in `real_proofs.rs` or `chains.rs`.

- [ ] **Step 4: NIGHTLY = C only: record a local full run as evidence**

Create `scripts/prove-suite.sh`:

```bash
#!/usr/bin/env bash
# Proves the whole ADR-036 suite locally, one chains.rs test at a time (NIGHTLY tier C, ADR-045).
#   scripts/prove-suite.sh <po2> <timeout_seconds_per_test>
set -euo pipefail
po2="${1:?po2}"; limit="${2:?timeout per test}"
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
out="$HOME/lez-target/plan3b-proofs/suite-po2-$po2"
mkdir -p "$out"
rm -f "$out/proofs.jsonl"
for t in every_instruction_fits_the_chain_budget debt_and_solvency_chains_fit_the_budget implicit_burn_variants_fit_the_budget; do
  "$repo/scripts/disk-guard.sh"
  ( cd "$repo" && AUCTION_PROVE=1 RISC0_DEV_MODE=0 AUCTION_PROVE_PO2="$po2" PROOF_REPORT="$out/proofs.jsonl" \
      /usr/bin/time -v timeout "$limit" scripts/wsl-cargo.sh test --locked -p engine_integration_tests --test chains -- --exact "$t" --test-threads 1 --nocapture ) \
    > "$out/$t.log" 2>&1 || { echo "prove-suite: $t failed, see $out/$t.log" >&2; exit 1; }
done
python3 "$repo/scripts/check-proof-report.py" "$out/proofs.jsonl" --expect 'liquidate,take_debt,check_solvency(freeze)' | tee "$out/summary.txt"
```

Run it in the background: `scripts/prove-suite.sh <PO2> 21600` (timeout 7200000 per Bash call; split into one call per test if needed).
Expected: the summary lists every label with "all receipts verified". Create `docs/evidence/real-proofs-2026-10.md` with:
- the commit hash;
- `rustc`, `r0vm` and po2;
- the `summary.txt` content;
- per-test wall time and `Maximum resident set size` from each log;
- the sha256 of `proofs.jsonl`. The file itself is not committed: it stays in `$HOME/lez-target/plan3b-proofs/` and is too large, and the digests stand in for it.

Document the self-hosted runner in `docs/architecture/real-proofs.md` under "Optional self-hosted runner":
- requirements: Linux x64, at least 16 GiB RAM, at least 8 cores, at least 60 GB disk, rzup with the pinned components;
- registration: a repository runner with the labels `self-hosted,linux,x64,risc0-prover`;
- enabling: repository variable `REAL_PROOF_SELF_HOSTED=true`.

State that the job is disabled by default and is not required for CI to be green.

- [ ] **Step 5: Re-measure ADR-023 with real proofs**

Run (background): `scripts/measure-proof.sh prove_bid <PO2> 5400`; when `PO2 = none`, use po2 18 and say so in the amendment. From its `PROOF[bid]` lines, take the engine execution's `total_cycles` (`T_e`) and `segments`, and the total seconds of the three executions (`S_bid`).

Rule:
- If `T_e + 94,904 ≤ segments × 2^PO2` (the per-bid `Accounting` write of S6 fits the segments already paid for), the real-proof marginal cost is 0. ADR-023's cost argument becomes moot under proving, and the decision stays for the reason ADR-036 records: the final bid already writes `Accounting`. The per-bid update is recorded as an M1 review item.
- Otherwise the update adds one segment. Estimate its cost as `S_bid / total_segments` and compare it with 5 % of `S_bid`. ADR-023 stands if the cost is above 5 %, and becomes an M1 item if below.

No engine change is made in 3b: an engine change would refresh the ImageIDs again (ADR-047).

Append to ADR-036:

```markdown
**Amended (Plan 3b, Task 7)**
Real-proof re-measure (ADR-045, po2 <PO2>): non-final `bid` engine execution <T_e> total cycles in <segments> segment(s), <S_bid> s for the three executions; the S6 per-bid `Accounting` update (+94,904 cycles) <fits the paid segments: marginal proving cost 0 | adds one segment: about <x> % of the bid's proving time>. Outcome: <ADR-023 stands | ADR-023 stands; the per-bid update is an M1 review item>. No engine change in Plan 3b.
```

- [ ] **Step 6: Observe the CI runs (controller; per Open decision 1)**

- Tier A or B: after the commit, the controller runs `gh workflow run real-proofs.yml --ref feat/plan-3b` (the dispatch runs the nightly job) and, if `real-proof-push` exists, a temporary push trigger is not used: the push job is observed after the merge to `main`.
- Tier A1 or A3: record the hosted wall time and `check-proof-report` summary in `docs/architecture/real-proofs.md`.

Decision rule:
- If a hosted job is killed (exit 137, or the report checker says "did not finish"), lower `AUCTION_PROVE_PO2` by one and dispatch again, once.
- If it still fails, move that tier one step down (A → B → C, A1 → A3 → C), update ADR-045 and `real-proofs.yml` in the same task, and say so in the commit body.

- [ ] **Step 7: Finalise the docs**

- In ADR-045, change **Status** to `Accepted (Plan 3b, Task 1); finalised in Task 7 (CI tiers as built: PUSH <..>, NIGHTLY <..>, first hosted timings in docs/architecture/real-proofs.md)`.
- In `CYCLES.md`, under "Real proofs", add the per-instruction proving table: from the CI nightly artifact (A1/A3) or the local evidence (C). Columns: label, executions, segments, seconds.
- `docs/architecture/ci.md`: add the `real-proofs.yml` jobs.

- [ ] **Step 8: Gates**

Run the lint gate and the format gate. Expected: clean.

- [ ] **Step 9: Commit (controller, GPG signing on Windows)**

```bash
git add .github/workflows/real-proofs.yml scripts/check-proof-report.py scripts/tests/test_check_proof_report.py \
  docs/architecture/real-proofs.md docs/architecture/ci.md CYCLES.md docs/design-log.md
# NIGHTLY = C only:
git add scripts/prove-suite.sh docs/evidence/real-proofs-2026-10.md
git commit -m "ci: real-proof tiers per ADR-045 and the ADR-023 re-measure with real proofs"
```

---
### Task 8: Coverage gate, at least 80 % lines per Rust crate (ADR-049)

This is Plan 3b scope item 6 and spec §9 ("≥ 80% coverage on the Rust crates"). `cargo-llvm-cov` measures our five crates. A small checker applies the threshold per crate rather than to the workspace total, so a well-tested crate cannot hide a weak one.

**Files:**
- Create: `scripts/check-coverage.py`, `scripts/tests/test_check_coverage.py`
- Modify: `.github/workflows/core.yml` (job `coverage`), `docs/architecture/ci.md`, `docs/design-log.md` (ADR-049)
- Modify (only as the decision rule in Step 4 requires): test modules of the crate(s) under 80 %

**Interfaces:**
- Consumes: the test suites of `auction_core`, `auction_engine_core`, `auction_engine_program`, `auction_sdk` (features `testing`, `rpc`) and `auction_cli`.
- Produces:
  - `scripts/check-coverage.py COV_JSON [--root DIR] [--threshold 80]`: exit 1 naming each crate under the threshold or without data.
  - Job `coverage` in `core.yml`.
  - The exclusion regex `IGNORE='(/tests/|_tests\.rs$|/testing/|/methods/|/examples/)'`, used identically locally and in CI.

- [ ] **Step 1: Write the checker's failing tests**

Create `scripts/tests/test_check_coverage.py`:

```python
"""The coverage gate is per crate (Plan 3b Task 8)."""
import importlib.util
import pathlib
import unittest

SPEC = importlib.util.spec_from_file_location(
    "check_coverage", pathlib.Path(__file__).resolve().parents[1] / "check-coverage.py")
cov = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cov)


def export(files):
    return {"data": [{"files": [
        {"filename": f"/repo/{name}", "summary": {"lines": {"count": count, "covered": covered}}}
        for name, count, covered in files]}]}


ALL_GOOD = [("crates/auction_core/src/math.rs", 100, 90),
            ("programs/auction_engine/core/src/state.rs", 100, 90),
            ("programs/auction_engine/src/bid.rs", 100, 90),
            ("crates/auction_sdk/src/quote.rs", 100, 90),
            ("cli/src/env.rs", 100, 90)]


class CoverageTest(unittest.TestCase):
    def test_every_crate_above_the_threshold_passes(self):
        self.assertEqual(cov.check(cov.per_crate(export(ALL_GOOD), "/repo")), [])

    def test_one_weak_crate_fails_even_when_the_total_passes(self):
        files = ALL_GOOD[:-1] + [("cli/src/env.rs", 100, 70)]
        self.assertEqual(cov.check(cov.per_crate(export(files), "/repo")),
                         ["auction_cli: 70.0 % of lines covered (< 80 %)"])

    def test_a_crate_without_data_fails(self):
        errors = cov.check(cov.per_crate(export(ALL_GOOD[1:]), "/repo"))
        self.assertEqual(errors, ["auction_core: no coverage data (not built, or every file ignored)"])

    def test_vendored_files_do_not_count(self):
        files = ALL_GOOD + [("programs/stablecoin/src/lib.rs", 1000, 0)]
        self.assertEqual(cov.check(cov.per_crate(export(files), "/repo")), [])


if __name__ == "__main__":
    unittest.main()
```

Run: `python3 -m unittest discover -s scripts/tests -p 'test_check_coverage.py'`
Expected: ERROR (no `check-coverage.py`).

- [ ] **Step 2: Write the checker**

Create `scripts/check-coverage.py`:

```python
#!/usr/bin/env python3
"""Per-crate line coverage gate (spec §9, ADR-049).

usage: check-coverage.py COV_JSON [--root DIR] [--threshold 80]
COV_JSON is `cargo llvm-cov ... --json --summary-only --output-path COV_JSON`. Every crate below must reach
the threshold on its own; files of other crates (vendored code, dependencies) are ignored.
"""
import json
import os
import sys

CRATES = {
    "auction_core": "crates/auction_core/",
    "auction_engine_core": "programs/auction_engine/core/",
    "auction_engine_program": "programs/auction_engine/src/",
    "auction_sdk": "crates/auction_sdk/",
    "auction_cli": "cli/",
}
THRESHOLD = 80


def per_crate(export, root):
    totals = {c: [0, 0] for c in CRATES}
    for f in export["data"][0]["files"]:
        rel = os.path.relpath(f["filename"], root).replace(os.sep, "/")
        for crate, prefix in CRATES.items():
            if rel.startswith(prefix):
                totals[crate][0] += f["summary"]["lines"]["count"]
                totals[crate][1] += f["summary"]["lines"]["covered"]
    return totals


def check(totals, threshold=THRESHOLD):
    errors = []
    for crate, (count, covered) in totals.items():
        if count == 0:
            errors.append(f"{crate}: no coverage data (not built, or every file ignored)")
            continue
        pct = 100 * covered / count
        if pct < threshold:
            errors.append(f"{crate}: {pct:.1f} % of lines covered (< {threshold} %)")
    return errors


def main(argv):
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    root = argv[argv.index("--root") + 1] if "--root" in argv else os.getcwd()
    threshold = float(argv[argv.index("--threshold") + 1]) if "--threshold" in argv else THRESHOLD
    totals = per_crate(json.load(open(argv[1], encoding="utf-8")), root)
    for crate, (count, covered) in totals.items():
        if count:
            print(f"{crate}: {100 * covered / count:.1f} % ({covered}/{count} lines)")
    errors = check(totals, threshold)
    for e in errors:
        print("check-coverage: " + e, file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
```

Run: `python3 -m unittest discover -s scripts/tests -p 'test_check_coverage.py' -v`
Expected: 4 tests OK.

- [ ] **Step 3: Measure locally**

Run inside WSL:

```bash
scripts/disk-guard.sh
rustup component add llvm-tools-preview --toolchain 1.94.0
scripts/wsl-env.sh cargo install --locked cargo-llvm-cov
```

Then, in the background (timeout 3600000):

```bash
IGNORE='(/tests/|_tests\.rs$|/testing/|/methods/|/examples/)'
scripts/wsl-env.sh cargo llvm-cov --locked -p auction_core -p auction_engine_core -p auction_engine_program -p auction_sdk -p auction_cli \
  --features auction_sdk/testing,auction_sdk/rpc --ignore-filename-regex "$IGNORE" \
  --json --summary-only --output-path /tmp/cov.json
python3 scripts/check-coverage.py /tmp/cov.json --root "$PWD"
scripts/disk-guard.sh
```

Expected: five `crate: NN.N %` lines and the checker's verdict.

- [ ] **Step 4: Apply the decision rule**

- **Every crate is at 80 % or more:** go to Step 5.
- **A crate is below 80 %:** list its least-covered files with `scripts/wsl-env.sh cargo llvm-cov report --ignore-filename-regex "$IGNORE" --summary-only | sort -k4 -n | head -20`. For each file, in order of uncovered lines, add tests to that file's existing test module, writing each test first and watching it fail where it pins a behaviour. Target error paths and branches the suites skip; do not write tests that only execute lines. Re-measure after each file. Stop when the crate reaches 80 %.
- **Lines no host test can reach:** some lines run only in another process, for example `cli/src/main.rs` (process wiring around `run_with`, exercised by the localnet smoke). Such a file may be added to the exclusion regex, and only if all of these hold:
  - (a) it holds no decision logic;
  - (b) a named runbook or test exercises it;
  - (c) ADR-049 lists it with the reason.

  Excluding a file only to pass the gate is not allowed.

Then delete the instrumented build: `scripts/wsl-env.sh cargo llvm-cov clean --workspace && scripts/disk-guard.sh`.

- [ ] **Step 5: Add the CI job**

Append to `.github/workflows/core.yml`:

```yaml
  coverage:
    runs-on: ubuntu-22.04
    timeout-minutes: 120
    env:
      RISC0_DEV_MODE: "1"
      IGNORE: '(/tests/|_tests\.rs$|/testing/|/methods/|/examples/)'
    steps:
      - uses: actions/checkout@v4
      - run: scripts/ci/free-disk.sh
      - run: sudo apt-get update && sudo apt-get install -y --no-install-recommends clang libclang-dev pkg-config libssl-dev
      - uses: dtolnay/rust-toolchain@1.94.0
        with: { components: llvm-tools-preview }
      - uses: ./.github/actions/install-risc0
      - uses: taiki-e/install-action@v2
        with: { tool: cargo-llvm-cov }
      - uses: Swatinem/rust-cache@v2
        with: { shared-key: coverage }
      - run: >-
          cargo llvm-cov --locked -p auction_core -p auction_engine_core -p auction_engine_program -p auction_sdk -p auction_cli
          --features auction_sdk/testing,auction_sdk/rpc --ignore-filename-regex "$IGNORE"
          --json --summary-only --output-path "$RUNNER_TEMP/cov.json"
      - run: python3 scripts/check-coverage.py "$RUNNER_TEMP/cov.json" --root "$GITHUB_WORKSPACE"
```

If Step 4 excluded a file, extend `IGNORE` identically in both places. Run: `"$HOME/.local/bin/actionlint" .github/workflows/core.yml`
Expected: no output.

- [ ] **Step 6: Update the docs**

Append ADR-049 to `docs/design-log.md`:

```markdown
## ADR-049: Coverage Gate: 80 % Lines per Crate, Our Five Crates, Tests Excluded

**Context**
Spec §9: TDD throughout and at least 80 % coverage on the Rust crates. A workspace total would let a well-tested crate hide a weak one, and counting test files inflates the figure.

**Options**
- (A) `--fail-under-lines 80` on the workspace total.
- (B) Per-crate threshold over production files only.
- (C) No gate, a report only.

**Decision**
(B). `cargo llvm-cov` over `auction_core`, `auction_engine_core`, `auction_engine_program`, `auction_sdk` (features `testing`, `rpc`) and `auction_cli`; files matching `(/tests/|_tests\.rs$|/testing/|/methods/|/examples/)` are not counted (test code, test fixtures, the guest wrappers that run only inside the zkVM, the localnet tool). `scripts/check-coverage.py` requires 80 % of lines per crate. Measured at Plan 3b Task 8: <the five percentages>. <Excluded production files and why, or "None.">

**Trade-offs**
Line coverage, not branch coverage. Guest code (`methods/guest`) runs in the zkVM and is covered by the real-guest integration suites, not by this gate.

**Status**
Accepted (Plan 3b, Task 8)

---
```

Add the `coverage` job to `docs/architecture/ci.md`.

- [ ] **Step 7: Gates**

Run the lint gate and the format gate, then `python3 -m unittest discover -s scripts/tests`. Expected: clean, and every checker test OK.

- [ ] **Step 8: Commit (controller, GPG signing on Windows)**

```bash
git add scripts/check-coverage.py scripts/tests/test_check_coverage.py .github/workflows/core.yml docs/architecture/ci.md docs/design-log.md
# plus every test file Step 4 changed
git commit -m "ci: per-crate 80 % line coverage gate (ADR-049)"
```

---

### Task 9: `cargo-fuzz` on the decoders, the keystore, the cursor, the deployment file and the bid quote

This is Plan 3b scope item 7 and spec §10. These targets read untrusted input: the node's account bytes, files on disk and the deployment JSON. The quote path takes arbitrary amounts, clocks and oracle prices. Each target must never panic, and the quote target also checks invariant 3 (`charged ≤ coin_in_max`, `out ≥ min_out`).

**Files:**
- Create: `fuzz/Cargo.toml`, `fuzz/Cargo.lock`, `fuzz/.gitignore`, `fuzz/fuzz_targets/{keystore_parse,deployment_json,cursor_json,decode_accounts,quote_bid}.rs`
- Create: `scripts/fuzz-smoke.sh`, `.github/workflows/fuzz.yml`
- Modify: `Cargo.toml` (`exclude` += `"fuzz"`), `docs/architecture/sdk.md` (fuzzing section), `docs/architecture/ci.md`

**Interfaces:**
- Consumes: `auction_sdk::keystore::Keystore::parse(&str, &Path)`, `auction_sdk::deployment::Deployment::from_json(&str, &str)`, `auction_sdk::scan::Cursor` (serde) with `auction_ids()` and `position_ids()`, `auction_sdk::decode::Reader::{new, config, coll_config, accounting, auction, surplus_auction, debt_auction, record_count}`, `auction_sdk::testing::{FixtureChain, standard}`, `auction_sdk::quote::quote_bid`, `auction_sdk::verify::verify_config`.
- Produces:
  - Fuzz targets `keystore_parse`, `deployment_json`, `cursor_json`, `decode_accounts` and `quote_bid`.
  - `scripts/fuzz-smoke.sh SECONDS` (runs every target).
  - Job `fuzz-nightly` (`fuzz.yml`).
- Exception to the Global Constraints: the fuzz targets cannot `#![forbid(unsafe_code)]`, because `fuzz_target!` expands to a `#[no_mangle] extern "C"` entry point.

- [ ] **Step 1: Install the tools**

Run inside WSL: `scripts/disk-guard.sh && rustup toolchain install nightly --profile minimal && scripts/wsl-env.sh cargo install --locked cargo-fuzz && cargo +nightly fuzz --version && scripts/disk-guard.sh`
Expected: a `cargo-fuzz` version line; the guard passes.

- [ ] **Step 2: Create the fuzz crate**

Add `"fuzz",` to `exclude` in the root `Cargo.toml`. Create `fuzz/Cargo.toml`:

```toml
[package]
name = "auction-fuzz"
version = "0.0.0"
publish = false
edition = "2021"

[package.metadata]
cargo-fuzz = true

# Its own workspace (cargo-fuzz builds with nightly and sanitizer flags).
[workspace]
members = ["."]

[dependencies]
libfuzzer-sys = "0.4"
arbitrary = { version = "1", features = ["derive"] }
serde_json = "1"
auction_sdk = { path = "../crates/auction_sdk", features = ["testing"] }
lee_core = { git = "https://github.com/logos-blockchain/logos-execution-zone.git", tag = "v0.2.4", features = ["host"] }

[[bin]]
name = "keystore_parse"
path = "fuzz_targets/keystore_parse.rs"
test = false
doc = false
bench = false

[[bin]]
name = "deployment_json"
path = "fuzz_targets/deployment_json.rs"
test = false
doc = false
bench = false

[[bin]]
name = "cursor_json"
path = "fuzz_targets/cursor_json.rs"
test = false
doc = false
bench = false

[[bin]]
name = "decode_accounts"
path = "fuzz_targets/decode_accounts.rs"
test = false
doc = false
bench = false

[[bin]]
name = "quote_bid"
path = "fuzz_targets/quote_bid.rs"
test = false
doc = false
bench = false
```

Create `fuzz/.gitignore` with `corpus/`, `artifacts/`, `coverage/` (one per line).

Create `fuzz/fuzz_targets/keystore_parse.rs`:

```rust
//! Any wallet storage text: `Keystore::parse` returns a typed error or a keystore, never panics.
#![no_main]
use std::path::Path;

use auction_sdk::keystore::Keystore;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        if let Err(e) = Keystore::parse(text, Path::new("fuzz-storage.json")) {
            let _ = e.to_string();
        }
    }
});
```

Create `fuzz/fuzz_targets/deployment_json.rs`:

```rust
//! Any deployment file: parsed or refused with a typed error, never a panic.
#![no_main]
use auction_sdk::deployment::Deployment;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = Deployment::from_json(text, "fuzz.json");
    }
});
```

Create `fuzz/fuzz_targets/cursor_json.rs`:

```rust
//! Any scanner cursor file content: loading and reading its id sets never panics.
#![no_main]
use auction_sdk::scan::Cursor;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(c) = serde_json::from_slice::<Cursor>(data) {
        let _ = c.auction_ids();
        let _ = c.position_ids();
    }
});
```

Create `fuzz/fuzz_targets/decode_accounts.rs`:

```rust
//! Any account bytes owned by the engine: every decoder returns a typed error or a state, never panics.
#![no_main]
use auction_sdk::{decode::Reader, testing::FixtureChain};
use lee_core::account::{Account, AccountId, Data};
use libfuzzer_sys::fuzz_target;

const ENGINE: [u32; 8] = [0xE1; 8];

fuzz_target!(|data: &[u8]| {
    let Ok(d) = Data::try_from(data.to_vec()) else {
        return;
    };
    let chain = FixtureChain::new();
    let id = AccountId::new([7; 32]);
    chain.put(
        id,
        Account {
            program_owner: ENGINE,
            data: d,
            ..Account::default()
        },
    );
    let r = Reader::new(&chain, ENGINE);
    let _ = r.config(&id);
    let _ = r.coll_config(&id);
    let _ = r.accounting(&id);
    let _ = r.auction(&id);
    let _ = r.surplus_auction(&id);
    let _ = r.debt_auction(&id);
    let _ = r.record_count(&id);
});
```

Create `fuzz/fuzz_targets/quote_bid.rs`:

```rust
//! The bid quote on the standard live auction with arbitrary amounts, clocks and oracle prices (valid,
//! band-clamped, out of range, zero): an error or a fill that respects invariant 3 (spec §5.13).
#![no_main]
use arbitrary::Arbitrary;
use auction_sdk::{quote::quote_bid, testing::standard, verify::verify_config};
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct Input {
    coin_in_max: u128,
    min_out: u128,
    clock_offset_ms: u32,
    /// Oracle price in 1/10_000 of R: 0 to about 6.5 R.
    price_steps: u16,
}

fuzz_target!(|i: Input| {
    let fx = standard::standard();
    let v = verify_config(&fx.chain, &fx.deployment, &standard::config_id()).expect("fixture verifies");
    let now = standard::T0 + u64::from(i.clock_offset_ms);
    fx.chain.set_clock(now);
    let price = standard::R / 10_000 * u128::from(i.price_steps);
    fx.chain.put_state(
        standard::ORACLE,
        standard::ORACLE_OWNER,
        &standard::oracle_state(price, now, &fx.host),
    );
    if let Ok(q) = quote_bid(&fx.chain, &v, &fx.live_auction, i.coin_in_max, i.min_out, now) {
        assert!(q.fill.charged <= i.coin_in_max, "charged {} > coin_in_max {}", q.fill.charged, i.coin_in_max);
        assert!(q.fill.collateral_out >= i.min_out, "out {} < min_out {}", q.fill.collateral_out, i.min_out);
        assert!(q.fill.collateral_out <= q.collateral_left, "out above the lot");
    }
});
```

Seed the lock file from the workspace so the fuzz build uses the same versions: `cp Cargo.lock fuzz/Cargo.lock`.

- [ ] **Step 3: Build and run every target for 60 s**

Create `scripts/fuzz-smoke.sh`:

```bash
#!/usr/bin/env bash
# Runs every cargo-fuzz target for SECONDS each (spec §10). Needs `rustup toolchain install nightly` and
# cargo-fuzz. Builds go to $CARGO_TARGET_DIR/fuzz (WSL: $HOME/lez-target/plan2/fuzz, deleted after use).
#   scripts/fuzz-smoke.sh [SECONDS=60]
set -euo pipefail
secs="${1:-60}"
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
tdir="${CARGO_TARGET_DIR:-$repo/target}/fuzz"
cd "$repo"
for t in $(cargo +nightly fuzz list); do
  echo "== fuzz $t for $secs s"
  cargo +nightly fuzz run --target-dir "$tdir" "$t" -- -max_total_time="$secs" -rss_limit_mb=4096
done
echo "fuzz-smoke: every target ran $secs s without a crash"
```

Make it executable. Run (background, timeout 3600000) inside WSL: `scripts/disk-guard.sh && scripts/wsl-env.sh scripts/fuzz-smoke.sh 60; scripts/disk-guard.sh`
Expected: `fuzz-smoke: every target ran 60 s without a crash`.

Decision rule on a crash (libFuzzer writes `fuzz/artifacts/<target>/crash-*`):
1. Reproduce: `cargo +nightly fuzz run --target-dir ... <target> fuzz/artifacts/<target>/crash-<hash>`.
2. Minimise: `cargo +nightly fuzz tmin ...`.
3. If the panic is in our crates, write a unit test with the minimised input in the owning crate's test module (red), fix (green), and re-run the target for 60 s.
4. If the panic is inside LEZ or borsh (vendored or dependency code), STOP and report it with the input. It is an upstream finding for Task 17's drafts, and the target needs a ruling.

When the run is clean, delete the build: `rm -rf $HOME/lez-target/plan2/fuzz` (the guard passes).

- [ ] **Step 4: Add the nightly job**

Create `.github/workflows/fuzz.yml`:

```yaml
name: fuzz
on:
  schedule:
    - cron: "33 1 * * *"
  workflow_dispatch:
jobs:
  fuzz-nightly:
    runs-on: ubuntu-22.04
    timeout-minutes: 90
    steps:
      - uses: actions/checkout@v4
      - run: scripts/ci/free-disk.sh
      - run: sudo apt-get update && sudo apt-get install -y --no-install-recommends clang libclang-dev pkg-config libssl-dev
      - uses: dtolnay/rust-toolchain@nightly
      - uses: taiki-e/install-action@v2
        with: { tool: cargo-fuzz }
      - run: scripts/fuzz-smoke.sh 300
      - uses: actions/upload-artifact@v4
        if: failure()
        with: { name: fuzz-crashes, path: fuzz/artifacts, if-no-files-found: ignore }
```

Run: `"$HOME/.local/bin/actionlint" .github/workflows/fuzz.yml`
Expected: no output.

- [ ] **Step 5: Update the docs**

Add a "Fuzzing" section to `docs/architecture/sdk.md`. It covers:
- the five targets and the property each checks;
- how to run them: `scripts/wsl-env.sh scripts/fuzz-smoke.sh 60`, or one target with `cargo +nightly fuzz run <t>`;
- the crash procedure from Step 3;
- the nightly job.

Add `fuzz-nightly` to `docs/architecture/ci.md`.

- [ ] **Step 6: Gates**

Run the lint gate and the format gate. The fuzz crate is outside the workspace; also run `scripts/wsl-env.sh cargo +nightly fmt --manifest-path fuzz/Cargo.toml -- --check`. Expected: clean.

- [ ] **Step 7: Commit (controller, GPG signing on Windows)**

```bash
git add Cargo.toml fuzz/Cargo.toml fuzz/Cargo.lock fuzz/.gitignore fuzz/fuzz_targets scripts/fuzz-smoke.sh \
  .github/workflows/fuzz.yml docs/architecture/sdk.md docs/architecture/ci.md
git commit -m "test(fuzz): cargo-fuzz targets for decoders, keystore, cursor, deployment file and bid quote"
```

---

### Task 10: The localnet price publisher and keeper, and `host status` (ADR-050)

This is Plan 3b scope item 8 and localnet finding 2. The localnet oracle is a twap_oracle `OraclePriceAccount` created with a RAY price; twap's own `PublishPrice` writes a Q64.64 TWAP of ticks. Rather than mix scales (spec §5.2), the publisher moves the price by rotation:
1. create a price account for the same source at the next free window, with the new price;
2. send the host's `SetMarketPriceOracle`;
3. send the engine's `set_collateral_config` with unchanged parameters and the new oracle (ADR-028: the engine must follow the host).

The keeper re-publishes before the oracle's age limit. The price is checked on the engine's own validity path before anything is signed.

**Files:**
- Create: `crates/auction_sdk/src/oracle.rs`; Modify: `crates/auction_sdk/src/lib.rs` (`pub mod oracle;`)
- Modify: `crates/auction_sdk/src/plan_host.rs` (`set_market_price_oracle` + test)
- Create: `cli/src/oracle_cmds.rs`; Modify: `cli/src/lib.rs` (`mod oracle_cmds;`), `cli/src/args.rs` (`DevCommand::{OraclePublish, Keeper}`, `OraclePublishCmd`, `HostCommand::Status`, `signs()`), `cli/src/dev_cmds.rs` (dispatch), `cli/src/host_cmds.rs` (`status`)
- Modify: `programs/integration_tests/tests/common/mod.rs` (`put_oracle_at`); Create: `programs/integration_tests/tests/publisher.rs`
- Modify: `cli/tests/admin_golden.rs`; Create: `cli/tests/golden/{dev_oracle_publish,host_status}.txt`
- Modify: `docs/design-log.md` (ADR-050), `docs/architecture/sdk.md`, `cli/README.md`, `docs/architecture/localnet.md`, `docs/architecture/programs.md`

**Interfaces:**
- Consumes:
  - `plan_host::create_oracle_price_account(twap, source, base, quote, price, window) -> (Plan, AccountId)`;
  - `plan::set_collateral_config(&VerifiedConfig, admin, oracle, &CollateralArgs)`;
  - `auction_engine_program::feed::{redemption_price, observe, require_live}`;
  - `Reader::{config, coll_config, oracle, meta}`;
  - `verify_chain_identity` and the dev-command guard (Task 3);
  - `Env::{verified, announce_admin, account, chain_now, send_plain}`;
  - `admin_cmds::{host_addresses, protocol_parameters, print_after}`.
- Produces:
  - `auction_sdk::plan_host::set_market_price_oracle(host: &HostAddresses, admin: AccountId, new_oracle: AccountId) -> SdkResult<Plan>` (stablecoin enum index 10; accounts `[admin, protocol_parameters, new_oracle]`; signer `admin`).
  - `auction_sdk::oracle::collateral_args_from_state(&CollateralConfigState) -> SdkResult<CollateralArgs>`.
  - `auction_sdk::oracle::check_new_price<C: Chain + ?Sized>(chain, v: &VerifiedConfig, new_oracle: AccountId, price_source: AccountId, price: u128, now_ms: u64) -> SdkResult<()>`.
  - `auction_sdk::oracle::free_oracle_window<C: Chain + ?Sized>(chain, twap: ProgramId, source: AccountId, base: u64) -> SdkResult<(u64, AccountId)>` and `pub const MAX_WINDOW_PROBES: u64 = 10_000`.
  - CLI `auction-cli dev oracle-publish --price-source <acct> --price <RAY> --host-admin <acct> --engine-admin <acct> [--base-window-ms 3600000]`.
  - CLI `auction-cli dev keeper <the same flags> [--interval-ms 60000] [--iterations N] [--refresh-before-ms 600000]`.
  - CLI `auction-cli host status` (read-only; keys `frozen`, `admin`, `freeze authority`, `market oracle`, `min collateral ratio`, `max oracle age ms`).
  - `World::put_oracle_at(at: AccountId, price: u128, timestamp: u64)`.

- [ ] **Step 1: Write the failing SDK tests**

Create `crates/auction_sdk/src/oracle.rs` with only its test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{id, standard::{self, k}};
    use crate::verify::verify_config;

    fn verified(fx: &standard::Fixture) -> VerifiedConfig {
        verify_config(&fx.chain, &fx.deployment, &standard::config_id()).expect("verifies")
    }

    #[test]
    fn check_new_price_refuses_a_price_outside_the_validity_range() {
        let fx = standard::standard();
        let v = verified(&fx);
        let new = AccountId::new([0x74; 32]);
        check_new_price(&fx.chain, &v, new, id(k::CALLER), standard::R * 19 / 10, standard::T0)
            .expect("1.9 R is inside [R/2, 2R]");
        let e = check_new_price(&fx.chain, &v, new, id(k::CALLER), standard::R * 3, standard::T0)
            .expect_err("3 R would pause the engine");
        assert!(format!("{e:?}").contains("PriceFeedOutOfRange"), "{e:?}");
        let e = check_new_price(&fx.chain, &v, new, id(k::CALLER), 0, standard::T0)
            .expect_err("a zero price is invalid");
        assert!(format!("{e:?}").contains("PriceFeed"), "{e:?}");
    }

    #[test]
    fn collateral_args_round_trip_the_stored_config() {
        let fx = standard::standard();
        let v = verified(&fx);
        let st = v.coll(&fx.chain).expect("coll config");
        let args = collateral_args_from_state(&st).expect("args");
        assert_eq!(args.schedule.len(), 2 * st.schedule.len());
        assert_eq!(args.penalty_bps, u32::from(st.penalty_bps));
        assert_eq!(args.band_hi_bps, u32::from(st.band_hi_bps));
        assert_eq!(args.type_cap, st.type_cap);
        assert_eq!(args.admin_paused, st.admin_paused);
        args.validate(v.state.global_cap).expect("the stored config is valid again");
    }

    #[test]
    fn free_oracle_window_skips_existing_price_accounts() {
        let fx = standard::standard();
        let source = id(k::CALLER);
        let taken = twap_oracle_core::compute_oracle_price_account_pda(standard::TWAP, source, 3_600_000);
        fx.chain.put_state(taken, standard::ORACLE_OWNER, &standard::oracle_state(standard::R, standard::T0, &fx.host));
        let (w, id_) = free_oracle_window(&fx.chain, standard::TWAP, source, 3_600_000).expect("free");
        assert_eq!(w, 3_600_001);
        assert_eq!(id_, twap_oracle_core::compute_oracle_price_account_pda(standard::TWAP, source, 3_600_001));
    }
}
```

Add `pub mod oracle;` to `crates/auction_sdk/src/lib.rs`. In `crates/auction_sdk/src/plan_host.rs` `mod tests`, add:

```rust
    #[test]
    fn set_market_price_oracle_encodes_enum_index_10_with_three_accounts() {
        let host = HostAddresses::new(standard::HOST);
        let new_oracle = AccountId::new([0x75; 32]);
        let p = set_market_price_oracle(&host, id(k::HOST_ADMIN), new_oracle).expect("plan");
        assert_eq!(p.variant_index(), Some(10), "enum index, not IDL position 16");
        assert_eq!(p.account_ids, vec![id(k::HOST_ADMIN), host.protocol_parameters, new_oracle]);
        assert_eq!(p.signing_requirements, vec![id(k::HOST_ADMIN)]);
    }
```

(Add `use crate::addresses::HostAddresses;` and `use lee_core::account::AccountId;` to that test module if they are not in scope.)

Run: `scripts/wsl-cargo.sh test -p auction_sdk --features testing oracle set_market_price_oracle`
Expected: compile errors (`check_new_price`, `collateral_args_from_state`, `free_oracle_window`, `set_market_price_oracle` not found).

- [ ] **Step 2: Implement the SDK side**

Prepend to `crates/auction_sdk/src/oracle.rs`:

```rust
//! Localnet price publisher support (ADR-050). The localnet market oracle is a twap_oracle
//! `OraclePriceAccount` created at RAY scale; twap's own `PublishPrice` writes a Q64.64 TWAP of ticks
//! (the open oracle-scale question, spec §5.2). So the price moves by rotation: a new price account for
//! the same source at the next free window, then the host's `SetMarketPriceOracle`, then the engine's
//! `set_collateral_config` with unchanged parameters and the new oracle (ADR-028: the engine must
//! follow the host). Production uses a real publisher once the scale question is settled (M2).

use auction_engine_core::{args::CollateralArgs, state::CollateralConfigState};
use auction_engine_program::feed;
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::ProgramId,
};
use twap_oracle_core::OraclePriceAccount;

use crate::{
    chain::Chain,
    decode::Reader,
    error::{SdkError, SdkResult},
    verify::VerifiedConfig,
};

/// Window probes before `free_oracle_window` gives up (each probe is one account read).
pub const MAX_WINDOW_PROBES: u64 = 10_000;

/// The `set_collateral_config` arguments that reproduce `st` (the oracle is an account, passed apart).
pub fn collateral_args_from_state(st: &CollateralConfigState) -> SdkResult<CollateralArgs> {
    let mut schedule = Vec::with_capacity(st.schedule.len().saturating_mul(2));
    for p in &st.schedule {
        let elapsed = u32::try_from(p.elapsed_ms).map_err(|_| {
            SdkError::Encode(format!(
                "schedule point at {} ms does not fit the u32 argument",
                p.elapsed_ms
            ))
        })?;
        schedule.push(elapsed);
        schedule.push(u32::from(p.discount_bps));
    }
    Ok(CollateralArgs {
        oracle_scale: st.oracle_scale,
        max_oracle_age_ms: st.max_oracle_age_ms,
        max_conf_bps: u32::from(st.max_conf_bps),
        band_lo_bps: u32::from(st.band_lo_bps),
        band_hi_bps: u32::from(st.band_hi_bps),
        penalty_bps: u32::from(st.penalty_bps),
        tip: st.tip,
        chip_bps: u32::from(st.chip_bps),
        reward_cap: st.reward_cap,
        debt_floor: st.debt_floor,
        min_bid: st.min_bid,
        type_cap: st.type_cap,
        schedule,
        admin_paused: st.admin_paused,
    })
}

/// Runs the engine's own feed validation (`feed::observe` + `feed::require_live`, the `liquidate`/`bid`
/// path) on a hypothetical new oracle at `now_ms`, so a price the engine would reject (outside
/// `[R/2, 2R]`, zero) is refused before anything is signed. The band only clamps; it never refuses.
pub fn check_new_price<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    new_oracle: AccountId,
    price_source: AccountId,
    price: u128,
    now_ms: u64,
) -> SdkResult<()> {
    let r = Reader::new(chain, v.engine.program);
    let cfg = r.config(&v.config_id)?;
    let mut coll = r.coll_config(&v.coll_config_id())?;
    let current = r.oracle(&coll.oracle_id)?;
    let current_account = chain.account(&coll.oracle_id)?;
    let candidate = OraclePriceAccount {
        price,
        timestamp: now_ms,
        source_id: price_source,
        confidence_interval: 0,
        ..current
    };
    coll.oracle_id = new_oracle;
    let meta = AccountWithMetadata::new(
        Account {
            data: Data::from(&candidate),
            ..current_account
        },
        false,
        new_oracle,
    );
    let redemption =
        feed::redemption_price(&cfg, &r.meta(&v.host.redemption_price_state)?, now_ms)?;
    let obs = feed::observe(&coll, &meta, redemption, now_ms)?;
    feed::require_live(&coll, &obs)?;
    Ok(())
}

/// The first window `>= base` whose price account for `source` does not exist yet, and that account.
pub fn free_oracle_window<C: Chain + ?Sized>(
    chain: &C,
    twap: ProgramId,
    source: AccountId,
    base: u64,
) -> SdkResult<(u64, AccountId)> {
    for k in 0..MAX_WINDOW_PROBES {
        let window = base
            .checked_add(k)
            .ok_or_else(|| SdkError::Encode("oracle window overflows u64".into()))?;
        let id = twap_oracle_core::compute_oracle_price_account_pda(twap, source, window);
        if chain.account(&id)? == Account::default() {
            return Ok((window, id));
        }
    }
    Err(SdkError::Chain(format!(
        "no free oracle window in {base}..{base}+{MAX_WINDOW_PROBES} for this price source; use another --price-source"
    )))
}
```

Append to `crates/auction_sdk/src/plan_host.rs`, after `set_controller_gains`:

```rust
/// Host `SetMarketPriceOracle` (stablecoin enum index 10, IDL position 16): admin, protocol_parameters
/// (written), new_oracle (read). The host checks the new account's shape and base/quote pair only.
pub fn set_market_price_oracle(
    host: &HostAddresses,
    admin: AccountId,
    new_oracle: AccountId,
) -> SdkResult<Plan> {
    Plan::new(
        host.program,
        vec![admin, host.protocol_parameters, new_oracle],
        vec![admin],
        &stablecoin_core::Instruction::SetMarketPriceOracle,
    )
}
```

Run: `scripts/wsl-cargo.sh test -p auction_sdk --features testing`
Expected: all pass. If a `feed::*` signature differs from the calls above, mirror exactly how `quote::quote_bid` calls it; the call order is the same as there.

- [ ] **Step 3: Prove the rotation on the real guests (failing test first)**

Add to `impl World` in `programs/integration_tests/tests/common/mod.rs`, after `set_oracle_full`:

```rust
    /// A RAY-scale price account at `at` (the localnet publisher's new oracle; the twap guest is not
    /// part of the in-process World, so the account is force-inserted like `set_oracle`).
    pub fn put_oracle_at(&mut self, at: AccountId, price: u128, timestamp: u64) {
        self.state.force_insert_account(
            at,
            Account {
                program_owner: [9u32; 8],
                balance: 0,
                data: Data::from(&OraclePriceAccount {
                    base_asset: stable_def(),
                    quote_asset: collateral_def(),
                    price,
                    timestamp,
                    source_id: at,
                    confidence_interval: 0,
                }),
                nonce: Nonce(0),
            },
        );
    }
```

Create `programs/integration_tests/tests/publisher.rs`:

```rust
//! ADR-050: the localnet publisher's rotation on the real guests (host `SetMarketPriceOracle`, then the
//! engine's `set_collateral_config` from `collateral_args_from_state`), exactly the plans the CLI sends.

mod common;

use auction_engine_core::{state::CollateralConfigState, Instruction as Ix};
use auction_sdk::{addresses::HostAddresses, oracle::collateral_args_from_state, plan, plan_host};
use common::*;
use lee_core::account::AccountId;

fn with_new_oracle(accounts: Vec<AccountId>, new_oracle: AccountId) -> Vec<AccountId> {
    accounts
        .into_iter()
        .map(|a| if a == oracle() { new_oracle } else { a })
        .collect()
}

#[test]
fn rotating_the_oracle_mid_auction_keeps_bids_on_the_new_feed() {
    let (mut w, pos) = liquidated_world();
    w.bid(pos, 100_000, 0).expect("partial bid on the old feed");
    let new_oracle = AccountId::new([0x73; 32]);
    w.put_oracle_at(new_oracle, R * 11 / 10, w.now);
    let host = HostAddresses::new(host_program());
    let rotate = plan_host::set_market_price_oracle(&host, id(k::HOST_ADMIN), new_oracle).expect("plan");
    w.run_plan(&rotate, &[k::HOST_ADMIN]).expect("the host rotates");
    let v = w.sdk_config();
    let before: CollateralConfigState = w.read(coll_config());
    let args = collateral_args_from_state(&before).expect("args");
    let follow = plan::set_collateral_config(&v, id(k::ENGINE_ADMIN), new_oracle, &args).expect("plan");
    w.run_plan(&follow, &[k::ENGINE_ADMIN]).expect("the engine follows the host");
    let after: CollateralConfigState = w.read(coll_config());
    assert_eq!(after.oracle_id, new_oracle);
    assert_eq!(collateral_args_from_state(&after).expect("args"), args, "parameters unchanged");
    assert_eq!(after.type_reserved, before.type_reserved, "the live reservation is untouched");
    let old = w.bid_accounts(pos, 0, k::BIDDER_STABLE, k::BIDDER_COLL, false);
    assert_err(
        w.engine(old.clone(), &[k::BIDDER_STABLE], Ix::Bid { coin_in_max: 50_000, min_collateral_out: 0 }),
        "OracleMismatch",
    );
    w.engine(
        with_new_oracle(old, new_oracle),
        &[k::BIDDER_STABLE],
        Ix::Bid { coin_in_max: 50_000, min_collateral_out: 0 },
    )
    .expect("a bid on the new feed fills");
}
```

Run: `scripts/wsl-cargo.sh test -p engine_integration_tests --test publisher`
Expected: PASS once Steps 2 and 3 are in place. If the host refuses the rotation, its error names the check; the new account's base/quote must equal the bootstrap pair, which `put_oracle_at` uses. If the engine refuses, the `set_collateral_config` error is the authority: report it, because a validation the publisher must satisfy is missing from ADR-050.

- [ ] **Step 4: Write the failing CLI golden tests**

Append to `cli/tests/admin_golden.rs`:

```rust
fn publish_args(price: u128) -> Vec<String> {
    [
        "dev",
        "oracle-publish",
        "--price-source",
        "caller",
        "--price",
        &price.to_string(),
        "--host-admin",
        "host-admin",
        "--engine-admin",
        "engine-admin",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

fn run_strings(c: &Case, args: &[String]) -> String {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    c.run(&refs)
}

#[test]
fn dev_oracle_publish_creates_rotates_and_follows() {
    let c = localnet_case();
    let out = run_strings(&c, &publish_args(standard::R * 19 / 10));
    assert!(out.ends_with("exit: 0\n"), "{out}");
    assert_eq!(c.fx.chain.sent().len(), 3, "price account, host rotation, engine follow-up");
    check("dev_oracle_publish", &out);
}

#[test]
fn dev_oracle_publish_refuses_an_invalid_price_before_signing() {
    let c = localnet_case();
    let out = run_strings(&c, &publish_args(standard::R * 3));
    assert!(out.contains("PriceFeedOutOfRange"), "{out}");
    assert!(out.ends_with("exit: 1\n"), "{out}");
    assert!(c.fx.chain.sent().is_empty(), "nothing signed");
}

#[test]
fn dev_keeper_publishes_only_near_the_age_limit() {
    let c = localnet_case();
    let mut args = publish_args(standard::R);
    args[1] = "keeper".into();
    args.extend(["--iterations", "1", "--interval-ms", "0"].map(String::from));
    let out = run_strings(&c, &args);
    assert!(out.contains("fresh"), "{out}");
    assert!(c.fx.chain.sent().is_empty(), "a fresh oracle is left alone");
    args.extend(["--refresh-before-ms", "999999999"].map(String::from));
    let out = run_strings(&c, &args);
    assert!(out.contains("publishing"), "{out}");
    assert_eq!(c.fx.chain.sent().len(), 3);
}

#[test]
fn host_status_reads_the_protocol_parameters() {
    let c = Case::new();
    let out = c.run(&["host", "status"]);
    assert!(out.contains("frozen            no"), "{out}");
    check("host_status", &out);
}
```

Run: `scripts/wsl-cargo.sh test -p auction_cli --features auction_sdk/testing --test admin_golden`
Expected: compile errors or failures (unknown subcommands `oracle-publish`, `keeper`, `status`).

- [ ] **Step 5: Implement the CLI side**

In `cli/src/args.rs`:
- add to `DevCommand`:

```rust
    /// Move the localnet market price (ADR-050): new price account, host rotation, engine follow-up.
    OraclePublish(OraclePublishCmd),
    /// Re-publish the current price before the oracle reaches its age limit (localnet keeper).
    Keeper {
        #[command(flatten)]
        publish: OraclePublishCmd,
        #[arg(long, default_value_t = 60_000)]
        interval_ms: u64,
        #[arg(long)]
        iterations: Option<u32>,
        #[arg(long, default_value_t = 600_000)]
        refresh_before_ms: u64,
    },
```

- add the struct:

```rust
#[derive(Debug, Args)]
pub struct OraclePublishCmd {
    /// The price-source account (signs the new price account; one per localnet).
    #[arg(long)]
    pub price_source: String,
    /// Collateral per 1 stablecoin, RAY (1e27).
    #[arg(long)]
    pub price: u128,
    #[arg(long)]
    pub host_admin: String,
    #[arg(long)]
    pub engine_admin: String,
    /// First window tried; each publish takes the next free one (part of the account address).
    #[arg(long, default_value_t = 3_600_000)]
    pub base_window_ms: u64,
}
```

- add `HostCommand::Status` with doc `/// Read-only: the host's freeze flag, admins, oracle and ratio.`;
- in `Command::signs`, add before `_ => true`:

```rust
            Self::Host(HostCommand::Addresses | HostCommand::Status) => false,
            Self::Dev(d) => !crate::dev_cmds::is_read_only(d),
```

  and make `is_read_only` in `cli/src/dev_cmds.rs` `pub(crate)`. If an existing `deployment_note` test in `args.rs` expects a note for `host addresses` or `dev resolve`, change it: read-only commands print no note.
- `dev keeper` and `dev oracle-publish` sign with the price source more than once, so on a real localnet that account must be `wallet auth-transfer init`-ed (ADR-018). Say so in their `--help` text and in `cli/README.md`.

Create `cli/src/oracle_cmds.rs`:

```rust
//! `dev oracle-publish` and `dev keeper` (ADR-050): the localnet price publisher. The dev-command guard
//! (localnet + recorded chain, ADR-046) has already run in `dev_cmds::dev`.

use std::io::Write;
use std::time::Duration;

use auction_sdk::{chain::Chain, decode::Reader, error::SdkError, oracle, plan, plan_host};

use crate::{
    admin_cmds::{host_addresses, print_after, protocol_parameters},
    args::OraclePublishCmd,
    env::Env,
    error::CliError,
    output::kv,
};

const CHECK: &str = "host status";

pub fn publish<C: Chain>(
    env: &Env<C>,
    a: &OraclePublishCmd,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let v = env.verified()?;
    env.announce_admin(&v, out)?;
    let twap = env.deployment.twap_oracle_program.ok_or_else(|| {
        CliError::Usage("the deployment file has no twap_oracle_program_id".into())
    })?;
    let host = host_addresses(env);
    let params = protocol_parameters(env, &host)?;
    let host_admin = env.account(&a.host_admin)?;
    if host_admin != params.admin_account_id {
        return Err(CliError::Verify(SdkError::Mismatch {
            what: "host admin (refusing before signing: the signer is not the host admin)",
            expected: params.admin_account_id.to_string(),
            found: host_admin.to_string(),
        }));
    }
    let engine_admin = env.account(&a.engine_admin)?;
    if engine_admin != v.state.admin {
        return Err(CliError::Verify(SdkError::Mismatch {
            what: "engine admin (refusing before signing: the signer is not the config admin)",
            expected: v.state.admin.to_string(),
            found: engine_admin.to_string(),
        }));
    }
    let source = env.account(&a.price_source)?;
    let coll = v.coll(&env.chain)?;
    let old = Reader::new(&env.chain, v.engine.program).oracle(&coll.oracle_id)?;
    let now = env.chain_now()?;
    let (window, new_oracle) =
        oracle::free_oracle_window(&env.chain, twap, source, a.base_window_ms)?;
    oracle::check_new_price(&env.chain, &v, new_oracle, source, a.price, now).map_err(|e| {
        CliError::Usage(format!(
            "refusing before signing: price {} would make the feed invalid for the engine ({e:?})",
            a.price
        ))
    })?;
    kv(out, "price", &format!("{} -> {}", old.price, a.price))?;
    kv(out, "new oracle", &new_oracle.to_string())?;
    kv(out, "window ms", &window.to_string())?;
    let (create, created) = plan_host::create_oracle_price_account(
        twap,
        source,
        params.stablecoin_definition_id,
        params.collateral_definition_id,
        a.price,
        window,
    )?;
    if created != new_oracle {
        return Err(CliError::Usage("oracle address derivation disagrees".into()));
    }
    env.send_plain(&create, "create_oracle_price_account", CHECK, out)?;
    let rotate = plan_host::set_market_price_oracle(&host, host_admin, new_oracle)?;
    env.send_plain(&rotate, "set_market_price_oracle", CHECK, out)?;
    let args = oracle::collateral_args_from_state(&coll)?;
    let follow = plan::set_collateral_config(&v, engine_admin, new_oracle, &args)?;
    env.send_plain(&follow, "set_collateral_config", CHECK, out)?;
    print_after(
        out,
        "engine oracle",
        v.coll(&env.chain).map(|c| c.oracle_id.to_string()),
    )
}

pub fn keeper<C: Chain>(
    env: &Env<C>,
    a: &OraclePublishCmd,
    (interval_ms, iterations, refresh_before_ms): (u64, Option<u32>, u64),
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let mut round = 0u32;
    loop {
        round = round.saturating_add(1);
        let v = env.verified()?;
        let coll = v.coll(&env.chain)?;
        let ts = Reader::new(&env.chain, v.engine.program)
            .oracle(&coll.oracle_id)?
            .timestamp;
        let age = env.chain_now()?.saturating_sub(ts);
        if age.saturating_add(refresh_before_ms) >= coll.max_oracle_age_ms {
            kv(out, "keeper", &format!("round {round}: oracle age {age} ms, publishing"))?;
            publish(env, a, out)?;
        } else {
            kv(out, "keeper", &format!("round {round}: oracle age {age} ms, fresh"))?;
        }
        if iterations.is_some_and(|n| round >= n) {
            return Ok(());
        }
        (env.sleep)(Duration::from_millis(interval_ms));
    }
}
```

In `cli/src/dev_cmds.rs`, add the match arms:

```rust
        DevCommand::OraclePublish(a) => crate::oracle_cmds::publish(env, a, out),
        DevCommand::Keeper {
            publish,
            interval_ms,
            iterations,
            refresh_before_ms,
        } => crate::oracle_cmds::keeper(
            env,
            publish,
            (*interval_ms, *iterations, *refresh_before_ms),
            out,
        ),
```

In `cli/src/host_cmds.rs`:
- extend the `refuse_at_ms` exemption to `HostCommand::Addresses | HostCommand::Status`;
- add the arm `HostCommand::Status => status(env, out),`;
- add:

```rust
/// Read-only view of the host's `ProtocolParameters` (the freeze flag is the current state; the
/// engine's `host_frozen_by_engine` only means "has ever frozen", THREAT_MODEL runbook).
fn status<C: Chain>(env: &Env<C>, out: &mut dyn Write) -> Result<(), CliError> {
    let p = protocol_parameters(env, &host_addresses(env))?;
    kv(out, "frozen", yes_no(p.is_frozen))?;
    kv(out, "admin", &p.admin_account_id.to_string())?;
    kv(out, "freeze authority", &p.freeze_authority_account_id.to_string())?;
    kv(out, "market oracle", &p.market_price_oracle_id.to_string())?;
    kv(out, "min collateral ratio", &p.minimum_collateralization_ratio.to_string())?;
    kv(out, "max oracle age ms", &p.maximum_oracle_price_age_milliseconds.to_string())
}
```

Add `mod oracle_cmds;` to `cli/src/lib.rs`.

Run: `UPDATE_GOLDEN=1 scripts/wsl-cargo.sh test -p auction_cli --features auction_sdk/testing --test admin_golden dev_oracle_publish host_status && git status --short cli/tests/golden && scripts/wsl-cargo.sh test -p auction_cli --features auction_sdk/testing`
Expected: only `dev_oracle_publish.txt` and `host_status.txt` are new. Review `dev_oracle_publish.txt`: it has the `price ... -> ...`, `new oracle`, `window ms 3600000` lines, three `plan ...` blocks, and `after engine oracle <the old oracle>` (FixtureChain does not execute programs). All CLI tests pass.

- [ ] **Step 6: Update the docs**

Append ADR-050 to `docs/design-log.md`:

```markdown
## ADR-050: Localnet Price Publisher by Oracle Rotation

**Context**
Localnet finding 2: the localnet oracle has no publisher, so it goes stale after an hour, and without a price move there is no shortfall, hence no F4 debt auction and no freeze path on localnet. The oracle is a twap_oracle `OraclePriceAccount` created at RAY scale; twap's `PublishPrice` writes a Q64.64 TWAP of ticks (lez-programs #401), and the engine and host read one scale per instance (spec §5.2, `Ray` in the PoC).

**Options**
- (A) Use twap `PublishPrice` and switch the instance to `Q64x64` (the host reads 1e27; mixing scales is the open upstream question).
- (B) A test-only oracle program.
- (C) Rotation: a new RAY price account for the same source at the next free window, the host's `SetMarketPriceOracle`, then the engine's `set_collateral_config` with unchanged parameters and the new oracle.

**Decision**
(C), as `auction-cli dev oracle-publish` and `dev keeper` (localnet only, ADR-046). The price is checked on the engine's own feed path (`oracle::check_new_price`: `feed::observe` + `feed::require_live`) before anything is signed; both admins are checked before signing. The keeper re-publishes the current price when the oracle's age plus `--refresh-before-ms` reaches the configured maximum age. Proved on the real guests mid-auction (`rotating_the_oracle_mid_auction_keeps_bids_on_the_new_feed`).

**Trade-offs**
Three transactions per move. Between the host rotation and the engine follow-up, `liquidate` fails with `OracleMismatch` (ADR-028); a failure there leaves the two pointing at different oracles, and re-running `oracle-publish` fixes it. Each move uses a new account (accounts are never released; localnet only). Production needs a real publisher once the oracle scale is settled (M2).

**Status**
Accepted (Plan 3b, Task 10)

---
```

Other doc edits:
- `docs/architecture/sdk.md`: add an `oracle.rs` section and a `set_market_price_oracle` row to the host plans table.
- `cli/README.md`: document `dev oracle-publish`, `dev keeper` and `host status`, including the partial-failure note.
- `docs/architecture/localnet.md`, finding 2: append `Resolved for localnet by the publisher (ADR-050, Plan 3b Task 10).`
- `docs/architecture/programs.md`: the "oracle price publisher / keeper" carry-over becomes `Done (Plan 3b Task 10, ADR-050)`.

- [ ] **Step 7: Gates**

Run the lint gate and the format gate, and `scripts/wsl-cargo.sh test -p engine_integration_tests --test publisher --test sdk_plans`. Expected: clean and green.

- [ ] **Step 8: Commit (controller, GPG signing on Windows)**

```bash
git add crates/auction_sdk/src/oracle.rs crates/auction_sdk/src/lib.rs crates/auction_sdk/src/plan_host.rs \
  cli/src/oracle_cmds.rs cli/src/lib.rs cli/src/args.rs cli/src/dev_cmds.rs cli/src/host_cmds.rs \
  programs/integration_tests/tests/common/mod.rs programs/integration_tests/tests/publisher.rs \
  cli/tests/admin_golden.rs cli/tests/golden/dev_oracle_publish.txt cli/tests/golden/host_status.txt \
  docs/design-log.md docs/architecture/sdk.md cli/README.md docs/architecture/localnet.md docs/architecture/programs.md
git commit -m "feat(cli): localnet price publisher and keeper by oracle rotation, host status (ADR-050)"
```

---

### Task 11: The demo scenario on the real guests, with the demo parameters (ADR-051)

The one-command demo (Task 13) must reach a shortfall, a debt auction and the freeze path on localnet. That run has many steps and a 15-second block time, so the economic path is pinned first on the real in-process guests, with the exact parameter values the demo will load. The values live in one place (`auction_sdk::demo`), and a CLI test pins that the demo's JSON files carry them.

**Files:**
- Create: `crates/auction_sdk/src/demo.rs`; Modify: `crates/auction_sdk/src/lib.rs` (`pub mod demo;`)
- Create: `cli/params/demo-engine.json`, `cli/params/demo-collateral.json`; Modify: `cli/src/params.rs` (test)
- Create: `programs/integration_tests/tests/demo_scenario.rs`
- Modify: `docs/design-log.md` (ADR-051), `cli/README.md` (parameter files)

**Interfaces:**
- Consumes: `EngineParamsArgs::defaults_for_tests`, `CollateralArgs::defaults_for_tests`, `World::{new, create_protocol_token, init_engine_config, bind_engine, set_collateral_config, open_position, set_min_cr, liquidate, bidder, bid, bid_settle, start_surplus, take_surplus, set_oracle, settle_debt, start_debt, fund_stable, take_debt, set_engine_params, check_solvency, host_frozen, balance, read}`, `LIQ_COLLATERAL`, `LIQ_DEBT`, `LIQ_TARGET`, `R`, `ONE`.
- Produces:
  - `auction_sdk::demo::{engine_params() -> EngineParamsArgs, collateral_args() -> CollateralArgs, PRICE_MOVE_NUM: u128 = 19, PRICE_MOVE_DEN: u128 = 10, DEBT_LOT: u128 = 5_000, FREEZE_THRESHOLD: u128 = 5_000, BAND_HI_BPS: u32 = 20_000}`.
  - `cli/params/demo-engine.json` and `cli/params/demo-collateral.json`.
  - Test `demo_scenario_runs_f1_to_f5_and_the_freeze_path`.

- [ ] **Step 1: Write the failing tests**

Create `programs/integration_tests/tests/demo_scenario.rs`:

```rust
//! ADR-051: the one-command demo's economic path on the real guests with the demo's parameters
//! (`auction_sdk::demo`). The localnet run (`scripts/demo.sh`, R = 1 RAY, debt 300_000) follows the
//! same steps at the same proportions; this test pins that each condition holds when the demo needs it.

mod common;

use auction_engine_core::{
    args::EngineParamsArgs,
    state::{AccountingState, AuctionState},
};
use auction_sdk::demo;
use common::*;

fn demo_world() -> World {
    let mut w = World::new();
    w.create_protocol_token(10_000, mint_authority())
        .expect("mock protocol token");
    w.init_engine_config(&demo::engine_params()).expect("init_config");
    w.bind_engine().expect("bind");
    w.set_collateral_config(k::ENGINE_ADMIN, &demo::collateral_args())
        .expect("collateral config");
    w.holding(k::TAKER_STABLE, stable_def(), 0);
    w
}

fn acc(w: &World) -> AccountingState {
    w.read(accounting())
}

#[test]
fn demo_scenario_runs_f1_to_f5_and_the_freeze_path() {
    let mut w = demo_world();
    // The demo's p0 and p2, opened while safe, made unsafe together by the min-ratio raise.
    let p0 = w.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 0, LIQ_COLLATERAL, LIQ_DEBT);
    let p2 = w.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 2, LIQ_COLLATERAL, LIQ_DEBT);
    w.set_min_cr(2 * ONE);

    // F1-F3 at the redemption price: partial bid, final bid, penalty into the buffer.
    w.liquidate(p0, 0).expect("liquidate p0");
    w.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 2_000_000);
    w.bid(p0, 100_000, 0).expect("partial bid");
    w.bid_settle(p0, LIQ_TARGET, 0).expect("final bid");
    let penalty = LIQ_TARGET - LIQ_DEBT;
    assert_eq!(w.balance(buffer()), penalty);

    // F5: part of the buffer sold for burned protocol tokens.
    w.start_surplus(0).expect("start surplus");
    w.take_surplus(0, 500, u128::MAX).expect("take surplus");
    let buffer_after_f5 = w.balance(buffer());
    assert_eq!(buffer_after_f5, penalty - 500);

    // The price moves to 1.9 R: valid (inside [R/2, 2R]), above the default band, inside the demo band.
    w.set_oracle(R * demo::PRICE_MOVE_NUM / demo::PRICE_MOVE_DEN, w.now);
    w.liquidate(p2, 0).expect("liquidate p2 at the moved price");
    w.bid_settle(p2, LIQ_TARGET, 0).expect("lot-binding bid");
    let a: AuctionState = w.read(auction(p2, 0));
    assert!(a.debt_raised < a.debt_principal, "the lot does not cover the debt at 1.9 R");
    let shortfall = a.debt_principal - a.debt_raised;
    assert_eq!(acc(&w).unbacked_debt, shortfall);

    // F4: settle against the buffer, then one debt auction lot mints against burned coin.
    w.settle_debt(u128::MAX).expect("settle_debt");
    assert_eq!(acc(&w).unbacked_debt, shortfall - buffer_after_f5);
    assert!(acc(&w).unbacked_debt >= demo::DEBT_LOT, "enough bad debt for one lot");
    w.start_debt(0).expect("start debt auction");
    w.fund_stable(k::TAKER_STABLE, demo::DEBT_LOT);
    let tokens_before = w.balance(id(k::TAKER_PROTOCOL));
    w.take_debt(0, k::TAKER_STABLE, k::TAKER_PROTOCOL, &[k::TAKER_STABLE], demo::DEBT_LOT, 0)
        .expect("take the whole lot");
    assert!(w.balance(id(k::TAKER_PROTOCOL)) > tokens_before, "protocol tokens minted");
    let remaining = shortfall - buffer_after_f5 - demo::DEBT_LOT;
    assert_eq!(acc(&w).unbacked_debt, remaining);
    assert!(remaining > demo::FREEZE_THRESHOLD, "still above the threshold after one lot");

    // Freeze path: without a protocol token, bad debt above the threshold freezes new debt.
    w.set_engine_params(
        k::ENGINE_ADMIN,
        &EngineParamsArgs {
            protocol_token_enabled: false,
            ..demo::engine_params()
        },
    )
    .expect("disable the protocol token");
    assert!(!w.host_frozen());
    w.check_solvency().expect("check_solvency");
    assert!(w.host_frozen(), "the host froze new debt");
    assert!(acc(&w).host_frozen_by_engine);
}
```

In `cli/src/params.rs` `mod tests`, add:

```rust
    #[test]
    fn the_demo_files_carry_the_pinned_scenario() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("params");
        let p = engine_params(&dir.join("demo-engine.json"), true, false).expect("engine params");
        assert_eq!(p, auction_sdk::demo::engine_params());
        p.validate().expect("valid");
        let c = collateral_args(&dir.join("demo-collateral.json")).expect("collateral args");
        assert_eq!(c, auction_sdk::demo::collateral_args());
        c.validate(p.global_cap).expect("valid");
    }
```

Run: `scripts/wsl-cargo.sh test -p engine_integration_tests --test demo_scenario` and `scripts/wsl-cargo.sh test -p auction_cli --features auction_sdk/testing the_demo_files`
Expected: compile error (`auction_sdk::demo` not found).

- [ ] **Step 2: Implement**

Create `crates/auction_sdk/src/demo.rs`:

```rust
//! The parameters of the one-command demo (`scripts/demo.sh`, ADR-051), shared by the CLI's
//! parameter-file test and the in-process scenario test (`demo_scenario.rs`), so the numbers the
//! demo relies on are pinned on the real guests.

use auction_engine_core::args::{CollateralArgs, EngineParamsArgs};

/// The demo moves the market price to `PRICE_MOVE_NUM / PRICE_MOVE_DEN` of the redemption price R:
/// 1.9 R is inside the validity range `[R/2, 2R]` (no pause) and makes the lot worth less than the debt.
pub const PRICE_MOVE_NUM: u128 = 19;
pub const PRICE_MOVE_DEN: u128 = 10;
/// The band must reach the moved price, otherwise the clamp at 1.1 R hides the move (spec §5.2 allows
/// `band_hi_bps` up to 20_000).
pub const BAND_HI_BPS: u32 = 20_000;
/// One debt-auction lot (stablecoin base units) and the freeze threshold (spec §5.12: threshold >= lot).
pub const DEBT_LOT: u128 = 5_000;
pub const FREEZE_THRESHOLD: u128 = 5_000;

/// Engine parameters with the protocol token enabled (the demo disables it for the freeze path).
#[must_use]
pub fn engine_params() -> EngineParamsArgs {
    EngineParamsArgs {
        protocol_token_enabled: true,
        debt_lot: DEBT_LOT,
        bad_debt_freeze_threshold: FREEZE_THRESHOLD,
        ..EngineParamsArgs::defaults_for_tests()
    }
}

#[must_use]
pub fn collateral_args() -> CollateralArgs {
    CollateralArgs {
        band_hi_bps: BAND_HI_BPS,
        ..CollateralArgs::defaults_for_tests()
    }
}
```

Add `pub mod demo;` to `crates/auction_sdk/src/lib.rs`.

Create `cli/params/demo-engine.json`: `cli/params/localnet-engine.json` with `"bad_debt_freeze_threshold": "5000"` and `"debt_lot": "5000"`, all other fields identical. Create `cli/params/demo-collateral.json`: `cli/params/localnet-collateral.json` with `"band_hi_bps": 20000`.

Run: `scripts/wsl-cargo.sh test -p engine_integration_tests --test demo_scenario -- --nocapture` and `scripts/wsl-cargo.sh test -p auction_cli --features auction_sdk/testing the_demo_files`
Expected: both PASS.

If `demo_scenario` fails, the failing assertion names the condition. Change the demo constants, not the assertions, and keep `FREEZE_THRESHOLD >= DEBT_LOT`. Record any change in ADR-051. Shared Reference R4 then has to be recomputed with the same formulas.

- [ ] **Step 3: Update the docs**

Append ADR-051 to `docs/design-log.md`:

```markdown
## ADR-051: Demo Scenario Parameters

**Context**
Spec §1.1: the one-command reproduction must drive a shortfall into the debt auction, a surplus into the surplus auction, and show the "no protocol token, so the host freezes new debt" path. With the default band (9000–11000 bps) the clamped price cannot leave the lot short of the debt, and a localnet run is too slow to tune by trial.

**Options**
- (A) Make positions underwater through the stability fee (slow, host-parameter dependent).
- (B) Move the market price inside the validity range with a wider band.
- (C) Inject state (not possible on a real node).

**Decision**
(B), pinned on the real guests by `demo_scenario_runs_f1_to_f5_and_the_freeze_path` with `auction_sdk::demo` (the same values as `cli/params/demo-*.json`, pinned by `the_demo_files_carry_the_pinned_scenario`). The defaults change in three places: `band_hi_bps = 20000` (the bound in spec §5.12), `debt_lot = 5000` and `bad_debt_freeze_threshold = 5000`. The price moves to 1.9 R (valid: R/2 ≤ 1.9 R ≤ 2R). A 500_000 / 300_000 position (R = 1) then raises at most about 257_850 coin from its lot, leaving a shortfall of at least about 42_150 whatever the discount at inclusion (2 % to 10 %). After the buffer (29_500) and one debt lot (5_000), at least about 7_650 of bad debt remains, above the threshold, so disabling the protocol token lets `check_solvency` freeze the host.

**Trade-offs**
The demo's band is wider than the PoC default; that is a parameter choice within spec bounds, stated in DEMO.md. The in-process scenario runs at R = 0.5 with twice the debt (the harness's standard position): same proportions, different absolute numbers.

**Status**
Accepted (Plan 3b, Task 11)

---
```

In `cli/README.md`, "Parameter files": list `demo-engine.json` and `demo-collateral.json` and what differs from the localnet files.

- [ ] **Step 4: Gates**

Run the lint gate and the format gate. Expected: clean.

- [ ] **Step 5: Commit (controller, GPG signing on Windows)**

```bash
git add crates/auction_sdk/src/demo.rs crates/auction_sdk/src/lib.rs cli/params/demo-engine.json cli/params/demo-collateral.json \
  cli/src/params.rs programs/integration_tests/tests/demo_scenario.rs docs/design-log.md cli/README.md
git commit -m "test(demo): pin the demo's shortfall, debt auction and freeze path on the real guests (ADR-051)"
```

---
### Task 12: Shared localnet library, LEZ bootstrap, demo preflight, and the smoke re-run (Plan 3a M7)

The demo (Task 13), the CI end-to-end job (Task 14) and the existing smoke share one bash library. This task extracts it from `scripts/localnet-smoke.sh` with no change in behaviour. It also adds the bootstrap a clean machine needs (the LEZ v0.2.4 sequencer and wallet), plus a preflight that stops a dirty machine before anything starts. Finally it re-runs the smoke end to end on the result, which closes Plan 3a M7: the hardened smoke script had not been re-run after its last fix round.

**Files:**
- Create: `scripts/lib/localnet.sh`, `scripts/bootstrap-lez.sh`, `scripts/tests/demo-preflight.sh`
- Create: `scripts/demo.sh` (preflight only in this task; Task 13 adds the run)
- Modify: `scripts/localnet-smoke.sh` (thin wrapper over the library; same step names)
- Modify: `.github/workflows/core.yml` (`core` job: run `scripts/tests/demo-preflight.sh`), `docs/architecture/localnet.md`

**Interfaces:**
- Consumes: `scripts/disk-guard.sh` (Task 1), `scripts/check-guests.sh --sha-only` (Task 5), `artifacts/guests/manifest.json`, `auction-cli dev chain-id` (Task 3).
- Produces:
  - `scripts/lib/localnet.sh`, sourced after setting `LN`.
    - Variables: `REPO`, `LEZ_DEPS` (default `$HOME/lez-spikes/s1` when its sequencer exists, else `$HOME/lez-deps`), `LEZ`, `SEQ_BIN`, `W`, `CLI_BIN`, `DEP`, `PORT=3040`, `ONE`, `GUESTS_DIR` (default `$REPO/artifacts/guests`).
    - Functions: `cli`, `field`, `run_field`, `id_of`, `remember`, `recall`, `set_deployment_field`, `ln_preflight`, `ln_build`, `ln_start_sequencer`, `ln_accounts`, `ln_auth <labels...>`, `ln_deploy`, `ln_host`, `ln_engine <params.json>`, `ln_bind`, `ln_collateral <params.json>`, `ln_holdings`, `ln_check_log`, `ln_peak`, `ln_stop_sequencer`, `unbacked`, `balance_of <account>`.
  - `ln_preflight` exit codes: 3 disk, 4 missing binary or tool, 5 port in use, 6 guest mismatch.
  - `scripts/bootstrap-lez.sh` (idempotent; `LEZ_DEPS`; exit 4 with the apt line when a tool is missing).
  - `scripts/demo.sh --preflight-only` prints `preflight ok`. Environment: `DEMO_RUN_DIR` (default `$HOME/lez-target/demo`), `LEZ_DEPS`, `GUESTS_DIR`.
  - `scripts/tests/demo-preflight.sh`, with cases `clean`, `low_disk`, `no_lez_binaries`, `port_in_use`, `guest_mismatch`.

- [ ] **Step 1: Write the failing preflight test**

Create `scripts/tests/demo-preflight.sh`:

```bash
#!/usr/bin/env bash
# Review Focus 1: scripts/demo.sh refuses a machine that is not clean, in preflight, with a named reason
# and a distinct exit code, before any sequencer starts. Stubs `df` and `ss` on PATH; fake LEZ binaries.
set -uo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fails=0
mkdir -p "$tmp/deps/lez/target/debug" "$tmp/bin" "$tmp/empty"
for b in sequencer_service wallet; do
  printf '#!/bin/sh\nexit 0\n' > "$tmp/deps/lez/target/debug/$b"
  chmod +x "$tmp/deps/lez/target/debug/$b"
done
stub() { printf '#!/bin/sh\n%s\n' "$2" > "$tmp/bin/$1"; chmod +x "$tmp/bin/$1"; }
run_case() {
  local name="$1" want="$2" text="$3" out got
  shift 3
  out="$(env "$@" "$repo/scripts/demo.sh" --preflight-only 2>&1)"
  got=$?
  if [[ "$got" != "$want" || "$out" != *"$text"* ]]; then
    echo "FAIL $name: exit $got (want $want), output: $out"
    fails=$((fails + 1))
  else
    echo "ok   $name"
  fi
}
stub df 'echo Avail; echo 100G'
stub ss 'exit 0'
base=(PATH="$tmp/bin:$PATH" LEZ_DEPS="$tmp/deps" DEMO_RUN_DIR="$tmp/run")
run_case clean 0 "preflight ok" "${base[@]}"
stub df 'echo Avail; echo 2G'
run_case low_disk 3 "STOP" "${base[@]}"
stub df 'echo Avail; echo 100G'
run_case no_lez_binaries 4 "bootstrap-lez.sh" PATH="$tmp/bin:$PATH" LEZ_DEPS="$tmp/empty" DEMO_RUN_DIR="$tmp/run"
stub ss 'echo "LISTEN 0 128 127.0.0.1:3040 0.0.0.0:* users:((\"sequencer_servi\",pid=4242,fd=9))"'
run_case port_in_use 5 "port 3040 is in use" "${base[@]}"
stub ss 'exit 0'
mkdir -p "$tmp/guests"
cp "$repo"/artifacts/guests/* "$tmp/guests/"
printf 'x' >> "$tmp/guests/auction_engine.bin"
run_case guest_mismatch 6 "do not match" "${base[@]}" GUESTS_DIR="$tmp/guests"
if (( fails == 0 )); then echo "demo-preflight: all cases pass"; else echo "demo-preflight: $fails case(s) failed"; exit 1; fi
```

Make it executable. Run inside WSL: `scripts/tests/demo-preflight.sh`
Expected: FAIL for every case (`scripts/demo.sh` does not exist yet; exit 127).

- [ ] **Step 2: Write the library**

Create `scripts/lib/localnet.sh`, moving the bodies of the smoke script's helpers and steps (Plan 3a Task 16, as amended in Tasks 3 and 5 of this plan) into functions:

```bash
#!/usr/bin/env bash
# Shared localnet helpers (Plan 3b Task 12) for scripts/localnet-smoke.sh, scripts/demo.sh and CI.
# Source it after setting LN (the run directory). Linux or WSL only. Requires the LEZ v0.2.4 sequencer
# and wallet (scripts/bootstrap-lez.sh), python3, ss, and the committed guests (artifacts/guests,
# ADR-047). Only the sequencer recorded in $LN/sequencer.pid is ever signalled.
LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
REPO="$(cd "$LIB_DIR/../.." && pwd -P)"
: "${LN:?set LN (the run directory) before sourcing scripts/lib/localnet.sh}"
if [[ -z "${LEZ_DEPS:-}" ]]; then
  if [[ -x "$HOME/lez-spikes/s1/lez/target/debug/sequencer_service" ]]; then LEZ_DEPS="$HOME/lez-spikes/s1"; else LEZ_DEPS="$HOME/lez-deps"; fi
fi
LEZ="$LEZ_DEPS/lez"
SEQ_BIN="$LEZ/target/debug/sequencer_service"
W="$LEZ/target/debug/wallet"
CLI_BIN="${CARGO_TARGET_DIR:-$REPO/target}/debug/auction-cli"
DEP="$LN/deployment.json"
PORT=3040
ONE=1000000000000000000000000000
GUESTS_DIR="${GUESTS_DIR:-$REPO/artifacts/guests}"
LABELS="${LABELS:-admin freeze oracle-src coll-def owner owner-coll owner-stable engine-admin keeper keeper-coll caller protocol-def taker-protocol taker-stable}"
export LEE_WALLET_HOME_DIR="$LN/wallet-home"
# shellcheck disable=SC1091
if [[ -f "$LEZ_DEPS/env.sh" ]]; then . "$LEZ_DEPS/env.sh"; fi

cli() { "$CLI_BIN" --deployment "$DEP" --state-dir "$LN/cli-state" --wallet-home "$LEE_WALLET_HOME_DIR" "$@"; }
# Last field of the first `key  value` line starting with $1; reads all input (no SIGPIPE under pipefail).
field() {
  awk -v k="$1" 'index($0, k) == 1 && !done { print $NF; done = 1 }
    END { if (!done) { print "error: key \"" k "\" not found in auction-cli output" > "/dev/stderr"; exit 1 } }'
}
# Runs auction-cli, shows its output on stderr, prints the value of key $1.
run_field() { local key="$1" out; shift; out="$(cli "$@")" || { printf '%s\n' "$out" >&2; return 1; }; printf '%s\n' "$out" >&2; printf '%s\n' "$out" | field "$key"; }
id_of() { cli dev resolve --label "$1" | field id; }
remember() { printf '%s=%s\n' "$1" "$2" >> "$LN/run.env"; }
recall() { sed -n "s/^$1=//p" "$LN/run.env" | tail -1; }
unbacked() { cli config | sed -n 's/^accounting *unbacked debt \([0-9]*\),.*/\1/p'; }
balance_of() { run_field balance dev balance --account "$1"; }

set_deployment_field() {
  python3 - "$DEP" "$1" "$2" <<'PY'
import json, sys
path, key, value = sys.argv[1:]
d = json.load(open(path, encoding="utf-8"))
d[key] = value
json.dump(d, open(path, "w", encoding="utf-8"), indent=2)
PY
}

ln_preflight() {
  "$REPO/scripts/disk-guard.sh" || exit 3
  local b t missing=""
  for b in "$SEQ_BIN" "$W"; do [[ -x "$b" ]] || missing+=" $b"; done
  if [[ -n "$missing" ]]; then
    echo "preflight: missing LEZ binaries:$missing; run scripts/bootstrap-lez.sh (LEZ_DEPS=$LEZ_DEPS)" >&2
    exit 4
  fi
  for t in python3 ss awk sha256sum; do
    command -v "$t" >/dev/null || { echo "preflight: missing tool $t" >&2; exit 4; }
  done
  local holder
  holder="$(ss -ltnp 2>/dev/null | grep ":$PORT " | sed -n 's/.*pid=\([0-9]*\).*/pid \1/p' | head -1 || true)"
  if ss -ltnp 2>/dev/null | grep -q ":$PORT "; then
    echo "preflight: port $PORT is in use (${holder:-unknown process}); stop it first (scripts/localnet-smoke.sh step_stop stops a sequencer this repository started)" >&2
    exit 5
  fi
  "$REPO/scripts/check-guests.sh" --sha-only "$GUESTS_DIR" \
    || { echo "preflight: the guests in $GUESTS_DIR do not match their manifest" >&2; exit 6; }
}

ln_build() {
  (cd "$REPO" && cargo build --locked -p auction_cli)
  local engine host tok twap
  read -r engine host tok twap < <(python3 - "$GUESTS_DIR/manifest.json" <<'PY'
import json, sys
g = json.load(open(sys.argv[1]))["guests"]
print(g["auction_engine"]["image_id"], g["stablecoin"]["image_id"], g["token"]["image_id"], g["twap_oracle"]["image_id"])
PY
)
  python3 - "$DEP" "$engine" "$host" "$tok" "$twap" <<'PY'
import json, sys
path, engine, host, tok, twap = sys.argv[1:]
json.dump({"network": "localnet", "sequencer_url": "http://127.0.0.1:3040", "engine_program_id": engine,
           "host_program_id": host, "token_program_id": tok, "twap_oracle_program_id": twap,
           "engine_admin": None, "config": None, "chain_genesis_hash": None},
          open(path, "w", encoding="utf-8"), indent=2)
PY
  echo "deployment file: $DEP"
}

ln_start_sequencer() {
  rm -rf "${LN:?}/seqhome" "${LN:?}/cli-state" && mkdir -p "$LN/seqhome"
  (cd "$LEZ" && RISC0_DEV_MODE="${SEQ_RISC0_DEV_MODE:-1}" RUST_LOG=info setsid nohup "$SEQ_BIN" \
    lez/sequencer/service/configs/debug/sequencer_config.json --home "$LN/seqhome" --listen-address 127.0.0.1 -p "$PORT" \
    > "$LN/logs/sequencer.log" 2>&1 < /dev/null &)
  rm -f "$LN/sequencer.pid"
  local pid="" i genesis=""
  for i in $(seq 1 30); do
    pid="$(ss -ltnp 2>/dev/null | sed -n "s/.*:$PORT .*pid=\([0-9]*\).*/\1/p" | head -1)"
    [[ -n "$pid" ]] && break
    sleep 2
  done
  [[ -n "$pid" ]] || { echo "sequencer did not open port $PORT within 60 s; see $LN/logs/sequencer.log" >&2; exit 1; }
  echo "$pid" > "$LN/sequencer.pid"
  # ADR-046: state-changing dev commands require the chain this deployment file was recorded on.
  for i in $(seq 1 15); do
    genesis="$(cli dev chain-id 2>/dev/null | awk '$1 == "genesis" { print $2 }')" && [[ -n "$genesis" ]] && break
    sleep 2
  done
  [[ -n "$genesis" ]] || { echo "could not read block 1 from the new sequencer" >&2; exit 1; }
  set_deployment_field chain_genesis_hash "$genesis"
}

ln_accounts() {
  # Fresh wallet for every run (duplicate labels would make id_of ambiguous). Throwaway localnet wallet.
  rm -rf "${LN:?}/wallet-home"
  mkdir -p "$LEE_WALLET_HOME_DIR" && cp "$LEZ/lez/wallet/configs/debug/wallet_config.json" "$LEE_WALLET_HOME_DIR/"
  local l
  for l in $LABELS; do echo pw | "$W" account new public --label "$l"; done
  : > "$LN/run.env"
  # New keys mean a new engine admin and config address: forget the previous run's (ln_engine records them).
  python3 - "$DEP" <<'PY'
import json, sys
d = json.load(open(sys.argv[1], encoding="utf-8"))
d["engine_admin"] = None
d["config"] = None
json.dump(d, open(sys.argv[1], "w", encoding="utf-8"), indent=2)
PY
}

ln_auth() {
  # Keys that sign more than one transaction must be auth-transfer-initialised before first use (ADR-018).
  local l
  for l in "$@"; do "$W" auth-transfer init --account-id "Public/$(id_of "$l")"; done
}

ln_deploy() {
  local p
  for p in token twap_oracle token_mint_authority stablecoin auction_engine; do "$W" deploy-program "$GUESTS_DIR/$p.bin"; done
}

ln_host() {
  local sdef oracle
  cli dev token-new --definition coll-def --holding owner-coll --name COLL --supply 100000000000
  sdef="$(run_field "stablecoin definition" host addresses)"
  oracle="$(run_field oracle dev oracle-create --price-source oracle-src --base "$sdef" --quote coll-def \
    --price "$ONE" --window-ms 3600000)"
  remember SDEF "$sdef"
  remember ORACLE "$oracle"
  cli host init-program --admin admin --freeze freeze --collateral-def coll-def --oracle "$oracle" \
    --min-cr 1500000000000000000000000000 --redemption-price "$ONE"
}

ln_engine() {
  local params="$1" mint config admin
  mint="$(run_field "mint authority" admin addresses --admin engine-admin --salt 1)"
  config="$(run_field config admin addresses --admin engine-admin --salt 1)"
  admin="$(id_of engine-admin)"
  # ADR-038: the protocol token must exist (mint authority = this config's MINT_AUTH PDA) before the config names it.
  cli dev token-new --definition protocol-def --holding taker-protocol --name MOCK --supply 10000 --mint-authority "$mint"
  cli admin init-config --admin engine-admin --salt 1 --params "$params" --protocol-token protocol-def
  set_deployment_field config "$config"
  set_deployment_field engine_admin "$admin"
}

ln_bind() { cli host bind --admin admin; }
ln_collateral() { cli admin set-collateral-config --admin engine-admin --params "$1"; }

ln_holdings() {
  local sdef
  sdef="$(recall SDEF)"
  cli dev token-init-account --definition "$sdef" --account owner-stable
  cli dev token-init-account --definition coll-def --account keeper-coll
  cli dev token-init-account --definition "$sdef" --account taker-stable
}

ln_check_log() {
  [[ -f "$LN/logs/sequencer.log" ]] || { echo "sequencer log $LN/logs/sequencer.log not found" >&2; exit 1; }
  if grep -qE "failed execution check" "$LN/logs/sequencer.log"; then
    grep -E "failed execution check" "$LN/logs/sequencer.log" | tail -20
    echo "the sequencer log contains failed execution checks" >&2
    exit 1
  fi
  echo "no failed execution check"
}

ln_peak() {
  local pid
  pid="$(cat "$LN/sequencer.pid")"
  echo "sequencer peak RSS: $(sed -n 's/^VmHWM:[[:space:]]*//p' "/proc/$pid/status") (RISC0_DEV_MODE=${SEQ_RISC0_DEV_MODE:-1})"
}

ln_stop_sequencer() {
  local pid i
  pid="$(cat "$LN/sequencer.pid" 2>/dev/null || true)"
  if [[ ! "$pid" =~ ^[0-9]+$ ]]; then
    echo "no sequencer pid recorded in $LN/sequencer.pid; nothing stopped (check manually, do not pkill by name)" >&2
    return 1
  fi
  if ! tr '\000' ' ' < "/proc/$pid/cmdline" 2>/dev/null | grep -q "sequencer_service"; then
    echo "pid $pid is not a running sequencer_service; not signalling it" >&2
  else
    kill "$pid" 2>/dev/null || echo "pid $pid was not running" >&2
    for i in $(seq 1 30); do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
    if kill -0 "$pid" 2>/dev/null; then echo "sequencer pid $pid did not exit within 30 s" >&2; return 1; fi
  fi
  rm -f "$LN/sequencer.pid"
}
```

`ln_accounts` resets `engine_admin` and `config` to `null` exactly as Plan 3a's `step_accounts` did.

- [ ] **Step 3: Make the smoke script a thin wrapper**

Replace `scripts/localnet-smoke.sh` with:

```bash
#!/usr/bin/env bash
# Plan 3a localnet smoke (Plan 3b Task 12: on scripts/lib/localnet.sh). Run INSIDE WSL. Steps, in order
# (several may be given in one call):
#   step_build step_sequencer step_accounts step_auth step_deploy step_host step_engine step_bind
#   step_collateral step_holdings step_flow step_system step_check step_peak step_stop
# SEQ_RISC0_DEV_MODE=0 runs the sequencer with dev mode off (RFP S2). Deploys the committed guests
# (artifacts/guests, ADR-047); needs the LEZ sequencer and wallet (S1 builds or scripts/bootstrap-lez.sh).
set -euo pipefail
LN="${SMOKE_RUN_DIR:-$HOME/lez-target/plan3a-localnet}"
mkdir -p "$LN/logs"
# shellcheck disable=SC1091
. "$(dirname "${BASH_SOURCE[0]}")/lib/localnet.sh"

step_build() { "$REPO/scripts/check-guests.sh" --sha-only "$GUESTS_DIR"; ln_build; }
step_sequencer() {
  if (ss -ltn 2>/dev/null || true) | grep -q ":$PORT "; then
    echo "port $PORT is already in use; not starting a second sequencer" >&2
    exit 1
  fi
  ln_start_sequencer
}
step_accounts() { ln_accounts; }
step_auth() { ln_auth admin engine-admin keeper owner caller; }
step_deploy() { ln_deploy; }
step_host() { ln_host; }
step_engine() { ln_engine "$REPO/cli/params/localnet-engine.json"; }
step_bind() { ln_bind; }
step_collateral() { ln_collateral "$REPO/cli/params/localnet-collateral.json"; }
step_holdings() { ln_holdings; }
```

followed by the unchanged bodies of `step_flow`, `step_system` and `step_check`. In `step_check`, replace its log-scan tail with `ln_check_log`. Then:

```bash
step_peak() { ln_peak; }
step_stop() { ln_stop_sequencer; }

for step in "$@"; do
  case "$step" in
    step_*) "$step" ;;
    *) echo "unknown step: $step" >&2; exit 2 ;;
  esac
done
```

Run: `bash -n scripts/lib/localnet.sh scripts/localnet-smoke.sh && echo SYNTAX-OK`
Expected: `SYNTAX-OK`.

- [ ] **Step 4: Add the bootstrap and the demo's preflight entry point**

Create `scripts/bootstrap-lez.sh`:

```bash
#!/usr/bin/env bash
# Builds the LEZ v0.2.4 sequencer and wallet the localnet scripts need (demo.sh, localnet-smoke.sh, CI).
# Idempotent: skips what exists. Default location $HOME/lez-deps (override with LEZ_DEPS). The author's
# machine reuses the S1 builds (LEZ_DEPS=$HOME/lez-spikes/s1, never modified by this script).
set -euo pipefail
LEZ_DEPS="${LEZ_DEPS:-$HOME/lez-deps}"
LEZ_REV=47eba256479f6f785acbd138834340703cd03401
if [[ -x "$LEZ_DEPS/lez/target/debug/sequencer_service" && -x "$LEZ_DEPS/lez/target/debug/wallet" ]]; then
  echo "bootstrap-lez: sequencer_service and wallet already built under $LEZ_DEPS/lez/target/debug"
  exit 0
fi
need() {
  command -v "$1" >/dev/null || {
    echo "bootstrap-lez: missing $1. On Ubuntu: sudo apt-get install -y build-essential clang libclang-dev pkg-config libssl-dev libpcsclite-dev python3 git curl; Rust via rustup (rust-toolchain.toml pins 1.94.0)" >&2
    exit 4
  }
}
for t in git cargo clang pkg-config python3; do need "$t"; done
mkdir -p "$LEZ_DEPS/lez"
cd "$LEZ_DEPS/lez"
# init + fetch works in a directory that already holds a cached target/ (CI restores the binaries only).
if [[ ! -d .git ]]; then
  git init -q
  git remote add origin https://github.com/logos-blockchain/logos-execution-zone
fi
git fetch -q --depth 1 origin "$LEZ_REV"
git checkout -q FETCH_HEAD
test "$(git rev-parse HEAD)" = "$LEZ_REV"
# shellcheck disable=SC1091
if [[ -f "$LEZ_DEPS/env.sh" ]]; then . "$LEZ_DEPS/env.sh"; fi
[[ -x target/debug/sequencer_service ]] || env -u CARGO_TARGET_DIR cargo build --locked --features standalone -p sequencer_service
[[ -x target/debug/wallet ]] || env -u CARGO_TARGET_DIR cargo build --locked -p wallet
echo "bootstrap-lez: sequencer_service and wallet ready under $LEZ_DEPS/lez/target/debug"
```

Create `scripts/demo.sh` (Task 13 completes it):

```bash
#!/usr/bin/env bash
# One-command reproduction (spec §1.1, ADR-051). Inside Linux or WSL:
#   scripts/demo.sh [--sequencer-dev-mode 0|1] [--keep] [--preflight-only]
# The author's machine: scripts/wsl-env.sh scripts/demo.sh (shared target dir, S1 builds).
# A clean machine first runs scripts/bootstrap-lez.sh (DEMO.md).
set -euo pipefail
seq_mode=1; keep=0; preflight_only=0
while (( $# )); do
  case "$1" in
    --sequencer-dev-mode) seq_mode="${2:?0 or 1}"; shift ;;
    --keep) keep=1 ;;
    --preflight-only) preflight_only=1 ;;
    *) echo "usage: scripts/demo.sh [--sequencer-dev-mode 0|1] [--keep] [--preflight-only]" >&2; exit 2 ;;
  esac
  shift
done
LN="${DEMO_RUN_DIR:-$HOME/lez-target/demo}"
# shellcheck disable=SC1091
. "$(dirname "${BASH_SOURCE[0]}")/lib/localnet.sh"
ln_preflight
if (( preflight_only )); then echo "preflight ok"; exit 0; fi
echo "the demo run is added in Plan 3b Task 13" >&2
exit 2
```

Make all three new scripts executable (`chmod +x` and `git update-index --chmod=+x`).

Run: `scripts/tests/demo-preflight.sh`
Expected: `ok` for the five cases and `demo-preflight: all cases pass`.

- [ ] **Step 5: Re-run the smoke end to end (Plan 3a M7)**

Run (background, timeout 3600000) inside WSL: `scripts/wsl-env.sh scripts/localnet-smoke.sh step_build step_sequencer step_accounts step_auth step_deploy step_host step_engine step_bind step_collateral step_holdings step_flow step_system step_check step_stop 2>&1 | tee $HOME/lez-target/plan3a-localnet/logs/plan3b-task12.log`
Expected: every transaction `included`, `no failed execution check`, and the final balances of `docs/architecture/localnet.md`: owner-coll 99_998_999_950, owner-stable 70_000, keeper-coll 50, taker-stable 500, taker-protocol 9_800, buffer 29_500.

If a balance differs, that is a behaviour change in the refactor. Diff `scripts/localnet-smoke.sh` against `HEAD~` step by step, fix it, and re-run.

- [ ] **Step 6: CI and docs**

In `.github/workflows/core.yml`, job `core`, append `- run: scripts/tests/demo-preflight.sh`.

In `docs/architecture/localnet.md`:
- add "Plan 3b re-run (Task 12, M7)" with the date, the result and the balances;
- state that the smoke runs on `scripts/lib/localnet.sh` and the committed guests;
- describe `scripts/bootstrap-lez.sh`;
- give the preflight exit codes.

- [ ] **Step 7: Gates**

Run the lint gate and the format gate, and `bash -n` on every new script. Expected: clean.

- [ ] **Step 8: Commit (controller, GPG signing on Windows)**

```bash
git add scripts/lib/localnet.sh scripts/bootstrap-lez.sh scripts/demo.sh scripts/localnet-smoke.sh scripts/tests/demo-preflight.sh \
  .github/workflows/core.yml docs/architecture/localnet.md
git commit -m "refactor(localnet): shared localnet library, LEZ bootstrap, demo preflight; smoke re-run"
```

---

### Task 13: `scripts/demo.sh`, the one-command reproduction, and `DEMO.md`

This is Plan 3b scope item 9 and spec §1.1. On top of the Task 12 library, the demo runs the whole success-criterion flow in the order of Shared Reference R4. It checks each economic condition with the CLI's own read-back and writes a summary. It stops its sequencer at the end unless `--keep` is given.

**Files:**
- Modify: `scripts/demo.sh` (the run)
- Create: `DEMO.md`
- Modify: `docs/architecture/localnet.md` (demo run record), `README.md` ("Reproduce in one command")

**Interfaces:**
- Consumes: the Task 12 library; `auction-cli` commands `host addresses|open-position|generate-debt|set-min-cr|status`, `list-at-risk`, `quote-liquidation`, `liquidate`, `quote-bid`, `bid`, `claim-leftover`, `list-auctions`, `observe-feed`, `settle-debt`, `check-solvency`, `start-surplus`, `take-surplus`, `start-debt`, `take-debt`, `config`, `admin addresses|set-engine-params|verify`, `dev oracle-publish|token-transfer|balance`; `cli/params/demo-*.json` (Task 11).
- Produces:
  - `scripts/demo.sh` with exit 0 on success; on failure a non-zero exit with `demo FAILED` and the log paths.
  - `$DEMO_RUN_DIR/demo-summary.txt`.
  - `DEMO.md`.
  - The flags `--sequencer-dev-mode`, `--keep` and `--preflight-only`, which Task 14 uses.

- [ ] **Step 1: Write the run**

In `scripts/demo.sh`, replace the two lines after `if (( preflight_only )) ...` with:

```bash
export SEQ_RISC0_DEV_MODE="$seq_mode"
SUMMARY="$LN/demo-summary.txt"
phase() { printf '\n=== %s\n' "$*" | tee -a "$SUMMARY"; }
# expect NAME ACTUAL eq|gt|ge EXPECTED (integers of any size)
expect() {
  local name="$1" got="$2" op="$3" want="$4"
  if python3 -c 'import sys
a, op, b = int(sys.argv[1]), sys.argv[2], int(sys.argv[3])
sys.exit(0 if {"eq": a == b, "gt": a > b, "ge": a >= b}[op] else 1)' "$got" "$op" "$want"; then
    echo "check $name: $got $op $want: ok" | tee -a "$SUMMARY"
  else
    echo "check $name: $got is not $op $want" | tee -a "$SUMMARY" >&2
    exit 1
  fi
}
on_exit() {
  local s=$?
  if (( keep == 0 )) && [[ -f "$LN/sequencer.pid" ]]; then ln_stop_sequencer || true; fi
  if (( s != 0 )); then echo "demo FAILED (exit $s); logs in $LN/logs, summary in $SUMMARY" >&2; fi
}

rm -rf "${LN:?}"
mkdir -p "$LN/logs"
: > "$SUMMARY"
trap on_exit EXIT

phase "build auction-cli; committed guests (artifacts/guests, ADR-047)"
ln_build
phase "standalone LEZ v0.2.4 sequencer (RISC0_DEV_MODE=$seq_mode)"
ln_start_sequencer
phase "accounts, auth-transfer init, deploy token, twap_oracle, token_mint_authority, stablecoin (+hooks), auction_engine"
ln_accounts
# oracle-src signs twice (the first oracle, then the publisher's), so it is initialised too (ADR-018).
ln_auth admin engine-admin keeper owner caller oracle-src
ln_deploy
phase "host: collateral token, oracle at R = 1, initialize_program"
ln_host
phase "engine: mock protocol token (mint authority = engine PDA), init_config (demo parameters), bind, collateral config"
ln_engine "$REPO/cli/params/demo-engine.json"
ln_bind
ln_collateral "$REPO/cli/params/demo-collateral.json"
ln_holdings

phase "positions p0, p1, p2; min collateral ratio 1.5 -> 2.0 makes p0 and p2 liquidatable"
p0="$(run_field position host open-position --owner owner --owner-collateral owner-coll --nonce 0 --amount 500000)"
cli host generate-debt --owner owner --position "$p0" --owner-stable owner-stable --amount 300000
p1="$(run_field position host open-position --owner owner --owner-collateral owner-coll --nonce 1 --amount 1000000)"
cli host generate-debt --owner owner --position "$p1" --owner-stable owner-stable --amount 100000
p2="$(run_field position host open-position --owner owner --owner-collateral owner-coll --nonce 2 --amount 500000)"
cli host generate-debt --owner owner --position "$p2" --owner-stable owner-stable --amount 300000
cli host set-min-cr --admin admin --ratio 2000000000000000000000000000
cli list-at-risk | tee -a "$SUMMARY"

phase "F1-F3: liquidate p0 (keeper reward), partial bid, final bid, leftover claimed"
cli quote-liquidation --position "$p0" | tee -a "$SUMMARY"
a0="$(run_field auction liquidate --position "$p0" --keeper keeper --keeper-holding keeper-coll)"
cli quote-bid --auction "$a0" --coin-in-max 100000 | tee -a "$SUMMARY"
cli bid --auction "$a0" --coin-in-max 100000 --bidder-stable owner-stable --bidder-collateral owner-coll
cli bid --auction "$a0" --coin-in-max 230000 --bidder-stable owner-stable --bidder-collateral owner-coll
cli claim-leftover --owner owner --owner-holding owner-coll --auction "$a0"
buffer_id="$(run_field buffer admin addresses --admin engine-admin --salt 1)"
expect "buffer holds the 10 % penalty" "$(balance_of "$buffer_id")" eq 30000

phase "F5: surplus auction sells buffer coin for protocol tokens, which are burned"
cli observe-feed --caller caller
cli settle-debt --caller caller
cli check-solvency --caller caller
cli start-surplus --caller caller
cli take-surplus --payer taker-protocol --receiver taker-stable --amount 500
expect "buffer after the surplus lot" "$(balance_of "$buffer_id")" eq 29500

phase "price move: the localnet publisher sets the market price to 1.9 R (ADR-050)"
cli dev oracle-publish --price-source oracle-src --price 1900000000000000000000000000 --host-admin admin --engine-admin engine-admin
cli host status | tee -a "$SUMMARY"

phase "shortfall: liquidate p2 at 1.9 R; the lot-binding bid raises less than the debt"
a2="$(run_field auction liquidate --position "$p2" --keeper keeper --keeper-holding keeper-coll)"
cli bid --auction "$a2" --coin-in-max 330000 --bidder-stable owner-stable --bidder-collateral owner-coll
cli list-auctions --all | tee -a "$SUMMARY"
u1="$(unbacked)"
expect "unbacked debt after the shortfall" "$u1" gt 0

phase "F4: settle against the buffer, start a debt auction, take one lot (stablecoin burned, protocol tokens minted)"
cli settle-debt --caller caller
u2="$(unbacked)"
expect "the buffer settled 29_500 of the bad debt" "$u2" eq "$((u1 - 29500))"
expect "enough bad debt for one lot" "$u2" ge 5000
cli start-debt --caller caller
cli dev token-transfer --from owner-stable --to taker-stable --amount 5000
tokens_before="$(balance_of taker-protocol)"
cli take-debt --payer taker-stable --receiver taker-protocol --amount 5000
expect "protocol tokens minted to the taker" "$(balance_of taker-protocol)" gt "$tokens_before"
u3="$(unbacked)"
expect "one lot of bad debt burned" "$u3" eq "$((u2 - 5000))"
expect "bad debt still above the 5_000 freeze threshold" "$u3" gt 5000

phase "freeze path: without a protocol token, bad debt above the threshold freezes new debt"
cli admin set-engine-params --admin engine-admin --params "$REPO/cli/params/demo-engine.json" --disable-protocol-token
cli check-solvency --caller caller
frozen="$(run_field frozen host status)"
[[ "$frozen" == yes ]] || { echo "check host frozen: got $frozen" | tee -a "$SUMMARY" >&2; exit 1; }
echo "check host frozen: yes" | tee -a "$SUMMARY"

phase "final check"
cli admin verify | tee -a "$SUMMARY"
ln_check_log | tee -a "$SUMMARY"
echo "demo complete" | tee -a "$SUMMARY"
```

Run: `bash -n scripts/demo.sh && scripts/tests/demo-preflight.sh`
Expected: no syntax error; all preflight cases still pass.

- [ ] **Step 2: Run the demo on the dev machine**

Run (background, timeout 3600000) inside WSL: `scripts/wsl-env.sh scripts/demo.sh 2>&1 | tee $HOME/lez-target/demo-run.log; echo EXIT ${PIPESTATUS[0]}`
Expected: every `check ...: ok`, `check host frozen: yes`, `no failed execution check`, `demo complete`, `EXIT 0`.

Decision rule on a failed `check`:
- The in-process scenario (Task 11) pinned the conditions. A failing check means the localnet diverged from it, through block timing or a different redemption price. Read `demo-summary.txt` and the sequencer log.
- If `unbacked debt after the shortfall` is 0, the price move did not reach the auction. Confirm that `host status`'s `market oracle` and the engine config's oracle (`config`) are the new account, and that `bid` printed `bid_with_settlement`.
- Fix the script, not the thresholds. Thresholds change only through ADR-051 and its pinned test.

- [ ] **Step 3: Run it with a `RISC0_DEV_MODE=0` sequencer**

Run (background, timeout 3600000): `scripts/wsl-env.sh scripts/demo.sh --sequencer-dev-mode 0 2>&1 | tee $HOME/lez-target/demo-run-dev-mode-0.log; echo EXIT ${PIPESTATUS[0]}`
Expected: `demo complete`, `EXIT 0`. This is the local run of RFP S2's path that Task 14 runs in CI.

- [ ] **Step 4: Write `DEMO.md` and record the run**

Create `DEMO.md`:

```markdown
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

A run takes about <minutes from Step 2> minutes (blocks every 15 s). The summary is written to
`$HOME/lez-target/demo/demo-summary.txt`.

## What it shows (localnet values, R = 1)

<the R4 table of the plan, with the "Expected on chain" column replaced by the values of the recorded run,
copied from demo-summary.txt of Step 2: buffer 30_000 then 29_500, the unbacked debt after the shortfall,
after settlement and after the lot, the minted token balance, frozen yes>

Parameters: `cli/params/demo-engine.json`, `cli/params/demo-collateral.json` (the defaults except a
`band_hi_bps` of 20_000, a debt lot and freeze threshold of 5_000; ADR-051).

## Preflight exit codes

3 low disk; 4 LEZ binaries or tools missing (run `scripts/bootstrap-lez.sh`); 5 port 3040 in use; 6 a
committed guest does not match `artifacts/guests/manifest.json`.

## Recorded runs

| Date | Commit | Sequencer | Result |
|---|---|---|---|
| <date> | <commit> | RISC0_DEV_MODE=1 | demo complete (<wall time>) |
| <date> | <commit> | RISC0_DEV_MODE=0 | demo complete (<wall time>) |
```

Fill the `<...>` cells from the two logs of Steps 2 and 3. They are measured values: copy them, do not round them away.

In `docs/architecture/localnet.md`, add "Plan 3b demo run" with the two results and the transaction counts (`grep -c '^submitted' $HOME/lez-target/demo-run.log`). Remove "Not covered on localnet: F4 and the freeze path" and replace it with a pointer to `DEMO.md`. In `README.md`, add a "Reproduce in one command" section pointing to `DEMO.md`.

- [ ] **Step 5: Gates**

Run the lint gate and the format gate. Expected: clean.

- [ ] **Step 6: Commit (controller, GPG signing on Windows)**

```bash
git add scripts/demo.sh DEMO.md docs/architecture/localnet.md README.md
git commit -m "feat(demo): one-command reproduction through F4 and the freeze path (spec §1.1)"
```

---

### Task 14: CI localnet end-to-end, with `RISC0_DEV_MODE=0` on every push to `main` (RFP S2)

Spec §1.3 asks CI to run dev-mode sequencer end-to-end tests and one `RISC0_DEV_MODE=0` end-to-end job on every push to `main`. RFP S2 asks for end-to-end tests against a standalone sequencer with `RISC0_DEV_MODE=0`, included in CI. This task runs `scripts/demo.sh` on a hosted runner: with `RISC0_DEV_MODE=0` on every push to `main`, and with both modes nightly. Task 1's R1 already showed locally that this path runs.

**Files:**
- Create: `.github/workflows/e2e.yml`
- Modify: `docs/architecture/ci.md`, `docs/architecture/real-proofs.md` (S2 section links the job)

**Interfaces:**
- Consumes: `scripts/bootstrap-lez.sh`, `scripts/demo.sh --sequencer-dev-mode N` (Tasks 12 and 13), `./.github/actions/install-risc0`, `scripts/ci/free-disk.sh`.
- Produces: workflow `e2e`, job `localnet-e2e` (matrix `sequencer_dev_mode`), artifact `demo-<mode>` (summary and logs).

- [ ] **Step 1: Write the workflow**

Create `.github/workflows/e2e.yml`:

```yaml
name: e2e
on:
  push:
    branches: [main]
  schedule:
    - cron: "41 3 * * *"
  workflow_dispatch:
jobs:
  localnet-e2e:
    runs-on: ubuntu-22.04
    timeout-minutes: 240
    strategy:
      fail-fast: false
      matrix:
        # Every push to main: RISC0_DEV_MODE=0 (RFP S2). Nightly and on dispatch: both modes.
        sequencer_dev_mode: ${{ fromJSON(github.event_name == 'push' && '["0"]' || '["0","1"]') }}
    env:
      LEZ_DEPS: /home/runner/lez-deps
      DEMO_RUN_DIR: /home/runner/demo
    steps:
      - uses: actions/checkout@v4
      - run: scripts/ci/free-disk.sh
      - run: sudo apt-get update && sudo apt-get install -y --no-install-recommends build-essential clang libclang-dev pkg-config libssl-dev libpcsclite-dev
      - uses: dtolnay/rust-toolchain@1.94.0
      - uses: ./.github/actions/install-risc0
      - uses: Swatinem/rust-cache@v2
        with: { shared-key: e2e }
      - name: cache the LEZ v0.2.4 sequencer and wallet binaries
        uses: actions/cache@v4
        with:
          path: |
            /home/runner/lez-deps/lez/target/debug/sequencer_service
            /home/runner/lez-deps/lez/target/debug/wallet
          key: lez-47eba256-ubuntu-22.04-bin-v1
      - run: scripts/bootstrap-lez.sh
      - name: one-command reproduction (spec §1.1) against a standalone sequencer
        run: scripts/demo.sh --sequencer-dev-mode ${{ matrix.sequencer_dev_mode }}
      - uses: actions/upload-artifact@v4
        if: always()
        with:
          name: demo-${{ matrix.sequencer_dev_mode }}
          path: |
            /home/runner/demo/demo-summary.txt
            /home/runner/demo/logs
          if-no-files-found: warn
```

Run: `"$HOME/.local/bin/actionlint" .github/workflows/e2e.yml`
Expected: no output.

- [ ] **Step 2: Observe the run (controller; per Open decision 1)**

After the commit and push, the controller runs `gh workflow run e2e.yml --ref feat/plan-3b` and then `gh run watch --exit-status <id>`.
Expected: both matrix legs succeed; the `demo-0` artifact ends with `demo complete`.

Decision rules:
- **The cold bootstrap exceeds the job limit** (LEZ is a large build on 2 vCPUs): re-run once; the binary cache then holds the build.
- **It still exceeds the limit:** add a separate job `lez-bootstrap` (timeout 350) that only builds and caches, and make `localnet-e2e` depend on it with `needs: lez-bootstrap`.
- **The demo fails only in CI:** download the artifact. A `check` failure there is the same triage as in Task 13 Step 2; an infrastructure failure (port, disk) is fixed in the workflow.

Record the cold and warm wall times in `docs/architecture/ci.md`.

If Open decision 1 is declined, the local runs of Task 13 Steps 2 and 3 are the evidence until the first push to `main`.

- [ ] **Step 3: Update the docs**

- `docs/architecture/ci.md`: add the `e2e` row and the timings.
- `docs/architecture/real-proofs.md`: in "What real proof means here", item 1, add `CI: .github/workflows/e2e.yml, job localnet-e2e (RISC0_DEV_MODE=0 on every push to main; both modes nightly).`

- [ ] **Step 4: Commit (controller, GPG signing on Windows)**

```bash
git add .github/workflows/e2e.yml docs/architecture/ci.md docs/architecture/real-proofs.md
git commit -m "ci: localnet end-to-end with a RISC0_DEV_MODE=0 sequencer on every push to main (RFP S2)"
```

---
### Task 15: `REQUIREMENTS.md` and its checker (spec §1.4, §9 "Requirements", RFP S4)

This is Plan 3b scope item 5. Every RFP-014 hard requirement is mapped to named evidence (a test, a golden file, a script, a CI job or a document), a milestone, or an honest descope with its reason. A checker fails CI when a named piece of evidence does not exist or is not what it claims to be. It runs after the demo and the CI tasks, so the whole map can be checked against code that exists.

**Files:**
- Create: `REQUIREMENTS.md`, `scripts/check-requirements.py`, `scripts/tests/test_check_requirements.py`
- Modify: `.github/workflows/core.yml` (job `requirements`), `docs/architecture/ci.md`, `README.md` (link)

**Interfaces:**
- Consumes: the test names, golden files, scripts and workflow jobs that Plans 1–3b created.
- Produces:
  - `scripts/check-requirements.py REQUIREMENTS.md [--root DIR]`: exit 1 with `<ID>: <reason>` lines.
  - The `check(text: str, root: str) -> list[str]` function, used by the tests.
  - Job `requirements`.
  - Evidence token syntax for `REQUIREMENTS.md`:
    - `` `test_name` ``: a Rust function preceded by `#[test]` or `#[kani::proof]` attributes;
    - `` `test_name`[ignored] ``: the same, `#[ignore]`d on purpose;
    - `golden:NAME`: `cli/tests/golden/NAME.txt` exists and a CLI test names `"NAME"`;
    - `script:PATH`;
    - `ci:WORKFLOW_FILE/JOB`;
    - `doc:PATH`.
  - Statuses: `PoC`, `PoC (partial)`, `Plan 4`, `M1`…`M7`, `Descoped`.

- [ ] **Step 1: Write the checker's failing tests**

Create `scripts/tests/test_check_requirements.py`:

```python
"""Review Focus 5: the requirements checker must not be fooled (Plan 3b Task 15)."""
import importlib.util
import os
import pathlib
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "check_requirements", pathlib.Path(__file__).resolve().parents[1] / "check-requirements.py")
cr = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cr)

HEADER = "| ID | Requirement | Status | Evidence | Notes |\n|---|---|---|---|---|\n"


class CheckerTest(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.mkdtemp()
        self.write("crates/x/src/lib.rs", """
// fn only_in_a_comment() is not a test
#[test]
fn real_test() {}

#[test]
#[ignore = "real proof"]
fn slow_test() {}
""")
        self.write("cli/tests/golden/bid.txt", "exit: 0\n")
        self.write("cli/tests/golden.rs", 'check("bid", &out);\n')
        self.write(".github/workflows/core.yml", "jobs:\n  programs:\n    runs-on: x\n")
        self.write("README.md", "x")

    def write(self, rel, text):
        path = os.path.join(self.root, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8") as f:
            f.write(text)

    def errors(self, rows, required=("F1",)):
        return cr.check(HEADER + rows, self.root, required)

    def test_valid_rows_pass(self):
        rows = ("| F1 | x | PoC | `real_test`, `slow_test`[ignored], golden:bid, ci:core.yml/programs, doc:README.md | |\n")
        self.assertEqual(self.errors(rows), [])

    def test_name_only_in_a_comment_fails(self):
        self.assertEqual(self.errors("| F1 | x | PoC | `only_in_a_comment` | |\n"),
                         ["F1: no test `only_in_a_comment` (a #[test] fn of that name)"])

    def test_ignored_test_without_the_marker_fails(self):
        self.assertEqual(self.errors("| F1 | x | PoC | `slow_test` | |\n"),
                         ["F1: `slow_test` is #[ignore]d; write `slow_test`[ignored] and name the job that runs it"])

    def test_marker_on_a_running_test_fails(self):
        self.assertEqual(self.errors("| F1 | x | PoC | `real_test`[ignored] | |\n"),
                         ["F1: `real_test` is marked [ignored] but runs by default"])

    def test_renamed_golden_fails(self):
        self.assertEqual(self.errors("| F1 | x | PoC | golden:bid_old | |\n"),
                         ["F1: golden:bid_old has no cli/tests/golden/bid_old.txt"])

    def test_unknown_ci_job_fails(self):
        self.assertEqual(self.errors("| F1 | x | PoC | ci:core.yml/coverage | |\n"),
                         ["F1: ci:core.yml/coverage: no job coverage in .github/workflows/core.yml"])

    def test_poc_row_without_evidence_fails(self):
        self.assertEqual(self.errors("| F1 | x | PoC |  | |\n"), ["F1: status PoC needs evidence"])

    def test_milestone_row_needs_a_note(self):
        self.assertEqual(self.errors("| F1 | x | M4 |  |  |\n"), ["F1: status M4 needs a note (why, and where it is promised)"])

    def test_missing_required_id_fails(self):
        self.assertEqual(self.errors("| F1 | x | PoC | `real_test` | |\n", required=("F1", "F2")),
                         ["F2: missing from REQUIREMENTS.md"])

    def test_duplicate_id_fails(self):
        row = "| F1 | x | PoC | `real_test` | |\n"
        self.assertEqual(self.errors(row + row), ["F1: listed twice"])


if __name__ == "__main__":
    unittest.main()
```

Run: `python3 -m unittest discover -s scripts/tests -p 'test_check_requirements.py'`
Expected: ERROR (no `check-requirements.py`).

- [ ] **Step 2: Write the checker**

Create `scripts/check-requirements.py`:

```python
#!/usr/bin/env python3
"""REQUIREMENTS.md checker (spec §1.4, §9; RFP S4).

usage: check-requirements.py REQUIREMENTS.md [--root DIR]
Every required RFP-014 id must have one row. A PoC or PoC (partial) row needs evidence; every evidence
token must resolve:
  `name`            a Rust fn preceded by #[test] or #[kani::proof] attributes (not #[ignore]d)
  `name`[ignored]   the same, #[ignore]d on purpose (a named CI job runs it)
  golden:NAME       cli/tests/golden/NAME.txt exists and a CLI test names "NAME"
  script:PATH       an existing script
  ci:FILE/JOB       job JOB exists in .github/workflows/FILE
  doc:PATH          an existing file or directory
Plan 4, M1-M7 and Descoped rows need a note.
"""
import os
import re
import sys

REQUIRED = (
    [f"F{i}" for i in range(1, 8)] + [f"U{i}" for i in range(1, 7)] + [f"R{i}" for i in range(1, 7)]
    + [f"P{i}" for i in range(1, 4)] + [f"S{i}" for i in range(1, 6)] + [f"PRIV{i}" for i in range(1, 4)]
    + [f"PA{i}" for i in range(1, 5)] + [f"SOFT{i}" for i in range(1, 4)]
)
STATUSES = {"PoC", "PoC (partial)", "Plan 4", "Descoped"} | {f"M{i}" for i in range(1, 8)}
NEEDS_EVIDENCE = {"PoC", "PoC (partial)"}
TOKEN = re.compile(r"`([A-Za-z0-9_]+)`(\[ignored\])?|(golden|script|ci|doc):([^\s,|]+)")
SOURCE_DIRS = ("crates", "programs", "cli", "fuzz")


def rust_sources(root):
    texts = []
    for d in SOURCE_DIRS:
        for dirpath, dirnames, files in os.walk(os.path.join(root, d)):
            dirnames[:] = [x for x in dirnames if x != "target"]
            texts += [open(os.path.join(dirpath, f), encoding="utf-8", errors="replace").read()
                      for f in files if f.endswith(".rs")]
    return "\n".join(texts)


def find_test(sources, name):
    """None if absent, else True when #[ignore]d."""
    m = re.search(r"#\[(?:test|kani::proof)\]((?:[^{])*?)\bfn\s+" + re.escape(name) + r"\s*[(<]", sources)
    if not m:
        return None
    return "#[ignore" in m.group(1)


def resolve(token, root, sources):
    name, ignored, kind, value = token
    if name:
        state = find_test(sources, name)
        if state is None:
            return f"no test `{name}` (a #[test] fn of that name)"
        if state and not ignored:
            return f"`{name}` is #[ignore]d; write `{name}`[ignored] and name the job that runs it"
        if ignored and not state:
            return f"`{name}` is marked [ignored] but runs by default"
        return None
    path = os.path.join(root, value)
    if kind == "golden":
        if not os.path.exists(os.path.join(root, "cli/tests/golden", value + ".txt")):
            return f"golden:{value} has no cli/tests/golden/{value}.txt"
        tests = "".join(open(os.path.join(root, "cli/tests", f), encoding="utf-8").read()
                        for f in os.listdir(os.path.join(root, "cli/tests")) if f.endswith(".rs"))
        return None if f'"{value}"' in tests else f'golden:{value} is not named by any CLI test'
    if kind in ("script", "doc"):
        return None if os.path.exists(path) else f"{kind}:{value} does not exist"
    wf, _, job = value.partition("/")
    wf_path = os.path.join(root, ".github/workflows", wf)
    if not os.path.exists(wf_path):
        return f"ci:{value}: no workflow {wf}"
    if not re.search(rf"^  {re.escape(job)}:\s*$", open(wf_path, encoding="utf-8").read(), re.M):
        return f"ci:{value}: no job {job} in .github/workflows/{wf}"
    return None


def check(text, root, required=REQUIRED):
    errors, seen = [], set()
    sources = rust_sources(root)
    for line in text.splitlines():
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 5 or not re.fullmatch(r"[A-Z]+[0-9]+", cells[0]):
            continue
        rid, _, status, evidence, notes = cells[:5]
        if rid in seen:
            errors.append(f"{rid}: listed twice")
            continue
        seen.add(rid)
        if status not in STATUSES:
            errors.append(f"{rid}: unknown status {status!r}")
            continue
        tokens = TOKEN.findall(evidence)
        if status in NEEDS_EVIDENCE and not tokens:
            errors.append(f"{rid}: status {status} needs evidence")
        if status not in NEEDS_EVIDENCE and not notes:
            errors.append(f"{rid}: status {status} needs a note (why, and where it is promised)")
        for t in tokens:
            e = resolve(t, root, sources)
            if e:
                errors.append(f"{rid}: {e}")
    errors += [f"{rid}: missing from REQUIREMENTS.md" for rid in required if rid not in seen]
    return errors


def main(argv):
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    root = argv[argv.index("--root") + 1] if "--root" in argv else os.path.dirname(os.path.abspath(argv[1]))
    errors = check(open(argv[1], encoding="utf-8").read(), root)
    for e in errors:
        print("check-requirements: " + e, file=sys.stderr)
    if not errors:
        print("check-requirements: every requirement maps to existing evidence, a milestone or a descope")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
```

Run: `python3 -m unittest discover -s scripts/tests -p 'test_check_requirements.py' -v`
Expected: 10 tests OK.

- [ ] **Step 3: CI and docs (first, so `ci:core.yml/requirements` resolves)**

Append to `.github/workflows/core.yml`:

```yaml
  requirements:
    runs-on: ubuntu-22.04
    steps:
      - uses: actions/checkout@v4
      - run: python3 scripts/check-requirements.py REQUIREMENTS.md
      - run: python3 -m unittest discover -s scripts/tests -p 'test_check_requirements.py' -v
```

Run `actionlint` on `core.yml`; expected: no output. Add the `requirements` job to `docs/architecture/ci.md`, and link `REQUIREMENTS.md` from `README.md`.

- [ ] **Step 4: Write `REQUIREMENTS.md`**

Create `REQUIREMENTS.md`. The requirement texts are those of `docs/research/research-5-traceability.md`, shortened.

```markdown
# RFP-014 requirements: evidence, milestones and descopes

Every hard requirement of RFP-014 maps to evidence in this repository (PoC), a proposal milestone (M1–M7,
spec §15), the thin GUI of Plan 4, or an honest descope. `scripts/check-requirements.py` (CI job
`requirements`) fails if a named test, golden file, script, CI job or document does not exist, if an
`#[ignore]`d test is not marked `[ignored]`, or if a required id is missing. Status meanings: **PoC** shown
here; **PoC (partial)** shown here in part, the rest promised; **Plan 4** in this PoC's next plan (FFI,
module, thin Basecamp app); **M#** a proposal milestone; **Descoped** with the reason.

| ID | Requirement | Status | Evidence | Notes |
|---|---|---|---|---|
| F1 | Permissionless liquidation with a liquidator incentive | PoC | `liquidate_happy_path`, `keeper_reward_to_fresh_holding`, `underwater_reward_is_capped_at_the_seizure`, golden:liquidate | Reward carved from the penalty, capped (spec §5.9). |
| F2 | Increasing discount over time, configurable schedule, early termination at target | PoC | `final_bid_reaches_target_and_settles_once`, `schedule_change_affects_only_new_auctions`, `unsold_auction_holds_max_discount_and_keeps_its_reservation`, `discount_frozen_while_paused` | No expiry: an unsold auction holds the maximum discount (ADR-012). |
| F3 | Partial lots | PoC | `partial_bid_burns_debt_first_and_leaves_accounting_alone`, `lot_binds_charges_only_for_the_lot_and_books_the_shortfall`, `split_bid_neutrality` | |
| F4 | Debt auction for shortfalls (mock token, configurable token); buffer absorbs shortfalls; freeze new debt above the buffer without a token | PoC | `debt_auction_mints_against_burned_coin`, `mint_never_exceeds_the_absolute_cap`, `no_protocol_token_freezes_above_threshold_and_only_the_freeze_authority_unfreezes`, `shortfall_at_threshold_does_not_freeze`, `demo_scenario_runs_f1_to_f5_and_the_freeze_path`, script:scripts/demo.sh | Interpretation: the freeze triggers above an absolute admin-set threshold (default 1,000 LSC, at least one debt lot), not at the first unit above the buffer, so one unit of rounding cannot freeze the stablecoin (spec §5.11, ADR-017). Hardening M3. |
| F5 | Surplus auction for a protocol token, burned; buffer accumulates without a token | PoC | `surplus_auction_burns_protocol_tokens`, `take_surplus_max_amount_fills_lot`, `close_after_ttl_returns_the_remainder`, golden:take_surplus | Hardening M3. |
| F6 | Strict, forward-only, atomic state transitions | PoC | `shortfall_is_booked_once_and_a_settled_lot_binding_auction_rejects_bids`, `settle_empty_applies_once_and_blocks_further_bids`, `auction_invariants` | |
| F7 | Well-defined host interface; one configured instance per product | PoC | `host_admin_binds_engine_through_the_guest`, `instances_have_distinct_programs_and_state`, `rogue_config_with_the_same_salt_cannot_liquidate_the_bound_host`, doc:docs/architecture/hooks.md | Hooks offered upstream as a patch series (`patches/stablecoin`, PR draft in docs/outreach). |
| U1 | Built with SPEL; IDL generated | PoC | `every_engine_variant_has_a_sample_and_an_idl_entry_at_its_enum_index`, `stablecoin_idl_position_maps_to_enum_index`, ci:core.yml/idl-drift | spel-cli mis-encodes stablecoin positions 8–18; our SDK encodes enum indices (ADR-040); upstream issue draft in docs/outreach. |
| U2 | Logos mini-app GUI loadable in Basecamp | Plan 4 | | The thin app (Live auctions view, bid flow) is Plan 4; the full GUI per docs/design/ui is M5 (spec §7). |
| U3 | CLI covering the essential operations | PoC | golden:liquidate, golden:bid_partial, golden:bid_final, golden:take_debt, golden:list_auctions, doc:cli/README.md | |
| U4 | Undercollateralised positions with liquidator incentive; trigger liquidation | PoC (partial) | golden:list_at_risk, golden:quote_liquidation, `liquidation_quote_equals_the_auction_the_engine_opens` | CLI shows it; the GUI At-risk view is M5. |
| U5 | Active auctions with discount, raised vs target, time; bid previews | PoC (partial) | golden:quote_bid, golden:list_auctions_held_at_max, `bid_quote_at_inclusion_time_equals_the_on_chain_fill`, `stale_quote_is_protected_by_min_collateral_out` | CLI now; GUI Live view in Plan 4. |
| U6 | SDK for modules, with the deshield-interact-reshield private path | PoC (partial) | `liquidate_bid_settle_and_claim_via_sdk_plans`, `verify_config_rejects_every_mismatch`, doc:docs/architecture/sdk.md | Public SDK here; the private path is M4 (spec §8). |
| R1 | Non-reentrant operations, ordered transitions | PoC | `laundering_freeze_through_an_intermediate_program_is_rejected`, `engine_takes_no_program_id_argument`, `set_liquidation_engine_through_a_chained_call_is_rejected` | LEZ v0.2.4 has no reentrancy guard; R1 rests on our programs never calling back (THREAT_MODEL). |
| R2 | Time-weighted or confidence-adjusted prices; stale feeds pause that type | PoC | `liquidate_rejects_stale_feed_and_admin_pause`, `low_confidence_and_out_of_range_open_the_clock`, `bid_rejects_invalid_feed`, `derived_policy_bounded_when_publishes_observed` | Honesty note: the twap producer's Q64.64 scale is not yet reconciled with the host's 1e27 (lez-programs #401), so the PoC uses a RAY price account; every current producer writes confidence 0, so the confidence check never triggers today. Live TWAP is M2. |
| R3 | Concurrent, isolated auctions | PoC | `r3_a_bid_touches_only_its_own_auction`, `load_concurrent_auctions_with_interleaved_bids_stay_isolated` | |
| R4 | A failed feed pauses only that type | PoC | `stale_feed_on_a_does_not_pause_b`, `stale_feed_on_b_does_not_pause_a`, `two_type_pause_isolation` | The host has one collateral type per instance; shown with two host+engine instances (spec §5.4, ADR-035). |
| R5 | Other types and non-liquidation operations continue | PoC | `admin_pause_on_a_does_not_pause_b`, `admin_pause_on_b_does_not_pause_a`, `pause_isolation_offsets_open_pause_and_discount` | As R4. |
| R6 | Seized = in auction; shortfalls accounted; system balance | PoC | `collateral_invariants_hold_over_random_bids`, `system_invariants_burn_and_mint_totals`, `inv10_debt_clear_matches_seizure_over_full_chain` | |
| P1 | Liquidation in one block | PoC | `every_instruction_fits_the_chain_budget`, script:scripts/check-cycles.py, doc:CYCLES.md | One transaction, at most 5 executions, within 32 Mi cycles each. |
| P2 | Many concurrent auctions within compute limits | PoC | `load_concurrent_auctions_with_interleaved_bids_stay_isolated`, ci:core.yml/programs, doc:CYCLES.md | |
| P3 | Bid in one block | PoC | `debt_and_solvency_chains_fit_the_budget`, `prove_bid_with_settlement`[ignored], doc:CYCLES.md | `prove_bid_with_settlement` runs in `real-proofs.yml` per ADR-045. |
| S1 | Deployed and tested on LEZ devnet/testnet | M6 | | Blocked: the testnet is v0.3 and SPEL has no v0.3 support (spec §3.1); the PoC reproduces on a standalone localnet (DEMO.md). |
| S2 | End-to-end tests against a standalone sequencer with `RISC0_DEV_MODE=0`, in CI | PoC | ci:e2e.yml/localnet-e2e, script:scripts/demo.sh, `prove_liquidate`[ignored], doc:docs/architecture/real-proofs.md | LEZ v0.2.4 executes public transactions without proving them; real receipts per execution are produced separately (ADR-045). |
| S3 | CI green on the default branch | PoC | ci:core.yml/core, ci:core.yml/programs, doc:docs/architecture/ci.md | |
| S4 | Every hard requirement has a test | PoC | script:scripts/check-requirements.py, ci:core.yml/requirements | This file. |
| S5 | README with deployment, addresses and CLI/mini-app walkthrough | PoC (partial) | doc:README.md, doc:DEMO.md, doc:DEPLOYMENTS.md, doc:cli/README.md | Mini-app walkthrough in Plan 4. |
| PRIV1 | Public and deshield-interact-reshield paths; reshield not skippable | M4 | | Spec §8: public transactions only in the PoC; designs exist (docs/design/ui). |
| PRIV2 | Reshield target must be private; explicit error otherwise | M4 | | As PRIV1. |
| PRIV3 | Fresh ephemeral account per operation | M4 | | As PRIV1. |
| PA1 | Private flow for liquidate, bid, settle | M4 | `private_path_liquidate_is_not_expressible_on_v0_2_4` | `liquidate` is not private-capable on v0.2.4 with this chain shape (ADR-034); bid and settle in M4. |
| PA2 | Atomic deshield of token and gas | M4 | | As PRIV1. |
| PA3 | Outputs reshielded; account never reused | M4 | | As PRIV1. |
| PA4 | What is public and what is private | M4 | | `docs/PRIVACY.md` stub (Task 17); full document M6. |
| SOFT1 | Multi-oracle redundancy | M2 | | Single validated feed with pause in the PoC (spec §2.2). |
| SOFT2 | Formal verification of auction invariants | PoC (partial) | `collateral_invariants_hold_over_random_bids`, doc:crates/auction_core/src/kani_proofs.rs | Property tests and Kani for `mul_div` and the schedule; more in M6. |
| SOFT3 | Liquidator bot SDK | PoC (partial) | golden:watch_observe, doc:docs/architecture/sdk.md | `watch` and the SDK scanner; a bot SDK product is M4. |
| PEND2 | CLI documentation | PoC | doc:cli/README.md | |
| PEND4 | Privacy and anonymisation document | M6 | | Stub added in Plan 3b Task 17. |
| PEND7 | Design decision log | PoC | doc:docs/design-log.md | |
| PEND8 | Audit programme | M7 | | Audit line in the proposal (spec §15). |
| PEND10 | Committed, reproducible benchmark harness | PoC | script:scripts/cycles.sh, doc:CYCLES.md | |
| PEND11 | Threat model (adversary classes, C- and F-class) | PoC | doc:THREAT_MODEL.md | |
```

Run: `python3 scripts/check-requirements.py REQUIREMENTS.md`
Expected: `every requirement maps to existing evidence ...`.

If a token fails, the checker names the row and the reason. Fix the token by looking up the real name: `grep -rn "fn <partial name>" crates programs cli`, or `ls cli/tests/golden`. Never remove a row's evidence to make it pass. A row whose evidence does not exist is a real gap: report it.

- [ ] **Step 5: Commit (controller, GPG signing on Windows)**

```bash
git add REQUIREMENTS.md scripts/check-requirements.py scripts/tests/test_check_requirements.py .github/workflows/core.yml \
  docs/architecture/ci.md README.md
git commit -m "docs: REQUIREMENTS.md with a CI checker mapping every RFP-014 requirement to evidence"
```

---

### Task 16: Red-team pass (dedicated reviewer seat), `cargo audit`, and fixes for Critical and High

This is Plan 3b scope item 10 and spec §10 ("a red-team pass (`security-redteam`) before submission"). This is a review activity, so the findings come from a reviewer who did not implement the code under review. The implementer runs the tools and fixes what the ruling sends back, test first.

**Files:**
- Create: `docs/security/redteam-2026-10.md` (written by the reviewer seat)
- Modify: `THREAT_MODEL.md` (review-log entry; rows and residuals for accepted findings), `docs/architecture/security-checklist.md`
- Modify (only when a Critical or High finding requires it): the owning code and its tests, as Step 4 lists them

**Interfaces:**
- Consumes: the whole tree at `HEAD`; `THREAT_MODEL.md`; `docs/architecture/security-checklist.md`; ADR-019, 026, 027, 033, 038, 039, 042 to 047 and 050.
- Produces:
  - `docs/security/redteam-2026-10.md`, with a findings table `| ID | Severity | Component | Finding | Reproduction | Ruling | Resolution |`; IDs are `RT-01`, `RT-02`, and so on.
  - For each fixed finding, a test named `redteam_rt_<nn>_<slug>`.

- [ ] **Step 1: Dispatch the reviewer seat (controller)**

The controller dispatches a fresh reviewer on the most capable model with the `anthropic-skills:security-redteam` skill. This must not be the session that implemented any 3b task. Its brief:

```
Red-team the RFP-014 auction engine PoC at <HEAD sha> in C:\Users\user\Documents\basecamp\lez-auction-engine
(branch feat/plan-3b). Read-only: do not edit, stage or commit anything except creating
docs/security/redteam-2026-10.md. You may run tests through scripts/wsl-cargo.sh (WSL) and read anything.

Scope, in priority order:
1. programs/auction_engine (all 17 handlers, auction_engine_core, auction_core where the handlers call it):
   fund movement, rounding, caps, pause clock, chain-end effects, system auctions, check_solvency.
2. The stablecoin hooks (programs/stablecoin/src/liquidation_hooks.rs, patches/stablecoin): authentication
   (Variant C), proportional bound, freeze semantics, laundering.
3. crates/auction_sdk: encoders vs enum index, verify_config/verify_binding/verify_chain_identity (I4),
   confirmation and the lock file, the scanner cursor, keystore handling (no key in any output), quotes.
4. cli/: what is signed vs what is shown, exit codes, the dev commands' guard, the oracle publisher.
5. Scripts and CI: scripts/*.sh, scripts/lib/localnet.sh, .github/workflows: command injection, secrets,
   what deletes what, supply chain (pinned actions, curl | bash).
Known and accepted (do not re-report unless you find a new consequence): THREAT_MODEL.md residuals and
review log; docs/design-log.md ADR rulings; .superpowers/sdd/*/final-review.md "Declined to judge".

For each finding give: severity (Critical: funds or keys lost or the host frozen without cause; High: a
safety invariant or I4 broken under realistic conditions; Medium; Low; Info), component and file:line, the
attack or failure in steps, a concrete reproduction (a test sketch or commands), and a suggested fix.
Write docs/security/redteam-2026-10.md: a scope and method section, the findings table
| ID | Severity | Component | Finding | Reproduction | Ruling | Resolution | (leave Ruling and Resolution
empty), then one section per finding. If you find nothing above Low, say so explicitly and list what you
tried.
```

- [ ] **Step 2: Re-run `cargo audit` (implementer)**

Run inside WSL: `scripts/disk-guard.sh && scripts/wsl-env.sh cargo install --locked cargo-audit && scripts/wsl-env.sh cargo audit 2>&1 | tee /tmp/audit.txt`
Then do the same for the fuzz lock file: `scripts/wsl-env.sh cargo audit --file fuzz/Cargo.lock`.

Append an "Appendix: cargo audit (<date>)" to `docs/security/redteam-2026-10.md`. It lists every advisory with:
- its crate and path, from `cargo tree -i <crate>`;
- whether it is new since the 2026-10-07 list in `THREAT_MODEL.md` (that list has seven advisories: `ruint` ×2, `hickory-proto` ×2, `rsa`, `rustls`, `tracing-subscriber`);
- whether anything that runs on chain or in the CLI's signing path depends on it.

- [ ] **Step 3: Ruling (controller)**

For each finding, the controller records in the ledger `Ruling: RT-<nn> <fix | accept | park to Mn> — cost if wrong: ...` and copies the ruling into the table.
- Every Critical and every High is `fix`: spec §10 requires the pass before submission, and the PoC ships no known Critical or High.
- A Medium is `fix` when the fix is local and testable within this task; otherwise `accept`, documented as a THREAT_MODEL residual.
- Low and Info are accepted or parked, with a sentence each.

**Branch A: no Critical or High, and no Medium ruled `fix`.** Skip Step 4 and go to Step 6.

- [ ] **Step 4: Fix each finding ruled `fix`, test first (implementer)**

For each finding, in order of severity:
1. Write the failing test `redteam_rt_<nn>_<slug>` in the owning test module, from the reproduction in the findings document:
   - engine handler: `programs/auction_engine/src/*_tests.rs`, then a real-guest test in `programs/integration_tests/tests/security.rs`;
   - SDK: the module's `mod tests`;
   - CLI: a golden test;
   - script: a case in `scripts/tests/`.
2. Run it and see it fail for the reason the finding gives.
3. Implement the smallest fix and run the test until it passes.
4. Run the owning suite.
5. Apply the Global Constraints that the change triggers:
   - an on-chain change needs a security-reviewer subagent pass on the diff, a regenerated IDL (`scripts/wsl-env.sh scripts/ci/idl-drift.sh` and commit `idl/` if it changed), and rebuilt guests with a refreshed manifest and ImageIDs (Task 5 Steps 4, 6 and 7);
   - a change under `programs/stablecoin/**` must follow the patch-series rules (ADR-025): a hook commit, `scripts/export-hook-patches.sh`, `scripts/check-hook-patches.sh`;
   - a new on-chain behaviour adds a row to `docs/architecture/security-checklist.md`.
6. Fill the finding's `Resolution` cell with the commit's intent and the test name.

- [ ] **Step 5: Scoped re-review (controller)**

The same reviewer seat re-reviews only the fix diffs: are the findings closed, and are there regressions? The controller records the verdict in `docs/security/redteam-2026-10.md` ("Re-review") and in the ledger. A re-review finding of Critical or High goes back to Step 4.

- [ ] **Step 6: Update the threat model and the checklist**

Append to `THREAT_MODEL.md` "Review log":

```markdown
**2026-10-<day>, Plan 3b Task 16 red-team pass.** Reviewer: an independent red-team seat (security-redteam), not the implementer, over the engine, the hooks, the SDK, the CLI, the scripts and CI at <sha>. Findings: <counts per severity> (`docs/security/redteam-2026-10.md`). Fixed: <RT ids with test names, or "none required">. Accepted residuals added above: <RT ids, or "none">. `cargo audit` (<date>): <n> advisories, <new ones or "none new since 2026-10-07">; none in on-chain code or in the CLI signing path, or else <the exception and its ruling>.
```

Add every accepted Medium as a residual bullet. Add every fixed on-chain finding as a row of the attacks table, or as a line in its existing row.

- [ ] **Step 7: Gates**

Run the lint gate and the format gate. If code changed, also run the full suite of Task 18 Step 2 for the touched crates, `scripts/wsl-env.sh scripts/check-guests.sh`, and `python3 scripts/check-requirements.py REQUIREMENTS.md`. Expected: green.

- [ ] **Step 8: Commit (controller, GPG signing on Windows)**

```bash
git add docs/security/redteam-2026-10.md THREAT_MODEL.md docs/architecture/security-checklist.md
# plus, for each fix: the code, its redteam_rt_<nn>_* test, and (on-chain) idl/, artifacts/guests, DEPLOYMENTS.md,
# deployments/localnet.json, patches/stablecoin
git commit -m "docs(security): red-team pass and cargo audit before submission"
```

When fixes landed, commit each fix separately first: `fix(<area>): <finding summary> (RT-<nn>)`. The docs commit comes last.

---

### Task 17: Evidence and outreach drafts, `docs/v03-port-plan.md`, `docs/PRIVACY.md` stub, `SUBMISSION.md`

This is Plan 3b scope item 11, spec §1.2 and §1.5–§1.6, and §12. Everything here is a draft or a record: nothing is posted, filed or published. The user sends what they choose.

**Files:**
- Create: `docs/outreach/2026-10-13-upstream-issue-spel-idl-order.md`, `docs/outreach/2026-10-13-upstream-issue-oracle-scale.md`, `docs/outreach/2026-10-13-upstream-pr-stablecoin-hooks.md`, `docs/outreach/2026-10-13-ecosystem-status-note.md`
- Create: `docs/v03-port-plan.md`, `docs/PRIVACY.md`, `SUBMISSION.md`, `scripts/submission-hashes.sh`
- Modify: `REQUIREMENTS.md` (PEND4 row, PA4 note), `README.md` (links)

**Interfaces:**
- Consumes: `artifacts/guests/manifest.json`, `patches/stablecoin/*.patch`, `DEPLOYMENTS.md`, `CYCLES.md`, `docs/architecture/real-proofs.md`, `docs/evidence/` (if Task 7 created it), `DEMO.md`, `REQUIREMENTS.md`, `docs/security/redteam-2026-10.md`, `docs/architecture/localnet.md` (findings 1, 3, 4, 6).
- Produces: `scripts/submission-hashes.sh` (prints the hash block of `SUBMISSION.md`) and the documents listed under Files.

- [ ] **Step 1: Write the hash script**

Create `scripts/submission-hashes.sh`:

```bash
#!/usr/bin/env bash
# Prints the hashes SUBMISSION.md records (spec §1.2). Run from the repository root.
set -euo pipefail
echo "## Pins"
echo "- LEZ v0.2.4 47eba256479f6f785acbd138834340703cd03401; SPEL 1ef0500f8fc8ce3ddf95523726ae159db83f744f; risc0-zkvm 3.0.5; Rust 1.94.0; lez-programs 64c01210adecd6b557dbd021a3cc94553f243fd8"
echo
echo "## Deployable guests (artifacts/guests/manifest.json)"
python3 - <<'PY'
import json
m = json.load(open("artifacts/guests/manifest.json"))
print(f"- builder: `{m['builder_image']}`")
for name, g in sorted(m["guests"].items()):
    print(f"- {name}: ImageID `{g['image_id']}`, sha256 `{g['sha256']}`")
PY
echo
echo "## Stablecoin hook patch series (patches/stablecoin)"
for p in patches/stablecoin/*.patch; do echo "- $(basename "$p"): sha256 \`$(sha256sum "$p" | cut -d' ' -f1)\`"; done
echo
echo "## Evidence files"
for f in REQUIREMENTS.md CYCLES.md cycles/baseline.json DEMO.md THREAT_MODEL.md docs/security/redteam-2026-10.md docs/architecture/real-proofs.md; do
  echo "- $f: sha256 \`$(sha256sum "$f" | cut -d' ' -f1)\`"
done
if compgen -G "docs/evidence/*.md" > /dev/null; then
  for f in docs/evidence/*.md; do echo "- $f: sha256 \`$(sha256sum "$f" | cut -d' ' -f1)\`"; done
fi
```

Make it executable. Run: `scripts/submission-hashes.sh`
Expected: four sections, with no empty values.

- [ ] **Step 2: Write the upstream drafts**

Create `docs/outreach/2026-10-13-upstream-issue-spel-idl-order.md`:

```markdown
# Draft issue (for logos-co/spel; cross-link from logos-blockchain/lez-programs). Not filed; the user files it.

**Title:** spel-cli encodes an instruction by its IDL position, which differs from the enum index when guest
functions are not in enum order (stablecoin positions 8–18)

**Body**

`spel-cli` (rev 1ef0500f, `spel-cli/src/tx.rs`) encodes an instruction as
`idl.instructions.iter().position(...)`. The guest decodes the risc0-serde enum, whose first word is the
**enum index**. The IDL lists instructions in guest-function order. In lez-programs 64c01210 the stablecoin's
guest functions are not in enum order from position 8 on, so spel-cli sends a different instruction than the
user asked for.

Observed on a v0.2.4 localnet: `set_minimum_collateralization_ratio` (enum 8, IDL position 11) sent with
spel-cli executed `set_timing_parameters` (enum 11) and failed with
"new_minimum_milliseconds_between_rate_updates above maximum".

Full mapping (IDL position → enum index), checked by a test in our repository:

| IDL position | Instruction | Enum index |
|---|---|---|
| 0–7 | initialize_program … close_position | 0–7 |
| 8 | withdraw_collateral | 17 |
| 9 | repay_debt | 18 |
| 10 | set_stability_fee_per_millisecond | 14 |
| 11 | set_minimum_collateralization_ratio | 8 |
| 12 | set_controller_gains | 9 |
| 13 | set_timing_parameters | 11 |
| 14 | set_admin | 12 |
| 15 | set_freeze_authority | 13 |
| 16 | set_market_price_oracle | 10 |
| 17 | freeze | 15 |
| 18 | unfreeze | 16 |

Possible fixes: (a) spel emits the enum index into the IDL (`"discriminant": n`) and spel-cli encodes that;
(b) the IDL generator orders instructions by enum index; (c) lez-programs reorders the stablecoin's guest
functions to enum order. We worked around it with our own encoders (enum index from the program's own enum).
Related: spel-cli cannot encode `i128` arguments (the stablecoin's controller gains), and cannot resolve a PDA
seeded by another PDA of the same instruction.
```

Create `docs/outreach/2026-10-13-upstream-issue-oracle-scale.md`:

```markdown
# Draft issue (for logos-blockchain/lez-programs). Not filed; the user files it.

**Title:** One `OraclePriceAccount`, two scales: twap_oracle writes Q64.64, the stablecoin reads 1e27

**Body**

`twap_oracle`'s `PublishPrice` stores a TWAP tick converted to a Q64.64 ratio (`price / 2^64`), and its
`CreateOraclePriceAccount` documents `initial_price` as Q64.64. The stablecoin reads
`OraclePriceAccount.price` as a 1e27 fixed-point value (its `FIXED_POINT_ONE`). PR #401 discusses this.
Consumers of the same struct cannot tell the scale from the account.

Consequences we hit while building an RFP-014 liquidation engine on v0.2.4:
- a live twap feed cannot drive the stablecoin (or a liquidation engine on 1e27) without a conversion and a
  convention for which scale an account carries;
- our engine therefore carries `oracle_scale ∈ {Ray, Q64x64}` per collateral and converts, but the localnet
  feed is a RAY price account created directly (the twap path is not usable live yet);
- `confidence_interval` is written as 0 by every producer, so confidence checks never trigger.

Questions: which scale should `OraclePriceAccount.price` carry? Should the account carry its scale (or a
version)? Would a "price in 1e27" producer path in twap_oracle be accepted?
```

Create `docs/outreach/2026-10-13-upstream-pr-stablecoin-hooks.md`. It is the pull-request description for the patch series in `patches/stablecoin`, and it states:
- the three appended instructions (`set_liquidation_engine`, `liquidate_position`, `engine_freeze_new_debt`), appended at the end of both the enum and the guest function list, so the enum index equals the IDL index;
- the authentication: the immediate caller program id must be the bound engine, and the bound engine-authority PDA must be authorised (Variant C);
- the proportional bound on `collateral_to_seize`;
- that the freeze reuses `is_frozen` and that only the freeze authority unfreezes;
- that the upstream suite passes unchanged;
- the two discussion points from spec §5.10 and §5.11: the freeze is broader than "pause new debt generation", and liquidation is allowed while frozen;
- that `set_liquidation_engine` is top-level only, so a PDA or multisig host admin cannot bind;
- how to apply: `git am patches/stablecoin/*.patch` on 64c01210, then check with `scripts/check-hook-patches.sh`.

Take the facts from `docs/architecture/hooks.md`, ADR-019, ADR-025, ADR-026 and `THREAT_MODEL.md`; the draft cites them by section.

Create `docs/outreach/2026-10-13-ecosystem-status-note.md`: a short status note of at most 200 words for ecosystem#205 or `#builder-hub`, for the user to post or not. It covers what is built (the R4 table in one sentence each for F1–F5, the CLI, real-proof CI per ADR-045, the one-command demo), what is not (the GUI is next, private path M4, testnet S1 blocked by v0.3), and the repository's visibility state. Write it in the voice of `docs/outreach/2026-10-01-rfp014-intro.md`.

- [ ] **Step 3: Write `docs/v03-port-plan.md` and `docs/PRIVACY.md`**

Create `docs/v03-port-plan.md` with these sections:
1. "Why not now": v0.3 needs SPEL support, which does not exist yet (spec §3.1).
2. "What stays": `auction_core`, which is pure and LEZ-free, and the SDK's quote and plan logic.
3. "What changes": the program layer (SPEL v0.3 API); confirmation, where v0.3's `poll_transaction_status` replaces nonce polling (ADR-042); discovery, where the indexer's `getTransactionsByAccount` and then events replace the block scan (ADR-043); the encoder seam (ADR-040).
4. "Order of work": port `auction_core` callers, then the programs, then the SDK seams, then the CLI; re-measure `CYCLES.md` and the real proofs (ADR-045).
5. "Risks": API churn; the host stablecoin's own port.

Each item cites the ADR it touches.

Create `docs/PRIVACY.md` (stub):

```markdown
# Privacy (stub; full document in milestone M6)

The PoC submits public transactions only (spec §8). Everything an auction does is public: auction state,
liquidation events and amounts, price feeds and position health, and which account liquidated or bid.

What the design keeps possible for the private path (sub-project B, milestone M4): every user action settles
instantly; F4 and F5 are instant Dutch auctions with no escrow and no refunds; the only pull claim is the
position owner's leftover. `liquidate` is not private-capable on LEZ v0.2.4 with its current chain shape
(ADR-034); bids, settlement and system auctions have not been exercised on the private path.

Client behaviour the privacy guarantees will depend on (to be specified in M6): fresh ephemeral accounts per
operation (PRIV3, PA3), a non-skippable reshield (PRIV1), a private reshield target (PRIV2), and atomic
token-and-gas deshield (PA2). What happens if a user bypasses the expected path is part of the M6 document
(template S9).
```

In `REQUIREMENTS.md`:
- change the PEND4 row to `| PEND4 | Privacy and anonymisation document | PoC (partial) | doc:docs/PRIVACY.md | Stub now; full document M6. |`;
- change the PA4 note to `docs/PRIVACY.md stub; full document M6.`

Run: `python3 scripts/check-requirements.py REQUIREMENTS.md`
Expected: pass.

- [ ] **Step 4: Write `SUBMISSION.md`**

Create `SUBMISSION.md`:

```markdown
# RFP-014 liquidation and auction engine: PoC submission

Repository: this repository, at the commit tagged `rfp014-poc` (the tag is created by the author at
submission; `git rev-parse rfp014-poc` gives the full hash, and `scripts/submission-hashes.sh` reproduces
every hash below from that commit).

## What is built
- Engine, stablecoin hooks, F4 debt auction and F5 surplus auction with a mock protocol token
  (`docs/architecture/programs.md`, `docs/architecture/hooks.md`).
- `auction_sdk` and `auction-cli` with their own enum-index encoders (`docs/architecture/sdk.md`, `cli/README.md`).
- CI: core, programs, IDL drift, hook patches, coverage, requirements; real-proof tiers (ADR-045); reproducible
  guests (ADR-047); fuzzing; localnet end-to-end with a `RISC0_DEV_MODE=0` sequencer
  (`docs/architecture/ci.md`).
- Requirements map: `REQUIREMENTS.md`. Threat model and red-team pass: `THREAT_MODEL.md`,
  `docs/security/redteam-2026-10.md`. Cycles: `CYCLES.md`. Real proofs: `docs/architecture/real-proofs.md`.

## Reproduce
`DEMO.md`: `scripts/bootstrap-lez.sh` once, then `scripts/demo.sh` (about <minutes> minutes). Deviation from
spec §1.1: the prerequisites are documented for Ubuntu (apt, rustup, rzup) instead of a Nix flake.

## Demo recording
Recorded by the author (not AI-narrated); link added by the author at submission.

## Hashes
<the output of scripts/submission-hashes.sh, pasted verbatim>

## Honest notes
- LEZ v0.2.4 executes public transactions without proving them; real receipts are produced separately for
  every execution of each instruction (ADR-045).
- The F4 freeze triggers above an absolute threshold (spec §5.11, `REQUIREMENTS.md` F4).
- R2: no live TWAP until the oracle scale is settled (`docs/outreach/2026-10-13-upstream-issue-oracle-scale.md`).
- Testnet deployment (S1) is blocked by v0.3 (`docs/v03-port-plan.md`).
- Upstream drafts (not yet filed): `docs/outreach/2026-10-13-*.md`.
```

Paste the Step 1 output into "Hashes", and the demo duration from `DEMO.md`. Both are recorded values.

- [ ] **Step 5: Links and gates**

In `README.md`, link `SUBMISSION.md`, `DEMO.md`, `REQUIREMENTS.md`, `CYCLES.md` and `docs/outreach/`. Run the lint gate, the format gate and `python3 scripts/check-requirements.py REQUIREMENTS.md`. Expected: clean.

- [ ] **Step 6: Commit (controller, GPG signing on Windows)**

```bash
git add docs/outreach/2026-10-13-*.md docs/v03-port-plan.md docs/PRIVACY.md SUBMISSION.md scripts/submission-hashes.sh REQUIREMENTS.md README.md
git commit -m "docs: upstream and outreach drafts, v0.3 port plan, privacy stub and SUBMISSION.md"
```

---

### Task 18: Plan 3b wrap-up and final gates

**Files:**
- Modify: `README.md` (status), `docs/architecture/programs.md` (carry-over list), `docs/architecture/sdk.md` (Plan 3b changes index)
- Modify: `SUBMISSION.md` (refresh the hashes if anything changed since Task 17)

**Interfaces:**
- Consumes: everything above.
- Produces: a green tree at the head of `feat/plan-3b`, ready for the user's merge decision.

- [ ] **Step 1: Update the status docs**

- `README.md` "Status": add **Plan 3b complete: CI, real proofs, reproducible builds, evidence.** with one bullet each for CI (`docs/architecture/ci.md`), real proofs (ADR-045), reproducible guests (ADR-047), `CYCLES.md`, `REQUIREMENTS.md`, the demo (`DEMO.md`), the red-team pass and `SUBMISSION.md`. Then `Next: Plan 4 (FFI, module, thin GUI).`
- `docs/architecture/programs.md` "Plan 3 carry-overs": every remaining bullet now reads `Done (Plan 3b Task N, ADR-0NN)`. The upstream-issue bullet reads `Drafted (docs/outreach/2026-10-13-upstream-issue-spel-idl-order.md); filing is the author's`.

- [ ] **Step 2: Final gates (background, timeout 3600000 each)**

Run inside WSL, in order, with `scripts/disk-guard.sh` before the first and after the last:
1. `scripts/wsl-cargo.sh test --locked -p auction_core -p auction_engine_core -p auction_engine_program -p engine_integration_tests -p auction_sdk -p auction_cli --features auction_sdk/testing,auction_sdk/rpc`
2. `scripts/wsl-cargo.sh test --locked -p stablecoin_core -p stablecoin_program -p token_program -p integration_tests`
3. the lint gate and the format gate
4. `scripts/wsl-env.sh scripts/ci/idl-drift.sh`
5. `scripts/wsl-env.sh env LEZ_PROGRAMS=$HOME/lez-spikes/s1/lez-programs scripts/check-hook-patches.sh`
6. `scripts/wsl-env.sh scripts/check-guests.sh`
7. `scripts/wsl-env.sh scripts/cycles.sh /tmp/c.jsonl && python3 scripts/check-cycles.py cycles/baseline.json /tmp/c.jsonl`
8. `python3 -m unittest discover -s scripts/tests -v`
9. `python3 scripts/check-requirements.py REQUIREMENTS.md`
10. `scripts/tests/demo-preflight.sh`
11. `for f in .github/workflows/*.yml; do "$HOME/.local/bin/actionlint" "$f"; done`

Expected: every step green. A failure is fixed in the task that owns the file, and that task's commit message type is used for the fix.

- [ ] **Step 3: Refresh the submission hashes**

Run `scripts/submission-hashes.sh` and compare its output with `SUBMISSION.md`. Paste it again if anything differs.

- [ ] **Step 4: Commit (controller, GPG signing on Windows)**

```bash
git add README.md docs/architecture/programs.md docs/architecture/sdk.md SUBMISSION.md
git commit -m "docs: Plan 3b wrap-up"
```

The controller then runs the whole-branch final review (superpowers:subagent-driven-development) and asks the user about merge and PR. Nothing is pushed beyond Open decision 1's feature branch without the user's go-ahead.

---

## Spec coverage (Plan 3b)

| Spec item | Where |
|---|---|
| §1.1 one-command reproduction incl. F4, F5 and the freeze path | Tasks 10–13 (`DEMO.md`, ADR-050, ADR-051) |
| §1.2 `SUBMISSION.md` with linked hashes; author-recorded demo | Task 17 (recording is the author's) |
| §1.3 CI: unit and property tests, dev-mode sequencer e2e, one `RISC0_DEV_MODE=0` e2e per push to `main`, full real-proof suite nightly and on tags, IDL/encoder drift, cycle regression, `REQUIREMENTS.md` check | Tasks 4 (programs, idl-drift), 14 (localnet-e2e), 1 and 7 (real proofs, ADR-045), 6 (cycles), 15 (requirements). Header drift is FFI (Plan 4). |
| §1.4 every hard requirement mapped | Task 15 |
| §1.5 upstream suite unchanged; hooks offered upstream; two upstream issues | Task 4 (`programs` runs the vendored suites; `hook-patches`), Task 17 (PR and issue drafts; filing is the author's) |
| §1.6 early-engagement note drafted | Task 17 (status note; the 2026-10-01 intro exists) |
| §9 real proofs; SDK/CLI; requirements; ≥ 80 % coverage | Tasks 1, 7, 14; Tasks 2–3, 10; Task 15; Task 8 |
| §10 red team, `cargo-fuzz`, `cargo-audit`, clippy | Tasks 16, 9, 16, 4 |
| §11 `CYCLES.md`, > 10 % regression, transaction sizes, load test, committed harness | Task 6 (real-proof column Task 7) |
| §12 docs kept current (`REQUIREMENTS.md`, `SUBMISSION.md`, `DEMO.md`, `CYCLES.md`, `docs/v03-port-plan.md`, `docs/PRIVACY.md`) | Tasks 6, 13, 15, 17 |
| §16 risks: oracle scale, solo throughput and WSL memory/disk | ADR-050 and the Task 17 draft; disk guard and proving discipline (Global Constraints) |
| THREAT_MODEL I4 native-build residual | Task 5 (ADR-047) |
| Plan 3a parked M6, M7, N1–N4, read-only dev commands, panic-after-send test, uncovered crates in CI, `idl-gen` diff | Tasks 3, 12, 2, 3, 2, 4, 4 |

---

## Open decisions for the user

Each has a default; execution proceeds on the default unless the user says otherwise.

1. **Push the feature branch to observe CI during execution.** The CI tasks (4–9, 14, 15) can only be confirmed on GitHub.
   **Default:** the controller pushes `feat/plan-3b` to the private `origin` after those commits and watches the runs with `gh`. There is no PR and no merge, and nothing is pushed anywhere else. If declined, local runs are the verification, and the first CI run is the user's PR.
2. **Runners for real proofs if Task 1 lands in tier C** (hosted runners too small or too slow).
   **Default:** GitHub-hosted runners only. A self-hosted job is written but disabled (`vars.REAL_PROOF_SELF_HOSTED`), and one recorded local full run is committed as evidence. There are no paid larger runners.
3. **Repository visibility and Actions minutes.** As a private repository, the hosted runners are 2 vCPU / 7 GiB with 2000 free minutes a month. The nightly workflows (real proofs, guest repro, fuzz, two-mode e2e) use a large part of that. A public repository gets 4 vCPU / 16 GiB runners and unlimited minutes.
   **Default:** stay private; keep the schedules. If minutes run low, the user switches the nightly workflows to `workflow_dispatch` only.
4. **"A clean machine with Nix"** (spec §1.1).
   **Default:** no Nix flake in 3b. `DEMO.md` documents the Ubuntu prerequisites (apt, rustup, rzup) plus `scripts/bootstrap-lez.sh`, and `SUBMISSION.md` states the deviation. A `flake.nix` devShell is a follow-up.
5. **What "real-proof end-to-end" means on LEZ v0.2.4.** The sequencer never proves public transactions.
   **Default:** present it exactly as ADR-045 does. One part is a `RISC0_DEV_MODE=0` sequencer run of the demo on every push to `main` (RFP S2). The other is real receipts, generated and verified per execution in the tiers the measurement allows. The submission says so in plain words and does not claim proven public transactions.
