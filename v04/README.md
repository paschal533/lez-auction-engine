# v04 — the auction engine on LEZ v0.4.0-rc1

A separate cargo workspace (the root one, on LEZ v0.2.4, lists `v04` in `exclude`). Design:
[`docs/v04-port-design.md`](../docs/v04-port-design.md); plan: Plan 5 (the liquidation slice).

| Path | What |
|---|---|
| `programs/methods` | the guest programs, embedded as `<NAME>_ELF` / `<NAME>_ID` by `risc0_build` |
| `programs/methods/guest` | the guest crate (its own workspace, built for `riscv32im-risc0-zkvm-elf`) |
| `harness` | `harness::Chain`: an in-process `lee::V03State` with our guests and the clock program, signed sends, simulation (`cycles`) and harness-set time (`set_now_ms`) |

## Build

Needs Rust 1.98.1 (pinned in `rust-toolchain.toml`, as LEZ v0.4.0-rc1) and the risc0 toolchain
(`rzup install`) for the guests; `risc0_build` compiles them with the local toolchain, no Docker.
librocksdb-sys (via LEZ) needs libclang.

```bash
cd v04
cargo test -p harness          # builds the guests, then runs them in-process
```

Memory-constrained machines: `CARGO_BUILD_JOBS=2`. The first build compiles LEZ and takes 15-20 minutes with two jobs. `Cargo.lock` (and the guest's
`programs/methods/guest/Cargo.lock`) started from LEZ v0.4.0-rc1's own lockfile, so crate versions
match the tag's.

## Time in-process

`V03State` has no block producer, so nothing ticks the clock program. `Chain::set_now_ms(ms)` does
what the sequencer's per-block tick leaves behind: it writes `ClockAccountData { block_id, timestamp: ms }`
into the clock program's actor state on all three clock accounts, and later sends run at timestamp
`ms`. `Chain::clock()` is the every-block clock actor that `AssertTimestamp` reads.
