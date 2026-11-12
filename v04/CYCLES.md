# Cycles and fees per instruction

Measured 2026-11-03 on LEZ `v0.4.0-rc1` (`1806e873`) and re-measured 2026-11-11 after Plan 9 (the
legacy preset: TWAP, breaker and D4 off) by `harness/tests/cycles.rs`
(`cargo test -p harness --test cycles -- --nocapture`; the shared helpers are in
`harness/src/cycles.rs`), which simulates each transaction with
`lee::simulate_public_transaction` right before it is sent. The transactions are the ones the other
tests send. Fresh holdings are granted by the token's `HOLDING` seed (ADR-062), so the only
admission evidence is the buffer holding's `Pda`, on a system op that reads that holding while its
account is absent (`StartDebt` below).

- **Gas cap:** 10,000,000 cycles per transaction (`max_gas_exec`, spike S8). The test asserts every
  row succeeds and stays below it; the most expensive measured instruction, `Liquidate` under the
  Tier 2 preset with a full ring at the price 0.5, uses under 9% of it. That is a fast-path figure:
  at high prices (collateral base units per coin base unit) the `Liquidate` and `Bid` rows may cost
  more, and that has not been measured (see "Price-dependent cost" in the Tier 2 section).
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

| Instruction | Executions | Cycles (2026-11-11) | Fee at base fee 8 | 2026-11-03 | Delta |
|---|---|---|---|---|---|
| Init | 9 | 268,510 | 2,148,080 | 269,226 | -716 |
| SetCollateral | 11 | 321,027 | 2,568,216 | 325,765 | -4,738 |
| Liquidate | 29 | 845,645 | 6,765,160 | 839,357 | +6,288 |
| Bid (partial) | 15 | 410,776 | 3,286,208 | 407,700 | +3,076 |
| Bid (final) | 21 | 582,089 | 4,656,712 | 576,148 | +5,941 |
| ClaimLeftover | 6 | 137,873 | 1,102,984 | 136,440 | +1,433 |
| SettleEmpty | 6 | 212,506 | 1,700,048 | 208,291 | +4,215 |

Plan 9 delta (legacy preset, empty ring): no row gained an execution, and every row is within
6,300 cycles of its 2026-11-03 figure. Plan 9's TWAP terms, the feed check's confidence limit and
sanity bounds, `conf_bps`, the confidence edge, the TWAP anchor, the deviation test and the stale
cap and floored price use `risk_core::math::mul_div_floor`/`mul_div_ceil`, which give exactly
`auction_core::math`'s results but divide natively when the product fits a u128; the band clamp
(`auction_core::price::bid_price`) and the Q64x64 `to_ray` path still call the frozen
`mul_div_floor`. `auction_core`'s 128-step bit-serial division
costs thousands of cycles per call in the guest. Before that change the observing rows had grown
by 32,000-63,000 cycles. Plan 5 (2026-11-01) figures for the history: Init 7 / 178,715,
SetCollateral 11 / 309,596, Liquidate 27 / 772,287, Bid (partial) 13 / 363,522, Bid (final)
18 / 498,372, ClaimLeftover 5 / 109,917, SettleEmpty 6 / 190,764.

The ADR-062 proxy hop adds one execution per token debit: `Liquidate` +2, `Bid` (partial) +2,
`Bid` (final) +3, `ClaimLeftover` +1. Rows without a debit (`Init`, `SetCollateral`,
`SettleEmpty`) grew too: Plan 6's larger config and accounting states and message set (the system
parameters, the protocol token, the system scratch) cost more to decode and commit everywhere, so
the deltas are not all the hop. `Init` grew again with ADR-065: the host's `ReadState` and its reply are
two more executions and 52,655 more cycles than the 216,571 it cost before; the other rows are not
re-recorded for it. (Before Plan 9, re-measured on the Plan 6 final tree, the other slice rows
differed from their 2026-11-03 figures by between 549 cycles below and 1,060 above.)

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

