# v04 — the auction engine on LEZ v0.4.0-rc1

A separate cargo workspace (the root one, on LEZ v0.2.4, lists `v04` in `exclude`). Design:
[`docs/v04-port-design.md`](../docs/v04-port-design.md); plans: Plan 5 (the liquidation slice), Plan 6
(holdings per owner and definition, system auctions and solvency); decisions: ADR-056 to ADR-063 in
[`docs/design-log.md`](../docs/design-log.md).

| Path | What |
|---|---|
| `programs/methods` | the guest programs, embedded as `<NAME>_ELF` / `<NAME>_ID` by `risc0_build` |
| `programs/methods/guest` | the guest crate (its own workspace, built for `riscv32im-risc0-zkvm-elf`): engine, token, oracle, mock host, relay, test guests |
| `programs/{token,oracle,mock_host}_core` | the message and state types those guests speak |
| `engine_core` | the engine's messages, actor addresses, states and pure decision code (calls into `crates/auction_core`) |
| `harness` | `harness::Chain`: an in-process `lee::V03State` with our guests and the clock program, signed sends, simulation (`cycles`) and harness-set time (`set_now_ms`); `World`, the deployed token, oracle, mock host and engine the tests share |
| `sdk` | plans per instruction (`builder`: the slice; `system`: the system instructions), `simulate_with` over any state source, `fit_gas`, and `simulate` / `send` / `confirm` over a live sequencer |
| `sdk/examples/slice.rs` | the slice end to end on a live sequencer |
| `sdk/examples/system.rs` | `settle_debt`, one debt auction, one surplus auction and `check_solvency` on a live sequencer |
| `scripts/e2e.sh` | standalone sequencer, deploy, run both examples |
| `CYCLES.md` | cycles and fees per instruction |

## Build and test

Needs Rust 1.98.1 (pinned in `rust-toolchain.toml`, as LEZ v0.4.0-rc1) and the risc0 toolchain
(rzup: r0vm 3.0.5, the components of `.github/actions/install-risc0`) for the guests; `risc0_build`
compiles them with the local toolchain, no Docker. librocksdb-sys (via LEZ) needs libclang; the
wallet crate links libpcsclite.

```bash
cd v04
cargo test --workspace                                         # builds the guests, then runs everything in-process
RISC0_SKIP_BUILD=1 cargo clippy --workspace --all-targets -- -D warnings   # ruling R-2b
cargo test -p harness --test cycles -- --nocapture             # the CYCLES.md table
```

Memory-constrained machines: `CARGO_BUILD_JOBS=2`. The first build compiles LEZ and takes 15-20 minutes with two jobs. `Cargo.lock` (and the guest's
`programs/methods/guest/Cargo.lock`) started from LEZ v0.4.0-rc1's own lockfile, so crate versions
match the tag's.

CI: `.github/workflows/v04.yml` runs fmt, clippy and `cargo test --workspace` on changes under `v04/`,
`crates/auction_core/` or `crates/auction_engine_params/`. The e2e below stays local, like the v0.2.4
localnet story: it needs a LEZ build and about 25 minutes of 15 s blocks.

## End to end on a standalone sequencer

```bash
source ~/lez-spikes/s1/env.sh   # wherever libclang / libpcsclite come from (spike S1)
v04/scripts/e2e.sh              # or in stages: up, deploy, slice, system, down
```

`e2e.sh` builds (or reuses) LEZ v0.4.0-rc1's `sequencer_service` (standalone) and `wallet`, builds the
guests and the example, sets up a fresh sequencer home from `lez/sequencer/service/networks/local.json`,
imports the genesis-funded public dev key A as the fee payer, deploys token, oracle, mock host, relay and
engine with `wallet program-loader deploy` (segment count from a dry run, spike S8), runs
`cargo run -p sdk --example slice`, then `cargo run -p sdk --example system`, and stops the sequencer with
`pkill -x sequencer_servi`. It exits non-zero on any failed step and expects `slice complete` and
`system complete`. The `system` stage needs the `up` and `deploy` stages (it reads
`$E2E_HOME/programs.env`) but not the slice: the system example derives its keys from `RUN_TAG` + `/system`,
so it creates its own token definitions, oracle, mock-host params and engine config on the same deployed
programs.

