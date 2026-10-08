# Engine program (`auction_engine`)

## Layout

- `programs/auction_engine/core` (`auction_engine_core`): shared, guest-safe types. Instruction enum, borsh account states, PDA seed functions, `EngineError`, argument parsing and validation. Depends on `auction_core` (pure math) and `lee_core`; used by the guest, the host-side handlers, tests and the Plan 3 SDK.
- `programs/auction_engine` (`auction_engine_program`): handlers that take `lee_core` accounts and return post-states plus chained calls; pure Rust, unit-testable without a zkVM.
- `programs/auction_engine/methods`: the risc0 guest (one `#[instruction]` function per `Instruction` variant, in enum order) and the host build crate that exposes `AUCTION_ENGINE_ELF` / `AUCTION_ENGINE_ID`.

## Accounts and seeds


Seed encoding (ADR-020, verified in S3): a `literal("X")` is UTF-8 zero-padded to 32 bytes; `account("a")` is the 32-byte account id; a `u64` argument is little-endian zero-padded to 32 bytes; several seeds are combined as `SHA-256(s1 || s2 || ...)`; one seed is used as is. `AccountId::for_public_pda(&program_id, &PdaSeed::new(combined))`.

| Account | Seeds | Owner | Data |
|---|---|---|---|
| `EngineConfig` | `["CONFIG", admin, salt: u64]` (admin = the `init_config` signer; amends spec §5.4 `["CONFIG", salt]`, ADR-027) | engine | `EngineConfigState` |
| `CollateralConfig` | `["COLL", config, collateral_definition]` | engine | `CollateralConfigState` |
| `Accounting` | `["ACCT", config]` | engine | `AccountingState` |
| `SurplusBuffer` | `["BUFFER", config]` | token | `TokenHolding::Fungible` (stablecoin) |
| `LiquidationRecord` | `["LIQREC", config, position]` | engine | `LiquidationRecordState` |
| `Auction` | `["AUCTION", config, position, nonce: u64]` | engine | `AuctionState` |
| `AuctionVault` | `["VAULT", auction]` | token | collateral holding |
| `SurplusAuction` | `["SURP", config, nonce: u64]` | engine | `SurplusAuctionState` |
| `SurplusVault` | `["SURPVAULT", surplus_auction]` | token | stablecoin holding |
| `DebtAuction` | `["DEBT", config, nonce: u64]` | engine | `DebtAuctionState` |
| `EngineAuthority` | `["ENGINE_AUTH", config]` | none (never initialised) | — |
| `MintAuthority` | `["MINT_AUTH", config]` | none (never initialised) | — |
| `UnusedSlot(k)` | `["UNUSED", config, k: u64]` | none (never initialised) | — |

Host (stablecoin) addition: `LiquidationEngineState` at the single seed `SHA-256(b"STABLECOIN__LIQUIDATION_ENGINE__")`, owned by the stablecoin, data `{ engine_program_id: ProgramId, engine_authority_id: AccountId }`.

Optional-account slots (ADR-030): slot 0 = protocol-token reference oracle, slot 1 = protocol-token definition. When the configured id is `None`, the caller passes `UnusedSlot(k)` and the engine asserts that exact id.

## Instructions and account lists


`(s)` = signer, `(m)` = written by the engine or a chained call, `(i)` = must be uninitialised.

| # | Instruction (args) | Accounts in order |
|---|---|---|
| 0 | `init_config(salt, …engine params)` | admin(s), config(i,m), accounting(i,m), buffer(m), protocol_parameters, stablecoin_definition, protocol_token_definition [slot 1], ref_oracle [slot 0] |
| 1 | `set_engine_params(…engine params)` | admin(s), config(m), accounting, coll_config, protocol_token_definition [slot 1], ref_oracle [slot 0] |
| 2 | `set_collateral_config(…collateral params, admin_paused)` | admin(s), config, collateral_definition, coll_config(m), oracle, protocol_parameters, redemption_price_state, clock |
| 3 | `observe_feed()` | caller(s), config, coll_config(m), redemption_price_state, oracle, clock |
| 4 | `liquidate(nonce)` | keeper(s), keeper_collateral_holding(m), config(m), coll_config(m), position(m), record(m), auction(i,m), auction_vault(m), engine_authority, liquidation_engine_state, host_vault(m), collateral_definition, stability_fee_accumulator, redemption_price_state, oracle, protocol_parameters, clock |
| 5 | `bid(coin_in_max, min_collateral_out)` | bidder_stable_holding(s,m), bidder_collateral_holding(m), config, coll_config(m), auction(m), auction_vault(m), buffer(m), stablecoin_definition(m), redemption_price_state, oracle, clock |
| 6 | `bid_with_settlement(coin_in_max, min_collateral_out)` | as `bid`, then accounting(m); config is written at chain end |
| 7 | `settle_empty()` | caller(s), config(m), coll_config(m), accounting(m), auction(m) |
| 8 | `claim_leftover()` | owner(s), owner_collateral_holding(m), config, auction(m), auction_vault(m) |
| 9 | `settle_debt(amount)` | caller(s), config, accounting(m), buffer(m), stablecoin_definition(m) |
| 10 | `start_surplus_auction(nonce)` | caller(s), config, accounting(m), buffer(m), stablecoin_definition(m), surplus_auction(i,m), surplus_vault(m), ref_oracle [slot 0], clock |
| 11 | `take_surplus(amount_coin, max_protocol_in)` | taker_protocol_holding(s,m), taker_stable_holding(m), config, accounting(m), surplus_auction(m), surplus_vault(m), protocol_token_definition(m), clock |
| 12 | `close_surplus_auction()` | caller(s), config, accounting(m), surplus_auction(m), surplus_vault(m), buffer(m), clock |
| 13 | `start_debt_auction(nonce)` | caller(s), config, accounting(m), buffer(m), stablecoin_definition(m), debt_auction(i,m), protocol_token_definition [slot 1], ref_oracle [slot 0], clock |
| 14 | `take_debt(amount_coin, min_protocol_out)` | taker_stable_holding(s,m), taker_protocol_holding(m), config, accounting(m), debt_auction(m), stablecoin_definition(m), protocol_token_definition(m), mint_authority, clock |
| 15 | `close_debt_auction()` | caller(s), config, accounting(m), debt_auction(m), clock |
| 16 | `check_solvency()` | caller(s), config, accounting(m), buffer(m), stablecoin_definition(m), protocol_token_definition [slot 1], ref_oracle [slot 0], engine_authority, liquidation_engine_state, protocol_parameters(m), clock |