| Instruction | Executions | Cycles (2026-11-11) | Fee at base fee 8 (cycles x 8) | 2026-11-03 | Delta |
|---|---|---|---|---|---|
| SetEngineParams | 6 | 197,182 | 1,577,456 | 193,353 | +3,829 |
| SetSlack | 5 | 159,059 | 1,272,472 | 155,235 | +3,824 |
| ObserveFeed | 7 | 169,522 | 1,356,176 | 176,218 | -6,696 |
| SettleDebt | 8 | 207,540 | 1,660,320 | 206,767 | +773 |
| StartSurplus | 10 | 255,707 | 2,045,656 | 255,298 | +409 |
| TakeSurplus (partial) | 11 | 243,219 | 1,945,752 | 243,063 | +156 |
| TakeSurplus (final) | 11 | 245,448 | 1,963,584 | 245,296 | +152 |
| CloseSurplus | 11 | 234,480 | 1,875,840 | 234,469 | +11 |
| StartDebt | 7 | 218,152 | 1,745,216 | 217,756 | +396 |
| TakeDebt | 11 | 271,714 | 2,173,712 | 271,617 | +97 |
| CloseDebt | 5 | 111,562 | 892,496 | 111,516 | +46 |
| CheckSolvency (Healthy, burns the buffer) | 9 | 219,264 | 1,754,112 | 218,253 | +1,011 |
| CheckSolvency (Freeze) | 8 | 237,436 | 1,899,488 | 236,486 | +950 |

Plan 9 delta: `ObserveFeed` runs the Observe block and came out 6,696 below its earlier figure
(see the slice's note). `SetEngineParams` and `SetSlack`, which reach the collateral actor without
observing, grew by under 4,000; the rest by under 1,100.

`SetSlack` (ADR-064), with the collateral type recorded, is the proxy, the config probe, the config,
the collateral probe and `SlackUpdate` at the collateral actor (3 without a recorded type). It never
touches the accounting. (Before Plan 9, re-measured on the Plan 6 final tree, the other system rows
came out between 338 cycles below and 343 above their 2026-11-03 figures, `SetEngineParams` +740.)

Path-dependent executions: an implicit settle that burns adds the buffer's `Burn` (+3) to
`StartSurplus`, `StartDebt` and `CheckSolvency`; a configured reference oracle adds its read (+2) to
`StartSurplus`, `StartDebt` and `CheckSolvency` only (the accounting's system ops; takes, closes,
`SettleDebt`, `ObserveFeed` and `SetEngineParams` never read it); `SettleDebt` that burns nothing
is 5; `SetEngineParams` without a
locked-field change is 5; `CheckSolvency` `Healthy`/`NoAction` without a burn is 6. Every system row
stays under 3% of the cap.

## Tier 2 (Plan 9): TWAP preset, ring full

Measured 2026-11-11 by `harness/tests/cycles_tier2.rs`
(`cargo test -p harness --test cycles_tier2 -- --nocapture`). Scenario: `World::new_tier2` under
`RiskArgs::recommended()` (30 min window, 5 min spacing, `min_obs` 3, 20 % breaker, 1 h stale
window), feed max age 600_000 ms, the slice's amounts (collateral 500_000, debt 600_000, redemption
price and oracle 0.5). The ring holds 8 samples before every row, the worst case for ring fill
(ruling R9-10) at this price; it is not the worst case over prices (below).
Each row runs at least 1 s after the newest sample, because a sample at `now` has no weight.
`ObserveFeed` is measured on a fresh publish, so it evicts the oldest sample. `SetPauseLevel` sets
level 1. Budgets are asserted by the test. The R9-13 e2e values are not used here: R9-3 refuses
them at this world's feed max age.

| Instruction | Executions | Cycles | Fee at base fee 8 | Budget | Spec §4 estimate |
|---|---|---|---|---|---|
| Liquidate | 29 | 883,636 | 7,069,088 | <= 1,000,000 | 920,000-980,000 |
| Bid (partial) | 15 | 431,669 | 3,453,352 | <= 520,000 | 470,000-490,000 |
| Bid (final) | 21 | 606,461 | 4,851,688 | <= 720,000 | 650,000-690,000 |
| ObserveFeed | 7 | 181,947 | 1,455,576 | gas cap | ~200,000 |
| SetCollateral | 11 | 359,048 | 2,872,384 | gas cap | ~350,000 |
| SetPauseLevel | 12 | 360,079 | 2,880,632 | <= 400,000 (R9-10) | ~330,000 |

