# Real proofs on LEZ v0.2.4 (ADR-045)

## What "real proof" means here

LEZ v0.2.4's sequencer executes public transactions with the risc0 executor and never proves them
(`lee/state_machine/src/program/mod.rs`); only the privacy circuit (`execute_and_prove`) and the wallet's
private path call the prover. The PoC submits public transactions only (spec §8). So:

1. **RFP S2 / spec §9 end to end:** the localnet flow driven by `auction-cli` against a standalone
   sequencer started with `RISC0_DEV_MODE=0` (`scripts/demo.sh --sequencer-dev-mode 0`, CI job
   `localnet-e2e`; both added in Task 14). Nothing in that path accepts a dev-mode receipt.
   CI: `.github/workflows/e2e.yml`, job `localnet-e2e` (`RISC0_DEV_MODE=0` on every push to `main`; both modes nightly).
2. **Receipts:** every execution of an instruction's chained calls proved with the risc0 prover
   (`AUCTION_PROVE=1 RISC0_DEV_MODE=0`, external `r0vm` 3.0.5) and verified against its ImageID
   (`programs/integration_tests/tests/common/prove.rs`). These are the receipts the v0.3 port or the
   private path must produce. The guests proved are the native builds of the same source; the deployed
   Docker builds have other ImageIDs (ADR-047).

## Measurement (Plan 3b Task 1, 2026-10-13)

Machine: WSL Ubuntu-20.04, `nproc` = 8, 11 GiB RAM + 16 GiB swap, r0vm 3.0.5, composite receipts.

| Test | po2 | Status | Wall | Peak RSS (MiB) | Executions / segments (from PROOF lines) |
|---|---|---|---|---|---|
| prove_settle_debt | 18 | 0 | 6:07.61 | 2364 | 1 execution / 3 segments (auction_engine 579,824 user cycles, 353.6 s) |
| prove_liquidate | 18 | 0 | 36:07.12 | 2368 | 5 executions / 18 segments (auction_engine 10, token 1, stablecoin 5, token 1, token 1) |
| prove_liquidate | 19 | 0 | 45:29.15 | 4719 | 5 executions / 10 segments (auction_engine 5, token 1, stablecoin 2, token 1, token 1) |

Peak RSS is GNU `time -v`'s maximum over the process tree; a separate `/proc` VmHWM sampler on the
`r0vm` process read the same 4719 MiB at po2 19. Peak RSS doubled from po2 18 to po2 19; that is one step, so the doubling is an observation, not a law.

Not measured in Task 1, deferred for disk space (each proving run grows the Windows pagefile on C:):
- `prove_liquidate` at po2 20: not run; expected about 9.4 GiB from the po2 18→19 doubling, above the
  5632 MiB ceiling; PO2 = 19 by controller ruling.
- `prove_settle_debt` and `prove_bid_with_settlement` at po2 19, and `prove_ppe_observe_feed`: deferred
  to Task 7 (disk). The full suite then ran at po2 19 (below); `prove_ppe_observe_feed` was not re-run.

Sequencer with `RISC0_DEV_MODE=0`: run locally on 2026-10-16 as the full demo (`scripts/demo.sh
--sequencer-dev-mode 0`: 973 s, 37 transactions, every check `ok`, sequencer about 84 MB, no `r0vm` process), which
covers the Plan 3a smoke's path and more; see [DEMO.md](../../DEMO.md). CI: the `e2e` workflow (Task 14).

## Tiers

(B), tiered by the Plan 3b Task 1 measurement:
- RFP S2: the `localnet-e2e` job (added in Task 14) runs `scripts/demo.sh` (added in Task 14) against a sequencer with `RISC0_DEV_MODE=0` on
  every push to `main` (R1; its first green run is recorded in Task 14).
- Segment size `AUCTION_PROVE_PO2 = 19` (R2): the largest po2 whose `prove_liquidate` run passed
  (4719 MiB ≤ 5632 MiB).