Stablecoin hook instructions (enum indices 19, 20, 21; IDL indices 19, 20, 21):

| # | Instruction (args) | Accounts in order |
|---|---|---|
| 19 | `set_liquidation_engine(engine_program_id: ProgramId, engine_authority_id: AccountId)` | admin(s), protocol_parameters, liquidation_engine_state(m) |
| 20 | `liquidate_position(collateral_to_seize: u128, normalized_debt_to_cover: u128)` | engine_authority(s), liquidation_engine_state, position(m), vault(m), custody(m), stability_fee_accumulator, redemption_price_state, market_price_oracle, protocol_parameters, clock |
| 21 | `engine_freeze_new_debt()` | engine_authority(s), liquidation_engine_state, protocol_parameters(m) |

## Errors

Code names and numbers (`EngineError::code()`); the guest panics with `Program error [6000+code]: Program error {code}: <Name>`.

`NotAdmin 1, ZeroId 2, ZeroRefPrice 3, ThresholdBelowDebtLot 4, InvalidConfig 5, TypeCapAboveGlobal 6, BpsOutOfRange 7, ScheduleEncoding 8, TtlOutOfRange 9, ChipAbovePenalty 10, HostMismatch 11, WrongAccount 12, OracleMismatch 13, WrongClock 14, ProtocolTokenLocked 15, RefPriceStepTooLarge 16, Paused 20, PriceFeedStale 21, PriceFeedLowConfidence 22, PriceFeedOutOfRange 23, NonceMismatch 30, ZeroDebt 31, VaultBelowCollateral 32, CapacityExhausted 33, NotAuctioning 34, ZeroFill 35, Slippage 36, BelowMinBid 37, DustRemainder 38, NotOwner 39, NothingToClaim 40, ChainEndAccountsMissing 41, InsufficientBalance 42, RecipientMustSign 43, WrongDefinition 44, VaultBelowBook 45, TokenNotConfigured 50, UnbackedDebtPresent 51, BufferBelowTarget 52, DelayNotElapsed 53, InsufficientBadDebt 54, MintCapConfig 55, MintAuthorityMissing 56, ProtocolTokenMismatch 57, DebtAuctionAlreadyLive 58, SurplusAuctionAlreadyLive 59, NotLive 60, RecoveryInProgress 61, Math 99`.

## Argument encoding

Arguments use spel-encodable types only (ADR-030): bps as `u32` (≤ 10_000; collateral `band_hi_bps` ≤ 20_000), schedules as `Vec<u32>` of interleaved `(elapsed_ms, bps)` pairs (1–8 points; discounts ≤ 3000 bps). `args::EngineParamsArgs::validate` and `args::CollateralArgs::validate` convert them to `auction_core` parameter structs and enforce the config-time rules (ADR-024). `args::check_ref_price_step(old, new)` enforces that one admin change of the protocol-token reference price stays within `[old / 2, 2 * old]` (inclusive; `RefPriceStepTooLarge`; plan review ruling I5, recorded in ADR-031, Task 13). `EngineParamsArgs::validate` does not call it (init has no old value); `set_engine_params` must call `check_ref_price_step(stored_old, new)`.

## Interfaces for Plan 3 (SDK, CLI, CI)

