# Plan 10: Pinned Readings, Grace Period and the Five F8 Gaps — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the five values the admin cannot set today into init parameters (E9), treat a reading at the feed's own answer bound as invalid (E3, 6076), and refuse `Liquidate` for a grace period after a feed or chain outage (E3, 6072).

**Architecture:**
- Pure rules live in `v04/risk_core` (`EngineExtArgs`, `effective_ref_price_ext`, the pinned check in `validate_feed_ext`, a new `grace` module, a `Grace` feed case in the pause table).
- `engine_core` stores them (`ConfigExt.engine`, three appended `CollateralExt` fields, `SystemCtx.ext`) and applies them in `feed`, `gate` and `system`.
- The guest validates the new arguments on `Init`, `SetEngineParams` and `SetCollateral`. No new `EngineMsg` variant; `Init` (0) and `SetEngineParams` (20) gain one field at their end.

**Tech Stack:** Rust 1.98.1, risc0 zkVM guest, borsh, proptest, LEZ v0.4.0-rc1; Python 3 for the doc generators.

**Spec:** `docs/superpowers/specs/2026-11-08-tier2-engine-design.md` §3.0, §3.3 (E3), §3.9 (E9), §3.10 (codes 6072, 6076), §5 (Plan 10 row), §8 (Plan 10 row). ADR-075 and ADR-082.

**Out of scope:** E4-E6 (Plan 11), E7-E8 (Plan 12), `SetEngineParams.now_ms`, `ApplyPending`/`CancelPending` (indices 49, 50 stay reserved), events and SDK state views (R9-15 stands).

