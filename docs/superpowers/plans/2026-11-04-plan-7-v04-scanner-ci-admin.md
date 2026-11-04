# Plan 7: v0.4 scanner, hosted e2e, slack setter and Init host check

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the port tasks that need no upstream release: the SDK scanner (design note §8 task 9), the e2e on hosted CI (task 10), a bounded `slack_ms` setter, the host `ProtocolParameters` check at `Init` (R-7c), and a feature gate on `sdk::session`.

**Architecture:**
- The scanner ports v0.2.4's cursor, anchor and rewind logic (`crates/auction_sdk/src/scan.rs`) to an async `BlockSource` over 0.4 blocks. Discovery is by root call (`to.program_account_id == E`, borsh `EngineMsg`). Discoveries are candidates until an actor-state read confirms them (design note §7).
- The setter is a new appended `EngineMsg` variant.
- The host check adds a second pending read to the existing `InitReads` read window.
- The hosted e2e is a new workflow that runs `v04/scripts/e2e.sh all` with the LEZ binaries cached.

**Tech Stack:** Rust 1.98.1, `lee`/`lee_core` v0.4.0-rc1, borsh, risc0 3.0.5, tokio, GitHub Actions (ubuntu-22.04).

**Spec:**
- `docs/v04-port-design.md`: §7 (discovery), §8 (tasks 9 and 10).
- `docs/design-log.md`: ADR-058 (claimed time and slack), ADR-061 (mock host), ADR-063.
- `v04/README.md`: "Deferred".

There is no separate spec. Rulings made while executing the plan are provisional and must go in the ledger.

**Out of scope:**
- **Native collateral.** It cuts across bid, claim, the host's seizure, the R6 invariant and the SDK plans. It needs its own ADR and isn't an RFP requirement.
- **The IDL.** It waits for SPEL on 0.4.
- **Tasks 11–15.** They are blocked upstream.

## Global Constraints

- **Frozen v0.2.4 paths.** Never modify `programs/auction_engine`, `crates/auction_core`, `crates/auction_engine_params`, `artifacts/guests` or `Cargo.lock`. Guard: `git diff --exit-code 82877d3 -- programs/auction_engine crates/auction_core crates/auction_engine_params artifacts/guests Cargo.lock`. `crates/auction_sdk/src/scan.rs` is read-only reference material.
- **File size.** Every file stays ≤ 800 lines. Files already near the limit are `engine_core/src/state.rs` (750), `sdk/tests/builder.rs` (784), `harness/tests/security_system.rs` (771) and `harness/src/world.rs` (658). Put new tests in new files.
- **Wire format.** `EngineMsg` variants are only ever appended. The next free index is **45**. Existing variants keep their fields.
- **Error codes.** `EngineError` lives in the frozen `programs/auction_engine/core/src/error.rs`, so no new variants can be added. Out-of-range slack reverts with the existing `InvalidConfig` (code 5).
- **Dependencies.** No new crates. Persist the cursor with borsh, which the sdk already depends on.
- **Commits.** Commits are GPG-signed, with conventional messages and no AI attribution lines. Never bypass signing.
- **Gate.** Run from `v04/`, inside WSL, with `CARGO_BUILD_JOBS=2`:
  - `cargo fmt -p engine_core -p methods -p mock_host_core -p oracle_core -p token_core -p harness -p sdk -- --check`
  - `RISC0_SKIP_BUILD=1 cargo clippy --locked --workspace --all-targets -- -D warnings`
  - `cargo test --locked --workspace`
  - the guard above.
- **WSL invocation:** `wsl -d Ubuntu-20.04 -e bash -lc 'source ~/lez-spikes/s1/env.sh && export CARGO_TARGET_DIR=$HOME/lez-target/v04-port CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 && cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine-port/v04 && <cmd>'`
- **Slack bounds:** `MIN_SLACK_MS = 1_000` and `MAX_SLACK_MS = 300_000`, applied at both `Init` and `SetSlack`. The harness uses 6 000 and the e2e default is 60 000; both are inside the bounds.