- Instruction encoding: risc0 serde of `auction_engine_core::Instruction` (`risc0_zkvm::serde::to_vec`, first word = variant index = IDL index). Stablecoin hooks: `stablecoin_core::Instruction` indices 19–21.
- Stablecoin instructions: encode by ENUM index (`risc0_zkvm::serde::to_vec(&ix)[0]`), never by IDL position. `idl/stablecoin-idl.json` follows the upstream guest function order, which differs from `stablecoin_core::Instruction` for IDL positions 8–18 (spec §3.2, known upstream fact; the IDL is pinned to upstream and is not reordered). The table below is pinned by `stablecoin_idl_position_maps_to_enum_index` (`programs/integration_tests/tests/idl.rs`).
- Account layouts: borsh of the `auction_engine_core::state` structs, each starting with its 8-byte tag (`TAG_*`). Decoders must check the tag.
- PDAs: `auction_engine_core::pda::*_pda` (the SDK must reuse these functions, not re-implement them).
- IDLs: `idl/auction_engine-idl.json`, `idl/stablecoin-idl.json` (regenerated with `tools/idl-gen` by every task that changes a guest).
- Errors: `EngineError::code()` (raw codes in the Errors section above, from `auction_engine_core::error`); the guest panic reads `Program error [6000+code]: Program error {code}: <Name>`; match on the name or the raw code.
- Program-id verification (I4): before building ANY transaction for a config, the SDK/CLI reads its `EngineConfig` and refuses unless `host_program_id` and `token_program_id` equal the expected ImageIDs in `DEPLOYMENTS.md` (and `admin` is the expected operator): the engine chains to those stored ids with the user's signer flags.
- Bid account choice (ADR-029, added in Task 11): list `accounting` (`bid_with_settlement`) whenever `coin_in_max >= remaining_target` or the quoted fill would empty the lot within the quote's slippage margin.
- Optional slots (ADR-030): pass `unused_slot_pda(config, 0|1)` when the reference oracle / protocol token is not configured.
- Signers that sign more than one stablecoin-program transaction must be `wallet auth-transfer init`-ed first (ADR-018).
- Verified in Task 19 against the code: instruction order and indices = `idl/auction_engine-idl.json` (`tests/idl.rs`, R2 order 0–16), account lists and signer/writable flags = the R2 table above (stablecoin hooks 19–21 = `idl/stablecoin-idl.json`), error names and codes = `auction_engine_core::error` (the Errors section above).
- Quote-test fixtures the SDK can reuse (`programs/integration_tests/tests/common/mod.rs`): `World::with_engine()` = `EngineParamsArgs::defaults_for_tests()` + `CollateralArgs::defaults_for_tests()` (Ray scale, 60-minute max oracle age, band 9000–11000 bps, penalty 10%) on a host with redemption price `R = 0.5` collateral per coin, min CR 1.5 and a stability fee of exactly 1.0 per ms; `World::unsafe_position()` opens the standard liquidatable position, whose expected quote is `LIQ_COLLATERAL = 500_000` (seized), `LIQ_DEBT = 600_000` (nominal debt), `LIQ_TARGET = 660_000` (amount to raise, +10%), `LIQ_REWARD = 25` collateral (`min(tip + chip·D = 605, reward_cap = 50, penalty·D = 60_000)` = 50 coin at R = 0.5), `LIQ_LOT = LIQ_COLLATERAL - LIQ_REWARD = 475_000`. Other constants: `T0` (initial clock, ms), `ONE` (= RAY = `FIXED_POINT_ONE`), `R`, `SALT = 1`, and the signer/holding seeds in `common::k`.
- Client obligations from `THREAT_MODEL.md`: verify program ids (I4, above), knowing that the published ImageIDs come from a native build until Plan 3's Docker builds; show unsold auctions that keep their cap reservation; call `observe_feed` on refresh (S5); simulate `observed()` before every `t_eff` preview (ADR-024); never unfreeze, or advise unfreezing, while settled `unbacked_debt` is above `bad_debt_freeze_threshold` (immediate re-freeze); treat `Accounting.host_frozen_by_engine` as "has ever frozen", not as the current state.

Stablecoin IDL position to enum index (the enum index is what the guest decodes and what the SDK must encode):

| IDL position | Instruction | Enum index |
|---|---|---|
| 0-7 | `initialize_program` ... `close_position` | same as the position (0-7) |
| 8 | `withdraw_collateral` | 17 |
| 9 | `repay_debt` | 18 |
| 10 | `set_stability_fee_per_millisecond` | 14 |
| 11 | `set_minimum_collateralization_ratio` | 8 |
| 12 | `set_controller_gains` | 9 |
| 13 | `set_timing_parameters` | 11 |
| 14 | `set_admin` | 12 |
| 15 | `set_freeze_authority` | 13 |
| 16 | `set_market_price_oracle` | 10 |
| 17 | `freeze` | 15 |
| 18 | `unfreeze` | 16 |
| 19-21 | `set_liquidation_engine`, `liquidate_position`, `engine_freeze_new_debt` | same as the position (19-21) |

Plan 3 carry-overs:

- Program CI with the risc0 toolchain: engine and stablecoin suites, the hook order test, and an `idl-gen` diff against the committed IDLs.
- SDK encoders keyed by enum index, using the table above.
- Config-time check that `protocol_token` is a token definition owned by the token program (THREAT_MODEL finding 4).
- Debt relist griefing hardening (`debt_delay_ms`, or an atomic close-expired in `set_engine_params`).
- Reproducible Docker builds, then refresh the ImageIDs in `DEPLOYMENTS.md`.
- `CYCLES.md` plus a regression check, and re-measure ADR-023 with real proofs.
- `REQUIREMENTS.md` plus its checker, `cargo-fuzz`, and a red-team pass.
- An oracle price publisher / keeper.
- Upstream issue for the stablecoin guest function order (spel mis-encodes IDL positions 8-18).
- Split oversized test files (`tests/common/mod.rs` 1273 lines, `surplus_tests.rs` 958 lines).
- Make the drift tests catch new variants: an exhaustive `match` over the engine `Instruction` next to `instruction_samples()`, and over `stablecoin_core::Instruction` in `stablecoin_idl_position_maps_to_enum_index` (a variant with no IDL entry currently passes).

## Test harness

