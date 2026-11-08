# Plan 9: TWAP, confidence pricing, pause levels and D4 stale bids (Tier 2, E1 + E2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the v04 engine an 8-slot on-chain TWAP with confidence-edge pricing and a spot-vs-TWAP breaker (E1), stepped pause levels 1–3 with the full operation × level matrix, and D4 bids on a stale feed held at a floor (E2). The `legacy` preset keeps today's behaviour.

**Architecture:**
- The pure logic goes into a new v04-only crate, `v04/risk_core`: `validate_feed_ext`, the TWAP ring, the prices, the pause rule table, the stale floor, `RiskArgs` and `RiskError`. It depends on `auction_core` only.
- `engine_core` holds the state and message plumbing (`ext.rs`), the `Refusal` wrapper (`check.rs`) and the per-operation gates (`gate.rs`).
- The guest calls those gates from the collateral actor's Observe `proceed` (Liquidate, Bid) and from the config's system roots.
- New state is appended to the existing states, and new message fields and variants are appended to `EngineMsg`. Nothing under the frozen v0.2.4 paths changes.

**Tech Stack:** Rust 1.98.1 (`v04/rust-toolchain.toml`), `lee`/`lee_core` v0.4.0-rc1, borsh 1.5.7, risc0 3.0.5, proptest 1 (already in `v04/Cargo.lock` at 1.11.0), Kani harnesses behind `cfg(kani)`, bash, Python 3.

**Spec:** `docs/superpowers/specs/2026-11-08-tier2-engine-design.md`, which this plan argues from.
- Plan 9 covers §2 (ADR-069), §3.0, §3.1, §3.2, §3.10 (Plan 9 codes only), §4 (Plan 9 rows), §5 (row "9"), §6 (ADR-069 to 074, 076 and 082) and §8 (row "Plan 9").
- Background: parent spec `docs/superpowers/specs/2026-11-07-rfp014-competitive-upgrade-design.md` §2 D4, and `docs/design-log.md` ADR-067.

**Out of scope (later plans):**
- Plan 10: E3 (pinned readings, `PriceFeedPinned` 6076, grace, `GracePeriod` 6072) and E9.
- Plan 11: E4 (`min_cr_ray`), E5 (`max_debt`) and E6.
- Plan 12: E7 (`Release.shortfall`, `AccountingExt`, 6078), E8 (`ApplyPending` 49, `CancelPending` 50, 6079, 6080, delayed lowering), and with it the `ApplyPending`/`CancelPending` rows of the matrix.

## Global Constraints

- **Frozen v0.2.4 paths (ADR-069).** Never change any file under `programs/**`, `crates/auction_core/**` or `crates/auction_engine_params/**`, nor the root `Cargo.toml` or `Cargo.lock`. Feature-gated changes are banned too. (Root-level `programs/` is frozen; `v04/programs/` is not.)
  - Check before every commit: `git diff --stat b14e59b -- programs/ crates/auction_core/ crates/auction_engine_params/ Cargo.toml Cargo.lock` must print nothing.
  - From Task 1 on, also run `scripts/check-frozen-v024.sh` (exit 0) and `scripts/check-guests.sh --sha-only` (exit 0).
  - `23a57bb` and `b14e59b` differ only in the spec file, so both bases give the same result.
- **cargo runs only inside WSL, from `v04/`.** From Git Bash:
  `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'source ~/lez-spikes/s1/env.sh && export CARGO_TARGET_DIR=$HOME/lez-target/v04-port CARGO_BUILD_JOBS=2 && cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine-port/v04 && cargo <args>'`
  - `scripts/wsl-cargo.sh` is the root workspace's wrapper. It changes directory to the repo root and uses the `plan2` target, so do not use it for v04.
  - Builds are slow: harness tests rebuild the risc0 guests, which can take 20+ minutes on a cold cache. Run cargo in the background.
  - A message such as `Blocking waiting for file lock` means another cargo is running. Wait for it. Never kill cargo, and never delete the lock.
  - During TDD, run one test target, e.g. `cargo test -p risk_core twap`. Run the full gate at the end of each task.
  - cargo takes one test-name filter before `--`. For several, pass them to the test binary: `cargo test -p risk_core -- twap record ring`.
  - `risk_core`'s tests that call `RiskArgs::legacy()`/`recommended()` are `#[cfg(feature = "presets")]`. Run them with `cargo test -p risk_core --features presets ..`.
- **Gate (end of every task):**
  - `cargo fmt -p risk_core -p engine_core -p methods -p mock_host_core -p oracle_core -p token_core -p harness -p sdk -- --check`. Task 1 runs it without `-p risk_core`, because the crate arrives in Task 2.
  - `RISC0_SKIP_BUILD=1 cargo clippy --locked --workspace --all-targets -- -D warnings`
  - `RISC0_SKIP_BUILD=1 cargo clippy --locked -p sdk --all-targets --features session -- -D warnings`
  - `cargo test --locked --workspace`
  - the frozen-path checks above
  - `python3 -m unittest discover -s scripts/tests -v` (from the repo root)
- **Existing tests keep their assertions.** Every existing v04 test, and the `slice` and `system` stages of `v04/scripts/e2e.sh`, must keep passing under the `legacy` preset.
  - The only allowed edit to an existing test is adding the new appended fields at their legacy values: `risk: Box::new(RiskArgs::legacy())`, `paused_since_ms: None`, `fallback: false`, `ext: ...`, `stale_cap_ray: 0`.
  - So a legacy auction (D4 off) must store `stale_cap_ray: 0` at every phase. `harness/tests/liquidate.rs`'s `expected_auction` literal, compared at `:123` and `:241`, depends on it (Task 9).
  - Migration note (spec §3.0): local 0.4 state does not decode after this plan. Recreate it. Tags are unchanged.
- **Wire format.** `EngineMsg` variants are append-only. The next free index is **47**: `SetPauseLevel` = 47, `PauseLevelUpdate` = 48. Indices 49 and 50 are reserved for Plan 12. Existing variants gain fields only at their end.
- **State layout.** Append only:
  - `ConfigState.ext: ConfigExt`
  - `CollateralState.ext: CollateralExt`
  - `AuctionState.stale_cap_ray: u128`
- **Refusal codes.**
  - `EngineError` cannot gain variants.
  - The new codes live in `risk_core::RiskError`. Plan 9 defines exactly `TwapUnavailable` = 70, `SpotTwapDeviation` = 71, `LiquidationsPaused` = 73, `SystemAuctionsPaused` = 74, `BidsPaused` = 75 and `LastGoodTwapTooOld` = 77 (wire 6070–6077).
  - An out-of-range value is `EngineError::InvalidConfig` (6005). Feed faults keep 6021–6023, and `admin_paused` keeps `Paused` (6020).
- **No production values in on-chain code (F8, ADR-082).** The presets are `RiskArgs::legacy()` and `RiskArgs::recommended()`.
  - They live in `risk_core::presets` behind the cargo feature `presets`. `sdk` and `harness` enable that feature. `engine_core`'s normal dependency and the guest do not.
- **File size.** No file may exceed 800 lines. Files already near the limit are `engine_core/src/state.rs` (756), `sdk/tests/builder.rs` (784), `harness/tests/security_system.rs` (771) and `harness/src/world.rs` (713). New types and tests go in new files.
- **Lints.** The v04 workspace lints apply to `risk_core`: no `as`, no unchecked arithmetic, no slice indexing, no wildcard enum arms, and every `#[allow]` carries a `reason`. All TWAP and price arithmetic is `checked_*`/`saturating_*` or `auction_core::math::mul_div_floor`.
- **Kani.** Kani is not installed in this WSL (checked 2026-11-08), and it does not run on Windows.
  - Write each Kani harness in `risk_core/src/kani_proofs.rs` behind `#[cfg(kani)]`.
  - Run `cargo kani -p risk_core` only if it is available. Otherwise record "written, not run" in the docs task, as ADR-015 did. Never cite a Kani proof as proven unless it was run.
- **Commits.**
  - Sign every commit: `git commit -S -m "<type>: <description>"`.
  - Run `git add <paths>` and `git commit` as two separate calls.
  - No `Co-Authored-By`, no AI attribution, never `--no-verify`.
  - If a GPG prompt blocks, stop and report BLOCKED.
- **Copy rule.** No rival bid or rival team is named in code, docs or commit messages.
- **Generated docs.** `README.md`'s test-count block and `docs/architecture/refusal-codes.md` are regenerated only in Task 14. Their `--check` gates are expected to drift between Task 1 and Task 14.

## Review Focus

1. **A claimed `now_ms` older than the newest ring sample.** A claim may lag the clock by up to `slack_ms`, while the newest sample was recorded at a later claim. The TWAP must not underflow or panic. Test `twap_with_now_before_the_newest_sample_does_not_underflow` (Task 4).
2. **A bid while the TWAP is on but not yet available** (warm-up, or the restart after an outage). The bid must still fill, unanchored but band-clamped (ruling below). Liveness comes first, as in the Black Thursday lesson. Test `bid_during_twap_warm_up_is_not_refused` (Task 7).
3. **A risk config that can never work.**
   - A `twap_min_obs` the window and spacing cannot reach.
   - A spacing longer than the feed's max age.
   - `stale_bid_max_age_ms <= max_oracle_age_ms`, which makes D4 impossible for a stale feed.
   - A window too short to hold `min_obs` samples once the feed is stale, which also makes D4 impossible: on a stale feed the TWAP anchor freezes at `newest.ts + max_age`.
   - Each must be refused at `SetCollateral` with `InvalidConfig`. Test `risk_args_reject_unreachable_twap_and_dead_d4` (Task 6).
4. **A pause-level byte out of range** (4, 255). It must revert `InvalidConfig` and change nothing. Test `set_pause_level_rejects_level_above_3` (Task 8).
5. **`SetCollateral` re-run on a type with a full ring and a raised level.** It must keep the ring and the level and change only the parameters. Test `set_collateral_keeps_the_ring_and_the_level` (Task 7).

---

## Rulings this plan makes (the spec leaves them open)

Each ruling is marked *(Plan 9 ruling)* where it is used.

- **R9-1 Presets.** `RiskArgs::legacy()` and `RiskArgs::recommended()` are inherent methods on `risk_core::args::RiskArgs`, behind risk_core's `presets` feature.
  - `sdk` re-exports `RiskArgs`, and the harness depends on `risk_core` with `presets`.
  - Reason: an inherent method must live in the type's crate, and the guest must decode the type.
  - In Plan 9, `recommended()` takes no argument. Plans 10 and 11 add the oracle bounds and the type cap.
  - Plan 9 has no `EngineExtArgs`. It arrives with E9 in Plan 10, together with the `Init`/`SetEngineParams` `ext` fields.
- **R9-2 RiskArgs grows per plan.**
  - Plan 9 fields: `twap_window_ms`, `twap_min_spacing_ms`, `twap_min_obs`, `max_spot_twap_dev_bps`, `stale_bid_max_age_ms`.
  - `FeedPolicy` already carries the sanity window and pin bounds, filled with legacy values.
- **R9-3 Extra bounds.** These bounds are added because a config that breaks them can never liquidate, or can never use D4:
  - `twap_min_spacing_ms <= max_oracle_age_ms`
  - `twap_min_obs <= twap_window_ms / twap_min_spacing_ms`
  - `stale_bid_max_age_ms > max_oracle_age_ms` whenever `stale_bid_max_age_ms > 0`
  - `twap_window_ms > max_oracle_age_ms + (twap_min_obs − 2) · twap_min_spacing_ms` (saturating; `twap_min_obs − 2` floors at 0) whenever `stale_bid_max_age_ms > 0`. Reason: on a stale feed `twap` anchors at `newest.ts + max_age`. The `k`-th newest sample is at most `newest.ts − (k − 1) · spacing`, so a window at or below this bound can never hold `min_obs` samples, and D4 would always refuse `TwapUnavailable`. The refused field is `twap_window_ms`.
  - With `twap_window_ms = 0`, every other Plan 9 field must be 0. The refused field is the first nonzero one.
  - `validate` checks the §3.1.6 and §3.2.4 ranges first, then these bounds in the order listed. The first failure is returned.
