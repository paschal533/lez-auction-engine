# Draft pull-request description (for logos-blockchain/lez-programs, stablecoin hooks)

**Status:** DRAFT. Not offered: no pull request has been opened and nothing has been sent upstream. The author
decides whether and where to open it, and edits this text first. The section references below point into this
repository (`lez-auction-engine`); rewrite them as links or inline text for an upstream reader.

---

**Title:** stablecoin: liquidation-engine hooks (`set_liquidation_engine`, `liquidate_position`, `engine_freeze_new_debt`)

## Summary

Three instructions are added to the stablecoin so that a separate, permissionless auction engine can liquidate
unsafe positions and freeze new debt, without the stablecoin knowing anything about auctions. They are
**appended** at the end of both `stablecoin_core::Instruction` and the guest function list, so for the new
instructions the enum index equals the IDL index (19, 20, 21). A test, `hook_instructions_share_enum_and_idl_index`,
checks this. (The existing instructions at IDL positions 8-18 do not share their enum index; see the separate
spel issue draft. The patches do not touch them.)

| # | Instruction | Caller | Effect |
|---|---|---|---|
| 19 | `set_liquidation_engine(engine_program_id, engine_authority_id)` | host admin, top-level only | creates or rotates `LiquidationEngineState` |
| 20 | `liquidate_position(collateral_to_seize, normalized_debt_to_cover)` | the bound engine | seizes collateral from an unsafe position into the engine's custody holding |
| 21 | `engine_freeze_new_debt()` | the bound engine | sets the existing `ProtocolParameters.is_frozen` |

## Authentication

The engine-only hooks require both that the immediate caller program id is the bound engine program and that
the bound engine-authority PDA is authorised in the transaction (Variant C; `docs/architecture/hooks.md`
"Binding and authentication", ADR-019, ADR-026). The host stores both ids in `LiquidationEngineState`
(a PDA under the stablecoin program id; its seed is risc0's `sha::Impl::hash_bytes`, i.e. SHA-256, of the
32-byte domain `STABLECOIN__LIQUIDATION_ENGINE__`); it does not need to know the
engine's seed scheme. `set_liquidation_engine` rejects zero ids.

`set_liquidation_engine` is **top-level only** (`caller_program_id == 0`). The existing admin and freeze checks
are plain `is_authorized` assertions, which an admin PDA could have laundered through another program; the new
entry point closes that for itself and leaves the existing instructions unchanged. Consequence for review: a
host whose admin is a PDA or a multisig program cannot call `set_liquidation_engine`, so it cannot bind an
engine. Admin and freeze authorities must stay user keys for the existing instructions (ADR-026).

## What the bound engine can take

`liquidate_position` is the final interface shape for partial liquidation. The host checks, in order: the
caller guard; the protocol parameters, accumulator and redemption-price state; the clock; the configured
oracle is fresh; the position is the one derived from its own fields; debt is non-zero;
`0 < cover <= normalized_debt` and `0 < seize <= collateral`; the vault and custody holding match; and the
position is **not** collateralised at `minimum_collateralization_ratio` (the same exact U512 comparison as
`withdraw_collateral`).

**Proportional bound.** The host itself enforces, in U512,
`collateral_to_seize <= min(C, floor(C * cover / N * 1.25))`, with `MAX_PENALTY_BPS = 2500` a host constant. A
correct or buggy engine therefore cannot take more than its share of the collateral plus the maximum penalty.
The bound rounds **down** (ADR-026 amendment): a ceiling would let every call take at least one unit however
small `cover` is, so repeated tiny partial liquidations could drain a position while clearing almost no debt.
Full seizure is unaffected (`floor(1.25 * C) >= C`, capped at `C`).

Full seizure zeroes the position and moves the whole vault balance (donations included) to custody; partial
seizure moves exactly `collateral_to_seize`.

## Freeze

`engine_freeze_new_debt` sets the existing `is_frozen` flag and nothing else, so no existing instruction's
account list changes. Only the freeze authority's existing `unfreeze` clears it; the engine cannot unfreeze.

## Discussion points

1. **The freeze is broader than "pause new debt generation".** The host's `is_frozen` also blocks
   `open_position` and `withdraw_collateral`. If a narrower flag is preferred, the hook can set that instead
   (`docs/architecture/hooks.md` "Freeze semantics"; spec 5.10).
2. **Liquidation is allowed while the host is frozen**, because it reduces risk. This needs a maintainer's
   confirmation (spec 5.10). Related: there is no separate liquidation ratio; liquidation uses
   `minimum_collateralization_ratio`.
3. **`set_liquidation_engine` is top-level only**, as described above; a PDA or multisig host admin cannot bind.
4. **IDL.** The upstream PR must regenerate `artifacts/stablecoin-idl.json` with the three new instructions;
   our test looks for the IDL at `idl/stablecoin-idl.json` first, then at the upstream path.

## Tests

`scripts/check-hook-patches.sh` (in `lez-auction-engine`) checks only that the series applies with `git am` to a
pristine checkout of lez-programs 64c01210 and that the patched tree equals our vendored tree; it does not run
any tests. We have not run the upstream test suite against the patched tree as a separate step; our vendored
copy of the stablecoin tests runs as part of our own CI. The series adds code and new tests and does not edit any
existing test. The one change to existing upstream code is in `checks.rs` (patch 0003): the body of
`assert_position_is_collateralized` moves into a new `is_position_collateralized` returning a bool, and the
assert now calls it. The comparison is unchanged and its callers (`generate_debt`, `withdraw_collateral`) are unchanged. The hook tests live in `programs/stablecoin` (binding, the proportional bound and its rounding,
health at the boundary, freeze); the attack cases are listed in `THREAT_MODEL.md` "Authentication of the
engine", and the engine-side tests are named in `REQUIREMENTS.md` F7.

## How to apply

On a checkout of lez-programs at `64c01210adecd6b557dbd021a3cc94553f243fd8`:

    git am patches/stablecoin/*.patch

The series is four patches (`patches/stablecoin/`): the binding and freeze, a test-setter and IDL-path fix,
`liquidate_position`, and the round-down fix. To check a series against pristine upstream, run
`scripts/check-hook-patches.sh` from `lez-auction-engine`.

References in `lez-auction-engine`: `docs/architecture/hooks.md`, ADR-019, ADR-025, ADR-026, `THREAT_MODEL.md`.

<author>
