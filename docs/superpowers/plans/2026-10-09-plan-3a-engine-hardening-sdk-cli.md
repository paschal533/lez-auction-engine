# Plan 3a: Engine Hardening, `auction_sdk` and `auction_cli` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the two accepted MEDIUM engine findings (THREAT_MODEL findings 4 and 8), then ship the Rust client stack of the RFP-014 PoC: `auction_sdk` (own enum-index encoders, PDAs, decoders, quotes on the program's own pricing path, plan builders, program-id verification, nonce-based confirmation, block-scan discovery, wallet-keystore signing) and `auction_cli` (every spec §6.6 command), and replace spel with the CLI in the localnet smoke.

**Architecture:**
- The two engine fixes stay inside `programs/auction_engine` (handlers, guest, `auction_engine_core` state): config-time validation of the protocol-token slot (ADR-038) and a fixed relist delay between debt auctions (ADR-039). The vendored stablecoin is not touched.
- `crates/auction_sdk` is a synchronous library over one trait, `Chain` (account, nonces, height, block range, send). Production uses `RpcChain` (feature `rpc`, the LEZ v0.2.4 `sequencer_service_rpc` client); tests use the in-memory `FixtureChain` (feature `testing`) and, in `programs/integration_tests`, a `WorldChain` adapter over the real in-process guests. Encoders are the risc0 serde encoding of each program's own instruction enum (first word = enum index), never the IDL position. Quotes call the same `auction_engine_program::feed` functions and `auction_core` functions, in the same order, as the handlers. Every transaction-building path first runs `verify_config` against a machine-readable deployment file (I4, ADR-027).
- `cli/` (package `auction_cli`, binary `auction-cli`) is a thin clap front end over the SDK. `run(&Command, &Env<C: Chain>, &mut dyn Write)` is generic over the chain, so golden tests drive every command against `FixtureChain`, and `main.rs` plugs in `RpcChain`.
- `scripts/localnet-smoke.sh` keeps its step structure but uses `auction-cli` for every instruction (wallet stays for key creation, `auth-transfer init` and program deployment).

**Scope decision (split).** Plan 3 as scoped by the spec (§6.1, §6.4–§6.6, §9 "Real proofs" / "SDK/CLI" / "Requirements" rows, §11, §12) plus the 13 carry-overs in `docs/architecture/programs.md` is about 30 tasks in two independent deliverables, so it is split (ADR-037, written in Task 1):
- **3a (this plan, 16 tasks):** engine hardening (findings 4 and 8), test hygiene and exhaustive drift tests, `auction_sdk`, `auction_cli`, the CLI-driven localnet smoke. It ships working software on its own: a user can liquidate, bid, settle and run a surplus auction on localnet from the CLI with no spel.
- **3b (next plan, "Plan 3b scope" at the end):** program CI with risc0, real-proof CI, reproducible Docker builds and the ImageID refresh, `CYCLES.md` + regression check + the ADR-023 re-measure with real proofs, `REQUIREMENTS.md` + checker, `cargo-fuzz`, coverage gate, red-team pass, the oracle price publisher/keeper, the one-command demo (`scripts/demo.sh`, F4 and freeze paths), upstream issue drafts, `SUBMISSION.md`. 3b depends on 3a (its real-proof end-to-end job and demo drive the CLI); 3a does not depend on 3b. Doing 3a first also means the ImageIDs are refreshed once, by 3b's Docker builds, after the last guest change (Tasks 2 and 3 here).

**Tech Stack:** Rust 1.94.0; LEZ v0.2.4 (`lee`, `lee_core`, `clock_core`, and new in 3a `common` and `sequencer_service_rpc` with feature `client`); risc0-zkvm `=3.0.5` (guests, integration tests); SPEL `1ef0500f` (`idl-gen` only); clap 4 (derive); serde/serde_json; thiserror 2; tokio 1 (current-thread runtime inside `RpcChain`); tempfile (tests); WSL Ubuntu (`Ubuntu-20.04`) for every build and test.

**Spec:** `docs/superpowers/specs/2026-10-01-rfp014-auction-engine-poc-design.md` (v5), as amended by `docs/design-log.md` ADR-018 to ADR-036. Read spec §3.2, §5.4, §5.11, §6.1, §6.4–§6.6, §9, §10 and ADR-008, ADR-009, ADR-018, ADR-024, ADR-027, ADR-029, ADR-030, ADR-036, plus `docs/architecture/programs.md` ("Interfaces for Plan 3", "Plan 3 carry-overs") and `THREAT_MODEL.md` before any task.

## Global Constraints

- Pins (spec §3.1): LEZ (`lee`, `lee_core`, `clock_core`, `common`, `sequencer_service_rpc`) v0.2.4 `47eba256479f6f785acbd138834340703cd03401` (git tag `v0.2.4` of `https://github.com/logos-blockchain/logos-execution-zone.git`); SPEL `1ef0500f8fc8ce3ddf95523726ae159db83f744f`; risc0-zkvm `=3.0.5`; Rust `1.94.0`; lez-programs (vendored stablecoin) `64c01210adecd6b557dbd021a3cc94553f243fd8`; `lez_core` module `acf0cd501b262c4c15969e3735e85318297b85bf` (metadata 0.4.1) is Plan 4 only and is not a dependency here.
- Units (spec §5.1): amounts are raw base units as `u128`; prices are RAY = `1_000_000_000_000_000_000_000_000_000` (1e27), collateral per 1 stablecoin; bps convert to RAY by `× 1e23`; time is `u64` milliseconds, on-chain only from `CLOCK_01` (the previous block's timestamp, spec §3.2). Every product goes through `auction_core::math::mul_div_floor` / `mul_div_ceil`; amounts paid out round down, amounts owed round up; never write `a * b / c` on `u128`.
- Platform facts the client must respect (spec §3.2): no account-listing RPC and no events (discovery is a block scan, §6.4); lez_core 0.4.1 / sequencer `sendTransaction` success means **accepted, not executed**: failed transactions are silently dropped at block build and the signer nonce advances only on apply, so confirmation is nonce polling (§6.5, ADR-009); at most 10 chained calls per transaction; 32 Mi cycles per execution.
- Encoding (spec §3.2, §6.1, ADR-018): every SDK encoder is `lee::program::Program::serialize_instruction` of the program's own instruction enum (risc0 serde; first word = **enum index**). Never encode an IDL position. `idl/stablecoin-idl.json` differs from `stablecoin_core::Instruction` for IDL positions 8..=18; the pinned IDL-position→enum table is `0,1,2,3,4,5,6,7,17,18,14,8,9,11,12,13,10,15,16,19,20,21`. Signed `i128` controller gains must encode (spel cannot).
- Program-id verification (I4, ADR-027): before building ANY transaction for a config, the SDK/CLI reads its `EngineConfig` and refuses unless the config is owned by the expected engine ImageID, sits at `["CONFIG", admin, salt]`, its `host_program_id` and `token_program_id` equal the deployment file, its admin equals the expected operator when the file names one, and the host's `ProtocolParameters` is owned by the expected host program. The deployment file (`deployments/localnet.json`) must agree with `DEPLOYMENTS.md` (drift test, Task 5).
- Optional slots (ADR-030): slot 0 = reference oracle, slot 1 = protocol-token definition; pass the configured id, or `["UNUSED", config, slot]` when unset. Bid account choice (ADR-029): `bid_with_settlement` whenever the quoted fill ends the auction, `coin_in_max >= remaining_target`, or the fill is within the slippage margin of emptying the lot; otherwise `bid`.
- Quotes (ADR-024): every `t_eff` evaluation, including SDK and CLI previews, first simulates `PauseClock::observed()` at `now` through `auction_engine_program::feed::observe`. Client obligations (THREAT_MODEL): show unsold auctions that keep their cap reservation; offer `observe-feed` on refresh; treat `Accounting.host_frozen_by_engine` as "has ever frozen"; never advise unfreezing while settled `unbacked_debt > bad_debt_freeze_threshold`.
- Signers (ADR-018): accounts that sign more than one stablecoin-program transaction must be `wallet auth-transfer init`-ed first; a fresh recipient holding must sign (`RecipientMustSign`). The PoC submits **public transactions only** (spec §8); a private wallet account is refused as a signer.
- Secrets: keystore contents and keys are never printed, logged, put in error messages or written by the SDK/CLI; `Keystore` has a hand-written `Debug` that lists account ids only. No network access in unit or golden tests; live-network tests are `#[ignore]`d.
- Vendored code: no task edits `programs/stablecoin/**`, `programs/token/**`, `programs/twap_oracle/**` or `programs/upstream_integration_tests/**`. If that ever becomes unavoidable, follow the patch-series rules (ADR-025): a hook commit touches only what upstream needs, run `scripts/export-hook-patches.sh` then `scripts/check-hook-patches.sh`; repo-only changes are `[local]` commits. Never reformat vendored code.
- On-chain changes (Tasks 2 and 3): TDD at handler level first, then integration tests with the real guests in `programs/integration_tests`; regenerate the engine IDL with `tools/idl-gen` and diff it; update `THREAT_MODEL.md` (finding row, residual, review-log entry), `docs/architecture/security-checklist.md`, `docs/architecture/programs.md` and the design log in the same task.
- ADR numbers continue after ADR-036: ADR-037 (Plan 3 split, Task 1), ADR-038 (Task 2), ADR-039 (Task 3), ADR-040 (Task 4), ADR-041 (Task 9), ADR-042 (Task 10), ADR-043 (Task 11), ADR-044 (Task 13). ADRs are appended at the end of `docs/design-log.md` in the existing format (`## ADR-0NN: Title`, **Context**, **Options**, **Decision**, **Trade-offs**, **Status**, then `---`).
- Build and test only inside WSL through the wrapper (it sets `CARGO_TARGET_DIR=$HOME/lez-target/plan2` and `RISC0_DEV_MODE=1`). From Git Bash every cargo command is:
  `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh <cargo args>'`
  Long first builds of a guest take minutes: run them with the Bash tool's `run_in_background` and a 3600000 ms timeout.
- Lint gate (every task, before the commit step):
  `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && RISC0_SKIP_BUILD=1 scripts/wsl-cargo.sh clippy --workspace --all-targets --exclude integration_tests -- -D warnings'`
  Format gate (our crates only; add `-p auction_sdk` from Task 4 and `-p auction_cli` from Task 13):
  `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh fmt -p auction_core -p auction_engine_core -p auction_engine_program -p engine_integration_tests -- --check'`
- Files: every source file < 800 lines; `#![forbid(unsafe_code)]` in every crate this plan creates; UTF-8 + LF (`.gitattributes` enforces `eol=lf`).
- Commits: conventional format `<type>: <description>` (types feat, fix, refactor, docs, test, chore, perf, ci). **No `Co-Authored-By` line and no AI attribution of any kind** (user requirement; this overrides the Plan 1/2 convention). Every commit step is "Commit (controller — GPG signing on Windows)": the implementer stages the listed files and reports; the controller commits from Git Bash on Windows (GPG-signed), never from WSL. Nothing is pushed.
- Docs are updated in the same task as the code they describe; `cli/README.md` is created in Task 13 and extended by Tasks 14 and 15.

## Review Focus

Inputs and conditions the spec implies that are most likely to bite an SDK/CLI user, most likely first. Each is pinned by a named test in its owning task.

1. **Stale quote vs on-chain result.** A quote is taken at clock `T`; the transaction lands later at `T + Δ` (discount moved, or the oracle moved). Expected: at inclusion time the on-chain fill equals the SDK quote evaluated at the inclusion clock; when the price moves against the bidder, the `min_collateral_out` derived from the quote with the chosen slippage makes the bid revert with `Slippage` instead of filling worse. Pinned in Task 8 (`bid_quote_at_inclusion_time_equals_the_on_chain_fill`, `stale_quote_is_protected_by_min_collateral_out`).
2. **A dropped transaction.** The sequencer accepted it but block build dropped it (e.g. the bid turned final without `Accounting`, or the feed went stale). Expected: `Dropped` after 5 blocks with the nonce unchanged, the in-flight lock released, no automatic resend of the stale transaction, and the CLI prints "nothing was applied; re-quote and resend" and exits 3. Pinned in Task 10 (`dropped_transaction_reports_dropped_and_releases_the_lock`) and Task 14 (`bid_dropped` golden).
3. **Wrong network or ImageID mismatch.** The deployment file names one host/token/engine, but the config on the node was built on another host, a fake token program, a different admin, or the CLI points at another network. Expected: `verify_config` refuses with a named mismatch before any transaction is built; `host bind` refuses to bind; the CLI exits 2. Pinned in Task 5 (`verify_config_rejects_every_mismatch`) and Task 15 (`bind_wrong_host` golden).
4. **Keystore problems.** `LEE_WALLET_HOME_DIR` unset, `storage.json` missing or malformed, an unknown label, an account the wallet has no key for, a private account as signer, or a key that does not derive the stored account id. Expected: a distinct typed error per case, no panic, no key material in any message, CLI exit 4. Pinned in Task 9 (`keystore_errors_are_typed_and_never_print_key_material`) and Task 14 (`keystore_missing` golden).
5. **Scanner cursor after a restart, a gap or a replaced block.** The persisted cursor's block now has a different hash (localnet restarted or a block was replaced), the node cannot return a block in the range, or the process dies while writing the cursor. Expected: the scanner rewinds and rescans (no missed auction, no duplicate), a missing block is an error that leaves the cursor unchanged, and the cursor file is written atomically. Pinned in Task 11 (`scanner_rewinds_when_the_cursor_block_hash_changed`, `scanner_leaves_the_cursor_unchanged_on_a_missing_block`, `cursor_write_is_atomic_and_a_corrupt_cursor_is_an_error`).

---

## Shared Reference

### S1. Engine accounts per instruction (unchanged by this plan; `docs/architecture/programs.md`)

`(s)` = signer, `(m)` = written. Slots: `[slot 0]` reference oracle, `[slot 1]` protocol-token definition.

| # | Instruction | Accounts in order |
|---|---|---|
| 0 | `init_config` | admin(s), config, accounting, buffer, protocol_parameters, stablecoin_definition, protocol_token_definition [slot 1], ref_oracle [slot 0] |
| 1 | `set_engine_params` | admin(s), config, accounting, coll_config, protocol_token_definition [slot 1], ref_oracle [slot 0] |
| 2 | `set_collateral_config` | admin(s), config, collateral_definition, coll_config, oracle, protocol_parameters, redemption_price_state, clock |
| 3 | `observe_feed` | caller(s), config, coll_config, redemption_price_state, oracle, clock |
| 4 | `liquidate(nonce)` | keeper(s), keeper_collateral_holding, config, coll_config, position, record, auction, auction_vault, engine_authority, liquidation_engine_state, host_vault, collateral_definition, stability_fee_accumulator, redemption_price_state, oracle, protocol_parameters, clock |
| 5 | `bid` | bidder_stable_holding(s), bidder_collateral_holding, config, coll_config, auction, auction_vault, buffer, stablecoin_definition, redemption_price_state, oracle, clock |
| 6 | `bid_with_settlement` | as `bid`, then accounting |
| 7 | `settle_empty` | caller(s), config, coll_config, accounting, auction |
| 8 | `claim_leftover` | owner(s), owner_collateral_holding, config, auction, auction_vault |
| 9 | `settle_debt(amount)` | caller(s), config, accounting, buffer, stablecoin_definition |
| 10 | `start_surplus_auction(nonce)` | caller(s), config, accounting, buffer, stablecoin_definition, surplus_auction, surplus_vault, ref_oracle [slot 0], clock |
| 11 | `take_surplus` | taker_protocol_holding(s), taker_stable_holding, config, accounting, surplus_auction, surplus_vault, protocol_token_definition [slot 1], clock |
| 12 | `close_surplus_auction` | caller(s), config, accounting, surplus_auction, surplus_vault, buffer, clock |
| 13 | `start_debt_auction(nonce)` | caller(s), config, accounting, buffer, stablecoin_definition, debt_auction, protocol_token_definition [slot 1], ref_oracle [slot 0], clock |
| 14 | `take_debt` | taker_stable_holding(s), taker_protocol_holding, config, accounting, debt_auction, stablecoin_definition, protocol_token_definition [slot 1], mint_authority, clock |
| 15 | `close_debt_auction` | caller(s), config, accounting, debt_auction, clock |
| 16 | `check_solvency` | caller(s), config, accounting, buffer, stablecoin_definition, protocol_token_definition [slot 1], ref_oracle [slot 0], engine_authority, liquidation_engine_state, protocol_parameters, clock |

A fresh (never initialised) recipient holding must also sign: keeper holding (liquidate), bidder collateral holding (bid), owner holding (claim_leftover), taker stablecoin holding (take_surplus), taker protocol holding (take_debt).

### S2. Host, token and oracle instructions the SDK encodes (enum index first; accounts from the vendored IDLs)

| Program | Instruction (enum index) | Accounts in order | Signers |
|---|---|---|---|
| stablecoin | `InitializeProgram` (0) | admin, protocol_parameters, stability_fee_accumulator, redemption_price_state, stablecoin_definition, stablecoin_master_holding, collateral_definition, market_price_oracle, clock | admin |
| stablecoin | `OpenPosition{position_nonce, initial_collateral_amount}` (4) | owner, position, vault, user_collateral_holding, collateral_definition, protocol_parameters, clock | owner, user_collateral_holding |
| stablecoin | `GenerateDebt{amount}` (6) | owner, position, stablecoin_definition, user_stablecoin_holding, stability_fee_accumulator, redemption_price_state, market_price_oracle, protocol_parameters, clock | owner |
| stablecoin | `SetMinimumCollateralizationRatio{new_ratio}` (8) | admin, protocol_parameters | admin |
| stablecoin | `SetControllerGains{new_proportional_gain: i128, new_integral_gain: i128}` (9) | admin, protocol_parameters | admin |
| stablecoin | `SetLiquidationEngine{engine_program_id, engine_authority_id}` (19) | admin, protocol_parameters, liquidation_engine_state | admin |
| token | `NewFungibleDefinition{name, total_supply, mint_authority}` (1) | definition_target_account, holding_target_account | both |
| token | `InitializeAccount` (3) | definition_account, account_to_initialize | account_to_initialize |
| token | `Transfer{amount_to_transfer}` (0) | sender, recipient | sender |
| twap_oracle | `CreateOraclePriceAccount{base_asset, quote_asset, initial_price, window_duration}` (1) | oracle_price_account = `compute_oracle_price_account_pda(twap, price_source, window)`, price_source, clock | price_source |

### S3. Engine error names (`EngineError::code()`, unchanged)

`NotAdmin 1, ZeroId 2, ZeroRefPrice 3, ThresholdBelowDebtLot 4, InvalidConfig 5, TypeCapAboveGlobal 6, BpsOutOfRange 7, ScheduleEncoding 8, TtlOutOfRange 9, ChipAbovePenalty 10, HostMismatch 11, WrongAccount 12, OracleMismatch 13, WrongClock 14, ProtocolTokenLocked 15, RefPriceStepTooLarge 16, Paused 20, PriceFeedStale 21, PriceFeedLowConfidence 22, PriceFeedOutOfRange 23, NonceMismatch 30, ZeroDebt 31, VaultBelowCollateral 32, CapacityExhausted 33, NotAuctioning 34, ZeroFill 35, Slippage 36, BelowMinBid 37, DustRemainder 38, NotOwner 39, NothingToClaim 40, ChainEndAccountsMissing 41, InsufficientBalance 42, RecipientMustSign 43, WrongDefinition 44, VaultBelowBook 45, TokenNotConfigured 50, UnbackedDebtPresent 51, BufferBelowTarget 52, DelayNotElapsed 53, InsufficientBadDebt 54, MintCapConfig 55, MintAuthorityMissing 56, ProtocolTokenMismatch 57, DebtAuctionAlreadyLive 58, SurplusAuctionAlreadyLive 59, NotLive 60, RecoveryInProgress 61, Math 99`. Tasks 2 and 3 reuse `ProtocolTokenMismatch` and `DelayNotElapsed`; no code is added or renumbered.

### S4. CLI exit codes (Task 13, ADR-044)

`0` success; `1` usage or other error; `2` verification failure (program-id / deployment mismatch, I4); `3` transaction dropped (re-quote); `4` keystore error.

---

## File structure (this plan)

```
Cargo.toml                                        members += crates/auction_sdk, cli (Tasks 4, 13)
programs/auction_engine/core/src/instruction.rs   Instruction::name() exhaustive + test (Task 1)
programs/auction_engine/core/src/args.rs          DEBT_RELIST_DELAY_MS (Task 3)
programs/auction_engine/core/src/state.rs         AccountingState.last_debt_end_ms (Task 3)
programs/auction_engine/src/accounts.rs           require_token_definition (Task 2)
programs/auction_engine/src/admin.rs              protocol-token validation calls (Task 2)
programs/auction_engine/src/debt.rs               relist delay, end-time bookkeeping (Task 3)
programs/auction_engine/src/{surplus_tests,system_tests,surplus_take_tests,debt_tests,debt_relist_tests,admin_tests}.rs
programs/integration_tests/tests/common/{mod,engine,system,sdk}.rs     split (Task 1), SDK adapter (Task 8)
programs/integration_tests/tests/{idl,config,debt,solvency,invariants,sdk_plans,sdk_quotes}.rs
crates/auction_sdk/src/
  lib.rs error.rs chain.rs encode.rs plan.rs testing.rs        (Task 4)
  addresses.rs decode.rs deployment.rs verify.rs               (Task 5)
  quote.rs + testing::standard()                               (Task 6)
  plan.rs builders, plan_host.rs                               (Task 7)
  keystore.rs tx.rs                                            (Task 9)
  confirm.rs                                                   (Task 10)
  scan.rs                                                      (Task 11)
  rpc.rs                                                       (Task 12, feature `rpc`)
deployments/localnet.json                                      (Task 5, refreshed Task 16)
cli/Cargo.toml cli/README.md cli/params/*.json
cli/src/{main,lib,args,env,output,read_cmds,tx_cmds,admin_cmds,params}.rs
cli/tests/{golden.rs,golden/*.txt}
scripts/localnet-smoke.sh                                      CLI instead of spel (Task 16)
docs/architecture/{sdk.md (new), programs.md, localnet.md, security-checklist.md}, THREAT_MODEL.md, DEPLOYMENTS.md, README.md, docs/design-log.md
```

---

### Task 1: Test hygiene: split oversized test files, exhaustive drift tests, ADR-037

Carry-overs "split oversized test files" and "make the drift tests catch new variants". No behaviour change: the number of passing tests per test binary is unchanged except where stated in Step 9.

**Files:**
- Modify: `programs/auction_engine/core/src/instruction.rs` (add `impl Instruction { pub fn name() }` and a test module)
- Modify: `programs/integration_tests/tests/idl.rs` (full replacement below)
- Split: `programs/integration_tests/tests/common/mod.rs` (1273 lines) into `common/mod.rs`, `common/engine.rs`, `common/system.rs`
- Split: `programs/auction_engine/src/surplus_tests.rs` (958 lines) into `surplus_tests.rs`, `system_tests.rs`, `surplus_take_tests.rs`; Modify: `programs/auction_engine/src/lib.rs`
- Modify: `docs/design-log.md` (ADR-037), `docs/architecture/programs.md` (carry-over list)

**Interfaces:**
- Consumes: `auction_engine_core::instruction::instruction_samples()` (hidden, test support).
- Produces: `auction_engine_core::Instruction::name(&self) -> &'static str` (the IDL / guest-function name; exhaustive match). Integration-test helpers keep their names and signatures; they now live in `common/engine.rs` (config, collateral, liquidate, bid, settle helpers) and `common/system.rs` (protocol token, surplus, debt, solvency helpers), re-exported by `common/mod.rs`. Every module-level `fn` and `const` of `surplus_tests.rs` becomes `pub(crate)`.

- [ ] **Step 1: Record the baseline**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_engine_core -p auction_engine_program -p engine_integration_tests 2>&1 | grep -E "Running|test result" | tee /tmp/plan3a-task1-baseline.txt'`
Expected: every line `test result: ok.`; the file is compared in Step 9.

- [ ] **Step 2: Write the failing core test**

Append to `programs/auction_engine/core/src/instruction.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instruction_samples_cover_every_variant_in_order() {
        let samples = instruction_samples();
        for (i, ix) in samples.iter().enumerate() {
            let words = risc0_zkvm::serde::to_vec(ix).expect("serializes");
            assert_eq!(
                usize::try_from(words[0]).expect("u32 fits usize"),
                i,
                "sample {i} ({}) is variant {i}",
                ix.name()
            );
        }
        let mut names: Vec<&str> = samples.iter().map(Instruction::name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), samples.len(), "one sample per variant");
    }
}
```

- [ ] **Step 3: Run it to verify it fails**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_engine_core instruction_samples_cover'`
Expected: FAIL to compile with `no method named 'name' found`.

- [ ] **Step 4: Add `Instruction::name`**

Insert between the enum and `instruction_samples()` in `programs/auction_engine/core/src/instruction.rs`:

```rust
impl Instruction {
    /// IDL name of the variant (== guest function name). Exhaustive on purpose: a new variant does not
    /// compile until it is named here; `instruction_samples` must then gain its sample, which the
    /// integration test `every_engine_variant_has_a_sample_and_an_idl_entry_at_its_enum_index` checks
    /// against the regenerated IDL.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::InitConfig { .. } => "init_config",
            Self::SetEngineParams { .. } => "set_engine_params",
            Self::SetCollateralConfig { .. } => "set_collateral_config",
            Self::ObserveFeed => "observe_feed",
            Self::Liquidate { .. } => "liquidate",
            Self::Bid { .. } => "bid",
            Self::BidWithSettlement { .. } => "bid_with_settlement",
            Self::SettleEmpty => "settle_empty",
            Self::ClaimLeftover => "claim_leftover",
            Self::SettleDebt { .. } => "settle_debt",
            Self::StartSurplusAuction { .. } => "start_surplus_auction",
            Self::TakeSurplus { .. } => "take_surplus",
            Self::CloseSurplusAuction => "close_surplus_auction",
            Self::StartDebtAuction { .. } => "start_debt_auction",
            Self::TakeDebt { .. } => "take_debt",
            Self::CloseDebtAuction => "close_debt_auction",
            Self::CheckSolvency => "check_solvency",
        }
    }
}
```

Run the Step 3 command. Expected: `test instruction::tests::instruction_samples_cover_every_variant_in_order ... ok`.

- [ ] **Step 5: Replace `programs/integration_tests/tests/idl.rs`**

The engine test now derives names from the exhaustive `Instruction::name`; the stablecoin test derives names from an exhaustive match (`stablecoin_name`; Task 4 moves it into the SDK) and additionally checks that the sample set equals the IDL set, so an IDL entry without a sample, or a sample without an IDL entry, fails. (A variant appended to the enum *and* the match but whose guest function was not added is caught by 3b's `idl-gen` diff in CI.)

```rust
//! Spec §3.2/§5.10: IDL position == enum index for every engine instruction (Shared Reference S1), and
//! the pinned IDL-position -> enum-index table of the vendored stablecoin.

fn idl_names(file: &str) -> Vec<String> {
    let path = format!("{}/../../idl/{file}", env!("CARGO_MANIFEST_DIR"));
    let idl: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("IDL file")).expect("IDL json");
    idl["instructions"]
        .as_array()
        .expect("array")
        .iter()
        .map(|i| i["name"].as_str().expect("name").to_owned())
        .collect()
}

fn word0<T: serde::Serialize>(ix: &T) -> u32 {
    risc0_zkvm::serde::to_vec(ix).expect("serializes")[0]
}

#[test]
fn every_engine_variant_has_a_sample_and_an_idl_entry_at_its_enum_index() {
    let samples = auction_engine_core::instruction::instruction_samples();
    let names = idl_names("auction_engine-idl.json");
    assert_eq!(
        samples.len(),
        names.len(),
        "one sample per IDL instruction (regenerate the IDL after adding a variant)"
    );
    for (i, ix) in samples.iter().enumerate() {
        assert_eq!(
            usize::try_from(word0(ix)).expect("u32 fits usize"),
            i,
            "sample {i} is variant {i}"
        );
        assert_eq!(ix.name(), names[i], "IDL position {i}");
    }
}

/// Exhaustive: a new stablecoin variant does not compile until it is named here.
fn stablecoin_name(ix: &stablecoin_core::Instruction) -> &'static str {
    use stablecoin_core::Instruction as I;
    match ix {
        I::InitializeProgram { .. } => "initialize_program",
        I::AccrueStabilityFee => "accrue_stability_fee",
        I::UpdateRedemptionRate => "update_redemption_rate",
        I::RefreshGlobals => "refresh_globals",
        I::OpenPosition { .. } => "open_position",
        I::DepositCollateral { .. } => "deposit_collateral",
        I::GenerateDebt { .. } => "generate_debt",
        I::ClosePosition => "close_position",
        I::SetMinimumCollateralizationRatio { .. } => "set_minimum_collateralization_ratio",
        I::SetControllerGains { .. } => "set_controller_gains",
        I::SetMarketPriceOracle => "set_market_price_oracle",
        I::SetTimingParameters { .. } => "set_timing_parameters",
        I::SetAdmin { .. } => "set_admin",
        I::SetFreezeAuthority { .. } => "set_freeze_authority",
        I::SetStabilityFeePerMillisecond { .. } => "set_stability_fee_per_millisecond",
        I::Freeze => "freeze",
        I::Unfreeze => "unfreeze",
        I::WithdrawCollateral { .. } => "withdraw_collateral",
        I::RepayDebt { .. } => "repay_debt",
        I::SetLiquidationEngine { .. } => "set_liquidation_engine",
        I::LiquidatePosition { .. } => "liquidate_position",
        I::EngineFreezeNewDebt => "engine_freeze_new_debt",
    }
}

/// One sample per `stablecoin_core::Instruction` variant (any order).
fn stablecoin_samples() -> Vec<stablecoin_core::Instruction> {
    use stablecoin_core::Instruction as I;
    let account = lee_core::account::AccountId::new([1; 32]);
    vec![
        I::InitializeProgram {
            freeze_authority_account_id: account,
            initial_stability_fee_per_millisecond: 0,
            initial_controller_proportional_gain: 0,
            initial_controller_integral_gain: 0,
            initial_minimum_collateralization_ratio: 0,
            minimum_milliseconds_between_rate_updates: 0,
            maximum_oracle_price_age_milliseconds: 0,
            initial_redemption_price: 0,
            stablecoin_name: String::new(),
        },
        I::AccrueStabilityFee,
        I::UpdateRedemptionRate,
        I::RefreshGlobals,
        I::OpenPosition {
            position_nonce: 0,
            initial_collateral_amount: 0,
        },
        I::DepositCollateral { amount: 0 },
        I::GenerateDebt { amount: 0 },
        I::ClosePosition,
        I::WithdrawCollateral { amount: 0 },
        I::RepayDebt { amount: 0 },
        I::SetStabilityFeePerMillisecond { new_rate: 0 },
        I::SetMinimumCollateralizationRatio { new_ratio: 0 },
        I::SetControllerGains {
            new_proportional_gain: 0,
            new_integral_gain: 0,
        },
        I::SetTimingParameters {
            new_minimum_milliseconds_between_rate_updates: 0,
            new_maximum_oracle_price_age_milliseconds: 0,
        },
        I::SetAdmin {
            new_admin_account_id: account,
        },
        I::SetFreezeAuthority {
            new_freeze_authority_account_id: account,
        },
        I::SetMarketPriceOracle,
        I::Freeze,
        I::Unfreeze,
        I::SetLiquidationEngine {
            engine_program_id: [1; 8],
            engine_authority_id: account,
        },
        I::LiquidatePosition {
            collateral_to_seize: 0,
            normalized_debt_to_cover: 0,
        },
        I::EngineFreezeNewDebt,
    ]
}

/// SDK encoders key on the enum index, never on the IDL position: for IDL positions 8..=18 the two
/// differ (the vendored IDL follows the upstream guest function order). `idl/stablecoin-idl.json` is
/// pinned to upstream and is not reordered.
#[test]
fn stablecoin_idl_position_maps_to_enum_index() {
    // IDL position -> enum index (docs/architecture/programs.md "Interfaces for Plan 3").
    const EXPECTED: [u32; 22] = [
        0, 1, 2, 3, 4, 5, 6, 7, 17, 18, 14, 8, 9, 11, 12, 13, 10, 15, 16, 19, 20, 21,
    ];
    let idl = idl_names("stablecoin-idl.json");
    let samples = stablecoin_samples();
    let mut sample_names: Vec<&str> = samples.iter().map(stablecoin_name).collect();
    sample_names.sort_unstable();
    let mut idl_sorted: Vec<&str> = idl.iter().map(String::as_str).collect();
    idl_sorted.sort_unstable();
    assert_eq!(
        sample_names, idl_sorted,
        "every variant has an IDL entry and every IDL entry has a sample"
    );
    assert_eq!(idl.len(), EXPECTED.len());
    for (position, name) in idl.iter().enumerate() {
        let ix = samples
            .iter()
            .find(|s| stablecoin_name(s) == name)
            .expect("sample exists (checked above)");
        assert_eq!(word0(ix), EXPECTED[position], "IDL position {position} ({name})");
    }
    let mut seen: Vec<u32> = samples.iter().map(word0).collect();
    seen.sort_unstable();
    assert_eq!(seen, (0..22).collect::<Vec<u32>>(), "enum indices 0..=21, each once");
}
```

`serde`, `serde_json` and `risc0-zkvm` are already dependencies of `engine_integration_tests`.

- [ ] **Step 6: Split `tests/common/mod.rs` (pure move)**

Run inside WSL (exact line ranges of the current file; the `use` lines 464–468 and 612 stay in `mod.rs`, so `measured()` and `checks.rs` keep resolving `Ix`, `pda` and the args types, and the moved files see them through `use super::*`):

```bash
cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine/programs/integration_tests/tests/common
cp mod.rs /tmp/common-mod-orig.rs
{ printf '%s\n' '//! Engine-side World helpers: config, collateral, liquidate, bid, settle (moved from mod.rs, Plan 3a Task 1).' 'use super::*;' ''; sed -n '470,611p;613,947p' /tmp/common-mod-orig.rs; } > engine.rs
{ printf '%s\n' '//! System-side World helpers: protocol token, surplus, debt, solvency (moved from mod.rs, Plan 3a Task 1).' 'use super::*;' ''; sed -n '949,1247p' /tmp/common-mod-orig.rs; } > system.rs
{ sed -n '1,461p' /tmp/common-mod-orig.rs; printf '\n'; sed -n '463,468p;612p' /tmp/common-mod-orig.rs; printf '%s\n' '' 'mod engine;' 'mod system;' 'pub use engine::*;' 'pub use system::*;' ''; sed -n '1249,1273p' /tmp/common-mod-orig.rs; } > mod.rs
wc -l mod.rs engine.rs system.rs
```

Expected: `mod.rs` about 500 lines, `engine.rs` about 480, `system.rs` about 300; all < 800.

- [ ] **Step 7: Split `surplus_tests.rs` (pure move)**

```bash
cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine/programs/auction_engine/src
cp surplus_tests.rs /tmp/surplus-tests-orig.rs
sed -i -e 's/^fn /pub(crate) fn /' -e 's/^const /pub(crate) const /' /tmp/surplus-tests-orig.rs
{ printf '%s\n' '//! Handler tests of `system::ref_price` and `system::protocol_token_status` (moved from surplus_tests.rs).'; sed -n '2,28p' /tmp/surplus-tests-orig.rs; printf '%s\n' 'use crate::surplus_tests::*;' ''; sed -n '461,591p' /tmp/surplus-tests-orig.rs; } > system_tests.rs
{ printf '%s\n' '//! Handler tests of `take_surplus` (moved from surplus_tests.rs).'; sed -n '2,28p' /tmp/surplus-tests-orig.rs; printf '%s\n' 'use crate::surplus_tests::*;' ''; sed -n '592,862p' /tmp/surplus-tests-orig.rs; } > surplus_take_tests.rs
{ sed -n '1,460p' /tmp/surplus-tests-orig.rs; sed -n '863,958p' /tmp/surplus-tests-orig.rs; } > surplus_tests.rs
wc -l surplus_tests.rs system_tests.rs surplus_take_tests.rs
```

Lines 2–28 are the module's `use` block (line 1 is its `//!` line). In `programs/auction_engine/src/lib.rs`, directly after the `mod surplus_tests;` line add:

```rust
#[cfg(test)]
mod surplus_take_tests;
#[cfg(test)]
mod system_tests;
```

Compile: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_engine_program --no-run 2>&1 | grep -E "^(warning|error)" -A4'`. The copied `use` blocks import more than each part needs: delete exactly the names rustc reports as `unused import` in `system_tests.rs` / `surplus_take_tests.rs` (and in `surplus_tests.rs` if it reports any), and nothing else. Re-run until it prints nothing.

- [ ] **Step 8: ADR-037 and the carry-over list**

Append to `docs/design-log.md`:

```markdown

---

## ADR-037: Plan 3 Is Split into 3a (Engine Hardening, SDK, CLI) and 3b (CI, Real Proofs, Reproducible Builds, Evidence)

**Context**
Plan 3 (ADR-018 split: SDK / CLI / CI / real proofs) gathered the spec §6.1, §6.4–§6.6, §9, §11 and §12 obligations plus thirteen carry-overs from Plan 2 (`docs/architecture/programs.md`). Planned as one document it is about 30 tasks in two independent deliverables.

**Options**
- (A) One Plan 3.
- (B) 3a = engine hardening (THREAT_MODEL findings 4 and 8), test hygiene, `auction_sdk`, `auction_cli`, CLI-driven localnet smoke; 3b = program CI with risc0, real-proof CI, Docker reproducible builds and the ImageID refresh, `CYCLES.md` and the ADR-023 re-measure, `REQUIREMENTS.md` and its checker, `cargo-fuzz`, coverage gate, red-team pass, oracle publisher/keeper, one-command demo, upstream issue drafts, `SUBMISSION.md`.
- (C) Split by layer (programs vs clients).

**Decision**
(B). 3a ships working software on its own (a user drives the engine from the CLI on localnet without spel). 3b depends on 3a (its real-proof end-to-end job and the demo drive the CLI); 3a does not depend on 3b. Both on-chain changes land in 3a, so 3b's reproducible builds refresh the ImageIDs once, after the last guest change.

**Trade-offs**
Until 3b, CI still runs only the `auction_core` job, and the ImageIDs in `DEPLOYMENTS.md` remain native-build values (THREAT_MODEL I4 residual). The 3a localnet run cannot show F4 or the freeze path: both need a shortfall, which needs a moving oracle price (3b publisher).

**Status**
Accepted (Plan 3a, Task 1)
```

In `docs/architecture/programs.md`, section "Plan 3 carry-overs", replace the two bullets that start "Split oversized test files" and "Make the drift tests catch new variants" with:

```markdown
- Done (Plan 3a Task 1): `tests/common/mod.rs` split into `mod.rs`/`engine.rs`/`system.rs`, `surplus_tests.rs` into `surplus_tests.rs`/`system_tests.rs`/`surplus_take_tests.rs`; the drift tests are exhaustive (`Instruction::name`, `stablecoin_name`) and compare the sample sets with the IDLs.
- Plan split: 3a (engine hardening, SDK, CLI) and 3b (CI, real proofs, reproducible builds, evidence), ADR-037.
```

- [ ] **Step 9: Run everything and compare with the baseline**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_engine_core -p auction_engine_program -p engine_integration_tests 2>&1 | grep -E "Running|test result"'`
Expected: all `ok`. Against `/tmp/plan3a-task1-baseline.txt`: the `auction_engine_core` unit binary has +1 passed; `tests/idl.rs` has 2 passed (it had 3: the two engine tests merged into one); `auction_engine_program` has the same total (tests only moved between files); every other binary is identical.

Then run the lint gate and the format gate (Global Constraints). Expected: both clean.

- [ ] **Step 10: Commit (controller — GPG signing on Windows)**

```bash
git add programs/auction_engine/core/src/instruction.rs programs/auction_engine/src/lib.rs programs/auction_engine/src/surplus_tests.rs programs/auction_engine/src/system_tests.rs programs/auction_engine/src/surplus_take_tests.rs programs/integration_tests/tests/idl.rs programs/integration_tests/tests/common/mod.rs programs/integration_tests/tests/common/engine.rs programs/integration_tests/tests/common/system.rs docs/design-log.md docs/architecture/programs.md
git commit -m "test: split oversized test files and make the drift tests exhaustive"
```

---

### Task 2: Config-time validation of the protocol-token slot (THREAT_MODEL finding 4, ADR-038)

`init_config` and `set_engine_params` record an enabled protocol-token slot by id only. A non-token account there lets an admin mistake lock a surplus lot until the TTL and, above the threshold, make `check_solvency` freeze the host. Fix: when the slot is enabled, require a **fungible token definition owned by the config's token program**. The mint authority is still checked at use time (`MintAuthorityMissing` stays a freeze reason, spec §5.11; the token program lets an authority be rotated with `SetAuthority`). In `set_engine_params` the check runs **after** the live-auction lock, so a locked change still reports `ProtocolTokenLocked`.

**Files:**
- Modify: `programs/auction_engine/src/accounts.rs` (new `require_token_definition`)
- Modify: `programs/auction_engine/src/admin.rs` (two call sites)
- Test: `programs/auction_engine/src/admin_tests.rs`; `programs/integration_tests/tests/config.rs`, `invariants.rs`, `debt.rs`, `solvency.rs`; Modify: `programs/integration_tests/tests/common/system.rs` (`with_system`)
- Modify: `THREAT_MODEL.md`, `docs/architecture/security-checklist.md`, `docs/architecture/programs.md`, `docs/design-log.md` (ADR-038)

**Interfaces:**
- Consumes: `token_core::TokenDefinition`, `EngineError::ProtocolTokenMismatch`, `World::create_protocol_token(supply, authority)`, `mint_authority()`, `protocol_token_def()` (Task 1 left them in `common/system.rs` / `common/engine.rs`).
- Produces: `pub fn require_token_definition(acc: &AccountWithMetadata, token_program: &ProgramId) -> Result<(), EngineError>` in `auction_engine_program::accounts`. Behaviour: an enabled protocol-token slot that is not a fungible definition owned by `cfg.token_program_id` fails `ProtocolTokenMismatch` (57) at `init_config` / `set_engine_params`. Fixtures (and the Task 16 runbook) create the mock token before a config names it. `admin_tests::token_def(id, owner)` is `pub(crate)` for later handler tests.

- [ ] **Step 1: Write the failing handler tests**

In `programs/auction_engine/src/admin_tests.rs`, add to the imports `use lee_core::account::Data;` and `use token_core::TokenDefinition;`, then append:

```rust
pub(crate) fn token_def(id: AccountId, owner: ProgramId) -> AccountWithMetadata {
    meta(
        id,
        Account {
            program_owner: owner,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("MOCK"),
                total_supply: 0,
                metadata_id: None,
                authority: None,
            }),
            ..Account::default()
        },
        false,
    )
}

fn nft_def(id: AccountId) -> AccountWithMetadata {
    meta(
        id,
        Account {
            program_owner: TOKEN,
            data: Data::from(&TokenDefinition::NonFungible {
                name: String::from("NFT"),
                printable_supply: 1,
                metadata_id: OTHER,
            }),
            ..Account::default()
        },
        false,
    )
}

#[test]
fn init_config_requires_the_protocol_token_to_be_a_fungible_token_definition() {
    let enabled = EngineParamsArgs {
        protocol_token_enabled: true,
        ..EngineParamsArgs::defaults_for_tests()
    };
    for (what, slot) in [
        ("uninitialised account", fresh(OTHER)),
        ("owned by another program", token_def(OTHER, HOST)),
        ("non-fungible definition", nft_def(OTHER)),
    ] {
        let mut a = init_accounts(ADMIN);
        a.protocol_token_definition = slot;
        assert_eq!(
            rejected(init(a, &enabled)),
            EngineError::ProtocolTokenMismatch,
            "{what}"
        );
    }
    let mut a = init_accounts(ADMIN);
    a.protocol_token_definition = token_def(OTHER, TOKEN);
    assert!(init(a, &enabled).is_ok(), "a fungible definition of the token program");
}

#[test]
fn set_engine_params_requires_a_token_definition_for_the_protocol_token() {
    let enabled = EngineParamsArgs {
        protocol_token_enabled: true,
        ..at_price(OLD_REF_PRICE)
    };
    let mut a = plain(ADMIN, true);
    a.protocol_token_definition = fresh(OTHER);
    assert_eq!(rejected(set(a, &enabled)), EngineError::ProtocolTokenMismatch);
    let mut a = plain(ADMIN, true);
    a.protocol_token_definition = token_def(OTHER, TOKEN);
    assert!(set(a, &enabled).is_ok());
}

#[test]
fn a_locked_protocol_token_change_still_reports_the_lock_first() {
    let live = AccountingState {
        surplus_live: true,
        ..AccountingState::new()
    };
    let enabled = EngineParamsArgs {
        protocol_token_enabled: true,
        ..at_price(OLD_REF_PRICE)
    };
    let mut a = set_accounts(ADMIN, true, None, &live);
    a.protocol_token_definition = fresh(OTHER);
    assert_eq!(rejected(set(a, &enabled)), EngineError::ProtocolTokenLocked);
}
```

In the existing test `set_engine_params_locks_a_protocol_token_swap_while_live`, replace its last block

```rust
    // Not live: the same swap is accepted.
    let mut a = plain(ADMIN, true);
    a.protocol_token_definition = fresh(OTHER);
    assert!(set(a, &enabled).is_ok());
```

with

```rust
    // Not live: the same swap is accepted when the slot is a real token definition (ADR-038).
    let mut a = plain(ADMIN, true);
    a.protocol_token_definition = token_def(OTHER, TOKEN);
    assert!(set(a, &enabled).is_ok());
```

- [ ] **Step 2: Run them to verify they fail**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_engine_program admin_tests'`
Expected: FAIL: `init_config_requires_the_protocol_token_to_be_a_fungible_token_definition` and `set_engine_params_requires_a_token_definition_for_the_protocol_token` panic with `handler must reject`; the other admin tests pass.

- [ ] **Step 3: Implement**

In `programs/auction_engine/src/accounts.rs`, change `use token_core::TokenHolding;` to `use token_core::{TokenDefinition, TokenHolding};` and add after `check_distinct_slots`:

```rust
/// ADR-038 (THREAT_MODEL finding 4): an enabled protocol-token slot must be a fungible token
/// definition owned by the config's token program. The mint authority is checked at use time
/// (`system::protocol_token_status`), because the token program lets it be rotated.
pub fn require_token_definition(
    acc: &AccountWithMetadata,
    token_program: &ProgramId,
) -> Result<(), EngineError> {
    if acc.account.program_owner != *token_program {
        return Err(EngineError::ProtocolTokenMismatch);
    }
    match TokenDefinition::try_from(&acc.account.data) {
        Ok(TokenDefinition::Fungible { .. }) => Ok(()),
        _ => Err(EngineError::ProtocolTokenMismatch),
    }
}
```

In `programs/auction_engine/src/admin.rs`, `init_config`: directly after the `check_distinct_slots(...)?;` call insert

```rust
    if protocol_token.is_some() {
        require_token_definition(&a.protocol_token_definition, &token_program_id)?;
    }
```

and in `set_engine_params`, directly after the block `if locked_fields_change && (acc.surplus_live || acc.debt_live) { return Err(EngineError::ProtocolTokenLocked); }` insert

```rust
    if protocol_token.is_some() {
        require_token_definition(&a.protocol_token_definition, &cfg.token_program_id)?;
    }
```

(`use crate::accounts::*` in `admin.rs` already brings it in.)

- [ ] **Step 4: Run the handler tests**

Run the Step 2 command. Expected: every `admin_tests` test passes.

- [ ] **Step 5: Integration tests and fixture order**

In `programs/integration_tests/tests/common/system.rs`, `World::with_system`: move the statement `w.create_protocol_token(10_000, mint_authority()).expect("mock protocol token");` so that it runs directly after `let mut w = Self::new();` (before `if ref_oracle_enabled`), with the comment `// ADR-038: the token must exist before the config names it.` above it.

In `programs/integration_tests/tests/debt.rs`, replace `fn debt_world` with:

```rust
fn debt_world(authority: Option<AccountId>, params: EngineParamsArgs) -> World {
    let mut w = World::new();
    // ADR-038: an enabled protocol token must already be a token definition when the config names it.
    w.create_protocol_token(10_000, authority.unwrap_or_else(mint_authority))
        .unwrap();
    w.init_engine_config(&params).unwrap();
    w.bind_engine().unwrap();
    w.set_collateral_config(k::ENGINE_ADMIN, &CollateralArgs::defaults_for_tests())
        .unwrap();
    if authority.is_none() {
        // Defensive path only: a validated slot cannot stop being a token definition on-chain, so the
        // use-time ProtocolTokenMismatch is reached by force-inserting an empty account over it.
        w.state
            .force_insert_account(protocol_token_def(), lee_core::account::Account::default());
    }
    w.book_shortfall();
    w.fund_stable(k::TAKER_STABLE, 1_000);
    w
}
```

In `programs/integration_tests/tests/solvency.rs`, replace `fn system_world` with:

```rust
fn system_world(authority: Option<AccountId>, params: EngineParamsArgs) -> World {
    let mut w = World::new();
    // ADR-038: an enabled protocol token must already be a token definition when the config names it.
    w.create_protocol_token(10_000, authority.unwrap_or_else(mint_authority))
        .unwrap();
    w.init_engine_config(&params).unwrap();
    w.bind_engine().unwrap();
    w.set_collateral_config(k::ENGINE_ADMIN, &CollateralArgs::defaults_for_tests())
        .unwrap();
    if authority.is_none() {
        // Defensive path only (see debt.rs): reached on-chain only through a force-inserted account.
        w.state
            .force_insert_account(protocol_token_def(), lee_core::account::Account::default());
    }
    w.book_shortfall();
    w
}
```

In `programs/integration_tests/tests/config.rs` append:

```rust
#[test]
fn protocol_token_must_be_a_token_definition_at_config_time() {
    let mut w = World::new();
    let p = EngineParamsArgs {
        protocol_token_enabled: true,
        ..EngineParamsArgs::defaults_for_tests()
    };
    // The slot names the mock-token id, but no definition exists there yet.
    assert_err(w.init_engine_config(&p), "ProtocolTokenMismatch");
    w.create_protocol_token(10_000, mint_authority())
        .expect("mock protocol token");
    w.init_engine_config(&p)
        .expect("a fungible definition owned by the token program");
    let cfg: EngineConfigState = w.read(config());
    assert_eq!(cfg.protocol_token, Some(protocol_token_def()));
}
```

In `programs/integration_tests/tests/invariants.rs`, replace the whole last function, from its doc comment line `/// Finding for the controller: with no live system auction nothing but` to the end of the file, with:

```rust
/// ADR-038 (THREAT_MODEL finding 4): the protocol-token slot is validated at config time, so the admin
/// can no longer point it at a fund account; the config and every fund stay unchanged.
#[test]
fn admin_cannot_point_the_protocol_token_slot_at_a_fund_account() {
    let mut w = World::with_system(false);
    let pos = w.unsafe_position();
    w.liquidate(pos, 0).unwrap();
    w.donate_stable(buffer(), 1_500);
    let before = fund_snapshot(&w, pos, false);
    let a = params_attempt(
        "set_engine_params (buffer as protocol-token slot, no live auction)",
        &admin_params(),
        [buffer(), unused(0)],
        accounting(),
        Some("ProtocolTokenMismatch"),
    );
    run_attempt(&mut w, pos, false, &a);
    let cfg: EngineConfigState = w.read(config());
    assert_eq!(
        cfg.protocol_token,
        Some(protocol_token_def()),
        "the configured token is unchanged"
    );
    assert_eq!(before, fund_snapshot(&w, pos, false), "no fund or book moved");
}
```

- [ ] **Step 6: Run the integration suite**

Run (background, timeout 3600000): `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_engine_program -p engine_integration_tests 2>&1 | grep -E "Running|test result|FAILED|panicked"'`
Expected: every `test result: ok`; `config.rs` +1 passed; `invariants.rs` same count (one test replaced); `debt.rs` `start_rejections_map_to_the_freeze_list` and the `solvency.rs` freeze-list test still pass (their `None` case now goes through the force-inserted account).

- [ ] **Step 7: Regenerate the IDL (no change expected)**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh run -q -p idl-gen -- programs/auction_engine/methods/guest/src/bin/auction_engine.rs > idl/auction_engine-idl.json && git diff --exit-code idl/ && echo IDL-UNCHANGED'`
Expected: `IDL-UNCHANGED` (no argument or account list changed).

- [ ] **Step 8: Security pass and documentation**

Re-read `init_config` and `set_engine_params` against the checklist columns (owner/definition checks; checks before effects; no fund path); the controller dispatches the security-reviewer on the diff (spec §10 process). Then:

`THREAT_MODEL.md`:
- Replace the residual bullet that starts with `- **Protocol token set to a non-token account.**` (one line) with:
  ``- **Protocol token set to a non-token account: fixed (ADR-038).** `init_config` and `set_engine_params` require an enabled protocol-token slot to be a fungible token definition owned by the config's token program (`ProtocolTokenMismatch`; tests `init_config_requires_the_protocol_token_to_be_a_fungible_token_definition`, `protocol_token_must_be_a_token_definition_at_config_time`, `admin_cannot_point_the_protocol_token_slot_at_a_fund_account`). The mint authority is still checked at use time, because the token program can rotate it; a token without `["MINT_AUTH", config]` as authority keeps `MintAuthorityMissing` as a `check_solvency` freeze reason (spec §5.11). The use-time `ProtocolTokenMismatch` checks stay as defence in depth; on-chain they are no longer reachable for a validated slot.``
- In the review-log table, row 4, replace the Resolution cell text `accepted for Plan 2 (admin-only, no fund loss; residual documented above); config-time validation proposed for the final review / Plan 3` with `fixed in Plan 3a Task 2 (ADR-038): config-time validation of the slot`.
- Append after the last review-log paragraph:
  ``**2026-10-09, Plan 3a Task 2 (finding 4).** `require_token_definition` (owner = `cfg.token_program_id`, `TokenDefinition::Fungible`) runs in `init_config` after the slot checks and in `set_engine_params` after the live-auction lock, so a locked change still reports `ProtocolTokenLocked`. No fund path changed; IDL unchanged.``
  The controller appends to this paragraph one sentence stating the security reviewer's verdict on the diff (reviewer, date, highest severity found) before committing.

`docs/architecture/security-checklist.md`, table "Engine instructions": in the `init_config` row replace the last cell `n/a — protocol-token slot not validated as a token (THREAT_MODEL: MEDIUM, accepted)` with ``yes — an enabled protocol-token slot must be a fungible definition of `cfg.token_program_id` (ADR-038)``; in the `set_engine_params` row, column "Owner / tag / definition", replace ``yes — `load_*` (owner, tag)`` with ``yes — `load_*` (owner, tag); an enabled protocol-token slot is a fungible definition of the token program, checked after the lock (ADR-038)``.

`docs/architecture/programs.md`: in "Plan 3 carry-overs" replace the bullet "Config-time check that `protocol_token` is a token definition owned by the token program (THREAT_MODEL finding 4)." with "Done (Plan 3a Task 2, ADR-038): config-time check that an enabled `protocol_token` is a fungible token definition owned by the token program." In the F5 section, after the sentence that ends "(`ProtocolTokenMismatch`; `TokenNotConfigured` when unset).", add: "Since ADR-038 the slot is also validated when the config names it."

Append to `docs/design-log.md`:

```markdown

---

## ADR-038: The Protocol-Token Slot Is Validated at Config Time

**Context**
THREAT_MODEL finding 4 (MEDIUM, accepted in Plan 2): `init_config` / `set_engine_params` recorded an enabled protocol-token slot by id only. A non-token account there let `start_surplus_auction` lock the surplus lot until `close_surplus_auction` (every take fails `ProtocolTokenMismatch`), and made `start_debt` fail `ProtocolTokenMismatch`, which `check_solvency` maps to Freeze above the threshold.

**Options**
- (A) Require a fungible token definition owned by the config's token program.
- (B) (A) plus `["MINT_AUTH", config]` as its mint authority.
- (C) Keep use-time checks only.

**Decision**
(A), checked in `init_config` after the slot checks and in `set_engine_params` after the live-auction lock (so a locked change still reports `ProtocolTokenLocked`). Not (B): the token program can rotate a mint authority (`SetAuthority`), so a config-time authority check would not hold later, and a surplus-only token without a mint authority is legitimate; `MintAuthorityMissing` remains a spec §5.11 freeze reason at use time.

**Trade-offs**
The mock token must exist before a config names it (fixtures and the localnet runbook create it first). The use-time `ProtocolTokenMismatch` checks remain as defence in depth; tests reach them only through force-inserted accounts.

**Status**
Accepted (Plan 3a, Task 2)
```

- [ ] **Step 9: Lint and format gates**

Run both gates from Global Constraints. Expected: clean.

- [ ] **Step 10: Commit (controller — GPG signing on Windows)**

```bash
git add programs/auction_engine/src/accounts.rs programs/auction_engine/src/admin.rs programs/auction_engine/src/admin_tests.rs programs/integration_tests/tests/common/system.rs programs/integration_tests/tests/config.rs programs/integration_tests/tests/invariants.rs programs/integration_tests/tests/debt.rs programs/integration_tests/tests/solvency.rs THREAT_MODEL.md docs/architecture/security-checklist.md docs/architecture/programs.md docs/design-log.md
git commit -m "fix(engine): validate the protocol-token slot at config time (finding 4)"
```

---

### Task 3: Debt-auction relist delay (THREAT_MODEL finding 8, ADR-039)

Relist griefing: anyone can start a debt auction, wait `system_ttl_ms`, close it and start the next, keeping `debt_live` true, so the admin can change the protocol token, its oracle or the system parameters only by winning the race between a close and the next start. Of the two candidate fixes in the carry-over list, an atomic close-expired in `set_engine_params` still loses that race when the griefer's close and start are ordered first in the same block. Fix chosen: a fixed relist delay `DEBT_RELIST_DELAY_MS = 600_000` (10 min, the spec's minimum system TTL) after any debt auction ends (close or last take), enforced in `start_debt_auction`. During the delay no debt auction is live, so `set_engine_params` is unlocked: the admin always has a guaranteed window. No instruction argument or account list changes (no IDL change); `AccountingState` gains one field, `last_debt_end_ms`.

**Files:**
- Modify: `programs/auction_engine/core/src/args.rs` (constant), `programs/auction_engine/core/src/state.rs` (`AccountingState.last_debt_end_ms`)
- Modify: `programs/auction_engine/src/debt.rs` (check in `start_debt_auction`, end time in `take_debt` and `close_debt_auction`)
- Modify: `programs/auction_engine/src/debt_tests.rs` (helpers `pub(crate)`); Create: `programs/auction_engine/src/debt_relist_tests.rs`; Modify: `programs/auction_engine/src/lib.rs`
- Test: `programs/integration_tests/tests/debt.rs`
- Modify: `THREAT_MODEL.md`, `docs/architecture/security-checklist.md`, `docs/architecture/programs.md`, `docs/design-log.md` (ADR-039)

**Interfaces:**
- Consumes: `debt_tests` helpers `start_fixture()`, `start_accounting()`, `run_start(a, nonce)`, `debt_id(nonce)`, `take_fixture()`, `run_take(a, coin, min_out)`, `close_fixture(lot_left, now)`, constants `LOT`; `surplus_tests::{accounting_account, accounting_of, NOW}`; `admin_tests::{fresh, rejected, ENGINE}`; integration helpers `debt_world`, `token_params`, `take`, `acc`, `World::{start_debt, close_debt, advance, set_engine_params}`.
- Produces: `pub const DEBT_RELIST_DELAY_MS: u64 = 600_000;` in `auction_engine_core::args`; `pub last_debt_end_ms: u64` as the **last** field of `AccountingState` (borsh layout grows by 8 bytes; 0 = no debt auction has ended yet). `start_debt_auction` fails `DelayNotElapsed` (53) when `last_debt_end_ms > 0 && now < last_debt_end_ms + DEBT_RELIST_DELAY_MS`. The SDK (Task 6) and CLI show the earliest next start.

- [ ] **Step 1: Make the debt handler-test helpers shareable**

Run inside WSL:

```bash
cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine/programs/auction_engine/src
sed -i -e 's/^fn /pub(crate) fn /' -e 's/^const /pub(crate) const /' debt_tests.rs
```

(Test functions become `pub(crate) fn` too; harmless.)

- [ ] **Step 2: Write the failing handler tests**

Create `programs/auction_engine/src/debt_relist_tests.rs`:

```rust
//! ADR-039 (THREAT_MODEL finding 8): a debt auction cannot be relisted until DEBT_RELIST_DELAY_MS after
//! the previous one ended, which gives the admin a window with no live debt auction.

use auction_engine_core::{
    args::{EngineParamsArgs, DEBT_RELIST_DELAY_MS},
    state::*,
    EngineError,
};

use crate::{
    admin_tests::{fresh, rejected, ENGINE},
    debt::close_debt_auction,
    debt_tests::*,
    surplus_tests::{accounting_account, accounting_of, NOW},
};

fn ended_at(ms: u64) -> AccountingState {
    AccountingState {
        last_debt_end_ms: ms,
        debt_nonce: 1,
        ..start_accounting()
    }
}

#[test]
fn start_waits_for_the_relist_delay_after_the_previous_auction_ended() {
    let mut f = start_fixture();
    f.accounting = accounting_account(&ended_at(NOW - DEBT_RELIST_DELAY_MS + 1));
    f.debt_auction = fresh(debt_id(1));
    assert_eq!(rejected(run_start(f, 1)), EngineError::DelayNotElapsed);

    let mut f = start_fixture();
    f.accounting = accounting_account(&ended_at(NOW - DEBT_RELIST_DELAY_MS));
    f.debt_auction = fresh(debt_id(1));
    assert!(run_start(f, 1).is_ok(), "the delay has elapsed exactly");
}

#[test]
fn the_first_debt_auction_has_no_delay() {
    assert!(run_start(start_fixture(), 0).is_ok());
}

#[test]
fn close_records_the_end_time() {
    let ttl = EngineParamsArgs::defaults_for_tests().system_ttl_ms;
    let (post, _) =
        close_debt_auction(ENGINE, close_fixture(LOT, NOW + ttl)).expect("close after the ttl");
    assert_eq!(accounting_of(&post[2]).last_debt_end_ms, NOW + ttl);
}

#[test]
fn the_last_take_records_the_end_time_and_a_partial_take_does_not() {
    let (post, _) = run_take(take_fixture(), LOT, 0).expect("full take ends the auction");
    assert_eq!(accounting_of(&post[3]).last_debt_end_ms, NOW);
    let (post, _) = run_take(take_fixture(), LOT / 2, 0).expect("partial take");
    assert_eq!(accounting_of(&post[3]).last_debt_end_ms, 0);
}
```

In `programs/auction_engine/src/lib.rs`, after `mod debt_tests;` add:

```rust
#[cfg(test)]
mod debt_relist_tests;
```

- [ ] **Step 3: Run them to verify they fail**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_engine_program debt_relist'`
Expected: FAIL to compile: `cannot find value DEBT_RELIST_DELAY_MS` and `struct AccountingState has no field named last_debt_end_ms`.

- [ ] **Step 4: Implement**

`programs/auction_engine/core/src/args.rs`, after `MAX_SYSTEM_TTL_MS`:

```rust
/// ADR-039: no debt auction starts within this delay after the previous one ended (close or last
/// take), so the admin always has a window in which no debt auction is live (THREAT_MODEL finding 8).
/// Equal to the minimum system-auction TTL (spec §5.12).
pub const DEBT_RELIST_DELAY_MS: u64 = 600_000;
```

`programs/auction_engine/core/src/state.rs`: append the field as the last field of `AccountingState`:

```rust
    /// When the last debt auction ended (close or final take), ms; 0 before the first one (ADR-039).
    pub last_debt_end_ms: u64,
```

and in `AccountingState::empty()` add `last_debt_end_ms: 0,` as its last field.

`programs/auction_engine/src/debt.rs`: change the import to `use auction_engine_core::{args::DEBT_RELIST_DELAY_MS, pda, state::*, EngineError};`. In `start_debt_auction`, directly after the `if nonce != acc.debt_nonce { return Err(EngineError::NonceMismatch); }` block insert:

```rust
    // ADR-039: no relisting within DEBT_RELIST_DELAY_MS of the previous debt auction's end.
    if acc.last_debt_end_ms > 0 && now < acc.last_debt_end_ms.saturating_add(DEBT_RELIST_DELAY_MS) {
        return Err(EngineError::DelayNotElapsed);
    }
```

In `take_debt`, replace

```rust
    if !next.live {
        next_acc.debt_live = false;
    }
```

with

```rust
    if !next.live {
        next_acc.debt_live = false;
        next_acc.last_debt_end_ms = now; // ADR-039
    }
```

In `close_debt_auction`, replace

```rust
    let next_acc = AccountingState {
        debt_live: false,
        ..AccountingState::with_core(&acc, &acc.core().record_debt_close(released))
    };
```

with

```rust
    let next_acc = AccountingState {
        debt_live: false,
        last_debt_end_ms: now, // ADR-039
        ..AccountingState::with_core(&acc, &acc.core().record_debt_close(released))
    };
```

- [ ] **Step 5: Run the handler tests**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_engine_core -p auction_engine_program'`
Expected: all pass, `debt_relist_tests` 4 passed.

- [ ] **Step 6: Integration tests (real guests)**

Append to `programs/integration_tests/tests/debt.rs` (add `use auction_engine_core::args::DEBT_RELIST_DELAY_MS;` to the imports):

```rust
#[test]
fn a_closed_debt_auction_cannot_be_relisted_before_the_delay_so_the_admin_can_swap_the_token() {
    let mut w = debt_world(Some(mint_authority()), token_params());
    w.start_debt(0).unwrap();
    w.advance(token_params().system_ttl_ms);
    w.close_debt(0).expect("anyone closes after the ttl");
    assert_err(w.start_debt(1), "DelayNotElapsed");
    let off = EngineParamsArgs {
        protocol_token_enabled: false,
        ..token_params()
    };
    w.set_engine_params(k::ENGINE_ADMIN, &off)
        .expect("no debt auction is live during the relist delay: the admin may change the token");
    w.advance(DEBT_RELIST_DELAY_MS);
    assert_err(w.start_debt(1), "TokenNotConfigured");
}

#[test]
fn a_fully_taken_debt_auction_also_starts_the_relist_delay() {
    let mut w = debt_world(Some(mint_authority()), token_params());
    w.start_debt(0).unwrap();
    take(&mut w, 500, 0).expect("take the whole lot");
    assert_err(w.start_debt(1), "DelayNotElapsed");
    w.advance(DEBT_RELIST_DELAY_MS);
    w.start_debt(1).expect("relist after the delay");
    assert!(acc(&w).debt_live);
}
```

Run (background, timeout 3600000): `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p engine_integration_tests 2>&1 | grep -E "Running|test result|FAILED|panicked"'`
Before running, update the one existing test that relists immediately: in `programs/integration_tests/tests/debt.rs`, `start_needs_the_next_nonce_and_a_single_live_auction`, insert the line `w.advance(DEBT_RELIST_DELAY_MS); // ADR-039 relist delay` directly after `take(&mut w, TAKE_ALL, 0).unwrap();` (before `w.start_debt(1)`). No other existing integration test starts a second debt auction (`grep -n "start_debt(" programs/integration_tests/tests/*.rs` lists nonce 0 elsewhere).

Expected: all `ok`; `debt.rs` +2 passed.

- [ ] **Step 7: Regenerate the IDL (no change expected)**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh run -q -p idl-gen -- programs/auction_engine/methods/guest/src/bin/auction_engine.rs > idl/auction_engine-idl.json && git diff --exit-code idl/ && echo IDL-UNCHANGED'`
Expected: `IDL-UNCHANGED`.

- [ ] **Step 8: Security pass and documentation**

The controller dispatches the security-reviewer on the diff (spec §10). Check: the delay cannot be bypassed (both end paths record it; `last_debt_end_ms` is only written by the engine on the `Accounting` PDA); `check_solvency` is unaffected (it never starts an auction; during the delay with a working token it returns `NoAction`, with a broken token it still freezes).

`THREAT_MODEL.md`:
- Replace the residual bullet that starts with `- **Debt-auction relist griefing.**` with:
  ``- **Debt-auction relist griefing: mitigated (ADR-039).** `start_debt_auction` refuses (`DelayNotElapsed`) within `DEBT_RELIST_DELAY_MS` (10 min) of the previous debt auction's end (`close_debt_auction` or the final take), so after any end the admin has a window with `debt_live == false` in which `set_engine_params` can change the protocol token, its oracle or the system parameters (`a_closed_debt_auction_cannot_be_relisted_before_the_delay_so_the_admin_can_swap_the_token`). A griefer can still keep one auction live for `system_ttl_ms` and then relist after the delay, and while it is live `check_solvency` reverts `RecoveryInProgress` (spec §5.11 behaviour). The surplus side relies on the admin-set `surplus_delay_ms` for the same window; set it above zero in production.``
- Review-log row 8: replace the Resolution cell ``accepted for Plan 2 (residual documented above); `debt_delay_ms` or an atomic close in `set_engine_params` proposed for Plan 3`` with `mitigated in Plan 3a Task 3 (ADR-039): fixed relist delay after any debt-auction end`.
- Append: ``**2026-10-09, Plan 3a Task 3 (finding 8).** `AccountingState.last_debt_end_ms` is set by `close_debt_auction` and by the final `take_debt`; `start_debt_auction` refuses within `DEBT_RELIST_DELAY_MS`. No argument or account list changed (IDL unchanged); the `Accounting` borsh layout grew by one `u64` (a fresh deployment is required).`` The controller appends one sentence with the security reviewer's verdict on the diff (reviewer, date, highest severity found) before committing.

`docs/architecture/security-checklist.md`, `start_debt_auction` row, column "Caps": append `; relist delay after the previous end (ADR-039)`.

`docs/architecture/programs.md`: in the F4 section list of `start_debt_auction` conditions, after "`nonce == Accounting.debt_nonce` (`NonceMismatch`);" insert "no relist within `DEBT_RELIST_DELAY_MS` (600_000 ms) of the previous debt auction's end, `Accounting.last_debt_end_ms` (`DelayNotElapsed`, ADR-039);". In "Plan 3 carry-overs" replace the bullet "Debt relist griefing hardening (...)" with "Done (Plan 3a Task 3, ADR-039): fixed relist delay between debt auctions."

Append to `docs/design-log.md`:

```markdown

---

## ADR-039: Fixed Relist Delay Between Debt Auctions

**Context**
THREAT_MODEL finding 8 (MEDIUM, accepted in Plan 2): `start_debt_auction` and `close_debt_auction` are permissionless and the debt side had no delay, so a griefer could keep `debt_live` true almost all the time; `set_engine_params` then refuses protocol-token, oracle and system-parameter changes (`ProtocolTokenLocked`), and the admin wins only by ordering two transactions (close, change) between the griefer's close and next start.

**Options**
- (A) A `debt_delay_ms` engine parameter: new instruction argument, `SystemParams` field in `auction_core`, `start_debt` signature change (about 60 edit sites, IDL change).
- (B) `set_engine_params` closes an expired debt auction atomically: the admin still loses when the griefer's close and start are ordered first at expiry.
- (C) A fixed program-layer relist delay after any debt-auction end, recorded in `Accounting`.

**Decision**
(C), `DEBT_RELIST_DELAY_MS = 600_000` (the spec's minimum system TTL). After any end the admin has a guaranteed 10-minute window with no live debt auction. Engine-only: no argument, account list or `auction_core` change.

**Trade-offs**
Debt auctions are at most one per `system_ttl_ms + 10 min`, which slows recovery negligibly. The delay is not configurable (a parameter can be added in M3 if governance wants one). `AccountingState` grows by one `u64`, so existing localnet state needs a fresh deployment (Task 16 redeploys anyway).

**Status**
Accepted (Plan 3a, Task 3)
```

- [ ] **Step 9: Lint and format gates**

Run both gates. Expected: clean.

- [ ] **Step 10: Commit (controller — GPG signing on Windows)**

```bash
git add programs/auction_engine/core/src/args.rs programs/auction_engine/core/src/state.rs programs/auction_engine/src/debt.rs programs/auction_engine/src/debt_tests.rs programs/auction_engine/src/debt_relist_tests.rs programs/auction_engine/src/lib.rs programs/integration_tests/tests THREAT_MODEL.md docs/architecture/security-checklist.md docs/architecture/programs.md docs/design-log.md
git commit -m "fix(engine): relist delay between debt auctions (finding 8)"
```

---

### Task 4: `auction_sdk` scaffold: errors, `Chain`, `FixtureChain`, own encoders, `Plan`

**Files:**
- Modify: `Cargo.toml` (workspace member and dependency)
- Create: `crates/auction_sdk/Cargo.toml`, `crates/auction_sdk/src/{lib.rs,error.rs,chain.rs,encode.rs,plan.rs}`, `crates/auction_sdk/src/testing/mod.rs`
- Modify: `programs/integration_tests/Cargo.toml` (dev-dependency), `programs/integration_tests/tests/idl.rs` (use the SDK's `stablecoin_name`)
- Create: `docs/architecture/sdk.md`; Modify: `docs/design-log.md` (ADR-040), `README.md` (Status)

**Interfaces:**
- Consumes: `auction_engine_core::{Instruction, instruction::instruction_samples, args::{EngineParamsArgs, CollateralArgs}}`, `lee::program::Program::serialize_instruction`, `clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID}`.
- Produces (later tasks rely on these exact names):
  - `auction_sdk::error::{SdkError, SdkResult}`; `SdkError` variants `Encode(String)`, `Missing { what: &'static str, id: String }`, `Decode { what: &'static str, id: String, reason: String }`, `Mismatch { what: &'static str, expected: String, found: String }`, `Engine { name: String, code: u32 }`, `Deployment { path: String, reason: String }`, `Chain(String)`, `InFlight { signer: String, lock: String, blocks: u64 }`, `Scan(String)`, `Io { path: String, reason: String }`; `impl From<EngineError> for SdkError`.
  - `auction_sdk::chain::{Chain, BlockSummary, TxSummary, clock_ms}` (plus `impl<T: Chain + ?Sized> Chain for &T`): `trait Chain { fn account(&self, id: &AccountId) -> SdkResult<Account>; fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<Nonce>>; fn last_block_id(&self) -> SdkResult<u64>; fn blocks(&self, from: u64, to: u64) -> SdkResult<Vec<BlockSummary>>; fn send(&self, tx: &PublicTransaction) -> SdkResult<[u8; 32]>; }`; `BlockSummary { id: u64, hash: [u8; 32], prev_hash: [u8; 32], timestamp_ms: u64, txs: Vec<TxSummary> }`; `TxSummary { hash: [u8; 32], program_id: ProgramId, account_ids: Vec<AccountId>, instruction_words: Vec<u32> }`; `fn clock_ms<C: Chain + ?Sized>(chain: &C) -> SdkResult<u64>`.
  - `auction_sdk::encode::{words, variant_index, stablecoin_name, init_config_ix, set_engine_params_ix, set_collateral_config_ix, STABLECOIN_IDL_POSITION_TO_ENUM}`.
  - `auction_sdk::plan::Plan { program_id: ProgramId, account_ids: Vec<AccountId>, signing_requirements: Vec<AccountId>, instruction_words: Vec<u32> }`, `Plan::new<T: Serialize>(program_id, account_ids, signing_requirements, ix: &T) -> SdkResult<Plan>` (rejects duplicate accounts, signers not listed, no signer), `Plan::variant_index(&self) -> Option<u32>`.
  - `auction_sdk::testing` (feature `testing`): `key(n: u8) -> PrivateKey`, `id(n: u8) -> AccountId` (same scheme as the integration harness), `FixtureChain` with `new, put, put_state, put_holding, set_clock, push_block, rewrite_from, hide_block, set_send_mode, set_height_step, set_nonce, sent`, `SendMode { Include, Drop, Reject }`.

- [ ] **Step 1: Workspace and crate manifest**

Root `Cargo.toml`: add `"crates/auction_sdk",` to `members` directly after `"crates/auction_core",`, and add to `[workspace.dependencies]`:

```toml
auction_sdk = { path = "crates/auction_sdk" }
```

Create `crates/auction_sdk/Cargo.toml`:

```toml
[package]
name = "auction_sdk"
version = "0.1.0"
edition.workspace = true
license.workspace = true
description = "Client SDK of the RFP-014 auction engine on LEZ v0.2.4: own encoders, quotes, plans, confirmation, discovery."

[features]
default = []
# In-memory FixtureChain and the standard fixture, shared by SDK, CLI and integration tests.
testing = []

[dependencies]
auction_core = { workspace = true }
auction_engine_core = { workspace = true }
stablecoin_core = { workspace = true }
token_core = { workspace = true }
twap_oracle_core = { workspace = true }
lee = { workspace = true }
lee_core = { workspace = true }
clock_core = { git = "https://github.com/logos-blockchain/logos-execution-zone.git", tag = "v0.2.4" }
borsh = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
hex = "0.4"
thiserror = "2"

[dev-dependencies]
risc0-zkvm = { workspace = true }
```

In `programs/integration_tests/Cargo.toml` `[dev-dependencies]` add `auction_sdk = { workspace = true, features = ["testing"] }`.

- [ ] **Step 2: Write the failing encoder tests**

Create `crates/auction_sdk/src/encode.rs` with only the test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use auction_engine_core::instruction::instruction_samples;

    fn idl_names(json: &str) -> Vec<String> {
        let v: serde_json::Value = serde_json::from_str(json).expect("IDL json");
        v["instructions"]
            .as_array()
            .expect("array")
            .iter()
            .map(|i| i["name"].as_str().expect("name").to_owned())
            .collect()
    }

    #[test]
    fn engine_encoding_is_the_enum_index_and_equals_the_idl_position() {
        let names = idl_names(include_str!("../../../idl/auction_engine-idl.json"));
        let samples = instruction_samples();
        assert_eq!(samples.len(), names.len());
        for (i, ix) in samples.iter().enumerate() {
            let w = words(ix).expect("encodes");
            assert_eq!(variant_index(&w), Some(u32::try_from(i).expect("small")));
            assert_eq!(ix.name(), names[i], "IDL position {i}");
        }
    }

    fn stablecoin_samples() -> Vec<stablecoin_core::Instruction> {
        use stablecoin_core::Instruction as I;
        let a = lee_core::account::AccountId::new([1; 32]);
        vec![
            I::InitializeProgram {
                freeze_authority_account_id: a,
                initial_stability_fee_per_millisecond: 0,
                initial_controller_proportional_gain: 0,
                initial_controller_integral_gain: 0,
                initial_minimum_collateralization_ratio: 0,
                minimum_milliseconds_between_rate_updates: 0,
                maximum_oracle_price_age_milliseconds: 0,
                initial_redemption_price: 0,
                stablecoin_name: String::new(),
            },
            I::AccrueStabilityFee,
            I::UpdateRedemptionRate,
            I::RefreshGlobals,
            I::OpenPosition { position_nonce: 0, initial_collateral_amount: 0 },
            I::DepositCollateral { amount: 0 },
            I::GenerateDebt { amount: 0 },
            I::ClosePosition,
            I::WithdrawCollateral { amount: 0 },
            I::RepayDebt { amount: 0 },
            I::SetStabilityFeePerMillisecond { new_rate: 0 },
            I::SetMinimumCollateralizationRatio { new_ratio: 0 },
            I::SetControllerGains { new_proportional_gain: 0, new_integral_gain: 0 },
            I::SetTimingParameters {
                new_minimum_milliseconds_between_rate_updates: 0,
                new_maximum_oracle_price_age_milliseconds: 0,
            },
            I::SetAdmin { new_admin_account_id: a },
            I::SetFreezeAuthority { new_freeze_authority_account_id: a },
            I::SetMarketPriceOracle,
            I::Freeze,
            I::Unfreeze,
            I::SetLiquidationEngine { engine_program_id: [1; 8], engine_authority_id: a },
            I::LiquidatePosition { collateral_to_seize: 0, normalized_debt_to_cover: 0 },
            I::EngineFreezeNewDebt,
        ]
    }

    #[test]
    fn stablecoin_encoding_uses_the_enum_index_not_the_idl_position() {
        let names = idl_names(include_str!("../../../idl/stablecoin-idl.json"));
        assert_eq!(names.len(), STABLECOIN_IDL_POSITION_TO_ENUM.len());
        let samples = stablecoin_samples();
        assert_eq!(samples.len(), names.len(), "one sample per variant");
        for (position, name) in names.iter().enumerate() {
            let ix = samples
                .iter()
                .find(|s| stablecoin_name(s) == name)
                .unwrap_or_else(|| panic!("no sample for {name}"));
            assert_eq!(
                variant_index(&words(ix).expect("encodes")),
                Some(STABLECOIN_IDL_POSITION_TO_ENUM[position]),
                "{name}"
            );
        }
        // Spec §3.2 example: Withdraw is enum 17 but IDL position 8.
        let w = words(&stablecoin_core::Instruction::WithdrawCollateral { amount: 1 }).expect("encodes");
        assert_eq!(w[0], 17);
    }

    #[test]
    fn negative_controller_gains_encode_and_round_trip() {
        let ix = stablecoin_core::Instruction::SetControllerGains {
            new_proportional_gain: -5_000_000_000_000_000_000_000_000_000,
            new_integral_gain: -1,
        };
        let w = words(&ix).expect("i128 encodes");
        assert_eq!(w[0], 9);
        let back: stablecoin_core::Instruction =
            risc0_zkvm::serde::from_slice(&w).expect("the guest decodes it");
        assert_eq!(format!("{back:?}"), format!("{ix:?}"));
    }

    #[test]
    fn config_instruction_builders_round_trip() {
        let p = EngineParamsArgs::defaults_for_tests();
        let w = words(&init_config_ix(7, &p)).expect("encodes");
        let back: Instruction = risc0_zkvm::serde::from_slice(&w).expect("decodes");
        assert_eq!(back.name(), "init_config");
        assert!(format!("{back:?}").contains("salt: 7"));
        let w = words(&set_collateral_config_ix(&CollateralArgs::defaults_for_tests())).expect("encodes");
        assert_eq!(variant_index(&w), Some(2));
        let w = words(&set_engine_params_ix(&p)).expect("encodes");
        assert_eq!(variant_index(&w), Some(1));
    }
}
```

Create `crates/auction_sdk/src/lib.rs`:

```rust
//! Client SDK of the RFP-014 auction engine (spec §6.1): own instruction encoders keyed by enum index,
//! PDAs and decoders, quotes on the program's own pricing path, plan builders, program-id
//! verification, nonce-based confirmation and block-scan discovery. Public transactions only.
#![forbid(unsafe_code)]

pub mod chain;
pub mod encode;
pub mod error;
pub mod plan;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use chain::{BlockSummary, Chain, TxSummary};
pub use error::{SdkError, SdkResult};
pub use plan::Plan;
```

- [ ] **Step 3: Run to verify the tests fail**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_sdk'`
Expected: FAIL to compile (`file not found for module chain`, then `cannot find function words`).

- [ ] **Step 4: Implement `error.rs`, `chain.rs`, `encode.rs`, `plan.rs`**

`crates/auction_sdk/src/error.rs`:

```rust
//! One error type for the SDK. Messages name accounts and programs, never key material.

use auction_engine_core::EngineError;

#[derive(Debug, thiserror::Error)]
pub enum SdkError {
    #[error("instruction encoding failed: {0}")]
    Encode(String),
    #[error("account {id} ({what}) is not initialised")]
    Missing { what: &'static str, id: String },
    #[error("account {id} is not a valid {what}: {reason}")]
    Decode {
        what: &'static str,
        id: String,
        reason: String,
    },
    #[error("{what}: expected {expected}, found {found}; refusing to build a transaction (I4, ADR-027)")]
    Mismatch {
        what: &'static str,
        expected: String,
        found: String,
    },
    #[error("the engine would reject this: {name} (code {code})")]
    Engine { name: String, code: u32 },
    #[error("deployment file {path}: {reason}")]
    Deployment { path: String, reason: String },
    #[error("chain: {0}")]
    Chain(String),
    #[error("signer {signer} already has a transaction in flight (lock {lock}); it resolves within {blocks} blocks")]
    InFlight {
        signer: String,
        lock: String,
        blocks: u64,
    },
    #[error("scanner: {0}")]
    Scan(String),
    #[error("{path}: {reason}")]
    Io { path: String, reason: String },
}

impl From<EngineError> for SdkError {
    fn from(e: EngineError) -> Self {
        Self::Engine {
            name: e.to_string(),
            code: e.code(),
        }
    }
}

pub type SdkResult<T> = Result<T, SdkError>;
```

`crates/auction_sdk/src/chain.rs`:

```rust
//! The SDK's only view of the network. Production: `rpc::RpcChain` (feature `rpc`, Task 12). Tests:
//! `testing::FixtureChain` and the integration harness's `WorldChain` (real guests). Synchronous on
//! purpose: the CLI and the Plan 4 FFI (`char* op(const char* json)`) are synchronous.

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee::PublicTransaction;
use lee_core::{
    account::{Account, AccountId, Nonce},
    program::ProgramId,
};

use crate::error::{SdkError, SdkResult};

/// One public transaction of a block, reduced to what discovery needs (spec §6.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxSummary {
    pub hash: [u8; 32],
    pub program_id: ProgramId,
    pub account_ids: Vec<AccountId>,
    pub instruction_words: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockSummary {
    pub id: u64,
    pub hash: [u8; 32],
    pub prev_hash: [u8; 32],
    pub timestamp_ms: u64,
    pub txs: Vec<TxSummary>,
}

pub trait Chain {
    /// The account at `id`; an uninitialised account is `Account::default()`, never an error.
    fn account(&self, id: &AccountId) -> SdkResult<Account>;
    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<Nonce>>;
    fn last_block_id(&self) -> SdkResult<u64>;
    /// Blocks `from..=to`. A block the node cannot return is an error, never silently skipped.
    fn blocks(&self, from: u64, to: u64) -> SdkResult<Vec<BlockSummary>>;
    /// Hands the transaction to the sequencer. Acceptance is not execution (spec §3.2): confirm with
    /// `confirm::confirm`.
    fn send(&self, tx: &PublicTransaction) -> SdkResult<[u8; 32]>;
}

/// Lets a caller lend a chain (`Env { chain: &fixture, .. }` in the CLI tests) and keep using it.
impl<T: Chain + ?Sized> Chain for &T {
    fn account(&self, id: &AccountId) -> SdkResult<Account> {
        (**self).account(id)
    }
    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<Nonce>> {
        (**self).nonces(ids)
    }
    fn last_block_id(&self) -> SdkResult<u64> {
        (**self).last_block_id()
    }
    fn blocks(&self, from: u64, to: u64) -> SdkResult<Vec<BlockSummary>> {
        (**self).blocks(from, to)
    }
    fn send(&self, tx: &PublicTransaction) -> SdkResult<[u8; 32]> {
        (**self).send(tx)
    }
}

/// `CLOCK_01` timestamp in ms: the previous block's timestamp (spec §3.2). Quotes default to it.
pub fn clock_ms<C: Chain + ?Sized>(chain: &C) -> SdkResult<u64> {
    let acc = chain.account(&CLOCK_01_PROGRAM_ACCOUNT_ID)?;
    if acc == Account::default() {
        return Err(SdkError::Missing {
            what: "CLOCK_01",
            id: CLOCK_01_PROGRAM_ACCOUNT_ID.to_string(),
        });
    }
    borsh::from_slice::<ClockAccountData>(acc.data.as_ref())
        .map(|c| c.timestamp)
        .map_err(|e| SdkError::Decode {
            what: "CLOCK_01",
            id: CLOCK_01_PROGRAM_ACCOUNT_ID.to_string(),
            reason: e.to_string(),
        })
}
```

`crates/auction_sdk/src/encode.rs` (above the test module written in Step 2):

```rust
//! Own instruction encoders (spec §6.1). Each program's instruction enum is encoded with risc0 serde
//! through `lee::program::Program::serialize_instruction`, exactly as `Message::try_new` does; the first
//! word is the ENUM index the guest decodes. Never an IDL position: spel-cli encodes the IDL position,
//! and the stablecoin IDL differs from its enum for positions 8..=18 (spec §3.2).

use auction_engine_core::{
    args::{CollateralArgs, EngineParamsArgs},
    Instruction,
};
use lee::program::Program;
use serde::Serialize;

use crate::error::{SdkError, SdkResult};

/// IDL position -> enum index of the vendored `idl/stablecoin-idl.json` (docs/architecture/programs.md).
pub const STABLECOIN_IDL_POSITION_TO_ENUM: [u32; 22] = [
    0, 1, 2, 3, 4, 5, 6, 7, 17, 18, 14, 8, 9, 11, 12, 13, 10, 15, 16, 19, 20, 21,
];

/// Instruction words of any program instruction (risc0 serde; first word = enum index).
pub fn words<T: Serialize>(ix: &T) -> SdkResult<Vec<u32>> {
    Program::serialize_instruction(ix).map_err(|e| SdkError::Encode(format!("{e:?}")))
}

#[must_use]
pub fn variant_index(words: &[u32]) -> Option<u32> {
    words.first().copied()
}

/// IDL name of a stablecoin instruction. Exhaustive: a new upstream variant does not compile until
/// it is named here (drift test `stablecoin_idl_position_maps_to_enum_index`).
#[must_use]
pub fn stablecoin_name(ix: &stablecoin_core::Instruction) -> &'static str {
    use stablecoin_core::Instruction as I;
    match ix {
        I::InitializeProgram { .. } => "initialize_program",
        I::AccrueStabilityFee => "accrue_stability_fee",
        I::UpdateRedemptionRate => "update_redemption_rate",
        I::RefreshGlobals => "refresh_globals",
        I::OpenPosition { .. } => "open_position",
        I::DepositCollateral { .. } => "deposit_collateral",
        I::GenerateDebt { .. } => "generate_debt",
        I::ClosePosition => "close_position",
        I::SetMinimumCollateralizationRatio { .. } => "set_minimum_collateralization_ratio",
        I::SetControllerGains { .. } => "set_controller_gains",
        I::SetMarketPriceOracle => "set_market_price_oracle",
        I::SetTimingParameters { .. } => "set_timing_parameters",
        I::SetAdmin { .. } => "set_admin",
        I::SetFreezeAuthority { .. } => "set_freeze_authority",
        I::SetStabilityFeePerMillisecond { .. } => "set_stability_fee_per_millisecond",
        I::Freeze => "freeze",
        I::Unfreeze => "unfreeze",
        I::WithdrawCollateral { .. } => "withdraw_collateral",
        I::RepayDebt { .. } => "repay_debt",
        I::SetLiquidationEngine { .. } => "set_liquidation_engine",
        I::LiquidatePosition { .. } => "liquidate_position",
        I::EngineFreezeNewDebt => "engine_freeze_new_debt",
    }
}

#[must_use]
pub fn init_config_ix(salt: u64, p: &EngineParamsArgs) -> Instruction {
    Instruction::InitConfig {
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

#[must_use]
pub fn set_engine_params_ix(p: &EngineParamsArgs) -> Instruction {
    Instruction::SetEngineParams {
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

#[must_use]
pub fn set_collateral_config_ix(c: &CollateralArgs) -> Instruction {
    Instruction::SetCollateralConfig {
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
```

`crates/auction_sdk/src/plan.rs` (builders are added in Task 7):

```rust
//! Plans (spec §6.1): `{program_id, account_ids, signing_requirements, instruction_words}`. A plan is
//! everything a signer needs; nothing is signed here.

use std::collections::BTreeSet;

use lee_core::{account::AccountId, program::ProgramId};
use serde::Serialize;

use crate::{
    encode::words,
    error::{SdkError, SdkResult},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub program_id: ProgramId,
    pub account_ids: Vec<AccountId>,
    /// Accounts that must sign, in witness order; the message nonces follow the same order.
    pub signing_requirements: Vec<AccountId>,
    pub instruction_words: Vec<u32>,
}

impl Plan {
    /// Rejects what LEZ would reject anyway: a duplicate account id, a signer that is not one of the
    /// accounts, or no signer at all (a public transaction needs one).
    pub fn new<T: Serialize>(
        program_id: ProgramId,
        account_ids: Vec<AccountId>,
        signing_requirements: Vec<AccountId>,
        ix: &T,
    ) -> SdkResult<Self> {
        let mut seen = BTreeSet::new();
        for a in &account_ids {
            if !seen.insert(*a) {
                return Err(SdkError::Encode(format!(
                    "account {a} is listed twice (LEZ rejects duplicate account ids)"
                )));
            }
        }
        if signing_requirements.is_empty() {
            return Err(SdkError::Encode("a public transaction needs a signer".into()));
        }
        for s in &signing_requirements {
            if !seen.contains(s) {
                return Err(SdkError::Encode(format!(
                    "signer {s} is not one of the plan's accounts"
                )));
            }
        }
        Ok(Self {
            program_id,
            account_ids,
            signing_requirements,
            instruction_words: words(ix)?,
        })
    }

    #[must_use]
    pub fn variant_index(&self) -> Option<u32> {
        self.instruction_words.first().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use auction_engine_core::Instruction;
    use lee_core::account::AccountId;

    const P: ProgramId = [7; 8];

    #[test]
    fn plan_rejects_duplicates_unlisted_signers_and_no_signer() {
        let (a, b) = (AccountId::new([1; 32]), AccountId::new([2; 32]));
        let ix = Instruction::ObserveFeed;
        assert!(matches!(
            Plan::new(P, vec![a, a], vec![a], &ix),
            Err(SdkError::Encode(m)) if m.contains("listed twice")
        ));
        assert!(matches!(
            Plan::new(P, vec![a], vec![b], &ix),
            Err(SdkError::Encode(m)) if m.contains("not one of")
        ));
        assert!(matches!(Plan::new(P, vec![a], vec![], &ix), Err(SdkError::Encode(_))));
        let ok = Plan::new(P, vec![a, b], vec![a], &ix).expect("valid");
        assert_eq!(ok.variant_index(), Some(3));
    }
}
```

- [ ] **Step 5: `FixtureChain`**

Create `crates/auction_sdk/src/testing/mod.rs`:

```rust
//! In-memory `Chain` for SDK and CLI tests (feature `testing`). Not a ledger: it never runs programs.
//! `send` records the transaction and, in `SendMode::Include`, bumps every signer's nonce and appends a
//! block holding it, which is all the confirmation and scanner logic observe.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee::{PrivateKey, PublicKey, PublicTransaction};
use lee_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use token_core::TokenHolding;

use crate::chain::{BlockSummary, Chain, TxSummary};
use crate::error::{SdkError, SdkResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendMode {
    /// Applied: signer nonces +1 and a new block with the transaction.
    Include,
    /// Accepted by `send`, never applied (spec §3.2 silent drop).
    Drop,
    /// `send` itself fails.
    Reject,
}

/// Test key `n`: `PrivateKey::try_new([n; 32])` (the integration harness uses the same scheme).
#[must_use]
pub fn key(n: u8) -> PrivateKey {
    PrivateKey::try_new([n; 32]).expect("valid test key")
}

#[must_use]
pub fn id(n: u8) -> AccountId {
    AccountId::from(&PublicKey::new_from_private_key(&key(n)))
}

fn block_hash(id: u64, generation: u8) -> [u8; 32] {
    let mut h = [0u8; 32];
    h[..8].copy_from_slice(&id.to_le_bytes());
    h[31] = generation;
    h
}

#[derive(Debug)]
pub struct FixtureChain {
    accounts: RefCell<BTreeMap<AccountId, Account>>,
    blocks: RefCell<Vec<BlockSummary>>,
    hidden: RefCell<BTreeSet<u64>>,
    height: Cell<u64>,
    height_step: Cell<u64>,
    mode: Cell<SendMode>,
    sent: RefCell<Vec<PublicTransaction>>,
}

impl Default for FixtureChain {
    fn default() -> Self {
        Self::new()
    }
}

impl FixtureChain {
    #[must_use]
    pub fn new() -> Self {
        Self {
            accounts: RefCell::new(BTreeMap::new()),
            blocks: RefCell::new(Vec::new()),
            hidden: RefCell::new(BTreeSet::new()),
            height: Cell::new(0),
            height_step: Cell::new(0),
            mode: Cell::new(SendMode::Include),
            sent: RefCell::new(Vec::new()),
        }
    }

    pub fn put(&self, id: AccountId, account: Account) {
        self.accounts.borrow_mut().insert(id, account);
    }

    pub fn put_state<T>(&self, id: AccountId, owner: ProgramId, state: &T)
    where
        Data: for<'a> From<&'a T>,
    {
        self.put(
            id,
            Account {
                program_owner: owner,
                data: Data::from(state),
                ..Account::default()
            },
        );
    }

    pub fn put_holding(&self, id: AccountId, token: ProgramId, definition: AccountId, balance: u128) {
        self.put_state(
            id,
            token,
            &TokenHolding::Fungible {
                definition_id: definition,
                balance,
            },
        );
    }

    pub fn set_clock(&self, ms: u64) {
        let data = ClockAccountData {
            block_id: u64::try_from(self.blocks.borrow().len()).expect("small"),
            timestamp: ms,
        }
        .to_bytes();
        self.put(
            CLOCK_01_PROGRAM_ACCOUNT_ID,
            Account {
                program_owner: [8; 8],
                data: Data::try_from(data).expect("clock data fits"),
                ..Account::default()
            },
        );
    }

    pub fn set_nonce(&self, id: AccountId, nonce: u128) {
        self.accounts.borrow_mut().entry(id).or_default().nonce = Nonce(nonce);
    }

    /// Appends a block (ids start at 1, like LEZ's genesis block id) and returns it.
    pub fn push_block(&self, txs: Vec<TxSummary>) -> BlockSummary {
        let mut blocks = self.blocks.borrow_mut();
        let id = u64::try_from(blocks.len()).expect("small") + 1;
        let prev_hash = blocks.last().map_or([0; 32], |b| b.hash);
        let b = BlockSummary {
            id,
            hash: block_hash(id, 0),
            prev_hash,
            timestamp_ms: 0,
            txs,
        };
        blocks.push(b.clone());
        b
    }

    /// Re-hashes block `id` and every later block (a node that rebuilt them, e.g. a restarted localnet).
    pub fn rewrite_from(&self, id: u64, generation: u8) {
        let mut blocks = self.blocks.borrow_mut();
        let start = usize::try_from(id).expect("small").saturating_sub(1);
        let mut prev = if start == 0 { [0; 32] } else { blocks[start - 1].hash };
        for b in blocks.iter_mut().skip(start) {
            b.prev_hash = prev;
            b.hash = block_hash(b.id, generation);
            prev = b.hash;
        }
    }

    /// The node answers "not found" for block `id`.
    pub fn hide_block(&self, id: u64) {
        self.hidden.borrow_mut().insert(id);
    }

    pub fn set_send_mode(&self, mode: SendMode) {
        self.mode.set(mode);
    }

    /// Every `last_block_id` call advances the height by `step` (simulates blocks during polling).
    pub fn set_height_step(&self, step: u64) {
        self.height_step.set(step);
    }

    #[must_use]
    pub fn sent(&self) -> Vec<PublicTransaction> {
        self.sent.borrow().clone()
    }
}

impl Chain for FixtureChain {
    fn account(&self, id: &AccountId) -> SdkResult<Account> {
        Ok(self.accounts.borrow().get(id).cloned().unwrap_or_default())
    }

    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<Nonce>> {
        let accounts = self.accounts.borrow();
        Ok(ids
            .iter()
            .map(|i| accounts.get(i).map_or(Nonce(0), |a| a.nonce))
            .collect())
    }

    fn last_block_id(&self) -> SdkResult<u64> {
        let next = self.height.get() + self.height_step.get();
        self.height.set(next);
        Ok(next.max(u64::try_from(self.blocks.borrow().len()).expect("small")))
    }

    fn blocks(&self, from: u64, to: u64) -> SdkResult<Vec<BlockSummary>> {
        let blocks = self.blocks.borrow();
        let hidden = self.hidden.borrow();
        (from..=to)
            .map(|i| {
                let found = if hidden.contains(&i) || i == 0 {
                    None
                } else {
                    blocks.get(usize::try_from(i - 1).expect("small")).cloned()
                };
                found.ok_or_else(|| SdkError::Chain(format!("block {i} not found")))
            })
            .collect()
    }

    fn send(&self, tx: &PublicTransaction) -> SdkResult<[u8; 32]> {
        self.sent.borrow_mut().push(tx.clone());
        let hash = tx.hash();
        match self.mode.get() {
            SendMode::Reject => Err(SdkError::Chain("sequencer rejected the transaction".into())),
            SendMode::Drop => Ok(hash),
            SendMode::Include => {
                for (_, pk) in tx.witness_set().signatures_and_public_keys() {
                    let mut accounts = self.accounts.borrow_mut();
                    let acc = accounts.entry(AccountId::from(pk)).or_default();
                    acc.nonce = Nonce(acc.nonce.0 + 1);
                }
                let m = tx.message();
                self.push_block(vec![TxSummary {
                    hash,
                    program_id: m.program_id,
                    account_ids: m.account_ids.clone(),
                    instruction_words: m.instruction_data.clone(),
                }]);
                Ok(hash)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_chain_blocks_clock_and_missing_blocks() {
        let c = FixtureChain::new();
        c.set_clock(42);
        assert_eq!(crate::chain::clock_ms(&c).expect("clock"), 42);
        let b1 = c.push_block(vec![]);
        let b2 = c.push_block(vec![]);
        assert_eq!((b1.id, b2.prev_hash), (1, b1.hash));
        assert_eq!(c.blocks(1, 2).expect("two blocks").len(), 2);
        c.hide_block(2);
        assert!(matches!(c.blocks(1, 2), Err(SdkError::Chain(m)) if m.contains("block 2")));
        c.rewrite_from(1, 9);
        assert_ne!(c.blocks(1, 1).expect("block 1")[0].hash, b1.hash);
        assert_eq!(c.last_block_id().expect("height"), 2);
    }
}
```

- [ ] **Step 6: Run the SDK tests**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_sdk --features testing'`
Expected: 6 passed (4 encode, 1 plan, 1 testing). The first run compiles the LEZ git crates for the SDK (minutes).

- [ ] **Step 7: Point the integration drift test at the SDK**

In `programs/integration_tests/tests/idl.rs` delete the local `fn stablecoin_name(...) { ... }` and add at the top `use auction_sdk::encode::stablecoin_name;`. Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p engine_integration_tests --test idl'` → 2 passed.

- [ ] **Step 8: Docs and ADR-040**

Create `docs/architecture/sdk.md`:

```markdown
# Client SDK (`crates/auction_sdk`)

The SDK is the only way the CLI (and, in Plan 4, the FFI) talks to the engine. It is synchronous and generic over one trait.

## Chain

`chain::Chain` = `account`, `nonces`, `last_block_id`, `blocks(from..=to)`, `send`. Implementations: `rpc::RpcChain` (sequencer JSON-RPC, feature `rpc`), `testing::FixtureChain` (in memory, feature `testing`), and `WorldChain` in `programs/integration_tests/tests/common/sdk.rs` (the real guests in an in-process `V03State`). An uninitialised account is `Account::default()`, never an error. `send` success means accepted, not executed (spec §3.2).

## Encoding

`encode::words(&ix)` is `lee::program::Program::serialize_instruction` (risc0 serde) of the program's own instruction enum, so the first word is the enum index the guest decodes. Never the IDL position: `idl/stablecoin-idl.json` differs from `stablecoin_core::Instruction` for positions 8..=18 (`STABLECOIN_IDL_POSITION_TO_ENUM`). Signed `i128` controller gains encode (spel cannot, ADR-018). Drift tests: `encode::tests` and `programs/integration_tests/tests/idl.rs`.

## Plans

`plan::Plan { program_id, account_ids, signing_requirements, instruction_words }`; `Plan::new` rejects duplicate accounts, unlisted signers and signer-less plans.

## Module map

| Module | Task | Purpose |
|---|---|---|
| `chain`, `encode`, `error`, `plan`, `testing` | 4 | trait, encoders, errors, plan type, fixtures |
```

Append to `docs/design-log.md`:

```markdown

---

## ADR-040: SDK Architecture: Synchronous `Chain` Trait, Own Enum-Index Encoders, Handler-Mirroring Quotes

**Context**
Spec §6.1 asks for an SDK with its own encoders (spel-cli encodes IDL positions and mis-encodes the stablecoin, spec §3.2), PDAs, decoders, quotes that call the same `auction_core` functions as the program, plan builders, discovery and confirmation. The CLI is synchronous and the Plan 4 FFI is a synchronous `char* op(const char* json)`.

**Options**
- (A) Async SDK over the jsonrpsee client.
- (B) Synchronous SDK over a `Chain` trait; the RPC implementation owns a current-thread tokio runtime.
- (C) Wrap spel-cli.

**Decision**
(B). Encoders are `lee::program::Program::serialize_instruction` of each program's own enum (the path `Message::try_new` uses), so the first word is the enum index by construction. Quotes call `auction_engine_program::feed::{redemption_price, observe, require_live}` and the `auction_core` functions in the handler's order (ADR-024 `observed()` simulation included) instead of re-implementing them; `programs/integration_tests` proves quote == on-chain result on the real guests (Task 8). Every transaction path starts with `verify::verify_config` against a deployment file (I4).

**Trade-offs**
The SDK depends on the pure handler crate `auction_engine_program` (no zkVM), which ties it to the program's source revision; that is the point (same code). A blocking runtime per `RpcChain` is fine for a CLI, not for a server.

**Status**
Accepted (Plan 3a, Task 4)
```

In `README.md`, section "Status", add above "**Plan 2 complete: on-chain programs.**":

```markdown
**Plan 3a in progress: engine hardening, SDK and CLI.** See `docs/superpowers/plans/2026-10-09-plan-3a-engine-hardening-sdk-cli.md` and [docs/architecture/sdk.md](docs/architecture/sdk.md).
```

- [ ] **Step 9: Gates**

Run the lint gate, and the format gate with `-p auction_sdk` added. Expected: clean.

- [ ] **Step 10: Commit (controller — GPG signing on Windows)**

```bash
git add Cargo.toml Cargo.lock crates/auction_sdk programs/integration_tests/Cargo.toml programs/integration_tests/tests/idl.rs docs/architecture/sdk.md docs/design-log.md README.md
git commit -m "feat(sdk): auction_sdk scaffold with own enum-index encoders and FixtureChain"
```

---

### Task 5: Addresses, decoders, deployment file and program-id verification (I4)

**Files:**
- Create: `crates/auction_sdk/src/{addresses.rs,decode.rs,deployment.rs,verify.rs}`, `crates/auction_sdk/src/testing/standard.rs`, `deployments/localnet.json`
- Modify: `crates/auction_sdk/src/lib.rs`, `crates/auction_sdk/src/testing/mod.rs`, `docs/architecture/sdk.md`

**Interfaces:**
- Consumes: `auction_engine_core::pda::*_pda`, `stablecoin_core::compute_*_pda`, state types, `Chain`.
- Produces:
  - `addresses::HostAddresses { program, protocol_parameters, stability_fee_accumulator, redemption_price_state, stablecoin_definition, stablecoin_master_holding, liquidation_engine_state }` with `new(program: ProgramId)`, `position(&self, owner: &AccountId, nonce: u64) -> AccountId`, `position_vault(&self, position: &AccountId) -> AccountId`.
  - `addresses::EngineAddresses { program, config, accounting, buffer, engine_authority, mint_authority, unused_ref_oracle, unused_protocol_token }` with `new(program, admin: &AccountId, salt: u64)`, `for_config(program, config: AccountId)`, `coll_config(&self, collateral_definition: &AccountId)`, `record(&self, position: &AccountId)`, `auction(&self, position: &AccountId, nonce: u64)`, `auction_vault(&self, auction: &AccountId)`, `surplus_auction(&self, nonce: u64)`, `surplus_vault(&self, nonce: u64)`, `debt_auction(&self, nonce: u64)`.
  - `decode::Reader<'a, C: Chain + ?Sized>`: `new(chain, engine: ProgramId)`, `config`, `coll_config`, `accounting`, `auction`, `surplus_auction`, `debt_auction` (each `(&self, id: &AccountId) -> SdkResult<State>`; checks initialised, owner = engine, tag), `record_count(&self, id) -> SdkResult<u64>` (0 when uninitialised), `host<T>(&self, id, host: &ProgramId, what: &'static str) -> SdkResult<T>`, `oracle(&self, id) -> SdkResult<OraclePriceAccount>`, `token_definition(&self, id) -> SdkResult<TokenDefinition>`, `balance(&self, id, token: &ProgramId, definition: &AccountId) -> SdkResult<Option<u128>>` (`None` = uninitialised), `meta(&self, id) -> SdkResult<AccountWithMetadata>`.
  - `deployment::{Deployment, program_id_hex, program_id_from_hex}`; `Deployment { network, sequencer_url, engine_program, host_program, token_program, twap_oracle_program: Option<ProgramId>, engine_admin: Option<AccountId>, config: Option<AccountId> }`, `Deployment::from_json(text, path) / load(path)`.
  - `verify::{VerifiedConfig, verify_config, verify_binding}`; `VerifiedConfig { config_id, state: EngineConfigState, engine: EngineAddresses, host: HostAddresses }` with `slot(&self, slot: u64) -> AccountId`, `coll_config_id(&self) -> AccountId`, `coll<C>(&self, chain) -> SdkResult<CollateralConfigState>`.
  - `testing::standard::{Fixture, standard, deployment, config_id, oracle_state, ENGINE, HOST, TOKEN, TWAP, T0, ONE, R, SALT, COLLATERAL, ORACLE, ORACLE_OWNER, PROTOCOL, k}` (the "standard world", numbers below).

The standard fixture (used by Tasks 5, 6, 7, 10, 11, 13–15): host `HOST = [0xA1; 8]`, engine `ENGINE = [0xE1; 8]`, token `TOKEN = [0xB1; 8]`, twap `TWAP = [0xC1; 8]`; clock `T0 = 1_700_000_000_000`; redemption price `R = 0.5 RAY` (rate 1.0), stability fee exactly 1.0 per ms, min CR 2.0; oracle `ORACLE` at `R`, `T0`; config `["CONFIG", id(ENGINE_ADMIN), 1]` with `EngineParamsArgs::defaults_for_tests()` except protocol token `PROTOCOL` (mint authority `["MINT_AUTH", config]`) and `global_reserved = 660_000`; collateral config = `CollateralArgs::defaults_for_tests()` with `type_reserved = 660_000`, `last_seen_oracle_ts = T0`; unsafe position `P0` = owner `OWNER`, nonce 0, collateral 500_000, normalized debt 600_000 (vault holds 500_000); liquidated position `P1` = owner `OWNER2`, nonce 0, record count 1; live auction `A` = `["AUCTION", config, P1, 0]`: seized 500_000, reward 25, lot 475_000, target 660_000, principal 600_000, start `T0 − 600_000`, default schedule (vault holds 475_000); accounting: unbacked 1_000, `debt_on_auction` 500, surplus and debt auctions live (nonce 0 each, `*_nonce = 1`); surplus auction lot 500, `ref_ray = 2 RAY`, start `T0 − 600_000` (vault 500); debt auction lot 500, start amount 250, start `T0 − 600_000`; buffer 2_000; holdings: bidder stable 1_000_000, bidder collateral 0, taker stable 1_000, taker protocol 10_000; LES binds `(ENGINE, ["ENGINE_AUTH", config])`; blocks 1–3 = `OpenPosition(P1)`, `OpenPosition(P0)`, `Liquidate(P1, 0)`.

- [ ] **Step 1: Write the failing verification and decoder tests**

Create `crates/auction_sdk/src/verify.rs` with only its test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::standard::{self, k, ENGINE, HOST};
    use crate::testing::id;

    fn what_of(e: SdkError) -> String {
        match e {
            SdkError::Mismatch { what, .. } => what.to_owned(),
            SdkError::Missing { what, .. } => format!("missing {what}"),
            other => panic!("unexpected {other}"),
        }
    }

    #[test]
    fn verify_config_accepts_the_standard_world() {
        let fx = standard::standard();
        let v = verify_config(&fx.chain, &fx.deployment, &standard::config_id()).expect("verifies");
        assert_eq!(v.engine.program, ENGINE);
        assert_eq!(v.host.program, HOST);
        verify_binding(&fx.chain, &v).expect("the host binds this config");
    }

    #[test]
    fn verify_config_rejects_every_mismatch() {
        let fx = standard::standard();
        let cfg = standard::config_id();
        let cases: Vec<(&str, Deployment)> = vec![
            ("engine program", Deployment { engine_program: [0xE2; 8], ..fx.deployment.clone() }),
            ("host program id", Deployment { host_program: [0xA2; 8], ..fx.deployment.clone() }),
            ("token program id", Deployment { token_program: [0xB2; 8], ..fx.deployment.clone() }),
            ("engine admin", Deployment { engine_admin: Some(id(k::KEEPER)), ..fx.deployment.clone() }),
        ];
        for (needle, d) in cases {
            let e = verify_config(&fx.chain, &d, &cfg).expect_err(needle);
            assert!(what_of(e).contains(needle), "{needle}");
        }
        // The config account copied to another address is not the PDA of its own admin and salt.
        let elsewhere = id(k::CALLER);
        fx.chain.put(elsewhere, fx.chain.account(&cfg).expect("config"));
        let e = verify_config(&fx.chain, &fx.deployment, &elsewhere).expect_err("address");
        assert!(what_of(e).contains("config address"));
        // Wrong network: the host's ProtocolParameters is owned by another program there.
        let pp = fx.chain.account(&fx.host.protocol_parameters).expect("pp");
        fx.chain.put(fx.host.protocol_parameters, Account { program_owner: [0xEE; 8], ..pp });
        let e = verify_config(&fx.chain, &fx.deployment, &cfg).expect_err("network");
        assert!(what_of(e).contains("ProtocolParameters"));
        let e = verify_config(&fx.chain, &fx.deployment, &id(99)).expect_err("missing");
        assert!(what_of(e).contains("missing engine config"));
    }

    #[test]
    fn verify_binding_rejects_a_host_bound_to_another_config() {
        let fx = standard::standard();
        let v = verify_config(&fx.chain, &fx.deployment, &standard::config_id()).expect("verifies");
        fx.chain.put_state(
            v.host.liquidation_engine_state,
            HOST,
            &stablecoin_core::LiquidationEngineState {
                engine_program_id: ENGINE,
                engine_authority_id: id(k::KEEPER),
            },
        );
        let e = verify_binding(&fx.chain, &v).expect_err("bound elsewhere");
        assert!(what_of(e).contains("engine authority"));
    }

    #[test]
    fn localnet_json_matches_deployments_md() {
        let d = Deployment::from_json(
            include_str!("../../../deployments/localnet.json"),
            "deployments/localnet.json",
        )
        .expect("parses");
        let md = include_str!("../../../DEPLOYMENTS.md");
        for (name, id) in [
            ("token", d.token_program),
            ("stablecoin (+ hooks)", d.host_program),
            ("auction_engine", d.engine_program),
        ] {
            let row = format!("| {name} | `{}` |", program_id_hex(&id));
            assert!(md.contains(&row), "DEPLOYMENTS.md has no row starting {row}");
        }
    }
}
```

Create `crates/auction_sdk/src/decode.rs` with only its tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::standard::{self, k, COLLATERAL, ENGINE, TOKEN};
    use crate::testing::id;

    #[test]
    fn reader_checks_initialisation_owner_tag_and_definition() {
        let fx = standard::standard();
        let r = Reader::new(&fx.chain, ENGINE);
        let cfg = r.config(&standard::config_id()).expect("config decodes");
        assert_eq!(cfg.salt, standard::SALT);
        assert!(matches!(r.config(&id(99)), Err(SdkError::Missing { .. })));
        // An accounting account read as a config: right owner, wrong tag.
        assert!(matches!(r.config(&fx.engine.accounting), Err(SdkError::Decode { .. })));
        // A host-owned account read as an engine account.
        assert!(matches!(r.config(&fx.host.protocol_parameters), Err(SdkError::Mismatch { .. })));
        assert_eq!(r.record_count(&fx.engine.record(&fx.unsafe_position)).expect("fresh"), 0);
        assert_eq!(r.record_count(&fx.engine.record(&fx.auction_position)).expect("one"), 1);
        assert_eq!(
            r.balance(&id(k::BIDDER_STABLE), &TOKEN, &fx.host.stablecoin_definition).expect("ok"),
            Some(1_000_000)
        );
        assert!(matches!(
            r.balance(&id(k::BIDDER_STABLE), &TOKEN, &COLLATERAL),
            Err(SdkError::Mismatch { .. })
        ));
        assert_eq!(r.balance(&id(98), &TOKEN, &COLLATERAL).expect("fresh"), None);
    }
}
```

Add to `crates/auction_sdk/src/lib.rs`: `pub mod addresses; pub mod decode; pub mod deployment; pub mod verify;` and in `testing/mod.rs` add `pub mod standard;`.

- [ ] **Step 2: Run to verify they fail**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_sdk --features testing'`
Expected: FAIL to compile (`file not found for module addresses`, then unresolved `verify_config`, `Reader`, `standard`).

- [ ] **Step 3: `addresses.rs`**

```rust
//! Every address a client needs, from the engine's and the host's own PDA functions (never
//! re-implemented: `auction_engine_core::pda`, `stablecoin_core::compute_*_pda`).

use auction_engine_core::pda::{self, SLOT_PROTOCOL_TOKEN, SLOT_REF_ORACLE};
use lee_core::{account::AccountId, program::ProgramId};
use stablecoin_core::{
    compute_liquidation_engine_state_pda, compute_position_pda, compute_position_vault_pda,
    compute_protocol_parameters_pda, compute_redemption_price_state_pda,
    compute_stability_fee_accumulator_pda, compute_stablecoin_definition_pda,
    compute_stablecoin_master_holding_pda,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostAddresses {
    pub program: ProgramId,
    pub protocol_parameters: AccountId,
    pub stability_fee_accumulator: AccountId,
    pub redemption_price_state: AccountId,
    pub stablecoin_definition: AccountId,
    pub stablecoin_master_holding: AccountId,
    pub liquidation_engine_state: AccountId,
}

impl HostAddresses {
    #[must_use]
    pub fn new(program: ProgramId) -> Self {
        Self {
            program,
            protocol_parameters: compute_protocol_parameters_pda(program),
            stability_fee_accumulator: compute_stability_fee_accumulator_pda(program),
            redemption_price_state: compute_redemption_price_state_pda(program),
            stablecoin_definition: compute_stablecoin_definition_pda(program),
            stablecoin_master_holding: compute_stablecoin_master_holding_pda(program),
            liquidation_engine_state: compute_liquidation_engine_state_pda(program),
        }
    }

    #[must_use]
    pub fn position(&self, owner: &AccountId, nonce: u64) -> AccountId {
        compute_position_pda(self.program, *owner, nonce)
    }

    #[must_use]
    pub fn position_vault(&self, position: &AccountId) -> AccountId {
        compute_position_vault_pda(self.program, *position)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineAddresses {
    pub program: ProgramId,
    pub config: AccountId,
    pub accounting: AccountId,
    pub buffer: AccountId,
    pub engine_authority: AccountId,
    pub mint_authority: AccountId,
    pub unused_ref_oracle: AccountId,
    pub unused_protocol_token: AccountId,
}

impl EngineAddresses {
    /// The config `["CONFIG", admin, salt]` (ADR-027) and everything seeded by it.
    #[must_use]
    pub fn new(program: ProgramId, admin: &AccountId, salt: u64) -> Self {
        Self::for_config(program, pda::config_pda(&program, admin, salt))
    }

    #[must_use]
    pub fn for_config(program: ProgramId, config: AccountId) -> Self {
        Self {
            program,
            config,
            accounting: pda::accounting_pda(&program, &config),
            buffer: pda::buffer_pda(&program, &config),
            engine_authority: pda::engine_authority_pda(&program, &config),
            mint_authority: pda::mint_authority_pda(&program, &config),
            unused_ref_oracle: pda::unused_slot_pda(&program, &config, SLOT_REF_ORACLE),
            unused_protocol_token: pda::unused_slot_pda(&program, &config, SLOT_PROTOCOL_TOKEN),
        }
    }

    #[must_use]
    pub fn coll_config(&self, collateral_definition: &AccountId) -> AccountId {
        pda::coll_config_pda(&self.program, &self.config, collateral_definition)
    }

    #[must_use]
    pub fn record(&self, position: &AccountId) -> AccountId {
        pda::liqrec_pda(&self.program, &self.config, position)
    }

    #[must_use]
    pub fn auction(&self, position: &AccountId, nonce: u64) -> AccountId {
        pda::auction_pda(&self.program, &self.config, position, nonce)
    }

    #[must_use]
    pub fn auction_vault(&self, auction: &AccountId) -> AccountId {
        pda::vault_pda(&self.program, auction)
    }

    #[must_use]
    pub fn surplus_auction(&self, nonce: u64) -> AccountId {
        pda::surplus_auction_pda(&self.program, &self.config, nonce)
    }

    #[must_use]
    pub fn surplus_vault(&self, nonce: u64) -> AccountId {
        pda::surplus_vault_pda(&self.program, &self.surplus_auction(nonce))
    }

    #[must_use]
    pub fn debt_auction(&self, nonce: u64) -> AccountId {
        pda::debt_auction_pda(&self.program, &self.config, nonce)
    }
}
```

- [ ] **Step 4: `decode.rs` (above its tests)**

```rust
//! Decoders for every account a client reads. Engine accounts: initialised, owned by the engine, right
//! 8-byte tag (`TryFrom<&Data>` checks it). Host accounts: owned by the host program.

use auction_engine_core::state::{
    AccountingState, AuctionState, CollateralConfigState, DebtAuctionState, EngineConfigState,
    LiquidationRecordState, SurplusAuctionState,
};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::ProgramId,
};
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::OraclePriceAccount;

use crate::{
    chain::Chain,
    deployment::program_id_hex,
    error::{SdkError, SdkResult},
};

pub struct Reader<'a, C: Chain + ?Sized> {
    chain: &'a C,
    engine: ProgramId,
}

fn decode_err(what: &'static str, id: &AccountId, e: &std::io::Error) -> SdkError {
    SdkError::Decode {
        what,
        id: id.to_string(),
        reason: e.to_string(),
    }
}

impl<'a, C: Chain + ?Sized> Reader<'a, C> {
    pub fn new(chain: &'a C, engine: ProgramId) -> Self {
        Self { chain, engine }
    }

    fn initialised(&self, id: &AccountId, what: &'static str) -> SdkResult<Account> {
        let acc = self.chain.account(id)?;
        if acc == Account::default() {
            return Err(SdkError::Missing {
                what,
                id: id.to_string(),
            });
        }
        Ok(acc)
    }

    fn owned<T>(&self, id: &AccountId, owner: &ProgramId, what: &'static str) -> SdkResult<T>
    where
        T: for<'d> TryFrom<&'d Data, Error = std::io::Error>,
    {
        let acc = self.initialised(id, what)?;
        if acc.program_owner != *owner {
            return Err(SdkError::Mismatch {
                what,
                expected: format!("owner {}", program_id_hex(owner)),
                found: format!("owner {}", program_id_hex(&acc.program_owner)),
            });
        }
        T::try_from(&acc.data).map_err(|e| decode_err(what, id, &e))
    }

    pub fn config(&self, id: &AccountId) -> SdkResult<EngineConfigState> {
        self.owned(id, &self.engine, "EngineConfig")
    }

    pub fn coll_config(&self, id: &AccountId) -> SdkResult<CollateralConfigState> {
        self.owned(id, &self.engine, "CollateralConfig")
    }

    pub fn accounting(&self, id: &AccountId) -> SdkResult<AccountingState> {
        self.owned(id, &self.engine, "Accounting")
    }

    pub fn auction(&self, id: &AccountId) -> SdkResult<AuctionState> {
        self.owned(id, &self.engine, "Auction")
    }

    pub fn surplus_auction(&self, id: &AccountId) -> SdkResult<SurplusAuctionState> {
        self.owned(id, &self.engine, "SurplusAuction")
    }

    pub fn debt_auction(&self, id: &AccountId) -> SdkResult<DebtAuctionState> {
        self.owned(id, &self.engine, "DebtAuction")
    }

    /// `LiquidationRecord.count` (the next liquidation nonce of a position); 0 before the first.
    pub fn record_count(&self, id: &AccountId) -> SdkResult<u64> {
        if self.chain.account(id)? == Account::default() {
            return Ok(0);
        }
        let r: LiquidationRecordState = self.owned(id, &self.engine, "LiquidationRecord")?;
        Ok(r.count)
    }

    /// A host-owned account (`Position`, `ProtocolParameters`, `StabilityFeeAccumulator`,
    /// `RedemptionPriceState`, `LiquidationEngineState`).
    pub fn host<T>(&self, id: &AccountId, host: &ProgramId, what: &'static str) -> SdkResult<T>
    where
        T: for<'d> TryFrom<&'d Data, Error = std::io::Error>,
    {
        self.owned(id, host, what)
    }

    /// The engine pins the oracle id; its owner is the oracle program, which the SDK does not assume.
    pub fn oracle(&self, id: &AccountId) -> SdkResult<OraclePriceAccount> {
        let acc = self.initialised(id, "OraclePriceAccount")?;
        OraclePriceAccount::try_from(&acc.data).map_err(|e| decode_err("OraclePriceAccount", id, &e))
    }

    pub fn token_definition(&self, id: &AccountId) -> SdkResult<TokenDefinition> {
        let acc = self.initialised(id, "TokenDefinition")?;
        TokenDefinition::try_from(&acc.data).map_err(|e| decode_err("TokenDefinition", id, &e))
    }

    /// Fungible balance; `None` for an uninitialised (fresh) holding.
    pub fn balance(
        &self,
        id: &AccountId,
        token: &ProgramId,
        definition: &AccountId,
    ) -> SdkResult<Option<u128>> {
        let acc = self.chain.account(id)?;
        if acc == Account::default() {
            return Ok(None);
        }
        if acc.program_owner != *token {
            return Err(SdkError::Mismatch {
                what: "holding owner (token program)",
                expected: program_id_hex(token),
                found: program_id_hex(&acc.program_owner),
            });
        }
        match TokenHolding::try_from(&acc.data) {
            Ok(TokenHolding::Fungible { definition_id, balance }) if definition_id == *definition => {
                Ok(Some(balance))
            }
            Ok(TokenHolding::Fungible { definition_id, .. }) => Err(SdkError::Mismatch {
                what: "holding definition",
                expected: definition.to_string(),
                found: definition_id.to_string(),
            }),
            Ok(_) => Err(SdkError::Decode {
                what: "fungible holding",
                id: id.to_string(),
                reason: "not a fungible holding".into(),
            }),
            Err(e) => Err(decode_err("fungible holding", id, &e)),
        }
    }

    /// The account as the handlers see it (read-only, unsigned), for `auction_engine_program::feed`.
    pub fn meta(&self, id: &AccountId) -> SdkResult<AccountWithMetadata> {
        Ok(AccountWithMetadata::new(self.chain.account(id)?, false, *id))
    }
}
```

- [ ] **Step 5: `deployment.rs`**

```rust
//! Machine-readable deployment (I4): the ImageIDs a client trusts. `deployments/<network>.json` must
//! agree with `DEPLOYMENTS.md` (test `localnet_json_matches_deployments_md`).

use std::path::Path;

use lee_core::{account::AccountId, program::ProgramId};
use serde::{Deserialize, Serialize};

use crate::error::{SdkError, SdkResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentFile {
    pub network: String,
    pub sequencer_url: String,
    pub engine_program_id: String,
    pub host_program_id: String,
    pub token_program_id: String,
    #[serde(default)]
    pub twap_oracle_program_id: Option<String>,
    #[serde(default)]
    pub engine_admin: Option<String>,
    #[serde(default)]
    pub config: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deployment {
    pub network: String,
    pub sequencer_url: String,
    pub engine_program: ProgramId,
    pub host_program: ProgramId,
    pub token_program: ProgramId,
    pub twap_oracle_program: Option<ProgramId>,
    /// When set, `verify_config` also requires this config admin (the expected operator, ADR-027).
    pub engine_admin: Option<AccountId>,
    /// Default config for commands that do not name one.
    pub config: Option<AccountId>,
}

/// ImageID as 64 hex characters (little-endian limbs, the `DEPLOYMENTS.md` / `spel program-id` form).
#[must_use]
pub fn program_id_hex(id: &ProgramId) -> String {
    id.iter()
        .flat_map(|l| l.to_le_bytes())
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn program_id_from_hex(s: &str) -> Result<ProgramId, String> {
    let bytes = hex::decode(s.trim()).map_err(|e| format!("not hex: {e}"))?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|b: Vec<u8>| format!("an ImageID is 32 bytes, got {}", b.len()))?;
    let mut id = [0u32; 8];
    for (limb, c) in id.iter_mut().zip(arr.chunks_exact(4)) {
        *limb = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
    }
    Ok(id)
}

impl Deployment {
    pub fn from_json(text: &str, path: &str) -> SdkResult<Self> {
        let bad = |reason: String| SdkError::Deployment {
            path: path.to_owned(),
            reason,
        };
        let f: DeploymentFile = serde_json::from_str(text).map_err(|e| bad(e.to_string()))?;
        let pid = |s: &str, what: &str| program_id_from_hex(s).map_err(|e| bad(format!("{what}: {e}")));
        let acct = |s: &str, what: &str| {
            s.parse::<AccountId>()
                .map_err(|_| bad(format!("{what} is not a base58 account id")))
        };
        Ok(Self {
            network: f.network.clone(),
            sequencer_url: f.sequencer_url.clone(),
            engine_program: pid(&f.engine_program_id, "engine_program_id")?,
            host_program: pid(&f.host_program_id, "host_program_id")?,
            token_program: pid(&f.token_program_id, "token_program_id")?,
            twap_oracle_program: f
                .twap_oracle_program_id
                .as_deref()
                .map(|s| pid(s, "twap_oracle_program_id"))
                .transpose()?,
            engine_admin: f
                .engine_admin
                .as_deref()
                .map(|s| acct(s, "engine_admin"))
                .transpose()?,
            config: f.config.as_deref().map(|s| acct(s, "config")).transpose()?,
        })
    }

    pub fn load(path: &Path) -> SdkResult<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| SdkError::Deployment {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;
        Self::from_json(&text, &path.display().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_id_hex_round_trips_and_rejects_bad_input() {
        let id: ProgramId = [1, 2, 3, 4, 5, 6, 7, 0xFFFF_FFFF];
        assert_eq!(program_id_from_hex(&program_id_hex(&id)).expect("round trip"), id);
        assert!(program_id_from_hex("zz").is_err());
        assert!(program_id_from_hex("abcd").is_err());
        let e = Deployment::from_json("{\"network\":1}", "x.json").expect_err("bad json");
        assert!(e.to_string().contains("x.json"));
    }
}
```

`deployments/localnet.json` (the current `DEPLOYMENTS.md` values; Task 16 refreshes them after the redeploy):

```json
{
  "network": "localnet",
  "sequencer_url": "http://127.0.0.1:3040",
  "engine_program_id": "3d0d15fa3c375179e1a4e9e02e828affb1924cd8dc559653cc5fe266ec23f477",
  "host_program_id": "91c790a23cadc2aa72be1533a4f4daa8ab6734e0dad5165fb9303cb438736953",
  "token_program_id": "14b3bc6cc129d0359f9595ba0f60a13e4a8df08ffe8eb1a83251c47abecc67a8",
  "twap_oracle_program_id": "5681e27c27b3ece4a7c161bbf8e12e6ece6ca69925a87b77f9e193fa4a249b31",
  "engine_admin": null,
  "config": null
}
```

- [ ] **Step 6: `verify.rs` (above its tests)**

```rust
//! Program-id verification (I4, ADR-027): no transaction is built for a config that this module does
//! not accept. The engine chains to the program ids stored in its config with the user's signer flags,
//! so a config built on a fake host or token program must be refused client-side.

use auction_engine_core::{
    pda::{self, SLOT_PROTOCOL_TOKEN},
    state::{CollateralConfigState, EngineConfigState},
};
use lee_core::account::{Account, AccountId};
use stablecoin_core::LiquidationEngineState;

use crate::{
    addresses::{EngineAddresses, HostAddresses},
    chain::Chain,
    decode::Reader,
    deployment::{program_id_hex, Deployment},
    error::{SdkError, SdkResult},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedConfig {
    pub config_id: AccountId,
    pub state: EngineConfigState,
    pub engine: EngineAddresses,
    pub host: HostAddresses,
}

fn mismatch(what: &'static str, expected: String, found: String) -> SdkError {
    SdkError::Mismatch {
        what,
        expected,
        found,
    }
}

pub fn verify_config<C: Chain + ?Sized>(
    chain: &C,
    d: &Deployment,
    config_id: &AccountId,
) -> SdkResult<VerifiedConfig> {
    let acc = chain.account(config_id)?;
    if acc == Account::default() {
        return Err(SdkError::Missing {
            what: "engine config",
            id: config_id.to_string(),
        });
    }
    if acc.program_owner != d.engine_program {
        return Err(mismatch(
            "engine program (owner of the config account)",
            program_id_hex(&d.engine_program),
            program_id_hex(&acc.program_owner),
        ));
    }
    let state = EngineConfigState::try_from(&acc.data).map_err(|e| SdkError::Decode {
        what: "EngineConfig",
        id: config_id.to_string(),
        reason: e.to_string(),
    })?;
    let expected = pda::config_pda(&d.engine_program, &state.admin, state.salt);
    if expected != *config_id {
        return Err(mismatch(
            "config address [CONFIG, admin, salt]",
            expected.to_string(),
            config_id.to_string(),
        ));
    }
    if state.host_program_id != d.host_program {
        return Err(mismatch(
            "host program id stored in the config",
            program_id_hex(&d.host_program),
            program_id_hex(&state.host_program_id),
        ));
    }
    if state.token_program_id != d.token_program {
        return Err(mismatch(
            "token program id stored in the config",
            program_id_hex(&d.token_program),
            program_id_hex(&state.token_program_id),
        ));
    }
    if let Some(admin) = d.engine_admin {
        if state.admin != admin {
            return Err(mismatch("engine admin", admin.to_string(), state.admin.to_string()));
        }
    }
    let host = HostAddresses::new(d.host_program);
    if state.host_protocol_parameters_id != host.protocol_parameters {
        return Err(mismatch(
            "host ProtocolParameters id stored in the config",
            host.protocol_parameters.to_string(),
            state.host_protocol_parameters_id.to_string(),
        ));
    }
    if state.stablecoin_definition_id != host.stablecoin_definition {
        return Err(mismatch(
            "stablecoin definition id stored in the config",
            host.stablecoin_definition.to_string(),
            state.stablecoin_definition_id.to_string(),
        ));
    }
    let pp = chain.account(&host.protocol_parameters)?;
    if pp.program_owner != d.host_program {
        return Err(mismatch(
            "owner of the host ProtocolParameters (wrong network?)",
            program_id_hex(&d.host_program),
            program_id_hex(&pp.program_owner),
        ));
    }
    Ok(VerifiedConfig {
        config_id: *config_id,
        engine: EngineAddresses::for_config(d.engine_program, *config_id),
        host,
        state,
    })
}

/// The host binds exactly this config's engine authority (checked before `liquidate`; a host bound to
/// another config would reject the hook anyway, but the user learns why before signing).
pub fn verify_binding<C: Chain + ?Sized>(chain: &C, v: &VerifiedConfig) -> SdkResult<()> {
    let les: LiquidationEngineState = Reader::new(chain, v.engine.program).host(
        &v.host.liquidation_engine_state,
        &v.host.program,
        "LiquidationEngineState",
    )?;
    if les.engine_program_id != v.engine.program {
        return Err(mismatch(
            "engine program bound by the host",
            program_id_hex(&v.engine.program),
            program_id_hex(&les.engine_program_id),
        ));
    }
    if les.engine_authority_id != v.engine.engine_authority {
        return Err(mismatch(
            "engine authority bound by the host (another config is bound)",
            v.engine.engine_authority.to_string(),
            les.engine_authority_id.to_string(),
        ));
    }
    Ok(())
}

impl VerifiedConfig {
    /// Slot account to pass (ADR-030): the configured id, or `["UNUSED", config, slot]`.
    #[must_use]
    pub fn slot(&self, slot: u64) -> AccountId {
        if slot == SLOT_PROTOCOL_TOKEN {
            self.state
                .protocol_token
                .unwrap_or(self.engine.unused_protocol_token)
        } else {
            self.state
                .protocol_token_oracle_id
                .unwrap_or(self.engine.unused_ref_oracle)
        }
    }

    #[must_use]
    pub fn coll_config_id(&self) -> AccountId {
        self.engine.coll_config(&self.state.collateral_definition_id)
    }

    pub fn coll<C: Chain + ?Sized>(&self, chain: &C) -> SdkResult<CollateralConfigState> {
        Reader::new(chain, self.engine.program).coll_config(&self.coll_config_id())
    }
}
```

- [ ] **Step 7: The standard fixture**

Create `crates/auction_sdk/src/testing/standard.rs`:

```rust
//! The "standard world" shared by SDK, CLI and golden tests (numbers in Plan 3a Task 5). Accounts are
//! inserted directly (no programs run); every value is consistent with the engine's own state rules.

use auction_core::units::RAY;
use auction_engine_core::{
    args::{CollateralArgs, EngineParamsArgs},
    state::*,
    Instruction,
};
use lee_core::{account::AccountId, program::ProgramId};
use stablecoin_core::{
    LiquidationEngineState, Position, ProtocolParameters, RedemptionPriceState,
    StabilityFeeAccumulator,
};
use token_core::TokenDefinition;
use twap_oracle_core::OraclePriceAccount;

use super::{id, FixtureChain};
use crate::{
    addresses::{EngineAddresses, HostAddresses},
    chain::TxSummary,
    deployment::Deployment,
    encode::words,
};

pub const ENGINE: ProgramId = [0xE1; 8];
pub const HOST: ProgramId = [0xA1; 8];
pub const TOKEN: ProgramId = [0xB1; 8];
pub const TWAP: ProgramId = [0xC1; 8];
pub const ORACLE_OWNER: ProgramId = [9; 8];
pub const T0: u64 = 1_700_000_000_000;
pub const ONE: u128 = RAY;
pub const R: u128 = RAY / 2;
pub const SALT: u64 = 1;
pub const COLLATERAL: AccountId = AccountId::new([0x30; 32]);
pub const ORACLE: AccountId = AccountId::new([0x50; 32]);
pub const PROTOCOL: AccountId = AccountId::new([0x91; 32]);

/// Key seeds (same values as the integration harness's `common::k`).
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
    pub const TAKER_STABLE: u8 = 57;
    pub const TAKER_PROTOCOL: u8 = 58;
    pub const CALLER: u8 = 62;
    pub const OWNER2: u8 = 65;
}

pub struct Fixture {
    pub chain: FixtureChain,
    pub deployment: Deployment,
    pub engine: EngineAddresses,
    pub host: HostAddresses,
    pub unsafe_position: AccountId,
    pub auction_position: AccountId,
    pub live_auction: AccountId,
}

#[must_use]
pub fn config_id() -> AccountId {
    EngineAddresses::new(ENGINE, &id(k::ENGINE_ADMIN), SALT).config
}

#[must_use]
pub fn deployment() -> Deployment {
    Deployment {
        network: "fixture".into(),
        sequencer_url: String::new(),
        engine_program: ENGINE,
        host_program: HOST,
        token_program: TOKEN,
        twap_oracle_program: Some(TWAP),
        engine_admin: Some(id(k::ENGINE_ADMIN)),
        config: Some(config_id()),
    }
}

#[must_use]
pub fn oracle_state(price: u128, timestamp: u64, host: &HostAddresses) -> OraclePriceAccount {
    OraclePriceAccount {
        base_asset: host.stablecoin_definition,
        quote_asset: COLLATERAL,
        price,
        timestamp,
        source_id: ORACLE,
        confidence_interval: 0,
    }
}

fn schedule_recs() -> Vec<SchedulePointRec> {
    vec![
        SchedulePointRec { elapsed_ms: 0, discount_bps: 200 },
        SchedulePointRec { elapsed_ms: 2_700_000, discount_bps: 1_000 },
    ]
}

fn put_host(c: &FixtureChain, host: &HostAddresses, engine: &EngineAddresses) {
    c.put_state(
        host.protocol_parameters,
        HOST,
        &ProtocolParameters {
            admin_account_id: id(k::HOST_ADMIN),
            freeze_authority_account_id: id(k::FREEZE),
            stablecoin_definition_id: host.stablecoin_definition,
            collateral_definition_id: COLLATERAL,
            market_price_oracle_id: ORACLE,
            stability_fee_per_millisecond: ONE,
            controller_proportional_gain: 0,
            controller_integral_gain: 0,
            minimum_collateralization_ratio: 2 * ONE,
            minimum_milliseconds_between_rate_updates: 300_000,
            maximum_oracle_price_age_milliseconds: 900_000,
            is_frozen: false,
        },
    );
    c.put_state(
        host.stability_fee_accumulator,
        HOST,
        &StabilityFeeAccumulator { accumulated_rate_at_last_accrual: ONE, last_accrued_at: T0 },
    );
    c.put_state(
        host.redemption_price_state,
        HOST,
        &RedemptionPriceState {
            redemption_price_at_last_update: R,
            redemption_rate_per_millisecond: ONE,
            controller_integral_term: 0,
            last_updated_at: T0,
        },
    );
    c.put_state(
        host.liquidation_engine_state,
        HOST,
        &LiquidationEngineState { engine_program_id: ENGINE, engine_authority_id: engine.engine_authority },
    );
    for (def, name, authority) in [
        (host.stablecoin_definition, "LSC", None),
        (COLLATERAL, "COLL", None),
        (PROTOCOL, "MOCK", Some(engine.mint_authority)),
    ] {
        c.put_state(
            def,
            TOKEN,
            &TokenDefinition::Fungible {
                name: name.into(),
                total_supply: 10_000_000,
                metadata_id: None,
                authority,
            },
        );
    }
    c.put_state(ORACLE, ORACLE_OWNER, &oracle_state(R, T0, host));
}

fn put_engine(c: &FixtureChain, host: &HostAddresses, e: &EngineAddresses) {
    let system = EngineParamsArgs::defaults_for_tests().validate().expect("valid defaults");
    c.put_state(
        e.config,
        ENGINE,
        &EngineConfigState {
            tag: TAG_CONFIG,
            salt: SALT,
            admin: id(k::ENGINE_ADMIN),
            host_program_id: HOST,
            host_protocol_parameters_id: host.protocol_parameters,
            stablecoin_definition_id: host.stablecoin_definition,
            collateral_definition_id: COLLATERAL,
            token_program_id: TOKEN,
            protocol_token: Some(PROTOCOL),
            protocol_token_ref_price: 2 * ONE,
            protocol_token_oracle_id: None,
            ref_oracle_max_age_ms: 3_600_000,
            global_cap: 10_000_000,
            global_reserved: 660_000,
            freeze_on_bad_debt: true,
            bad_debt_freeze_threshold: 1_000,
            system: SystemParamsRec::from_core(&system),
        },
    );
    let c_args = CollateralArgs::defaults_for_tests();
    c.put_state(
        e.coll_config(&COLLATERAL),
        ENGINE,
        &CollateralConfigState {
            tag: TAG_COLL,
            config: e.config,
            collateral_definition_id: COLLATERAL,
            oracle_id: ORACLE,
            oracle_scale: c_args.oracle_scale,
            max_oracle_age_ms: c_args.max_oracle_age_ms,
            max_conf_bps: 100,
            band_lo_bps: 9_000,
            band_hi_bps: 11_000,
            penalty_bps: 1_000,
            tip: c_args.tip,
            chip_bps: 10,
            reward_cap: c_args.reward_cap,
            debt_floor: c_args.debt_floor,
            min_bid: c_args.min_bid,
            type_cap: c_args.type_cap,
            type_reserved: 660_000,
            schedule: schedule_recs(),
            admin_paused: false,
            paused_since_ms: None,
            paused_total_ms: 0,
            last_seen_oracle_ts: Some(T0),
        },
    );
    c.put_state(
        e.accounting,
        ENGINE,
        &AccountingState {
            unbacked_debt: 1_000,
            debt_on_auction: 500,
            debt_live: true,
            debt_nonce: 1,
            surplus_live: true,
            surplus_nonce: 1,
            total_surplus_lots_out: 500,
            ..AccountingState::new()
        },
    );
    c.put_holding(e.buffer, TOKEN, host.stablecoin_definition, 2_000);
    c.put_state(
        e.surplus_auction(0),
        ENGINE,
        &SurplusAuctionState {
            tag: TAG_SURPLUS,
            config: e.config,
            nonce: 0,
            lot_left: 500,
            start_ms: T0 - 600_000,
            live: true,
            ref_ray: 2 * ONE,
        },
    );
    c.put_holding(e.surplus_vault(0), TOKEN, host.stablecoin_definition, 500);
    c.put_state(
        e.debt_auction(0),
        ENGINE,
        &DebtAuctionState {
            tag: TAG_DEBT,
            config: e.config,
            nonce: 0,
            lot_left: 500,
            start_amount: 250,
            start_ms: T0 - 600_000,
            live: true,
        },
    );
}

fn tx(program_id: ProgramId, account_ids: Vec<AccountId>, words: Vec<u32>, n: u8) -> TxSummary {
    TxSummary { hash: [n; 32], program_id, account_ids, instruction_words: words }
}

#[must_use]
pub fn standard() -> Fixture {
    let chain = FixtureChain::new();
    let host = HostAddresses::new(HOST);
    let engine = EngineAddresses::new(ENGINE, &id(k::ENGINE_ADMIN), SALT);
    chain.set_clock(T0);
    put_host(&chain, &host, &engine);
    put_engine(&chain, &host, &engine);

    let p0 = host.position(&id(k::OWNER), 0);
    chain.put_state(
        p0,
        HOST,
        &Position {
            owner_account_id: id(k::OWNER),
            position_nonce: 0,
            vault_account_id: host.position_vault(&p0),
            collateral_amount: 500_000,
            normalized_debt_amount: 600_000,
            opened_at: T0 - 1_000_000,
        },
    );
    chain.put_holding(host.position_vault(&p0), TOKEN, COLLATERAL, 500_000);

    let p1 = host.position(&id(k::OWNER2), 0);
    chain.put_state(
        p1,
        HOST,
        &Position {
            owner_account_id: id(k::OWNER2),
            position_nonce: 0,
            vault_account_id: host.position_vault(&p1),
            collateral_amount: 0,
            normalized_debt_amount: 0,
            opened_at: T0 - 2_000_000,
        },
    );
    chain.put_state(
        engine.record(&p1),
        ENGINE,
        &LiquidationRecordState { tag: TAG_LIQREC, position: p1, count: 1 },
    );
    let a = engine.auction(&p1, 0);
    chain.put_state(
        a,
        ENGINE,
        &AuctionState {
            tag: TAG_AUCTION,
            config: engine.config,
            position: p1,
            nonce: 0,
            owner: id(k::OWNER2),
            keeper: id(k::KEEPER),
            settled: false,
            collateral_seized: 500_000,
            collateral_sold: 0,
            collateral_left: 475_000,
            keeper_reward: 25,
            leftover_claimable: 0,
            leftover_claimed: 0,
            debt_principal: 600_000,
            amount_to_raise: 660_000,
            reserved_amount: 660_000,
            debt_raised: 0,
            penalty_raised: 0,
            start_ms: T0 - 600_000,
            schedule: schedule_recs(),
            type_paused_total_at_start: 0,
        },
    );
    chain.put_holding(engine.auction_vault(&a), TOKEN, COLLATERAL, 475_000);

    chain.put_holding(id(k::BIDDER_STABLE), TOKEN, host.stablecoin_definition, 1_000_000);
    chain.put_holding(id(k::BIDDER_COLL), TOKEN, COLLATERAL, 0);
    chain.put_holding(id(k::OWNER_COLL), TOKEN, COLLATERAL, 0);
    chain.put_holding(id(k::TAKER_STABLE), TOKEN, host.stablecoin_definition, 1_000);
    chain.put_holding(id(k::TAKER_PROTOCOL), TOKEN, PROTOCOL, 10_000);

    let open = |owner: u8, pos: AccountId| {
        let ix = stablecoin_core::Instruction::OpenPosition {
            position_nonce: 0,
            initial_collateral_amount: 500_000,
        };
        tx(
            HOST,
            vec![
                id(owner),
                pos,
                host.position_vault(&pos),
                AccountId::new([owner; 32]),
                COLLATERAL,
                host.protocol_parameters,
            ],
            words(&ix).expect("encodes"),
            owner,
        )
    };
    chain.push_block(vec![open(k::OWNER2, p1)]);
    chain.push_block(vec![open(k::OWNER, p0)]);
    let liq_accounts = vec![
        id(k::KEEPER),
        id(k::KEEPER_COLL),
        engine.config,
        engine.coll_config(&COLLATERAL),
        p1,
        engine.record(&p1),
        a,
        engine.auction_vault(&a),
    ];
    chain.push_block(vec![tx(
        ENGINE,
        liq_accounts,
        words(&Instruction::Liquidate { nonce: 0 }).expect("encodes"),
        3,
    )]);

    Fixture {
        chain,
        deployment: deployment(),
        engine,
        host,
        unsafe_position: p0,
        auction_position: p1,
        live_auction: a,
    }
}
```

(The block transactions are summaries for the scanner, not executable transactions; the liquidate summary lists only the leading accounts the scanner reads: index 2 = config, 4 = position, 6 = auction.)

- [ ] **Step 8: Run the tests**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_sdk --features testing'`
Expected: all pass; 6 new tests (4 in `verify::tests`, 1 in `decode::tests`, 1 in `deployment::tests`).

- [ ] **Step 9: Docs**

In `docs/architecture/sdk.md` append rows to the module map:

```markdown
| `addresses`, `decode` | 5 | PDAs from the engine's and host's own functions; decoders that check initialisation, owner and tag |
| `deployment`, `verify` | 5 | `deployments/<network>.json` (must match `DEPLOYMENTS.md`); `verify_config` refuses a config not owned by the engine ImageID, not at `["CONFIG", admin, salt]`, or storing another host/token program, admin or `ProtocolParameters`, or whose host `ProtocolParameters` has another owner (I4, ADR-027); `verify_binding` checks the host binds this config |
```

and a section:

```markdown
## Program-id verification (I4)

Every command that builds a transaction first calls `verify_config(chain, &deployment, &config)`. A mismatch is `SdkError::Mismatch { what, expected, found }` (CLI exit 2). The ImageIDs in `deployments/localnet.json` are native-build values until Plan 3b's reproducible Docker builds (THREAT_MODEL I4 residual).
```

- [ ] **Step 10: Gates and commit (controller — GPG signing on Windows)**

Run the lint gate and the format gate (with `-p auction_sdk`). Expected: clean.

```bash
git add crates/auction_sdk deployments/localnet.json docs/architecture/sdk.md
git commit -m "feat(sdk): addresses, decoders, deployment file and program-id verification"
```

---

### Task 6: Quotes on the program's own pricing path (ADR-024 `observed()` simulation)

**Files:**
- Create: `crates/auction_sdk/src/quote.rs`
- Modify: `crates/auction_sdk/Cargo.toml` (dependencies `auction_engine_program`, `stablecoin_program`), `crates/auction_sdk/src/lib.rs`, `docs/architecture/sdk.md`

**Interfaces:**
- Consumes: `auction_engine_program::feed::{redemption_price, observe, require_live, u512_to_u128}`, `auction_core::{auction::quote_bid, liquidation::quote_liquidation, price::bid_price, system::{take_surplus, take_debt}}`, `stablecoin_core::math::{compute_current_accumulated_rate_wide, compute_current_redemption_price_wide}`, `stablecoin_program::checks::is_position_collateralized`, `Reader`, `VerifiedConfig`, `testing::standard`.
- Produces (`auction_sdk::quote`):
  - `PositionHealth { position, owner, position_nonce, collateral, normalized_debt, nominal_debt, unsafe_at_min_cr }`; `position_health(chain, v, position: &AccountId, now_ms) -> SdkResult<PositionHealth>`.
  - `LiquidationQuoteView { health: PositionHealth, nonce: u64, auction: AccountId, now_ms, vault_balance, redemption_ray, market_ray, price_ray, quote: LiquidationQuote }`; `quote_liquidation(chain, v, position, now_ms)`.
  - `BidQuote { auction, now_ms, t_eff_ms, redemption_ray, price_ray, remaining_target, collateral_left, coin_in_max, fill: BidFill }` with `needs_settlement(&self, slippage_bps: u32) -> bool` (ADR-029); `quote_bid(chain, v, auction, coin_in_max, min_collateral_out, now_ms)`.
  - `AuctionView { id, state: AuctionState, t_eff_ms, discount_ray, held_at_max: bool, paused: Option<String> }`; `auction_view(chain, v, auction, now_ms)` (never fails on an invalid feed: it reports `paused`).
  - `SystemTakeQuote { nonce, now_ms, filled, tokens, lot_left_after }`; `quote_take_surplus(chain, v, nonce, amount_coin, now_ms)` (tokens the taker pays), `quote_take_debt(chain, v, nonce, amount_coin, now_ms)` (tokens minted).
  - `min_out(amount: u128, slippage_bps: u32) -> u128` (floor), `max_in(amount: u128, slippage_bps: u32) -> u128` (ceil), `earliest_debt_start(acc: &AccountingState) -> Option<u64>`.

Every quote mirrors its handler step for step: liquidation = `liquidate.rs` (redemption price → `observe` → `require_live` → position → vault balance → `D = ceil(normalized × rate / RAY)` → `bid_price` → `quote_liquidation`); bid = `bid.rs` (redemption price → `observe` → `require_live` → `bid_price` → `effective_elapsed` on the *observed* clock → `quote_bid`); system takes = `surplus.rs` / `debt.rs` (snapshotted `ref_ray` / `start_amount`, the config's `SystemParams`). Expected standard-world values: liquidation of `P0` → target 660_000, keeper reward 25, lot 475_000, nonce 0; bid of 100_000 on `A` at `T0` → `t_eff = 600_000`, discount `2% + 8% × 600_000 / 2_700_000` floored in RAY, collateral out 51_963; surplus take of 500 at `T0` → 200 protocol tokens (multiplier floored at 8_000 bps, ref 2 RAY); debt take of 500 → 325 minted (`floor(250 × 13_000 / 10_000)`).

- [ ] **Step 1: Dependencies**

In `crates/auction_sdk/Cargo.toml` `[dependencies]` add:

```toml
auction_engine_program = { path = "../../programs/auction_engine" }
stablecoin_program = { workspace = true }
```

and in `lib.rs` add `pub mod quote;`.

- [ ] **Step 2: Write the failing tests**

Create `crates/auction_sdk/src/quote.rs` with the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SdkError;
    use crate::testing::standard::{self, oracle_state, ORACLE, ORACLE_OWNER, R, T0};
    use crate::verify::verify_config;
    use auction_core::units::bps_to_ray;

    fn setup() -> (standard::Fixture, VerifiedConfig) {
        let fx = standard::standard();
        let v = verify_config(&fx.chain, &fx.deployment, &standard::config_id()).expect("verifies");
        (fx, v)
    }

    #[test]
    fn liquidation_quote_matches_the_hand_computed_standard_position() {
        let (fx, v) = setup();
        let q = quote_liquidation(&fx.chain, &v, &fx.unsafe_position, T0).expect("quote");
        assert!(q.health.unsafe_at_min_cr);
        assert_eq!(q.health.nominal_debt, 600_000);
        assert_eq!(
            (q.quote.amount_to_raise, q.quote.keeper_reward, q.quote.lot, q.quote.seized),
            (660_000, 25, 475_000, 500_000)
        );
        assert_eq!((q.nonce, q.price_ray), (0, R));
        assert_eq!(q.auction, v.engine.auction(&fx.unsafe_position, 0));
        let next = quote_liquidation(&fx.chain, &v, &fx.auction_position, T0)
            .expect_err("a zero-debt position is not liquidatable");
        assert!(matches!(next, SdkError::Engine { ref name, .. } if name == "ZeroDebt"));
    }

    #[test]
    fn bid_quote_uses_the_auction_schedule_at_t_eff() {
        let (fx, v) = setup();
        let q = quote_bid(&fx.chain, &v, &fx.live_auction, 100_000, 0, T0).expect("quote");
        assert_eq!((q.t_eff_ms, q.price_ray, q.remaining_target), (600_000, R, 660_000));
        let expected_d = bps_to_ray(200) + (bps_to_ray(1_000) - bps_to_ray(200)) * 600_000 / 2_700_000;
        assert_eq!(q.fill.discount_ray, expected_d);
        assert_eq!((q.fill.charged, q.fill.collateral_out), (100_000, 51_963));
        assert!(!q.fill.ends_auction);
        assert!(!q.needs_settlement(50), "a partial bid lists no Accounting");
        let all = quote_bid(&fx.chain, &v, &fx.live_auction, u128::MAX, 0, T0).expect("quote");
        assert!(all.fill.ends_auction && all.needs_settlement(50));
    }

    #[test]
    fn bid_quote_simulates_observed_before_t_eff() {
        // ADR-024: an unobserved stale gap (oracle last seen 5_000_000 ms ago, max age 3_600_000) is
        // folded into the pause clock first, which freezes the discount; a naive t_eff would be 600_000.
        let (fx, v) = setup();
        let mut coll = v.coll(&fx.chain).expect("coll");
        coll.last_seen_oracle_ts = Some(T0 - 5_000_000);
        fx.chain.put_state(v.coll_config_id(), standard::ENGINE, &coll);
        let q = quote_bid(&fx.chain, &v, &fx.live_auction, 100_000, 0, T0).expect("quote");
        assert_eq!(q.t_eff_ms, 0);
        assert_eq!(q.fill.discount_ray, bps_to_ray(200));
        assert_eq!(q.fill.collateral_out, 51_020);
    }

    #[test]
    fn a_stale_feed_is_the_engine_error_and_the_view_reports_the_pause() {
        let (fx, v) = setup();
        fx.chain.put_state(ORACLE, ORACLE_OWNER, &oracle_state(R, T0 - 3_600_001, &v.host));
        let e = quote_bid(&fx.chain, &v, &fx.live_auction, 100_000, 0, T0).expect_err("stale");
        assert!(matches!(e, SdkError::Engine { ref name, .. } if name == "PriceFeedStale"));
        let view = auction_view(&fx.chain, &v, &fx.live_auction, T0).expect("view never fails on a bad feed");
        assert_eq!(view.paused.as_deref(), Some("PriceFeedStale"));
    }

    #[test]
    fn auction_view_flags_an_auction_held_at_max_discount() {
        let (fx, v) = setup();
        let at_t0 = auction_view(&fx.chain, &v, &fx.live_auction, T0).expect("view");
        assert!(!at_t0.held_at_max && at_t0.paused.is_none());
        let later = T0 + 2_100_000; // t_eff = 2_700_000, the schedule's last point
        fx.chain.put_state(ORACLE, ORACLE_OWNER, &oracle_state(R, later, &v.host));
        let view = auction_view(&fx.chain, &v, &fx.live_auction, later).expect("view");
        assert!(view.held_at_max);
        assert_eq!(view.discount_ray, bps_to_ray(1_000));
    }

    #[test]
    fn system_take_quotes_use_the_snapshot_and_the_schedule() {
        let (fx, v) = setup();
        let s = quote_take_surplus(&fx.chain, &v, 0, 500, T0).expect("surplus quote");
        assert_eq!((s.filled, s.tokens, s.lot_left_after), (500, 200, 0));
        let d = quote_take_debt(&fx.chain, &v, 0, 500, T0).expect("debt quote");
        assert_eq!((d.filled, d.tokens, d.lot_left_after), (500, 325, 0));
        let e = quote_take_debt(&fx.chain, &v, 0, 495, T0).expect_err("dust remainder");
        assert!(matches!(e, SdkError::Engine { ref name, .. } if name == "DustRemainder"));
    }

    #[test]
    fn slippage_helpers_round_in_the_protocol_direction() {
        assert_eq!(min_out(10_000, 50), 9_950);
        assert_eq!(min_out(10_001, 50), 9_950);
        assert_eq!(max_in(200, 50), 201);
        assert_eq!(min_out(5, 20_000), 0);
        let acc = AccountingState { last_debt_end_ms: 100, ..AccountingState::new() };
        assert_eq!(earliest_debt_start(&acc), Some(600_100));
        assert_eq!(earliest_debt_start(&AccountingState::new()), None);
    }
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_sdk --features testing quote'`
Expected: FAIL to compile (`cannot find function quote_liquidation`, ...).

- [ ] **Step 4: Implement (above the tests)**

```rust
//! Quotes (spec §6.1): the program's own pricing path, evaluated on chain state at a chosen `now`.
//! Each quote calls the same `auction_engine_program::feed` and `auction_core` functions, in the same
//! order, as its handler (`liquidate.rs`, `bid.rs`, `surplus.rs`, `debt.rs`), and simulates
//! `observed()` before any `t_eff` (ADR-024). `programs/integration_tests/tests/sdk_quotes.rs` proves
//! quote == on-chain result on the real guests.

use auction_core::{
    auction::{quote_bid as core_quote_bid, BidFill, BidRequest},
    liquidation::{
        quote_liquidation as core_quote_liquidation, CapState, LiquidationInput, LiquidationQuote,
    },
    math::{mul_div_ceil, mul_div_floor},
    price::bid_price,
    system::{take_debt as core_take_debt, take_surplus as core_take_surplus},
    units::{BPS, RAY},
};
use auction_engine_core::{
    args::DEBT_RELIST_DELAY_MS,
    state::{AccountingState, AuctionState},
    EngineError,
};
use auction_engine_program::feed;
use lee_core::account::AccountId;
use stablecoin_core::{
    math::{compute_current_accumulated_rate_wide, compute_current_redemption_price_wide},
    Position, ProtocolParameters, RedemptionPriceState, StabilityFeeAccumulator,
};
use stablecoin_program::checks::is_position_collateralized;

use crate::{chain::Chain, decode::Reader, error::SdkResult, verify::VerifiedConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionHealth {
    pub position: AccountId,
    pub owner: AccountId,
    pub position_nonce: u64,
    pub collateral: u128,
    pub normalized_debt: u128,
    pub nominal_debt: u128,
    /// The host's own test (`is_position_collateralized` at `minimum_collateralization_ratio`, R at now).
    pub unsafe_at_min_cr: bool,
}

pub fn position_health<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    position: &AccountId,
    now_ms: u64,
) -> SdkResult<PositionHealth> {
    let r = Reader::new(chain, v.engine.program);
    let host = v.host.program;
    let pos: Position = r.host(position, &host, "Position")?;
    if *position != v.host.position(&pos.owner_account_id, pos.position_nonce) {
        return Err(EngineError::WrongAccount("position").into());
    }
    let pp: ProtocolParameters = r.host(&v.host.protocol_parameters, &host, "ProtocolParameters")?;
    let acc: StabilityFeeAccumulator =
        r.host(&v.host.stability_fee_accumulator, &host, "StabilityFeeAccumulator")?;
    let rps: RedemptionPriceState =
        r.host(&v.host.redemption_price_state, &host, "RedemptionPriceState")?;
    let rate = compute_current_accumulated_rate_wide(
        acc.accumulated_rate_at_last_accrual,
        pp.stability_fee_per_millisecond,
        acc.last_accrued_at,
        now_ms,
    );
    let redemption = compute_current_redemption_price_wide(
        rps.redemption_price_at_last_update,
        rps.redemption_rate_per_millisecond,
        rps.last_updated_at,
        now_ms,
    );
    // Nominal debt, owed -> rounded up (liquidate.rs).
    let nominal_debt = mul_div_ceil(pos.normalized_debt_amount, feed::u512_to_u128(rate)?, RAY)
        .map_err(EngineError::from)?;
    Ok(PositionHealth {
        position: *position,
        owner: pos.owner_account_id,
        position_nonce: pos.position_nonce,
        collateral: pos.collateral_amount,
        normalized_debt: pos.normalized_debt_amount,
        nominal_debt,
        unsafe_at_min_cr: !is_position_collateralized(
            &pos,
            rate,
            redemption,
            pp.minimum_collateralization_ratio,
        ),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiquidationQuoteView {
    pub health: PositionHealth,
    /// `LiquidationRecord.count`: the nonce `liquidate` must carry (ADR-004).
    pub nonce: u64,
    pub auction: AccountId,
    pub now_ms: u64,
    pub vault_balance: u128,
    pub redemption_ray: u128,
    pub market_ray: u128,
    pub price_ray: u128,
    pub quote: LiquidationQuote,
}

pub fn quote_liquidation<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    position: &AccountId,
    now_ms: u64,
) -> SdkResult<LiquidationQuoteView> {
    let r = Reader::new(chain, v.engine.program);
    let cfg = r.config(&v.config_id)?;
    let coll = r.coll_config(&v.coll_config_id())?;
    let pp: ProtocolParameters =
        r.host(&v.host.protocol_parameters, &v.host.program, "ProtocolParameters")?;
    if coll.oracle_id != pp.market_price_oracle_id {
        return Err(EngineError::OracleMismatch.into()); // ADR-028
    }
    let redemption = feed::redemption_price(&cfg, &r.meta(&v.host.redemption_price_state)?, now_ms)?;
    let obs = feed::observe(&coll, &r.meta(&coll.oracle_id)?, redemption, now_ms)?;
    let market = feed::require_live(&coll, &obs)?;
    let coll = coll.with_pause_clock(obs.clock);
    let health = position_health(chain, v, position, now_ms)?;
    let vault_balance = r
        .balance(
            &v.host.position_vault(position),
            &cfg.token_program_id,
            &cfg.collateral_definition_id,
        )?
        .unwrap_or(0);
    let price = bid_price(market, redemption, &coll.feed_params()?);
    let quote = core_quote_liquidation(
        &LiquidationInput {
            position_collateral: health.collateral,
            vault_balance,
            nominal_debt: health.nominal_debt,
            price_ray: price,
        },
        &coll.collateral_params()?,
        &CapState {
            type_cap: coll.type_cap,
            type_reserved: coll.type_reserved,
            global_cap: cfg.global_cap,
            global_reserved: cfg.global_reserved,
        },
    )
    .map_err(EngineError::from)?;
    let nonce = r.record_count(&v.engine.record(position))?;
    Ok(LiquidationQuoteView {
        health,
        nonce,
        auction: v.engine.auction(position, nonce),
        now_ms,
        vault_balance,
        redemption_ray: redemption,
        market_ray: market,
        price_ray: price,
        quote,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BidQuote {
    pub auction: AccountId,
    pub now_ms: u64,
    pub t_eff_ms: u64,
    pub redemption_ray: u128,
    pub price_ray: u128,
    pub remaining_target: u128,
    pub collateral_left: u128,
    pub coin_in_max: u128,
    pub fill: BidFill,
}

impl BidQuote {
    /// ADR-029: list `Accounting` (`bid_with_settlement`) when the fill ends the auction, when
    /// `coin_in_max` covers the remaining target, or when the fill is within the slippage margin of
    /// emptying the lot (a later price or discount could make it final in flight).
    #[must_use]
    pub fn needs_settlement(&self, slippage_bps: u32) -> bool {
        let margin = mul_div_ceil(
            self.fill.collateral_out,
            BPS + u128::from(slippage_bps),
            BPS,
        )
        .map_or(true, |m| m >= self.collateral_left);
        self.fill.ends_auction || self.coin_in_max >= self.remaining_target || margin
    }
}

fn check_auction(v: &VerifiedConfig, id: &AccountId, st: &AuctionState) -> SdkResult<()> {
    if st.config != v.config_id || *id != v.engine.auction(&st.position, st.nonce) {
        return Err(EngineError::WrongAccount("auction").into());
    }
    Ok(())
}

pub fn quote_bid<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    auction: &AccountId,
    coin_in_max: u128,
    min_collateral_out: u128,
    now_ms: u64,
) -> SdkResult<BidQuote> {
    let r = Reader::new(chain, v.engine.program);
    let cfg = r.config(&v.config_id)?;
    let coll = r.coll_config(&v.coll_config_id())?;
    let st = r.auction(auction)?;
    check_auction(v, auction, &st)?;
    let current = st.to_core()?;
    // Observation precedes every t_eff use (ADR-024), exactly as bid.rs.
    let redemption = feed::redemption_price(&cfg, &r.meta(&v.host.redemption_price_state)?, now_ms)?;
    let obs = feed::observe(&coll, &r.meta(&coll.oracle_id)?, redemption, now_ms)?;
    let market = feed::require_live(&coll, &obs)?;
    let coll = coll.with_pause_clock(obs.clock);
    let params = coll.collateral_params()?;
    let price = bid_price(market, redemption, &coll.feed_params()?);
    let t_eff = obs
        .clock
        .effective_elapsed(current.start_ms, current.type_paused_total_at_start, now_ms);
    let fill = core_quote_bid(
        &current,
        &BidRequest {
            coin_in_max,
            min_collateral_out,
        },
        price,
        t_eff,
        params.min_bid,
        params.chost().map_err(EngineError::from)?,
    )
    .map_err(EngineError::from)?;
    Ok(BidQuote {
        auction: *auction,
        now_ms,
        t_eff_ms: t_eff,
        redemption_ray: redemption,
        price_ray: price,
        remaining_target: current.remaining_target(),
        collateral_left: current.collateral_left,
        coin_in_max,
        fill,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuctionView {
    pub id: AccountId,
    pub state: AuctionState,
    pub t_eff_ms: u64,
    pub discount_ray: u128,
    /// Past the schedule's last point: unsold, held at the maximum discount, still holding its cap
    /// reservation (no expiry, ADR-012; THREAT_MODEL client obligation).
    pub held_at_max: bool,
    /// Why a bid would revert now (`Paused`, `PriceFeedStale`, ...), if it would.
    pub paused: Option<String>,
}

pub fn auction_view<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    auction: &AccountId,
    now_ms: u64,
) -> SdkResult<AuctionView> {
    let r = Reader::new(chain, v.engine.program);
    let cfg = r.config(&v.config_id)?;
    let coll = r.coll_config(&v.coll_config_id())?;
    let st = r.auction(auction)?;
    check_auction(v, auction, &st)?;
    let current = st.to_core()?;
    let redemption = feed::redemption_price(&cfg, &r.meta(&v.host.redemption_price_state)?, now_ms)?;
    let obs = feed::observe(&coll, &r.meta(&coll.oracle_id)?, redemption, now_ms)?;
    let paused = feed::require_live(&coll, &obs).err().map(|e| e.to_string());
    let t_eff = obs
        .clock
        .effective_elapsed(current.start_ms, current.type_paused_total_at_start, now_ms);
    let last_point = current.schedule.points().last().map_or(0, |p| p.elapsed_ms);
    Ok(AuctionView {
        id: *auction,
        discount_ray: current.schedule.discount_ray(t_eff),
        held_at_max: !st.settled && t_eff >= last_point,
        t_eff_ms: t_eff,
        paused,
        state: st,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemTakeQuote {
    pub nonce: u64,
    pub now_ms: u64,
    /// Coin actually filled (capped at the lot).
    pub filled: u128,
    /// Surplus: protocol tokens the taker pays (rounded up). Debt: protocol tokens minted (rounded down).
    pub tokens: u128,
    pub lot_left_after: u128,
}

pub fn quote_take_surplus<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    nonce: u64,
    amount_coin: u128,
    now_ms: u64,
) -> SdkResult<SystemTakeQuote> {
    let r = Reader::new(chain, v.engine.program);
    let cfg = r.config(&v.config_id)?;
    let sa = r.surplus_auction(&v.engine.surplus_auction(nonce))?;
    if sa.config != v.config_id {
        return Err(EngineError::WrongAccount("surplus_auction").into());
    }
    let (next, tokens) = core_take_surplus(
        &sa.core(),
        amount_coin,
        u128::MAX,
        sa.ref_ray,
        &cfg.system.to_core()?,
        now_ms,
    )
    .map_err(EngineError::from)?;
    Ok(SystemTakeQuote {
        nonce,
        now_ms,
        filled: sa.lot_left.checked_sub(next.lot_left).ok_or(EngineError::Math)?,
        tokens,
        lot_left_after: next.lot_left,
    })
}

pub fn quote_take_debt<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    nonce: u64,
    amount_coin: u128,
    now_ms: u64,
) -> SdkResult<SystemTakeQuote> {
    let r = Reader::new(chain, v.engine.program);
    let cfg = r.config(&v.config_id)?;
    let da = r.debt_auction(&v.engine.debt_auction(nonce))?;
    if da.config != v.config_id {
        return Err(EngineError::WrongAccount("debt_auction").into());
    }
    let (next, minted) = core_take_debt(&da.core(), amount_coin, 0, &cfg.system.to_core()?, now_ms)
        .map_err(EngineError::from)?;
    Ok(SystemTakeQuote {
        nonce,
        now_ms,
        filled: da.lot_left.checked_sub(next.lot_left).ok_or(EngineError::Math)?,
        tokens: minted,
        lot_left_after: next.lot_left,
    })
}

/// Minimum acceptable output after `slippage_bps` (rounded down).
#[must_use]
pub fn min_out(amount: u128, slippage_bps: u32) -> u128 {
    let keep = BPS.saturating_sub(u128::from(slippage_bps));
    mul_div_floor(amount, keep, BPS).unwrap_or(0)
}

/// Maximum acceptable input after `slippage_bps` (rounded up).
#[must_use]
pub fn max_in(amount: u128, slippage_bps: u32) -> u128 {
    mul_div_ceil(amount, BPS + u128::from(slippage_bps), BPS).unwrap_or(u128::MAX)
}

/// When the next debt auction may start (ADR-039), if one has ended before.
#[must_use]
pub fn earliest_debt_start(acc: &AccountingState) -> Option<u64> {
    (acc.last_debt_end_ms > 0).then(|| acc.last_debt_end_ms.saturating_add(DEBT_RELIST_DELAY_MS))
}
```

- [ ] **Step 5: Run the tests**

Run the Step 3 command. Expected: 7 passed.

- [ ] **Step 6: Docs**

`docs/architecture/sdk.md`: add the module-map row `| quote | 6 | liquidation, bid, surplus-take and debt-take quotes on the handler's own path; auction view (held at max discount, paused reason); slippage helpers; next debt start (ADR-039) |` and the section:

```markdown
## Quotes

A quote is the handler's pricing path run on fetched accounts at a chosen `now` (default: the chain clock, `chain::clock_ms`). It calls `auction_engine_program::feed::{redemption_price, observe, require_live}` and the `auction_core` functions in the handler's order, so `t_eff` always comes from the clock *after* simulating `observed()` (ADR-024): an unobserved stale gap freezes the discount in the quote exactly as it will on-chain. A quote is valid only at its `now`; the on-chain result at inclusion time can differ (discount moved, oracle moved), which is why every transaction command derives `min_collateral_out` / `max_protocol_in` / `min_protocol_out` from the quote with a slippage margin (Review Focus 1, `sdk_quotes.rs`).
```

- [ ] **Step 7: Gates and commit (controller — GPG signing on Windows)**

Run the lint and format gates. Expected: clean.

```bash
git add crates/auction_sdk Cargo.lock docs/architecture/sdk.md
git commit -m "feat(sdk): quotes on the program's own pricing path with observed() simulation"
```

---

### Task 7: Plan builders for every engine, host, token and oracle instruction the clients send

**Files:**
- Modify: `crates/auction_sdk/src/plan.rs` (engine builders)
- Create: `crates/auction_sdk/src/plan_host.rs` (stablecoin, token, twap_oracle builders)
- Modify: `crates/auction_sdk/src/lib.rs`, `docs/architecture/sdk.md`

**Interfaces:**
- Consumes: `Plan::new`, `VerifiedConfig::{slot, coll_config_id}`, `EngineAddresses`, `HostAddresses`, `encode::{init_config_ix, set_engine_params_ix, set_collateral_config_ix}`, `clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID`, `twap_oracle_core::compute_oracle_price_account_pda`.
- Produces (`auction_sdk::plan`):
  - `init_config(engine_program: ProgramId, host: &HostAddresses, admin: AccountId, salt: u64, p: &EngineParamsArgs, protocol_token: Option<AccountId>, ref_oracle: Option<AccountId>) -> SdkResult<Plan>`
  - `set_engine_params(v, admin, p, protocol_token: Option<AccountId>, ref_oracle: Option<AccountId>)`, `set_collateral_config(v, admin, oracle, c: &CollateralArgs)`, `observe_feed(v, caller, oracle)`
  - `LiquidateArgs { keeper, keeper_collateral_holding, keeper_holding_is_fresh: bool, position, nonce: u64, oracle }`, `liquidate(v, &LiquidateArgs)`
  - `BidArgs { bidder_stable, bidder_collateral, bidder_collateral_is_fresh: bool, auction, coin_in_max, min_collateral_out, with_settlement: bool, oracle }`, `bid(v, &BidArgs)`
  - `settle_empty(v, caller, auction)`, `claim_leftover(v, owner, owner_holding, owner_holding_is_fresh, auction)`, `settle_debt(v, caller, amount)`
  - `TakeArgs { payer, receiver, receiver_is_fresh: bool, nonce: u64, amount_coin: u128, limit: u128 }`; `start_surplus(v, caller, nonce)`, `take_surplus(v, &TakeArgs)` (payer = taker protocol holding, receiver = taker stable holding, limit = `max_protocol_in`), `close_surplus(v, caller, nonce)`, `start_debt(v, caller, nonce)`, `take_debt(v, &TakeArgs)` (payer = taker stable holding, receiver = taker protocol holding, limit = `min_protocol_out`), `close_debt(v, caller, nonce)`, `check_solvency(v, caller)`.
- Produces (`auction_sdk::plan_host`): `InitializeProgramArgs { admin, freeze_authority, collateral_definition, market_price_oracle, stability_fee_per_ms: u128, proportional_gain: i128, integral_gain: i128, min_collateralization_ratio: u128, min_ms_between_rate_updates: u64, max_oracle_age_ms: u64, redemption_price: u128, stablecoin_name: String }`; `initialize_program(host, &InitializeProgramArgs)`, `set_liquidation_engine(host, admin, engine_program, engine_authority)`, `open_position(host, owner, owner_collateral_holding, collateral_definition, nonce, amount)`, `generate_debt(host, owner, position, owner_stable_holding, market_price_oracle, amount)`, `set_minimum_collateralization_ratio(host, admin, ratio)`, `set_controller_gains(host, admin, proportional: i128, integral: i128)`, `token_new_fungible(token, definition, holding, name: &str, supply, mint_authority: Option<AccountId>)`, `token_initialize_account(token, definition, account)`, `token_transfer(token, from, to, amount)`, `create_oracle_price_account(twap, price_source, base, quote, initial_price, window_ms) -> SdkResult<(Plan, AccountId)>`.

Account lists are Shared Reference S1/S2 verbatim; Task 8 proves every builder against the real guests.

- [ ] **Step 1: Write the failing builder tests**

Append to the test module in `crates/auction_sdk/src/plan.rs`:

```rust
    use crate::testing::standard::{self, k, COLLATERAL, ORACLE, PROTOCOL};
    use crate::testing::id;
    use crate::verify::verify_config;
    use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID as CLOCK;

    fn v() -> (standard::Fixture, VerifiedConfig) {
        let fx = standard::standard();
        let v = verify_config(&fx.chain, &fx.deployment, &standard::config_id()).expect("verifies");
        (fx, v)
    }

    fn name_of(p: &Plan) -> &'static str {
        let ix: Instruction = risc0_zkvm::serde::from_slice(&p.instruction_words).expect("decodes");
        ix.name()
    }

    #[test]
    fn liquidate_lists_the_seventeen_accounts_in_order() {
        let (fx, v) = v();
        let pos = fx.unsafe_position;
        let p = liquidate(
            &v,
            &LiquidateArgs {
                keeper: id(k::KEEPER),
                keeper_collateral_holding: id(k::KEEPER_COLL),
                keeper_holding_is_fresh: true,
                position: pos,
                nonce: 0,
                oracle: ORACLE,
            },
        )
        .expect("plan");
        let auction = v.engine.auction(&pos, 0);
        assert_eq!(
            p.account_ids,
            vec![
                id(k::KEEPER),
                id(k::KEEPER_COLL),
                v.config_id,
                v.coll_config_id(),
                pos,
                v.engine.record(&pos),
                auction,
                v.engine.auction_vault(&auction),
                v.engine.engine_authority,
                v.host.liquidation_engine_state,
                v.host.position_vault(&pos),
                COLLATERAL,
                v.host.stability_fee_accumulator,
                v.host.redemption_price_state,
                ORACLE,
                v.host.protocol_parameters,
                CLOCK,
            ]
        );
        assert_eq!(p.signing_requirements, vec![id(k::KEEPER), id(k::KEEPER_COLL)]);
        assert_eq!(name_of(&p), "liquidate");
    }

    #[test]
    fn bid_switches_to_bid_with_settlement_and_appends_accounting() {
        let (fx, v) = v();
        let args = BidArgs {
            bidder_stable: id(k::BIDDER_STABLE),
            bidder_collateral: id(k::BIDDER_COLL),
            bidder_collateral_is_fresh: false,
            auction: fx.live_auction,
            coin_in_max: 100_000,
            min_collateral_out: 51_000,
            with_settlement: false,
            oracle: ORACLE,
        };
        let plain = bid(&v, &args).expect("bid");
        assert_eq!((name_of(&plain), plain.account_ids.len()), ("bid", 11));
        let settle = bid(&v, &BidArgs { with_settlement: true, ..args }).expect("bid_with_settlement");
        assert_eq!(name_of(&settle), "bid_with_settlement");
        assert_eq!(settle.account_ids.last(), Some(&v.engine.accounting));
        assert_eq!(settle.signing_requirements, vec![id(k::BIDDER_STABLE)]);
    }

    #[test]
    fn slots_follow_the_config_and_init_config_checks_the_flags() {
        let (fx, v) = v();
        let p = start_debt(&v, id(k::CALLER), 1).expect("plan");
        assert_eq!(p.account_ids[6], PROTOCOL, "configured protocol token");
        assert_eq!(p.account_ids[7], v.engine.unused_ref_oracle, "no reference oracle: placeholder");
        let params = auction_engine_core::args::EngineParamsArgs::defaults_for_tests();
        let e = init_config(standard::ENGINE, &fx.host, id(k::ENGINE_ADMIN), 1, &params, Some(PROTOCOL), None)
            .expect_err("flag says disabled, slot says enabled");
        assert!(matches!(e, SdkError::Encode(_)));
        let ok = init_config(standard::ENGINE, &fx.host, id(k::ENGINE_ADMIN), 1, &params, None, None)
            .expect("plan");
        assert_eq!(ok.account_ids[1], standard::config_id());
        assert_eq!(ok.account_ids[6], v.engine.unused_protocol_token);
    }

    #[test]
    fn every_engine_builder_encodes_its_own_variant() {
        let (fx, v) = v();
        let caller = id(k::CALLER);
        let take = TakeArgs {
            payer: id(k::TAKER_PROTOCOL),
            receiver: id(k::TAKER_STABLE),
            receiver_is_fresh: false,
            nonce: 0,
            amount_coin: 500,
            limit: 200,
        };
        let plans = vec![
            (observe_feed(&v, caller, ORACLE), "observe_feed"),
            (settle_empty(&v, caller, fx.live_auction), "settle_empty"),
            (claim_leftover(&v, id(k::OWNER2), id(k::OWNER_COLL), false, fx.live_auction), "claim_leftover"),
            (settle_debt(&v, caller, u128::MAX), "settle_debt"),
            (start_surplus(&v, caller, 1), "start_surplus_auction"),
            (take_surplus(&v, &take), "take_surplus"),
            (close_surplus(&v, caller, 0), "close_surplus_auction"),
            (take_debt(&v, &TakeArgs { payer: id(k::TAKER_STABLE), receiver: id(k::TAKER_PROTOCOL), ..take }), "take_debt"),
            (close_debt(&v, caller, 0), "close_debt_auction"),
            (check_solvency(&v, caller), "check_solvency"),
        ];
        for (plan, name) in plans {
            let plan = plan.expect(name);
            assert_eq!(name_of(&plan), name);
        }
    }
```

(`use` declarations may sit anywhere in the module; `VerifiedConfig`, `Instruction`, `SdkError` and the builders come in through `super::*` once Step 3 adds them.)

Create `crates/auction_sdk/src/plan_host.rs` with only its tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::variant_index;
    use crate::testing::standard::{self, k, COLLATERAL, ORACLE, TOKEN, TWAP};
    use crate::testing::id;

    #[test]
    fn host_builders_use_enum_indices_and_the_idl_account_lists() {
        let host = HostAddresses::new(standard::HOST);
        let p = set_controller_gains(&host, id(k::HOST_ADMIN), -1, -2).expect("plan");
        assert_eq!(variant_index(&p.instruction_words), Some(9), "enum 9, IDL position 12");
        assert_eq!(p.account_ids, vec![id(k::HOST_ADMIN), host.protocol_parameters]);
        let p = set_minimum_collateralization_ratio(&host, id(k::HOST_ADMIN), 2).expect("plan");
        assert_eq!(variant_index(&p.instruction_words), Some(8), "enum 8, IDL position 11");
        let p = open_position(&host, id(k::OWNER), id(k::OWNER_COLL), COLLATERAL, 0, 500_000).expect("plan");
        assert_eq!(p.signing_requirements, vec![id(k::OWNER), id(k::OWNER_COLL)]);
        assert_eq!(p.account_ids[1], host.position(&id(k::OWNER), 0));
        let p = generate_debt(&host, id(k::OWNER), host.position(&id(k::OWNER), 0), id(k::OWNER_STABLE), ORACLE, 1)
            .expect("plan");
        assert_eq!((variant_index(&p.instruction_words), p.account_ids.len()), (Some(6), 9));
        let p = set_liquidation_engine(&host, id(k::HOST_ADMIN), standard::ENGINE, id(k::KEEPER)).expect("plan");
        assert_eq!(variant_index(&p.instruction_words), Some(19));
    }

    #[test]
    fn token_and_oracle_builders() {
        let p = token_new_fungible(TOKEN, id(1), id(2), "MOCK", 10, Some(id(3))).expect("plan");
        assert_eq!((variant_index(&p.instruction_words), p.signing_requirements.len()), (Some(1), 2));
        let p = token_initialize_account(TOKEN, id(1), id(2)).expect("plan");
        assert_eq!((variant_index(&p.instruction_words), p.signing_requirements.clone()), (Some(3), vec![id(2)]));
        let p = token_transfer(TOKEN, id(2), id(4), 5).expect("plan");
        assert_eq!(variant_index(&p.instruction_words), Some(0));
        let (p, oracle) = create_oracle_price_account(TWAP, id(5), id(6), id(7), 1, 3_600_000).expect("plan");
        assert_eq!(p.account_ids[0], oracle);
        assert_eq!(oracle, twap_oracle_core::compute_oracle_price_account_pda(TWAP, id(5), 3_600_000));
        assert_eq!(variant_index(&p.instruction_words), Some(1));
    }
}
```

Add `pub mod plan_host;` to `lib.rs`.

- [ ] **Step 2: Run to verify they fail**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_sdk --features testing plan'`
Expected: FAIL to compile (`cannot find function liquidate`, `cannot find struct LiquidateArgs`, ...).

- [ ] **Step 3: Engine builders**

Insert into `crates/auction_sdk/src/plan.rs`, after `impl Plan { ... }` and before the tests (add the imports to the top of the file):

```rust
use auction_engine_core::{
    args::{CollateralArgs, EngineParamsArgs},
    pda::{SLOT_PROTOCOL_TOKEN, SLOT_REF_ORACLE},
    Instruction,
};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID as CLOCK;

use crate::{
    addresses::{EngineAddresses, HostAddresses},
    encode::{init_config_ix, set_collateral_config_ix, set_engine_params_ix},
    verify::VerifiedConfig,
};

fn signers(primary: AccountId, holding: AccountId, holding_is_fresh: bool) -> Vec<AccountId> {
    if holding_is_fresh {
        vec![primary, holding]
    } else {
        vec![primary]
    }
}

/// `init_config` (no config exists yet, so there is nothing to verify; the CLI checks the host and
/// token owners before calling this). The enabled flags must agree with the slot accounts.
pub fn init_config(
    engine_program: ProgramId,
    host: &HostAddresses,
    admin: AccountId,
    salt: u64,
    p: &EngineParamsArgs,
    protocol_token: Option<AccountId>,
    ref_oracle: Option<AccountId>,
) -> SdkResult<Plan> {
    if p.protocol_token_enabled != protocol_token.is_some() || p.ref_oracle_enabled != ref_oracle.is_some() {
        return Err(SdkError::Encode(
            "protocol_token_enabled / ref_oracle_enabled must match the slot accounts passed".into(),
        ));
    }
    let e = EngineAddresses::new(engine_program, &admin, salt);
    Plan::new(
        engine_program,
        vec![
            admin,
            e.config,
            e.accounting,
            e.buffer,
            host.protocol_parameters,
            host.stablecoin_definition,
            protocol_token.unwrap_or(e.unused_protocol_token),
            ref_oracle.unwrap_or(e.unused_ref_oracle),
        ],
        vec![admin],
        &init_config_ix(salt, p),
    )
}

/// The slots carry the NEW values (the handler checks them against the new enabled flags).
pub fn set_engine_params(
    v: &VerifiedConfig,
    admin: AccountId,
    p: &EngineParamsArgs,
    protocol_token: Option<AccountId>,
    ref_oracle: Option<AccountId>,
) -> SdkResult<Plan> {
    if p.protocol_token_enabled != protocol_token.is_some() || p.ref_oracle_enabled != ref_oracle.is_some() {
        return Err(SdkError::Encode(
            "protocol_token_enabled / ref_oracle_enabled must match the slot accounts passed".into(),
        ));
    }
    Plan::new(
        v.engine.program,
        vec![
            admin,
            v.config_id,
            v.engine.accounting,
            v.coll_config_id(),
            protocol_token.unwrap_or(v.engine.unused_protocol_token),
            ref_oracle.unwrap_or(v.engine.unused_ref_oracle),
        ],
        vec![admin],
        &set_engine_params_ix(p),
    )
}

pub fn set_collateral_config(
    v: &VerifiedConfig,
    admin: AccountId,
    oracle: AccountId,
    c: &CollateralArgs,
) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![
            admin,
            v.config_id,
            v.state.collateral_definition_id,
            v.coll_config_id(),
            oracle,
            v.host.protocol_parameters,
            v.host.redemption_price_state,
            CLOCK,
        ],
        vec![admin],
        &set_collateral_config_ix(c),
    )
}

pub fn observe_feed(v: &VerifiedConfig, caller: AccountId, oracle: AccountId) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![caller, v.config_id, v.coll_config_id(), v.host.redemption_price_state, oracle, CLOCK],
        vec![caller],
        &Instruction::ObserveFeed,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiquidateArgs {
    pub keeper: AccountId,
    pub keeper_collateral_holding: AccountId,
    /// A never-initialised reward holding must sign (`RecipientMustSign`).
    pub keeper_holding_is_fresh: bool,
    pub position: AccountId,
    /// `LiquidationRecord.count` (from `quote::quote_liquidation`).
    pub nonce: u64,
    /// The collateral config's oracle (= the host's market oracle, ADR-028).
    pub oracle: AccountId,
}

pub fn liquidate(v: &VerifiedConfig, a: &LiquidateArgs) -> SdkResult<Plan> {
    let auction = v.engine.auction(&a.position, a.nonce);
    Plan::new(
        v.engine.program,
        vec![
            a.keeper,
            a.keeper_collateral_holding,
            v.config_id,
            v.coll_config_id(),
            a.position,
            v.engine.record(&a.position),
            auction,
            v.engine.auction_vault(&auction),
            v.engine.engine_authority,
            v.host.liquidation_engine_state,
            v.host.position_vault(&a.position),
            v.state.collateral_definition_id,
            v.host.stability_fee_accumulator,
            v.host.redemption_price_state,
            a.oracle,
            v.host.protocol_parameters,
            CLOCK,
        ],
        signers(a.keeper, a.keeper_collateral_holding, a.keeper_holding_is_fresh),
        &Instruction::Liquidate { nonce: a.nonce },
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BidArgs {
    pub bidder_stable: AccountId,
    pub bidder_collateral: AccountId,
    pub bidder_collateral_is_fresh: bool,
    pub auction: AccountId,
    pub coin_in_max: u128,
    pub min_collateral_out: u128,
    /// ADR-029: `quote::BidQuote::needs_settlement`.
    pub with_settlement: bool,
    pub oracle: AccountId,
}

pub fn bid(v: &VerifiedConfig, a: &BidArgs) -> SdkResult<Plan> {
    let mut accounts = vec![
        a.bidder_stable,
        a.bidder_collateral,
        v.config_id,
        v.coll_config_id(),
        a.auction,
        v.engine.auction_vault(&a.auction),
        v.engine.buffer,
        v.state.stablecoin_definition_id,
        v.host.redemption_price_state,
        a.oracle,
        CLOCK,
    ];
    let ix = if a.with_settlement {
        accounts.push(v.engine.accounting);
        Instruction::BidWithSettlement {
            coin_in_max: a.coin_in_max,
            min_collateral_out: a.min_collateral_out,
        }
    } else {
        Instruction::Bid {
            coin_in_max: a.coin_in_max,
            min_collateral_out: a.min_collateral_out,
        }
    };
    Plan::new(
        v.engine.program,
        accounts,
        signers(a.bidder_stable, a.bidder_collateral, a.bidder_collateral_is_fresh),
        &ix,
    )
}

pub fn settle_empty(v: &VerifiedConfig, caller: AccountId, auction: AccountId) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![caller, v.config_id, v.coll_config_id(), v.engine.accounting, auction],
        vec![caller],
        &Instruction::SettleEmpty,
    )
}

pub fn claim_leftover(
    v: &VerifiedConfig,
    owner: AccountId,
    owner_holding: AccountId,
    owner_holding_is_fresh: bool,
    auction: AccountId,
) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![owner, owner_holding, v.config_id, auction, v.engine.auction_vault(&auction)],
        signers(owner, owner_holding, owner_holding_is_fresh),
        &Instruction::ClaimLeftover,
    )
}

pub fn settle_debt(v: &VerifiedConfig, caller: AccountId, amount: u128) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![caller, v.config_id, v.engine.accounting, v.engine.buffer, v.state.stablecoin_definition_id],
        vec![caller],
        &Instruction::SettleDebt { amount },
    )
}

pub fn start_surplus(v: &VerifiedConfig, caller: AccountId, nonce: u64) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![
            caller,
            v.config_id,
            v.engine.accounting,
            v.engine.buffer,
            v.state.stablecoin_definition_id,
            v.engine.surplus_auction(nonce),
            v.engine.surplus_vault(nonce),
            v.slot(SLOT_REF_ORACLE),
            CLOCK,
        ],
        vec![caller],
        &Instruction::StartSurplusAuction { nonce },
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TakeArgs {
    /// Signs and pays: protocol-token holding (surplus) or stablecoin holding (debt).
    pub payer: AccountId,
    /// Receives: stablecoin holding (surplus) or protocol-token holding (debt).
    pub receiver: AccountId,
    pub receiver_is_fresh: bool,
    pub nonce: u64,
    pub amount_coin: u128,
    /// `max_protocol_in` (surplus) or `min_protocol_out` (debt).
    pub limit: u128,
}

pub fn take_surplus(v: &VerifiedConfig, a: &TakeArgs) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![
            a.payer,
            a.receiver,
            v.config_id,
            v.engine.accounting,
            v.engine.surplus_auction(a.nonce),
            v.engine.surplus_vault(a.nonce),
            v.slot(SLOT_PROTOCOL_TOKEN),
            CLOCK,
        ],
        signers(a.payer, a.receiver, a.receiver_is_fresh),
        &Instruction::TakeSurplus {
            amount_coin: a.amount_coin,
            max_protocol_in: a.limit,
        },
    )
}

pub fn close_surplus(v: &VerifiedConfig, caller: AccountId, nonce: u64) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![
            caller,
            v.config_id,
            v.engine.accounting,
            v.engine.surplus_auction(nonce),
            v.engine.surplus_vault(nonce),
            v.engine.buffer,
            CLOCK,
        ],
        vec![caller],
        &Instruction::CloseSurplusAuction,
    )
}

pub fn start_debt(v: &VerifiedConfig, caller: AccountId, nonce: u64) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![
            caller,
            v.config_id,
            v.engine.accounting,
            v.engine.buffer,
            v.state.stablecoin_definition_id,
            v.engine.debt_auction(nonce),
            v.slot(SLOT_PROTOCOL_TOKEN),
            v.slot(SLOT_REF_ORACLE),
            CLOCK,
        ],
        vec![caller],
        &Instruction::StartDebtAuction { nonce },
    )
}

pub fn take_debt(v: &VerifiedConfig, a: &TakeArgs) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![
            a.payer,
            a.receiver,
            v.config_id,
            v.engine.accounting,
            v.engine.debt_auction(a.nonce),
            v.state.stablecoin_definition_id,
            v.slot(SLOT_PROTOCOL_TOKEN),
            v.engine.mint_authority,
            CLOCK,
        ],
        signers(a.payer, a.receiver, a.receiver_is_fresh),
        &Instruction::TakeDebt {
            amount_coin: a.amount_coin,
            min_protocol_out: a.limit,
        },
    )
}

pub fn close_debt(v: &VerifiedConfig, caller: AccountId, nonce: u64) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![caller, v.config_id, v.engine.accounting, v.engine.debt_auction(nonce), CLOCK],
        vec![caller],
        &Instruction::CloseDebtAuction,
    )
}

pub fn check_solvency(v: &VerifiedConfig, caller: AccountId) -> SdkResult<Plan> {
    Plan::new(
        v.engine.program,
        vec![
            caller,
            v.config_id,
            v.engine.accounting,
            v.engine.buffer,
            v.state.stablecoin_definition_id,
            v.slot(SLOT_PROTOCOL_TOKEN),
            v.slot(SLOT_REF_ORACLE),
            v.engine.engine_authority,
            v.host.liquidation_engine_state,
            v.host.protocol_parameters,
            CLOCK,
        ],
        vec![caller],
        &Instruction::CheckSolvency,
    )
}
```

The `#[allow(clippy::too_many_arguments)]` attribute is not needed: `init_config` has 7 parameters, clippy's limit.

- [ ] **Step 4: Host, token and oracle builders**

`crates/auction_sdk/src/plan_host.rs` (above its tests):

```rust
//! Plans for the vendored stablecoin, token and twap_oracle programs (Shared Reference S2). Encoded by
//! ENUM index through `encode::words` (never the IDL position, spec §3.2); `i128` gains encode.

use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID as CLOCK;
use lee_core::{account::AccountId, program::ProgramId};

use crate::{addresses::HostAddresses, error::SdkResult, plan::Plan};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitializeProgramArgs {
    pub admin: AccountId,
    pub freeze_authority: AccountId,
    pub collateral_definition: AccountId,
    pub market_price_oracle: AccountId,
    pub stability_fee_per_ms: u128,
    pub proportional_gain: i128,
    pub integral_gain: i128,
    pub min_collateralization_ratio: u128,
    pub min_ms_between_rate_updates: u64,
    pub max_oracle_age_ms: u64,
    pub redemption_price: u128,
    pub stablecoin_name: String,
}

pub fn initialize_program(host: &HostAddresses, a: &InitializeProgramArgs) -> SdkResult<Plan> {
    Plan::new(
        host.program,
        vec![
            a.admin,
            host.protocol_parameters,
            host.stability_fee_accumulator,
            host.redemption_price_state,
            host.stablecoin_definition,
            host.stablecoin_master_holding,
            a.collateral_definition,
            a.market_price_oracle,
            CLOCK,
        ],
        vec![a.admin],
        &stablecoin_core::Instruction::InitializeProgram {
            freeze_authority_account_id: a.freeze_authority,
            initial_stability_fee_per_millisecond: a.stability_fee_per_ms,
            initial_controller_proportional_gain: a.proportional_gain,
            initial_controller_integral_gain: a.integral_gain,
            initial_minimum_collateralization_ratio: a.min_collateralization_ratio,
            minimum_milliseconds_between_rate_updates: a.min_ms_between_rate_updates,
            maximum_oracle_price_age_milliseconds: a.max_oracle_age_ms,
            initial_redemption_price: a.redemption_price,
            stablecoin_name: a.stablecoin_name.clone(),
        },
    )
}

pub fn set_liquidation_engine(
    host: &HostAddresses,
    admin: AccountId,
    engine_program: ProgramId,
    engine_authority: AccountId,
) -> SdkResult<Plan> {
    Plan::new(
        host.program,
        vec![admin, host.protocol_parameters, host.liquidation_engine_state],
        vec![admin],
        &stablecoin_core::Instruction::SetLiquidationEngine {
            engine_program_id: engine_program,
            engine_authority_id: engine_authority,
        },
    )
}

pub fn open_position(
    host: &HostAddresses,
    owner: AccountId,
    owner_collateral_holding: AccountId,
    collateral_definition: AccountId,
    nonce: u64,
    amount: u128,
) -> SdkResult<Plan> {
    let position = host.position(&owner, nonce);
    Plan::new(
        host.program,
        vec![
            owner,
            position,
            host.position_vault(&position),
            owner_collateral_holding,
            collateral_definition,
            host.protocol_parameters,
            CLOCK,
        ],
        vec![owner, owner_collateral_holding],
        &stablecoin_core::Instruction::OpenPosition {
            position_nonce: nonce,
            initial_collateral_amount: amount,
        },
    )
}

pub fn generate_debt(
    host: &HostAddresses,
    owner: AccountId,
    position: AccountId,
    owner_stable_holding: AccountId,
    market_price_oracle: AccountId,
    amount: u128,
) -> SdkResult<Plan> {
    Plan::new(
        host.program,
        vec![
            owner,
            position,
            host.stablecoin_definition,
            owner_stable_holding,
            host.stability_fee_accumulator,
            host.redemption_price_state,
            market_price_oracle,
            host.protocol_parameters,
            CLOCK,
        ],
        vec![owner],
        &stablecoin_core::Instruction::GenerateDebt { amount },
    )
}

pub fn set_minimum_collateralization_ratio(
    host: &HostAddresses,
    admin: AccountId,
    ratio: u128,
) -> SdkResult<Plan> {
    Plan::new(
        host.program,
        vec![admin, host.protocol_parameters],
        vec![admin],
        &stablecoin_core::Instruction::SetMinimumCollateralizationRatio { new_ratio: ratio },
    )
}

/// Signed gains (spel at `1ef0500f` cannot encode `i128`, ADR-018).
pub fn set_controller_gains(
    host: &HostAddresses,
    admin: AccountId,
    proportional: i128,
    integral: i128,
) -> SdkResult<Plan> {
    Plan::new(
        host.program,
        vec![admin, host.protocol_parameters],
        vec![admin],
        &stablecoin_core::Instruction::SetControllerGains {
            new_proportional_gain: proportional,
            new_integral_gain: integral,
        },
    )
}

pub fn token_new_fungible(
    token: ProgramId,
    definition: AccountId,
    holding: AccountId,
    name: &str,
    supply: u128,
    mint_authority: Option<AccountId>,
) -> SdkResult<Plan> {
    Plan::new(
        token,
        vec![definition, holding],
        vec![definition, holding],
        &token_core::Instruction::NewFungibleDefinition {
            name: name.to_owned(),
            total_supply: supply,
            mint_authority,
        },
    )
}

pub fn token_initialize_account(token: ProgramId, definition: AccountId, account: AccountId) -> SdkResult<Plan> {
    Plan::new(
        token,
        vec![definition, account],
        vec![account],
        &token_core::Instruction::InitializeAccount,
    )
}

pub fn token_transfer(token: ProgramId, from: AccountId, to: AccountId, amount: u128) -> SdkResult<Plan> {
    Plan::new(
        token,
        vec![from, to],
        vec![from],
        &token_core::Instruction::Transfer { amount_to_transfer: amount },
    )
}

/// Returns the plan and the oracle account it creates (`compute_oracle_price_account_pda`).
pub fn create_oracle_price_account(
    twap: ProgramId,
    price_source: AccountId,
    base: AccountId,
    quote: AccountId,
    initial_price: u128,
    window_ms: u64,
) -> SdkResult<(Plan, AccountId)> {
    let oracle = twap_oracle_core::compute_oracle_price_account_pda(twap, price_source, window_ms);
    let plan = Plan::new(
        twap,
        vec![oracle, price_source, CLOCK],
        vec![price_source],
        &twap_oracle_core::Instruction::CreateOraclePriceAccount {
            base_asset: base,
            quote_asset: quote,
            initial_price,
            window_duration: window_ms,
        },
    )?;
    Ok((plan, oracle))
}
```

If rustc reports that `token_core::Instruction::NewFungibleDefinition` has a different field set than `{ name, total_supply, mint_authority }`, or that `twap_oracle_core::Instruction::CreateOraclePriceAccount` differs from `{ base_asset, quote_asset, initial_price, window_duration }`, stop and report: the vendored enums are the authority (they were read for this plan from `programs/token/core/src/lib.rs` and `programs/twap_oracle/core/src/lib.rs`).

- [ ] **Step 5: Run the tests**

Run the Step 2 command. Expected: all `plan` and `plan_host` tests pass (4 + 1 previous in `plan::tests`, 2 in `plan_host::tests`).

- [ ] **Step 6: Docs, gates, commit (controller — GPG signing on Windows)**

`docs/architecture/sdk.md`: module-map rows `| plan (builders) | 7 | one builder per engine instruction (Shared Reference S1 lists); fresh recipient holdings added as signers; slots from the config (ADR-030); bid vs bid_with_settlement chosen by the caller from \`BidQuote::needs_settlement\` (ADR-029) |` and `| plan_host | 7 | stablecoin (incl. signed i128 gains), token and twap_oracle builders used by the CLI's admin/host/dev commands |`.

Run the lint and format gates. Expected: clean.

```bash
git add crates/auction_sdk docs/architecture/sdk.md
git commit -m "feat(sdk): plan builders for engine, host, token and oracle instructions"
```

---

### Task 8: Prove the SDK on the real guests: every plan runs, quotes equal on-chain results

**Files:**
- Create: `programs/integration_tests/tests/common/sdk.rs`, `programs/integration_tests/tests/sdk_plans.rs`, `programs/integration_tests/tests/sdk_quotes.rs`
- Modify: `programs/integration_tests/tests/common/mod.rs` (`mod sdk; pub use sdk::*;`), `docs/architecture/sdk.md`

**Interfaces:**
- Consumes: every `auction_sdk::plan` / `plan_host` builder, `auction_sdk::quote::*`, `verify_config`, `verify_binding`; World helpers (`with_engine`, `with_system`, `unsafe_position`, `liquidated_world`, `bidder`, `book_shortfall`, `donate_stable`, `fund_stable`, `set_oracle`, `advance`, `balance`, `read`, `create_protocol_token`).
- Produces: `WorldChain<'a>(pub &'a World)` implementing `auction_sdk::chain::Chain` over the in-process state (no blocks, no send); `sdk_deployment() -> Deployment`; `World::sdk_config(&self) -> VerifiedConfig`; `World::run_plan(&mut self, plan: &Plan, signers: &[u8]) -> Result<(), LeeError>` (signers must equal `plan.signing_requirements`, in order).

- [ ] **Step 1: The adapter**

Create `programs/integration_tests/tests/common/sdk.rs`:

```rust
//! The SDK against the real in-process guests (Plan 3a Task 8): `WorldChain` reads the V03State, and
//! `run_plan` signs an SDK plan with the test keys and applies it like any other transaction.
use super::*;
use auction_sdk::{
    chain::{BlockSummary, Chain},
    deployment::Deployment,
    error::{SdkError, SdkResult},
    plan::Plan,
    verify::{verify_config, VerifiedConfig},
};
use lee::public_transaction::{Message, WitnessSet};

pub struct WorldChain<'a>(pub &'a World);

impl Chain for WorldChain<'_> {
    fn account(&self, id: &AccountId) -> SdkResult<Account> {
        Ok(self.0.state.get_account_by_id(*id))
    }
    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<Nonce>> {
        Ok(ids
            .iter()
            .map(|i| self.0.state.get_account_by_id(*i).nonce)
            .collect())
    }
    fn last_block_id(&self) -> SdkResult<u64> {
        Ok(self.0.block)
    }
    fn blocks(&self, _from: u64, _to: u64) -> SdkResult<Vec<BlockSummary>> {
        Err(SdkError::Chain("the in-process World keeps no blocks".into()))
    }
    fn send(&self, _tx: &PublicTransaction) -> SdkResult<[u8; 32]> {
        Err(SdkError::Chain("use World::run_plan".into()))
    }
}

pub fn sdk_deployment() -> Deployment {
    Deployment {
        network: "in-process".into(),
        sequencer_url: String::new(),
        engine_program: engine_program(),
        host_program: host_program(),
        token_program: token_program(),
        twap_oracle_program: None,
        engine_admin: Some(id(k::ENGINE_ADMIN)),
        config: Some(config()),
    }
}

impl World {
    pub fn sdk_config(&self) -> VerifiedConfig {
        verify_config(&WorldChain(self), &sdk_deployment(), &config()).expect("the World config verifies")
    }

    /// Signs `plan` with the test keys of `signers` (== `plan.signing_requirements`) and applies it.
    pub fn run_plan(&mut self, plan: &Plan, signers: &[u8]) -> Result<(), LeeError> {
        assert_eq!(
            signers.iter().map(|n| id(*n)).collect::<Vec<_>>(),
            plan.signing_requirements,
            "signers follow the plan"
        );
        self.seed_clock();
        let keys: Vec<PrivateKey> = signers.iter().map(|n| key(*n)).collect();
        let refs: Vec<&PrivateKey> = keys.iter().collect();
        let nonces = plan
            .signing_requirements
            .iter()
            .map(|a| self.state.get_account_by_id(*a).nonce)
            .collect();
        let message = Message::new_preserialized(
            plan.program_id,
            plan.account_ids.clone(),
            nonces,
            plan.instruction_words.clone(),
        );
        let witness = WitnessSet::for_message(&message, &refs);
        let result = self
            .state
            .transition_from_public_transaction(
                &PublicTransaction::new(message, witness),
                self.block,
                self.now,
            )
            .map(|_| ());
        if result.is_ok() {
            self.block += 1;
        }
        result
    }
}
```

In `programs/integration_tests/tests/common/mod.rs`, next to `mod engine; mod system;` add `mod sdk;` and `pub use sdk::*;`.

- [ ] **Step 2: Write the plan tests**

Create `programs/integration_tests/tests/sdk_plans.rs`:

```rust
//! Plan 3a Task 8: every SDK plan runs on the real token, stablecoin and engine guests.
mod common;

use auction_engine_core::{
    args::{CollateralArgs, EngineParamsArgs},
    state::*,
};
use auction_engine_core::args::DEBT_RELIST_DELAY_MS;
use auction_sdk::{plan, plan_host, quote, verify::verify_binding};
use common::*;

#[test]
fn the_world_config_verifies_and_the_host_binds_it() {
    let w = World::with_engine();
    let v = w.sdk_config();
    verify_binding(&WorldChain(&w), &v).expect("bound");
    assert_eq!(v.engine.buffer, buffer());
}

#[test]
fn liquidate_bid_settle_and_claim_via_sdk_plans() {
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    let v = w.sdk_config();
    let q = quote::quote_liquidation(&WorldChain(&w), &v, &pos, w.now).expect("quote");
    let p = plan::liquidate(
        &v,
        &plan::LiquidateArgs {
            keeper: id(k::KEEPER),
            keeper_collateral_holding: id(k::KEEPER_COLL),
            keeper_holding_is_fresh: true,
            position: pos,
            nonce: q.nonce,
            oracle: oracle(),
        },
    )
    .expect("plan");
    w.run_plan(&p, &[k::KEEPER, k::KEEPER_COLL]).expect("liquidate");
    w.bidder(k::BIDDER_STABLE, k::BIDDER_COLL, 1_000_000);
    let a = auction(pos, 0);
    let bid = |with_settlement: bool, coin: u128| plan::BidArgs {
        bidder_stable: id(k::BIDDER_STABLE),
        bidder_collateral: id(k::BIDDER_COLL),
        bidder_collateral_is_fresh: false,
        auction: a,
        coin_in_max: coin,
        min_collateral_out: 0,
        with_settlement,
        oracle: oracle(),
    };
    w.run_plan(&plan::bid(&v, &bid(false, 100_000)).expect("plan"), &[k::BIDDER_STABLE])
        .expect("partial bid");
    w.run_plan(&plan::bid(&v, &bid(true, u128::MAX)).expect("plan"), &[k::BIDDER_STABLE])
        .expect("final bid with settlement");
    let st: AuctionState = w.read(a);
    assert!(st.settled && st.leftover_claimable > 0);
    let claim = plan::claim_leftover(&v, id(k::OWNER), id(k::OWNER_COLL), false, a).expect("plan");
    w.run_plan(&claim, &[k::OWNER]).expect("claim");
    assert_eq!(w.read::<AuctionState>(a).leftover_claimed, st.leftover_claimable);
}

#[test]
fn observe_settle_debt_and_check_solvency_via_sdk_plans() {
    let mut w = World::with_engine();
    let v = w.sdk_config();
    for p in [
        plan::observe_feed(&v, id(k::CALLER), oracle()),
        plan::settle_debt(&v, id(k::CALLER), u128::MAX),
        plan::check_solvency(&v, id(k::CALLER)),
    ] {
        w.run_plan(&p.expect("plan"), &[k::CALLER]).expect("applies");
    }
    assert!(!w.host_frozen());
}

#[test]
fn surplus_auction_via_sdk_plans() {
    let mut w = World::with_system(false);
    w.donate_stable(buffer(), 1_500);
    let v = w.sdk_config();
    w.run_plan(&plan::start_surplus(&v, id(k::CALLER), 0).expect("plan"), &[k::CALLER])
        .expect("start");
    let q = quote::quote_take_surplus(&WorldChain(&w), &v, 0, 500, w.now).expect("quote");
    let before = w.balance(id(k::TAKER_PROTOCOL));
    let take = plan::TakeArgs {
        payer: id(k::TAKER_PROTOCOL),
        receiver: id(k::TAKER_STABLE),
        receiver_is_fresh: false,
        nonce: 0,
        amount_coin: 500,
        limit: quote::max_in(q.tokens, 50),
    };
    w.run_plan(&plan::take_surplus(&v, &take).expect("plan"), &[k::TAKER_PROTOCOL])
        .expect("take");
    assert_eq!(before - w.balance(id(k::TAKER_PROTOCOL)), q.tokens, "burned == quote");
    assert_eq!(w.balance(id(k::TAKER_STABLE)), 500);
}

#[test]
fn debt_auction_via_sdk_plans() {
    let mut w = World::with_system(false);
    w.book_shortfall();
    w.fund_stable(k::TAKER_STABLE, 1_000);
    let v = w.sdk_config();
    w.run_plan(&plan::start_debt(&v, id(k::CALLER), 0).expect("plan"), &[k::CALLER])
        .expect("start");
    let q = quote::quote_take_debt(&WorldChain(&w), &v, 0, 500, w.now).expect("quote");
    let before = w.balance(id(k::TAKER_PROTOCOL));
    let take = plan::TakeArgs {
        payer: id(k::TAKER_STABLE),
        receiver: id(k::TAKER_PROTOCOL),
        receiver_is_fresh: false,
        nonce: 0,
        amount_coin: 500,
        limit: quote::min_out(q.tokens, 50),
    };
    w.run_plan(&plan::take_debt(&v, &take).expect("plan"), &[k::TAKER_STABLE])
        .expect("take");
    assert_eq!(w.balance(id(k::TAKER_PROTOCOL)) - before, q.tokens, "minted == quote");
    let start_again = plan::start_debt(&w.sdk_config(), id(k::CALLER), 1).expect("plan");
    assert_err(w.run_plan(&start_again, &[k::CALLER]), "DelayNotElapsed");
    w.advance(DEBT_RELIST_DELAY_MS);
    w.run_plan(&start_again, &[k::CALLER])
        .expect("relist after the delay (ADR-039)");
}

#[test]
fn admin_plans_run_on_the_real_guests() {
    let mut w = World::new();
    w.create_protocol_token(10_000, mint_authority()).expect("token first (ADR-038)");
    let p = EngineParamsArgs {
        protocol_token_enabled: true,
        ..EngineParamsArgs::defaults_for_tests()
    };
    let host = auction_sdk::addresses::HostAddresses::new(host_program());
    let init = plan::init_config(engine_program(), &host, id(k::ENGINE_ADMIN), SALT, &p, Some(protocol_token_def()), None)
        .expect("plan");
    w.run_plan(&init, &[k::ENGINE_ADMIN]).expect("init_config");
    let bind = plan_host::set_liquidation_engine(&host, id(k::HOST_ADMIN), engine_program(), engine_authority())
        .expect("plan");
    w.run_plan(&bind, &[k::HOST_ADMIN]).expect("bind");
    let v = w.sdk_config();
    let coll = plan::set_collateral_config(&v, id(k::ENGINE_ADMIN), oracle(), &CollateralArgs::defaults_for_tests())
        .expect("plan");
    w.run_plan(&coll, &[k::ENGINE_ADMIN]).expect("set_collateral_config");
    let p2 = EngineParamsArgs {
        protocol_token_ref_price: p.protocol_token_ref_price * 3 / 2,
        ..p.clone()
    };
    let set = plan::set_engine_params(&v, id(k::ENGINE_ADMIN), &p2, Some(protocol_token_def()), None).expect("plan");
    w.run_plan(&set, &[k::ENGINE_ADMIN]).expect("set_engine_params");
    assert_eq!(w.read::<EngineConfigState>(config()).protocol_token_ref_price, p2.protocol_token_ref_price);
}

#[test]
fn negative_controller_gains_are_encoded_and_accepted_by_the_host() {
    let mut w = World::new();
    let host = auction_sdk::addresses::HostAddresses::new(host_program());
    let minus_one = -i128::try_from(ONE).expect("RAY fits i128");
    let p = plan_host::set_controller_gains(&host, id(k::HOST_ADMIN), minus_one, -1).expect("plan");
    w.run_plan(&p, &[k::HOST_ADMIN]).expect("signed gains accepted");
    let pp: stablecoin_core::ProtocolParameters = w.read(protocol_parameters());
    assert_eq!((pp.controller_proportional_gain, pp.controller_integral_gain), (minus_one, -1));
}
```

- [ ] **Step 3: Write the quote tests (Review Focus 1)**

Create `programs/integration_tests/tests/sdk_quotes.rs`:

```rust
//! Plan 3a Task 8 / Review Focus 1: SDK quotes equal on-chain results at inclusion time, and a stale
//! quote is protected by the slippage bound derived from it.
mod common;

use auction_engine_core::state::*;
use auction_sdk::{plan, quote};
use common::*;

fn bid_plan(
    v: &auction_sdk::verify::VerifiedConfig,
    a: lee_core::account::AccountId,
    coin: u128,
    min_out: u128,
) -> auction_sdk::plan::Plan {
    plan::bid(
        v,
        &plan::BidArgs {
            bidder_stable: id(k::BIDDER_STABLE),
            bidder_collateral: id(k::BIDDER_COLL),
            bidder_collateral_is_fresh: false,
            auction: a,
            coin_in_max: coin,
            min_collateral_out: min_out,
            with_settlement: false,
            oracle: oracle(),
        },
    )
    .expect("plan")
}

#[test]
fn liquidation_quote_equals_the_auction_the_engine_opens() {
    let mut w = World::with_engine();
    let pos = w.unsafe_position();
    let v = w.sdk_config();
    let q = quote::quote_liquidation(&WorldChain(&w), &v, &pos, w.now).expect("quote");
    assert!(q.health.unsafe_at_min_cr);
    assert_eq!((q.quote.amount_to_raise, q.quote.keeper_reward, q.quote.lot), (LIQ_TARGET, LIQ_REWARD, LIQ_LOT));
    w.liquidate(pos, q.nonce).expect("liquidate");
    let a: AuctionState = w.read(auction(pos, q.nonce));
    assert_eq!(
        (a.collateral_seized, a.keeper_reward, a.collateral_left, a.amount_to_raise, a.debt_principal),
        (q.quote.seized, q.quote.keeper_reward, q.quote.lot, q.quote.amount_to_raise, q.quote.debt_principal)
    );
}

#[test]
fn bid_quote_at_inclusion_time_equals_the_on_chain_fill() {
    let (mut w, pos) = liquidated_world();
    let a = auction(pos, 0);
    let v = w.sdk_config();
    let early = quote::quote_bid(&WorldChain(&w), &v, &a, 100_000, 0, w.now).expect("quote at T");
    w.advance(300_000);
    w.set_oracle(R, w.now); // keep the feed fresh; the discount alone moves
    let at_inclusion = quote::quote_bid(&WorldChain(&w), &v, &a, 100_000, 0, w.now).expect("quote at T + 300 s");
    assert!(at_inclusion.fill.collateral_out > early.fill.collateral_out, "the discount grew");
    let (stable_before, coll_before) = (w.balance(id(k::BIDDER_STABLE)), w.balance(id(k::BIDDER_COLL)));
    w.run_plan(&bid_plan(&v, a, 100_000, quote::min_out(early.fill.collateral_out, 0)), &[k::BIDDER_STABLE])
        .expect("bid lands at the inclusion clock");
    assert_eq!(w.balance(id(k::BIDDER_COLL)) - coll_before, at_inclusion.fill.collateral_out);
    assert_eq!(stable_before - w.balance(id(k::BIDDER_STABLE)), at_inclusion.fill.charged);
}

#[test]
fn stale_quote_is_protected_by_min_collateral_out() {
    let (mut w, pos) = liquidated_world();
    let a = auction(pos, 0);
    let v = w.sdk_config();
    let q = quote::quote_bid(&WorldChain(&w), &v, &a, 100_000, 0, w.now).expect("quote");
    // The oracle moves 5% against the bidder (fewer collateral per coin) before inclusion.
    w.set_oracle(R * 95 / 100, w.now);
    let tight = bid_plan(&v, a, 100_000, quote::min_out(q.fill.collateral_out, 50));
    assert_err(w.run_plan(&tight, &[k::BIDDER_STABLE]), "Slippage");
    let loose = bid_plan(&v, a, 100_000, quote::min_out(q.fill.collateral_out, 1_000));
    w.run_plan(&loose, &[k::BIDDER_STABLE]).expect("within a 10% margin");
}
```

- [ ] **Step 4: Run them**

Run (background, timeout 3600000): `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p engine_integration_tests --test sdk_plans --test sdk_quotes 2>&1 | grep -E "test |test result|panicked"'`
Expected: `sdk_plans` 7 passed, `sdk_quotes` 3 passed. A failure here means an SDK account list, encoding or quote path differs from the program: fix the SDK, never the program or the assertion.

- [ ] **Step 5: Full suite, docs, gates, commit**

Run the whole `engine_integration_tests` suite (background): every binary `ok`.

`docs/architecture/sdk.md`, append:

```markdown
## Proof on the real guests

`programs/integration_tests/tests/sdk_plans.rs` runs every builder through `World::run_plan` against the real token, stablecoin and engine guests (liquidate, partial and final bid, claim, observe, settle_debt, check_solvency, surplus and debt auctions, init/bind/collateral/params, signed controller gains). `sdk_quotes.rs` pins quote == on-chain result: the liquidation quote equals the opened auction, a bid quote taken at the inclusion clock equals the fill, and a quote taken earlier protects the bidder through `min_collateral_out` when the oracle moves (Review Focus 1).
```

Run the lint and format gates. Expected: clean.

Commit (controller — GPG signing on Windows):

```bash
git add programs/integration_tests/tests/common/sdk.rs programs/integration_tests/tests/common/mod.rs programs/integration_tests/tests/sdk_plans.rs programs/integration_tests/tests/sdk_quotes.rs docs/architecture/sdk.md
git commit -m "test(sdk): run every SDK plan on the real guests and pin quote == on-chain result"
```

---

### Task 9: Wallet keystore and signing (ADR-041)

The CLI signs with the LEZ wallet's own keystore (spec §6.6). The wallet crate itself is not a dependency: it pulls `keycard_wallet` → `pcsc` (a native smart-card library) and a full sync client. The SDK reads the wallet's `storage.json` format (LEZ v0.2.4 `wallet::storage::persistent`) directly: `key_chain.accounts[]` entries `{"Public": {account_id, data: {ssk, ...}}}` (the wallet signs public transactions with `ssk`: `impl From<&ChildKeysPublic> for &PrivateKey` returns `&value.ssk`), `{"ImportedPublic": {account_id, pub_sign_key}}`, private entries, and `labels: {label: {"Public"|"Private": id}}`. Every key is checked to derive its account id.

**Files:**
- Create: `crates/auction_sdk/src/keystore.rs`, `crates/auction_sdk/src/tx.rs`, `crates/auction_sdk/src/testing/wallet.rs`
- Modify: `crates/auction_sdk/src/{lib.rs,error.rs}`, `crates/auction_sdk/src/testing/mod.rs`, `crates/auction_sdk/Cargo.toml` (dev-dependency `tempfile`), `programs/integration_tests/tests/common/sdk.rs` (`run_plan` uses `tx::sign`), `docs/architecture/sdk.md`, `docs/design-log.md` (ADR-041)

**Interfaces:**
- Consumes: `lee::{PrivateKey, PublicKey, PublicTransaction}`, `lee::public_transaction::{Message, WitnessSet}`, `Plan`.
- Produces:
  - `keystore::{Keystore, KeystoreError, HOME_ENV}`; `KeystoreError::{HomeNotSet, NotFound(String), Unreadable { path, reason }, Malformed { path, reason }, UnknownLabel(String), NoKey(String), PrivateAccount(String), KeyMismatch(String)}`; `Keystore::default_path(wallet_home: Option<&Path>) -> Result<PathBuf, KeystoreError>`, `Keystore::path_from(wallet_home: Option<&Path>, env_home: Option<OsString>)` (pure), `open(path)`, `parse(text, path)`, `resolve(&self, label_or_id: &str) -> Result<AccountId, KeystoreError>`, `key(&self, id) -> Result<&PrivateKey, KeystoreError>`; hand-written `Debug` (ids and labels only).
  - `SdkError::Keystore(KeystoreError)` (with `From`).
  - `tx::{KeySource, sign}`: `trait KeySource { fn signing_key(&self, id: &AccountId) -> SdkResult<&PrivateKey>; }` (implemented by `Keystore` and `testing::TestKeys`); `sign(plan: &Plan, nonces: &[Nonce], keys: &dyn KeySource) -> SdkResult<PublicTransaction>`.
  - `testing::{TestKeys, keystore_json, write_keystore, STANDARD_LABELS}`: `TestKeys::of(seeds: &[u8])`; `write_keystore(dir: &Path, entries: &[(&str, u8)]) -> std::io::Result<PathBuf>` writes a wallet-format `storage.json`; `STANDARD_LABELS: [(&str, u8); 14]` = engine-admin 50, keeper 51, keeper-coll 52, bidder-stable 53, bidder-coll 54, owner 41, owner-coll 42, owner-stable 43, host-admin 44, freeze 45, caller 62, taker-stable 57, taker-protocol 58, owner2 65.

- [ ] **Step 1: Write the failing tests (Review Focus 4)**

Create `crates/auction_sdk/src/keystore.rs` with only its tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{id, key, keystore_json};
    use std::ffi::OsString;

    fn store(json: &str) -> Result<Keystore, KeystoreError> {
        Keystore::parse(json, Path::new("/w/storage.json"))
    }

    #[test]
    fn keystore_resolves_labels_and_ids_and_signs_with_ssk() {
        let ks = store(&keystore_json(&[("keeper", 51), ("caller", 62)])).expect("parses");
        assert_eq!(ks.resolve("keeper").expect("label"), id(51));
        assert_eq!(ks.resolve(&id(62).to_string()).expect("base58 id"), id(62));
        assert_eq!(ks.key(&id(51)).expect("key"), &key(51));
        // A non-signing account (e.g. a recipient) resolves by id even without a key.
        assert_eq!(ks.resolve(&id(9).to_string()).expect("any id"), id(9));
    }

    #[test]
    fn keystore_errors_are_typed_and_never_print_key_material() {
        let secret = key(50).to_string();
        let home = Keystore::path_from(None, None).expect_err("no home");
        assert_eq!(home, KeystoreError::HomeNotSet);
        assert_eq!(
            Keystore::path_from(None, Some(OsString::from("/w"))).expect("env"),
            PathBuf::from("/w/storage.json")
        );
        let dir = tempfile::tempdir().expect("tmp");
        let missing = Keystore::open(&dir.path().join("none/storage.json")).expect_err("missing");
        assert!(matches!(missing, KeystoreError::NotFound(_)));
        let bad = store("{ not json").expect_err("malformed");
        assert!(matches!(bad, KeystoreError::Malformed { .. }));
        let ks = store(&keystore_json(&[("engine-admin", 50)])).expect("parses");
        assert!(matches!(ks.resolve("nobody"), Err(KeystoreError::UnknownLabel(_))));
        assert!(matches!(ks.key(&id(51)), Err(KeystoreError::NoKey(_))));
        // A key stored under another account's id.
        let swapped = keystore_json(&[("engine-admin", 50)]).replace(&id(50).to_string(), &id(51).to_string());
        let mismatch = store(&swapped).expect_err("key does not derive the id");
        assert!(matches!(mismatch, KeystoreError::KeyMismatch(_)));
        // A private account is refused as a signer and as a resolved account.
        let with_private = format!(
            "{{\"key_chain\":{{\"accounts\":[{{\"Private\":{{\"account_id\":\"{}\"}}}}]}},\"labels\":{{\"secret\":{{\"Private\":\"{}\"}}}}}}",
            id(70),
            id(70)
        );
        let ks2 = store(&with_private).expect("parses");
        assert!(matches!(ks2.resolve("secret"), Err(KeystoreError::PrivateAccount(_))));
        assert!(matches!(ks2.key(&id(70)), Err(KeystoreError::PrivateAccount(_))));
        for e in [home, missing, bad, mismatch] {
            assert!(!e.to_string().contains(&secret), "{e}");
        }
        assert!(!format!("{ks:?}").contains(&secret), "Debug lists ids only");
    }
}
```

Create `crates/auction_sdk/src/tx.rs` with only its test:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::keystore::KeystoreError;
    use crate::testing::standard::{self, k, ORACLE};
    use crate::testing::{id, TestKeys};
    use crate::verify::verify_config;

    #[test]
    fn sign_produces_a_witness_valid_for_the_message_in_signer_order() {
        let fx = standard::standard();
        let v = verify_config(&fx.chain, &fx.deployment, &standard::config_id()).expect("verifies");
        let plan = crate::plan::observe_feed(&v, id(k::CALLER), ORACLE).expect("plan");
        let tx = sign(&plan, &[Nonce(3)], &TestKeys::of(&[k::CALLER])).expect("signed");
        assert!(tx.witness_set().is_valid_for(tx.message()));
        assert_eq!(tx.message().nonces, vec![Nonce(3)]);
        assert_eq!(tx.message().instruction_data, plan.instruction_words);
        let e = sign(&plan, &[Nonce(3)], &TestKeys::of(&[k::KEEPER])).expect_err("no key");
        assert!(matches!(e, SdkError::Keystore(KeystoreError::NoKey(_))));
        let e = sign(&plan, &[], &TestKeys::of(&[k::CALLER])).expect_err("nonce count");
        assert!(matches!(e, SdkError::Encode(_)));
    }
}
```

Add to `lib.rs`: `pub mod keystore; pub mod tx;`; in `testing/mod.rs`: `mod wallet; pub use wallet::{keystore_json, write_keystore, TestKeys, STANDARD_LABELS};`. In `crates/auction_sdk/Cargo.toml` `[dev-dependencies]` add `tempfile = "3"` (the CLI declares its own in Task 13).

- [ ] **Step 2: Run to verify they fail**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_sdk --features testing keystore tx'`
Expected: FAIL to compile (`cannot find type Keystore`, `cannot find function sign`).

- [ ] **Step 3: Implement `keystore.rs` (above its tests)**

```rust
//! The LEZ wallet keystore (`$LEE_WALLET_HOME_DIR/storage.json`, LEZ v0.2.4 `wallet::storage`), read
//! directly (ADR-041). Public accounts sign with the stored `ssk` (the key the wallet itself signs
//! with); imported public accounts with `pub_sign_key`. Private accounts are recorded only so they can
//! be refused: the PoC submits public transactions (spec §8). The v0.2.4 wallet stores keys
//! unencrypted (its password argument is unused upstream); this module never prints, logs or returns
//! key material in errors or `Debug`.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use lee::{PrivateKey, PublicKey};
use lee_core::account::AccountId;
use serde_json::Value;

pub const HOME_ENV: &str = "LEE_WALLET_HOME_DIR";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeystoreError {
    #[error("LEE_WALLET_HOME_DIR is not set and no --wallet-home was given")]
    HomeNotSet,
    #[error("wallet storage not found at {0} (create accounts with `wallet account new public`)")]
    NotFound(String),
    #[error("cannot read wallet storage {path}: {reason}")]
    Unreadable { path: String, reason: String },
    #[error("wallet storage {path} is malformed: {reason}")]
    Malformed { path: String, reason: String },
    #[error("no wallet account is labelled `{0}`")]
    UnknownLabel(String),
    #[error("account {0} is not in the wallet (no signing key)")]
    NoKey(String),
    #[error("account {0} is a private account; the PoC signs public transactions only (spec §8)")]
    PrivateAccount(String),
    #[error("the wallet entry for {0} holds a key that does not derive that account id")]
    KeyMismatch(String),
}

pub struct Keystore {
    path: PathBuf,
    keys: BTreeMap<AccountId, PrivateKey>,
    private: BTreeSet<AccountId>,
    labels: BTreeMap<String, AccountId>,
}

impl std::fmt::Debug for Keystore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keystore")
            .field("path", &self.path)
            .field("public_accounts", &self.keys.keys().collect::<Vec<_>>())
            .field("labels", &self.labels.keys().collect::<Vec<_>>())
            .finish()
    }
}

fn insert(
    keys: &mut BTreeMap<AccountId, PrivateKey>,
    id: AccountId,
    key: PrivateKey,
) -> Result<(), KeystoreError> {
    if AccountId::from(&PublicKey::new_from_private_key(&key)) != id {
        return Err(KeystoreError::KeyMismatch(id.to_string()));
    }
    keys.insert(id, key);
    Ok(())
}

impl Keystore {
    /// `--wallet-home` wins over `$LEE_WALLET_HOME_DIR`.
    pub fn default_path(wallet_home: Option<&Path>) -> Result<PathBuf, KeystoreError> {
        Self::path_from(wallet_home, std::env::var_os(HOME_ENV))
    }

    pub fn path_from(
        wallet_home: Option<&Path>,
        env_home: Option<OsString>,
    ) -> Result<PathBuf, KeystoreError> {
        let home = match wallet_home {
            Some(h) => h.to_path_buf(),
            None => env_home.map(PathBuf::from).ok_or(KeystoreError::HomeNotSet)?,
        };
        Ok(home.join("storage.json"))
    }

    pub fn open(path: &Path) -> Result<Self, KeystoreError> {
        let shown = path.display().to_string();
        let text = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                KeystoreError::NotFound(shown.clone())
            } else {
                KeystoreError::Unreadable {
                    path: shown.clone(),
                    reason: e.kind().to_string(),
                }
            }
        })?;
        Self::parse(&text, path)
    }

    pub fn parse(text: &str, path: &Path) -> Result<Self, KeystoreError> {
        let shown = path.display().to_string();
        let malformed = |reason: String| KeystoreError::Malformed {
            path: shown.clone(),
            reason,
        };
        // serde_json errors name a line and a column, never the content.
        let v: Value = serde_json::from_str(text).map_err(|e| malformed(e.to_string()))?;
        let entries = v
            .pointer("/key_chain/accounts")
            .and_then(Value::as_array)
            .ok_or_else(|| malformed("key_chain.accounts is missing".into()))?;
        let mut keys = BTreeMap::new();
        let mut private = BTreeSet::new();
        for (i, entry) in entries.iter().enumerate() {
            let (kind, body) = entry
                .as_object()
                .and_then(|o| o.iter().next())
                .ok_or_else(|| malformed(format!("account entry {i} is not an object")))?;
            let account = |field: &str| {
                body.get(field)
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse::<AccountId>().ok())
                    .ok_or_else(|| malformed(format!("account entry {i}: missing or invalid {field}")))
            };
            let signing_key = |pointer: &str| {
                body.pointer(pointer)
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse::<PrivateKey>().ok())
                    .ok_or_else(|| malformed(format!("account entry {i}: missing or invalid signing key")))
            };
            match kind.as_str() {
                "Public" => insert(&mut keys, account("account_id")?, signing_key("/data/ssk")?)?,
                "ImportedPublic" => {
                    insert(&mut keys, account("account_id")?, signing_key("/pub_sign_key")?)?;
                }
                "Private" | "ImportedPrivate" => {
                    if let Ok(id) = account("account_id") {
                        private.insert(id);
                    }
                }
                other => return Err(malformed(format!("account entry {i}: unknown kind {other}"))),
            }
        }
        let mut labels = BTreeMap::new();
        if let Some(map) = v.get("labels").and_then(Value::as_object) {
            for (label, target) in map {
                let (kind, raw) = target
                    .as_object()
                    .and_then(|o| o.iter().next())
                    .and_then(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                    .ok_or_else(|| malformed(format!("label {label} is malformed")))?;
                let id: AccountId = raw
                    .parse()
                    .map_err(|_| malformed(format!("label {label}: invalid account id")))?;
                if kind == "Private" {
                    private.insert(id);
                }
                labels.insert(label.clone(), id);
            }
        }
        Ok(Self {
            path: path.to_path_buf(),
            keys,
            private,
            labels,
        })
    }

    /// A wallet label, or a base58 account id (recipients need no key). Private accounts are refused.
    pub fn resolve(&self, label_or_id: &str) -> Result<AccountId, KeystoreError> {
        let id = match self.labels.get(label_or_id) {
            Some(id) => *id,
            None => label_or_id
                .parse::<AccountId>()
                .map_err(|_| KeystoreError::UnknownLabel(label_or_id.to_owned()))?,
        };
        if self.private.contains(&id) {
            return Err(KeystoreError::PrivateAccount(id.to_string()));
        }
        Ok(id)
    }

    pub fn key(&self, id: &AccountId) -> Result<&PrivateKey, KeystoreError> {
        if self.private.contains(id) {
            return Err(KeystoreError::PrivateAccount(id.to_string()));
        }
        self.keys
            .get(id)
            .ok_or_else(|| KeystoreError::NoKey(id.to_string()))
    }
}
```

In `crates/auction_sdk/src/error.rs` add the variant (after `Chain`):

```rust
    #[error(transparent)]
    Keystore(#[from] crate::keystore::KeystoreError),
```

- [ ] **Step 4: Implement `tx.rs` and the test helpers**

`crates/auction_sdk/src/tx.rs` (above its test):

```rust
//! Signing: a plan plus the signers' current nonces become a `PublicTransaction`. The message is built
//! with `Message::new_preserialized` from the plan's own instruction words, so what is signed is
//! exactly what the encoders produced.

use lee::{
    public_transaction::{Message, WitnessSet},
    PrivateKey, PublicTransaction,
};
use lee_core::account::{AccountId, Nonce};

use crate::{
    error::{SdkError, SdkResult},
    keystore::Keystore,
    plan::Plan,
};

pub trait KeySource {
    fn signing_key(&self, id: &AccountId) -> SdkResult<&PrivateKey>;
}

impl KeySource for Keystore {
    fn signing_key(&self, id: &AccountId) -> SdkResult<&PrivateKey> {
        self.key(id).map_err(SdkError::from)
    }
}

/// `nonces[i]` is the current nonce of `plan.signing_requirements[i]`.
pub fn sign(plan: &Plan, nonces: &[Nonce], keys: &dyn KeySource) -> SdkResult<PublicTransaction> {
    if nonces.len() != plan.signing_requirements.len() {
        return Err(SdkError::Encode(format!(
            "{} nonces for {} signers",
            nonces.len(),
            plan.signing_requirements.len()
        )));
    }
    let signing: Vec<&PrivateKey> = plan
        .signing_requirements
        .iter()
        .map(|id| keys.signing_key(id))
        .collect::<SdkResult<_>>()?;
    let message = Message::new_preserialized(
        plan.program_id,
        plan.account_ids.clone(),
        nonces.to_vec(),
        plan.instruction_words.clone(),
    );
    let witness = WitnessSet::for_message(&message, &signing);
    Ok(PublicTransaction::new(message, witness))
}
```

Create `crates/auction_sdk/src/testing/wallet.rs`:

```rust
//! Wallet-format keystores and in-memory key sources for tests (never real keys: seeds `[n; 32]`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lee::PrivateKey;
use lee_core::account::AccountId;
use serde_json::{json, Map, Value};

use super::{id, key};
use crate::{error::{SdkError, SdkResult}, keystore::KeystoreError, tx::KeySource};

/// The labels the CLI golden tests and the standard fixture use.
pub const STANDARD_LABELS: [(&str, u8); 14] = [
    ("engine-admin", 50),
    ("keeper", 51),
    ("keeper-coll", 52),
    ("bidder-stable", 53),
    ("bidder-coll", 54),
    ("owner", 41),
    ("owner-coll", 42),
    ("owner-stable", 43),
    ("host-admin", 44),
    ("freeze", 45),
    ("caller", 62),
    ("taker-stable", 57),
    ("taker-protocol", 58),
    ("owner2", 65),
];

/// A `storage.json` in the LEZ v0.2.4 wallet format with one public account per entry.
#[must_use]
pub fn keystore_json(entries: &[(&str, u8)]) -> String {
    let accounts: Vec<Value> = entries
        .iter()
        .map(|(_, n)| {
            json!({ "Public": {
                "account_id": id(*n).to_string(),
                "chain_index": [*n],
                "data": {
                    "sk": key(*n).to_string(),
                    "ssk": key(*n).to_string(),
                    "pk": "00",
                    "cc": vec![0u8; 32],
                    "cci": *n
                }
            }})
        })
        .collect();
    let labels: Map<String, Value> = entries
        .iter()
        .map(|(l, n)| ((*l).to_owned(), json!({ "Public": id(*n).to_string() })))
        .collect();
    json!({ "key_chain": { "accounts": accounts }, "last_synced_block": 0, "labels": labels })
        .to_string()
}

pub fn write_keystore(dir: &Path, entries: &[(&str, u8)]) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("storage.json");
    std::fs::write(&path, keystore_json(entries))?;
    Ok(path)
}

pub struct TestKeys(pub BTreeMap<AccountId, PrivateKey>);

impl TestKeys {
    #[must_use]
    pub fn of(seeds: &[u8]) -> Self {
        Self(seeds.iter().map(|n| (id(*n), key(*n))).collect())
    }
}

impl KeySource for TestKeys {
    fn signing_key(&self, id: &AccountId) -> SdkResult<&PrivateKey> {
        self.0
            .get(id)
            .ok_or_else(|| SdkError::Keystore(KeystoreError::NoKey(id.to_string())))
    }
}
```

In `programs/integration_tests/tests/common/sdk.rs`, replace the body of `run_plan` after `self.seed_clock();` so that it signs through the SDK (proving `tx::sign` on the real guests):

```rust
        let nonces: Vec<Nonce> = plan
            .signing_requirements
            .iter()
            .map(|a| self.state.get_account_by_id(*a).nonce)
            .collect();
        let tx = auction_sdk::tx::sign(plan, &nonces, &auction_sdk::testing::TestKeys::of(signers))
            .expect("the plan signs");
        let result = self
            .state
            .transition_from_public_transaction(&tx, self.block, self.now)
            .map(|_| ());
        if result.is_ok() {
            self.block += 1;
        }
        result
```

and drop the now-unused `use lee::public_transaction::{Message, WitnessSet};` and `keys`/`refs` lines.

- [ ] **Step 5: Run the tests**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_sdk --features testing && scripts/wsl-cargo.sh test -p engine_integration_tests --test sdk_plans --test sdk_quotes 2>&1 | grep -E "test result|panicked"'`
Expected: SDK all pass (3 new); `sdk_plans` 7 and `sdk_quotes` 3 still pass with SDK signing.

- [ ] **Step 6: Docs and ADR-041**

`docs/architecture/sdk.md`: module-map rows `| keystore | 9 | the wallet's storage.json read directly; labels or base58 ids; keys verified against their ids; private accounts refused |` and `| tx | 9 | KeySource, sign(plan, nonces, keys) |`, and a section "## Keystore" stating: path = `--wallet-home` or `$LEE_WALLET_HOME_DIR`, then `/storage.json`; public accounts sign with `ssk`; the v0.2.4 wallet keeps keys unencrypted on disk, so protect the wallet home like a key file; errors are typed (`KeystoreError`) and never contain key material.

Append to `docs/design-log.md`:

```markdown

---

## ADR-041: The CLI Reads the Wallet Keystore File Directly

**Context**
Spec §6.6: "Signing goes through the LEZ wallet keystore". The LEZ v0.2.4 `wallet` crate exposes `Storage::from_path` and `pub_account_signing_key`, but depending on it pulls `keycard_wallet` → `pcsc` (a native smart-card library) and the wallet's sync client into the SDK.

**Options**
- (A) Depend on the `wallet` crate.
- (B) Shell out to the `wallet` binary for every signature.
- (C) Read `storage.json` (the wallet's persisted format) and sign with `lee::PrivateKey`.

**Decision**
(C). The SDK parses `key_chain.accounts` (`Public` → `data.ssk`, `ImportedPublic` → `pub_sign_key`, private entries recorded to be refused) and `labels`, and checks that every key derives its stored account id. Accounts are created, `auth-transfer init`-ed and funded with the real `wallet` binary.

**Trade-offs**
A future wallet storage format change breaks the reader (pinned to LEZ v0.2.4; the format is tested by `keystore_json` fixtures). Keys are unencrypted on disk in v0.2.4 (upstream has not implemented storage encryption yet); the SDK never prints them. Hardware (keycard) accounts are not supported by the CLI.

**Status**
Accepted (Plan 3a, Task 9)
```

- [ ] **Step 7: Gates and commit (controller — GPG signing on Windows)**

Run the lint and format gates. Expected: clean.

```bash
git add crates/auction_sdk programs/integration_tests/tests/common/sdk.rs docs/architecture/sdk.md docs/design-log.md Cargo.lock
git commit -m "feat(sdk): wallet keystore reader and transaction signing"
```

---

### Task 10: Transaction confirmation and one in-flight transaction per signer (spec §6.5, ADR-042)

**Files:**
- Create: `crates/auction_sdk/src/confirm.rs`
- Modify: `crates/auction_sdk/src/lib.rs`, `docs/architecture/sdk.md`, `docs/design-log.md` (ADR-042)

**Interfaces:**
- Consumes: `Chain::{nonces, last_block_id, send}`, `tx::{sign, KeySource}`, `Plan`.
- Produces (`auction_sdk::confirm`): `ConfirmSettings { poll_interval: Duration, drop_after_blocks: u64, max_polls: u32 }` (default 2 s, 5, 900); `Outcome::{Included { nonce_after: u128 }, Dropped { height: u64 }}`; `Submitted { hash: [u8; 32], outcome: Outcome }`; `confirm(chain, signer: &AccountId, n0: u128, h0: u64, s: &ConfirmSettings, sleep: &dyn Fn(Duration)) -> SdkResult<Outcome>`; `InFlightLock::acquire(dir: &Path, signer: &AccountId, n0: u128, h0: u64, s: &ConfirmSettings) -> SdkResult<InFlightLock>` (released on drop); `submit(chain, keys: &dyn KeySource, plan: &Plan, lock_dir: &Path, s: &ConfirmSettings, sleep: &dyn Fn(Duration)) -> SdkResult<Submitted>`.

Semantics (ADR-009, spec §6.5): record the primary signer's nonce `n0` and the height `h0`; acquire the signer's lock; sign and send; poll: nonce `> n0` → `Included`; still `n0` when the height reaches `h0 + 5` → `Dropped` (nothing applied; the caller re-quotes, the SDK never resends). A lock file `<dir>/<signer>.lock` holds `{nonce, height}`; an existing lock is stale (and replaced) when the current nonce moved past it or the height passed `height + 5`, or when it does not parse (torn write after a crash); otherwise `SdkError::InFlight`. The "target account changed" check of spec §6.5 step 2 is redundant on v0.2.4 (the nonce advances only on apply) and is not implemented; recorded in ADR-042.

- [ ] **Step 1: Write the failing tests (Review Focus 2)**

Create `crates/auction_sdk/src/confirm.rs` with its tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::standard::{self, k, ORACLE};
    use crate::testing::{id, SendMode, TestKeys};
    use crate::verify::verify_config;

    fn fast() -> ConfirmSettings {
        ConfirmSettings {
            poll_interval: Duration::ZERO,
            drop_after_blocks: 5,
            max_polls: 50,
        }
    }

    fn observe_plan(fx: &standard::Fixture) -> Plan {
        let v = verify_config(&fx.chain, &fx.deployment, &standard::config_id()).expect("verifies");
        crate::plan::observe_feed(&v, id(k::CALLER), ORACLE).expect("plan")
    }

    #[test]
    fn included_transaction_reports_the_new_nonce_and_releases_the_lock() {
        let fx = standard::standard();
        let dir = tempfile::tempdir().expect("tmp");
        let s = submit(&fx.chain, &TestKeys::of(&[k::CALLER]), &observe_plan(&fx), dir.path(), &fast(), &|_| {})
            .expect("submitted");
        assert_eq!(s.outcome, Outcome::Included { nonce_after: 1 });
        assert!(std::fs::read_dir(dir.path()).expect("dir").next().is_none(), "lock released");
    }

    #[test]
    fn dropped_transaction_reports_dropped_and_releases_the_lock() {
        let fx = standard::standard();
        fx.chain.set_send_mode(SendMode::Drop);
        fx.chain.set_height_step(1);
        let dir = tempfile::tempdir().expect("tmp");
        let s = submit(&fx.chain, &TestKeys::of(&[k::CALLER]), &observe_plan(&fx), dir.path(), &fast(), &|_| {})
            .expect("a verdict");
        assert!(matches!(s.outcome, Outcome::Dropped { .. }));
        assert_eq!(fx.chain.nonces(&[id(k::CALLER)]).expect("nonce")[0].0, 0, "nothing applied");
        assert_eq!(fx.chain.sent().len(), 1, "never resent automatically");
        assert!(std::fs::read_dir(dir.path()).expect("dir").next().is_none(), "lock released");
    }

    #[test]
    fn a_second_transaction_of_the_same_signer_is_refused_while_one_is_in_flight() {
        let fx = standard::standard();
        let dir = tempfile::tempdir().expect("tmp");
        let h = fx.chain.last_block_id().expect("height");
        let held = InFlightLock::acquire(dir.path(), &id(k::CALLER), 0, h, &fast()).expect("first");
        let e = submit(&fx.chain, &TestKeys::of(&[k::CALLER]), &observe_plan(&fx), dir.path(), &fast(), &|_| {})
            .expect_err("in flight");
        assert!(matches!(e, SdkError::InFlight { .. }));
        assert!(fx.chain.sent().is_empty(), "nothing was sent");
        drop(held);
        submit(&fx.chain, &TestKeys::of(&[k::CALLER]), &observe_plan(&fx), dir.path(), &fast(), &|_| {})
            .expect("free again");
    }

    #[test]
    fn a_lock_left_by_a_crashed_process_is_recovered() {
        let fx = standard::standard();
        let dir = tempfile::tempdir().expect("tmp");
        let lock = dir.path().join(format!("{}.lock", id(k::CALLER)));
        std::fs::write(&lock, "{\"nonce\":0,\"height\":0}").expect("write");
        // The standard world has 3 blocks (3 < 0 + 5: still in flight); push until the window has passed.
        for _ in 0..3 {
            fx.chain.push_block(vec![]);
        }
        submit(&fx.chain, &TestKeys::of(&[k::CALLER]), &observe_plan(&fx), dir.path(), &fast(), &|_| {})
            .expect("height 6 passed the window of a lock taken at height 0");
        std::fs::write(&lock, "{ torn").expect("write");
        InFlightLock::acquire(dir.path(), &id(k::CALLER), 1, 6, &fast()).expect("a torn lock is replaced");
    }

    #[test]
    fn a_rejected_send_releases_the_lock_and_no_verdict_times_out() {
        let fx = standard::standard();
        let dir = tempfile::tempdir().expect("tmp");
        fx.chain.set_send_mode(SendMode::Reject);
        let e = submit(&fx.chain, &TestKeys::of(&[k::CALLER]), &observe_plan(&fx), dir.path(), &fast(), &|_| {})
            .expect_err("rejected");
        assert!(matches!(e, SdkError::Chain(_)));
        assert!(std::fs::read_dir(dir.path()).expect("dir").next().is_none());
        fx.chain.set_send_mode(SendMode::Drop);
        let few = ConfirmSettings { max_polls: 3, ..fast() };
        let e = submit(&fx.chain, &TestKeys::of(&[k::CALLER]), &observe_plan(&fx), dir.path(), &few, &|_| {})
            .expect_err("no verdict");
        assert!(matches!(e, SdkError::Chain(m) if m.contains("no confirmation")));
    }
}
```

Add `pub mod confirm;` to `lib.rs`.

Note on `a_lock_left_by_a_crashed_process_is_recovered`: the standard fixture has 3 blocks, so after 3 more the height is 6 ≥ 0 + 5.

- [ ] **Step 2: Run to verify they fail**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_sdk --features testing confirm'`
Expected: FAIL to compile (`cannot find function submit`).

- [ ] **Step 3: Implement (above the tests)**

```rust
//! Transaction confirmation (spec §6.5, ADR-009, ADR-042). On LEZ v0.2.4 `sendTransaction` success means
//! accepted, not executed: a transaction that fails at block build is silently dropped, and the signer
//! nonce advances only on apply. So: record the primary signer's nonce `n0` and the height `h0`, send,
//! poll the nonce; `nonce > n0` -> Included; still `n0` at height `h0 + drop_after_blocks` -> Dropped
//! (nothing was applied: re-quote). One transaction in flight per signer, enforced by a lock file.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use lee_core::account::AccountId;
use serde::{Deserialize, Serialize};

use crate::{
    chain::Chain,
    error::{SdkError, SdkResult},
    plan::Plan,
    tx::{sign, KeySource},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfirmSettings {
    pub poll_interval: Duration,
    pub drop_after_blocks: u64,
    pub max_polls: u32,
}

impl Default for ConfirmSettings {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(2),
            drop_after_blocks: 5,
            max_polls: 900,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Included { nonce_after: u128 },
    /// Nothing was applied; re-quote before sending anything again.
    Dropped { height: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Submitted {
    pub hash: [u8; 32],
    pub outcome: Outcome,
}

pub fn confirm<C: Chain + ?Sized>(
    chain: &C,
    signer: &AccountId,
    n0: u128,
    h0: u64,
    s: &ConfirmSettings,
    sleep: &dyn Fn(Duration),
) -> SdkResult<Outcome> {
    for _ in 0..s.max_polls {
        let nonce = chain
            .nonces(std::slice::from_ref(signer))?
            .first()
            .map_or(0, |n| n.0);
        if nonce > n0 {
            return Ok(Outcome::Included { nonce_after: nonce });
        }
        let height = chain.last_block_id()?;
        if height >= h0.saturating_add(s.drop_after_blocks) {
            return Ok(Outcome::Dropped { height });
        }
        sleep(s.poll_interval);
    }
    Err(SdkError::Chain(format!(
        "no confirmation after {} polls; the transaction may still land: check the nonce of {signer} before sending again",
        s.max_polls
    )))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct LockRecord {
    nonce: u128,
    height: u64,
}

/// One in-flight transaction per signer, across processes (spec §6.5 step 4).
#[derive(Debug)]
pub struct InFlightLock {
    path: PathBuf,
}

fn io_err(path: &Path, e: &std::io::Error) -> SdkError {
    SdkError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    }
}

impl InFlightLock {
    /// `n0`/`h0` are the signer's current nonce and the current height.
    pub fn acquire(
        dir: &Path,
        signer: &AccountId,
        n0: u128,
        h0: u64,
        s: &ConfirmSettings,
    ) -> SdkResult<Self> {
        fs::create_dir_all(dir).map_err(|e| io_err(dir, &e))?;
        let path = dir.join(format!("{signer}.lock"));
        if let Ok(text) = fs::read_to_string(&path) {
            if let Ok(r) = serde_json::from_str::<LockRecord>(&text) {
                let window_end = r.height.saturating_add(s.drop_after_blocks);
                let resolved = n0 > r.nonce || h0 >= window_end;
                if !resolved {
                    return Err(SdkError::InFlight {
                        signer: signer.to_string(),
                        lock: path.display().to_string(),
                        blocks: window_end.saturating_sub(h0),
                    });
                }
            }
            // Resolved, or torn by a crash: nothing can still be in flight under it.
            fs::remove_file(&path).map_err(|e| io_err(&path, &e))?;
        }
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    SdkError::InFlight {
                        signer: signer.to_string(),
                        lock: path.display().to_string(),
                        blocks: s.drop_after_blocks,
                    }
                } else {
                    io_err(&path, &e)
                }
            })?;
        let record = serde_json::to_string(&LockRecord { nonce: n0, height: h0 })
            .map_err(|e| SdkError::Io {
                path: path.display().to_string(),
                reason: e.to_string(),
            })?;
        f.write_all(record.as_bytes()).map_err(|e| io_err(&path, &e))?;
        Ok(Self { path })
    }
}

impl Drop for InFlightLock {
    fn drop(&mut self) {
        // A lock that cannot be removed here is recognised as resolved by the next `acquire` (the
        // nonce or the height has moved past it), so ignoring this error loses nothing.
        let _ = fs::remove_file(&self.path);
    }
}

pub fn submit<C: Chain + ?Sized>(
    chain: &C,
    keys: &dyn KeySource,
    plan: &Plan,
    lock_dir: &Path,
    s: &ConfirmSettings,
    sleep: &dyn Fn(Duration),
) -> SdkResult<Submitted> {
    let primary = *plan
        .signing_requirements
        .first()
        .ok_or_else(|| SdkError::Encode("plan has no signer".into()))?;
    let nonces = chain.nonces(&plan.signing_requirements)?;
    let n0 = nonces.first().map_or(0, |n| n.0);
    let h0 = chain.last_block_id()?;
    let _lock = InFlightLock::acquire(lock_dir, &primary, n0, h0, s)?;
    let tx = sign(plan, &nonces, keys)?;
    let hash = chain.send(&tx)?;
    let outcome = confirm(chain, &primary, n0, h0, s, sleep)?;
    Ok(Submitted { hash, outcome })
}
```

- [ ] **Step 4: Run the tests**

Run the Step 2 command. Expected: 5 passed.

- [ ] **Step 5: Docs, ADR-042, gates, commit**

`docs/architecture/sdk.md`: row `| confirm | 10 | nonce-polling confirmation (Included / Dropped after 5 blocks), one in-flight transaction per signer via a lock file, \`submit\` = lock + sign + send + confirm |` and a section "## Confirmation" restating the semantics above and that a Dropped transaction is never resent (re-quote first).

Append to `docs/design-log.md`:

```markdown

---

## ADR-042: Confirmation Without a Target-Account Check; Lock File for One In-Flight Transaction per Signer

**Context**
Spec §6.5: record the signer nonce `n0` and height `h0`; poll the nonce about every 2 s "checking that the target account changed"; `nonce > n0` is Included, still `n0` at `h0 + 5` is Dropped; one in-flight transaction per signer.

**Options**
- (A) Also compare a target account before and after.
- (B) Nonce only; per-signer lock file in the CLI state directory.
- (C) Nonce only; in-process lock only.

**Decision**
(B). On v0.2.4 the signer nonce advances only when the transaction is applied, so a nonce above `n0` already proves application; a target-account comparison adds a read and can be confused by unrelated writes to the same account. The lock is a file `<state>/locks/<signer>.lock` holding `{nonce, height}`, created with `create_new`; a lock whose nonce or height window has passed, or that does not parse, is stale and replaced, so a crashed CLI never blocks a signer for more than 5 blocks.

**Trade-offs**
Two CLIs with different state directories can still race the same signer (the second transaction is then dropped by the sequencer for a stale nonce and reported as Dropped). `poll_transaction_status` replaces all of this after the v0.3 port.

**Status**
Accepted (Plan 3a, Task 10)
```

Run the lint and format gates. Expected: clean.

Commit (controller — GPG signing on Windows):

```bash
git add crates/auction_sdk docs/architecture/sdk.md docs/design-log.md
git commit -m "feat(sdk): nonce-based confirmation with one in-flight transaction per signer"
```

---

### Task 11: Block scanner with a persisted cursor (spec §6.4, ADR-043)

**Files:**
- Create: `crates/auction_sdk/src/scan.rs`
- Modify: `crates/auction_sdk/src/lib.rs`, `docs/architecture/sdk.md`, `docs/design-log.md` (ADR-043)

**Interfaces:**
- Consumes: `Chain::{last_block_id, blocks}`, `BlockSummary`, `encode::{words, variant_index}`, `VerifiedConfig`.
- Produces (`auction_sdk::scan`): `Cursor { last_block: u64, last_hash: String, auctions: BTreeSet<String>, positions: BTreeSet<String> }` (serde; `Default` = genesis); `ScanTargets { engine_program, config, host_program, protocol_parameters }` with `ScanTargets::of(v: &VerifiedConfig)`; `ScanSettings { batch: u64, reorg_depth: u64 }` (default 100, 10); `scan(chain, t: &ScanTargets, cursor: &Cursor, s: &ScanSettings) -> SdkResult<Cursor>` (returns the new cursor; the input is never mutated, so a failed scan leaves the caller's cursor as it was); `load_cursor(path) -> SdkResult<Cursor>` (missing file → `Cursor::default()`, unparsable → `SdkError::Io`); `save_cursor(path, &Cursor) -> SdkResult<()>` (write `path.tmp`, then rename); `Cursor::auction_ids(&self) -> SdkResult<Vec<AccountId>>`, `Cursor::position_ids(&self) -> SdkResult<Vec<AccountId>>`.

Discovery rules: an engine transaction whose first word is `Liquidate`'s enum index and whose account 2 is our config → account 6 is an auction, account 4 a position; a host transaction listing our `ProtocolParameters` whose first word is `OpenPosition`'s enum index → account 1 is a position. Reorg/restart rule: if the block at `cursor.last_block` no longer has `cursor.last_hash`, rewind to `last_block − reorg_depth` (floor 0) and rescan; discoveries are sets, so rescanning never duplicates. Within a scan every block must extend the previous one (`prev_hash`), and the node must return every requested block; otherwise the scan fails and the cursor is not advanced.

- [ ] **Step 1: Write the failing tests (Review Focus 5)**

Create `crates/auction_sdk/src/scan.rs` with its tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::standard;
    use crate::verify::verify_config;

    fn setup() -> (standard::Fixture, ScanTargets) {
        let fx = standard::standard();
        let v = verify_config(&fx.chain, &fx.deployment, &standard::config_id()).expect("verifies");
        (fx, ScanTargets::of(&v))
    }

    #[test]
    fn scanner_finds_the_standard_auction_and_positions_incrementally() {
        let (fx, t) = setup();
        let c = scan(&fx.chain, &t, &Cursor::default(), &ScanSettings::default()).expect("scan");
        assert_eq!(c.last_block, 3);
        assert_eq!(c.auction_ids().expect("ids"), vec![fx.live_auction]);
        let mut positions = c.position_ids().expect("ids");
        positions.sort();
        let mut expected = vec![fx.unsafe_position, fx.auction_position];
        expected.sort();
        assert_eq!(positions, expected);
        let again = scan(&fx.chain, &t, &c, &ScanSettings::default()).expect("no new blocks");
        assert_eq!(again, c);
        let small = ScanSettings { batch: 1, ..ScanSettings::default() };
        assert_eq!(scan(&fx.chain, &t, &Cursor::default(), &small).expect("batch 1"), c);
    }

    #[test]
    fn scanner_rewinds_when_the_cursor_block_hash_changed() {
        let (fx, t) = setup();
        let c = scan(&fx.chain, &t, &Cursor::default(), &ScanSettings::default()).expect("scan");
        fx.chain.rewrite_from(2, 7); // the node rebuilt blocks 2..=3 (e.g. a restarted localnet)
        let after = scan(&fx.chain, &t, &c, &ScanSettings::default()).expect("rescan");
        assert_ne!(after.last_hash, c.last_hash);
        assert_eq!(after.auctions, c.auctions, "nothing missed, nothing duplicated");
        assert_eq!(after.last_block, 3);
    }

    #[test]
    fn scanner_leaves_the_cursor_unchanged_on_a_missing_block() {
        let (fx, t) = setup();
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("cursor.json");
        save_cursor(&path, &Cursor::default()).expect("save");
        fx.chain.hide_block(2);
        let start = load_cursor(&path).expect("load");
        let e = scan(&fx.chain, &t, &start, &ScanSettings::default()).expect_err("gap");
        assert!(matches!(e, SdkError::Chain(m) if m.contains("block 2")));
        assert_eq!(load_cursor(&path).expect("load"), Cursor::default(), "not advanced");
    }

    #[test]
    fn cursor_write_is_atomic_and_a_corrupt_cursor_is_an_error() {
        let (fx, t) = setup();
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("cursor.json");
        assert_eq!(load_cursor(&path).expect("missing = genesis"), Cursor::default());
        let c = scan(&fx.chain, &t, &Cursor::default(), &ScanSettings::default()).expect("scan");
        save_cursor(&path, &c).expect("save");
        assert_eq!(load_cursor(&path).expect("round trip"), c);
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .expect("dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["cursor.json".to_owned()], "no temporary file left behind");
        std::fs::write(&path, "{ half").expect("write");
        let e = load_cursor(&path).expect_err("corrupt");
        assert!(matches!(e, SdkError::Io { ref path, .. } if path.ends_with("cursor.json")));
    }
}
```

Add `pub mod scan;` to `lib.rs`.

- [ ] **Step 2: Run to verify they fail**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_sdk --features testing scan'`
Expected: FAIL to compile (`cannot find type Cursor`).

- [ ] **Step 3: Implement (above the tests)**

```rust
//! Discovery by block scan (spec §6.4, ADR-008, ADR-043): no account-listing RPC and no events exist
//! on v0.2.4. The scan keeps engine `liquidate` transactions that name our config (auction and
//! position ids) and host `open_position` transactions that name our `ProtocolParameters`
//! (position ids), and persists a cursor `{last_block, last_hash, auctions, positions}`.

use std::collections::BTreeSet;
use std::path::Path;

use auction_engine_core::Instruction;
use lee_core::{account::AccountId, program::ProgramId};
use serde::{Deserialize, Serialize};

use crate::{
    chain::{BlockSummary, Chain},
    encode::{variant_index, words},
    error::{SdkError, SdkResult},
    verify::VerifiedConfig,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub last_block: u64,
    /// Hex hash of `last_block` ("" before the first scan).
    pub last_hash: String,
    pub auctions: BTreeSet<String>,
    pub positions: BTreeSet<String>,
}

fn parse_ids(set: &BTreeSet<String>) -> SdkResult<Vec<AccountId>> {
    set.iter()
        .map(|s| {
            s.parse::<AccountId>()
                .map_err(|_| SdkError::Scan(format!("cursor holds an invalid account id {s}")))
        })
        .collect()
}

impl Cursor {
    pub fn auction_ids(&self) -> SdkResult<Vec<AccountId>> {
        parse_ids(&self.auctions)
    }

    pub fn position_ids(&self) -> SdkResult<Vec<AccountId>> {
        parse_ids(&self.positions)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanTargets {
    pub engine_program: ProgramId,
    pub config: AccountId,
    pub host_program: ProgramId,
    pub protocol_parameters: AccountId,
}

impl ScanTargets {
    #[must_use]
    pub fn of(v: &VerifiedConfig) -> Self {
        Self {
            engine_program: v.engine.program,
            config: v.config_id,
            host_program: v.host.program,
            protocol_parameters: v.host.protocol_parameters,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanSettings {
    pub batch: u64,
    pub reorg_depth: u64,
}

impl Default for ScanSettings {
    fn default() -> Self {
        Self {
            batch: 100,
            reorg_depth: 10,
        }
    }
}

fn index_of<T: serde::Serialize>(ix: &T) -> SdkResult<u32> {
    variant_index(&words(ix)?).ok_or_else(|| SdkError::Encode("empty instruction".into()))
}

fn collect(t: &ScanTargets, b: &BlockSummary, next: &mut Cursor, liquidate: u32, open: u32) {
    for tx in &b.txs {
        let kind = variant_index(&tx.instruction_words);
        if tx.program_id == t.engine_program
            && kind == Some(liquidate)
            && tx.account_ids.get(2) == Some(&t.config)
        {
            if let Some(a) = tx.account_ids.get(6) {
                next.auctions.insert(a.to_string());
            }
            if let Some(p) = tx.account_ids.get(4) {
                next.positions.insert(p.to_string());
            }
        }
        if tx.program_id == t.host_program
            && kind == Some(open)
            && tx.account_ids.contains(&t.protocol_parameters)
        {
            if let Some(p) = tx.account_ids.get(1) {
                next.positions.insert(p.to_string());
            }
        }
    }
}

fn hash_at<C: Chain + ?Sized>(chain: &C, id: u64) -> SdkResult<String> {
    if id == 0 {
        return Ok(String::new());
    }
    let b = chain.blocks(id, id)?;
    b.first()
        .map(|b| hex::encode(b.hash))
        .ok_or_else(|| SdkError::Chain(format!("block {id} not found")))
}

pub fn scan<C: Chain + ?Sized>(
    chain: &C,
    t: &ScanTargets,
    cursor: &Cursor,
    s: &ScanSettings,
) -> SdkResult<Cursor> {
    let liquidate = index_of(&Instruction::Liquidate { nonce: 0 })?;
    let open = index_of(&stablecoin_core::Instruction::OpenPosition {
        position_nonce: 0,
        initial_collateral_amount: 0,
    })?;
    let head = chain.last_block_id()?;
    let mut next = cursor.clone();
    if cursor.last_block > 0 && hash_at(chain, cursor.last_block)? != cursor.last_hash {
        // The node no longer has the block we stopped at: rewind and rescan (sets never duplicate).
        next.last_block = cursor.last_block.saturating_sub(s.reorg_depth);
        next.last_hash = hash_at(chain, next.last_block)?;
    }
    let batch = s.batch.max(1);
    let mut from = next.last_block + 1;
    while from <= head {
        let to = head.min(from.saturating_add(batch - 1));
        let blocks = chain.blocks(from, to)?;
        let expected = usize::try_from(to - from + 1).map_err(|_| SdkError::Scan("range too large".into()))?;
        if blocks.len() != expected {
            return Err(SdkError::Scan(format!(
                "the node returned {} blocks for {from}..={to}",
                blocks.len()
            )));
        }
        for (i, b) in blocks.iter().enumerate() {
            let want = from + u64::try_from(i).map_err(|_| SdkError::Scan("index".into()))?;
            if b.id != want {
                return Err(SdkError::Scan(format!("expected block {want}, got {}", b.id)));
            }
            if !next.last_hash.is_empty() && hex::encode(b.prev_hash) != next.last_hash {
                return Err(SdkError::Scan(format!(
                    "block {} does not extend the scanned chain; run `rescan`",
                    b.id
                )));
            }
            collect(t, b, &mut next, liquidate, open);
            next.last_block = b.id;
            next.last_hash = hex::encode(b.hash);
        }
        from = to + 1;
    }
    Ok(next)
}

pub fn load_cursor(path: &Path) -> SdkResult<Cursor> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Cursor::default()),
        Err(e) => {
            return Err(SdkError::Io {
                path: path.display().to_string(),
                reason: e.to_string(),
            })
        }
    };
    serde_json::from_str(&text).map_err(|e| SdkError::Io {
        path: path.display().to_string(),
        reason: format!("corrupt cursor ({e}); delete it or run `rescan --from 1`"),
    })
}

/// Atomic: write `<path>.tmp`, then rename over `path`.
pub fn save_cursor(path: &Path, c: &Cursor) -> SdkResult<()> {
    let io = |e: std::io::Error| SdkError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(c).map_err(|e| SdkError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    std::fs::write(&tmp, text).map_err(io)?;
    std::fs::rename(&tmp, path).map_err(io)
}
```

- [ ] **Step 4: Run the tests**

Run the Step 2 command. Expected: 4 passed.

- [ ] **Step 5: Docs, ADR-043, gates, commit**

`docs/architecture/sdk.md`: row `| scan | 11 | block scan with a persisted, atomically written cursor; rewinds when the cursor's block hash changed; fails (cursor unchanged) on a missing or non-extending block |` and a section "## Discovery" with the rules above.

Append to `docs/design-log.md`:

```markdown

---

## ADR-043: Scanner Cursor Stores the Block Hash and the Discoveries

**Context**
Spec §6.4: positions and auctions are found by a `getBlockRange` scan with a persisted cursor ("Scanned to block N · Rescan"). A localnet can be restarted (block N then has another hash), the node can fail to return a block, and the process can die mid-write.

**Options**
- (A) Cursor = last block number only; discoveries recomputed each run.
- (B) Cursor = `{last_block, last_hash, auctions, positions}`; rewind `reorg_depth` blocks when the stored hash no longer matches; atomic tmp+rename writes.

**Decision**
(B). Discoveries are sets of ids, so a rescan never duplicates; list commands read each discovered account and drop ids that no longer decode. A missing or non-extending block is an error and the cursor is not advanced.

**Trade-offs**
A rewind deeper than `reorg_depth` (10) needs `rescan --from 1`. Ids discovered in rebuilt blocks that no longer exist stay in the set until they fail to decode (harmless). Production uses the indexer's `getTransactionsByAccount`, then v0.3 events (spec §6.4).

**Status**
Accepted (Plan 3a, Task 11)
```

Run the lint and format gates. Expected: clean.

Commit (controller — GPG signing on Windows):

```bash
git add crates/auction_sdk docs/architecture/sdk.md docs/design-log.md
git commit -m "feat(sdk): block scanner with a persisted, rewinding cursor"
```

---

### Task 12: `RpcChain`: the sequencer JSON-RPC implementation of `Chain` (feature `rpc`)

**Files:**
- Create: `crates/auction_sdk/src/rpc.rs`
- Modify: `crates/auction_sdk/Cargo.toml`, `crates/auction_sdk/src/lib.rs`, `docs/architecture/sdk.md`

**Interfaces:**
- Consumes: LEZ v0.2.4 `sequencer_service_rpc` (`RpcClient` trait, `SequencerClient`, `SequencerClientBuilder`, `ClientError`; methods `get_account`, `get_accounts_nonces`, `get_last_block_id`, `get_block_range`, `send_transaction`), `common::{block::Block, transaction::LeeTransaction}`.
- Produces: `auction_sdk::rpc::{RpcChain, summarize}`; `RpcChain::connect(url: &str) -> SdkResult<RpcChain>`; `impl Chain for RpcChain`; `summarize(&Block) -> BlockSummary` (public transactions only).

- [ ] **Step 1: Dependencies and feature**

In `crates/auction_sdk/Cargo.toml` add to `[features]`:

```toml
# Sequencer JSON-RPC client (LEZ v0.2.4 `sequencer_service_rpc` + `common`, current-thread tokio).
rpc = ["dep:sequencer_service_rpc", "dep:common", "dep:tokio"]
```

and to `[dependencies]`:

```toml
sequencer_service_rpc = { git = "https://github.com/logos-blockchain/logos-execution-zone.git", tag = "v0.2.4", features = ["client"], optional = true }
common = { git = "https://github.com/logos-blockchain/logos-execution-zone.git", tag = "v0.2.4", optional = true }
tokio = { version = "1", features = ["rt", "net", "time"], optional = true }
```

In `lib.rs` add:

```rust
#[cfg(feature = "rpc")]
pub mod rpc;
```

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh check -p auction_sdk --features rpc'`
Expected after Step 3: compiles. (`common` is already in `Cargo.lock` through the LEZ crates; `jsonrpsee 0.26` arrives with `sequencer_service_rpc`.) If cargo reports a version conflict between the LEZ workspace's pins and ours, stop and report it: do not change any pin.

- [ ] **Step 2: Write the tests**

Create `crates/auction_sdk/src/rpc.rs` with its tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_rejects_a_malformed_url() {
        assert!(matches!(RpcChain::connect("not a url"), Err(SdkError::Chain(_))));
    }

    /// Needs a running localnet sequencer (Task 16): `LEZ_RPC_URL=http://127.0.0.1:3040 cargo test -p
    /// auction_sdk --features rpc -- --ignored live_rpc`.
    #[test]
    #[ignore = "needs a running sequencer at LEZ_RPC_URL"]
    fn live_rpc_reads_height_clock_and_blocks() {
        let url = std::env::var("LEZ_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:3040".into());
        let c = RpcChain::connect(&url).expect("connect");
        let head = c.last_block_id().expect("height");
        assert!(head >= 1);
        assert!(crate::chain::clock_ms(&c).expect("clock") > 0);
        let b = c.blocks(head, head).expect("block");
        assert_eq!(b[0].id, head);
    }
}
```

- [ ] **Step 3: Implement (above the tests)**

```rust
//! `Chain` over the LEZ v0.2.4 sequencer JSON-RPC (`sequencer_service_rpc`, feature `rpc`). Each
//! `RpcChain` owns a current-thread tokio runtime and blocks on every call (ADR-040).

use std::future::Future;

use common::{block::Block, transaction::LeeTransaction};
use lee::PublicTransaction;
use lee_core::account::{Account, AccountId, Nonce};
use sequencer_service_rpc::{ClientError, RpcClient as _, SequencerClient, SequencerClientBuilder};

use crate::{
    chain::{BlockSummary, Chain, TxSummary},
    error::{SdkError, SdkResult},
};

pub struct RpcChain {
    client: SequencerClient,
    rt: tokio::runtime::Runtime,
    url: String,
}

impl std::fmt::Debug for RpcChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcChain").field("url", &self.url).finish()
    }
}

impl RpcChain {
    pub fn connect(url: &str) -> SdkResult<Self> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| SdkError::Chain(format!("tokio runtime: {e}")))?;
        let client = {
            let _guard = rt.enter();
            SequencerClientBuilder::default()
                .build(url)
                .map_err(|e| SdkError::Chain(format!("{url}: {e}")))?
        };
        Ok(Self {
            client,
            rt,
            url: url.to_owned(),
        })
    }

    fn call<T, F>(&self, what: &str, f: F) -> SdkResult<T>
    where
        F: Future<Output = Result<T, ClientError>>,
    {
        self.rt
            .block_on(f)
            .map_err(|e| SdkError::Chain(format!("{what} at {}: {e}", self.url)))
    }
}

/// The public transactions of a block (privacy-preserving and deployment transactions are skipped).
#[must_use]
pub fn summarize(b: &Block) -> BlockSummary {
    BlockSummary {
        id: b.header.block_id,
        hash: b.header.hash.0,
        prev_hash: b.header.prev_block_hash.0,
        timestamp_ms: b.header.timestamp,
        txs: b
            .body
            .transactions
            .iter()
            .filter_map(|t| match t {
                LeeTransaction::Public(p) => Some(TxSummary {
                    hash: p.hash(),
                    program_id: p.message.program_id,
                    account_ids: p.message.account_ids.clone(),
                    instruction_words: p.message.instruction_data.clone(),
                }),
                LeeTransaction::PrivacyPreserving(_) | LeeTransaction::ProgramDeployment(_) => None,
            })
            .collect(),
    }
}

impl Chain for RpcChain {
    fn account(&self, id: &AccountId) -> SdkResult<Account> {
        self.call("getAccount", self.client.get_account(*id))
    }

    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<Nonce>> {
        self.call("getAccountsNonces", self.client.get_accounts_nonces(ids.to_vec()))
    }

    fn last_block_id(&self) -> SdkResult<u64> {
        self.call("getLastBlockId", self.client.get_last_block_id())
    }

    fn blocks(&self, from: u64, to: u64) -> SdkResult<Vec<BlockSummary>> {
        let blocks = self.call("getBlockRange", self.client.get_block_range(from, to))?;
        Ok(blocks.iter().map(summarize).collect())
    }

    fn send(&self, tx: &PublicTransaction) -> SdkResult<[u8; 32]> {
        let hash = self.call(
            "sendTransaction",
            self.client.send_transaction(LeeTransaction::Public(tx.clone())),
        )?;
        Ok(hash.0)
    }
}
```

The two non-public kinds are listed explicitly (no wildcard), so a new transaction kind upstream is a compile error here, not a silent skip: write the last arm as `LeeTransaction::PrivacyPreserving(_) | LeeTransaction::ProgramDeployment(_) => None`.

- [ ] **Step 4: Run**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_sdk --features rpc,testing'`
Expected: all SDK tests pass, `live_rpc_reads_height_clock_and_blocks` reported `ignored`.

- [ ] **Step 5: Docs, gates, commit**

`docs/architecture/sdk.md`: row `| rpc | 12 | RpcChain over sequencer_service_rpc (getAccount, getAccountsNonces, getLastBlockId, getBlockRange, sendTransaction); blocks summarised to their public transactions |`.

Run the lint gate (it builds the `rpc` feature through the CLI from Task 13 on; here also run `RISC0_SKIP_BUILD=1 scripts/wsl-cargo.sh clippy -p auction_sdk --features rpc,testing --all-targets -- -D warnings`) and the format gate. Expected: clean.

Commit (controller — GPG signing on Windows):

```bash
git add crates/auction_sdk Cargo.lock docs/architecture/sdk.md
git commit -m "feat(sdk): RpcChain over the LEZ v0.2.4 sequencer JSON-RPC"
```

---

### Task 13: `auction_cli` scaffold, read-only commands and the golden harness (ADR-044)

**Files:**
- Modify: `Cargo.toml` (member `cli`, workspace dependency `auction_cli`)
- Create: `cli/Cargo.toml`, `cli/src/{main.rs,lib.rs,args.rs,env.rs,error.rs,output.rs,read_cmds.rs}`, `cli/tests/golden.rs`, `cli/tests/golden/*.txt` (generated), `cli/README.md`
- Modify: `docs/design-log.md` (ADR-044), `README.md`

**Interfaces:**
- Consumes: the whole SDK (`verify_config`, `quote::*`, `scan::*`, `chain::clock_ms`, `keystore::Keystore`, `confirm::ConfirmSettings`, `rpc::RpcChain`, `testing::{standard, write_keystore, STANDARD_LABELS}`).
- Produces (later CLI tasks extend these):
  - `auction_cli::args::{Cli, Command}` (clap derive; global options `--deployment` (default `deployments/localnet.json`), `--config`, `--rpc`, `--wallet-home`, `--state-dir` (default `.auction-cli`), `--at-ms`).
  - `auction_cli::env::Env<C: Chain> { chain: C, deployment: Deployment, config: Option<AccountId>, wallet_home: Option<PathBuf>, state_dir: PathBuf, at_ms: Option<u64>, confirm: ConfirmSettings, sleep: fn(Duration) }` with `config_id()`, `verified()`, `now()`, `keystore()`, `account(arg: &str)`, `is_fresh(id)`, `cursor_path(config)`, `lock_dir()`, `send(plan, what, out)` (Task 14 uses `send`).
  - `auction_cli::error::CliError::{Usage(String), Verify(SdkError), Dropped { height }, Keystore(KeystoreError), Sdk(SdkError), Output(String)}` with `exit_code() -> u8` (S4: 1, 2, 3, 4) and `From<SdkError>` (keystore errors → `Keystore`, `Mismatch` → `Verify`).
  - `auction_cli::output::{kv, bps, yes_no}`: `kv(out, key, value)` writes `format!("{key:<17} {value}")` (always at least one space, so scripts can take the last field); `bps(ray)` formats a RAY discount as `N.NN bps`.
  - `auction_cli::run<C: Chain>(cmd: &Command, env: &Env<C>, out: &mut dyn Write) -> Result<(), CliError>`.
  - Commands: `quote-liquidation --position`, `quote-bid --auction --coin-in-max [--slippage-bps 50]`, `list-auctions [--all]`, `list-at-risk`, `rescan [--from N]`, `watch [--interval-ms 15000] [--iterations N]`. Account arguments accept a base58 id or a wallet label.
  - Golden harness `cli/tests/golden.rs`: `Case::new()`, `Case::run(&self, args) -> String` (stdout + `error:`/`exit:` lines, temp paths → `<tmp>`, transaction hashes → `<hash>`), `check(name, &text)` (compares with `cli/tests/golden/<name>.txt`; `UPDATE_GOLDEN=1` rewrites), `value(text, key) -> &str`.

- [ ] **Step 1: Crate manifest**

Root `Cargo.toml`: add `"cli",` to `members` and `auction_cli = { path = "cli" }` to `[workspace.dependencies]`.

`cli/Cargo.toml`:

```toml
[package]
name = "auction_cli"
version = "0.1.0"
edition.workspace = true
license.workspace = true
description = "Command-line client of the RFP-014 auction engine (spec §6.6)."

[[bin]]
name = "auction-cli"
path = "src/main.rs"

[dependencies]
auction_sdk = { workspace = true, features = ["rpc"] }
auction_core = { workspace = true }
auction_engine_core = { workspace = true }
lee_core = { workspace = true }
stablecoin_core = { workspace = true }
clap = { version = "4", features = ["derive"] }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = "2"
hex = "0.4"

[dev-dependencies]
auction_sdk = { workspace = true, features = ["rpc", "testing"] }
risc0-zkvm = { workspace = true }
tempfile = "3"
```

- [ ] **Step 2: Write the failing golden tests**

Create `cli/tests/golden.rs`:

```rust
//! Golden tests (spec §9 "CLI golden tests"): every command against the SDK's standard fixture.
//! Regenerate with `UPDATE_GOLDEN=1` and review the diff; never edit a golden file by hand to make a
//! test pass. Each test also asserts its key numbers independently of the golden file.

use std::path::{Path, PathBuf};
use std::time::Duration;

use auction_cli::{args::Cli, env::Env, run};
use auction_sdk::{
    confirm::ConfirmSettings,
    deployment::Deployment,
    testing::{
        standard::{self, Fixture},
        write_keystore, STANDARD_LABELS,
    },
};
use clap::Parser;

pub struct Case {
    pub fx: Fixture,
    pub dir: tempfile::TempDir,
    pub with_wallet: bool,
    pub deployment: Option<Deployment>,
}

impl Case {
    pub fn new() -> Self {
        Self {
            fx: standard::standard(),
            dir: tempfile::tempdir().expect("tmp"),
            with_wallet: true,
            deployment: None,
        }
    }

    pub fn run(&self, args: &[&str]) -> String {
        let wallet = self.dir.path().join("wallet");
        if self.with_wallet {
            write_keystore(&wallet, &STANDARD_LABELS).expect("wallet");
        }
        let mut argv = vec!["auction-cli"];
        argv.extend_from_slice(args);
        let cli = Cli::try_parse_from(argv).expect("arguments parse");
        let env = Env {
            chain: &self.fx.chain,
            deployment: self.deployment.clone().unwrap_or_else(|| self.fx.deployment.clone()),
            config: cli.config.as_deref().map(|s| s.parse().expect("config id")),
            wallet_home: Some(wallet),
            state_dir: self.dir.path().join("state"),
            at_ms: cli.at_ms,
            confirm: ConfirmSettings {
                poll_interval: Duration::ZERO,
                drop_after_blocks: 5,
                max_polls: 50,
            },
            sleep: |_| {},
        };
        let mut out = Vec::new();
        let result = run(&cli.command, &env, &mut out);
        let mut text = String::from_utf8(out).expect("utf-8");
        match result {
            Ok(()) => text.push_str("exit: 0\n"),
            Err(e) => text.push_str(&format!("error: {e}\nexit: {}\n", e.exit_code())),
        }
        normalise(&text, self.dir.path())
    }
}

fn normalise(text: &str, tmp: &Path) -> String {
    let replaced = text.replace(&tmp.display().to_string(), "<tmp>");
    let mut lines: Vec<String> = replaced
        .lines()
        .map(|l| {
            if l.starts_with("submitted") {
                format!("{:<17} <hash>", "submitted")
            } else {
                l.to_owned()
            }
        })
        .collect();
    lines.push(String::new());
    lines.join("\n")
}

pub fn check(name: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.txt"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("golden dir");
        std::fs::write(&path, actual).expect("write golden");
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing {}: run with UPDATE_GOLDEN=1 and review it", path.display()));
    assert_eq!(actual, expected, "golden {name} changed");
}

pub fn value<'a>(text: &'a str, key: &str) -> &'a str {
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .map(str::trim)
        .unwrap_or_else(|| panic!("no line {key:?} in\n{text}"))
}

#[test]
fn quote_liquidation() {
    let c = Case::new();
    let p = c.fx.unsafe_position.to_string();
    let out = c.run(&["quote-liquidation", "--position", &p]);
    assert_eq!(value(&out, "unsafe (min CR)"), "yes");
    assert_eq!(value(&out, "amount to raise"), "660000");
    assert_eq!(value(&out, "keeper reward"), "25");
    assert_eq!(value(&out, "lot "), "475000");
    assert_eq!(value(&out, "nonce"), "0");
    assert!(out.ends_with("exit: 0\n"));
    check("quote_liquidation", &out);
}

#[test]
fn quote_bid() {
    let c = Case::new();
    let a = c.fx.live_auction.to_string();
    let out = c.run(&["quote-bid", "--auction", &a, "--coin-in-max", "100000"]);
    assert_eq!(value(&out, "t_eff (ms)"), "600000");
    assert_eq!(value(&out, "collateral out"), "51963");
    assert_eq!(value(&out, "min out (50 bps)"), "51703");
    assert_eq!(value(&out, "instruction"), "bid");
    check("quote_bid", &out);
}

#[test]
fn quote_bid_of_a_non_auction_account_is_refused() {
    // A host-owned account where an engine Auction is expected: the decoder's owner check is a
    // mismatch, reported like any verification failure (exit 2), before anything is quoted.
    let c = Case::new();
    let p = c.fx.unsafe_position.to_string();
    let out = c.run(&["quote-bid", "--auction", &p, "--coin-in-max", "1"]);
    assert!(out.contains("verification failed: Auction"), "{out}");
    assert!(out.ends_with("exit: 2\n"), "{out}");
    check("quote_bid_not_an_auction", &out);
}

#[test]
fn list_auctions() {
    let c = Case::new();
    let out = c.run(&["list-auctions"]);
    assert_eq!(value(&out, "scanned to block"), "3");
    assert!(out.contains(&format!("{}  live  lot 475000", c.fx.live_auction)));
    assert!(!out.contains("held at max discount"));
    check("list_auctions", &out);
}

#[test]
fn list_auctions_shows_an_auction_held_at_max_discount() {
    let c = Case::new();
    let later = (standard::T0 + 2_100_000).to_string();
    let out = c.run(&["--at-ms", &later, "list-auctions"]);
    assert!(out.contains("held at max discount (keeps its cap reservation of 660000)"), "{out}");
    check("list_auctions_held_at_max", &out);
}

#[test]
fn list_at_risk() {
    let c = Case::new();
    let out = c.run(&["list-at-risk"]);
    assert!(out.contains(&format!("{}  owner", c.fx.unsafe_position)));
    assert!(!out.contains(&c.fx.auction_position.to_string()), "zero-debt position not listed");
    check("list_at_risk", &out);
}

#[test]
fn rescan_from_the_first_block() {
    let c = Case::new();
    let out = c.run(&["rescan", "--from", "1"]);
    assert_eq!(value(&out, "scanned to block"), "3");
    assert_eq!(value(&out, "auctions"), "1");
    assert_eq!(value(&out, "positions"), "2");
    check("rescan", &out);
}

#[test]
fn watch_two_rounds() {
    let c = Case::new();
    let out = c.run(&["watch", "--iterations", "2", "--interval-ms", "0"]);
    assert_eq!(out.matches("scanned to block").count(), 2);
    check("watch", &out);
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_cli'`
Expected: FAIL to compile (`unresolved import auction_cli::args`).

- [ ] **Step 4: `error.rs`, `output.rs`, `env.rs`**

`cli/src/error.rs`:

```rust
//! CLI errors and exit codes (Shared Reference S4, ADR-044).

use auction_sdk::{error::SdkError, keystore::KeystoreError};

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("{0}")]
    Usage(String),
    #[error("verification failed: {0}")]
    Verify(SdkError),
    #[error("transaction dropped (not applied by block {height}): nothing was applied; re-quote and resend (spec §6.5)")]
    Dropped { height: u64 },
    #[error("keystore: {0}")]
    Keystore(KeystoreError),
    #[error("{0}")]
    Sdk(SdkError),
    #[error("cannot write output: {0}")]
    Output(String),
}

impl CliError {
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) | Self::Sdk(_) | Self::Output(_) => 1,
            Self::Verify(_) => 2,
            Self::Dropped { .. } => 3,
            Self::Keystore(_) => 4,
        }
    }
}

impl From<SdkError> for CliError {
    fn from(e: SdkError) -> Self {
        match e {
            SdkError::Keystore(k) => Self::Keystore(k),
            e @ SdkError::Mismatch { .. } => Self::Verify(e),
            other => Self::Sdk(other),
        }
    }
}

impl From<KeystoreError> for CliError {
    fn from(e: KeystoreError) -> Self {
        Self::Keystore(e)
    }
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        Self::Output(e.to_string())
    }
}
```

`cli/src/output.rs`:

```rust
//! Plain `key  value` output, one item per line, stable for golden tests.

use std::io::Write;

use auction_core::units::BPS_TO_RAY;

use crate::error::CliError;

pub fn kv(out: &mut dyn Write, key: &str, value: &str) -> Result<(), CliError> {
    writeln!(out, "{key:<17} {value}")?;
    Ok(())
}

/// A RAY discount as basis points with two decimals (floor).
#[must_use]
pub fn bps(ray: u128) -> String {
    let whole = ray / BPS_TO_RAY;
    let hundredths = (ray % BPS_TO_RAY) * 100 / BPS_TO_RAY;
    format!("{whole}.{hundredths:02} bps")
}

#[must_use]
pub fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bps_formats_ray_discounts() {
        assert_eq!(bps(2 * BPS_TO_RAY * 100), "200.00 bps");
        assert_eq!(bps(37_777_777_777_777_777_777_777_777), "377.77 bps");
        assert_eq!(bps(0), "0.00 bps");
    }
}
```

`cli/src/env.rs`:

```rust
//! Everything a command needs: the chain, the trusted deployment, the config, the wallet, the state
//! directory (scan cursor, in-flight locks) and the confirmation settings.

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use auction_sdk::{
    chain::{clock_ms, Chain},
    confirm::{submit, ConfirmSettings, Outcome},
    deployment::Deployment,
    keystore::Keystore,
    plan::Plan,
    verify::{verify_config, VerifiedConfig},
};
use lee_core::account::{Account, AccountId};

use crate::{error::CliError, output::kv};

pub struct Env<C: Chain> {
    pub chain: C,
    pub deployment: Deployment,
    /// `--config`; falls back to the deployment file's `config`.
    pub config: Option<AccountId>,
    pub wallet_home: Option<PathBuf>,
    pub state_dir: PathBuf,
    /// `--at-ms`: evaluate quotes at this clock instead of `CLOCK_01`.
    pub at_ms: Option<u64>,
    pub confirm: ConfirmSettings,
    pub sleep: fn(Duration),
}

impl<C: Chain> Env<C> {
    pub fn config_id(&self) -> Result<AccountId, CliError> {
        self.config.or(self.deployment.config).ok_or_else(|| {
            CliError::Usage(
                "no engine config: pass --config or set \"config\" in the deployment file".into(),
            )
        })
    }

    /// I4 / ADR-027: every command that uses a config verifies it first.
    pub fn verified(&self) -> Result<VerifiedConfig, CliError> {
        Ok(verify_config(&self.chain, &self.deployment, &self.config_id()?)?)
    }

    pub fn now(&self) -> Result<u64, CliError> {
        match self.at_ms {
            Some(t) => Ok(t),
            None => Ok(clock_ms(&self.chain)?),
        }
    }

    pub fn keystore(&self) -> Result<Keystore, CliError> {
        let path = Keystore::default_path(self.wallet_home.as_deref())?;
        Ok(Keystore::open(&path)?)
    }

    /// A base58 account id, or a wallet label (which needs the keystore).
    pub fn account(&self, arg: &str) -> Result<AccountId, CliError> {
        if let Ok(id) = arg.parse::<AccountId>() {
            return Ok(id);
        }
        Ok(self.keystore()?.resolve(arg)?)
    }

    pub fn is_fresh(&self, id: &AccountId) -> Result<bool, CliError> {
        Ok(self.chain.account(id)? == Account::default())
    }

    #[must_use]
    pub fn cursor_path(&self, config: &AccountId) -> PathBuf {
        self.state_dir.join(format!("{config}.cursor.json"))
    }

    #[must_use]
    pub fn lock_dir(&self) -> PathBuf {
        self.state_dir.join("locks")
    }

    /// Lock, sign with the wallet, send, confirm (spec §6.5). Dropped is an error (exit 3).
    pub fn send(&self, plan: &Plan, what: &str, out: &mut dyn Write) -> Result<(), CliError> {
        let keystore = self.keystore()?;
        kv(
            out,
            "plan",
            &format!(
                "{what} ({} accounts, {} signer(s))",
                plan.account_ids.len(),
                plan.signing_requirements.len()
            ),
        )?;
        let s = submit(&self.chain, &keystore, plan, &self.lock_dir(), &self.confirm, &self.sleep)?;
        kv(out, "submitted", &hex::encode(s.hash))?;
        match s.outcome {
            Outcome::Included { nonce_after } => kv(out, "included", &format!("signer nonce {nonce_after}")),
            Outcome::Dropped { height } => Err(CliError::Dropped { height }),
        }
    }
}
```

- [ ] **Step 5: `args.rs`, `read_cmds.rs`, `lib.rs`, `main.rs`**

`cli/src/args.rs`:

```rust
//! Command line (spec §6.6). Account arguments take a base58 id or a wallet label.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "auction-cli",
    version,
    about = "RFP-014 auction engine CLI (LEZ v0.2.4, public transactions only)"
)]
pub struct Cli {
    /// Deployment file with the trusted ImageIDs (I4).
    #[arg(long, global = true, default_value = "deployments/localnet.json")]
    pub deployment: PathBuf,
    /// Engine config account (overrides the deployment file).
    #[arg(long, global = true)]
    pub config: Option<String>,
    /// Sequencer URL (overrides the deployment file).
    #[arg(long, global = true)]
    pub rpc: Option<String>,
    /// Wallet home holding storage.json (default: $LEE_WALLET_HOME_DIR).
    #[arg(long, global = true)]
    pub wallet_home: Option<PathBuf>,
    /// Directory for the scan cursor and the in-flight locks.
    #[arg(long, global = true, default_value = ".auction-cli")]
    pub state_dir: PathBuf,
    /// Evaluate quotes at this clock (ms) instead of the chain's CLOCK_01.
    #[arg(long, global = true)]
    pub at_ms: Option<u64>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Quote the liquidation of a position (no transaction).
    QuoteLiquidation {
        #[arg(long)]
        position: String,
    },
    /// Quote a bid (no transaction).
    QuoteBid {
        #[arg(long)]
        auction: String,
        #[arg(long)]
        coin_in_max: u128,
        #[arg(long, default_value_t = 50)]
        slippage_bps: u32,
    },
    /// Scan new blocks, then list live auctions (settled ones too with --all).
    ListAuctions {
        #[arg(long)]
        all: bool,
    },
    /// Scan new blocks, then list positions unsafe at the host's minimum ratio.
    ListAtRisk,
    /// Rescan from a block (default: continue from the cursor).
    Rescan {
        #[arg(long)]
        from: Option<u64>,
    },
    /// Refresh periodically: scan and list auctions.
    Watch {
        #[arg(long, default_value_t = 15_000)]
        interval_ms: u64,
        #[arg(long)]
        iterations: Option<u32>,
    },
}
```

`cli/src/read_cmds.rs`:

```rust
//! Read-only commands: quotes, lists, rescan, watch. Every one verifies the config first (I4).

use std::io::Write;
use std::time::Duration;

use auction_sdk::{
    chain::Chain,
    error::SdkError,
    quote::{self, AuctionView},
    scan::{self, Cursor, ScanSettings, ScanTargets},
    verify::VerifiedConfig,
};

use crate::{
    env::Env,
    error::CliError,
    output::{bps, kv, yes_no},
};

pub fn quote_liquidation<C: Chain>(env: &Env<C>, position: &str, out: &mut dyn Write) -> Result<(), CliError> {
    let v = env.verified()?;
    let pos = env.account(position)?;
    let now = env.now()?;
    let q = quote::quote_liquidation(&env.chain, &v, &pos, now)?;
    kv(out, "position", &pos.to_string())?;
    kv(out, "owner", &q.health.owner.to_string())?;
    kv(out, "unsafe (min CR)", yes_no(q.health.unsafe_at_min_cr))?;
    kv(out, "nonce", &q.nonce.to_string())?;
    kv(out, "auction", &q.auction.to_string())?;
    kv(out, "collateral", &q.health.collateral.to_string())?;
    kv(out, "vault balance", &q.vault_balance.to_string())?;
    kv(out, "nominal debt", &q.health.nominal_debt.to_string())?;
    kv(out, "amount to raise", &q.quote.amount_to_raise.to_string())?;
    kv(out, "keeper reward", &q.quote.keeper_reward.to_string())?;
    kv(out, "lot", &q.quote.lot.to_string())?;
    kv(out, "price (RAY)", &q.price_ray.to_string())?;
    kv(out, "clock (ms)", &now.to_string())
}

pub fn quote_bid<C: Chain>(
    env: &Env<C>,
    auction: &str,
    coin_in_max: u128,
    slippage_bps: u32,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let v = env.verified()?;
    let a = env.account(auction)?;
    let now = env.now()?;
    let q = quote::quote_bid(&env.chain, &v, &a, coin_in_max, 0, now)?;
    kv(out, "auction", &a.to_string())?;
    kv(out, "clock (ms)", &now.to_string())?;
    kv(out, "t_eff (ms)", &q.t_eff_ms.to_string())?;
    kv(out, "discount", &bps(q.fill.discount_ray))?;
    kv(out, "price (RAY)", &q.price_ray.to_string())?;
    kv(out, "remaining target", &q.remaining_target.to_string())?;
    kv(out, "collateral left", &q.collateral_left.to_string())?;
    kv(out, "charged", &q.fill.charged.to_string())?;
    kv(out, "collateral out", &q.fill.collateral_out.to_string())?;
    kv(
        out,
        &format!("min out ({slippage_bps} bps)"),
        &quote::min_out(q.fill.collateral_out, slippage_bps).to_string(),
    )?;
    kv(out, "debt part", &q.fill.debt_part.to_string())?;
    kv(out, "penalty part", &q.fill.penalty_part.to_string())?;
    kv(out, "ends auction", yes_no(q.fill.ends_auction))?;
    let ix = if q.needs_settlement(slippage_bps) {
        "bid_with_settlement"
    } else {
        "bid"
    };
    kv(out, "instruction", ix)
}

pub(crate) fn scan_now<C: Chain>(env: &Env<C>, v: &VerifiedConfig) -> Result<Cursor, CliError> {
    let path = env.cursor_path(&v.config_id);
    let cursor = scan::load_cursor(&path)?;
    let next = scan::scan(&env.chain, &ScanTargets::of(v), &cursor, &ScanSettings::default())?;
    scan::save_cursor(&path, &next)?;
    Ok(next)
}

fn auction_line(view: &AuctionView) -> String {
    let s = &view.state;
    let raised = s.debt_raised + s.penalty_raised;
    if s.settled {
        return format!(
            "{}  settled  sold {}  raised {}/{}  leftover {} (claimed {})",
            view.id, s.collateral_sold, raised, s.amount_to_raise, s.leftover_claimable, s.leftover_claimed
        );
    }
    let mut line = format!(
        "{}  live  lot {}  raised {}/{}  discount {}  t_eff {} ms",
        view.id,
        s.collateral_left,
        raised,
        s.amount_to_raise,
        bps(view.discount_ray),
        view.t_eff_ms
    );
    if view.held_at_max {
        line.push_str(&format!(
            "  held at max discount (keeps its cap reservation of {})",
            s.reserved_amount
        ));
    }
    if let Some(p) = &view.paused {
        line.push_str(&format!("  bids revert now: {p} (run observe-feed)"));
    }
    line
}

fn skippable(e: &SdkError) -> bool {
    matches!(
        e,
        SdkError::Missing { .. } | SdkError::Decode { .. } | SdkError::Mismatch { .. } | SdkError::Engine { .. }
    )
}

pub fn list_auctions<C: Chain>(env: &Env<C>, all: bool, out: &mut dyn Write) -> Result<(), CliError> {
    let v = env.verified()?;
    let cursor = scan_now(env, &v)?;
    let now = env.now()?;
    kv(out, "scanned to block", &cursor.last_block.to_string())?;
    let mut shown = 0usize;
    for id in cursor.auction_ids()? {
        let view = match quote::auction_view(&env.chain, &v, &id, now) {
            Ok(view) => view,
            Err(e) if skippable(&e) => continue,
            Err(e) => return Err(e.into()),
        };
        if view.state.settled && !all {
            continue;
        }
        shown += 1;
        writeln!(out, "{}", auction_line(&view))?;
    }
    if shown == 0 {
        writeln!(out, "no auctions")?;
    }
    Ok(())
}

pub fn list_at_risk<C: Chain>(env: &Env<C>, out: &mut dyn Write) -> Result<(), CliError> {
    let v = env.verified()?;
    let cursor = scan_now(env, &v)?;
    let now = env.now()?;
    kv(out, "scanned to block", &cursor.last_block.to_string())?;
    let mut shown = 0usize;
    for id in cursor.position_ids()? {
        let h = match quote::position_health(&env.chain, &v, &id, now) {
            Ok(h) => h,
            Err(e) if skippable(&e) => continue,
            Err(e) => return Err(e.into()),
        };
        if h.unsafe_at_min_cr && h.normalized_debt > 0 {
            shown += 1;
            writeln!(
                out,
                "{}  owner {}  collateral {}  debt {}  unsafe at min CR",
                h.position, h.owner, h.collateral, h.nominal_debt
            )?;
        }
    }
    if shown == 0 {
        writeln!(out, "no position is unsafe")?;
    }
    Ok(())
}

pub fn rescan<C: Chain>(env: &Env<C>, from: Option<u64>, out: &mut dyn Write) -> Result<(), CliError> {
    let v = env.verified()?;
    let path = env.cursor_path(&v.config_id);
    let mut cursor = scan::load_cursor(&path)?;
    if let Some(from) = from {
        let start = from.saturating_sub(1);
        let last_hash = if start == 0 {
            String::new()
        } else {
            let b = env.chain.blocks(start, start)?;
            hex::encode(
                b.first()
                    .ok_or_else(|| CliError::Usage(format!("block {start} not found")))?
                    .hash,
            )
        };
        cursor = Cursor {
            last_block: start,
            last_hash,
            ..cursor
        };
    }
    let next = scan::scan(&env.chain, &ScanTargets::of(&v), &cursor, &ScanSettings::default())?;
    scan::save_cursor(&path, &next)?;
    kv(out, "scanned to block", &next.last_block.to_string())?;
    kv(out, "auctions", &next.auctions.len().to_string())?;
    kv(out, "positions", &next.positions.len().to_string())
}

pub fn watch<C: Chain>(
    env: &Env<C>,
    interval_ms: u64,
    iterations: Option<u32>,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let mut round = 0u32;
    loop {
        list_auctions(env, false, out)?;
        round = round.saturating_add(1);
        if iterations.is_some_and(|n| round >= n) {
            return Ok(());
        }
        (env.sleep)(Duration::from_millis(interval_ms));
    }
}
```

`cli/src/lib.rs`:

```rust
//! `auction-cli` (spec §6.6): a thin front end over `auction_sdk`. `run` is generic over the chain so
//! golden tests drive every command against `FixtureChain`; `main.rs` plugs in `RpcChain`.
#![forbid(unsafe_code)]

pub mod args;
pub mod env;
pub mod error;
pub mod output;
mod read_cmds;

use std::io::Write;

use auction_sdk::chain::Chain;

pub use args::{Cli, Command};
pub use env::Env;
pub use error::CliError;

pub fn run<C: Chain>(cmd: &Command, env: &Env<C>, out: &mut dyn Write) -> Result<(), CliError> {
    match cmd {
        Command::QuoteLiquidation { position } => read_cmds::quote_liquidation(env, position, out),
        Command::QuoteBid {
            auction,
            coin_in_max,
            slippage_bps,
        } => read_cmds::quote_bid(env, auction, *coin_in_max, *slippage_bps, out),
        Command::ListAuctions { all } => read_cmds::list_auctions(env, *all, out),
        Command::ListAtRisk => read_cmds::list_at_risk(env, out),
        Command::Rescan { from } => read_cmds::rescan(env, *from, out),
        Command::Watch {
            interval_ms,
            iterations,
        } => read_cmds::watch(env, *interval_ms, *iterations, out),
    }
}
```

`cli/src/main.rs`:

```rust
//! `auction-cli`: parse arguments, load the deployment file, connect to the sequencer, run.
#![forbid(unsafe_code)]

use std::process::ExitCode;

use auction_cli::{args::Cli, env::Env, run};
use auction_sdk::{confirm::ConfirmSettings, deployment::Deployment, rpc::RpcChain};
use clap::Parser;
use lee_core::account::AccountId;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let deployment = match Deployment::load(&cli.deployment) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let config = match cli.config.as_deref().map(str::parse::<AccountId>).transpose() {
        Ok(c) => c,
        Err(_) => {
            eprintln!("error: --config is not a base58 account id");
            return ExitCode::from(1);
        }
    };
    let url = cli.rpc.clone().unwrap_or_else(|| deployment.sequencer_url.clone());
    let chain = match RpcChain::connect(&url) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };
    let env = Env {
        chain,
        deployment,
        config,
        wallet_home: cli.wallet_home.clone(),
        state_dir: cli.state_dir.clone(),
        at_ms: cli.at_ms,
        confirm: ConfirmSettings::default(),
        sleep: std::thread::sleep,
    };
    let mut out = std::io::stdout().lock();
    match run(&cli.command, &env, &mut out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(e.exit_code())
        }
    }
}
```

- [ ] **Step 6: Generate the golden files and review them**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && UPDATE_GOLDEN=1 scripts/wsl-cargo.sh test -p auction_cli && scripts/wsl-cargo.sh test -p auction_cli'`
Expected: the first run writes `cli/tests/golden/{quote_liquidation,quote_bid,quote_bid_not_an_auction,list_auctions,list_auctions_held_at_max,list_at_risk,rescan,watch}.txt`; the second run passes all 8 golden tests plus `output::tests`. Review every file before staging: values match the asserted numbers; no absolute path (only `<tmp>`); `quote_bid_not_an_auction.txt` ends with `error: verification failed: Auction: expected owner ...` and `exit: 2`.

- [ ] **Step 7: `cli/README.md`, ADR-044, README**

Create `cli/README.md`:

```markdown
# auction-cli

Command-line client of the RFP-014 auction engine (spec §6.6), built on `auction_sdk` (own encoders; not spel-cli, which mis-encodes stablecoin instructions, spec §3.2). LEZ v0.2.4, public transactions only.

## Build (WSL)

    scripts/wsl-cargo.sh build -p auction_cli        # binary: $HOME/lez-target/plan2/debug/auction-cli

## Global options

| Option | Default | Meaning |
|---|---|---|
| `--deployment PATH` | `deployments/localnet.json` | trusted ImageIDs, sequencer URL, optional config and admin (I4) |
| `--config ID` | the file's `config` | engine config account |
| `--rpc URL` | the file's `sequencer_url` | sequencer JSON-RPC |
| `--wallet-home DIR` | `$LEE_WALLET_HOME_DIR` | LEZ wallet home (`storage.json`) |
| `--state-dir DIR` | `.auction-cli` | scan cursor (`<config>.cursor.json`) and in-flight locks |
| `--at-ms MS` | chain clock | evaluate quotes at this time |

Account arguments accept a base58 id or a wallet label. Every command that touches a config first verifies it against the deployment file (owner = engine ImageID, `["CONFIG", admin, salt]`, stored host/token program ids, admin, host `ProtocolParameters` owner) and refuses on any mismatch.

## Exit codes

`0` ok · `1` usage or other error · `2` verification failed (wrong network / ImageID) · `3` transaction dropped: nothing applied, re-quote · `4` keystore error.

## Read-only commands

    auction-cli quote-liquidation --position <id>
    auction-cli quote-bid --auction <id> --coin-in-max <coin> [--slippage-bps 50]
    auction-cli list-auctions [--all]          # scans new blocks first; flags auctions held at max discount
    auction-cli list-at-risk                   # positions unsafe at the host's minimum ratio
    auction-cli rescan [--from <block>]
    auction-cli watch [--interval-ms 15000] [--iterations N]

Quotes are valid only at their clock; the transaction commands derive their slippage bounds from a fresh quote.
```

Append to `docs/design-log.md`:

```markdown

---

## ADR-044: CLI Conventions: Labels or Ids, Plain Key-Value Output, Exit Codes, Golden Tests

**Context**
Spec §6.6 lists the commands; spec §9 asks for CLI golden tests. The CLI must be scriptable (the localnet runbook drives it) and its failures must be distinguishable.

**Decision**
Account arguments take a base58 id or a wallet label. Output is `key value` lines (key padded to 17 columns, then a space) and one line per listed item, with no colour or tables, so scripts can `awk` it and golden files stay stable. Exit codes: 1 usage/other, 2 verification (I4), 3 dropped (re-quote), 4 keystore. `run` is generic over `Chain`; golden tests run every command against `FixtureChain` with a temporary wallet, replacing temp paths and transaction hashes.

**Trade-offs**
No JSON output in the PoC (the Plan 4 FFI is the machine interface). Golden files must be reviewed when output changes (`UPDATE_GOLDEN=1`).

**Status**
Accepted (Plan 3a, Task 13)
```

In `README.md`, extend the Plan 3a status paragraph with: "CLI: [cli/README.md](cli/README.md)."

- [ ] **Step 8: Gates and commit (controller — GPG signing on Windows)**

Run the lint gate and the format gate with `-p auction_sdk -p auction_cli`. Expected: clean.

```bash
git add Cargo.toml Cargo.lock cli docs/design-log.md README.md
git commit -m "feat(cli): auction-cli with read-only commands and golden tests"
```

---

### Task 14: CLI transaction commands (liquidate, bid, settlement, surplus and debt auctions, observe)

**Files:**
- Create: `cli/src/tx_cmds.rs`
- Modify: `cli/src/args.rs`, `cli/src/lib.rs`, `cli/src/read_cmds.rs` (`watch --observe-with`), `cli/tests/golden.rs`, `cli/README.md`
- Create (generated): `cli/tests/golden/{liquidate,liquidate_safe_position,bid_partial,bid_final,bid_dropped,keystore_missing,settle_debt,check_solvency,observe_feed,start_surplus_while_live,take_surplus,take_debt,close_surplus,close_debt,claim_leftover,settle_empty,watch_observe}.txt`

**Interfaces:**
- Consumes: `Env::{verified, account, now, is_fresh, send}`, `quote::{quote_liquidation, quote_bid, quote_take_surplus, quote_take_debt, min_out, max_in, earliest_debt_start}`, `verify_binding`, every engine builder in `plan`, `decode::Reader::accounting`.
- Produces: `args::{LiquidateCmd, BidCmd, TakeCmd}` (clap `Args`) and `Command` variants `Liquidate(LiquidateCmd)`, `Bid(BidCmd)`, `SettleDebt { caller, amount: Option<u128> }`, `CheckSolvency { caller }`, `StartSurplus { caller }`, `TakeSurplus(TakeCmd)`, `StartDebt { caller }`, `TakeDebt(TakeCmd)`, `ObserveFeed { caller }`, `ClaimLeftover { owner, owner_holding, auction }`, `SettleEmpty { caller, auction }`, `CloseSurplus { caller }`, `CloseDebt { caller }`; `Watch` gains `observe_with: Option<String>`. `TakeCmd { payer, receiver, amount: u128, slippage_bps: u32 }` is used by both takes (`--payer` signs: protocol-token holding for surplus, stablecoin holding for debt; `--receiver` receives). Nonces: `start-*` use `Accounting.{surplus,debt}_nonce`; `take-*`/`close-*` use the latest auction (`nonce − 1`).

Client-side pre-checks (cheaper than a dropped transaction; the engine still decides): `liquidate` refuses a position that is safe at the host's minimum ratio and first runs `verify_binding`; `start-surplus`/`start-debt` refuse while one is live, and `start-debt` refuses inside the relist delay (ADR-039), printing the earliest start.

- [ ] **Step 1: Write the failing golden tests (Review Focus 2 and 4)**

Append to `cli/tests/golden.rs`:

```rust
use auction_sdk::testing::SendMode;

fn sent_engine_names(c: &Case) -> Vec<String> {
    c.fx.chain
        .sent()
        .iter()
        .map(|tx| {
            let ix: auction_engine_core::Instruction =
                risc0_zkvm::serde::from_slice(&tx.message().instruction_data).expect("engine instruction");
            ix.name().to_owned()
        })
        .collect()
}

#[test]
fn liquidate() {
    let c = Case::new();
    let p = c.fx.unsafe_position.to_string();
    let out = c.run(&["liquidate", "--position", &p, "--keeper", "keeper", "--keeper-holding", "keeper-coll"]);
    assert_eq!(value(&out, "plan"), "liquidate (17 accounts, 2 signer(s))", "fresh reward holding signs");
    assert_eq!(value(&out, "included"), "signer nonce 1");
    assert_eq!(sent_engine_names(&c), vec!["liquidate"]);
    check("liquidate", &out);
}

#[test]
fn liquidate_refuses_a_position_that_is_safe() {
    let c = Case::new();
    let r = auction_sdk::decode::Reader::new(&c.fx.chain, standard::ENGINE);
    let mut pp: stablecoin_core::ProtocolParameters = r
        .host(&c.fx.host.protocol_parameters, &standard::HOST, "ProtocolParameters")
        .expect("pp");
    pp.minimum_collateralization_ratio = standard::ONE * 3 / 2; // 500_000 >= 600_000 x 0.5 x 1.5
    c.fx.chain.put_state(c.fx.host.protocol_parameters, standard::HOST, &pp);
    let p = c.fx.unsafe_position.to_string();
    let out = c.run(&["liquidate", "--position", &p, "--keeper", "keeper", "--keeper-holding", "keeper-coll"]);
    assert!(out.ends_with("exit: 1\n"), "{out}");
    assert!(c.fx.chain.sent().is_empty());
    check("liquidate_safe_position", &out);
}

fn bid_args(a: &str, coin: &str) -> Vec<String> {
    ["bid", "--auction", a, "--coin-in-max", coin, "--bidder-stable", "bidder-stable", "--bidder-collateral", "bidder-coll"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
}

fn run_owned(c: &Case, args: &[String]) -> String {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    c.run(&refs)
}

#[test]
fn bid_partial() {
    let c = Case::new();
    let out = run_owned(&c, &bid_args(&c.fx.live_auction.to_string(), "100000"));
    assert_eq!(value(&out, "quoted out"), "51963");
    assert_eq!(value(&out, "min out"), "51703");
    assert_eq!(value(&out, "plan"), "bid (11 accounts, 1 signer(s))");
    check("bid_partial", &out);
}

#[test]
fn bid_final() {
    let c = Case::new();
    let out = run_owned(&c, &bid_args(&c.fx.live_auction.to_string(), "660000"));
    assert_eq!(value(&out, "plan"), "bid_with_settlement (12 accounts, 1 signer(s))");
    assert_eq!(sent_engine_names(&c), vec!["bid_with_settlement"]);
    check("bid_final", &out);
}

#[test]
fn bid_dropped() {
    let c = Case::new();
    c.fx.chain.set_send_mode(SendMode::Drop);
    c.fx.chain.set_height_step(1);
    let out = run_owned(&c, &bid_args(&c.fx.live_auction.to_string(), "100000"));
    assert!(out.contains("nothing was applied; re-quote and resend"), "{out}");
    assert!(out.ends_with("exit: 3\n"), "{out}");
    assert_eq!(c.fx.chain.sent().len(), 1, "never resent");
    let locks = c.dir.path().join("state/locks");
    assert!(std::fs::read_dir(&locks).map_or(true, |mut d| d.next().is_none()), "lock released");
    check("bid_dropped", &out);
}

#[test]
fn keystore_missing() {
    let c = Case { with_wallet: false, ..Case::new() };
    let out = run_owned(&c, &bid_args(&c.fx.live_auction.to_string(), "100000"));
    assert!(out.contains("keystore: wallet storage not found at <tmp>/wallet/storage.json"), "{out}");
    assert!(out.ends_with("exit: 4\n"), "{out}");
    assert!(c.fx.chain.sent().is_empty());
    check("keystore_missing", &out);
}

#[test]
fn system_and_maintenance_commands() {
    let a = standard::standard().live_auction.to_string();
    let cases: Vec<(&str, Vec<&str>, Option<&str>)> = vec![
        ("settle_debt", vec!["settle-debt", "--caller", "caller"], Some("settle_debt")),
        ("check_solvency", vec!["check-solvency", "--caller", "caller"], Some("check_solvency")),
        ("observe_feed", vec!["observe-feed", "--caller", "caller"], Some("observe_feed")),
        ("start_surplus_while_live", vec!["start-surplus", "--caller", "caller"], None),
        (
            "take_surplus",
            vec!["take-surplus", "--payer", "taker-protocol", "--receiver", "taker-stable", "--amount", "500"],
            Some("take_surplus"),
        ),
        (
            "take_debt",
            vec!["take-debt", "--payer", "taker-stable", "--receiver", "taker-protocol", "--amount", "500"],
            Some("take_debt"),
        ),
        ("close_surplus", vec!["close-surplus", "--caller", "caller"], Some("close_surplus_auction")),
        ("close_debt", vec!["close-debt", "--caller", "caller"], Some("close_debt_auction")),
        (
            "claim_leftover",
            vec!["claim-leftover", "--owner", "owner2", "--owner-holding", "owner-coll", "--auction", &a],
            Some("claim_leftover"),
        ),
        ("settle_empty", vec!["settle-empty", "--caller", "caller", "--auction", &a], Some("settle_empty")),
    ];
    for (name, args, sent) in cases {
        let c = Case::new();
        let out = c.run(&args);
        match sent {
            Some(ix) => {
                assert!(out.ends_with("exit: 0\n"), "{name}: {out}");
                assert_eq!(sent_engine_names(&c), vec![ix], "{name}");
            }
            None => {
                assert!(out.ends_with("exit: 1\n"), "{name}: {out}");
                assert!(c.fx.chain.sent().is_empty(), "{name}");
            }
        }
        check(name, &out);
    }
}

#[test]
fn take_quotes_set_the_slippage_bounds() {
    let c = Case::new();
    let out = c.run(&["take-surplus", "--payer", "taker-protocol", "--receiver", "taker-stable", "--amount", "500"]);
    assert_eq!(value(&out, "quoted tokens"), "200");
    assert_eq!(value(&out, "max protocol in"), "201");
    let c = Case::new();
    let out = c.run(&["take-debt", "--payer", "taker-stable", "--receiver", "taker-protocol", "--amount", "500"]);
    assert_eq!(value(&out, "quoted tokens"), "325");
    assert_eq!(value(&out, "min protocol out"), "323");
}

#[test]
fn watch_observe() {
    let c = Case::new();
    let out = c.run(&["watch", "--iterations", "1", "--interval-ms", "0", "--observe-with", "caller"]);
    assert_eq!(sent_engine_names(&c), vec!["observe_feed"]);
    check("watch_observe", &out);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_cli'`
Expected: the new tests fail with `arguments parse` panics (`unrecognized subcommand 'liquidate'`).

- [ ] **Step 3: Arguments**

In `cli/src/args.rs` change `use clap::{Parser, Subcommand};` to `use clap::{Args, Parser, Subcommand};`, replace the `Watch` variant with

```rust
    /// Refresh periodically: scan and list auctions; with --observe-with, send observe_feed first each
    /// round (keepers and apps refresh the pause clock, THREAT_MODEL S5 residual).
    Watch {
        #[arg(long, default_value_t = 15_000)]
        interval_ms: u64,
        #[arg(long)]
        iterations: Option<u32>,
        #[arg(long)]
        observe_with: Option<String>,
    },
```

and add these variants to `Command`:

```rust
    /// Liquidate an unsafe position (quotes first; refuses a safe position).
    Liquidate(LiquidateCmd),
    /// Bid; picks bid or bid_with_settlement from the quote (ADR-029).
    Bid(BidCmd),
    /// Burn buffer coin against unbacked debt (default: as much as possible).
    SettleDebt {
        #[arg(long)]
        caller: String,
        #[arg(long)]
        amount: Option<u128>,
    },
    /// Freeze new debt if bad debt exceeds the threshold and no working protocol token exists.
    CheckSolvency {
        #[arg(long)]
        caller: String,
    },
    StartSurplus {
        #[arg(long)]
        caller: String,
    },
    /// Pay protocol tokens (--payer), receive stablecoin (--receiver).
    TakeSurplus(TakeCmd),
    StartDebt {
        #[arg(long)]
        caller: String,
    },
    /// Pay stablecoin (--payer), receive minted protocol tokens (--receiver).
    TakeDebt(TakeCmd),
    /// Persist a pause-clock observation (never reverts on a bad feed).
    ObserveFeed {
        #[arg(long)]
        caller: String,
    },
    ClaimLeftover {
        #[arg(long)]
        owner: String,
        #[arg(long)]
        owner_holding: String,
        #[arg(long)]
        auction: String,
    },
    /// Settle an auction whose lot is already 0.
    SettleEmpty {
        #[arg(long)]
        caller: String,
        #[arg(long)]
        auction: String,
    },
    CloseSurplus {
        #[arg(long)]
        caller: String,
    },
    CloseDebt {
        #[arg(long)]
        caller: String,
    },
```

and the argument structs:

```rust
#[derive(Debug, Args)]
pub struct LiquidateCmd {
    #[arg(long)]
    pub position: String,
    #[arg(long)]
    pub keeper: String,
    /// Collateral holding that receives the keeper reward (signs too if it is fresh).
    #[arg(long)]
    pub keeper_holding: String,
}

#[derive(Debug, Args)]
pub struct BidCmd {
    #[arg(long)]
    pub auction: String,
    #[arg(long)]
    pub coin_in_max: u128,
    #[arg(long)]
    pub bidder_stable: String,
    #[arg(long)]
    pub bidder_collateral: String,
    #[arg(long, default_value_t = 50)]
    pub slippage_bps: u32,
    /// Explicit minimum collateral out (overrides --slippage-bps).
    #[arg(long)]
    pub min_out: Option<u128>,
}

#[derive(Debug, Args)]
pub struct TakeCmd {
    #[arg(long)]
    pub payer: String,
    #[arg(long)]
    pub receiver: String,
    #[arg(long)]
    pub amount: u128,
    #[arg(long, default_value_t = 50)]
    pub slippage_bps: u32,
}
```

- [ ] **Step 4: `tx_cmds.rs`**

```rust
//! Transaction commands (spec §6.6). Each one: verify the config (I4) -> quote where a price is
//! involved -> build the plan -> lock, sign with the wallet, send, confirm (spec §6.5).

use std::io::Write;

use auction_engine_core::state::AccountingState;
use auction_sdk::{
    chain::Chain,
    decode::Reader,
    plan::{self, BidArgs, LiquidateArgs, TakeArgs},
    quote,
    verify::{verify_binding, VerifiedConfig},
};

use crate::{
    args::{BidCmd, LiquidateCmd, TakeCmd},
    env::Env,
    error::CliError,
    output::kv,
};

fn accounting<C: Chain>(env: &Env<C>, v: &VerifiedConfig) -> Result<AccountingState, CliError> {
    Ok(Reader::new(&env.chain, v.engine.program).accounting(&v.engine.accounting)?)
}

fn latest(next_nonce: u64, what: &str) -> Result<u64, CliError> {
    next_nonce
        .checked_sub(1)
        .ok_or_else(|| CliError::Usage(format!("no {what} auction has been started")))
}

pub fn liquidate<C: Chain>(env: &Env<C>, a: &LiquidateCmd, out: &mut dyn Write) -> Result<(), CliError> {
    let v = env.verified()?;
    verify_binding(&env.chain, &v)?;
    let position = env.account(&a.position)?;
    let keeper = env.account(&a.keeper)?;
    let holding = env.account(&a.keeper_holding)?;
    let q = quote::quote_liquidation(&env.chain, &v, &position, env.now()?)?;
    if !q.health.unsafe_at_min_cr {
        return Err(CliError::Usage(
            "the position is collateralised at the host's minimum ratio: the host would reject the liquidation".into(),
        ));
    }
    kv(out, "nonce", &q.nonce.to_string())?;
    kv(out, "auction", &q.auction.to_string())?;
    kv(out, "keeper reward", &q.quote.keeper_reward.to_string())?;
    let oracle = v.coll(&env.chain)?.oracle_id;
    let p = plan::liquidate(
        &v,
        &LiquidateArgs {
            keeper,
            keeper_collateral_holding: holding,
            keeper_holding_is_fresh: env.is_fresh(&holding)?,
            position,
            nonce: q.nonce,
            oracle,
        },
    )?;
    env.send(&p, "liquidate", out)
}

pub fn bid<C: Chain>(env: &Env<C>, a: &BidCmd, out: &mut dyn Write) -> Result<(), CliError> {
    let v = env.verified()?;
    let auction = env.account(&a.auction)?;
    let stable = env.account(&a.bidder_stable)?;
    let collateral = env.account(&a.bidder_collateral)?;
    let q = quote::quote_bid(&env.chain, &v, &auction, a.coin_in_max, 0, env.now()?)?;
    let min_out = a
        .min_out
        .unwrap_or_else(|| quote::min_out(q.fill.collateral_out, a.slippage_bps));
    let with_settlement = q.needs_settlement(a.slippage_bps);
    kv(out, "quoted out", &q.fill.collateral_out.to_string())?;
    kv(out, "min out", &min_out.to_string())?;
    let p = plan::bid(
        &v,
        &BidArgs {
            bidder_stable: stable,
            bidder_collateral: collateral,
            bidder_collateral_is_fresh: env.is_fresh(&collateral)?,
            auction,
            coin_in_max: a.coin_in_max,
            min_collateral_out: min_out,
            with_settlement,
            oracle: v.coll(&env.chain)?.oracle_id,
        },
    )?;
    env.send(&p, if with_settlement { "bid_with_settlement" } else { "bid" }, out)
}

pub fn settle_debt<C: Chain>(env: &Env<C>, caller: &str, amount: Option<u128>, out: &mut dyn Write) -> Result<(), CliError> {
    let v = env.verified()?;
    let p = plan::settle_debt(&v, env.account(caller)?, amount.unwrap_or(u128::MAX))?;
    env.send(&p, "settle_debt", out)
}

pub fn check_solvency<C: Chain>(env: &Env<C>, caller: &str, out: &mut dyn Write) -> Result<(), CliError> {
    let v = env.verified()?;
    let p = plan::check_solvency(&v, env.account(caller)?)?;
    env.send(&p, "check_solvency", out)
}

pub fn observe_feed<C: Chain>(env: &Env<C>, caller: &str, out: &mut dyn Write) -> Result<(), CliError> {
    let v = env.verified()?;
    let p = plan::observe_feed(&v, env.account(caller)?, v.coll(&env.chain)?.oracle_id)?;
    env.send(&p, "observe_feed", out)
}

pub fn start_surplus<C: Chain>(env: &Env<C>, caller: &str, out: &mut dyn Write) -> Result<(), CliError> {
    let v = env.verified()?;
    let acc = accounting(env, &v)?;
    if acc.surplus_live {
        return Err(CliError::Usage(format!(
            "a surplus auction is live (nonce {}); take it or close it after its TTL first",
            latest(acc.surplus_nonce, "surplus")?
        )));
    }
    let p = plan::start_surplus(&v, env.account(caller)?, acc.surplus_nonce)?;
    env.send(&p, "start_surplus_auction", out)
}

pub fn take_surplus<C: Chain>(env: &Env<C>, a: &TakeCmd, out: &mut dyn Write) -> Result<(), CliError> {
    let v = env.verified()?;
    let nonce = latest(accounting(env, &v)?.surplus_nonce, "surplus")?;
    let q = quote::quote_take_surplus(&env.chain, &v, nonce, a.amount, env.now()?)?;
    let limit = quote::max_in(q.tokens, a.slippage_bps);
    kv(out, "quoted tokens", &q.tokens.to_string())?;
    kv(out, "max protocol in", &limit.to_string())?;
    let receiver = env.account(&a.receiver)?;
    let p = plan::take_surplus(
        &v,
        &TakeArgs {
            payer: env.account(&a.payer)?,
            receiver,
            receiver_is_fresh: env.is_fresh(&receiver)?,
            nonce,
            amount_coin: a.amount,
            limit,
        },
    )?;
    env.send(&p, "take_surplus", out)
}

pub fn close_surplus<C: Chain>(env: &Env<C>, caller: &str, out: &mut dyn Write) -> Result<(), CliError> {
    let v = env.verified()?;
    let nonce = latest(accounting(env, &v)?.surplus_nonce, "surplus")?;
    let p = plan::close_surplus(&v, env.account(caller)?, nonce)?;
    env.send(&p, "close_surplus_auction", out)
}

pub fn start_debt<C: Chain>(env: &Env<C>, caller: &str, out: &mut dyn Write) -> Result<(), CliError> {
    let v = env.verified()?;
    let acc = accounting(env, &v)?;
    if acc.debt_live {
        return Err(CliError::Usage(format!(
            "a debt auction is live (nonce {}); take it or close it after its TTL first",
            latest(acc.debt_nonce, "debt")?
        )));
    }
    if let Some(t) = quote::earliest_debt_start(&acc) {
        if env.now()? < t {
            return Err(CliError::Usage(format!(
                "relist delay (ADR-039): the next debt auction may start at {t} ms"
            )));
        }
    }
    let p = plan::start_debt(&v, env.account(caller)?, acc.debt_nonce)?;
    env.send(&p, "start_debt_auction", out)
}

pub fn take_debt<C: Chain>(env: &Env<C>, a: &TakeCmd, out: &mut dyn Write) -> Result<(), CliError> {
    let v = env.verified()?;
    let nonce = latest(accounting(env, &v)?.debt_nonce, "debt")?;
    let q = quote::quote_take_debt(&env.chain, &v, nonce, a.amount, env.now()?)?;
    let limit = quote::min_out(q.tokens, a.slippage_bps);
    kv(out, "quoted tokens", &q.tokens.to_string())?;
    kv(out, "min protocol out", &limit.to_string())?;
    let receiver = env.account(&a.receiver)?;
    let p = plan::take_debt(
        &v,
        &TakeArgs {
            payer: env.account(&a.payer)?,
            receiver,
            receiver_is_fresh: env.is_fresh(&receiver)?,
            nonce,
            amount_coin: a.amount,
            limit,
        },
    )?;
    env.send(&p, "take_debt", out)
}

pub fn close_debt<C: Chain>(env: &Env<C>, caller: &str, out: &mut dyn Write) -> Result<(), CliError> {
    let v = env.verified()?;
    let nonce = latest(accounting(env, &v)?.debt_nonce, "debt")?;
    let p = plan::close_debt(&v, env.account(caller)?, nonce)?;
    env.send(&p, "close_debt_auction", out)
}

pub fn claim_leftover<C: Chain>(
    env: &Env<C>,
    owner: &str,
    owner_holding: &str,
    auction: &str,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let v = env.verified()?;
    let holding = env.account(owner_holding)?;
    let p = plan::claim_leftover(&v, env.account(owner)?, holding, env.is_fresh(&holding)?, env.account(auction)?)?;
    env.send(&p, "claim_leftover", out)
}

pub fn settle_empty<C: Chain>(env: &Env<C>, caller: &str, auction: &str, out: &mut dyn Write) -> Result<(), CliError> {
    let v = env.verified()?;
    let p = plan::settle_empty(&v, env.account(caller)?, env.account(auction)?)?;
    env.send(&p, "settle_empty", out)
}
```

In `cli/src/read_cmds.rs`, change `watch` to take `observe_with: Option<&str>` and, at the top of each round, before `list_auctions`, add:

```rust
        if let Some(caller) = observe_with {
            crate::tx_cmds::observe_feed(env, caller, out)?;
        }
```

(new signature: `pub fn watch<C: Chain>(env: &Env<C>, interval_ms: u64, iterations: Option<u32>, observe_with: Option<&str>, out: &mut dyn Write)`).

In `cli/src/lib.rs` add `mod tx_cmds;` and extend `run`:

```rust
        Command::Watch {
            interval_ms,
            iterations,
            observe_with,
        } => read_cmds::watch(env, *interval_ms, *iterations, observe_with.as_deref(), out),
        Command::Liquidate(a) => tx_cmds::liquidate(env, a, out),
        Command::Bid(a) => tx_cmds::bid(env, a, out),
        Command::SettleDebt { caller, amount } => tx_cmds::settle_debt(env, caller, *amount, out),
        Command::CheckSolvency { caller } => tx_cmds::check_solvency(env, caller, out),
        Command::StartSurplus { caller } => tx_cmds::start_surplus(env, caller, out),
        Command::TakeSurplus(a) => tx_cmds::take_surplus(env, a, out),
        Command::StartDebt { caller } => tx_cmds::start_debt(env, caller, out),
        Command::TakeDebt(a) => tx_cmds::take_debt(env, a, out),
        Command::ObserveFeed { caller } => tx_cmds::observe_feed(env, caller, out),
        Command::ClaimLeftover {
            owner,
            owner_holding,
            auction,
        } => tx_cmds::claim_leftover(env, owner, owner_holding, auction, out),
        Command::SettleEmpty { caller, auction } => tx_cmds::settle_empty(env, caller, auction, out),
        Command::CloseSurplus { caller } => tx_cmds::close_surplus(env, caller, out),
        Command::CloseDebt { caller } => tx_cmds::close_debt(env, caller, out),
```

(replacing the old `Command::Watch` arm).

- [ ] **Step 5: Generate and review the golden files**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && UPDATE_GOLDEN=1 scripts/wsl-cargo.sh test -p auction_cli && scripts/wsl-cargo.sh test -p auction_cli'`
Expected: second run all pass (8 from Task 13, 9 new tests). Review each new file: `submitted` lines show `<hash>`; `bid_dropped.txt` ends with the dropped message and `exit: 3`; `keystore_missing.txt` with `exit: 4` and no `plan` line; `start_surplus_while_live.txt` with `exit: 1` and no `plan` line; no file contains a 64-hex-character key.

- [ ] **Step 6: README, gates, commit**

Append to `cli/README.md`:

```markdown
## Transaction commands

Every one verifies the config (exit 2 on mismatch), signs with the wallet (exit 4 on a keystore problem), sends, and confirms by nonce polling: `included` when the signer's nonce moves, exit 3 ("dropped: nothing was applied; re-quote") when it has not moved 5 blocks later. One transaction per signer is in flight at a time (lock in `<state-dir>/locks`).

    auction-cli liquidate --position <id> --keeper <signer> --keeper-holding <collateral holding>
    auction-cli bid --auction <id> --coin-in-max <coin> --bidder-stable <holding> --bidder-collateral <holding> [--slippage-bps 50 | --min-out N]
    auction-cli claim-leftover --owner <signer> --owner-holding <holding> --auction <id>
    auction-cli settle-empty --caller <signer> --auction <id>
    auction-cli settle-debt --caller <signer> [--amount N]
    auction-cli check-solvency --caller <signer>
    auction-cli observe-feed --caller <signer>
    auction-cli start-surplus --caller <signer>
    auction-cli take-surplus --payer <protocol-token holding> --receiver <stablecoin holding> --amount <coin> [--slippage-bps 50]
    auction-cli close-surplus --caller <signer>
    auction-cli start-debt --caller <signer>
    auction-cli take-debt --payer <stablecoin holding> --receiver <protocol-token holding> --amount <coin> [--slippage-bps 50]
    auction-cli close-debt --caller <signer>
    auction-cli watch --observe-with <signer>  # refresh the pause clock every round

`bid` quotes first and sets `min_collateral_out` from the quote and the slippage; it sends `bid_with_settlement` when the fill may end the auction (ADR-029). A fresh (never initialised) recipient holding must also be in the wallet: it signs (`RecipientMustSign`). Signers that sign more than one stablecoin-program transaction must be `wallet auth-transfer init`-ed first (ADR-018). Never unfreeze the host, or advise it, while settled bad debt is above the freeze threshold (THREAT_MODEL); `host_frozen_by_engine` means "has ever frozen".
```

Run the lint gate and the format gate (with `-p auction_sdk -p auction_cli`). Expected: clean.

Commit (controller — GPG signing on Windows):

```bash
git add cli
git commit -m "feat(cli): transaction commands with quotes, slippage bounds and confirmation"
```

---

### Task 15: CLI admin, host and dev commands (bind-time verification, signed gains, localnet setup)

These replace every spel call of the localnet runbook. `host bind` is the ADR-027 bind-time verification automated: it refuses unless `verify_config` accepts the config.

**Files:**
- Create: `cli/src/{admin_cmds.rs,params.rs}`, `cli/params/localnet-engine.json`, `cli/params/localnet-collateral.json`
- Modify: `cli/src/args.rs`, `cli/src/lib.rs`, `cli/tests/golden.rs`, `cli/README.md`
- Create (generated): `cli/tests/golden/{admin_addresses,admin_verify,admin_init_config,admin_set_collateral_config,admin_set_engine_params,host_addresses,host_bind,bind_wrong_host,host_set_controller_gains,host_init_program,host_open_position,dev_token_new,dev_oracle_create,dev_balance,dev_resolve}.txt`

**Interfaces:**
- Consumes: `plan::{init_config, set_engine_params, set_collateral_config}`, `plan_host::*`, `verify_config`, `verify_binding`, `EngineAddresses`, `HostAddresses`, `Reader`, `deployment::program_id_hex`.
- Produces: `Command::{Admin(AdminCommand), Host(HostCommand), Dev(DevCommand)}`:
  - `admin addresses --admin <l> [--salt 1]`, `admin init-config --admin <l> [--salt 1] --params <json> [--protocol-token <id|l>] [--ref-oracle <id>]`, `admin set-engine-params --admin <l> --params <json> [--protocol-token] [--ref-oracle]`, `admin set-collateral-config --admin <l> --params <json>`, `admin verify`;
  - `host addresses`, `host bind --admin <l>`, `host init-program --admin <l> --freeze <id|l> --collateral-def <id|l> --oracle <id> [--kp i128] [--ki i128] [--min-cr] [--redemption-price] [--stability-fee] [--min-ms-between-updates] [--max-oracle-age-ms] [--name]`, `host open-position --owner --owner-collateral [--nonce 0] --amount`, `host generate-debt --owner --position --owner-stable --amount`, `host set-min-cr --admin --ratio`, `host set-controller-gains --admin --kp --ki`;
  - `dev resolve --label <l>`, `dev token-new --definition --holding --name --supply [--mint-authority]`, `dev token-init-account --definition --account`, `dev token-transfer --from --to --amount`, `dev oracle-create --price-source --base --quote --price [--window-ms 3600000]`, `dev balance --account`.
- `params::{engine_params(path, protocol_token_enabled, ref_oracle_enabled) -> Result<EngineParamsArgs, CliError>, collateral_args(path) -> Result<CollateralArgs, CliError>}`: JSON files with `u128` values as decimal strings; the two `*_enabled` flags come from the presence of `--protocol-token` / `--ref-oracle`, never from the file. `admin set-engine-params` prints a `ref price change` line when the reference price changes (record it in `DEPLOYMENTS.md`, ADR-017).

- [ ] **Step 1: Parameter files**

`cli/params/localnet-engine.json` (the Plan 2 smoke values, spec §5.12 shapes):

```json
{
  "global_cap": "10000000",
  "freeze_on_bad_debt": true,
  "bad_debt_freeze_threshold": "1000",
  "protocol_token_ref_price": "2000000000000000000000000000",
  "ref_oracle_max_age_ms": 3600000,
  "surplus_buffer_target": "1000",
  "surplus_lot": "500",
  "surplus_delay_ms": 0,
  "surplus_premium_bps": 500,
  "surplus_max_discount_bps": 2000,
  "debt_lot": "500",
  "debt_initial_discount_bps": 0,
  "debt_growth": [0, 0, 1000, 3000],
  "surplus_decay": [0, 0, 1000, 2500],
  "max_mint_per_auction": "5000",
  "min_take": "10",
  "system_ttl_ms": 14400000
}
```

`cli/params/localnet-collateral.json`:

```json
{
  "oracle_scale": 0,
  "max_oracle_age_ms": 3600000,
  "max_conf_bps": 100,
  "band_lo_bps": 9000,
  "band_hi_bps": 11000,
  "penalty_bps": 1000,
  "tip": "5",
  "chip_bps": 10,
  "reward_cap": "50",
  "debt_floor": "100",
  "min_bid": "25",
  "type_cap": "5000000",
  "schedule": [0, 200, 2700000, 1000],
  "admin_paused": false
}
```

- [ ] **Step 2: Write the failing golden tests (Review Focus 3)**

Append to `cli/tests/golden.rs`:

```rust
fn params(file: &str) -> String {
    format!("{}/params/{file}", env!("CARGO_MANIFEST_DIR"))
}

fn sent_host_debug(c: &Case) -> String {
    let sent = c.fx.chain.sent();
    let tx = sent.last().expect("sent");
    let ix: stablecoin_core::Instruction =
        risc0_zkvm::serde::from_slice(&tx.message().instruction_data).expect("stablecoin instruction");
    format!("{ix:?}")
}

#[test]
fn admin_commands() {
    let protocol = standard::PROTOCOL.to_string();
    let engine_params = params("localnet-engine.json");
    let collateral_params = params("localnet-collateral.json");
    let cases: Vec<(&str, Vec<&str>, Option<&str>)> = vec![
        ("admin_addresses", vec!["admin", "addresses", "--admin", "engine-admin", "--salt", "1"], None),
        ("admin_verify", vec!["admin", "verify"], None),
        (
            "admin_init_config",
            vec!["admin", "init-config", "--admin", "engine-admin", "--salt", "2", "--params", &engine_params, "--protocol-token", &protocol],
            Some("init_config"),
        ),
        (
            "admin_set_engine_params",
            vec!["admin", "set-engine-params", "--admin", "engine-admin", "--params", &engine_params, "--protocol-token", &protocol],
            Some("set_engine_params"),
        ),
        (
            "admin_set_collateral_config",
            vec!["admin", "set-collateral-config", "--admin", "engine-admin", "--params", &collateral_params],
            Some("set_collateral_config"),
        ),
    ];
    for (name, args, sent) in cases {
        let c = Case::new();
        let out = c.run(&args);
        assert!(out.ends_with("exit: 0\n"), "{name}: {out}");
        assert_eq!(sent_engine_names(&c), sent.map(|s| vec![s.to_owned()]).unwrap_or_default(), "{name}");
        check(name, &out);
    }
    let c = Case::new();
    let out = c.run(&["admin", "addresses", "--admin", "engine-admin", "--salt", "1"]);
    assert_eq!(value(&out, "config"), standard::config_id().to_string());
}

#[test]
fn host_bind() {
    let c = Case::new();
    let out = c.run(&["host", "bind", "--admin", "host-admin"]);
    assert!(out.contains("verified"), "{out}");
    assert!(sent_host_debug(&c).starts_with("SetLiquidationEngine"));
    check("host_bind", &out);
}

#[test]
fn bind_wrong_host() {
    let c = Case {
        deployment: Some(Deployment {
            host_program: [0xA2; 8],
            ..standard::deployment()
        }),
        ..Case::new()
    };
    let out = c.run(&["host", "bind", "--admin", "host-admin"]);
    assert!(out.contains("verification failed: host program id stored in the config"), "{out}");
    assert!(out.ends_with("exit: 2\n"), "{out}");
    assert!(c.fx.chain.sent().is_empty(), "nothing is bound");
    check("bind_wrong_host", &out);
}

#[test]
fn host_set_controller_gains() {
    let c = Case::new();
    let out = c.run(&["host", "set-controller-gains", "--admin", "host-admin", "--kp", "-1000000000000000000000000000", "--ki", "-1"]);
    assert!(out.ends_with("exit: 0\n"), "{out}");
    let ix = sent_host_debug(&c);
    assert!(ix.contains("new_proportional_gain: -1000000000000000000000000000"), "{ix}");
    assert!(ix.contains("new_integral_gain: -1"), "{ix}");
    check("host_set_controller_gains", &out);
}

#[test]
fn host_and_dev_commands() {
    let col = standard::COLLATERAL.to_string();
    let oracle = standard::ORACLE.to_string();
    let owner_coll_holding = auction_sdk::testing::id(standard::k::OWNER_COLL).to_string();
    let cases: Vec<(&str, Vec<&str>)> = vec![
        ("host_addresses", vec!["host", "addresses"]),
        (
            "host_init_program",
            vec!["host", "init-program", "--admin", "host-admin", "--freeze", "freeze", "--collateral-def", &col, "--oracle", &oracle, "--kp", "-1"],
        ),
        ("host_open_position", vec!["host", "open-position", "--owner", "owner", "--owner-collateral", "owner-coll", "--nonce", "3", "--amount", "500000"]),
        ("dev_resolve", vec!["dev", "resolve", "--label", "keeper"]),
        ("dev_token_new", vec!["dev", "token-new", "--definition", "keeper", "--holding", "keeper-coll", "--name", "MOCK", "--supply", "10000"]),
        ("dev_oracle_create", vec!["dev", "oracle-create", "--price-source", "caller", "--base", &col, "--quote", &col, "--price", "1000"]),
        ("dev_balance", vec!["dev", "balance", "--account", &owner_coll_holding]),
    ];
    for (name, args) in cases {
        let c = Case::new();
        let out = c.run(&args);
        assert!(out.ends_with("exit: 0\n"), "{name}: {out}");
        check(name, &out);
    }
    let c = Case::new();
    c.run(&["host", "init-program", "--admin", "host-admin", "--freeze", "freeze", "--collateral-def", &col, "--oracle", &oracle, "--kp", "-1"]);
    assert!(sent_host_debug(&c).contains("initial_controller_proportional_gain: -1"));
}
```

`dev_token_new` uses keeper/keeper-coll as definition/holding accounts only because they exist in the test wallet; the FixtureChain does not execute it.

- [ ] **Step 3: Run to verify they fail**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_cli'`
Expected: the new tests panic with `arguments parse` (`unrecognized subcommand 'admin'`).

- [ ] **Step 4: Arguments**

Append to `cli/src/args.rs`:

```rust
#[derive(Debug, Subcommand)]
pub enum AdminCommand {
    /// Print the PDAs of the config `["CONFIG", admin, salt]` (no transaction).
    Addresses {
        #[arg(long)]
        admin: String,
        #[arg(long, default_value_t = 1)]
        salt: u64,
    },
    /// init_config: the signer becomes the config's admin (ADR-027).
    InitConfig {
        #[arg(long)]
        admin: String,
        #[arg(long, default_value_t = 1)]
        salt: u64,
        #[arg(long)]
        params: PathBuf,
        #[arg(long)]
        protocol_token: Option<String>,
        #[arg(long)]
        ref_oracle: Option<String>,
    },
    SetEngineParams {
        #[arg(long)]
        admin: String,
        #[arg(long)]
        params: PathBuf,
        #[arg(long)]
        protocol_token: Option<String>,
        #[arg(long)]
        ref_oracle: Option<String>,
    },
    SetCollateralConfig {
        #[arg(long)]
        admin: String,
        #[arg(long)]
        params: PathBuf,
    },
    /// Verify the config against the deployment file and print what a host admin must check (ADR-027).
    Verify,
}

#[derive(Debug, Subcommand)]
pub enum HostCommand {
    /// Print the host PDAs (no transaction).
    Addresses,
    /// set_liquidation_engine, only after verify_config accepts the config (ADR-027).
    Bind {
        #[arg(long)]
        admin: String,
    },
    InitProgram(InitProgramCmd),
    OpenPosition {
        #[arg(long)]
        owner: String,
        #[arg(long)]
        owner_collateral: String,
        #[arg(long, default_value_t = 0)]
        nonce: u64,
        #[arg(long)]
        amount: u128,
    },
    GenerateDebt {
        #[arg(long)]
        owner: String,
        #[arg(long)]
        position: String,
        #[arg(long)]
        owner_stable: String,
        #[arg(long)]
        amount: u128,
    },
    SetMinCr {
        #[arg(long)]
        admin: String,
        #[arg(long)]
        ratio: u128,
    },
    /// Signed gains (spel cannot encode i128, ADR-018).
    SetControllerGains {
        #[arg(long)]
        admin: String,
        #[arg(long, allow_hyphen_values = true)]
        kp: i128,
        #[arg(long, allow_hyphen_values = true)]
        ki: i128,
    },
}

#[derive(Debug, Args)]
pub struct InitProgramCmd {
    #[arg(long)]
    pub admin: String,
    #[arg(long)]
    pub freeze: String,
    #[arg(long)]
    pub collateral_def: String,
    #[arg(long)]
    pub oracle: String,
    #[arg(long, allow_hyphen_values = true, default_value_t = 0)]
    pub kp: i128,
    #[arg(long, allow_hyphen_values = true, default_value_t = 0)]
    pub ki: i128,
    #[arg(long, default_value = "1500000000000000000000000000")]
    pub min_cr: u128,
    #[arg(long, default_value = "1000000000000000000000000000")]
    pub redemption_price: u128,
    #[arg(long, default_value = "1000000000000000000000000000")]
    pub stability_fee: u128,
    #[arg(long, default_value_t = 60_000)]
    pub min_ms_between_updates: u64,
    #[arg(long, default_value_t = 3_600_000)]
    pub max_oracle_age_ms: u64,
    #[arg(long, default_value = "LSC")]
    pub name: String,
}

#[derive(Debug, Subcommand)]
pub enum DevCommand {
    /// Print the base58 id of a wallet label.
    Resolve {
        #[arg(long)]
        label: String,
    },
    TokenNew {
        #[arg(long)]
        definition: String,
        #[arg(long)]
        holding: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        supply: u128,
        #[arg(long)]
        mint_authority: Option<String>,
    },
    TokenInitAccount {
        #[arg(long)]
        definition: String,
        #[arg(long)]
        account: String,
    },
    TokenTransfer {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u128,
    },
    /// twap_oracle CreateOraclePriceAccount (needs `twap_oracle_program_id` in the deployment file).
    OracleCreate {
        #[arg(long)]
        price_source: String,
        #[arg(long)]
        base: String,
        #[arg(long)]
        quote: String,
        #[arg(long)]
        price: u128,
        #[arg(long, default_value_t = 3_600_000)]
        window_ms: u64,
    },
    /// Token-holding balance (no transaction).
    Balance {
        #[arg(long)]
        account: String,
    },
}
```

and these `Command` variants:

```rust
    #[command(subcommand)]
    Admin(AdminCommand),
    #[command(subcommand)]
    Host(HostCommand),
    #[command(subcommand)]
    Dev(DevCommand),
```

In `lib.rs` add `mod admin_cmds; mod params;` and the arms `Command::Admin(c) => admin_cmds::admin(env, c, out), Command::Host(c) => admin_cmds::host(env, c, out), Command::Dev(c) => admin_cmds::dev(env, c, out),`.

- [ ] **Step 5: `params.rs`**

```rust
//! Parameter files of `admin init-config` / `set-engine-params` / `set-collateral-config`. `u128` values
//! are decimal strings (JSON numbers cannot carry them safely); the two optional-slot flags come from
//! `--protocol-token` / `--ref-oracle`, never from the file, so they cannot disagree with the slots.

use std::path::Path;

use auction_engine_core::args::{CollateralArgs, EngineParamsArgs};
use serde::{de::DeserializeOwned, Deserialize};

use crate::error::CliError;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineParamsFile {
    global_cap: String,
    freeze_on_bad_debt: bool,
    bad_debt_freeze_threshold: String,
    protocol_token_ref_price: String,
    ref_oracle_max_age_ms: u64,
    surplus_buffer_target: String,
    surplus_lot: String,
    surplus_delay_ms: u64,
    surplus_premium_bps: u32,
    surplus_max_discount_bps: u32,
    debt_lot: String,
    debt_initial_discount_bps: u32,
    debt_growth: Vec<u32>,
    surplus_decay: Vec<u32>,
    max_mint_per_auction: String,
    min_take: String,
    system_ttl_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CollateralFile {
    oracle_scale: u8,
    max_oracle_age_ms: u64,
    max_conf_bps: u32,
    band_lo_bps: u32,
    band_hi_bps: u32,
    penalty_bps: u32,
    tip: String,
    chip_bps: u32,
    reward_cap: String,
    debt_floor: String,
    min_bid: String,
    type_cap: String,
    schedule: Vec<u32>,
    admin_paused: bool,
}

fn read<T: DeserializeOwned>(path: &Path) -> Result<T, CliError> {
    let text = std::fs::read_to_string(path).map_err(|e| CliError::Usage(format!("{}: {e}", path.display())))?;
    serde_json::from_str(&text).map_err(|e| CliError::Usage(format!("{}: {e}", path.display())))
}

fn big(s: &str, field: &str) -> Result<u128, CliError> {
    s.parse::<u128>()
        .map_err(|_| CliError::Usage(format!("{field} must be a decimal u128 string")))
}

pub fn engine_params(
    path: &Path,
    protocol_token_enabled: bool,
    ref_oracle_enabled: bool,
) -> Result<EngineParamsArgs, CliError> {
    let f: EngineParamsFile = read(path)?;
    let p = EngineParamsArgs {
        global_cap: big(&f.global_cap, "global_cap")?,
        freeze_on_bad_debt: f.freeze_on_bad_debt,
        bad_debt_freeze_threshold: big(&f.bad_debt_freeze_threshold, "bad_debt_freeze_threshold")?,
        protocol_token_enabled,
        protocol_token_ref_price: big(&f.protocol_token_ref_price, "protocol_token_ref_price")?,
        ref_oracle_enabled,
        ref_oracle_max_age_ms: f.ref_oracle_max_age_ms,
        surplus_buffer_target: big(&f.surplus_buffer_target, "surplus_buffer_target")?,
        surplus_lot: big(&f.surplus_lot, "surplus_lot")?,
        surplus_delay_ms: f.surplus_delay_ms,
        surplus_premium_bps: f.surplus_premium_bps,
        surplus_max_discount_bps: f.surplus_max_discount_bps,
        debt_lot: big(&f.debt_lot, "debt_lot")?,
        debt_initial_discount_bps: f.debt_initial_discount_bps,
        debt_growth: f.debt_growth,
        surplus_decay: f.surplus_decay,
        max_mint_per_auction: big(&f.max_mint_per_auction, "max_mint_per_auction")?,
        min_take: big(&f.min_take, "min_take")?,
        system_ttl_ms: f.system_ttl_ms,
    };
    // Same rules the engine applies at config time (ADR-024): fail before signing anything.
    p.validate()
        .map_err(|e| CliError::Usage(format!("invalid engine parameters: {e}")))?;
    Ok(p)
}

pub fn collateral_args(path: &Path) -> Result<CollateralArgs, CliError> {
    let f: CollateralFile = read(path)?;
    Ok(CollateralArgs {
        oracle_scale: f.oracle_scale,
        max_oracle_age_ms: f.max_oracle_age_ms,
        max_conf_bps: f.max_conf_bps,
        band_lo_bps: f.band_lo_bps,
        band_hi_bps: f.band_hi_bps,
        penalty_bps: f.penalty_bps,
        tip: big(&f.tip, "tip")?,
        chip_bps: f.chip_bps,
        reward_cap: big(&f.reward_cap, "reward_cap")?,
        debt_floor: big(&f.debt_floor, "debt_floor")?,
        min_bid: big(&f.min_bid, "min_bid")?,
        type_cap: big(&f.type_cap, "type_cap")?,
        schedule: f.schedule,
        admin_paused: f.admin_paused,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_localnet_files_parse_and_validate() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("params");
        let p = engine_params(&dir.join("localnet-engine.json"), true, false).expect("engine params");
        assert_eq!(p.protocol_token_ref_price, 2_000_000_000_000_000_000_000_000_000);
        assert!(p.protocol_token_enabled && !p.ref_oracle_enabled);
        let c = collateral_args(&dir.join("localnet-collateral.json")).expect("collateral");
        assert_eq!(c.validate(p.global_cap).expect("valid").0.min_bid, 25);
    }

    #[test]
    fn a_bad_number_names_its_field() {
        let dir = tempfile::tempdir().expect("tmp");
        let f = dir.path().join("p.json");
        let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("params/localnet-engine.json"))
            .expect("read")
            .replace("\"10000000\"", "\"ten\"");
        std::fs::write(&f, text).expect("write");
        let e = engine_params(&f, false, false).expect_err("bad number");
        assert!(e.to_string().contains("global_cap"), "{e}");
    }
}
```

- [ ] **Step 6: `admin_cmds.rs`**

```rust
//! Admin, host and dev commands: everything the localnet runbook needs without spel. `host bind` runs
//! the ADR-027 bind-time verification first and refuses on any mismatch.

use std::io::Write;

use auction_sdk::{
    addresses::{EngineAddresses, HostAddresses},
    chain::Chain,
    decode::Reader,
    deployment::program_id_hex,
    error::SdkError,
    plan, plan_host,
    verify::verify_binding,
};
use lee_core::account::Account;
use stablecoin_core::ProtocolParameters;
use token_core::TokenHolding;

use crate::{
    args::{AdminCommand, DevCommand, HostCommand, InitProgramCmd},
    env::Env,
    error::CliError,
    output::{kv, yes_no},
    params,
};

fn host_addresses<C: Chain>(env: &Env<C>) -> HostAddresses {
    HostAddresses::new(env.deployment.host_program)
}

/// Before a config exists: the host and the token program on this node are the trusted ones.
fn check_network<C: Chain>(env: &Env<C>, host: &HostAddresses) -> Result<(), CliError> {
    let pp = env.chain.account(&host.protocol_parameters)?;
    if pp.program_owner != env.deployment.host_program {
        return Err(CliError::Verify(SdkError::Mismatch {
            what: "owner of the host ProtocolParameters (wrong network?)",
            expected: program_id_hex(&env.deployment.host_program),
            found: program_id_hex(&pp.program_owner),
        }));
    }
    let sdef = env.chain.account(&host.stablecoin_definition)?;
    if sdef.program_owner != env.deployment.token_program {
        return Err(CliError::Verify(SdkError::Mismatch {
            what: "token program (owner of the stablecoin definition)",
            expected: program_id_hex(&env.deployment.token_program),
            found: program_id_hex(&sdef.program_owner),
        }));
    }
    Ok(())
}

fn protocol_parameters<C: Chain>(env: &Env<C>, host: &HostAddresses) -> Result<ProtocolParameters, CliError> {
    Ok(Reader::new(&env.chain, env.deployment.engine_program).host(
        &host.protocol_parameters,
        &host.program,
        "ProtocolParameters",
    )?)
}

fn optional<C: Chain>(env: &Env<C>, arg: Option<&String>) -> Result<Option<lee_core::account::AccountId>, CliError> {
    arg.map(|s| env.account(s)).transpose()
}

pub fn admin<C: Chain>(env: &Env<C>, cmd: &AdminCommand, out: &mut dyn Write) -> Result<(), CliError> {
    match cmd {
        AdminCommand::Addresses { admin, salt } => {
            let e = EngineAddresses::new(env.deployment.engine_program, &env.account(admin)?, *salt);
            kv(out, "config", &e.config.to_string())?;
            kv(out, "accounting", &e.accounting.to_string())?;
            kv(out, "buffer", &e.buffer.to_string())?;
            kv(out, "engine authority", &e.engine_authority.to_string())?;
            kv(out, "mint authority", &e.mint_authority.to_string())?;
            kv(out, "unused slot 0", &e.unused_ref_oracle.to_string())?;
            kv(out, "unused slot 1", &e.unused_protocol_token.to_string())
        }
        AdminCommand::InitConfig { admin, salt, params: file, protocol_token, ref_oracle } => {
            let host = host_addresses(env);
            check_network(env, &host)?;
            let admin = env.account(admin)?;
            let token = optional(env, protocol_token.as_ref())?;
            let oracle = optional(env, ref_oracle.as_ref())?;
            let p = params::engine_params(file, token.is_some(), oracle.is_some())?;
            let e = EngineAddresses::new(env.deployment.engine_program, &admin, *salt);
            kv(out, "config", &e.config.to_string())?;
            kv(out, "ref price (RAY)", &p.protocol_token_ref_price.to_string())?;
            kv(out, "record", "the initial reference price in DEPLOYMENTS.md (ADR-017)")?;
            let plan = plan::init_config(env.deployment.engine_program, &host, admin, *salt, &p, token, oracle)?;
            env.send(&plan, "init_config", out)
        }
        AdminCommand::SetEngineParams { admin, params: file, protocol_token, ref_oracle } => {
            let v = env.verified()?;
            let token = optional(env, protocol_token.as_ref())?;
            let oracle = optional(env, ref_oracle.as_ref())?;
            let p = params::engine_params(file, token.is_some(), oracle.is_some())?;
            if p.protocol_token_ref_price != v.state.protocol_token_ref_price {
                kv(
                    out,
                    "ref price change",
                    &format!(
                        "{} -> {} (record it in DEPLOYMENTS.md, ADR-017)",
                        v.state.protocol_token_ref_price, p.protocol_token_ref_price
                    ),
                )?;
            }
            let plan = plan::set_engine_params(&v, env.account(admin)?, &p, token, oracle)?;
            env.send(&plan, "set_engine_params", out)
        }
        AdminCommand::SetCollateralConfig { admin, params: file } => {
            let v = env.verified()?;
            let c = params::collateral_args(file)?;
            let oracle = protocol_parameters(env, &v.host)?.market_price_oracle_id; // ADR-028
            let plan = plan::set_collateral_config(&v, env.account(admin)?, oracle, &c)?;
            env.send(&plan, "set_collateral_config", out)
        }
        AdminCommand::Verify => {
            let v = env.verified()?;
            kv(out, "verified", "config owner, address, host and token program ids, admin, host ProtocolParameters")?;
            kv(out, "config", &v.config_id.to_string())?;
            kv(out, "admin", &v.state.admin.to_string())?;
            kv(out, "salt", &v.state.salt.to_string())?;
            kv(out, "host program", &program_id_hex(&v.state.host_program_id))?;
            kv(out, "token program", &program_id_hex(&v.state.token_program_id))?;
            kv(
                out,
                "protocol token",
                &v.state.protocol_token.map_or_else(|| "none".to_owned(), |t| t.to_string()),
            )?;
            kv(out, "ref price (RAY)", &v.state.protocol_token_ref_price.to_string())?;
            kv(out, "freeze threshold", &v.state.bad_debt_freeze_threshold.to_string())?;
            kv(out, "freeze on bad debt", yes_no(v.state.freeze_on_bad_debt))?;
            kv(out, "global cap", &v.state.global_cap.to_string())?;
            let bound = match verify_binding(&env.chain, &v) {
                Ok(()) => "yes".to_owned(),
                Err(e) => format!("no ({e})"),
            };
            kv(out, "bound by the host", &bound)
        }
    }
}

fn init_program_args<C: Chain>(env: &Env<C>, a: &InitProgramCmd) -> Result<plan_host::InitializeProgramArgs, CliError> {
    Ok(plan_host::InitializeProgramArgs {
        admin: env.account(&a.admin)?,
        freeze_authority: env.account(&a.freeze)?,
        collateral_definition: env.account(&a.collateral_def)?,
        market_price_oracle: env.account(&a.oracle)?,
        stability_fee_per_ms: a.stability_fee,
        proportional_gain: a.kp,
        integral_gain: a.ki,
        min_collateralization_ratio: a.min_cr,
        min_ms_between_rate_updates: a.min_ms_between_updates,
        max_oracle_age_ms: a.max_oracle_age_ms,
        redemption_price: a.redemption_price,
        stablecoin_name: a.name.clone(),
    })
}

pub fn host<C: Chain>(env: &Env<C>, cmd: &HostCommand, out: &mut dyn Write) -> Result<(), CliError> {
    let host = host_addresses(env);
    match cmd {
        HostCommand::Addresses => {
            kv(out, "protocol parameters", &host.protocol_parameters.to_string())?;
            kv(out, "stablecoin definition", &host.stablecoin_definition.to_string())?;
            kv(out, "stablecoin master", &host.stablecoin_master_holding.to_string())?;
            kv(out, "stability fee acc", &host.stability_fee_accumulator.to_string())?;
            kv(out, "redemption state", &host.redemption_price_state.to_string())?;
            kv(out, "engine binding", &host.liquidation_engine_state.to_string())
        }
        HostCommand::Bind { admin } => {
            // ADR-027 bind-time verification: refuses (exit 2) unless the config is the trusted one.
            let v = env.verified()?;
            kv(out, "verified", "config owner, address, host and token program ids, admin, host ProtocolParameters")?;
            kv(out, "config", &v.config_id.to_string())?;
            kv(out, "engine authority", &v.engine.engine_authority.to_string())?;
            let plan = plan_host::set_liquidation_engine(&host, env.account(admin)?, v.engine.program, v.engine.engine_authority)?;
            env.send(&plan, "set_liquidation_engine", out)
        }
        HostCommand::InitProgram(a) => {
            let plan = plan_host::initialize_program(&host, &init_program_args(env, a)?)?;
            env.send(&plan, "initialize_program", out)
        }
        HostCommand::OpenPosition { owner, owner_collateral, nonce, amount } => {
            let owner = env.account(owner)?;
            let collateral = protocol_parameters(env, &host)?.collateral_definition_id;
            kv(out, "position", &host.position(&owner, *nonce).to_string())?;
            let plan = plan_host::open_position(&host, owner, env.account(owner_collateral)?, collateral, *nonce, *amount)?;
            env.send(&plan, "open_position", out)
        }
        HostCommand::GenerateDebt { owner, position, owner_stable, amount } => {
            let oracle = protocol_parameters(env, &host)?.market_price_oracle_id;
            let plan = plan_host::generate_debt(
                &host,
                env.account(owner)?,
                env.account(position)?,
                env.account(owner_stable)?,
                oracle,
                *amount,
            )?;
            env.send(&plan, "generate_debt", out)
        }
        HostCommand::SetMinCr { admin, ratio } => {
            let plan = plan_host::set_minimum_collateralization_ratio(&host, env.account(admin)?, *ratio)?;
            env.send(&plan, "set_minimum_collateralization_ratio", out)
        }
        HostCommand::SetControllerGains { admin, kp, ki } => {
            let plan = plan_host::set_controller_gains(&host, env.account(admin)?, *kp, *ki)?;
            env.send(&plan, "set_controller_gains", out)
        }
    }
}

pub fn dev<C: Chain>(env: &Env<C>, cmd: &DevCommand, out: &mut dyn Write) -> Result<(), CliError> {
    let token = env.deployment.token_program;
    match cmd {
        DevCommand::Resolve { label } => kv(out, "id", &env.keystore()?.resolve(label)?.to_string()),
        DevCommand::TokenNew { definition, holding, name, supply, mint_authority } => {
            let definition = env.account(definition)?;
            let authority = optional(env, mint_authority.as_ref())?;
            kv(out, "definition", &definition.to_string())?;
            let plan = plan_host::token_new_fungible(token, definition, env.account(holding)?, name, *supply, authority)?;
            env.send(&plan, "new_fungible_definition", out)
        }
        DevCommand::TokenInitAccount { definition, account } => {
            let plan = plan_host::token_initialize_account(token, env.account(definition)?, env.account(account)?)?;
            env.send(&plan, "initialize_account", out)
        }
        DevCommand::TokenTransfer { from, to, amount } => {
            let plan = plan_host::token_transfer(token, env.account(from)?, env.account(to)?, *amount)?;
            env.send(&plan, "transfer", out)
        }
        DevCommand::OracleCreate { price_source, base, quote, price, window_ms } => {
            let twap = env.deployment.twap_oracle_program.ok_or_else(|| {
                CliError::Usage("the deployment file has no twap_oracle_program_id".into())
            })?;
            let (plan, oracle) = plan_host::create_oracle_price_account(
                twap,
                env.account(price_source)?,
                env.account(base)?,
                env.account(quote)?,
                *price,
                *window_ms,
            )?;
            kv(out, "oracle", &oracle.to_string())?;
            env.send(&plan, "create_oracle_price_account", out)
        }
        DevCommand::Balance { account } => {
            let id = env.account(account)?;
            let acc = env.chain.account(&id)?;
            kv(out, "account", &id.to_string())?;
            if acc == Account::default() {
                return kv(out, "state", "uninitialised");
            }
            match TokenHolding::try_from(&acc.data) {
                Ok(TokenHolding::Fungible { definition_id, balance }) => {
                    kv(out, "definition", &definition_id.to_string())?;
                    kv(out, "balance", &balance.to_string())
                }
                _ => kv(out, "state", "not a fungible token holding"),
            }
        }
    }
}
```

Add `token_core = { workspace = true }` to `cli/Cargo.toml` `[dependencies]`.

- [ ] **Step 7: Generate and review the golden files**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && UPDATE_GOLDEN=1 scripts/wsl-cargo.sh test -p auction_cli && scripts/wsl-cargo.sh test -p auction_cli'`
Expected: second run all pass. Review: `bind_wrong_host.txt` ends with the host-program mismatch and `exit: 2` and has no `plan` line; `admin_verify.txt` shows `bound by the host  yes` and the fixture's hex ids; `dev_balance.txt` shows balance 0 of the collateral definition; `admin_init_config.txt` prints the salt-2 config id and the ref price line.

- [ ] **Step 8: README, gates, commit**

Append to `cli/README.md`:

```markdown
## Admin, host and dev commands (localnet setup without spel)

    auction-cli admin addresses --admin <label> [--salt 1]
    auction-cli admin init-config --admin <label> [--salt 1] --params cli/params/localnet-engine.json [--protocol-token <id|label>] [--ref-oracle <id>]
    auction-cli admin set-engine-params --admin <label> --params <json> [--protocol-token ...] [--ref-oracle ...]
    auction-cli admin set-collateral-config --admin <label> --params cli/params/localnet-collateral.json
    auction-cli admin verify
    auction-cli host addresses
    auction-cli host bind --admin <host admin>      # refuses unless the config verifies (ADR-027)
    auction-cli host init-program --admin <l> --freeze <l> --collateral-def <l> --oracle <id> [--kp <i128>] [--ki <i128>] ...
    auction-cli host open-position --owner <l> --owner-collateral <l> [--nonce 0] --amount <n>
    auction-cli host generate-debt --owner <l> --position <id> --owner-stable <l> --amount <n>
    auction-cli host set-min-cr --admin <l> --ratio <RAY>
    auction-cli host set-controller-gains --admin <l> --kp <i128> --ki <i128>   # negative values allowed
    auction-cli dev resolve --label <l>
    auction-cli dev token-new --definition <l> --holding <l> --name <s> --supply <n> [--mint-authority <id>]
    auction-cli dev token-init-account --definition <id|l> --account <l>
    auction-cli dev token-transfer --from <l> --to <id|l> --amount <n>
    auction-cli dev oracle-create --price-source <l> --base <id> --quote <id|l> --price <n> [--window-ms 3600000]
    auction-cli dev balance --account <id|l>

Parameter files hold `u128` values as decimal strings; the protocol-token and reference-oracle flags come from `--protocol-token` / `--ref-oracle`. An enabled protocol token must already exist as a fungible token definition (ADR-038): create it first with `dev token-new ... --mint-authority <mint authority from admin addresses>`. Every reference-price change printed by `set-engine-params` goes into `DEPLOYMENTS.md` (ADR-017).
```

Run the lint gate and the format gate (with `-p auction_sdk -p auction_cli`). Expected: clean.

Commit (controller — GPG signing on Windows):

```bash
git add cli Cargo.lock
git commit -m "feat(cli): admin, host and dev commands with bind-time verification"
```

---

### Task 16: CLI-driven localnet smoke (spel removed), refreshed deployment record, Plan 3a wrap-up

The Plan 2 runbook, re-run on a fresh standalone LEZ v0.2.4 sequencer with every program instruction sent by `auction-cli` (encoded by the SDK by enum index). Adds a partial bid, `observe_feed`, `settle_debt`, `check_solvency` and a full F5 surplus auction (the protocol token is configured, ADR-038). F4 and the freeze path need a shortfall, which needs a moving oracle price: Plan 3b (oracle publisher, one-command demo).

**Files:**
- Modify (rewrite): `scripts/localnet-smoke.sh`
- Modify: `docs/architecture/localnet.md`, `DEPLOYMENTS.md`, `deployments/localnet.json`, `docs/architecture/programs.md`, `docs/architecture/sdk.md`, `README.md`

**Interfaces:**
- Consumes: the `auction-cli` binary (Tasks 13–15), `localnet_tool export-guests` (Plan 2), the S1 wallet, sequencer and Docker-built upstream guests.
- Produces: a recorded localnet run (transaction outcomes and final balances in `docs/architecture/localnet.md`), the refreshed native-build ImageIDs in `DEPLOYMENTS.md` and `deployments/localnet.json` (still native, THREAT_MODEL I4; Plan 3b replaces them with Docker-built ids), the initial reference price in the `DEPLOYMENTS.md` log.

- [ ] **Step 1: Rewrite `scripts/localnet-smoke.sh`**

```bash
#!/usr/bin/env bash
# Plan 3a Task 16 localnet smoke: the Plan 2 runbook with auction-cli for every program instruction (no spel).
# Run INSIDE WSL. Steps, in order (several may be given in one call):
#   step_build step_sequencer step_accounts step_auth step_deploy step_host step_engine step_bind
#   step_collateral step_holdings step_flow step_system step_check step_stop
# Requires the S1 builds (sequencer_service, wallet) and S1's Docker-built upstream guests (token, twap_oracle,
# token_mint_authority). The wallet binary still does wallet business: key creation, auth-transfer init and
# program deployment. Only the sequencer started by step_sequencer (pid file) is ever signalled.
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
S1="$HOME/lez-spikes/s1"
LN="$HOME/lez-target/plan3a-localnet"
W="$S1/lez/target/debug/wallet"
UP="$S1/lez-programs/target/guest"
CLI_BIN="${CARGO_TARGET_DIR:-$HOME/lez-target/plan2}/debug/auction-cli"
export LEE_WALLET_HOME_DIR="$LN/wallet-home"
# shellcheck disable=SC1091
. "$S1/env.sh"
mkdir -p "$LN/logs" "$LN/guest"
# Fixed by S1 (same upstream binaries -> same ids):
TOK=14b3bc6cc129d0359f9595ba0f60a13e4a8df08ffe8eb1a83251c47abecc67a8
TWAP=5681e27c27b3ece4a7c161bbf8e12e6ece6ca69925a87b77f9e193fa4a249b31
ONE=1000000000000000000000000000
DEP="$LN/deployment.json"
LABELS="admin freeze oracle-src coll-def owner owner-coll owner-stable engine-admin keeper keeper-coll caller protocol-def taker-protocol taker-stable"

cli() { "$CLI_BIN" --deployment "$DEP" --state-dir "$LN/cli-state" --wallet-home "$LEE_WALLET_HOME_DIR" "$@"; }
# Last field of the first `key  value` line starting with $1; reads all input (no SIGPIPE under pipefail).
field() { awk -v k="$1" 'index($0, k) == 1 && !done { print $NF; done = 1 }'; }
# Runs auction-cli, shows its output on stderr, prints the value of key $1.
run_field() { local key="$1"; shift; cli "$@" | tee /dev/stderr | field "$key"; }
id_of() { cli dev resolve --label "$1" | field id; }
remember() { printf '%s=%s\n' "$1" "$2" >> "$LN/run.env"; }
recall() { sed -n "s/^$1=//p" "$LN/run.env" | tail -1; }

set_deployment_field() {
  python3 - "$DEP" "$1" "$2" <<'PY'
import json, sys
path, key, value = sys.argv[1:]
d = json.load(open(path, encoding="utf-8"))
d[key] = value
json.dump(d, open(path, "w", encoding="utf-8"), indent=2)
PY
}

step_build() {
  (cd "$REPO" && scripts/wsl-cargo.sh build -p auction_cli)
  (cd "$REPO" && scripts/wsl-cargo.sh run -q -p engine_integration_tests --example localnet_tool -- export-guests "$LN/guest") \
    | tee "$LN/guest/ids.txt"
  local engine host
  engine="$(awk '/^AUCTION_ENGINE_ID/ { print $2 }' "$LN/guest/ids.txt")"
  host="$(awk '/^STABLECOIN_ID/ { print $2 }' "$LN/guest/ids.txt")"
  python3 - "$DEP" "$engine" "$host" "$TOK" "$TWAP" <<'PY'
import json, sys
path, engine, host, tok, twap = sys.argv[1:]
json.dump({"network": "localnet", "sequencer_url": "http://127.0.0.1:3040", "engine_program_id": engine,
           "host_program_id": host, "token_program_id": tok, "twap_oracle_program_id": twap,
           "engine_admin": None, "config": None}, open(path, "w", encoding="utf-8"), indent=2)
PY
  echo "deployment file: $DEP"
}

step_sequencer() {
  if (ss -ltn 2>/dev/null || true) | grep -q ':3040 '; then
    echo "port 3040 is already in use; not starting a second sequencer" >&2
    exit 1
  fi
  rm -rf "$LN/seqhome" && mkdir -p "$LN/seqhome"
  (cd "$S1/lez" && RISC0_DEV_MODE=1 RUST_LOG=info setsid nohup target/debug/sequencer_service \
    lez/sequencer/service/configs/debug/sequencer_config.json --home "$LN/seqhome" --listen-address 127.0.0.1 -p 3040 \
    > "$LN/logs/sequencer.log" 2>&1 < /dev/null &)
  rm -f "$LN/sequencer.pid"
  local pid="" i
  for i in $(seq 1 30); do
    pid="$(ss -ltnp 2>/dev/null | sed -n 's/.*:3040 .*pid=\([0-9]*\).*/\1/p' | head -1)"
    [[ -n "$pid" ]] && break
    sleep 2
  done
  if [[ -z "$pid" ]]; then
    echo "sequencer did not open port 3040 within 60 s; see $LN/logs/sequencer.log" >&2
    exit 1
  fi
  echo "$pid" > "$LN/sequencer.pid"
}

step_accounts() {
  mkdir -p "$LEE_WALLET_HOME_DIR" && cp "$S1/lez/lez/wallet/configs/debug/wallet_config.json" "$LEE_WALLET_HOME_DIR/"
  local l
  for l in $LABELS; do echo pw | "$W" account new public --label "$l"; done
  : > "$LN/run.env"
}

step_auth() {
  # Keys that sign more than one transaction must be auth-transfer-initialised before first use (ADR-018).
  local l
  for l in admin engine-admin keeper owner caller; do "$W" auth-transfer init --account-id "Public/$(id_of "$l")"; done
}

step_deploy() {
  local p
  for p in token twap_oracle token_mint_authority; do "$W" deploy-program "$UP/$p.bin"; done
  "$W" deploy-program "$LN/guest/stablecoin.bin"
  "$W" deploy-program "$LN/guest/auction_engine.bin"
}

step_host() {
  local sdef oracle
  cli dev token-new --definition coll-def --holding owner-coll --name COLL --supply 100000000000
  sdef="$(run_field "stablecoin definition" host addresses)"
  oracle="$(run_field oracle dev oracle-create --price-source oracle-src --base "$sdef" --quote coll-def \
    --price "$ONE" --window-ms 3600000)"
  remember SDEF "$sdef"
  remember ORACLE "$oracle"
  cli host init-program --admin admin --freeze freeze --collateral-def coll-def --oracle "$oracle" \
    --min-cr 1500000000000000000000000000 --redemption-price "$ONE"
}

step_engine() {
  local mint config admin
  mint="$(run_field "mint authority" admin addresses --admin engine-admin --salt 1)"
  config="$(run_field config admin addresses --admin engine-admin --salt 1)"
  admin="$(id_of engine-admin)"
  # ADR-038: the protocol token must exist (mint authority = this config's MINT_AUTH PDA) before the config names it.
  cli dev token-new --definition protocol-def --holding taker-protocol --name MOCK --supply 10000 --mint-authority "$mint"
  cli admin init-config --admin engine-admin --salt 1 --params "$REPO/cli/params/localnet-engine.json" --protocol-token protocol-def
  set_deployment_field config "$config"
  set_deployment_field engine_admin "$admin"
}

step_bind() {
  # Bind-time verification (ADR-027) is part of the command: it refuses (exit 2) unless the config verifies.
  cli host bind --admin admin
}

step_collateral() {
  cli admin set-collateral-config --admin engine-admin --params "$REPO/cli/params/localnet-collateral.json"
}

step_holdings() {
  local sdef
  sdef="$(recall SDEF)"
  cli dev token-init-account --definition "$sdef" --account owner-stable
  cli dev token-init-account --definition coll-def --account keeper-coll
  cli dev token-init-account --definition "$sdef" --account taker-stable
}

step_flow() {
  local p0 p1 a
  p0="$(run_field position host open-position --owner owner --owner-collateral owner-coll --nonce 0 --amount 500000)"
  cli host generate-debt --owner owner --position "$p0" --owner-stable owner-stable --amount 300000
  p1="$(run_field position host open-position --owner owner --owner-collateral owner-coll --nonce 1 --amount 1000000)"
  cli host generate-debt --owner owner --position "$p1" --owner-stable owner-stable --amount 100000
  cli host set-min-cr --admin admin --ratio 2000000000000000000000000000
  cli list-at-risk
  cli quote-liquidation --position "$p0"
  a="$(run_field auction liquidate --position "$p0" --keeper keeper --keeper-holding keeper-coll)"
  remember P0 "$p0"
  remember AUCTION "$a"
  cli list-auctions
  cli quote-bid --auction "$a" --coin-in-max 100000
  cli bid --auction "$a" --coin-in-max 100000 --bidder-stable owner-stable --bidder-collateral owner-coll
  cli bid --auction "$a" --coin-in-max 230000 --bidder-stable owner-stable --bidder-collateral owner-coll
  cli claim-leftover --owner owner --owner-holding owner-coll --auction "$a"
  cli list-auctions --all
}

step_system() {
  cli observe-feed --caller caller
  cli settle-debt --caller caller
  cli check-solvency --caller caller
  cli start-surplus --caller caller
  cli take-surplus --payer taker-protocol --receiver taker-stable --amount 500
  cli list-auctions --all
}

step_check() {
  cli admin verify
  local a
  for a in owner-coll owner-stable keeper-coll taker-stable taker-protocol; do cli dev balance --account "$a"; done
  cli dev balance --account "$(run_field buffer admin addresses --admin engine-admin --salt 1)"
  grep -E "failed execution check" "$LN/logs/sequencer.log" | tail -20 || echo "no failed execution check"
}

step_stop() {
  local pid
  pid="$(cat "$LN/sequencer.pid" 2>/dev/null || true)"
  if [[ ! "$pid" =~ ^[0-9]+$ ]]; then
    echo "no sequencer pid recorded in $LN/sequencer.pid; nothing stopped (check manually, do not pkill by name)" >&2
    exit 1
  fi
  kill "$pid" 2>/dev/null || echo "pid $pid was not running" >&2
  rm -f "$LN/sequencer.pid"
}

for step in "$@"; do
  case "$step" in
    step_*) "$step" ;;
    *) echo "unknown step: $step" >&2; exit 2 ;;
  esac
done
```

- [ ] **Step 2: Static check**

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && bash -n scripts/localnet-smoke.sh && (command -v shellcheck >/dev/null && shellcheck scripts/localnet-smoke.sh || echo "shellcheck not installed") && echo SYNTAX-OK'`
Expected: `SYNTAX-OK` (and no shellcheck finding if shellcheck is installed).

- [ ] **Step 3: Run the smoke, one group of steps at a time**

Each command below runs from Git Bash as `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/localnet-smoke.sh <steps>'` (use the Bash tool's `run_in_background` for `step_build`; the first CLI build takes minutes). After every group, read `$HOME/lez-target/plan3a-localnet/logs/sequencer.log` for `failed execution check`.

| Steps | Expected |
|---|---|
| `step_build` | `STABLECOIN_ID <hex>` and `AUCTION_ENGINE_ID <hex>` printed; `deployment file: .../deployment.json`. The engine id differs from Plan 2's `3d0d15fa...` (Tasks 1–3 changed the guest); the stablecoin id is expected to be unchanged (`91c790a2...`): if it differs, record the new value in Step 4. |
| `step_sequencer` | port 3040 listening, pid recorded |
| `step_accounts` | 14 public accounts created (labels above) |
| `step_auth` | 5 `auth-transfer init` transactions confirmed |
| `step_deploy` | 5 program deployments confirmed |
| `step_host` | `included` for COLL `new_fungible_definition`, `create_oracle_price_account`, `initialize_program` |
| `step_engine` | `included` for MOCK `new_fungible_definition` and `init_config`; the printed `ref price (RAY)` is `2000000000000000000000000000` |
| `step_bind` | `verified ...` then `included` for `set_liquidation_engine` |
| `step_collateral`, `step_holdings` | `included` for `set_collateral_config` and three `initialize_account` |
| `step_flow` | positions opened and debt generated; `list-at-risk` shows position 0 only after `set-min-cr`; `quote-liquidation` shows `amount to raise 330000`, `keeper reward 50`, `lot 499950`; liquidate `included`; first bid `bid (11 accounts, 1 signer(s))` included; second bid `bid_with_settlement (12 accounts, ...)` included; `claim-leftover` included; `list-auctions --all` shows the auction `settled` |
| `step_system` | `observe_feed`, `settle_debt` (nothing to burn), `check_solvency` (healthy), `start_surplus_auction`, `take_surplus` all `included`; `take-surplus` printed `quoted tokens 200` (the decay schedule reaches its 2500 bps floor after 1 s) |
| `step_check` | `admin verify` shows `bound by the host  yes`; balances: keeper-coll 50, owner-stable 70000, owner-coll 99998999950, taker-stable 500, taker-protocol 9800, buffer 29500; `no failed execution check` |

If a step prints `exit 3` (dropped): read the sequencer log for the reason, fix the cause (usually a stale oracle after more than an hour: re-run from `step_host` on a fresh sequencer), never edit the expected values. If the oracle goes stale mid-run (`PriceFeedStale`), the whole run took over an hour; restart from `step_sequencer` (the Plan 3b publisher removes this limit).

Then, with the sequencer still running, run the live RPC test:
`MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && LEZ_RPC_URL=http://127.0.0.1:3040 scripts/wsl-cargo.sh test -p auction_sdk --features rpc -- --ignored live_rpc'` → `1 passed`. Finally `scripts/localnet-smoke.sh step_stop`.

- [ ] **Step 4: Record the deployment**

`DEPLOYMENTS.md`: retitle the first section `## Localnet (Plan 3a CLI smoke, standalone LEZ v0.2.4 sequencer, dev-mode proofs)`; replace the hex in the `auction_engine` row (and the `stablecoin (+ hooks)` row if Step 3 printed a different id) with the value from `$HOME/lez-target/plan3a-localnet/guest/ids.txt`; keep the native-build warning paragraph unchanged except "Plan 2 smoke" → "Plan 3a smoke"; add a row to the reference-price table: Date = the date of the run (YYYY-MM-DD), Config = `localnet smoke (Plan 3a) init_config`, Old → New = `none → 2_000_000_000_000_000_000_000_000_000 (2e27 RAY)`, Reason = `initial value from cli/params/localnet-engine.json (step_engine)`.

`deployments/localnet.json`: set `engine_program_id` (and `host_program_id` if it changed) to the same hex; keep `engine_admin` and `config` `null` (they are keys of one machine's wallet; the run's own file is `$HOME/lez-target/plan3a-localnet/deployment.json`).

Run: `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_sdk --features testing localnet_json_matches_deployments_md'` → 1 passed.

- [ ] **Step 5: Documentation**

`docs/architecture/localnet.md`: insert after the title a section `## Plan 3a run (auction-cli, no spel)` containing: the command list of Step 3 (step names and what each sends), the ImageIDs of this run, a table "Step → transactions → outcome" filled from the run (transaction hashes as printed by `submitted`, first 8 hex characters plus `...`, exactly as the Plan 2 table does), the final balances table from `step_check`, and this paragraph: "All program instructions are encoded by `auction_sdk` (enum index, ADR-040): the Plan 2 workarounds are gone (sorted host IDL copy, finding 1; IDL copy without `pda` markings, finding 3; u128 copy of the i128 gains, finding 6). Bind-time verification is `auction-cli host bind` itself (ADR-027). Not covered on localnet: F4 and the freeze path (they need a shortfall, hence a moving price: Plan 3b oracle publisher), `settle_empty`, `close_*` (TTL 4 h), the private path." Rename the existing first heading's content to a section `## Plan 2 run (spel)` and keep it unchanged below as history.

`docs/architecture/programs.md`, "Interfaces for Plan 3": append "Implemented by `crates/auction_sdk` (docs/architecture/sdk.md) and `cli/` (cli/README.md), Plan 3a." In "Plan 3 carry-overs", replace "SDK encoders keyed by enum index, using the table above." with "Done (Plan 3a Tasks 4–8): SDK encoders keyed by enum index (`encode.rs`), proven on the real guests (`sdk_plans.rs`)."

`docs/architecture/sdk.md`: append "## Localnet" with one sentence pointing to `docs/architecture/localnet.md` (Plan 3a run) and to `scripts/localnet-smoke.sh`.

`README.md`: change the Plan 3a status paragraph to "**Plan 3a complete: engine hardening, SDK and CLI.**" followed by three bullets: the two engine fixes (ADR-038, ADR-039); `auction_sdk` (own encoders, quotes, plans, verification, confirmation, scanner, keystore; [sdk.md](docs/architecture/sdk.md)); `auction-cli` and the CLI-driven localnet smoke ([cli/README.md](cli/README.md), [localnet.md](docs/architecture/localnet.md)); and "Next: Plan 3b (CI with risc0 and real proofs, reproducible builds, CYCLES.md, REQUIREMENTS.md, fuzzing, red team, oracle publisher, one-command demo), then Plan 4 (FFI, module, thin GUI)." Add `[docs/architecture/sdk.md](docs/architecture/sdk.md)` to the Architecture line of "Documents".

- [ ] **Step 6: Final gates**

Run (background, timeout 3600000):
`MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_core -p auction_engine_core -p auction_engine_program -p engine_integration_tests -p auction_sdk -p auction_cli --features auction_sdk/testing 2>&1 | grep -E "test result|FAILED|panicked"'`
Expected: every `test result: ok`. Then the lint gate and the format gate with `-p auction_sdk -p auction_cli`. Expected: clean. Finally `scripts/check-hook-patches.sh` (WSL): expected unchanged result (no task touched `programs/stablecoin/**`).

- [ ] **Step 7: Commit (controller — GPG signing on Windows)**

```bash
git add scripts/localnet-smoke.sh docs/architecture/localnet.md DEPLOYMENTS.md deployments/localnet.json docs/architecture/programs.md docs/architecture/sdk.md README.md
git commit -m "feat(localnet): drive the localnet smoke with auction-cli instead of spel"
```

---

## Spec coverage (Plan 3a)

| Spec item | Where |
|---|---|
| §3.2 IDL-position vs enum mismatch; i128 gains | Task 1 (exhaustive drift tests), Task 4 (`encode.rs`), Task 8 (`negative_controller_gains_...`), Task 15 (`host set-controller-gains`) |
| §5.11 / THREAT_MODEL findings 4 and 8 | Tasks 2 and 3 |
| §6.1 SDK: PDAs, decoders, quotes, own encoders, plan builders | Tasks 4–8 |
| §6.4 discovery with persisted cursor | Task 11; CLI `list-*`, `rescan`, `watch` (Task 13) |
| §6.5 confirmation, Included/Dropped, one in flight | Task 10; CLI `Env::send` (Task 13), goldens (Task 14) |
| §6.6 CLI commands, wallet keystore, `cli/README.md` | Tasks 9, 13–15 |
| §9 SDK/CLI row: quote == on-chain, encoder index == enum and IDL, fixtures, CLI golden tests | Tasks 4, 8, 13–15 (panic safety and header drift are FFI, Plan 4) |
| §10 checklist and process for the on-chain changes | Tasks 2–3 (security-reviewer pass, THREAT_MODEL, checklist) |
| I4 / ADR-027 program-id and bind-time verification | Task 5 (`verify_config`, `verify_binding`), Task 15 (`host bind`) |
| ADR-024 `observed()` before every `t_eff` preview | Task 6 |
| ADR-029 bid account choice, ADR-030 slots | Tasks 6–7 |
| Localnet reproduction without spel | Task 16 |

---

## Plan 3b scope (next plan)

Moved out of this plan because each item is CI, evidence or tooling around the software 3a ships, depends on 3a's CLI, and is independently reviewable. Listed in the order Plan 3b should take them:

1. **Program CI with risc0** (carry-over): a workflow job that installs the pinned risc0 toolchain, builds the guests, runs `auction_engine_program`, `engine_integration_tests` (incl. `sdk_plans`, `sdk_quotes`), the upstream stablecoin suite, `auction_sdk`, `auction_cli`, the hook-order test, an `idl-gen` diff against the committed IDLs, `scripts/check-hook-patches.sh`, and the fmt list extended with `-p auction_sdk -p auction_cli`.
2. **Real-proof CI** (spec §1.3, §9 "Real proofs"): one `RISC0_DEV_MODE=0` end-to-end path per push to `main` (liquidate → bid → settle through `auction-cli` against a CI sequencer), the full real-proof suite nightly and on tags.
3. **Reproducible Docker guest builds and the ImageID refresh** (carry-over, THREAT_MODEL I4 residual): build stablecoin and engine with the Docker `make build-programs` path (ADR-018), publish the ids in `DEPLOYMENTS.md` and `deployments/localnet.json`, close the I4 "native build" residual.
4. **`CYCLES.md` and the regression check** (spec §11, carry-over): cycles per instruction against 32 Mi and transaction sizes, measured with real proofs; CI fails on a > 10 % regression; re-measure ADR-023 / ADR-036's 1.9 M line with real proofs; the load test (N concurrent auctions, interleaved bids; P2).
5. **`REQUIREMENTS.md` and its checker** (spec §1.4, §9 "Requirements"): every RFP-014 hard requirement → a named test, a milestone or an honest descope; a script that fails CI when a named test does not exist.
6. **Coverage gate** (spec §9: ≥ 80 % on the Rust crates) with `cargo-llvm-cov` in CI.
7. **`cargo-fuzz`** (spec §10) on the SDK decoders, `Keystore::parse`, `Cursor` loading and the quote paths.
8. **Oracle price publisher / keeper** (carry-over; localnet finding 2): keeps the localnet oracle fresh and moves the price, which F4 and the freeze path need.
9. **One-command reproduction** (spec §1.1, `scripts/demo.sh`): the full success-criterion flow including the debt auction and the "no protocol token → freeze" path, on top of the 3a CLI and item 8; `DEMO.md`.
10. **Red-team pass** (`security-redteam`, spec §10) over the engine, hooks, SDK and CLI; `cargo audit` re-run; THREAT_MODEL review-log entry.
11. **Evidence and outreach**: upstream issue drafts in `docs/outreach/` (stablecoin guest-function order vs enum, the oracle-scale question), `docs/v03-port-plan.md`, `docs/PRIVACY.md` stub, `SUBMISSION.md` with linked hashes.

Plan 4 (unchanged): `auction_ffi`, the C++ `auction_engine_module`, the thin Basecamp app.
