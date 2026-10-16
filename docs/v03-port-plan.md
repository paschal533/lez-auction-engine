# Port plan: LEZ v0.3

Status: a plan, not started. The PoC targets LEZ v0.2.4 and SPEL `1ef0500f` (ADR-001).

## Why not now

v0.3 needs SPEL support, which does not exist yet (spec §3.1). Porting before it exists would mean writing
the program layer against an API that has no released toolchain. The testnet deployment (RFP S1) waits on
the same thing.

## What stays

- `auction_core`: pure, `no_std` and LEZ-free, so it does not change.
- The SDK's quote and plan logic, which calls the same `auction_core` functions as the program (ADR-040).

## What changes

- **The program layer** moves to the SPEL v0.3 API: the `auction_engine` guest and the stablecoin hooks
  (ADR-025, ADR-026), and the vendored host's own port.
- **Confirmation.** v0.3's `poll_transaction_status` replaces the nonce polling of the SDK (ADR-042).
- **Discovery.** The indexer's `getTransactionsByAccount`, and then events, replace the block scan (ADR-043).
- **The encoder seam** (ADR-040): the SDK's own enum-index encoders stay the single place that builds
  instruction data, so the transport and the signing path change behind them.

## Order of work

1. Port the `auction_core` callers.
2. Port the programs (engine, then the hook patch series against the host's v0.3 release).
3. Port the SDK seams (confirmation, discovery, encoders).
4. Port the CLI.
5. Re-measure `CYCLES.md` and repeat the real proofs (ADR-045), then regenerate the ImageIDs and
   `DEPLOYMENTS.md` (ADR-047).

## Risks

- API churn between the v0.3 release candidates and the final release.
- The host stablecoin's own port: the hook patch series applies to 64c01210 only, and a new upstream base is a
  new vendor commit followed by `git am` of the series (ADR-025).
