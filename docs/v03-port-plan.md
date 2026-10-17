# Port plan: LEZ v0.3 and the "testnet 0.4" target

Status: a plan, not started. The PoC targets LEZ v0.2.4 and SPEL `1ef0500f` (ADR-001). Upstream facts below
were checked on 2026-10-17.

## Where upstream is

- **LEZ.** v0.3.0 (2026-09-30) and v0.3.1 (2026-10-11) are released; the public testnet
  (`testnet.lez.logos.co`) runs v0.3.1. A legacy testnet, `testnet-legacy.lez.logos.co`, runs v0.2.4
  (lez-programs PR #423), and LEZ tagged `v0.2.4-lez-legacy` (2026-10-13). On `dev`, PR #935 ("transactional
  actors", merged 2026-10-14) replaces v0.3's plan/apply guest model again (`actor_entrypoint!`, `Response`,
  `SyncMessage`); that is expected to become the next release.
- **The RFP target.** Open rfp PR #214 (2026-10-12) changes the supportability requirement of RFP-014 and
  others to "delivered for, deployed and tested on LEZ testnet 0.4". RFP-014 itself is still closed.
- **SPEL.** v0.7.0 (2026-09-17) still targets LEZ v0.2.4; `main` targets v0.2.5-rc2. v0.3 support exists only as
  unreleased work (spel #291). The IDL-order bug of spec §3.2 is fixed on `main` (spel #288, closed; commits
  `d58984a4`, `adb46c66`), after v0.7.0, so no release for v0.2.4 has the fix and our enum-index encoders stay.
- **lez-programs.** `main` is still `64c01210`, the commit we vendor, on LEZ v0.2.4; its v0.3 work is an
  unmerged branch (`chore/upgrade-to-lez-0.3.0-rc`).
- **Public transactions** are still executed, not proved, by the sequencer on v0.3 (LEZ README: validators
  verify proofs only for privacy-preserving transactions), so ADR-045 holds after a port.

## Why not now

A port to v0.3 now would be done twice: the v0.3 guest API (plan/apply, chained calls addressed by deployed
`AccountId`, PDAs from the deployed address, sharded `AccountData`, borsh I/O, fees, Rust 1.98.1) is already
replaced on `dev`, the RFP is moving to testnet 0.4, SPEL has no v0.3 release, and the host stablecoin has no
v0.3 port to rebase the hook series onto. Until 0.4 is released, the interim public-testnet evidence (RFP S1)
is a deployment on the v0.2.4 legacy testnet, which runs the same LEZ as this PoC.

## What stays

- `auction_core`: pure, `no_std` and LEZ-free, so it does not change.
- The SDK's quote and plan logic, which calls the same `auction_core` functions as the program (ADR-040).
- The encoder seam (ADR-040): the SDK's own enum-index encoders stay the single place that builds instruction
  data, so the transport and the signing path change behind them.

## What changes (against the 0.4 actor model, once released)

- **The program layer**: the `auction_engine` guest and the stablecoin hooks (ADR-025, ADR-026) move to the
  released guest API and the matching SPEL release, after the host stablecoin's own port.
- **Chained calls and PDAs**: callee and PDA derivation by deployed address instead of ImageID; the engine's
  authorisation checks (variant C, ADR-024) are re-proved against the new caller model.
- **Accounts and fees**: the account layout, and a fee payer for every transaction the SDK and CLI send.
- **Confirmation**: v0.3's transaction-status polling replaces the nonce polling of the SDK (ADR-042).
- **Discovery**: the indexer and events replace the block scan (ADR-043).
- **Toolchain**: the Rust version LEZ pins (1.98.1 for v0.3).

## Order of work

1. Port the `auction_core` callers.
2. Port the programs (engine, then the hook patch series against the host's release).
3. Port the SDK seams (confirmation, discovery, fees, encoders).
4. Port the CLI.
5. Re-measure `CYCLES.md` and repeat the real proofs (ADR-045), then regenerate the ImageIDs and
   `DEPLOYMENTS.md` (ADR-047).
6. Deploy and test on testnet 0.4 (RFP S1 as rfp #214 words it).

## Watch

LEZ #935 and its follow-ups (#956, #957, #974), spel #291, lez-programs `chore/upgrade-to-lez-0.3.0-rc`, and
rfp #214.

## Risks

- The 0.4 API is not released, and its date is unknown.
- The host stablecoin's own port: the hook patch series applies to 64c01210 only, and a new upstream base is a
  new vendor commit followed by `git am` of the series (ADR-025).
- The legacy testnet is likely temporary.