- Per push to `main`: Tier B: `real-proof-push` proves `settle_debt` on a hosted runner; the
  liquidate-bid path is proved by the Tier C self-hosted job when it is enabled, plus the committed local
  full run (R3). Tier A is out: 4 × 2729 s for `liquidate` alone exceeds the
  2700 s budget. Tier B holds at po2 18 (4 × 367.6 s = 1470 s ≤ 2700 s); the po2 19 time of
  `settle_debt` is confirmed in Task 7: 304 s at po2 19 in the local full run (2 segments), so the
  assumed hosted factor of 4 gives about 1216 s, inside the 2700 s budget.
- Nightly and on tags: Tier C: a self-hosted job (disabled unless `vars.REAL_PROOF_SELF_HOSTED == 'true'`)
  plus a committed local full run in `docs/evidence/` (R4). The hosted estimate
  4 × 2729 s × 10.12 = 110,478 s exceeds even three jobs (3 × 19,800 s = 59,400 s).
- Privacy circuit (`observe_feed`): not scheduled (`PPE_NIGHTLY=no`); `prove_ppe_observe_feed` was not
  re-run in Task 7's local full run.

Workflow constants as built in Task 7 (`.github/workflows/real-proofs.yml`): `AUCTION_PROVE_PO2=19`,
`PUSH_TIER=B`, `NIGHTLY_TIER=C`, `PPE_NIGHTLY=no`.

## Local full run (Plan 3b Task 7, 2026-10-16)

The whole ADR-036 suite was proved at po2 19 with `scripts/prove-suite.sh 19 21600`: 42 executions, 75
segments, all receipts verified, 17 labels. The three chain-budget tests took 5:02:43, 32:35 and 47:52, each
with a peak RSS of about 4.6 GiB. The numbers, the `summary.txt` and the digest of `proofs.jsonl` are in
[docs/evidence/real-proofs-2026-10.md](../evidence/real-proofs-2026-10.md); the per-instruction proving table
is in `CYCLES.md` ("Real proofs").

## Hosted timings

Pending. `real-proofs.yml` is not on `main`, and a schedule or dispatch trigger only works from the default
branch, so no hosted run of `real-proof-push` or `real-proof-nightly` exists yet. The first timing of
`real-proof-push` (`settle_debt`, po2 19) is recorded here after the merge to `main`. If that job is killed
(exit 137 or "did not finish" from the checker), the rule is: lower `AUCTION_PROVE_PO2` by one and dispatch
once more; if it still fails, move the tier down and update ADR-045 and the workflow in the same change.

## Optional self-hosted runner

`real-proof-nightly` (the whole ADR-036 suite) runs only on a self-hosted runner. The job is disabled by
default and is not required for CI to be green: while the repository variable `REAL_PROOF_SELF_HOSTED` is
unset, the job is skipped.

- Requirements: Linux x64, at least 16 GiB RAM, at least 8 cores, at least 60 GB disk, and `rzup` with the
  pinned components (see `.github/actions/install-risc0` for the versions).
- Registration: a repository runner with the labels `self-hosted,linux,x64,risc0-prover`.
- Enabling: set the repository variable `REAL_PROOF_SELF_HOSTED=true`.

The job has no `free-disk.sh` or `apt-get` step, so the runner must already have clang, libclang,
pkg-config and libssl. Until such a runner exists, the committed local full run above is the evidence for
the nightly tier.

## Running a proof locally

`scripts/measure-proof.sh <prove_settle_debt|prove_liquidate|prove_bid|prove_bid_with_settlement|prove_ppe_observe_feed> <po2> <timeout_s>`
(inside WSL; one at a time; disk guard before and after; logs in `$HOME/lez-target/plan3b-proofs/`).
On the Windows dev machine, proving memory grows the system-managed pagefile on C:; check
`scripts/disk-guard.sh` before each run.

Notes:
- The prover's `user_cycles` (579,824 for `settle_debt`) is lower than the executor's `cycles()` used in
  ADR-036 (594,757 for the same call): the two count differently, so compare like with like.
- Seal size is about 256 KB per segment (composite receipt).