- **R9-4 `Configure` (15) gains `risk: Box<RiskArgs>`.** The config relays the validated args to the collateral actor. §3.0 lists only `SetCollateral`.
- **R9-5 Live bid with the TWAP unavailable or `dev = 0`.** The price is `band(edge(M))`, unanchored, and the bid is not refused. `dev = 0` also skips the deviation breaker (spec: "0 (off)").
- **R9-6 Recording.**
  - A sample is recorded only when `twap_window_ms > 0`, so legacy state keeps an empty ring.
  - `SetCollateral` keeps the ring and the pause level.
- **R9-7 `pause::allowed(op, level, feed) -> Result<Gate, RiskError>`.**
  - Plan 10 adds the `grace` parameter and 6072.
  - The feed's own codes (`EngineError`) and `Paused` (6020) are applied by `engine_core::gate`, because `risk_core` cannot name `EngineError`.
- **R9-8 "The 17 operations".** These are the §3.2.2 rows that exist in Plan 9, `Init` excluded (a config being created has no level):
  - Liquidate, Bid, StartSurplus, StartDebt, TakeSurplus, TakeDebt, SettleEmpty, ClaimLeftover, SettleDebt, CheckSolvency, CloseSurplus, CloseDebt, ObserveFeed, SetCollateral, SetEngineParams, SetSlack, SetPauseLevel.
- **R9-9 `SetPauseLevel` routing.**
  - A config with no recorded type, or one naming another `collateral_def`, refuses with `WrongAccount("collateral_def")`.
  - The level is checked `<= 3` at the config.
- **R9-10 Cycle budget.**
  - §4 says `SetPauseLevel` is "~330,000" and "< 3 % of cap" (300,000). Those contradict. The assertion is `<= 400,000` (4 % of the cap). Its executions are hand-counted at 12 and documented, not asserted, as `cycles.rs` does for every row: `Chain::cycles` returns cycles only.
  - The budget rows (partial bid <= 520,000, final bid <= 720,000, `Liquidate` <= 1,000,000) are measured under `RiskArgs::recommended()` in `World::new_tier2` with a full ring, the worst case. The R9-13 e2e values are not used here: R9-3 refuses them at the harness's `TIER2_MAX_AGE_MS` (600_000). The legacy rows are re-measured too.
- **R9-11 Generator order.** The refusal-generator change moves from the spec's second step to Task 13, after every new code has a proving test. Its `--check` would fail otherwise.
- **R9-12 Test files.** The level tests stay in `harness/tests/pause_levels.rs` (spec). The D4 tests go in `harness/tests/stale_bids.rs`, and the two matrix tests in `harness/tests/pause_matrix.rs` (800-line limit).
- **R9-13 e2e `tier2` stage values** (left to the builder):
  - `RiskArgs`: window 300_000, spacing 10_000, `min_obs` 3, `dev` 2_000, `stale_bid_max_age_ms` 600_000.
  - Collateral: `max_oracle_age_ms` 180_000, `max_conf_bps` 100, `band_hi_bps` 15_000.
  - R9-3 holds: 300_000 > 180_000 + (3 − 2) · 10_000 = 190_000, and 600_000 > 180_000.
  - Deviation from spec §5: the spec's 60 s window is below the 180 s max age, so D4 could never fire. The window is 300 s instead. It also keeps every live sample in the window across the e2e's 15 s blocks.
  - Grace is Plan 10.
- **R9-14 Harness Tier 2 config.**
  - `RiskArgs::recommended()` with, in the harness only, `tier2_collateral_args() = CollateralArgs { max_oracle_age_ms: 600_000, band_hi_bps: 15_000, ..CollateralArgs::defaults_for_tests() }`. The frozen `args.rs` is not edited.
  - `max_oracle_age_ms: 600_000` is needed by R9-3 (recommended: 1_800_000 > 600_000 + 300_000, and 3_600_000 > 600_000). `band_hi_bps: 15_000` lets the anchor and the floor bind inside the band.
  - The harness constant is `TIER2_MAX_AGE_MS = 600_000`.
- **R9-15 Events** are deferred (spec §7: best-effort). SDK state views are deferred too. The e2e example decodes `CollateralState` with `engine_core`.

---

### Task 1: Frozen-guest check script

**Files:**
- Create: `scripts/check-frozen-v024.sh`
- Create: `scripts/tests/test_check_frozen_v024.py`
- Modify: `.github/workflows/guests.yml`:
  - the two `paths:` lists: add `"scripts/check-frozen-v024.sh"`;
  - the checkout step: add `fetch-depth: 0`;
  - a new step after checkout.

**Interfaces:**
- Produces: `scripts/check-frozen-v024.sh [BASE]`. `BASE` defaults to `23a57bb`, and env `FROZEN_ROOT` defaults to the repo.
  - It prints nothing and exits 0 when `git -C "$FROZEN_ROOT" diff --stat "$BASE" -- programs/ crates/auction_core/ crates/auction_engine_params/ Cargo.toml Cargo.lock` is empty.
  - Otherwise it prints that stat to stderr, under `check-frozen-v024: frozen v0.2.4 paths changed since $BASE`, and exits 1.

- [ ] **Step 1: Write the failing test.**
  - The test builds a temp git repo without committing: `git init`, write `programs/a.rs` and `v04/b.rs`, `git add -A`, then `tree = git write-tree`.
  - It runs the script with `FROZEN_ROOT=<tmp>` and argument `tree`.

```python
def test_clean_tree_passes(self):        # exit 0, stdout and stderr empty
def test_frozen_file_change_fails(self): # edit programs/a.rs -> exit 1, "programs/a.rs" in stderr
def test_v04_change_passes(self):        # edit v04/b.rs -> exit 0
def test_root_manifest_change_fails(self):  # write Cargo.toml after the tree, `git add Cargo.toml` -> exit 1
```

  `git diff <tree>` does not see untracked files, so `test_root_manifest_change_fails` must stage the new `Cargo.toml` before running the script.

- [ ] **Step 2: Run it.** `python3 -m unittest scripts/tests/test_check_frozen_v024.py -v` fails: the script is missing.
- [ ] **Step 3: Write the script** (`set -euo pipefail`; the executable bit is set when it is staged in Step 5). Add the guests.yml step `name: frozen v0.2.4 paths unchanged since the Tier 2 base (ADR-069)`, `run: scripts/check-frozen-v024.sh`. `fetch-depth: 0` goes in the checkout's existing `with:` block, next to `persist-credentials: false`.
- [ ] **Step 4: Run it again.** The test passes, `scripts/check-frozen-v024.sh` exits 0 on the real tree, and `scripts/check-guests.sh --sha-only` exits 0. The gate runs without `-p risk_core` (Global Constraints).
- [ ] **Step 5: Commit.** `git add --chmod=+x scripts/check-frozen-v024.sh`, then `git add scripts/tests/test_check_frozen_v024.py .github/workflows/guests.yml`, then `git commit -S -m "ci: frozen v0.2.4 path check (ADR-069)"`.

### Task 2: `risk_core` scaffold, `RiskError` and the `Refusal` wrapper

**Files:**
- Create: `v04/risk_core/Cargo.toml`, `v04/risk_core/src/lib.rs`, `v04/risk_core/src/error.rs`
- Modify: `v04/Cargo.toml`:
  - `members`: add `"risk_core"`;
  - `[workspace.dependencies]`: add `risk_core = { path = "risk_core" }`;
  - `[workspace.lints.rust]`: add `unexpected_cfgs = { level = "warn", check-cfg = ['cfg(kani)'] }`.
- Modify: `v04/Cargo.lock` (regenerated by cargo)
- Modify: `v04/engine_core/Cargo.toml`:
  - `[dependencies]`: `risk_core = { path = "../risk_core", features = ["borsh"] }`;
  - `[dev-dependencies]`: `risk_core = { path = "../risk_core", features = ["borsh", "presets"] }`.
- Modify: `v04/engine_core/src/lib.rs` (`pub use risk_core;`)
- Modify: `v04/engine_core/src/check.rs:51-64` (`fail_text`, `fail`)
- Modify: `v04/programs/methods/guest/Cargo.toml`: add `risk_core = { path = "../../../risk_core", features = ["borsh"] }`.
- Modify: `v04/programs/methods/guest/Cargo.lock`: regenerate it with `cargo metadata --format-version 1 --manifest-path programs/methods/guest/Cargo.toml > /dev/null`, run inside WSL from `v04/`.
- Modify: `.github/workflows/v04.yml`: the `cargo fmt` line gains `-p risk_core`.

**Interfaces:**
- Produces:
  - `risk_core` features: `borsh = ["dep:borsh"]`, `presets = []`. Dependencies: `auction_core = { path = "../../crates/auction_core" }`, `borsh = { version = "1.5.7", features = ["derive"], optional = true }`. Dev-dependency: `proptest = "1"`.
  - `lib.rs`: `#![forbid(unsafe_code)]`, `pub mod error; pub use error::RiskError;`, plus `#[cfg(kani)] mod kani_proofs;` once Task 3 adds the file.
  - `pub enum RiskError { TwapUnavailable, SpotTwapDeviation, LiquidationsPaused, SystemAuctionsPaused, BidsPaused, LastGoodTwapTooOld }`, with `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`.
  - `pub const fn code(&self) -> u32`, returning 70, 71, 73, 74, 75, 77. The match lists one arm per line in the form `Self::X => 70,` (Task 13 parses it).
  - `impl core::fmt::Display` writing `{self:?}`.
  - `engine_core::check::Refusal { Engine(EngineError), Risk(RiskError) }`, with `From<EngineError>`, `From<RiskError>`, `code() -> u32` and `Display` (the inner name).
  - `pub fn fail_text(err: impl Into<Refusal>) -> String` and `pub fn fail(err: impl Into<Refusal>) -> !`. The text format is unchanged.

- [ ] **Step 1: Write the failing tests.** In `risk_core/src/error.rs`:

```rust
#[test]
fn risk_error_codes_are_70_and_up() {
    use RiskError::*;
    let codes: Vec<u32> = [TwapUnavailable, SpotTwapDeviation, LiquidationsPaused,
        SystemAuctionsPaused, BidsPaused, LastGoodTwapTooOld].iter().map(RiskError::code).collect();
    assert_eq!(codes, [70, 71, 73, 74, 75, 77]);
}
```

  In `engine_core/src/check.rs` tests (`fail_text_matches_v024_format` stays as it is):

```rust
#[test]
fn fail_text_formats_risk_errors() {
    assert_eq!(fail_text(RiskError::TwapUnavailable),
        "Program error [6070]: Program error 70: TwapUnavailable");
    assert_eq!(fail_text(RiskError::LastGoodTwapTooOld),
        "Program error [6077]: Program error 77: LastGoodTwapTooOld");
}
#[test]
#[should_panic(expected = "Program error [6075]: Program error 75: BidsPaused")]
fn fail_panics_with_a_risk_error() { fail(RiskError::BidsPaused) }
```

- [ ] **Step 2: Run them.** `cargo test -p risk_core` and `cargo test -p engine_core fail_` both fail to compile.
- [ ] **Step 3: Implement.** Every existing `fail(EngineError::..)` and `.unwrap_or_else(|err| fail(err))` call site compiles unchanged. `expect_from`/`expect_root` keep taking `EngineError`.
- [ ] **Step 4: Run them again,** then the full gate. Building the guest proves the guest lock picked up `risk_core`.
- [ ] **Step 5: Commit.** `git add v04/risk_core v04/Cargo.toml v04/Cargo.lock v04/engine_core v04/programs/methods/guest/Cargo.toml v04/programs/methods/guest/Cargo.lock .github/workflows/v04.yml`, then `git commit -S -m "feat(v04): risk_core crate, RiskError 6070+ and the Refusal wrapper (ADR-076)"`.

### Task 3: `validate_feed_ext` and the legacy differential

**Files:**
- Create: `v04/risk_core/src/feed.rs`, `v04/risk_core/src/kani_proofs.rs`
- Modify: `v04/risk_core/src/lib.rs`
- Modify: `v04/engine_core/src/feed.rs:30-50` (`observe`)