## Review Focus

1. **Undecodable message.** A root call to an engine actor whose bytes don't decode as `EngineMsg` must be skipped, not fail the scan. Test `collect_skips_undecodable_root_message` (Task 4).
2. **Restarted sequencer.** A cursor from an earlier chain, with the same block ids but a new block 2 hash, must restart from block 1 with `restarted == true`. Test `replaced_chain_rescans_from_genesis` (Task 4).
3. **Reverted Liquidate.** A reverted `Liquidate` is included in the block but opens nothing. It must not appear in `Found.auctions`. Test `collect_skips_reverted_tx` (Task 4).
4. **Slack bounds.** Exactly 1 000 and 300 000 are accepted. 999 and 300 001 are rejected with `InvalidConfig` (5). A non-admin signer is rejected with `NotAdmin`. Tests in Task 2.
5. **Silent or lying host.**
   - A host that never replies must not let `Init` finish: `Proceed` fails while the host read is pending.
   - A host whose `Params` name another stablecoin must revert the whole transaction, leaving no config.
   - Tests in Task 3.

---

### Task 1: Feature-gate `sdk::session`

**Files:**
- Modify: `v04/sdk/Cargo.toml`
- Modify: `v04/sdk/src/lib.rs` (the `pub mod session;` line and its doc link)
- Modify: `v04/scripts/e2e.sh:69`, `:141`

**Interfaces:**
- Produces: the sdk feature `session`, off by default. Tasks 5 and 6 build the examples with `--features session`.

- [ ] **Step 1:** In `v04/sdk/Cargo.toml`:
  - mark `mock_host_core`, `clock_core` and `sha2` as `optional = true`;
  - add `[features] default = []` and `session = ["dep:mock_host_core", "dep:clock_core", "dep:sha2"]`;
  - add `[[example]]` entries for `slice` and `system`, each with `required-features = ["session"]`.
- [ ] **Step 2:** In `lib.rs`, put `#[cfg(feature = "session")]` on `pub mod session;` and turn the intra-doc link to `session` into plain backticks.
- [ ] **Step 3:** In `e2e.sh`, make both `cargo build`/`cargo run -p sdk --example …` lines pass `--features session`.
- [ ] **Step 4: Verify both feature sets.** Run `cargo check -p sdk --no-default-features` and `RISC0_SKIP_BUILD=1 cargo clippy -p sdk --all-targets --features session -- -D warnings`. Both must succeed. Then run `cargo tree -p sdk -e normal | grep -c mock_host_core`. Expected: `0`.
- [ ] **Step 5:** Run the full gate. Commit `refactor(v04): put sdk::session behind the session feature`.

---

### Task 2: Bounded `slack_ms`: `SetSlack` and an `Init` check (ADR-064)

**Files:**
- Modify: `v04/engine_core/src/msg.rs` (append variant 45). Add a round-trip case to `v04/engine_core/src/msg_tests.rs`.
- Modify: `v04/engine_core/src/check.rs`
  - `pub const MIN_SLACK_MS: u64 = 1_000;`
  - `pub const MAX_SLACK_MS: u64 = 300_000;`
  - `pub fn slack_in_range(slack_ms: u64) -> Result<(), EngineError>`
- Modify: `v04/programs/methods/guest/src/bin/engine/admin.rs`. Add an `on_set_slack` handler, and add the bound check in `init_config` next to `params.validate()`.
- Modify: `v04/programs/methods/guest/src/bin/engine/proxy.rs` (`forward_admin`) and `engine.rs` (the dispatch arm).
- Modify: `v04/sdk/src/system.rs`. Add `pub fn plan_set_slack(d: &Deployment, admin: AccountId, slack_ms: u64) -> Result<Plan, SdkError>` and re-export it from `lib.rs`.
- Test: `v04/harness/tests/slack.rs` (new) and `v04/sdk/tests/slack_plan.rs` (new).
- Docs:
  - ADR-064 in `docs/design-log.md`: bounds and why, and that the new value applies from the next relay.
  - The `msg.rs:188` comment: `slack_ms` is now settable.
  - The README "Deferred" bullet: remove it.
  - Variant counts: "45 borsh variants" becomes 46 in the README and in design note §8, task 6.

