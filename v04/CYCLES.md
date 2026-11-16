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
  Tier 2 preset with a full ring, uses under 10% of it, at the price 0.5 (909,608) and at a price
  of 2,000,000 (913,706; Plan 10 re-measure, 2026-11-16, at the end; Plan 9's figures were 884,567
  and 888,662, see "Price and cost" in the Tier 2 section).
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

Measured 2026-11-12 by `harness/tests/cycles_tier2.rs`
(`cargo test -p harness --test cycles_tier2 -- --nocapture`). Scenario: `World::new_tier2` under
`RiskArgs::recommended()` (30 min window, 5 min spacing, `min_obs` 3, 20 % breaker, 1 h stale
window), feed max age 600_000 ms, the slice's amounts (collateral 500_000, debt 600_000, redemption
price and oracle 0.5). The ring holds 8 samples before every row, the worst case for ring fill
(ruling R9-10). `Liquidate` and a partial `Bid` are measured again at a price of 2,000,000 (below).
Each row runs at least 1 s after the newest sample, because a sample at `now` has no weight.
`ObserveFeed` is measured on a fresh publish, so it evicts the oldest sample. `SetPauseLevel` sets
level 1. Budgets are asserted by the test. The R9-13 e2e values are not used here: R9-3 refuses
them at this world's feed max age.

| Instruction | Executions | Cycles | Fee at base fee 8 | Budget | Spec §4 estimate |
|---|---|---|---|---|---|
| Liquidate | 29 | 884,567 | 7,076,536 | <= 1,000,000 | 920,000-980,000 |
| Bid (partial) | 15 | 431,804 | 3,454,432 | <= 520,000 | 470,000-490,000 |
| Bid (final) | 21 | 606,596 | 4,852,768 | <= 720,000 | 650,000-690,000 |
| ObserveFeed | 7 | 182,074 | 1,456,592 | gas cap | ~200,000 |
| SetCollateral | 11 | 359,182 | 2,873,456 | gas cap | ~350,000 |
| SetPauseLevel | 12 | 360,206 | 2,881,648 | <= 400,000 (R9-10) | ~330,000 |

At a price of 2,000,000 (`tier2_rows_fit_the_budget_at_a_high_price`, same preset and ring):

| Instruction | Executions | Cycles | Fee at base fee 8 | Budget |
|---|---|---|---|---|
| Liquidate | 29 | 888,662 | 7,109,296 | <= 1,000,000 |
| Bid (partial) | 15 | 434,981 | 3,479,848 | <= 520,000 |

- Every budgeted row fits. `Liquidate`, `Bid (partial)`, `Bid (final)` and `ObserveFeed` come in
  under the spec's estimates.
- **Price and cost.** `risk_core::math` has two native routes: `x · y / d` when the product fits a u128, and otherwise `(x / d) · y + (x mod d) · y / d` when that second product fits
  (`risk_core/src/math.rs`). Only a product that fits neither way calls the frozen 128-step
  division, about 10,000 cycles a call (spike S4). A TWAP term is `p · w / Σw` with both weights
  below 2^64, so the second route always applies: no TWAP term takes the frozen division at any
  price (`a_twap_term_never_takes_the_frozen_path`). The same holds for every call that divides by
  10,000 (the confidence limit, the sanity bounds, `edge`, the anchor, the deviation test).
  - Measured: at a price of 2,000,000, where `p · w` is past u128 for every term of full weight,
    `Liquidate` costs 888,662 (4,095 more than at 0.5) and a partial `Bid` 434,981
    (3,177 more). Both fit their budgets, and the test also requires each to cost at most 20,000
    more than at 0.5, which a TWAP back on the frozen division (about 60,000) would break. The
    world for that row raises the mock host's minimum collateral ratio, because at that price the host's safety rule admits no other lot
    that outlasts the keeper reward (`HIGH` in `cycles_tier2.rs`).
  - Not measured: `Bid (final)` at a high price. The mock host admits no bid that reaches the
    target there. It prices through the same `bid_pricing` call as the partial bid.
  - Still on the frozen division at every price, so already inside the figures above: the band
    clamp and the settlement arithmetic in `auction_core`, and the stale cap that `Liquidate`
    stores with D4 on (both operands and the divisor are RAY-scale).
  - One call can still switch with magnitude: `conf_bps = ceil(c · 10_000 / M)` takes the frozen
    division when the confidence `c`, as a price, is about 34,000,000 or more. That is one
    division against a headroom of 111,338 (`Liquidate`), 85,019 (partial bid) and
    113,404 (final bid). It has not been measured.
  - Fallback (D4) bids are not in this table. Their floored price takes one frozen division.
  - History: before `risk_core::math` existed every multiply-divide took the frozen division
    (`Liquidate` 1,012,946, `Bid (partial)` 561,253, `Bid (final)` 736,045, `SetPauseLevel`
    407,582; ADR-070). With the first route only (2026-11-11) the rows were 883,636, 431,669,
    606,461 and 360,079, and TWAP terms went back to the frozen division above a price of about
    567,000 in this world.
