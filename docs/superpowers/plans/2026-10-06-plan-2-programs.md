# Plan 2: On-chain Programs (stablecoin hooks + auction_engine)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the two on-chain programs of the RFP-014 PoC on LEZ v0.2.4: the vendored Logos stablecoin with three appended engine hooks, and the `auction_engine` SPEL program (collateral auctions, F4 debt auction, F5 surplus auction, solvency freeze) as a thin layer over `auction_core`, proven by in-process `V03State` tests, a security pass and a localnet smoke run.

**Architecture:**
- `programs/stablecoin/` is lez-programs `64c01210` vendored pristine in one commit; the hooks are ordinary commits on top, exported as an ordered patch series for the upstream PR (ADR-025). New instructions are appended to the end of both the enum and the guest function list.
- `programs/auction_engine/` follows the upstream program layout: `core/` (`auction_engine_core`: instruction enum, borsh account states, PDA seeds, errors, argument parsing), `src/` (`auction_engine_program`: host-testable handlers that return `(Vec<AccountPostState>, Vec<ChainedCall>)`), `methods/guest/` (thin SPEL dispatch). Every decision is a call into `auction_core`; the program owns accounts, clock, oracle reads, token legs and authentication.
- The host authenticates the engine with Variant C (immediate `caller_program_id` plus an engine-authority PDA seeded by the engine config, ADR-019/ADR-027). The engine only ever chains to the program ids stored in its config (host and token), fixed at `init_config`; clients verify them against `DEPLOYMENTS.md` before building any transaction (I4).
- `programs/integration_tests/` runs every instruction against the real token, stablecoin and engine guests in an in-process `V03State` (risc0 dev mode), including attack programs from `programs/test_programs/`.

**Tech Stack:** Rust 1.94.0; LEZ v0.2.4 (`lee`, `lee_core`, `clock_core`); SPEL `1ef0500f` (`spel-framework`, `idl-gen`); risc0-zkvm `=3.0.5` (`risc0-build` for guests); borsh 1.5; WSL Ubuntu (distro name `Ubuntu-20.04`) for every build; Docker only for the localnet guest build (Task 20).

**Spec:** `docs/superpowers/specs/2026-10-01-rfp014-auction-engine-poc-design.md` (v5) as amended by `docs/design-log.md` ADR-018 to ADR-024. Read spec §3.2, §5, §9, §10 and ADR-019, ADR-020, ADR-021, ADR-023, ADR-024 before any task.

## Global Constraints

- Pins: LEZ v0.2.4 `47eba256479f6f785acbd138834340703cd03401`; SPEL `1ef0500f8fc8ce3ddf95523726ae159db83f744f`; risc0-zkvm `=3.0.5`; Rust `1.94.0`; lez-programs (vendored stablecoin) `64c01210adecd6b557dbd021a3cc94553f243fd8`.
- Units (spec §5.1): amounts are raw base units as `u128`; prices are RAY = `1_000_000_000_000_000_000_000_000_000` (1e27; equal to the host's `FIXED_POINT_ONE`), collateral per 1 stablecoin; bps convert to RAY by `× 1e23`; time is `u64` milliseconds read only from `CLOCK_01`.
- Rounding (spec §5.1): **amounts paid out round down; amounts owed or charged round up.** The only exception is the final-fill payout of at most 1 collateral base unit per auction (ADR-024). Every product goes through `auction_core::math::mul_div_floor` / `mul_div_ceil`; never write `a * b / c` on `u128`.
- `overflow-checks = true` in release (root `Cargo.toml`); `#![forbid(unsafe_code)]` in every crate this plan creates.
- Chained calls (ADR-018): at most 11 program executions per public transaction, 10 per private one. Every engine instruction is designed for ≤ 10 executions (Task 17 measures each).
- Engine authentication (ADR-019, Variant C): the hook checks `ctx.caller_program_id == stored engine_program_id` **and** that the stored engine-authority PDA is authorised. Invariants: (i) the engine supplies `ENGINE_AUTH` seeds only on calls whose program id is `EngineConfig.host_program_id`, and it only ever chains to the program ids stored in its config (`host_program_id`, `token_program_id`); those ids are fixed at `init_config` and must be verified by clients against `DEPLOYMENTS.md` (I4) — a config pointing at a fake host or token program receives the user signer flags it is given; (ii) the host never chains to a user-chosen program while the engine authority is authorised; (iii) every hook argument and every forwarded account id is derived from or checked against engine-validated on-chain state read in the same transaction.
- Config validation (ADR-024): `CollateralParams::validate`, `FeedParams::validate`, `SystemParams::validate` run at config time; zero program ids and zero account ids are rejected; `protocol_token_ref_price == 0` is rejected; `bad_debt_freeze_threshold >= debt_lot`.
- Bounds (spec §5.12): penalty 0–2500 bps; schedule ≤ 8 points with max ≤ 3000 bps; system auction ttl 10 min – 7 d (600_000–604_800_000 ms); band lo ∈ [5000, 10000], hi ∈ [10000, 20000]; max oracle age ≤ 24 h; max confidence ≤ 1000 bps; `chost ≥ min_bid`; keeper `chip_bps ≤ penalty_bps`; `type_cap ≤ global_cap`; `0 < max_mint_per_auction ≤ MAX_MINT_HARD_CAP`.
- Balances a third party can raise are never compared with `==`: use "at least X" and sweep the whole balance (spec §10).
- Handler order: checks → effects → chained calls. No admin path moves auction collateral, buffer funds or system-auction funds.
- `Accounting` is written only at chain end (ADR-023): every chain-end path calls `Accounting::record_settled` (Task 2) exactly once.
- Build and test inside WSL against the repo at `/mnt/c/Users/user/Documents/basecamp/lez-auction-engine` with `CARGO_TARGET_DIR=$HOME/lez-target/plan2` (controller ruling), always through `scripts/wsl-cargo.sh` (Task 1). From Git Bash, prefix `MSYS_NO_PATHCONV=1` so `/mnt/...` arguments are not mangled (S4), and call `wsl -d Ubuntu-20.04 -e bash -lc '…'` (`-e` passes the command unchanged, so `$HOME`/`$VAR` expand inside WSL).
- Commits: conventional format `<type>: <description>`, with no co-author trailer. Every task's commit step is labelled "Commit (controller — GPG signing on Windows)": the implementer stages the listed files and reports; the controller runs the commit from Git Bash on Windows; never from WSL. Nothing is pushed.
- Docs are updated in the same task as the code they describe (`docs/architecture/*.md`, `docs/design-log.md`, `THREAT_MODEL.md`).
- Every instruction argument type is spel-encodable: `bool`, `u8`, `u32`, `u64`, `u128`, `Vec<u32>`, `AccountId`, `ProgramId` (ADR-030). bps travel as `u32` and are range-checked; schedules travel as `Vec<u32>` of interleaved `(elapsed_ms, bps)` pairs.
- The **Shared Reference** section below (seeds, account layouts, instruction account lists, error names) is part of every task.

## Review Focus

Inputs the spec implies but no happy-path test exercises, most likely first. Each line has a pinned test in the owning task.

1. **A bid that unexpectedly ends the auction, sent without the chain-end account.** Price and `t_eff` move between quote and inclusion, so a "partial" `bid` can turn final. Expected: revert with `ChainEndAccountsMissing`, no state change, and the same bid via `bid_with_settlement` settles exactly once. Pinned in Task 11 (`bid_that_turns_final_without_accounting_reverts`).
2. **A fresh, never-initialised recipient holding** (bidder collateral holding, keeper reward holding, taker protocol-token holding, owner leftover holding). Expected: succeeds when the holder signs; fails with `RecipientMustSign` (not a token-program panic) when it does not; a holding of the wrong definition fails with `WrongDefinition`. Pinned in Task 10 (`keeper_reward_to_fresh_holding`), Task 11 (`fresh_bidder_holding_must_sign`), Task 14 (`take_debt_mints_into_fresh_holding`).
3. **Donations** (a third party transfers into the host vault, an auction vault, the buffer or a surplus vault). Expected: nothing reverts; liquidation sweeps the whole host vault, a lot-emptying bid sweeps the auction vault to the bidder, `claim_leftover` sweeps to the owner, `settle_debt` burns from the actual buffer balance. Pinned in Task 5 (`donation_into_vault_is_swept`), Task 11 (`lot_emptying_bid_sweeps_donation`), Task 12 (`settle_debt_uses_actual_buffer_balance`).
4. **"Take everything" arguments**: `coin_in_max = u128::MAX`, `settle_debt(u128::MAX)`, `take_surplus`/`take_debt` with `amount_coin = u128::MAX`. Expected: no overflow panic; fills are capped at the remaining target or lot and only the filled amount moves. Pinned in Task 11 (`max_coin_in_charges_only_remaining`), Task 12 (`settle_debt_max_amount`), Task 13/14 (`take_*_max_amount_fills_lot`).
5. **The feed turns stale or the admin pauses between signing and inclusion.** Expected: `liquidate` and `bid` revert with `PriceFeedStale` / `PriceFeedLowConfidence` / `PriceFeedOutOfRange` / `Paused`; `observe_feed` succeeds and persists the pause clock; the discount does not advance over the paused interval. Pinned in Task 9 (`observe_feed_persists_stale_pause`), Task 10 (`liquidate_rejects_stale_feed_and_admin_pause`, incl. low-confidence and out-of-range), Task 11 (`bid_rejects_invalid_feed`, `discount_frozen_while_paused`).

---

## Shared Reference

### R1. PDA seeds (engine program id = `AUCTION_ENGINE_ID`)

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

### R2. Engine instructions and account lists (order = enum order = guest function order = IDL order)

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

### R3. Chained-call shapes (lee rule: a chained call's pre-states must equal the current state, and each `is_authorized` flag must equal "signer, or a PDA whose seed this call attaches"; siblings do not share PDA authorisation on the public path)

| Instruction | Chain (depth-first) | Executions |
|---|---|---|
| `init_config` | engine → `Token::InitializeAccount(stable_def, buffer*)` | 2 |
| `liquidate` | engine → `InitializeAccount(coll_def, auction_vault*)` → `stablecoin.liquidate_position(engine_authority*, …, custody=auction_vault post-init)` → [`Token::Transfer(host_vault*, custody)`] → `Token::Transfer(auction_vault* post-sweep, keeper_holding, reward)` if reward > 0 | 5 |
| `bid` / `bid_with_settlement` | engine → `Token::Burn(stable_def, bidder_stable, debt_part)` if > 0 → `Token::Transfer(bidder_stable after burn, buffer, penalty_part)` if > 0 → `Token::Transfer(auction_vault*, bidder_coll, out or whole vault on lot exhaustion)` | 4 |
| `claim_leftover` | engine → `Transfer(auction_vault*, owner_holding, whole vault)` | 2 |
| `settle_debt` | engine → `Burn(stable_def, buffer*, burned)` if > 0 | 2 |
| `start_surplus_auction` | engine → `InitializeAccount(stable_def, surplus_vault*)` → `Burn(stable_def, buffer*, implicit settle)` if > 0 → `Transfer(buffer* after burn, surplus_vault post-init, lot)` | 4 |
| `take_surplus` | engine → `Burn(protocol_def, taker_protocol, tokens)` → `Transfer(surplus_vault*, taker_stable, filled)` | 3 |
| `close_surplus_auction` | engine → `Transfer(surplus_vault*, buffer, whole vault)` if > 0 | 2 |
| `start_debt_auction` | engine → `Burn(stable_def, buffer*, implicit settle)` if > 0 | 2 |
| `take_debt` | engine → `Burn(stable_def, taker_stable, filled)` → `MintWithAuthority(protocol_def, taker_protocol, mint_authority*, minted)` | 3 |
| `close_debt_auction`, `settle_empty`, `observe_feed`, `set_*` | engine only | 1 |
| `check_solvency` | engine → `Burn(stable_def, buffer*, implicit settle)` if > 0 → `stablecoin.engine_freeze_new_debt(engine_authority*, …)` on Freeze | 3 |

`*` = authorised in that call by the engine's PDA seed. Maximum: 5 executions (`liquidate`), under the private-path limit of 10.

### R4. Engine error names and codes (`SpelError::Custom` displays `Program error {code}: {msg}` with the raw code below; only `error_code()` adds 6000, so the guest panic reads `Program error [6000+code]: Program error {code}: <Name>`; tests match on the name)

`NotAdmin 1, ZeroId 2, ZeroRefPrice 3, ThresholdBelowDebtLot 4, InvalidConfig 5, TypeCapAboveGlobal 6, BpsOutOfRange 7, ScheduleEncoding 8, TtlOutOfRange 9, ChipAbovePenalty 10, HostMismatch 11, WrongAccount 12, OracleMismatch 13, WrongClock 14, ProtocolTokenLocked 15, RefPriceStepTooLarge 16, Paused 20, PriceFeedStale 21, PriceFeedLowConfidence 22, PriceFeedOutOfRange 23, NonceMismatch 30, ZeroDebt 31, VaultBelowCollateral 32, CapacityExhausted 33, NotAuctioning 34, ZeroFill 35, Slippage 36, BelowMinBid 37, DustRemainder 38, NotOwner 39, NothingToClaim 40, ChainEndAccountsMissing 41, InsufficientBalance 42, RecipientMustSign 43, WrongDefinition 44, VaultBelowBook 45, TokenNotConfigured 50, UnbackedDebtPresent 51, BufferBelowTarget 52, DelayNotElapsed 53, InsufficientBadDebt 54, MintCapConfig 55, MintAuthorityMissing 56, ProtocolTokenMismatch 57, DebtAuctionAlreadyLive 58, SurplusAuctionAlreadyLive 59, NotLive 60, RecoveryInProgress 61, Math 99`.

### R5. Build and test commands (from Git Bash, repo root)

```bash
# one cargo command inside WSL (Task 1 creates the script)
MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh <cargo args>'
```

The first build of a guest takes minutes (risc0 local toolchain); later builds reuse `$HOME/lez-target/plan2`. Long builds: run with the Bash tool's `run_in_background` and `timeout: 3600000`, or wrap in `setsid nohup … < /dev/null &` inside WSL (S1 problem 9).

---

## File structure (this plan)

```
.gitattributes                              * text=auto eol=lf (Task 1)
scripts/wsl-cargo.sh                        WSL cargo wrapper: env, CARGO_TARGET_DIR, RISC0_DEV_MODE (Task 1)
scripts/export-hook-patches.sh              writes patches/stablecoin/*.patch from git history (Task 3)
scripts/check-hook-patches.sh               `git am` the series onto a pristine lez-programs copy (Task 3)
scripts/localnet-smoke.sh                   deploy + liquidate→bid→settle→claim on a standalone sequencer (Task 20)
DEPLOYMENTS.md                              localnet ImageIDs, reference-price change log (Task 20)
Cargo.toml                                  workspace: core, vendored programs, engine, tests, tools (Task 3)
Cargo.lock                                  seeded from lez-programs 64c01210 (Task 3)
.github/workflows/core.yml                  fmt scoped to our crates (Task 1)
crates/auction_core/src/accounting.rs       + record_settled (Task 2)
crates/auction_core/tests/invariants.rs     chain-end accounting restatement (Task 2)
programs/UPSTREAM.md                        vendored revision, vendor commit, patch list (Task 3)
programs/token/**                           vendored token program (core, program, methods, guest) (Task 3)
programs/twap_oracle/core/**                vendored OraclePriceAccount crate (Task 3)
programs/stablecoin/**                      vendored stablecoin + hooks (Tasks 3–5, 18)
programs/stablecoin/core/src/liquidation_engine_state.rs   LiquidationEngineState + PDA (Task 4)
programs/stablecoin/src/liquidation_hooks.rs set_liquidation_engine, liquidate_position, engine_freeze_new_debt + unit tests (Tasks 4–5)
programs/stablecoin/methods_b/**            instance-B methods crate (feature instance-b) (Task 18)
patches/stablecoin/*.patch                  exported hook patch series, outside programs/stablecoin (Tasks 3–5)
programs/{stablecoin,token,twap_oracle}/{clippy.toml,rustfmt.toml}   upstream lint/format config (Task 3)
programs/upstream_integration_tests/**      vendored upstream integration_tests (stablecoin.rs only) (Task 3)
programs/auction_engine/core/src/{lib,pda,state,instruction,error,args}.rs   auction_engine_core (Task 6)
programs/auction_engine/src/lib.rs          module list, EngineResult (Task 8)
programs/auction_engine/src/accounts.rs     account loading/validation helpers (Task 8)
programs/auction_engine/src/token_calls.rs  chained token-call builders and simulated holding states (Task 8)
programs/auction_engine/src/admin.rs        init_config, set_engine_params (Task 8), set_collateral_config (Task 9)
programs/auction_engine/src/feed.rs         redemption price, oracle read, pause observation (Task 9)
programs/auction_engine/src/observe.rs      observe_feed (Task 9)
programs/auction_engine/src/liquidate.rs    liquidate (Task 10)
programs/auction_engine/src/bid.rs          bid, bid_with_settlement (Task 11)
programs/auction_engine/src/settle.rs       settle_empty, claim_leftover, settle_debt (Task 12)
programs/auction_engine/src/system.rs       implicit settle, ref price, protocol-token status (Task 13)
programs/auction_engine/src/surplus.rs      start/take/close surplus (Task 13)
programs/auction_engine/src/debt.rs         start/take/close debt (Task 14)
programs/auction_engine/src/solvency.rs     check_solvency (Task 15)
programs/auction_engine/methods/{Cargo.toml,build.rs,src/lib.rs}             embeds AUCTION_ENGINE_ELF/ID (Task 8)
programs/auction_engine/methods/guest/{Cargo.toml,Cargo.lock,src/bin/auction_engine.rs}  SPEL dispatch (Tasks 8–15)
programs/test_programs/**                   attacker guest `impostor` (Task 16)
programs/integration_tests/Cargo.toml       package engine_integration_tests (Task 7, engine deps Task 8)
programs/integration_tests/tests/common/mod.rs      World harness (Task 7, extended per task)
programs/integration_tests/tests/common/measure.rs  chain replay with cycle counts (Task 17)
programs/integration_tests/tests/{harness,hooks,config,idl,observe,liquidate,bid,settle,surplus,debt,solvency,security,invariants,chains,two_instances}.rs
programs/integration_tests/examples/localnet_tool.rs   export guest binaries, print localnet PDAs (Task 20)
idl/{stablecoin,auction_engine}-idl.json    generated IDLs (Tasks 4, 8–15)
tools/idl-gen/**                            vendored IDL generator (Task 3)
THREAT_MODEL.md                             (Task 19)
docs/architecture/hooks.md                  stablecoin hooks (Tasks 4–5)
docs/architecture/programs.md               engine program, accounts, Plan 3 interfaces (Tasks 6–17)
docs/architecture/security-checklist.md     per-instruction checklist (Task 19)
docs/architecture/localnet.md               localnet smoke commands and result (Task 20)
docs/design-log.md                          ADR-025 … ADR-036 (owning tasks)
```

---
### Task 1: Repository plumbing (line endings, WSL build wrapper)

**Files:**
- Create: `.gitattributes`
- Create: `scripts/wsl-cargo.sh`
- Modify: `README.md` (new section "Build and test the programs (WSL)")

**Interfaces:**
- Consumes: the S1 environment file `~/lez-spikes/s1/env.sh` inside WSL (libclang, sysroot and rzup paths; see `docs/spikes/S1.md` "Missing system packages").
- Produces: `scripts/wsl-cargo.sh <cargo args…>` — runs `cargo` in WSL with `CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$HOME/lez-target/plan2}`, `RISC0_DEV_MODE=${RISC0_DEV_MODE:-1}`, the S1 environment sourced, and the repo root as working directory. Every later task builds and tests through it (Shared Reference R5).

- [ ] **Step 1: Show the failure modes this task removes**

Run (Git Bash, repo root):
```bash
git ls-files --eol | grep -c 'i/crlf' ; MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh --version'
```
Expected: the first number may be 0 or more (CRLF files in the index on this autocrlf checkout); the second command fails with `bash: scripts/wsl-cargo.sh: No such file or directory`.

- [ ] **Step 2: Add `.gitattributes`**

```gitattributes
# Controller ruling (Plan 2): WSL builds and shell scripts break on CRLF; the repo lives on C: with autocrlf.
* text=auto eol=lf
*.png binary
*.bin binary
```

- [ ] **Step 3: Renormalise the index and the working tree**

Run (Git Bash; `git status` was clean at plan start, so nothing local is lost):
```bash
git add --renormalize .
git rm --cached -r -q . && git reset -q --hard
git add --renormalize . && git status --short | head -50
git ls-files --eol | grep -v 'i/lf' | grep -v 'i/-text' | grep -v 'i/none'
```
Expected: the last command prints nothing (every text file is stored with LF). If `git status` before Step 2 showed local changes, stop and ask the controller instead of running `reset --hard`.

- [ ] **Step 4: Write `scripts/wsl-cargo.sh`**

```bash
#!/usr/bin/env bash
# Runs one cargo command inside WSL for this repository (Plan 2 build convention).
# From Git Bash:
#   MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc \
#     'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_core'
# The target directory lives on the WSL-native disk; reading sources over /mnt/c is slow, writing targets there is slower.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
env_file="${LEZ_ENV_FILE:-$HOME/lez-spikes/s1/env.sh}"
if [[ ! -f "$env_file" ]]; then
  echo "wsl-cargo.sh: missing $env_file (see docs/spikes/S1.md, 'Missing system packages')" >&2
  exit 2
fi
# shellcheck disable=SC1090
. "$env_file"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/lez-target/plan2}"
export RISC0_DEV_MODE="${RISC0_DEV_MODE:-1}"
mkdir -p "$CARGO_TARGET_DIR"
cd "$repo_root"
exec cargo "$@"
```

Then (Git Bash): `git add --chmod=+x scripts/wsl-cargo.sh`.

- [ ] **Step 5: Run the core suite through the wrapper**

Run:
```bash
MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh --version && scripts/wsl-cargo.sh test -q -p auction_core'
```
Expected: `cargo 1.94.0 (…)`, then every `auction_core` test passes (`test result: ok.`, 0 failed).

- [ ] **Step 6: Document the convention in `README.md`**

Append after the "Run the tests" section:

````markdown
## Build and test the programs (WSL)

The on-chain programs build only inside WSL (risc0 toolchain, `~/lez-spikes/s1/env.sh` from spike S1). Run every cargo command through the wrapper; it keeps the target directory on the WSL disk (`$HOME/lez-target/plan2`) and sets `RISC0_DEV_MODE=1`:

```bash
MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc \
  'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p engine_integration_tests'
```

`MSYS_NO_PATHCONV=1` stops Git Bash from rewriting `/mnt/...` paths. Commit from Git Bash on Windows (GPG signing), not from WSL. The repository stores text with LF line endings (`.gitattributes`).
````

- [ ] **Step 7: Commit (controller — GPG signing on Windows)**

The implementer stages the files and reports; the controller commits from Git Bash.

```bash
git add .gitattributes scripts/wsl-cargo.sh README.md
git commit -m "chore: LF line endings and WSL cargo wrapper for program builds"
```
(Files renormalised in Step 3 are already staged and go into this commit; name them in the body as "renormalised to LF".)

---

### Task 2: `auction_core` chain-end accounting helper (`Accounting::record_settled`)

**Why a core change:** ADR-023 consequence 2 and ADR-024 require every chain-end path to fold an auction's cumulative `debt_raised`/`penalty_raised` and book its shortfall exactly once, and recommend a named core helper so no program builds a synthetic `BidFill`. This is the only core change in Plan 2.

**Files:**
- Modify: `crates/auction_core/src/accounting.rs` (add `record_settled` after `record_chain_end`, around line 32; add a test module; change line 2 import)
- Modify: `crates/auction_core/tests/invariants.rs` (line 49 and the `auction_invariants` body, lines 84–148)
- Modify: `docs/architecture/core.md` ("Accounting" section)

**Interfaces:**
- Consumes: `auction_core::auction::{Auction, ChainEnd, Status, settle_empty}`; `Accounting::{record_bid, record_chain_end}`.
- Produces: `pub fn record_settled(self, a: &Auction, e: &ChainEnd) -> Accounting` — adds `a.debt_raised` to `total_debt_burned` and `a.penalty_raised` to `total_penalty_collected` (saturating), then applies `record_chain_end(e)`. Used by Tasks 11 and 12 on every chain-end path.

- [ ] **Step 1: Write the failing test**

Append to `crates/auction_core/src/accounting.rs`:

```rust
#[cfg(test)]
#[allow(clippy::arithmetic_side_effects)] // the crate denies it; test sums of small constants
mod record_settled_tests {
    use super::*;
    use crate::auction::{apply_bid, quote_bid, settle_empty, Auction, BidRequest, ChainEnd, Status};
    use crate::liquidation::LiquidationQuote;
    use crate::schedule::{Schedule, SchedulePoint};
    use crate::units::RAY;

    fn flat() -> Schedule {
        Schedule::new(&[SchedulePoint {
            elapsed_ms: 0,
            discount_bps: 0,
        }])
        .unwrap()
    }

    #[test]
    fn fold_at_chain_end_equals_per_bid_fold() {
        let q = LiquidationQuote {
            debt_principal: 900,
            amount_to_raise: 990,
            reserve: 990,
            keeper_reward: 0,
            lot: 2_000,
            seized: 2_000,
        };
        let mut a = Auction::open(&q, 0, flat(), 0);
        let mut per_bid = Accounting::default();
        let mut end = None;
        for coin in [400u128, 590] {
            let req = BidRequest {
                coin_in_max: coin,
                min_collateral_out: 0,
            };
            let f = quote_bid(&a, &req, RAY, 0, 25, 110).unwrap();
            per_bid = per_bid.record_bid(&f);
            let (n, e) = apply_bid(&a, &f);
            a = n;
            end = e;
        }
        let e: ChainEnd = end.expect("second bid reaches the target");
        assert_eq!(a.status, Status::Settled);
        let per_bid = per_bid.record_chain_end(&e);
        let at_end = Accounting::default().record_settled(&a, &e);
        assert_eq!(at_end, per_bid);
        assert_eq!(
            at_end.total_debt_burned + at_end.total_penalty_collected,
            a.raised()
        );
    }

    #[test]
    fn settle_empty_with_earlier_raise_books_raised_and_shortfall() {
        let q = LiquidationQuote {
            debt_principal: 900,
            amount_to_raise: 990,
            reserve: 990,
            keeper_reward: 0,
            lot: 0,
            seized: 0,
        };
        let mut a = Auction::open(&q, 0, flat(), 0);
        a.debt_raised = 300; // raised by earlier bids before the lot ran out
        let (s, e) = settle_empty(&a).unwrap();
        let acc = Accounting::default().record_settled(&s, &e);
        assert_eq!(acc.total_debt_burned, 300);
        assert_eq!(acc.unbacked_debt, 600);
        assert_eq!(acc.total_shortfall_booked, 600);
    }
}
```

- [ ] **Step 2: Run it and see it fail**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_core --lib record_settled'`
Expected: compile error ``no method named `record_settled` found for struct `Accounting` ``.

- [ ] **Step 3: Implement `record_settled`**

Change line 2 of `crates/auction_core/src/accounting.rs` to `use crate::auction::{Auction, BidFill, ChainEnd};` and insert after `record_chain_end`:

```rust
    /// Chain-end fold (ADR-023): `Accounting` is not written by non-final bids, so the settling
    /// transition adds the auction's cumulative raised amounts and books its shortfall, once.
    /// Call it on every chain-end path (`apply_bid` returning `Some(ChainEnd)`, `settle_empty`).
    pub fn record_settled(self, a: &Auction, e: &ChainEnd) -> Self {
        debug_assert!(a.status == crate::auction::Status::Settled);
        Self {
            total_debt_burned: self.total_debt_burned.saturating_add(a.debt_raised),
            total_penalty_collected: self.total_penalty_collected.saturating_add(a.penalty_raised),
            ..self
        }
        .record_chain_end(e)
    }
```

- [ ] **Step 4: Run it and see it pass**

Run: the Step 2 command. Expected: `2 passed`.

- [ ] **Step 5: Restate the property test for chain-end accounting**

In `crates/auction_core/tests/invariants.rs`:
- in `check_settle_empty` replace `let acc = Accounting::default().record_chain_end(&e);` with `let acc = Accounting::default().record_settled(&s, &e);`;
- in `auction_invariants` delete the line `acc = acc.record_bid(&f);`;
- inside `if let Some(e) = end {` replace `acc = acc.record_chain_end(&e);` with `acc = acc.record_settled(&n, &e);`;
- replace the final `prop_assert_eq!(acc.total_debt_burned + acc.total_penalty_collected, a.raised());` with:

```rust
        // ADR-023: the counters lag until chain end, then equal the auction's cumulative raise.
        if a.status == Status::Settled {
            prop_assert_eq!(acc.total_debt_burned + acc.total_penalty_collected, a.raised());
        } else {
            prop_assert_eq!(acc.total_debt_burned + acc.total_penalty_collected, 0);
        }
```

- [ ] **Step 6: Run the full core suite, clippy and the differential check**

Run:
```bash
MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh fmt -p auction_core -- --check && scripts/wsl-cargo.sh clippy -p auction_core --all-targets -- -D warnings && PROPTEST_CASES=2000 scripts/wsl-cargo.sh test -p auction_core && scripts/wsl-cargo.sh run -q -p auction_core --example vectors > /tmp/vectors.json && python3 tools/differential/check.py /tmp/vectors.json'
```
Expected: everything passes; the differential checker prints its OK line.

- [ ] **Step 7: Update `docs/architecture/core.md`**

In the "Accounting" section add after the `record_chain_end` bullet:

```markdown
- `record_settled(&Auction, &ChainEnd)`: the chain-end fold the program uses (ADR-023). Non-final bids do not write `Accounting`; the settling transition adds the auction's cumulative `debt_raised` to `total_debt_burned` and `penalty_raised` to `total_penalty_collected`, then applies `record_chain_end`. Every chain-end path (settling bid, `settle_empty`) calls it exactly once. For live auctions the raised amounts live only in the `Auction`; dashboards must add them (ADR-023 consequence 4).
```

- [ ] **Step 8: Commit (controller — GPG signing on Windows)**

The implementer stages the files and reports; the controller commits from Git Bash.

```bash
git add crates/auction_core/src/accounting.rs crates/auction_core/tests/invariants.rs docs/architecture/core.md
git commit -m "feat(core): Accounting::record_settled chain-end fold; restate invariants for chain-end accounting"
```

---

### Task 3: Vendor lez-programs `64c01210` (stablecoin, token, oracle core, idl-gen) and keep the upstream suite green

**Files:**
- Create (vendored, pristine, from `git archive 64c01210adecd6b557dbd021a3cc94553f243fd8` in `~/lez-spikes/s1/lez-programs`): `programs/token/**`, `programs/twap_oracle/core/**`, `programs/stablecoin/**`, `programs/upstream_integration_tests/{Cargo.toml,src/lib.rs,tests/stablecoin.rs}` (upstream path `programs/integration_tests/`), `tools/idl-gen/**`, `programs/LICENSE-lez-programs` (upstream `LICENSE`), `idl/upstream/stablecoin-idl.json` and `idl/upstream/token-idl.json` (upstream `artifacts/`)
- Modify: `programs/upstream_integration_tests/Cargo.toml` (drop the amm/ata/token_mint_authority dependencies whose tests are not vendored), `Cargo.toml` (workspace), `Cargo.lock`, `.github/workflows/core.yml` (fmt scope)
- Create: `programs/UPSTREAM.md`, `scripts/export-hook-patches.sh`, `scripts/check-hook-patches.sh`, `idl/stablecoin-idl.json` (regenerated by the vendored `idl-gen`), `programs/{stablecoin,token,twap_oracle}/{clippy.toml,rustfmt.toml}` (upstream root config)
- Modify: `docs/design-log.md` (ADR-025)

**Interfaces:**
- Consumes: the pristine clone `~/lez-spikes/s1/lez-programs` (its working tree carries spike edits; always read through `git archive 64c01210…`, never copy the working tree).
- Produces: workspace packages `token_core`, `token_program`, `token-methods` (`TOKEN_ELF`, `TOKEN_ID`), `twap_oracle_core` (`OraclePriceAccount`), `stablecoin_core`, `stablecoin_program`, `stablecoin-methods` (`STABLECOIN_ELF`, `STABLECOIN_ID`), `integration_tests` (upstream suite), `idl-gen` (binary: `idl-gen <guest source file>` prints IDL JSON). The vendor commit's subject is exactly `vendor: lez-programs 64c01210 (stablecoin, token, twap_oracle core, idl-gen)`; `scripts/export-hook-patches.sh` finds it by that subject.

- [ ] **Step 1: Confirm the packages do not exist yet**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p stablecoin_program --no-run'`
Expected: `error: package ID specification \`stablecoin_program\` did not match any packages`.

- [ ] **Step 2: Extract the pristine sources**

Run (one WSL call; `-e` hands the script to bash unchanged, so `$HOME`, `$R`, `$U`, `$REV` expand inside WSL, not in Git Bash):
```bash
MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'set -euo pipefail
R=/mnt/c/Users/user/Documents/basecamp/lez-auction-engine; U=$HOME/lez-spikes/s1/lez-programs; REV=64c01210adecd6b557dbd021a3cc94553f243fd8
git -C "$U" cat-file -e "$REV^{commit}"
git -C "$U" archive "$REV" programs/stablecoin programs/token programs/twap_oracle/core tools/idl-gen | tar -x -C "$R"
mkdir -p "$R/programs/upstream_integration_tests/src" "$R/programs/upstream_integration_tests/tests" "$R/idl/upstream"
git -C "$U" show "$REV:programs/integration_tests/Cargo.toml"        > "$R/programs/upstream_integration_tests/Cargo.toml"
git -C "$U" show "$REV:programs/integration_tests/src/lib.rs"        > "$R/programs/upstream_integration_tests/src/lib.rs"
git -C "$U" show "$REV:programs/integration_tests/tests/stablecoin.rs" > "$R/programs/upstream_integration_tests/tests/stablecoin.rs"
git -C "$U" show "$REV:LICENSE"                                      > "$R/programs/LICENSE-lez-programs"
git -C "$U" show "$REV:artifacts/stablecoin-idl.json"                > "$R/idl/upstream/stablecoin-idl.json"
git -C "$U" show "$REV:artifacts/token-idl.json"                     > "$R/idl/upstream/token-idl.json"
git -C "$U" show "$REV:Cargo.lock"                                   > "$R/programs/Cargo.lock.upstream"
ls "$R/programs"'
```
Expected: `programs/` lists `LICENSE-lez-programs Cargo.lock.upstream stablecoin token twap_oracle upstream_integration_tests`.

- [ ] **Step 3: Commit (controller — GPG signing on Windows) — the pristine vendor tree (no edits yet)**

The implementer stages the files and reports; the controller commits from Git Bash.

```bash
git add programs/stablecoin programs/token programs/twap_oracle programs/upstream_integration_tests programs/LICENSE-lez-programs tools/idl-gen idl/upstream
git commit -m "vendor: lez-programs 64c01210 (stablecoin, token, twap_oracle core, idl-gen)

Pristine git archive of logos-blockchain/lez-programs 64c01210adecd6b557dbd021a3cc94553f243fd8.
programs/integration_tests is vendored as programs/upstream_integration_tests (Cargo.toml,
src/lib.rs, tests/stablecoin.rs only). No local edits in this commit."
```
(`programs/Cargo.lock.upstream` stays untracked; Step 5 consumes and deletes it.)

- [ ] **Step 4: Wire the workspace**

Replace the root `Cargo.toml` with:

```toml
[workspace]
members = [
    "crates/auction_core",
    "programs/token/core",
    "programs/token",
    "programs/token/methods",
    "programs/twap_oracle/core",
    "programs/stablecoin/core",
    "programs/stablecoin",
    "programs/stablecoin/methods",
    "programs/upstream_integration_tests",
    "tools/idl-gen",
]
exclude = [
    "spikes",
    "programs/token/methods/guest",
    "programs/stablecoin/methods/guest",
]
resolver = "2"

[workspace.package]
edition = "2021"
license = "MIT OR Apache-2.0"

[workspace.dependencies]
lee_core = { git = "https://github.com/logos-blockchain/logos-execution-zone.git", tag = "v0.2.4", features = ["host"] }
lee = { git = "https://github.com/logos-blockchain/logos-execution-zone.git", tag = "v0.2.4", features = ["test-utils"] }
token_core = { path = "programs/token/core" }
token_program = { path = "programs/token" }
twap_oracle_core = { path = "programs/twap_oracle/core" }
stablecoin_core = { path = "programs/stablecoin/core" }
stablecoin_program = { path = "programs/stablecoin" }
serde = { version = "1.0", features = ["derive"] }
borsh = { version = "1.5", features = ["derive"] }
risc0-zkvm = { version = "=3.0.5" }
serde_json = "1.0"

# Copied verbatim from lez-programs 64c01210 Cargo.toml: the vendored crates opt in with `[lints] workspace = true`.
[workspace.lints.rust]
rust_2018_idioms = { level = "deny", priority = -1 }
unsafe_code = "deny"

[workspace.lints.clippy]
correctness = { level = "deny", priority = -1 }
suspicious  = { level = "deny", priority = -1 }
perf        = { level = "deny", priority = -1 }
style       = { level = "warn", priority = -1 }
dbg_macro = "deny"
todo = "deny"
unimplemented = "deny"
unwrap_used = "deny"
allow_attributes = "warn"
allow_attributes_without_reason = "deny"
arithmetic_side_effects = "deny"
indexing_slicing = "deny"
as_conversions = "deny"
cast_possible_truncation = "deny"
cast_possible_wrap = "deny"
cast_sign_loss = "deny"
large_enum_variant = "deny"
wildcard_enum_match_arm = "deny"
module_name_repetitions = "allow"
similar_names = "allow"

[profile.release]
overflow-checks = true

[profile.test]
overflow-checks = true
```

Copy upstream's root lint/format config next to the vendored crates (local adjustment, recorded in `programs/UPSTREAM.md`):

```bash
MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'set -euo pipefail
R=/mnt/c/Users/user/Documents/basecamp/lez-auction-engine; U=$HOME/lez-spikes/s1/lez-programs; REV=64c01210adecd6b557dbd021a3cc94553f243fd8
for d in programs/stablecoin programs/token programs/twap_oracle; do
  git -C "$U" show "$REV:clippy.toml" > "$R/$d/clippy.toml"
  git -C "$U" show "$REV:rustfmt.toml" > "$R/$d/rustfmt.toml"
done
rustup toolchain list'
```

Record the toolchain list in `programs/UPSTREAM.md` (expected: no nightly; then hook tasks use the stable check below). After Step 5, record the formatting baseline of the pristine stablecoin with stable rustfmt and the vendored config: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh fmt -p stablecoin_core -p stablecoin_program -- --check > $HOME/lez-target/plan2-stablecoin-fmt-baseline.txt 2>&1; wc -l $HOME/lez-target/plan2-stablecoin-fmt-baseline.txt'` (nightly-only options are ignored with warnings; the line count is the baseline). Hook tasks require that `fmt --check` reports no diff in any hook-added line.

In `programs/upstream_integration_tests/Cargo.toml` delete exactly these dependency lines (their tests are not vendored): `amm_core`, `ata_core`, `token_mint_authority_core`, `amm-methods`, `ata-methods`, `twap-oracle-methods`, `token-mint-authority-methods`. Keep `[lints] workspace = true`, `lee`, `lee_core`, `clock_core`, `key_protocol`, `token_core`, `stablecoin_core`, `twap_oracle_core`, `token-methods`, `stablecoin-methods`, and add `risc0-zkvm = { workspace = true }` only if the build in Step 6 asks for it.

- [ ] **Step 5: Seed `Cargo.lock` from upstream and keep the core's dev-dependency versions**

Run:
```bash
MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'set -euo pipefail; cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine
cp programs/Cargo.lock.upstream Cargo.lock && rm programs/Cargo.lock.upstream
scripts/wsl-cargo.sh metadata --format-version 1 > /dev/null
scripts/wsl-cargo.sh update -p proptest --precise 1.11.0
grep -A1 "name = \"ruint\"" Cargo.lock'
```
Expected: `ruint` resolves to `1.17.0` (stablecoin_core pins `=1.17.0`; `auction_core`'s dev-dependency `ruint = "1"` unifies to it). The upstream lock keeps every shared crate at the version lez-programs tested; cargo prunes the amm/ata entries.

- [ ] **Step 6: Run the upstream suite (build is long the first time)**

Run (background; up to an hour cold):
```bash
MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p stablecoin_core -p stablecoin_program -p token_program -p integration_tests 2>&1 | tee $HOME/lez-target/plan2-upstream-suite.log | grep "test result"'
```
Expected: every `test result: ok.` line, 0 failed. Record the summed passed count in `programs/UPSTREAM.md` (Step 8) as the baseline. Then `scripts/wsl-cargo.sh test -q -p auction_core` still passes (ruint 1.17.0).

- [ ] **Step 7: Regenerate the stablecoin IDL with the vendored tool and compare**

Run:
```bash
MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh run -q -p idl-gen -- programs/stablecoin/methods/guest/src/bin/stablecoin.rs > idl/stablecoin-idl.json && diff idl/stablecoin-idl.json idl/upstream/stablecoin-idl.json && echo IDL-IDENTICAL'
```
Expected: `IDL-IDENTICAL` (19 instructions, `initialize_program` … `unfreeze`).

- [ ] **Step 8: Write `programs/UPSTREAM.md` and `scripts/export-hook-patches.sh`**

`programs/UPSTREAM.md`:

```markdown
# Vendored upstream code

| Path here | Upstream path | Repository | Revision |
|---|---|---|---|
| `programs/stablecoin/` | `programs/stablecoin/` | logos-blockchain/lez-programs | `64c01210adecd6b557dbd021a3cc94553f243fd8` |
| `programs/token/` | `programs/token/` | same | same |
| `programs/twap_oracle/core/` | `programs/twap_oracle/core/` | same | same |
| `programs/upstream_integration_tests/` | `programs/integration_tests/` (only `Cargo.toml`, `src/lib.rs`, `tests/stablecoin.rs`) | same | same |
| `tools/idl-gen/` | `tools/idl-gen/` | same | same |
| `idl/upstream/*.json` | `artifacts/*.json` | same | same |

The vendor commit has the subject `vendor: lez-programs 64c01210 (stablecoin, token, twap_oracle core, idl-gen)` and contains no local edits.

## Local adjustments (not offered upstream)

- `programs/upstream_integration_tests/Cargo.toml`: amm, ata and token_mint_authority dependencies removed (their tests are not vendored).
- Root `Cargo.toml` carries upstream's `[workspace.lints]` verbatim so `[lints] workspace = true` resolves.
- Upstream's root `clippy.toml` and `rustfmt.toml` are copied into `programs/stablecoin/`, `programs/token/` and `programs/twap_oracle/` (not the repo root, so our crates keep their own style). The stablecoin crates do not opt into `[workspace.lints]` upstream either; their clippy gate is `-D warnings` with this `clippy.toml`. No nightly toolchain is installed in WSL (`rustup toolchain list`: stable, 1.94.0, risc0), so upstream's `cargo +nightly fmt` cannot run here; formatting is checked with stable rustfmt and this config against the Task 3 baseline (Task 3 Step 4).

## Hook patch series (offered upstream)

Every later commit touching `programs/stablecoin/` (except commits whose subject contains `[local]`) is one patch. Regenerate with `scripts/export-hook-patches.sh`; the files land in `patches/stablecoin/` at the repo root (outside `programs/stablecoin/`, so a patch never contains an earlier patch) and apply at the lez-programs root with `git am`; `scripts/check-hook-patches.sh` proves that on a pristine copy.

## Upstream suite baseline

`scripts/wsl-cargo.sh test -p stablecoin_core -p stablecoin_program -p token_program -p integration_tests`: <N> passed, 0 failed at the vendor commit (N from Task 3 Step 6). Every hook task re-runs it and must keep 0 failed.
```

Replace `<N>` with the number from Step 6 before committing (it is a measured value, not a placeholder for later).

`scripts/export-hook-patches.sh`:

```bash
#!/usr/bin/env bash
# Exports the stablecoin hook patch series (ADR-025): one patch per commit after the vendor commit that
# touches programs/stablecoin/, skipping commits whose subject contains "[local]". Paths are identical to
# upstream's, so the patches apply with `git am` at the lez-programs root.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
base="$(git log --format=%H -1 --grep='^vendor: lez-programs 64c01210')"
if [[ -z "$base" ]]; then echo "export-hook-patches: vendor commit not found" >&2; exit 1; fi
out=patches/stablecoin
mkdir -p "$out"; rm -f "$out"/*.patch
n=0
for sha in $(git rev-list --reverse "$base"..HEAD -- programs/stablecoin); do
  if git log -1 --format=%s "$sha" | grep -q '\[local\]'; then continue; fi
  n=$((n + 1))
  git format-patch -1 "$sha" --start-number "$n" -o "$out" --quiet -- programs/stablecoin \
    ':(exclude)programs/stablecoin/methods_b' ':(exclude)programs/stablecoin/clippy.toml' ':(exclude)programs/stablecoin/rustfmt.toml'
done
echo "export-hook-patches: $n patch(es) in $out"
```

`scripts/check-hook-patches.sh` (runs inside WSL; applies the exported series to a pristine `git archive` of upstream in a scratch directory):

```bash
#!/usr/bin/env bash
# Proves the exported hook series applies with `git am` to pristine lez-programs 64c01210 (ADR-025).
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
upstream="${LEZ_PROGRAMS:-$HOME/lez-spikes/s1/lez-programs}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
git -C "$upstream" archive 64c01210adecd6b557dbd021a3cc94553f243fd8 | tar -x -C "$tmp"
cd "$tmp"
git init -q && git add -A && git -c user.email=check@local -c user.name=check commit -qm pristine
shopt -s nullglob
patches=("$repo"/patches/stablecoin/*.patch)
if (( ${#patches[@]} == 0 )); then echo "check-hook-patches: no patches"; exit 0; fi
git -c user.email=check@local -c user.name=check am -q "${patches[@]}"
echo "check-hook-patches: ${#patches[@]} patch(es) apply cleanly"
```

Run `git add --chmod=+x scripts/export-hook-patches.sh scripts/check-hook-patches.sh`, then `bash scripts/export-hook-patches.sh` (Git Bash). Expected: `export-hook-patches: 0 patch(es) in patches/stablecoin` (no hook commits yet). Then `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/check-hook-patches.sh'`. Expected: `check-hook-patches: no patches`.

- [ ] **Step 9: Scope `cargo fmt` in CI to our own crates**

In `.github/workflows/core.yml` replace `- run: cargo fmt --all -- --check` with `- run: cargo fmt -p auction_core -- --check`. Reason (comment above the line): vendored crates keep upstream formatting (`cargo +nightly fmt` per lez-programs `Makefile`); our crates are listed explicitly and later tasks append `-p auction_engine_core -p auction_engine_program -p engine_integration_tests`.

- [ ] **Step 10: Record ADR-025 in `docs/design-log.md`**

```markdown
---

## ADR-025: Vendored Stablecoin as a Committed Tree; Hooks Exported as a Patch Series

**Context**
Spec §4.2 asks for `programs/stablecoin/` "vendored @64c0120 + hook patch series" and §1 for the hooks to be offered upstream with the upstream suite unchanged. The stablecoin path-depends on `programs/token/core` and `programs/twap_oracle/core`, and its tests need the token guest.

**Options**
- (A) Pristine sources plus `.patch` files applied at build time (quilt style).
- (B) A pristine vendor commit, hooks as ordinary commits on top, and the patch series exported from history.

**Decision**
(B). The vendor commit (`vendor: lez-programs 64c01210 …`) is a `git archive` of upstream with no edits; token, twap_oracle core and idl-gen are vendored at the same paths so the stablecoin's relative path dependencies resolve unchanged. Each hook commit touches only what upstream needs; `scripts/export-hook-patches.sh` writes one patch per commit into `patches/stablecoin/` at the repo root (outside `programs/stablecoin/`, so no patch ever carries an earlier exported patch; commits tagged `[local]`, such as the instance-B build, are skipped). Patch paths equal upstream paths, so `git am` applies them at the lez-programs root; `scripts/check-hook-patches.sh` checks exactly that against a pristine `git archive` after every export.

**Trade-offs**
Builds, IDE and tests see one tree, and the series is reproducible from history. A rebase onto a newer upstream is a new pristine vendor commit followed by `git am` of the exported series. The upstream integration tests live at `programs/upstream_integration_tests/` (upstream `programs/integration_tests/`) because our engine tests own the spec's `programs/integration_tests/` path; its manifest is trimmed of crates we do not vendor (`programs/UPSTREAM.md`). Formatting of vendored code follows upstream (nightly rustfmt, not available in WSL here; stable rustfmt with the vendored config is the local check), so CI formats only our crates. Upstream's `clippy.toml`/`rustfmt.toml` live next to the vendored crates, not at the repo root.

**Status**
Accepted (Plan 2, Task 3)
```

- [ ] **Step 11: Commit (controller — GPG signing on Windows) — the wiring**

The implementer stages the files and reports; the controller commits from Git Bash.

```bash
git add Cargo.toml Cargo.lock programs/upstream_integration_tests/Cargo.toml programs/UPSTREAM.md programs/stablecoin/clippy.toml programs/stablecoin/rustfmt.toml programs/token/clippy.toml programs/token/rustfmt.toml programs/twap_oracle/clippy.toml programs/twap_oracle/rustfmt.toml scripts/export-hook-patches.sh scripts/check-hook-patches.sh idl/stablecoin-idl.json .github/workflows/core.yml docs/design-log.md
git commit -m "build: wire vendored lez-programs into the workspace; upstream suite green; ADR-025"
```

---

### Task 4: Stablecoin hooks A — engine binding, Variant C guard, `engine_freeze_new_debt`, appended order

**Files:**
- Create: `programs/stablecoin/core/src/liquidation_engine_state.rs`
- Modify: `programs/stablecoin/core/src/lib.rs` (add `pub mod liquidation_engine_state;` after line 11; re-export after line 31; append three variants immediately after RepayDebt's closing `},` (L376 in the reviewer's numbering; `cat -n` shows `},` on line 375 and the enum's `}` on line 376 — insert between them); add the order test module at the end of the file)
- Create: `programs/stablecoin/src/liquidation_hooks.rs`
- Modify: `programs/stablecoin/src/lib.rs` (add the module after line 42 `pub mod withdraw_collateral;`)
- Modify: `programs/stablecoin/methods/guest/src/bin/stablecoin.rs` (append two `#[instruction]` functions after `unfreeze`, before the module's closing `}` on line 707; Task 5 appends the third in between, see Step 7)
- Modify: `idl/stablecoin-idl.json` (regenerated)
- Create: `docs/architecture/hooks.md`; Modify: `docs/design-log.md` (ADR-026)

**Interfaces:**
- Consumes: `crate::checks::{decode_global, assert_admin_authorized}` (pub(crate), `src/checks.rs:105` (assert at `:109`), `:159`), `stablecoin_core::{compute_protocol_parameters_pda, ProtocolParameters}`, `lee_core::program::DEFAULT_PROGRAM_ID` (`[0; 8]`), `crate::test_support::*` (cfg(test) fixtures, `src/test_support.rs`).
- Produces:
  - `stablecoin_core::LiquidationEngineState { engine_program_id: ProgramId, engine_authority_id: AccountId }` (borsh), `compute_liquidation_engine_state_pda_seed() -> PdaSeed`, `compute_liquidation_engine_state_pda(ProgramId) -> AccountId`.
  - `stablecoin_core::Instruction::{SetLiquidationEngine { engine_program_id: ProgramId, engine_authority_id: AccountId }, LiquidatePosition { collateral_to_seize: u128, normalized_debt_to_cover: u128 }, EngineFreezeNewDebt}` at enum indices 19, 20, 21.
  - `stablecoin_program::liquidation_hooks::{set_liquidation_engine, engine_freeze_new_debt, assert_engine_call, MAX_PENALTY_BPS}` (Task 5 adds `liquidate_position`).
  - Guest functions `set_liquidation_engine`, `engine_freeze_new_debt` (IDL 19 and 21 once Task 5 inserts `liquidate_position` at 20).

- [ ] **Step 1: Write the failing unit tests**

Create `programs/stablecoin/src/liquidation_hooks.rs` with only the test module for now:

```rust
#[cfg(test)]
#[allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "test fixtures build accounts from constants, index fixed-size outputs and assert panics via #[should_panic]"
)]
mod tests {
    use lee_core::{
        account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
        program::{ProgramId, DEFAULT_PROGRAM_ID},
    };
    use stablecoin_core::{
        compute_liquidation_engine_state_pda, LiquidationEngineState, ProtocolParameters,
    };

    use super::*;
    use crate::test_support::*;

    pub(super) const ENGINE: ProgramId = [0xE1; 8];
    pub(super) const IMPOSTOR: ProgramId = [0xBA; 8];

    pub(super) fn engine_authority_id() -> AccountId {
        AccountId::new([0xEA; 32])
    }
    pub(super) fn signer(id: AccountId) -> AccountWithMetadata {
        AccountWithMetadata { account: Account::default(), is_authorized: true, account_id: id }
    }
    pub(super) fn engine_authority(authorized: bool) -> AccountWithMetadata {
        AccountWithMetadata { account: Account::default(), is_authorized: authorized, account_id: engine_authority_id() }
    }
    pub(super) fn engine_state() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: STABLECOIN_PROGRAM_ID,
                balance: 0,
                data: Data::from(&LiquidationEngineState {
                    engine_program_id: ENGINE,
                    engine_authority_id: engine_authority_id(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: compute_liquidation_engine_state_pda(STABLECOIN_PROGRAM_ID),
        }
    }
    fn unbound_state() -> AccountWithMetadata {
        uninitialized(compute_liquidation_engine_state_pda(STABLECOIN_PROGRAM_ID))
    }
    fn decode_params(post: &lee_core::program::AccountPostState) -> ProtocolParameters {
        ProtocolParameters::try_from(&post.account().data).unwrap()
    }

    #[test]
    fn admin_binds_engine_on_first_use() {
        let (post, calls) = set_liquidation_engine(
            signer(admin_id()),
            protocol_parameters_account(ParameterOverrides::default()),
            unbound_state(),
            STABLECOIN_PROGRAM_ID,
            DEFAULT_PROGRAM_ID,
            ENGINE,
            engine_authority_id(),
        );
        assert!(calls.is_empty());
        let state = LiquidationEngineState::try_from(&post[2].account().data).unwrap();
        assert_eq!(state.engine_program_id, ENGINE);
        assert_eq!(state.engine_authority_id, engine_authority_id());
        assert!(post[2].required_claim().is_some(), "first bind claims the PDA");
    }

    #[test]
    fn admin_can_rotate_the_binding() {
        let (post, _) = set_liquidation_engine(
            signer(admin_id()),
            protocol_parameters_account(ParameterOverrides::default()),
            engine_state(),
            STABLECOIN_PROGRAM_ID,
            DEFAULT_PROGRAM_ID,
            [0xE2; 8],
            AccountId::new([0xEB; 32]),
        );
        let state = LiquidationEngineState::try_from(&post[2].account().data).unwrap();
        assert_eq!(state.engine_program_id, [0xE2; 8]);
        assert!(post[2].required_claim().is_none());
    }

    #[test]
    #[should_panic(expected = "set_liquidation_engine must be a top-level call")]
    fn bind_through_another_program_is_rejected() {
        set_liquidation_engine(
            signer(admin_id()),
            protocol_parameters_account(ParameterOverrides::default()),
            unbound_state(),
            STABLECOIN_PROGRAM_ID,
            IMPOSTOR,
            ENGINE,
            engine_authority_id(),
        );
    }

    #[test]
    #[should_panic(expected = "Signer is not the protocol's admin")]
    fn non_admin_cannot_bind() {
        set_liquidation_engine(
            signer(caller_id()),
            protocol_parameters_account(ParameterOverrides::default()),
            unbound_state(),
            STABLECOIN_PROGRAM_ID,
            DEFAULT_PROGRAM_ID,
            ENGINE,
            engine_authority_id(),
        );
    }

    #[test]
    #[should_panic(expected = "Engine program id must be non-zero")]
    fn zero_engine_program_id_rejected() {
        set_liquidation_engine(
            signer(admin_id()),
            protocol_parameters_account(ParameterOverrides::default()),
            unbound_state(),
            STABLECOIN_PROGRAM_ID,
            DEFAULT_PROGRAM_ID,
            DEFAULT_PROGRAM_ID,
            engine_authority_id(),
        );
    }

    #[test]
    #[should_panic(expected = "Engine authority id must be non-zero")]
    fn zero_engine_authority_rejected() {
        set_liquidation_engine(
            signer(admin_id()),
            protocol_parameters_account(ParameterOverrides::default()),
            unbound_state(),
            STABLECOIN_PROGRAM_ID,
            DEFAULT_PROGRAM_ID,
            ENGINE,
            AccountId::new([0; 32]),
        );
    }

    #[test]
    fn engine_freeze_sets_is_frozen() {
        let (post, calls) = engine_freeze_new_debt(
            engine_authority(true),
            engine_state(),
            protocol_parameters_account(ParameterOverrides::default()),
            STABLECOIN_PROGRAM_ID,
            ENGINE,
        );
        assert!(calls.is_empty());
        assert!(decode_params(&post[2]).is_frozen);
    }

    #[test]
    #[should_panic(expected = "Caller program is not the liquidation engine")]
    fn freeze_from_impostor_program_rejected() {
        engine_freeze_new_debt(
            engine_authority(true),
            engine_state(),
            protocol_parameters_account(ParameterOverrides::default()),
            STABLECOIN_PROGRAM_ID,
            IMPOSTOR,
        );
    }

    #[test]
    #[should_panic(expected = "Caller program is not the liquidation engine")]
    fn freeze_top_level_rejected() {
        engine_freeze_new_debt(
            engine_authority(true),
            engine_state(),
            protocol_parameters_account(ParameterOverrides::default()),
            STABLECOIN_PROGRAM_ID,
            DEFAULT_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "Engine authority authorization is missing")]
    fn freeze_without_authority_rejected() {
        engine_freeze_new_debt(
            engine_authority(false),
            engine_state(),
            protocol_parameters_account(ParameterOverrides::default()),
            STABLECOIN_PROGRAM_ID,
            ENGINE,
        );
    }

    #[test]
    #[should_panic(expected = "Engine authority account is not the bound engine authority")]
    fn freeze_with_authorised_but_wrong_authority_rejected() {
        engine_freeze_new_debt(
            signer(AccountId::new([0xEB; 32])),
            engine_state(),
            protocol_parameters_account(ParameterOverrides::default()),
            STABLECOIN_PROGRAM_ID,
            ENGINE,
        );
    }

    #[test]
    #[should_panic(expected = "LiquidationEngineState account must be initialized")]
    fn freeze_with_no_binding_rejected() {
        engine_freeze_new_debt(
            engine_authority(true),
            unbound_state(),
            protocol_parameters_account(ParameterOverrides::default()),
            STABLECOIN_PROGRAM_ID,
            ENGINE,
        );
    }

    #[test]
    #[should_panic(expected = "Signer is not the protocol's freeze authority")]
    fn only_freeze_authority_can_unfreeze() {
        let frozen = protocol_parameters_account(ParameterOverrides {
            is_frozen: true,
            ..ParameterOverrides::default()
        });
        crate::freeze::unfreeze(engine_authority(true), frozen, STABLECOIN_PROGRAM_ID);
    }
}
```

Add `pub mod liquidation_hooks;` (with the doc line `/// Liquidation-engine hooks: engine binding, liquidate_position, engine_freeze_new_debt.`) after line 42 of `programs/stablecoin/src/lib.rs`.

- [ ] **Step 2: Run them and see them fail**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p stablecoin_program liquidation_hooks'`
Expected: compile errors: unresolved imports `compute_liquidation_engine_state_pda`, `LiquidationEngineState`; cannot find function `set_liquidation_engine`, `engine_freeze_new_debt`.

- [ ] **Step 3: Add `LiquidationEngineState` to `stablecoin_core`**

`programs/stablecoin/core/src/liquidation_engine_state.rs`:

```rust
//! Binding between this stablecoin instance and its liquidation engine (lez-auction-engine hooks).
//!
//! Read by `liquidate_position` and `engine_freeze_new_debt` to authenticate the engine with both
//! the immediate caller program id and an authorised engine PDA (lez-auction-engine ADR-019, Variant C).

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{
    account::{AccountId, Data},
    program::{PdaSeed, ProgramId},
};
use serde::{Deserialize, Serialize};
use spel_framework_macros::account_type;

/// PDA seed domain for the single-instance [`LiquidationEngineState`] account.
const LIQUIDATION_ENGINE_STATE_PDA_DOMAIN: &[u8; 32] = b"STABLECOIN__LIQUIDATION_ENGINE__";

/// The engine allowed to call the engine-only hooks.
#[account_type]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct LiquidationEngineState {
    /// Must equal the hook's immediate `caller_program_id`.
    pub engine_program_id: ProgramId,
    /// Engine PDA that must be authorised in the hook call.
    pub engine_authority_id: AccountId,
}

impl TryFrom<&Data> for LiquidationEngineState {
    type Error = std::io::Error;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        Self::try_from_slice(data.as_ref())
    }
}

impl From<&LiquidationEngineState> for Data {
    fn from(state: &LiquidationEngineState) -> Self {
        let len = borsh::object_length(state).expect("LiquidationEngineState length must be known");
        let mut buf = Vec::with_capacity(len);
        BorshSerialize::serialize(state, &mut buf)
            .expect("LiquidationEngineState serialization should not fail");
        Self::try_from(buf).expect("LiquidationEngineState encoded data should fit into Data")
    }
}

/// PDA seed for [`LiquidationEngineState`] (single instance per stablecoin program id).
#[must_use]
pub fn compute_liquidation_engine_state_pda_seed() -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256 as _};

    let mut out = [0u8; 32];
    out.copy_from_slice(Impl::hash_bytes(LIQUIDATION_ENGINE_STATE_PDA_DOMAIN).as_bytes());
    PdaSeed::new(out)
}

/// Account id of the [`LiquidationEngineState`] PDA under `stablecoin_program_id`.
#[must_use]
pub fn compute_liquidation_engine_state_pda(stablecoin_program_id: ProgramId) -> AccountId {
    AccountId::for_public_pda(
        &stablecoin_program_id,
        &compute_liquidation_engine_state_pda_seed(),
    )
}
```

If `#[account_type]` fails to compile on the `ProgramId` (`[u32; 8]`) field, remove that one attribute (it only feeds `spel inspect`) and note it in `docs/architecture/hooks.md`; everything else is unchanged.

In `programs/stablecoin/core/src/lib.rs`: after line 11 add `pub mod liquidation_engine_state;`; after line 31 add

```rust
pub use liquidation_engine_state::{
    compute_liquidation_engine_state_pda, compute_liquidation_engine_state_pda_seed,
    LiquidationEngineState,
};
```

and append immediately after RepayDebt's closing `},` (before the enum's closing `}`):

```rust
    /// Bind the liquidation engine allowed to call [`Instruction::LiquidatePosition`] and
    /// [`Instruction::EngineFreezeNewDebt`] (lez-auction-engine hook). Admin-only and top-level only:
    /// `caller_program_id` must be zero, so an authorised admin PDA cannot be laundered through
    /// another program. Rotation overwrites the binding.
    ///
    /// Required accounts (3), in order:
    /// 1. `admin` — authorized; must equal `ProtocolParameters.admin_account_id`.
    /// 2. `protocol_parameters` — initialized, read-only; at its canonical PDA.
    /// 3. `liquidation_engine_state` — at `compute_liquidation_engine_state_pda(self_program_id)`;
    ///    claimed on first use.
    SetLiquidationEngine {
        /// Engine program id (non-zero); compared with the hook's immediate caller.
        engine_program_id: ProgramId,
        /// Engine authority PDA (non-zero); must be authorised in every hook call.
        engine_authority_id: AccountId,
    },
    /// Seize collateral from an unsafe position into the engine's custody holding
    /// (lez-auction-engine hook; see `liquidation_hooks::liquidate_position`). Engine-only
    /// (Variant C). Allowed while frozen.
    ///
    /// Required accounts (10), in order: `engine_authority` (authorized), `liquidation_engine_state`,
    /// `position` (writable), `vault` (writable), `custody` (writable, initialized collateral
    /// holding), `stability_fee_accumulator`, `redemption_price_state`, `market_price_oracle`,
    /// `protocol_parameters`, `clock`.
    LiquidatePosition {
        /// `0 < x <= Position.collateral_amount`, and at most the proportional bound.
        collateral_to_seize: u128,
        /// `0 < x <= Position.normalized_debt_amount`.
        normalized_debt_to_cover: u128,
    },
    /// Set `ProtocolParameters.is_frozen` (lez-auction-engine hook). Engine-only (Variant C). It can
    /// only freeze; unfreezing stays with the freeze authority's [`Instruction::Unfreeze`].
    ///
    /// Required accounts (3), in order: `engine_authority` (authorized), `liquidation_engine_state`,
    /// `protocol_parameters` (writable).
    EngineFreezeNewDebt,
```

- [ ] **Step 4: Implement the binding and freeze hooks**

Prepend to `programs/stablecoin/src/liquidation_hooks.rs` (above the test module):

```rust
//! Liquidation-engine hooks appended by lez-auction-engine (offered upstream).
//!
//! Authentication is Variant C (lez-auction-engine ADR-019): the immediate `caller_program_id`
//! must be the bound engine program AND the bound engine-authority PDA must be authorised.
//! `caller_program_id` alone would accept any program the engine chained to with a forwarded
//! authorisation only if the engine misbehaved; the PDA alone is launderable because lee keeps an
//! authorised account authorised down the call chain (`validated_state_diff/mod.rs:262-290`).

use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, Claim, ProgramId, DEFAULT_PROGRAM_ID},
};
use stablecoin_core::{
    compute_liquidation_engine_state_pda, compute_liquidation_engine_state_pda_seed,
    compute_protocol_parameters_pda, LiquidationEngineState, ProtocolParameters,
};

/// Host bound on the engine's liquidation penalty, used by the proportionality check.
pub const MAX_PENALTY_BPS: u128 = 2_500;

fn load_parameters(protocol_parameters: &AccountWithMetadata, program_id: ProgramId) -> ProtocolParameters {
    ProtocolParameters::try_from(&crate::checks::decode_global(
        protocol_parameters,
        compute_protocol_parameters_pda(program_id),
        program_id,
        "ProtocolParameters",
    ))
    .expect("ProtocolParameters must decode")
}

pub(crate) fn load_engine_state(state: &AccountWithMetadata, program_id: ProgramId) -> LiquidationEngineState {
    LiquidationEngineState::try_from(&crate::checks::decode_global(
        state,
        compute_liquidation_engine_state_pda(program_id),
        program_id,
        "LiquidationEngineState",
    ))
    .expect("LiquidationEngineState must decode")
}

/// Variant C guard shared by both engine-only hooks.
///
/// # Panics
/// If the immediate caller is not the bound engine program, the authority account is not the
/// bound engine authority, or it is not authorised in this call.
pub fn assert_engine_call(
    caller_program_id: ProgramId,
    engine_authority: &AccountWithMetadata,
    state: &LiquidationEngineState,
) {
    assert_eq!(
        caller_program_id, state.engine_program_id,
        "Caller program is not the liquidation engine"
    );
    assert_eq!(
        engine_authority.account_id, state.engine_authority_id,
        "Engine authority account is not the bound engine authority"
    );
    assert!(
        engine_authority.is_authorized,
        "Engine authority authorization is missing"
    );
}

/// Bind (or rotate) the liquidation engine. Admin-only, top-level only.
///
/// # Panics
/// Not a top-level call; admin not authorised or not the stored admin; zero engine program id or
/// zero authority id; `liquidation_engine_state` not at its PDA or owned by another program.
#[allow(clippy::too_many_arguments, reason = "three accounts plus context and two arguments mirror the guest ABI")]
pub fn set_liquidation_engine(
    admin: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    liquidation_engine_state: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    caller_program_id: ProgramId,
    engine_program_id: ProgramId,
    engine_authority_id: AccountId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    assert_eq!(
        caller_program_id, DEFAULT_PROGRAM_ID,
        "set_liquidation_engine must be a top-level call"
    );
    let parameters = load_parameters(&protocol_parameters, stablecoin_program_id);
    crate::checks::assert_admin_authorized(&admin, &parameters);
    assert_ne!(engine_program_id, DEFAULT_PROGRAM_ID, "Engine program id must be non-zero");
    assert_ne!(engine_authority_id, AccountId::new([0; 32]), "Engine authority id must be non-zero");
    assert_eq!(
        liquidation_engine_state.account_id,
        compute_liquidation_engine_state_pda(stablecoin_program_id),
        "LiquidationEngineState account ID does not match expected PDA derivation"
    );
    let is_new = liquidation_engine_state.account == Account::default();
    if !is_new {
        assert_eq!(
            liquidation_engine_state.account.program_owner, stablecoin_program_id,
            "LiquidationEngineState account must be owned by the stablecoin program"
        );
    }
    let mut state_post = liquidation_engine_state.account;
    state_post.data = Data::from(&LiquidationEngineState {
        engine_program_id,
        engine_authority_id,
    });
    let state_post = if is_new {
        AccountPostState::new_claimed(state_post, Claim::Pda(compute_liquidation_engine_state_pda_seed()))
    } else {
        AccountPostState::new(state_post)
    };
    (
        vec![
            AccountPostState::new(admin.account),
            AccountPostState::new(protocol_parameters.account),
            state_post,
        ],
        vec![],
    )
}

/// Set `ProtocolParameters.is_frozen` on the engine's behalf (F4 "pause new debt generation").
///
/// # Panics
/// The Variant C guard fails, or a global does not validate.
pub fn engine_freeze_new_debt(
    engine_authority: AccountWithMetadata,
    liquidation_engine_state: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    caller_program_id: ProgramId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    let state = load_engine_state(&liquidation_engine_state, stablecoin_program_id);
    assert_engine_call(caller_program_id, &engine_authority, &state);
    let mut parameters = load_parameters(&protocol_parameters, stablecoin_program_id);
    parameters.is_frozen = true;
    let mut parameters_post = protocol_parameters.account;
    parameters_post.data = Data::from(&parameters);
    (
        vec![
            AccountPostState::new(engine_authority.account),
            AccountPostState::new(liquidation_engine_state.account),
            AccountPostState::new(parameters_post),
        ],
        vec![],
    )
}
```

- [ ] **Step 5: Run the unit tests and see them pass**

Run: the Step 2 command. Expected: `13 passed`.

- [ ] **Step 6: Write the failing enum/IDL order test**

Append to `programs/stablecoin/core/src/lib.rs`:

```rust
#[cfg(test)]
mod hook_order_tests {
    //! Spec §5.10 / §3.2: spel-cli encodes by IDL position, the guest decodes by enum index, so the
    //! appended hooks must sit at the same index in both. Upstream: point IDL_PATH at
    //! `../../../../artifacts/stablecoin-idl.json`.
    use super::*;

    const IDL_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../idl/stablecoin-idl.json");

    fn enum_index(instruction: &Instruction) -> u32 {
        risc0_zkvm::serde::to_vec(instruction).expect("instruction serializes")[0]
    }

    #[test]
    fn hook_instructions_share_enum_and_idl_index() {
        let idl: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(IDL_PATH).expect("IDL file")).expect("IDL json");
        let names: Vec<String> = idl["instructions"]
            .as_array()
            .expect("instructions array")
            .iter()
            .map(|i| i["name"].as_str().expect("name").to_owned())
            .collect();
        let hooks = [
            (
                "set_liquidation_engine",
                Instruction::SetLiquidationEngine {
                    engine_program_id: [1; 8],
                    engine_authority_id: AccountId::new([2; 32]),
                },
            ),
            (
                "liquidate_position",
                Instruction::LiquidatePosition {
                    collateral_to_seize: 1,
                    normalized_debt_to_cover: 1,
                },
            ),
            ("engine_freeze_new_debt", Instruction::EngineFreezeNewDebt),
        ];
        for (expected, (name, instruction)) in (19u32..).zip(hooks.iter()) {
            assert_eq!(enum_index(instruction), expected, "{name}: enum index");
            assert_eq!(
                names.iter().position(|n| n == name),
                Some(usize::try_from(expected).expect("fits")),
                "{name}: IDL index"
            );
        }
        assert_eq!(names.len(), 22, "no other instruction was added or removed");
    }
}
```

If `risc0_zkvm::serde::to_vec` is not reachable with `stablecoin_core`'s `default-features = false` dependency, add `risc0-zkvm = { version = "=3.0.5" }` under a new `[dev-dependencies]` key in `programs/stablecoin/core/Cargo.toml` (it already has `serde_json`).

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p stablecoin_core hook_order'`
Expected: FAIL — `set_liquidation_engine: IDL index` left `None` (the guest has no such function yet).

- [ ] **Step 7: Append the guest functions and regenerate the IDL**

In `programs/stablecoin/methods/guest/src/bin/stablecoin.rs`, after the `unfreeze` function and before the module's closing `}`, append (keep this order; Task 5 inserts `liquidate_position` between the two):

```rust
    /// Bind the liquidation engine (lez-auction-engine hook; host fn
    /// `stablecoin_program::liquidation_hooks::set_liquidation_engine`). Admin-only, top-level only.
    ///
    /// # Errors
    /// Returns the host program's panic-converted error if any precondition fails.
    #[instruction]
    pub fn set_liquidation_engine(
        ctx: ProgramContext,
        #[account(signer)]
        admin: AccountWithMetadata,
        protocol_parameters: AccountWithMetadata,
        #[account(mut)]
        liquidation_engine_state: AccountWithMetadata,
        engine_program_id: nssa_core::program::ProgramId,
        engine_authority_id: nssa_core::account::AccountId,
    ) -> SpelResult {
        let (post_states, chained_calls) =
            stablecoin_program::liquidation_hooks::set_liquidation_engine(
                admin,
                protocol_parameters,
                liquidation_engine_state,
                ctx.self_program_id,
                ctx.caller_program_id,
                engine_program_id,
                engine_authority_id,
            );
        Ok(spel_framework::SpelOutput::execute(post_states, chained_calls))
    }

    /// Freeze new debt on the engine's behalf (lez-auction-engine hook; host fn
    /// `stablecoin_program::liquidation_hooks::engine_freeze_new_debt`). Engine-only.
    ///
    /// # Errors
    /// Returns the host program's panic-converted error if any precondition fails.
    #[instruction]
    pub fn engine_freeze_new_debt(
        ctx: ProgramContext,
        #[account(signer)]
        engine_authority: AccountWithMetadata,
        liquidation_engine_state: AccountWithMetadata,
        #[account(mut)]
        protocol_parameters: AccountWithMetadata,
    ) -> SpelResult {
        let (post_states, chained_calls) =
            stablecoin_program::liquidation_hooks::engine_freeze_new_debt(
                engine_authority,
                liquidation_engine_state,
                protocol_parameters,
                ctx.self_program_id,
                ctx.caller_program_id,
            );
        Ok(spel_framework::SpelOutput::execute(post_states, chained_calls))
    }
```

The macro maps PascalCase variants to these snake_case names (`spel-framework-macros/src/lib.rs:830-845`), so the guest compiles only once Task 5 adds a `liquidate_position` handler for `Instruction::LiquidatePosition`. Until then add this temporary arm between the two functions so the match is exhaustive:

```rust
    /// Implemented in the next patch (liquidate_position).
    #[instruction]
    pub fn liquidate_position(
        collateral_to_seize: u128,
        normalized_debt_to_cover: u128,
    ) -> SpelResult {
        let _ = (collateral_to_seize, normalized_debt_to_cover);
        panic!("liquidate_position is not implemented yet")
    }
```

Task 5 replaces this function body and signature; the `panic!` never ships past Task 5 (Task 5's self-check greps for it).

Regenerate the IDL: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh run -q -p idl-gen -- programs/stablecoin/methods/guest/src/bin/stablecoin.rs > idl/stablecoin-idl.json'`

- [ ] **Step 8: Run the order test, the unit tests and the upstream suite**

Run:
```bash
MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p stablecoin_core -p stablecoin_program -p token_program -p integration_tests 2>&1 | grep "test result"'
```
Expected: all `ok`, 0 failed; total = the Task 3 baseline + 14 (13 hook unit tests + 1 order test). The upstream suite exercises the rebuilt guest, so this also proves the appended functions compile and nothing upstream moved.

Then the upstream lint and format gates (I3):
```bash
MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && RISC0_SKIP_BUILD=1 scripts/wsl-cargo.sh clippy -p stablecoin_core -p stablecoin_program --all-targets -- -D warnings && scripts/wsl-cargo.sh fmt -p stablecoin_core -p stablecoin_program -- --check 2>&1 | diff - $HOME/lez-target/plan2-stablecoin-fmt-baseline.txt | grep -E "liquidation_engine_state|liquidation_hooks|hook_order" || echo FMT-OK'
```
Expected: no clippy warnings; `FMT-OK` (no new diff in hook files). If `rustup toolchain list` ever shows a nightly, run `cargo +nightly fmt -p stablecoin_core -p stablecoin_program` instead before committing.

- [ ] **Step 9: Docs and ADR-026**

Create `docs/architecture/hooks.md`:

```markdown
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
```

Append ADR-026 to `docs/design-log.md`:

```markdown
---

## ADR-026: Hook Binding and Authentication

**Context**
ADR-019 chose Variant C and left open how the host stores the engine and how the upstream laundering note (admin/freeze checks are plain `is_authorized`) is handled. Spec §5.10 names `LiquidationEngineState { engine_authority_id }`.

**Options**
- (A) Store only the authority id; derive nothing else.
- (B) Store `engine_program_id` and `engine_authority_id` explicitly; the admin binds both.
- (C) Store the program id and derive the authority from the engine's seed convention.

**Decision**
(B). Variant C needs the program id (immediate caller) and the authority (authorised PDA); storing both keeps the host ignorant of the engine's seed scheme. `set_liquidation_engine` rejects zero ids (SPEL reports a zero caller for top-level calls, ADR-019) and requires `caller_program_id == 0`, so the binding cannot be changed through a program in which an admin PDA was authorised. The existing admin and freeze instructions are left unchanged (upstream suite unchanged); the upstream PR documents that their authorities must remain user keys.

**Trade-offs**
Rotation is a single admin step (like `set_admin`). A wrong pair simply makes the hooks unusable until rebound. The binding names one engine configuration per host instance (ADR-027).

**Status**
Accepted (Plan 2, Task 4)
```

- [ ] **Step 10: Commit (controller — GPG signing on Windows) — then export the patch series**

The implementer stages the files and reports; the controller commits from Git Bash.

```bash
git add programs/stablecoin/core/src/liquidation_engine_state.rs programs/stablecoin/core/src/lib.rs programs/stablecoin/core/Cargo.toml programs/stablecoin/src/liquidation_hooks.rs programs/stablecoin/src/lib.rs programs/stablecoin/methods/guest/src/bin/stablecoin.rs idl/stablecoin-idl.json docs/architecture/hooks.md docs/design-log.md
git commit -m "feat(stablecoin): engine binding and Variant C freeze hook, appended at enum/IDL 19-21; ADR-026"
bash scripts/export-hook-patches.sh && MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/check-hook-patches.sh'
git add patches/stablecoin && git commit -m "chore: export stablecoin hook patch series"
```

---

### Task 5: Stablecoin hooks B — `liquidate_position`

**Files:**
- Modify: `programs/stablecoin/src/checks.rs:57-83` (add `is_position_collateralized`; `assert_position_is_collateralized` calls it)
- Modify: `programs/stablecoin/src/liquidation_hooks.rs` (add `liquidate_position`, `proportional_seize_bound`, tests)
- Modify: `programs/stablecoin/methods/guest/src/bin/stablecoin.rs` (replace the temporary `liquidate_position` from Task 4 Step 7)
- Modify: `idl/stablecoin-idl.json`, `docs/architecture/hooks.md`

**Interfaces:**
- Consumes: Task 4 (`load_engine_state`, `assert_engine_call`, `MAX_PENALTY_BPS`, fixtures `ENGINE`, `IMPOSTOR`, `engine_authority`, `engine_state`); upstream `crate::accrue_stability_fee::read_clock` (pub(crate), `src/accrue_stability_fee.rs:123`), `crate::update_redemption_rate::decode_oracle` (pub(crate), `:128`), `stablecoin_core::{compute_position_pda, verify_position_vault_and_get_seed, Position, StabilityFeeAccumulator, RedemptionPriceState, compute_stability_fee_accumulator_pda, compute_redemption_price_state_pda, math::{compute_current_accumulated_rate_wide, compute_current_redemption_price_wide}}`.
- Produces: `pub fn liquidate_position(engine_authority, liquidation_engine_state, position, vault, custody, stability_fee_accumulator, redemption_price_state, market_price_oracle, protocol_parameters, clock: AccountWithMetadata, stablecoin_program_id: ProgramId, caller_program_id: ProgramId, collateral_to_seize: u128, normalized_debt_to_cover: u128) -> (Vec<AccountPostState>, Vec<ChainedCall>)`; `pub fn is_position_collateralized(&Position, U512, U512, u128) -> bool`. The returned chain is one `Token::Transfer(vault authorised by its PDA seed, custody as received)`; on full seizure the amount is the whole vault balance.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` of `programs/stablecoin/src/liquidation_hooks.rs`:

```rust
    use stablecoin_core::{compute_position_pda, compute_position_vault_pda, Position};
    use token_core::TokenHolding;

    const TOKEN: ProgramId = [6u32; 8];

    fn owner_id() -> AccountId {
        AccountId::new([0x0E; 32])
    }
    fn position_id() -> AccountId {
        compute_position_pda(STABLECOIN_PROGRAM_ID, owner_id(), 0)
    }
    fn vault_id() -> AccountId {
        compute_position_vault_pda(STABLECOIN_PROGRAM_ID, position_id())
    }
    fn custody_id() -> AccountId {
        AccountId::new([0xC5; 32])
    }
    fn position(collateral: u128, debt: u128) -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: STABLECOIN_PROGRAM_ID,
                balance: 0,
                data: Data::from(&Position {
                    owner_account_id: owner_id(),
                    position_nonce: 0,
                    vault_account_id: vault_id(),
                    collateral_amount: collateral,
                    normalized_debt_amount: debt,
                    opened_at: 0,
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: position_id(),
        }
    }
    fn holding(id: AccountId, definition: AccountId, balance: u128) -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN,
                balance: 0,
                data: Data::from(&TokenHolding::Fungible { definition_id: definition, balance }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: id,
        }
    }

    struct Case {
        caller: ProgramId,
        authorized: bool,
        collateral: u128,
        debt: u128,
        vault_balance: u128,
        seize: u128,
        cover: u128,
        oracle_ts: u64,
        frozen: bool,
    }

    /// Defaults: min CR 1.5, redemption 0.5, accumulator 1.0 → liquidatable iff collateral < 0.75 × debt.
    fn unsafe_case() -> Case {
        Case { caller: ENGINE, authorized: true, collateral: 1_000, debt: 2_000, vault_balance: 1_000,
               seize: 1_000, cover: 2_000, oracle_ts: NOW, frozen: false }
    }

    fn run(c: Case) -> (Vec<lee_core::program::AccountPostState>, Vec<lee_core::program::ChainedCall>) {
        liquidate_position(
            engine_authority(c.authorized),
            engine_state(),
            position(c.collateral, c.debt),
            holding(vault_id(), collateral_definition_id(), c.vault_balance),
            holding(custody_id(), collateral_definition_id(), 0),
            accumulator_account(ACCUMULATOR_ANCHOR, NOW),
            redemption_price_state_account(NOW),
            oracle_account(c.oracle_ts, REDEMPTION_PRICE_ANCHOR),
            protocol_parameters_account(ParameterOverrides { is_frozen: c.frozen, ..ParameterOverrides::default() }),
            clock_account(NOW),
            STABLECOIN_PROGRAM_ID,
            c.caller,
            c.seize,
            c.cover,
        )
    }

    fn transferred(calls: &[lee_core::program::ChainedCall], amount: u128) -> bool {
        let expected = lee_core::program::ChainedCall::new(
            TOKEN,
            vec![],
            &token_core::Instruction::Transfer { amount_to_transfer: amount },
        );
        calls.len() == 1
            && calls[0].program_id == TOKEN
            && calls[0].instruction_data == expected.instruction_data
            && calls[0].pda_seeds.len() == 1
            && AccountId::for_public_pda(&STABLECOIN_PROGRAM_ID, &calls[0].pda_seeds[0]) == vault_id()
            && calls[0].pre_states[0].account_id == vault_id()
            && calls[0].pre_states[0].is_authorized
            && calls[0].pre_states[1].account_id == custody_id()
    }

    fn position_after(post: &[lee_core::program::AccountPostState]) -> Position {
        Position::try_from(&post[2].account().data).unwrap()
    }

    #[test]
    fn full_seizure_zeroes_position_and_moves_vault() {
        let (post, calls) = run(unsafe_case());
        let p = position_after(&post);
        assert_eq!((p.collateral_amount, p.normalized_debt_amount), (0, 0));
        assert!(transferred(&calls, 1_000));
    }

    #[test]
    fn donation_into_vault_is_swept() {
        // Review Focus 3: a third party raised the vault balance above the position's collateral.
        let (_, calls) = run(Case { vault_balance: 1_005, ..unsafe_case() });
        assert!(transferred(&calls, 1_005));
    }

    #[test]
    fn partial_seizure_moves_exactly_the_amount() {
        // bound = ceil(1000 × 1000 / 2000 × 1.25) = 625
        let (post, calls) = run(Case { seize: 400, cover: 1_000, vault_balance: 1_010, ..unsafe_case() });
        let p = position_after(&post);
        assert_eq!((p.collateral_amount, p.normalized_debt_amount), (600, 1_000));
        assert!(transferred(&calls, 400));
    }

    #[test]
    fn allowed_while_frozen() {
        let (post, _) = run(Case { frozen: true, ..unsafe_case() });
        assert_eq!(position_after(&post).collateral_amount, 0);
    }

    #[test]
    #[should_panic(expected = "collateral_to_seize exceeds the proportional bound")]
    fn partial_seizure_above_bound_rejected() {
        run(Case { seize: 626, cover: 1_000, ..unsafe_case() });
    }

    #[test]
    #[should_panic(expected = "Position is not liquidatable: it is collateralized")]
    fn healthy_position_rejected() {
        run(Case { collateral: 2_000, debt: 1_000, vault_balance: 2_000, seize: 2_000, cover: 1_000, ..unsafe_case() });
    }

    #[test]
    #[should_panic(expected = "Caller program is not the liquidation engine")]
    fn non_engine_caller_rejected() {
        run(Case { caller: IMPOSTOR, ..unsafe_case() });
    }

    #[test]
    #[should_panic(expected = "Caller program is not the liquidation engine")]
    fn direct_top_level_call_rejected() {
        run(Case { caller: DEFAULT_PROGRAM_ID, ..unsafe_case() });
    }

    #[test]
    #[should_panic(expected = "Engine authority authorization is missing")]
    fn unauthorised_authority_rejected() {
        run(Case { authorized: false, ..unsafe_case() });
    }

    #[test]
    #[should_panic(expected = "collateral_to_seize out of bounds")]
    fn seize_more_than_position_rejected() {
        run(Case { seize: 1_001, vault_balance: 1_001, ..unsafe_case() });
    }

    #[test]
    #[should_panic(expected = "normalized_debt_to_cover out of bounds")]
    fn cover_more_than_debt_rejected() {
        run(Case { cover: 2_001, ..unsafe_case() });
    }

    #[test]
    #[should_panic(expected = "normalized_debt_to_cover out of bounds")]
    fn zero_cover_rejected() {
        run(Case { cover: 0, ..unsafe_case() });
    }

    #[test]
    #[should_panic(expected = "Vault balance is below the position collateral")]
    fn vault_below_collateral_rejected() {
        run(Case { vault_balance: 999, ..unsafe_case() });
    }

    #[test]
    #[should_panic(expected = "Position has no debt")]
    fn zero_debt_rejected() {
        run(Case { debt: 0, cover: 0, ..unsafe_case() });
    }

    #[test]
    #[should_panic(expected = "Market price oracle observation is stale")]
    fn stale_oracle_rejected() {
        run(Case { oracle_ts: NOW - TEST_MAXIMUM_ORACLE_PRICE_AGE_MILLISECONDS - 1, ..unsafe_case() });
    }

    #[test]
    #[should_panic(expected = "Custody holding does not match the protocol's collateral definition")]
    fn custody_of_wrong_definition_rejected() {
        liquidate_position(
            engine_authority(true), engine_state(), position(1_000, 2_000),
            holding(vault_id(), collateral_definition_id(), 1_000),
            holding(custody_id(), stablecoin_definition_id(), 0),
            accumulator_account(ACCUMULATOR_ANCHOR, NOW), redemption_price_state_account(NOW),
            oracle_account(NOW, REDEMPTION_PRICE_ANCHOR), protocol_parameters_account(ParameterOverrides::default()),
            clock_account(NOW), STABLECOIN_PROGRAM_ID, ENGINE, 1_000, 2_000,
        );
    }

    #[test]
    fn proportional_bound_is_capped_at_collateral_and_rounds_up() {
        assert_eq!(proportional_seize_bound(1_000, 2_000, 2_000), 1_000); // 1250 capped
        assert_eq!(proportional_seize_bound(1_000, 1_000, 2_000), 625);
        assert_eq!(proportional_seize_bound(3, 1, 7), 1); // ceil(3 × 1 × 12500 / 70000) = ceil(0.535…)
        assert_eq!(proportional_seize_bound(u128::MAX, u128::MAX, u128::MAX), u128::MAX);
    }
```

- [ ] **Step 2: Run them and see them fail**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p stablecoin_program liquidation_hooks'`
Expected: compile errors `cannot find function \`liquidate_position\`` and `cannot find function \`proportional_seize_bound\``.

- [ ] **Step 3: Add the boolean health check**

Replace `programs/stablecoin/src/checks.rs` lines 57–83 (the body of `assert_position_is_collateralized`) with:

```rust
pub fn assert_position_is_collateralized(
    position: &Position,
    current_accumulator: U512,
    current_redemption_price: U512,
    minimum_collateralization_ratio: u128,
) {
    assert!(
        is_position_collateralized(
            position,
            current_accumulator,
            current_redemption_price,
            minimum_collateralization_ratio,
        ),
        "Position is undercollateralized"
    );
}

/// Boolean form of [`assert_position_is_collateralized`], same exact `U512` comparison
/// (used by `liquidation_hooks::liquidate_position`, which requires the negation).
#[must_use]
pub fn is_position_collateralized(
    position: &Position,
    current_accumulator: U512,
    current_redemption_price: U512,
    minimum_collateralization_ratio: u128,
) -> bool {
    if position.normalized_debt_amount == 0 {
        return true;
    }
    let one = U512::from(FIXED_POINT_ONE);
    // No division anywhere: `/ FIXED_POINT_ONE` on the debt side is carried as an
    // extra `× FIXED_POINT_ONE` on the collateral side, keeping the check exact.
    let collateral_value = U512::from(position.collateral_amount) * one * one * one;
    let required_collateral_value = U512::from(position.normalized_debt_amount)
        .saturating_mul(current_accumulator)
        .saturating_mul(current_redemption_price)
        .saturating_mul(U512::from(minimum_collateralization_ratio));
    collateral_value >= required_collateral_value
}
```

Keep the existing doc comment above `assert_position_is_collateralized` unchanged.

- [ ] **Step 4: Implement `liquidate_position`**

Add to the imports of `liquidation_hooks.rs`:

```rust
use alloy_primitives::U512;
use stablecoin_core::{
    compute_position_pda, compute_redemption_price_state_pda, compute_stability_fee_accumulator_pda,
    math::{compute_current_accumulated_rate_wide, compute_current_redemption_price_wide},
    verify_position_vault_and_get_seed, Position, RedemptionPriceState, StabilityFeeAccumulator,
};
use token_core::TokenHolding;
```

and append (above the test module):

```rust
const BPS: u128 = 10_000;

/// `min(collateral, ceil(collateral × cover / debt × (1e4 + MAX_PENALTY_BPS) / 1e4))` in U512
/// (owner protection independent of the engine, lez-auction-engine spec §5.10). `debt > 0`.
pub(crate) fn proportional_seize_bound(collateral: u128, cover: u128, debt: u128) -> u128 {
    // Checked U512 arithmetic (upstream's denied `arithmetic_side_effects` style); cannot overflow:
    // u128 × u128 × 12_500 < 2^270.
    let factor = U512::from(BPS.checked_add(MAX_PENALTY_BPS).expect("constant"));
    let numerator = U512::from(collateral)
        .checked_mul(U512::from(cover))
        .and_then(|v| v.checked_mul(factor))
        .expect("fits U512");
    let denominator = U512::from(debt).checked_mul(U512::from(BPS)).expect("fits U512");
    let bound = numerator
        .checked_add(denominator)
        .and_then(|v| v.checked_sub(U512::from(1u64)))
        .and_then(|v| v.checked_div(denominator))
        .expect("debt > 0 is asserted before the bound");
    if bound >= U512::from(collateral) {
        collateral
    } else {
        bound.to::<u128>()
    }
}

fn fungible_balance(holding: &TokenHolding, label: &str) -> u128 {
    match holding {
        TokenHolding::Fungible { balance, .. } => *balance,
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            panic!("{label} must be a fungible holding")
        }
    }
}

fn decode<T: for<'a> TryFrom<&'a Data>>(account: &AccountWithMetadata, expected: AccountId, program_id: ProgramId, label: &str) -> T {
    T::try_from(&crate::checks::decode_global(account, expected, program_id, label))
        .unwrap_or_else(|_| panic!("{label} must decode"))
}

/// Seize collateral from an unsafe position into the engine's custody holding.
///
/// Full seizure (both arguments equal the position's values) zeroes the position and moves the
/// WHOLE vault balance, sweeping donations. Partial seizure subtracts both amounts and moves
/// exactly `collateral_to_seize`. Allowed while frozen (it reduces risk).
///
/// # Panics
/// Variant C guard; uninitialised or foreign position or wrong PDA; zero debt; out-of-bounds
/// amounts or above the proportional bound; vault PDA, definition or balance (`>=` collateral);
/// custody uninitialised, of another token program or definition; stale or future-dated oracle;
/// redemption price projected to zero; the position is collateralised at the minimum ratio.
#[allow(clippy::too_many_arguments, reason = "ten account inputs plus context and two arguments mirror the guest ABI")]
pub fn liquidate_position(
    engine_authority: AccountWithMetadata,
    liquidation_engine_state: AccountWithMetadata,
    position: AccountWithMetadata,
    vault: AccountWithMetadata,
    custody: AccountWithMetadata,
    stability_fee_accumulator: AccountWithMetadata,
    redemption_price_state: AccountWithMetadata,
    market_price_oracle: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    clock: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    caller_program_id: ProgramId,
    collateral_to_seize: u128,
    normalized_debt_to_cover: u128,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    let engine = load_engine_state(&liquidation_engine_state, stablecoin_program_id);
    assert_engine_call(caller_program_id, &engine_authority, &engine);

    let parameters = load_parameters(&protocol_parameters, stablecoin_program_id);
    let accumulator: StabilityFeeAccumulator = decode(
        &stability_fee_accumulator,
        compute_stability_fee_accumulator_pda(stablecoin_program_id),
        stablecoin_program_id,
        "StabilityFeeAccumulator",
    );
    let redemption: RedemptionPriceState = decode(
        &redemption_price_state,
        compute_redemption_price_state_pda(stablecoin_program_id),
        stablecoin_program_id,
        "RedemptionPriceState",
    );
    let now = crate::accrue_stability_fee::read_clock(&clock);
    let oracle = crate::update_redemption_rate::decode_oracle(&market_price_oracle, &parameters);
    let oracle_age = now
        .checked_sub(oracle.timestamp)
        .expect("Market price oracle observation is dated in the future");
    assert!(
        oracle_age <= parameters.maximum_oracle_price_age_milliseconds,
        "Market price oracle observation is stale"
    );

    assert_ne!(position.account, Account::default(), "Position account must be initialized");
    assert_eq!(
        position.account.program_owner, stablecoin_program_id,
        "Position is not owned by this stablecoin program"
    );
    let current = Position::try_from(&position.account.data)
        .expect("Position account must hold valid Position state");
    assert_eq!(
        position.account_id,
        compute_position_pda(stablecoin_program_id, current.owner_account_id, current.position_nonce),
        "Position account ID does not match expected derivation"
    );
    assert!(current.normalized_debt_amount > 0, "Position has no debt");
    assert!(
        normalized_debt_to_cover > 0 && normalized_debt_to_cover <= current.normalized_debt_amount,
        "normalized_debt_to_cover out of bounds"
    );
    assert!(
        collateral_to_seize > 0 && collateral_to_seize <= current.collateral_amount,
        "collateral_to_seize out of bounds"
    );
    assert!(
        collateral_to_seize
            <= proportional_seize_bound(current.collateral_amount, normalized_debt_to_cover, current.normalized_debt_amount),
        "collateral_to_seize exceeds the proportional bound"
    );

    let vault_seed = verify_position_vault_and_get_seed(&vault, position.account_id, stablecoin_program_id);
    assert_eq!(
        current.vault_account_id, vault.account_id,
        "Position vault_account_id does not match the vault account"
    );
    let vault_holding = TokenHolding::try_from(&vault.account.data)
        .expect("Vault account must hold a valid TokenHolding");
    assert_eq!(
        vault_holding.definition_id(),
        parameters.collateral_definition_id,
        "Vault holding does not match the protocol's collateral definition"
    );
    let vault_balance = fungible_balance(&vault_holding, "Vault holding");
    assert!(
        vault_balance >= current.collateral_amount,
        "Vault balance is below the position collateral"
    );
    let token_program_id = vault.account.program_owner;
    assert_ne!(custody.account, Account::default(), "Custody holding must be initialized");
    assert_eq!(
        custody.account.program_owner, token_program_id,
        "Custody holding must be owned by the vault's Token Program"
    );
    let custody_holding = TokenHolding::try_from(&custody.account.data)
        .expect("Custody account must hold a valid TokenHolding");
    assert_eq!(
        custody_holding.definition_id(),
        parameters.collateral_definition_id,
        "Custody holding does not match the protocol's collateral definition"
    );

    let redemption_price = compute_current_redemption_price_wide(
        redemption.redemption_price_at_last_update,
        redemption.redemption_rate_per_millisecond,
        redemption.last_updated_at,
        now,
    );
    assert!(!redemption_price.is_zero(), "Redemption price projected to zero");
    let collateralized = crate::checks::is_position_collateralized(
        &current,
        compute_current_accumulated_rate_wide(
            accumulator.accumulated_rate_at_last_accrual,
            parameters.stability_fee_per_millisecond,
            accumulator.last_accrued_at,
            now,
        ),
        redemption_price,
        parameters.minimum_collateralization_ratio,
    );
    assert!(!collateralized, "Position is not liquidatable: it is collateralized");

    let full = collateral_to_seize == current.collateral_amount
        && normalized_debt_to_cover == current.normalized_debt_amount;
    let updated = Position {
        owner_account_id: current.owner_account_id,
        position_nonce: current.position_nonce,
        vault_account_id: current.vault_account_id,
        collateral_amount: current
            .collateral_amount
            .checked_sub(collateral_to_seize)
            .expect("bounded above"),
        normalized_debt_amount: current
            .normalized_debt_amount
            .checked_sub(normalized_debt_to_cover)
            .expect("bounded above"),
        opened_at: current.opened_at,
    };
    let amount = if full { vault_balance } else { collateral_to_seize };

    let mut position_post = position.account.clone();
    position_post.data = Data::from(&updated);
    let post_states = vec![
        AccountPostState::new(engine_authority.account),
        AccountPostState::new(liquidation_engine_state.account),
        AccountPostState::new(position_post),
        AccountPostState::new(vault.account.clone()),
        AccountPostState::new(custody.account.clone()),
        AccountPostState::new(stability_fee_accumulator.account),
        AccountPostState::new(redemption_price_state.account),
        AccountPostState::new(market_price_oracle.account),
        AccountPostState::new(protocol_parameters.account),
        AccountPostState::new(clock.account),
    ];
    let mut vault_authorized = vault;
    vault_authorized.is_authorized = true;
    let transfer = ChainedCall::new(
        token_program_id,
        vec![vault_authorized, custody],
        &token_core::Instruction::Transfer { amount_to_transfer: amount },
    )
    .with_pda_seeds(vec![vault_seed]);
    (post_states, vec![transfer])
}
```

`stablecoin_program`'s `Cargo.toml` already depends on `alloy-primitives`, `stablecoin_core` and `token_core`.

- [ ] **Step 5: Run the hook tests and see them pass**

Run: the Step 2 command. Expected: `30 passed` (13 from Task 4 + 17 new).

- [ ] **Step 6: Replace the temporary guest function and regenerate the IDL**

Replace the temporary `liquidate_position` in the guest with:

```rust
    /// Seize collateral into the engine's custody holding (lez-auction-engine hook; host fn
    /// `stablecoin_program::liquidation_hooks::liquidate_position`). Engine-only. Allowed while frozen.
    ///
    /// # Errors
    /// Returns the host program's panic-converted error if any precondition fails.
    #[instruction]
    #[allow(clippy::too_many_arguments, reason = "ten account inputs mirror the host function's ABI")]
    pub fn liquidate_position(
        ctx: ProgramContext,
        #[account(signer)]
        engine_authority: AccountWithMetadata,
        liquidation_engine_state: AccountWithMetadata,
        #[account(mut)]
        position: AccountWithMetadata,
        #[account(mut)]
        vault: AccountWithMetadata,
        #[account(mut)]
        custody: AccountWithMetadata,
        stability_fee_accumulator: AccountWithMetadata,
        redemption_price_state: AccountWithMetadata,
        market_price_oracle: AccountWithMetadata,
        protocol_parameters: AccountWithMetadata,
        clock: AccountWithMetadata,
        collateral_to_seize: u128,
        normalized_debt_to_cover: u128,
    ) -> SpelResult {
        let (post_states, chained_calls) = stablecoin_program::liquidation_hooks::liquidate_position(
            engine_authority,
            liquidation_engine_state,
            position,
            vault,
            custody,
            stability_fee_accumulator,
            redemption_price_state,
            market_price_oracle,
            protocol_parameters,
            clock,
            ctx.self_program_id,
            ctx.caller_program_id,
            collateral_to_seize,
            normalized_debt_to_cover,
        );
        Ok(spel_framework::SpelOutput::execute(post_states, chained_calls))
    }
```

Regenerate: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh run -q -p idl-gen -- programs/stablecoin/methods/guest/src/bin/stablecoin.rs > idl/stablecoin-idl.json'`, then confirm the temporary body is gone: `grep -c "not implemented yet" programs/stablecoin/methods/guest/src/bin/stablecoin.rs` prints `0`.

- [ ] **Step 7: Run the whole stablecoin suite (upstream unchanged + hooks + order)**

Run: the Task 4 Step 8 commands (suite, then clippy and the fmt check; the fmt grep also matches `checks.rs` changes: add `|checks.rs` to the pattern). Expected: all ok, 0 failed; total = Task 3 baseline + 31 (14 from Task 4 + 17 here); no clippy warnings; `FMT-OK`.

- [ ] **Step 8: Docs**

Append to `docs/architecture/hooks.md`:

```markdown
## `liquidate_position`

Checks, in order: Variant C guard; `ProtocolParameters`, accumulator and redemption-price state at their PDAs; clock is `CLOCK_01`; the host oracle is the configured one, not future-dated, and no older than `maximum_oracle_price_age_milliseconds`; the position is initialised, owned by this program and at `compute_position_pda(owner, nonce)` from its own stored fields; debt > 0; `0 < cover <= normalized_debt`, `0 < seize <= collateral`; the proportionality bound `seize <= min(C, ceil(C·cover/N·1.25))` in U512 (`MAX_PENALTY_BPS = 2500`); the vault is the position's PDA vault, holds the collateral definition and at least `collateral_amount`; the custody holding is initialised, owned by the vault's token program and of the collateral definition; the redemption price projects to non-zero; and the position is **not** collateralised at `minimum_collateralization_ratio` (the same exact U512 comparison as `withdraw_collateral`, via `checks::is_position_collateralized`).

Effects: full seizure zeroes the position and moves the whole vault balance (donations included) to custody; partial seizure (milestone M1) subtracts both amounts and moves exactly `collateral_to_seize`, leaving donations for the owner's next `deposit_collateral`. The position account is never released (LEZ cannot release accounts); its owner may `close_position` it once the vault is empty. Allowed while frozen.
```

- [ ] **Step 9: Commit (controller — GPG signing on Windows) — then export the patch series**

The implementer stages the files and reports; the controller commits from Git Bash.

```bash
git add programs/stablecoin/src/checks.rs programs/stablecoin/src/liquidation_hooks.rs programs/stablecoin/methods/guest/src/bin/stablecoin.rs idl/stablecoin-idl.json docs/architecture/hooks.md
git commit -m "feat(stablecoin): liquidate_position hook with proportionality bound and vault sweep"
bash scripts/export-hook-patches.sh && MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/check-hook-patches.sh'
git add patches/stablecoin && git commit -m "chore: export stablecoin hook patch series"
```

---

### Task 6: `auction_engine_core` — instruction enum, account states, PDA seeds, errors, argument parsing

**Files:**
- Create: `programs/auction_engine/core/Cargo.toml`
- Create: `programs/auction_engine/core/src/lib.rs`, `pda.rs`, `error.rs`, `state.rs`, `args.rs`, `instruction.rs`
- Modify: root `Cargo.toml` (`members` += `"programs/auction_engine/core"`; `[workspace.dependencies]` += `auction_core = { path = "crates/auction_core" }`, `auction_engine_core = { path = "programs/auction_engine/core" }`), `.github/workflows/core.yml` (fmt += `-p auction_engine_core`)
- Create: `docs/architecture/programs.md`; Modify: `docs/design-log.md` (ADR-027, ADR-030)

**Interfaces:**
- Consumes: `auction_core::{schedule::{Schedule, SchedulePoint, MAX_POINTS}, price::{FeedParams, OracleScale}, pause::PauseClock, liquidation::CollateralParams, auction::{Auction, Status}, accounting::Accounting, system::{SystemParams, SurplusAuction, DebtAuction, SystemAuctionError}, error::{AuctionError, ConfigError, PauseReason}, math::MathError}`; `lee_core::{account::{AccountId, Data}, program::{PdaSeed, ProgramId}}`.
- Produces (every later task uses these exact names):
  - `pda`: `seed_from_str`, `u64_seed`, `combine_seeds`; seed fns `config_seed(&AccountId /*admin*/, u64)`, `coll_config_seed(&AccountId, &AccountId)`, `accounting_seed(&AccountId)`, `buffer_seed(&AccountId)`, `liqrec_seed(&AccountId, &AccountId)`, `auction_seed(&AccountId, &AccountId, u64)`, `vault_seed(&AccountId)`, `surplus_auction_seed(&AccountId, u64)`, `surplus_vault_seed(&AccountId)`, `debt_auction_seed(&AccountId, u64)`, `engine_authority_seed(&AccountId)`, `mint_authority_seed(&AccountId)`, `unused_slot_seed(&AccountId, u64)` (all `-> PdaSeed`), and the same names with `_pda(&ProgramId, …) -> AccountId`; `SLOT_REF_ORACLE: u64 = 0`, `SLOT_PROTOCOL_TOKEN: u64 = 1`.
  - `error::EngineError` (variants and codes of Shared Reference R4), `EngineError::code(&self) -> u32`, `From<AuctionError | SystemAuctionError | ConfigError | MathError | PauseReason>`.
  - `state`: `SchedulePointRec`, `SystemParamsRec`, `EngineConfigState`, `CollateralConfigState`, `AccountingState`, `LiquidationRecordState`, `AuctionState`, `SurplusAuctionState`, `DebtAuctionState`, tag constants `TAG_*`, `From<&T> for Data` and `TryFrom<&Data> for T` for each; conversions listed in Step 5.
  - `args`: `EngineParamsArgs`, `CollateralArgs`, `bps(u32) -> Result<u16, EngineError>`, `schedule_from_words(&[u32]) -> Result<Schedule, EngineError>`, `EngineParamsArgs::validate(&self) -> Result<SystemParams, EngineError>`, `CollateralArgs::validate(&self, global_cap: u128) -> Result<(CollateralParams, FeedParams), EngineError>`.
  - `instruction::Instruction` with `InitConfig` (0) and `SetEngineParams` (1); Tasks 9–15 append the other variants in Shared Reference R2 order (the guest macro needs a function for every variant, so variants arrive with their handlers).

- [ ] **Step 1: Write the failing tests**

Create `programs/auction_engine/core/Cargo.toml`:

```toml
[package]
name = "auction_engine_core"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[dependencies]
auction_core = { path = "../../../crates/auction_core" }
lee_core = { git = "https://github.com/logos-blockchain/logos-execution-zone.git", tag = "v0.2.4", features = ["host"] }
borsh = { version = "1.5", features = ["derive"] }
serde = { version = "1.0", features = ["derive"] }
risc0-zkvm = { version = "=3.0.5", default-features = false }

[dev-dependencies]
spel-framework = { git = "https://github.com/logos-co/spel.git", rev = "1ef0500f8fc8ce3ddf95523726ae159db83f744f", package = "spel-framework" }
risc0-zkvm = { version = "=3.0.5" }
```

(The dev-dependency enables `risc0_zkvm::serde::to_vec` with std for the index test; the normal dependency stays `default-features = false` so the guest build is unchanged.)

Create `programs/auction_engine/core/src/lib.rs` with the module list and a test module:

```rust
//! Shared types of the `auction_engine` program: instruction enum, account states, PDA seeds,
//! errors and argument parsing. Used by the guest, the host handlers, tests and (Plan 3) the SDK.
#![forbid(unsafe_code)]

pub mod args;
pub mod error;
pub mod instruction;
pub mod pda;
pub mod state;

pub use error::EngineError;
pub use instruction::Instruction;

#[cfg(test)]
mod tests {
    use super::*;
    use auction_core::schedule::SchedulePoint;
    use lee_core::account::{AccountId, Data};
    use pda::*;
    use state::*;

    const ENGINE: lee_core::program::ProgramId = [0xE1; 8];

    #[test]
    fn pdas_match_spel_derivation() {
        let config = AccountId::new([0x11; 32]);
        let position = AccountId::new([0x22; 32]);
        let auction = auction_pda(&ENGINE, &config, &position, 7);
        let spel = spel_framework::pda::compute_pda(
            &ENGINE,
            &[&seed_from_str("AUCTION"), config.value(), position.value(), &u64_seed(7)],
        );
        assert_eq!(auction, spel);
        let admin = AccountId::new([0x33; 32]);
        assert_eq!(
            config_pda(&ENGINE, &admin, 42),
            spel_framework::pda::compute_pda(&ENGINE, &[&seed_from_str("CONFIG"), admin.value(), &u64_seed(42)])
        );
        assert_ne!(config_pda(&ENGINE, &admin, 42), config_pda(&ENGINE, &position, 42), "admin is part of the seed");
        assert_eq!(
            vault_pda(&ENGINE, &auction),
            spel_framework::pda::compute_pda(&ENGINE, &[&seed_from_str("VAULT"), auction.value()])
        );
        assert_eq!(
            engine_authority_pda(&ENGINE, &config),
            spel_framework::pda::compute_pda(&ENGINE, &[&seed_from_str("ENGINE_AUTH"), config.value()])
        );
        assert_ne!(engine_authority_pda(&ENGINE, &config), engine_authority_pda(&ENGINE, &position));
        assert_ne!(unused_slot_pda(&ENGINE, &config, 0), unused_slot_pda(&ENGINE, &config, 1));
    }

    #[test]
    fn auction_state_round_trips_and_rejects_wrong_tag() {
        let s = AuctionState {
            tag: TAG_AUCTION,
            config: AccountId::new([1; 32]),
            position: AccountId::new([2; 32]),
            nonce: 3,
            owner: AccountId::new([4; 32]),
            keeper: AccountId::new([5; 32]),
            settled: false,
            collateral_seized: 10,
            collateral_sold: 0,
            collateral_left: 9,
            keeper_reward: 1,
            leftover_claimable: 0,
            leftover_claimed: 0,
            debt_principal: 100,
            amount_to_raise: 110,
            reserved_amount: 110,
            debt_raised: 0,
            penalty_raised: 0,
            start_ms: 1_000,
            schedule: vec![SchedulePointRec { elapsed_ms: 0, discount_bps: 200 }],
            type_paused_total_at_start: 0,
        };
        let data = Data::from(&s);
        assert_eq!(AuctionState::try_from(&data).unwrap(), s);
        let core = s.to_core().unwrap();
        assert_eq!(core.collateral_left, 9);
        assert_eq!(AuctionState::from_core(&s, &core), s);
        let wrong = AccountingState::default_for_tests();
        assert!(AuctionState::try_from(&Data::from(&wrong)).is_err());
    }

    #[test]
    fn schedule_words_parse_and_validate() {
        let s = args::schedule_from_words(&[0, 200, 2_700_000, 1000]).unwrap();
        assert_eq!(s.points()[1], SchedulePoint { elapsed_ms: 2_700_000, discount_bps: 1000 });
        assert_eq!(args::schedule_from_words(&[0, 200, 5]), Err(EngineError::ScheduleEncoding));
        assert_eq!(args::schedule_from_words(&[]), Err(EngineError::ScheduleEncoding));
        assert_eq!(args::schedule_from_words(&[0, 3_001]), Err(EngineError::InvalidConfig));
        assert_eq!(args::schedule_from_words(&[0, 70_000]), Err(EngineError::BpsOutOfRange));
    }

    #[test]
    fn engine_params_validation() {
        let ok = args::EngineParamsArgs::defaults_for_tests();
        assert!(ok.validate().is_ok());
        let zero_ref = args::EngineParamsArgs { protocol_token_ref_price: 0, ..ok.clone() };
        assert_eq!(zero_ref.validate(), Err(EngineError::ZeroRefPrice));
        let low_threshold = args::EngineParamsArgs { bad_debt_freeze_threshold: ok.debt_lot - 1, ..ok.clone() };
        assert_eq!(low_threshold.validate(), Err(EngineError::ThresholdBelowDebtLot));
        let short_ttl = args::EngineParamsArgs { system_ttl_ms: 599_999, ..ok.clone() };
        assert_eq!(short_ttl.validate(), Err(EngineError::TtlOutOfRange));
        let zero_lot = args::EngineParamsArgs { surplus_lot: 0, ..ok.clone() };
        assert_eq!(zero_lot.validate(), Err(EngineError::InvalidConfig));
        let old_oracle = args::EngineParamsArgs { ref_oracle_max_age_ms: 86_400_001, ..ok.clone() };
        assert_eq!(old_oracle.validate(), Err(EngineError::InvalidConfig));
        let zero_cap = args::EngineParamsArgs { global_cap: 0, ..ok };
        assert_eq!(zero_cap.validate(), Err(EngineError::InvalidConfig));
    }

    #[test]
    fn collateral_args_validation() {
        let ok = args::CollateralArgs::defaults_for_tests();
        assert!(ok.validate(10_000_000).is_ok());
        assert_eq!(ok.validate(ok.type_cap - 1), Err(EngineError::TypeCapAboveGlobal));
        let bad_penalty = args::CollateralArgs { penalty_bps: 2_501, ..ok.clone() };
        assert_eq!(bad_penalty.validate(10_000_000), Err(EngineError::InvalidConfig));
        let chip = args::CollateralArgs { chip_bps: 1_001, ..ok.clone() };
        assert_eq!(chip.validate(10_000_000), Err(EngineError::ChipAbovePenalty));
        let band = args::CollateralArgs { band_lo_bps: 4_999, ..ok.clone() };
        assert_eq!(band.validate(10_000_000), Err(EngineError::InvalidConfig));
        let scale = args::CollateralArgs { oracle_scale: 2, ..ok };
        assert_eq!(scale.validate(10_000_000), Err(EngineError::InvalidConfig));
    }

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(EngineError::CapacityExhausted.code(), 33);
        assert_eq!(EngineError::ChainEndAccountsMissing.code(), 41);
        assert_eq!(EngineError::RecoveryInProgress.code(), 61);
        assert_eq!(EngineError::RefPriceStepTooLarge.code(), 16);
        assert_eq!(EngineError::from(auction_core::error::AuctionError::DustRemainder), EngineError::DustRemainder);
        assert_eq!(
            EngineError::from(auction_core::error::PauseReason::ZeroPrice),
            EngineError::PriceFeedOutOfRange
        );
    }

    pub(crate) fn idx(i: &Instruction) -> u32 {
        risc0_zkvm::serde::to_vec(i).unwrap()[0]
    }

    #[test]
    fn instruction_indices_follow_reference_order() {
        assert_eq!(idx(&Instruction::SetEngineParams {
            global_cap: 0, freeze_on_bad_debt: false, bad_debt_freeze_threshold: 0, protocol_token_enabled: false,
            protocol_token_ref_price: 0, ref_oracle_enabled: false, ref_oracle_max_age_ms: 0, surplus_buffer_target: 0,
            surplus_lot: 0, surplus_delay_ms: 0, surplus_premium_bps: 0, surplus_max_discount_bps: 0, debt_lot: 0,
            debt_initial_discount_bps: 0, debt_growth: vec![], surplus_decay: vec![], max_mint_per_auction: 0,
            min_take: 0, system_ttl_ms: 0,
        }), 1);
        // Later tasks append one assertion per appended variant (Shared Reference R2 index).
    }
}
```

Add `"programs/auction_engine/core"` to the root workspace `members` and the two `[workspace.dependencies]` lines.

- [ ] **Step 2: Run them and see them fail**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_engine_core'`
Expected: compile errors: file not found for modules `args`, `error`, `instruction`, `pda`, `state`.

- [ ] **Step 3: Write `pda.rs`**

```rust
//! PDA seeds of the engine (Shared Reference R1). Encoding = SPEL's (ADR-020, verified in S3):
//! literal → UTF-8 zero-padded to 32 bytes, account → 32-byte id, u64 → little-endian zero-padded,
//! several seeds → SHA-256 of the concatenation.

use lee_core::{
    account::AccountId,
    program::{PdaSeed, ProgramId},
};
use risc0_zkvm::sha::{Impl, Sha256 as _};

pub const SLOT_REF_ORACLE: u64 = 0;
pub const SLOT_PROTOCOL_TOKEN: u64 = 1;

#[must_use]
pub fn seed_from_str(s: &str) -> [u8; 32] {
    let src = s.as_bytes();
    assert!(src.len() <= 32, "seed literal exceeds 32 bytes");
    let mut out = [0u8; 32];
    out[..src.len()].copy_from_slice(src);
    out
}

#[must_use]
pub fn u64_seed(n: u64) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[..8].copy_from_slice(&n.to_le_bytes());
    out
}

#[must_use]
pub fn combine_seeds(seeds: &[&[u8; 32]]) -> [u8; 32] {
    assert!(!seeds.is_empty(), "a PDA needs at least one seed");
    if seeds.len() == 1 {
        return *seeds[0];
    }
    let mut buf = Vec::with_capacity(seeds.len() * 32);
    for s in seeds {
        buf.extend_from_slice(*s);
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(Impl::hash_bytes(&buf).as_bytes());
    out
}

fn seed(parts: &[&[u8; 32]]) -> PdaSeed {
    PdaSeed::new(combine_seeds(parts))
}

fn pda(program_id: &ProgramId, s: &PdaSeed) -> AccountId {
    AccountId::for_public_pda(program_id, s)
}

macro_rules! pda_fns {
    ($seed_fn:ident, $pda_fn:ident, ($($arg:ident : $ty:ty),*), $body:expr) => {
        #[must_use]
        pub fn $seed_fn($($arg: $ty),*) -> PdaSeed { $body }
        #[must_use]
        pub fn $pda_fn(program_id: &ProgramId, $($arg: $ty),*) -> AccountId { pda(program_id, &$seed_fn($($arg),*)) }
    };
}

pda_fns!(config_seed, config_pda, (admin: &AccountId, salt: u64),
    seed(&[&seed_from_str("CONFIG"), admin.value(), &u64_seed(salt)]));
pda_fns!(coll_config_seed, coll_config_pda, (config: &AccountId, collateral_definition: &AccountId),
    seed(&[&seed_from_str("COLL"), config.value(), collateral_definition.value()]));
pda_fns!(accounting_seed, accounting_pda, (config: &AccountId), seed(&[&seed_from_str("ACCT"), config.value()]));
pda_fns!(buffer_seed, buffer_pda, (config: &AccountId), seed(&[&seed_from_str("BUFFER"), config.value()]));
pda_fns!(liqrec_seed, liqrec_pda, (config: &AccountId, position: &AccountId),
    seed(&[&seed_from_str("LIQREC"), config.value(), position.value()]));
pda_fns!(auction_seed, auction_pda, (config: &AccountId, position: &AccountId, nonce: u64),
    seed(&[&seed_from_str("AUCTION"), config.value(), position.value(), &u64_seed(nonce)]));
pda_fns!(vault_seed, vault_pda, (auction: &AccountId), seed(&[&seed_from_str("VAULT"), auction.value()]));
pda_fns!(surplus_auction_seed, surplus_auction_pda, (config: &AccountId, nonce: u64),
    seed(&[&seed_from_str("SURP"), config.value(), &u64_seed(nonce)]));
pda_fns!(surplus_vault_seed, surplus_vault_pda, (surplus_auction: &AccountId),
    seed(&[&seed_from_str("SURPVAULT"), surplus_auction.value()]));
pda_fns!(debt_auction_seed, debt_auction_pda, (config: &AccountId, nonce: u64),
    seed(&[&seed_from_str("DEBT"), config.value(), &u64_seed(nonce)]));
pda_fns!(engine_authority_seed, engine_authority_pda, (config: &AccountId),
    seed(&[&seed_from_str("ENGINE_AUTH"), config.value()]));
pda_fns!(mint_authority_seed, mint_authority_pda, (config: &AccountId),
    seed(&[&seed_from_str("MINT_AUTH"), config.value()]));
pda_fns!(unused_slot_seed, unused_slot_pda, (config: &AccountId, slot: u64),
    seed(&[&seed_from_str("UNUSED"), config.value(), &u64_seed(slot)]));
```

- [ ] **Step 4: Write `error.rs`**

```rust
//! Engine errors (Shared Reference R4). The guest maps them to `SpelError::Custom { code, message }`
//! (`Program error {code}: {message}`); the entry point prefixes `[6000+code]`.

use auction_core::{
    error::{AuctionError, ConfigError, PauseReason},
    math::MathError,
    system::SystemAuctionError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineError {
    NotAdmin,
    ZeroId,
    ZeroRefPrice,
    ThresholdBelowDebtLot,
    InvalidConfig,
    TypeCapAboveGlobal,
    BpsOutOfRange,
    ScheduleEncoding,
    TtlOutOfRange,
    ChipAbovePenalty,
    HostMismatch,
    WrongAccount(&'static str),
    OracleMismatch,
    WrongClock,
    ProtocolTokenLocked,
    RefPriceStepTooLarge,
    Paused,
    PriceFeedStale,
    PriceFeedLowConfidence,
    PriceFeedOutOfRange,
    NonceMismatch,
    ZeroDebt,
    VaultBelowCollateral,
    CapacityExhausted,
    NotAuctioning,
    ZeroFill,
    Slippage,
    BelowMinBid,
    DustRemainder,
    NotOwner,
    NothingToClaim,
    ChainEndAccountsMissing,
    InsufficientBalance,
    RecipientMustSign,
    WrongDefinition,
    VaultBelowBook,
    TokenNotConfigured,
    UnbackedDebtPresent,
    BufferBelowTarget,
    DelayNotElapsed,
    InsufficientBadDebt,
    MintCapConfig,
    MintAuthorityMissing,
    ProtocolTokenMismatch,
    DebtAuctionAlreadyLive,
    SurplusAuctionAlreadyLive,
    NotLive,
    RecoveryInProgress,
    Math,
}

impl EngineError {
    #[must_use]
    pub const fn code(&self) -> u32 {
        match self {
            Self::NotAdmin => 1,
            Self::ZeroId => 2,
            Self::ZeroRefPrice => 3,
            Self::ThresholdBelowDebtLot => 4,
            Self::InvalidConfig => 5,
            Self::TypeCapAboveGlobal => 6,
            Self::BpsOutOfRange => 7,
            Self::ScheduleEncoding => 8,
            Self::TtlOutOfRange => 9,
            Self::ChipAbovePenalty => 10,
            Self::HostMismatch => 11,
            Self::WrongAccount(_) => 12,
            Self::OracleMismatch => 13,
            Self::WrongClock => 14,
            Self::ProtocolTokenLocked => 15,
            Self::RefPriceStepTooLarge => 16,
            Self::Paused => 20,
            Self::PriceFeedStale => 21,
            Self::PriceFeedLowConfidence => 22,
            Self::PriceFeedOutOfRange => 23,
            Self::NonceMismatch => 30,
            Self::ZeroDebt => 31,
            Self::VaultBelowCollateral => 32,
            Self::CapacityExhausted => 33,
            Self::NotAuctioning => 34,
            Self::ZeroFill => 35,
            Self::Slippage => 36,
            Self::BelowMinBid => 37,
            Self::DustRemainder => 38,
            Self::NotOwner => 39,
            Self::NothingToClaim => 40,
            Self::ChainEndAccountsMissing => 41,
            Self::InsufficientBalance => 42,
            Self::RecipientMustSign => 43,
            Self::WrongDefinition => 44,
            Self::VaultBelowBook => 45,
            Self::TokenNotConfigured => 50,
            Self::UnbackedDebtPresent => 51,
            Self::BufferBelowTarget => 52,
            Self::DelayNotElapsed => 53,
            Self::InsufficientBadDebt => 54,
            Self::MintCapConfig => 55,
            Self::MintAuthorityMissing => 56,
            Self::ProtocolTokenMismatch => 57,
            Self::DebtAuctionAlreadyLive => 58,
            Self::SurplusAuctionAlreadyLive => 59,
            Self::NotLive => 60,
            Self::RecoveryInProgress => 61,
            Self::Math => 99,
        }
    }
}

impl core::fmt::Display for EngineError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl From<AuctionError> for EngineError {
    fn from(e: AuctionError) -> Self {
        match e {
            AuctionError::ZeroDebt => Self::ZeroDebt,
            AuctionError::VaultBelowCollateral => Self::VaultBelowCollateral,
            AuctionError::CapacityExhausted => Self::CapacityExhausted,
            AuctionError::NotAuctioning => Self::NotAuctioning,
            AuctionError::ZeroFill => Self::ZeroFill,
            AuctionError::Slippage => Self::Slippage,
            AuctionError::BelowMinBid => Self::BelowMinBid,
            AuctionError::DustRemainder => Self::DustRemainder,
            AuctionError::NotOwner => Self::NotOwner,
            AuctionError::NothingToClaim => Self::NothingToClaim,
            AuctionError::Math => Self::Math,
        }
    }
}

impl From<SystemAuctionError> for EngineError {
    fn from(e: SystemAuctionError) -> Self {
        match e {
            SystemAuctionError::TokenNotConfigured => Self::TokenNotConfigured,
            SystemAuctionError::UnbackedDebtPresent => Self::UnbackedDebtPresent,
            SystemAuctionError::BufferBelowTarget => Self::BufferBelowTarget,
            SystemAuctionError::DelayNotElapsed => Self::DelayNotElapsed,
            SystemAuctionError::InsufficientBadDebt => Self::InsufficientBadDebt,
            SystemAuctionError::MintCapConfig => Self::MintCapConfig,
            SystemAuctionError::MintAuthorityMissing => Self::MintAuthorityMissing,
            SystemAuctionError::ProtocolTokenMismatch => Self::ProtocolTokenMismatch,
            SystemAuctionError::DebtAuctionAlreadyLive => Self::DebtAuctionAlreadyLive,
            SystemAuctionError::SurplusAuctionAlreadyLive => Self::SurplusAuctionAlreadyLive,
            SystemAuctionError::NotLive => Self::NotLive,
            SystemAuctionError::Slippage => Self::Slippage,
            SystemAuctionError::DustRemainder => Self::DustRemainder,
            SystemAuctionError::ZeroFill => Self::ZeroFill,
            SystemAuctionError::Math => Self::Math,
        }
    }
}

impl From<ConfigError> for EngineError {
    fn from(_: ConfigError) -> Self {
        Self::InvalidConfig
    }
}

impl From<MathError> for EngineError {
    fn from(_: MathError) -> Self {
        Self::Math
    }
}

impl From<PauseReason> for EngineError {
    fn from(r: PauseReason) -> Self {
        match r {
            PauseReason::AdminPaused => Self::Paused,
            PauseReason::Stale => Self::PriceFeedStale,
            PauseReason::LowConfidence => Self::PriceFeedLowConfidence,
            PauseReason::ZeroPrice | PauseReason::OutOfRange => Self::PriceFeedOutOfRange,
        }
    }
}
```

- [ ] **Step 5: Write `state.rs`**

```rust
//! Borsh account states of the engine (Shared Reference R1) and their conversions to `auction_core`.
//! Every state starts with an 8-byte tag so one engine account type can never decode as another.

use auction_core::{
    accounting::Accounting,
    auction::{Auction, Status},
    liquidation::CollateralParams,
    pause::PauseClock,
    price::{FeedParams, OracleScale},
    schedule::{Schedule, SchedulePoint},
    system::{DebtAuction, SurplusAuction, SystemParams},
};
use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{
    account::{AccountId, Data},
    program::ProgramId,
};

use crate::EngineError;

pub const TAG_CONFIG: [u8; 8] = *b"AECONFIG";
pub const TAG_COLL: [u8; 8] = *b"AECOLLAT";
pub const TAG_ACCOUNTING: [u8; 8] = *b"AEACCTNG";
pub const TAG_LIQREC: [u8; 8] = *b"AELIQREC";
pub const TAG_AUCTION: [u8; 8] = *b"AEAUCTN_";
pub const TAG_SURPLUS: [u8; 8] = *b"AESURPLS";
pub const TAG_DEBT: [u8; 8] = *b"AEDEBT__";

macro_rules! account_data {
    ($t:ty, $tag:expr) => {
        impl TryFrom<&Data> for $t {
            type Error = std::io::Error;
            fn try_from(data: &Data) -> Result<Self, Self::Error> {
                let v = <$t>::try_from_slice(data.as_ref())?;
                if v.tag != $tag {
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "wrong account tag"));
                }
                Ok(v)
            }
        }
        impl From<&$t> for Data {
            fn from(v: &$t) -> Self {
                let buf = borsh::to_vec(v).expect("engine state serialises");
                Data::try_from(buf).expect("engine state fits into Data")
            }
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SchedulePointRec {
    pub elapsed_ms: u64,
    pub discount_bps: u16,
}

#[must_use]
pub fn recs_from_schedule(s: &Schedule) -> Vec<SchedulePointRec> {
    s.points()
        .iter()
        .map(|p| SchedulePointRec { elapsed_ms: p.elapsed_ms, discount_bps: p.discount_bps })
        .collect()
}

pub fn schedule_from_recs(recs: &[SchedulePointRec]) -> Result<Schedule, EngineError> {
    let pts: Vec<SchedulePoint> = recs
        .iter()
        .map(|r| SchedulePoint { elapsed_ms: r.elapsed_ms, discount_bps: r.discount_bps })
        .collect();
    Ok(Schedule::new(&pts)?)
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SystemParamsRec {
    pub surplus_buffer_target: u128,
    pub surplus_lot: u128,
    pub surplus_delay_ms: u64,
    pub surplus_premium_bps: u16,
    pub surplus_max_discount_bps: u16,
    pub debt_lot: u128,
    pub debt_initial_discount_bps: u16,
    pub debt_growth: Vec<SchedulePointRec>,
    pub surplus_decay: Vec<SchedulePointRec>,
    pub max_mint_per_auction: u128,
    pub min_take: u128,
    pub system_ttl_ms: u64,
}

impl SystemParamsRec {
    #[must_use]
    pub fn from_core(s: &SystemParams) -> Self {
        Self {
            surplus_buffer_target: s.surplus_buffer_target,
            surplus_lot: s.surplus_lot,
            surplus_delay_ms: s.surplus_delay_ms,
            surplus_premium_bps: s.surplus_premium_bps,
            surplus_max_discount_bps: s.surplus_max_discount_bps,
            debt_lot: s.debt_lot,
            debt_initial_discount_bps: s.debt_initial_discount_bps,
            debt_growth: recs_from_schedule(&s.debt_growth),
            surplus_decay: recs_from_schedule(&s.surplus_decay),
            max_mint_per_auction: s.max_mint_per_auction,
            min_take: s.min_take,
            system_ttl_ms: s.system_ttl_ms,
        }
    }

    pub fn to_core(&self) -> Result<SystemParams, EngineError> {
        let s = SystemParams {
            surplus_buffer_target: self.surplus_buffer_target,
            surplus_lot: self.surplus_lot,
            surplus_delay_ms: self.surplus_delay_ms,
            surplus_premium_bps: self.surplus_premium_bps,
            surplus_max_discount_bps: self.surplus_max_discount_bps,
            debt_lot: self.debt_lot,
            debt_initial_discount_bps: self.debt_initial_discount_bps,
            debt_growth: schedule_from_recs(&self.debt_growth)?,
            surplus_decay: schedule_from_recs(&self.surplus_decay)?,
            max_mint_per_auction: self.max_mint_per_auction,
            min_take: self.min_take,
            system_ttl_ms: self.system_ttl_ms,
        };
        s.validate()?;
        Ok(s)
    }
}

/// `["CONFIG", admin, salt]`. Admin, host binding and token program are fixed at `init_config`
/// (the admin is part of the PDA; clients verify the stored program ids, I4).
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct EngineConfigState {
    pub tag: [u8; 8],
    pub salt: u64,
    pub admin: AccountId,
    pub host_program_id: ProgramId,
    pub host_protocol_parameters_id: AccountId,
    pub stablecoin_definition_id: AccountId,
    pub collateral_definition_id: AccountId,
    pub token_program_id: ProgramId,
    pub protocol_token: Option<AccountId>,
    pub protocol_token_ref_price: u128,
    pub protocol_token_oracle_id: Option<AccountId>,
    pub ref_oracle_max_age_ms: u64,
    pub global_cap: u128,
    pub global_reserved: u128,
    pub freeze_on_bad_debt: bool,
    pub bad_debt_freeze_threshold: u128,
    pub system: SystemParamsRec,
}
account_data!(EngineConfigState, TAG_CONFIG);

/// `["COLL", config, collateral_definition]`. One per engine instance (the host has one collateral type).
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CollateralConfigState {
    pub tag: [u8; 8],
    pub config: AccountId,
    pub collateral_definition_id: AccountId,
    pub oracle_id: AccountId,
    /// 0 = Ray, 1 = Q64x64.
    pub oracle_scale: u8,
    pub max_oracle_age_ms: u64,
    pub max_conf_bps: u16,
    pub band_lo_bps: u16,
    pub band_hi_bps: u16,
    pub penalty_bps: u16,
    pub tip: u128,
    pub chip_bps: u16,
    pub reward_cap: u128,
    pub debt_floor: u128,
    pub min_bid: u128,
    pub type_cap: u128,
    pub type_reserved: u128,
    pub schedule: Vec<SchedulePointRec>,
    pub admin_paused: bool,
    pub paused_since_ms: Option<u64>,
    pub paused_total_ms: u64,
    pub last_seen_oracle_ts: Option<u64>,
}
account_data!(CollateralConfigState, TAG_COLL);

impl CollateralConfigState {
    pub fn feed_params(&self) -> Result<FeedParams, EngineError> {
        let scale = match self.oracle_scale {
            0 => OracleScale::Ray,
            1 => OracleScale::Q64x64,
            _ => return Err(EngineError::InvalidConfig),
        };
        Ok(FeedParams {
            scale,
            max_age_ms: self.max_oracle_age_ms,
            max_conf_bps: self.max_conf_bps,
            band_lo_bps: self.band_lo_bps,
            band_hi_bps: self.band_hi_bps,
        })
    }

    pub fn collateral_params(&self) -> Result<CollateralParams, EngineError> {
        Ok(CollateralParams {
            penalty_bps: self.penalty_bps,
            tip: self.tip,
            chip_bps: self.chip_bps,
            reward_cap: self.reward_cap,
            debt_floor: self.debt_floor,
            min_bid: self.min_bid,
            schedule: schedule_from_recs(&self.schedule)?,
        })
    }

    #[must_use]
    pub fn pause_clock(&self) -> PauseClock {
        PauseClock {
            paused_since_ms: self.paused_since_ms,
            paused_total_ms: self.paused_total_ms,
            last_seen_oracle_ts: self.last_seen_oracle_ts,
        }
    }

    #[must_use]
    pub fn with_pause_clock(&self, c: PauseClock) -> Self {
        Self {
            paused_since_ms: c.paused_since_ms,
            paused_total_ms: c.paused_total_ms,
            last_seen_oracle_ts: c.last_seen_oracle_ts,
            ..self.clone()
        }
    }
}

/// `["ACCT", config]`: the core `Accounting` fields plus the program-layer system-auction state.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct AccountingState {
    pub tag: [u8; 8],
    pub unbacked_debt: u128,
    pub total_shortfall_booked: u128,
    pub total_penalty_collected: u128,
    pub total_debt_burned: u128,
    pub total_debt_settled: u128,
    pub surplus_nonce: u64,
    pub debt_nonce: u64,
    pub host_frozen_by_engine: bool,
    pub debt_on_auction: u128,
    pub total_debt_auctioned: u128,
    pub surplus_live: bool,
    pub debt_live: bool,
    pub last_surplus_end_ms: u64,
    pub total_protocol_burned: u128,
    pub total_protocol_minted: u128,
    pub total_surplus_lots_out: u128,
    pub total_surplus_returned: u128,
}
account_data!(AccountingState, TAG_ACCOUNTING);

impl AccountingState {
    #[must_use]
    pub fn new() -> Self {
        Self::with_core(&Self::empty(), &Accounting::default())
    }

    fn empty() -> Self {
        Self {
            tag: TAG_ACCOUNTING,
            unbacked_debt: 0,
            total_shortfall_booked: 0,
            total_penalty_collected: 0,
            total_debt_burned: 0,
            total_debt_settled: 0,
            surplus_nonce: 0,
            debt_nonce: 0,
            host_frozen_by_engine: false,
            debt_on_auction: 0,
            total_debt_auctioned: 0,
            surplus_live: false,
            debt_live: false,
            last_surplus_end_ms: 0,
            total_protocol_burned: 0,
            total_protocol_minted: 0,
            total_surplus_lots_out: 0,
            total_surplus_returned: 0,
        }
    }

    /// Test helper so other crates can build a tagged, empty state.
    #[must_use]
    pub fn default_for_tests() -> Self {
        Self::new()
    }

    #[must_use]
    pub fn core(&self) -> Accounting {
        Accounting {
            unbacked_debt: self.unbacked_debt,
            total_shortfall_booked: self.total_shortfall_booked,
            total_penalty_collected: self.total_penalty_collected,
            total_debt_burned: self.total_debt_burned,
            total_debt_settled: self.total_debt_settled,
            surplus_nonce: self.surplus_nonce,
            debt_nonce: self.debt_nonce,
            host_frozen_by_engine: self.host_frozen_by_engine,
            debt_on_auction: self.debt_on_auction,
            total_debt_auctioned: self.total_debt_auctioned,
        }
    }

    #[must_use]
    pub fn with_core(base: &Self, a: &Accounting) -> Self {
        Self {
            unbacked_debt: a.unbacked_debt,
            total_shortfall_booked: a.total_shortfall_booked,
            total_penalty_collected: a.total_penalty_collected,
            total_debt_burned: a.total_debt_burned,
            total_debt_settled: a.total_debt_settled,
            surplus_nonce: a.surplus_nonce,
            debt_nonce: a.debt_nonce,
            host_frozen_by_engine: a.host_frozen_by_engine,
            debt_on_auction: a.debt_on_auction,
            total_debt_auctioned: a.total_debt_auctioned,
            ..base.clone()
        }
    }
}

impl Default for AccountingState {
    fn default() -> Self {
        Self::new()
    }
}

/// `["LIQREC", config, position]`: the per-position auction counter (ADR-004, ADR-020).
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct LiquidationRecordState {
    pub tag: [u8; 8],
    pub position: AccountId,
    pub count: u64,
}
account_data!(LiquidationRecordState, TAG_LIQREC);

/// `["AUCTION", config, position, nonce]`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct AuctionState {
    pub tag: [u8; 8],
    pub config: AccountId,
    pub position: AccountId,
    pub nonce: u64,
    pub owner: AccountId,
    pub keeper: AccountId,
    pub settled: bool,
    pub collateral_seized: u128,
    pub collateral_sold: u128,
    pub collateral_left: u128,
    pub keeper_reward: u128,
    pub leftover_claimable: u128,
    pub leftover_claimed: u128,
    pub debt_principal: u128,
    pub amount_to_raise: u128,
    pub reserved_amount: u128,
    pub debt_raised: u128,
    pub penalty_raised: u128,
    pub start_ms: u64,
    pub schedule: Vec<SchedulePointRec>,
    pub type_paused_total_at_start: u64,
}
account_data!(AuctionState, TAG_AUCTION);

impl AuctionState {
    pub fn to_core(&self) -> Result<Auction, EngineError> {
        Ok(Auction {
            status: if self.settled { Status::Settled } else { Status::Auctioning },
            collateral_seized: self.collateral_seized,
            collateral_sold: self.collateral_sold,
            collateral_left: self.collateral_left,
            keeper_reward: self.keeper_reward,
            leftover_claimable: self.leftover_claimable,
            leftover_claimed: self.leftover_claimed,
            debt_principal: self.debt_principal,
            amount_to_raise: self.amount_to_raise,
            reserved_amount: self.reserved_amount,
            debt_raised: self.debt_raised,
            penalty_raised: self.penalty_raised,
            start_ms: self.start_ms,
            schedule: schedule_from_recs(&self.schedule)?,
            type_paused_total_at_start: self.type_paused_total_at_start,
        })
    }

    /// Keeps the identity fields of `base` and copies every core field of `a`.
    #[must_use]
    pub fn from_core(base: &Self, a: &Auction) -> Self {
        Self {
            settled: a.status == Status::Settled,
            collateral_seized: a.collateral_seized,
            collateral_sold: a.collateral_sold,
            collateral_left: a.collateral_left,
            keeper_reward: a.keeper_reward,
            leftover_claimable: a.leftover_claimable,
            leftover_claimed: a.leftover_claimed,
            debt_principal: a.debt_principal,
            amount_to_raise: a.amount_to_raise,
            reserved_amount: a.reserved_amount,
            debt_raised: a.debt_raised,
            penalty_raised: a.penalty_raised,
            start_ms: a.start_ms,
            schedule: recs_from_schedule(&a.schedule),
            type_paused_total_at_start: a.type_paused_total_at_start,
            ..base.clone()
        }
    }
}

/// `["SURP", config, nonce]`. `ref_ray` is snapshotted at start, so takes need no oracle account.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SurplusAuctionState {
    pub tag: [u8; 8],
    pub config: AccountId,
    pub nonce: u64,
    pub lot_left: u128,
    pub start_ms: u64,
    pub live: bool,
    pub ref_ray: u128,
}
account_data!(SurplusAuctionState, TAG_SURPLUS);

impl SurplusAuctionState {
    #[must_use]
    pub fn core(&self) -> SurplusAuction {
        SurplusAuction { lot_left: self.lot_left, start_ms: self.start_ms, live: self.live }
    }
    #[must_use]
    pub fn with_core(&self, a: &SurplusAuction) -> Self {
        Self { lot_left: a.lot_left, start_ms: a.start_ms, live: a.live, ..self.clone() }
    }
}

/// `["DEBT", config, nonce]`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct DebtAuctionState {
    pub tag: [u8; 8],
    pub config: AccountId,
    pub nonce: u64,
    pub lot_left: u128,
    pub start_amount: u128,
    pub start_ms: u64,
    pub live: bool,
}
account_data!(DebtAuctionState, TAG_DEBT);

impl DebtAuctionState {
    #[must_use]
    pub fn core(&self) -> DebtAuction {
        DebtAuction { lot_left: self.lot_left, start_amount: self.start_amount, start_ms: self.start_ms, live: self.live }
    }
    #[must_use]
    pub fn with_core(&self, a: &DebtAuction) -> Self {
        Self { lot_left: a.lot_left, start_amount: a.start_amount, start_ms: a.start_ms, live: a.live, ..self.clone() }
    }
}
```

Note: `AccountingState::new()` calls `with_core(&Self::empty(), …)`; `AccountingState::try_from(&Data::from(&AccountingState))` in the round-trip test decodes as `AuctionState` and fails on borsh length or tag; both are an `Err`.

- [ ] **Step 6: Write `args.rs`**

```rust
//! Instruction arguments arrive in spel-encodable types (ADR-030): bps as `u32`, schedules as
//! `Vec<u32>` of interleaved `(elapsed_ms, bps)` pairs. This module converts and validates them.

use auction_core::{
    liquidation::CollateralParams,
    price::{FeedParams, OracleScale},
    schedule::{Schedule, SchedulePoint, MAX_POINTS},
    system::SystemParams,
    units::BPS,
};

use crate::EngineError;

/// System auction ttl bounds (spec §5.12: 10 min – 7 d).
pub const MIN_SYSTEM_TTL_MS: u64 = 600_000;
pub const MAX_SYSTEM_TTL_MS: u64 = 604_800_000;

pub fn bps(v: u32) -> Result<u16, EngineError> {
    if u128::from(v) > BPS {
        return Err(EngineError::BpsOutOfRange);
    }
    u16::try_from(v).map_err(|_| EngineError::BpsOutOfRange)
}

pub fn schedule_from_words(words: &[u32]) -> Result<Schedule, EngineError> {
    if words.is_empty() || words.len() % 2 != 0 || words.len() > 2 * MAX_POINTS {
        return Err(EngineError::ScheduleEncoding);
    }
    let mut points = Vec::with_capacity(words.len() / 2);
    for pair in words.chunks_exact(2) {
        points.push(SchedulePoint { elapsed_ms: u64::from(pair[0]), discount_bps: bps(pair[1])? });
    }
    Ok(Schedule::new(&points)?)
}

/// Flat engine parameters of `init_config` / `set_engine_params`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineParamsArgs {
    pub global_cap: u128,
    pub freeze_on_bad_debt: bool,
    pub bad_debt_freeze_threshold: u128,
    pub protocol_token_enabled: bool,
    pub protocol_token_ref_price: u128,
    pub ref_oracle_enabled: bool,
    pub ref_oracle_max_age_ms: u64,
    pub surplus_buffer_target: u128,
    pub surplus_lot: u128,
    pub surplus_delay_ms: u64,
    pub surplus_premium_bps: u32,
    pub surplus_max_discount_bps: u32,
    pub debt_lot: u128,
    pub debt_initial_discount_bps: u32,
    pub debt_growth: Vec<u32>,
    pub surplus_decay: Vec<u32>,
    pub max_mint_per_auction: u128,
    pub min_take: u128,
    pub system_ttl_ms: u64,
}

impl EngineParamsArgs {
    /// Validates everything ADR-024 requires at config time and returns the core `SystemParams`.
    pub fn validate(&self) -> Result<SystemParams, EngineError> {
        if self.protocol_token_ref_price == 0 {
            return Err(EngineError::ZeroRefPrice);
        }
        if self.global_cap == 0 {
            return Err(EngineError::InvalidConfig);
        }
        if !(MIN_SYSTEM_TTL_MS..=MAX_SYSTEM_TTL_MS).contains(&self.system_ttl_ms) {
            return Err(EngineError::TtlOutOfRange);
        }
        if (self.ref_oracle_enabled && self.ref_oracle_max_age_ms == 0)
            || self.ref_oracle_max_age_ms > auction_core::price::MAX_ORACLE_AGE_MS
        {
            return Err(EngineError::InvalidConfig); // ≤ 24 h, like the collateral feed (spec §5.12)
        }
        let s = SystemParams {
            surplus_buffer_target: self.surplus_buffer_target,
            surplus_lot: self.surplus_lot,
            surplus_delay_ms: self.surplus_delay_ms,
            surplus_premium_bps: bps(self.surplus_premium_bps)?,
            surplus_max_discount_bps: bps(self.surplus_max_discount_bps)?,
            debt_lot: self.debt_lot,
            debt_initial_discount_bps: bps(self.debt_initial_discount_bps)?,
            debt_growth: schedule_from_words(&self.debt_growth)?,
            surplus_decay: schedule_from_words(&self.surplus_decay)?,
            max_mint_per_auction: self.max_mint_per_auction,
            min_take: self.min_take,
            system_ttl_ms: self.system_ttl_ms,
        };
        s.validate()?;
        if self.bad_debt_freeze_threshold < s.debt_lot {
            return Err(EngineError::ThresholdBelowDebtLot);
        }
        Ok(s)
    }

    /// Defaults used by tests and the localnet smoke (raw units; spec §5.12 shapes).
    #[must_use]
    pub fn defaults_for_tests() -> Self {
        Self {
            global_cap: 10_000_000,
            freeze_on_bad_debt: true,
            bad_debt_freeze_threshold: 1_000,
            protocol_token_enabled: false,
            protocol_token_ref_price: 2 * auction_core::units::RAY,
            ref_oracle_enabled: false,
            ref_oracle_max_age_ms: 3_600_000,
            surplus_buffer_target: 1_000,
            surplus_lot: 500,
            surplus_delay_ms: 0,
            surplus_premium_bps: 500,
            surplus_max_discount_bps: 2_000,
            debt_lot: 500,
            debt_initial_discount_bps: 0,
            debt_growth: vec![0, 0, 1_000, 3_000],
            surplus_decay: vec![0, 0, 1_000, 2_500],
            max_mint_per_auction: 5_000,
            min_take: 10,
            system_ttl_ms: 14_400_000,
        }
    }
}

/// Flat collateral parameters of `set_collateral_config`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollateralArgs {
    pub oracle_scale: u8,
    pub max_oracle_age_ms: u64,
    pub max_conf_bps: u32,
    pub band_lo_bps: u32,
    pub band_hi_bps: u32,
    pub penalty_bps: u32,
    pub tip: u128,
    pub chip_bps: u32,
    pub reward_cap: u128,
    pub debt_floor: u128,
    pub min_bid: u128,
    pub type_cap: u128,
    pub schedule: Vec<u32>,
    pub admin_paused: bool,
}

impl CollateralArgs {
    pub fn validate(&self, global_cap: u128) -> Result<(CollateralParams, FeedParams), EngineError> {
        let scale = match self.oracle_scale {
            0 => OracleScale::Ray,
            1 => OracleScale::Q64x64,
            _ => return Err(EngineError::InvalidConfig),
        };
        let band_hi = u16::try_from(self.band_hi_bps).map_err(|_| EngineError::BpsOutOfRange)?;
        let feed = FeedParams {
            scale,
            max_age_ms: self.max_oracle_age_ms,
            max_conf_bps: bps(self.max_conf_bps)?,
            band_lo_bps: bps(self.band_lo_bps)?,
            band_hi_bps: band_hi,
        };
        feed.validate()?;
        if feed.max_age_ms == 0 {
            return Err(EngineError::InvalidConfig);
        }
        let params = CollateralParams {
            penalty_bps: bps(self.penalty_bps)?,
            tip: self.tip,
            chip_bps: bps(self.chip_bps)?,
            reward_cap: self.reward_cap,
            debt_floor: self.debt_floor,
            min_bid: self.min_bid,
            schedule: schedule_from_words(&self.schedule)?,
        };
        params.validate()?;
        if params.chip_bps > params.penalty_bps {
            return Err(EngineError::ChipAbovePenalty);
        }
        if self.type_cap == 0 {
            return Err(EngineError::InvalidConfig);
        }
        if self.type_cap > global_cap {
            return Err(EngineError::TypeCapAboveGlobal);
        }
        Ok((params, feed))
    }

    #[must_use]
    pub fn defaults_for_tests() -> Self {
        Self {
            oracle_scale: 0,
            max_oracle_age_ms: 3_600_000,
            max_conf_bps: 100,
            band_lo_bps: 9_000,
            band_hi_bps: 11_000,
            penalty_bps: 1_000,
            tip: 5,
            chip_bps: 10,
            reward_cap: 50,
            debt_floor: 100,
            min_bid: 25,
            type_cap: 5_000_000,
            schedule: vec![0, 200, 2_700_000, 1_000],
            admin_paused: false,
        }
    }
}
```

`band_hi_bps` may exceed 10_000 (up to 20_000), so it is converted with `u16::try_from` and range-checked by `FeedParams::validate`, not by `bps()`.

- [ ] **Step 7: Write `instruction.rs`**

```rust
//! The engine instruction enum. Variant order == guest function order == IDL order (spel-cli
//! encodes by IDL position; the guest decodes by enum index). Never reorder; only append, in
//! Shared Reference R2 order.
//! Account lists: Shared Reference R2 / `docs/architecture/programs.md`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Instruction {
    /// Accounts: admin(s), config(i), accounting(i), buffer, protocol_parameters, stablecoin_definition,
    /// protocol_token_definition [slot 1], ref_oracle [slot 0].
    InitConfig {
        salt: u64,
        global_cap: u128,
        freeze_on_bad_debt: bool,
        bad_debt_freeze_threshold: u128,
        protocol_token_enabled: bool,
        protocol_token_ref_price: u128,
        ref_oracle_enabled: bool,
        ref_oracle_max_age_ms: u64,
        surplus_buffer_target: u128,
        surplus_lot: u128,
        surplus_delay_ms: u64,
        surplus_premium_bps: u32,
        surplus_max_discount_bps: u32,
        debt_lot: u128,
        debt_initial_discount_bps: u32,
        debt_growth: Vec<u32>,
        surplus_decay: Vec<u32>,
        max_mint_per_auction: u128,
        min_take: u128,
        system_ttl_ms: u64,
    },
    /// Accounts: admin(s), config, accounting, coll_config (read, may be uninitialised), protocol_token_definition
    /// [slot 1], ref_oracle [slot 0].
    SetEngineParams {
        global_cap: u128,
        freeze_on_bad_debt: bool,
        bad_debt_freeze_threshold: u128,
        protocol_token_enabled: bool,
        protocol_token_ref_price: u128,
        ref_oracle_enabled: bool,
        ref_oracle_max_age_ms: u64,
        surplus_buffer_target: u128,
        surplus_lot: u128,
        surplus_delay_ms: u64,
        surplus_premium_bps: u32,
        surplus_max_discount_bps: u32,
        debt_lot: u128,
        debt_initial_discount_bps: u32,
        debt_growth: Vec<u32>,
        surplus_decay: Vec<u32>,
        max_mint_per_auction: u128,
        min_take: u128,
        system_ttl_ms: u64,
    },
    // Tasks 9–15 append the remaining variants here, in Shared Reference R2 order
    // (set_collateral_config = 2 … check_solvency = 16). Each appending task also appends the guest
    // function and an index assertion to `instruction_indices_follow_reference_order`.
}
```

- [ ] **Step 8: Run the tests and see them pass**

Run: the Step 2 command. Expected: `7 passed`. Then `scripts/wsl-cargo.sh clippy -p auction_engine_core --all-targets -- -D warnings` — no warnings.

- [ ] **Step 9: Docs, ADR-027, ADR-030, CI fmt scope**

Create `docs/architecture/programs.md` with the sections "Layout" (core/src/methods split), "Accounts and seeds" (paste Shared Reference R1 verbatim), "Instructions and account lists" (paste R2), "Errors" (R4), and "Interfaces for Plan 3" with:

```markdown
## Interfaces for Plan 3 (SDK, CLI, CI)

- Instruction encoding: risc0 serde of `auction_engine_core::Instruction` (`risc0_zkvm::serde::to_vec`, first word = variant index = IDL index). Stablecoin hooks: `stablecoin_core::Instruction` indices 19–21.
- Account layouts: borsh of the `auction_engine_core::state` structs, each starting with its 8-byte tag (`TAG_*`). Decoders must check the tag.
- PDAs: `auction_engine_core::pda::*_pda` (the SDK must reuse these functions, not re-implement them).
- IDLs: `idl/auction_engine-idl.json`, `idl/stablecoin-idl.json` (regenerated with `tools/idl-gen` by every task that changes a guest).
- Errors: `EngineError::code()`; the guest message is `Program error [6000+code]: <Name>`.
- Program-id verification (I4): before building ANY transaction for a config, the SDK/CLI reads its `EngineConfig` and refuses unless `host_program_id` and `token_program_id` equal the expected ImageIDs in `DEPLOYMENTS.md` (and `admin` is the expected operator): the engine chains to those stored ids with the user's signer flags.
- Bid account choice (ADR-029): list `accounting` (`bid_with_settlement`) whenever `coin_in_max >= remaining_target` or the quoted fill would empty the lot within the quote's slippage margin.
- Optional slots (ADR-030): pass `unused_slot_pda(config, 0|1)` when the reference oracle / protocol token is not configured.
- Signers that sign more than one stablecoin-program transaction must be `wallet auth-transfer init`-ed first (ADR-018).
```

Append ADR-027 and ADR-030 to `docs/design-log.md`:

```markdown
---

## ADR-027: Engine PDAs Are Seeded by the Engine Configuration

**Context**
One engine program (one ImageID) serves several `EngineConfig`s (spec §5.4 seeds them `["CONFIG", salt]`), one per host instance. `init_config` is permissionless by design (whoever signs becomes that config's admin). If the engine authority were a single program-wide PDA (`["ENGINE_AUTH"]`, as in spike S2), a rogue config created against host A's `ProtocolParameters` could call `liquidate_position` on A's positions with its own penalty and keeper reward, and A's Variant C guard would accept it (right caller program, authorised PDA). The same holds for the protocol-token mint authority.

**Decision**
Engine-authority and mint-authority PDAs include the config: `["ENGINE_AUTH", config]`, `["MINT_AUTH", config]`. A host binds exactly one engine config by storing that config's authority id (ADR-026). This is how the engine "identifies the host instance": the host names the config, and the config stores the host program id and `ProtocolParameters` id, re-checked on every hook call path.

The config PDA itself includes its admin: `["CONFIG", admin, salt]` (amends spec §5.4 `["CONFIG", salt]`). With `["CONFIG", salt]` anyone could front-run `init_config` at the address an operator announced and own that config; with the admin in the seed, a config initialised by a different admin with the same salt is a different account (tests `nobody_can_squat_the_admins_config_address`, `rogue_config_with_the_same_salt_cannot_liquidate_the_bound_host`). The admin is therefore immutable.

**Bind-time verification.** Before `set_liquidation_engine`, the host admin reads the `EngineConfig` it is about to bind and checks: `admin` is the expected operator, `host_program_id` and `host_protocol_parameters_id` are this host, `token_program_id` is the expected token ImageID, and the parameters (caps, penalty bounds via its `CollateralConfig`, protocol token) are the agreed ones. The runbook (Task 20) and `docs/architecture/hooks.md` carry the step; the SDK (Plan 3) automates it.

**Trade-offs**
Rotating a host to a new engine config is a host-admin `set_liquidation_engine`. Auctions opened under the old config stay settleable under it (their accounts are seeded by it). Tested by `rogue_config_with_the_same_salt_cannot_liquidate_the_bound_host` and `nobody_can_squat_the_admins_config_address`.

**Status**
Accepted (Plan 2, Task 6)

---

## ADR-030: Spel-Encodable Arguments and Optional-Account Slots

**Context**
spel-cli encodes only `bool`, `u8`, `u32`, `u64`, `u128`, `string`, `account_id`, `program_id`, byte arrays, some `Vec`s and `Option` (`spel-cli/src/serialize.rs:122-139`); no `u16`, no `i128` (ADR-018), no nested structs. LEZ needs every account at signing time and rejects duplicate account ids (`validated_state_diff/mod.rs:72-75`), so an "optional" account cannot be omitted or repeated.

**Decision**
Instruction arguments use only `bool`, `u8`, `u32`, `u64`, `u128`, `Vec<u32>`; bps travel as `u32` (range-checked to ≤ 10_000, band hi to ≤ 20_000); schedules as interleaved `(elapsed_ms, bps)` `u32` pairs, so a schedule point's elapsed time is at most `u32::MAX` ms (about 49.7 days), far beyond any schedule. The host program id is read from the `ProtocolParameters` account owner, not passed. Optional accounts (protocol-token reference oracle, protocol-token definition) occupy fixed slots; when not configured the caller passes the never-initialised PDA `["UNUSED", config, slot]`, and the engine asserts that exact id, so a caller can never substitute an account that changes a decision (e.g. force a freeze in `check_solvency`).

**Trade-offs**
Instruction structs are wide and flat. The slot convention must be followed by the SDK (Plan 3).

**Status**
Accepted (Plan 2, Task 6)
```

In `.github/workflows/core.yml` change the fmt line to `cargo fmt -p auction_core -p auction_engine_core -- --check`. Run `scripts/wsl-cargo.sh fmt -p auction_engine_core` first.

- [ ] **Step 10: Commit (controller — GPG signing on Windows)**

The implementer stages the files and reports; the controller commits from Git Bash.

```bash
git add Cargo.toml Cargo.lock programs/auction_engine/core docs/architecture/programs.md docs/design-log.md .github/workflows/core.yml
git commit -m "feat(engine): auction_engine_core (instruction enum, account states, PDAs, errors, args); ADR-027, ADR-030"
```

---

### Task 7: Integration-test crate and the host-side `World` harness

**Files:**
- Create: `programs/integration_tests/{Cargo.toml,src/lib.rs,tests/common/mod.rs,tests/harness.rs,tests/hooks.rs}`
- Modify: root `Cargo.toml` (`members` += `"programs/integration_tests"`), `.github/workflows/core.yml` (fmt += `-p engine_integration_tests`), `docs/architecture/programs.md`

**Interfaces:**
- Consumes: Task 3 (`token-methods`, `stablecoin-methods`, upstream `stablecoin_core`, `token_core`, `twap_oracle_core`, `clock_core`); Task 4 (`stablecoin_core::{Instruction::{SetLiquidationEngine, EngineFreezeNewDebt}, LiquidationEngineState, compute_liquidation_engine_state_pda}`).
- Produces: package `engine_integration_tests`; harness `programs/integration_tests/tests/common/mod.rs` with module `k` (key bytes), `key`, `id`, `host_program`, `token_program`, `collateral_def`, `oracle`, `ref_oracle`, `protocol_token_def`, `stable_def`, `protocol_parameters`, `accumulator`, `redemption_state`, `les`, `position`, `host_vault`, `assert_err`, constants `T0`, `ONE`, `R`, and `World` with `new` (token + stablecoin deployed, host bootstrapped by the real `InitializeProgram`), `deploy`, `ensure_signer`, `seed_clock`, `advance`, `set_oracle_full`, `set_oracle`, `set_redemption_price`, `submit`, `host`, `holding`, `fund_stable`, `fund_collateral`, `balance`, `supply`, `read`, `open_position`, `set_min_cr`, `bind_engine_to`. Task 8 appends the engine part; later tasks append further helpers.

- [ ] **Step 1: Write the failing harness tests**

`programs/integration_tests/Cargo.toml`:

`programs/integration_tests/Cargo.toml`:

```toml
[package]
name = "engine_integration_tests"
version = "0.1.0"
edition.workspace = true
license.workspace = true
publish = false

[dependencies]
lee = { workspace = true }
lee_core = { workspace = true, features = ["host", "test_utils"] }
clock_core = { git = "https://github.com/logos-blockchain/logos-execution-zone.git", tag = "v0.2.4" }
risc0-zkvm = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
stablecoin_core = { workspace = true }
token_core = { workspace = true }
twap_oracle_core = { workspace = true }
token-methods = { path = "../token/methods" }
stablecoin-methods = { path = "../stablecoin/methods" }
```

`programs/integration_tests/src/lib.rs`:

```rust
//! Engine integration tests live in tests/ (in-process V03State, risc0 dev mode).
#![forbid(unsafe_code)]
```

`programs/integration_tests/tests/harness.rs` (proves the harness drives the real host):

```rust
mod common;

use common::*;
use stablecoin_core::Position;

#[test]
fn world_bootstraps_the_host_and_opens_a_position() {
    let mut w = World::new();
    let pos = w.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 0, 500_000, 600_000);
    let p: Position = w.read(pos);
    assert_eq!((p.collateral_amount, p.normalized_debt_amount), (500_000, 600_000));
    assert_eq!(w.balance(id(k::OWNER_STABLE)), 600_000);
    assert_eq!(w.balance(host_vault(pos)), 500_000);
    w.set_min_cr(2 * ONE);
}
```

`programs/integration_tests/tests/hooks.rs` (the Task 4/5 guest entry points through real transactions):

```rust
mod common;

use common::*;
use lee_core::{account::AccountId, program::ProgramId};
use stablecoin_core::{LiquidationEngineState, ProtocolParameters};

const TEST_ENGINE: ProgramId = [0xE1; 8];

fn test_authority() -> AccountId {
    AccountId::new([0xEA; 32])
}

#[test]
fn host_admin_binds_engine_through_the_guest() {
    let mut w = World::new();
    w.bind_engine_to(TEST_ENGINE, test_authority()).expect("set_liquidation_engine");
    let s: LiquidationEngineState = w.read(les());
    assert_eq!((s.engine_program_id, s.engine_authority_id), (TEST_ENGINE, test_authority()));
}

#[test]
fn non_admin_bind_is_rejected_through_the_guest() {
    let mut w = World::new();
    let r = w.host(
        vec![id(k::ATTACKER), protocol_parameters(), les()],
        &[k::ATTACKER],
        stablecoin_core::Instruction::SetLiquidationEngine { engine_program_id: TEST_ENGINE, engine_authority_id: id(k::ATTACKER) },
    );
    assert_err(r, "Signer is not the protocol's admin");
}

#[test]
fn direct_engine_freeze_is_rejected_through_the_guest() {
    let mut w = World::new();
    w.bind_engine_to(TEST_ENGINE, test_authority()).unwrap();
    let r = w.host(
        vec![id(k::ATTACKER), les(), protocol_parameters()],
        &[k::ATTACKER],
        stablecoin_core::Instruction::EngineFreezeNewDebt,
    );
    assert_err(r, "Caller program is not the liquidation engine");
    let p: ProtocolParameters = w.read(protocol_parameters());
    assert!(!p.is_frozen);
}
```

Add `"programs/integration_tests"` to the root `members`.

- [ ] **Step 2: Run and see the failure**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p engine_integration_tests --no-run'`
Expected: compile error: file not found for module `common` (`tests/common/mod.rs` does not exist yet).

- [ ] **Step 3: Write the host-side harness `programs/integration_tests/tests/common/mod.rs`**

```rust
//! World harness: the real token and stablecoin guests (Task 8 adds the auction_engine) in an in-process V03State
//! (risc0 dev mode). Host bootstrap uses the real `InitializeProgram`; positions use the real
//! `OpenPosition` + `GenerateDebt`. Fixture accounts are force-inserted like the upstream tests.
#![allow(dead_code)]

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee::{
    error::LeeError,
    program_deployment_transaction::{self, ProgramDeploymentTransaction},
    public_transaction, PrivateKey, PublicKey, PublicTransaction, V03State,
};
use lee_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use serde::Serialize;
use stablecoin_core::{
    compute_liquidation_engine_state_pda, compute_position_pda, compute_position_vault_pda,
    compute_protocol_parameters_pda, compute_redemption_price_state_pda,
    compute_stability_fee_accumulator_pda, compute_stablecoin_definition_pda,
    compute_stablecoin_master_holding_pda, math::FIXED_POINT_ONE, RedemptionPriceState,
};
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::OraclePriceAccount;

pub const T0: u64 = 1_700_000_000_000;
pub const ONE: u128 = FIXED_POINT_ONE; // == auction_core::units::RAY
pub const R: u128 = FIXED_POINT_ONE / 2; // initial redemption price: 0.5 collateral per coin

/// Key bytes: `PrivateKey::try_new([n; 32])`. Holdings sign with their own key.
pub mod k {
    pub const OWNER: u8 = 41;
    pub const OWNER_COLL: u8 = 42;
    pub const OWNER_STABLE: u8 = 43;
    pub const HOST_ADMIN: u8 = 44;
    pub const FREEZE: u8 = 45;
    pub const ENGINE_ADMIN: u8 = 50;
    pub const KEEPER: u8 = 51;
    pub const KEEPER_COLL: u8 = 52;
    pub const BIDDER_STABLE: u8 = 53;
    pub const BIDDER_COLL: u8 = 54;
    pub const BIDDER2_STABLE: u8 = 55;
    pub const BIDDER2_COLL: u8 = 56;
    pub const TAKER_STABLE: u8 = 57;
    pub const TAKER_PROTOCOL: u8 = 58;
    pub const PROTOCOL_DEF: u8 = 59;
    pub const PROTOCOL_SEED_HOLDING: u8 = 60;
    pub const ATTACKER: u8 = 61;
    pub const CALLER: u8 = 62;
    pub const DONOR_COLL: u8 = 63;
    pub const DONOR_STABLE: u8 = 64;
    pub const OWNER2: u8 = 65;
    pub const OWNER2_COLL: u8 = 66;
    pub const OWNER2_STABLE: u8 = 67;
}

pub fn key(n: u8) -> PrivateKey {
    PrivateKey::try_new([n; 32]).expect("valid private key")
}
pub fn id(n: u8) -> AccountId {
    AccountId::from(&PublicKey::new_from_private_key(&key(n)))
}
pub fn host_program() -> ProgramId {
    stablecoin_methods::STABLECOIN_ID
}
pub fn token_program() -> ProgramId {
    token_methods::TOKEN_ID
}
pub fn collateral_def() -> AccountId {
    AccountId::new([5; 32])
}
pub fn oracle() -> AccountId {
    AccountId::new([0x70; 32])
}
pub fn ref_oracle() -> AccountId {
    AccountId::new([0x71; 32])
}
pub fn protocol_token_def() -> AccountId {
    id(k::PROTOCOL_DEF)
}
pub fn stable_def() -> AccountId {
    compute_stablecoin_definition_pda(host_program())
}
pub fn protocol_parameters() -> AccountId {
    compute_protocol_parameters_pda(host_program())
}
pub fn accumulator() -> AccountId {
    compute_stability_fee_accumulator_pda(host_program())
}
pub fn redemption_state() -> AccountId {
    compute_redemption_price_state_pda(host_program())
}
pub fn les() -> AccountId {
    compute_liquidation_engine_state_pda(host_program())
}
pub fn position(owner: u8, nonce: u64) -> AccountId {
    compute_position_pda(host_program(), id(owner), nonce)
}
pub fn host_vault(pos: AccountId) -> AccountId {
    compute_position_vault_pda(host_program(), pos)
}

pub fn assert_err(result: Result<(), LeeError>, needle: &str) {
    let text = format!("{:?}", result.expect_err("transaction was expected to fail"));
    assert!(text.contains(needle), "expected {needle:?} in: {text}");
}

pub struct World {
    pub state: V03State,
    pub now: u64,
    pub block: u64,
    /// Stablecoin force-minted into test holdings (supply invariant, Task 17).
    pub injected_stable: u128,
}

impl World {
    /// Host bootstrapped with the real `InitializeProgram`: stability fee exactly 1.0 per ms (debt stays
    /// exact), min CR 1.5, redemption price 0.5, oracle fresh at 0.5.
    pub fn new() -> Self {
        let mut w = Self { state: V03State::new(), now: T0, block: 1, injected_stable: 0 };
        w.seed_clock();
        for elf in [token_methods::TOKEN_ELF, stablecoin_methods::STABLECOIN_ELF] {
            w.deploy(elf);
        }
        for n in [k::HOST_ADMIN, k::FREEZE, k::OWNER, k::ENGINE_ADMIN, k::KEEPER, k::ATTACKER, k::CALLER] {
            w.ensure_signer(n);
        }
        w.state.force_insert_account(
            collateral_def(),
            Account {
                program_owner: token_program(),
                balance: 0,
                data: Data::from(&TokenDefinition::Fungible {
                    name: String::from("COLL"),
                    total_supply: 0,
                    metadata_id: None,
                    authority: None,
                }),
                nonce: Nonce(0),
            },
        );
        w.set_oracle(R, T0);
        let init = stablecoin_core::Instruction::InitializeProgram {
            freeze_authority_account_id: id(k::FREEZE),
            initial_stability_fee_per_millisecond: ONE,
            initial_controller_proportional_gain: 0,
            initial_controller_integral_gain: 0,
            initial_minimum_collateralization_ratio: ONE * 3 / 2,
            minimum_milliseconds_between_rate_updates: 300_000,
            maximum_oracle_price_age_milliseconds: 900_000,
            initial_redemption_price: R,
            stablecoin_name: String::from("LSC"),
        };
        w.host(
            vec![
                id(k::HOST_ADMIN),
                protocol_parameters(),
                accumulator(),
                redemption_state(),
                stable_def(),
                compute_stablecoin_master_holding_pda(host_program()),
                collateral_def(),
                oracle(),
                CLOCK_01_PROGRAM_ACCOUNT_ID,
            ],
            &[k::HOST_ADMIN],
            init,
        )
        .expect("initialize_program must succeed");
        w
    }

    pub fn deploy(&mut self, elf: &[u8]) {
        self.state
            .transition_from_program_deployment_transaction(&ProgramDeploymentTransaction::new(
                program_deployment_transaction::Message::new(elf.to_vec()),
            ))
            .expect("program deployment must succeed");
    }

    /// A signer owned by a placeholder non-default program, so it can sign many transactions
    /// (S1 problem 6; upstream fixtures do the same).
    pub fn ensure_signer(&mut self, n: u8) {
        if self.state.get_account_by_id(id(n)) == Account::default() {
            self.state
                .force_insert_account(id(n), Account { program_owner: [7u32; 8], ..Account::default() });
        }
    }

    pub fn seed_clock(&mut self) {
        let data = ClockAccountData { block_id: self.block, timestamp: self.now }.to_bytes();
        self.state.force_insert_account(
            CLOCK_01_PROGRAM_ACCOUNT_ID,
            Account {
                program_owner: [8u32; 8],
                data: Data::try_from(data).expect("clock data fits"),
                ..Account::default()
            },
        );
    }

    pub fn advance(&mut self, ms: u64) {
        self.now += ms;
        self.seed_clock();
    }

    pub fn set_oracle_full(&mut self, price: u128, timestamp: u64, confidence: u128) {
        self.state.force_insert_account(
            oracle(),
            Account {
                program_owner: [9u32; 8],
                balance: 0,
                data: Data::from(&OraclePriceAccount {
                    base_asset: stable_def(),
                    quote_asset: collateral_def(),
                    price,
                    timestamp,
                    source_id: oracle(),
                    confidence_interval: confidence,
                }),
                nonce: Nonce(0),
            },
        );
    }

    pub fn set_oracle(&mut self, price: u128, timestamp: u64) {
        self.set_oracle_full(price, timestamp, 0);
    }

    /// Re-anchors the host redemption price at `price` (rate 1.0) as of `now`.
    pub fn set_redemption_price(&mut self, price: u128) {
        let mut account = self.state.get_account_by_id(redemption_state());
        let mut s = RedemptionPriceState::try_from(&account.data).expect("redemption state");
        s.redemption_price_at_last_update = price;
        s.redemption_rate_per_millisecond = ONE;
        s.last_updated_at = self.now;
        account.data = Data::from(&s);
        self.state.force_insert_account(redemption_state(), account);
    }

    pub fn submit<I: Serialize>(
        &mut self,
        program: ProgramId,
        accounts: Vec<AccountId>,
        signers: &[u8],
        instruction: I,
    ) -> Result<(), LeeError> {
        self.seed_clock();
        let keys: Vec<PrivateKey> = signers.iter().map(|n| key(*n)).collect();
        let nonces = signers.iter().map(|n| self.state.get_account_by_id(id(*n)).nonce).collect();
        let message = public_transaction::Message::try_new(program, accounts, nonces, instruction)
            .expect("message is valid");
        let refs: Vec<&PrivateKey> = keys.iter().collect();
        let witness = public_transaction::WitnessSet::for_message(&message, &refs);
        let result = self
            .state
            .transition_from_public_transaction(&PublicTransaction::new(message, witness), self.block, self.now)
            .map(|_| ());
        if result.is_ok() {
            self.block += 1;
        }
        result
    }

    pub fn host<I: Serialize>(&mut self, accounts: Vec<AccountId>, signers: &[u8], ix: I) -> Result<(), LeeError> {
        self.submit(host_program(), accounts, signers, ix)
    }

    /// Writes holding `id(n)` of `definition` with `balance` (no supply change).
    pub fn holding(&mut self, n: u8, definition: AccountId, balance: u128) {
        self.state.force_insert_account(
            id(n),
            Account {
                program_owner: token_program(),
                balance: 0,
                data: Data::from(&TokenHolding::Fungible { definition_id: definition, balance }),
                nonce: Nonce(0),
            },
        );
    }

    fn add_supply(&mut self, definition: AccountId, amount: u128) {
        let mut account = self.state.get_account_by_id(definition);
        let next = match TokenDefinition::try_from(&account.data).expect("definition") {
            TokenDefinition::Fungible { name, total_supply, metadata_id, authority } => TokenDefinition::Fungible {
                name,
                total_supply: total_supply + amount,
                metadata_id,
                authority,
            },
            TokenDefinition::NonFungible { .. } => panic!("fungible definition expected"),
        };
        account.data = Data::from(&next);
        self.state.force_insert_account(definition, account);
    }

    /// Adds `amount` stablecoin to holding `id(n)` and to the stablecoin supply (books stay consistent).
    pub fn fund_stable(&mut self, n: u8, amount: u128) {
        let current = self.balance(id(n));
        self.holding(n, stable_def(), current + amount);
        self.add_supply(stable_def(), amount);
        self.injected_stable += amount;
    }

    pub fn fund_collateral(&mut self, n: u8, amount: u128) {
        let current = self.balance(id(n));
        self.holding(n, collateral_def(), current + amount);
        self.add_supply(collateral_def(), amount);
    }

    pub fn balance(&self, account: AccountId) -> u128 {
        match TokenHolding::try_from(&self.state.get_account_by_id(account).data) {
            Ok(TokenHolding::Fungible { balance, .. }) => balance,
            _ => 0,
        }
    }

    pub fn supply(&self, definition: AccountId) -> u128 {
        match TokenDefinition::try_from(&self.state.get_account_by_id(definition).data) {
            Ok(TokenDefinition::Fungible { total_supply, .. }) => total_supply,
            _ => 0,
        }
    }

    pub fn read<T: for<'a> TryFrom<&'a Data>>(&self, account: AccountId) -> T {
        T::try_from(&self.state.get_account_by_id(account).data)
            .unwrap_or_else(|_| panic!("account {account:?} does not decode"))
    }

    /// Real `OpenPosition` then `GenerateDebt` (debt minted to `stable_holding`). Returns the position id.
    pub fn open_position(
        &mut self,
        owner: u8,
        coll_holding: u8,
        stable_holding: u8,
        nonce: u64,
        collateral: u128,
        debt: u128,
    ) -> AccountId {
        self.ensure_signer(owner);
        self.fund_collateral(coll_holding, collateral);
        if self.state.get_account_by_id(id(stable_holding)) == Account::default() {
            self.holding(stable_holding, stable_def(), 0);
        }
        let pos = position(owner, nonce);
        self.host(
            vec![id(owner), pos, host_vault(pos), id(coll_holding), collateral_def(), protocol_parameters(), CLOCK_01_PROGRAM_ACCOUNT_ID],
            &[owner, coll_holding],
            stablecoin_core::Instruction::OpenPosition { position_nonce: nonce, initial_collateral_amount: collateral },
        )
        .expect("open_position must succeed");
        if debt > 0 {
            self.host(
                vec![
                    id(owner),
                    pos,
                    stable_def(),
                    id(stable_holding),
                    accumulator(),
                    redemption_state(),
                    oracle(),
                    protocol_parameters(),
                    CLOCK_01_PROGRAM_ACCOUNT_ID,
                ],
                &[owner],
                stablecoin_core::Instruction::GenerateDebt { amount: debt },
            )
            .expect("generate_debt must succeed");
        }
        pos
    }

    /// Host admin retunes the minimum collateralisation ratio (used to make positions unsafe).
    pub fn set_min_cr(&mut self, ratio: u128) {
        self.host(
            vec![id(k::HOST_ADMIN), protocol_parameters()],
            &[k::HOST_ADMIN],
            stablecoin_core::Instruction::SetMinimumCollateralizationRatio { new_ratio: ratio },
        )
        .expect("set_minimum_collateralization_ratio must succeed");
    }


    /// Host admin binds an engine (program id, authority) through the real guest (ADR-026).
    pub fn bind_engine_to(&mut self, engine_program_id: ProgramId, engine_authority_id: AccountId) -> Result<(), LeeError> {
        self.host(
            vec![id(k::HOST_ADMIN), protocol_parameters(), les()],
            &[k::HOST_ADMIN],
            stablecoin_core::Instruction::SetLiquidationEngine { engine_program_id, engine_authority_id },
        )
    }
}
```

- [ ] **Step 4: Run the harness and hook tests**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p engine_integration_tests 2>&1 | grep -E "test result|FAILED|panicked"'`
Expected: `harness` 1 passed, `hooks` 3 passed.

- [ ] **Step 5: Docs and CI fmt scope**

Append to `docs/architecture/programs.md`:

```markdown
## Test harness

`programs/integration_tests` (package `engine_integration_tests`) runs the real token, stablecoin and (from Task 8) auction_engine guests in an in-process `V03State` (risc0 dev mode). `World::new` bootstraps the host with the real `InitializeProgram` (stability fee exactly 1.0 per ms so debt stays exact, min CR 1.5, redemption price 0.5); positions use the real `OpenPosition` + `GenerateDebt`; fixture accounts are force-inserted like the upstream tests; signers are owned by a placeholder program so they can sign repeatedly (S1 problem 6).
```

Change the CI fmt line to `cargo fmt -p auction_core -p auction_engine_core -p engine_integration_tests -- --check` (run `scripts/wsl-cargo.sh fmt -p engine_integration_tests` first).

- [ ] **Step 6: Commit (controller — GPG signing on Windows)**

The implementer stages the files and reports; the controller commits from Git Bash.

```bash
git add Cargo.toml Cargo.lock programs/integration_tests docs/architecture/programs.md .github/workflows/core.yml
git commit -m "test: integration-test crate and host-side World harness"
```

---

### Task 8: Engine program skeleton, guest, `init_config` / `set_engine_params`

**Files:**
- Create: `programs/auction_engine/Cargo.toml`, `programs/auction_engine/src/{lib.rs,accounts.rs,token_calls.rs,admin.rs}`
- Create: `programs/auction_engine/methods/{Cargo.toml,build.rs,src/lib.rs}`, `programs/auction_engine/methods/guest/{Cargo.toml,Cargo.lock,src/bin/auction_engine.rs}`
- Create: `programs/integration_tests/tests/{config.rs,idl.rs}`, `idl/auction_engine-idl.json`
- Modify: `programs/integration_tests/Cargo.toml` (+ `auction_core`, `auction_engine_core`, `auction-engine-methods`), `programs/integration_tests/tests/common/mod.rs` (deploy the engine; append the engine part), root `Cargo.toml` (`members` += `"programs/auction_engine"`, `"programs/auction_engine/methods"`; `exclude` += `"programs/auction_engine/methods/guest"`), `.github/workflows/core.yml` (fmt += `-p auction_engine_program`), `docs/architecture/programs.md`

**Interfaces:**
- Consumes: Task 6 (`auction_engine_core::{pda::*, state::*, args::{EngineParamsArgs, CollateralArgs}, EngineError, Instruction}`); Task 7 (harness: `World`, `k`, `id`, host id helpers, `assert_err`, `World::bind_engine_to`); upstream `stablecoin_core::{ProtocolParameters, compute_protocol_parameters_pda, …}`, `token_core::{Instruction, TokenHolding, TokenDefinition}`, `clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID}`.
- Produces:
  - `auction_engine_program::EngineResult = Result<(Vec<AccountPostState>, Vec<ChainedCall>), EngineError>`.
  - `accounts`: `ZERO_ID`, `nonzero_id`, `nonzero_program`, `read_clock`, `require_admin`, `load_config`, `load_coll`, `load_accounting`, `load_auction`, `fungible_balance`, `check_recipient`, `expect_id`, `expect_default`, `optional_slot`, `check_distinct_slots`, `expect_slot`, `echo`, `write`, `create`, `write_or_create` (Step 4).
  - `token_calls`: `authorized`, `with_balance`, `post_init`, `transfer`, `burn`, `mint_with_authority`, `initialize_account` (Step 4).
  - `admin::{InitConfigAccounts, SetEngineParamsAccounts, init_config, set_engine_params}`.
  - `auction_engine_methods::{AUCTION_ENGINE_ELF, AUCTION_ENGINE_ID}`.
  - Guest helpers `to_spel(EngineError) -> SpelError`, `finish(EngineResult) -> SpelResult`; guest functions `init_config` (IDL 0), `set_engine_params` (IDL 1).
  - Harness engine part: `SALT`, `engine_program`, `config` (= `["CONFIG", id(k::ENGINE_ADMIN), SALT]`), `coll_config`, `accounting`, `buffer`, `engine_authority`, `mint_authority`, `unused`, `record`, `auction`, `auction_vault`, `init_config_ix`, `set_engine_params_ix`, `slot_account`, `World::{engine, init_engine_config, set_engine_params, bind_engine}`.

- [ ] **Step 1: Create the crates and write the failing integration tests**

`programs/auction_engine/Cargo.toml`:

```toml
[package]
name = "auction_engine_program"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[dependencies]
auction_core = { path = "../../crates/auction_core" }
auction_engine_core = { path = "core" }
lee_core = { git = "https://github.com/logos-blockchain/logos-execution-zone.git", tag = "v0.2.4", features = ["host"] }
clock_core = { git = "https://github.com/logos-blockchain/logos-execution-zone.git", tag = "v0.2.4" }
stablecoin_core = { path = "../stablecoin/core" }
token_core = { path = "../token/core" }
twap_oracle_core = { path = "../twap_oracle/core" }
alloy-primitives = { version = "1", default-features = false }
borsh = { version = "1.5", features = ["derive"] }
```

`programs/auction_engine/methods/Cargo.toml`, `build.rs`, `src/lib.rs` (same shape as `programs/stablecoin/methods`):

```toml
[package]
name = "auction-engine-methods"
version = "0.1.0"
edition = "2021"

[build-dependencies]
risc0-build = "=3.0.5"

[dependencies]
risc0-zkvm = { version = "=3.0.5", features = ["std"] }

[package.metadata.risc0]
methods = ["guest"]
```

```rust
//! Build script that embeds the auction_engine RISC Zero guest ELF.
fn main() {
    risc0_build::embed_methods();
}
```

```rust
//! Host-side embedding of the auction_engine guest: `AUCTION_ENGINE_ELF`, `AUCTION_ENGINE_ID`.
include!(concat!(env!("OUT_DIR"), "/methods.rs"));
```

`programs/auction_engine/methods/guest/Cargo.toml`:

```toml
[package]
name = "auction-engine-guest"
version = "0.1.0"
edition = "2021"

[workspace]

[profile.release]
debug = 0
strip = "symbols"

[[bin]]
name = "auction_engine"
path = "src/bin/auction_engine.rs"

[dependencies]
spel-framework = { git = "https://github.com/logos-co/spel.git", rev = "1ef0500f8fc8ce3ddf95523726ae159db83f744f", package = "spel-framework" }
nssa_core = { git = "https://github.com/logos-blockchain/logos-execution-zone.git", tag = "v0.2.4", package = "lee_core" }
risc0-zkvm = { version = "=3.0.5", default-features = false }
auction_engine_program = { path = "../.." }
auction_engine_core = { path = "../../core" }
serde = { version = "1.0", features = ["derive"] }
borsh = "1.5"
```

Add to `programs/integration_tests/Cargo.toml` `[dependencies]`:

```toml
auction_core = { workspace = true }
auction_engine_core = { workspace = true }
auction-engine-methods = { path = "../auction_engine/methods" }
```

In `programs/integration_tests/tests/common/mod.rs`, `World::new`, replace `for elf in [token_methods::TOKEN_ELF, stablecoin_methods::STABLECOIN_ELF] {` with:

```rust
        for elf in [
            token_methods::TOKEN_ELF,
            stablecoin_methods::STABLECOIN_ELF,
            auction_engine_methods::AUCTION_ENGINE_ELF,
        ] {
```

and append the engine part:

```rust
// ---- engine part (Task 8) ----
use auction_engine_core::{
    args::EngineParamsArgs,
    pda::{self, SLOT_PROTOCOL_TOKEN, SLOT_REF_ORACLE},
    Instruction as Ix,
};

pub const SALT: u64 = 1;

pub fn engine_program() -> ProgramId {
    auction_engine_methods::AUCTION_ENGINE_ID
}
pub fn config() -> AccountId {
    pda::config_pda(&engine_program(), &id(k::ENGINE_ADMIN), SALT)
}
pub fn coll_config() -> AccountId {
    pda::coll_config_pda(&engine_program(), &config(), &collateral_def())
}
pub fn accounting() -> AccountId {
    pda::accounting_pda(&engine_program(), &config())
}
pub fn buffer() -> AccountId {
    pda::buffer_pda(&engine_program(), &config())
}
pub fn engine_authority() -> AccountId {
    pda::engine_authority_pda(&engine_program(), &config())
}
pub fn mint_authority() -> AccountId {
    pda::mint_authority_pda(&engine_program(), &config())
}
pub fn unused(slot: u64) -> AccountId {
    pda::unused_slot_pda(&engine_program(), &config(), slot)
}
pub fn record(pos: AccountId) -> AccountId {
    pda::liqrec_pda(&engine_program(), &config(), &pos)
}
pub fn auction(pos: AccountId, nonce: u64) -> AccountId {
    pda::auction_pda(&engine_program(), &config(), &pos, nonce)
}
pub fn auction_vault(pos: AccountId, nonce: u64) -> AccountId {
    pda::vault_pda(&engine_program(), &auction(pos, nonce))
}

pub fn init_config_ix(salt: u64, p: &EngineParamsArgs) -> Ix {
    Ix::InitConfig {
        salt,
        global_cap: p.global_cap,
        freeze_on_bad_debt: p.freeze_on_bad_debt,
        bad_debt_freeze_threshold: p.bad_debt_freeze_threshold,
        protocol_token_enabled: p.protocol_token_enabled,
        protocol_token_ref_price: p.protocol_token_ref_price,
        ref_oracle_enabled: p.ref_oracle_enabled,
        ref_oracle_max_age_ms: p.ref_oracle_max_age_ms,
        surplus_buffer_target: p.surplus_buffer_target,
        surplus_lot: p.surplus_lot,
        surplus_delay_ms: p.surplus_delay_ms,
        surplus_premium_bps: p.surplus_premium_bps,
        surplus_max_discount_bps: p.surplus_max_discount_bps,
        debt_lot: p.debt_lot,
        debt_initial_discount_bps: p.debt_initial_discount_bps,
        debt_growth: p.debt_growth.clone(),
        surplus_decay: p.surplus_decay.clone(),
        max_mint_per_auction: p.max_mint_per_auction,
        min_take: p.min_take,
        system_ttl_ms: p.system_ttl_ms,
    }
}

pub fn set_engine_params_ix(p: &EngineParamsArgs) -> Ix {
    Ix::SetEngineParams {
        global_cap: p.global_cap,
        freeze_on_bad_debt: p.freeze_on_bad_debt,
        bad_debt_freeze_threshold: p.bad_debt_freeze_threshold,
        protocol_token_enabled: p.protocol_token_enabled,
        protocol_token_ref_price: p.protocol_token_ref_price,
        ref_oracle_enabled: p.ref_oracle_enabled,
        ref_oracle_max_age_ms: p.ref_oracle_max_age_ms,
        surplus_buffer_target: p.surplus_buffer_target,
        surplus_lot: p.surplus_lot,
        surplus_delay_ms: p.surplus_delay_ms,
        surplus_premium_bps: p.surplus_premium_bps,
        surplus_max_discount_bps: p.surplus_max_discount_bps,
        debt_lot: p.debt_lot,
        debt_initial_discount_bps: p.debt_initial_discount_bps,
        debt_growth: p.debt_growth.clone(),
        surplus_decay: p.surplus_decay.clone(),
        max_mint_per_auction: p.max_mint_per_auction,
        min_take: p.min_take,
        system_ttl_ms: p.system_ttl_ms,
    }
}

pub fn slot_account(slot: u64, enabled: bool) -> AccountId {
    match (slot, enabled) {
        (SLOT_PROTOCOL_TOKEN, true) => protocol_token_def(),
        (SLOT_REF_ORACLE, true) => ref_oracle(),
        _ => unused(slot),
    }
}

impl World {
    pub fn engine(&mut self, accounts: Vec<AccountId>, signers: &[u8], ix: Ix) -> Result<(), LeeError> {
        self.submit(engine_program(), accounts, signers, ix)
    }

    pub fn init_engine_config(&mut self, p: &EngineParamsArgs) -> Result<(), LeeError> {
        self.engine(
            vec![
                id(k::ENGINE_ADMIN),
                config(),
                accounting(),
                buffer(),
                protocol_parameters(),
                stable_def(),
                slot_account(SLOT_PROTOCOL_TOKEN, p.protocol_token_enabled),
                slot_account(SLOT_REF_ORACLE, p.ref_oracle_enabled),
            ],
            &[k::ENGINE_ADMIN],
            init_config_ix(SALT, p),
        )
    }

    pub fn set_engine_params(&mut self, signer: u8, p: &EngineParamsArgs) -> Result<(), LeeError> {
        self.engine(
            vec![
                id(signer),
                config(),
                accounting(),
                coll_config(),
                slot_account(SLOT_PROTOCOL_TOKEN, p.protocol_token_enabled),
                slot_account(SLOT_REF_ORACLE, p.ref_oracle_enabled),
            ],
            &[signer],
            set_engine_params_ix(p),
        )
    }

    /// Host admin binds `config()`'s engine authority (ADR-026/027).
    pub fn bind_engine(&mut self) -> Result<(), LeeError> {
        self.bind_engine_to(engine_program(), engine_authority())
    }
}
```

`programs/integration_tests/tests/config.rs`:

```rust
mod common;

use auction_engine_core::{args::EngineParamsArgs, pda, state::*};
use common::*;
use lee_core::account::AccountId;
use token_core::TokenHolding;

#[test]
fn init_config_creates_config_accounting_and_buffer() {
    let mut w = World::new();
    w.init_engine_config(&EngineParamsArgs::defaults_for_tests()).expect("init_config");
    let cfg: EngineConfigState = w.read(config());
    assert_eq!(cfg.host_program_id, host_program());
    assert_eq!(cfg.host_protocol_parameters_id, protocol_parameters());
    assert_eq!(cfg.stablecoin_definition_id, stable_def());
    assert_eq!(cfg.collateral_definition_id, collateral_def());
    assert_eq!(cfg.token_program_id, token_program());
    assert_eq!(cfg.admin, id(k::ENGINE_ADMIN));
    assert_eq!((cfg.global_reserved, cfg.protocol_token, cfg.protocol_token_oracle_id), (0, None, None));
    let acc: AccountingState = w.read(accounting());
    assert_eq!(acc, AccountingState::new());
    let buffer_holding: TokenHolding = w.read(buffer());
    assert_eq!(buffer_holding, TokenHolding::Fungible { definition_id: stable_def(), balance: 0 });
    assert_eq!(w.state.get_account_by_id(buffer()).program_owner, token_program());
}

#[test]
fn init_config_twice_is_rejected() {
    let mut w = World::new();
    w.init_engine_config(&EngineParamsArgs::defaults_for_tests()).unwrap();
    assert_err(w.init_engine_config(&EngineParamsArgs::defaults_for_tests()), "AccountAlreadyInitialized");
}

#[test]
fn zero_ref_price_is_rejected() {
    let mut w = World::new();
    let p = EngineParamsArgs { protocol_token_ref_price: 0, ..EngineParamsArgs::defaults_for_tests() };
    assert_err(w.init_engine_config(&p), "ZeroRefPrice");
}

#[test]
fn threshold_below_debt_lot_is_rejected() {
    let mut w = World::new();
    let p = EngineParamsArgs { bad_debt_freeze_threshold: 499, ..EngineParamsArgs::defaults_for_tests() };
    assert_err(w.init_engine_config(&p), "ThresholdBelowDebtLot");
}

#[test]
fn invalid_system_params_are_rejected() {
    let mut w = World::new();
    let p = EngineParamsArgs { min_take: 501, ..EngineParamsArgs::defaults_for_tests() };
    assert_err(w.init_engine_config(&p), "InvalidConfig");
}

#[test]
fn wrong_host_parameters_account_is_rejected() {
    let mut w = World::new();
    let p = EngineParamsArgs::defaults_for_tests();
    let r = w.engine(
        vec![id(k::ENGINE_ADMIN), config(), accounting(), buffer(), accumulator(), stable_def(), unused(1), unused(0)],
        &[k::ENGINE_ADMIN],
        init_config_ix(SALT, &p),
    );
    assert_err(r, "HostMismatch");
}

#[test]
fn disabled_slot_must_be_the_placeholder() {
    let mut w = World::new();
    let p = EngineParamsArgs::defaults_for_tests();
    let r = w.engine(
        vec![id(k::ENGINE_ADMIN), config(), accounting(), buffer(), protocol_parameters(), stable_def(), id(k::ATTACKER), unused(0)],
        &[k::ENGINE_ADMIN],
        init_config_ix(SALT, &p),
    );
    assert_err(r, "WrongAccount");
}

#[test]
fn zero_protocol_token_id_is_rejected() {
    let mut w = World::new();
    let p = EngineParamsArgs { protocol_token_enabled: true, ..EngineParamsArgs::defaults_for_tests() };
    let r = w.engine(
        vec![id(k::ENGINE_ADMIN), config(), accounting(), buffer(), protocol_parameters(), stable_def(), AccountId::new([0; 32]), unused(0)],
        &[k::ENGINE_ADMIN],
        init_config_ix(SALT, &p),
    );
    assert_err(r, "ZeroId");
}

#[test]
fn set_engine_params_admin_only_and_keeps_binding() {
    let mut w = World::new();
    w.init_engine_config(&EngineParamsArgs::defaults_for_tests()).unwrap();
    let p = EngineParamsArgs { global_cap: 20_000_000, ..EngineParamsArgs::defaults_for_tests() };
    assert_err(w.set_engine_params(k::ATTACKER, &p), "NotAdmin");
    w.set_engine_params(k::ENGINE_ADMIN, &p).expect("admin updates params");
    let cfg: EngineConfigState = w.read(config());
    assert_eq!(cfg.global_cap, 20_000_000);
    assert_eq!(cfg.host_program_id, host_program());
}

#[test]
fn ref_price_steps_are_bounded_to_half_and_double() {
    // I5 / ADR-031: each admin change of the reference price stays within [old/2, 2·old].
    let mut w = World::new();
    w.init_engine_config(&EngineParamsArgs::defaults_for_tests()).unwrap();
    let at = |price: u128| EngineParamsArgs { protocol_token_ref_price: price, ..EngineParamsArgs::defaults_for_tests() };
    assert_err(w.set_engine_params(k::ENGINE_ADMIN, &at(4 * ONE + 1)), "RefPriceStepTooLarge");
    w.set_engine_params(k::ENGINE_ADMIN, &at(4 * ONE)).expect("exactly 2x is allowed");
    assert_err(w.set_engine_params(k::ENGINE_ADMIN, &at(2 * ONE - 1)), "RefPriceStepTooLarge");
    w.set_engine_params(k::ENGINE_ADMIN, &at(2 * ONE)).expect("exactly 1/2 is allowed");
}

#[test]
fn protocol_token_must_differ_from_the_collateral() {
    // M7: protocol token ∉ {stablecoin, collateral, reference oracle}.
    let mut w = World::new();
    let p = EngineParamsArgs { protocol_token_enabled: true, ..EngineParamsArgs::defaults_for_tests() };
    let r = w.engine(
        vec![id(k::ENGINE_ADMIN), config(), accounting(), buffer(), protocol_parameters(), stable_def(), collateral_def(), unused(0)],
        &[k::ENGINE_ADMIN],
        init_config_ix(SALT, &p),
    );
    assert_err(r, "WrongAccount");
}

#[test]
fn nobody_can_squat_the_admins_config_address() {
    // I7 / ADR-027: the config PDA includes its admin, so another signer cannot create it first.
    let mut w = World::new();
    let e = engine_program();
    let r = w.engine(
        vec![id(k::ATTACKER), config(), accounting(), buffer(), protocol_parameters(), stable_def(), unused(1), unused(0)],
        &[k::ATTACKER],
        init_config_ix(SALT, &EngineParamsArgs::defaults_for_tests()),
    );
    assert_err(r, "PdaMismatch");
    assert_ne!(pda::config_pda(&e, &id(k::ATTACKER), SALT), config());
}
```

`programs/integration_tests/tests/idl.rs`:

`programs/integration_tests/tests/idl.rs`:

```rust
//! Spec §3.2/§5.10: IDL position == enum index for every engine instruction (Shared Reference R2).

const R2: [&str; 17] = [
    "init_config", "set_engine_params", "set_collateral_config", "observe_feed", "liquidate", "bid",
    "bid_with_settlement", "settle_empty", "claim_leftover", "settle_debt", "start_surplus_auction",
    "take_surplus", "close_surplus_auction", "start_debt_auction", "take_debt", "close_debt_auction",
    "check_solvency",
];

fn idl_names() -> Vec<String> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../idl/auction_engine-idl.json");
    let idl: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("engine IDL")).expect("IDL json");
    idl["instructions"].as_array().expect("array").iter().map(|i| i["name"].as_str().expect("name").to_owned()).collect()
}

#[test]
fn engine_idl_is_a_prefix_of_the_reference_order() {
    let names = idl_names();
    assert!(!names.is_empty() && names.len() <= R2.len());
    for (i, name) in names.iter().enumerate() {
        assert_eq!(name, R2[i], "IDL position {i}");
    }
}
```

Add the two new members and the guest exclude to the root `Cargo.toml`.

- [ ] **Step 2: Run and see the failure**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p engine_integration_tests --no-run'`
Expected: build error: `programs/auction_engine/src/lib.rs` and the guest source do not exist yet.

- [ ] **Step 3: Note on the config PDA (ADR-027, I7)**

`init_config` derives `config` from `["CONFIG", admin, salt]`; `load_config` re-derives it from the stored `admin` and `salt`. The admin is therefore immutable (no rotation instruction exists in the PoC; a new admin means a new config and a host re-bind).

- [ ] **Step 4: Write `lib.rs` and the account helpers**

`programs/auction_engine/src/lib.rs`:

```rust
//! Host-testable handlers of the `auction_engine` program: a thin layer over `auction_core`.
//! Handlers validate accounts, call the core, and return post-states (one per input account,
//! in order) plus chained calls. Checks → effects → chained calls.
#![forbid(unsafe_code)]

pub mod accounts;
pub mod admin;
pub mod token_calls;

use auction_engine_core::EngineError;
use lee_core::program::{AccountPostState, ChainedCall};

pub type EngineResult = Result<(Vec<AccountPostState>, Vec<ChainedCall>), EngineError>;
```

`programs/auction_engine/src/accounts.rs`:

```rust
//! Account loading and validation shared by every handler. Every engine account is checked for
//! owner, tag and PDA; every holding for token program and definition (spec §10 checklist).

use auction_engine_core::{pda, state::*, EngineError};
use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, Claim, PdaSeed, ProgramId, DEFAULT_PROGRAM_ID},
};
use token_core::TokenHolding;

pub const ZERO_ID: AccountId = AccountId::new([0; 32]);

pub fn nonzero_id(id: &AccountId) -> Result<(), EngineError> {
    if *id == ZERO_ID { Err(EngineError::ZeroId) } else { Ok(()) }
}

pub fn nonzero_program(p: &ProgramId) -> Result<(), EngineError> {
    if *p == DEFAULT_PROGRAM_ID { Err(EngineError::ZeroId) } else { Ok(()) }
}

/// Time comes only from `CLOCK_01` (spec §3.2).
pub fn read_clock(clock: &AccountWithMetadata) -> Result<u64, EngineError> {
    if clock.account_id != CLOCK_01_PROGRAM_ACCOUNT_ID || clock.account == Account::default() {
        return Err(EngineError::WrongClock);
    }
    borsh::from_slice::<ClockAccountData>(clock.account.data.as_ref())
        .map(|c| c.timestamp)
        .map_err(|_| EngineError::WrongClock)
}

pub fn require_admin(admin: &AccountWithMetadata, cfg: &EngineConfigState) -> Result<(), EngineError> {
    if admin.is_authorized && admin.account_id == cfg.admin { Ok(()) } else { Err(EngineError::NotAdmin) }
}

pub fn expect_id(acc: &AccountWithMetadata, expected: &AccountId, what: &'static str) -> Result<(), EngineError> {
    if acc.account_id == *expected { Ok(()) } else { Err(EngineError::WrongAccount(what)) }
}

pub fn expect_default(acc: &AccountWithMetadata, what: &'static str) -> Result<(), EngineError> {
    if acc.account == Account::default() { Ok(()) } else { Err(EngineError::WrongAccount(what)) }
}

fn owned<'a>(acc: &'a AccountWithMetadata, program_id: &ProgramId, what: &'static str) -> Result<&'a Data, EngineError> {
    if acc.account.program_owner == *program_id { Ok(&acc.account.data) } else { Err(EngineError::WrongAccount(what)) }
}

pub fn load_config(program_id: &ProgramId, acc: &AccountWithMetadata) -> Result<EngineConfigState, EngineError> {
    let cfg = EngineConfigState::try_from(owned(acc, program_id, "config")?)
        .map_err(|_| EngineError::WrongAccount("config"))?;
    expect_id(acc, &pda::config_pda(program_id, &cfg.admin, cfg.salt), "config")?;
    Ok(cfg)
}

pub fn load_coll(
    program_id: &ProgramId,
    config_id: &AccountId,
    cfg: &EngineConfigState,
    acc: &AccountWithMetadata,
) -> Result<CollateralConfigState, EngineError> {
    let coll = CollateralConfigState::try_from(owned(acc, program_id, "coll_config")?)
        .map_err(|_| EngineError::WrongAccount("coll_config"))?;
    expect_id(acc, &pda::coll_config_pda(program_id, config_id, &cfg.collateral_definition_id), "coll_config")?;
    if coll.config != *config_id {
        return Err(EngineError::WrongAccount("coll_config"));
    }
    Ok(coll)
}

pub fn load_accounting(program_id: &ProgramId, config_id: &AccountId, acc: &AccountWithMetadata) -> Result<AccountingState, EngineError> {
    let a = AccountingState::try_from(owned(acc, program_id, "accounting")?)
        .map_err(|_| EngineError::WrongAccount("accounting"))?;
    expect_id(acc, &pda::accounting_pda(program_id, config_id), "accounting")?;
    Ok(a)
}

pub fn load_auction(program_id: &ProgramId, config_id: &AccountId, acc: &AccountWithMetadata) -> Result<AuctionState, EngineError> {
    let a = AuctionState::try_from(owned(acc, program_id, "auction")?)
        .map_err(|_| EngineError::WrongAccount("auction"))?;
    if a.config != *config_id {
        return Err(EngineError::WrongAccount("auction"));
    }
    expect_id(acc, &pda::auction_pda(program_id, config_id, &a.position, a.nonce), "auction")?;
    Ok(a)
}

/// Balance of an initialised fungible holding of `definition` owned by `token_program`.
pub fn fungible_balance(acc: &AccountWithMetadata, token_program: &ProgramId, definition: &AccountId) -> Result<u128, EngineError> {
    if acc.account == Account::default() || acc.account.program_owner != *token_program {
        return Err(EngineError::WrongDefinition);
    }
    match TokenHolding::try_from(&acc.account.data) {
        Ok(TokenHolding::Fungible { definition_id, balance }) if definition_id == *definition => Ok(balance),
        _ => Err(EngineError::WrongDefinition),
    }
}

/// A holding that will receive tokens: initialised with `definition`, or fresh and signed by its
/// holder (the token program claims a fresh recipient with `Claim::Authorized`). Review Focus 2.
pub fn check_recipient(acc: &AccountWithMetadata, token_program: &ProgramId, definition: &AccountId) -> Result<(), EngineError> {
    if acc.account == Account::default() {
        return if acc.is_authorized { Ok(()) } else { Err(EngineError::RecipientMustSign) };
    }
    fungible_balance(acc, token_program, definition).map(|_| ())
}

/// Enabled → the slot holds the configured account (non-zero, not the placeholder); disabled → the
/// slot must be the placeholder `["UNUSED", config, slot]` (ADR-030).
pub fn optional_slot(
    program_id: &ProgramId,
    config_id: &AccountId,
    slot: u64,
    enabled: bool,
    acc: &AccountWithMetadata,
) -> Result<Option<AccountId>, EngineError> {
    let placeholder = pda::unused_slot_pda(program_id, config_id, slot);
    if enabled {
        nonzero_id(&acc.account_id)?;
        if acc.account_id == placeholder {
            return Err(EngineError::WrongAccount("slot"));
        }
        Ok(Some(acc.account_id))
    } else {
        expect_id(acc, &placeholder, "unused slot")?;
        Ok(None)
    }
}

/// The protocol token must not be the stablecoin, the collateral or the reference oracle, and the
/// reference oracle must not be the stablecoin or the collateral (M7).
pub fn check_distinct_slots(
    protocol_token: Option<AccountId>,
    ref_oracle: Option<AccountId>,
    stablecoin: &AccountId,
    collateral: &AccountId,
) -> Result<(), EngineError> {
    if let Some(t) = protocol_token {
        if t == *stablecoin || t == *collateral || Some(t) == ref_oracle {
            return Err(EngineError::WrongAccount("protocol_token"));
        }
    }
    if let Some(o) = ref_oracle {
        if o == *stablecoin || o == *collateral {
            return Err(EngineError::WrongAccount("ref_oracle"));
        }
    }
    Ok(())
}

/// At use time: the slot account must be exactly the configured one, or the placeholder.
pub fn expect_slot(
    program_id: &ProgramId,
    config_id: &AccountId,
    slot: u64,
    configured: Option<AccountId>,
    acc: &AccountWithMetadata,
) -> Result<(), EngineError> {
    match configured {
        Some(id) => expect_id(acc, &id, "slot"),
        None => expect_id(acc, &pda::unused_slot_pda(program_id, config_id, slot), "unused slot"),
    }
}

#[must_use]
pub fn echo(acc: &AccountWithMetadata) -> AccountPostState {
    AccountPostState::new(acc.account.clone())
}

pub fn write<T>(acc: &AccountWithMetadata, state: &T) -> AccountPostState
where
    Data: for<'a> From<&'a T>,
{
    let mut a = acc.account.clone();
    a.data = Data::from(state);
    AccountPostState::new(a)
}

pub fn create<T>(acc: &AccountWithMetadata, state: &T, seed: PdaSeed) -> AccountPostState
where
    Data: for<'a> From<&'a T>,
{
    let mut a = acc.account.clone();
    a.data = Data::from(state);
    AccountPostState::new_claimed(a, Claim::Pda(seed))
}

pub fn write_or_create<T>(acc: &AccountWithMetadata, state: &T, seed: PdaSeed) -> AccountPostState
where
    Data: for<'a> From<&'a T>,
{
    let mut a = acc.account.clone();
    a.data = Data::from(state);
    AccountPostState::new_claimed_if_default(a, Claim::Pda(seed))
}
```

`programs/auction_engine/src/token_calls.rs`:

```rust
//! Chained token calls. lee requires each chained call's pre-states to equal the CURRENT state
//! (`InconsistentAccountPreState`) and each `is_authorized` flag to equal "signer, or a PDA whose seed
//! this call attaches" (`InvalidAccountAuthorization` / `AuthorizedAccountMarkedAsNotAuthorized`).
//! So a later call carries the simulated post-state of an earlier one (`with_balance`, `post_init`),
//! and a PDA is flagged authorised only in the call that attaches its seed.

use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{ChainedCall, PdaSeed, ProgramId},
};
use token_core::{Instruction as TokenIx, TokenHolding};

#[must_use]
pub fn authorized(acc: &AccountWithMetadata) -> AccountWithMetadata {
    let mut a = acc.clone();
    a.is_authorized = true;
    a
}

/// The holding as the token program will have written it with `balance` (same borsh encoding).
#[must_use]
pub fn with_balance(acc: &AccountWithMetadata, definition: AccountId, balance: u128) -> AccountWithMetadata {
    let mut a = acc.clone();
    a.account.data = Data::from(&TokenHolding::Fungible { definition_id: definition, balance });
    a
}

/// A fresh account right after `Token::InitializeAccount` (like `open_position`'s `post_init_vault`).
#[must_use]
pub fn post_init(acc: &AccountWithMetadata, token_program: ProgramId, definition: AccountId) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: token_program,
            balance: 0,
            data: Data::from(&TokenHolding::Fungible { definition_id: definition, balance: 0 }),
            nonce: acc.account.nonce,
        },
        is_authorized: false,
        account_id: acc.account_id,
    }
}

#[must_use]
pub fn transfer(token: ProgramId, from: AccountWithMetadata, to: AccountWithMetadata, amount: u128, seeds: Vec<PdaSeed>) -> ChainedCall {
    ChainedCall::new(token, vec![from, to], &TokenIx::Transfer { amount_to_transfer: amount }).with_pda_seeds(seeds)
}

#[must_use]
pub fn burn(token: ProgramId, definition: AccountWithMetadata, holding: AccountWithMetadata, amount: u128, seeds: Vec<PdaSeed>) -> ChainedCall {
    ChainedCall::new(token, vec![definition, holding], &TokenIx::Burn { amount_to_burn: amount }).with_pda_seeds(seeds)
}

#[must_use]
pub fn mint_with_authority(
    token: ProgramId,
    definition: AccountWithMetadata,
    holding: AccountWithMetadata,
    authority: AccountWithMetadata,
    amount: u128,
    seeds: Vec<PdaSeed>,
) -> ChainedCall {
    ChainedCall::new(token, vec![definition, holding, authority], &TokenIx::MintWithAuthority { amount_to_mint: amount })
        .with_pda_seeds(seeds)
}

#[must_use]
pub fn initialize_account(token: ProgramId, definition: AccountWithMetadata, holding: AccountWithMetadata, seeds: Vec<PdaSeed>) -> ChainedCall {
    ChainedCall::new(token, vec![definition, holding], &TokenIx::InitializeAccount).with_pda_seeds(seeds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lee_core::account::Nonce;

    #[test]
    fn simulated_states_use_the_token_encoding() {
        let id = AccountId::new([3; 32]);
        let def = AccountId::new([4; 32]);
        let fresh = AccountWithMetadata { account: Account::default(), is_authorized: true, account_id: id };
        let p = post_init(&fresh, [6; 8], def);
        assert_eq!(p.account.program_owner, [6; 8]);
        assert!(!p.is_authorized);
        assert_eq!(TokenHolding::try_from(&p.account.data).unwrap(), TokenHolding::Fungible { definition_id: def, balance: 0 });
        let q = with_balance(&p, def, 77);
        assert_eq!(TokenHolding::try_from(&q.account.data).unwrap(), TokenHolding::Fungible { definition_id: def, balance: 77 });
        assert_eq!(q.account.nonce, Nonce(0));
    }
}
```

- [ ] **Step 5: Write `admin.rs` (`init_config`, `set_engine_params`)**

```rust
//! Admin instructions. No admin path moves auction collateral, buffer funds or system-auction funds.

use auction_engine_core::{
    args::EngineParamsArgs,
    pda::{self, SLOT_PROTOCOL_TOKEN, SLOT_REF_ORACLE},
    state::*,
    EngineError,
};
use lee_core::{account::AccountWithMetadata, program::ProgramId};
use stablecoin_core::{compute_protocol_parameters_pda, ProtocolParameters};

use crate::{accounts::*, token_calls, EngineResult};

pub struct InitConfigAccounts {
    pub admin: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    pub buffer: AccountWithMetadata,
    pub protocol_parameters: AccountWithMetadata,
    pub stablecoin_definition: AccountWithMetadata,
    pub protocol_token_definition: AccountWithMetadata,
    pub ref_oracle: AccountWithMetadata,
}

/// Creates `EngineConfig`, `Accounting` and the `SurplusBuffer` holding. Permissionless: the signer
/// becomes this config's admin; a host trusts a config only after its admin binds it (ADR-027).
pub fn init_config(program_id: ProgramId, a: InitConfigAccounts, salt: u64, p: &EngineParamsArgs) -> EngineResult {
    if !a.admin.is_authorized {
        return Err(EngineError::NotAdmin);
    }
    nonzero_id(&a.admin.account_id)?;
    let system = p.validate()?;
    // ADR-027 (I7): the config PDA includes its admin, so nobody can squat another admin's address.
    expect_id(&a.config, &pda::config_pda(&program_id, &a.admin.account_id, salt), "config")?;
    expect_default(&a.config, "config")?;
    let config_id = a.config.account_id;
    expect_id(&a.accounting, &pda::accounting_pda(&program_id, &config_id), "accounting")?;
    expect_default(&a.accounting, "accounting")?;
    expect_id(&a.buffer, &pda::buffer_pda(&program_id, &config_id), "buffer")?;
    expect_default(&a.buffer, "buffer")?;

    // ADR-030: the host program id is the ProtocolParameters owner, checked against its PDA.
    let host_program_id = a.protocol_parameters.account.program_owner;
    nonzero_program(&host_program_id)?;
    if a.protocol_parameters.account_id != compute_protocol_parameters_pda(host_program_id) {
        return Err(EngineError::HostMismatch);
    }
    let params = ProtocolParameters::try_from(&a.protocol_parameters.account.data).map_err(|_| EngineError::HostMismatch)?;
    expect_id(&a.stablecoin_definition, &params.stablecoin_definition_id, "stablecoin_definition")?;
    let token_program_id = a.stablecoin_definition.account.program_owner;
    nonzero_program(&token_program_id)?;
    nonzero_id(&params.collateral_definition_id)?;
    let protocol_token = optional_slot(&program_id, &config_id, SLOT_PROTOCOL_TOKEN, p.protocol_token_enabled, &a.protocol_token_definition)?;
    let ref_oracle = optional_slot(&program_id, &config_id, SLOT_REF_ORACLE, p.ref_oracle_enabled, &a.ref_oracle)?;
    check_distinct_slots(protocol_token, ref_oracle, &params.stablecoin_definition_id, &params.collateral_definition_id)?;

    let cfg = EngineConfigState {
        tag: TAG_CONFIG,
        salt,
        admin: a.admin.account_id,
        host_program_id,
        host_protocol_parameters_id: a.protocol_parameters.account_id,
        stablecoin_definition_id: params.stablecoin_definition_id,
        collateral_definition_id: params.collateral_definition_id,
        token_program_id,
        protocol_token,
        protocol_token_ref_price: p.protocol_token_ref_price,
        protocol_token_oracle_id: ref_oracle,
        ref_oracle_max_age_ms: p.ref_oracle_max_age_ms,
        global_cap: p.global_cap,
        global_reserved: 0,
        freeze_on_bad_debt: p.freeze_on_bad_debt,
        bad_debt_freeze_threshold: p.bad_debt_freeze_threshold,
        system: SystemParamsRec::from_core(&system),
    };
    let post = vec![
        echo(&a.admin),
        create(&a.config, &cfg, pda::config_seed(&a.admin.account_id, salt)),
        create(&a.accounting, &AccountingState::new(), pda::accounting_seed(&config_id)),
        echo(&a.buffer),
        echo(&a.protocol_parameters),
        echo(&a.stablecoin_definition),
        echo(&a.protocol_token_definition),
        echo(&a.ref_oracle),
    ];
    let init_buffer = token_calls::initialize_account(
        token_program_id,
        a.stablecoin_definition.clone(),
        token_calls::authorized(&a.buffer),
        vec![pda::buffer_seed(&config_id)],
    );
    Ok((post, vec![init_buffer]))
}

pub struct SetEngineParamsAccounts {
    pub admin: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    /// Read only to re-check `global_cap >= type_cap` (M7); may still be uninitialised.
    pub coll_config: AccountWithMetadata,
    pub protocol_token_definition: AccountWithMetadata,
    pub ref_oracle: AccountWithMetadata,
}

/// Admin update. Admin, host binding, token program, salt and `global_reserved` are immutable (the admin
/// is part of the config PDA). While a surplus or debt auction is live, the protocol token, its oracle
/// and the system-auction parameters cannot change (`ProtocolTokenLocked`): live takes read them. Each
/// change of the reference price stays within [old/2, 2·old] (`RefPriceStepTooLarge`, ADR-031), and
/// `global_cap` may not drop below the collateral type cap (`TypeCapAboveGlobal`).
pub fn set_engine_params(program_id: ProgramId, a: SetEngineParamsAccounts, p: &EngineParamsArgs) -> EngineResult {
    let cfg = load_config(&program_id, &a.config)?;
    require_admin(&a.admin, &cfg)?;
    let system = p.validate()?;
    let config_id = a.config.account_id;
    let acc = load_accounting(&program_id, &config_id, &a.accounting)?;
    let protocol_token = optional_slot(&program_id, &config_id, SLOT_PROTOCOL_TOKEN, p.protocol_token_enabled, &a.protocol_token_definition)?;
    let ref_oracle = optional_slot(&program_id, &config_id, SLOT_REF_ORACLE, p.ref_oracle_enabled, &a.ref_oracle)?;
    check_distinct_slots(protocol_token, ref_oracle, &cfg.stablecoin_definition_id, &cfg.collateral_definition_id)?;
    let old_ref = cfg.protocol_token_ref_price;
    let new_ref = p.protocol_token_ref_price;
    if new_ref != old_ref && (new_ref < old_ref / 2 || new_ref > old_ref.saturating_mul(2)) {
        return Err(EngineError::RefPriceStepTooLarge);
    }
    expect_id(&a.coll_config, &pda::coll_config_pda(&program_id, &config_id, &cfg.collateral_definition_id), "coll_config")?;
    if a.coll_config.account != lee_core::account::Account::default() {
        let coll = load_coll(&program_id, &config_id, &cfg, &a.coll_config)?;
        if coll.type_cap > p.global_cap {
            return Err(EngineError::TypeCapAboveGlobal);
        }
    }
    let system_rec = SystemParamsRec::from_core(&system);
    let locked_fields_change = protocol_token != cfg.protocol_token
        || ref_oracle != cfg.protocol_token_oracle_id
        || system_rec != cfg.system;
    if locked_fields_change && (acc.surplus_live || acc.debt_live) {
        return Err(EngineError::ProtocolTokenLocked);
    }
    let next = EngineConfigState {
        protocol_token,
        protocol_token_ref_price: p.protocol_token_ref_price,
        protocol_token_oracle_id: ref_oracle,
        ref_oracle_max_age_ms: p.ref_oracle_max_age_ms,
        global_cap: p.global_cap,
        freeze_on_bad_debt: p.freeze_on_bad_debt,
        bad_debt_freeze_threshold: p.bad_debt_freeze_threshold,
        system: system_rec,
        ..cfg
    };
    Ok((
        vec![
            echo(&a.admin),
            write(&a.config, &next),
            echo(&a.accounting),
            echo(&a.coll_config),
            echo(&a.protocol_token_definition),
            echo(&a.ref_oracle),
        ],
        vec![],
    ))
}
```

The `ProtocolTokenLocked` branch is exercised in Task 14 (`protocol_token_locked_while_debt_auction_live`), the first task with a live system auction; the `TypeCapAboveGlobal` re-check in Task 9 (`global_cap_below_type_cap_is_rejected`), the first task with a `CollateralConfig`.

- [ ] **Step 6: Write the guest**

`programs/auction_engine/methods/guest/src/bin/auction_engine.rs`:

```rust
#![cfg_attr(not(test), no_main)]

use auction_engine_core::{args::EngineParamsArgs, EngineError};
use auction_engine_program as engine;
use spel_framework::prelude::*;

#[cfg(not(test))]
risc0_zkvm::guest::entry!(main);

/// `SpelError::Custom { code, message }` displays as `Program error {code}: {message}` (raw code);
/// the guest entry point prefixes `[{error_code()}]`, which is 6000 + code (Shared Reference R4).
fn to_spel(e: EngineError) -> spel_framework::error::SpelError {
    spel_framework::error::SpelError::custom(e.code(), e.to_string())
}

/// Three-segment `spel_framework::SpelOutput::execute` path: the macro does not rewrite it, so the
/// handler's explicit claims are kept (ADR-020).
fn finish(result: engine::EngineResult) -> SpelResult {
    let (post_states, chained_calls) = result.map_err(to_spel)?;
    Ok(spel_framework::SpelOutput::execute(post_states, chained_calls))
}

#[lez_program(instruction = "auction_engine_core::Instruction")]
mod auction_engine {
    #[allow(unused_imports)]
    use super::*;

    #[instruction]
    #[allow(clippy::too_many_arguments, reason = "flat spel-encodable arguments (ADR-030)")]
    pub fn init_config(
        ctx: ProgramContext,
        #[account(signer)] admin: AccountWithMetadata,
        #[account(init, pda = [literal("CONFIG"), account("admin"), arg("salt")])] config: AccountWithMetadata,
        #[account(init, pda = [literal("ACCT"), account("config")])] accounting: AccountWithMetadata,
        #[account(mut, pda = [literal("BUFFER"), account("config")])] buffer: AccountWithMetadata,
        protocol_parameters: AccountWithMetadata,
        stablecoin_definition: AccountWithMetadata,
        protocol_token_definition: AccountWithMetadata,
        ref_oracle: AccountWithMetadata,
        salt: u64,
        global_cap: u128,
        freeze_on_bad_debt: bool,
        bad_debt_freeze_threshold: u128,
        protocol_token_enabled: bool,
        protocol_token_ref_price: u128,
        ref_oracle_enabled: bool,
        ref_oracle_max_age_ms: u64,
        surplus_buffer_target: u128,
        surplus_lot: u128,
        surplus_delay_ms: u64,
        surplus_premium_bps: u32,
        surplus_max_discount_bps: u32,
        debt_lot: u128,
        debt_initial_discount_bps: u32,
        debt_growth: Vec<u32>,
        surplus_decay: Vec<u32>,
        max_mint_per_auction: u128,
        min_take: u128,
        system_ttl_ms: u64,
    ) -> SpelResult {
        let p = EngineParamsArgs {
            global_cap, freeze_on_bad_debt, bad_debt_freeze_threshold, protocol_token_enabled,
            protocol_token_ref_price, ref_oracle_enabled, ref_oracle_max_age_ms, surplus_buffer_target,
            surplus_lot, surplus_delay_ms, surplus_premium_bps, surplus_max_discount_bps, debt_lot,
            debt_initial_discount_bps, debt_growth, surplus_decay, max_mint_per_auction, min_take, system_ttl_ms,
        };
        let a = engine::admin::InitConfigAccounts {
            admin, config, accounting, buffer, protocol_parameters, stablecoin_definition,
            protocol_token_definition, ref_oracle,
        };
        finish(engine::admin::init_config(ctx.self_program_id, a, salt, &p))
    }

    #[instruction]
    #[allow(clippy::too_many_arguments, reason = "flat spel-encodable arguments (ADR-030)")]
    pub fn set_engine_params(
        ctx: ProgramContext,
        #[account(signer)] admin: AccountWithMetadata,
        #[account(mut)] config: AccountWithMetadata,
        accounting: AccountWithMetadata,
        coll_config: AccountWithMetadata,
        protocol_token_definition: AccountWithMetadata,
        ref_oracle: AccountWithMetadata,
        global_cap: u128,
        freeze_on_bad_debt: bool,
        bad_debt_freeze_threshold: u128,
        protocol_token_enabled: bool,
        protocol_token_ref_price: u128,
        ref_oracle_enabled: bool,
        ref_oracle_max_age_ms: u64,
        surplus_buffer_target: u128,
        surplus_lot: u128,
        surplus_delay_ms: u64,
        surplus_premium_bps: u32,
        surplus_max_discount_bps: u32,
        debt_lot: u128,
        debt_initial_discount_bps: u32,
        debt_growth: Vec<u32>,
        surplus_decay: Vec<u32>,
        max_mint_per_auction: u128,
        min_take: u128,
        system_ttl_ms: u64,
    ) -> SpelResult {
        let p = EngineParamsArgs {
            global_cap, freeze_on_bad_debt, bad_debt_freeze_threshold, protocol_token_enabled,
            protocol_token_ref_price, ref_oracle_enabled, ref_oracle_max_age_ms, surplus_buffer_target,
            surplus_lot, surplus_delay_ms, surplus_premium_bps, surplus_max_discount_bps, debt_lot,
            debt_initial_discount_bps, debt_growth, surplus_decay, max_mint_per_auction, min_take, system_ttl_ms,
        };
        let a = engine::admin::SetEngineParamsAccounts { admin, config, accounting, coll_config, protocol_token_definition, ref_oracle };
        finish(engine::admin::set_engine_params(ctx.self_program_id, a, &p))
    }
}
```

Keep the guest warning-free: a warning in a guest crate triggered a rustc ICE in spike S2 ("Problems hit").

- [ ] **Step 7: Seed the guest lock file**

Run:
```bash
MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'set -e; cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine
cp programs/stablecoin/methods/guest/Cargo.lock programs/auction_engine/methods/guest/Cargo.lock
. $HOME/lez-spikes/s1/env.sh; cargo metadata --format-version 1 --manifest-path programs/auction_engine/methods/guest/Cargo.toml > /dev/null
grep -c "name = \"auction_engine_core\"" programs/auction_engine/methods/guest/Cargo.lock'
```
Expected: `1` (the stablecoin guest's resolved versions are kept; only the engine's crates are added).

- [ ] **Step 8: Build, generate the engine IDL, run the tests**

Run (background the first time; the guest build takes several minutes):
```bash
MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh run -q -p idl-gen -- programs/auction_engine/methods/guest/src/bin/auction_engine.rs > idl/auction_engine-idl.json && scripts/wsl-cargo.sh test -p auction_engine_program -p engine_integration_tests 2>&1 | grep -E "test result|FAILED|panicked"'
```
Expected: `auction_engine_program` 1 passed; `engine_integration_tests`: `config` 12 passed, `hooks` 3 passed, `harness` 1 passed, `idl` 1 passed. If `idl-gen` cannot resolve the external `auction_engine_core::Instruction` types, run it with the same arguments as for the stablecoin (it scans path dependencies, `tools/idl-gen/src/main.rs:16`); the stablecoin guest uses the same external-enum pattern.

- [ ] **Step 9: Docs and CI fmt scope**

Append to `docs/architecture/programs.md`:

```markdown
## Handler conventions

- Handlers live in `programs/auction_engine/src/` and take `(program_id, <Name>Accounts, args…)`; they return `EngineResult` = post-states for every input account in order plus chained calls. The guest only converts arguments and maps `EngineError` to `SpelError::Custom(code)`.
- Engine accounts are loaded through `accounts::load_*` (owner = engine, 8-byte tag, PDA re-derived from stored fields). Holdings through `fungible_balance` (token program and definition) or `check_recipient` (initialised with the definition, or fresh and signed).
- Chained token calls are built by `token_calls`; a later call in the same transaction carries the simulated post-state of an earlier one, and only the call that attaches a PDA's seed marks it authorised (Shared Reference R3).
- `init_config` is permissionless: the signer becomes that config's admin, the config PDA is `["CONFIG", admin, salt]`, and a host trusts a config only after its admin verifies and binds that config's engine authority (ADR-026/027). The host program id comes from the `ProtocolParameters` owner and the token program from the stablecoin definition owner; both are fixed at `init_config` and clients must verify them against `DEPLOYMENTS.md` (I4).
```

Change the CI fmt line to `cargo fmt -p auction_core -p auction_engine_core -p auction_engine_program -p engine_integration_tests -- --check` (run `scripts/wsl-cargo.sh fmt` on those packages first).

- [ ] **Step 10: Commit (controller — GPG signing on Windows)**

The implementer stages the files and reports; the controller commits from Git Bash.

```bash
git add Cargo.toml Cargo.lock programs/auction_engine programs/integration_tests idl/auction_engine-idl.json docs/architecture/programs.md .github/workflows/core.yml
git commit -m "feat(engine): program skeleton, guest, init_config/set_engine_params"
```

---

### Task 9: `set_collateral_config`, feed validity, pause observation and `observe_feed`

**Files:**
- Create: `programs/auction_engine/src/feed.rs`, `programs/auction_engine/src/observe.rs`
- Modify: `programs/auction_engine/src/lib.rs` (modules), `programs/auction_engine/src/admin.rs` (add `set_collateral_config`)
- Modify: `programs/auction_engine/core/src/instruction.rs` (append `SetCollateralConfig`, `ObserveFeed`), `programs/auction_engine/core/src/lib.rs` (index assertions)
- Modify: guest `auction_engine.rs` (append two functions), `idl/auction_engine-idl.json`
- Modify: `programs/integration_tests/tests/common/mod.rs` (append helpers); Create: `programs/integration_tests/tests/observe.rs`
- Modify: `docs/architecture/programs.md`, `docs/design-log.md` (ADR-028, ADR-032)

**Interfaces:**
- Consumes: Task 6 (`CollateralArgs::validate`, `CollateralConfigState::{feed_params, pause_clock, with_pause_clock}`, `recs_from_schedule`, `TAG_COLL`, `pda::{coll_config_pda, coll_config_seed}`); Task 8 (`accounts::*`, `EngineResult`, guest `finish`); Tasks 7–8 (harness `World`, `assert_err`, `k`, id helpers); `auction_core::{price::{validate_feed, OracleReading}, pause::{Observation, OnsetPolicy, PauseClock}, error::PauseReason}`; `stablecoin_core::{RedemptionPriceState, compute_redemption_price_state_pda, ProtocolParameters, math::compute_current_redemption_price_wide}`; `twap_oracle_core::OraclePriceAccount`.
- Produces:
  - `feed::{u512_to_u128(U512) -> Result<u128, EngineError>, redemption_price(&EngineConfigState, &AccountWithMetadata, u64) -> Result<u128, EngineError>, Observed { clock: PauseClock, feed: Result<u128, PauseReason> }, observe(&CollateralConfigState, &AccountWithMetadata, u128, u64) -> Result<Observed, EngineError>, require_live(&CollateralConfigState, &Observed) -> Result<u128, EngineError>}` — used by Tasks 10 and 11 before every `t_eff` use.
  - `admin::{SetCollateralConfigAccounts, set_collateral_config}`; `observe::{ObserveFeedAccounts, observe_feed}`.
  - `Instruction::SetCollateralConfig {…}` (index 2), `Instruction::ObserveFeed` (index 3).
  - Harness: `collateral_ix(&CollateralArgs) -> Ix`, `World::set_collateral_config(&mut self, signer: u8, c: &CollateralArgs)`, `World::observe_feed(&mut self)`, `World::with_engine() -> World` (host + engine configured with defaults and bound).

- [ ] **Step 1: Write the failing tests**

Append to `programs/integration_tests/tests/common/mod.rs`:

```rust
use auction_engine_core::args::CollateralArgs;

pub fn collateral_ix(c: &CollateralArgs) -> Ix {
    Ix::SetCollateralConfig {
        oracle_scale: c.oracle_scale,
        max_oracle_age_ms: c.max_oracle_age_ms,
        max_conf_bps: c.max_conf_bps,
        band_lo_bps: c.band_lo_bps,
        band_hi_bps: c.band_hi_bps,
        penalty_bps: c.penalty_bps,
        tip: c.tip,
        chip_bps: c.chip_bps,
        reward_cap: c.reward_cap,
        debt_floor: c.debt_floor,
        min_bid: c.min_bid,
        type_cap: c.type_cap,
        schedule: c.schedule.clone(),
        admin_paused: c.admin_paused,
    }
}

impl World {
    pub fn set_collateral_config(&mut self, signer: u8, c: &CollateralArgs) -> Result<(), LeeError> {
        self.engine(
            vec![
                id(signer),
                config(),
                collateral_def(),
                coll_config(),
                oracle(),
                protocol_parameters(),
                redemption_state(),
                CLOCK_01_PROGRAM_ACCOUNT_ID,
            ],
            &[signer],
            collateral_ix(c),
        )
    }

    pub fn observe_feed(&mut self) -> Result<(), LeeError> {
        self.engine(
            vec![id(k::CALLER), config(), coll_config(), redemption_state(), oracle(), CLOCK_01_PROGRAM_ACCOUNT_ID],
            &[k::CALLER],
            Ix::ObserveFeed,
        )
    }

    /// Host bootstrapped, engine config created with defaults, bound by the host admin, collateral
    /// configured with defaults (Ray scale, 60 min max age, band 9000–11000, penalty 10%).
    pub fn with_engine() -> Self {
        let mut w = Self::new();
        w.init_engine_config(&EngineParamsArgs::defaults_for_tests()).expect("init_config");
        w.bind_engine().expect("set_liquidation_engine");
        w.set_collateral_config(k::ENGINE_ADMIN, &CollateralArgs::defaults_for_tests())
            .expect("set_collateral_config");
        w
    }
}
```

`programs/integration_tests/tests/observe.rs`:

```rust
mod common;

use auction_engine_core::{args::CollateralArgs, state::CollateralConfigState};
use common::*;

fn coll(w: &World) -> CollateralConfigState {
    w.read(coll_config())
}

#[test]
fn set_collateral_config_creates_then_updates() {
    let mut w = World::with_engine();
    let c = coll(&w);
    assert_eq!((c.oracle_id, c.penalty_bps, c.type_reserved, c.admin_paused), (oracle(), 1_000, 0, false));
    assert_eq!(c.last_seen_oracle_ts, Some(T0));
    let tighter = CollateralArgs { penalty_bps: 1_200, ..CollateralArgs::defaults_for_tests() };
    w.set_collateral_config(k::ENGINE_ADMIN, &tighter).expect("update");
    assert_eq!(coll(&w).penalty_bps, 1_200);
}

#[test]
fn set_collateral_config_rejections() {
    let mut w = World::with_engine();
    assert_err(w.set_collateral_config(k::ATTACKER, &CollateralArgs::defaults_for_tests()), "NotAdmin");
    let too_big = CollateralArgs { type_cap: 10_000_001, ..CollateralArgs::defaults_for_tests() };
    assert_err(w.set_collateral_config(k::ENGINE_ADMIN, &too_big), "TypeCapAboveGlobal");
    let r = w.engine(
        vec![id(k::ENGINE_ADMIN), config(), collateral_def(), coll_config(), ref_oracle(), protocol_parameters(), redemption_state(), clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID],
        &[k::ENGINE_ADMIN],
        collateral_ix(&CollateralArgs::defaults_for_tests()),
    );
    assert_err(r, "OracleMismatch");
}

#[test]
fn global_cap_below_type_cap_is_rejected() {
    // M7: set_engine_params re-checks global_cap >= CollateralConfig.type_cap (5_000_000 by default).
    let mut w = World::with_engine();
    let low = auction_engine_core::args::EngineParamsArgs { global_cap: 4_999_999, ..auction_engine_core::args::EngineParamsArgs::defaults_for_tests() };
    assert_err(w.set_engine_params(k::ENGINE_ADMIN, &low), "TypeCapAboveGlobal");
}

#[test]
fn admin_pause_is_exact() {
    let mut w = World::with_engine();
    w.advance(1_000);
    w.set_oracle(R, w.now);
    let paused = CollateralArgs { admin_paused: true, ..CollateralArgs::defaults_for_tests() };
    w.set_collateral_config(k::ENGINE_ADMIN, &paused).unwrap();
    assert_eq!(coll(&w).paused_since_ms, Some(T0 + 1_000));
    w.advance(10_000);
    w.set_oracle(R, w.now);
    w.set_collateral_config(k::ENGINE_ADMIN, &CollateralArgs::defaults_for_tests()).unwrap();
    let c = coll(&w);
    assert_eq!((c.paused_since_ms, c.paused_total_ms), (None, 10_000));
}

#[test]
fn observe_feed_persists_stale_pause() {
    // Review Focus 5: the feed goes stale with nobody watching; observe_feed records the derived onset.
    let mut w = World::with_engine();
    w.advance(3_600_000 + 5_000);
    w.observe_feed().expect("observe_feed never fails on a bad feed");
    assert_eq!(coll(&w).paused_since_ms, Some(T0 + 3_600_000));
    w.advance(1_000);
    w.set_oracle(R, w.now);
    w.observe_feed().unwrap();
    let c = coll(&w);
    assert_eq!(c.paused_since_ms, None);
    assert_eq!(c.paused_total_ms, w.now - (T0 + 3_600_000));
}

#[test]
fn low_confidence_and_out_of_range_open_the_clock() {
    let mut w = World::with_engine();
    w.advance(1_000);
    w.set_oracle_full(R, w.now, R / 50); // 2% > max_conf 1%
    w.observe_feed().unwrap();
    assert_eq!(coll(&w).paused_since_ms, Some(w.now));

    let mut w = World::with_engine();
    w.advance(1_000);
    w.set_oracle(3 * R, w.now); // above 2R
    w.observe_feed().unwrap();
    assert_eq!(coll(&w).paused_since_ms, Some(w.now));
}

#[test]
fn observe_with_wrong_oracle_is_rejected() {
    let mut w = World::with_engine();
    let r = w.engine(
        vec![id(k::CALLER), config(), coll_config(), redemption_state(), ref_oracle(), clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID],
        &[k::CALLER],
        auction_engine_core::Instruction::ObserveFeed,
    );
    assert_err(r, "OracleMismatch");
}
```

Append to the `tests` module of `programs/auction_engine/core/src/lib.rs`:

```rust
    #[test]
    fn collateral_and_observe_indices() {
        assert_eq!(idx(&Instruction::ObserveFeed), 3);
        let c = args::CollateralArgs::defaults_for_tests();
        assert_eq!(
            idx(&Instruction::SetCollateralConfig {
                oracle_scale: c.oracle_scale, max_oracle_age_ms: c.max_oracle_age_ms, max_conf_bps: c.max_conf_bps,
                band_lo_bps: c.band_lo_bps, band_hi_bps: c.band_hi_bps, penalty_bps: c.penalty_bps, tip: c.tip,
                chip_bps: c.chip_bps, reward_cap: c.reward_cap, debt_floor: c.debt_floor, min_bid: c.min_bid,
                type_cap: c.type_cap, schedule: c.schedule.clone(), admin_paused: c.admin_paused,
            }),
            2
        );
    }
```

- [ ] **Step 2: Run and see the failure**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_engine_core -p engine_integration_tests --test observe'`
Expected: compile errors: no variant `SetCollateralConfig` / `ObserveFeed` on `Instruction`.

- [ ] **Step 3: Append the variants**

In `programs/auction_engine/core/src/instruction.rs`, insert directly after `SetEngineParams { … },` (i.e. above the `// Tasks 9–15 append …` comment, which stays last until Task 15 removes it):

```rust
    /// Accounts: admin(s), config, collateral_definition, coll_config(m), oracle, protocol_parameters,
    /// redemption_price_state, clock.
    SetCollateralConfig {
        oracle_scale: u8,
        max_oracle_age_ms: u64,
        max_conf_bps: u32,
        band_lo_bps: u32,
        band_hi_bps: u32,
        penalty_bps: u32,
        tip: u128,
        chip_bps: u32,
        reward_cap: u128,
        debt_floor: u128,
        min_bid: u128,
        type_cap: u128,
        schedule: Vec<u32>,
        admin_paused: bool,
    },
    /// Accounts: caller(s), config, coll_config(m), redemption_price_state, oracle, clock.
    ObserveFeed,
```


- [ ] **Step 4: Write `feed.rs`**

```rust
//! Redemption price, oracle validity and the type-level pause observation (spec §5.2, §5.7).

use alloy_primitives::U512;
use auction_core::{
    error::PauseReason,
    pause::{Observation, OnsetPolicy, PauseClock},
    price::{validate_feed, OracleReading},
};
use auction_engine_core::{state::*, EngineError};
use lee_core::account::AccountWithMetadata;
use stablecoin_core::{
    compute_redemption_price_state_pda, math::compute_current_redemption_price_wide, RedemptionPriceState,
};
use twap_oracle_core::OraclePriceAccount;

pub fn u512_to_u128(v: U512) -> Result<u128, EngineError> {
    if v > U512::from(u128::MAX) {
        Err(EngineError::Math)
    } else {
        Ok(v.to::<u128>())
    }
}

/// R at `now`: the host's own projection, asserted to fit `u128` (spec §5.2).
pub fn redemption_price(cfg: &EngineConfigState, rps: &AccountWithMetadata, now: u64) -> Result<u128, EngineError> {
    if rps.account_id != compute_redemption_price_state_pda(cfg.host_program_id)
        || rps.account.program_owner != cfg.host_program_id
    {
        return Err(EngineError::WrongAccount("redemption_price_state"));
    }
    let s = RedemptionPriceState::try_from(&rps.account.data)
        .map_err(|_| EngineError::WrongAccount("redemption_price_state"))?;
    u512_to_u128(compute_current_redemption_price_wide(
        s.redemption_price_at_last_update,
        s.redemption_rate_per_millisecond,
        s.last_updated_at,
        now,
    ))
}

pub struct Observed {
    pub clock: PauseClock,
    pub feed: Result<u128, PauseReason>,
}

/// One pause observation at `now` (policy (b), ADR-007). `feed_valid` is derived with `validate_feed`
/// from the same `(oracle_ts, now, max_age)` that `observed` receives (ADR-024 obligation).
pub fn observe(coll: &CollateralConfigState, oracle: &AccountWithMetadata, r_ray: u128, now: u64) -> Result<Observed, EngineError> {
    if oracle.account_id != coll.oracle_id {
        return Err(EngineError::OracleMismatch);
    }
    let o = OraclePriceAccount::try_from(&oracle.account.data).map_err(|_| EngineError::WrongAccount("oracle"))?;
    let params = coll.feed_params()?;
    let reading = OracleReading { price_raw: o.price, confidence_raw: o.confidence_interval, timestamp_ms: o.timestamp };
    let feed = validate_feed(&reading, &params, r_ray, now);
    let clock = coll.pause_clock().observed(
        Observation {
            now_ms: now,
            feed_valid: feed.is_ok(),
            admin_paused: coll.admin_paused,
            oracle_ts: o.timestamp,
            max_age_ms: params.max_age_ms,
        },
        OnsetPolicy::DerivedFromOracle,
    );
    Ok(Observed { clock, feed })
}

/// The market price if the type is live, else the pause as an error (`Paused`, `PriceFeedStale`,
/// `PriceFeedLowConfidence`, `PriceFeedOutOfRange`). A reverting transaction drops its observation;
/// policy (b) recovers the onset from the oracle timestamp at the next successful one (ADR-032).
pub fn require_live(coll: &CollateralConfigState, obs: &Observed) -> Result<u128, EngineError> {
    if coll.admin_paused {
        return Err(EngineError::Paused);
    }
    obs.feed.map_err(EngineError::from)
}
```

- [ ] **Step 5: Write `observe.rs` and `admin::set_collateral_config`**

`programs/auction_engine/src/observe.rs`:

```rust
//! `observe_feed`: permissionless pause observation (spec §5.7, ADR-032). Never fails on a bad feed.

use auction_engine_core::EngineError;
use lee_core::{account::AccountWithMetadata, program::ProgramId};

use crate::{accounts::*, feed, EngineResult};

pub struct ObserveFeedAccounts {
    pub caller: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub coll_config: AccountWithMetadata,
    pub redemption_price_state: AccountWithMetadata,
    pub oracle: AccountWithMetadata,
    pub clock: AccountWithMetadata,
}

pub fn observe_feed(program_id: ProgramId, a: ObserveFeedAccounts) -> EngineResult {
    if !a.caller.is_authorized {
        return Err(EngineError::WrongAccount("caller"));
    }
    let now = read_clock(&a.clock)?;
    let cfg = load_config(&program_id, &a.config)?;
    let coll = load_coll(&program_id, &a.config.account_id, &cfg, &a.coll_config)?;
    let r = feed::redemption_price(&cfg, &a.redemption_price_state, now)?;
    let obs = feed::observe(&coll, &a.oracle, r, now)?;
    let next = coll.with_pause_clock(obs.clock);
    Ok((
        vec![
            echo(&a.caller),
            echo(&a.config),
            write(&a.coll_config, &next),
            echo(&a.redemption_price_state),
            echo(&a.oracle),
            echo(&a.clock),
        ],
        vec![],
    ))
}
```

Append to `programs/auction_engine/src/admin.rs` (and add `use auction_engine_core::args::CollateralArgs; use lee_core::account::Account; use crate::feed;` to its imports):

```rust
pub struct SetCollateralConfigAccounts {
    pub admin: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub collateral_definition: AccountWithMetadata,
    pub coll_config: AccountWithMetadata,
    pub oracle: AccountWithMetadata,
    pub protocol_parameters: AccountWithMetadata,
    pub redemption_price_state: AccountWithMetadata,
    pub clock: AccountWithMetadata,
}

/// Creates or updates the `CollateralConfig`. Schedule changes affect only new auctions (each
/// auction snapshots its schedule). `admin_paused` changes the pause clock in this transaction, so
/// an admin pause starts and ends exactly at its transaction's clock (spec §5.7).
pub fn set_collateral_config(program_id: ProgramId, a: SetCollateralConfigAccounts, c: &CollateralArgs) -> EngineResult {
    let now = read_clock(&a.clock)?;
    let cfg = load_config(&program_id, &a.config)?;
    require_admin(&a.admin, &cfg)?;
    let (params, feed_params) = c.validate(cfg.global_cap)?;
    let config_id = a.config.account_id;
    expect_id(&a.collateral_definition, &cfg.collateral_definition_id, "collateral_definition")?;
    expect_id(&a.coll_config, &pda::coll_config_pda(&program_id, &config_id, &cfg.collateral_definition_id), "coll_config")?;
    expect_id(&a.protocol_parameters, &cfg.host_protocol_parameters_id, "protocol_parameters")?;
    if a.protocol_parameters.account.program_owner != cfg.host_program_id {
        return Err(EngineError::HostMismatch);
    }
    let host = ProtocolParameters::try_from(&a.protocol_parameters.account.data).map_err(|_| EngineError::HostMismatch)?;
    // ADR-028: price from the same observation the host's health gate uses.
    if a.oracle.account_id != host.market_price_oracle_id {
        return Err(EngineError::OracleMismatch);
    }
    let base = if a.coll_config.account == Account::default() {
        CollateralConfigState {
            tag: TAG_COLL,
            config: config_id,
            collateral_definition_id: cfg.collateral_definition_id,
            oracle_id: a.oracle.account_id,
            oracle_scale: 0,
            max_oracle_age_ms: 0,
            max_conf_bps: 0,
            band_lo_bps: 0,
            band_hi_bps: 0,
            penalty_bps: 0,
            tip: 0,
            chip_bps: 0,
            reward_cap: 0,
            debt_floor: 0,
            min_bid: 0,
            type_cap: 0,
            type_reserved: 0,
            schedule: vec![],
            admin_paused: false,
            paused_since_ms: None,
            paused_total_ms: 0,
            last_seen_oracle_ts: None,
        }
    } else {
        load_coll(&program_id, &config_id, &cfg, &a.coll_config)?
    };
    let next = CollateralConfigState {
        oracle_id: a.oracle.account_id,
        oracle_scale: c.oracle_scale,
        max_oracle_age_ms: feed_params.max_age_ms,
        max_conf_bps: feed_params.max_conf_bps,
        band_lo_bps: feed_params.band_lo_bps,
        band_hi_bps: feed_params.band_hi_bps,
        penalty_bps: params.penalty_bps,
        tip: params.tip,
        chip_bps: params.chip_bps,
        reward_cap: params.reward_cap,
        debt_floor: params.debt_floor,
        min_bid: params.min_bid,
        type_cap: c.type_cap,
        schedule: recs_from_schedule(&params.schedule),
        admin_paused: c.admin_paused,
        ..base
    };
    let r = feed::redemption_price(&cfg, &a.redemption_price_state, now)?;
    let obs = feed::observe(&next, &a.oracle, r, now)?;
    let next = next.with_pause_clock(obs.clock);
    Ok((
        vec![
            echo(&a.admin),
            echo(&a.config),
            echo(&a.collateral_definition),
            write_or_create(&a.coll_config, &next, pda::coll_config_seed(&config_id, &cfg.collateral_definition_id)),
            echo(&a.oracle),
            echo(&a.protocol_parameters),
            echo(&a.redemption_price_state),
            echo(&a.clock),
        ],
        vec![],
    ))
}
```

Add `pub mod feed;` and `pub mod observe;` to `lib.rs`.

- [ ] **Step 6: Append the guest functions**

```rust
    #[instruction]
    #[allow(clippy::too_many_arguments, reason = "flat spel-encodable arguments (ADR-030)")]
    pub fn set_collateral_config(
        ctx: ProgramContext,
        #[account(signer)] admin: AccountWithMetadata,
        config: AccountWithMetadata,
        collateral_definition: AccountWithMetadata,
        #[account(mut, pda = [literal("COLL"), account("config"), account("collateral_definition")])]
        coll_config: AccountWithMetadata,
        oracle: AccountWithMetadata,
        protocol_parameters: AccountWithMetadata,
        redemption_price_state: AccountWithMetadata,
        clock: AccountWithMetadata,
        oracle_scale: u8,
        max_oracle_age_ms: u64,
        max_conf_bps: u32,
        band_lo_bps: u32,
        band_hi_bps: u32,
        penalty_bps: u32,
        tip: u128,
        chip_bps: u32,
        reward_cap: u128,
        debt_floor: u128,
        min_bid: u128,
        type_cap: u128,
        schedule: Vec<u32>,
        admin_paused: bool,
    ) -> SpelResult {
        let c = auction_engine_core::args::CollateralArgs {
            oracle_scale, max_oracle_age_ms, max_conf_bps, band_lo_bps, band_hi_bps, penalty_bps, tip,
            chip_bps, reward_cap, debt_floor, min_bid, type_cap, schedule, admin_paused,
        };
        let a = engine::admin::SetCollateralConfigAccounts {
            admin, config, collateral_definition, coll_config, oracle, protocol_parameters,
            redemption_price_state, clock,
        };
        finish(engine::admin::set_collateral_config(ctx.self_program_id, a, &c))
    }

    #[instruction]
    pub fn observe_feed(
        ctx: ProgramContext,
        #[account(signer)] caller: AccountWithMetadata,
        config: AccountWithMetadata,
        #[account(mut)] coll_config: AccountWithMetadata,
        redemption_price_state: AccountWithMetadata,
        oracle: AccountWithMetadata,
        clock: AccountWithMetadata,
    ) -> SpelResult {
        let a = engine::observe::ObserveFeedAccounts { caller, config, coll_config, redemption_price_state, oracle, clock };
        finish(engine::observe::observe_feed(ctx.self_program_id, a))
    }
```

- [ ] **Step 7: Regenerate the IDL and run the tests**

Run:
```bash
MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh run -q -p idl-gen -- programs/auction_engine/methods/guest/src/bin/auction_engine.rs > idl/auction_engine-idl.json && scripts/wsl-cargo.sh test -p auction_engine_core -p auction_engine_program -p engine_integration_tests 2>&1 | grep -E "test result|FAILED|panicked"'
```
Expected: all ok; `observe` 7 passed; `idl` still passes (4 names, a prefix of R2).

- [ ] **Step 8: Docs, ADR-028, ADR-032**

Append to `docs/architecture/programs.md`:

```markdown
## Feed, pause clock and `observe_feed`

Every `liquidate` and `bid` first observes the feed at `now` (`feed::observe`): `validate_feed` (age, zero price, confidence, `[R/2, 2R]`) with R projected by the host's own `compute_current_redemption_price_wide`, then `PauseClock::observed` with policy (b) `DerivedFromOracle` (ADR-007). Only then is `t_eff` computed. A paused type (admin flag or invalid feed) reverts with `Paused` / `PriceFeedStale` / `PriceFeedLowConfidence` / `PriceFeedOutOfRange`; the reverted observation is lost, which policy (b) tolerates because the stale onset is derived from the oracle timestamp at the next successful observation. `observe_feed` is that successful observation: permissionless, never reverts on a bad feed, writes only the pause clock. `set_collateral_config` writes the admin flag and observes in the same transaction, so admin pauses are exact.
```

Append ADR-028 and ADR-032 to `docs/design-log.md`:

```markdown
---

## ADR-028: The Engine Prices from the Host's Market Oracle

**Context**
Spec §5.4 stores an `oracle_id` in `CollateralConfig`; spec §5.10 makes the hook check the host's market oracle for freshness. LEZ rejects duplicate account ids in a transaction, so listing "engine oracle" and "host oracle" separately when they are the same account is impossible, and listing two different oracles would let the host's health check and the engine's price disagree within one liquidation.

**Decision**
`set_collateral_config` requires `oracle_id == ProtocolParameters.market_price_oracle_id`, and `liquidate` re-checks it against the live `ProtocolParameters` (`OracleMismatch` if the host admin rotated its oracle; the engine admin then re-runs `set_collateral_config`). One `oracle` account serves both the engine's validity/price and the hook's freshness gate. Bids keep using the stored id.

**Trade-offs**
A host oracle rotation pauses new liquidations until the engine admin follows; bids on live auctions are unaffected.

**Status**
Accepted (Plan 2, Task 9)

---

## ADR-032: `observe_feed` Is Permissionless; Reverted Observations Are Dropped

**Context**
Spec §5.7: `liquidate`, `bid` and `observe_feed` observe the feed; any panic reverts the whole transaction (spec §3.2), so a `liquidate` or `bid` that finds the feed invalid cannot persist the pause it observed.

**Decision**
`observe_feed` is permissionless and never reverts on a bad feed; it is the one way to persist an observation of an invalid feed. `liquidate`/`bid` observe in memory, revert when paused, and their observation is lost. Policy (b) (S5, ADR-007) derives the stale onset from `last_seen_oracle_ts + max_age`, so a lost observation does not move the onset; the S5 property test bounds an adversary's extra discount to one block. Keepers and the app call `observe_feed` on refresh (Plan 3).

**Trade-offs**
An observation costs a transaction; the residual (unobserved publishes, a re-stamping producer) is the S5 residual recorded in `THREAT_MODEL.md`.

**Status**
Accepted (Plan 2, Task 9)
```

- [ ] **Step 9: Commit (controller — GPG signing on Windows)**

The implementer stages the files and reports; the controller commits from Git Bash.

```bash
git add programs/auction_engine programs/integration_tests idl/auction_engine-idl.json docs/architecture/programs.md docs/design-log.md
git commit -m "feat(engine): set_collateral_config, feed validity, pause observation, observe_feed; ADR-028, ADR-032"
```

---

### Task 10: `liquidate`

**Files:**
- Create: `programs/auction_engine/src/liquidate.rs`
- Modify: `programs/auction_engine/src/lib.rs`, `programs/auction_engine/core/src/{instruction.rs,lib.rs}` (append `Liquidate { nonce: u64 }`, index 4), guest (append `liquidate`), `idl/auction_engine-idl.json`
- Modify: `programs/integration_tests/tests/common/mod.rs` (append); Create: `programs/integration_tests/tests/liquidate.rs`
- Modify: `docs/architecture/programs.md`

**Interfaces:**
- Consumes: Tasks 6–9 (`pda::*`, `AuctionState::from_core`, `LiquidationRecordState`, `TAG_*`, `accounts::*`, `token_calls::*`, `feed::{redemption_price, observe, require_live, u512_to_u128}`); Task 5 (`stablecoin_core::Instruction::LiquidatePosition` — the host performs the health check and the sweep); `auction_core::{liquidation::{quote_liquidation, CapState, LiquidationInput}, auction::Auction, price::bid_price, math::mul_div_ceil, units::RAY}`; `stablecoin_core::{Position, ProtocolParameters, StabilityFeeAccumulator, compute_position_pda, compute_position_vault_pda, compute_stability_fee_accumulator_pda, compute_liquidation_engine_state_pda, math::compute_current_accumulated_rate_wide}`.
- Produces: `liquidate::{LiquidateAccounts, liquidate(ProgramId, LiquidateAccounts, u64) -> EngineResult}`; `Instruction::Liquidate { nonce: u64 }` (index 4); harness `World::{unsafe_position, open_small_unsafe_position, liquidate_accounts, liquidate, transfer_tokens}` and `LIQ` constants (`LIQ_COLLATERAL = 500_000`, `LIQ_DEBT = 600_000`, `LIQ_TARGET = 660_000`, `LIQ_REWARD = 25`, `LIQ_LOT = 499_975`). Chain: `InitializeAccount(auction_vault*)` → `liquidate_position(engine_authority*, …)` → [host `Transfer(host_vault*, auction_vault)`] → `Transfer(auction_vault*, keeper_holding, reward)` if reward > 0 (5 executions).

- [ ] **Step 1: Write the failing tests**

Append to `programs/integration_tests/tests/common/mod.rs`:

```rust
/// Standard unsafe position (Task 10): 500_000 collateral, 600_000 debt at R = 0.5 (safe at min CR 1.5,
/// unsafe at 2.0). Penalty 10% → target 660_000; reward min(5 + 600, 50, 60_000) = 50 coin = 25 collateral.
pub const LIQ_COLLATERAL: u128 = 500_000;
pub const LIQ_DEBT: u128 = 600_000;
pub const LIQ_TARGET: u128 = 660_000;
pub const LIQ_REWARD: u128 = 25;
pub const LIQ_LOT: u128 = LIQ_COLLATERAL - LIQ_REWARD;

impl World {
    pub fn unsafe_position(&mut self) -> AccountId {
        let pos = self.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 0, LIQ_COLLATERAL, LIQ_DEBT);
        self.set_min_cr(2 * ONE);
        pos
    }

    /// A second, smaller position (100_000 collateral, 120_000 debt → target 132_000); open it BEFORE
    /// `set_min_cr(2 * ONE)`.
    pub fn open_small_position(&mut self) -> AccountId {
        self.open_position(k::OWNER2, k::OWNER2_COLL, k::OWNER2_STABLE, 0, 100_000, 120_000)
    }

    pub fn liquidate_accounts(&self, pos: AccountId, nonce: u64, keeper_holding: AccountId) -> Vec<AccountId> {
        vec![
            id(k::KEEPER),
            keeper_holding,
            config(),
            coll_config(),
            pos,
            record(pos),
            auction(pos, nonce),
            auction_vault(pos, nonce),
            engine_authority(),
            les(),
            host_vault(pos),
            collateral_def(),
            accumulator(),
            redemption_state(),
            oracle(),
            protocol_parameters(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ]
    }

    /// Liquidates with the keeper's initialised collateral holding `id(k::KEEPER_COLL)`.
    pub fn liquidate(&mut self, pos: AccountId, nonce: u64) -> Result<(), LeeError> {
        if self.state.get_account_by_id(id(k::KEEPER_COLL)) == Account::default() {
            self.holding(k::KEEPER_COLL, collateral_def(), 0);
        }
        let accounts = self.liquidate_accounts(pos, nonce, id(k::KEEPER_COLL));
        self.engine(accounts, &[k::KEEPER], Ix::Liquidate { nonce })
    }

    /// A plain `Token::Transfer` signed by holding `id(from)` (donations in tests).
    pub fn transfer_tokens(&mut self, from: u8, to: AccountId, amount: u128) -> Result<(), LeeError> {
        self.submit(token_program(), vec![id(from), to], &[from], token_core::Instruction::Transfer { amount_to_transfer: amount })
    }
}
```

`programs/integration_tests/tests/liquidate.rs`:

```rust
mod common;

use auction_engine_core::{
    args::CollateralArgs,
    state::{AuctionState, CollateralConfigState, EngineConfigState, LiquidationRecordState},
    Instruction as Ix,
};
use common::*;
use stablecoin_core::Position;

fn coll(w: &World) -> CollateralConfigState {
    w.read(coll_config())
}

#[test]
fn liquidate_happy_path() {
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    w.liquidate(pos, 0).expect("liquidate");
    let a: AuctionState = w.read(auction(pos, 0));
    assert_eq!(
        (a.collateral_seized, a.keeper_reward, a.collateral_left, a.debt_principal, a.amount_to_raise, a.reserved_amount),
        (LIQ_COLLATERAL, LIQ_REWARD, LIQ_LOT, LIQ_DEBT, LIQ_TARGET, LIQ_TARGET)
    );
    assert_eq!((a.owner, a.keeper, a.settled, a.start_ms), (id(k::OWNER), id(k::KEEPER), false, w.now));
    let p: Position = w.read(pos);
    assert_eq!((p.collateral_amount, p.normalized_debt_amount), (0, 0));
    assert_eq!(w.balance(host_vault(pos)), 0);
    assert_eq!(w.balance(auction_vault(pos, 0)), LIQ_LOT);
    assert_eq!(w.balance(id(k::KEEPER_COLL)), LIQ_REWARD);
    assert_eq!(coll(&w).type_reserved, LIQ_TARGET);
    assert_eq!(w.read::<EngineConfigState>(config()).global_reserved, LIQ_TARGET);
    assert_eq!(w.read::<LiquidationRecordState>(record(pos)).count, 1);
}

#[test]
fn replayed_and_skipped_nonces_are_rejected() {
    let mut w = World::with_engine();
    let pos = w.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 0, LIQ_COLLATERAL, LIQ_DEBT);
    let small = w.open_small_position();
    w.set_min_cr(2 * ONE);
    w.liquidate(pos, 0).unwrap();
    assert_err(w.liquidate(pos, 0), "AccountAlreadyInitialized");
    assert_err(w.liquidate(small, 1), "NonceMismatch");
}

#[test]
fn refilled_position_is_liquidated_again_with_next_nonce() {
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    w.liquidate(pos, 0).unwrap();
    // Owner refills the same position: deposit 500_000, borrow 400_000 (safe at CR 2.0), then CR 3.0.
    w.fund_collateral(k::OWNER_COLL, 500_000);
    w.host(
        vec![id(k::OWNER), pos, host_vault(pos), id(k::OWNER_COLL), protocol_parameters()],
        &[k::OWNER, k::OWNER_COLL],
        stablecoin_core::Instruction::DepositCollateral { amount: 500_000 },
    )
    .expect("deposit");
    w.host(
        vec![id(k::OWNER), pos, stable_def(), id(k::OWNER_STABLE), accumulator(), redemption_state(), oracle(), protocol_parameters(), clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID],
        &[k::OWNER],
        stablecoin_core::Instruction::GenerateDebt { amount: 400_000 },
    )
    .expect("borrow");
    w.set_min_cr(3 * ONE);
    w.liquidate(pos, 1).expect("second liquidation");
    assert_eq!(w.read::<LiquidationRecordState>(record(pos)).count, 2);
    assert_eq!(coll(&w).type_reserved, LIQ_TARGET + 440_000);
}

#[test]
fn oversized_auction_blocks_others_and_small_auction_blocks_whale() {
    let cap = CollateralArgs { type_cap: 600_000, ..CollateralArgs::defaults_for_tests() };
    // Direction 1: the oversized (660_000 > 600_000) liquidation goes alone, then blocks the small one.
    let mut w = World::with_engine();
    let whale = w.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 0, LIQ_COLLATERAL, LIQ_DEBT);
    let small = w.open_small_position();
    w.set_min_cr(2 * ONE);
    w.set_collateral_config(k::ENGINE_ADMIN, &cap).unwrap();
    w.liquidate(whale, 0).expect("oversized alone");
    assert_err(w.liquidate(small, 0), "CapacityExhausted");
    // Direction 2: a live small auction (132_000) makes the whale wait (660_000 > 600_000 - 132_000).
    let mut w = World::with_engine();
    let whale = w.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 0, LIQ_COLLATERAL, LIQ_DEBT);
    let small = w.open_small_position();
    w.set_min_cr(2 * ONE);
    w.set_collateral_config(k::ENGINE_ADMIN, &cap).unwrap();
    w.liquidate(small, 0).expect("small fits");
    assert_err(w.liquidate(whale, 0), "CapacityExhausted");
}

#[test]
fn underwater_reward_is_capped_at_the_seizure() {
    // Spec §5.9 / invariant 16: reward_coll = min(floor(6 coin × 250 collateral/coin), c = 1_000) = 1_000.
    let mut w = World::with_engine();
    let pos = w.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 0, 1_000, 1_000);
    w.set_redemption_price(250 * ONE);
    w.set_oracle(250 * ONE, w.now);
    w.liquidate(pos, 0).expect("liquidate underwater");
    let a: AuctionState = w.read(auction(pos, 0));
    assert_eq!((a.keeper_reward, a.collateral_left), (1_000, 0));
    assert_eq!(w.balance(id(k::KEEPER_COLL)), 1_000);
}

#[test]
fn liquidate_rejects_stale_feed_and_admin_pause() {
    // Review Focus 5.
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    w.advance(3_600_001);
    assert_err(w.liquidate(pos, 0), "PriceFeedStale");
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    let paused = CollateralArgs { admin_paused: true, ..CollateralArgs::defaults_for_tests() };
    w.set_collateral_config(k::ENGINE_ADMIN, &paused).unwrap();
    assert_err(w.liquidate(pos, 0), "Paused");
    for (needle, price, confidence) in [("PriceFeedLowConfidence", R, R / 50), ("PriceFeedOutOfRange", 3 * R, 0)] {
        let mut w = World::with_engine();
        let pos = w.unsafe_position();
        w.set_oracle_full(price, w.now, confidence);
        assert_err(w.liquidate(pos, 0), needle);
    }
}

#[test]
fn healthy_position_is_rejected_by_the_host() {
    let mut w = World::with_engine();
    let pos = w.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 0, LIQ_COLLATERAL, 300_000);
    assert_err(w.liquidate(pos, 0), "Position is not liquidatable");
    assert_eq!(coll(&w).type_reserved, 0, "the whole transaction reverted");
}

#[test]
fn keeper_reward_to_fresh_holding() {
    // Review Focus 2: a never-initialised keeper holding works when it signs, fails clearly when not.
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    let accounts = w.liquidate_accounts(pos, 0, id(k::KEEPER_COLL));
    assert_err(w.engine(accounts.clone(), &[k::KEEPER], Ix::Liquidate { nonce: 0 }), "RecipientMustSign");
    w.engine(accounts, &[k::KEEPER, k::KEEPER_COLL], Ix::Liquidate { nonce: 0 }).expect("fresh signed holding");
    assert_eq!(w.balance(id(k::KEEPER_COLL)), LIQ_REWARD);
}

#[test]
fn donation_into_host_vault_is_seized_too() {
    // Review Focus 3: the hook sweeps the whole vault; the lot includes the donation.
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    w.fund_collateral(k::DONOR_COLL, 10);
    w.transfer_tokens(k::DONOR_COLL, host_vault(pos), 10).expect("donation");
    w.liquidate(pos, 0).unwrap();
    let a: AuctionState = w.read(auction(pos, 0));
    assert_eq!((a.collateral_seized, a.collateral_left), (LIQ_COLLATERAL + 10, LIQ_LOT + 10));
    assert_eq!(w.balance(auction_vault(pos, 0)), LIQ_LOT + 10);
}

#[test]
fn foreign_account_as_position_is_rejected() {
    // Confused deputy (iii): nothing but the bound host's real position and its own vault reaches the hook.
    let mut w = World::with_engine();
    let pos = w.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 0, LIQ_COLLATERAL, LIQ_DEBT);
    let small = w.open_small_position();
    w.set_min_cr(2 * ONE);
    // Not owned by the host.
    assert_err(w.liquidate(accounting(), 0), "HostMismatch");
    // Host-owned but not a position.
    let fake = lee_core::account::AccountId::new([0x99; 32]);
    w.state.force_insert_account(
        fake,
        lee_core::account::Account {
            program_owner: host_program(),
            data: lee_core::account::Data::try_from(vec![1u8, 2, 3]).expect("data"),
            ..lee_core::account::Account::default()
        },
    );
    assert_err(w.liquidate(fake, 0), "WrongAccount");
    // A real position paired with another position's vault (index 10 = host_vault).
    w.holding(k::KEEPER_COLL, collateral_def(), 0);
    let mut accounts = w.liquidate_accounts(pos, 0, id(k::KEEPER_COLL));
    accounts[10] = host_vault(small);
    assert_err(w.engine(accounts, &[k::KEEPER], Ix::Liquidate { nonce: 0 }), "WrongAccount");
}
```

Append to the `tests` module of `programs/auction_engine/core/src/lib.rs`: `#[test] fn liquidate_index() { assert_eq!(idx(&Instruction::Liquidate { nonce: 0 }), 4); }`

- [ ] **Step 2: Run and see the failure**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p engine_integration_tests --test liquidate'`
Expected: compile error: no variant `Liquidate` on `Instruction`.

- [ ] **Step 3: Append the variant**

```rust
    /// Accounts: keeper(s), keeper_collateral_holding(m), config(m), coll_config(m), position(m), record(m),
    /// auction(i,m), auction_vault(m), engine_authority, liquidation_engine_state, host_vault(m),
    /// collateral_definition, stability_fee_accumulator, redemption_price_state, oracle, protocol_parameters, clock.
    Liquidate { nonce: u64 },
```

- [ ] **Step 4: Write `liquidate.rs`**

```rust
//! `liquidate(nonce)` (spec §5.6 row 2, §5.9): observe → quote (caps, reward) → record nonce →
//! open the auction and reserve → chained: init vault, host hook (sweeps the whole host vault into
//! the auction vault), keeper reward. Hook arguments come only from the position read here
//! (ADR-019 invariant iii); `ENGINE_AUTH` seeds go only to `cfg.host_program_id` (invariant i).

use auction_core::{
    auction::Auction,
    liquidation::{quote_liquidation, CapState, LiquidationInput},
    math::mul_div_ceil,
    price::bid_price,
    units::RAY,
};
use auction_engine_core::{pda, state::*, EngineError};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use stablecoin_core::{
    compute_liquidation_engine_state_pda, compute_position_pda, compute_position_vault_pda,
    compute_stability_fee_accumulator_pda, math::compute_current_accumulated_rate_wide, Position,
    ProtocolParameters, StabilityFeeAccumulator,
};

use crate::{accounts::*, feed, token_calls, EngineResult};

pub struct LiquidateAccounts {
    pub keeper: AccountWithMetadata,
    pub keeper_collateral_holding: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub coll_config: AccountWithMetadata,
    pub position: AccountWithMetadata,
    pub record: AccountWithMetadata,
    pub auction: AccountWithMetadata,
    pub auction_vault: AccountWithMetadata,
    pub engine_authority: AccountWithMetadata,
    pub liquidation_engine_state: AccountWithMetadata,
    pub host_vault: AccountWithMetadata,
    pub collateral_definition: AccountWithMetadata,
    pub stability_fee_accumulator: AccountWithMetadata,
    pub redemption_price_state: AccountWithMetadata,
    pub oracle: AccountWithMetadata,
    pub protocol_parameters: AccountWithMetadata,
    pub clock: AccountWithMetadata,
}

fn host_owned(acc: &AccountWithMetadata, cfg: &EngineConfigState) -> Result<(), EngineError> {
    if acc.account.program_owner == cfg.host_program_id { Ok(()) } else { Err(EngineError::HostMismatch) }
}

/// ProtocolParameters, accumulator (→ current accumulated rate) and the hook state, all checked
/// against the bound host (confused-deputy invariant iii).
fn load_host(cfg: &EngineConfigState, a: &LiquidateAccounts, now: u64) -> Result<(ProtocolParameters, u128), EngineError> {
    expect_id(&a.protocol_parameters, &cfg.host_protocol_parameters_id, "protocol_parameters")?;
    host_owned(&a.protocol_parameters, cfg)?;
    let params = ProtocolParameters::try_from(&a.protocol_parameters.account.data).map_err(|_| EngineError::HostMismatch)?;
    expect_id(&a.stability_fee_accumulator, &compute_stability_fee_accumulator_pda(cfg.host_program_id), "stability_fee_accumulator")?;
    host_owned(&a.stability_fee_accumulator, cfg)?;
    let acc = StabilityFeeAccumulator::try_from(&a.stability_fee_accumulator.account.data).map_err(|_| EngineError::HostMismatch)?;
    expect_id(&a.liquidation_engine_state, &compute_liquidation_engine_state_pda(cfg.host_program_id), "liquidation_engine_state")?;
    let rate = feed::u512_to_u128(compute_current_accumulated_rate_wide(
        acc.accumulated_rate_at_last_accrual,
        params.stability_fee_per_millisecond,
        acc.last_accrued_at,
        now,
    ))?;
    Ok((params, rate))
}

fn load_position(cfg: &EngineConfigState, a: &LiquidateAccounts) -> Result<Position, EngineError> {
    host_owned(&a.position, cfg)?;
    let pos = Position::try_from(&a.position.account.data).map_err(|_| EngineError::WrongAccount("position"))?;
    if a.position.account_id != compute_position_pda(cfg.host_program_id, pos.owner_account_id, pos.position_nonce) {
        return Err(EngineError::WrongAccount("position"));
    }
    expect_id(&a.host_vault, &compute_position_vault_pda(cfg.host_program_id, a.position.account_id), "host_vault")?;
    if pos.vault_account_id != a.host_vault.account_id {
        return Err(EngineError::WrongAccount("host_vault"));
    }
    Ok(pos)
}

/// `LiquidationRecord` created on first use; `nonce == count`, then `count += 1` (ADR-004, ADR-020:
/// only auction creation increments the counter).
fn bump_record(program_id: &ProgramId, config_id: &AccountId, position_id: &AccountId, record: &AccountWithMetadata, nonce: u64) -> Result<AccountPostState, EngineError> {
    expect_id(record, &pda::liqrec_pda(program_id, config_id, position_id), "record")?;
    let current = if record.account == Account::default() {
        LiquidationRecordState { tag: TAG_LIQREC, position: *position_id, count: 0 }
    } else {
        if record.account.program_owner != *program_id {
            return Err(EngineError::WrongAccount("record"));
        }
        let s = LiquidationRecordState::try_from(&record.account.data).map_err(|_| EngineError::WrongAccount("record"))?;
        if s.position != *position_id {
            return Err(EngineError::WrongAccount("record"));
        }
        s
    };
    if nonce != current.count {
        return Err(EngineError::NonceMismatch);
    }
    let next = LiquidationRecordState { count: current.count.checked_add(1).ok_or(EngineError::Math)?, ..current };
    Ok(write_or_create(record, &next, pda::liqrec_seed(config_id, position_id)))
}

pub fn liquidate(program_id: ProgramId, a: LiquidateAccounts, nonce: u64) -> EngineResult {
    if !a.keeper.is_authorized {
        return Err(EngineError::WrongAccount("keeper"));
    }
    let now = read_clock(&a.clock)?;
    let mut cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let coll = load_coll(&program_id, &config_id, &cfg, &a.coll_config)?;
    let (host_params, acc_rate) = load_host(&cfg, &a, now)?;
    if coll.oracle_id != host_params.market_price_oracle_id {
        return Err(EngineError::OracleMismatch); // ADR-028
    }
    let r = feed::redemption_price(&cfg, &a.redemption_price_state, now)?;
    let obs = feed::observe(&coll, &a.oracle, r, now)?;
    let m = feed::require_live(&coll, &obs)?;
    let mut coll = coll.with_pause_clock(obs.clock);

    let pos = load_position(&cfg, &a)?;
    let c = fungible_balance(&a.host_vault, &cfg.token_program_id, &cfg.collateral_definition_id)?;
    let d = mul_div_ceil(pos.normalized_debt_amount, acc_rate, RAY)?; // nominal debt, owed → up
    let p = bid_price(m, r, &coll.feed_params()?);
    let params = coll.collateral_params()?;
    let caps = CapState {
        type_cap: coll.type_cap,
        type_reserved: coll.type_reserved,
        global_cap: cfg.global_cap,
        global_reserved: cfg.global_reserved,
    };
    let q = quote_liquidation(
        &LiquidationInput { position_collateral: pos.collateral_amount, vault_balance: c, nominal_debt: d, price_ray: p },
        &params,
        &caps,
    )?;

    let record_post = bump_record(&program_id, &config_id, &a.position.account_id, &a.record, nonce)?;
    expect_id(&a.auction, &pda::auction_pda(&program_id, &config_id, &a.position.account_id, nonce), "auction")?;
    expect_default(&a.auction, "auction")?;
    expect_id(&a.auction_vault, &pda::vault_pda(&program_id, &a.auction.account_id), "auction_vault")?;
    expect_default(&a.auction_vault, "auction_vault")?;
    expect_id(&a.engine_authority, &pda::engine_authority_pda(&program_id, &config_id), "engine_authority")?;
    expect_id(&a.collateral_definition, &cfg.collateral_definition_id, "collateral_definition")?;
    if q.keeper_reward > 0 {
        check_recipient(&a.keeper_collateral_holding, &cfg.token_program_id, &cfg.collateral_definition_id)?;
    }

    // Effects.
    let opened = Auction::open(&q, now, params.schedule, coll.paused_total_ms);
    let identity = AuctionState {
        tag: TAG_AUCTION,
        config: config_id,
        position: a.position.account_id,
        nonce,
        owner: pos.owner_account_id,
        keeper: a.keeper.account_id,
        settled: false,
        collateral_seized: 0,
        collateral_sold: 0,
        collateral_left: 0,
        keeper_reward: 0,
        leftover_claimable: 0,
        leftover_claimed: 0,
        debt_principal: 0,
        amount_to_raise: 0,
        reserved_amount: 0,
        debt_raised: 0,
        penalty_raised: 0,
        start_ms: 0,
        schedule: vec![],
        type_paused_total_at_start: 0,
    };
    let auction_state = AuctionState::from_core(&identity, &opened);
    coll.type_reserved = coll.type_reserved.checked_add(q.reserve).ok_or(EngineError::Math)?;
    cfg.global_reserved = cfg.global_reserved.checked_add(q.reserve).ok_or(EngineError::Math)?;

    // Chained calls (Shared Reference R3).
    let token = cfg.token_program_id;
    let init_vault = token_calls::initialize_account(
        token,
        a.collateral_definition.clone(),
        token_calls::authorized(&a.auction_vault),
        vec![pda::vault_seed(&a.auction.account_id)],
    );
    let custody = token_calls::post_init(&a.auction_vault, token, cfg.collateral_definition_id);
    let hook = ChainedCall::new(
        cfg.host_program_id,
        vec![
            token_calls::authorized(&a.engine_authority),
            a.liquidation_engine_state.clone(),
            a.position.clone(),
            a.host_vault.clone(),
            custody.clone(),
            a.stability_fee_accumulator.clone(),
            a.redemption_price_state.clone(),
            a.oracle.clone(),
            a.protocol_parameters.clone(),
            a.clock.clone(),
        ],
        &stablecoin_core::Instruction::LiquidatePosition {
            collateral_to_seize: pos.collateral_amount,
            normalized_debt_to_cover: pos.normalized_debt_amount,
        },
    )
    .with_pda_seeds(vec![pda::engine_authority_seed(&config_id)]);
    let mut calls = vec![init_vault, hook];
    if q.keeper_reward > 0 {
        // Full seizure: the hook moved the whole host vault balance `c` into custody.
        let swept = token_calls::authorized(&token_calls::with_balance(&custody, cfg.collateral_definition_id, c));
        calls.push(token_calls::transfer(
            token,
            swept,
            a.keeper_collateral_holding.clone(),
            q.keeper_reward,
            vec![pda::vault_seed(&a.auction.account_id)],
        ));
    }
    let post = vec![
        echo(&a.keeper),
        echo(&a.keeper_collateral_holding),
        write(&a.config, &cfg),
        write(&a.coll_config, &coll),
        echo(&a.position),
        record_post,
        create(&a.auction, &auction_state, pda::auction_seed(&config_id, &a.position.account_id, nonce)),
        echo(&a.auction_vault),
        echo(&a.engine_authority),
        echo(&a.liquidation_engine_state),
        echo(&a.host_vault),
        echo(&a.collateral_definition),
        echo(&a.stability_fee_accumulator),
        echo(&a.redemption_price_state),
        echo(&a.oracle),
        echo(&a.protocol_parameters),
        echo(&a.clock),
    ];
    Ok((post, calls))
}
```

Add `pub mod liquidate;` to `lib.rs`.

- [ ] **Step 5: Append the guest function**

```rust
    #[instruction]
    #[allow(clippy::too_many_arguments, reason = "seventeen accounts (Shared Reference R2)")]
    pub fn liquidate(
        ctx: ProgramContext,
        #[account(signer)] keeper: AccountWithMetadata,
        #[account(mut)] keeper_collateral_holding: AccountWithMetadata,
        #[account(mut)] config: AccountWithMetadata,
        #[account(mut)] coll_config: AccountWithMetadata,
        #[account(mut)] position: AccountWithMetadata,
        #[account(mut, pda = [literal("LIQREC"), account("config"), account("position")])] record: AccountWithMetadata,
        #[account(init, pda = [literal("AUCTION"), account("config"), account("position"), arg("nonce")])]
        auction: AccountWithMetadata,
        #[account(mut, pda = [literal("VAULT"), account("auction")])] auction_vault: AccountWithMetadata,
        #[account(pda = [literal("ENGINE_AUTH"), account("config")])] engine_authority: AccountWithMetadata,
        liquidation_engine_state: AccountWithMetadata,
        #[account(mut)] host_vault: AccountWithMetadata,
        collateral_definition: AccountWithMetadata,
        stability_fee_accumulator: AccountWithMetadata,
        redemption_price_state: AccountWithMetadata,
        oracle: AccountWithMetadata,
        protocol_parameters: AccountWithMetadata,
        clock: AccountWithMetadata,
        nonce: u64,
    ) -> SpelResult {
        let a = engine::liquidate::LiquidateAccounts {
            keeper, keeper_collateral_holding, config, coll_config, position, record, auction, auction_vault,
            engine_authority, liquidation_engine_state, host_vault, collateral_definition,
            stability_fee_accumulator, redemption_price_state, oracle, protocol_parameters, clock,
        };
        finish(engine::liquidate::liquidate(ctx.self_program_id, a, nonce))
    }
```

- [ ] **Step 6: Regenerate the IDL and run the tests**

Run: the Task 9 Step 7 command with `--test liquidate`. Expected: `liquidate` 10 passed; all earlier suites still pass.

- [ ] **Step 7: Docs**

Append to `docs/architecture/programs.md`:

```markdown
## `liquidate(nonce)`

1. Observe the feed; revert if paused (ADR-032). The engine's oracle must still be the host's market oracle (ADR-028).
2. Read the position (owned by the bound host, at its PDA), the host vault's actual balance `c ≥ collateral_amount`, and the nominal debt `D = ceil(normalized × accumulator / RAY)` projected to `now`.
3. `quote_liquidation`: `amount_to_raise = ceil(D·(1e4+penalty)/1e4)`; caps with the oversized-alone rule; keeper reward `min(tip + chip·D, cap, penalty·D)` → collateral at the band-clamped price, capped at `c`.
4. `LiquidationRecord`: created on first use, `nonce == count`, `count += 1` (only here).
5. Open the `Auction` (schedule snapshot, `type_paused_total_at_start`), reserve on `CollateralConfig.type_reserved` and `EngineConfig.global_reserved`.
6. Chained: `InitializeAccount(auction_vault)` → `stablecoin.liquidate_position(full amounts)` signed by `["ENGINE_AUTH", config]` (the host checks health, seizes, sweeps the whole vault into the auction vault) → `Transfer(reward)` to the keeper's holding (fresh holdings must sign).

The host, not the engine, decides whether the position is unsafe (`is_position_collateralized` at the minimum ratio against the redemption price). A safe position reverts the whole transaction.
```

- [ ] **Step 8: Commit (controller — GPG signing on Windows)**

The implementer stages the files and reports; the controller commits from Git Bash.

```bash
git add programs/auction_engine programs/integration_tests idl/auction_engine-idl.json docs/architecture/programs.md
git commit -m "feat(engine): liquidate with caps, keeper reward, nonce record and host hook chain"
```

---

### Task 11: `bid` and `bid_with_settlement` (chain-end accounting, sweep on lot exhaustion)

**Files:**
- Create: `programs/auction_engine/src/bid.rs`
- Modify: `programs/auction_engine/src/lib.rs`, `programs/auction_engine/core/src/{instruction.rs,lib.rs}` (append `Bid` = 5, `BidWithSettlement` = 6), guest (append two functions), `idl/auction_engine-idl.json`
- Modify: `programs/integration_tests/tests/common/mod.rs` (append); Create: `programs/integration_tests/tests/bid.rs`
- Modify: `docs/architecture/programs.md`, `docs/design-log.md` (ADR-029, ADR-033)

**Interfaces:**
- Consumes: Task 2 (`Accounting::record_settled`); Tasks 6–10 (`AuctionState::{to_core, from_core}`, `AccountingState::{core, with_core}`, `accounts::*`, `token_calls::*`, `feed::*`, harness `World::{with_engine, unsafe_position, liquidate, transfer_tokens}`, `LIQ_*`); `auction_core::{auction::{quote_bid, apply_bid, BidRequest}, price::bid_price}`.
- Produces: `bid::{BidAccounts { …, accounting: Option<AccountWithMetadata> }, bid(ProgramId, BidAccounts, u128, u128) -> EngineResult}`; `Instruction::{Bid { coin_in_max: u128, min_collateral_out: u128 }, BidWithSettlement { coin_in_max: u128, min_collateral_out: u128 }}`; harness `World::{bidder, bid_accounts, bid, bid_settle}` and `liquidated_world() -> (World, AccountId)` (in `tests/common/mod.rs`).

- [ ] **Step 1: Write the failing tests**

Append to `programs/integration_tests/tests/common/mod.rs`:

```rust
impl World {
    /// Funds `stable` with stablecoin and makes sure `coll` exists as an empty collateral holding.
    pub fn bidder(&mut self, stable: u8, coll: u8, amount: u128) {
        self.fund_stable(stable, amount);
        if self.state.get_account_by_id(id(coll)) == Account::default() {
            self.holding(coll, collateral_def(), 0);
        }
    }

    pub fn bid_accounts(&self, pos: AccountId, nonce: u64, stable: u8, coll: u8, with_accounting: bool) -> Vec<AccountId> {
        let mut v = vec![
            id(stable),
            id(coll),
            config(),
            coll_config(),
            auction(pos, nonce),
            auction_vault(pos, nonce),
            buffer(),
            stable_def(),
            redemption_state(),
            oracle(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ];
        if with_accounting {
            v.push(accounting());
        }
        v
    }

    /// `bid` by the default bidder (no Accounting account; must not end the auction).
    pub fn bid(&mut self, pos: AccountId, coin_in_max: u128, min_out: u128) -> Result<(), LeeError> {
        let accounts = self.bid_accounts(pos, 0, k::BIDDER_STABLE, k::BIDDER_COLL, false);
        self.engine(accounts, &[k::BIDDER_STABLE], Ix::Bid { coin_in_max, min_collateral_out: min_out })
    }

    /// `bid_with_settlement` by the default bidder (lists Accounting; may end the auction).
    pub fn bid_settle(&mut self, pos: AccountId, coin_in_max: u128, min_out: u128) -> Result<(), LeeError> {
        let accounts = self.bid_accounts(pos, 0, k::BIDDER_STABLE, k::BIDDER_COLL, true);
        self.engine(accounts, &[k::BIDDER_STABLE], Ix::BidWithSettlement { coin_in_max, min_collateral_out: min_out })
    }
}

/// Engine configured, the standard unsafe position liquidated (nonce 0), bidder funded with 1_000_000.
pub fn liquidated_world() -> (World, AccountId) {
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    w.liquidate(pos, 0).expect("liquidate");
    w.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 1_000_000);
    (w, pos)
}

/// Underwater variant: position 500_000 / 600_000 opened at R = 0.5, then R re-anchored to 2.0
/// (oracle 2.0). Reward 50 coin = 100 collateral, lot 499_900; the lot binds at 244_951 coin.
pub fn underwater_world() -> (World, AccountId) {
    let mut w = World::with_engine();
    let pos = w.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 0, LIQ_COLLATERAL, LIQ_DEBT);
    w.set_redemption_price(2 * ONE);
    w.set_oracle(2 * ONE, w.now);
    w.liquidate(pos, 0).expect("liquidate");
    w.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 1_000_000);
    (w, pos)
}
```

`programs/integration_tests/tests/bid.rs`:

```rust
mod common;

use auction_engine_core::{
    args::CollateralArgs,
    state::{AccountingState, AuctionState, CollateralConfigState, EngineConfigState},
    Instruction as Ix,
};
use common::*;

fn auction_of(w: &World, pos: lee_core::account::AccountId) -> AuctionState {
    w.read(auction(pos, 0))
}

#[test]
fn partial_bid_burns_debt_first_and_leaves_accounting_alone() {
    let (mut w, pos) = liquidated_world();
    let supply_before = w.supply(stable_def());
    w.bid(pos, 100_000, 51_020).expect("partial bid");
    // floor(100_000 × 0.5 / 0.98) = 51_020; the whole charge is debt (debt first).
    let a = auction_of(&w, pos);
    assert_eq!((a.debt_raised, a.penalty_raised, a.collateral_left, a.settled), (100_000, 0, LIQ_LOT - 51_020, false));
    assert_eq!(w.balance(id(k::BIDDER_COLL)), 51_020);
    assert_eq!(w.balance(id(k::BIDDER_STABLE)), 900_000);
    assert_eq!(w.supply(stable_def()), supply_before - 100_000);
    assert_eq!(w.balance(buffer()), 0);
    assert_eq!(w.read::<AccountingState>(accounting()), AccountingState::new(), "S6: no per-bid Accounting");
}

#[test]
fn final_bid_reaches_target_and_settles_once() {
    let (mut w, pos) = liquidated_world();
    w.bid(pos, 100_000, 0).unwrap();
    w.bid_settle(pos, 100_000, 0).expect("non-final bid_with_settlement is fine");
    assert_eq!(w.read::<AccountingState>(accounting()), AccountingState::new());
    w.bid_settle(pos, 460_000, 0).expect("final bid");
    let a = auction_of(&w, pos);
    assert!(a.settled);
    assert_eq!((a.debt_raised, a.penalty_raised), (LIQ_DEBT, 60_000));
    assert_eq!(a.leftover_claimable, a.collateral_left);
    let acc: AccountingState = w.read(accounting());
    assert_eq!((acc.total_debt_burned, acc.total_penalty_collected, acc.unbacked_debt), (LIQ_DEBT, 60_000, 0));
    assert_eq!(w.balance(buffer()), 60_000);
    assert_eq!(w.read::<CollateralConfigState>(coll_config()).type_reserved, 0);
    assert_eq!(w.read::<EngineConfigState>(config()).global_reserved, 0);
    assert_err(w.bid_settle(pos, 1_000, 0), "NotAuctioning");
}

#[test]
fn bid_that_turns_final_without_accounting_reverts() {
    // Review Focus 1 / ADR-029.
    let (mut w, pos) = liquidated_world();
    assert_err(w.bid(pos, LIQ_TARGET, 0), "ChainEndAccountsMissing");
    assert_eq!(auction_of(&w, pos).debt_raised, 0, "nothing applied");
    w.bid_settle(pos, LIQ_TARGET, 0).expect("same bid with the chain-end account");
    assert!(auction_of(&w, pos).settled);
}

#[test]
fn max_coin_in_charges_only_remaining() {
    // Review Focus 4.
    let (mut w, pos) = liquidated_world();
    w.bid_settle(pos, u128::MAX, 0).expect("u128::MAX coin_in_max");
    assert_eq!(w.balance(id(k::BIDDER_STABLE)), 1_000_000 - LIQ_TARGET);
    assert!(auction_of(&w, pos).settled);
}

#[test]
fn lot_binds_charges_only_for_the_lot_and_books_the_shortfall() {
    let (mut w, pos) = underwater_world();
    w.bid_settle(pos, LIQ_TARGET, 0).expect("lot-binding bid");
    let a = auction_of(&w, pos);
    // charged = ceil(499_900 × 0.98 / 2) = 244_951; shortfall = 600_000 − 244_951.
    assert_eq!((a.debt_raised, a.collateral_left, a.leftover_claimable, a.settled), (244_951, 0, 0, true));
    assert_eq!(w.balance(id(k::BIDDER_STABLE)), 1_000_000 - 244_951);
    assert_eq!(w.balance(id(k::BIDDER_COLL)), 499_900);
    let acc: AccountingState = w.read(accounting());
    assert_eq!((acc.unbacked_debt, acc.total_shortfall_booked), (355_049, 355_049));
}

#[test]
fn lot_emptying_bid_sweeps_donation() {
    // Review Focus 3 / ADR-033.
    let (mut w, pos) = underwater_world();
    w.fund_collateral(k::DONOR_COLL, 7);
    w.transfer_tokens(k::DONOR_COLL, auction_vault(pos, 0), 7).unwrap();
    w.bid_settle(pos, LIQ_TARGET, 0).unwrap();
    assert_eq!(w.balance(id(k::BIDDER_COLL)), 499_907);
    assert_eq!(w.balance(auction_vault(pos, 0)), 0);
}

#[test]
fn dust_min_bid_and_slippage_are_rejected() {
    let (mut w, pos) = liquidated_world();
    assert_err(w.bid(pos, 10, 0), "BelowMinBid");
    assert_err(w.bid(pos, LIQ_TARGET - 50, 0), "DustRemainder");
    assert_err(w.bid(pos, 100_000, 51_021), "Slippage");
}

#[test]
fn fresh_bidder_holding_must_sign() {
    // Review Focus 2.
    let (mut w, pos) = liquidated_world();
    w.fund_stable(k::BIDDER2_STABLE, 200_000);
    let accounts = w.bid_accounts(pos, 0, k::BIDDER2_STABLE, k::BIDDER2_COLL, false);
    let ix = || Ix::Bid { coin_in_max: 100_000, min_collateral_out: 0 };
    assert_err(w.engine(accounts.clone(), &[k::BIDDER2_STABLE], ix()), "RecipientMustSign");
    w.engine(accounts, &[k::BIDDER2_STABLE, k::BIDDER2_COLL], ix()).expect("fresh holding signs");
    assert_eq!(w.balance(id(k::BIDDER2_COLL)), 51_020);
}

#[test]
fn holding_of_wrong_definition_is_rejected() {
    let (mut w, pos) = liquidated_world();
    w.holding(k::BIDDER2_COLL, stable_def(), 0);
    let accounts = w.bid_accounts(pos, 0, k::BIDDER_STABLE, k::BIDDER2_COLL, false);
    assert_err(
        w.engine(accounts, &[k::BIDDER_STABLE], Ix::Bid { coin_in_max: 100_000, min_collateral_out: 0 }),
        "WrongDefinition",
    );
}

#[test]
fn discount_frozen_while_paused() {
    // Review Focus 5 / invariant 14: 1_000_000 ms of admin pause add no discount.
    let (mut w, pos) = liquidated_world();
    w.advance(1_350_000);
    let paused = CollateralArgs { admin_paused: true, ..CollateralArgs::defaults_for_tests() };
    w.set_collateral_config(k::ENGINE_ADMIN, &paused).unwrap();
    w.advance(1_000_000);
    assert_err(w.bid(pos, 100_000, 0), "Paused");
    w.set_collateral_config(k::ENGINE_ADMIN, &CollateralArgs::defaults_for_tests()).unwrap();
    w.bid(pos, 100_000, 0).unwrap();
    // t_eff = 1_350_000 → 6% → floor(100_000 × 0.5 / 0.94) = 53_191 (not 54_920 at 8.96%).
    assert_eq!(w.balance(id(k::BIDDER_COLL)), 53_191);
}

#[test]
fn bid_rejects_invalid_feed() {
    // Review Focus 5 (I6): stale, low-confidence and out-of-range feeds all stop bids; nothing is applied.
    for (needle, case) in [("PriceFeedStale", 0u8), ("PriceFeedLowConfidence", 1), ("PriceFeedOutOfRange", 2)] {
        let (mut w, pos) = liquidated_world();
        match case {
            0 => w.advance(3_600_001),
            1 => {
                w.advance(1_000);
                w.set_oracle_full(R, w.now, R / 50);
            }
            _ => {
                w.advance(1_000);
                w.set_oracle(3 * R, w.now);
            }
        }
        assert_err(w.bid_settle(pos, 100_000, 0), needle);
        let a = auction_of(&w, pos);
        assert_eq!((a.debt_raised, a.collateral_left), (0, LIQ_LOT), "{needle}: auction unchanged");
        assert_eq!(w.balance(id(k::BIDDER_STABLE)), 1_000_000, "{needle}: nothing charged");
    }
}

#[test]
fn unsold_auction_holds_max_discount_and_keeps_its_reservation() {
    let (mut w, pos) = liquidated_world();
    w.advance(36_000_000);
    w.set_oracle(R, w.now);
    w.bid(pos, 100_000, 0).unwrap();
    // No expiry: 10% max discount → floor(100_000 × 0.5 / 0.9) = 55_555.
    assert_eq!(w.balance(id(k::BIDDER_COLL)), 55_555);
    assert_eq!(w.read::<CollateralConfigState>(coll_config()).type_reserved, LIQ_TARGET);
    assert!(!auction_of(&w, pos).settled);
}

#[test]
fn schedule_change_affects_only_new_auctions() {
    let (mut w, pos) = liquidated_world();
    let flat = CollateralArgs { schedule: vec![0, 3_000], ..CollateralArgs::defaults_for_tests() };
    w.set_collateral_config(k::ENGINE_ADMIN, &flat).unwrap();
    w.bid(pos, 100_000, 0).unwrap();
    assert_eq!(w.balance(id(k::BIDDER_COLL)), 51_020, "snapshot 2%, not 30%");
}
```

Append to the core `tests` module:

```rust
    #[test]
    fn bid_indices() {
        assert_eq!(idx(&Instruction::Bid { coin_in_max: 0, min_collateral_out: 0 }), 5);
        assert_eq!(idx(&Instruction::BidWithSettlement { coin_in_max: 0, min_collateral_out: 0 }), 6);
    }
```

- [ ] **Step 2: Run and see the failure**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p engine_integration_tests --test bid'`
Expected: compile error: no variant `Bid` / `BidWithSettlement`.

- [ ] **Step 3: Append the variants**

```rust
    /// Accounts: bidder_stable_holding(s,m), bidder_collateral_holding(m), config, coll_config(m), auction(m),
    /// auction_vault(m), buffer(m), stablecoin_definition(m), redemption_price_state, oracle, clock.
    /// Reverts with `ChainEndAccountsMissing` if the fill would end the auction (ADR-029).
    Bid { coin_in_max: u128, min_collateral_out: u128 },
    /// Accounts: as `Bid`, then accounting(m). May end the auction (chain-end effects applied once).
    BidWithSettlement { coin_in_max: u128, min_collateral_out: u128 },
```

- [ ] **Step 4: Write `bid.rs`**

```rust
//! `bid` / `bid_with_settlement` (spec §5.6 row 3). Debt first: Burn the debt part from the bidder →
//! Transfer the penalty part to the buffer → Transfer collateral to the bidder (ADR-010). Chain end
//! (target reached or lot exhausted) needs the Accounting account (ADR-023, ADR-029). A lot-emptying
//! fill pays the whole vault balance, sweeping donations (ADR-033).

use auction_core::{
    auction::{apply_bid, quote_bid, BidRequest},
    price::bid_price,
};
use auction_engine_core::{pda, state::*, EngineError};
use lee_core::{account::AccountWithMetadata, program::ProgramId};

use crate::{accounts::*, feed, token_calls, EngineResult};

pub struct BidAccounts {
    pub bidder_stable_holding: AccountWithMetadata,
    pub bidder_collateral_holding: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub coll_config: AccountWithMetadata,
    pub auction: AccountWithMetadata,
    pub auction_vault: AccountWithMetadata,
    pub buffer: AccountWithMetadata,
    pub stablecoin_definition: AccountWithMetadata,
    pub redemption_price_state: AccountWithMetadata,
    pub oracle: AccountWithMetadata,
    pub clock: AccountWithMetadata,
    /// `Some` only for `bid_with_settlement`.
    pub accounting: Option<AccountWithMetadata>,
}

pub fn bid(program_id: ProgramId, a: BidAccounts, coin_in_max: u128, min_collateral_out: u128) -> EngineResult {
    if !a.bidder_stable_holding.is_authorized {
        return Err(EngineError::WrongAccount("bidder_stable_holding"));
    }
    let now = read_clock(&a.clock)?;
    let mut cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let coll = load_coll(&program_id, &config_id, &cfg, &a.coll_config)?;
    let st = load_auction(&program_id, &config_id, &a.auction)?;
    let current = st.to_core()?;

    // Observation precedes every t_eff use (ADR-024).
    let r = feed::redemption_price(&cfg, &a.redemption_price_state, now)?;
    let obs = feed::observe(&coll, &a.oracle, r, now)?;
    let m = feed::require_live(&coll, &obs)?;
    let mut coll = coll.with_pause_clock(obs.clock);
    let params = coll.collateral_params()?;
    let p = bid_price(m, r, &coll.feed_params()?);
    let t_eff = obs.clock.effective_elapsed(current.start_ms, current.type_paused_total_at_start, now);
    let fill = quote_bid(
        &current,
        &BidRequest { coin_in_max, min_collateral_out },
        p,
        t_eff,
        params.min_bid,
        params.chost()?,
    )?;
    let (next, end) = apply_bid(&current, &fill);

    let token = cfg.token_program_id;
    expect_id(&a.stablecoin_definition, &cfg.stablecoin_definition_id, "stablecoin_definition")?;
    expect_id(&a.buffer, &pda::buffer_pda(&program_id, &config_id), "buffer")?;
    fungible_balance(&a.buffer, &token, &cfg.stablecoin_definition_id)?;
    expect_id(&a.auction_vault, &pda::vault_pda(&program_id, &a.auction.account_id), "auction_vault")?;
    let vault_balance = fungible_balance(&a.auction_vault, &token, &cfg.collateral_definition_id)?;
    if vault_balance < fill.collateral_out {
        return Err(EngineError::VaultBelowBook);
    }
    let stable_balance = fungible_balance(&a.bidder_stable_holding, &token, &cfg.stablecoin_definition_id)?;
    if stable_balance < fill.charged {
        return Err(EngineError::InsufficientBalance);
    }
    check_recipient(&a.bidder_collateral_holding, &token, &cfg.collateral_definition_id)?;

    // Chain end: fold the cumulative raise and book the shortfall once, release the reservation.
    let accounting_post = match (end, a.accounting.as_ref()) {
        (Some(_), None) => return Err(EngineError::ChainEndAccountsMissing),
        (Some(e), Some(acc_meta)) => {
            let acc = load_accounting(&program_id, &config_id, acc_meta)?;
            coll.type_reserved = coll.type_reserved.checked_sub(e.release).ok_or(EngineError::Math)?;
            cfg.global_reserved = cfg.global_reserved.checked_sub(e.release).ok_or(EngineError::Math)?;
            Some(write(acc_meta, &AccountingState::with_core(&acc, &acc.core().record_settled(&next, &e))))
        }
        (None, Some(acc_meta)) => {
            load_accounting(&program_id, &config_id, acc_meta)?;
            Some(echo(acc_meta))
        }
        (None, None) => None,
    };

    let lot_empties = fill.collateral_out == current.collateral_left;
    let collateral_paid = if lot_empties { vault_balance } else { fill.collateral_out };
    let stable_after_burn = stable_balance.checked_sub(fill.debt_part).ok_or(EngineError::Math)?;

    let mut calls = Vec::with_capacity(3);
    if fill.debt_part > 0 {
        calls.push(token_calls::burn(token, a.stablecoin_definition.clone(), a.bidder_stable_holding.clone(), fill.debt_part, vec![]));
    }
    if fill.penalty_part > 0 {
        calls.push(token_calls::transfer(
            token,
            token_calls::with_balance(&a.bidder_stable_holding, cfg.stablecoin_definition_id, stable_after_burn),
            a.buffer.clone(),
            fill.penalty_part,
            vec![],
        ));
    }
    calls.push(token_calls::transfer(
        token,
        token_calls::authorized(&a.auction_vault),
        a.bidder_collateral_holding.clone(),
        collateral_paid,
        vec![pda::vault_seed(&a.auction.account_id)],
    ));

    let mut post = vec![
        echo(&a.bidder_stable_holding),
        echo(&a.bidder_collateral_holding),
        write(&a.config, &cfg),
        write(&a.coll_config, &coll),
        write(&a.auction, &AuctionState::from_core(&st, &next)),
        echo(&a.auction_vault),
        echo(&a.buffer),
        echo(&a.stablecoin_definition),
        echo(&a.redemption_price_state),
        echo(&a.oracle),
        echo(&a.clock),
    ];
    if let Some(p) = accounting_post {
        post.push(p);
    }
    Ok((post, calls))
}
```

Add `pub mod bid;` to `lib.rs`.

- [ ] **Step 5: Append the guest functions**

```rust
    #[instruction]
    #[allow(clippy::too_many_arguments, reason = "eleven accounts (Shared Reference R2)")]
    pub fn bid(
        ctx: ProgramContext,
        #[account(mut, signer)] bidder_stable_holding: AccountWithMetadata,
        #[account(mut)] bidder_collateral_holding: AccountWithMetadata,
        config: AccountWithMetadata,
        #[account(mut)] coll_config: AccountWithMetadata,
        #[account(mut)] auction: AccountWithMetadata,
        #[account(mut, pda = [literal("VAULT"), account("auction")])] auction_vault: AccountWithMetadata,
        #[account(mut, pda = [literal("BUFFER"), account("config")])] buffer: AccountWithMetadata,
        #[account(mut)] stablecoin_definition: AccountWithMetadata,
        redemption_price_state: AccountWithMetadata,
        oracle: AccountWithMetadata,
        clock: AccountWithMetadata,
        coin_in_max: u128,
        min_collateral_out: u128,
    ) -> SpelResult {
        let a = engine::bid::BidAccounts {
            bidder_stable_holding, bidder_collateral_holding, config, coll_config, auction, auction_vault,
            buffer, stablecoin_definition, redemption_price_state, oracle, clock, accounting: None,
        };
        finish(engine::bid::bid(ctx.self_program_id, a, coin_in_max, min_collateral_out))
    }

    #[instruction]
    #[allow(clippy::too_many_arguments, reason = "twelve accounts (Shared Reference R2)")]
    pub fn bid_with_settlement(
        ctx: ProgramContext,
        #[account(mut, signer)] bidder_stable_holding: AccountWithMetadata,
        #[account(mut)] bidder_collateral_holding: AccountWithMetadata,
        #[account(mut)] config: AccountWithMetadata,
        #[account(mut)] coll_config: AccountWithMetadata,
        #[account(mut)] auction: AccountWithMetadata,
        #[account(mut, pda = [literal("VAULT"), account("auction")])] auction_vault: AccountWithMetadata,
        #[account(mut, pda = [literal("BUFFER"), account("config")])] buffer: AccountWithMetadata,
        #[account(mut)] stablecoin_definition: AccountWithMetadata,
        redemption_price_state: AccountWithMetadata,
        oracle: AccountWithMetadata,
        clock: AccountWithMetadata,
        #[account(mut, pda = [literal("ACCT"), account("config")])] accounting: AccountWithMetadata,
        coin_in_max: u128,
        min_collateral_out: u128,
    ) -> SpelResult {
        let a = engine::bid::BidAccounts {
            bidder_stable_holding, bidder_collateral_holding, config, coll_config, auction, auction_vault,
            buffer, stablecoin_definition, redemption_price_state, oracle, clock, accounting: Some(accounting),
        };
        finish(engine::bid::bid(ctx.self_program_id, a, coin_in_max, min_collateral_out))
    }
```

- [ ] **Step 6: Regenerate the IDL and run the tests**

Run: the Task 9 Step 7 command with `--test bid`. Expected: `bid` 13 passed; earlier suites green.

- [ ] **Step 7: Docs, ADR-029, ADR-033**

Append to `docs/architecture/programs.md` a "`bid` / `bid_with_settlement`" section restating the module doc above, the account-choice rule for clients (Interfaces for Plan 3), and the sweep rule.

Append to `docs/design-log.md`:

```markdown
---

## ADR-029: Two Bid Instructions; a Bid That Turns Final Without `Accounting` Reverts

**Context**
ADR-023 (S6) updates `Accounting` only at chain end, and its consequence 6 asks Plan 2 to decide what happens when a bid signed without the chain-end accounts turns out to end the auction (price and `t_eff` move between quote and inclusion). In this design the reservation release writes `CollateralConfig` and `EngineConfig`, which every bid already lists (pause observation and validation), so `Accounting` is the only chain-end-only account.

**Options**
- (A) Revert with a distinct error so the client re-quotes with the account.
- (B) A permissionless `finalize` that applies chain-end effects later (an extra "settled, not finalized" state).
- (C) Always list `Accounting` (gives up the S6 saving).

**Decision**
(A), as two instructions sharing one handler: `bid` (no `Accounting`; reverts `ChainEndAccountsMissing` if the fill ends the auction) and `bid_with_settlement` (lists `Accounting`; any fill). Spec §5.5/§5.6 require chain-end effects "in the same transaction" and "exactly once"; (B) would let `unbacked_debt` lag a settled shortfall, which `check_solvency` and `start_debt_auction` read, and adds a state. Rest-account (`Vec<AccountWithMetadata>`) optional lists exist in the SPEL macro but were never exercised in the spikes; two fixed lists use only confirmed features and keep the IDL simple. Clients (Plan 3) use `bid_with_settlement` whenever `coin_in_max >= remaining_target` or the quoted fill is within the slippage margin of emptying the lot. If most bids end up listing `Accounting`, revisit S6 (Plan 3 `CYCLES.md`).

**Trade-offs**
A racing bid can revert and need a re-quote; nothing is applied when it does (test `bid_that_turns_final_without_accounting_reverts`).

**R3 deviation (M8).** Spec §5.4 lists a non-final bid's writes as its auction, vault, buffer and the bidder's holdings. Here every bid also writes `CollateralConfig` (the type-level pause clock observed before `t_eff`, spec §5.7) and lists `EngineConfig` (echoed unchanged unless the bid settles). Both are per-instance shared state by design; no other auction's data changes (test `r3_a_bid_touches_only_its_own_auction`).

**Status**
Accepted (Plan 2, Task 11)

---

## ADR-033: Sweep Rules for Engine-Held Holdings

**Context**
Anyone can raise any holding's balance (spec §3.2); spec §5.13 invariant 2 says terminal sweeps take the whole balance.

**Decision**
A fill that empties the lot transfers the whole auction-vault balance to the bidder; `claim_leftover` transfers the whole vault balance to the owner; `settle_debt` burns from the buffer's actual balance; `close_surplus_auction` returns the whole surplus-vault balance to the buffer. `settle_empty` (lot 0) moves nothing: any donation into such a vault stays there (the donor's loss, no protocol effect). Balances are only ever compared with `>=`.

**Trade-offs**
Donations go to whoever the sweep pays; they never change a booked number.

**Status**
Accepted (Plan 2, Task 11)
```

- [ ] **Step 8: Commit (controller — GPG signing on Windows)**

The implementer stages the files and reports; the controller commits from Git Bash.

```bash
git add programs/auction_engine programs/integration_tests idl/auction_engine-idl.json docs/architecture/programs.md docs/design-log.md
git commit -m "feat(engine): bid and bid_with_settlement with chain-end accounting and lot sweep; ADR-029, ADR-033"
```

---

### Task 12: `settle_empty`, `claim_leftover`, `settle_debt` (burn from an engine-PDA holding)

**Files:**
- Create: `programs/auction_engine/src/settle.rs`
- Modify: `programs/auction_engine/src/lib.rs`, `programs/auction_engine/core/src/{instruction.rs,lib.rs}` (append `SettleEmpty` = 7, `ClaimLeftover` = 8, `SettleDebt { amount: u128 }` = 9), guest (append three functions), `idl/auction_engine-idl.json`
- Modify: `programs/integration_tests/tests/common/mod.rs` (append); Create: `programs/integration_tests/tests/settle.rs`
- Modify: `docs/architecture/programs.md`

**Interfaces:**
- Consumes: Task 2 (`record_settled`); Tasks 6–11 (`accounts::*`, `token_calls::*`, `AuctionState`, `AccountingState`, harness `liquidated_world`, `underwater_world`, `World::{bid_settle, transfer_tokens, fund_stable, fund_collateral}`); `auction_core::auction::{settle_empty, claim_leftover}`; `auction_core::accounting::Accounting::settle_debt`.
- Produces: `settle::{SettleEmptyAccounts, settle_empty, ClaimLeftoverAccounts, claim_leftover, SettleDebtAccounts, settle_debt, implicit_settle, buffer_burn}`:
  - `pub fn implicit_settle(acc: &AccountingState, buffer_balance: u128) -> (AccountingState, u128)` — `settle_debt(u128::MAX, buffer_balance)`; returns the new state and the amount to burn. Used by Tasks 13–15.
  - `pub fn buffer_burn(cfg: &EngineConfigState, config_id: &AccountId, stablecoin_definition: &AccountWithMetadata, buffer: &AccountWithMetadata, burned: u128) -> Option<ChainedCall>` — `Token::Burn` from the buffer under its PDA seed, `None` when `burned == 0`.
  - Harness: `World::{settle_empty, claim_leftover, settle_debt, donate_stable}`.

- [ ] **Step 1: Write the failing tests**

Append to `programs/integration_tests/tests/common/mod.rs`:

```rust
impl World {
    pub fn settle_empty(&mut self, pos: AccountId) -> Result<(), LeeError> {
        self.engine(vec![id(k::CALLER), config(), coll_config(), accounting(), auction(pos, 0)], &[k::CALLER], Ix::SettleEmpty)
    }

    pub fn claim_leftover(&mut self, signer: u8, holding: u8, pos: AccountId) -> Result<(), LeeError> {
        self.engine(
            vec![id(signer), id(holding), config(), auction(pos, 0), auction_vault(pos, 0)],
            &[signer],
            Ix::ClaimLeftover,
        )
    }

    pub fn settle_debt(&mut self, amount: u128) -> Result<(), LeeError> {
        self.engine(vec![id(k::CALLER), config(), accounting(), buffer(), stable_def()], &[k::CALLER], Ix::SettleDebt { amount })
    }

    /// A third party raises the buffer's balance (stablecoin donation).
    pub fn donate_stable(&mut self, to: AccountId, amount: u128) {
        self.fund_stable(k::DONOR_STABLE, amount);
        self.transfer_tokens(k::DONOR_STABLE, to, amount).expect("donation");
    }
}
```

`programs/integration_tests/tests/settle.rs`:

```rust
mod common;

use auction_engine_core::state::{AccountingState, AuctionState, CollateralConfigState, EngineConfigState};
use common::*;

/// Reward consumes the whole seizure (Task 10 underwater case): lot 0, debt 1_000.
fn empty_lot_world() -> (World, lee_core::account::AccountId) {
    let mut w = World::with_engine();
    let pos = w.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 0, 1_000, 1_000);
    w.set_redemption_price(250 * ONE);
    w.set_oracle(250 * ONE, w.now);
    w.liquidate(pos, 0).unwrap();
    w.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 10_000);
    (w, pos)
}

#[test]
fn settle_empty_books_the_full_shortfall_and_releases_reservations() {
    let (mut w, pos) = empty_lot_world();
    assert_err(w.bid_settle(pos, 1_100, 0), "ZeroFill");
    w.settle_empty(pos).expect("settle_empty");
    let a: AuctionState = w.read(auction(pos, 0));
    assert!(a.settled);
    let acc: AccountingState = w.read(accounting());
    assert_eq!((acc.unbacked_debt, acc.total_shortfall_booked, acc.total_debt_burned), (1_000, 1_000, 0));
    assert_eq!(w.read::<CollateralConfigState>(coll_config()).type_reserved, 0);
    assert_eq!(w.read::<EngineConfigState>(config()).global_reserved, 0);
    assert_err(w.settle_empty(pos), "NotAuctioning");
}

#[test]
fn settle_empty_on_a_live_lot_is_rejected() {
    let (mut w, pos) = liquidated_world();
    assert_err(w.settle_empty(pos), "NotAuctioning");
}

#[test]
fn claim_leftover_pays_the_owner_once_and_sweeps() {
    let (mut w, pos) = liquidated_world();
    w.bid_settle(pos, LIQ_TARGET, 0).unwrap();
    // out = floor(660_000 × 0.5 / 0.98) = 336_734 → leftover = 499_975 − 336_734 = 163_241.
    w.fund_collateral(k::DONOR_COLL, 3);
    w.transfer_tokens(k::DONOR_COLL, auction_vault(pos, 0), 3).unwrap();
    assert_err(w.claim_leftover(k::ATTACKER, k::OWNER_COLL, pos), "NotOwner");
    let before = w.balance(id(k::OWNER_COLL));
    w.claim_leftover(k::OWNER, k::OWNER_COLL, pos).expect("owner claims");
    assert_eq!(w.balance(id(k::OWNER_COLL)), before + 163_244);
    assert_eq!(w.balance(auction_vault(pos, 0)), 0);
    assert_err(w.claim_leftover(k::OWNER, k::OWNER_COLL, pos), "NothingToClaim");
}

#[test]
fn settle_debt_uses_actual_buffer_balance() {
    // Review Focus 3 + S7 prerequisite: burn from an engine-PDA-held holding with seeds.
    let (mut w, pos) = underwater_world();
    w.bid_settle(pos, LIQ_TARGET, 0).unwrap();
    assert_eq!(w.read::<AccountingState>(accounting()).unbacked_debt, 355_049);
    w.settle_debt(u128::MAX).expect("nothing to burn is a no-op");
    w.donate_stable(buffer(), 1_000);
    let supply = w.supply(stable_def());
    w.settle_debt(500).expect("partial settle");
    assert_eq!(w.balance(buffer()), 500);
    assert_eq!(w.supply(stable_def()), supply - 500);
    let acc: AccountingState = w.read(accounting());
    assert_eq!((acc.unbacked_debt, acc.total_debt_settled), (354_549, 500));
}

#[test]
fn settle_debt_max_amount() {
    // Review Focus 4: u128::MAX burns min(buffer, unbacked − on_auction) and nothing more.
    let (mut w, pos) = underwater_world();
    w.bid_settle(pos, LIQ_TARGET, 0).unwrap();
    w.donate_stable(buffer(), 1_000);
    w.settle_debt(u128::MAX).unwrap();
    assert_eq!(w.balance(buffer()), 0);
    assert_eq!(w.read::<AccountingState>(accounting()).unbacked_debt, 354_049);
}
```

Append to the core `tests` module:

```rust
    #[test]
    fn settle_indices() {
        assert_eq!(idx(&Instruction::SettleEmpty), 7);
        assert_eq!(idx(&Instruction::ClaimLeftover), 8);
        assert_eq!(idx(&Instruction::SettleDebt { amount: 0 }), 9);
    }
```

- [ ] **Step 2: Run and see the failure**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p engine_integration_tests --test settle'`
Expected: compile error: no variants `SettleEmpty`, `ClaimLeftover`, `SettleDebt`.

- [ ] **Step 3: Append the variants**

```rust
    /// Accounts: caller(s), config(m), coll_config(m), accounting(m), auction(m). Settles a lot-0 auction.
    SettleEmpty,
    /// Accounts: owner(s), owner_collateral_holding(m), config, auction(m), auction_vault(m).
    ClaimLeftover,
    /// Accounts: caller(s), config, accounting(m), buffer(m), stablecoin_definition(m).
    SettleDebt { amount: u128 },
```

- [ ] **Step 4: Write `settle.rs`**

```rust
//! `settle_empty` (ADR-024), `claim_leftover` (pull model, owner check), `settle_debt` (burns from the
//! buffer's ACTUAL balance, never the debt a live debt auction is selling).

use auction_core::auction::{claim_leftover as core_claim, settle_empty as core_settle_empty};
use auction_engine_core::{pda, state::*, EngineError};
use lee_core::{
    account::{AccountId, AccountWithMetadata},
    program::{ChainedCall, ProgramId},
};

use crate::{accounts::*, token_calls, EngineResult};

pub struct SettleEmptyAccounts {
    pub caller: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub coll_config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    pub auction: AccountWithMetadata,
}

pub fn settle_empty(program_id: ProgramId, a: SettleEmptyAccounts) -> EngineResult {
    if !a.caller.is_authorized {
        return Err(EngineError::WrongAccount("caller"));
    }
    let mut cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let mut coll = load_coll(&program_id, &config_id, &cfg, &a.coll_config)?;
    let acc = load_accounting(&program_id, &config_id, &a.accounting)?;
    let st = load_auction(&program_id, &config_id, &a.auction)?;
    let (next, end) = core_settle_empty(&st.to_core()?)?;
    coll.type_reserved = coll.type_reserved.checked_sub(end.release).ok_or(EngineError::Math)?;
    cfg.global_reserved = cfg.global_reserved.checked_sub(end.release).ok_or(EngineError::Math)?;
    let folded = AccountingState::with_core(&acc, &acc.core().record_settled(&next, &end));
    Ok((
        vec![
            echo(&a.caller),
            write(&a.config, &cfg),
            write(&a.coll_config, &coll),
            write(&a.accounting, &folded),
            write(&a.auction, &AuctionState::from_core(&st, &next)),
        ],
        vec![],
    ))
}

pub struct ClaimLeftoverAccounts {
    pub owner: AccountWithMetadata,
    pub owner_collateral_holding: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub auction: AccountWithMetadata,
    pub auction_vault: AccountWithMetadata,
}

/// Pays `leftover_claimable − leftover_claimed` by sweeping the whole vault (ADR-033). Only the
/// position owner recorded at liquidation may claim (`NotOwner`).
pub fn claim_leftover(program_id: ProgramId, a: ClaimLeftoverAccounts) -> EngineResult {
    let cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let st = load_auction(&program_id, &config_id, &a.auction)?;
    if !a.owner.is_authorized || a.owner.account_id != st.owner {
        return Err(EngineError::NotOwner);
    }
    let (next, amount) = core_claim(&st.to_core()?)?;
    expect_id(&a.auction_vault, &pda::vault_pda(&program_id, &a.auction.account_id), "auction_vault")?;
    let token = cfg.token_program_id;
    let vault_balance = fungible_balance(&a.auction_vault, &token, &cfg.collateral_definition_id)?;
    if vault_balance < amount {
        return Err(EngineError::VaultBelowBook);
    }
    check_recipient(&a.owner_collateral_holding, &token, &cfg.collateral_definition_id)?;
    let pay = token_calls::transfer(
        token,
        token_calls::authorized(&a.auction_vault),
        a.owner_collateral_holding.clone(),
        vault_balance,
        vec![pda::vault_seed(&a.auction.account_id)],
    );
    Ok((
        vec![
            echo(&a.owner),
            echo(&a.owner_collateral_holding),
            echo(&a.config),
            write(&a.auction, &AuctionState::from_core(&st, &next)),
            echo(&a.auction_vault),
        ],
        vec![pay],
    ))
}

/// `settle_debt(MAX)` against the actual buffer balance; returns (new state, amount to burn).
#[must_use]
pub fn implicit_settle(acc: &AccountingState, buffer_balance: u128) -> (AccountingState, u128) {
    let (next, burned) = acc.core().settle_debt(u128::MAX, buffer_balance);
    (AccountingState::with_core(acc, &next), burned)
}

/// `Token::Burn(stablecoin, buffer)` authorised by the buffer's PDA seed; `None` if nothing burns.
#[must_use]
pub fn buffer_burn(
    cfg: &EngineConfigState,
    config_id: &AccountId,
    stablecoin_definition: &AccountWithMetadata,
    buffer: &AccountWithMetadata,
    burned: u128,
) -> Option<ChainedCall> {
    (burned > 0).then(|| {
        token_calls::burn(
            cfg.token_program_id,
            stablecoin_definition.clone(),
            token_calls::authorized(buffer),
            burned,
            vec![pda::buffer_seed(config_id)],
        )
    })
}

pub struct SettleDebtAccounts {
    pub caller: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    pub buffer: AccountWithMetadata,
    pub stablecoin_definition: AccountWithMetadata,
}

pub fn settle_debt(program_id: ProgramId, a: SettleDebtAccounts, amount: u128) -> EngineResult {
    if !a.caller.is_authorized {
        return Err(EngineError::WrongAccount("caller"));
    }
    let cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let acc = load_accounting(&program_id, &config_id, &a.accounting)?;
    expect_id(&a.buffer, &pda::buffer_pda(&program_id, &config_id), "buffer")?;
    expect_id(&a.stablecoin_definition, &cfg.stablecoin_definition_id, "stablecoin_definition")?;
    let balance = fungible_balance(&a.buffer, &cfg.token_program_id, &cfg.stablecoin_definition_id)?;
    let (core_next, burned) = acc.core().settle_debt(amount, balance);
    let next = AccountingState::with_core(&acc, &core_next);
    let calls = buffer_burn(&cfg, &config_id, &a.stablecoin_definition, &a.buffer, burned).into_iter().collect();
    Ok((
        vec![
            echo(&a.caller),
            echo(&a.config),
            write(&a.accounting, &next),
            echo(&a.buffer),
            echo(&a.stablecoin_definition),
        ],
        calls,
    ))
}
```

Add `pub mod settle;` to `lib.rs`.

- [ ] **Step 5: Append the guest functions**

```rust
    #[instruction]
    pub fn settle_empty(
        ctx: ProgramContext,
        #[account(signer)] caller: AccountWithMetadata,
        #[account(mut)] config: AccountWithMetadata,
        #[account(mut)] coll_config: AccountWithMetadata,
        #[account(mut, pda = [literal("ACCT"), account("config")])] accounting: AccountWithMetadata,
        #[account(mut)] auction: AccountWithMetadata,
    ) -> SpelResult {
        let a = engine::settle::SettleEmptyAccounts { caller, config, coll_config, accounting, auction };
        finish(engine::settle::settle_empty(ctx.self_program_id, a))
    }

    #[instruction]
    pub fn claim_leftover(
        ctx: ProgramContext,
        #[account(signer)] owner: AccountWithMetadata,
        #[account(mut)] owner_collateral_holding: AccountWithMetadata,
        config: AccountWithMetadata,
        #[account(mut)] auction: AccountWithMetadata,
        #[account(mut, pda = [literal("VAULT"), account("auction")])] auction_vault: AccountWithMetadata,
    ) -> SpelResult {
        let a = engine::settle::ClaimLeftoverAccounts { owner, owner_collateral_holding, config, auction, auction_vault };
        finish(engine::settle::claim_leftover(ctx.self_program_id, a))
    }

    #[instruction]
    pub fn settle_debt(
        ctx: ProgramContext,
        #[account(signer)] caller: AccountWithMetadata,
        config: AccountWithMetadata,
        #[account(mut, pda = [literal("ACCT"), account("config")])] accounting: AccountWithMetadata,
        #[account(mut, pda = [literal("BUFFER"), account("config")])] buffer: AccountWithMetadata,
        #[account(mut)] stablecoin_definition: AccountWithMetadata,
        amount: u128,
    ) -> SpelResult {
        let a = engine::settle::SettleDebtAccounts { caller, config, accounting, buffer, stablecoin_definition };
        finish(engine::settle::settle_debt(ctx.self_program_id, a, amount))
    }
```

- [ ] **Step 6: Regenerate the IDL and run the tests**

Run: the Task 9 Step 7 command with `--test settle`. Expected: `settle` 5 passed; earlier suites green.

- [ ] **Step 7: Docs**

Append to `docs/architecture/programs.md` a section "Settlement paths" listing the two chain-end paths (`bid_with_settlement` that ends the auction, `settle_empty`) and stating that both call `record_settled` exactly once and release `reserved_amount` from both caps; `claim_leftover` (owner only, sweeps the vault); `settle_debt` (burns `min(amount, buffer actual balance, unbacked − debt_on_auction)` under the buffer's PDA seed — the S7 prerequisite "burn from an engine-PDA-held holding", test `settle_debt_uses_actual_buffer_balance`).

- [ ] **Step 8: Commit (controller — GPG signing on Windows)**

The implementer stages the files and reports; the controller commits from Git Bash.

```bash
git add programs/auction_engine programs/integration_tests idl/auction_engine-idl.json docs/architecture/programs.md
git commit -m "feat(engine): settle_empty, claim_leftover and settle_debt (buffer burn under PDA seed)"
```

---

### Task 13: Mock protocol token and the F5 surplus auction (start / take / close, reference price)

**Files:**
- Create: `programs/auction_engine/src/system.rs`, `programs/auction_engine/src/surplus.rs`
- Modify: `programs/auction_engine/src/lib.rs`, `programs/auction_engine/core/src/{instruction.rs,lib.rs}` (append `StartSurplusAuction { nonce: u64 }` = 10, `TakeSurplus { amount_coin: u128, max_protocol_in: u128 }` = 11, `CloseSurplusAuction` = 12), guest (append three functions), `idl/auction_engine-idl.json`
- Modify: `programs/integration_tests/tests/common/mod.rs` (append); Create: `programs/integration_tests/tests/surplus.rs`
- Modify: `docs/architecture/programs.md`, `docs/design-log.md` (ADR-031)

**Interfaces:**
- Consumes: Task 12 (`settle::{implicit_settle, buffer_burn}`); Tasks 6–11 (`SurplusAuctionState::{core, with_core}`, `SystemParamsRec::to_core`, `accounts::{expect_slot, …}`, `token_calls::*`, harness); `auction_core::{system::{start_surplus, take_surplus, close_surplus}, price::{effective_ref_price, validate_feed, FeedParams, OracleReading, OracleScale, MAX_CONF_BPS}, error::PauseReason}`; `token_core::{TokenDefinition, Instruction::NewFungibleDefinition}`.
- Produces:
  - `system::{ref_price(&ProgramId, &AccountId, &EngineConfigState, &AccountWithMetadata, u64) -> Result<u128, EngineError>, TokenStatus { configured: bool, token_ok: bool, mint_authority_ok: bool }, protocol_token_status(&ProgramId, &AccountId, &EngineConfigState, &AccountWithMetadata) -> Result<TokenStatus, EngineError>}` (Tasks 14–15 use both).
  - `surplus::{StartSurplusAccounts, start_surplus_auction, TakeSurplusAccounts, take_surplus, CloseSurplusAccounts, close_surplus_auction}`.
  - Harness: `surplus_auction(nonce)`, `surplus_vault(nonce)`, `World::{create_protocol_token, set_ref_oracle, with_system, book_shortfall, start_surplus, take_surplus, close_surplus}`.

- [ ] **Step 1: Write the failing tests**

Append to `programs/integration_tests/tests/common/mod.rs`:

```rust
pub fn surplus_auction(nonce: u64) -> AccountId {
    pda::surplus_auction_pda(&engine_program(), &config(), nonce)
}
pub fn surplus_vault(nonce: u64) -> AccountId {
    pda::surplus_vault_pda(&engine_program(), &surplus_auction(nonce))
}

impl World {
    /// Real `Token::NewFungibleDefinition` of the mock protocol token: definition `id(k::PROTOCOL_DEF)`,
    /// initial supply to the taker's holding `id(k::TAKER_PROTOCOL)`, mint authority `authority`.
    pub fn create_protocol_token(&mut self, initial_supply: u128, authority: AccountId) -> Result<(), LeeError> {
        self.submit(
            token_program(),
            vec![protocol_token_def(), id(k::TAKER_PROTOCOL)],
            &[k::PROTOCOL_DEF, k::TAKER_PROTOCOL],
            token_core::Instruction::NewFungibleDefinition {
                name: String::from("MOCK"),
                total_supply: initial_supply,
                mint_authority: Some(authority),
            },
        )
    }

    /// The protocol-token reference oracle at `ref_oracle()` (coin per protocol token, RAY).
    pub fn set_ref_oracle(&mut self, price: u128, timestamp: u64) {
        self.state.force_insert_account(
            ref_oracle(),
            Account {
                program_owner: [9u32; 8],
                balance: 0,
                data: Data::from(&OraclePriceAccount {
                    base_asset: protocol_token_def(),
                    quote_asset: stable_def(),
                    price,
                    timestamp,
                    source_id: ref_oracle(),
                    confidence_interval: 0,
                }),
                nonce: Nonce(0),
            },
        );
    }

    /// Engine with the protocol token configured (ref price 2 coin/token), optionally with the
    /// reference oracle; the mock token exists with mint authority `["MINT_AUTH", config]` and 10_000
    /// tokens in the taker's holding; the taker's stablecoin holding exists (empty).
    pub fn with_system(ref_oracle_enabled: bool) -> Self {
        let mut w = Self::new();
        let p = EngineParamsArgs {
            protocol_token_enabled: true,
            ref_oracle_enabled,
            ..EngineParamsArgs::defaults_for_tests()
        };
        if ref_oracle_enabled {
            w.set_ref_oracle(2 * ONE, w.now);
        }
        w.init_engine_config(&p).expect("init_config");
        w.bind_engine().expect("bind");
        w.set_collateral_config(k::ENGINE_ADMIN, &CollateralArgs::defaults_for_tests()).expect("collateral");
        w.create_protocol_token(10_000, mint_authority()).expect("mock protocol token");
        w.holding(k::TAKER_STABLE, stable_def(), 0);
        w
    }

    /// Books a shortfall on a configured world: underwater position (R → 2.0), liquidated, lot sold.
    /// Leaves `unbacked_debt == 355_049` and the bidder with 1_000_000 − 244_951 stablecoin.
    pub fn book_shortfall(&mut self) -> AccountId {
        let pos = self.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 0, LIQ_COLLATERAL, LIQ_DEBT);
        self.set_redemption_price(2 * ONE);
        self.set_oracle(2 * ONE, self.now);
        self.liquidate(pos, 0).expect("liquidate");
        self.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 1_000_000);
        self.bid_settle(pos, LIQ_TARGET, 0).expect("lot-binding bid");
        pos
    }

    fn ref_slot(&self) -> AccountId {
        let cfg: auction_engine_core::state::EngineConfigState = self.read(config());
        cfg.protocol_token_oracle_id.unwrap_or_else(|| unused(SLOT_REF_ORACLE))
    }

    pub fn start_surplus(&mut self, nonce: u64) -> Result<(), LeeError> {
        let slot = self.ref_slot();
        self.engine(
            vec![id(k::CALLER), config(), accounting(), buffer(), stable_def(), surplus_auction(nonce), surplus_vault(nonce), slot, CLOCK_01_PROGRAM_ACCOUNT_ID],
            &[k::CALLER],
            Ix::StartSurplusAuction { nonce },
        )
    }

    pub fn take_surplus(&mut self, nonce: u64, amount_coin: u128, max_protocol_in: u128) -> Result<(), LeeError> {
        self.engine(
            vec![id(k::TAKER_PROTOCOL), id(k::TAKER_STABLE), config(), accounting(), surplus_auction(nonce), surplus_vault(nonce), protocol_token_def(), CLOCK_01_PROGRAM_ACCOUNT_ID],
            &[k::TAKER_PROTOCOL],
            Ix::TakeSurplus { amount_coin, max_protocol_in },
        )
    }

    pub fn close_surplus(&mut self, nonce: u64) -> Result<(), LeeError> {
        self.engine(
            vec![id(k::CALLER), config(), accounting(), surplus_auction(nonce), surplus_vault(nonce), buffer(), CLOCK_01_PROGRAM_ACCOUNT_ID],
            &[k::CALLER],
            Ix::CloseSurplusAuction,
        )
    }
}
```

`programs/integration_tests/tests/surplus.rs`:

```rust
mod common;

use auction_engine_core::{
    args::EngineParamsArgs,
    state::{AccountingState, SurplusAuctionState},
};
use common::*;

fn acc(w: &World) -> AccountingState {
    w.read(accounting())
}

#[test]
fn surplus_auction_burns_protocol_tokens() {
    let mut w = World::with_system(false);
    w.donate_stable(buffer(), 1_500);
    w.start_surplus(0).expect("start_surplus_auction");
    let sa: SurplusAuctionState = w.read(surplus_auction(0));
    assert_eq!((sa.lot_left, sa.live, sa.ref_ray), (500, true, 2 * ONE), "admin price, no oracle");
    assert_eq!((w.balance(buffer()), w.balance(surplus_vault(0))), (1_000, 500));
    assert_eq!((acc(&w).surplus_live, acc(&w).surplus_nonce, acc(&w).total_surplus_lots_out), (true, 1, 500));
    // tokens = ceil(ceil(500 × 1.05) × RAY / 2 RAY) = ceil(525 / 2) = 263, burned.
    w.take_surplus(0, 500, 263).expect("take");
    assert_eq!(w.balance(id(k::TAKER_PROTOCOL)), 10_000 - 263);
    assert_eq!(w.supply(protocol_token_def()), 10_000 - 263);
    assert_eq!(w.balance(id(k::TAKER_STABLE)), 500);
    let a = acc(&w);
    assert_eq!((a.surplus_live, a.total_protocol_burned, a.last_surplus_end_ms), (false, 263, w.now));
}

#[test]
fn take_surplus_max_amount_fills_lot() {
    // Review Focus 4.
    let mut w = World::with_system(false);
    w.donate_stable(buffer(), 1_500);
    w.start_surplus(0).unwrap();
    w.take_surplus(0, u128::MAX, u128::MAX).expect("max take");
    assert_eq!(w.balance(id(k::TAKER_STABLE)), 500);
    assert!(!w.read::<SurplusAuctionState>(surplus_auction(0)).live);
}

#[test]
fn partial_take_dust_and_slippage() {
    let mut w = World::with_system(false);
    w.donate_stable(buffer(), 1_500);
    w.start_surplus(0).unwrap();
    assert_err(w.take_surplus(0, 495, u128::MAX), "DustRemainder");
    assert_err(w.take_surplus(0, 250, 1), "Slippage");
    w.take_surplus(0, 250, u128::MAX).unwrap();
    assert_eq!(w.read::<SurplusAuctionState>(surplus_auction(0)).lot_left, 250);
}

#[test]
fn start_rejections() {
    let mut w = World::with_engine();
    w.donate_stable(buffer(), 1_500);
    assert_err(w.start_surplus(0), "TokenNotConfigured");

    let mut w = World::with_system(false);
    w.donate_stable(buffer(), 1_499);
    assert_err(w.start_surplus(0), "BufferBelowTarget");
    w.donate_stable(buffer(), 1);
    assert_err(w.start_surplus(1), "NonceMismatch");
    w.start_surplus(0).unwrap();
    w.donate_stable(buffer(), 1_500);
    assert_err(w.start_surplus(1), "SurplusAuctionAlreadyLive");
}

#[test]
fn implicit_settle_runs_first_and_unbacked_debt_blocks_start() {
    let mut w = World::with_system(false);
    w.book_shortfall();
    w.donate_stable(buffer(), 1_500);
    assert_err(w.start_surplus(0), "UnbackedDebtPresent");
    assert_eq!(w.balance(buffer()), 1_500, "the implicit burn reverted with the transaction");
}

#[test]
fn close_after_ttl_returns_the_remainder() {
    let mut w = World::with_system(false);
    w.donate_stable(buffer(), 1_500);
    w.start_surplus(0).unwrap();
    w.take_surplus(0, 250, u128::MAX).unwrap();
    assert_err(w.close_surplus(0), "DelayNotElapsed");
    w.advance(EngineParamsArgs::defaults_for_tests().system_ttl_ms);
    w.close_surplus(0).expect("close");
    assert_eq!(w.balance(buffer()), 1_250);
    let a = acc(&w);
    assert_eq!((a.surplus_live, a.total_surplus_returned, a.last_surplus_end_ms), (false, 250, w.now));
    assert_err(w.close_surplus(0), "NotLive");
}

#[test]
fn reference_oracle_overrides_within_twenty_percent() {
    // spec §5.11 / ADR-017: valid oracle at 3.0 → clamped to 2.4; 1.9 → 1.9; stale → admin 2.0.
    for (price, age, expected) in [(3 * ONE, 0, 24 * ONE / 10), (19 * ONE / 10, 0, 19 * ONE / 10), (3 * ONE, 3_600_001, 2 * ONE)] {
        let mut w = World::with_system(true);
        w.advance(3_600_001);
        w.set_ref_oracle(price, w.now - age);
        w.set_oracle(R, w.now);
        w.donate_stable(buffer(), 1_500);
        w.start_surplus(0).unwrap();
        assert_eq!(w.read::<SurplusAuctionState>(surplus_auction(0)).ref_ray, expected, "oracle {price} age {age}");
    }
}
```

Append to the core `tests` module:

```rust
    #[test]
    fn surplus_indices() {
        assert_eq!(idx(&Instruction::StartSurplusAuction { nonce: 0 }), 10);
        assert_eq!(idx(&Instruction::TakeSurplus { amount_coin: 0, max_protocol_in: 0 }), 11);
        assert_eq!(idx(&Instruction::CloseSurplusAuction), 12);
    }
```

- [ ] **Step 2: Run and see the failure**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p engine_integration_tests --test surplus'`
Expected: compile errors: no variants `StartSurplusAuction`, `TakeSurplus`, `CloseSurplusAuction`.

- [ ] **Step 3: Append the variants**

```rust
    /// Accounts: caller(s), config, accounting(m), buffer(m), stablecoin_definition(m), surplus_auction(i,m),
    /// surplus_vault(m), ref_oracle [slot 0], clock.
    StartSurplusAuction { nonce: u64 },
    /// Accounts: taker_protocol_holding(s,m), taker_stable_holding(m), config, accounting(m), surplus_auction(m),
    /// surplus_vault(m), protocol_token_definition(m), clock.
    TakeSurplus { amount_coin: u128, max_protocol_in: u128 },
    /// Accounts: caller(s), config, accounting(m), surplus_auction(m), surplus_vault(m), buffer(m), clock.
    CloseSurplusAuction,
```

- [ ] **Step 4: Write `system.rs`**

```rust
//! F4/F5 shared helpers (spec §5.11, ADR-017): the protocol-token reference price and the protocol
//! token's status. Optional accounts are pinned to their configured id or the placeholder (ADR-030),
//! so a caller can never substitute an account that changes a decision.

use auction_core::{
    error::PauseReason,
    price::{effective_ref_price, validate_feed, FeedParams, OracleReading, OracleScale, MAX_CONF_BPS},
};
use auction_engine_core::{
    pda::{self, SLOT_PROTOCOL_TOKEN, SLOT_REF_ORACLE},
    state::EngineConfigState,
    EngineError,
};
use lee_core::{
    account::{AccountId, AccountWithMetadata},
    program::ProgramId,
};
use token_core::TokenDefinition;
use twap_oracle_core::OraclePriceAccount;

use crate::accounts::expect_slot;

/// Admin price; a configured oracle that is valid (age, confidence, within [admin/2, 2·admin]) overrides
/// it, clamped to ±20% of the admin price. An invalid oracle falls back to the admin price.
pub fn ref_price(
    program_id: &ProgramId,
    config_id: &AccountId,
    cfg: &EngineConfigState,
    slot: &AccountWithMetadata,
    now: u64,
) -> Result<u128, EngineError> {
    expect_slot(program_id, config_id, SLOT_REF_ORACLE, cfg.protocol_token_oracle_id, slot)?;
    if cfg.protocol_token_oracle_id.is_none() {
        return Ok(cfg.protocol_token_ref_price);
    }
    let params = FeedParams {
        scale: OracleScale::Ray,
        max_age_ms: cfg.ref_oracle_max_age_ms,
        max_conf_bps: MAX_CONF_BPS,
        band_lo_bps: 10_000,
        band_hi_bps: 10_000,
    };
    let validity = match OraclePriceAccount::try_from(&slot.account.data) {
        Ok(o) => validate_feed(
            &OracleReading { price_raw: o.price, confidence_raw: o.confidence_interval, timestamp_ms: o.timestamp },
            &params,
            cfg.protocol_token_ref_price,
            now,
        ),
        Err(_) => Err(PauseReason::OutOfRange),
    };
    Ok(effective_ref_price(cfg.protocol_token_ref_price, Some(validity)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenStatus {
    pub configured: bool,
    /// The configured definition exists, is owned by the config's token program and is fungible.
    pub token_ok: bool,
    /// Its mint authority is this config's `["MINT_AUTH", config]` PDA.
    pub mint_authority_ok: bool,
}

pub fn protocol_token_status(
    program_id: &ProgramId,
    config_id: &AccountId,
    cfg: &EngineConfigState,
    slot: &AccountWithMetadata,
) -> Result<TokenStatus, EngineError> {
    expect_slot(program_id, config_id, SLOT_PROTOCOL_TOKEN, cfg.protocol_token, slot)?;
    let not_ok = |configured| TokenStatus { configured, token_ok: false, mint_authority_ok: false };
    if cfg.protocol_token.is_none() {
        return Ok(not_ok(false));
    }
    if slot.account.program_owner != cfg.token_program_id {
        return Ok(not_ok(true));
    }
    match TokenDefinition::try_from(&slot.account.data) {
        Ok(TokenDefinition::Fungible { authority, .. }) => Ok(TokenStatus {
            configured: true,
            token_ok: true,
            mint_authority_ok: authority == Some(pda::mint_authority_pda(program_id, config_id)),
        }),
        _ => Ok(not_ok(true)),
    }
}
```

- [ ] **Step 5: Write `surplus.rs`**

```rust
//! F5 surplus auction (spec §5.11): instant Dutch, protocol tokens burned in the same transaction.
//! The lot moves from the buffer into its own `["SURPVAULT", surplus_auction]` holding at start, so
//! a later `settle_debt` cannot spend it (ADR-031). The reference price is snapshotted at start.

use auction_core::system::{close_surplus, start_surplus, take_surplus as core_take};
use auction_engine_core::{pda, state::*, EngineError};
use lee_core::{account::{AccountId, AccountWithMetadata}, program::ProgramId};

use crate::{accounts::*, settle, system, token_calls, EngineResult};

fn load_surplus(program_id: &ProgramId, config_id: &AccountId, acc: &AccountWithMetadata) -> Result<SurplusAuctionState, EngineError> {
    if acc.account.program_owner != *program_id {
        return Err(EngineError::WrongAccount("surplus_auction"));
    }
    let s = SurplusAuctionState::try_from(&acc.account.data).map_err(|_| EngineError::WrongAccount("surplus_auction"))?;
    if s.config != *config_id {
        return Err(EngineError::WrongAccount("surplus_auction"));
    }
    expect_id(acc, &pda::surplus_auction_pda(program_id, config_id, s.nonce), "surplus_auction")?;
    Ok(s)
}

pub struct StartSurplusAccounts {
    pub caller: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    pub buffer: AccountWithMetadata,
    pub stablecoin_definition: AccountWithMetadata,
    pub surplus_auction: AccountWithMetadata,
    pub surplus_vault: AccountWithMetadata,
    pub ref_oracle: AccountWithMetadata,
    pub clock: AccountWithMetadata,
}

pub fn start_surplus_auction(program_id: ProgramId, a: StartSurplusAccounts, nonce: u64) -> EngineResult {
    if !a.caller.is_authorized {
        return Err(EngineError::WrongAccount("caller"));
    }
    let now = read_clock(&a.clock)?;
    let cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let acc = load_accounting(&program_id, &config_id, &a.accounting)?;
    if nonce != acc.surplus_nonce {
        return Err(EngineError::NonceMismatch);
    }
    let token = cfg.token_program_id;
    expect_id(&a.buffer, &pda::buffer_pda(&program_id, &config_id), "buffer")?;
    expect_id(&a.stablecoin_definition, &cfg.stablecoin_definition_id, "stablecoin_definition")?;
    let buffer_balance = fungible_balance(&a.buffer, &token, &cfg.stablecoin_definition_id)?;
    expect_id(&a.surplus_auction, &pda::surplus_auction_pda(&program_id, &config_id, nonce), "surplus_auction")?;
    expect_default(&a.surplus_auction, "surplus_auction")?;
    expect_id(&a.surplus_vault, &pda::surplus_vault_pda(&program_id, &a.surplus_auction.account_id), "surplus_vault")?;
    expect_default(&a.surplus_vault, "surplus_vault")?;
    let reference = system::ref_price(&program_id, &config_id, &cfg, &a.ref_oracle, now)?;

    let (settled, burned) = settle::implicit_settle(&acc, buffer_balance);
    let buffer_after = buffer_balance.checked_sub(burned).ok_or(EngineError::Math)?;
    let sys = cfg.system.to_core()?;
    let started = start_surplus(&settled.core(), buffer_after, cfg.protocol_token.is_some(), acc.last_surplus_end_ms, now, &sys, acc.surplus_live)?;
    let state = SurplusAuctionState {
        tag: TAG_SURPLUS,
        config: config_id,
        nonce,
        lot_left: started.lot_left,
        start_ms: started.start_ms,
        live: started.live,
        ref_ray: reference,
    };
    let next_acc = AccountingState {
        surplus_live: true,
        surplus_nonce: acc.surplus_nonce.checked_add(1).ok_or(EngineError::Math)?,
        total_surplus_lots_out: settled.total_surplus_lots_out.checked_add(started.lot_left).ok_or(EngineError::Math)?,
        ..settled
    };

    let surplus_vault_seed = pda::surplus_vault_seed(&a.surplus_auction.account_id);
    let mut calls = vec![token_calls::initialize_account(
        token,
        a.stablecoin_definition.clone(),
        token_calls::authorized(&a.surplus_vault),
        vec![surplus_vault_seed],
    )];
    calls.extend(settle::buffer_burn(&cfg, &config_id, &a.stablecoin_definition, &a.buffer, burned));
    calls.push(token_calls::transfer(
        token,
        token_calls::authorized(&token_calls::with_balance(&a.buffer, cfg.stablecoin_definition_id, buffer_after)),
        token_calls::post_init(&a.surplus_vault, token, cfg.stablecoin_definition_id),
        started.lot_left,
        vec![pda::buffer_seed(&config_id)],
    ));
    Ok((
        vec![
            echo(&a.caller),
            echo(&a.config),
            write(&a.accounting, &next_acc),
            echo(&a.buffer),
            echo(&a.stablecoin_definition),
            create(&a.surplus_auction, &state, pda::surplus_auction_seed(&config_id, nonce)),
            echo(&a.surplus_vault),
            echo(&a.ref_oracle),
            echo(&a.clock),
        ],
        calls,
    ))
}

pub struct TakeSurplusAccounts {
    pub taker_protocol_holding: AccountWithMetadata,
    pub taker_stable_holding: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    pub surplus_auction: AccountWithMetadata,
    pub surplus_vault: AccountWithMetadata,
    pub protocol_token_definition: AccountWithMetadata,
    pub clock: AccountWithMetadata,
}

/// The taker pays protocol tokens (burned) for `filled = old.lot_left − new.lot_left` stablecoin.
pub fn take_surplus(program_id: ProgramId, a: TakeSurplusAccounts, amount_coin: u128, max_protocol_in: u128) -> EngineResult {
    if !a.taker_protocol_holding.is_authorized {
        return Err(EngineError::WrongAccount("taker_protocol_holding"));
    }
    let now = read_clock(&a.clock)?;
    let cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let acc = load_accounting(&program_id, &config_id, &a.accounting)?;
    let sa = load_surplus(&program_id, &config_id, &a.surplus_auction)?;
    let protocol = cfg.protocol_token.ok_or(EngineError::TokenNotConfigured)?;
    expect_id(&a.protocol_token_definition, &protocol, "protocol_token_definition")?;
    let token = cfg.token_program_id;
    expect_id(&a.surplus_vault, &pda::surplus_vault_pda(&program_id, &a.surplus_auction.account_id), "surplus_vault")?;
    let vault_balance = fungible_balance(&a.surplus_vault, &token, &cfg.stablecoin_definition_id)?;

    let sys = cfg.system.to_core()?;
    let (next, tokens) = core_take(&sa.core(), amount_coin, max_protocol_in, sa.ref_ray, &sys, now)?;
    let filled = sa.lot_left.checked_sub(next.lot_left).ok_or(EngineError::Math)?;
    if vault_balance < filled {
        return Err(EngineError::VaultBelowBook);
    }
    if fungible_balance(&a.taker_protocol_holding, &token, &protocol)? < tokens {
        return Err(EngineError::InsufficientBalance);
    }
    check_recipient(&a.taker_stable_holding, &token, &cfg.stablecoin_definition_id)?;

    let mut next_acc = acc.clone();
    next_acc.total_protocol_burned = next_acc.total_protocol_burned.checked_add(tokens).ok_or(EngineError::Math)?;
    if !next.live {
        next_acc.surplus_live = false;
        next_acc.last_surplus_end_ms = now;
    }
    let calls = vec![
        token_calls::burn(token, a.protocol_token_definition.clone(), a.taker_protocol_holding.clone(), tokens, vec![]),
        token_calls::transfer(
            token,
            token_calls::authorized(&a.surplus_vault),
            a.taker_stable_holding.clone(),
            filled,
            vec![pda::surplus_vault_seed(&a.surplus_auction.account_id)],
        ),
    ];
    Ok((
        vec![
            echo(&a.taker_protocol_holding),
            echo(&a.taker_stable_holding),
            echo(&a.config),
            write(&a.accounting, &next_acc),
            write(&a.surplus_auction, &sa.with_core(&next)),
            echo(&a.surplus_vault),
            echo(&a.protocol_token_definition),
            echo(&a.clock),
        ],
        calls,
    ))
}

pub struct CloseSurplusAccounts {
    pub caller: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    pub surplus_auction: AccountWithMetadata,
    pub surplus_vault: AccountWithMetadata,
    pub buffer: AccountWithMetadata,
    pub clock: AccountWithMetadata,
}

/// After `system_ttl_ms` the untaken lot (whole vault balance, ADR-033) returns to the buffer.
pub fn close_surplus_auction(program_id: ProgramId, a: CloseSurplusAccounts) -> EngineResult {
    if !a.caller.is_authorized {
        return Err(EngineError::WrongAccount("caller"));
    }
    let now = read_clock(&a.clock)?;
    let cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let acc = load_accounting(&program_id, &config_id, &a.accounting)?;
    let sa = load_surplus(&program_id, &config_id, &a.surplus_auction)?;
    let (next, _remainder) = close_surplus(&sa.core(), &cfg.system.to_core()?, now)?;
    let token = cfg.token_program_id;
    expect_id(&a.surplus_vault, &pda::surplus_vault_pda(&program_id, &a.surplus_auction.account_id), "surplus_vault")?;
    expect_id(&a.buffer, &pda::buffer_pda(&program_id, &config_id), "buffer")?;
    let vault_balance = fungible_balance(&a.surplus_vault, &token, &cfg.stablecoin_definition_id)?;
    let next_acc = AccountingState {
        surplus_live: false,
        last_surplus_end_ms: now,
        total_surplus_returned: acc.total_surplus_returned.checked_add(vault_balance).ok_or(EngineError::Math)?,
        ..acc
    };
    let calls = if vault_balance > 0 {
        vec![token_calls::transfer(
            token,
            token_calls::authorized(&a.surplus_vault),
            a.buffer.clone(),
            vault_balance,
            vec![pda::surplus_vault_seed(&a.surplus_auction.account_id)],
        )]
    } else {
        vec![]
    };
    Ok((
        vec![
            echo(&a.caller),
            echo(&a.config),
            write(&a.accounting, &next_acc),
            write(&a.surplus_auction, &sa.with_core(&next)),
            echo(&a.surplus_vault),
            echo(&a.buffer),
            echo(&a.clock),
        ],
        calls,
    ))
}
```

Add `pub mod surplus;` and `pub mod system;` to `lib.rs`.

- [ ] **Step 6: Append the guest functions**

```rust
    #[instruction]
    #[allow(clippy::too_many_arguments, reason = "nine accounts (Shared Reference R2)")]
    pub fn start_surplus_auction(
        ctx: ProgramContext,
        #[account(signer)] caller: AccountWithMetadata,
        config: AccountWithMetadata,
        #[account(mut, pda = [literal("ACCT"), account("config")])] accounting: AccountWithMetadata,
        #[account(mut, pda = [literal("BUFFER"), account("config")])] buffer: AccountWithMetadata,
        #[account(mut)] stablecoin_definition: AccountWithMetadata,
        #[account(init, pda = [literal("SURP"), account("config"), arg("nonce")])] surplus_auction: AccountWithMetadata,
        #[account(mut, pda = [literal("SURPVAULT"), account("surplus_auction")])] surplus_vault: AccountWithMetadata,
        ref_oracle: AccountWithMetadata,
        clock: AccountWithMetadata,
        nonce: u64,
    ) -> SpelResult {
        let a = engine::surplus::StartSurplusAccounts {
            caller, config, accounting, buffer, stablecoin_definition, surplus_auction, surplus_vault, ref_oracle, clock,
        };
        finish(engine::surplus::start_surplus_auction(ctx.self_program_id, a, nonce))
    }

    #[instruction]
    #[allow(clippy::too_many_arguments, reason = "eight accounts (Shared Reference R2)")]
    pub fn take_surplus(
        ctx: ProgramContext,
        #[account(mut, signer)] taker_protocol_holding: AccountWithMetadata,
        #[account(mut)] taker_stable_holding: AccountWithMetadata,
        config: AccountWithMetadata,
        #[account(mut, pda = [literal("ACCT"), account("config")])] accounting: AccountWithMetadata,
        #[account(mut)] surplus_auction: AccountWithMetadata,
        #[account(mut, pda = [literal("SURPVAULT"), account("surplus_auction")])] surplus_vault: AccountWithMetadata,
        #[account(mut)] protocol_token_definition: AccountWithMetadata,
        clock: AccountWithMetadata,
        amount_coin: u128,
        max_protocol_in: u128,
    ) -> SpelResult {
        let a = engine::surplus::TakeSurplusAccounts {
            taker_protocol_holding, taker_stable_holding, config, accounting, surplus_auction, surplus_vault,
            protocol_token_definition, clock,
        };
        finish(engine::surplus::take_surplus(ctx.self_program_id, a, amount_coin, max_protocol_in))
    }

    #[instruction]
    pub fn close_surplus_auction(
        ctx: ProgramContext,
        #[account(signer)] caller: AccountWithMetadata,
        config: AccountWithMetadata,
        #[account(mut, pda = [literal("ACCT"), account("config")])] accounting: AccountWithMetadata,
        #[account(mut)] surplus_auction: AccountWithMetadata,
        #[account(mut, pda = [literal("SURPVAULT"), account("surplus_auction")])] surplus_vault: AccountWithMetadata,
        #[account(mut, pda = [literal("BUFFER"), account("config")])] buffer: AccountWithMetadata,
        clock: AccountWithMetadata,
    ) -> SpelResult {
        let a = engine::surplus::CloseSurplusAccounts { caller, config, accounting, surplus_auction, surplus_vault, buffer, clock };
        finish(engine::surplus::close_surplus_auction(ctx.self_program_id, a))
    }
```

- [ ] **Step 7: Regenerate the IDL and run the tests**

Run: the Task 9 Step 7 command with `--test surplus`. Expected: `surplus` 7 passed; earlier suites green.

- [ ] **Step 8: Docs and ADR-031**

Append to `docs/architecture/programs.md` a section "F5 surplus auction" with the three instructions, the pricing (`tokens = ceil(ceil(coin × mult / 1e4) × RAY / ref)`), the implicit `settle_debt` at start, the reference price rule (admin price; optional oracle valid within `[admin/2, 2·admin]`, clamped ±20%; invalid → admin price) and the snapshot at start; and a paragraph "Mock protocol token": a real `Token::NewFungibleDefinition` whose `mint_authority` is `["MINT_AUTH", config]` (test `with_system`), used only for F4/F5.

Append ADR-031 to `docs/design-log.md`:

```markdown
---

## ADR-031: System-Auction Accounts and Lifecycle

**Context**
Spec §5.11 describes F4/F5 as instant Dutch auctions addressed by `Accounting.surplus_nonce` / `debt_nonce`, at most one of each live, closed by `close_system_auction` after `system_ttl_ms`. The core (`auction_core::system`) is stateless about liveness and the last surplus end; the protocol-token reference price may come from an optional oracle.

**Decision**
- Program-layer `Accounting` fields: `surplus_live`, `debt_live`, `last_surplus_end_ms`, and counters `total_protocol_burned`, `total_protocol_minted`, `total_surplus_lots_out`, `total_surplus_returned` (invariants 9 and 17). `start_*(nonce)` requires `nonce == *_nonce`, then increments it.
- The surplus lot moves into its own holding `["SURPVAULT", surplus_auction]` at start (spec: "into the auction's holding"), so `settle_debt` cannot spend coin a live surplus auction is selling.
- `ref_ray` is snapshotted into `SurplusAuction` at start (the debt auction snapshots `start_amount` in the core), so takes need no oracle account.
- `close_system_auction` is two instructions, `close_surplus_auction` and `close_debt_auction`, because their account lists differ (the surplus close returns the vault balance to the buffer; the debt close only releases `debt_on_auction`).
- `set_engine_params` refuses to change the protocol token, its oracle or the system-auction parameters while a system auction is live (`ProtocolTokenLocked`), since takes read them.
- Each admin change of `protocol_token_ref_price` stays within `[old/2, 2·old]` (`RefPriceStepTooLarge`, I5; tests `ref_price_steps_are_bounded_to_half_and_double`).

**Trade-offs**
One more holding per surplus auction (accounts are never released). The reference price of a running auction does not follow the oracle (it is a Dutch schedule seed, ADR-017). Governance-trust residual (I5): the step bound limits one change, not repeated changes; switching or disabling the protocol token (allowed whenever no system auction is live) remains an admin power, and disabling it lets anyone's `check_solvency` freeze the host when `unbacked_debt > bad_debt_freeze_threshold` (recorded in `THREAT_MODEL.md`).

**Status**
Accepted (Plan 2, Task 13)
```

- [ ] **Step 9: Commit (controller — GPG signing on Windows)**

The implementer stages the files and reports; the controller commits from Git Bash.

```bash
git add programs/auction_engine programs/integration_tests idl/auction_engine-idl.json docs/architecture/programs.md docs/design-log.md
git commit -m "feat(engine): F5 surplus auction with mock protocol token burn and reference price; ADR-031"
```

---

### Task 14: F4 debt auction (start / take / close; mint from the engine PDA into a possibly fresh holding)

**Files:**
- Create: `programs/auction_engine/src/debt.rs`
- Modify: `programs/auction_engine/src/lib.rs`, `programs/auction_engine/core/src/{instruction.rs,lib.rs}` (append `StartDebtAuction { nonce: u64 }` = 13, `TakeDebt { amount_coin: u128, min_protocol_out: u128 }` = 14, `CloseDebtAuction` = 15), guest (append three functions), `idl/auction_engine-idl.json`
- Modify: `programs/integration_tests/tests/common/mod.rs` (append); Create: `programs/integration_tests/tests/debt.rs`
- Modify: `docs/architecture/programs.md`

**Interfaces:**
- Consumes: Task 13 (`system::{ref_price, protocol_token_status, TokenStatus}`, harness `World::{with_system, book_shortfall, create_protocol_token, ref_slot}`); Task 12 (`settle::{implicit_settle, buffer_burn}`); Task 8 (`admin::set_engine_params` `ProtocolTokenLocked` branch, tested here); `DebtAuctionState::{core, with_core}`; `auction_core::{system::{start_debt, take_debt, close_debt}, accounting::Accounting::{record_debt_start, record_debt_take, record_debt_close}}`.
- Produces: `debt::{StartDebtAccounts, start_debt_auction, TakeDebtAccounts, take_debt, CloseDebtAccounts, close_debt_auction}`; harness `debt_auction(nonce)`, `TAKER2_PROTOCOL`, `World::{start_debt, take_debt, close_debt}`. Pairing (ADR-024): `start_debt` ↔ `record_debt_start(debt_lot)`, `take_debt` ↔ `record_debt_take(old.lot_left − new.lot_left)` and the burn of exactly that filled amount, `close_debt` ↔ `record_debt_close(lot_left)`.

- [ ] **Step 1: Write the failing tests**

Append to `programs/integration_tests/tests/common/mod.rs`:

```rust
pub const TAKER2_PROTOCOL: u8 = 68;

pub fn debt_auction(nonce: u64) -> AccountId {
    pda::debt_auction_pda(&engine_program(), &config(), nonce)
}

impl World {
    pub fn start_debt(&mut self, nonce: u64) -> Result<(), LeeError> {
        let cfg: auction_engine_core::state::EngineConfigState = self.read(config());
        let token_slot = cfg.protocol_token.unwrap_or_else(|| unused(SLOT_PROTOCOL_TOKEN));
        let ref_slot = self.ref_slot();
        self.engine(
            vec![id(k::CALLER), config(), accounting(), buffer(), stable_def(), debt_auction(nonce), token_slot, ref_slot, CLOCK_01_PROGRAM_ACCOUNT_ID],
            &[k::CALLER],
            Ix::StartDebtAuction { nonce },
        )
    }

    /// `take_debt` by `stable` (signer) receiving tokens in `protocol`; `signers` must include `stable`.
    pub fn take_debt(&mut self, nonce: u64, stable: u8, protocol: u8, signers: &[u8], amount_coin: u128, min_out: u128) -> Result<(), LeeError> {
        self.engine(
            vec![id(stable), id(protocol), config(), accounting(), debt_auction(nonce), stable_def(), protocol_token_def(), mint_authority(), CLOCK_01_PROGRAM_ACCOUNT_ID],
            signers,
            Ix::TakeDebt { amount_coin, min_protocol_out: min_out },
        )
    }

    pub fn close_debt(&mut self, nonce: u64) -> Result<(), LeeError> {
        self.engine(
            vec![id(k::CALLER), config(), accounting(), debt_auction(nonce), CLOCK_01_PROGRAM_ACCOUNT_ID],
            &[k::CALLER],
            Ix::CloseDebtAuction,
        )
    }
}
```

`programs/integration_tests/tests/debt.rs`:

```rust
mod common;

use auction_engine_core::{
    args::{CollateralArgs, EngineParamsArgs},
    state::{AccountingState, DebtAuctionState},
};
use common::*;
use lee_core::account::AccountId;

fn acc(w: &World) -> AccountingState {
    w.read(accounting())
}

/// Protocol token configured; the mock token is created with `authority` (or not at all); shortfall
/// booked (unbacked 355_049); the taker holds 1_000 stablecoin.
fn debt_world(authority: Option<AccountId>, params: EngineParamsArgs) -> World {
    let mut w = World::new();
    w.init_engine_config(&params).unwrap();
    w.bind_engine().unwrap();
    w.set_collateral_config(k::ENGINE_ADMIN, &CollateralArgs::defaults_for_tests()).unwrap();
    if let Some(a) = authority {
        w.create_protocol_token(10_000, a).unwrap();
    }
    w.book_shortfall();
    w.fund_stable(k::TAKER_STABLE, 1_000);
    w
}

fn token_params() -> EngineParamsArgs {
    EngineParamsArgs { protocol_token_enabled: true, ..EngineParamsArgs::defaults_for_tests() }
}

#[test]
fn debt_auction_mints_against_burned_coin() {
    let mut w = debt_world(Some(mint_authority()), token_params());
    let stable_supply = w.supply(stable_def());
    w.start_debt(0).expect("start_debt_auction");
    let a = acc(&w);
    assert_eq!((a.debt_on_auction, a.debt_live, a.debt_nonce), (500, true, 1));
    // start_amount = floor(500 / 2) = 250 tokens for the whole 500-coin lot at t = 0.
    w.take_debt(0, k::TAKER_STABLE, k::TAKER_PROTOCOL, &[k::TAKER_STABLE], 500, 250).expect("take_debt");
    assert_eq!(w.balance(id(k::TAKER_PROTOCOL)), 10_250);
    assert_eq!(w.supply(protocol_token_def()), 10_250);
    assert_eq!(w.balance(id(k::TAKER_STABLE)), 500);
    assert_eq!(w.supply(stable_def()), stable_supply - 500);
    let a = acc(&w);
    assert_eq!((a.unbacked_debt, a.debt_on_auction, a.total_debt_auctioned), (355_049 - 500, 0, 500));
    assert_eq!((a.total_protocol_minted, a.debt_live), (250, false));
}

#[test]
fn take_debt_mints_into_fresh_holding() {
    // Review Focus 2 + S7 prerequisite: mint into a taker's never-initialised holding.
    let mut w = debt_world(Some(mint_authority()), token_params());
    w.start_debt(0).unwrap();
    assert_err(w.take_debt(0, k::TAKER_STABLE, TAKER2_PROTOCOL, &[k::TAKER_STABLE], 500, 0), "RecipientMustSign");
    w.take_debt(0, k::TAKER_STABLE, TAKER2_PROTOCOL, &[k::TAKER_STABLE, TAKER2_PROTOCOL], 500, 0).expect("fresh signed holding");
    assert_eq!(w.balance(id(TAKER2_PROTOCOL)), 250);
}

#[test]
fn take_debt_max_amount_fills_lot() {
    // Review Focus 4: only the filled 500 coin is burned.
    let mut w = debt_world(Some(mint_authority()), token_params());
    w.start_debt(0).unwrap();
    w.take_debt(0, k::TAKER_STABLE, k::TAKER_PROTOCOL, &[k::TAKER_STABLE], u128::MAX, 0).unwrap();
    assert_eq!(w.balance(id(k::TAKER_STABLE)), 500);
}

#[test]
fn partial_take_then_close_releases_the_reservation() {
    let mut w = debt_world(Some(mint_authority()), token_params());
    w.start_debt(0).unwrap();
    w.take_debt(0, k::TAKER_STABLE, k::TAKER_PROTOCOL, &[k::TAKER_STABLE], 300, 150).unwrap();
    assert_eq!(acc(&w).debt_on_auction, 200);
    assert_err(w.close_debt(0), "DelayNotElapsed");
    w.advance(token_params().system_ttl_ms);
    w.close_debt(0).expect("close");
    let a = acc(&w);
    assert_eq!((a.debt_on_auction, a.debt_live), (0, false));
    assert!(!w.read::<DebtAuctionState>(debt_auction(0)).live);
}

#[test]
fn start_rejections_map_to_the_freeze_list() {
    let mut w = debt_world(Some(id(k::ATTACKER)), token_params());
    assert_err(w.start_debt(0), "MintAuthorityMissing");
    let mut w = debt_world(None, token_params());
    assert_err(w.start_debt(0), "ProtocolTokenMismatch");
    let mut w = debt_world(Some(mint_authority()), EngineParamsArgs { protocol_token_ref_price: ONE / 100, ..token_params() });
    assert_err(w.start_debt(0), "MintCapConfig");
    let mut w = World::with_system(false);
    assert_err(w.start_debt(0), "InsufficientBadDebt");
}

#[test]
fn mint_never_exceeds_the_absolute_cap() {
    // start = floor(500 × 9) = 4_500 ≤ 5_000; growth 30% would offer 5_850 → capped at 5_000.
    let mut w = debt_world(Some(mint_authority()), EngineParamsArgs { protocol_token_ref_price: ONE / 9, ..token_params() });
    w.start_debt(0).unwrap();
    w.advance(1_000);
    w.take_debt(0, k::TAKER_STABLE, k::TAKER_PROTOCOL, &[k::TAKER_STABLE], 500, 0).unwrap();
    assert_eq!(acc(&w).total_protocol_minted, 5_000);
}

#[test]
fn protocol_token_locked_while_debt_auction_live() {
    let mut w = debt_world(Some(mint_authority()), token_params());
    w.start_debt(0).unwrap();
    let off = EngineParamsArgs { protocol_token_enabled: false, ..token_params() };
    assert_err(w.set_engine_params(k::ENGINE_ADMIN, &off), "ProtocolTokenLocked");
    let new_price = EngineParamsArgs { protocol_token_ref_price: 3 * ONE, ..token_params() };
    w.set_engine_params(k::ENGINE_ADMIN, &new_price).expect("ref price may change: auctions snapshot it");
}
```

Append to the core `tests` module:

```rust
    #[test]
    fn debt_indices() {
        assert_eq!(idx(&Instruction::StartDebtAuction { nonce: 0 }), 13);
        assert_eq!(idx(&Instruction::TakeDebt { amount_coin: 0, min_protocol_out: 0 }), 14);
        assert_eq!(idx(&Instruction::CloseDebtAuction), 15);
    }
```

- [ ] **Step 2: Run and see the failure**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p engine_integration_tests --test debt'`
Expected: compile errors: no variants `StartDebtAuction`, `TakeDebt`, `CloseDebtAuction`.

- [ ] **Step 3: Append the variants**

```rust
    /// Accounts: caller(s), config, accounting(m), buffer(m), stablecoin_definition(m), debt_auction(i,m),
    /// protocol_token_definition [slot 1], ref_oracle [slot 0], clock.
    StartDebtAuction { nonce: u64 },
    /// Accounts: taker_stable_holding(s,m), taker_protocol_holding(m), config, accounting(m), debt_auction(m),
    /// stablecoin_definition(m), protocol_token_definition(m), mint_authority, clock.
    TakeDebt { amount_coin: u128, min_protocol_out: u128 },
    /// Accounts: caller(s), config, accounting(m), debt_auction(m), clock.
    CloseDebtAuction,
```

- [ ] **Step 4: Write `debt.rs`**

```rust
//! F4 debt auction (spec §5.11, ADR-017, ADR-024): the taker's stablecoin is burned and reduces
//! `unbacked_debt`; mock protocol tokens are minted by `["MINT_AUTH", config]`. `debt_on_auction`
//! reserves the lot so `settle_debt` cannot cover the same debt twice.

use auction_core::system::{close_debt, start_debt, take_debt as core_take};
use auction_engine_core::{pda, state::*, EngineError};
use lee_core::{account::{AccountId, AccountWithMetadata}, program::ProgramId};

use crate::{accounts::*, settle, system, token_calls, EngineResult};

fn load_debt(program_id: &ProgramId, config_id: &AccountId, acc: &AccountWithMetadata) -> Result<DebtAuctionState, EngineError> {
    if acc.account.program_owner != *program_id {
        return Err(EngineError::WrongAccount("debt_auction"));
    }
    let d = DebtAuctionState::try_from(&acc.account.data).map_err(|_| EngineError::WrongAccount("debt_auction"))?;
    if d.config != *config_id {
        return Err(EngineError::WrongAccount("debt_auction"));
    }
    expect_id(acc, &pda::debt_auction_pda(program_id, config_id, d.nonce), "debt_auction")?;
    Ok(d)
}

pub struct StartDebtAccounts {
    pub caller: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    pub buffer: AccountWithMetadata,
    pub stablecoin_definition: AccountWithMetadata,
    pub debt_auction: AccountWithMetadata,
    pub protocol_token_definition: AccountWithMetadata,
    pub ref_oracle: AccountWithMetadata,
    pub clock: AccountWithMetadata,
}

pub fn start_debt_auction(program_id: ProgramId, a: StartDebtAccounts, nonce: u64) -> EngineResult {
    if !a.caller.is_authorized {
        return Err(EngineError::WrongAccount("caller"));
    }
    let now = read_clock(&a.clock)?;
    let cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let acc = load_accounting(&program_id, &config_id, &a.accounting)?;
    if nonce != acc.debt_nonce {
        return Err(EngineError::NonceMismatch);
    }
    let token = cfg.token_program_id;
    expect_id(&a.buffer, &pda::buffer_pda(&program_id, &config_id), "buffer")?;
    expect_id(&a.stablecoin_definition, &cfg.stablecoin_definition_id, "stablecoin_definition")?;
    let buffer_balance = fungible_balance(&a.buffer, &token, &cfg.stablecoin_definition_id)?;
    expect_id(&a.debt_auction, &pda::debt_auction_pda(&program_id, &config_id, nonce), "debt_auction")?;
    expect_default(&a.debt_auction, "debt_auction")?;
    let status = system::protocol_token_status(&program_id, &config_id, &cfg, &a.protocol_token_definition)?;
    let reference = system::ref_price(&program_id, &config_id, &cfg, &a.ref_oracle, now)?;

    let (settled, burned) = settle::implicit_settle(&acc, buffer_balance);
    let buffer_after = buffer_balance.checked_sub(burned).ok_or(EngineError::Math)?;
    let sys = cfg.system.to_core()?;
    let started = start_debt(
        &settled.core(),
        buffer_after,
        status.configured,
        status.mint_authority_ok,
        status.token_ok,
        reference,
        &sys,
        now,
        acc.debt_live,
    )?;
    let next_acc = AccountingState {
        debt_live: true,
        debt_nonce: acc.debt_nonce.checked_add(1).ok_or(EngineError::Math)?,
        ..AccountingState::with_core(&settled, &settled.core().record_debt_start(sys.debt_lot))
    };
    let state = DebtAuctionState {
        tag: TAG_DEBT,
        config: config_id,
        nonce,
        lot_left: started.lot_left,
        start_amount: started.start_amount,
        start_ms: started.start_ms,
        live: started.live,
    };
    let calls = settle::buffer_burn(&cfg, &config_id, &a.stablecoin_definition, &a.buffer, burned).into_iter().collect();
    Ok((
        vec![
            echo(&a.caller),
            echo(&a.config),
            write(&a.accounting, &next_acc),
            echo(&a.buffer),
            echo(&a.stablecoin_definition),
            create(&a.debt_auction, &state, pda::debt_auction_seed(&config_id, nonce)),
            echo(&a.protocol_token_definition),
            echo(&a.ref_oracle),
            echo(&a.clock),
        ],
        calls,
    ))
}

pub struct TakeDebtAccounts {
    pub taker_stable_holding: AccountWithMetadata,
    pub taker_protocol_holding: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    pub debt_auction: AccountWithMetadata,
    pub stablecoin_definition: AccountWithMetadata,
    pub protocol_token_definition: AccountWithMetadata,
    pub mint_authority: AccountWithMetadata,
    pub clock: AccountWithMetadata,
}

pub fn take_debt(program_id: ProgramId, a: TakeDebtAccounts, amount_coin: u128, min_protocol_out: u128) -> EngineResult {
    if !a.taker_stable_holding.is_authorized {
        return Err(EngineError::WrongAccount("taker_stable_holding"));
    }
    let now = read_clock(&a.clock)?;
    let cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let acc = load_accounting(&program_id, &config_id, &a.accounting)?;
    let da = load_debt(&program_id, &config_id, &a.debt_auction)?;
    let protocol = cfg.protocol_token.ok_or(EngineError::TokenNotConfigured)?;
    expect_id(&a.protocol_token_definition, &protocol, "protocol_token_definition")?;
    expect_id(&a.stablecoin_definition, &cfg.stablecoin_definition_id, "stablecoin_definition")?;
    expect_id(&a.mint_authority, &pda::mint_authority_pda(&program_id, &config_id), "mint_authority")?;
    let token = cfg.token_program_id;

    let (next, minted) = core_take(&da.core(), amount_coin, min_protocol_out, &cfg.system.to_core()?, now)?;
    let filled = da.lot_left.checked_sub(next.lot_left).ok_or(EngineError::Math)?;
    if fungible_balance(&a.taker_stable_holding, &token, &cfg.stablecoin_definition_id)? < filled {
        return Err(EngineError::InsufficientBalance);
    }
    check_recipient(&a.taker_protocol_holding, &token, &protocol)?;

    let mut next_acc = AccountingState::with_core(&acc, &acc.core().record_debt_take(filled));
    next_acc.total_protocol_minted = next_acc.total_protocol_minted.checked_add(minted).ok_or(EngineError::Math)?;
    if !next.live {
        next_acc.debt_live = false;
    }
    let calls = vec![
        token_calls::burn(token, a.stablecoin_definition.clone(), a.taker_stable_holding.clone(), filled, vec![]),
        token_calls::mint_with_authority(
            token,
            a.protocol_token_definition.clone(),
            a.taker_protocol_holding.clone(),
            token_calls::authorized(&a.mint_authority),
            minted,
            vec![pda::mint_authority_seed(&config_id)],
        ),
    ];
    Ok((
        vec![
            echo(&a.taker_stable_holding),
            echo(&a.taker_protocol_holding),
            echo(&a.config),
            write(&a.accounting, &next_acc),
            write(&a.debt_auction, &da.with_core(&next)),
            echo(&a.stablecoin_definition),
            echo(&a.protocol_token_definition),
            echo(&a.mint_authority),
            echo(&a.clock),
        ],
        calls,
    ))
}

pub struct CloseDebtAccounts {
    pub caller: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    pub debt_auction: AccountWithMetadata,
    pub clock: AccountWithMetadata,
}

/// After `system_ttl_ms`: nothing was minted for the untaken part; release its reservation.
pub fn close_debt_auction(program_id: ProgramId, a: CloseDebtAccounts) -> EngineResult {
    if !a.caller.is_authorized {
        return Err(EngineError::WrongAccount("caller"));
    }
    let now = read_clock(&a.clock)?;
    let cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let acc = load_accounting(&program_id, &config_id, &a.accounting)?;
    let da = load_debt(&program_id, &config_id, &a.debt_auction)?;
    let (next, released) = close_debt(&da.core(), &cfg.system.to_core()?, now)?;
    let next_acc = AccountingState {
        debt_live: false,
        ..AccountingState::with_core(&acc, &acc.core().record_debt_close(released))
    };
    Ok((
        vec![
            echo(&a.caller),
            echo(&a.config),
            write(&a.accounting, &next_acc),
            write(&a.debt_auction, &da.with_core(&next)),
            echo(&a.clock),
        ],
        vec![],
    ))
}
```

Add `pub mod debt;` to `lib.rs`.

- [ ] **Step 5: Append the guest functions**

```rust
    #[instruction]
    #[allow(clippy::too_many_arguments, reason = "nine accounts (Shared Reference R2)")]
    pub fn start_debt_auction(
        ctx: ProgramContext,
        #[account(signer)] caller: AccountWithMetadata,
        config: AccountWithMetadata,
        #[account(mut, pda = [literal("ACCT"), account("config")])] accounting: AccountWithMetadata,
        #[account(mut, pda = [literal("BUFFER"), account("config")])] buffer: AccountWithMetadata,
        #[account(mut)] stablecoin_definition: AccountWithMetadata,
        #[account(init, pda = [literal("DEBT"), account("config"), arg("nonce")])] debt_auction: AccountWithMetadata,
        protocol_token_definition: AccountWithMetadata,
        ref_oracle: AccountWithMetadata,
        clock: AccountWithMetadata,
        nonce: u64,
    ) -> SpelResult {
        let a = engine::debt::StartDebtAccounts {
            caller, config, accounting, buffer, stablecoin_definition, debt_auction, protocol_token_definition, ref_oracle, clock,
        };
        finish(engine::debt::start_debt_auction(ctx.self_program_id, a, nonce))
    }

    #[instruction]
    #[allow(clippy::too_many_arguments, reason = "nine accounts (Shared Reference R2)")]
    pub fn take_debt(
        ctx: ProgramContext,
        #[account(mut, signer)] taker_stable_holding: AccountWithMetadata,
        #[account(mut)] taker_protocol_holding: AccountWithMetadata,
        config: AccountWithMetadata,
        #[account(mut, pda = [literal("ACCT"), account("config")])] accounting: AccountWithMetadata,
        #[account(mut)] debt_auction: AccountWithMetadata,
        #[account(mut)] stablecoin_definition: AccountWithMetadata,
        #[account(mut)] protocol_token_definition: AccountWithMetadata,
        #[account(pda = [literal("MINT_AUTH"), account("config")])] mint_authority: AccountWithMetadata,
        clock: AccountWithMetadata,
        amount_coin: u128,
        min_protocol_out: u128,
    ) -> SpelResult {
        let a = engine::debt::TakeDebtAccounts {
            taker_stable_holding, taker_protocol_holding, config, accounting, debt_auction, stablecoin_definition,
            protocol_token_definition, mint_authority, clock,
        };
        finish(engine::debt::take_debt(ctx.self_program_id, a, amount_coin, min_protocol_out))
    }

    #[instruction]
    pub fn close_debt_auction(
        ctx: ProgramContext,
        #[account(signer)] caller: AccountWithMetadata,
        config: AccountWithMetadata,
        #[account(mut, pda = [literal("ACCT"), account("config")])] accounting: AccountWithMetadata,
        #[account(mut)] debt_auction: AccountWithMetadata,
        clock: AccountWithMetadata,
    ) -> SpelResult {
        let a = engine::debt::CloseDebtAccounts { caller, config, accounting, debt_auction, clock };
        finish(engine::debt::close_debt_auction(ctx.self_program_id, a))
    }
```

- [ ] **Step 6: Regenerate the IDL and run the tests**

Run: the Task 9 Step 7 command with `--test debt`. Expected: `debt` 7 passed; earlier suites green (including the Task 8 `ProtocolTokenLocked` branch, now exercised).

- [ ] **Step 7: Docs**

Append to `docs/architecture/programs.md` a section "F4 debt auction" covering: start conditions in order (live, token configured, `ProtocolTokenMismatch`, `MintAuthorityMissing`, `unbacked − buffer ≥ debt_lot` after the implicit settle, `MintCapConfig` including a zero start amount); the pairing with `record_debt_start/take/close`; burning exactly the filled amount; minting `floor(offered × filled / debt_lot)` capped at `max_mint_per_auction`; that a fresh taker holding must sign (the token program claims it with `Claim::Authorized`, `programs/token/src/mint.rs:137`; S7 prerequisite, test `take_debt_mints_into_fresh_holding`); and that the PDA `["MINT_AUTH", config]` is authorised only in the `MintWithAuthority` call that attaches its seed.

- [ ] **Step 8: Commit (controller — GPG signing on Windows)**

The implementer stages the files and reports; the controller commits from Git Bash.

```bash
git add programs/auction_engine programs/integration_tests idl/auction_engine-idl.json docs/architecture/programs.md
git commit -m "feat(engine): F4 debt auction with PDA mint, debt reservation pairing and absolute mint cap"
```

---

### Task 15: `check_solvency` and the host freeze chain

**Files:**
- Create: `programs/auction_engine/src/solvency.rs`
- Modify: `programs/auction_engine/src/lib.rs`, `programs/auction_engine/core/src/{instruction.rs,lib.rs}` (append `CheckSolvency` = 16; remove the "later tasks append" comment), guest (append `check_solvency`), `idl/auction_engine-idl.json`
- Modify: `programs/integration_tests/tests/common/mod.rs` (append), `programs/integration_tests/tests/idl.rs` (assert all 17); Create: `programs/integration_tests/tests/solvency.rs`
- Modify: `docs/architecture/programs.md`

**Interfaces:**
- Consumes: Tasks 12–14 (`settle::{implicit_settle, buffer_burn}`, `system::{ref_price, protocol_token_status}`, harness `debt_world`-style setup via `World::{with_system, book_shortfall, create_protocol_token, start_debt}`); Task 4 (`stablecoin_core::Instruction::EngineFreezeNewDebt`, `compute_liquidation_engine_state_pda`); `auction_core::system::{check_solvency, start_debt, SolvencyDecision, SystemAuctionError}`.
- Produces: `solvency::{CheckSolvencyAccounts, check_solvency}`; `Instruction::CheckSolvency` (16); harness `World::check_solvency`. Chain on Freeze: [`Burn(buffer*)` if the implicit settle burns] → `stablecoin.engine_freeze_new_debt(engine_authority*, liquidation_engine_state, protocol_parameters)` with seed `["ENGINE_AUTH", config]` (invariant i: program id = `cfg.host_program_id`).

- [ ] **Step 1: Write the failing tests**

Append to `programs/integration_tests/tests/common/mod.rs`:

```rust
impl World {
    pub fn check_solvency(&mut self) -> Result<(), LeeError> {
        let cfg: auction_engine_core::state::EngineConfigState = self.read(config());
        let token_slot = cfg.protocol_token.unwrap_or_else(|| unused(SLOT_PROTOCOL_TOKEN));
        let ref_slot = self.ref_slot();
        self.check_solvency_with(token_slot, ref_slot)
    }

    pub fn check_solvency_with(&mut self, token_slot: AccountId, ref_slot: AccountId) -> Result<(), LeeError> {
        self.engine(
            vec![
                id(k::CALLER), config(), accounting(), buffer(), stable_def(), token_slot, ref_slot,
                engine_authority(), les(), protocol_parameters(), CLOCK_01_PROGRAM_ACCOUNT_ID,
            ],
            &[k::CALLER],
            Ix::CheckSolvency,
        )
    }

    pub fn host_frozen(&self) -> bool {
        self.read::<stablecoin_core::ProtocolParameters>(protocol_parameters()).is_frozen
    }
}
```

`programs/integration_tests/tests/solvency.rs`:

```rust
mod common;

use auction_engine_core::{
    args::{CollateralArgs, EngineParamsArgs},
    state::AccountingState,
};
use common::*;
use lee_core::account::AccountId;

fn token_params() -> EngineParamsArgs {
    EngineParamsArgs { protocol_token_enabled: true, ..EngineParamsArgs::defaults_for_tests() }
}

fn system_world(authority: Option<AccountId>, params: EngineParamsArgs) -> World {
    let mut w = World::new();
    w.init_engine_config(&params).unwrap();
    w.bind_engine().unwrap();
    w.set_collateral_config(k::ENGINE_ADMIN, &CollateralArgs::defaults_for_tests()).unwrap();
    if let Some(a) = authority {
        w.create_protocol_token(10_000, a).unwrap();
    }
    w.book_shortfall();
    w
}

#[test]
fn no_protocol_token_freezes_above_threshold_and_only_the_freeze_authority_unfreezes() {
    let mut w = World::with_engine();
    w.book_shortfall();
    w.check_solvency().expect("check_solvency");
    assert!(w.host_frozen());
    assert!(w.read::<AccountingState>(accounting()).host_frozen_by_engine);
    // The host's existing freeze semantics apply: new positions (and debt) are blocked.
    w.ensure_signer(k::OWNER2);
    w.fund_collateral(k::OWNER2_COLL, 1_000);
    let pos2 = position(k::OWNER2, 0);
    let r = w.host(
        vec![id(k::OWNER2), pos2, host_vault(pos2), id(k::OWNER2_COLL), collateral_def(), protocol_parameters(), clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID],
        &[k::OWNER2, k::OWNER2_COLL],
        stablecoin_core::Instruction::OpenPosition { position_nonce: 0, initial_collateral_amount: 1_000 },
    );
    assert_err(r, "Protocol is frozen");
    w.host(vec![id(k::FREEZE), protocol_parameters()], &[k::FREEZE], stablecoin_core::Instruction::Unfreeze)
        .expect("freeze authority unfreezes");
    assert!(!w.host_frozen());
}

#[test]
fn shortfall_at_threshold_does_not_freeze() {
    // A small shortfall (1_000 = threshold) must not freeze the stablecoin (spec §5.11 "why the threshold").
    let mut w = World::with_engine();
    let pos = w.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 0, 1_000, 1_000);
    w.set_redemption_price(250 * ONE);
    w.set_oracle(250 * ONE, w.now);
    w.liquidate(pos, 0).unwrap();
    w.settle_empty(pos).unwrap();
    assert_eq!(w.read::<AccountingState>(accounting()).unbacked_debt, 1_000);
    w.check_solvency().expect("healthy");
    assert!(!w.host_frozen());
}

#[test]
fn live_debt_auction_reverts_with_recovery_in_progress() {
    let mut w = system_world(Some(mint_authority()), token_params());
    w.start_debt(0).unwrap();
    assert_err(w.check_solvency(), "RecoveryInProgress");
    assert!(!w.host_frozen());
}

#[test]
fn unstartable_debt_auction_freezes() {
    for (authority, params) in [
        (Some(id(k::ATTACKER)), token_params()),                                                   // MintAuthorityMissing
        (None, token_params()),                                                                    // ProtocolTokenMismatch
        (Some(mint_authority()), EngineParamsArgs { protocol_token_ref_price: ONE / 100, ..token_params() }), // MintCapConfig
    ] {
        let mut w = system_world(authority, params);
        w.check_solvency().expect("check_solvency");
        assert!(w.host_frozen(), "authority {authority:?}");
    }
}

#[test]
fn working_protocol_token_never_freezes() {
    let mut w = system_world(Some(mint_authority()), token_params());
    w.check_solvency().expect("NoAction: a keeper should start a debt auction");
    assert!(!w.host_frozen());
}

#[test]
fn freeze_disabled_takes_no_action() {
    let mut w = system_world(Some(id(k::ATTACKER)), EngineParamsArgs { freeze_on_bad_debt: false, ..token_params() });
    w.check_solvency().unwrap();
    assert!(!w.host_frozen());
}

#[test]
fn implicit_settle_runs_before_the_decision() {
    let mut w = World::with_engine();
    w.book_shortfall();
    w.donate_stable(buffer(), 355_000);
    w.check_solvency().expect("healthy after the implicit settle");
    assert!(!w.host_frozen());
    assert_eq!(w.balance(buffer()), 0);
    assert_eq!(w.read::<AccountingState>(accounting()).unbacked_debt, 49);
}

#[test]
fn caller_cannot_substitute_the_protocol_token_account() {
    // ADR-030: a wrong slot account cannot turn a working token into "ProtocolTokenMismatch" → freeze.
    let mut w = system_world(Some(mint_authority()), token_params());
    let ref_slot = unused(0);
    assert_err(w.check_solvency_with(id(k::ATTACKER), ref_slot), "WrongAccount");
    assert!(!w.host_frozen());
}
```

In `programs/integration_tests/tests/idl.rs` add:

```rust
#[test]
fn engine_idl_is_complete() {
    assert_eq!(idl_names(), R2.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>());
}
```

Append to the core `tests` module: `#[test] fn solvency_index() { assert_eq!(idx(&Instruction::CheckSolvency), 16); }`

- [ ] **Step 2: Run and see the failure**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p engine_integration_tests --test solvency'`
Expected: compile error: no variant `CheckSolvency`.

- [ ] **Step 3: Append the variant (and drop the "later tasks append" comment)**

```rust
    /// Accounts: caller(s), config, accounting(m), buffer(m), stablecoin_definition(m), protocol_token_definition
    /// [slot 1], ref_oracle [slot 0], engine_authority, liquidation_engine_state, protocol_parameters(m), clock.
    CheckSolvency,
```

- [ ] **Step 4: Write `solvency.rs`**

```rust
//! `check_solvency()` (spec §5.11, ADR-017): implicit `settle_debt`, then the core decision. Freeze →
//! chain `stablecoin.engine_freeze_new_debt` signed by `["ENGINE_AUTH", config]`; RecoveryInProgress →
//! revert (freezes nothing); Healthy / NoAction → only the implicit settle is applied.

use auction_core::system::{check_solvency as decide, start_debt, SolvencyDecision, SystemAuctionError};
use auction_engine_core::{pda, state::*, EngineError};
use lee_core::{
    account::AccountWithMetadata,
    program::{ChainedCall, ProgramId},
};
use stablecoin_core::compute_liquidation_engine_state_pda;

use crate::{accounts::*, settle, system, token_calls, EngineResult};

pub struct CheckSolvencyAccounts {
    pub caller: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub accounting: AccountWithMetadata,
    pub buffer: AccountWithMetadata,
    pub stablecoin_definition: AccountWithMetadata,
    pub protocol_token_definition: AccountWithMetadata,
    pub ref_oracle: AccountWithMetadata,
    pub engine_authority: AccountWithMetadata,
    pub liquidation_engine_state: AccountWithMetadata,
    pub protocol_parameters: AccountWithMetadata,
    pub clock: AccountWithMetadata,
}

pub fn check_solvency(program_id: ProgramId, a: CheckSolvencyAccounts) -> EngineResult {
    if !a.caller.is_authorized {
        return Err(EngineError::WrongAccount("caller"));
    }
    let now = read_clock(&a.clock)?;
    let cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let acc = load_accounting(&program_id, &config_id, &a.accounting)?;
    let token = cfg.token_program_id;
    expect_id(&a.buffer, &pda::buffer_pda(&program_id, &config_id), "buffer")?;
    expect_id(&a.stablecoin_definition, &cfg.stablecoin_definition_id, "stablecoin_definition")?;
    let buffer_balance = fungible_balance(&a.buffer, &token, &cfg.stablecoin_definition_id)?;
    expect_id(&a.engine_authority, &pda::engine_authority_pda(&program_id, &config_id), "engine_authority")?;
    expect_id(&a.liquidation_engine_state, &compute_liquidation_engine_state_pda(cfg.host_program_id), "liquidation_engine_state")?;
    expect_id(&a.protocol_parameters, &cfg.host_protocol_parameters_id, "protocol_parameters")?;
    let status = system::protocol_token_status(&program_id, &config_id, &cfg, &a.protocol_token_definition)?;
    let reference = system::ref_price(&program_id, &config_id, &cfg, &a.ref_oracle, now)?;

    let (settled, burned) = settle::implicit_settle(&acc, buffer_balance);
    let buffer_after = buffer_balance.checked_sub(burned).ok_or(EngineError::Math)?;
    let start_check = if status.configured {
        start_debt(
            &settled.core(),
            buffer_after,
            true,
            status.mint_authority_ok,
            status.token_ok,
            reference,
            &cfg.system.to_core()?,
            now,
            acc.debt_live,
        )
        .map(|_| ())
    } else {
        Err(SystemAuctionError::TokenNotConfigured)
    };
    let decision = decide(
        settled.unbacked_debt,
        cfg.bad_debt_freeze_threshold,
        status.configured,
        cfg.freeze_on_bad_debt,
        acc.debt_live,
        start_check,
    );
    let mut calls: Vec<ChainedCall> = settle::buffer_burn(&cfg, &config_id, &a.stablecoin_definition, &a.buffer, burned)
        .into_iter()
        .collect();
    let next_acc = match decision {
        SolvencyDecision::RecoveryInProgress => return Err(EngineError::RecoveryInProgress),
        SolvencyDecision::Freeze => {
            calls.push(
                ChainedCall::new(
                    cfg.host_program_id,
                    vec![
                        token_calls::authorized(&a.engine_authority),
                        a.liquidation_engine_state.clone(),
                        a.protocol_parameters.clone(),
                    ],
                    &stablecoin_core::Instruction::EngineFreezeNewDebt,
                )
                .with_pda_seeds(vec![pda::engine_authority_seed(&config_id)]),
            );
            AccountingState { host_frozen_by_engine: true, ..settled }
        }
        SolvencyDecision::Healthy | SolvencyDecision::NoAction => settled,
    };
    Ok((
        vec![
            echo(&a.caller),
            echo(&a.config),
            write(&a.accounting, &next_acc),
            echo(&a.buffer),
            echo(&a.stablecoin_definition),
            echo(&a.protocol_token_definition),
            echo(&a.ref_oracle),
            echo(&a.engine_authority),
            echo(&a.liquidation_engine_state),
            echo(&a.protocol_parameters),
            echo(&a.clock),
        ],
        calls,
    ))
}
```

Add `pub mod solvency;` to `lib.rs`.

- [ ] **Step 5: Append the guest function**

```rust
    #[instruction]
    #[allow(clippy::too_many_arguments, reason = "eleven accounts (Shared Reference R2)")]
    pub fn check_solvency(
        ctx: ProgramContext,
        #[account(signer)] caller: AccountWithMetadata,
        config: AccountWithMetadata,
        #[account(mut, pda = [literal("ACCT"), account("config")])] accounting: AccountWithMetadata,
        #[account(mut, pda = [literal("BUFFER"), account("config")])] buffer: AccountWithMetadata,
        #[account(mut)] stablecoin_definition: AccountWithMetadata,
        protocol_token_definition: AccountWithMetadata,
        ref_oracle: AccountWithMetadata,
        #[account(pda = [literal("ENGINE_AUTH"), account("config")])] engine_authority: AccountWithMetadata,
        liquidation_engine_state: AccountWithMetadata,
        #[account(mut)] protocol_parameters: AccountWithMetadata,
        clock: AccountWithMetadata,
    ) -> SpelResult {
        let a = engine::solvency::CheckSolvencyAccounts {
            caller, config, accounting, buffer, stablecoin_definition, protocol_token_definition, ref_oracle,
            engine_authority, liquidation_engine_state, protocol_parameters, clock,
        };
        finish(engine::solvency::check_solvency(ctx.self_program_id, a))
    }
```

- [ ] **Step 6: Regenerate the IDL and run the whole engine suite**

Run:
```bash
MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh run -q -p idl-gen -- programs/auction_engine/methods/guest/src/bin/auction_engine.rs > idl/auction_engine-idl.json && scripts/wsl-cargo.sh test -p auction_engine_core -p auction_engine_program -p engine_integration_tests 2>&1 | grep -E "test result|FAILED|panicked"'
```
Expected: `solvency` 8 passed; `idl` 2 passed (17 instructions in R2 order); every other suite green.

- [ ] **Step 7: Docs**

Append to `docs/architecture/programs.md` a "`check_solvency`" section with the core decision table from `docs/architecture/core.md` ("check_solvency"), the program-layer facts (implicit settle first; the token-status and reference-oracle accounts are pinned, so a caller cannot force a freeze; Freeze chains `engine_freeze_new_debt`; RecoveryInProgress reverts; the host's freeze authority alone unfreezes), and the F4 interpretation note (freeze only above the absolute `bad_debt_freeze_threshold`, ADR-017).

- [ ] **Step 8: Commit (controller — GPG signing on Windows)**

The implementer stages the files and reports; the controller commits from Git Bash.

```bash
git add programs/auction_engine programs/integration_tests idl/auction_engine-idl.json docs/architecture/programs.md
git commit -m "feat(engine): check_solvency with implicit settle and host freeze chain"
```

---

### Task 16: Attack tests — impostor program, rogue config, laundering surface, private-path Variant C

**Files:**
- Create: `programs/test_programs/core/{Cargo.toml,src/lib.rs}` (`test_programs_core::ImpostorInstruction`)
- Create: `programs/test_programs/methods/{Cargo.toml,build.rs,src/lib.rs}`, `programs/test_programs/methods/guest/{Cargo.toml,Cargo.lock,src/bin/impostor.rs}`
- Modify: root `Cargo.toml` (`members` += `"programs/test_programs/core"`, `"programs/test_programs/methods"`; `exclude` += `"programs/test_programs/methods/guest"`), `programs/integration_tests/Cargo.toml` (+ `test-programs-methods`, `test_programs_core`)
- Create: `programs/integration_tests/tests/security.rs`
- Modify: `docs/design-log.md` (ADR-034), `docs/architecture/programs.md`

**Interfaces:**
- Consumes: Tasks 4–15 (hooks, every engine instruction, harness `World::{with_engine, unsafe_position, liquidate_accounts, host_frozen}`, `engine_program`, `host_program`, `token_program`); `auction_engine_core::pda::{seed_from_str, combine_seeds, config_pda, coll_config_pda, accounting_pda, buffer_pda, liqrec_pda, auction_pda, vault_pda, engine_authority_pda, unused_slot_pda}`; `lee::{execute_and_prove, privacy_preserving_transaction::circuit::ProgramWithDependencies, program::Program}`, `lee_core::InputAccountIdentity` (upstream usage: `programs/upstream_integration_tests/tests/stablecoin.rs:758-850`).
- Produces: guest `impostor` with `attack_freeze(host_program_id: ProgramId, mode: u8, seed: [u8; 32])` (mode 0: forge `is_authorized` on the given authority, no seeds; mode 1: authorise the given account by attaching `seed` under the impostor's own id); `test_programs_methods::{IMPOSTOR_ELF, IMPOSTOR_ID}`. Tests named in Step 1.

- [ ] **Step 1: Write the attack tests (they must fail before the guest exists)**

`programs/test_programs/core/Cargo.toml`:

```toml
[package]
name = "test_programs_core"
version = "0.1.0"
edition.workspace = true
license.workspace = true
publish = false

[dependencies]
lee_core = { git = "https://github.com/logos-blockchain/logos-execution-zone.git", tag = "v0.2.4", features = ["host"] }
serde = { version = "1.0", features = ["derive"] }
```

`programs/test_programs/core/src/lib.rs`:

```rust
//! Attacker programs used only by tests (never deployed outside tests).
#![forbid(unsafe_code)]

use lee_core::program::ProgramId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImpostorInstruction {
    /// Chains `stablecoin.engine_freeze_new_debt`. mode 0: forge `is_authorized` on `authority` with no
    /// seeds; mode 1: attach `seed` (authorises `for_public_pda(IMPOSTOR_ID, seed)` legitimately).
    AttackFreeze { host_program_id: ProgramId, mode: u8, seed: [u8; 32] },
}
```

`programs/integration_tests/tests/security.rs`:

```rust
mod common;

use std::collections::HashMap;

use auction_engine_core::{
    args::{CollateralArgs, EngineParamsArgs},
    pda,
    state::EngineConfigState,
    Instruction as Ix,
};
use common::*;
use lee::{execute_and_prove, privacy_preserving_transaction::circuit::ProgramWithDependencies, program::Program};
use lee_core::{
    account::{AccountId, AccountWithMetadata},
    program::PdaSeed,
    InputAccountIdentity,
};
use stablecoin_core::Position;
use test_programs_core::ImpostorInstruction;

fn auth_seed_bytes(config_id: &AccountId) -> [u8; 32] {
    pda::combine_seeds(&[&pda::seed_from_str("ENGINE_AUTH"), config_id.value()])
}

fn impostor_world() -> World {
    let mut w = World::with_engine();
    w.deploy(test_programs_methods::IMPOSTOR_ELF);
    w
}

#[test]
fn impostor_forging_the_authority_flag_is_rejected() {
    // The host's immediate-caller check fires first (S2 table, variant C, attack 3a); were it absent,
    // lee would still reject the forged flag (InvalidAccountAuthorization).
    let mut w = impostor_world();
    let r = w.submit(
        test_programs_methods::IMPOSTOR_ID,
        vec![engine_authority(), les(), protocol_parameters()],
        &[k::ATTACKER],
        ImpostorInstruction::AttackFreeze { host_program_id: host_program(), mode: 0, seed: [0; 32] },
    );
    assert_err(r, "Caller program is not the liquidation engine");
    assert!(!w.host_frozen());
}

#[test]
fn impostor_own_pda_is_rejected_by_the_immediate_caller_check() {
    let mut w = impostor_world();
    let seed = auth_seed_bytes(&config());
    let own = AccountId::for_public_pda(&test_programs_methods::IMPOSTOR_ID, &PdaSeed::new(seed));
    let r = w.submit(
        test_programs_methods::IMPOSTOR_ID,
        vec![own, les(), protocol_parameters()],
        &[k::ATTACKER],
        ImpostorInstruction::AttackFreeze { host_program_id: host_program(), mode: 1, seed },
    );
    assert_err(r, "Caller program is not the liquidation engine");
    assert!(!w.host_frozen());
}

#[test]
fn rogue_config_with_the_same_salt_cannot_liquidate_the_bound_host() {
    // ADR-027 (I7): anyone may create a config on the same host, even with the bound config's salt; it is a
    // different PDA (the admin is in the seed) and only the bound config's authority passes the hook.
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    let e = engine_program();
    let rogue = pda::config_pda(&e, &id(k::ATTACKER), SALT);
    assert_ne!(rogue, config());
    let rogue_params = EngineParamsArgs { global_cap: 100_000_000, ..EngineParamsArgs::defaults_for_tests() };
    w.engine(
        vec![
            id(k::ATTACKER), rogue, pda::accounting_pda(&e, &rogue), pda::buffer_pda(&e, &rogue),
            protocol_parameters(), stable_def(), pda::unused_slot_pda(&e, &rogue, 1), pda::unused_slot_pda(&e, &rogue, 0),
        ],
        &[k::ATTACKER],
        init_config_ix(SALT, &rogue_params),
    )
    .expect("anyone can create a config");
    let greedy = CollateralArgs { penalty_bps: 2_500, tip: 1_000, chip_bps: 2_500, reward_cap: 1_000_000, ..CollateralArgs::defaults_for_tests() };
    w.engine(
        vec![
            id(k::ATTACKER), rogue, collateral_def(), pda::coll_config_pda(&e, &rogue, &collateral_def()),
            oracle(), protocol_parameters(), redemption_state(), clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        &[k::ATTACKER],
        collateral_ix(&greedy),
    )
    .expect("rogue collateral config");
    w.holding(k::KEEPER_COLL, collateral_def(), 0);
    let auction_id = pda::auction_pda(&e, &rogue, &pos, 0);
    let r = w.engine(
        vec![
            id(k::KEEPER), id(k::KEEPER_COLL), rogue, pda::coll_config_pda(&e, &rogue, &collateral_def()), pos,
            pda::liqrec_pda(&e, &rogue, &pos), auction_id, pda::vault_pda(&e, &auction_id),
            pda::engine_authority_pda(&e, &rogue), les(), host_vault(pos), collateral_def(), accumulator(),
            redemption_state(), oracle(), protocol_parameters(), clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        &[k::KEEPER],
        Ix::Liquidate { nonce: 0 },
    );
    assert_err(r, "Engine authority account is not the bound engine authority");
    let p: Position = w.read(pos);
    assert_eq!(p.collateral_amount, LIQ_COLLATERAL, "position untouched");
}

#[test]
fn engine_takes_no_program_id_argument() {
    // Documentation only (M5): records that no instruction argument is a program id. Invariant (i) itself
    // is enforced in code (`cfg.host_program_id` / `cfg.token_program_id` only) and reviewed in Task 19.
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../idl/auction_engine-idl.json");
    let idl = std::fs::read_to_string(path).expect("engine IDL");
    assert!(!idl.contains("program_id"), "an engine instruction takes a program id argument");
}

#[test]
fn fake_host_config_is_accepted_so_clients_must_verify_program_ids() {
    // I4: init_config takes the host program id from the ProtocolParameters owner. A config built on a
    // fake "host" is accepted; every later engine call would chain to that program (and to its token
    // program) with the signer flags the user gives. The defence is client-side: the SDK/CLI refuses any
    // config whose host_program_id / token_program_id differ from the ImageIDs in DEPLOYMENTS.md.
    let mut w = World::with_engine();
    w.deploy(test_programs_methods::IMPOSTOR_ELF);
    let fake_host = test_programs_methods::IMPOSTOR_ID;
    let fake_pp = stablecoin_core::compute_protocol_parameters_pda(fake_host);
    let mut pp = w.state.get_account_by_id(protocol_parameters());
    pp.program_owner = fake_host;
    w.state.force_insert_account(fake_pp, pp);
    let e = engine_program();
    let fake_cfg = pda::config_pda(&e, &id(k::ATTACKER), 7);
    w.engine(
        vec![
            id(k::ATTACKER), fake_cfg, pda::accounting_pda(&e, &fake_cfg), pda::buffer_pda(&e, &fake_cfg), fake_pp,
            stable_def(), pda::unused_slot_pda(&e, &fake_cfg, 1), pda::unused_slot_pda(&e, &fake_cfg, 0),
        ],
        &[k::ATTACKER],
        init_config_ix(7, &EngineParamsArgs::defaults_for_tests()),
    )
    .expect("the engine cannot tell a fake host from a real one");
    let cfg: EngineConfigState = w.read(fake_cfg);
    assert_eq!(cfg.host_program_id, fake_host);
    assert_ne!(cfg.host_program_id, host_program(), "a client checking DEPLOYMENTS.md refuses this config");
}

#[test]
fn engine_authority_cannot_be_signed_at_top_level() {
    // A user transaction listing the engine authority cannot authorise it (no key exists for a PDA).
    let mut w = World::with_engine();
    let r = w.host(vec![engine_authority(), les(), protocol_parameters()], &[], stablecoin_core::Instruction::EngineFreezeNewDebt);
    assert_err(r, "must be a signer"); // SPEL's `#[account(signer)]` check, before the handler runs
}

fn with_deps(main_elf: &[u8]) -> ProgramWithDependencies {
    ProgramWithDependencies::new(
        Program::new(main_elf.to_vec().into()).expect("valid ELF"),
        HashMap::from([
            (host_program(), Program::new(stablecoin_methods::STABLECOIN_ELF.to_vec().into()).expect("stablecoin ELF")),
            (token_program(), Program::new(token_methods::TOKEN_ELF.to_vec().into()).expect("token ELF")),
        ]),
    )
}

fn public_pre(w: &World, ids: &[AccountId], signers: &[AccountId]) -> Vec<AccountWithMetadata> {
    ids.iter().map(|i| AccountWithMetadata::new(w.state.get_account_by_id(*i), signers.contains(i), *i)).collect()
}

#[test]
fn private_path_impostor_is_rejected_by_the_immediate_caller_check() {
    // ADR-019 open item: caller_program_id is enforced per call in the privacy circuit too.
    let w = World::with_engine();
    let seed = auth_seed_bytes(&config());
    let own = AccountId::for_public_pda(&test_programs_methods::IMPOSTOR_ID, &PdaSeed::new(seed));
    let ids = [own, les(), protocol_parameters()];
    let result = execute_and_prove(
        public_pre(&w, &ids, &[]),
        Program::serialize_instruction(ImpostorInstruction::AttackFreeze { host_program_id: host_program(), mode: 1, seed })
            .expect("instruction"),
        vec![InputAccountIdentity::Public, InputAccountIdentity::Public, InputAccountIdentity::Public],
        &with_deps(test_programs_methods::IMPOSTOR_ELF),
    );
    let text = format!("{:?}", result.expect_err("the impostor must be rejected on the private path"));
    println!("PRIVATE-PATH impostor: {text}");
    assert!(text.contains("Caller program is not the liquidation engine"), "{text}");
}

#[test]
fn private_path_liquidate_is_not_expressible_on_v0_2_4() {
    // ADR-034: the auction vault is authorised by its seed in InitializeAccount and passed unauthorised to
    // the hook; the circuit's transaction-wide authorisation set rejects that, as it rejects upstream
    // open_position (programs/upstream_integration_tests/tests/stablecoin.rs:775-850).
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    w.holding(k::KEEPER_COLL, collateral_def(), 0);
    let ids = w.liquidate_accounts(pos, 0, id(k::KEEPER_COLL));
    let identities = ids.iter().map(|_| InputAccountIdentity::Public).collect();
    let result = execute_and_prove(
        public_pre(&w, &ids, &[id(k::KEEPER)]),
        Program::serialize_instruction(Ix::Liquidate { nonce: 0 }).expect("instruction"),
        identities,
        &with_deps(auction_engine_methods::AUCTION_ENGINE_ELF),
    );
    let text = format!("{:?}", result.expect_err("liquidate is not private-path capable on v0.2.4"));
    println!("PRIVATE-PATH liquidate: {text}");
    assert!(text.contains("Inconsistent authorization for account"), "{text}");
}
```

Add to `programs/integration_tests/Cargo.toml` `[dependencies]`: `test_programs_core = { path = "../test_programs/core" }`, `test-programs-methods = { path = "../test_programs/methods" }`.

- [ ] **Step 2: Run and see the failure**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p engine_integration_tests --test security'`
Expected: build error: package `test-programs-methods` not found (path does not exist yet).

- [ ] **Step 3: Write the impostor guest and its methods crate**

`programs/test_programs/methods/Cargo.toml`, `build.rs`, `src/lib.rs`: identical in shape to `programs/auction_engine/methods` (package `test-programs-methods`, `risc0-build = "=3.0.5"`, `[package.metadata.risc0] methods = ["guest"]`, `include!(concat!(env!("OUT_DIR"), "/methods.rs"));`).

`programs/test_programs/methods/guest/Cargo.toml`:

```toml
[package]
name = "test-programs-guest"
version = "0.1.0"
edition = "2021"

[workspace]

[profile.release]
debug = 0
strip = "symbols"

[[bin]]
name = "impostor"
path = "src/bin/impostor.rs"

[dependencies]
spel-framework = { git = "https://github.com/logos-co/spel.git", rev = "1ef0500f8fc8ce3ddf95523726ae159db83f744f", package = "spel-framework" }
nssa_core = { git = "https://github.com/logos-blockchain/logos-execution-zone.git", tag = "v0.2.4", package = "lee_core" }
risc0-zkvm = { version = "=3.0.5", default-features = false }
test_programs_core = { path = "../../core" }
stablecoin_core = { path = "../../../stablecoin/core" }
serde = { version = "1.0", features = ["derive"] }
borsh = "1.5"
```

`programs/test_programs/methods/guest/src/bin/impostor.rs`:

```rust
#![cfg_attr(not(test), no_main)]

use spel_framework::prelude::*;

#[cfg(not(test))]
risc0_zkvm::guest::entry!(main);

#[lez_program(instruction = "test_programs_core::ImpostorInstruction")]
mod impostor {
    #[allow(unused_imports)]
    use super::*;

    #[instruction]
    pub fn attack_freeze(
        authority: AccountWithMetadata,
        liquidation_engine_state: AccountWithMetadata,
        #[account(mut)] protocol_parameters: AccountWithMetadata,
        host_program_id: ProgramId,
        mode: u8,
        seed: [u8; 32],
    ) -> SpelResult {
        let mut claimed = authority.clone();
        claimed.is_authorized = true;
        let seeds = if mode == 1 { vec![PdaSeed::new(seed)] } else { vec![] };
        let call = ChainedCall::new(
            host_program_id,
            vec![claimed, liquidation_engine_state.clone(), protocol_parameters.clone()],
            &stablecoin_core::Instruction::EngineFreezeNewDebt,
        )
        .with_pda_seeds(seeds);
        Ok(spel_framework::SpelOutput::execute(
            vec![
                AccountPostState::new(authority.account),
                AccountPostState::new(liquidation_engine_state.account),
                AccountPostState::new(protocol_parameters.account),
            ],
            vec![call],
        ))
    }
}
```

Seed the guest lock: `cp programs/stablecoin/methods/guest/Cargo.lock programs/test_programs/methods/guest/Cargo.lock` then `cargo metadata --manifest-path programs/test_programs/methods/guest/Cargo.toml --format-version 1 > /dev/null` inside WSL (as in Task 8 Step 7). Add the members/exclude to the root `Cargo.toml`.

- [ ] **Step 4: Run the attack tests**

Run: the Step 2 command. Expected: `security` 8 passed. If `private_path_impostor_is_rejected_by_the_immediate_caller_check` fails only on the message (the circuit may report the guest panic differently), keep `expect_err`, replace the `contains` assertion with the printed text's stable fragment, and quote that text in ADR-034; if it returns `Ok`, stop and report to the controller — Variant C would not hold on the private path.

- [ ] **Step 5: ADR-034 and docs**

Append to `docs/design-log.md`:

```markdown
---

## ADR-034: Private Path: Variant C Holds; `liquidate` Is Public-Only on v0.2.4

**Context**
ADR-019 left a private-path test of Variant C open (the circuit keeps one authorised set per transaction). Spec §8: the PoC submits public transactions only; the private SDK is sub-project B (milestone M4).

**Decision**
Two in-process `execute_and_prove` tests (all-public identities, `security.rs`): (1) an impostor chaining to `engine_freeze_new_debt` with its own legitimately authorised PDA is rejected by the hook's immediate-caller check, so Variant C holds on the private path; (2) `liquidate` is rejected by the circuit with "Inconsistent authorization for account": the auction vault is authorised by its seed in `InitializeAccount` and passed unauthorised to the hook, which the transaction-wide set forbids — the same limitation the upstream `open_position` has. Making `liquidate` private-capable needs a different vault-initialisation shape (e.g. the hook initialising custody) and is recorded for sub-project B; nothing in the PoC depends on it.

**Trade-offs**
Private liquidation is not available on v0.2.4 with this chain shape. Bids, claims and system auctions were not exercised on the private path in Plan 2.

**Status**
Accepted (Plan 2, Task 16)
```

Append to `docs/architecture/programs.md` a "Security tests" section listing the eight tests and the attack each one covers (forged flag, impostor PDA, rogue config with the same salt, laundering surface via program-id arguments (documentation only), fake-host config (client-side check, I4), top-level PDA, private-path impostor, private-path liquidate).

- [ ] **Step 6: Commit (controller — GPG signing on Windows)**

The implementer stages the files and reports; the controller commits from Git Bash.

```bash
git add Cargo.toml Cargo.lock programs/test_programs programs/integration_tests docs/design-log.md docs/architecture/programs.md
git commit -m "test(security): impostor, rogue config, laundering surface and private-path Variant C tests; ADR-034"
```

---

### Task 17: Account-level invariants, R3 isolation and the chained-call budget of every instruction

**Files:**
- Create: `programs/integration_tests/tests/common/measure.rs`, `programs/integration_tests/tests/chains.rs`, `programs/integration_tests/tests/invariants.rs`
- Modify: `programs/integration_tests/tests/common/mod.rs` (add `pub mod measure;` and the snapshot helper)
- Modify: `docs/design-log.md` (ADR-036), `docs/architecture/programs.md`

**Interfaces:**
- Consumes: every engine instruction and harness helper from Tasks 7–15; spike measurement method (`spikes/s2_s3_s7/spike_tests/tests/common/mod.rs:136-230`, same as S1 step 6).
- Produces: `measure::{measure(label, &V03State, ProgramId, &[AccountId], &[&PrivateKey], &I) -> Measured, Measured { per_exec, total }}`; `World::measured(label, accounts, signers, ix) -> (usize, Result<(), LeeError>)`; the chain table in ADR-036. Invariants checked: spec §5.13 2, 7, 9, 10, 11, 12, 17 and R3.

- [ ] **Step 1: Write the measurement helper and the failing chain test**

`programs/integration_tests/tests/common/measure.rs`:

```rust
//! Replays a transaction's whole chain with the risc0 executor (S1 step 6 / S2 method) and prints one
//! line per program execution. Executor cycles of natively built guests, dev mode: not proving cost.

use std::collections::VecDeque;

use lee::{program::Program, PrivateKey, PublicKey, V03State};
use lee_core::{
    account::{AccountId, AccountWithMetadata},
    program::{ChainedCall, ProgramId, ProgramOutput},
};
use risc0_zkvm::{default_executor, ExecutorEnv};
use serde::Serialize;

pub struct Measured {
    pub per_exec: Vec<(&'static str, u64)>,
    pub total: u64,
}

fn elf_for(program_id: ProgramId) -> (&'static str, &'static [u8]) {
    if program_id == token_methods::TOKEN_ID {
        ("token", token_methods::TOKEN_ELF)
    } else if program_id == stablecoin_methods::STABLECOIN_ID {
        ("stablecoin", stablecoin_methods::STABLECOIN_ELF)
    } else if program_id == auction_engine_methods::AUCTION_ENGINE_ID {
        ("auction_engine", auction_engine_methods::AUCTION_ENGINE_ELF)
    } else {
        panic!("unknown program in chain")
    }
}

fn run_one(call: &ChainedCall, caller: Option<ProgramId>) -> (&'static str, ProgramOutput, u64) {
    let (name, elf) = elf_for(call.program_id);
    let mut builder = ExecutorEnv::builder();
    builder.session_limit(Some(32 * 1024 * 1024));
    builder.write(&call.program_id).unwrap();
    builder.write(&caller).unwrap();
    builder.write(&call.pre_states).unwrap();
    builder.write(&call.instruction_data).unwrap();
    let info = default_executor().execute(builder.build().unwrap(), elf).expect("guest executes");
    let out: ProgramOutput = info.journal.decode().unwrap();
    (name, out, info.cycles())
}

pub fn measure<I: Serialize>(
    label: &str,
    state: &V03State,
    program_id: ProgramId,
    accounts: &[AccountId],
    signers: &[&PrivateKey],
    instruction: &I,
) -> Measured {
    let signer_ids: Vec<AccountId> = signers.iter().map(|k| AccountId::from(&PublicKey::new_from_private_key(k))).collect();
    let first = ChainedCall {
        program_id,
        instruction_data: Program::serialize_instruction(instruction).unwrap(),
        pre_states: accounts
            .iter()
            .map(|id| AccountWithMetadata::new(state.get_account_by_id(*id), signer_ids.contains(id), *id))
            .collect(),
        pda_seeds: vec![],
    };
    let mut queue = VecDeque::from([(first, None, 0usize)]);
    let (mut per_exec, mut total) = (Vec::new(), 0u64);
    while let Some((call, caller, depth)) = queue.pop_front() {
        let (name, out, cycles) = run_one(&call, caller);
        println!("CYCLES[{label}] exec#{} depth={depth} program={name} cycles={cycles}", per_exec.len() + 1);
        per_exec.push((name, cycles));
        total += cycles;
        for c in out.chained_calls {
            queue.push_back((c, Some(call.program_id), depth + 1));
        }
    }
    println!("CYCLES[{label}] TOTAL executions={} cycles={total}", per_exec.len());
    Measured { per_exec, total }
}
```

Append to `programs/integration_tests/tests/common/mod.rs`:

```rust
pub mod measure;

impl World {
    /// Measures the chain (executions, cycles), then submits. Returns (executions, result).
    pub fn measured(&mut self, label: &str, accounts: Vec<AccountId>, signers: &[u8], ix: Ix) -> (usize, Result<(), LeeError>) {
        self.seed_clock();
        let keys: Vec<PrivateKey> = signers.iter().map(|n| key(*n)).collect();
        let refs: Vec<&PrivateKey> = keys.iter().collect();
        let m = measure::measure(label, &self.state, engine_program(), &accounts, &refs, &ix);
        (m.per_exec.len(), self.engine(accounts, signers, ix))
    }

    /// Raw bytes of an account (R3 isolation checks).
    pub fn raw(&self, account: AccountId) -> Account {
        self.state.get_account_by_id(account)
    }
}
```

`programs/integration_tests/tests/chains.rs`:

```rust
mod common;

use auction_engine_core::{args::CollateralArgs, pda::SLOT_REF_ORACLE, Instruction as Ix};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID as CLOCK;
use common::*;

/// Shared Reference R3: every instruction fits the private-path limit (10 executions).
#[test]
fn every_instruction_fits_the_chain_budget() {
    let mut w = World::with_system(false);
    let mut table: Vec<(&str, usize)> = Vec::new();
    let mut record = |name: &'static str, (n, r): (usize, Result<(), lee::error::LeeError>)| {
        r.unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert!(n <= 10, "{name}: {n} executions");
        table.push((name, n));
    };
    record("observe_feed", w.measured("observe_feed", vec![id(k::CALLER), config(), coll_config(), redemption_state(), oracle(), CLOCK], &[k::CALLER], Ix::ObserveFeed));
    let c = CollateralArgs::defaults_for_tests();
    record("set_collateral_config", w.measured("set_collateral_config", vec![id(k::ENGINE_ADMIN), config(), collateral_def(), coll_config(), oracle(), protocol_parameters(), redemption_state(), CLOCK], &[k::ENGINE_ADMIN], collateral_ix(&c)));
    let pos = w.unsafe_position();
    w.holding(k::KEEPER_COLL, collateral_def(), 0);
    record("liquidate", w.measured("liquidate", w.liquidate_accounts(pos, 0, id(k::KEEPER_COLL)), &[k::KEEPER], Ix::Liquidate { nonce: 0 }));
    w.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 1_000_000);
    record("bid", w.measured("bid", w.bid_accounts(pos, 0, k::BIDDER_STABLE, k::BIDDER_COLL, false), &[k::BIDDER_STABLE], Ix::Bid { coin_in_max: 100_000, min_collateral_out: 0 }));
    record("bid_with_settlement(final)", w.measured("bid_final", w.bid_accounts(pos, 0, k::BIDDER_STABLE, k::BIDDER_COLL, true), &[k::BIDDER_STABLE], Ix::BidWithSettlement { coin_in_max: 560_000, min_collateral_out: 0 }));
    record("claim_leftover", w.measured("claim_leftover", vec![id(k::OWNER), id(k::OWNER_COLL), config(), auction(pos, 0), auction_vault(pos, 0)], &[k::OWNER], Ix::ClaimLeftover));
    w.donate_stable(buffer(), 1_500);
    record("start_surplus_auction", w.measured("start_surplus", vec![id(k::CALLER), config(), accounting(), buffer(), stable_def(), surplus_auction(0), surplus_vault(0), unused(SLOT_REF_ORACLE), CLOCK], &[k::CALLER], Ix::StartSurplusAuction { nonce: 0 }));
    record("take_surplus", w.measured("take_surplus", vec![id(k::TAKER_PROTOCOL), id(k::TAKER_STABLE), config(), accounting(), surplus_auction(0), surplus_vault(0), protocol_token_def(), CLOCK], &[k::TAKER_PROTOCOL], Ix::TakeSurplus { amount_coin: 250, max_protocol_in: u128::MAX }));
    w.advance(14_400_000);
    record("close_surplus_auction", w.measured("close_surplus", vec![id(k::CALLER), config(), accounting(), surplus_auction(0), surplus_vault(0), buffer(), CLOCK], &[k::CALLER], Ix::CloseSurplusAuction));
    record("settle_debt", w.measured("settle_debt", vec![id(k::CALLER), config(), accounting(), buffer(), stable_def()], &[k::CALLER], Ix::SettleDebt { amount: u128::MAX }));
    println!("CHAIN TABLE {table:?}");
    let get = |n: &str| table.iter().find(|(name, _)| *name == n).map(|(_, c)| *c).expect("measured");
    assert_eq!(get("liquidate"), 5);
    assert_eq!(get("bid_with_settlement(final)"), 4);
    assert_eq!(get("take_surplus"), 3);
}

#[test]
fn debt_and_solvency_chains_fit_the_budget() {
    let mut w = World::with_system(false);
    w.book_shortfall();
    w.fund_stable(k::TAKER_STABLE, 1_000);
    let (n, r) = w.measured("start_debt", vec![id(k::CALLER), config(), accounting(), buffer(), stable_def(), debt_auction(0), protocol_token_def(), unused(SLOT_REF_ORACLE), CLOCK], &[k::CALLER], Ix::StartDebtAuction { nonce: 0 });
    r.unwrap();
    assert!(n <= 10);
    let (n, r) = w.measured("take_debt", vec![id(k::TAKER_STABLE), id(k::TAKER_PROTOCOL), config(), accounting(), debt_auction(0), stable_def(), protocol_token_def(), mint_authority(), CLOCK], &[k::TAKER_STABLE], Ix::TakeDebt { amount_coin: 500, min_protocol_out: 0 });
    r.unwrap();
    assert_eq!(n, 3);
    let mut w = World::with_engine();
    w.book_shortfall();
    let (n, r) = w.measured("check_solvency(freeze)", vec![id(k::CALLER), config(), accounting(), buffer(), stable_def(), unused(1), unused(0), engine_authority(), les(), protocol_parameters(), CLOCK], &[k::CALLER], Ix::CheckSolvency);
    r.unwrap();
    assert_eq!(n, 2, "engine + freeze hook (no implicit burn: empty buffer)");
}
```

- [ ] **Step 2: Run the chain tests**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p engine_integration_tests --test chains -- --nocapture --test-threads=1 2>&1 | grep -E "CYCLES|CHAIN TABLE|test result"'`
Expected: 2 passed; the `CYCLES[...]` lines are printed. A failure here means a chain exceeds the budget or an expected count differs: fix the handler, not the expectation (the expectations are Shared Reference R3).

- [ ] **Step 3: Write the account-level invariant tests**

`programs/integration_tests/tests/invariants.rs`:

```rust
//! Spec §5.13 invariants that need accounts (Plan 1 left them to Plan 2): 2, 7, 9, 10, 11, 12, 17 and R3.

mod common;

use auction_engine_core::state::{AccountingState, AuctionState, CollateralConfigState, EngineConfigState, SurplusAuctionState};
use common::*;
use lee_core::account::AccountId;
use stablecoin_core::Position;

struct Tracked {
    auctions: Vec<AccountId>,      // positions with an auction at nonce 0
    seized_from_host: u128,        // host vault balances swept (invariant 10)
    debt_from_host: u128,          // normalized debt covered (accumulator 1.0 → equals nominal; invariant 10)
}

fn check_all(w: &World, t: &Tracked, label: &str) {
    let cfg: EngineConfigState = w.read(config());
    let coll: CollateralConfigState = w.read(coll_config());
    let acc: AccountingState = w.read(accounting());
    let mut live_reserved = 0u128;
    let mut live_penalty = 0u128;
    let mut live_debt_open = 0u128;
    let mut seized = 0u128;
    let mut principal = 0u128;
    for pos in &t.auctions {
        let a: AuctionState = w.read(auction(*pos, 0));
        let vault = w.balance(auction_vault(*pos, 0));
        seized += a.collateral_seized;
        principal += a.debt_principal;
        // Invariant 2: vault ≥ book.
        let book = if a.settled { a.leftover_claimable - a.leftover_claimed } else { a.collateral_left };
        assert!(vault >= book, "{label}: inv 2 vault {vault} < book {book}");
        if !a.settled {
            live_reserved += a.reserved_amount;
            live_penalty += a.penalty_raised;
            live_debt_open += a.debt_principal - a.debt_raised;
        }
        // Invariant 10 (per position): the host side was zeroed.
        let p: Position = w.read(*pos);
        assert_eq!((p.collateral_amount, p.normalized_debt_amount), (0, 0), "{label}: inv 10 host position");
    }
    // Invariant 7: Σ live reserved == type_reserved == global_reserved (one type per instance).
    assert_eq!(live_reserved, coll.type_reserved, "{label}: inv 7 type");
    assert_eq!(coll.type_reserved, cfg.global_reserved, "{label}: inv 7 global");
    // Invariant 10: Σ seized == Σ host vault balances swept.
    assert_eq!(seized, t.seized_from_host, "{label}: inv 10 collateral");
    // Invariant 10: Σ debt_principal == Σ host debt seized.
    assert_eq!(principal, t.debt_from_host, "{label}: inv 10 debt");
    // Invariant 9: actual buffer ≥ book buffer.
    let book_buffer = (acc.total_penalty_collected + live_penalty + acc.total_surplus_returned)
        - acc.total_debt_settled
        - acc.total_surplus_lots_out;
    assert!(w.balance(buffer()) >= book_buffer, "{label}: inv 9 buffer");
    // Invariant 11 (accumulator stays 1.0 in the harness, so nominal == normalized).
    let positions_debt: u128 = [position(k::OWNER, 0), position(k::OWNER2, 0)]
        .iter()
        .map(|p| w.read::<Position>(*p).normalized_debt_amount)
        .sum();
    assert!(
        w.supply(stable_def()) <= positions_debt + live_debt_open + acc.unbacked_debt + w.injected_stable,
        "{label}: inv 11 supply"
    );
}

fn lcg(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
    *seed >> 33
}

#[test]
fn collateral_invariants_hold_over_random_bids() {
    for mut seed in [1u64, 7, 42] {
        let mut w = World::with_system(false);
        let big = w.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 0, LIQ_COLLATERAL, LIQ_DEBT);
        let small = w.open_small_position();
        w.set_min_cr(2 * ONE);
        let mut t = Tracked { auctions: vec![], seized_from_host: 0, debt_from_host: 0 };
        for pos in [big, small] {
            t.seized_from_host += w.balance(host_vault(pos));
            t.debt_from_host += w.read::<Position>(pos).normalized_debt_amount;
            w.liquidate(pos, 0).unwrap();
            t.auctions.push(pos);
            check_all(&w, &t, "after liquidate");
        }
        w.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 10_000_000);
        let mut ok_bids = 0u32;
        for step in 0..12 {
            w.advance(lcg(&mut seed) % 600_000);
            w.set_oracle(R, w.now);
            let pos = t.auctions[(lcg(&mut seed) % 2) as usize];
            let coin = 25 + u128::from(lcg(&mut seed)) % 200_000;
            // Dust/min-bid/not-auctioning rejections are fine here; count the bids that landed.
            if w.bid_settle(pos, coin, 0).is_ok() {
                ok_bids += 1;
            }
            check_all(&w, &t, &format!("seed {seed} step {step}"));
        }
        assert!(ok_bids > 0, "seed {seed}: no random bid succeeded, the test exercised nothing");
        for pos in t.auctions.clone() {
            let _ = w.bid_settle(pos, u128::MAX, 0);
        }
        for pos in &t.auctions {
            assert!(w.read::<AuctionState>(auction(*pos, 0)).settled, "seed {seed}: every auction settled");
        }
        check_all(&w, &t, "after settling everything");
        // F5 on the collected penalties (invariant 9 with lots out and remainders back).
        w.start_surplus(0).unwrap();
        w.take_surplus(0, 250, u128::MAX).unwrap();
        w.advance(14_400_000);
        w.close_surplus(0).unwrap();
        check_all(&w, &t, "after surplus auction");
    }
}

#[test]
fn liveness_unsafe_position_is_liquidatable_within_caps_or_alone() {
    // Invariant 12: blocked only while another auction holds the capacity; liquidatable once it settles.
    let mut w = World::with_engine();
    let whale = w.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 0, LIQ_COLLATERAL, LIQ_DEBT);
    let small = w.open_small_position();
    w.set_min_cr(2 * ONE);
    let cap = auction_engine_core::args::CollateralArgs { type_cap: 600_000, ..auction_engine_core::args::CollateralArgs::defaults_for_tests() };
    w.set_collateral_config(k::ENGINE_ADMIN, &cap).unwrap();
    w.liquidate(whale, 0).unwrap();
    assert_err(w.liquidate(small, 0), "CapacityExhausted");
    w.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 1_000_000);
    w.bid_settle(whale, u128::MAX, 0).unwrap();
    w.liquidate(small, 0).expect("liquidatable once the reservation is released");
}

#[test]
fn system_invariants_burn_and_mint_totals() {
    // Invariant 17: protocol burned == Σ surplus payments; minted ≤ cap; unbacked falls by exactly the coin burned.
    let mut w = World::with_system(false);
    w.book_shortfall();
    w.fund_stable(k::TAKER_STABLE, 2_000);
    let protocol_supply = w.supply(protocol_token_def());
    let unbacked = w.read::<AccountingState>(accounting()).unbacked_debt;
    w.start_debt(0).unwrap();
    w.take_debt(0, k::TAKER_STABLE, k::TAKER_PROTOCOL, &[k::TAKER_STABLE], 300, 0).unwrap();
    w.take_debt(0, k::TAKER_STABLE, k::TAKER_PROTOCOL, &[k::TAKER_STABLE], u128::MAX, 0).unwrap();
    let acc: AccountingState = w.read(accounting());
    assert_eq!(acc.unbacked_debt, unbacked - 500);
    assert!(acc.total_protocol_minted <= 5_000);
    assert_eq!(w.supply(protocol_token_def()), protocol_supply + acc.total_protocol_minted);
    // Surplus side: refill the buffer past target + lot after the debt is covered by donations.
    w.donate_stable(buffer(), acc.unbacked_debt + 1_500);
    w.start_surplus(0).unwrap();
    let before = w.supply(protocol_token_def());
    w.take_surplus(0, 500, u128::MAX).unwrap();
    let acc: AccountingState = w.read(accounting());
    assert_eq!(before - w.supply(protocol_token_def()), acc.total_protocol_burned);
    assert!(!w.read::<SurplusAuctionState>(surplus_auction(0)).live);
}

#[test]
fn r3_a_bid_touches_only_its_own_auction() {
    let mut w = World::with_engine();
    let a = w.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 0, LIQ_COLLATERAL, LIQ_DEBT);
    let b = w.open_small_position();
    w.set_min_cr(2 * ONE);
    w.liquidate(a, 0).unwrap();
    w.liquidate(b, 0).unwrap();
    w.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 1_000_000);
    let (b_auction, b_vault, acct) = (w.raw(auction(b, 0)), w.raw(auction_vault(b, 0)), w.raw(accounting()));
    w.bid(a, 100_000, 0).unwrap();
    assert_eq!(w.raw(auction(b, 0)), b_auction, "R3: other auction untouched");
    assert_eq!(w.raw(auction_vault(b, 0)), b_vault, "R3: other vault untouched");
    assert_eq!(w.raw(accounting()), acct, "S6: non-final bid does not write Accounting");
}
```

- [ ] **Step 4: Run the invariant tests**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p engine_integration_tests --test invariants -- --test-threads=1'`
Expected: 4 passed. A failing invariant is a bug in a handler: fix it with a regression test in that handler's task file (`liquidate.rs`, `bid.rs`, …) and re-run the full suite.

- [ ] **Step 5: ADR-036 and docs**

Append ADR-036 to `docs/design-log.md` with the measured table (fill the cycle column from the `CYCLES[...] TOTAL` lines printed in Step 2; executions are fixed by Shared Reference R3):

```markdown
---

## ADR-036: Chained-Call Budget per Instruction (measured)

**Context**
ADR-018: at most 11 executions per public transaction, 10 per private one. Spec §11 asks for cycles per instruction (`CYCLES.md` is Plan 3).

**Decision**
Every engine instruction stays within 10 executions; `programs/integration_tests/tests/chains.rs` asserts it. Measured in Plan 2 (executor cycles, dev mode, natively built guests; not proving cost):

| Instruction | Executions | Total cycles |
|---|---|---|
| observe_feed | 1 | from Step 2 |
| set_collateral_config | 1 | from Step 2 |
| liquidate | 5 | from Step 2 |
| bid (non-final) | 2–4 | from Step 2 |
| bid_with_settlement (final) | 4 | from Step 2 |
| claim_leftover | 2 | from Step 2 |
| start_surplus_auction | 3–4 | from Step 2 |
| take_surplus | 3 | from Step 2 |
| close_surplus_auction | 1–2 | from Step 2 |
| settle_debt | 1–2 | from Step 2 |
| start_debt_auction | 1–2 | from Step 2 |
| take_debt | 3 | from Step 2 |
| check_solvency (freeze) | 2–3 | from Step 2 |

**Trade-offs**
Cycle numbers are indicative; Plan 3 re-measures with the real-proof pipeline and owns the regression check. If a full bid exceeds ~1.9 M cycles, revisit ADR-023 (S6).

**Status**
Accepted (Plan 2, Task 17)
```

Replace every "from Step 2" cell with the measured total before committing (they are measurements this step produces). Append to `docs/architecture/programs.md` an "Invariants checked on accounts" section listing 2, 7, 9, 10, 11, 12, 17 and R3 with their test names.

- [ ] **Step 6: Commit (controller — GPG signing on Windows)**

The implementer stages the files and reports; the controller commits from Git Bash.

```bash
git add programs/integration_tests docs/design-log.md docs/architecture/programs.md
git commit -m "test(engine): account-level invariants, R3 isolation and per-instruction chain budget; ADR-036"
```

---

### Task 18: Two host instances — R4/R5 per-instance pause (cuttable, ADR-015 item 2)

**Cut rule (ADR-015):** if the schedule slips, this whole task is cut: record the descope in `docs/design-log.md` (amend ADR-015: "two-instance R4/R5 test moved to M2; single-instance pause tests `liquidate_rejects_stale_feed_and_admin_pause` and the core `two_type_pause_isolation` stay") and in `docs/architecture/programs.md`, and skip to Task 19. Nothing else depends on this task.

**Files:**
- Create: `programs/stablecoin/methods_b/{Cargo.toml,build.rs,src/lib.rs}`
- Modify: `programs/stablecoin/methods/guest/Cargo.toml` (add `[features] instance-b = []`), `programs/stablecoin/methods/guest/src/bin/stablecoin.rs` (after the `entry!` line, line 8: the tag static)
- Modify: root `Cargo.toml` (`members` += `"programs/stablecoin/methods_b"`), `programs/integration_tests/Cargo.toml` (+ `stablecoin-methods-b`)
- Create: `programs/integration_tests/tests/two_instances.rs`
- Modify: `docs/design-log.md` (ADR-035), `docs/architecture/programs.md`

**Interfaces:**
- Consumes: S1 finding (ADR-018: `#[used] static HOST_INSTANCE_TAG: [u8; 8] = *b"INST-B01";` changes only the stablecoin ImageID); risc0-build 3.0.5 `embed_methods_with_options(HashMap<&str, GuestOptions>)` and `GuestOptionsBuilder` (exported at the crate root, `risc0-build-3.0.5/src/lib.rs:48-53`; `methods` paths are joined to the manifest dir, `lib.rs:291-298`); harness from Tasks 7–10.
- Produces: `stablecoin_methods_b::{STABLECOIN_ELF, STABLECOIN_ID}` (instance B); tests `instances_have_distinct_programs_and_state`, `stale_feed_on_a_does_not_pause_b`, `admin_pause_on_a_does_not_pause_b`.

- [ ] **Step 1: Write the failing test**

`programs/integration_tests/tests/two_instances.rs`:

```rust
//! R4/R5 (spec §5.4, §9): only the instance whose feed is invalid (or that the admin paused) stops.
//! Instance B = the same stablecoin source built with feature `instance-b` (distinct ImageID, S1).

mod common;

use auction_engine_core::{
    args::{CollateralArgs, EngineParamsArgs},
    pda,
    Instruction as Ix,
};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID as CLOCK;
use common::*;
use lee_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use stablecoin_core::{
    compute_liquidation_engine_state_pda, compute_position_pda, compute_position_vault_pda,
    compute_protocol_parameters_pda, compute_redemption_price_state_pda, compute_stability_fee_accumulator_pda,
    compute_stablecoin_definition_pda, compute_stablecoin_master_holding_pda,
};
use token_core::TokenDefinition;
use twap_oracle_core::OraclePriceAccount;

const KEEPER_B_COLL: u8 = 69;

fn host_b() -> ProgramId { stablecoin_methods_b::STABLECOIN_ID }
fn coll_b() -> AccountId { AccountId::new([6; 32]) }
fn oracle_b() -> AccountId { AccountId::new([0x72; 32]) }
fn pp_b() -> AccountId { compute_protocol_parameters_pda(host_b()) }
fn acc_b() -> AccountId { compute_stability_fee_accumulator_pda(host_b()) }
fn rps_b() -> AccountId { compute_redemption_price_state_pda(host_b()) }
fn stable_b() -> AccountId { compute_stablecoin_definition_pda(host_b()) }
fn les_b() -> AccountId { compute_liquidation_engine_state_pda(host_b()) }
fn config_b() -> AccountId { pda::config_pda(&engine_program(), &id(k::ENGINE_ADMIN), 2) }
fn coll_config_b() -> AccountId { pda::coll_config_pda(&engine_program(), &config_b(), &coll_b()) }

fn set_oracle_b(w: &mut World, price: u128, ts: u64) {
    w.state.force_insert_account(
        oracle_b(),
        Account {
            program_owner: [9u32; 8],
            balance: 0,
            data: Data::from(&OraclePriceAccount {
                base_asset: stable_b(),
                quote_asset: coll_b(),
                price,
                timestamp: ts,
                source_id: oracle_b(),
                confidence_interval: 0,
            }),
            nonce: Nonce(0),
        },
    );
}

fn two_instances() -> (World, AccountId, AccountId) {
    let mut w = World::with_engine();
    w.deploy(stablecoin_methods_b::STABLECOIN_ELF);
    w.state.force_insert_account(
        coll_b(),
        Account {
            program_owner: token_program(),
            balance: 0,
            data: Data::from(&TokenDefinition::Fungible { name: String::from("COLLB"), total_supply: 0, metadata_id: None, authority: None }),
            nonce: Nonce(0),
        },
    );
    set_oracle_b(&mut w, R, w.now);
    w.submit(
        host_b(),
        vec![id(k::HOST_ADMIN), pp_b(), acc_b(), rps_b(), stable_b(), compute_stablecoin_master_holding_pda(host_b()), coll_b(), oracle_b(), CLOCK],
        &[k::HOST_ADMIN],
        stablecoin_core::Instruction::InitializeProgram {
            freeze_authority_account_id: id(k::FREEZE),
            initial_stability_fee_per_millisecond: ONE,
            initial_controller_proportional_gain: 0,
            initial_controller_integral_gain: 0,
            initial_minimum_collateralization_ratio: ONE * 3 / 2,
            minimum_milliseconds_between_rate_updates: 300_000,
            maximum_oracle_price_age_milliseconds: 900_000,
            initial_redemption_price: R,
            stablecoin_name: String::from("LSCB"),
        },
    )
    .expect("initialize host B");
    let e = engine_program();
    w.engine(
        vec![id(k::ENGINE_ADMIN), config_b(), pda::accounting_pda(&e, &config_b()), pda::buffer_pda(&e, &config_b()), pp_b(), stable_b(), pda::unused_slot_pda(&e, &config_b(), 1), pda::unused_slot_pda(&e, &config_b(), 0)],
        &[k::ENGINE_ADMIN],
        init_config_ix(2, &EngineParamsArgs::defaults_for_tests()),
    )
    .expect("engine config B");
    w.submit(
        host_b(),
        vec![id(k::HOST_ADMIN), pp_b(), les_b()],
        &[k::HOST_ADMIN],
        stablecoin_core::Instruction::SetLiquidationEngine { engine_program_id: e, engine_authority_id: pda::engine_authority_pda(&e, &config_b()) },
    )
    .expect("bind B");
    w.engine(
        vec![id(k::ENGINE_ADMIN), config_b(), coll_b(), coll_config_b(), oracle_b(), pp_b(), rps_b(), CLOCK],
        &[k::ENGINE_ADMIN],
        collateral_ix(&CollateralArgs::defaults_for_tests()),
    )
    .expect("collateral B");

    let pos_a = w.open_position(k::OWNER, k::OWNER_COLL, k::OWNER_STABLE, 0, LIQ_COLLATERAL, LIQ_DEBT);
    w.ensure_signer(k::OWNER2);
    w.holding(k::OWNER2_COLL, coll_b(), LIQ_COLLATERAL);
    w.holding(k::OWNER2_STABLE, stable_b(), 0);
    let pos_b = compute_position_pda(host_b(), id(k::OWNER2), 0);
    w.submit(
        host_b(),
        vec![id(k::OWNER2), pos_b, compute_position_vault_pda(host_b(), pos_b), id(k::OWNER2_COLL), coll_b(), pp_b(), CLOCK],
        &[k::OWNER2, k::OWNER2_COLL],
        stablecoin_core::Instruction::OpenPosition { position_nonce: 0, initial_collateral_amount: LIQ_COLLATERAL },
    )
    .expect("open B");
    w.submit(
        host_b(),
        vec![id(k::OWNER2), pos_b, stable_b(), id(k::OWNER2_STABLE), acc_b(), rps_b(), oracle_b(), pp_b(), CLOCK],
        &[k::OWNER2],
        stablecoin_core::Instruction::GenerateDebt { amount: LIQ_DEBT },
    )
    .expect("borrow B");
    w.set_min_cr(2 * ONE);
    w.submit(host_b(), vec![id(k::HOST_ADMIN), pp_b()], &[k::HOST_ADMIN], stablecoin_core::Instruction::SetMinimumCollateralizationRatio { new_ratio: 2 * ONE })
        .expect("CR B");
    (w, pos_a, pos_b)
}

fn liquidate_b(w: &mut World, pos_b: AccountId) -> Result<(), lee::error::LeeError> {
    let e = engine_program();
    w.holding(KEEPER_B_COLL, coll_b(), 0);
    let auction_b = pda::auction_pda(&e, &config_b(), &pos_b, 0);
    w.engine(
        vec![
            id(k::KEEPER), id(KEEPER_B_COLL), config_b(), coll_config_b(), pos_b, pda::liqrec_pda(&e, &config_b(), &pos_b),
            auction_b, pda::vault_pda(&e, &auction_b), pda::engine_authority_pda(&e, &config_b()), les_b(),
            compute_position_vault_pda(host_b(), pos_b), coll_b(), acc_b(), rps_b(), oracle_b(), pp_b(), CLOCK,
        ],
        &[k::KEEPER],
        Ix::Liquidate { nonce: 0 },
    )
}

#[test]
fn instances_have_distinct_programs_and_state() {
    let (w, _, _) = two_instances();
    assert_ne!(host_b(), host_program());
    assert_ne!(pp_b(), protocol_parameters());
    assert_ne!(config_b(), config());
    let _ = w;
}

#[test]
fn stale_feed_on_a_does_not_pause_b() {
    let (mut w, pos_a, pos_b) = two_instances();
    w.advance(3_600_001);
    set_oracle_b(&mut w, R, w.now);
    assert_err(w.liquidate(pos_a, 0), "PriceFeedStale");
    liquidate_b(&mut w, pos_b).expect("instance B keeps liquidating");
}

#[test]
fn admin_pause_on_a_does_not_pause_b() {
    let (mut w, pos_a, pos_b) = two_instances();
    let paused = CollateralArgs { admin_paused: true, ..CollateralArgs::defaults_for_tests() };
    w.set_collateral_config(k::ENGINE_ADMIN, &paused).unwrap();
    assert_err(w.liquidate(pos_a, 0), "Paused");
    liquidate_b(&mut w, pos_b).expect("instance B unaffected");
}
```

Add `stablecoin-methods-b = { path = "../stablecoin/methods_b" }` to `programs/integration_tests/Cargo.toml`.

- [ ] **Step 2: Run and see the failure**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p engine_integration_tests --test two_instances'`
Expected: build error: package `stablecoin-methods-b` not found.

- [ ] **Step 3: Add the instance tag and the instance-B methods crate**

`programs/stablecoin/methods/guest/Cargo.toml`: add

```toml
[features]
instance-b = []
```

`programs/stablecoin/methods/guest/src/bin/stablecoin.rs`, after `risc0_zkvm::guest::entry!(main);` (line 8):

```rust
/// Second host instance build (lez-auction-engine spec §5.4, spike S1): changes only the ImageID.
/// Local test build, not offered upstream.
#[cfg(feature = "instance-b")]
#[used]
static HOST_INSTANCE_TAG: [u8; 8] = *b"INST-B01";
```

`programs/stablecoin/methods_b/Cargo.toml`:

```toml
[package]
name = "stablecoin-methods-b"
version = "0.1.0"
edition = "2021"
publish = false

[build-dependencies]
risc0-build = "=3.0.5"

[dependencies]
risc0-zkvm = { version = "=3.0.5", features = ["std"] }

[package.metadata.risc0]
methods = ["../methods/guest"]
```

`programs/stablecoin/methods_b/build.rs`:

```rust
//! Builds the stablecoin guest with feature `instance-b` (distinct ImageID) for two-instance tests.
use std::collections::HashMap;

use risc0_build::{embed_methods_with_options, GuestOptionsBuilder};

fn main() {
    let options = GuestOptionsBuilder::default()
        .features(vec![String::from("instance-b")])
        .build()
        .expect("guest options");
    embed_methods_with_options(HashMap::from([("stablecoin-guest", options)]));
}
```

`programs/stablecoin/methods_b/src/lib.rs`:

```rust
//! Instance-B stablecoin guest: `STABLECOIN_ELF`, `STABLECOIN_ID` (feature `instance-b`).
include!(concat!(env!("OUT_DIR"), "/methods.rs"));
```

Add `"programs/stablecoin/methods_b"` to the root `members`.

- [ ] **Step 4: Run the tests and the upstream suite**

Run: the Step 2 command, then the Task 4 Step 8 command. Expected: `two_instances` 3 passed; the upstream suite unchanged (the default build has no tag).

- [ ] **Step 5: ADR-035 and docs**

Append to `docs/design-log.md`:

```markdown
---

## ADR-035: Second Host Instance via a Guest Feature

**Context**
Spec §5.4 and S1: one stablecoin binary supports one host instance; a second needs a distinct ImageID. S1 used a patched clone and a Docker build.

**Decision**
The vendored guest gains feature `instance-b`, which adds `#[used] static HOST_INSTANCE_TAG: [u8; 8] = *b"INST-B01"` (the S1 mechanism). `programs/stablecoin/methods_b` embeds the same guest package with that feature (`embed_methods_with_options`). The commit is tagged `[local]` and excluded from the upstream patch series. The engine needs nothing: its seeds include the config, and each config binds one host.

**Trade-offs**
A second guest build in the test workspace (one more long first build).

**Status**
Accepted (Plan 2, Task 18)
```

Append to `docs/architecture/programs.md`: "R4/R5 are shown across two host instances (one collateral type each) with two engine configs on one engine program; see `tests/two_instances.rs`."

- [ ] **Step 6: Commit (controller — GPG signing on Windows) — tagged `[local]`**

The implementer stages the files and reports; the controller commits from Git Bash.

```bash
git add Cargo.toml Cargo.lock programs/stablecoin/methods/guest/Cargo.toml programs/stablecoin/methods/guest/src/bin/stablecoin.rs programs/stablecoin/methods_b programs/integration_tests docs/design-log.md docs/architecture/programs.md
git commit -m "test(engine): two host instances for R4/R5 [local]; ADR-035"
bash scripts/export-hook-patches.sh
```
Expected export: the same patch count as after Task 5 (the `[local]` commit is skipped) and `git status --short patches/stablecoin` prints nothing; then `scripts/check-hook-patches.sh` (inside WSL) still reports a clean apply. If `patches/stablecoin` changed, investigate before committing anything.

---

### Task 19: `THREAT_MODEL.md`, per-instruction security checklist, and the security review pass

**Files:**
- Create: `THREAT_MODEL.md`, `docs/architecture/security-checklist.md`
- Modify: `README.md` (status), `docs/architecture/programs.md` (final pass of "Interfaces for Plan 3")
- Modify (only if the review finds issues): the handler file that owns each finding plus its test file

**Interfaces:**
- Consumes: everything from Tasks 1–18; ADR-019 (invariants i–iii, upstream laundering note), ADR-024 (THREAT_MODEL obligations), spec §10, research-3 "Top attacks and mitigations, ranked" (`docs/research/research-3-auction-mechanics.md:671-683`).
- Produces: `THREAT_MODEL.md` (the document Plan 3's `REQUIREMENTS.md` and the red-team pass build on); `docs/architecture/security-checklist.md`; a recorded review result.

- [ ] **Step 1: Write a check that fails until the documents exist**

Run (Git Bash):
```bash
for f in THREAT_MODEL.md docs/architecture/security-checklist.md; do test -f "$f" || echo "MISSING $f"; done
grep -c "S5 residual\|final-fill\|oversized\|keeper reward in a shortfall\|reentrancy\|laundering" THREAT_MODEL.md
```
Expected: `MISSING THREAT_MODEL.md`, `MISSING docs/architecture/security-checklist.md`, then a grep error.

- [ ] **Step 2: Write `THREAT_MODEL.md`**

```markdown
# Threat model (PoC, Plan 2)

Scope: the `auction_engine` program, the three stablecoin hooks, and their interaction with the token program on LEZ v0.2.4 (public transactions). Off-chain clients (Plan 3) and the GUI (Plan 4) are out of scope here except where they must follow a rule stated below.

## Trust assumptions

- **Upstream programs are trusted**: the vendored stablecoin (lez-programs `64c01210` + our hooks) and the token program. PDA authorisation is inherited down the call chain (lee `validated_state_diff/mod.rs:262-290`): an account the engine authorises for the host stays authorised for every program the host calls. The host only chains to the token program of its own position vault.
- **No reentrancy guard exists in LEZ v0.2.4.** R1 (no re-entrant liquidation) rests on our programs: the engine never calls back into itself, and the host never calls the engine.
- **The sequencer is sequential.** There are no write conflicts; a transaction whose precomputed addresses are stale fails.
- **Time** comes only from `CLOCK_01`, which reads the previous block's timestamp.

## Authentication of the engine (ADR-019, ADR-026, ADR-027)

- Hooks require Variant C: `caller_program_id == LiquidationEngineState.engine_program_id` **and** the bound engine-authority PDA authorised. Tests: `stablecoin_program::liquidation_hooks::tests`, `security.rs`.
- The engine authority is `["ENGINE_AUTH", config]`: only the config the host admin bound can act on that host. A rogue config created against the same host — even with the same salt, since the config PDA includes its admin (`["CONFIG", admin, salt]`) — is rejected by the hook (`rogue_config_with_the_same_salt_cannot_liquidate_the_bound_host`, `nobody_can_squat_the_admins_config_address`). **Bind-time verification:** the host admin reads the `EngineConfig` before binding (admin, host ids, token program id, parameters); see `docs/architecture/hooks.md`.
- **Config-chosen program ids (I4).** The engine only chains to the program ids stored in its config, fixed at `init_config` from the accounts it was given; a config built on a fake host or token program is accepted on-chain (`fake_host_config_is_accepted_so_clients_must_verify_program_ids`) and would receive the signer flags users attach (holdings that sign). The defence is client-side: the SDK/CLI refuses any config whose `host_program_id` / `token_program_id` differ from `DEPLOYMENTS.md`.
- Invariant (i): the engine supplies `ENGINE_AUTH` seeds only on calls to `EngineConfig.host_program_id`; the engine chains only to the program ids stored in its config, and no instruction argument is a program id (documentation test `engine_takes_no_program_id_argument`). Invariant (ii): the host chains only to the vault's token program. Invariant (iii): hook amounts come from the position read in the same transaction; every forwarded account id is re-derived or checked against `EngineConfig` (`foreign_account_as_position_is_rejected`).
- **Upstream laundering note.** The host's existing admin and freeze checks are plain `is_authorized` assertions (`programs/stablecoin/src/checks.rs:109`, `src/freeze.rs:58`). If the admin or freeze authority were a PDA, a program it authorised could launder that authority onward. The new `set_liquidation_engine` requires a top-level call; for the existing instructions, admin and freeze authorities must remain user keys (stated in the upstream PR).
- Zero ids: SPEL reports a zero `caller_program_id` for top-level calls, so zero engine program ids and authorities are rejected at bind time and zero ids at engine config time.
- Private path: the immediate-caller check also holds in the privacy circuit (`private_path_impostor_is_rejected_by_the_immediate_caller_check`); `liquidate` is not private-capable on v0.2.4 (ADR-034).

## Research-3 ranked attacks

| # | Attack | Mitigation here | Test |
|---|---|---|---|
| 1 | Stale or manipulated oracle | validity = age, zero price, confidence, `[R/2, 2R]`; price clamped to the band around R; per-type pause, no fallback (ADR-005) | `observe.rs`, `liquidate_rejects_stale_feed_and_admin_pause` |
| 2 | Rounding / split-bid extraction | floor paid, ceil owed; lot-binds re-strike; min bid; `chost` rule; core split-bid proptest | `bid.rs` (`dust_min_bid_and_slippage_are_rejected`), core `split_bid_neutrality` |
| 3 | Liquidation DoS | full seizure; oversized-alone; no callbacks; liveness test | `liveness_unsafe_position_is_liquidatable_within_caps_or_alone` |
| 4 | Zero-bid clearing | oracle-anchored price, max discount 30% hard bound, no expiry (hold at max) | `unsold_auction_holds_max_discount_and_keeps_its_reservation` |
| 5 | Unaccounted shortfall | sold and left tracked separately; shortfall booked at chain end only; `settle_empty` | `lot_binds_…_books_the_shortfall`, `settle_empty_books_the_full_shortfall…` |
| 6 | Caller-controlled time | `CLOCK_01` only, id asserted | every handler (`read_clock`) |
| 7 | Self-liquidation farming | reward carved from the penalty, capped, only above `debt_floor` | core `quote_liquidation` tests |
| 8 | Front-running between quote and fill | `coin_in_max`, `min_collateral_out`, `max_protocol_in`, `min_protocol_out`; independent fills | `bid.rs`, `surplus.rs`, `debt.rs` slippage tests |
| 9 | Bad debt beyond the buffer | `check_solvency` freezes new debt above an absolute threshold when no working protocol token exists; absolute mint cap | `solvency.rs`, `mint_never_exceeds_the_absolute_cap` |
| 10 | Privileged drain | no admin instruction moves auction, buffer or surplus funds directly; admin tunes bounded parameters, with each reference-price change bounded to [old/2, 2·old]. Admin powers that remain (governance trust, below): repeated price steps, switching or disabling the protocol token when no system auction is live | `ref_price_steps_are_bounded_to_half_and_double`, code review (Step 4) |
| 11 | Cross-instance and account confusion | every PDA seeded by config; owner, tag and definition checks | `security.rs`, `config.rs` |

Plus: C-class (griefing price discovery: anyone can bid early at a low discount; this is the Dutch design, bounded by min bid and the dust rule) and F-class (sniping: a bidder can wait for a higher discount; bounded by the schedule's maximum and by competition).

## Specific residuals (ADR-024 obligations and spec §10)

- **S5 residual (pause onset).** Policy (b) derives the stale onset from `last_seen_oracle_ts + max_age`. A producer that re-stamps an unchanged price keeps the feed "fresh"; publishes nobody observes are reconstructed at the next observation, and a reverted observation is lost (ADR-032). An adversary gains at most one block of extra discount (S5 property test). Keepers and the app call `observe_feed` on refresh.
- **Final-fill bound.** A target-reaching fill whose plain floor is 0 pays exactly 1 collateral base unit, at most once per auction (ADR-024), so a sub-unit remainder cannot strand an auction.
- **Oversized-alone limitation, both directions.** While an oversized auction is live its reservation exceeds the cap, so every other liquidation of the instance waits; conversely any live small auction makes a whale wait. Partial liquidation (M1) is the real fix (`oversized_auction_blocks_others_and_small_auction_blocks_whale`).
- **Keeper reward in a shortfall.** The reward is carved from the penalty; when the lot does not cover the debt it comes out of collateral that would otherwise reduce bad debt. `reward_cap` and `reward_coll ≤ c` bound it (`underwater_reward_is_capped_at_the_seizure`).
- **Unsold auction keeps its cap reservation** (no expiry, ADR-012); visible on-chain and to be shown by the CLI/GUI.
- **Donation griefing.** Anyone can raise any holding; balances are compared with `>=` only and sweeps take the whole balance (ADR-033). Donations into a lot-0 auction vault are stranded (donor's loss).
- **List-capacity griefing** is avoided by having no on-chain auction list (ADR-008).
- **Mock-token mint authority.** Only `["MINT_AUTH", config]` can mint, and only `take_debt` asks it to, for `floor(offered × filled / debt_lot) ≤ max_mint_per_auction`.
- **No freeze with a working protocol token.** If debt auctions keep closing untaken, `check_solvency` never freezes; governance must monitor (ADR-017).
- **Host freeze scope.** The host freeze also blocks `open_position` and `withdraw_collateral` (`no_protocol_token_freezes_above_threshold_and_only_the_freeze_authority_unfreezes`); only the freeze authority's `unfreeze` lifts it.
- **Governance trust in the engine admin (I5).** A single reference-price change is bounded to [old/2, 2·old], but repeated changes are not; the admin can switch the protocol token, or disable it, whenever no system auction is live. Disabling it with `unbacked_debt > bad_debt_freeze_threshold` lets anyone's `check_solvency` freeze the host. Every change is logged in `DEPLOYMENTS.md` (ADR-017).
- **Manipulated-but-valid reference oracle (M14).** A reference oracle that passes validation can move the effective price by up to ±20% of the admin price; near the `max_mint_per_auction` boundary that can flip `start_debt` into `MintCapConfig` and so make `check_solvency` freeze the host. Mitigation: keep the admin price's start amount well inside the cap.
- **Top-level-only engine binding (M14).** `set_liquidation_engine` requires `caller_program_id == 0`, so a host whose admin is a PDA or multisig program cannot bind an engine; raised in the upstream PR.

## Engine freeze recovery (ops runbook)

1. `check_solvency` froze the host (`Accounting.host_frozen_by_engine == true`, `ProtocolParameters.is_frozen == true`).
2. Restore solvency: penalties accumulate in the buffer; anyone calls `settle_debt(u128::MAX)`; or configure a working protocol token and run debt auctions.
3. When `unbacked_debt ≤ bad_debt_freeze_threshold` (or by governance decision), the host's freeze authority calls `unfreeze`. The engine never unfreezes.

## Review log

(Filled by Task 19 Step 4.)
```

- [ ] **Step 3: Write `docs/architecture/security-checklist.md`**

A table with one row per engine instruction (Shared Reference R2 order) and one per hook, and these columns, each cell "yes — <where>" or "n/a — <why>": time only from `CLOCK_01`; oracle id pinned and validated; every PDA re-derived; owner/tag/definition checks; zero ids rejected; ">= and sweep" for third-party-raisable holdings; checks → effects → chained calls; caps; dust; rounding direction; frozen-discount (observation before `t_eff`); no admin path to funds; irreversible actions (mint, freeze) net of held assets or above a threshold. Fill it from the handler code (each handler's first lines are its checks). Example row:

```markdown
| `bid` | yes — `read_clock` | yes — `feed::observe` (`coll.oracle_id`) | yes — `load_auction`, vault/buffer PDAs | yes — `fungible_balance`, `check_recipient` | n/a — no id arguments | yes — vault `>=` fill, sweep on lot exhaustion | yes | release only at chain end | yes — core `BelowMinBid`/`DustRemainder` | yes — core | yes — observe before `effective_elapsed` | n/a | n/a |
```

- [ ] **Step 4: Run the security review and record it**

1. Run the whole program suite and lints:
```bash
MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh clippy -p auction_core -p auction_engine_core -p auction_engine_program -p engine_integration_tests --all-targets -- -D warnings && RISC0_SKIP_BUILD=1 scripts/wsl-cargo.sh clippy -p stablecoin_core -p stablecoin_program --all-targets -- -D warnings && scripts/wsl-cargo.sh test -p auction_core -p stablecoin_core -p stablecoin_program -p token_program -p integration_tests -p auction_engine_core -p auction_engine_program -p engine_integration_tests 2>&1 | grep -E "test result|FAILED"'
```
Expected: no clippy warnings; every suite `ok`.
2. `cargo audit` inside WSL (`scripts/wsl-cargo.sh audit` if `cargo-audit` is installed; otherwise `cargo install cargo-audit --locked` first). Record advisories (or "none") in the Review log.
3. Dispatch a **security-reviewer** subagent (user rule: on-chain code needs a security review) with: the diff `git diff <vendor-commit>..HEAD -- programs/stablecoin programs/auction_engine` (vendor commit found by `git log --format=%H -1 --grep='^vendor: lez-programs 64c01210'`), this `THREAT_MODEL.md`, `docs/architecture/security-checklist.md`, ADR-019/026/027/029/030/033, and the instruction to check: authorisation and PDA derivation on every path; invariants (i)–(iii); checks-before-effects; rounding directions; caller-chosen accounts that could change a decision; u128 overflow and `checked_*` use; whether any chained call's pre-state or authorisation flag could be inconsistent.
4. For every CRITICAL or HIGH finding: write a failing test in the owning task's test file, fix the handler, re-run the suite, and commit separately (`fix(engine): …` / `fix(stablecoin): …`). MEDIUM findings are fixed when cheap, else recorded as accepted with a reason.
5. Replace "(Filled by Task 19 Step 4.)" in `THREAT_MODEL.md` with: the date, the reviewer, each finding (severity, location, resolution with commit or "accepted: reason"), the clippy and audit results.

- [ ] **Step 5: Final docs**

- `README.md` "Status": add "**Plan 2 complete: on-chain programs.**" with one line each for the stablecoin hooks (ADR-025/026), the engine (ADR-027–036), the test suites and how to run them (the R5 command with `-p engine_integration_tests`), and "Next: Plan 3 (SDK, CLI, CI and real proofs)".
- `docs/architecture/programs.md` "Interfaces for Plan 3": verify every bullet against the code (instruction indices from `idl.rs`, account lists from R2, error codes from `error.rs`), and add the list of harness constants the SDK quote tests can reuse (`LIQ_*`).

- [ ] **Step 6: Commit (controller — GPG signing on Windows)**

The implementer stages the files and reports; the controller commits from Git Bash.

```bash
git add THREAT_MODEL.md docs/architecture/security-checklist.md README.md docs/architecture/programs.md
git commit -m "docs: threat model, per-instruction security checklist and Plan 2 security review"
```

---

### Task 20: Localnet smoke — deploy both programs to a standalone v0.2.4 sequencer and run liquidate → bid → settle → claim with spel

**Scope and fallback (decided now):** this task proves the programs deploy and run on a real sequencer (dev-mode proofs). Clients are spel-cli (Plan 3 replaces it with SDK encoders). Engine arguments were designed spel-encodable (ADR-030) and the stablecoin's i128 gains use S1's u128 IDL copy with gains 0. If spel cannot encode one engine instruction (e.g. `Vec<u32>` or `program_id`), stop at that instruction, record the exact spel error in `docs/architecture/localnet.md`, and hand the remaining steps to Plan 3 (its SDK encoders); the in-process suite already covers every instruction. Docker is not needed: the deployed binaries are the `*_ELF` constants the tests use (the same bytes `ProgramDeploymentTransaction` deploys in-process).

**Files:**
- Create: `programs/integration_tests/examples/localnet_tool.rs`
- Create: `scripts/localnet-smoke.sh`
- Create: `docs/architecture/localnet.md`, `DEPLOYMENTS.md`

**Interfaces:**
- Consumes: S1 binaries and runbook (`spikes/s1/run.sh` steps 3–4: `sequencer_service`, `wallet`, `spel` under `~/lez-spikes/s1`, upstream Docker-built `token`, `twap_oracle`, `token_mint_authority` bins in `~/lez-spikes/s1/lez-programs/target/guest/`, faucet PDA `GmAwyCjGaT7ocZ2ePDgGFmNw9Yv51ohG421dHMukZRQC`, clock `4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU`); `idl/stablecoin-idl.json`, `idl/auction_engine-idl.json`; `auction_engine_core::pda::*`; `stablecoin_core::compute_*_pda`.
- Produces: `localnet_tool export-guests <dir>` (writes `stablecoin.bin`, `auction_engine.bin`, prints ImageID hex and decimal limbs); `localnet_tool pdas <engine_hex> <host_hex> <engine_admin_base58> <salt> <owner_base58> <position_nonce> <collateral_def_base58>` (prints every engine and host PDA used by the flow as `KEY=value`; the config PDA includes its admin, ADR-027); `localnet_tool decode-config <data_hex>` (prints the stored admin, host and token program ids for bind-time verification); the runbook script; the recorded result.

- [ ] **Step 1: Write the tool (its own check fails first)**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh run -q -p engine_integration_tests --example localnet_tool -- pdas'`
Expected: `error: no example target named \`localnet_tool\``.

`programs/integration_tests/examples/localnet_tool.rs`:

```rust
//! Localnet helper (Plan 2 Task 20): export guest binaries and print the PDAs the smoke flow needs.
use std::{env, fs, path::PathBuf};

use auction_engine_core::pda;
use lee_core::{account::AccountId, program::ProgramId};
use stablecoin_core::{
    compute_liquidation_engine_state_pda, compute_position_pda, compute_position_vault_pda,
    compute_protocol_parameters_pda, compute_redemption_price_state_pda, compute_stability_fee_accumulator_pda,
    compute_stablecoin_definition_pda, compute_stablecoin_master_holding_pda,
};

fn hex_of(id: &ProgramId) -> String {
    id.iter().flat_map(|l| l.to_le_bytes()).map(|b| format!("{b:02x}")).collect()
}

fn limbs_of(id: &ProgramId) -> String {
    id.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
}

fn program_from_hex(h: &str) -> ProgramId {
    assert_eq!(h.len(), 64, "ImageID must be 64 hex characters");
    let bytes: Vec<u8> = (0..32).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).expect("hex")).collect();
    core::array::from_fn(|i| u32::from_le_bytes([bytes[4 * i], bytes[4 * i + 1], bytes[4 * i + 2], bytes[4 * i + 3]]))
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("export-guests") => {
            let dir = PathBuf::from(&args[1]);
            fs::create_dir_all(&dir).expect("dir");
            fs::write(dir.join("stablecoin.bin"), stablecoin_methods::STABLECOIN_ELF).expect("write");
            fs::write(dir.join("auction_engine.bin"), auction_engine_methods::AUCTION_ENGINE_ELF).expect("write");
            println!("STABLECOIN_ID {} limbs {}", hex_of(&stablecoin_methods::STABLECOIN_ID), limbs_of(&stablecoin_methods::STABLECOIN_ID));
            println!("AUCTION_ENGINE_ID {} limbs {}", hex_of(&auction_engine_methods::AUCTION_ENGINE_ID), limbs_of(&auction_engine_methods::AUCTION_ENGINE_ID));
        }
        Some("pdas") if args.len() == 8 => {
            let engine = program_from_hex(&args[1]);
            let host = program_from_hex(&args[2]);
            let admin: AccountId = args[3].parse().expect("engine admin account id (base58)");
            let salt: u64 = args[4].parse().expect("salt");
            let owner: AccountId = args[5].parse().expect("owner account id (base58)");
            let nonce: u64 = args[6].parse().expect("position nonce");
            let collateral: AccountId = args[7].parse().expect("collateral definition (base58)");
            let config = pda::config_pda(&engine, &admin, salt);
            let position = compute_position_pda(host, owner, nonce);
            let auction = pda::auction_pda(&engine, &config, &position, 0);
            for (name, id) in [
                ("PP", compute_protocol_parameters_pda(host)),
                ("SFA", compute_stability_fee_accumulator_pda(host)),
                ("RPS", compute_redemption_price_state_pda(host)),
                ("SDEF", compute_stablecoin_definition_pda(host)),
                ("SMASTER", compute_stablecoin_master_holding_pda(host)),
                ("LES", compute_liquidation_engine_state_pda(host)),
                ("POSITION", position),
                ("HOST_VAULT", compute_position_vault_pda(host, position)),
                ("CONFIG", config),
                ("COLL_CONFIG", pda::coll_config_pda(&engine, &config, &collateral)),
                ("ACCOUNTING", pda::accounting_pda(&engine, &config)),
                ("BUFFER", pda::buffer_pda(&engine, &config)),
                ("ENGINE_AUTH", pda::engine_authority_pda(&engine, &config)),
                ("UNUSED0", pda::unused_slot_pda(&engine, &config, 0)),
                ("UNUSED1", pda::unused_slot_pda(&engine, &config, 1)),
                ("LIQREC", pda::liqrec_pda(&engine, &config, &position)),
                ("AUCTION", auction),
                ("AUCTION_VAULT", pda::vault_pda(&engine, &auction)),
            ] {
                println!("{name}={id}");
            }
            println!("ENGINE_LIMBS={}", limbs_of(&engine));
        }
        Some("decode-config") if args.len() == 2 => {
            // Bind-time verification (ADR-027, I7): decode an EngineConfig account's raw data.
            let h = args[1].trim();
            let bytes: Vec<u8> = (0..h.len() / 2).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).expect("hex")).collect();
            let data = lee_core::account::Data::try_from(bytes).expect("account data");
            let cfg = auction_engine_core::state::EngineConfigState::try_from(&data).expect("an EngineConfig account");
            println!("admin={} salt={}", cfg.admin, cfg.salt);
            println!("host_program_id={} token_program_id={}", hex_of(&cfg.host_program_id), hex_of(&cfg.token_program_id));
            println!("host_protocol_parameters_id={} stablecoin_definition_id={}", cfg.host_protocol_parameters_id, cfg.stablecoin_definition_id);
            println!("{cfg:?}");
        }
        _ => {
            eprintln!("usage: localnet_tool export-guests <dir> | pdas <engine_hex> <host_hex> <engine_admin_base58> <salt> <owner_base58> <position_nonce> <collateral_def_base58> | decode-config <data_hex>");
            std::process::exit(2);
        }
    }
}
```

`AccountId` prints as base58 (`Display`) and parses from it (`FromStr`, used the same way by upstream `programs/twap_oracle/examples/twap_oracle_pdas.rs`).

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh run -q -p engine_integration_tests --example localnet_tool -- export-guests $HOME/lez-target/plan2-localnet/guest'`
Expected: two lines `STABLECOIN_ID <hex> limbs …` and `AUCTION_ENGINE_ID <hex> limbs …`; both files exist.

- [ ] **Step 2: Write the runbook `scripts/localnet-smoke.sh`**

```bash
#!/usr/bin/env bash
# Plan 2 Task 20 localnet smoke. Run INSIDE WSL, one step at a time:
#   scripts/localnet-smoke.sh step_sequencer | step_accounts | step_auth | step_deploy | step_host | step_engine |
#                             step_verify_and_bind | step_bind | step_collateral | step_flow | step_check | step_stop
# Requires the S1 builds (sequencer_service, wallet, spel) and S1's Docker-built upstream guests.
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
S1="$HOME/lez-spikes/s1"
LN="$HOME/lez-target/plan2-localnet"
W="$S1/lez/target/debug/wallet"
SP="$S1/spel/target/debug/spel"
UP="$S1/lez-programs/target/guest"
export LEE_WALLET_HOME_DIR="$LN/wallet-home"
. "$S1/env.sh"
mkdir -p "$LN/logs"
# Fixed by S1 (same upstream binaries → same ids):
TOK=14b3bc6cc129d0359f9595ba0f60a13e4a8df08ffe8eb1a83251c47abecc67a8
TWAP=5681e27c27b3ece4a7c161bbf8e12e6ece6ca69925a87b77f9e193fa4a249b31
CLOCK=4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU
FAUCET_PDA=GmAwyCjGaT7ocZ2ePDgGFmNw9Yv51ohG421dHMukZRQC
ONE=1000000000000000000000000000
env_file="$LN/localnet.env"   # written by hand after step_accounts and step_engine (see docs/architecture/localnet.md)
load() { . "$env_file"; }
spel_host() { "$SP" --idl "$LN/stablecoin-idl-u128.json" -p "$STABLE" -- "$@"; }
spel_engine() { "$SP" --idl "$REPO/idl/auction_engine-idl.json" -p "$ENGINE" -- "$@"; }
spel_token() { "$SP" --idl "$S1/lez-programs/artifacts/token-idl.json" -p "$TOK" -- "$@"; }

step_sequencer() {
  pkill -f sequencer_service || true
  rm -rf "$LN/seqhome" && mkdir -p "$LN/seqhome"
  (cd "$S1/lez" && RISC0_DEV_MODE=1 RUST_LOG=info setsid nohup target/debug/sequencer_service \
    lez/sequencer/service/configs/debug/sequencer_config.json --home "$LN/seqhome" --listen-address 127.0.0.1 -p 3040 \
    > "$LN/logs/sequencer.log" 2>&1 < /dev/null &)
  sleep 20
  curl -s -X POST localhost:3040 -H 'content-type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"getLastBlockId","params":{}}'
}

step_accounts() {
  mkdir -p "$LEE_WALLET_HOME_DIR" && cp "$S1/lez/lez/wallet/configs/debug/wallet_config.json" "$LEE_WALLET_HOME_DIR/"
  for l in admin freeze oracle-src coll-def coll-hold engine-admin keeper keeper-coll owner owner-coll owner-stable; do
    echo pw | "$W" account new public --label "$l"
  done
  # Keys that sign more than one transaction must be auth-transfer-initialised BEFORE first use (ADR-018).
  "$W" account list
  echo "Write $env_file now (format in docs/architecture/localnet.md), then run: $0 step_auth"
}

step_auth() {
  load
  for a in "$ADMIN" "$ENGINE_ADMIN" "$KEEPER" "$OWNER"; do "$W" auth-transfer init --account-id "Public/$a"; done
}

step_deploy() {
  load
  for p in token twap_oracle token_mint_authority; do "$W" deploy-program "$UP/$p.bin"; done
  "$W" deploy-program "$LN/guest/stablecoin.bin"
  "$W" deploy-program "$LN/guest/auction_engine.bin"
  sed 's/"i128"/"u128"/g' "$REPO/idl/stablecoin-idl.json" > "$LN/stablecoin-idl-u128.json"
}

step_host() {
  load
  spel_token new-fungible-definition --name COLL --total-supply 100000000000 --mint-authority "$FAUCET_PDA" \
    --definition-target-account "$COLLDEF" --holding-target-account "$OWNER_COLL"
  "$SP" --idl "$S1/lez-programs/artifacts/twap_oracle-idl.json" -p "$TWAP" -- create-oracle-price-account \
    --base-asset "$SDEF" --quote-asset "$COLLDEF" --initial-price "$ONE" --window-duration 3600000 \
    --oracle-price-account "$ORACLE" --price-source "$ORSRC" --clock "$CLOCK"
  spel_host initialize-program --freeze-authority-account-id "$FREEZE" --initial-stability-fee-per-millisecond "$ONE" \
    --initial-controller-proportional-gain 0 --initial-controller-integral-gain 0 \
    --initial-minimum-collateralization-ratio 1500000000000000000000000000 --minimum-milliseconds-between-rate-updates 60000 \
    --maximum-oracle-price-age-milliseconds 3600000 --initial-redemption-price "$ONE" --stablecoin-name LSC \
    --admin "$ADMIN" --protocol-parameters "$PP" --stability-fee-accumulator "$SFA" --redemption-price-state "$RPS" \
    --stablecoin-definition "$SDEF" --stablecoin-master-holding "$SMASTER" --collateral-definition "$COLLDEF" \
    --market-price-oracle "$ORACLE" --clock "$CLOCK"
}

step_engine() {
  load
  spel_engine init-config --salt 1 --global-cap 10000000 --freeze-on-bad-debt true --bad-debt-freeze-threshold 1000 \
    --protocol-token-enabled false --protocol-token-ref-price 2000000000000000000000000000 --ref-oracle-enabled false \
    --ref-oracle-max-age-ms 3600000 --surplus-buffer-target 1000 --surplus-lot 500 --surplus-delay-ms 0 \
    --surplus-premium-bps 500 --surplus-max-discount-bps 2000 --debt-lot 500 --debt-initial-discount-bps 0 \
    --debt-growth "0,0,1000,3000" --surplus-decay "0,0,1000,2500" --max-mint-per-auction 5000 --min-take 10 \
    --system-ttl-ms 14400000 \
    --admin "$ENGINE_ADMIN" --config "$CONFIG" --accounting "$ACCOUNTING" --buffer "$BUFFER" --protocol-parameters "$PP" \
    --stablecoin-definition "$SDEF" --protocol-token-definition "$UNUSED1" --ref-oracle "$UNUSED0"
}

step_verify_and_bind() {
  load
  # Bind-time verification (ADR-027, I7): read the config the host is about to bind and compare.
  "$SP" inspect "$CONFIG" | tee "$LN/config-inspect.txt"
  echo "Decode the data hex above: scripts/wsl-cargo.sh run -q -p engine_integration_tests --example localnet_tool -- decode-config <hex>"
  echo "Bind only if admin == $ENGINE_ADMIN, host_program_id == $STABLE and token_program_id == $TOK; then run: $0 step_bind"
}

step_bind() {
  load
  spel_host set-liquidation-engine --engine-program-id "$ENGINE_LIMBS" --engine-authority-id "$ENGINE_AUTH" \
    --admin "$ADMIN" --protocol-parameters "$PP" --liquidation-engine-state "$LES"
}

step_collateral() {
  load
  spel_engine set-collateral-config --oracle-scale 0 --max-oracle-age-ms 3600000 --max-conf-bps 100 --band-lo-bps 9000 \
    --band-hi-bps 11000 --penalty-bps 1000 --tip 5 --chip-bps 10 --reward-cap 50 --debt-floor 100 --min-bid 25 \
    --type-cap 5000000 --schedule "0,200,2700000,1000" --admin-paused false \
    --admin "$ENGINE_ADMIN" --config "$CONFIG" --collateral-definition "$COLLDEF" --coll-config "$COLL_CONFIG" \
    --oracle "$ORACLE" --protocol-parameters "$PP" --redemption-price-state "$RPS" --clock "$CLOCK"
}

step_flow() {
  load
  # Owner: an unsafe position (nonce 0: 500_000 coll, 300_000 debt at R = 1) and a healthy one (nonce 1) for bidding funds.
  spel_host open-position --position-nonce 0 --initial-collateral-amount 500000 --owner "$OWNER" --position "$POSITION" \
    --vault "$HOST_VAULT" --user-collateral-holding "$OWNER_COLL" --collateral-definition "$COLLDEF" --protocol-parameters "$PP" --clock "$CLOCK"
  spel_host generate-debt --amount 300000 --owner "$OWNER" --position "$POSITION" --stablecoin-definition "$SDEF" \
    --user-stablecoin-holding "$OWNER_STABLE" --stability-fee-accumulator "$SFA" --redemption-price-state "$RPS" \
    --market-price-oracle "$ORACLE" --protocol-parameters "$PP" --clock "$CLOCK"
  spel_host open-position --position-nonce 1 --initial-collateral-amount 1000000 --owner "$OWNER" --position "$POSITION1" \
    --vault "$HOST_VAULT1" --user-collateral-holding "$OWNER_COLL" --collateral-definition "$COLLDEF" --protocol-parameters "$PP" --clock "$CLOCK"
  spel_host generate-debt --amount 100000 --owner "$OWNER" --position "$POSITION1" --stablecoin-definition "$SDEF" \
    --user-stablecoin-holding "$OWNER_STABLE" --stability-fee-accumulator "$SFA" --redemption-price-state "$RPS" \
    --market-price-oracle "$ORACLE" --protocol-parameters "$PP" --clock "$CLOCK"
  spel_host set-minimum-collateralization-ratio --new-ratio 2000000000000000000000000000 --admin "$ADMIN" --protocol-parameters "$PP"
  spel_engine liquidate --nonce 0 --keeper "$KEEPER" --keeper-collateral-holding "$KEEPER_COLL" --config "$CONFIG" \
    --coll-config "$COLL_CONFIG" --position "$POSITION" --record "$LIQREC" --auction "$AUCTION" --auction-vault "$AUCTION_VAULT" \
    --engine-authority "$ENGINE_AUTH" --liquidation-engine-state "$LES" --host-vault "$HOST_VAULT" \
    --collateral-definition "$COLLDEF" --stability-fee-accumulator "$SFA" --redemption-price-state "$RPS" \
    --oracle "$ORACLE" --protocol-parameters "$PP" --clock "$CLOCK"
  spel_engine bid-with-settlement --coin-in-max 330000 --min-collateral-out 0 --bidder-stable-holding "$OWNER_STABLE" \
    --bidder-collateral-holding "$OWNER_COLL" --config "$CONFIG" --coll-config "$COLL_CONFIG" --auction "$AUCTION" \
    --auction-vault "$AUCTION_VAULT" --buffer "$BUFFER" --stablecoin-definition "$SDEF" --redemption-price-state "$RPS" \
    --oracle "$ORACLE" --clock "$CLOCK" --accounting "$ACCOUNTING"
  spel_engine claim-leftover --owner "$OWNER" --owner-collateral-holding "$OWNER_COLL" --config "$CONFIG" \
    --auction "$AUCTION" --auction-vault "$AUCTION_VAULT"
}

step_stop() {
  pkill -f sequencer_service || true
}

step_check() {
  load
  "$SP" inspect "$POSITION" --idl "$LN/stablecoin-idl-u128.json" --type Position
  for a in "$AUCTION_VAULT" "$BUFFER" "$OWNER_COLL" "$KEEPER_COLL"; do "$SP" inspect "$a" --idl "$S1/lez-programs/artifacts/token-idl.json" --type TokenHolding; done
  grep -E "failed execution check|Transaction with hash" "$LN/logs/sequencer.log" | tail -20
}

"$@"
```

Then `git add --chmod=+x scripts/localnet-smoke.sh`. The `spel inspect` form is S1's (`spel inspect 8jZQ... --type ProtocolParameters`); spel flags for PDA accounts that spel derives itself from the IDL seeds may be rejected as unexpected — if so, drop exactly that flag (spel computes it).

- [ ] **Step 3: Run the runbook**

Run each step (`MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/localnet-smoke.sh <step>'`) in this order:

1. `step_sequencer`.
2. `step_accounts`, then create `$HOME/lez-target/plan2-localnet/localnet.env` with `ADMIN FREEZE ORSRC COLLDEF ENGINE_ADMIN KEEPER KEEPER_COLL OWNER OWNER_COLL OWNER_STABLE` (base58 ids from `wallet account list`; `oracle-src` → `ORSRC`, `coll-def` → `COLLDEF`, `coll-hold` is unused), plus `STABLE=<hex>` and `ENGINE=<hex>` from Step 1's output.
3. `step_auth`, then `step_deploy`.
4. Oracle PDA (upstream tool, as in S1; the env file is sourced first so `$ORSRC` expands inside WSL):
   `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc '. $HOME/lez-target/plan2-localnet/localnet.env && . $HOME/lez-spikes/s1/env.sh && cd $HOME/lez-spikes/s1/lez-programs && RISC0_SKIP_BUILD=1 cargo run -q -p twap_oracle_program --example twap_oracle_pdas -- 5681e27c27b3ece4a7c161bbf8e12e6ece6ca69925a87b77f9e193fa4a249b31 $ORSRC 3600000'`; add `ORACLE=<oracle price account>` to `$HOME/lez-target/plan2-localnet/localnet.env`.
5. Engine and host PDAs (appended to the absolute env path):
   `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc '. $HOME/lez-target/plan2-localnet/localnet.env && cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh run -q -p engine_integration_tests --example localnet_tool -- pdas $ENGINE $STABLE $ENGINE_ADMIN 1 $OWNER 0 $COLLDEF >> $HOME/lez-target/plan2-localnet/localnet.env'`; run it again with position nonce `1` into a scratch file and append only its `POSITION` and `HOST_VAULT` lines, renamed `POSITION1` and `HOST_VAULT1`, to the same absolute path.
6. `step_host`, `step_engine`, `step_verify_and_bind` (compare the decoded config, then) `step_bind`, `step_collateral`, `step_flow`, `step_check`, and finally `step_stop` (M10: the sequencer keeps running in the background otherwise). If `spel inspect` cannot print raw data for an account without an IDL type, record that in `docs/architecture/localnet.md` and do the verification with Plan 3's CLI before any non-local deployment.

Note on holdings: `OWNER_STABLE` and `KEEPER_COLL` must exist before they receive tokens unless they sign: the stablecoin's `generate_debt` requires an initialised destination, so initialise `OWNER_STABLE` with `spel_token initialize-account --definition-account "$SDEF" --account-to-initialize "$OWNER_STABLE"` (account names from `idl/upstream/token-idl.json`) and `KEEPER_COLL` likewise with `$COLLDEF`, before `step_flow`.

Expected end state (`step_check`): position 0 `collateral_amount 0`, `normalized_debt_amount 0`; buffer balance 30_000 (penalty); `OWNER_COLL` received `floor(330_000 / (1 − d))` collateral from the bid (d ≈ 2%, slightly more if blocks elapsed since the liquidation) plus the leftover `499_950 − that`; `KEEPER_COLL` received 50 (reward `min(5 + 300, 50, 30_000)` = 50 coin = 50 collateral at P = 1); the sequencer log shows no `failed execution check` lines for these transactions. Every spel call prints `Transaction confirmed`; a "not confirmed" result means read the sequencer log (S1 problem 7).

- [ ] **Step 4: Record the result**

`docs/architecture/localnet.md`: the exact commands run, the `localnet.env` variable list (names only, no keys), the ImageIDs, the outcome of each step (confirmed / failed with the sequencer-log line), the `step_check` output, and — if spel could not encode an instruction — the exact error and "remaining steps moved to Plan 3 (SDK encoders)".

`DEPLOYMENTS.md`:

```markdown
# Deployments

## Localnet (Plan 2 smoke, standalone LEZ v0.2.4 sequencer, dev-mode proofs)

| Program | ImageID | Source |
|---|---|---|
| token | `14b3bc6cc129d0359f9595ba0f60a13e4a8df08ffe8eb1a83251c47abecc67a8` | lez-programs 64c01210 Docker build (S1) |
| twap_oracle | `5681e27c27b3ece4a7c161bbf8e12e6ece6ca69925a87b77f9e193fa4a249b31` | same |
| stablecoin (+ hooks) | from Task 20 Step 1 | this repo, `stablecoin_methods::STABLECOIN_ELF` (native risc0 build) |
| auction_engine | from Task 20 Step 1 | this repo, `auction_engine_methods::AUCTION_ENGINE_ELF` (native risc0 build) |

Reproducible (Docker) builds and testnet deployment: Plan 3 / milestone M6.

## Reference price changes (ADR-017)

Every change of `protocol_token_ref_price` is recorded here: date, config, old → new, reason. (None yet.)
```

Replace both "from Task 20 Step 1" cells with the printed hex before committing.

- [ ] **Step 5: Commit (controller — GPG signing on Windows)**

The implementer stages the files and reports; the controller commits from Git Bash.

```bash
git add programs/integration_tests/examples/localnet_tool.rs scripts/localnet-smoke.sh docs/architecture/localnet.md DEPLOYMENTS.md
git commit -m "chore: localnet smoke runbook and result (deploy, liquidate, bid, settle, claim on a v0.2.4 sequencer)"
```

---

## Out of scope for Plan 2 (owned elsewhere)

- Plan 3: `auction_sdk` (its own encoders, decoders, quotes, block scanner, confirmation), `auction_ffi`, `auction_cli`, CI for programs (guest builds need the risc0 toolchain in CI), the per-push real-proof job, `CYCLES.md` and its regression check, `REQUIREMENTS.md` and its checker, SDK-side `observed()` simulation before every `t_eff` preview (ADR-024), reproducible Docker guest builds. Plan 3 consumes: `idl/*.json`, `auction_engine_core` (instruction enum, `state::*`, `pda::*`, `EngineError::code`), the account lists of Shared Reference R2, the bid account-choice rule (ADR-029) and the slot convention (ADR-030), all summarised in `docs/architecture/programs.md` "Interfaces for Plan 3".
- Plan 4: the thin GUI; its first task confirms `lez_core` loads in Basecamp and pins the Basecamp revision (ADR-018, ADR-024).
- Sub-project B: private-path SDK/UI; a private-capable `liquidate` chain shape (ADR-034).
- Not adopted from ADR-024's optional suggestion: a distinct `DebtLotTooSmall` error. The core keeps `MintCapConfig` for a zero start amount (freeze-eligible, documented in ADR-017/024); no core change is needed for Plan 2.

---

## Self-review (re-run after the independent review fixes, 2026-10-06)

### 1. Spec and ADR coverage

| Spec section / obligation | Task(s) |
|---|---|
| §2.1 engine instructions `init_config`, `set_collateral_config` | 8, 9 |
| §2.1 `liquidate`, `bid`, `observe_feed`, `claim_leftover`, `settle_debt` | 10, 11, 9, 12, 12 |
| §2.1 `start_surplus_auction`, `take_surplus`, `start_debt_auction`, `take_debt`, `close_system_auction`, `check_solvency` | 13, 13, 14, 14, 13+14 (ADR-031), 15 |
| §2.1 stablecoin hooks appended to enum and guest list; order test | 4, 5 (`hook_instructions_share_enum_and_idl_index`) |
| §2.1 mock protocol token (PDA mint authority) | 13 (real `NewFungibleDefinition`), 14 (mint) |
| §3.1 pins | Global Constraints; 3 |
| §3.2 chained-call limit; duplicate ids; inherited PDA authorisation; no reentrancy guard; sequential sequencer; `CLOCK_01`; spel IDL order | 17 (ADR-036); ADR-030; 19 (THREAT_MODEL); every handler (`read_clock`); 4, 8, 15 (`idl.rs`) |
| §4.2 repository layout (patch series now at `patches/stablecoin/`) | File structure; 3, 6, 7, 8, 16, 18 |
| §5.1 units, rounding, overflow checks, `forbid(unsafe_code)` (incl. the test crate, M12) | Global Constraints; 7; nominal debt ceil in 10 |
| §5.2 feed validity, band clamp, no fallback, R from the host projection, lot-binds charge | 9, 10, 11 |
| §5.3 schedule snapshot per auction | 10; `schedule_change_affects_only_new_auctions` (11) |
| §5.4 accounts and seeds (config seed amended to `["CONFIG", admin, salt]`, ADR-027); one engine config per host; ADR-004 nonce; ADR-008 no list; R3 isolation (deviation recorded in ADR-029) | 6 (R1); 8; 10; 17 (`r3_a_bid_touches_only_its_own_auction`) |
| §5.5 state machine, no restart, no expiry, unsold auction holds max discount and its reservation | 11 |
| §5.6 liquidate steps 1–7; bid rules; claim pull model; settle_debt on actual balance | 10; 11; 12; 12 |
| §5.7 pause clock, admin exact, observation in liquidate/bid/observe_feed, S5 policy (b), frozen discount | 9; 10, 11 (`bid_rejects_invalid_feed`, `discount_frozen_while_paused`) |
| §5.8 chain-end effects once; supply invariant | 2, 11, 12; 17 |
| §5.9 keeper reward, `reward_coll ≤ c`, debt floor | 10 (`underwater_reward_is_capped_at_the_seizure`) |
| §5.10 hook checks; `engine_freeze_new_debt` only freezes with the host's existing semantics; admin bind | 4, 5; 15 (`Protocol is frozen` after the engine freeze, M6) |
| §5.11 F4/F5 incl. reference price + ±20% clamp, freeze list, threshold, RecoveryInProgress, no freeze with a working token | 13, 14, 15 |
| §5.12 bounds (incl. reference-oracle age ≤ 24 h, global ≥ type cap on every update, distinct protocol-token accounts) | 6 (`args.rs`), 8 (`protocol_token_must_differ_from_the_collateral`), 9 (`global_cap_below_type_cap_is_rejected`) |
| §5.13 invariants 2, 7, 9, 10 (collateral and debt), 11, 12, 17 | 17 |
| §9 stablecoin hook test list | 4, 5, 7 (`hooks.rs`), suite runs in 3/4/5 |
| §9 engine test list (incl. R4/R5 two instances) | 9–18 |
| §10 THREAT_MODEL items, checklist, security-reviewer pass, clippy (incl. stablecoin crates) and audit | 19 |
| §11 cycles per instruction (indicative) | 17 |
| §12 docs kept current | every task's docs step |
| ADR-018 second build tag, `auth-transfer init`, spel i128 | 18, 20 |
| ADR-019 Variant C, invariants (i)–(iii), zero ids, private-path test | 4, 5, 10, 15, 16 |
| ADR-020 PDA syntax; counter incremented only by auction creation; `config` validated | 8 (`load_config`), 10 (`bump_record`) |
| ADR-021 prerequisites: mint into a fresh holding, burn from an engine-PDA holding | 14, 12 |
| ADR-023 chain-end accounting on every path; consequence 6 decided | 2, 11, 12; ADR-029 |
| ADR-024 obligations: feed_valid via validate_feed; observation before t_eff; validate params; threshold; zero ids and ref price; debt pairing; settle_empty/close exposed; THREAT_MODEL contents; invariants restated | 9; 9–11; 6, 8, 9; 6, 8; 4, 8; 14; 12–14; 19; 2 |
| Review fixes: I2 `check-hook-patches.sh` (3–5, 18); I3 vendored config and clippy/fmt gates (3, 4, 5, 19); I4 Global Constraints, `fake_host_config_is_accepted_so_clients_must_verify_program_ids` (16), Plan 3 interface; I5 `RefPriceStepTooLarge` (6, 8), ADR-031, THREAT_MODEL; I6 (10, 11); I7 config seed (6, 8, 16, 18, 20) and bind-time verification (hooks.md, THREAT_MODEL, runbook) | as listed |
| Localnet smoke | 20 |

### 2. Placeholder scan

Searched for `TBD`, `TODO`, `implement later`, `fill in`, `similar to Task`, `add appropriate`, `handle edge cases`, `write tests for`: none (the only `todo` hit is the upstream lint name `todo = "deny"`). Measured values written by a named step of the same task before its commit: upstream suite count (Task 3 Step 6), stablecoin fmt baseline (Task 3 Step 4), cycle totals (Task 17 Step 2), localnet ImageIDs (Task 20 Step 1). Conditional steps with a stated alternative: `#[account_type]` on a `ProgramId` field (Task 4), the private-path error text (Task 16), spel raw inspect for bind-time verification (Task 20), nightly fmt (none installed in WSL; the stable check against the baseline is used).

### 3. Type, name and seed consistency

- Config PDA: `config_seed(&admin, salt)` / `config_pda(&program_id, &admin, salt)` in `pda.rs`; used by `load_config` (stored admin), `init_config` (signer), the guest attribute `pda = [literal("CONFIG"), account("admin"), arg("salt")]`, the harness `config()` (`id(k::ENGINE_ADMIN)`, `SALT`), Task 8 `nobody_can_squat_the_admins_config_address`, Task 16 (`config_pda(&e, &id(k::ATTACKER), SALT)` and `… 7`), Task 18 (`config_b`), Task 20 (`localnet_tool pdas … <engine_admin> <salt> …`), R1 and ADR-027.
- New error `EngineError::RefPriceStepTooLarge` (code 16): R4, `error.rs` enum and `code()`, `error_codes_are_stable`, `set_engine_params`, `ref_price_steps_are_bounded_to_half_and_double`, ADR-031, THREAT_MODEL.
- `set_engine_params` gained the `coll_config` account: R2 row 1, the instruction doc comment, `SetEngineParamsAccounts`, the guest function, harness `World::set_engine_params`.
- New helper `accounts::check_distinct_slots` (used by `init_config` and `set_engine_params`; listed in Task 8 Interfaces).
- Task split: Task 7 (harness, `bind_engine_to`) and Task 8 (engine skeleton; `bind_engine` calls `bind_engine_to`). Every later "Task N" reference was renumbered (+1 for N ≥ 8); references to the old Task 7 now point to Task 7 (harness) or Task 8 (program; guest lock seeding is "Task 8 Step 7").
- Unchanged names re-checked across tasks: `EngineResult`, `accounts::*`, `token_calls::*`, `feed::*`, `settle::{implicit_settle, buffer_burn}`, `system::*`, state conversions, `Accounting::record_settled`, harness helpers. Test counts updated: Task 4 13 unit tests, Task 5 30, Task 8 `config` 12, Task 9 `observe` 7, Task 11 `bid` 13, Task 16 `security` 8.

### 4. Review Focus

| Line | Pinned test(s) |
|---|---|
| 1 Bid turns final without `Accounting` | Task 11 `bid_that_turns_final_without_accounting_reverts` |
| 2 Fresh recipient holding | Task 10 `keeper_reward_to_fresh_holding`, Task 11 `fresh_bidder_holding_must_sign`, `holding_of_wrong_definition_is_rejected`, Task 14 `take_debt_mints_into_fresh_holding` |
| 3 Donations | Task 5 `donation_into_vault_is_swept`, Task 10 `donation_into_host_vault_is_seized_too`, Task 11 `lot_emptying_bid_sweeps_donation`, Task 12 `settle_debt_uses_actual_buffer_balance`, `claim_leftover_pays_the_owner_once_and_sweeps` |
| 4 "Take everything" amounts | Task 11 `max_coin_in_charges_only_remaining`, Task 12 `settle_debt_max_amount`, Task 13 `take_surplus_max_amount_fills_lot`, Task 14 `take_debt_max_amount_fills_lot` |
| 5 Invalid feed / admin pause between signing and inclusion | Task 9 `observe_feed_persists_stale_pause`, Task 10 `liquidate_rejects_stale_feed_and_admin_pause` (stale, paused, low confidence, out of range), Task 11 `bid_rejects_invalid_feed` (stale, low confidence, out of range; auction unchanged), `discount_frozen_while_paused` |