**Interfaces:**
- Produces:
  - `EngineMsg::SetSlack { salt: u64, slack_ms: u64 }` at index 45;
  - `engine_core::check::{MIN_SLACK_MS, MAX_SLACK_MS, slack_in_range}` (returns `EngineError::InvalidConfig` when out of range);
  - `sdk::plan_set_slack`.

- [ ] **Step 1: Write the failing tests** in `harness/tests/slack.rs`, using the `World` the other admin tests use:
  - `set_slack_updates_config_and_next_observation_uses_it`. Admin sends `SetSlack { slack_ms: 30_000 }`. Assert that the config's `slack_ms == 30_000`. After the next `ObserveFeed`, assert that the observation context carries 30 000.
  - `set_slack_accepts_bounds`: both 1 000 and 300 000 apply.
  - `set_slack_rejects_out_of_range`: 999 and 300 001 revert with `InvalidConfig` (5), and the config is unchanged.
  - `set_slack_rejects_non_admin`: a non-admin proxy reverts with `NotAdmin` (1).
  - `init_rejects_out_of_range_slack`: `Init` with `slack_ms: 0` reverts with `InvalidConfig` (5) and creates no config.
  
  In `engine_core/src/msg_tests.rs`, add `set_slack_round_trips_at_index_45`: the borsh encoding's first byte is 45, and decoding gives back the same value.
  
  In `sdk/tests/slack_plan.rs`, add `plan_set_slack_declares_proxy_and_config`: the plan's root is `Actor::new(admin, engine)`, and its actors are exactly `[proxy, config]`.
- [ ] **Step 2: Watch them fail.** Run `cargo test -p harness --test slack`, `cargo test -p engine_core set_slack` and `cargo test -p sdk --test slack_plan`. Expected: FAIL to compile, because `SetSlack` is not defined.
- [ ] **Step 3: Implement.** Follow the `SetEngineParams` path (`admin.rs:267-326`, `proxy.rs:51,73-96`). Check the admin proxy, check `slack_in_range`, write the config with only `slack_ms` changed, and send nothing else. Skip `cap_check` and `AssertSystemIdle`: slack isn't a locked field.
- [ ] **Step 4:** Run the three commands from Step 2. Expected: PASS.
- [ ] **Step 5: Cycles.** Run `cargo test -p harness --test cycles -- --nocapture`. Add a `SetSlack` row to `v04/CYCLES.md`, and pin its execution count in `harness/tests/cycles.rs` the way `SET_ENGINE_PARAMS_EXECUTIONS` is pinned. No existing row may change.
- [ ] **Step 6:** Write the docs listed under Files. Run the full gate. Commit `feat(v04): bounded slack_ms with a SetSlack admin instruction (ADR-064)`.

---

### Task 3: `Init` checks the host's parameters (R-7c, ADR-065)

**Files:**
- Modify: `v04/engine_core/src/state.rs`. Append `pub pending_host_read: bool` to `ConfigState`, after the existing fields. The file is at 750 lines; if it would pass 800, move `ConfigState`'s impl block to a new `state_config.rs`.
- Modify: `v04/engine_core/src/check.rs`. Add `pub fn host_params_match(config: &ConfigState, stable_def: AccountId, token: AccountId, oracle: Actor) -> Result<(), EngineError>`. It returns `HostMismatch` (11) if `stable_def != config.stablecoin_definition_id` or `token != config.token`, and `OracleMismatch` (13) if `oracle != config.oracle`. These are the field names `init_config` writes.
- Modify: `v04/programs/methods/guest/src/bin/engine/admin.rs`
  - `init_config` sets `pending_host_read: true`.
  - `on_init_reads` also sends `ReadState` to `config.host`.