**Interfaces:**
- Consumes: `auction_core::price::{OracleReading, FeedParams, OracleScale, to_ray}`, `auction_core::error::PauseReason`, `auction_core::math::mul_div_floor`.
- Produces:
  - `pub const LEGACY_SANITY_LO_BPS: u32 = 5_000;` and `pub const LEGACY_SANITY_HI_BPS: u32 = 20_000;`
  - `pub struct FeedPolicy { pub scale: OracleScale, pub max_age_ms: u64, pub max_conf_bps: u16, pub sanity_lo_bps: u32, pub sanity_hi_bps: u32, pub feed_min_raw: u128, pub feed_max_raw: u128 }`
  - `impl FeedPolicy { pub fn legacy(p: &FeedParams) -> Self }`, which sets sanity 5_000/20_000 and pins `0` / `u128::MAX`.
  - `pub enum FeedFault { Stale, ZeroPrice, LowConfidence, OutOfRange }`, with `pub fn pause_reason(self) -> PauseReason` mapping one-to-one. `Pinned` arrives in Plan 10.
  - `pub fn validate_feed_ext(r: &OracleReading, policy: &FeedPolicy, redemption_ray: u128, age_now_ms: u64) -> Result<u128, FeedFault>`.
    - It checks in `validate_feed`'s order: age, scale, zero, confidence, range.
    - The sanity bounds are `lo = mul_div_floor(R, sanity_lo_bps, 10_000)` (0 on error) and `hi = mul_div_floor(R, sanity_hi_bps, 10_000)`, saturating to `u128::MAX` on error, exactly as v0.2.4's `R.saturating_mul(2)` does.
    - The pin fields are carried but not checked in Plan 9.
  - `pub fn engine_core::feed::feed_policy(coll: &CollateralState) -> Result<FeedPolicy, EngineError>` is `FeedPolicy::legacy(&coll.feed_params()?)`. It is a free fn in `engine_core/src/feed.rs`, not a method, because `state.rs` is at its size limit.
  - `feed::observe` now calls `validate_feed_ext(..).map_err(FeedFault::pause_reason)`. `Observed` is unchanged in this task.

- [ ] **Step 1: Write the failing tests.**

```rust
proptest! {
    #[test]
    fn validate_feed_ext_matches_v024_under_legacy_policy(
        price_raw in any::<u128>(), confidence_raw in any::<u128>(), ts in any::<u64>(),
        q64 in any::<bool>(), max_age_ms in 0..=MAX_ORACLE_AGE_MS, max_conf_bps in 0u16..=1_000,
        r in any::<u128>(), age_now in any::<u64>(),
    ) {
        let p = FeedParams { scale: if q64 { OracleScale::Q64x64 } else { OracleScale::Ray },
            max_age_ms, max_conf_bps, band_lo_bps: 9_000, band_hi_bps: 11_000 };
        let reading = OracleReading { price_raw, confidence_raw, timestamp_ms: ts };
        prop_assert_eq!(
            validate_feed_ext(&reading, &FeedPolicy::legacy(&p), r, age_now).map_err(FeedFault::pause_reason),
            validate_feed(&reading, &p, r, age_now));
    }
}
#[test]
fn validate_feed_ext_saturates_the_high_bound_like_v024() {
    // R above u128::MAX / 2: v0.2.4's hi is u128::MAX, so a huge M is in range.
    // Compare both functions at R = u128::MAX / 2 + 1 and M = u128::MAX; both must equal Ok(u128::MAX).
}
```

  Kani, in `kani_proofs.rs`: `validate_feed_ext_equals_validate_feed_legacy`. It uses `kani::any()` inputs with `price_raw`, `confidence_raw` and `r` bounded below `1 << 96` (`kani::assume`), and asserts the same equality.

- [ ] **Step 2: Run them.** `cargo test -p risk_core validate_feed_ext` fails to compile.
- [ ] **Step 3: Implement.** Then switch `engine_core::feed::observe`.
- [ ] **Step 4: Run them again.** Also run `cargo test -p engine_core feed`: `feed_age_counts_the_slack`, `live_price_clamps_to_the_band` and `out_of_band_and_unknown_scale_fail_as_in_v024` must pass unchanged. Run `PROPTEST_CASES=2000 cargo test -p risk_core validate_feed_ext` once. If Kani is available, run `cargo kani -p risk_core --harness validate_feed_ext_equals_validate_feed_legacy`. Then the full gate.
- [ ] **Step 5: Commit.** `git add v04/risk_core v04/engine_core/src/feed.rs`, then `git commit -S -m "feat(v04): validate_feed_ext with a parameterised sanity window, differential to v0.2.4"`.

### Task 4: The TWAP ring and `twap` (ADR-070)

**Files:**
- Create: `v04/risk_core/src/twap.rs`
- Modify: `v04/risk_core/src/lib.rs`, `v04/risk_core/src/kani_proofs.rs`

**Interfaces:**
- Produces:
  - `pub const TWAP_CAPACITY: usize = 8;`
  - `pub struct Sample { pub ts_ms: u64, pub price_ray: u128 }`
  - `pub struct TwapRing { pub samples: Vec<Sample>, pub last_conf_bps: u16 }`. It is ordered oldest first, with `len <= 8`, and derives `Default`.
  - Both types derive `Debug, Clone, PartialEq, Eq`, and borsh behind `#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize, borsh::BorshDeserialize))]`.
  - `impl TwapRing { pub fn newest(&self) -> Option<Sample> }`
  - `pub fn record(ring: &TwapRing, reading_ts_ms: u64, price_ray: u128, conf_bps: u16, now_ms: u64, spacing_ms: u64) -> TwapRing`. It returns a new ring and follows §3.1.2 exactly:
    - `ts = min(reading_ts, now)`;
    - it skips when `ts <= newest.ts` or `ts - newest.ts < spacing`;
    - otherwise it pushes, drops the oldest past 8, and sets `last_conf_bps`, but only when it pushes.
  - `pub fn twap(ring: &TwapRing, now_ms: u64, window_ms: u64, min_obs: u8, max_age_ms: u64) -> Option<(u128, u64)>` follows §3.1.3 exactly.
    - Every subtraction saturates, and `end` is never raised above its current value.
    - It returns `(T, newest.ts_ms)`.
    - `T` is `Σ mul_div_floor(p_i, w_i, Σw)`, and it returns `None` if a term errors (unreachable, since `w_i <= Σw`).
    - Bounds: `min(p_i).saturating_sub(7) <= T <= max(p_i)` over the used samples. Each of the at most 8 terms floors away less than 1 unit, so `T` can sit up to 7 below the smallest price (prices 1 and 1 with weights 1 and 2 give `T = 0`). It never exceeds the largest.

- [ ] **Step 1: Write the failing tests.** Prices are plain `u128` units; RAY is not needed for the arithmetic. Each call below is `twap(&ring, now, window, min_obs, max_age)`.

```rust
fn ring(s: &[(u64, u128)]) -> TwapRing { /* samples from (ts, price), last_conf_bps 0 */ }
#[test] fn twap_single_sample_is_its_price() {
    assert_eq!(twap(&ring(&[(1_000, 500)]), 2_000, 60_000, 1, 600_000), Some((500, 1_000))); }
#[test] fn twap_weights_by_time_current() {
    assert_eq!(twap(&ring(&[(0, 100), (10_000, 200)]), 40_000, 60_000, 2, 600_000), Some((175, 10_000))); }
#[test] fn twap_ignores_samples_before_the_window() {
    assert_eq!(twap(&ring(&[(0, 100), (10_000, 100), (50_000, 300)]), 70_000, 20_000, 1, 600_000),
        Some((300, 50_000))); }
#[test] fn twap_partly_weights_the_sample_crossing_the_window() {
    assert_eq!(twap(&ring(&[(0, 100), (30_000, 400)]), 50_000, 40_000, 2, 600_000), Some((250, 30_000))); }
#[test] fn twap_gap_longer_than_max_age_drops_older_samples() {
    let r = ring(&[(0, 100), (30_000, 300), (35_000, 300)]);
    assert_eq!(twap(&r, 40_000, 60_000, 2, 10_000), Some((300, 35_000)));
    assert_eq!(twap(&r, 40_000, 60_000, 3, 10_000), None); }
#[test] fn twap_below_min_obs_is_unavailable() {
    assert_eq!(twap(&ring(&[(1_000, 5)]), 2_000, 60_000, 2, 600_000), None);
    assert_eq!(twap(&TwapRing::default(), 2_000, 60_000, 1, 600_000), None); }
#[test] fn twap_is_constant_during_an_outage() {
    let r = ring(&[(0, 100), (10_000, 200)]);
    let at = |now| twap(&r, now, 60_000, 2, 20_000);
    assert_eq!(at(30_000), at(100_000)); assert_eq!(at(30_000), at(1_000_000)); assert!(at(30_000).is_some()); }
#[test] fn twap_with_now_before_the_newest_sample_does_not_underflow() {
    assert_eq!(twap(&ring(&[(0, 100), (10_000, 200)]), 5_000, 60_000, 1, 600_000), Some((100, 10_000))); }
#[test] fn record_skips_same_publish_and_close_spacing() {
    let a = record(&TwapRing::default(), 1_000, 7, 3, 1_000, 10_000);
    assert_eq!(a.samples.len(), 1); assert_eq!(a.last_conf_bps, 3);
    assert_eq!(record(&a, 1_000, 9, 5, 2_000, 10_000), a);     // same publish
    assert_eq!(record(&a, 5_000, 9, 5, 6_000, 10_000), a);     // closer than spacing
    let b = record(&a, 11_000, 9, 5, 12_000, 10_000);
    assert_eq!((b.samples.len(), b.last_conf_bps), (2, 5)); }
#[test] fn record_clamps_a_future_timestamp_to_now() {
    assert_eq!(record(&TwapRing::default(), 9_000, 7, 0, 5_000, 1_000).newest().map(|s| s.ts_ms), Some(5_000)); }
#[test] fn ring_drops_the_oldest_past_capacity() {
    // 9 records at ts 1_000, 2_000, ..., 9_000 with spacing 1_000 -> len 8, oldest ts 2_000.
}
proptest! { #[test] fn twap_within_sample_range(/* 1..=8 samples, strictly increasing ts,
    prices 1..=u128::MAX >> 4, now, window 1..=86_400_000, max_age 1..=86_400_000 */) {
    // if let Some((t, _)) = twap(..): min(prices).saturating_sub(7) <= t <= max(prices)
    // (floor rounding: up to 8 terms, each loses < 1 unit)
} }
#[test] fn twap_floor_rounding_can_fall_below_the_smallest_price() {
    // prices 1 and 1, weights 1 and 2 (ts 0 and 1, now 3, window 3): T = floor(1/3) + floor(2/3) = 0.
    assert_eq!(twap(&ring(&[(0, 1), (1, 1)]), 3, 3, 2, 600_000), Some((0, 1))); }
```

  Kani: `twap_bounded_by_samples` (`#[kani::unwind(9)]`). It builds a ring of `kani::any()` length `<= 3` with prices below `1 << 64`, and asserts `min.saturating_sub(7) <= T <= max` when `Some`.

- [ ] **Step 2: Run them.** `cargo test -p risk_core -- twap record ring` fails to compile.
- [ ] **Step 3: Implement** `record` and `twap`.
- [ ] **Step 4: Run them again.** Run `cargo test -p risk_core` (and `cargo kani -p risk_core --harness twap_bounded_by_samples` if available), then the full gate.
- [ ] **Step 5: Commit.** `git add v04/risk_core`, then `git commit -S -m "feat(v04): 8-slot TWAP ring with gap break and outage anchor (ADR-070)"`.

### Task 5: Prices per operation and the deviation test (ADR-071, ADR-072)

**Files:**
- Create: `v04/risk_core/src/price.rs`
- Modify: `v04/risk_core/src/lib.rs`

**Interfaces:**
- Consumes: `auction_core::price::{FeedParams, bid_price}`. Here `band(x) = bid_price(x, r, feed)`.
- Produces (every price is RAY collateral per coin):
  - `pub fn conf_bps(conf_ray: u128, m_ray: u128) -> u16`: `ceil(c * 10_000 / M)`, saturating at 10_000, and 10_000 when `M == 0`.
  - `pub fn edge(x: u128, conf_bps: u16) -> u128`: `floor(x * (10_000 - conf_bps) / 10_000)`, with `conf_bps` saturated at 10_000.
  - `pub fn liq_price(m: u128, t: Option<u128>, conf_bps: u16, r: u128, feed: &FeedParams) -> u128`: `band(edge(min(M, T)))`, or `band(edge(M))` when `t` is `None`.
  - `pub fn live_bid_price(m: u128, t: Option<u128>, conf_bps: u16, dev_bps: u16, r: u128, feed: &FeedParams) -> u128`: `band(min(edge(M), floor(T * (10_000 + dev) / 10_000)))`. The anchor saturates to `u128::MAX` on overflow. It is `band(edge(M))` when `t` is `None` or `dev_bps == 0` *(Plan 9 ruling R9-5)*.
  - `pub fn fallback_bid_price(t_lg: u128, last_conf_bps: u16, r: u128, feed: &FeedParams) -> u128`: `band(edge(T_lg))`.
  - `pub fn within_deviation(m: u128, t: u128, dev_bps: u16) -> bool`: `floor(|M − T| * 10_000 / T) <= dev`. It is `false` when `t == 0` or the math errors.

