# Plan 8: RFP-014 bid refresh (Tier 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the RFP-014 bid postable the day the RFP reopens: requirements matrix aligned with the PR #229 text, a v04-first README with generated test counts and a dependency table, a generated refusal-code catalogue, a pricing-model note, and a rewritten proposal draft — no engine code changes.

**Architecture:** Documents plus three small Python tools in `scripts/` (stdlib only, like the existing `check-*.py`), each with a `--check` mode wired into the existing `requirements` job in `.github/workflows/core.yml`, so nothing typed by hand can drift. The only Rust change is one new test file in `crates/auction_core/tests/` (never compiled into a guest).

**Tech Stack:** Python 3.12 stdlib + `unittest` (existing `scripts/tests/` pattern), Markdown, GitHub Actions, Rust test (cargo).

**Spec:** `docs/superpowers/specs/2026-11-07-rfp014-competitive-upgrade-design.md` (§4 Tier 1, §8 milestones, settled S1–S5).

## Global Constraints

- No change to any file compiled into the v0.2.4 guest (`programs/**`, `crates/auction_core/src/**`, `crates/auction_engine_params/src/**`): the `rfp014-poc` ImageID must not move (D6). `scripts/check-guests.sh` must still pass.
- Commits GPG-signed (`git commit -S`), conventional-commit messages, **no Claude / AI attribution, no Co-Authored-By trailer**. Never `--no-verify`, never bypass signing. If the GPG cache has expired, stop and leave the staged change plus the commit message for the author.
- Do not push, open PRs or post anything; the author does that.
- Never name a rival (competing bids A and B) in any committed file. The rival analysis stays out of the repo (S4).
- New scripts: Python 3 stdlib only, `#!/usr/bin/env python3`, module docstring with usage, exit 0/1/2 like `scripts/check-requirements.py`.
- Every number written in a doc (test counts, cycles, run IDs, durations) must come from a script output, a CI run ID already recorded in `docs/architecture/ci.md` / `real-proofs.md`, or `CYCLES.md` / `v04/CYCLES.md`. No typed estimates.
- Price $40,000 all-in incl. audit; 12 weeks; milestone payouts unchanged; durations M0 0.5 · M1 2.5 · M2 1.5 · M3 1 · M4 2.5 · M5 0.5 · M6 1.5 · M7 2.
- Heavy cargo runs go through WSL (`wsl -d Ubuntu-20.04 -e bash -lc '…'` with `scripts/wsl-cargo.sh`); Python tools run anywhere.

## Review Focus

- A REQUIREMENTS row with **only** v04 evidence (v0.2.4 cell `—`) must pass the checker; a row with `—` in both evidence cells and status PoC must fail. → Task 2 tests.
- A test name that exists only under `v04/` must resolve (today `SOURCE_DIRS` omits `v04`). → Task 2 test `test_v04_test_resolves`.
- An `EngineError` variant with no test that names it must show "**no test**" in the catalogue and make `--check` fail, not be silently skipped. → Task 4 test `test_variant_without_test_fails_check`.
- `count-tests.py` must not count `#[test]` inside `target/`, `fuzz/corpus`, or rival/scratch dirs, and must count `#[tokio::test]`/`#[kani::proof]` separately from `#[test]`. → Task 3 tests.
- Old 5-column REQUIREMENTS tables (e.g. a fixture) must give a clear error, not an IndexError. → Task 2 test `test_five_column_row_reports_format_error`.

---

### Task 1: Commit the ecosystem research

**Files:**
- Create: `docs/research/2026-11-07-evm-engines.md` (from `C:\Users\user\AppData\Local\Temp\claude\d84eecd5fe060e4c\d6ce8e81-456c-4d8b-8a82-319abe64b373\scratchpad\research\evm-engines.md`)
- Create: `docs/research/2026-11-07-non-evm-engines.md` (from `…\scratchpad\research\non-evm-engines.md`)

**Interfaces:** Produces: the two paths above, cited as `doc:` evidence by Task 6.

- [ ] **Step 1:** Copy both files. Prepend this header to each (exact text):

```markdown
> Research note, 2026-11-07. Sources are cited inline. Claims tagged *secondary* or *not found*
> come from search summaries or memory and must be re-checked before they are quoted in a proposal.
```