- Every budgeted row fits at this price (the fast path, below). `Liquidate`, `Bid (partial)`,
  `Bid (final)` and `ObserveFeed` come in under the spec's estimates.
- **Price-dependent cost (open item).** These are fast-path figures at the price 0.5
  (`P = RAY / 2`, `harness/tests/cycles_tier2.rs:28`). `risk_core::math` divides natively only
  while `x · y` fits a u128 (`risk_core/src/math.rs:13-18`); otherwise it calls the frozen 128-step
  division, about 10,000 cycles a call (spike S4). A TWAP term is `p · w`, with `p` the RAY price
  and `w` a sample's weight in ms, and every weight is at most `min(twap_window_ms,
  max_oracle_age_ms)` (`risk_core/src/twap.rs`). The TWAP limit, the price below which every
  term is guaranteed the fast path, is `p < 2^128 / min(window, max_age)`. As a price in collateral
  base units per coin base unit, that is about 567,000 in this world (max age 600_000 ms) and about
  226,000 at the largest max age the recommended preset accepts (1,499,999 ms). It is about 189,000
  for a 1,800,000 ms window when the max age is at least the window, and about 3,900 at the 24 h
  maximum window and max age. The limit is a sufficient condition: above it, only a term whose own
  `p · w` reaches 2^128 may take the slow path, at most 8 frozen divisions per `Liquidate` or `Bid`.
  Every other multiply-divide stays native up to far higher prices: the sanity bound to a
  redemption price of about 17,000,000, the anchor to about 23,000,000, `edge` to about
  34,000,000. So `ObserveFeed`, `SetCollateral` and `SetPauseLevel`, which compute no TWAP, keep
  their figures up to the first of these. **Slow-path rows have not been measured.** Whether
  `Liquidate` and `Bid` stay inside their assertions just above the TWAP limit has not been
  established by measurement. The pre-change measurement (ADR-070) was taken with every
  multiply-divide on the frozen path: `Liquidate` 1,012,946, `Bid (partial)` 561,253,
  `Bid (final)` 736,045, `SetPauseLevel` 407,582. It is a loose upper bound, approached only at
  prices far above the TWAP limit, where the remaining multiply-divides also switch. The budget
  assertions, "under 9 %" above, and the headroom for later plans are therefore established on the
  fast path only. Cost stays bounded by a constant (the TWAP loop is bounded at 8 samples), but it depends on the price. Not decided yet:
  a cheaper exact wide division in `risk_core::math`, or budgets for the slow path. Fallback (D4)
  bids are not in this table.
- `SetCollateral` (+9,048) and `SetPauseLevel` (+30,079) are above their estimates, which are not
  assertions. `SetPauseLevel` is 3.6 % of the cap: R9-10 set its assertion at 4 %, because §4's
  "~330,000" and "< 3 %" contradict each other.
- **Ring fill:** a partial bid with `twap_min_obs` 1 costs 415,266 cycles with 1 sample and 431,578
  with 8. That spread of 16,312 is under the asserted 40,000
  (`bid_cost_is_independent_of_ring_fill`).
- **Over the legacy preset**, the full ring and the recommended preset add 37,991 to `Liquidate`,
  20,893 to `Bid (partial)`, 24,372 to `Bid (final)`, 12,425 to `ObserveFeed` and 38,021 to
  `SetCollateral`.
- **Executions.** `SetPauseLevel` is 12: the admin's proxy; the seeded `Probe { Config }`;
  `SetPauseLevel` at the config; the seeded `Probe { Collateral }`; `PauseLevelUpdate` at the
  collateral actor; then the Observe block (`Observe`, the host's and the oracle's `ReadState` and
  their replies, the clock's `AssertTimestamp`, `Proceed`). The TWAP and the level checks add
  computation, not messages, so the other rows keep their slice and system counts.