- [ ] **Step 1: Write the failing tests.** The wide band is `FeedParams { band_lo_bps: 5_000, band_hi_bps: 20_000, .. }`, and `r = RAY`.

```rust
#[test] fn edge_lowers_p_by_the_confidence() {
    assert_eq!(edge(RAY, 100), RAY / 10_000 * 9_900); assert_eq!(edge(RAY, 0), RAY);
    assert_eq!(conf_bps(RAY / 100, RAY), 100); assert_eq!(conf_bps(1, RAY), 1); assert_eq!(conf_bps(0, RAY), 0); }
#[test] fn liq_price_is_the_smaller_of_spot_and_twap_at_the_edge() {
    let f = wide();
    assert_eq!(liq_price(RAY, Some(RAY / 10 * 9), 100, RAY, &f), edge(RAY / 10 * 9, 100));
    assert_eq!(liq_price(RAY, Some(RAY * 2), 100, RAY, &f), edge(RAY, 100));
    assert_eq!(liq_price(RAY, None, 100, RAY, &f), edge(RAY, 100)); }
#[test] fn bid_price_is_anchored_from_above_only() {
    let f = wide();
    assert_eq!(live_bid_price(RAY / 10 * 15, Some(RAY), 0, 2_000, RAY, &f), RAY / 10 * 12); // capped
    assert_eq!(live_bid_price(RAY / 10 * 8, Some(RAY), 0, 2_000, RAY, &f), RAY / 10 * 8);   // not raised
    assert_eq!(live_bid_price(RAY / 10 * 15, Some(RAY), 0, 0, RAY, &f), RAY / 10 * 15);     // dev 0: off
    assert_eq!(live_bid_price(RAY / 10 * 15, None, 0, 2_000, RAY, &f), RAY / 10 * 15); }    // no TWAP
#[test] fn deviation_is_floor_bps_inclusive() {
    assert!(within_deviation(1_200, 1_000, 2_000)); assert!(!within_deviation(1_201, 1_000, 2_000));
    assert!(within_deviation(800, 1_000, 2_000)); assert!(!within_deviation(799, 1_000, 2_000));
    assert!(!within_deviation(1, 0, 2_000)); }
#[test] fn prices_stay_inside_the_band() {
    // RAY-band 9_000/11_000 at r = RAY: liq_price(RAY * 2, None, 0, ..) == RAY / 10 * 11, and
    // fallback_bid_price(RAY / 2, 0, ..) == RAY / 10 * 9.
}
```

- [ ] **Step 2: Run them.** `cargo test -p risk_core price` fails to compile.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them again,** then the full gate.
- [ ] **Step 5: Commit.** `git add v04/risk_core`, then `git commit -S -m "feat(v04): confidence-edge prices and the one-sided TWAP anchor (ADR-071, ADR-072)"`.

### Task 6: `RiskArgs`, presets, state extensions and message fields (ADR-082)

**Files:**
- Create: `v04/risk_core/src/args.rs`, `v04/risk_core/src/presets.rs`
- Create: `v04/engine_core/src/ext.rs`
- Create: `v04/harness/src/tier2.rs`
- Create: `v04/harness/tests/risk_args.rs`
- Modify: `v04/risk_core/src/lib.rs`
- Modify: `v04/engine_core/src/lib.rs`
- Modify: `v04/engine_core/src/state.rs`:
  - `ConfigState` (`:137-178`): append `pub ext: ConfigExt`;
  - `CollateralState` (`:183-214`): append `pub ext: CollateralExt`;
  - `AuctionState` (`:396-433`): append `pub stale_cap_ray: u128`;
  - the fixtures `config()`, `coll()` and `auction()` (`:528-625`).
- Modify: `v04/engine_core/src/liquidation.rs:35-64` (`awaiting_seize` sets `stale_cap_ray: 0`)
- Modify: `v04/engine_core/src/msg.rs`:
  - `SetCollateral` (`:40-45`): append `risk: Box<RiskArgs>`;
  - `Configure` (`:159-163`): append `risk: Box<RiskArgs>` *(Plan 9 ruling R9-4)*;
  - `BidCtx` (`:143-154`): append `paused_since_ms: Option<u64>, fallback: bool`.
- Modify: `v04/engine_core/src/msg_tests.rs`
- Modify: `v04/programs/methods/guest/src/bin/engine.rs` (the `SetCollateral`, `Configure` and `BidCtx` arms)
- Modify: `v04/programs/methods/guest/src/bin/engine/admin.rs`:
  - `init_config` `:190-213`: `ext: ConfigExt::default()`;
  - `on_set_collateral` `:432-482`;
  - `on_configure` `:488-533`;
  - `fresh_collateral` `:535-565`.
- Modify: `v04/programs/methods/guest/src/bin/engine/bid.rs:62-91` (`bid_ctx` sends `paused_since_ms: None, fallback: false`)
- Modify: `v04/harness/Cargo.toml`: `risk_core = { workspace = true, features = ["borsh", "presets"] }`.
- Modify: `v04/harness/src/lib.rs` (`pub mod tier2;`)
- Modify: `v04/harness/src/world.rs`:
  - `set_collateral_msg` `:346-353` passes `RiskArgs::legacy()`;
  - `new_with` `:250-261` becomes `pub(crate) fn new_with(r: u128, edit: impl FnOnce(&mut Params), args: CollateralArgs, risk: RiskArgs) -> Self`, and its `SetCollateral` uses `set_collateral_msg_with(args, risk)`. `new_with_host` and `new_at` pass `Self::collateral_args(), RiskArgs::legacy()`. `new_tier2` calls it too, so no constructor body is copied.
- Modify: `v04/sdk/Cargo.toml`: `risk_core = { workspace = true, features = ["borsh", "presets"] }`.
- Modify: `v04/sdk/src/lib.rs`
- Modify: `v04/sdk/src/builder.rs:298-320`
- Modify: every existing full literal of `ConfigState`, `CollateralState`, `AuctionState`, `EngineMsg::SetCollateral`, `EngineMsg::Configure` and `EngineMsg::BidCtx`. Find them with `grep -rn --include=*.rs -E "EngineMsg::(SetCollateral|Configure|BidCtx) \{|(Config|Collateral|Auction)State \{" v04 | grep -v target`, and update only the ones that are not `..base` updates. The `Configure` literals are `engine_core/src/msg_tests.rs:140`, `harness/tests/security.rs:186` and the guest's `admin.rs:457`.

**Interfaces:**
- Consumes: `TwapRing` (Task 4).
- Produces:
  - `risk_core::args`:
    - `pub struct RiskArgs { pub twap_window_ms: u64, pub twap_min_spacing_ms: u64, pub twap_min_obs: u8, pub max_spot_twap_dev_bps: u16, pub stale_bid_max_age_ms: u64 }` (borsh behind the feature; `Debug, Clone, Copy, PartialEq, Eq`).
    - The bounds as `pub const`s: `TWAP_WINDOW_MIN_MS = 60_000`, `TWAP_WINDOW_MAX_MS = 86_400_000`, `TWAP_SPACING_MIN_MS = 1_000`, `TWAP_MIN_OBS_MAX = 8`, `DEV_MIN_BPS = 100`, `DEV_MAX_BPS = 5_000`. `stale_bid_max_age_ms` is bounded by `auction_core::price::MAX_ORACLE_AGE_MS`.
    - `pub struct RiskArgsError { pub field: &'static str }`.
    - `impl RiskArgs { pub fn validate(&self, max_oracle_age_ms: u64) -> Result<(), RiskArgsError> }` checks the §3.1.6 and §3.2.4 bounds plus *(Plan 9 ruling R9-3)*.
  - `risk_core::presets` (feature `presets`):
    - `impl RiskArgs { pub const fn legacy() -> Self; pub const fn recommended() -> Self }`.
    - legacy: all 0. recommended: `1_800_000, 300_000, 3, 2_000, 3_600_000`.
  - `engine_core::ext`:
    - `pub struct ConfigExt { pub system_level: u8 }` (`Default`).
    - `pub struct CollateralExt { pub risk: RiskArgs, pub pause_level: u8, pub ring: TwapRing }`.
    - `impl CollateralExt { pub fn new(risk: RiskArgs) -> Self }` (level 0, empty ring).
    - `impl CollateralState { pub fn effective_level(&self) -> u8; pub fn for_quote(&self) -> Self }`. `effective_level` is `max(pause_level, if admin_paused {3} else {0})`. `for_quote` returns the state with `ext.ring = TwapRing::default()`.
    - `pub fn validate_risk(risk: &RiskArgs, max_oracle_age_ms: u64) -> Result<(), EngineError>` (`InvalidConfig`).
  - `sdk`:
    - `pub use risk_core::args::RiskArgs;`
    - `pub fn plan_set_collateral_with(d: &Deployment, collateral_def: AccountId, args: CollateralArgs, risk: RiskArgs, now_ms: u64) -> Result<Plan, SdkError>` declares the same actors as `plan_set_collateral`.
    - `plan_set_collateral` keeps its signature and sends `RiskArgs::legacy()`.
  - `harness::tier2`:
    - `pub const TIER2_MAX_AGE_MS: u64 = 600_000;`
    - `pub fn tier2_collateral_args() -> CollateralArgs` *(Plan 9 ruling R9-14)*.
    - `impl World { pub fn new_tier2(risk: RiskArgs) -> World; pub fn set_collateral_msg_with(&self, args: CollateralArgs, risk: RiskArgs) -> EngineMsg; pub fn collateral_state(&self) -> CollateralState }`.
    - `new_tier2` is `World::new_with(RAY / 2, |_| {}, tier2_collateral_args(), risk)`.
  - Guest behaviour:
    - `on_set_collateral` runs `validate_risk(&risk, args.max_oracle_age_ms)` after `args.validate` and relays `risk` in `Configure`.
    - `on_configure` writes `ext.risk = *risk`, keeping `ext.pause_level` and `ext.ring` from `base` *(Plan 9 ruling R9-6)*.
    - `fresh_collateral` uses `CollateralExt::new(RiskArgs { all zero })`. Write the literal out: the guest has no presets.

- [ ] **Step 1: Write the failing tests.**

```rust
// risk_core/src/args.rs: the tests that call the presets are #[cfg(feature = "presets")]
#[cfg(feature = "presets")]
#[test] fn legacy_and_recommended_presets_validate() {
    assert_eq!(RiskArgs::legacy().validate(3_600_000), Ok(()));
    assert_eq!(RiskArgs::recommended().validate(600_000), Ok(())); }
#[test] fn risk_args_bounds_are_inclusive() {
    // Literals, no presets. window 59_999 / 86_400_001 rejected, 60_000 / 86_400_000 accepted (spacing 1_000,
    // min_obs 1, stale 0 so the R9-3 window bound is off, max age 86_400_000); spacing 999 rejected; min_obs 0 and 9 rejected; dev 99 / 5_001 rejected,
    // 100 / 5_000 accepted; stale 86_400_001 rejected.
}
#[cfg(feature = "presets")]
#[test] fn risk_args_reject_unreachable_twap_and_dead_d4() {   // Review Focus 3
    let r = RiskArgs::recommended();
    assert_eq!(r.validate(299_999).map_err(|e| e.field), Err("twap_min_spacing_ms"));   // spacing > max age
    assert_eq!(RiskArgs { twap_min_obs: 7, ..r }.validate(600_000).map_err(|e| e.field), Err("twap_min_obs"));
    assert_eq!(RiskArgs { stale_bid_max_age_ms: 600_000, ..r }.validate(600_000).map_err(|e| e.field),
        Err("stale_bid_max_age_ms"));
    // R9-3 window bound: 900_000 is not > 600_000 + (3 - 2) * 300_000; 900_001 is; D4 off lifts it.
    assert_eq!(RiskArgs { twap_window_ms: 900_000, ..r }.validate(600_000).map_err(|e| e.field),
        Err("twap_window_ms"));
    assert_eq!(RiskArgs { twap_window_ms: 900_001, ..r }.validate(600_000), Ok(()));
    assert_eq!(RiskArgs { twap_window_ms: 900_000, stale_bid_max_age_ms: 0, ..r }.validate(600_000), Ok(()));
    assert_eq!(RiskArgs { max_spot_twap_dev_bps: 2_000, ..RiskArgs::legacy() }.validate(600_000)
        .map_err(|e| e.field), Err("max_spot_twap_dev_bps")); }
// engine_core/src/ext.rs
#[test] fn for_quote_strips_the_ring() { /* coll() with 3 samples -> for_quote().ext.ring == default, rest equal */ }
#[test] fn effective_level_is_3_under_admin_pause() { /* pause_level 1 + admin_paused -> 3; 2 alone -> 2 */ }
// engine_core/src/msg_tests.rs: add to the existing round-trip tests; indices unchanged
#[test] fn set_collateral_and_bid_ctx_carry_the_appended_fields() { /* encode -> decode equal, first byte 1 / 13 */ }
// harness/tests/risk_args.rs
#[test] fn set_collateral_rejects_out_of_bound_risk_args() {
    // World::new(); send set_collateral_msg_with(defaults_for_tests(), RiskArgs { twap_window_ms: 1, ..legacy })
    // -> expect_revert(.., &fail_text(EngineError::InvalidConfig)); collateral state unchanged.
}
#[test] fn set_collateral_writes_the_risk_args() { /* World::new_tier2(recommended) -> collateral_state().ext.risk == recommended, pause_level 0, ring empty */ }
```