**Branch:** `feat/tier2-plan-10`, cut from `feat/tier2-engine` at `06fedda` (PR #11, not merged yet).

## Global Constraints

Plan 9's Global Constraints apply unchanged (`docs/superpowers/plans/2026-11-08-plan-9-twap-pause-levels.md`, lines 24-71). The ones that bite most, and what differs:

- **Frozen v0.2.4 paths (ADR-069).** Never change a file under `programs/**`, `crates/auction_core/**`, `crates/auction_engine_params/**`, nor the root `Cargo.toml`/`Cargo.lock`. Before every commit: `git diff --stat b14e59b -- programs/ crates/auction_core/ crates/auction_engine_params/ Cargo.toml Cargo.lock` prints nothing, `scripts/check-frozen-v024.sh` exits 0, `scripts/check-guests.sh --sha-only` exits 0.
- **cargo runs only inside WSL, from `v04/`:**
  `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'source ~/lez-spikes/s1/env.sh && export CARGO_TARGET_DIR=$HOME/lez-target/v04-port CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=1 && cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine-port/v04 && cargo <args>'`
  - During a task run one test target. `cargo test --locked --workspace` (about an hour) is run by the user in their own terminal at the end of Tasks 4, 7 and 9: a background shell in an agent session is reaped when memory is low.
  - `risk_core` preset tests need `--features presets`.
- **Gate (end of every task):** `cargo fmt -p risk_core -p engine_core -p methods -p mock_host_core -p oracle_core -p token_core -p harness -p sdk -- --check`; `RISC0_SKIP_BUILD=1 cargo clippy --locked --workspace --all-targets -- -D warnings`; the same clippy for `-p sdk --features session`; the task's own test targets; the frozen-path checks; `python3 -m unittest discover -s scripts/tests -v` from the repo root.
- **Existing tests keep their assertions.** The only allowed edit to an existing test is adding the appended fields at their legacy values (`ext: Box::new(EngineExtArgs::legacy())`, `engine: EngineExtArgs::legacy()`, the six new `RiskArgs` fields through `..RiskArgs::legacy()` or the literal legacy values, `last_good_ts_ms: None`, `feed_invalid_since_ms: None`, `grace_until_ms: 0`), and the changed expectations this plan names (the code array in `risk_error_codes_are_70_and_up`, the case count in `pause_table_is_exhaustive`, the fault array in `validate_feed_ext_checks_in_v024_order_and_maps_faults`).
- **Wire format.** No new variant. `Init` gains `ext: Box<EngineExtArgs>` last; `SetEngineParams` gains `ext: Box<EngineExtArgs>` last. `SetCollateral.risk` and `Configure.risk` carry the longer `RiskArgs`.
- **State layout.** Append only: `ConfigExt.engine`, `CollateralExt.{last_good_ts_ms, feed_invalid_since_ms, grace_until_ms}`, `SystemCtx.ext`. Migration note: local 0.4 state does not decode after this plan; recreate it; tags unchanged.
- **Refusal codes.** This plan adds exactly `GracePeriod` = 72 and `PriceFeedPinned` = 76 to `risk_core::RiskError`. An out-of-range argument stays `EngineError::InvalidConfig` (6005).
- **No production values in on-chain code (F8, ADR-082).** Presets stay in `risk_core::presets` behind the `presets` feature.
- **File size.** No file over 800 lines. Near the limit now: `engine_core/src/state.rs` 769, `sdk/tests/builder.rs` 784, `harness/tests/security_system.rs` 771, `engine_core/src/system.rs` 722, `harness/src/world.rs` 714. New types and tests go in new files; if an appended legacy field pushes one of these past 800, move its last test module into a sibling file in the same commit.
- **Lints.** No `as`, no unchecked arithmetic, no slice indexing, no wildcard enum arms; every `#[allow]` has a `reason`.
- **Commits.** `git add <paths>` then `git commit -S -m "<type>: <description>"` as two calls. No `Co-Authored-By`, no AI attribution, never `--no-verify`. Signing uses Gpg4win; a passphrase window may open on the user's desktop. If signing fails, stop and report BLOCKED. Never push without the user's go-ahead.
- **Copy rule.** No rival bid or team is named in code, docs or commit messages.
- **Generated docs.** `README.md`'s test-count block and `docs/architecture/refusal-codes.md` are regenerated only in Task 9.

## Review Focus

1. **A valid reading right after `SetCollateral` turns grace on.** With `feed_invalid_since_ms = None` and an old `last_good_ts_ms`, the first observation could start a grace nobody expects. Expected: only a real gap of at least `grace_min_gap_ms` starts it. Test: `turning_grace_on_does_not_start_one` (Task 7).
2. **`grace_ms > 0` with `grace_min_gap_ms = 0`.** Every valid observation would have `gap >= 0` and liquidation would never run. Expected: `validate` refuses the pair. Test: `grace_needs_a_nonzero_min_gap` (Task 4).
3. **A pinned reading that is also stale or zero.** Expected: stale wins (v0.2.4 order kept), then pinned, then zero price; under the legacy bounds nothing changes. Test: `pinned_is_checked_after_stale_and_before_zero` (Task 5).
4. **`SetEngineParams` changing `ref_scale` while a system auction is live.** The same oracle bytes would be read at another scale mid-auction. Expected: it needs `AssertSystemIdle`, like replacing the oracle. Test: `changing_ref_scale_needs_an_idle_system` (Task 2).
5. **`SetPauseLevel` wiping the engine parameters.** `ConfigExt { system_level: level }` is a full literal today. Expected: the level changes, `engine` is kept. Test: `set_pause_level_keeps_the_engine_ext` (Task 2).

---

## Rulings this plan makes (the spec leaves them open)

- **R10-1 `EngineExtArgs`** lives in `risk_core` (`engine_ext.rs`), fields in this order: `ref_conf_bps: u16, ref_clamp_bps: u16, debt_relist_delay_ms: u64, ref_sanity_lo_bps: u32, ref_sanity_hi_bps: u32, ref_scale: u8`. `legacy()` and `recommended()` return the same values (spec §3.9: "E9 is about who chooses"): `1_000, 2_000, 600_000, 5_000, 20_000, 0`.
- **R10-2 Bounds.** `ref_conf_bps <= 1_000`; `ref_clamp_bps <= 5_000`; `debt_relist_delay_ms` in `60_000..=604_800_000`; `1_000 <= ref_sanity_lo_bps <= 10_000 <= ref_sanity_hi_bps <= 100_000`; `ref_scale` 0 or 1. The spec's 60_000 floor is kept although it is below `MIN_SYSTEM_TTL_MS` (600_000): ADR-039's reason is anti-spam, and the admin now owns the choice. `risk_core` cannot name `auction_engine_params`, so it defines `RELIST_DELAY_MAX_MS = 604_800_000` and `REF_CONF_MAX_BPS = 1_000`, and an `engine_core` test asserts they equal `MAX_SYSTEM_TTL_MS` and `MAX_CONF_BPS`.
- **R10-3 No `now_ms` on `SetEngineParams`.** It belongs to E8 (Plan 12).
- **R10-4 `ConfigExt` loses `Default`.** It becomes `{ system_level: u8, engine: EngineExtArgs }` with `ConfigExt::new(engine)`. A zeroed `EngineExtArgs` is not a valid configuration, so no `Default` exists for it.
- **R10-5 Locked field.** A `SetEngineParams` that changes `ext.ref_scale` counts as a locked-field change (`AssertSystemIdle`); the other five fields do not, like `ref_oracle_max_age_ms` today. The SDK mirrors it in `plan_set_engine_params`.
- **R10-6 `SystemCtx` gains `ext: EngineExtArgs`** as its last field (21 bytes), copied by `from_config`.
- **R10-7 `RiskArgs` grows by six fields, appended in this order:** `sanity_lo_bps: u32, sanity_hi_bps: u32, feed_min_raw: u128, feed_max_raw: u128, grace_ms: u64, grace_min_gap_ms: u64`. They do not depend on the TWAP, so `check_off` is unchanged. `validate` becomes `validate(&self, max_oracle_age_ms: u64, band_lo_bps: u32, band_hi_bps: u32)`.
  - Legacy: `5_000, 20_000, 0, u128::MAX, 0, 0`. Recommended: `5_000, 20_000, 0, u128::MAX, 1_800_000, 600_000`.
  - Bounds: `1_000 <= sanity_lo_bps <= band_lo_bps`; `band_hi_bps <= sanity_hi_bps <= 100_000`; `feed_min_raw < feed_max_raw`; `grace_ms <= 86_400_000`; `grace_min_gap_ms <= 86_400_000`; `grace_ms > 0` requires `grace_min_gap_ms > 0` (Review Focus 2).
- **R10-8 Oracle bounds in the SDK.** `recommended()` stays argument-free with the pin bounds off. The SDK adds `enum OracleBounds { Undeclared, Declared { min_raw: u128, max_raw: u128 } }` and `fn recommended_risk(bounds: OracleBounds) -> RiskArgs`; a caller cannot get a recommended config without stating the oracle's bounds. That is the spec's "refuses to build … without them", done by the type.
- **R10-9 `FeedFault::Pinned`.** The frozen `PauseReason` has no such variant, so `FeedFault::pause_reason` returns `Option<PauseReason>` (`None` for `Pinned`), `Observed.feed` becomes `Result<u128, FeedFault>`, and `engine_core::feed::require_live` returns `Result<u128, Refusal>`: `Pinned` is `RiskError::PriceFeedPinned`, every other fault keeps its `EngineError`.
  - Check order in `validate_feed_ext`: stale, **pinned**, zero price, confidence, sanity window.
  - A pinned reading pauses the type through the same path as any invalid feed (`feed_valid: false`), is not recorded, and gives `Gate::Fallback` to a bid.
- **R10-10 `last_good_ts_ms`.** `CollateralState.last_seen_oracle_ts` advances on invalid readings too, so `CollateralExt` gains its own `last_good_ts_ms: Option<u64>`: the reading timestamp (clamped to `now`) of the newest valid observation.
- **R10-11 Grace is a `FeedCase`.** Grace matters only while the feed is valid (an invalid feed already refuses `Liquidate`), so `FeedCase` gains `Grace` = "valid feed, inside the grace period". `allowed` signature is unchanged. `Liquidate` order: level (6073), then feed, then grace (6072), then the TWAP checks (spec §3.1.4).
- **R10-12 Grace reads the state after the observation.** The observation that ends an outage starts the grace, and the `Liquidate` carrying that observation is refused. Grace ends at `now_ms == grace_until_ms` (refused while `now_ms < grace_until_ms`). `SetCollateral` keeps the three grace fields, like the ring (R9-6).
- **R10-13 e2e.** The `tier2` stage keeps its values and adds `grace_ms 30_000`, `grace_min_gap_ms 30_000`, a third unsafe position P3, and one step `grace_refused` after `fallback_final`: `SetPauseLevel 0`, publish, `ObserveFeed`, then a simulated `Liquidate` of P3 refused with 6072.
- **R10-14 Test homes.** Pinned and grace harness tests go in a new `v04/harness/tests/grace.rs`; reference-oracle parameter tests in a new `v04/harness/tests/engine_ext.rs`; `engine_core` reference tests in a new `v04/engine_core/src/system_ref_tests.rs`.

---

### Task 1: `EngineExtArgs` in `risk_core`

**Files:**
- Create: `v04/risk_core/src/engine_ext.rs`
- Modify: `v04/risk_core/src/lib.rs`, `v04/risk_core/src/presets.rs`

**Interfaces:**
- Produces:
  ```rust
  pub const REF_CONF_MAX_BPS: u16 = 1_000;
  pub const REF_CLAMP_MAX_BPS: u16 = 5_000;
  pub const RELIST_DELAY_MIN_MS: u64 = 60_000;
  pub const RELIST_DELAY_MAX_MS: u64 = 604_800_000;
  pub const SANITY_LO_MIN_BPS: u32 = 1_000;
  pub const SANITY_HI_MAX_BPS: u32 = 100_000;
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] // + borsh under the feature, like RiskArgs
  pub struct EngineExtArgs { /* six fields, R10-1 order */ }
  impl EngineExtArgs { pub fn validate(&self) -> Result<(), RiskArgsError>; }
  // presets.rs, feature "presets":
  impl EngineExtArgs { pub const fn legacy() -> Self; pub const fn recommended() -> Self; }
  ```
  All re-exported from `risk_core`.

- [ ] **Step 1: Write the failing tests** in `engine_ext.rs` (`#[cfg(all(test, feature = "presets"))]`):
  - `engine_ext_presets_validate`: `legacy().validate()` and `recommended().validate()` are `Ok`, and `legacy() == recommended()`.
  - `engine_ext_bounds_are_inclusive`: each of these is `Ok`: `ref_conf_bps` 0 and 1_000; `ref_clamp_bps` 0 and 5_000; `debt_relist_delay_ms` 60_000 and 604_800_000; `ref_sanity_lo_bps` 1_000 and 10_000; `ref_sanity_hi_bps` 10_000 and 100_000; `ref_scale` 0 and 1.
  - `engine_ext_refuses_the_first_bad_field`: one step past each bound gives `Err(RiskArgsError { field })` with `field` equal to the field's name (`"ref_conf_bps"` at 1_001, `"ref_clamp_bps"` at 5_001, `"debt_relist_delay_ms"` at 59_999 and 604_800_001, `"ref_sanity_lo_bps"` at 999 and 10_001, `"ref_sanity_hi_bps"` at 9_999 and 100_001, `"ref_scale"` at 2).
- [ ] **Step 2:** Run `cargo test -p risk_core --features presets engine_ext`. Expected: does not compile (`EngineExtArgs` not found).
- [ ] **Step 3:** Implement the struct, the constants, `validate` (fields checked in declaration order) and the two presets.
- [ ] **Step 4:** Run the same command. Expected: 3 passed.
- [ ] **Step 5:** Gate, then commit: `feat(v04): EngineExtArgs with bounds and presets (ADR-082)`.

### Task 2: Carry `EngineExtArgs` on the wire and in state

**Files:**
- Modify: `v04/engine_core/src/msg.rs`, `msg_tests.rs`, `ext.rs`, `system_state.rs`, `state.rs` (fixture only), `gate.rs` (test literal)
- Modify: `v04/programs/methods/guest/src/bin/engine/admin.rs`, `pause.rs`, `engine.rs`
- Modify: `v04/harness/src/world.rs`, `v04/sdk/src/builder.rs`, `v04/sdk/src/system.rs`, and every `InitArgs`/`SetEngineParamsArgs`/`EngineMsg::Init`/`SetEngineParams` literal (34 uses in 15 files; the compiler lists them)
- Create: `v04/harness/tests/engine_ext.rs`

**Interfaces:**
- Consumes: `EngineExtArgs`, `validate`, presets (Task 1).
- Produces:
  ```rust
  // msg.rs
  Init { .., protocol_genesis: Option<ProtocolGenesis>, ext: Box<EngineExtArgs> }      // 0
  SetEngineParams { salt, params, ref_oracle, ext: Box<EngineExtArgs> }                // 20
  // ext.rs
  pub struct ConfigExt { pub system_level: u8, pub engine: EngineExtArgs }             // no Default
  impl ConfigExt { pub const fn new(engine: EngineExtArgs) -> Self }                   // system_level 0
  pub fn validate_engine_ext(ext: &EngineExtArgs) -> Result<(), EngineError>           // InvalidConfig
  // system_state.rs
  pub struct SystemCtx { .., pub system: SystemParamsRec, pub ext: EngineExtArgs }
  // harness world.rs (existing signatures keep working with the legacy ext)
  pub fn init_msg_ext(&self, params: EngineParamsArgs, ext: EngineExtArgs, ref_oracle: Option<Actor>, genesis: Option<ProtocolGenesis>) -> EngineMsg
  pub fn set_engine_params_msg_ext(&self, params: EngineParamsArgs, ext: EngineExtArgs, ref_oracle: Option<Actor>) -> EngineMsg
  // sdk
  pub struct InitArgs { .., pub ext: EngineExtArgs }
  pub struct SetEngineParamsArgs { .., pub ext: EngineExtArgs }
  ```

- [ ] **Step 1: Write the failing tests.**
  - `msg_tests.rs`: `init_and_set_engine_params_carry_the_ext` — both messages round-trip through borsh with `EngineExtArgs { ref_clamp_bps: 1_500, ..EngineExtArgs::legacy() }`, first bytes 0 and 20.
  - `system_state.rs`: extend `ctx_copies_the_config` with `assert_eq!(ctx.ext, config.ext.engine)`.
  - `harness/tests/engine_ext.rs`:
    - `init_writes_the_engine_ext`: `Init` with `ref_clamp_bps: 1_500`; `config_state().ext.engine.ref_clamp_bps == 1_500`.
    - `init_rejects_an_out_of_bound_engine_ext`: `debt_relist_delay_ms: 59_999` reverts with `fail_text(EngineError::InvalidConfig)`.
    - `set_engine_params_replaces_the_engine_ext`: after it, the stored `engine` equals the new value and `system_level` is unchanged.
    - `set_engine_params_rejects_an_out_of_bound_engine_ext`: `ref_scale: 2` reverts `InvalidConfig`; state unchanged.
    - `changing_ref_scale_needs_an_idle_system`: with a debt auction live, `SetEngineParams` changing only `ref_scale` reverts with the same text the existing locked-field test expects for a `system` change (read it from `harness/tests/engine_params.rs`); changing only `ref_clamp_bps` succeeds.
    - `set_pause_level_keeps_the_engine_ext`: `Init` with `ref_clamp_bps: 1_500`, `SetPauseLevel 2`, then `ext.engine.ref_clamp_bps == 1_500` and `system_level == 2`.
- [ ] **Step 2:** Run `cargo test -p engine_core msg_tests`. Expected: does not compile.
- [ ] **Step 3:** Implement.
  - `init_config`: call `validate_engine_ext` right after `c.params.validate()`; write `ext: ConfigExt::new(*ext)`.
  - `updated_config`: validate the same way; write `ext: ConfigExt { engine: *ext, ..old.ext }`.
  - `on_set_engine_params`: `locked_fields_change` also true when `ext.ref_scale != old.ext.engine.ref_scale`; `plan_set_engine_params` in the SDK mirrors it.
  - guest `pause.rs:53-58`: `ConfigExt { system_level: level, ..config.ext }`.
  - `fixtures::config()`: `ext: ConfigExt::new(EngineExtArgs::legacy())`.
  - `init_msg_full` and `set_engine_params_msg` call the new `_ext` builders with `EngineExtArgs::legacy()`.
- [ ] **Step 4:** Run `cargo test -p engine_core`, then `cargo test -p harness --test engine_ext --test engine_params --test pause_levels`, then `cargo test -p sdk`. Expected: all pass; the six new harness tests pass.
- [ ] **Step 5:** Gate, then commit: `feat(v04): Init and SetEngineParams carry EngineExtArgs; ConfigExt and SystemCtx store it`.

### Task 3: The reference oracle and the relist delay read their parameters

**Files:**
- Modify: `v04/risk_core/src/feed.rs`, `v04/engine_core/src/system.rs`, guest `system.rs` (the `precheck` call)
- Create: `v04/engine_core/src/system_ref_tests.rs` (declared `#[cfg(test)] mod system_ref_tests;` in `lib.rs`)
- Modify: `v04/harness/tests/engine_ext.rs`, `v04/harness/tests/debt.rs`

**Interfaces:**
- Consumes: `SystemCtx.ext` (Task 2).
- Produces:
  ```rust
  // risk_core::feed
  pub fn effective_ref_price_ext(admin_ray: u128, oracle: Option<Result<u128, FeedFault>>, clamp_bps: u16) -> u128
  // engine_core::system
  pub fn ref_price(ctx: &SystemCtx, reading: Option<RefReading>, now_ms: u64) -> u128   // signature unchanged
  pub fn debt_precheck(acc: &AccountingState, nonce: u64, now_ms: u64, relist_delay_ms: u64) -> Result<(), EngineError>
  ```

- [ ] **Step 1: Write the failing tests.**
  - `risk_core/src/feed.rs`: proptest `effective_ref_price_ext_matches_v024_at_2000` — for any `admin_ray` and any `Option<Result<u128, ()>>` oracle value, `effective_ref_price_ext(admin, o, 2_000) == auction_core::price::effective_ref_price(admin, o)` (map the error side to `FeedFault::OutOfRange` / `PauseReason::OutOfRange`). Unit test `ref_clamp_zero_pins_to_the_admin_price`: clamp 0 returns `admin_ray` for any valid oracle price.
  - `system_ref_tests.rs` (admin price 2 RAY, as `ref_price_clamps_to_twenty_percent`):
    - `ref_clamp_is_a_parameter`: with `ref_clamp_bps: 1_000`, oracle 3 RAY gives 2.2 RAY and 1.5 RAY gives 1.8 RAY.
    - `ref_conf_limit_is_a_parameter`: a reading with confidence 5 % of its price is used at `ref_conf_bps: 1_000` and falls back to the admin price at `ref_conf_bps: 100`.
    - `q64_reference_oracle_reads`: with `ref_scale: 1`, a reading of `2 << 64` is used as 2 RAY.
    - `ref_sanity_window_is_a_parameter`: an oracle reading of 5 RAY (above twice the admin price) falls back to the admin price at `ref_sanity_hi_bps: 20_000`, and is used (then clamped to 2.4 RAY) at `ref_sanity_hi_bps: 30_000`.
    - `debt_relist_delay_is_a_parameter`: `debt_precheck` with delay 60_000 refuses `DelayNotElapsed` at `last_debt_end_ms + 59_999` and passes at `+ 60_000`.
    - `legacy_preset_equals_the_v024_constants`: `EngineExtArgs::legacy()` has `ref_conf_bps == MAX_CONF_BPS`, `ref_clamp_bps == REF_ORACLE_CLAMP_BPS`, `debt_relist_delay_ms == DEBT_RELIST_DELAY_MS`, `ref_sanity_lo_bps == LEGACY_SANITY_LO_BPS`, `ref_sanity_hi_bps == LEGACY_SANITY_HI_BPS`, `ref_scale == 0`; and `RELIST_DELAY_MAX_MS == MAX_SYSTEM_TTL_MS`, `REF_CONF_MAX_BPS == MAX_CONF_BPS`.
  - `harness/tests/debt.rs`: `relist_delay_follows_the_engine_ext` — a world initialised with `debt_relist_delay_ms: 60_000` starts a second debt auction 60_000 ms after the first one ended (the legacy 600_000 would refuse it).
- [ ] **Step 2:** Run `cargo test -p engine_core system_ref`. Expected: does not compile.
- [ ] **Step 3:** Implement.
  - `ref_price` builds `FeedPolicy { scale (0 → Ray, 1 → Q64x64), max_age_ms: ctx.ref_max_age_ms, max_conf_bps: ctx.ext.ref_conf_bps, sanity_lo_bps: ctx.ext.ref_sanity_lo_bps, sanity_hi_bps: ctx.ext.ref_sanity_hi_bps, feed_min_raw: 0, feed_max_raw: u128::MAX }`, calls `validate_feed_ext` with `now_ms.saturating_add(ctx.slack_ms)`, then `effective_ref_price_ext(admin, Some(validity), ctx.ext.ref_clamp_bps)`. An unknown `ref_scale` (unreachable after validation) falls back to the admin price.
  - `start_debt_op` passes `ctx.ext.debt_relist_delay_ms`; the guest `precheck` passes `config.ext.engine.debt_relist_delay_ms`.
  - Remove the now-unused imports of `MAX_CONF_BPS`, `effective_ref_price`, `validate_feed` and `DEBT_RELIST_DELAY_MS` from `system.rs` non-test code.
- [ ] **Step 4:** Run `cargo test -p risk_core feed`, `cargo test -p engine_core system`, `cargo test -p harness --test debt --test security_surplus --test solvency`. Expected: all pass, including the three existing `ref_price_*` tests unchanged.
- [ ] **Step 5:** Gate, then commit: `feat(v04): reference-oracle limits, scale and the debt relist delay are init parameters (E9)`.

### Task 4: `RiskArgs` gains the sanity window, pin bounds and grace fields; the per-type sanity window is live

**Files:**
- Modify: `v04/risk_core/src/args.rs`, `presets.rs`; `v04/engine_core/src/ext.rs`, `feed.rs`; guest `admin.rs` (`validate_risk` call at :460, `fresh_collateral`); `v04/sdk/examples/tier2.rs` (literal); `v04/harness/tests/risk_args.rs`

**Interfaces:**
- Produces:
  ```rust
  pub const GRACE_MAX_MS: u64 = 86_400_000;
  pub struct RiskArgs { /* five Plan 9 fields */, pub sanity_lo_bps: u32, pub sanity_hi_bps: u32,
      pub feed_min_raw: u128, pub feed_max_raw: u128, pub grace_ms: u64, pub grace_min_gap_ms: u64 }
  impl RiskArgs { pub fn validate(&self, max_oracle_age_ms: u64, band_lo_bps: u32, band_hi_bps: u32) -> Result<(), RiskArgsError> }
  // engine_core::ext
  pub fn validate_risk(risk: &RiskArgs, args: &CollateralArgs) -> Result<(), EngineError>
  // engine_core::feed
  pub fn feed_policy(coll: &CollateralState) -> Result<FeedPolicy, EngineError>   // now reads coll.ext.risk
  ```
  `fresh_collateral` writes `RiskArgs` with the five Plan 9 fields 0 and the six new ones at the legacy values of R10-7 (written as literals: the guest has no presets).

- [ ] **Step 1: Write the failing tests** (`MAX_AGE = 86_400_000`, bands `9_000`/`11_000`).
  - `args.rs`:
    - `sanity_window_bounds_are_inclusive`: `sanity_lo_bps` 1_000 and 9_000 are `Ok`, 999 and 9_001 refuse `"sanity_lo_bps"`; `sanity_hi_bps` 11_000 and 100_000 are `Ok`, 10_999 and 100_001 refuse `"sanity_hi_bps"`.
    - `pin_bounds_must_be_ordered`: `(0, u128::MAX)` and `(5, 6)` are `Ok`; `(6, 6)` and `(7, 6)` refuse `"feed_min_raw"`.
    - `grace_bounds_are_inclusive`: `grace_ms` 86_400_000 with gap 1 is `Ok`; 86_400_001 refuses `"grace_ms"`; `grace_min_gap_ms` 86_400_001 refuses `"grace_min_gap_ms"`.
    - `grace_needs_a_nonzero_min_gap`: `grace_ms: 1, grace_min_gap_ms: 0` refuses `"grace_min_gap_ms"`; `grace_ms: 0, grace_min_gap_ms: 5` is `Ok`.
    - `grace_and_pins_work_with_the_twap_off`: legacy plus `grace_ms: 1_800_000, grace_min_gap_ms: 600_000, feed_min_raw: 1` is `Ok`.
    - Existing `legacy_and_recommended_presets_validate` keeps passing with the new signature.
  - `engine_core/src/feed.rs`: `sanity_window_is_a_parameter` — with `R = RAY`, a reading of `3 * RAY` is `OutOfRange` under the legacy risk args and valid with `sanity_hi_bps: 40_000` (collateral `band_hi_bps` left at its fixture value).
  - `harness/tests/risk_args.rs`: extend `set_collateral_rejects_out_of_bound_risk_args` with `sanity_lo_bps: 9_001` (above the type's `band_lo_bps` 9_000) and `grace_ms: 1` with gap 0; extend `set_collateral_writes_the_risk_args` to assert the six new fields are stored.
- [ ] **Step 2:** Run `cargo test -p risk_core --features presets args`. Expected: does not compile.
- [ ] **Step 3:** Implement. New checks run in a fourth function `check_feed_and_grace(band_lo_bps, band_hi_bps)` called last by `validate`, in the field order of R10-7. `recommended()` and `legacy()` get the R10-7 values. `feed_policy` fills `sanity_*` and `feed_*_raw` from `coll.ext.risk`. `sdk/examples/tier2.rs` `risk_args()` becomes `RiskArgs { <its five values>, ..RiskArgs::legacy() }`.
- [ ] **Step 4:** Run `cargo test -p risk_core --features presets`, `cargo test -p engine_core feed`, `cargo test -p harness --test risk_args --test twap`. Expected: all pass; `validate_feed_ext_matches_v024_under_legacy_policy` still passes.
- [ ] **Step 5:** Gate; ask the user to run `cargo test --locked --workspace`; then commit: `feat(v04): per-type sanity window, pin bounds and grace fields in RiskArgs`.

### Task 5: The pinned rule (6076)

**Files:**
- Modify: `v04/risk_core/src/feed.rs`, `error.rs`, `kani_proofs.rs` (only if it names `pause_reason`); `v04/engine_core/src/feed.rs`, `gate.rs`; guest call sites of `require_live`/`Observed.feed`; `v04/harness/src/levels_fixtures.rs` (only if it matches on `obs.feed`)
- Create: `v04/harness/tests/grace.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum FeedFault { Stale, ZeroPrice, LowConfidence, OutOfRange, Pinned }
  impl FeedFault { pub const fn pause_reason(self) -> Option<PauseReason> }   // None for Pinned
  pub enum RiskError { .., PriceFeedPinned /* 76 */ }
  // engine_core::feed
  pub struct Observed { pub clock: PauseClock, pub feed: Result<u128, FeedFault>, pub conf_bps: u16 }
  pub fn feed_refusal(fault: FeedFault) -> Refusal
  pub fn require_live(coll: &CollateralState, obs: &Observed) -> Result<u128, Refusal>
  ```

- [ ] **Step 1: Write the failing tests.**
  - `risk_core/src/feed.rs` (policy: legacy plus `feed_min_raw: 100, feed_max_raw: 1_000`, scale Ray, `R` chosen so 100..=1_000 is inside the sanity window):
    - `pinned_low_and_high_readings_are_invalid`: `price_raw` 100 and 99 give `Pinned`; 1_000 and 1_001 give `Pinned`; 101 and 999 are `Ok`.
    - `pinned_is_checked_after_stale_and_before_zero`: a stale reading at 100 is `Stale`; a fresh reading with `price_raw: 0` is `Pinned` under this policy and `ZeroPrice` under `FeedPolicy::legacy`.
    - `unset_pin_bounds_pin_nothing`: with `(0, u128::MAX)`, `price_raw` 1 and `u128::MAX` never give `Pinned`.
    - Update `validate_feed_ext_checks_in_v024_order_and_maps_faults` to the `Option` return (four `Some(..)`, plus `Pinned → None`), and `risk_error_codes_are_70_and_up` to `[70, 71, 73, 74, 75, 76, 77]`.
  - `harness/tests/grace.rs` (world `new_tier2` with `RiskArgs { feed_min_raw: <raw of RAY/4>, feed_max_raw: <raw of RAY>, ..RiskArgs::recommended() }`, grace off for these three: `grace_ms: 0, grace_min_gap_ms: 0`):
    - `pinned_low_and_high_readings_pause_the_type`: publish the low bound, `Liquidate` reverts with `fail_text(RiskError::PriceFeedPinned)`; `ObserveFeed` then leaves `paused_since_ms` set; same for the high bound.
    - `pinned_reading_is_not_recorded`: warm the ring, publish a pinned reading, `ObserveFeed`; `ext.ring` is unchanged.
    - `a_bid_on_a_pinned_feed_uses_d4_or_the_pinned_code`: with D4 on, a bid on a live auction fills at the fallback price; with `stale_bid_max_age_ms: 0` (and its dependent fields valid) the bid reverts with `PriceFeedPinned`.
- [ ] **Step 2:** Run `cargo test -p risk_core feed`. Expected: does not compile (`Pinned`).
- [ ] **Step 3:** Implement. In `validate_feed_ext`, after the stale check: `if (policy.feed_min_raw > 0 && r.price_raw <= policy.feed_min_raw) || (policy.feed_max_raw < u128::MAX && r.price_raw >= policy.feed_max_raw) { return Err(FeedFault::Pinned) }`. `feed_refusal`: `Pinned` → `RiskError::PriceFeedPinned`; otherwise `EngineError::from(reason)`. `gate::fallback_pricing`'s D4-off branch returns `feed_refusal(fault)`.
- [ ] **Step 4:** Run `cargo test -p risk_core --features presets`, `cargo test -p engine_core`, `cargo test -p harness --test grace --test stale_bids --test pause_matrix --test liquidate`. Expected: all pass.
- [ ] **Step 5:** Gate, then commit: `feat(v04): a reading at the feed's answer bound is invalid (PriceFeedPinned, ADR-075)`.

### Task 6: Grace as a pure rule and a table column

**Files:**
- Create: `v04/risk_core/src/grace.rs`
- Modify: `v04/risk_core/src/lib.rs`, `error.rs`, `pause.rs`

**Interfaces:**
- Produces:
  ```rust
  // grace.rs
  #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
  pub struct GraceState { pub last_good_ts_ms: Option<u64>, pub feed_invalid_since_ms: Option<u64>, pub grace_until_ms: u64 }
  pub struct GraceObservation { pub valid: bool, pub reading_ts_ms: u64, pub now_ms: u64, pub max_age_ms: u64 }
  pub fn observed(state: GraceState, o: GraceObservation, grace_ms: u64, grace_min_gap_ms: u64) -> GraceState
  pub const fn in_grace(state: &GraceState, now_ms: u64) -> bool        // now_ms < grace_until_ms
  // error.rs
  GracePeriod /* 72 */
  // pause.rs
  pub enum FeedCase { Live, Stale, Grace }                              // ALL has 3
  ```

- [ ] **Step 1: Write the failing tests.**
  - `grace.rs` unit tests (`max_age 600_000`, `grace_ms 1_800_000`, `min_gap 600_000`):
    - `observed_outage_starts_grace`: valid at t=0; invalid observed at `now 700_000` sets `feed_invalid_since_ms = Some(600_000)` (`last_good + max_age`); valid at `now 1_300_000` gives `grace_until_ms = 3_100_000` and `feed_invalid_since_ms = None`.
    - `grace_needs_a_gap_of_at_least_min`: same, but the valid reading arrives at `now 1_199_999` (gap 599_999): `grace_until_ms` stays 0; at `1_200_000` it is set.
    - `unobserved_gap_starts_grace`: valid at ts 0; next valid reading has ts `1_200_000` at `now 1_200_000` with no invalid observation between: gap = `1_200_000 − (0 + 600_000) = 600_000`, grace starts.
    - `first_observation_starts_no_grace`: from `GraceState::default()`, a valid observation leaves `grace_until_ms == 0`.
    - `grace_off_never_starts`: `grace_ms = 0` with any gap leaves `grace_until_ms == 0`.
    - `a_second_outage_only_extends`: `grace_until_ms` never decreases.
    - `grace_expires_at_its_end`: `in_grace` is true at `grace_until_ms − 1` and false at `grace_until_ms`.
    - `invalid_onset_is_clamped_to_now`: an invalid observation at `now < last_good + max_age` (pinned, fresh) sets `feed_invalid_since_ms = Some(now)`; with no `last_good`, also `Some(now)`.
  - `grace.rs` proptest `grace_until_is_monotone_and_bounded`: for any state and observation, `next.grace_until_ms >= state.grace_until_ms` and `next.grace_until_ms <= max(state.grace_until_ms, now.saturating_add(grace_ms))`.
  - `pause.rs`: `TABLE` gains four `Grace` columns (L0-L3). They equal the `Live` columns except `Liquidate` at L0, which is `GracePeriod`. `pause_table_is_exhaustive` asserts `cases == 204`. `risk_error_codes_are_70_and_up` becomes `[70, 71, 72, 73, 74, 75, 76, 77]`.
- [ ] **Step 2:** Run `cargo test -p risk_core grace`. Expected: does not compile.
- [ ] **Step 3:** Implement `observed`:
  ```
  ts = min(reading_ts_ms, now_ms)
  valid:   gap = match feed_invalid_since { Some(s) => now_ms − s (saturating),
                                            None => last_good.map_or(0, |l| ts − (l + max_age)) (saturating) }
           if grace_ms > 0 && gap >= grace_min_gap_ms && gap > 0: grace_until = max(grace_until, now_ms + grace_ms)
           feed_invalid_since = None;  last_good = Some(max(last_good, ts))
  invalid: feed_invalid_since = Some(old) if set, else Some(min(last_good.map_or(now_ms, |l| l + max_age), now_ms))
  ```
  In `allowed`: `Op::Liquidate` after the level arm maps `FeedCase::Grace => Err(RiskError::GracePeriod)`; `Op::Bid` maps `Grace => Gate::Run`. No wildcard arm.
- [ ] **Step 4:** Run `cargo test -p risk_core --features presets`. Expected: all pass.
- [ ] **Step 5:** Gate (the `engine_core` matches on `FeedCase` need the new arm to compile: add it with the `Live` behaviour in this task), then commit: `feat(v04): grace-period rule and the Grace column of the pause table (ADR-075)`.

### Task 7: Grace in the engine

**Files:**
- Modify: `v04/engine_core/src/ext.rs`, `feed.rs`, `gate.rs`; guest `observe.rs` (only if `record`'s call changes); `v04/harness/src/levels_fixtures.rs`, `tier2.rs`; `v04/harness/tests/grace.rs`, `pause_matrix.rs`

**Interfaces:**
- Consumes: `grace::{GraceState, GraceObservation, observed, in_grace}`, `FeedCase::Grace` (Task 6); `Observed.feed: Result<u128, FeedFault>` (Task 5).
- Produces:
  ```rust
  pub struct CollateralExt { pub risk, pub pause_level, pub ring,
      pub last_good_ts_ms: Option<u64>, pub feed_invalid_since_ms: Option<u64>, pub grace_until_ms: u64 }
  impl CollateralExt { pub const fn grace(&self) -> GraceState; }
  // gate.rs
  pub fn feed_case(coll: &CollateralState, obs: &Observed, now_ms: u64) -> FeedCase
  pub fn collateral_gate(coll: &CollateralState, op: Op, feed: FeedCase) -> Result<Gate, Refusal>
  // harness
  pub fn grace_fixture(op: Op, risk: RiskArgs) -> Fixture        // levels_fixtures.rs
  pub fn start_grace(&mut self)                                  // tier2.rs: go_stale, observe, wait min gap, publish, observe
  ```
  `feed::record` keeps its signature and now writes the three grace fields on every observation (valid or not), and the ring as before.

- [ ] **Step 1: Write the failing tests.**
  - `engine_core/src/gate.rs`: `liq_gate_order_level_feed_grace_twap` — at level 1 inside grace the refusal is `LiquidationsPaused`; on an invalid feed inside a previous grace it is the feed's code; on a valid feed inside grace it is `GracePeriod` even while the TWAP is unavailable.
  - `engine_core/src/feed.rs`: `record_tracks_the_grace_state` — an invalid observation sets `feed_invalid_since_ms` and leaves the ring; the next valid one clears it, sets `last_good_ts_ms` and, past the minimum gap, `grace_until_ms`.
  - `harness/tests/grace.rs` (recommended preset: grace 1_800_000 / 600_000):
    - `grace_blocks_liquidate_not_bid`: one auction live and one unsafe position open; after `start_grace`, `Liquidate` reverts with `fail_text(RiskError::GracePeriod)` and a bid on the live auction fills.
    - `grace_refusal_moves_nothing`: `snapshot_all()` is equal before and after the refused `Liquidate`, and `invariants::check` holds.
    - `grace_expires_at_its_end`: `Liquidate` at `grace_until_ms − 1` reverts; at `grace_until_ms` (with a fresh price and a warm TWAP) it succeeds.
    - `a_short_outage_starts_no_grace`: an observed outage shorter than 600_000 ms leaves `grace_until_ms == 0` and `Liquidate` runs.
    - `turning_grace_on_does_not_start_one`: legacy world observed regularly; `SetCollateral` to the recommended risk args; the next `ObserveFeed` leaves `grace_until_ms == 0`.
    - `set_collateral_keeps_the_grace_state`: during grace, `SetCollateral` with the same args leaves `grace_until_ms` unchanged.
  - `harness/tests/pause_matrix.rs`: `every_operation_during_grace` — `GRACE_CASES = 17`; for each `op` in `Op::ALL`, `grace_fixture(op, RiskArgs::recommended())` and `expected(op, Level::Normal, FeedCase::Grace, &risk)`; same `mismatch` checks as the level test.
- [ ] **Step 2:** Run `cargo test -p engine_core gate`. Expected: does not compile.
- [ ] **Step 3:** Implement.
  - `feed_case`: `Stale` when `obs.feed` is `Err`; else `Grace` when `in_grace(&coll.ext.grace(), now_ms)`; else `Live`.
  - `gated_market` and `bid_pricing` take the case from `feed_case`; `liquidation_price` passes its `now_ms`. The guest already hands `liquidation_price` the post-`record` state (`observe.rs:240`), which is what R10-12 needs; assert it stays so.
  - `record`: build `GraceObservation { valid: obs.feed.is_ok(), reading_ts_ms: reading.timestamp_ms, now_ms, max_age_ms: coll.max_oracle_age_ms }` and call `grace::observed` with `risk.grace_ms`, `risk.grace_min_gap_ms`; write the result in both match arms; no full `CollateralExt` literal without `..coll.ext.clone()`.
  - `CollateralExt::new` sets the three fields to `None, None, 0`. `on_configure` already spreads `..base.ext.clone()`.
  - `grace_fixture`: `before_tx = World::start_grace`.
- [ ] **Step 4:** Run `cargo test -p engine_core`, `cargo test -p harness --test grace --test pause_matrix --test twap --test stale_bids --test liquidate --test bid`. Expected: all pass.
- [ ] **Step 5:** Gate; ask the user to run `cargo test --locked --workspace`; then commit: `feat(v04): grace period refuses Liquidate after a feed or chain gap (GracePeriod, ADR-075)`.

### Task 8: SDK oracle bounds, the e2e stage, cycles

**Files:**
- Modify: `v04/sdk/src/builder.rs`, `lib.rs`, `v04/sdk/tests/tier2_plans.rs`, `v04/sdk/examples/tier2.rs`, `v04/harness/tests/cycles_tier2.rs`, `v04/CYCLES.md`

**Interfaces:**
- Produces:
  ```rust
  pub enum OracleBounds { Undeclared, Declared { min_raw: u128, max_raw: u128 } }
  pub fn recommended_risk(bounds: OracleBounds) -> RiskArgs
  ```

- [ ] **Step 1: Write the failing tests** in `sdk/tests/tier2_plans.rs`:
  - `recommended_risk_takes_the_oracles_bounds`: `Undeclared` equals `RiskArgs::recommended()`; `Declared { min_raw: 5, max_raw: 9 }` has `feed_min_raw == 5`, `feed_max_raw == 9` and is otherwise equal.
  - `init_and_set_engine_params_plans_declare_every_touched_actor` with `ref_clamp_bps: 1_500` (same `run` helper as the existing two tests).
- [ ] **Step 2:** Run `cargo test -p sdk --test tier2_plans`. Expected: does not compile.
- [ ] **Step 3:** Implement `OracleBounds` and `recommended_risk`; re-export both.
- [ ] **Step 4:** Extend `examples/tier2.rs` per R10-13: `risk_args()` adds `grace_ms: 30_000, grace_min_gap_ms: 30_000`; `open_positions` opens P3 (same size as P2); new step `grace_refused` between `fallback_final` and `level_3_settles`. The step must print the refusal code it saw and fail unless it is 6072.
- [ ] **Step 5:** `cycles_tier2.rs`: measure the existing rows unchanged (budgets and `HIGH_PRICE_EXTRA` stay). Add an assertion that `Liquidate` under the recommended preset with grace configured but not active stays `<= 1_000_000`. Run `cargo test -p harness --test cycles --test cycles_tier2 -- --nocapture` and copy the new figures into `v04/CYCLES.md` from `$CARGO_TARGET_DIR/cycles-v04*.md`.
- [ ] **Step 6:** Ask the user to run `v04/scripts/e2e.sh tier2` in their own terminal (about 16 minutes). Expected: `tier2 complete`.
- [ ] **Step 7:** Gate, then commit: `feat(v04): SDK oracle bounds for the recommended preset; tier2 e2e shows the grace refusal`.

### Task 9: Docs, generators and the F8 check

**Files:**
- Modify: `scripts/gen-refusal-codes.py`, `scripts/check-requirements.py`, `scripts/tests/test_gen_refusal_codes.py`, `scripts/tests/test_check_requirements.py`
- Modify: `docs/architecture/refusal-codes.md` (generated), `docs/architecture/f8-parameters.md`, `docs/architecture/pricing-model.md`, `docs/design-log.md`, `docs/v04-port-design.md`, `docs/superpowers/specs/2026-11-08-tier2-engine-design.md`, `REQUIREMENTS.md`, `README.md` (generated block)

- [ ] **Step 1: Write the failing script tests.**
  - `test_gen_refusal_codes.py`: the catalogue has rows 6072 `GracePeriod` and 6076 `PriceFeedPinned`, each marked "(0.4 only)" with a proving test name.
  - `test_check_requirements.py`: `f8_gap_rows_fail_the_check` — a temp `f8-parameters.md` containing a table row with `production default — Tier 2` makes the check exit non-zero; without such a row it exits 0. The kind's definition line (not a table row) does not trip it.
- [ ] **Step 2:** Run `python3 -m unittest discover -s scripts/tests -v`. Expected: the two new tests fail.
- [ ] **Step 3:** Implement: two `MEANINGS` entries (72: liquidation refused during the grace period after a feed or chain gap; 76: the reading is at the feed's configured answer bound); in `check-requirements.py`, a check of `docs/architecture/f8-parameters.md` (path relative to `--root`) that fails on any table row whose Kind cell is `production default — Tier 2`.
- [ ] **Step 4:** Update the docs.
  - `f8-parameters.md`: the five gap rows and the `LEGACY_SANITY_*` row change Kind to init parameter (or init parameter bound for the new `*_MAX`/`*_MIN` constants, which get rows); new table "`EngineExtArgs` (v04, `Init` and `SetEngineParams`)" with legacy | recommended | bound | ADR; the `RiskArgs` table gains the six fields; the "F8 gaps" section states that no gap is left and names the closing tests; the counts line is recomputed.
  - `REQUIREMENTS.md`: F8 → "PoC" with the Task 2-4 test names; R2 note gains the pinned rule and grace; R5 note gains the grace column. Replace "it has not run in CI yet" (R2, R5) with the hosted run: `v04-e2e` run 43392160297 on `06fedda`, 2026-11-13.
  - `pricing-model.md` §2: grace and pinned as built; the same hosted-run correction.
  - `design-log.md`: ADR-075 written in full (Context, Options, Decision, Trade-offs, Evidence, Status), including R10-9 to R10-12; ADR-082 Status → built, with R10-1 to R10-8; the hosted-run correction where the log says the tier2 e2e has not run in CI.
  - `v04-port-design.md` §3: the appended state fields.
  - Spec: as-built amendments for the stale line references (§3.3 `pause.rs:335-339` is `crates/auction_core/src/pause.rs:63-67`), `last_good_ts_ms`, the `FeedCase::Grace` form of the matrix column, `OracleBounds`, and no `now_ms` in Plan 10.
  - Kani: state "written, not run" for any new harness; none is required by this plan.
  - The proposal's F8 row is not edited here: it is the user's document and is not posted without their go-ahead.
- [ ] **Step 5:** Regenerate: `python scripts/gen-refusal-codes.py`, `python scripts/count-tests.py --markdown` into the README block. Then run `python scripts/check-requirements.py REQUIREMENTS.md`, `python scripts/gen-refusal-codes.py --check`, `python scripts/count-tests.py --check README.md`. Expected: all exit 0.
- [ ] **Step 6:** Full gate; ask the user to run `cargo test --locked --workspace` one last time; commit in two parts: `chore(scripts): refusal codes 6072 and 6076; F8 gap check` and `docs: Plan 10 — F8 to PoC, ADR-075, ADR-082 built, pinned and grace as built`.
- [ ] **Step 7:** Report to the user: test totals, measured cycle rows, the e2e result, and ask whether to push and open the pull request. Do not push without that answer.
