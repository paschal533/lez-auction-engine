# Cycles and fees per instruction (liquidation slice)

Measured 2026-11-01 on LEZ `v0.4.0-rc1` (`1806e873`) by `harness/tests/cycles.rs`
(`cargo test -p harness --test cycles -- --nocapture`), which simulates each transaction with
`lee::simulate_public_transaction` right before it is sent. The transactions are the ones the other
tests send, with the admission evidence they need (the auction vault Pda for `Liquidate`, the
buffer Pda for the first `Bid`).

Scenario: collateral 500_000, debt 600_000, redemption price and oracle 0.5; `SettleEmpty` on a
3-unit lot whose whole seizure goes to the keeper reward.

| Instruction | Executions | Cycles | Fee at base fee 8 (cycles x 8) |
|---|---|---|---|
| Init | 7 | 178,715 | 1,429,720 |
| SetCollateral | 11 | 309,596 | 2,476,768 |
| Liquidate | 27 | 772,287 | 6,178,296 |
| Bid (partial) | 13 | 363,522 | 2,908,176 |
| Bid (final) | 18 | 498,372 | 3,986,976 |
| ClaimLeftover | 5 | 109,917 | 879,336 |
| SettleEmpty | 6 | 190,764 | 1,526,112 |

- **Gas cap:** 10,000,000 cycles per transaction (`max_gas_exec`, spike S8). The test asserts every
  row stays below it; the most expensive instruction, `Liquidate`, uses under 8% of it.
- **Executions** count every message delivered in the transaction, replies included. The
  simulator reports cycles only, so the counts come from the as-built message sequences.
- **Fee** is cycles x the base fee of 8. Storage bytes are charged separately at `base_fee_stor`
  and are not included.
- **Base fees move with demand.** The fee column is a snapshot at base fee 8, not a quote.