- [ ] **Step 2: Run them.** Run `cargo test -p risk_core --features presets args`, `cargo test -p engine_core -- ext msg` and `cargo test -p harness --test risk_args`. They fail to compile.
- [ ] **Step 3: Implement.** Add the fields, the messages and the guest changes, and update the full literals.
- [ ] **Step 4: Run them again,** then the full gate. Every existing test must pass with its assertions unchanged.
- [ ] **Step 5: Commit.** `git add v04/risk_core v04/engine_core v04/programs/methods/guest/src v04/harness v04/sdk v04/Cargo.lock`, then `git commit -S -m "feat(v04): RiskArgs with legacy/recommended presets, state ext and appended message fields (ADR-082)"`. In the commit body: "Migration: local 0.4 state must be recreated; tags unchanged."

### Task 7: E1 in the engine: ring recording, `P_liq`, the deviation breaker and the bid anchor

**Files:**
- Create: `v04/engine_core/src/gate.rs`
- Create: `v04/harness/tests/twap.rs`
- Modify: `v04/engine_core/src/lib.rs`
- Modify: `v04/engine_core/src/feed.rs`: `Observed` gains `pub conf_bps: u16`, and a new `record`.
- Modify: `v04/programs/methods/guest/src/bin/engine/observe.rs:168-220` (`proceed`)
- Modify: `v04/harness/src/tier2.rs`

**Interfaces:**
- Consumes: Tasks 3–6.
- Produces:
  - `feed::Observed { clock, feed, conf_bps }`. `conf_bps = price::conf_bps(to_ray(confidence_raw, scale), m)` when the feed is valid, else 0.
  - `pub fn feed::record(coll: &CollateralState, reading: &OracleReading, obs: &Observed, now_ms: u64) -> CollateralState`.
    - It returns `coll` unchanged unless `obs.feed` is `Ok(m)` and `ext.risk.twap_window_ms > 0`.
    - Otherwise it calls `twap::record(&ring, reading.timestamp_ms, m, obs.conf_bps, now_ms, risk.twap_min_spacing_ms)`.
  - `gate::liquidation_price(coll: &CollateralState, obs: &Observed, r_ray: u128, now_ms: u64) -> Result<u128, Refusal>`. In this task (levels arrive in Task 8):
    1. `admin_paused` → `Paused`.
    2. Feed invalid → its own `EngineError` code.
    3. With `twap_window_ms == 0`: `liq_price(m, None, ..)`.
    4. Otherwise `twap(..)`, else `TwapUnavailable`.
    5. If `dev > 0` and `!within_deviation` → `SpotTwapDeviation`.
    6. `liq_price(m, Some(t), conf_bps, r, feed)`.
  - `pub struct gate::BidPricing { pub price_ray: u128, pub fallback: bool, pub paused_since_ms: Option<u64> }`
  - `gate::bid_pricing(coll: &CollateralState, obs: &Observed, r_ray: u128, now_ms: u64) -> Result<BidPricing, Refusal>`.
    - Live: `live_bid_price(m, t, conf_bps, dev, r, feed)`, where `t = twap(..)` if `window > 0 && dev > 0`, else `None`. Result `fallback: false, paused_since_ms: None`.
    - Admin pause and invalid feed are refused as today (D4 arrives in Task 9).
  - `coll` here is the observed state with this observation's sample already recorded.
  - Guest `proceed`:
    - It builds `observed = record(&coll.with_pause_clock(obs.clock), ..)` with `pending: None`.
    - `Liquidate` uses `gate::liquidation_price(&observed, ..)` and sends `LiqCtx { collateral: Box::new(observed.for_quote()), price_ray, .. }`.
    - `Bid` uses `gate::bid_pricing` and passes `price_ray` to `bid_ctx`.
    - `proceed` is already 54 lines. Move the `Liquidate` and `Bid` continuations into private helpers in `observe.rs` (`liquidate_then(..)`, `bid_then(..)`), so each function stays under 50 lines.
  - `harness::tier2`:
    - `impl World { pub fn set_price_with_confidence(&mut self, price_ray: u128, confidence: u128); pub fn warm_twap(&mut self, price_ray: u128, samples: u8, spacing_ms: u64) }`.
    - `warm_twap` advances `now` by `spacing_ms`, publishes, and sends `observe_feed`, `samples` times.
  - A sample's weight is the time it has been current. A liquidation at the same ms as the newest publish therefore gives that sample no weight. The tests liquidate 1_000 ms after the last publish.

- [ ] **Step 1: Write the failing tests.**
  - Unit tests in `gate.rs`: `liq_gate_order_admin_feed_twap_deviation`, which uses a `coll()` fixture with recommended risk and checks each refusal in the order above, and `legacy_liquidation_price_is_band_edge_m`.
  - Harness `v04/harness/tests/twap.rs`. All tests use `open_unsafe_position(500_000, 600_000)`, and each ends with `invariants::check(&w)`. They use `World::new_tier2(RiskArgs::recommended())`, except `bid_during_twap_warm_up_is_not_refused`, which uses `World::new_tier2(RiskArgs::legacy())`.

```rust
#[test] fn liquidate_refused_until_twap_has_min_obs() {
    // warm 2 samples -> liquidate (now + 1_000) reverts fail_text(RiskError::TwapUnavailable), state unchanged;
    // one more sample -> liquidate succeeds.
}
#[test] fn keeper_reward_uses_the_smaller_of_spot_and_twap() {
    // warm 3 at p; publish 1.1p (within 20 %) 1_000 ms after the newest sample, closer than
    // twap_min_spacing_ms, so the engine does not record it; liquidate 1_000 ms later.
    // Assert the auction's keeper_reward equals auction_core's reward at
    // P = risk_core::price::liq_price(1.1p, Some(T), 0, R, &feed), with T = twap(&ring, now, ..)
    // on collateral_state().ext.ring (unchanged by the liquidation), and that this P is below band(1.1p).
}
#[test] fn a_spot_spike_is_refused_by_the_deviation_breaker() {
    // warm 3 at p; publish 1.3p; liquidate -> fail_text(RiskError::SpotTwapDeviation); nothing changed.
}
#[test] fn bid_price_takes_the_confidence_edge() {
    // warm 3 at p (warm_twap publishes confidence 0); liquidate; set_price_with_confidence(p, p / 200)
    // (50 bps <= max_conf 100); bid 100_000 coin. The bid's edge uses the current reading, so
    // collateral out equals the quote at edge(band-clamped M, 50); strictly less than at confidence 0.
}
#[test] fn bid_payout_capped_by_the_twap_anchor() {
    // warm 3 at p; liquidate; publish 1.4p (band_hi 15_000 keeps it unclamped) and bid.
    // Collateral out equals the quote at floor(T * 12_000 / 10_000), not at 1.4p.
}
#[test] fn invalid_reading_is_not_recorded() {
    // warm 3; publish a price below R / 2 (out of range); observe_feed; ring unchanged.
}
#[test] fn observe_feed_fills_the_ring() {
    // 9 publishes spaced 300_000 with observe_feed each -> ring len 8, strictly increasing ts.
}
#[test] fn bid_during_twap_warm_up_is_not_refused() {          // Review Focus 2
    // World::new_tier2(RiskArgs::legacy()); liquidate; SetCollateral(tier2_collateral_args(), recommended)
    // (its Observe records the valid reading, so the ring holds 1 sample, below min_obs 3);
    // bid succeeds at band(edge(M)).
}
#[test] fn set_collateral_keeps_the_ring_and_the_level() {     // Review Focus 5
    // warm 3; SetCollateral again with recommended -> ring and pause_level unchanged, risk rewritten.
    // (The level part is asserted after Task 8 adds SetPauseLevel: extend this test there.)
}
```

- [ ] **Step 2: Run them.** `cargo test -p engine_core gate` and `cargo test -p harness --test twap` fail.
- [ ] **Step 3: Implement** `feed::record`, `gate.rs` and the guest `proceed` changes.
- [ ] **Step 4: Run them again.** Then run `cargo test -p harness --test liquidate --test bid` (the legacy suite: `a_stale_feed_stops_the_bid` and `stale_feed_pauses_and_reverts_liquidate` unchanged), then the full gate.
- [ ] **Step 5: Commit.** `git add v04/engine_core v04/programs/methods/guest/src v04/harness`, then `git commit -S -m "feat(v04): TWAP-backed liquidation price, deviation breaker and bid anchor (E1)"`.

### Task 8: Pause levels and `SetPauseLevel` (ADR-073)

**Files:**
- Create: `v04/risk_core/src/pause.rs`
- Create: `v04/programs/methods/guest/src/bin/engine/pause.rs`
- Create: `v04/harness/src/levels.rs`
- Create: `v04/harness/tests/pause_levels.rs`
- Create: `v04/sdk/tests/tier2_plans.rs`
- Modify: `v04/engine_core/src/gate.rs`, `v04/engine_core/src/feed.rs` (`observe`: `admin_paused: coll.effective_level() == 3`)
- Modify: `v04/engine_core/src/assertions.rs`: `pub fn pause_level_update(input) -> Result<CollateralState, EngineError>`, following `slack_update` (`:54-56`).
- Modify: `v04/engine_core/src/msg.rs`: append `SetPauseLevel { salt: u64, collateral_def: AccountId, level: u8, now_ms: u64 }` (47) and `PauseLevelUpdate { level: u8 }` (48).
- Modify: `v04/engine_core/src/msg_tests.rs`
- Modify: `v04/programs/methods/guest/src/bin/engine.rs` (module and arms)
- Modify: `v04/programs/methods/guest/src/bin/engine/proxy.rs:47-70`: `SetPauseLevel { salt, .. }` joins the `forward_admin` arm.
- Modify: `v04/programs/methods/guest/src/bin/engine/system.rs:69-77` (`root`: the gate before the relay)
- Modify: `v04/programs/methods/guest/src/bin/engine/surplus.rs:126-140` and `debt.rs:194-208` (`root_at_config`)
- Modify: `v04/harness/src/invariants.rs`, `v04/harness/src/lib.rs`
- Modify: `v04/harness/src/world.rs` (`proxy_actors` `:359-444`): a `SetPauseLevel { salt, collateral_def, .. }` arm with the same actors as `SetCollateral` (proxy, config, collateral, host params, oracle, clock: the level is followed by an `Observe`). Without it `proxy_actors` panics "not a proxied engine message". `world.rs` goes from 713 to about 725 lines.
- Modify: `v04/harness/tests/twap.rs` (extend `set_collateral_keeps_the_ring_and_the_level`)
- Modify: `v04/sdk/src/builder.rs`, `v04/sdk/src/lib.rs`