`SLACK_MS` (the config's clock slack, ADR-058) defaults to 60 s in the example; the in-process harness uses
6 s (`harness::world::SLACK_MS`). A 15 s-block sequencer needs at least 60 s, since a claim must lie within
`slack` of a clock that only moves once per block.

The example sends every transaction through the SDK (simulate, `gas_limit = ceil(1.2 × cycles)`,
`max_fee` at the next block's price ceiling, send, confirm by the block's `reverted` list) and prints
each hash, the simulated cycles, `gas_limit` and the fee actually charged (the payer's balance before
and after). It asserts the auction is `Settled` after the final bid and the auction vault is empty after
the leftover claim.

### Recorded run (2026-11-01, LEZ v0.4.0-rc1, `networks/local.json`, base fees 8 / 8)

Two runs (one in stages, one `e2e.sh` end to end, each on a fresh sequencer) gave identical cycles,
gas limits and fees. Engine instructions of the staged run (the other 13 transactions are the token, oracle and mock-host setup;
all 20 are in the task report). Every transaction applied first time; the fee charged
matched `cycles × 8 + wire bytes × 8` in the recorded runs (the example asserts fee ≤ `max_fee`).

| Instruction | Block | Simulated cycles | CYCLES.md | gas_limit | Fee charged |
|---|---|---|---|---|---|
| Init | 63 | 178,715 | 178,715 | 214,458 | 1,439,688 |
| SetCollateral | 64 | 309,596 | 309,596 | 371,516 | 2,485,352 |
| Liquidate | 71 | 772,290 | 772,287 | 926,748 | 6,189,968 |
| Bid (partial) | 75 | 363,526 | 363,522 | 436,232 | 2,919,856 |
| Bid (final) | 76 | 498,376 | 498,372 | 598,052 | 3,998,656 |
| ClaimLeftover | 77 | 109,917 | 109,917 | 131,901 | 886,048 |

End state: auction `Settled`, debt raised 600,000, penalty 60,000, collateral sold 337,506, leftover
162,469 claimed by the owner, auction vault 0. The 3-4 extra cycles on the time-dependent instructions
come from the claimed time (epoch milliseconds on the sequencer, 1,000,000 in the harness), not from
storage. 20 transactions cost 22,262,464 in fees. `SettleEmpty` is covered in-process only.

### Recorded system run (2026-11-04, same setup, `e2e.sh all` on a fresh sequencer)

The slice ran first (20 transactions, `slice complete`; engine cycles within 21 of CYCLES.md's slice rows),
then `examples/system.rs`: 27 transactions, every one applied first time, `system complete`, 31,670,544 in
fees. Engine instructions (the other 13 are token, oracle and mock-host setup, the donations included; all
27 are in the Task 9 report):

| Instruction | Block | Simulated cycles | CYCLES.md | gas_limit | Fee charged |
|---|---|---|---|---|---|
| Init (protocol token, genesis) | 75 | 242,292 | — (no token: 214,766) | 290,751 | 1,949,728 |
| SetCollateral | 76 | 325,819 | 323,949 | 390,983 | 2,615,136 |
| Liquidate (lot 3) | 81 | 841,174 | 839,357 | 1,009,409 | 6,741,544 |
| SettleEmpty | 82 | 210,141 | 208,291 | 252,170 | 1,686,432 |
| StartDebt (buffer holding absent, `Pda` evidence) | 86 | 217,757 | 217,756 | 261,309 | 1,749,224 |
| TakeDebt (whole lot, 500) | 87 | 263,328 | 271,617 | 315,994 | 2,116,216 |
| SettleDebt (`u128::MAX`, burns 9,500) | 92 | 210,647 | 206,767 (burns 100) | 252,777 | 1,690,800 |
| StartSurplus | 94 | 255,298 | 255,298 | 306,358 | 2,049,032 |
| TakeSurplus (whole lot, 500) | 95 | 236,967 | 243,063 / 245,296 (two takes of 250) | 284,361 | 1,905,840 |
| CheckSolvency (`Healthy`, no burn) | 96 | 177,785 | — (with a burn: 218,253) | 213,342 | 1,427,840 |

End state: shortfall `U` = 10,000 booked by the empty lot-3 auction; the debt take burned 500 stablecoin and
minted 325 protocol tokens (30 % growth after more than a second); 9,500 donated and settled, `unbacked_debt`
0; 1,500 donated, the surplus lot of 500 taken for 200 protocol tokens; both auctions `Ended`, host not frozen,
protocol supply 10,125 = genesis 10,000 + 325 − 200, all held by the taker. Rows whose scenario matches
CYCLES.md (`StartDebt`, `StartSurplus`) agree to the cycle. The others differ by scenario (likely causes, not
measured one by one: a config with the protocol token is larger to decode (`SetCollateral`, `Liquidate`,
`SettleEmpty`, about +1,850), a bigger burn (`SettleDebt`), one whole-lot take instead of a partial one
(`TakeSurplus`), and a taker whose holdings differ from the harness's (`TakeDebt`)). Every system transaction
stays under 3 % of the cap.

## What this slice proves

- The design note's conversation model carries the engine's liquidation path on LEZ 0.4: `Init`,
  `SetCollateral`, `Liquidate`, partial and final `Bid`, `SettleEmpty` and `ClaimLeftover` run as actor
  conversations against our token, a minimal oracle and a mock CDP host, in-process and on a sequencer.
- Authorization holds by whole-actor `from` checks, with no authority PDAs (ADR-056, ADR-057); the ported
  security tests cover impostor programs, attacker-account engine actors, a rogue config, replies outside a
  pending phase and a host that never replies.
- Claimed time against the clock (ADR-058), one bid message (ADR-059) and the host's `Seized` report
  (ADR-060) keep R6 and the reservation invariants after every step.
- Every instruction fits far below the 10M gas cap (`Liquidate`, the largest, under 8 %; CYCLES.md), and
  simulate-then-send predicts what the sequencer charges (ADR-061).

## What Plan 6 proves

- **Junk tokens cannot block a debit.** A holding is one token actor per `(owner, definition)` (ADR-062), so
  junk credited to a host vault, an auction vault or the buffer leaves liquidation and bid costs unchanged,
  and no credit can pre-block a definition address.
- **The rest of the first milestone runs on 0.4** (design note §8): `set_engine_params`, `observe_feed`,
  `settle_debt`, surplus and debt auctions (start, take, close) and `check_solvency` with the host freeze, as
  conversations rooted at the config with one Read block at the accounting (ADR-063). The protocol token is
  the engine's PDA definition, created at `Init` with the config as minter; a debt take mints through it.
- **Invariants and security carry over.** R6, the reservation invariants and the new system-auction
  invariants hold after every step; impostor, rogue-config, forged-message and read-window tests cover the
  new internal message (`harness/tests/security_system.rs`, `security_surplus.rs`).
- **Keepers are not griefable cheaply.** SDK system plans declare the actors a run may touch (ruling R-8b),
  so a 1-unit buffer donation cannot revert a pending `CheckSolvency` or start; every system op fits under 3 %
  of the gas cap (CYCLES.md).
- **On a sequencer.** `examples/system.rs` runs `settle_debt`, one debt auction (started while the buffer
  holding's account is absent, with its `Pda` evidence), one surplus auction and `check_solvency` on the
  standalone sequencer (recorded run below).

## Deferred

- **The IDL.** The hand-kept IDL with a drift test waits for SPEL on 0.4: a drift test over 45 borsh variants
  needs a schema derive the crates do not have.
- **A `slack_ms` setter.** `slack_ms` is set at `Init` only (ADR-058); a wrong value means a new config.
- **Host `ProtocolParameters` validation at `Init`** (ruling R-7c): `Init` does not yet read the host's
  parameters to check the stablecoin definition and oracle.
- **Native collateral.** Collateral is a `T` token; native collateral would need Read + exact `Transfer`
  instead of `Sweep` (design note §6).
- **The SDK scanner** (design note §7) and CI for the e2e (it stays local).

## Time in-process

`V03State` has no block producer, so nothing ticks the clock program. `Chain::set_now_ms(ms)` does
what the sequencer's per-block tick leaves behind: it writes `ClockAccountData { block_id, timestamp: ms }`
into the clock program's actor state on all three clock accounts, and later sends run at timestamp
`ms`. `Chain::clock()` is the every-block clock actor that `AssertTimestamp` reads.
