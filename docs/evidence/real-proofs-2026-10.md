# Real-proof evidence, full ADR-036 suite (2026-10-16)

Local full run of the ADR-036 suite with the risc0 prover (`RISC0_DEV_MODE=0`, `AUCTION_PROVE=1`), as the
NIGHTLY tier C evidence of ADR-045. Every execution of every instruction's chain was proved and its receipt
verified against the ImageID of the natively built guest.

## Setup

| Item | Value |
|---|---|
| Date | 2026-10-16 |
| Commit | `0dec41064acffce5f6dd6735fd1cafd04cc2f14f` (the working tree differed only in CI scripts and docs; no Rust or guest change) |
| `rustc` | 1.94.0 (4a4ef493e 2026-03-02) |
| `r0vm` | risc0-r0vm 3.0.5 |
| Segment size | `AUCTION_PROVE_PO2=19` |
| Machine | WSL Ubuntu-20.04, 8 cores, memory capped at 8 GB (`.wslconfig`) |
| Command | `scripts/prove-suite.sh 19 21600` |

The three tests of `programs/integration_tests/tests/chains.rs` ran one at a time, each in its own process.
`prove_ppe_observe_feed` (the privacy circuit over `observe_feed`) was not run: `PPE_NIGHTLY=no` in
ADR-045, and it has never completed a run (earlier attempts died with the WSL VM). The `observe_feed` row below is the public-path execution of that instruction, not the privacy circuit.

An honest note on the run: two earlier attempts died before finishing because the Windows host ran out of
commit memory (event 2004), not because of the prover. The fix was to stop other applications and cap WSL at
`memory=8GB`; the third attempt, run detached inside WSL, completed. The 17-proof partial report of an earlier
run is archived separately and is not part of these numbers.

## Result

42 executions, 75 segments, all receipts verified, 17 labels. Summed prover time 11,673 s. `summary.txt`:

```
bid: 3 executions, 6 segments, 856 s, all receipts verified
bid_final: 4 executions, 7 segments, 1118 s, all receipts verified
check_solvency(burn+freeze): 3 executions, 5 segments, 883 s, all receipts verified
check_solvency(freeze): 2 executions, 4 segments, 693 s, all receipts verified
claim_leftover: 2 executions, 3 segments, 520 s, all receipts verified
close_surplus: 2 executions, 3 segments, 507 s, all receipts verified
liquidate: 5 executions, 10 segments, 1681 s, all receipts verified
observe_feed: 1 executions, 2 segments, 441 s, all receipts verified
set_collateral_config: 1 executions, 3 segments, 456 s, all receipts verified
settle_debt: 1 executions, 2 segments, 304 s, all receipts verified
settle_debt(burn): 2 executions, 3 segments, 500 s, all receipts verified
start_debt: 1 executions, 2 segments, 407 s, all receipts verified
start_debt(burn): 2 executions, 4 segments, 521 s, all receipts verified
start_surplus: 3 executions, 5 segments, 654 s, all receipts verified
start_surplus(burn): 4 executions, 6 segments, 749 s, all receipts verified
take_debt: 3 executions, 5 segments, 732 s, all receipts verified
take_surplus: 3 executions, 5 segments, 650 s, all receipts verified
```

## Per-test wall time and peak memory

From the `/usr/bin/time -v` footer of each test log (peak RSS is the maximum over the process tree):

| Test | Wall time | Maximum resident set size |
|---|---|---|
| `every_instruction_fits_the_chain_budget` | 5:02:43 | 4,839,180 kB (4.62 GiB) |
| `debt_and_solvency_chains_fit_the_budget` | 32:35.27 | 4,837,924 kB (4.61 GiB) |
| `implicit_burn_variants_fit_the_budget` | 47:52.41 | 4,838,748 kB (4.61 GiB) |

Peak memory is the same for all three (about 4.6 GiB), set by the largest segment at po2 19, and matches the
4719 MiB of the Task 1 `prove_liquidate` measurement.

## Report digest

`proofs.jsonl` (42 lines, one per execution) is not committed. It stays in
`$HOME/lez-target/plan3b-proofs/suite-po2-19/` inside WSL, and this digest stands in for it:

```
c9f73763fefe3ef9eed2fcbb97ccd51421591b9136864a7a92bc55d5b43e68a9  proofs.jsonl
```

`scripts/check-proof-report.py proofs.jsonl --expect 'liquidate,take_debt,check_solvency(freeze)'` passed.