- [ ] **Step 2:** `grep -niE "ant[u]mbra|web3[s]pell|rival|#18[7]|#15[9]" docs/research/2026-11-07-*.md` → no output. Remove any hit (sentence-level), keeping the protocol research intact.
- [ ] **Step 3:** Commit: `git add docs/research/2026-11-07-*.md && git commit -S -m "docs(research): liquidation and auction engines across EVM and non-EVM chains"`

### Task 2: Requirements checker — per-engine evidence columns, v04 sources, F8

**Files:**
- Modify: `scripts/check-requirements.py`
- Test: `scripts/tests/test_check_requirements.py`

**Interfaces:**
- Produces: table format `| ID | Requirement | Status | v04 evidence | v0.2.4 evidence | Notes |`; a cell of exactly `—` means "none for this engine". `REQUIRED` gains `F8`. `SOURCE_DIRS = ("crates", "programs", "cli", "fuzz", "v04")`.
- `check(text, root, required=REQUIRED) -> list[str]` keeps its signature.

- [ ] **Step 1: Write failing tests** in `scripts/tests/test_check_requirements.py` (follow the file's existing fixture style — temp root with fake `.rs` files):
  - `test_six_column_row_resolves_both_cells`: row `| F1 | x | PoC | \`a_v04\` | \`b_v024\` | |` with both tests present → `[]`.
  - `test_v04_only_evidence_passes`: v0.2.4 cell `—`, v04 cell names a test that lives under `v04/engine_core/src/x.rs` → `[]`.
  - `test_v04_test_resolves`: same test name present only under `v04/` → resolves (fails today).
  - `test_poc_with_no_evidence_in_either_cell_fails`: both `—` → error contains `needs evidence`.
  - `test_f8_required`: table without an F8 row → error `F8: missing from REQUIREMENTS.md`.
  - `test_five_column_row_reports_format_error`: an id row with 5 cells → error contains `expected 6 columns`.
- [ ] **Step 2:** `python3 -m unittest discover -s scripts/tests -p 'test_check_requirements.py' -v` → the new tests FAIL.
- [ ] **Step 3:** Implement: parse 6 cells; evidence = tokens from both evidence cells; `—` contributes none; id rows with ≠ 6 cells give `"{rid}: expected 6 columns (ID, Requirement, Status, v04 evidence, v0.2.4 evidence, Notes), got {n}"`; add `"v04"` to `SOURCE_DIRS`; `F8` in `REQUIRED` (`range(1, 9)`). Update the module docstring for the new columns. Keep existing tests passing by updating their fixtures to 6 columns.
- [ ] **Step 4:** Rerun the unittest command → all PASS. (`REQUIREMENTS.md` itself will fail until Task 6 — expected; do not run the CI job yet.)
- [ ] **Step 5:** Commit: `git commit -S -m "feat(scripts): requirement checker reads v04 and v0.2.4 evidence columns; F8 required"`

### Task 3: Generated test counts

**Files:**
- Create: `scripts/count-tests.py`
- Test: `scripts/tests/test_count_tests.py`

**Interfaces:**
- Produces: `count(root) -> dict[str, int]` with keys `v04`, `v024_programs`, `crates`, `cli`, `fuzz_kani`, `qml`, `cpp`, `python`, `total`; CLI `count-tests.py [--root DIR] [--markdown] [--check FILE]`. `--markdown` prints a table; `--check FILE` replaces nothing, but fails (exit 1) if the block between `<!-- test-counts:begin -->` and `<!-- test-counts:end -->` in FILE differs from `--markdown` output.
- Counting rules: Rust = functions with `#[test]`, `#[tokio::test]` or proptest `fn` inside `proptest!` blocks counted under their dir; `#[kani::proof]` counted in `fuzz_kani` regardless of dir; QML = `function test_` in `apps/**/tests/qml/*.qml`; C++ = `void test` slots / `TEST(` in `modules/**/tests`; Python = `def test_` in `scripts/tests`. Skip dirs named `target`, `node_modules`, `.git`, `corpus`, `artifacts`.

- [ ] **Step 1: Write failing tests** in `scripts/tests/test_count_tests.py`: `test_counts_by_area` (fixture tree with 2 v04 tests, 1 crates test, 1 kani proof, 1 qml, 1 python → exact dict incl. `total` 6); `test_skips_target_dirs` (a `#[test]` under `crates/x/target/` not counted); `test_check_detects_stale_block` (README block with an old number → exit 1); `test_check_passes_on_fresh_block`.
- [ ] **Step 2:** Run `python3 -m unittest discover -s scripts/tests -p 'test_count_tests.py' -v` → FAIL (no module).
- [ ] **Step 3:** Implement `scripts/count-tests.py` per the interface.
- [ ] **Step 4:** Rerun → PASS. Then `python3 scripts/count-tests.py --markdown` on the repo and sanity-check `total` is in the 1,550–1,700 range the 2026-11-07 grep found (≈1,569 `#[test]` plus QML/C++/Python); if far off, fix the rules, not the number.
- [ ] **Step 5:** Commit: `git commit -S -m "feat(scripts): count tests by area for the README"`

### Task 4: Generated refusal-code catalogue

**Files:**
- Create: `scripts/gen-refusal-codes.py`
- Create: `docs/architecture/refusal-codes.md` (generated)
- Test: `scripts/tests/test_gen_refusal_codes.py`

**Interfaces:**
- Consumes: `programs/auction_engine/core/src/error.rs` — `EngineError` variants and `const fn code()` arms (`Self::Name => N` / `Self::Name(_) => N`). The v04 engine uses the same enum via `crates/auction_engine_params` (`#[path]`), so one catalogue covers both engines.
- Produces: `parse_codes(error_rs_text) -> list[tuple[str, int]]`; `find_uses(root, variant) -> {"raised_in": [paths], "tests": [test fn names]}` (raised_in = non-test `.rs` files under `programs/`, `v04/`, `crates/` containing `EngineError::{variant}`, excluding `error.rs`; tests = `#[test]` fns whose body mentions `{variant}` — in either `EngineError::{variant}` form or the wire form `[60NN]`/`6000 + NN` string the integration tests assert); CLI `gen-refusal-codes.py [--root DIR] [--check]` writes/compares `docs/architecture/refusal-codes.md`.
- Doc columns: `Wire code` (`6000 + code`) | `Variant` | `Engine code` | `Raised in` | `Proved by` | `Meaning` (Meaning from a `MEANINGS` dict in the script: one short line per variant, written by the implementer from the raising code; a variant missing from `MEANINGS` fails `--check`).

- [ ] **Step 1: Write failing tests**: `test_parse_codes_handles_tuple_variants` (`WrongAccount(_) => 12` → `("WrongAccount", 12)`); `test_codes_unique` (two variants with the same code → error); `test_variant_without_test_fails_check` (fixture variant with no mentioning test → `--check` exit 1 and the row shows `**no test**`); `test_check_detects_stale_doc`.
- [ ] **Step 2:** Run `python3 -m unittest discover -s scripts/tests -p 'test_gen_refusal_codes.py' -v` → FAIL.
- [ ] **Step 3:** Implement; then run `python3 scripts/gen-refusal-codes.py` to write the doc. For each real variant reported `**no test**`, do **not** add engine code: list it in the doc's closing section "Gaps" with the plan that will add the test (Tier 2/3), and make `--check` accept variants listed in a `KNOWN_GAPS` set in the script (each with a one-line reason).
- [ ] **Step 4:** `python3 scripts/gen-refusal-codes.py --check` → exit 0; unittest → PASS.
- [ ] **Step 5:** Commit: `git commit -S -m "docs: generated refusal-code catalogue with the test that proves each code"`

### Task 5: F3 whole-lot test and F8 parameter audit

**Files:**
- Create: `crates/auction_core/tests/f3_whole_lot.rs` (integration-test file: never in a guest)
- Create: `docs/architecture/f8-parameters.md`

**Interfaces:** Produces test names `whole_remaining_lot_bid_is_accepted_below_min_bid` and `target_reaching_bid_is_accepted_below_min_bid` (cited by Task 6, F3 row); doc path cited by the F8 row.

- [ ] **Step 1: Write the tests** using `auction_core::auction::{Auction, BidRequest, quote_bid}` and the helpers the crate's existing tests use (`quote`, `rai_default` are private to `auction.rs` tests — build the `Auction` through the public `Auction::open` with an equivalent quote; if a helper is needed, define it in the new file). Assertions:
  - whole lot: auction whose remaining target < `min_bid` (25) and `chost` (110); `quote_bid` with `coin_in_max = remaining_target()` → `Ok(fill)` with `fill.collateral_out == a.collateral_left`.
  - target-reaching: a bid with `coin_in_max == remaining_target()` where `remaining_target() < min_bid` → `Ok(_)` and `charged == remaining_target()`.
- [ ] **Step 2:** Run in WSL: `scripts/wsl-cargo.sh test -p auction_core --test f3_whole_lot` → PASS (behaviour already exists, `auction.rs:131-139`; this pins it). If either FAILS, stop and report — that is an engine finding, not a doc task.
- [ ] **Step 3:** Write `docs/architecture/f8-parameters.md`: one table of every constant in `crates/auction_core/src/**`, `crates/auction_engine_params/src/**`, `v04/engine_core/src/**` that bounds or defaults an economic or authority value (find with `grep -rnE "const [A-Z_]+: " …`). Columns: `Constant` | `Value` | `File:line` | `Kind` (`init parameter bound` / `math constant` / `protocol limit` / `production default — Tier 2`) | `Why`. Then a list of every init-time parameter (from `args.rs` `InitConfig`/collateral args) showing which authority sets it. Any row classed `production default — Tier 2` is copied into the spec's §5 E9 list in the same commit.
- [ ] **Step 4:** `scripts/check-guests.sh` (WSL) → guests unchanged.
- [ ] **Step 5:** Commit: `git commit -S -m "test(core): pin whole-lot bids under min-bid and dust; docs: F8 parameter audit"`

### Task 6: REQUIREMENTS.md restructure

**Files:**
- Modify: `REQUIREMENTS.md`

**Interfaces:** Consumes Task 2 format, Task 1/4/5 paths and test names.

- [ ] **Step 1:** Convert every row to the 6-column format. Fill **v04 evidence** with v04 tests/CI jobs (`ci:v04.yml/…`, `ci:v04-e2e.yml/…`, test names under `v04/`), **v0.2.4 evidence** with today's evidence. Status reflects the stronger engine; notes say which engine is primary (S3).
- [ ] **Step 2:** Add `F8` (authorities and parameters set at instance init, no hardcoded production values): evidence `doc:docs/architecture/f8-parameters.md` plus the init tests that exist (search `init_` tests in `v04/` and `programs/integration_tests`); status `PoC` if the audit found no Tier-2 row, else `PoC (partial)` naming M1/E9.
- [ ] **Step 3:** Reword F3 to PR #229's text ("a bid may purchase any portion of the remaining lot"); add Task 5's two tests; note: "`BelowMinBid`/`DustRemainder` apply only to bids that leave a live remainder; any bid that ends the auction or takes the whole lot is accepted (Maker `chost` precedent: a remainder below dust costs more to settle than it raises)."
- [ ] **Step 4:** Every `PoC (partial)` row's note ends with `Closed in M<n>.` naming the §8 milestone (S1). R2 → M2, R5 → M1, R1 → M1, S1 → M6, U6 → M4, F5 → M3 (verify each against the rows' current notes).
- [ ] **Step 5:** Add a short "Dependencies" link line under the intro pointing to README §Dependencies (Task 7).
- [ ] **Step 6:** `python3 scripts/check-requirements.py REQUIREMENTS.md` → exit 0.
- [ ] **Step 7:** Commit: `git commit -S -m "docs(requirements): per-engine evidence, F8 row, F3 to the reworded text"`

