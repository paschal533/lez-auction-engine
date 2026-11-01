# v04 — the auction engine on LEZ v0.4.0-rc1

A separate cargo workspace (the root one, on LEZ v0.2.4, lists `v04` in `exclude`). Design:
[`docs/v04-port-design.md`](../docs/v04-port-design.md); plan: Plan 5 (the liquidation slice);
decisions: ADR-056 to ADR-061 in [`docs/design-log.md`](../docs/design-log.md).

| Path | What |
|---|---|
| `programs/methods` | the guest programs, embedded as `<NAME>_ELF` / `<NAME>_ID` by `risc0_build` |
| `programs/methods/guest` | the guest crate (its own workspace, built for `riscv32im-risc0-zkvm-elf`): engine, token, oracle, mock host, relay, test guests |
| `programs/{token,oracle,mock_host}_core` | the message and state types those guests speak |
| `engine_core` | the engine's messages, actor addresses, states and pure decision code (calls into `crates/auction_core`) |
| `harness` | `harness::Chain`: an in-process `lee::V03State` with our guests and the clock program, signed sends, simulation (`cycles`) and harness-set time (`set_now_ms`); `World`, the deployed token, oracle, mock host and engine the tests share |
| `sdk` | plans per instruction, `simulate_with` over any state source, `fit_gas`, and `simulate` / `send` / `confirm` over a live sequencer |
| `sdk/examples/slice.rs` | the slice end to end on a live sequencer |
| `scripts/e2e.sh` | standalone sequencer, deploy, run the example |
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
localnet story: it needs a LEZ build and about 15 minutes of 15 s blocks.

## End to end on a standalone sequencer

```bash
source ~/lez-spikes/s1/env.sh   # wherever libclang / libpcsclite come from (spike S1)
v04/scripts/e2e.sh              # or in stages: up, deploy, slice, down
```

`e2e.sh` builds (or reuses) LEZ v0.4.0-rc1's `sequencer_service` (standalone) and `wallet`, builds the
guests and the example, sets up a fresh sequencer home from `lez/sequencer/service/networks/local.json`,
imports the genesis-funded public dev key A as the fee payer, deploys token, oracle, mock host, relay and
engine with `wallet program-loader deploy` (segment count from a dry run, spike S8), runs
`cargo run -p sdk --example slice`, and stops the sequencer with `pkill -x sequencer_servi`. It exits
non-zero on any failed step and expects `slice complete`.

The example sends every transaction through the SDK (simulate, `gas_limit = ceil(1.2 × cycles)`,
`max_fee` at the next block's price ceiling, send, confirm by the block's `reverted` list) and prints
each hash, the simulated cycles, `gas_limit` and the fee actually charged (the payer's balance before
and after). It asserts the auction is `Settled` after the final bid and the auction vault is empty after
the leftover claim.

### Recorded run (2026-11-01, LEZ v0.4.0-rc1, `networks/local.json`, base fees 8 / 8)

Two runs (one in stages, one `e2e.sh` end to end, each on a fresh sequencer) gave identical cycles,
gas limits and fees. Engine instructions of the staged run (the other 13 transactions are the token, oracle and mock-host setup;
all 20 are in the task report). Every transaction applied first time; the fee charged equals
`cycles × 8 + wire bytes × 8` exactly, i.e. the simulation predicted settlement.

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

## What Plan 6 adds

`settle_debt`, surplus auctions (`start_surplus_auction`, `take_surplus`, `close_surplus_auction`), debt
auctions (`start_debt_auction`, `take_debt`, `close_debt_auction`), `check_solvency` with the host freeze,
`observe_feed`, the admin setters (`set_engine_params`), `Init`'s validation of the host's
`ProtocolParameters` and the protocol-token and reference-oracle fields (ruling R-7c), the SDK scanner, and
the hand-kept IDL.

## Time in-process

`V03State` has no block producer, so nothing ticks the clock program. `Chain::set_now_ms(ms)` does
what the sequencer's per-block tick leaves behind: it writes `ClockAccountData { block_id, timestamp: ms }`
into the clock program's actor state on all three clock accounts, and later sends run at timestamp
`ms`. `Chain::clock()` is the every-block clock actor that `AssertTimestamp` reads.
