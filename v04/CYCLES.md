# Cycles and fees per instruction

Measured 2026-11-03 on LEZ `v0.4.0-rc1` (`1806e873`) by `harness/tests/cycles.rs`
(`cargo test -p harness --test cycles -- --nocapture`), which simulates each transaction with
`lee::simulate_public_transaction` right before it is sent. The transactions are the ones the other
tests send. Fresh holdings are granted by the token's `HOLDING` seed (ADR-062), so the only
admission evidence is the buffer holding's `Pda`, on a system op that reads that holding while its
account is absent (`StartDebt` below).

- **Gas cap:** 10,000,000 cycles per transaction (`max_gas_exec`, spike S8). The test asserts every
  row succeeds and stays below it; the most expensive instruction, `Liquidate`, uses under 9% of it.
- **Gas limit:** the SDK simulates, then declares `ceil(cycles x 1.2)` (ADR-061): 20% headroom over
  the simulated cycles, still far below the cap for every row. A revert is charged its whole limit.
- **Executions** are counted from the message sequences: every message delivered in the
  transaction, replies included (a `ReadState` is two: the read and its `StateReply`; a token debit
  is three: the owner's token actor, its holding, then the credit or the definition's
  `SupplyDown`). The simulator reports cycles only.
- **Fee** is cycles x the base fee of 8. Storage bytes are charged separately at `base_fee_stor`
  and are not included. **Base fees move with demand:** the fee column is a snapshot, not a quote.

## Liquidation slice (Plan 5 rows, re-measured after ADR-062)

Scenario: collateral 500_000, debt 600_000, redemption price and oracle 0.5; `SettleEmpty` on a
3-unit lot whose whole seizure goes to the keeper reward.

| Instruction | Executions | Cycles | Fee at base fee 8 | Plan 5 (2026-11-01) | Delta |
|---|---|---|---|---|---|
| Init | 9 | 269,226 | 2,153,808 | 7 / 178,715 | +90,511 |
| SetCollateral | 11 | 325,765 | 2,606,120 | 11 / 309,596 | +16,169 |
| Liquidate | 29 | 839,357 | 6,714,856 | 27 / 772,287 | +67,070 |
| Bid (partial) | 15 | 407,700 | 3,261,600 | 13 / 363,522 | +44,178 |
| Bid (final) | 21 | 576,148 | 4,609,184 | 18 / 498,372 | +77,776 |
| ClaimLeftover | 6 | 136,440 | 1,091,520 | 5 / 109,917 | +26,523 |
| SettleEmpty | 6 | 208,291 | 1,666,328 | 6 / 190,764 | +17,527 |

The ADR-062 proxy hop adds one execution per token debit: `Liquidate` +2, `Bid` (partial) +2,
`Bid` (final) +3, `ClaimLeftover` +1. Rows without a debit (`Init`, `SetCollateral`,
`SettleEmpty`) grew too: Plan 6's larger config and accounting states and message set (the system
parameters, the protocol token, the system scratch) cost more to decode and commit everywhere, so
the deltas are not all the hop. `Init` grew again with ADR-065: the host's `ReadState` and its reply are
two more executions and 52,655 more cycles than the 216,571 it cost before; the other rows are not
re-recorded for it.

## System instructions (Plan 6)

Scenarios: no protocol token, the lot-3 shortfall booked (unbacked 10_000) for `ObserveFeed`,
`SettleDebt` (100 of a 4_000 buffer), `CheckSolvency` `Healthy` (threshold 10_000; the implicit
settle burns the remaining 3_900) and `Freeze` (threshold 1_000, empty buffer); the protocol token
enabled for the surplus auction (1_500 in the buffer, lot 500, takes of 250 and 250, a second
auction closed after the TTL) and for `SetEngineParams` (`surplus_lot` changes, so the accounting
asserts the system idle; the collateral type is recorded); the shortfall again for the debt auction
(the buffer holding absent at `StartDebt`, a whole-lot take of 500, a second auction closed after
the TTL). No reference oracle.

The rows measure the harness's actor sets (`World::system_actors`), which declare the bound host
and the buffer's burn actors on every system op that may reach them. The SDK plans declare the
same may-touch actors (ruling R-8b) and otherwise exactly what the run touches. A declared actor
the run does not reach never executes, so it adds no cycles: the SDK plans' simulated cycles match
these rows, and the SDK fits each send's gas from its own simulation anyway.

| Instruction | Executions | Cycles | Fee at base fee 8 (cycles x 8) |
|---|---|---|---|
| SetEngineParams | 6 | 193,353 | 1,546,824 |
| SetSlack | 5 | 155,069 | 1,240,552 |
| ObserveFeed | 7 | 176,218 | 1,409,744 |
| SettleDebt | 8 | 206,767 | 1,654,136 |
| StartSurplus | 10 | 255,298 | 2,042,384 |
| TakeSurplus (partial) | 11 | 243,063 | 1,944,504 |
| TakeSurplus (final) | 11 | 245,296 | 1,962,368 |
| CloseSurplus | 11 | 234,469 | 1,875,752 |
| StartDebt | 7 | 217,756 | 1,742,048 |
| TakeDebt | 11 | 271,617 | 2,172,936 |
| CloseDebt | 5 | 111,516 | 892,128 |
| CheckSolvency (Healthy, burns the buffer) | 9 | 218,253 | 1,746,024 |
| CheckSolvency (Freeze) | 8 | 236,486 | 1,891,888 |

`SetSlack` (ADR-064), with the collateral type recorded, is the proxy, the config probe, the config,
the collateral probe and `SlackUpdate` at the collateral actor (3 without a recorded type). It never
touches the accounting. Re-measured with it, the other rows came out 0 to 41 cycles above their
recorded figures, except `SetEngineParams` at +537 (0.3%). That is likely from the larger guest. They
keep their earlier figures here.

Path-dependent executions: an implicit settle that burns adds the buffer's `Burn` (+3) to
`StartSurplus`, `StartDebt` and `CheckSolvency`; a configured reference oracle adds its read (+2) to
`StartSurplus`, `StartDebt` and `CheckSolvency` only (the accounting's system ops; takes, closes,
`SettleDebt`, `ObserveFeed` and `SetEngineParams` never read it); `SettleDebt` that burns nothing
is 5; `SetEngineParams` without a
locked-field change is 5; `CheckSolvency` `Healthy`/`NoAction` without a burn is 6. Every system row
stays under 3% of the cap.