`programs/integration_tests` (package `engine_integration_tests`) runs the real token, stablecoin and (from Task 8) auction_engine guests in an in-process `V03State` (risc0 dev mode). `World::new` bootstraps the host with the real `InitializeProgram` (stability fee exactly 1.0 per ms so debt stays exact, min CR 1.5, redemption price 0.5); positions use the real `OpenPosition` + `GenerateDebt`; fixture accounts are force-inserted like the upstream tests; signers are owned by a placeholder program so they can sign repeatedly (S1 problem 6).

## Handler conventions

- Handlers live in `programs/auction_engine/src/` and take `(program_id, <Name>Accounts, args…)`; they return `EngineResult` = post-states for every input account in order plus chained calls. The guest only converts arguments and maps `EngineError` to `SpelError::Custom(code)`.
- Engine accounts are loaded through `accounts::load_*` (owner = engine, 8-byte tag, PDA re-derived from stored fields). Holdings through `fungible_balance` (token program and definition) or `check_recipient` (initialised with the definition, or fresh and signed).
- Chained token calls are built by `token_calls`; a later call in the same transaction carries the simulated post-state of an earlier one, and only the call that attaches a PDA's seed marks it authorised (Shared Reference R3).
- `init_config` is permissionless: the signer becomes that config's admin, the config PDA is `["CONFIG", admin, salt]`, and a host trusts a config only after its admin verifies and binds that config's engine authority (ADR-026/027). The host program id comes from the `ProtocolParameters` owner and the token program from the stablecoin definition owner; both are fixed at `init_config` and clients must verify them against `DEPLOYMENTS.md` (I4).

## Feed, pause clock and `observe_feed`

Every `liquidate` and `bid` first observes the feed at `now` (`feed::observe`): `validate_feed` (age, zero price, confidence, `[R/2, 2R]`) with R projected by the host's own `compute_current_redemption_price_wide`, then `PauseClock::observed` with policy (b) `DerivedFromOracle` (ADR-007). Only then is `t_eff` computed. A paused type (admin flag or invalid feed) reverts with `Paused` / `PriceFeedStale` / `PriceFeedLowConfidence` / `PriceFeedOutOfRange`; the reverted observation is lost, which policy (b) tolerates because the stale onset is derived from the oracle timestamp at the next successful observation. `observe_feed` is that successful observation: permissionless, never reverts on an *invalid reading* (it records it as a pause), writes only the pause clock. An *undecodable or wrong oracle account* is a different class: it reverts (`WrongAccount` / `OracleMismatch`) in every instruction, and policy (b) recovers the unobserved gap at the next successful observation. `set_collateral_config` writes the admin flag and observes in the same transaction, so admin pauses are exact.

## `liquidate(nonce)`

1. Observe the feed; revert if paused (ADR-032). The engine's oracle must still be the host's market oracle (ADR-028).
2. Read the position (owned by the bound host, at its PDA), the host vault's actual balance `c >= collateral_amount`, and the nominal debt `D = ceil(normalized x accumulator / RAY)` projected to `now`.
3. `quote_liquidation`: `amount_to_raise = ceil(D*(1e4+penalty)/1e4)`; caps with the oversized-alone rule (`cap.saturating_sub(reserved)`, so a `type_cap` lowered below `type_reserved` admits only an alone liquidation); keeper reward `min(tip + chip*D, cap, penalty*D)` -> collateral at the band-clamped price, capped at `c`.
4. `LiquidationRecord`: created on first use, `nonce == count`, `count += 1` (only auction creation bumps it, S3 ruling).
5. Open the `Auction` (schedule snapshot, `type_paused_total_at_start`), reserve on `CollateralConfig.type_reserved` and `EngineConfig.global_reserved`. `Accounting` is not touched (ADR-023: chain end only).
6. Chained: `InitializeAccount(auction_vault)` -> `stablecoin.liquidate_position(full amounts)` signed by `["ENGINE_AUTH", config]` (the host checks health, seizes, sweeps the whole vault into the auction vault) -> `Transfer(reward)` to the keeper's holding (fresh holdings must sign). Five executions.

Hook arguments come only from state the engine read in the same transaction (ADR-019 invariant iii): the collateral and normalised debt are the position's own, so the debt seized for is fully cleared (the host only bounds the maximum seize); the custody destination is always this auction's own vault `["VAULT", auction]`, which the engine pins by PDA and requires uninitialised; `ENGINE_AUTH` seeds go only on the call to `cfg.host_program_id` (invariant i).

The host, not the engine, decides whether the position is unsafe (`is_position_collateralized` at the minimum ratio against the redemption price). A safe position reverts the whole transaction.

## `bid` / `bid_with_settlement`

Debt first (ADR-010): a fill of `charged` coin splits into `debt_part` (burned from the bidder) and `penalty_part` (transferred to the buffer); the bidder receives `collateral_out` from the auction vault. Chain: `Token::Burn(stable_def, bidder_stable)` if `debt_part > 0` -> `Token::Transfer(bidder_stable after burn, buffer)` if `penalty_part > 0` -> `Token::Transfer(auction_vault*, bidder_collateral)`; at most 4 program executions.

