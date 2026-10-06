# Stablecoin hooks (lez-auction-engine patch series)

Three instructions are appended to the vendored stablecoin (lez-programs `64c01210`), at the end of both `stablecoin_core::Instruction` (indices 19–21) and the guest function list (IDL indices 19–21), so spel-cli's IDL-position encoding and the guest's enum decoding agree (`stablecoin_core` test `hook_instructions_share_enum_and_idl_index`).

| # | Instruction | Who | Effect |
|---|---|---|---|
| 19 | `set_liquidation_engine(engine_program_id, engine_authority_id)` | host admin, top-level only | creates or rotates `LiquidationEngineState` |
| 20 | `liquidate_position(collateral_to_seize, normalized_debt_to_cover)` | bound engine (Variant C) | seizes collateral from an unsafe position into the engine's custody holding |
| 21 | `engine_freeze_new_debt()` | bound engine (Variant C) | sets the existing `ProtocolParameters.is_frozen` |

## Binding and authentication

`LiquidationEngineState { engine_program_id, engine_authority_id }` lives at `SHA-256("STABLECOIN__LIQUIDATION_ENGINE__")` under the stablecoin program id. The engine's authority is the engine PDA `["ENGINE_AUTH", engine_config]`, so a host instance is bound to exactly one engine configuration (ADR-027).

Engine-only hooks require both the immediate `caller_program_id == engine_program_id` and an authorised `engine_authority` (Variant C, ADR-019). The admin hook requires a top-level call (`caller_program_id == 0`): the existing admin and freeze checks (`checks.rs:109`, `freeze.rs:58`) are plain `is_authorized` assertions, which a PDA admin could have laundered through another program; the new admin entry point closes that for itself, and the upstream PR states that admin and freeze authorities must stay user keys for the existing instructions (ADR-026). Consequence: a host whose admin is a PDA or a multisig program cannot call `set_liquidation_engine` (upstream-fit note for the PR).

**Bind-time verification (ADR-027).** Before `set_liquidation_engine`, the host admin reads the engine config it is about to bind (`["CONFIG", admin, salt]` under the engine ImageID) and checks: `admin` is the expected operator; `host_program_id` / `host_protocol_parameters_id` are this host; `token_program_id` is the expected token ImageID; the parameters and the `CollateralConfig` are the agreed ones. Binding hands that config the power to seize unsafe positions and freeze new debt.

## Freeze semantics

`engine_freeze_new_debt` only sets `is_frozen`. Only the freeze authority's existing `unfreeze` clears it. The host freeze also blocks `open_position` and `withdraw_collateral` (`freeze.rs`), which is broader than F4's "pause new debt generation"; the upstream PR asks whether a narrower flag is preferred.

## IDL path in the upstream PR

`hook_instructions_share_enum_and_idl_index` looks for the IDL at `idl/stablecoin-idl.json` (this repo), then `artifacts/stablecoin-idl.json` (upstream). The upstream PR must regenerate `artifacts/stablecoin-idl.json` with the three hook instructions. `assert_engine_call` also rejects a stored zero engine program id defensively (the setter already refuses to store one).

## `liquidate_position`

Checks, in order: Variant C guard; `ProtocolParameters`, accumulator and redemption-price state at their PDAs; clock is `CLOCK_01`; the host oracle is the configured one, not future-dated, and no older than `maximum_oracle_price_age_milliseconds`; the position is initialised, owned by this program and at `compute_position_pda(owner, nonce)` from its own stored fields; debt > 0; `0 < cover <= normalized_debt`, `0 < seize <= collateral`; the proportionality bound `seize <= min(C, ceil(C·cover/N·1.25))` in U512 (`MAX_PENALTY_BPS = 2500`); the vault is the position's PDA vault, holds the collateral definition and at least `collateral_amount`; the custody holding is initialised, owned by the vault's token program and of the collateral definition; the redemption price projects to non-zero; and the position is **not** collateralised at `minimum_collateralization_ratio` (the same exact U512 comparison as `withdraw_collateral`, via `checks::is_position_collateralized`).

Effects: full seizure zeroes the position and moves the whole vault balance (donations included) to custody; partial seizure (milestone M1) subtracts both amounts and moves exactly `collateral_to_seize`, leaving donations for the owner's next `deposit_collateral`. The position account is never released (LEZ cannot release accounts); its owner may `close_position` it once the vault is empty. Allowed while frozen.