- Modify: `v04/programs/methods/guest/src/bin/engine/read.rs`
  - `on_config_reply` accepts a reply where `from == config.host && config.pending_host_read`. It decodes `mock_host_core::Params`, calls `host_params_match(&config, p.stable_def, p.token, p.oracle)`, then clears the flag.
  - `proceed` fails with `HostMismatch` while `pending_host_read` is set.
- Modify: `v04/sdk/src/builder.rs`. `plan_init` declares `args.host`.
- Test: `v04/harness/tests/init_host.rs` (new). Unit tests for `host_params_match` go in `check.rs`.
- Docs:
  - ADR-065 in `docs/design-log.md`. It records that the check reads our mock's `Params` layout, so it must be re-pointed when the upstream host lands (task 12).
  - `v04/README.md`: remove the Deferred bullet.
  - ADR-061: the "does not validate yet (ruling R-7c)" sentence becomes "validated since ADR-065".

**Interfaces:**
- Consumes: `mock_host_core::Params { stable_def, token, oracle, .. }`, `Params::decode`, and the host's existing `HostMsg::ReadState` reply.
- Produces: `engine_core::check::host_params_match` and `ConfigState::pending_host_read`.

- [ ] **Step 1: Write the failing unit tests** in `check.rs`:
  - `host_params_match_accepts_equal`;
  - `host_params_match_rejects_stable_def` (expects 11);
  - `host_params_match_rejects_token` (expects 11);
  - `host_params_match_rejects_oracle` (expects 13).
- [ ] **Step 2: Write the failing harness tests** in `harness/tests/init_host.rs`.

  These need a harness helper: `World::new_with_host(edit: impl FnOnce(&mut mock_host_core::Params)) -> World`. It deploys the host with edited `Params` before `Init`. Put it in a new `harness/src/init.rs` if `world.rs` would pass 700 lines.

  The tests:
  - `init_succeeds_with_matching_host`: config exists and `pending_host_read == false`.
  - `init_reverts_when_host_names_another_stablecoin`: expects code 11. No config, no accounting and no protocol definition exist afterwards.
  - `init_reverts_when_host_names_another_oracle`: expects code 13.
  - `init_reverts_when_host_never_replies`. Use the existing impostor or a silent actor as `host`. The tx reverts with code 11 from `proceed`.
- [ ] **Step 3: Watch them fail.** Run `cargo test -p engine_core host_params` and `cargo test -p harness --test init_host`. Expected: FAIL.
- [ ] **Step 4:** Implement as described under Files.
- [ ] **Step 5:** Run Step 3's commands, then `cargo test -p harness`. All existing Init-based tests must still pass. The harness's default host already matches.
- [ ] **Step 6: Cycles.** Re-run the cycles test. Update the `Init` row in `CYCLES.md`: the execution count goes up by the host read. Update the README's Init figure, and update the pinned count in `cycles.rs`.
- [ ] **Step 7:** Write the docs listed under Files. Run the full gate. Commit `feat(v04): Init validates the host's parameters (R-7c, ADR-065)`.

---

### Task 4: Scanner core, with cursor, rewind and collect (ADR-066)

**Files:**
- Create: `v04/sdk/src/scan/mod.rs`. It holds the types, `BlockSource`, `scan` and `scan_page`.
- Create: `v04/sdk/src/scan/cursor.rs`. It holds the anchor and rewind decisions, ported from `crates/auction_sdk/src/scan.rs:1-12, 221-290`.
- Create: `v04/sdk/src/scan/collect.rs`. It turns a block into discoveries.
- Modify: `v04/sdk/src/lib.rs`. Add `pub mod scan;` and two `SdkError` variants: `ScanFork { at: u64 }` and `MissingBlock(u64)`.
- Test: `v04/sdk/tests/scan.rs` (new), using an in-memory `MemChain` defined in the test file.

**Interfaces (Produces, used by Task 5):**

