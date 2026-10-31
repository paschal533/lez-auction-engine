# Plan 5: the liquidation slice on LEZ 0.4 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The engine's whole liquidation path (`init → collateral config → liquidate → partial bid → final bid → claim leftover`) runs on LEZ v0.4.0-rc1 as an actor conversation: in-process on `lee` with our token actor, a minimal oracle and a mock CDP host, with the R6 and reservation invariants checked after every step, the ported security tests passing, a cycle/gas table, and the same flow end to end on a standalone 0.4 sequencer through an SDK builder that simulates before it sends.

**Architecture:** A separate cargo workspace `v04/` (own toolchain and lockfile) so the v0.2.4 PoC on `main` is untouched. It reuses `auction_core` (pure maths) and a new LEZ-free crate `auction_engine_params` (argument validation, error codes, seed bytes) shared with the v0.2.4 engine. Engine, token, oracle and mock host are `actor_entrypoint!` guests; every cross-actor effect is a sync message built from the design note's five blocks (Proxy, Relay, Read, Sentinel, Assert), and every internal check compares a whole `Actor`.

**Tech Stack:** Rust 1.98.1, edition 2024 (in `v04/` only), LEZ `v0.4.0-rc1` (`1806e873`) crates `lee`, `lee_core`, `sequencer_service_rpc`, `wallet`, `common`; risc0 3.0.5 / r0vm 3.0.5; borsh 1.5. Builds in WSL Ubuntu 24.04.

**Spec:** `docs/v04-port-design.md` (sections cited per task). Evidence it builds on: `docs/spikes/S8.md` (build, fees, admission, revert cost), `docs/spikes/S9.md` (conversation, whole-actor checks, PDA grants; code in `spikes/s9_lez_v04/`), `docs/research/2026-10-31-lez-0.4-port-research.md`. Behaviour of every handler = today's v0.2.4 handler in `programs/auction_engine/src/` (only the I/O changes); quote, cap, reward and accounting rules stay calls into `auction_core`.

## Global Constraints

- LEZ pinned by git tag `v0.4.0-rc1` (`1806e873`) for every LEZ crate in `v04/`; risc0-zkvm `=3.0.5`; Rust `1.98.1` via `v04/rust-toolchain.toml`.
- The root workspace (v0.2.4) keeps building and passing unchanged: `v04` is in the root `exclude` list; the committed guests, ImageIDs and `rfp014-poc` evidence do not change. Task 1 is the only change outside `v04/` that touches code, and it must be behaviour-neutral.
- WSL builds: `source ~/lez-spikes/s1/env.sh` (libclang, libpcsclite), `CARGO_TARGET_DIR=$HOME/lez-target/v04-port`, `CARGO_BUILD_JOBS=2`; no build starts with less than 10 GB free on C:.
- Messages and state are borsh. Every engine state encodes to non-empty bytes (empty data deletes an actor state, design note §3).
- Every check on `input.from` compares a whole `Actor` (account and program); `from_own_program()` alone is never sufficient (S9).
- Engine panics keep today's text `Program error [6000+code]: Program error {code}: <Name>` (design note §4.1) using `auction_engine_params::EngineError` codes.
- Time: the root message carries `now_ms`; the engine asserts it with `AssertTimestamp { at_least: now_ms, at_most: now_ms + slack_ms }` on the clock actor (design note §4.1). `slack_ms` is a config field.
- Public transactions sent to a sequencer always carry a `FeeDeclaration`; `gas_limit` = simulated cycles × 1.2, rounded up, never a flat default (S8, design note §4.3 b).
- No SPEL in `v04/` (none supports 0.4; design note §9 q1). The IDL is out of scope for this plan.
- Commits GPG-signed (never bypass), conventional messages, no AI attribution lines. Work on branch `feat/port-0.4`; nothing pushed without the author's go-ahead.

## Review Focus