### Task 7: README refresh + dependency table + CI wiring

**Files:**
- Modify: `README.md`
- Modify: `.github/workflows/core.yml` (job `requirements`)

**Interfaces:** Consumes Task 3 (`--markdown`, `--check`), Task 4 (`--check`).

- [ ] **Step 1:** Rewrite `## Status` v04-first: 0.4 engine (Plans 5–7 merged, PRs #4/#6), v04 CI and v04-e2e run IDs from `docs/architecture/ci.md`, v0.2.4 engine + legacy-testnet run, Basecamp app story result (from `docs/architecture/basecamp-app.md`). Remove anything stale (e.g. "v0.2.4 only", "Plan 4 unmerged").
- [ ] **Step 2:** Add `## Tests` with the generated block between `<!-- test-counts:begin -->` / `<!-- test-counts:end -->` (paste `count-tests.py --markdown` output).
- [ ] **Step 3:** Add `## Dependencies` table (PR #229 clause). Rows: price feed · host CDP · collateral & stable token programs · admin authority (RFP-001) · freeze authority (RFP-002) · runtime (LEZ) · framework (SPEL) · wallet/core module (`lez_core`). Columns: `Capability` | `Built against today (version/commit)` | `If late or changed` | `Where the interface is isolated`. Versions from `v04/Cargo.toml`, `flake.lock`, `docs/v04-port-design.md`, `docs/architecture/hooks.md` — cite, don't guess.
- [ ] **Step 4:** Add to `## Documents`: refusal codes, F8 audit, pricing model (Task 8 path), research notes.
- [ ] **Step 5:** In `core.yml` job `requirements`, after the existing `check-requirements.py` step, add steps `python3 scripts/count-tests.py --check README.md` and `python3 scripts/gen-refusal-codes.py --check`; extend the unittest step's `-p` to run all `test_*.py` (or add a second step for the two new test files).
- [ ] **Step 6:** Run all three `--check` commands locally → exit 0; `python3 -m unittest discover -s scripts/tests -v` → PASS.
- [ ] **Step 7:** Commit: `git commit -S -m "docs(readme): 0.4-first status, generated test counts, dependency table; ci: drift checks"`

### Task 8: Pricing-model design note

**Files:**
- Create: `docs/architecture/pricing-model.md`
- Modify: `docs/design-log.md` (one ADR entry, next free number after ADR-066 → ADR-067)

- [ ] **Step 1:** Write the note with these sections (spec §4 T1.6): *Live price on every bid* (why no `redo`/`cusp`/`tail`; cannot stick when the market passes max discount; cannot sell far under market when it rises — cite `crates/auction_core/src/auction.rs` and the Maker Clipper docs from Task 1's research); *Stale feeds and R5* (today's behaviour with `bid_rejects_invalid_feed`; the decided rule D4 — bounded last-good TWAP — marked "planned, M1/M2"); *Oracle-extractable value* (rising discount auctions it back; capped trigger reward is the only fixed prize); *What a liquidation costs* (cycles from `v04/CYCLES.md` broken down by transfers / custody / buffer accounting, and a measured or clearly-labelled estimate of the counters-only share). No rival named.
- [ ] **Step 2:** ADR-067 "Live oracle pricing on every bid; bounded last-good TWAP under a stale feed (decided, to build in M1/M2)" in the design log's existing ADR format.
- [ ] **Step 3:** `grep -niE "ant[u]mbra|web3[s]pell" docs/architecture/pricing-model.md docs/design-log.md` → none. Every path cited exists.
- [ ] **Step 4:** Commit: `git commit -S -m "docs: pricing model note and ADR-067 (live pricing, bounded stale-feed bids)"`

### Task 9: Proposal draft rewrite (untracked file; not committed)

**Files:**
- Modify: `C:\Users\user\Documents\basecamp\lez-auction-engine\docs\outreach\rfp014-proposal-draft.md` (untracked in the main checkout — back it up first to `…\scratchpad\rfp014-proposal-draft.backup-2026-11-07.md`)

- [ ] **Step 1:** Replace every stale number (e.g. "About 880 Rust tests") with Task 3's generated figures and cite run IDs/cycles as in README.
- [ ] **Step 2:** Insert the dependency table (same content as README §Dependencies).
- [ ] **Step 3:** Add a "Design choices" section stating, without naming anyone: real custody and payouts; live pricing on every bid; full F4/F5 with buffer, automatic freeze, burn and solvency; 0.4 engine with hosted e2e at `RISC0_DEV_MODE=0`; Basecamp app; refusal-code catalogue; requirement checker in CI.
- [ ] **Step 4:** Re-map milestones to spec §8: payouts unchanged; durations M0 0.5 · M1 2.5 · M2 1.5 · M3 1 · M4 2.5 · M5 0.5 · M6 1.5 · M7 2 (total 12); each milestone gets the spec §8 items in its Deliverables and a "**Built before the grant:**" line listing what already exists with links (for M0: Tasks 2–8 outputs). Remove the "knowingly below" M6 admission — the rebalance plus pre-built work replaces it.
- [ ] **Step 5:** "Requirements status" section: every `PoC (partial)` row from REQUIREMENTS listed as "partial now, closed in M<n>".
- [ ] **Step 6:** Refresh the checklist at the top: author items — audit quotes + named firm, upload demo video and link it, every `[FILL]`, final read; drop items already done.
- [ ] **Step 7:** `grep -niE "ant[u]mbra|web3[s]pell|880" <draft>` → none. Sum of payouts = $40,000; sum of durations = 12.

---

## Done when

`python3 scripts/check-requirements.py REQUIREMENTS.md`, `python3 scripts/count-tests.py --check README.md`, `python3 scripts/gen-refusal-codes.py --check`, `python3 -m unittest discover -s scripts/tests -v` and `scripts/check-guests.sh` all pass; branch `docs/rfp014-bid-refresh` holds signed commits for Tasks 1–8; the proposal draft is rewritten and its backup exists; nothing pushed.