```rust
pub const ANCHOR_BLOCK: u64 = 2;
pub struct TxSummary { pub hash: [u8; 32], pub root_to: Option<Actor>, pub message: Vec<u8>, pub reverted: bool }
pub struct BlockSummary { pub id: u64, pub hash: [u8; 32], pub prev_hash: [u8; 32], pub timestamp_ms: u64, pub txs: Vec<TxSummary> }
pub trait BlockSource {
    async fn last_block_id(&self) -> Result<u64, SdkError>;
    async fn blocks(&self, from: u64, to: u64) -> Result<Vec<BlockSummary>, SdkError>; // inclusive
}
pub struct ScanTargets { pub engine: AccountId, pub config: AccountId }
pub struct ScanSettings { pub batch: u64, pub reorg_depth: u64 }          // Default: 100, 10
#[derive(BorshSerialize, BorshDeserialize, Clone, Default, PartialEq, Debug)]
pub struct Found { pub auctions: BTreeSet<(AccountId, u64)>, pub positions: BTreeSet<AccountId>,
                   pub surplus: BTreeSet<u64>, pub debt: BTreeSet<u64> }
#[derive(BorshSerialize, BorshDeserialize, Clone, Default, PartialEq, Debug)]
pub struct Cursor { pub last_block: u64, pub last_hash: Option<[u8; 32]>, pub anchor: Option<[u8; 32]>,
                    pub recent: BTreeMap<u64, [u8; 32]>, pub found: Found }
pub struct Scanned { pub cursor: Cursor, pub restarted: bool }
pub struct ScanPage { pub scanned: Scanned, pub head: u64, pub done: bool }
pub async fn scan(src: &impl BlockSource, cursor: &Cursor, t: &ScanTargets, s: &ScanSettings) -> Result<Scanned, SdkError>;
pub async fn scan_page(src: &impl BlockSource, cursor: &Cursor, t: &ScanTargets, s: &ScanSettings, max_blocks: u64) -> Result<ScanPage, SdkError>;
pub fn collect(found: &Found, block: &BlockSummary, t: &ScanTargets) -> Found;
```

**Rules for `collect`:**
- Skip every tx that is reverted, has no `root_to`, or has `root_to.program_account_id != t.engine`.
- Skip a message that fails to decode with `EngineMsg::try_from_slice`.
- Count only messages whose `config == t.config`:
  - `Liquidate` inserts `(position, nonce)` into `auctions` and `position` into `positions`.
  - `Bid`, `ClaimLeftover` and `SettleEmpty` insert `(position, nonce)`.
  - `StartSurplus` inserts its `nonce` into `surplus`, and `StartDebt` into `debt`.
  - All other variants are ignored.
- Neither `scan` nor `collect` mutates its input.

- [ ] **Step 1: Write the failing tests** in `sdk/tests/scan.rs`.

  Port each v0.2.4 test (`crates/auction_sdk/src/scan_tests.rs`) to `MemChain`, with the same names and assertions:
  - `finds_auctions_and_positions_incrementally`
  - `rewinds_on_changed_hash`
  - `rewind_arithmetic_is_exact`
  - `replaced_chain_rescans_from_genesis`, covering both the shorter and the longer replacement chain
  - `matching_anchor_scans_incrementally`
  - `anchorless_cursor_is_rebuilt_once`
  - `cursor_above_head_rewinds_only_when_provable`
  - `fork_deeper_than_reorg_depth_fails` (expects `SdkError::ScanFork`)
  - `missing_cursor_block_is_an_error` (expects `SdkError::MissingBlock`)
  - `cursor_unchanged_on_missing_block`
  - `scan_page_stops_at_max_blocks_and_reports_done`

  Then add the 0.4 collect tests:
  - `collect_finds_liquidate_bid_surplus_debt`
  - `collect_ignores_other_config`
  - `collect_skips_reverted_tx`
  - `collect_ignores_other_program`
  - `collect_skips_undecodable_root_message`
  - `cursor_round_trips_through_borsh`