1. Observe the feed before any `t_eff` use (ADR-024); a stale, low-confidence, out-of-range or admin-paused type reverts the bid (`PriceFeed*` / `Paused`). `t_eff = effective_elapsed(start, type_paused_total_at_start, now)`, so the discount is frozen over paused intervals, and the discount schedule is the auction's own snapshot.
2. `quote_bid` caps the charge at the remaining target (`coin_in_max = u128::MAX` charges only the remainder) and at the lot (a lot-binding fill pays `ceil(lot x (1 - d) / P)`); `min_collateral_out`, `min_bid` and the dust rule apply to non-final fills; the target-reaching fill of less than one collateral unit pays exactly 1 unit (the only upward payout rounding, ADR-024).
3. Chain end (target reached or lot exhausted) needs the `Accounting` account (ADR-023, ADR-029): `Accounting::record_settled` folds the auction's cumulative raise and books the shortfall exactly once, and the reservation is released on `CollateralConfig.type_reserved` and `EngineConfig.global_reserved`. A settled auction persists `settled = true`, so every later bid fails with `NotAuctioning` and nothing is folded twice. `bid` (no `Accounting`) reverts with `ChainEndAccountsMissing` and applies nothing if the fill would end the auction; `bid_with_settlement` lists `Accounting` and accepts any fill (a non-final one echoes it unchanged).
4. Sweep (ADR-033): a fill that empties the lot transfers the WHOLE auction-vault balance to the bidder (donations included); every other fill transfers exactly the booked `collateral_out`. The vault balance is only checked with `>=` the booked amount (`VaultBelowBook`), never `==`. A target-reaching fill that leaves lot unsold pays the booked amount only; any donation stays in the vault for the whole-vault sweep of `claim_leftover`.
5. The bidder's stablecoin holding must sign; a fresh bidder collateral holding must sign too (`RecipientMustSign`), a holding of the wrong definition fails with `WrongDefinition`. `AuctionState.keeper` is identity only and is never a payment recipient.

Interfaces for Plan 3: use `bid_with_settlement` whenever `coin_in_max >= remaining_target` or the quoted fill is within the slippage margin of emptying the lot; otherwise `bid`. If a plain `bid` turns final in flight it reverts with `ChainEndAccountsMissing`; re-quote and resend with the chain-end account.

The final-fill exception is config/price-dependent and bounded at 1 collateral unit per auction. It needs a remaining target `r` with `chost <= r < (1 - d) / P` coin, so one collateral unit must be worth more than `chost` coin (a high collateral price in coin terms); with the default test config (`chost` 110, price 0.5) it cannot occur, with a low collateral-per-coin price it can. It is pinned at engine level by `bid_tests::the_final_fill_exception_pays_one_unit_and_settles_once` (price 0.005, a prior partial bid leaves 150) as well as in `auction_core`.

A `bid` that does not end the auction echoes the config account unchanged (only a chain end writes it, to release the reservation); the bidder's collateral holding must not be the auction vault (`WrongAccount("bidder_collateral_holding")`).

## Settlement paths

An auction ends in exactly one of two chain-end paths: the `bid_with_settlement` that reaches the target or empties the lot, or `settle_empty` for an auction whose lot is already 0 (the keeper reward consumed the whole seizure, so no bid can succeed). Both call `Accounting::record_settled` exactly once, book the shortfall, and release `reserved_amount` from `CollateralConfig.type_reserved` and `EngineConfig.global_reserved`; the persisted `settled = true` makes every later `bid` / `settle_empty` fail `NotAuctioning` (ADR-024).

- `settle_empty` (permissionless, signer required): requires `Auctioning` and `collateral_left == 0`, otherwise `NotAuctioning`. Engine only, no chained call.
- `claim_leftover` (owner only): the signer must be the position owner recorded in the `Auction` (`NotOwner`). It pays `leftover_claimable - leftover_claimed` once (second claim: `NothingToClaim`) by sweeping the whole auction-vault balance to the owner's collateral holding, so a donation made after settlement is paid out with it (ADR-033); the vault balance is checked with `>=` the claim (`VaultBelowBook`). A fresh recipient holding must sign (`RecipientMustSign`) and must not be the vault. Two executions.
- `settle_debt(amount)` (permissionless): burns `min(amount, buffer ACTUAL balance, unbacked_debt - debt_on_auction)` from the engine-PDA buffer, signed by the buffer's seed `["BUFFER", config]` (the S7 prerequisite "burn from an engine-PDA-held holding", test `settle_debt_uses_actual_buffer_balance`). The debt a live debt auction sells (`debt_on_auction`, ADR-024 reservation) is never settled from the buffer, `settle_debt(u128::MAX)` burns only what exists (`settle_debt_max_amount`), and donations to the buffer raise what can be burned. Nothing to burn is a successful no-op with no chained call. Two executions.

`settle::implicit_settle(acc, buffer_balance)` (`settle_debt(u128::MAX)` on the actual balance) and `settle::buffer_burn(...)` (the seeded `Token::Burn`, `None` for 0) are shared with the system-auction instructions of Tasks 13-15.

## F5 surplus auction

Three instructions (R2 indices 10-12), all with at most 4 program executions:

