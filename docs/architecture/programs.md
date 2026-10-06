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