- [ ] **Step 2:** Run `cargo test -p sdk --test scan`. Expected: FAIL to compile, because `sdk::scan` does not exist.
- [ ] **Step 3:** Implement the three files. `cursor.rs` is a port of the v0.2.4 decisions with `[u8; 32]` hashes; keep its comments on why the anchor is block 2. `collect.rs` follows the rules above.
- [ ] **Step 4:** Run `cargo test -p sdk --test scan`. Expected: PASS, 17 tests.
- [ ] **Step 5: ADR-066** in `docs/design-log.md`. It covers:
  - root-call discovery;
  - the candidate-then-state-read rule;
  - why there are no events: a standalone sequencer never finalizes;
  - the anchor;
  - that the cursor is persisted with borsh by the caller.
- [ ] **Step 6:** Run the full gate. Commit `feat(v04): sdk block scanner core with cursor and rewind (ADR-066)`.

---

### Task 5: Scanner on the live sequencer

**Files:**
- Create: `v04/sdk/src/scan/live.rs`
  - `impl BlockSource for sequencer_service_rpc::SequencerClient`, using `get_last_block_id` and `get_block_range(from, to)`.
  - It maps `common::block::Block { header: { block_id, prev_block_hash, hash, timestamp }, body }` to `BlockSummary`.
  - Only public transactions get a `root_to`/`message`, from their root call. All other tx kinds give `root_to: None`.
  - `reverted` comes from `body.is_reverted(i)`.
- Create: in the same file, `pub fn live_auctions(src: &impl StateSource, d: &Deployment, found: &Found) -> Result<Vec<Actor>, SdkError>`. It returns the auction actors (`d.auction(position, nonce)`) whose engine state is non-empty, in `found` order.
- Modify: `v04/sdk/examples/system.rs`. After its last step, scan from `Cursor::default()` to the head and assert both of these:
  - `found.debt` and `found.surplus` contain the nonces the example used;
  - the scan is idempotent: a second `scan` from the returned cursor finds nothing new and leaves `found` unchanged.
  
  Then print `scan ok: <n> auctions, <s> surplus, <d> debt`.
- Modify: `v04/scripts/e2e.sh`. The `system` stage fails unless `system.log` contains `scan ok:`.
- Test: `v04/sdk/tests/scan_live.rs` (new). It tests `live_auctions` against a `StateSource` stub, which `decode.rs` already abstracts over.

**Interfaces:**
- Consumes: Task 4's types; `StateSource` and `Deployment::auction` (`builder.rs:148-207`).
- Produces: `sdk::scan::live_auctions`, and the e2e marker line `scan ok:`.

- [ ] **Step 1: Write the failing tests** in `scan_live.rs`:
  - `live_auctions_drops_candidates_without_state`: a stub with state for one of two found auctions returns exactly that one.
  - `live_auctions_empty_when_nothing_found`.
- [ ] **Step 2:** Run `cargo test -p sdk --test scan_live`. Expected: FAIL.
- [ ] **Step 3:** Implement `live.rs` and update the example.
- [ ] **Step 4:** Run `cargo test -p sdk --test scan_live`, then `cargo build -p sdk --features session --example system`. Expected: PASS, and the example builds.
- [ ] **Step 5: Run the live check.** Run `v04/scripts/e2e.sh all`. It takes about 25 minutes; run it detached as in Plan 6. Expected: `slice complete`, `system complete`, and a `scan ok:` line with at least 1 surplus and 1 debt. While the stack is up, restart it with `e2e.sh down` and `e2e.sh up`. Record whether block 1's hash repeats across the restart in ADR-066; this is the anchor rationale on 0.4.
- [ ] **Step 6:**
  - Design note §8, task 9: status becomes "done, except the CLI that drives it (task 11)".
  - `v04/README.md` Deferred: remove "The SDK scanner".
  - Run the full gate. Commit `feat(v04): scanner over the live sequencer, checked in the e2e`.

---

### Task 6: Run the v0.4 e2e on hosted CI