- `start_surplus_auction(nonce)` (permissionless): requires `nonce == Accounting.surplus_nonce` (`NonceMismatch`), runs an implicit `settle_debt(MAX)` on the buffer's actual balance, then the core `start_surplus` rules: a configured protocol token (`TokenNotConfigured`), no live surplus auction (`SurplusAuctionAlreadyLive`), `unbacked_debt == 0` after the implicit settle (`UnbackedDebtPresent`; debt a live debt auction reserves is not settled, so it still blocks), buffer after the burn `>= surplus_buffer_target + surplus_lot` (`BufferBelowTarget`), and `surplus_delay_ms` since the last end (`DelayNotElapsed`). Chain: `Token::InitializeAccount(stable_def, surplus_vault*)` -> `Token::Burn(stable_def, buffer*, implicit settle)` if > 0 -> `Token::Transfer(buffer* after burn, surplus_vault post-init, lot)`. The `SurplusAuction` account (`["SURP", config, nonce]`) is created by the engine and the surplus vault (`["SURPVAULT", surplus_auction]`) is a separate holding, so a later `settle_debt` or implicit settle cannot spend the lot (ADR-031).
- `take_surplus(amount_coin, max_protocol_in)`: the taker signs its protocol-token holding, which is burned in the same transaction. The core caps the fill at the lot (`amount_coin = u128::MAX` fills at most the lot), applies `min_take` (`DustRemainder`, `ZeroFill`) and prices `tokens = ceil(ceil(coin x mult / 1e4) x RAY / ref)` (owed, so rounded up) with `mult = 1e4 + premium - decay(t)` floored at `1e4 - max_discount`; `tokens > max_protocol_in` fails `Slippage`. Chain: `Token::Burn(protocol_def, taker_protocol, tokens)` (no engine seed) -> `Token::Transfer(surplus_vault*, taker_stable, filled)`. Only the filled amount leaves the vault (a donation stays; ADR-033). The protocol-token definition slot is checked at use time: the configured id (or the placeholder), owned by `cfg.token_program_id`, fungible (`ProtocolTokenMismatch`; `TokenNotConfigured` when unset). The taker's stablecoin holding must not be the vault; a fresh one must sign (`RecipientMustSign`).
- `close_surplus_auction` (permissionless): after `system_ttl_ms` (`DelayNotElapsed`) a live auction (`NotLive`) closes and the whole surplus-vault balance returns to the buffer (`Token::Transfer(surplus_vault*, buffer)`, skipped for an empty vault); `total_surplus_returned` and `last_surplus_end_ms` are updated. A fully taken auction cannot be closed: it ended at its last take, which also sets `last_surplus_end_ms`. `system_ttl_ms` is not an expiry of the take: takes stay possible after the TTL until someone calls `close_surplus_auction`, at the price the schedule holds (the floor, `1e4 - max_discount`), because the core `take_surplus` does not read the TTL; a keeper should close promptly.

Reference price (`system::ref_price`): the admin price `protocol_token_ref_price`; when `ref_oracle_enabled`, a valid reference oracle reading (age `<= ref_oracle_max_age_ms`, confidence, within `[admin/2, 2 x admin]`) overrides it, clamped to +/-20% of the admin price (`effective_ref_price`); an invalid or stale oracle falls back to the admin price. The result is snapshotted into `SurplusAuction.ref_ray` at start: takes read neither the reference oracle, nor its max age, nor the config price (tests `live_take_ignores_later_ref_price_and_oracle_max_age_changes`, `take_price_follows_the_snapshot_and_the_decay_not_the_config_price`). `set_engine_params` therefore may change the price and the max age while an auction is live; it cannot change the protocol token, its oracle or the system parameters then (`ProtocolTokenLocked`).

`system::protocol_token_status` pins the protocol-token slot (configured id or `["UNUSED", config, 1]`) and reports `configured`, `token_ok` (token-program owner, fungible) and `mint_authority_ok` (`["MINT_AUTH", config]`); F4 (Task 14) and `check_solvency` (Task 15) use it with `ref_price`.

## F4 debt auction

Three instructions (R2 indices 13-15; `start_debt_auction` at most 2 program executions, `take_debt` 3, `close_debt_auction` 1):