**Interfaces:**
- Produces:
  - `risk_core::pause`. `Op`, `Level`, `FeedCase` and `Gate` all derive `Debug, Clone, Copy, PartialEq, Eq`:
    - `pub enum Op { Liquidate, Bid, StartSurplus, StartDebt, TakeSurplus, TakeDebt, SettleEmpty, ClaimLeftover, SettleDebt, CheckSolvency, CloseSurplus, CloseDebt, ObserveFeed, SetCollateral, SetEngineParams, SetSlack, SetPauseLevel }` with `pub const ALL: [Op; 17]` *(Plan 9 ruling R9-8)*.
    - `pub enum Level { Normal, NoLiquidations, NoSystemStarts, Halted }` with `pub fn from_u8(u8) -> Option<Level>`, `pub const fn as_u8(self) -> u8` (an exhaustive match; `as` is denied) and `ALL: [Level; 4]`.
    - `pub enum FeedCase { Live, Stale }` with `ALL`.
    - `pub enum Gate { Run, FeedRefusal, Fallback }`.
    - `pub fn allowed(op: Op, level: Level, feed: FeedCase) -> Result<Gate, RiskError>` *(Plan 9 ruling R9-7)*. It is one `match` with no wildcard (the lint `wildcard_enum_match_arm` makes it exhaustive at compile time):
      - Liquidate: `Err(LiquidationsPaused)` at level >= 1; `Run` when live; `FeedRefusal` when stale.
      - Bid: `Err(BidsPaused)` at level 3; `Run` when live; `Fallback` when stale.
      - StartSurplus / StartDebt: `Err(SystemAuctionsPaused)` at level >= 2.
      - TakeSurplus / TakeDebt: `Err(BidsPaused)` at level 3.
      - Everything else: `Run`.
      - The arms are grouped by behaviour (`Op::StartSurplus | Op::StartDebt => ..`, one or-pattern for every always-`Run` op), so the function stays under 50 lines and still names every variant.
  - `engine_core::gate`:
    - `pub fn collateral_gate(coll: &CollateralState, op: Op, feed_valid: bool) -> Result<Gate, Refusal>`. If `admin_paused` and `allowed(op, Halted, feed)` is an `Err` → `EngineError::Paused`. Otherwise `allowed(op, from_u8(ext.pause_level).ok_or(InvalidConfig)?, feed)`.
    - `liquidation_price` and `bid_pricing` start with `collateral_gate`. `FeedRefusal` → the feed's own code. In Task 8, `Fallback` → the feed's own code as well.
    - `pub fn system_gate(cfg: &ConfigState, op: Op) -> Result<(), Refusal>` checks `allowed(op, level_of(cfg.ext.system_level), FeedCase::Live)`.
  - Guest `pause.rs`:
    - `pub fn on_set_pause_level(input: &ReceiveInput, message: &EngineMsg) -> Response` at the config:
      1. `expect_admin_proxy` (move it from `admin.rs:69-75` to `pub(crate)`), then `config.admin == admin`, else `NotAdmin`.
      2. `level <= 3`, else `InvalidConfig`.
      3. `config.collateral_def == Some(collateral_def)`, else `WrongAccount("collateral_def")` *(Plan 9 ruling R9-9)*.
      4. Write `ext.system_level = level`.
      5. Send `to_collateral(.., PauseLevelUpdate { level })` (make `admin::to_collateral` `pub(crate)`), then `Observe { now_ms }` unseeded to the collateral actor.
    - `pub fn on_pause_level_update(input: &ReceiveInput, level: u8) -> Response` at the collateral actor: `assertions::pause_level_update`, `level <= 3` (else `InvalidConfig`), then write `ext.pause_level`. It sends nothing.
  - Guest system gates:
    - `system::root` maps `SystemOp` to `Op` (`SettleDebt`, `StartSurplus`, `StartDebt`, `CheckSolvency`) and calls `gate::system_gate(&c, op)` after `config_root`.
    - Both `root_at_config` functions call it with `Op::TakeSurplus` / `Op::TakeDebt` (when `token_required`) or `Op::CloseSurplus` / `Op::CloseDebt`.
  - `sdk`: `pub fn plan_set_pause_level(d: &Deployment, collateral_def: AccountId, level: u8, now_ms: u64) -> Result<Plan, SdkError>`. Its actors are the same as `plan_set_collateral`'s.
  - `harness::levels`: `impl World { pub fn set_pause_level_msg(&self, level: u8) -> EngineMsg; pub fn set_pause_level(&mut self, level: u8) -> Result<Vec<TransactionEvent>, LeeError>; pub fn snapshot_all(&self) -> Vec<(Actor, Option<Vec<u8>>)> }`.
    - `snapshot_all` covers the config, accounting, collateral, every owner's record, auctions and vault holdings, the buffer and every funded holding the world created.
  - Invariants (`invariants::check`):
    - `collateral.ext.pause_level <= 3`;
    - `config.ext.system_level == collateral.ext.pause_level`;
    - `ring.samples.len() <= 8` with strictly increasing `ts_ms`.

- [ ] **Step 1: Write the failing tests.**
  - In `risk_core/src/pause.rs`:
    - `pause_table_is_exhaustive` loops over `Op::ALL × Level::ALL × FeedCase::ALL` (136 cases). It asserts each result equals a literal expected table in the test, written row by row from spec §3.2.2 (L0–L3 with a live feed, and L0 with a stale feed: Liquidate `FeedRefusal`, Bid `Fallback`).
    - `risk_reducing_ops_allowed_everywhere`: `[SettleEmpty, ClaimLeftover, SettleDebt, CheckSolvency, CloseSurplus, CloseDebt, ObserveFeed]` give `Ok(Gate::Run)` everywhere.
  - In `msg_tests.rs`: `set_pause_level_round_trips_at_index_47` and `pause_level_update_round_trips_at_index_48`.
  - In `gate.rs`: `admin_pause_maps_level_3_refusals_to_paused`.
  - In `harness/tests/pause_levels.rs`, where `w = World::new_at(RAY / 2)` and the scenarios reuse `open_unsafe_position`. Every refusal asserts `snapshot_all()` is unchanged.

```rust
#[test] fn admin_paused_keeps_the_v024_code() {
    // pause_level 1 and admin_paused: liquidate -> fail_text(EngineError::Paused); bid -> Paused.
}
#[test] fn admin_paused_does_not_stop_system_auctions() {
    // with_system(token enabled) + admin_paused SetCollateral: start_surplus, take_surplus succeed.
}
#[test] fn level_2_blocks_starts_not_takes_or_closes() {
    // live surplus auction; level 2: start_debt -> fail_text(RiskError::SystemAuctionsPaused);
    // take_surplus succeeds; after TTL close_surplus succeeds.
}
#[test] fn lowering_the_level_resumes_liquidation() {
    // level 1: liquidate -> RiskError::LiquidationsPaused; level 0: liquidate succeeds.
}
#[test] fn set_pause_level_only_through_the_admin_proxy() {
    // a stranger signing at the admin's proxy -> NotAdmin; a stranger's own proxy -> WrongAccount("config")
    // (as engine_admin.rs and slack.rs assert); root at the config itself -> NotAdmin;
    // wrong collateral_def -> WrongAccount("collateral_def").
}
#[test] fn level_change_records_the_pause_clock() {
    // level 3 at t -> collateral paused_since_ms == Some(t); level 0 at t + 60_000 -> paused_since None,
    // paused_total_ms grew by 60_000; level 1 or 2 leaves paused_since None.
}
#[test] fn set_pause_level_rejects_level_above_3() {          // Review Focus 4
    // levels 4 and 255 -> fail_text(EngineError::InvalidConfig); snapshot_all unchanged.
}
```

  - Extend `set_collateral_keeps_the_ring_and_the_level` (Task 7) with level 2, which must survive the `SetCollateral`.
  - In `sdk/tests/tier2_plans.rs`: `set_pause_level_plan_declares_every_touched_actor`, which follows `plans_declare_every_touched_actor` in `sdk/tests/builder.rs`.
- [ ] **Step 2: Run them.** `cargo test -p risk_core pause`, `cargo test -p engine_core -- gate msg`, `cargo test -p harness --test pause_levels --test twap` and `cargo test -p sdk --test tier2_plans` fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them again.** Then run `cargo test -p harness` and `cargo test -p engine_core feed`. The existing `admin_pause_toggle_runs_observe` and `stale_feed_pauses_and_reverts_liquidate` (`harness/tests/liquidate.rs`) and `feed.rs::admin_pause_starts_the_clock_at_now_and_refuses_liquidation` keep their codes. Then the full gate.
- [ ] **Step 5: Commit.** `git add v04/risk_core v04/engine_core v04/programs/methods/guest/src v04/harness v04/sdk`, then `git commit -S -m "feat(v04): pause levels 1-3 with one rule table and SetPauseLevel (ADR-073)"`.

### Task 9: D4 bids on a stale feed, held at the floor (ADR-074)

