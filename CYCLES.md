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
| Instruction (scenario label) | Executions | Total cycles | Largest execution | % of 32 Mi | Signed tx bytes |
|---|---|---|---|---|---|
| bid | 3 | 1,627,002 | 1,354,630 | 4.0 | 548 |
| bid_final | 4 | 1,965,806 | 1,559,517 | 4.6 | 580 |
| check_solvency(burn+freeze) | 3 | 1,534,712 | 1,125,646 | 3.4 | 516 |
| check_solvency(freeze) | 2 | 1,339,486 | 1,068,785 | 3.2 | 516 |
| claim_leftover | 2 | 839,015 | 705,263 | 2.1 | 324 |
| close_surplus | 2 | 918,590 | 784,673 | 2.3 | 388 |
| liquidate | 5 | 3,151,473 | 1,961,706 | 5.8 | 716 |
| observe_feed | 1 | 690,208 | 690,208 | 2.1 | 356 |
| set_collateral_config | 1 | 926,987 | 926,987 | 2.8 | 556 |
| settle_debt | 1 | 594,757 | 594,757 | 1.8 | 340 |
| settle_debt(burn) | 2 | 789,676 | 651,311 | 1.9 | 340 |
| start_debt | 1 | 839,342 | 839,342 | 2.5 | 460 |
| start_debt(burn) | 2 | 1,034,258 | 895,893 | 2.7 | 460 |
| start_surplus | 3 | 1,161,395 | 905,203 | 2.7 | 460 |
| start_surplus(burn) | 4 | 1,356,516 | 961,959 | 2.9 | 460 |
| take_debt | 3 | 1,321,095 | 1,006,238 | 3.0 | 484 |
| take_surplus | 3 | 1,220,087 | 947,419 | 2.8 | 452 |
<!-- cycles:end -->

## P2 load test (N concurrent auctions, interleaved bids)

`load_concurrent_auctions_with_interleaved_bids_stay_isolated` (`tests/load.rs`) makes two assertions. A reference partial bid (30,000 coin in) is measured with exactly 1 live auction; then 6 positions are liquidated and 3 rounds of one partial bid on each auction are interleaved, 18 bids in all. Output lines:

```
LOAD reference_1_live_auction_cycles=1612060 first_round_N=6 min_cycles=1611475 max_cycles=1612210
LOAD N=6 bids=18 min_cycles=1611475 max_cycles=1612210 spread_percent=0.05
```

- A first-round bid with 6 live auctions (1,611,475 to 1,612,210 cycles) is within 5 % of the same bid with 1 live auction (1,612,060 cycles).
- The 18 bids over the 6 auctions and 3 rounds are uniform: the cheapest and the dearest differ by 0.05 % (limit 5 %).

Every auction then settles with a final bid and `type_reserved` returns to 0.

## Real proofs

See `docs/architecture/real-proofs.md` and the evidence linked from ADR-045 (Task 7).