- `SetCollateral` (+9,048) and `SetPauseLevel` (+30,079) are above their estimates, which are not
  assertions. `SetPauseLevel` is 3.6 % of the cap: R9-10 set its assertion at 4 %, because §4's
  "~330,000" and "< 3 %" contradict each other.
- **Ring fill:** a partial bid with `twap_min_obs` 1 costs 415,407 cycles with 1 sample and 431,713
  with 8. That spread of 16,306 is under the asserted 40,000
  (`bid_cost_is_independent_of_ring_fill`).
- **Over the legacy preset** (re-measured in the same run: `Liquidate` 845,785,
  `Bid (partial)` 410,912, `Bid (final)` 582,225, `SetCollateral`
  321,031), the full ring and the recommended preset add 38,782 to `Liquidate`,
  20,892 to `Bid (partial)`, 24,371 to `Bid (final)` and 38,151 to `SetCollateral`.
  The slice table above keeps its 2026-11-11 figures; the second route moved those rows by at
  most 140 cycles.
- **Executions.** `SetPauseLevel` is 12: the admin's proxy; the seeded `Probe { Config }`;
  `SetPauseLevel` at the config; the seeded `Probe { Collateral }`; `PauseLevelUpdate` at the
  collateral actor; then the Observe block (`Observe`, the host's and the oracle's `ReadState` and
  their replies, the clock's `AssertTimestamp`, `Proceed`). The TWAP and the level checks add
  computation, not messages, so the other rows keep their slice and system counts.

## Plan 10 re-measure (2026-11-16)

Re-measured during Plan 10 Task 8 (the five engine parameters in `ConfigExt.engine` and
`SystemCtx.ext`, the three grace fields appended to `CollateralExt`, the pinned check and the grace
rule on every observation) by `cargo test -p harness --test cycles --test cycles_tier2 --
--nocapture`, same scenarios as above. "Before" is the figure in the tables above (2026-11-11 for
the slice and system rows, 2026-11-12 for Tier 2). No row gained an execution, every budget still
holds, and the budgets and `HIGH_PRICE_EXTRA` are unchanged. The growth is decode and commit of the
longer collateral, config and context states, and the grace and pinned checks.

Slice (legacy preset):

| Instruction | Executions | Cycles (2026-11-16) | Fee at base fee 8 | Before | Delta |
|---|---|---|---|---|---|
| Init | 9 | 273,602 | 2,188,816 | 268,510 | +5,092 |
| SetCollateral | 11 | 334,872 | 2,678,976 | 321,027 | +13,845 |
| Liquidate | 29 | 870,879 | 6,967,032 | 845,645 | +25,234 |
| Bid (partial) | 15 | 419,829 | 3,358,632 | 410,776 | +9,053 |
| Bid (final) | 21 | 595,312 | 4,762,496 | 582,089 | +13,223 |
| ClaimLeftover | 6 | 138,105 | 1,104,840 | 137,873 | +232 |
| SettleEmpty | 6 | 217,684 | 1,741,472 | 212,506 | +5,178 |

System:

| Instruction | Executions | Cycles (2026-11-16) | Fee at base fee 8 | Before | Delta |
|---|---|---|---|---|---|
| SetEngineParams | 6 | 204,961 | 1,639,688 | 197,182 | +7,779 |
| SetSlack | 5 | 166,099 | 1,328,792 | 159,059 | +7,040 |
| ObserveFeed | 7 | 178,246 | 1,425,968 | 169,522 | +8,724 |
| SettleDebt | 8 | 211,680 | 1,693,440 | 207,540 | +4,140 |
| StartSurplus | 10 | 258,912 | 2,071,296 | 255,707 | +3,205 |
| TakeSurplus (partial) | 11 | 244,694 | 1,957,552 | 243,219 | +1,475 |
| TakeSurplus (final) | 11 | 246,923 | 1,975,384 | 245,448 | +1,475 |
| CloseSurplus | 11 | 235,794 | 1,886,352 | 234,480 | +1,314 |
| StartDebt | 7 | 221,946 | 1,775,568 | 218,152 | +3,794 |
| TakeDebt | 11 | 274,546 | 2,196,368 | 271,714 | +2,832 |
| CloseDebt | 5 | 113,130 | 905,040 | 111,562 | +1,568 |
| CheckSolvency (Healthy, burns the buffer) | 9 | 221,888 | 1,775,104 | 219,264 | +2,624 |
| CheckSolvency (Freeze) | 8 | 241,523 | 1,932,184 | 237,436 | +4,087 |

Tier 2, `RiskArgs::recommended()` (grace included; no grace is active when the rows run), ring
full (only the ring-fill rows use `recommended_no_grace()`):

| Instruction | Executions | Cycles (2026-11-16) | Fee at base fee 8 | Before | Delta | Budget |
|---|---|---|---|---|---|---|
| Liquidate | 29 | 909,608 | 7,276,864 | 884,567 | +25,041 | <= 1,000,000 |
| Bid (partial) | 15 | 440,420 | 3,523,360 | 431,804 | +8,616 | <= 520,000 |
| Bid (final) | 21 | 619,344 | 4,954,752 | 606,596 | +12,748 | <= 720,000 |
| ObserveFeed | 7 | 190,322 | 1,522,576 | 182,074 | +8,248 | gas cap |
| SetCollateral | 11 | 374,644 | 2,997,152 | 359,182 | +15,462 | gas cap |
| SetPauseLevel | 12 | 375,448 | 3,003,584 | 360,206 | +15,242 | <= 400,000 (R9-10) |

At a price of 2,000,000:

| Instruction | Executions | Cycles (2026-11-16) | Fee at base fee 8 | Before | Delta | Budget |
|---|---|---|---|---|---|---|
| Liquidate | 29 | 913,706 | 7,309,648 | 888,662 | +25,044 | <= 1,000,000 |
| Bid (partial) | 15 | 443,600 | 3,548,800 | 434,981 | +8,619 | <= 520,000 |

- `Liquidate` at 2,000,000 costs 4,098 more than at 0.5 (`HIGH_PRICE_EXTRA` 20,000) and a
  partial `Bid` 3,180 more.
- **Grace configured, not active** (`liquidate_with_grace_configured_but_inactive_fits_the_budget`):
  `Liquidate` under the full `RiskArgs::recommended()` (grace 30 min after an outage of at least
  10 min) in the gap-free `World::new_tier2_live`, ring full, `grace_until_ms == 0` asserted before
  measuring: 29 executions, 909,517 cycles (fee 7,276,136), against the `Liquidate` budget of
  1,000,000. The Tier 2 `Liquidate` row above runs the same preset in `World::new_tier2`; the
  two worlds' histories differ, and the rows by 91 cycles.
- **Ring fill:** a partial bid with `twap_min_obs` 1 costs 423,308 cycles with 1 sample and 440,327
  with 8, a spread of 17,019 (asserted at most 40,000).
- The most expensive row, `Liquidate` at a price of 2,000,000, is 9.1 % of the gas cap.