**Files:**
- Create: `v04/risk_core/src/stale.rs`
- Create: `v04/harness/tests/stale_bids.rs`
- Modify: `v04/risk_core/src/lib.rs`, `v04/risk_core/src/kani_proofs.rs`
- Modify: `v04/engine_core/src/gate.rs` (`bid_pricing`'s `Gate::Fallback` arm)
- Modify: `v04/engine_core/src/liquidation.rs:76-110` (`quote_seized` sets `stale_cap_ray`)
- Modify: `v04/engine_core/src/bid.rs`:
  - `BidQuote` `:26-37`: gains `pub paused_since_ms: Option<u64>, pub fallback: bool`;
  - `quote` `:53-83`.
- Modify: `v04/programs/methods/guest/src/bin/engine.rs` (the `BidCtx` arm passes both fields)
- Modify: `v04/programs/methods/guest/src/bin/engine/bid.rs:62-91` (`bid_ctx` takes a `BidPricing`)
- Modify: `v04/programs/methods/guest/src/bin/engine/observe.rs` (`proceed`'s Bid helper, `bid_ctx`'s only caller, passes the `BidPricing`)
- Modify: `v04/harness/src/tier2.rs`

**Interfaces:**
- Consumes: `twap` (Task 4), `fallback_bid_price` (Task 5), `BidPricing` and `collateral_gate` (Tasks 7–8), `auction_core::schedule::Schedule::{discount_ray, max_discount_ray}`.
- Produces:
  - `risk_core::stale`:
    - `pub fn stale_cap(p_liq_ray: u128, schedule: &Schedule) -> Result<u128, MathError>`: `floor(P_liq * RAY / (RAY - schedule.max_discount_ray()))`.
    - `pub fn floored_price(p_fb: u128, stale_cap_ray: u128, d_ray: u128) -> u128`: `min(P_fb, floor(stale_cap * (RAY - d) / RAY))`.
    - `pub fn fallback_twap(ring: &TwapRing, now_ms: u64, risk: &RiskArgs, max_age_ms: u64) -> Result<Option<u128>, RiskError>`, following §3.2.3:
      - `Ok(None)` when `stale_bid_max_age_ms == 0 || twap_window_ms == 0` (the caller uses the feed's own code);
      - `Err(TwapUnavailable)` without `min_obs` samples;
      - `Err(LastGoodTwapTooOld)` when `now - newest.ts > stale_bid_max_age_ms`;
      - else `Ok(Some(T_lg))`.
  - `quote_seized` stores `stale_cap_ray = stale_cap(ctx.price_ray, &params.schedule)?`, using the auction's snapshotted schedule (E4 caps it in Plan 11), only when D4 is on: `ctx.collateral.ext.risk.stale_bid_max_age_ms > 0 && ctx.collateral.ext.risk.twap_window_ms > 0` (`for_quote` strips only the ring, so `risk` is there). Otherwise it stores 0, the legacy value, so the full `AuctionState` literals in `harness/tests/liquidate.rs` (`:123`, `:241`) keep passing.
  - `gate::bid_pricing`, `Gate::Fallback` arm:
    - `fallback_twap(..)?`: `None` → the feed's own code; `Some(t)` → `BidPricing { price_ray: fallback_bid_price(t, ring.last_conf_bps, r, feed), fallback: true, paused_since_ms: coll.paused_since_ms }`.
    - `coll` is the observed state, whose pause clock is open.
  - `bid::quote`:
    - `PauseClock.paused_since_ms = q.paused_since_ms`, so live bids (`None`) are unchanged.
    - If `q.fallback`, the price passed to `quote_bid` is `floored_price(q.price_ray, a.stale_cap_ray, schedule.discount_ray(t_eff))`.
  - `harness::tier2`: `impl World { pub fn go_stale(&mut self) }` advances `now` past `newest publish + max_oracle_age_ms - SLACK_MS` without publishing. It reads `max_oracle_age_ms` from `collateral_state()`, not a constant, so it makes any world's feed stale.

- [ ] **Step 1: Write the failing tests.**

```rust
// risk_core/src/stale.rs
#[test] fn floor_uses_the_capped_max_discount() {
    // schedule [(0, 200), (2_700_000, 1_000)]: stale_cap(RAY / 2, &s) == mul_div_floor(RAY / 2, RAY, RAY - bps_to_ray(1_000));
    // a schedule ending at 3_000 gives the larger cap mul_div_floor(RAY / 2, RAY, RAY - bps_to_ray(3_000)).
}
#[test] fn fallback_price_clamped_to_the_floor() {
    let d = bps_to_ray(1_000);
    assert_eq!(floored_price(RAY, RAY, d), RAY / 10 * 9);
    assert_eq!(floored_price(RAY / 2, RAY, d), RAY / 2); }
#[cfg(feature = "presets")]
#[test] fn fallback_twap_follows_the_d4_conditions() {
    // legacy -> Ok(None); recommended + empty ring -> Err(TwapUnavailable);
    // newest 3_600_001 ms before now -> Err(LastGoodTwapTooOld); 3_600_000 -> Ok(Some(_)).
}
// Kani: fallback_price_never_breaches_the_floor: for p_fb, cap < 1 << 100 and d <= bps_to_ray(3_000),
// mul_div_floor(floored_price(p_fb, cap, d), RAY, RAY - d) <= cap.
```

  - `engine_core/src/bid.rs`: `fallback_quote_freezes_the_discount_and_applies_the_floor`, using the existing `liquidated()` fixture with `stale_cap_ray` set.
  - `harness/tests/stale_bids.rs`. All tests use `World::new_tier2(RiskArgs::recommended())`, warm 3, liquidate, then `go_stale()`, except `stale_feed_bid_refused_without_min_obs` (below). Every refusal asserts `snapshot_all()` is unchanged, and each test ends with `invariants::check`.

```rust
#[test] fn stale_feed_bid_uses_the_last_good_twap() { /* bid fills at floored_price(fallback_bid_price(T_lg, ..)); BidCtx fallback */ }
#[test] fn stale_feed_bid_refused_past_max_age() { /* now past newest + 3_600_000 -> RiskError::LastGoodTwapTooOld */ }
#[test] fn stale_feed_bid_refused_without_min_obs() {
    // World::new_tier2(RiskArgs::legacy()); liquidate; SetCollateral(tier2_collateral_args(), recommended)
    // (the ring holds the 1 sample its Observe records, below min_obs 3); go stale -> RiskError::TwapUnavailable
}
#[test] fn stale_feed_bid_held_at_the_floor() {
    // after the liquidation, publish 1.4p three times (the TWAP rises), then go stale:
    // the fill equals the quote at floor(stale_cap * (RAY - d) / RAY), below band(edge(T_lg)).
}
#[test] fn discount_stays_frozen_across_fallback_bids() { /* two fallback bids 10 min apart pay the same collateral per coin */ }
#[test] fn fallback_final_bid_settles_and_releases() { /* final fallback bid: Settled, type and global reservations released */ }
```

- [ ] **Step 2: Run them.** `cargo test -p risk_core --features presets stale`, `cargo test -p engine_core bid` and `cargo test -p harness --test stale_bids` fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them again.** `a_stale_feed_stops_the_bid` (`harness/tests/bid.rs:331`) still passes under legacy, and `cargo test -p harness --test liquidate` passes unchanged (legacy `stale_cap_ray` is 0). Run Kani if available, then the full gate.
- [ ] **Step 5: Commit.** `git add v04/risk_core v04/engine_core v04/programs/methods/guest/src v04/harness`, then `git commit -S -m "feat(v04): D4 bids on a stale feed at the last-good TWAP, held at the floor (ADR-074)"`.

### Task 10: The operation × level and stale-feed matrix tests

**Files:**
- Create: `v04/harness/tests/pause_matrix.rs`
- Modify: `v04/harness/src/levels.rs` (fixtures)

**Interfaces:**
- Consumes: `risk_core::pause::{Op, Level, FeedCase, Gate, allowed}`, `World::{set_pause_level, snapshot_all}`, `Chain::cycles` (`harness/src/lib.rs:215`, simulation without commit).
- Produces:
  - `harness::levels::Fixture { pub world: World, pub root: Actor, pub actors: Vec<Actor>, pub signers: Vec<PrivateKey>, pub msg: EngineMsg }`
  - `pub fn fixture(op: Op, risk: RiskArgs) -> Fixture`: a fresh `World::new_tier2(risk)` where `op`'s own preconditions hold. Every fixture, legacy included, is built this way, so `go_stale` and the collateral args match across the matrix. `fixture` dispatches on groups of ops that share a setup (position ops, auction ops, system ops, admin ops) to one helper each, so no function exceeds 50 lines. For example:
    - Liquidate: an unsafe position, with the ring warm under a TWAP preset;
    - Bid: a live auction;
    - ClaimLeftover: a settled auction with leftover;
    - SettleEmpty: the lot-3 auction of `cycles.rs`;
    - TakeSurplus: a live surplus auction;
    - CloseSurplus / CloseDebt: past the TTL.
  - `pub fn expected(op: Op, level: Level, feed: FeedCase) -> Option<String>`: `None` = runs, else the `fail_text`.
    - It is derived from `allowed`, plus `Paused` / the feed codes as in `gate`, plus D4's result for Bid: `Fallback` under recommended → runs, under legacy → `PriceFeedStale`.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn every_operation_at_every_pause_level() {
    for op in Op::ALL { for level in Level::ALL {
        let mut f = fixture(op, RiskArgs::legacy());
        f.world.set_pause_level(level.as_u8()).expect("level set");
        // SetPauseLevel's own row moves L to the next level, so raising and lowering are both covered.
        if op == Op::SetPauseLevel { f.msg = f.world.set_pause_level_msg(next(level).as_u8()); }
        match expected(op, level, FeedCase::Live) {
            None => assert!(f.world.chain.cycles(f.root, &f.actors, &signers(&f), &f.msg).1.is_ok(), "{op:?} at {level:?} runs"),
            Some(text) => { let before = f.world.snapshot_all();
                expect_revert(f.world.chain.send(f.root, &f.actors, &signers(&f), &f.msg), &text);
                assert_eq!(f.world.snapshot_all(), before, "{op:?} at {level:?} moved nothing"); }
        }
    } }
}
/// (L + 1) % 4 as an exhaustive match (no unchecked arithmetic): Normal -> NoLiquidations ->
/// NoSystemStarts -> Halted -> Normal.
fn next(level: Level) -> Level { /* match */ }
#[test]
fn every_operation_with_a_stale_feed() {
    // The same loop at level 0 with go_stale(): "Stale-ok" uses fixture(op, RiskArgs::recommended())
    // (D4 holds); "Stale-no" uses fixture(op, RiskArgs::legacy()). Bid: Stale-ok runs, Stale-no reverts
    // PriceFeedStale; Liquidate reverts PriceFeedStale in both; every other op runs.
}
```

  Successes are simulated, not sent, so one fixture serves exactly one case. Refusals are sent, so the revert's state rollback is observed. *(Plan 9 ruling.)*

- [ ] **Step 2: Run them.** `cargo test -p harness --test pause_matrix` fails to compile.
- [ ] **Step 3: Implement** `Fixture`, `fixture` and `expected`. `pause_matrix.rs` and `levels.rs` must each stay ≤ 800 lines. If needed, split the fixtures into `harness/src/levels_fixtures.rs`.
- [ ] **Step 4: Run them again.** This is slow, because each case builds a world, so run it in the background. Then the full gate.
- [ ] **Step 5: Commit.** `git add v04/harness`, then `git commit -S -m "test(v04): every operation at every pause level and under a stale feed"`.

### Task 11: Cycles: budget assertions, the `SetPauseLevel` row and `CYCLES.md`

**Files:**
- Create: `v04/harness/src/cycles.rs`: `Row`, `measured`, `proxied`, `table`, `write_table`, `GAS_CAP` and `BASE_FEE`, moved verbatim from `harness/tests/cycles.rs` (`:22-25`, `:44-93`, `:202-209`) and made `pub`. A pure refactor.
- Create: `v04/harness/tests/cycles_tier2.rs`. `cycles.rs` is 486 lines; keep the new rows separate.
- Modify: `v04/harness/src/lib.rs` (`pub mod cycles;`)
- Modify: `v04/harness/tests/cycles.rs`: `use harness::cycles::{..}` in place of the moved items. Every assertion stays.
- Modify: `v04/CYCLES.md`

**Interfaces:**
- Consumes: `World::new_tier2`, `warm_twap`, `set_pause_level_msg`, `Chain::cycles`, `harness::cycles::{Row, measured, proxied, table, write_table}`.
- Produces:
  - Consts: `SET_PAUSE_LEVEL_EXECUTIONS: u32 = 12` (hand-counted from the message sequence and documented as in `cycles.rs:210-230`; `measured` takes it as a constant, so it is not asserted), `PARTIAL_BID_BUDGET = 520_000`, `FINAL_BID_BUDGET = 720_000`, `LIQUIDATE_BUDGET = 1_000_000`, `SET_PAUSE_LEVEL_BUDGET = 400_000` *(Plan 9 ruling R9-10)*, `RING_FILL_SPREAD = 40_000`.
  - The tier2 scenario uses the `cycles.rs` amounts (collateral 500_000, debt 600_000, R and oracle 0.5) and `World::new_tier2(RiskArgs::recommended())` *(Plan 9 ruling R9-10)*. The R9-13 e2e values are refused at `TIER2_MAX_AGE_MS`. The ring holds 8 samples before every measured row.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn tier2_rows_fit_the_budget() {
    // Liquidate, Bid (partial), Bid (final) under RiskArgs::recommended() with a full ring, and SetPauseLevel:
    // each succeeds; cycles <= the budgets above; writes the table via write_table("tier2", ..).
    // The executions column uses the hand-counted constants (legacy 29, 15, 21; SET_PAUSE_LEVEL_EXECUTIONS).
}
#[test] fn bid_cost_is_independent_of_ring_fill() {
    // Two worlds with twap_min_obs 1: ring of 1 sample vs 8 samples; partial-bid cycles differ by <= RING_FILL_SPREAD.
}
```

- [ ] **Step 2: Run them.** `cargo test -p harness --test cycles_tier2 -- --nocapture` fails to compile.
- [ ] **Step 3: Implement.** Move the helpers into `harness/src/cycles.rs` first, then write `cycles_tier2.rs`. Then run `cargo test -p harness --test cycles -- --nocapture` too. Every legacy row still simulates and fits the cap. If a message sequence changed, re-count its executions by hand and update the documented constant.
- [ ] **Step 4: Update `v04/CYCLES.md`.**
  - Replace the legacy figures with the new measurements, and note the delta against the 2026-11-03 figures.
  - Add a section "Tier 2 (Plan 9): TWAP preset, ring full", with the rows `Liquidate`, `Bid (partial)`, `Bid (final)`, `ObserveFeed`, `SetCollateral`, `SetPauseLevel`, and each row's budget.
  - Check each measured row against the §4 estimates. Any row above its assertion is a failure, not a doc note. Re-run the full gate.
- [ ] **Step 5: Commit.** `git add v04/harness/src/cycles.rs v04/harness/src/lib.rs v04/harness/tests/cycles.rs v04/harness/tests/cycles_tier2.rs v04/CYCLES.md`, then `git commit -S -m "test(v04): Tier 2 cycle budgets and SetPauseLevel row; CYCLES.md re-measured"`.

### Task 12: The `tier2` e2e stage

**Files:**
- Create: `v04/sdk/examples/tier2.rs`
- Modify: `v04/sdk/Cargo.toml`: `[[example]] name = "tier2"`, `required-features = ["session"]`.
- Modify: `v04/sdk/src/session.rs:440-460`: add `publish_with_confidence(&self, live, label, price_ray, confidence)`. `publish` calls it with 0.
- Modify: `v04/scripts/e2e.sh`:
  - `build_v04` `:76-78`: add `--example tier2`;
  - the `case` block `:169-185`: a `tier2` stage; `all` runs it after `system`;
  - the usage text and the header comment.
  - `run_example` (`:145-155`) needs no change: it already checks `^$1 complete$`.
- Modify: `.github/workflows/v04-e2e.yml`:
  - a step `v04/scripts/e2e.sh tier2` after the `system` step;
  - the summary's log loop `for f in sequencer slice system` gains `tier2`;
  - the artifact `path:` list gains `${{ env.E2E_HOME }}/tier2.log`.

**Interfaces:**
- Consumes: `sdk::{plan_set_collateral_with, plan_set_pause_level, plan_liquidate, plan_bid, plan_settle_empty, plan_claim_leftover, plan_observe_feed}`, `sdk::session`.
- Produces: the example prints `tier2 complete` on success and exits non-zero on any failed check. It runs this flow with the R9-13 values *(Plan 9 ruling)*:
  1. `Init` (legacy engine params), then `SetCollateral` with the time-compressed preset.
  2. Open a large unsafe position P1 and a tiny one P2 (the lot-3 case).
  3. `Liquidate` P1 must revert with 6070 (warm-up). Simulate it, then assert the `fail_text`.
  4. Publish three prices ≥ 10 s apart with `confidence = price / 200`, each followed by `ObserveFeed`.
  5. `Liquidate` P1 and P2 right after the third `ObserveFeed`. The 300 s window (R9-13) keeps all three samples in it across the 15 s blocks.
  6. `SetPauseLevel 1`, then a partial bid on P1 fills.
  7. Stop publishing until the feed is stale (`now + slack > ts + 180_000`). A final bid on P1 fills as a fallback bid: assert the auction is `Settled`. D4 can fire because the window (300_000) exceeds `max_oracle_age_ms + (min_obs − 2) · spacing` = 190_000 (R9-3), and the bid lands within `stale_bid_max_age_ms` (600_000) of the newest sample.
  8. `SetPauseLevel 3`: `ClaimLeftover` on P1 and `SettleEmpty` on P2 both succeed.

- [ ] **Step 1: Build it.** Write the example, then run `cargo build -p sdk --features session --example tier2` (WSL). It fails until the example compiles.
- [ ] **Step 2: Make it compile,** and run `RISC0_SKIP_BUILD=1 cargo clippy --locked -p sdk --all-targets --features session -- -D warnings`.
- [ ] **Step 3: Run the stage if a local LEZ build is present** (`~/lez-target/v04/release/sequencer_service`): `v04/scripts/e2e.sh up && v04/scripts/e2e.sh deploy && v04/scripts/e2e.sh tier2; v04/scripts/e2e.sh down`. It runs for about 15 min. The expected output is `e2e: tier2 complete`. If no local LEZ build exists, record that the stage was verified on CI only, and push nothing in this task.
- [ ] **Step 4: Run the full gate.** The `slice` and `system` examples still build, and `sdk/tests/*` pass.
- [ ] **Step 5: Commit.** `git add v04/sdk v04/scripts/e2e.sh .github/workflows/v04-e2e.yml`, then `git commit -S -m "test(v04): tier2 e2e stage (warm-up, confidence, level 1 bid, fallback bid, level 3 settles)"`.

### Task 13: The refusal generator learns `RiskError`

**Files:**
- Modify: `scripts/gen-refusal-codes.py`
- Modify: `scripts/tests/test_gen_refusal_codes.py`

**Interfaces:**
- Produces:
  - `RISK_ERROR_RS_PATH = "v04/risk_core/src/error.rs"`.
  - `ENUMS = (("EngineError", ERROR_RS_PATH, ""), ("RiskError", RISK_ERROR_RS_PATH, " (0.4 only)"))`.
  - `parse_codes` is reused per file.
  - `proof_pattern(variant, code, enum_name="EngineError")` and the raise regex use `{enum_name}::{variant}` (not `enum`, which would shadow the stdlib module).
  - `build_index` excludes both error files from the raise sources.
  - In `render`, the RiskError rows follow the EngineError rows in code order. Their Variant cell reads `` `TwapUnavailable` (0.4 only) ``. The intro sentence adds: "Codes 6070 and up are the 0.4 engine's `RiskError` (`v04/risk_core`), wrapped with `EngineError` in `engine_core::check::Refusal`."
  - New `MEANINGS` lines:
    - `TwapUnavailable`: "The TWAP has fewer than `twap_min_obs` samples in its window (liquidation, or a stale-feed bid)."
    - `SpotTwapDeviation`: "Spot deviates from the TWAP by more than `max_spot_twap_dev_bps`; liquidation refused."
    - `LiquidationsPaused`: "The type is at pause level 1 or above."
    - `SystemAuctionsPaused`: "The engine is at pause level 2 or above; no new surplus or debt auction."
    - `BidsPaused`: "Pause level 3: no bids or system takes."
    - `LastGoodTwapTooOld`: "The last good TWAP is older than `stale_bid_max_age_ms`; stale-feed bid refused."

- [ ] **Step 1: Write the failing tests** in `test_gen_refusal_codes.py`. The fixture also writes a mini `v04/risk_core/src/error.rs` with `Self::TwapUnavailable => 70,` and a v04 test naming `RiskError::TwapUnavailable`.

```python
def test_risk_error_rows_are_marked_0_4_only(self):   # doc has "| 6070 | `TwapUnavailable` (0.4 only) | 70 |"
def test_risk_error_proved_by_variant_path(self):      # the v04 test is listed as "(0.4) <name>"
def test_risk_error_without_test_fails_check(self):    # remove the test -> "TwapUnavailable: no proving test"
```

- [ ] **Step 2: Run them.** `python3 -m unittest scripts/tests/test_gen_refusal_codes.py -v` fails.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them again.** Then `python3 scripts/gen-refusal-codes.py` on the real tree must exit 0: every Plan 9 code has a proving test from Tasks 2–10. Revert the regenerated doc for now (`git checkout docs/architecture/refusal-codes.md`), because Task 14 owns it.
- [ ] **Step 5: Commit.** `git add scripts/gen-refusal-codes.py scripts/tests/test_gen_refusal_codes.py`, then `git commit -S -m "chore(scripts): refusal-code catalogue includes RiskError (0.4 only)"`.

### Task 14: Docs, ADRs and final gates

**Files:**
- Modify: `REQUIREMENTS.md`: rows R2 (`:48`), R4 (`:50`) and R5 (`:51`).
- Modify: `docs/architecture/refusal-codes.md` (regenerated)
- Modify: `docs/architecture/f8-parameters.md`
- Modify: `docs/architecture/pricing-model.md`: §1, and §2 `:91-96` ("Decided rule (D4), planned").
- Modify: `docs/design-log.md`: ADR-067 Status; append ADR-069, 070, 071, 072, 073, 074, 076 and 082.
- Modify: `docs/v04-port-design.md` §3 (state layout)
- Modify: `README.md` (test-count block)

**Interfaces:**
- Consumes: test names from Tasks 2–12, and the Task 11 measurements.

- [ ] **Step 1: `REQUIREMENTS.md`.**
  - R2 → `PoC`. Cite `liquidate_refused_until_twap_has_min_obs`, `keeper_reward_uses_the_smaller_of_spot_and_twap`, `a_spot_spike_is_refused_by_the_deviation_breaker`, `bid_price_takes_the_confidence_edge`, `bid_payout_capped_by_the_twap_anchor`, `twap_within_sample_range` and `validate_feed_ext_matches_v024_under_legacy_policy`. Drop "confidence always 0" and "no TWAP yet": `bid_price_takes_the_confidence_edge` publishes nonzero confidence.
  - R5 → `PoC`. Cite `every_operation_at_every_pause_level`, `every_operation_with_a_stale_feed`, `stale_feed_bid_uses_the_last_good_twap`, `stale_feed_bid_held_at_the_floor` and `level_2_blocks_starts_not_takes_or_closes`.
  - R4 note: the levels are per type; one type per config still holds (R-F3).
  - Cite Kani harnesses as "written, not run" unless they were run.
  - Run `python3 scripts/check-requirements.py REQUIREMENTS.md`. Expected: exit 0.
- [ ] **Step 2: Regenerate the refusal codes.** Run `python3 scripts/gen-refusal-codes.py`, then `python3 scripts/gen-refusal-codes.py --check`. Expected: exit 0, with rows 6070, 6071, 6073, 6074, 6075 and 6077 marked "(0.4 only)".
- [ ] **Step 3: `f8-parameters.md`.** Add a section "`RiskArgs` (v04, per type, `SetCollateral`)": one row per Plan 9 field, with columns legacy | recommended | bound (including R9-3) | ADR. State that the presets live in `risk_core::presets` (SDK and harness only). The five "Tier 2" gap rows stay: that is Plan 10.
- [ ] **Step 4: `pricing-model.md`.**
  - §1: the TWAP (ADR-070) and the price per operation (ADR-071, 072).
  - §2: replace "Decided rule (D4), planned for M1/M2, not built" with "D4 as built (ADR-074)". Include the floor formula `stale_cap_ray = floor(P_liq * RAY / (RAY - d_max_eff))`, `P_used = min(P_fb, floor(stale_cap_ray * (RAY - d) / RAY))`, the four conditions, the frozen discount, and the test names from Task 9.
- [ ] **Step 5: `design-log.md`.**
  - ADR-067: Status → "Built (Plan 9)", with the test names.
  - Append ADR-069, 070, 071, 072, 073, 074, 076 and 082 in the existing format (Context, Options, Decision, Trade-offs, Evidence, Status), with the decisions from spec §6.
  - Record the R9 rulings in the ADR each belongs to: R9-3 and R9-5 in ADR-071/072, R9-7 and R9-8 in ADR-073, R9-1 in ADR-082, R9-10 in ADR-070's Evidence.
  - If Kani was not run, add "Kani harnesses written, not run (no Kani in WSL), as ADR-015".
- [ ] **Step 6: `v04-port-design.md` §3.** Document the appended fields: `ConfigExt { system_level }`, `CollateralExt { risk, pause_level, ring }`, `AuctionState.stale_cap_ray`, variants 47 and 48, `SetCollateral.risk`, `Configure.risk` and `BidCtx.{paused_since_ms, fallback}`. Add the migration note "recreate local state; tags unchanged".
- [ ] **Step 7: README counts.** Run `python3 scripts/count-tests.py --markdown`, paste the block between the markers, then `python3 scripts/count-tests.py --check README.md`. Expected: exit 0.
- [ ] **Step 8: Final gates.**
  - The full gate (Global Constraints).
  - `git diff --stat b14e59b -- programs/ crates/auction_core/ crates/auction_engine_params/ Cargo.toml Cargo.lock`, which must print nothing.
  - `scripts/check-frozen-v024.sh`, exit 0.
  - `scripts/check-guests.sh --sha-only`, exit 0.
  - `python3 -m unittest discover -s scripts/tests -v`.
- [ ] **Step 9: Commit.** `git add REQUIREMENTS.md README.md docs/architecture/refusal-codes.md docs/architecture/f8-parameters.md docs/architecture/pricing-model.md docs/design-log.md docs/v04-port-design.md`, then `git commit -S -m "docs: Plan 9 — R2/R5 to PoC, D4 as built, ADR-069..074/076/082, refusal codes and counts"`.

---

## Self-review notes

- **Spec coverage.** Each Plan 9 item and the task that covers it:

  | Spec item | Task |
  |---|---|
  | §2 check script and CI | 1 |
  | §3.10 Plan 9 codes | 2 (proving tests 7–10, catalogue 13–14) |
  | §3.1.5 | 3 |
  | §3.1.1–3 | 4 |
  | §3.1.4 | 5 (pure), 7 (wiring) |
  | §3.1.6 | 6 |
  | §3.0 state/messages/presets | 6 |
  | §3.2.1 and the §3.2.2 rule table | 8 |
  | §3.2.3–4 | 9 |
  | §3.2.6 matrix | 10 |
  | §3.1.9, §3.2.7 and §4 | 11 |
  | §5 e2e | 12 |
  | §8 docs | 14 |
- **Not in this repo.** The proposal draft is untracked in `../lez-auction-engine`, so its R2/R5 rows and the "delivered early" markers are not touched by this plan.
- **Names used across tasks:**
  - `liquidation_price`, `bid_pricing`, `BidPricing`, `collateral_gate`, `system_gate` (gate.rs, Tasks 7–9);
  - `fallback_twap`, `floored_price`, `stale_cap` (Task 9);
  - `twap`, `record`, `TwapRing` (Task 4);
  - `RiskArgs::{legacy, recommended, validate}` (Task 6);
  - `World::{new_tier2, warm_twap, go_stale, set_pause_level, snapshot_all}` (Tasks 6–9).
