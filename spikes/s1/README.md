# Spike S1: toolchain, sequencer, second host build, chained-call count

Throwaway spike code. The pinned sources are NOT in this repo: they were cloned and built in the
WSL-native filesystem at `~/lez-spikes/s1/` (the repo lives on `/mnt/c`, which is slow and has CRLF problems).
The full write-up is `docs/spikes/S1.md`; the design decision is ADR-018 in `docs/design-log.md`.

| File | What it is |
|---|---|
| `run.sh` | Runbook of the verified commands, one function per brief step (`./run.sh step3_sequencer`). Uses the WSL paths. |
| `global_pdas.rs` | Helper example: prints the five stablecoin global PDAs for a ProgramId. Copy to `lez-programs/programs/stablecoin/examples/`. |
| `inst-b01.patch` | The second-host-build change: `#[used] static HOST_INSTANCE_TAG: [u8; 8] = *b"INST-B01";` in the stablecoin guest. Apply to a scratch clone of lez-programs at the pinned SHA. |
| `chain_count.rs` | Appended to a copy of `lez-programs/programs/integration_tests/tests/stablecoin.rs` (reuses its fixtures); counts program executions and executor cycles per chained call. |

Pins (unchanged): logos-execution-zone `47eba256479f6f785acbd138834340703cd03401` (tag v0.2.4),
lez-programs `64c01210adecd6b557dbd021a3cc94553f243fd8`, risc0-zkvm `=3.0.5`,
spel `1ef0500f8fc8ce3ddf95523726ae159db83f744f` (the rev the stablecoin guest pins).
