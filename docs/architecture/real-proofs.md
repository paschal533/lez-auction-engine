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

## Measurement (Plan 3b Task 1, 2026-10-13)

Machine: WSL Ubuntu-20.04, `nproc` = 8, 11 GiB RAM + 16 GiB swap, r0vm 3.0.5, composite receipts.

| Test | po2 | Status | Wall | Peak RSS (MiB) | Executions / segments (from PROOF lines) |
|---|---|---|---|---|---|
| prove_settle_debt | 18 | 0 | 6:07.61 | 2364 | 1 execution / 3 segments (auction_engine 579,824 user cycles, 353.6 s) |
| prove_liquidate | 18 | 0 | 36:07.12 | 2368 | 5 executions / 18 segments (auction_engine 10, token 1, stablecoin 5, token 1, token 1) |
| prove_liquidate | 19 | 0 | 45:29.15 | 4719 | 5 executions / 10 segments (auction_engine 5, token 1, stablecoin 2, token 1, token 1) |

Peak RSS is GNU `time -v`'s maximum over the process tree; a separate `/proc` VmHWM sampler on the
`r0vm` process read the same 4719 MiB at po2 19. Peak RSS roughly doubles per po2 step.

Not measured in Task 1, deferred for disk space (each proving run grows the Windows pagefile on C:):
- `prove_liquidate` at po2 20: not run. RSS doubles per po2 (about 9.4 GiB expected), above the 5632 MiB
  hosted ceiling, so po2 20 fails R2 without being run.
- `prove_settle_debt` and `prove_bid_with_settlement` at po2 19, and `prove_ppe_observe_feed`: deferred
  to Task 7 (disk), measured in its local full run.

Sequencer with `RISC0_DEV_MODE=0` (Plan 3a smoke, 34 transactions): deferred to Task 14 (disk).

## Tiers

(B), tiered by the Plan 3b Task 1 measurement:
- RFP S2: the `localnet-e2e` job runs `scripts/demo.sh` against a sequencer with `RISC0_DEV_MODE=0` on
  every push to `main` (R1; its first green run is recorded in Task 14).
- Segment size `AUCTION_PROVE_PO2 = 19` (R2): the largest po2 whose `prove_liquidate` run passed
  (4719 MiB ≤ 5632 MiB).
- Per push to `main`: Tier B: `real-proof-push` proves `settle_debt` on a hosted runner; the
  liquidate-bid path is proved nightly (R3). Tier A is out: 4 × 2729 s for `liquidate` alone exceeds the
  2700 s budget. Tier B holds at po2 18 (4 × 367.6 s = 1470 s ≤ 2700 s); the po2 19 time of
  `settle_debt` is confirmed in Task 7.
- Nightly and on tags: Tier C: a self-hosted job (disabled unless `vars.REAL_PROOF_SELF_HOSTED == 'true'`)
  plus a committed local full run in `docs/evidence/` (R4). The hosted estimate
  4 × 2729 s × 10.12 = 110,478 s exceeds even three jobs (3 × 19,800 s = 59,400 s).
- Privacy circuit (`observe_feed`): deferred to Task 7 (disk); not scheduled until measured (R5).

Workflow constants for Task 7: `AUCTION_PROVE_PO2=19`, `PUSH_TIER=B`, `NIGHTLY_TIER=C`, `PPE_NIGHTLY=no`
(until Task 7 measures it).

## Running a proof locally

`scripts/measure-proof.sh <prove_settle_debt|prove_liquidate|prove_bid|prove_bid_with_settlement|prove_ppe_observe_feed> <po2> <timeout_s>`
(inside WSL; one at a time; disk guard before and after; logs in `$HOME/lez-target/plan3b-proofs/`).
On the Windows dev machine, proving memory grows the system-managed pagefile on C:; check
`scripts/disk-guard.sh` before each run.

Notes:
- The prover's `user_cycles` (579,824 for `settle_debt`) is lower than the executor's `cycles()` used in
  ADR-036 (594,757 for the same call): the two count differently, so compare like with like.
- Seal size is about 256 KB per segment (composite receipt).