**Files:**
- Create: `.github/workflows/v04-e2e.yml`
- Modify: `v04/scripts/e2e.sh`. Update the header comment ("Local only" becomes local and CI). Every path stays overridable by env, as it already is.
- Modify:
  - `.github/workflows/v04.yml:6` (the comment);
  - `v04/README.md:40`, `:168` (the "stays local" text);
  - `docs/architecture/ci.md:5`, `:155`. The repo is now public, so the minutes are free. Keep the weekly schedule and give the reason as runner time, not billing.

**Workflow shape (fixed decisions):**
- **Triggers:**
  - `push` to `main` with paths `v04/**`, `crates/auction_core/**`, `crates/auction_engine_params/**` and `.github/workflows/v04-e2e.yml`;
  - `schedule: "13 5 * * 0"`;
  - `workflow_dispatch`.
- **Runtime settings:**
  - `concurrency: { group: v04-e2e-${{ github.ref }}, cancel-in-progress: false }`;
  - `permissions: contents: read`;
  - one job on `ubuntu-22.04` with `timeout-minutes: 150`.
- **Setup steps,** as in `v04.yml:35-45`:
  - `scripts/ci/free-disk.sh`;
  - apt `clang libclang-dev pkg-config libssl-dev libpcsclite-dev build-essential`;
  - the toolchain from `v04/rust-toolchain.toml`;
  - `./.github/actions/install-risc0`;
  - `Swatinem/rust-cache@v2` with `workspaces: v04`, `shared-key: v04`, `save-if: false`.
- **LEZ binary cache:**
  - `actions/cache/restore`, then `actions/cache/save` with `if: always() && steps.lez.outcome == 'success' && steps.lez-cache.outputs.cache-hit != 'true'`;
  - paths: `~/lez-target/v04/release/sequencer_service` and `~/lez-target/v04/release/wallet`;
  - key: `lez-v04-${{ hashFiles('v04/scripts/e2e.sh', 'v04/rust-toolchain.toml') }}-ubuntu-22.04-v1`.
  
  The clone itself is not cached. `e2e.sh` clones for the config files and skips the build when the binaries exist.
- **Steps:**
  - `id: lez`: `v04/scripts/e2e.sh up` with `timeout-minutes: 75`;
  - then `e2e.sh deploy slice system` with `timeout-minutes: 60`. If `e2e.sh` takes one stage per call, use `all` in a single step with a 120 min timeout.
  - Env: `LEZ_DIR=$HOME/lez-spikes/lez-v04`, `LEZ_TARGET=$HOME/lez-target/v04`, `CARGO_TARGET_DIR=$HOME/lez-target/v04-port`, `E2E_HOME=${{ runner.temp }}/e2e`.
- **Failure artifacts:**
  - an `if: always()` upload of `sequencer.log`, `slice.log`, `system.log` and `programs.env` from `$E2E_HOME`, with `if-no-files-found: warn`. **Never** upload `$E2E_HOME/wallet`: it holds keys.
  - the last 50 lines of each log go to `$GITHUB_STEP_SUMMARY`.

- [ ] **Step 1:** Write the workflow. Run `bash -n v04/scripts/e2e.sh`. Then run `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/v04-e2e.yml'))"`, or `actionlint` if it is installed. Expected: no output.
- [ ] **Step 2:** Check that `grep -n "wallet" .github/workflows/v04-e2e.yml` shows no upload path containing `wallet`.
- [ ] **Step 3:** Make the doc edits listed under Files. Then:
  - Design note §8, task 10: status becomes "done: v04-e2e.yml (hosted), pending its first run".
  - `REQUIREMENTS.md` S2: add `ci:v04-e2e.yml` as evidence next to `e2e.yml`. If `scripts/check-requirements.py` checks evidence paths, run it.
- [ ] **Step 4:** Run the full gate. Commit `ci(v04): run the 0.4 e2e on hosted runners`.
- [ ] **Step 5 (after the user pushes or merges):** Ask the user before dispatching the workflow. Watch its first run. Then record the run ID, the cold build time and the e2e time in `docs/architecture/ci.md`, as was done for v0.2.4 (ci.md:109).