1. **A reply arrives twice or out of phase** (a buggy or hostile host sends `Seized` twice, or `Seized` reaches an auction already `Auctioning`). The second one panics; nothing is booked twice. Test: Task 10 `seized_outside_awaiting_seize_reverts`.
2. **`now_ms` outside the clock window** (a keeper's clock behind by more than `slack_ms`, or ahead of the chain clock). The transaction reverts at the `AssertTimestamp`, before any token moves. Test: Task 8 `liquidate_with_now_outside_the_clock_window_reverts`.
3. **A bid for zero or for more than is left** (`max_debt_in = 0`, or `min_collateral_out` above the lot). Refused in the auction's first handler with today's error, before the bidder's holding is touched. Test: Task 9 `zero_bid_and_oversized_min_out_are_refused_without_token_moves`.
4. **A fresh recipient holding not declared in the transaction** (a bidder's collateral holding that never existed). The SDK's simulation reports a clean error (no panic in the SDK, no fee spent); with the holding declared, the bid applies. Test: Task 12 `undeclared_fresh_recipient_is_caught_in_simulation`.
5. **The host never replies** (a host that accepts `LiquidatePosition` but sends no `Seized`). The Sentinel `Finish` panics, so no auction is left in `AwaitingSeize`. Test: Task 10 `host_that_never_replies_leaves_no_auction`.

---

## File Structure

```
crates/auction_engine_params/          (new, LEZ-free; shared by v0.2.4 and v04)
  Cargo.toml
  src/lib.rs        re-exports
  src/args.rs       moved from programs/auction_engine/core/src/args.rs
  src/error.rs      moved from programs/auction_engine/core/src/error.rs
  src/seeds.rs      seed_from_str, u64_seed, combine_seeds (moved from core/src/pda.rs)
programs/auction_engine/core/src/{lib.rs,pda.rs}   re-export from auction_engine_params (no behaviour change)

v04/
  Cargo.toml                 workspace (edition 2024), LEZ v0.4.0-rc1 deps
  rust-toolchain.toml        1.98.1
  README.md                  how to build, test, run e2e; status vs the design note
  CYCLES.md                  per-instruction cycles and fee at base fee 8 (Task 11)
  engine_core/               states, messages, actor/PDA helpers, checks (no guest code)
  programs/methods/          risc0-build crate embedding the four guests
  programs/methods/guest/src/bin/{engine.rs,token.rs,oracle.rs,mock_host.rs}
  programs/token_core/ oracle_core/ mock_host_core/   message + state types per program
  harness/                   in-process test harness on lee::V03State + all slice tests
  sdk/                       transaction builder, simulate-then-send, actor-state decoders
  scripts/e2e.sh             standalone-sequencer run of the slice
.github/workflows/v04.yml    builds v04 and runs its tests
```

---

### Task 1: Extract `auction_engine_params` (LEZ-free) from the v0.2.4 engine core

**Files:**
- Create: `crates/auction_engine_params/{Cargo.toml,src/lib.rs,src/args.rs,src/error.rs,src/seeds.rs}`
- Modify: `Cargo.toml` (root: add member, add `v04` to `exclude`), `programs/auction_engine/core/Cargo.toml`, `programs/auction_engine/core/src/lib.rs`, `programs/auction_engine/core/src/pda.rs`

**Interfaces:**
- Produces: crate `auction_engine_params` with the existing public items of `args.rs` and `error.rs` unchanged (`EngineParamsArgs`, `CollateralArgs`, their `validate`, `EngineError` and its codes) and `seeds::{seed_from_str, u64_seed, combine_seeds}` (signatures as in today's `pda.rs`). Dependencies: `auction_core`, `borsh`, `serde`, `risc0-zkvm` (`default-features = false`, for `sha`). No `lee`/`lee_core`.
- `auction_engine_core` re-exports them at the same paths (`auction_engine_core::args::*`, `auction_engine_core::EngineError`, `pda::seed_from_str` …) so no other crate changes.

- [ ] **Step 1:** Rebase `feat/port-0.4` onto `origin/main` (`fe9c36f` or later); resolve nothing (docs-only branch). Expected: `git log origin/main..HEAD` shows only the port commits.
- [ ] **Step 2: Write the failing test** `crates/auction_engine_params/tests/lee_free.rs::crate_has_no_lee_dependency`: runs `cargo metadata --format-version 1 --no-deps`… then asserts the `auction_engine_params` package's dependency names contain neither `lee` nor `lee_core`.
- [ ] **Step 3:** Run it: FAIL (crate does not exist).
- [ ] **Step 4:** Move the three files with `git mv` (history kept), add the crate, switch `auction_engine_core` to re-exports.
- [ ] **Step 5:** Run in WSL: `scripts/wsl-cargo.sh test -p auction_engine_params -p auction_engine_core -p auction_engine_program` and `RISC0_SKIP_BUILD=1 scripts/wsl-cargo.sh clippy --workspace --exclude integration_tests -- -D warnings`. Expected: all pass, no new warnings; `scripts/check-guests.sh --sha-only` still matches (guest bytes unchanged).
- [ ] **Step 6: Commit** `refactor(engine): move argument validation, errors and seed bytes into the LEZ-free auction_engine_params`

### Task 2: The `v04` workspace and an in-process harness that runs a guest

**Files:**
- Create: `v04/Cargo.toml`, `v04/rust-toolchain.toml`, `v04/programs/methods/{Cargo.toml,build.rs,src/lib.rs}`, `v04/programs/methods/guest/{Cargo.toml,src/bin/echo.rs}`, `v04/harness/{Cargo.toml,src/lib.rs,tests/smoke.rs}`, `v04/README.md` (build section)

**Interfaces:**
- Produces: `harness::Chain` wrapping `lee::V03State`:
  - `Chain::new(programs: &[(&'static str, &'static [u8])]) -> Chain` — installs each ELF with `V03State::with_programs` plus the genesis system programs (clock) via `with_genesis_programs`; keeps `name -> AccountId` (`AccountId::from_builtin_program(program.id())`).
  - `fn program(&self, name: &str) -> AccountId`
  - `fn fund(&mut self, account: AccountId, native: u128)` (`force_insert_account` with `Account::funded`)
  - `fn key(&mut self, label: &str) -> (PrivateKey, AccountId)` (deterministic per label)
  - `fn send(&mut self, root: Actor, actors: &[Actor], signers: &[&PrivateKey], msg: &impl BorshSerialize) -> Result<Vec<TransactionEvent>, LeeError>` — builds `Message::try_new` with the signers' current nonces, `WitnessSet::for_message`, and calls `transition_from_public_transaction(&tx, self.block, self.now_ms)`; increments `block`.
  - `fn set_now_ms(&mut self, ms: u64)`; `fn state<T: BorshDeserialize>(&self, actor: Actor) -> Option<T>` (decode `actor_states[actor.program_account_id]`, `None` if absent).
  - `fn cycles(&self, root: Actor, actors: &[Actor], signers: &[&PrivateKey], msg: &impl BorshSerialize) -> (u64, Result<(), LeeError>)` via `lee::simulate_public_transaction` (pattern: `spikes/s9_lez_v04/spike_s9.rs` `sim`).
- `methods` exposes `pub const <NAME>_ELF: &[u8]` from `risc0_build::embed_methods()` (pattern: LEZ `examples/program_deployment/methods`).

- [ ] **Step 1: Write the failing test** `v04/harness/tests/smoke.rs::echo_guest_writes_its_message`: install `echo` (appends the message bytes to its actor state, like LEZ's `hello_world`), `send` `b"hi"` signed by the target, assert `chain.state::<Vec<u8>>`-raw bytes equal `b"hi"` and `cycles(..).1 == Ok(())`.
- [ ] **Step 2:** Run: `cd v04 && cargo test -p harness` (WSL, env.sh): FAIL until the workspace exists.
- [ ] **Step 3:** Create the workspace (`members = ["engine_core", "programs/*", "harness", "sdk"]` as they appear; guest crate excluded), toolchain file, `methods` crate, `echo` guest, harness.
- [ ] **Step 4:** Run the test: PASS. Run root workspace `scripts/wsl-cargo.sh test -p auction_core`: still PASS (the root does not see `v04`).
- [ ] **Step 5: Commit** `feat(v04): workspace on LEZ v0.4.0-rc1 with an in-process harness`

### Task 3: Token actor (design note §6)

**Files:** Create `v04/programs/token_core/{Cargo.toml,src/lib.rs}`, `v04/programs/methods/guest/src/bin/token.rs`, `v04/harness/tests/token.rs`

**Interfaces:**
- Produces (in `token_core`): `TokenMsg` borsh enum, variants in this order: `NewDefinition { minter: Option<Actor>, initial_to: Option<AccountId>, initial_supply: u128 }`, `Transfer { to: AccountId, amount: u128 }`, `Sweep { to: AccountId, at_least: u128 }`, `Burn { amount: u128 }`, `MintTo { to: AccountId, amount: u128 }`, `Credit { definition: AccountId, amount: u128 }`, `SupplyDown { amount: u128 }`, `ReadState`. States: `Definition { supply: u128, minter: Option<Actor> }`, `Holding { definition: AccountId, balance: u128 }` (each with an 8-byte tag so the encoding is never empty). `StateReply { state: Vec<u8> }` is LEZ's convention (`lee_core::program`, PM:396-407). Checks per the §6 table; `Credit` only `from_own_program()` **and** `from.account_id` holding the same definition is not checkable, so `Credit` is accepted only from `(x, T)` actors (any holding of T) and an empty holding takes the definition, else `WrongDefinition`.
- Errors are token-local panics `token: <reason>` (not engine codes).

- [ ] **Step 1: Write the failing tests** in `tests/token.rs`: `new_definition_then_transfer_credits_a_fresh_holding` (supply 1000 to A; A→B 250; B holds 250, A 750, supply 1000); `transfer_without_authorization_reverts`; `sweep_moves_everything_and_enforces_at_least` (sweep with `at_least` = balance + 1 reverts, = balance moves all); `burn_lowers_supply`; `mint_to_only_from_the_minter_actor` (minter `(K, E_fake)` accepted from `(K, E_fake)`, refused from `(K, other)` and `(other, E_fake)`); `credit_with_another_definition_reverts`; `read_state_replies_to_from`.
- [ ] **Step 2:** Run `cargo test -p harness --test token`: FAIL.
- [ ] **Step 3:** Implement `token_core` and the guest; credits are **sync** (design note §6).
- [ ] **Step 4:** Run: PASS.
- [ ] **Step 5: Commit** `feat(v04): token actor (holdings, transfer, sweep, burn, mint, read)`

### Task 4: Minimal oracle actor

**Files:** Create `v04/programs/oracle_core/src/lib.rs`, `v04/programs/methods/guest/src/bin/oracle.rs`, `v04/harness/tests/oracle.rs`

**Interfaces:**
- Produces: `OracleMsg { Init { publisher: Actor }, Publish { price_ray: u128, timestamp_ms: u64, confidence: u128 }, ReadState }`; state `OracleState { publisher: Actor, price_ray: u128, timestamp_ms: u64, confidence: u128 }` (the fields `auction_core::price` validates; RAY = 1e27 as today, ADR-050's role). `Publish` only from exactly `publisher`.

- [ ] **Step 1: Write the failing tests** `publish_only_from_the_publisher_actor`, `read_state_returns_the_last_publication`.
- [ ] **Step 2:** Run: FAIL. **Step 3:** Implement. **Step 4:** Run: PASS.
- [ ] **Step 5: Commit** `feat(v04): minimal oracle actor`

### Task 5: Mock CDP host implementing the F7 interface (design note §5, option B)

**Files:** Create `v04/programs/mock_host_core/src/lib.rs`, `v04/programs/methods/guest/src/bin/mock_host.rs`, `v04/harness/tests/mock_host.rs`

**Interfaces:**
- Consumes: `TokenMsg`, `OracleMsg::ReadState` (Tasks 3–4).
- Produces: `HostMsg` variants in order: `Init { admin: Actor, token: AccountId, stable_def: AccountId, collateral_def: AccountId, oracle: Actor, redemption_price_ray: u128, min_cr_ray: u128 }` (at the host's params actor), `SetLiquidationEngine { engine_actor: Actor }` (root, host admin signs), `OpenPosition { owner: AccountId, collateral: u128 }`, `GenerateDebt { normalized_debt: u128 }` (mints stablecoin to the owner through `MintTo`; the host's params actor is the stablecoin `minter`), `LiquidatePosition { position: AccountId, custody: AccountId, reply_to: Actor }`, `ReadState`. Position state `Position { owner, collateral, normalized_debt, vault: AccountId }`; the position's collateral sits in a host vault holding `(pda(H, ["VAULT", position]), T)`.
- `LiquidatePosition`: `from == Some(engine_actor)` (whole actor; refuses before binding); position unsafe at `min_cr_ray` against the oracle price (today's `liquidation_hooks.rs` rule, accumulator = 1); seizes the **whole** position: vault `Sweep { to: custody, at_least: collateral }` with the vault seed, zero the position, then `send_sync(reply_to, EngineMsg::Seized { position, owner, collateral, normalized_debt, nominal_debt })` (**Transfer first, then Seized**, design note §5). The `Seized` layout is the engine's (Task 6), encoded by variant index.

- [ ] **Step 1: Write the failing tests** `open_position_and_generate_debt_mint_stablecoin`, `liquidate_refused_before_binding_and_from_any_other_actor` (rogue config of the same program, attacker program at the config account), `liquidate_refused_while_safe`, `liquidate_sweeps_to_custody_then_reports_seized` (use a tiny test receiver guest that stores the `Seized` it gets; assert custody holds the collateral and the report equals the position).
- [ ] **Step 2:** Run: FAIL. **Step 3:** Implement. **Step 4:** Run: PASS.
- [ ] **Step 5: Commit** `feat(v04): mock CDP host with the engine hook interface`

### Task 6: `engine_core` — states, messages, actor helpers

**Files:** Create `v04/engine_core/{Cargo.toml,src/lib.rs,src/state.rs,src/msg.rs,src/actors.rs,src/check.rs}`, tests inline (`#[cfg(test)]`)

**Interfaces:**
- Consumes: `auction_core`, `auction_engine_params` (Task 1).
- Produces:
  - `state.rs`: `ConfigState`, `CollateralState`, `AccountingState`, `LiquidationRecordState`, `AuctionState` = today's `EngineConfigState`, `CollateralConfigState`, `AccountingState`, `LiquidationRecordState`, `AuctionState` (`programs/auction_engine/core/src/state.rs`) with: program ids → deployed program `AccountId`s; `host: Actor`, `oracle: Actor`, `clock: Actor`, `slack_ms: u64` on the config; `AuctionState.phase: AuctionPhase { AwaitingSeize, Auctioning, Settled }`; `AuctionState.host: Actor` (the actor allowed to send `Seized`). Today's tags kept as the 8-byte prefix.
  - `msg.rs`: `EngineMsg` borsh enum. **Root variants first, fixed order:** `0 Init { salt: u64, params: EngineParamsArgs, host: Actor, token: AccountId, stable_def: AccountId, oracle: Actor, clock: Actor, slack_ms: u64 }`, `1 SetCollateral { salt: u64, collateral_def: AccountId, args: CollateralArgs }`, `2 Liquidate { config: AccountId, collateral_def: AccountId, position: AccountId, now_ms: u64 }`, `3 Bid { config: AccountId, collateral_def: AccountId, position: AccountId, nonce: u64, max_debt_in: u128, min_collateral_out: u128, now_ms: u64 }`, `4 ClaimLeftover { config: AccountId, position: AccountId, nonce: u64 }`. Internal variants follow (implementer names them per design note §4.2 row 4–8: context, `Open`, `Seized { position, owner, collateral, normalized_debt, nominal_debt }`, `Reserve`, `Release`, `Settled`, `Finish`, `StateReply` handling); `Seized` must be the variant the mock host encodes — export `pub const SEIZED_INDEX: u8` and test it.
  - `actors.rs`: `config_actor(engine, admin, salt) -> Actor`, `collateral_actor(engine, config, def)`, `accounting_actor(engine, config)`, `record_actor(engine, config, position)`, `auction_actor(engine, config, position, nonce)`, `vault_holding(engine, token, auction) -> Actor` (`(pda(E, ["VAULT", auction]), T)`), `buffer_holding(engine, token, config)`; seeds from `auction_engine_params::seeds` with today's literals (ADR-020) and `AccountId::for_public_pda(&engine, ..)`. Each also returns its `PdaSeed` where the engine must grant it (`*_seed` fns).
  - `check.rs`: `expect_from(input: &ReceiveInput, expected: Actor, err: EngineError)`, `expect_root(input)`, `expect_phase(state, AuctionPhase, err)`, `fail(err: EngineError) -> !` producing today's panic text.

- [ ] **Step 1: Write the failing tests:** `every_state_encodes_non_empty`, `root_variant_indices_are_fixed` (borsh first byte of each root variant = 0..4), `seized_index_matches_the_host_encoding` (mock host's `EngineMsg` stub encodes to the same bytes as `EngineMsg::Seized`), `pda_seeds_match_v024_literals` (combined seed bytes for CONFIG/COLL/ACCT/LIQREC/AUCTION/VAULT equal the v0.2.4 `pda.rs` combination for the same inputs), `fail_text_matches_v024_format`.
- [ ] **Step 2:** Run `cargo test -p engine_core`: FAIL. **Step 3:** Implement. **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat(v04): engine states, messages and actor helpers`

### Task 7: Engine guest — `Init` and `SetCollateral` through the signer proxy

**Files:** Create `v04/programs/methods/guest/src/bin/engine.rs` (+ `engine/` modules as it grows: `proxy.rs`, `admin.rs`), `v04/harness/src/world.rs` (deploys token, oracle, mock host, engine; funds; opens a position), `v04/harness/tests/engine_admin.rs`

**Interfaces:**
- Consumes: Tasks 3–6.
- Produces: `harness::World::new() -> World` with fields `chain`, `engine`, `token`, `oracle`, `host_prog`, `admin`, `config: Actor`, `collateral: Actor`, `accounting: Actor`, `stable_def`, `collateral_def`, and `fn open_unsafe_position(&mut self, collateral: u128, debt: u128) -> AccountId` (opens, generates debt, then moves the oracle price so the position is unsafe).
- Behaviour: root `Init` at `(admin, E)` (admin signs) → proxy forwards to `config_actor(E, admin, salt)`; config checks `from == (admin, E)`, re-derives its own address, validates `params` (`auction_engine_params`), Reads the oracle once (Read block) and stores; then Relay `Init` → accounting actor (`from == config`). `SetCollateral` likewise → config → collateral actor (`from == config`). Second `Init` on the same salt reverts (`AlreadyInitialized`-equivalent code from today).

- [ ] **Step 1: Write the failing tests:** `init_creates_config_and_accounting`, `init_is_refused_without_the_admin_signature`, `init_twice_reverts`, `squatter_cannot_init_another_admins_config` (signer B with admin A's salt derives a different config), `set_collateral_only_through_the_admin_proxy`, `invalid_params_revert_with_v024_error_codes` (one case per `validate` error already tested in v0.2.4 `admin_tests.rs`, at least three).
- [ ] **Step 2:** Run: FAIL. **Step 3:** Implement. **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat(v04): engine init and collateral config through the signer proxy`

### Task 8: Engine — Observe and `Liquidate` (design note §4.1 Observe, §4.2 row 4, §4.3 a)

**Files:** Modify `engine.rs` (+ `engine/observe.rs`, `engine/liquidate.rs`), Create `v04/harness/tests/liquidate.rs`, `v04/harness/src/invariants.rs`

**Interfaces:**
- Produces: `harness::invariants::check(world: &World)` asserting, for every auction the world created: R6 (`vault holding balance == collateral_seized − keeper_reward − collateral_sold − claimed`), collateral and config reservations equal the sum over live auctions (today's reservation invariants, `programs/integration_tests` inv 2/7), and phase ∈ {Auctioning, Settled} after every transaction.
- Behaviour: root `Liquidate` at `(keeper, E)` → collateral actor runs Observe (Read redemption price from host, Read oracle, Assert clock, Sentinel `Proceed` running `feed::observe`/`PauseClock::observed` from `auction_core`) and `require_live` → config context → record `Bump`; auction `Open` (phase `AwaitingSeize`, `host` stored); host `LiquidatePosition { custody: vault_holding, reply_to: auction }`; on `Seized` (from == stored host, phase AwaitingSeize) the auction quotes with `auction_core` exactly as v0.2.4 `liquidate.rs`, pays the keeper reward from the vault (VAULT seed), sends `Reserve` to collateral/config, moves to `Auctioning`; auction `Finish` Sentinel last, asserting `Auctioning` and that the vault balance equals `collateral − reward` (Read; the optional R6 check, design note §4.3 a).

- [ ] **Step 1: Write the failing tests:** `liquidate_opens_an_auction_and_pays_the_keeper` (state and balances equal what v0.2.4 `liquidate_happy_path` computes for the same numbers), `liquidate_safe_position_reverts`, `liquidate_twice_reverts`, `stale_feed_pauses_and_reverts_liquidate` (oracle timestamp older than max age), `liquidate_with_now_outside_the_clock_window_reverts` (Review Focus 2: `now_ms` = chain − slack − 1 and = chain + 1), and `invariants::check` called after each step.
- [ ] **Step 2:** Run: FAIL. **Step 3:** Implement. **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat(v04): liquidate as an actor conversation with a host seize report`

### Task 9: Engine — `Bid` (partial and final) and `ClaimLeftover` (§4.2 rows 5–8)

**Files:** Modify `engine.rs` (+ `engine/bid.rs`, `engine/claim.rs`), Create `v04/harness/tests/bid.rs`

**Interfaces:**
- Behaviour: one `Bid` message (no separate `bid_with_settlement`; design note §4.2 note 1). Root at the collateral actor; bidder signs. Observe → auction `BidCtx`; auction quotes with `auction_core` as v0.2.4 `bid.rs`; bidder stable holding `Burn(debt part)` then `Transfer(penalty part → buffer holding)`; vault `Transfer(collateral_out)` or, on the final fill, `Sweep { at_least }` to the bidder's collateral holding (VAULT seed); at chain end → accounting `Settled`, collateral/config `Release`. `ClaimLeftover` root at `(owner, E)` → auction (`from == (owner, E)`) → vault `Sweep { to: owner holding, at_least: claimable − claimed }`.

- [ ] **Step 1: Write the failing tests:** `partial_bid_then_final_bid_settles_once` (numbers equal v0.2.4 `final_bid_reaches_target_and_settles_once` for the same inputs), `bid_after_settlement_reverts`, `split_bid_neutrality_holds` (two halves = one whole, as `auction_core` proptest), `zero_bid_and_oversized_min_out_are_refused_without_token_moves` (Review Focus 3: bidder balances unchanged after the revert), `claim_leftover_only_by_the_owner_and_only_once`, invariants after every step.
- [ ] **Step 2:** Run: FAIL. **Step 3:** Implement. **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat(v04): bid and claim leftover on actors`

### Task 10: Security tests in actor form (§4.3 c)

**Files:** Create `v04/programs/methods/guest/src/bin/impostor.rs` (sends any `EngineMsg` it is given to any actor), `v04/harness/tests/security.rs`

- [ ] **Step 1: Write the tests:** `impostor_program_at_the_config_account_cannot_send_internal_messages` (`Seized`, `Reserve`, `Release`, `Settled` from `(config, Impostor)` all revert), `engine_actor_at_an_attacker_account_cannot_either` (from `(attacker, E)` via the attacker's own proxy), `rogue_config_of_the_same_engine_cannot_seize` (S9 case, through the real engine), `seized_outside_awaiting_seize_reverts` (Review Focus 1: a second `Seized` to an `Auctioning` auction), `root_messages_to_internal_actors_revert` (`Seized` sent as root to an auction), `host_that_never_replies_leaves_no_auction` (Review Focus 5: an impostor host bound as the host that accepts and replies nothing; `Finish` panics, auction actor stays empty).
- [ ] **Step 2:** Run: each passes for the right reason (assert the panic text names the expected error, not just "reverted").
- [ ] **Step 3: Commit** `test(v04): actor-form security tests`

### Task 11: Cycles and fees per instruction

**Files:** Create `v04/harness/tests/cycles.rs`, `v04/CYCLES.md`

- [ ] **Step 1: Write the test** `every_slice_instruction_fits_the_gas_cap`: for `Init`, `SetCollateral`, `Liquidate`, partial `Bid`, final `Bid`, `ClaimLeftover`, measure `chain.cycles(..)` right before sending, assert `Ok` and `cycles < 10_000_000` (`max_gas_exec`, S8), and write a markdown table (instruction, executions, cycles, fee at base fee 8 = cycles × 8) to `target/cycles-v04.md`.
- [ ] **Step 2:** Run; copy the table into `v04/CYCLES.md` with the date, LEZ tag and a note that base fees move.
- [ ] **Step 3: Commit** `test(v04): cycle and fee table for the liquidation slice`

### Task 12: SDK — build, simulate, fit gas, send

**Files:** Create `v04/sdk/{Cargo.toml,src/lib.rs,src/builder.rs,src/decode.rs}`, `v04/sdk/tests/builder.rs`

**Interfaces:**
- Produces:
  - `pub struct Plan { root: Actor, actors: Vec<Actor>, signers: Vec<AccountId>, evidence: Vec<PublicAccountEvidence>, message: Vec<u8> }` and constructors `plan_init`, `plan_set_collateral`, `plan_liquidate`, `plan_bid`, `plan_claim_leftover` (each declares every actor the conversation touches, using `engine_core::actors`; recipient holdings declared; no evidence for engine-created PDAs, S9).
  - `pub fn fit_gas(simulated_cycles: u64) -> u64` = `ceil(cycles × 1.2)`, capped at `10_000_000` (error above).
  - `pub async fn simulate(client: &SequencerClient, wallet: &WalletCore<SequencerClient>, plan: &Plan, payer: AccountId) -> Result<u64, SdkError>` — loads programs via `ProgramLoader`, live actor states via `getAccount`, admitted set = signers ∪ existing accounts ∪ evidence; returns cycles or `SdkError::WouldRevert(String)`.
  - `pub async fn send(..) -> Result<[u8; 32], SdkError>` — simulate, `fit_gas`, `FeeDeclaration { payer, gas_limit, tip: 0, max_fee: gas_limit × base_fee_exec(from getFeeState) + storage bound }`, sign (payer + plan signers), send.
  - `decode.rs`: `actor_state<T: BorshDeserialize>(client, actor) -> Option<T>`.

- [ ] **Step 1: Write the failing tests** (pure, no sequencer): `fit_gas_adds_twenty_percent_and_caps`, `plans_declare_every_touched_actor` (compare each plan's `actors` with the actors the in-process run of Task 8/9 actually touched — harness records them), `undeclared_fresh_recipient_is_caught_in_simulation` (Review Focus 4: build a bid plan without the bidder's fresh collateral holding, run the SDK's simulation path against the in-process state: `Err(WouldRevert)` naming the actor; with it declared: `Ok`).
- [ ] **Step 2:** Run: FAIL. **Step 3:** Implement. **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat(v04): SDK builder that simulates, fits gas and sends`

### Task 13: Standalone-sequencer e2e, CI and docs

**Files:** Create `v04/scripts/e2e.sh`, `v04/sdk/examples/slice.rs`, `.github/workflows/v04.yml`; Modify `v04/README.md`, `docs/v04-port-design.md` §8 (status column)

- [ ] **Step 1:** `v04/sdk/examples/slice.rs`: runs `init → set collateral → open unsafe position (mock host) → liquidate → partial bid → final bid → claim leftover` against `$SEQUENCER_URL` with the SDK, printing each transaction hash, simulated cycles, `gas_limit` and the charged fee (payer balance before/after), and asserting the final states (auction `Settled`, vault empty after claim).
- [ ] **Step 2:** `v04/scripts/e2e.sh`: build LEZ v0.4.0-rc1 sequencer and wallet (or reuse `~/lez-target/v04`), set up and start a standalone sequencer with `networks/local.json`, import the public dev key A (S8), deploy the four guests with `wallet program-loader deploy` (segment count from the dry run, S8), run the example, stop the sequencer by exact process name (`pkill -x sequencer_servi`). Exit non-zero on any failed step. Run it: expected `slice complete`; record cycles vs Task 11 (must match within the storage term).
- [ ] **Step 3:** `.github/workflows/v04.yml`: on push/PR touching `v04/**` or `crates/auction_core/**` or `crates/auction_engine_params/**`, plus `workflow_dispatch`: install Rust 1.98.1 and r0vm 3.0.5, `cd v04 && cargo test --workspace`. (The e2e stays local, like the v0.2.4 story; note it in the README.)
- [ ] **Step 4:** README: build, test, e2e commands; what this slice proves; what Plan 6 adds (settle_debt, surplus and debt auctions, check_solvency, admin setters); design note §8 status column updated for tasks 3–9. Record in `docs/design-log.md` the ADRs the slice fixes (next free numbers after ADR-055): actor layout and retired PDAs (`ENGINE_AUTH`, `MINT_AUTH`, slots); the five conversation blocks and whole-actor checks; claimed time with `AssertTimestamp` and `slack_ms`; one bid message; host interface v2 (`LiquidatePosition` + `Seized` report); simulate-then-send with fitted gas. Each cites the design note section and S8/S9.
- [ ] **Step 5:** Push the branch only after the author agrees; then confirm `v04` CI green and record the run id in the README.
- [ ] **Step 6: Commit** `feat(v04): standalone e2e of the liquidation slice, CI and docs`