- `start_debt_auction(nonce)` (permissionless), conditions in this order: `nonce == Accounting.debt_nonce` (`NonceMismatch`); the account pins (`debt_auction` is the uninitialised PDA `["DEBT", config, nonce]`; the protocol-token and reference-oracle slots are the configured ids or the placeholders); an implicit `settle_debt(MAX)` on the buffer's actual balance; then the core `start_debt`: no live debt auction (`DebtAuctionAlreadyLive`), a configured protocol token (`TokenNotConfigured`), a well-formed definition (`ProtocolTokenMismatch`), `["MINT_AUTH", config]` as its mint authority (`MintAuthorityMissing`), `unbacked_debt - buffer >= debt_lot` after the implicit settle (`InsufficientBadDebt`), and a start amount `floor(debt_lot x RAY / ref x (1e4 - initial_discount) / 1e4)` that is non-zero and at most `max_mint_per_auction` (`MintCapConfig`; a zero start amount is the same configuration fault). The reference price is `system::ref_price` (admin price, optional oracle clamped to +/-20%); it is snapshotted into `DebtAuctionState.start_amount`. Pairing (ADR-024): `Accounting::record_debt_start(debt_lot)` reserves the lot in `debt_on_auction`, sets `debt_live` and increments `debt_nonce`. Chain: `Token::Burn(stable_def, buffer*, implicit settle)` if > 0.
- `take_debt(amount_coin, min_protocol_out)`: the taker signs its stablecoin holding. The core caps the fill at the lot (`amount_coin = u128::MAX` fills at most the lot), applies `min_take` (`DustRemainder`, `ZeroFill`) and mints `floor(offered(t) x filled / debt_lot)` with `offered(t) = min(floor(start_amount x (1e4 + growth(t)) / 1e4), max_mint_per_auction)`; a zero mint is `ZeroFill`, `minted < min_protocol_out` is `Slippage`. Because every take prorates an offer that is itself capped and the fills sum to at most the lot, the total minted over an auction's life never exceeds `max_mint_per_auction` (test `multi_take_over_time_never_mints_more_than_the_cap`). `filled = old.lot_left - new.lot_left`; the pairing is `Accounting::record_debt_take(filled)` (unbacked debt and the reservation both fall by `filled`, `total_debt_auctioned` grows) and a burn of exactly `filled`; `total_protocol_minted` grows by the minted amount. A fill larger than the recorded reservation is refused (`Math`). Chain: `Token::Burn(stable_def, taker_stable, filled)` (taker-authorised, no seed) -> `Token::MintWithAuthority(protocol_def, taker_protocol, mint_authority*, minted)`, where the PDA `["MINT_AUTH", config]` is authorised only in this call, by attaching its seed. The protocol-token definition is checked at use time (configured id, token-program owner, fungible, mint authority: `ProtocolTokenMismatch`, `MintAuthorityMissing`, `TokenNotConfigured`). The taker's protocol holding may be fresh, but then it must sign: the token program claims a fresh recipient with `Claim::Authorized` (`programs/token/src/mint.rs:137`, S7 prerequisite), so an unsigned fresh holding fails `RecipientMustSign` (test `take_debt_mints_into_fresh_holding`). As with surplus takes, the core does not read the TTL: takes remain possible after `system_ttl_ms` until someone closes.
- `close_debt_auction` (permissionless): after `system_ttl_ms` (`DelayNotElapsed`) a live auction (`NotLive`) closes. Nothing is minted for the untaken part; `Accounting::record_debt_close(lot_left)` releases exactly that reservation and clears `debt_live`, so `settle_debt` can cover the released debt again (test `the_reserved_amount_becomes_settleable_again_after_close`). A fully taken auction ended at its last take and cannot be closed.

The invariant `debt_on_auction <= unbacked_debt` holds after every step; the integration tests assert it on every accounting read. While a debt auction is live, `settle_debt` and the implicit settles burn only `unbacked_debt - debt_on_auction` (test `settle_debt_excludes_the_reservation_of_a_live_debt_auction`), and `set_engine_params` refuses to change the protocol token, its oracle or the system parameters (`ProtocolTokenLocked`), while a reference-price change is allowed because the start amount is snapshotted.

### Mock protocol token

The tests create the protocol token with a real `Token::NewFungibleDefinition` whose `mint_authority` is `["MINT_AUTH", config]` (`World::with_system` / `create_protocol_token`); it is used only by F4 (mint) and F5 (burn). The engine never initialises the mint-authority PDA (S7, ADR-021).

## check_solvency

`check_solvency()` (R2 index 16, permissionless: any signer; at most 3 program executions) implements spec §5.11 and ADR-017. Order: account pins (buffer, stablecoin definition, `engine_authority`, `liquidation_engine_state`, host `protocol_parameters`, both optional slots), an implicit `settle_debt(MAX)` through `settle::implicit_settle` (so `debt_on_auction` stays excluded and `debt_on_auction <= unbacked_debt` keeps holding), then the core decision on the settled `unbacked_debt`:

| Decision | When (first match) | Program-layer effect |
|---|---|---|
| `Healthy` | `unbacked <= bad_debt_freeze_threshold` (equality is Healthy) | implicit settle only |
| `Freeze` | token not configured; or freeze enabled and the start check fails with `MintCapConfig` (incl. `start_amount == 0`, ADR-024), `MintAuthorityMissing` or `ProtocolTokenMismatch` | chain `Burn(buffer*)` if the settle burns, then `stablecoin.engine_freeze_new_debt(engine_authority*, liquidation_engine_state, protocol_parameters)`; `Accounting.host_frozen_by_engine = true` ("has ever frozen") |
| `RecoveryInProgress` | a debt auction is live (above threshold) | revert (`RecoveryInProgress`): nothing is frozen and the implicit settle is not applied |
| `NoAction` | `freeze_on_bad_debt == false` with a token, or a startable auction (a keeper should start it), or a transient start error | implicit settle only |

Variant C: the `["ENGINE_AUTH", config]` seed is attached only to the one call whose program id is `cfg.host_program_id`; the accounts forwarded to it are the pinned `engine_authority` PDA, `liquidation_engine_state` at `compute_liquidation_engine_state_pda(host)` and the config's `host_protocol_parameters_id`. Both optional slots are pinned to the configured id or the `["UNUSED", config, k]` placeholder (ADR-030), so a caller cannot swap in an account that forces a Freeze. The decision is therefore a function of engine-validated state only.

Idempotence: the host's `engine_freeze_new_debt` is idempotent, so a repeated `check_solvency` on an already-frozen host re-freezes harmlessly. `host_frozen_by_engine` means "the engine has frozen this host at least once": it is set on every Freeze and never cleared, so it stays `true` after the freeze authority unfreezes and is not the current freeze state (read the host's `ProtocolParameters.is_frozen` for that). Only the host's freeze authority unfreezes; a later Healthy check does not, and while settled `unbacked_debt` stays above the threshold anyone can re-freeze right after an unfreeze (THREAT_MODEL).

