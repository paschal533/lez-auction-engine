# Engine program (`auction_engine`)

## Layout

- `programs/auction_engine/core` (`auction_engine_core`): shared, guest-safe types. Instruction enum, borsh account states, PDA seed functions, `EngineError`, argument parsing and validation. Depends on `auction_core` (pure math) and `lee_core`; used by the guest, the host-side handlers, tests and the Plan 3 SDK.
- `programs/auction_engine` (planned, Task 8): handlers that take `lee_core` accounts and return post-states plus chained calls; pure Rust, unit-testable without a zkVM.
- `programs/auction_engine/methods` (planned, Task 8): the risc0 guest (one `#[instruction]` function per `Instruction` variant, in enum order) and the host build crate that exposes `AUCTION_ENGINE_ELF` / `AUCTION_ENGINE_ID`.

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
- Account layouts: borsh of the `auction_engine_core::state` structs, each starting with its 8-byte tag (`TAG_*`). Decoders must check the tag.
- PDAs: `auction_engine_core::pda::*_pda` (the SDK must reuse these functions, not re-implement them).
- IDLs: `idl/auction_engine-idl.json`, `idl/stablecoin-idl.json` (regenerated with `tools/idl-gen` by every task that changes a guest).
- Errors: `EngineError::code()`; the guest message is `Program error [6000+code]: <Name>`.
- Program-id verification (I4): before building ANY transaction for a config, the SDK/CLI reads its `EngineConfig` and refuses unless `host_program_id` and `token_program_id` equal the expected ImageIDs in `DEPLOYMENTS.md` (and `admin` is the expected operator): the engine chains to those stored ids with the user's signer flags.
- Bid account choice (ADR-029, added in Task 11): list `accounting` (`bid_with_settlement`) whenever `coin_in_max >= remaining_target` or the quoted fill would empty the lot within the quote's slippage margin.
- Optional slots (ADR-030): pass `unused_slot_pda(config, 0|1)` when the reference oracle / protocol token is not configured.
- Signers that sign more than one stablecoin-program transaction must be `wallet auth-transfer init`-ed first (ADR-018).

## Test harness

`programs/integration_tests` (package `engine_integration_tests`) runs the real token, stablecoin and (from Task 8) auction_engine guests in an in-process `V03State` (risc0 dev mode). `World::new` bootstraps the host with the real `InitializeProgram` (stability fee exactly 1.0 per ms so debt stays exact, min CR 1.5, redemption price 0.5); positions use the real `OpenPosition` + `GenerateDebt`; fixture accounts are force-inserted like the upstream tests; signers are owned by a placeholder program so they can sign repeatedly (S1 problem 6).