F4 interpretation (ADR-017): the freeze applies only above the absolute `bad_debt_freeze_threshold` (>= `debt_lot`), never for a small shortfall. After a Freeze the real host rejects `OpenPosition` and `GenerateDebt` with "Protocol is frozen" (tests in `programs/integration_tests/tests/solvency.rs`).

## Security tests

`programs/integration_tests/tests/security.rs` (13 tests; real lee, token, stablecoin and engine guests; attacker guests `impostor` and `flawed_engine` in `programs/test_programs`, test-only, never deployed). Variant C (ADR-019) and ADR-026 are pinned as follows.

| Test | Attack covered |
|---|---|
| `impostor_forging_the_authority_flag_is_rejected` | impostor chains `engine_freeze_new_debt` with a forged `is_authorized` on the real authority: `Caller program is not the liquidation engine` |
| `impostor_own_pda_is_rejected_by_the_immediate_caller_check` | impostor authorises its own PDA by seed and chains the freeze hook |
| `impostor_chaining_liquidate_position_is_rejected` | the same two shapes against `liquidate_position`; the second binds the host to (real engine program id, impostor PDA) so only the immediate-caller check is left |
| `impostor_chaining_both_hooks_is_rejected` | one transaction chains `engine_freeze_new_debt` and `liquidate_position` |
| `laundering_freeze_through_an_intermediate_program_is_rejected`, `laundering_liquidate_through_an_intermediate_program_is_rejected` | host bound to the test-only `flawed_engine`, which authorises its PDA for the impostor; the impostor forwards the received flags; rejected because the immediate caller is the impostor (the real engine offers no such path, invariants (i) and (ii)) |
| `set_liquidation_engine_through_a_chained_call_is_rejected` | the admin's signature forwarded through the impostor: `set_liquidation_engine must be a top-level call`; binding unchanged |
| `rogue_config_with_the_same_salt_cannot_liquidate_the_bound_host` | a config on the same host and salt is a different PDA (admin in the seed, ADR-027); its authority is not the bound one |
| `engine_takes_no_program_id_argument` | documentation only (M5): no engine instruction carries a program id in the IDL |
| `fake_host_config_is_accepted_so_clients_must_verify_program_ids` | I4: the engine cannot tell a fake host; clients verify program ids against DEPLOYMENTS.md |
| `engine_authority_cannot_be_signed_at_top_level` | a user cannot authorise the engine PDA (`must be a signer`) |
| `private_path_impostor_is_rejected_by_the_immediate_caller_check` | ADR-034: Variant C holds in the privacy circuit |
| `private_path_liquidate_is_not_expressible_on_v0_2_4` | ADR-034: `Inconsistent authorization for account` |

## Invariants checked on accounts

`programs/integration_tests/tests/invariants.rs` checks the spec section 5.13 invariants that need accounts, through the real guests. `check_all` returns the first violated invariant as text, so the negative tests can show that the check itself catches a broken engine.

| Invariant | Test | What is checked |
|---|---|---|
| 2 (vault >= book, collateral conservation) | `collateral_invariants_hold_over_random_bids` (3 seeds, after every step) | auction vault balance >= `collateral_left` (live) or unclaimed leftover (settled); `seized == reward + sold + left` |
| 7 (reservations) | same | sum of live `reserved_amount` == `type_reserved` == `global_reserved`; with two or more live auctions the reservations stay within the type and global caps (a single auction may exceed a cap alone) |
| 9 (buffer covers the book) | same, and `surplus_returned_ge_untaken_lot_with_vault_donation` | actual buffer == penalties + returned surplus + donations to the buffer - settled debt - lots out (exact; a donation into a surplus vault arrives through the returned surplus) |
| 10 (seizure and debt cleared match the host) | `inv10_debt_clear_matches_seizure_over_full_chain`, `inv10_check_catches_seizure_without_debt_clear` | host position zeroed; `collateral_seized` == host vault balance, `debt_principal` == host normalized debt (the host bounds only the MAXIMUM seizure); after settlement settled principal == burned + shortfall |
| 11 (supply is backed) | same as invariant 2 | stablecoin supply <= position debt + open auction debt + unbacked debt + injected |
| 12 (liveness) | `liveness_unsafe_position_is_liquidatable_within_caps_or_alone` | blocked only with `CapacityExhausted` while another auction holds the capacity, liquidatable once it settles |
| 17 (system burn/mint totals) | `system_invariants_burn_and_mint_totals` | minted <= cap; unbacked falls by exactly the coin burned; protocol-token supply moves by the minted/burned totals |
| R3 (isolation) | `r3_a_bid_touches_only_its_own_auction` | a bid leaves the other auction, vault, record, host position and Accounting byte-identical |
| surplus book uses `>=` | `surplus_returned_ge_untaken_lot_with_vault_donation` | a donation into the surplus vault makes `total_surplus_returned` exceed the untaken lot; `==` would reject a correct run |
| no admin path moves funds | `no_admin_instruction_moves_surplus_or_buffer_funds` | every admin instruction (`caller_of` is exhaustive over the instruction enum) with the buffer, surplus vault and auction vault in representative slots (the test asserts every admin instruction is attempted): funds, books and stablecoin supply unchanged |

Chain budgets: `programs/integration_tests/tests/chains.rs` (ADR-036).

R4/R5 are shown across two host instances (one collateral type each) with two engine configs on one engine program; see `tests/two_instances.rs`. Instance B (`programs/stablecoin/methods_b`) is a test-only build and is never deployed.
