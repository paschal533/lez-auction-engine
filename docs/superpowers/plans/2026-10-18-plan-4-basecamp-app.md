# Plan 4: the Auctions app in Basecamp — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Basecamp app that runs the whole RFP-014 demo story (F1–F5, shortfall, debt auction, freeze) on a localnet as UI actions only, built to the approved v3/v4/v5 boards, plus a scripted recording of that session.

**Architecture:** Three layers, mirroring lez-programs `apps/amm` + `modules/amm` (64c01210): a Rust `auction_ffi` cdylib (decode, quote, plan, chain *reads* via the SDK's `RpcChain`; no signing), a C++ universal core module `auction_engine` that submits plans through Basecamp's `lez_core` wallet module and polls confirmation without blocking, and a `ui_qml` app `auctions` (C++ `.rep` backend + QML on Logos.Theme/Controls). The CLI's orchestration moves into `auction_sdk::ops` so the CLI and the app cannot diverge.

**Tech Stack:** Rust 1.94.0, LEZ v0.2.4, SPEL 1ef0500f, risc0 3.0.5, cbindgen 0.28; C++17, Qt 6.8, logos-module-builder (`mkLogosModule`, `mkLogosQmlModule`), lez_core acf0cd50; Logos design system 88330d04; logos-qt-mcp test framework (Node); Nix 2.34.8 in WSL; Windows ffmpeg (gdigrab) for the recording.

**Spec:** `docs/superpowers/specs/2026-10-17-plan-4-basecamp-app-design.md` (approved 2026-10-18). UI boards: `docs/design/ui/auctions-main-v3.html`, `auctions-positions-and-confirm-v4.html`, `auctions-settled-system-dev-v5.html`, rules in `docs/design/ui/ui-review.md`. Research dossier with file:line references for every SDK/CLI/lez_core/amm name used here: copy it into `docs/superpowers/plans/2026-10-18-plan-4-dossier.md` in Task 1.

## Global Constraints

- LEZ v0.2.4, SPEL 1ef0500f, the committed guests: ImageIDs unchanged; the `rfp014-poc` evidence stays valid. No guest or program change in this plan.
- Wallet and signing through `lez_core` = `github:logos-blockchain/logos-execution-zone-module?rev=acf0cd501b262c4c15969e3735e85318297b85bf` (metadata 0.4.1, `version()` returns "0.3.0"; built on LEZ v0.2.4). Never the 0.5.x module (LEZ v0.3.1, 6-argument send, Borsh instructions).
- Basecamp at upstream master `e09962f5998792afd3bfed8c69c3b8f07f5825a5` or later (needs `--module` cf16f9d and PR #506 `nix run .#dev`). Design system `88330d04968954be956311b0be429048a41da2ea` (the one Basecamp master ships); our flakes pin a logos-module-builder whose design system has `LogosNotice`, `LogosStageLane`, `LogosLink`, `LogosIcon` (builder `e43a4817` or later).
- Builds inside WSL with Nix; cargo only through `scripts/wsl-cargo.sh` (target dir `~/lez-target/plan2`, never anywhere else). `RISC0_SKIP_BUILD=1` for builds that need no guest ELF.
- Disk: no Basecamp/Qt Nix build starts with less than 10 GB free on C:; `scripts/basecamp/host.sh` checks before and after.
- IPC calls between modules time out after 20 s: no FFI or module call may block on a confirmation or a full scan. Every FFI read uses `RpcChain::connect_with_timeout(url, 5 s)` with `ReadRetry { tries: 2, first_delay: 1 s }`.
- u128 values cross every boundary (FFI JSON, module, `.rep`, QML) as decimal strings.
- UI: Logos.Theme tokens and the boards' layout only; orange means "now / act"; primary button label black; Public Sans with `font.features: { "tnum": 1 }`; no legacy hex from older Basecamp views.
- Numbers shown in the app equal what the CLI prints for the same state (same `auction_sdk` functions).
- Commits GPG-signed (never bypass), conventional messages, no AI attribution lines of any kind. Nothing pushed without the author's go-ahead.
- Every interactive QML item has an `objectName` (e2e and recording use them).

## Review Focus

1. **The node stops answering mid-session** (localnet killed, testnet 502). Every view and submit returns a `chain` error within 12 s; the app shows a LogosNotice and stays responsive; an in-flight lane shows "outcome unknown, lock kept" (never "Dropped"). Test: Task 3 `view_against_a_closed_port_fails_within_12_s`; Task 11 `tst_header_shows_node_unreachable`.
2. **The wallet is already open** (another Basecamp app opened it, or a second launch). `lez_core.open` returns non-zero; the module treats that as open and continues. Test: Task 10 `open_wallet_treats_already_open_as_success`.
3. **Switching account while a transaction is in flight.** The lane stays with the signer that sent; a second send by the same signer is refused (`in_flight`), another signer may send. Test: Task 3 `tx_begin_twice_for_one_signer_is_in_flight`; Task 11 `tst_lane_survives_account_switch`.
4. **Amounts beyond 2^53** (RAY prices 2e27, u128 balances). They stay decimal strings end to end and format exactly. Test: Task 9 `prices_and_amounts_are_decimal_strings`; Task 11 `tst_format_amount_keeps_all_digits`.
5. **The localnet is restarted** (chain replaced, cursor stale). Scan reports `restarted` and starts again from the anchor; lists empty instead of showing stale auctions. Test: Task 8 `scan_page_reports_a_replaced_chain`; Task 12 `tst_rescan_after_restart_clears_list`.

---

## File Structure

```
crates/auction_sdk/src/
  confirm.rs            (modify) split submit into track_begin / poll_once / track_end
  ops/mod.rs            (new)    Prepared, Fact, prepare_* per transaction command
  ops/{liquidate,bid,claim,system,admin,dev}.rs (new) one file per command family
  views.rs              (new)    structured views the CLI and FFI share (accounting, host, auction, settled, at-risk, schedule)
  scan.rs               (modify) scan_page, Cursor.bids
  keystore.rs           (modify) labels()
  quote.rs              (modify) PositionHealth.cr_bps, discount_schedule
cli/src/{tx_cmds,read_cmds,host_cmds,dev_cmds,oracle_cmds,tx_report}.rs (modify) print from ops/views; output unchanged
crates/auction_ffi/
  Cargo.toml build.rs cbindgen.toml include/auction_ffi.h
  src/lib.rs            ABI functions only (the one place with unsafe)
  src/envelope.rs       call(), Envelope, ErrorKind mapping from SdkError
  src/ctx.rs            Ctx (deployment, state dir, wallet home) -> RpcChain + VerifiedConfig
  src/view.rs           view requests -> SDK views
  src/prepare.rs        prepare requests -> ops::prepare_* -> PlanJson
  src/track.rs          tx_begin / tx_poll / tx_end over confirm::track_*
  src/keysend.rs        fallback signer (Task F only)
  tests/*.rs
modules/auction_engine/
  flake.nix metadata.json CMakeLists.txt
  src/auction_engine_impl.{h,cpp}   LogosModuleContext impl (universal, Qt-free header)
  src/lez_args.{h,cpp}              Plan JSON -> lez_core send arguments (pure)
  tests/CMakeLists.txt tests/main.cpp tests/*_test.cpp tests/mocks/mock_auction_ffi.cpp tests/logos_sdk.h
apps/auctions/
  flake.nix metadata.json CMakeLists.txt icons/auctions.png
  src/AuctionsBackend.{rep,h,cpp} src/AuctionsPlugin.{h,cpp} src/AuctionsPluginInterface.h
  qml/Main.qml
  qml/shell/{Header,AccountPicker,TabStrip,TxDialog,TxLane,NodeNotice}.qml
  qml/common/{Format.js,KpiStrip,Fact,InsetCard,QuoteRows,StateNotice}.qml
  qml/live/{LivePage,AuctionList,AuctionDetail,DiscountChart,BidCard}.qml
  qml/risk/{RiskPage,PositionList,PositionDetail,CrGauge,LiquidationQuote}.qml
  qml/settled/{SettledPage,SettledList,SettledDetail,OutcomeBar,ClaimCard}.qml
  qml/system/{SystemPage,SystemList,AccountingDetail,SurplusDetail,DebtDetail,HostDetail}.qml
  qml/dev/{DevPage,DevList,OraclePriceDetail,TransferDetail,ProtocolTokenDetail}.qml
  tests/qml/tst_*.qml  tests/skeleton.mjs tests/story.mjs tests/lib/story-steps.mjs
flake.nix (root, new)   crane build of auction_ffi (pattern: lez-programs root flake mkHostFfi)
scripts/basecamp/{host.sh,run-app.sh,install-lgx.sh}
scripts/demo.sh (modify) --setup-only
scripts/record/{record-story.ps1,cut-idle.py,title-cards.txt}
docs/architecture/basecamp-app.md (new)
.github/workflows/core.yml (modify), .github/workflows/basecamp-module.yml (new)
```

---

## Phase A — walking skeleton: prove lez_core and the whole pipeline first

### Task 1: Host toolchain and the disk gate

**Files:**
- Create: `scripts/basecamp/host.sh`, `docs/architecture/basecamp-app.md`
- Modify: `.gitignore` (add `result-*`, `.logos-basecamp-dev/`)

**Interfaces:**
- Produces: `scripts/basecamp/host.sh build|paths|check`. `build` builds Basecamp `.#app-standalone` at `e09962f` into `~/lez-target/basecamp/app` and the lez_core install root (`nix build 'github:logos-blockchain/logos-execution-zone-module?rev=acf0cd50…#install'`) into `~/lez-target/basecamp/lez_core`; `paths` prints `BASECAMP_BIN=…` and `LEZ_CORE_ROOT=…` lines; `check` exits 2 if C: has under 10 GB free (`df --output=avail -B1G /mnt/c`).

- [ ] **Step 1:** Write `host.sh check` first and run it: expected `C: <N> GB free (min 10): ok` and exit 0 at the current ~28 GB; with `MIN_C_GB=1000` it must print `refusing: C: has <N> GB free, need 1000` and exit 2.
- [ ] **Step 2:** Add `build` (runs `check` before and after each `nix build`, `--option substituters` keeps the Logos cache from `~/.config/nix/nix.conf`) and `paths`. Run `scripts/basecamp/host.sh build`; expected: both result links exist and `check` still passes.
- [ ] **Step 3:** Launch smoke: `XDG_RUNTIME_DIR=/mnt/wslg/runtime-dir $BASECAMP_BIN --modules-dir $LEZ_CORE_ROOT --module <empty install root with plugins/>`; expected: window opens; `curl -s localhost:3768` (inspector) accepts a connection. Record the exact commands and versions in `docs/architecture/basecamp-app.md` ("Host").
- [ ] **Step 4:** Commit `chore: Basecamp master and lez_core build for the Auctions app`.

### Task 2: Non-blocking confirmation in the SDK

**Files:**
- Modify: `crates/auction_sdk/src/confirm.rs`
- Test: `crates/auction_sdk/src/confirm_tests.rs`

**Interfaces:**
- Produces:
  - `pub struct Tracking { pub signer: AccountId, pub nonce_before: u128, pub height_before: u64, pub lock_path: PathBuf }` (serde, so it can cross the FFI as JSON)
  - `pub fn track_begin<C: Chain>(chain: &C, signer: &AccountId, lock_dir: &Path, s: &ConfirmSettings) -> SdkResult<Tracking>` — acquires the lock with the current baseline (today's private `lock_with_current_baseline`), keeps the file on drop.
  - `pub enum Poll { Pending { blocks_waited: u64 }, Done(Outcome) }`; `pub fn poll_once<C: Chain>(chain: &C, t: &Tracking, s: &ConfirmSettings) -> SdkResult<Poll>` — one height read, one nonce read, the same rules as `confirm()` (height first, then nonce; Dropped after `drop_after_blocks`).
  - `pub fn track_end(t: &Tracking, outcome: Option<&Outcome>) -> SdkResult<()>` — removes the lock on `Some(Included|Dropped)`; on `None` (unknown) keeps it.
  - `submit` keeps its signature and behaviour, re-implemented as `track_begin` → sign → send → loop `poll_once` with `sleep`.

- [ ] **Step 1: Failing tests** in `confirm_tests.rs`: `poll_once_is_pending_until_the_nonce_moves` (FixtureChain, nonce unchanged, height +1 → `Pending{blocks_waited:1}`; nonce+1 → `Done(Included{nonce_after})`); `poll_once_reports_dropped_after_the_window` (`drop_after_blocks = 5`, height +6, nonce unchanged → `Done(Dropped{height})`); `track_begin_twice_for_one_signer_is_in_flight` (second call → `SdkError::InFlight`); `track_end_with_unknown_outcome_keeps_the_lock`.
- [ ] **Step 2:** Run `scripts/wsl-cargo.sh test -p auction_sdk --features testing confirm` — expected FAIL (unresolved `track_begin`).
- [ ] **Step 3:** Implement the four items; move `submit` onto them.
- [ ] **Step 4:** Run the same command — all of `confirm_tests.rs` (717 lines today) passes; then `scripts/wsl-cargo.sh test -p auction_cli --features auction_sdk/testing,auction_sdk/rpc` — every golden unchanged.
- [ ] **Step 5:** Commit `feat(sdk): non-blocking confirmation (track_begin, poll_once, track_end)`.

### Task 3: `auction_ffi` crate: ABI, envelope, context, tracking, token transfer

**Files:**
- Create: `crates/auction_ffi/{Cargo.toml,build.rs,cbindgen.toml,include/auction_ffi.h}`, `src/{lib.rs,envelope.rs,ctx.rs,prepare.rs,track.rs}`, `tests/{envelope.rs,track.rs,prepare_transfer.rs}`, `scripts/ci/ffi-header-drift.sh`
- Modify: root `Cargo.toml` (member), `.github/workflows/core.yml` (fmt/test/coverage lists, header-drift step), `scripts/check-coverage.py` (`CRATES` gains `auction_ffi`, 80 %)

**Interfaces:**
- Produces the C ABI (all take and return UTF-8 JSON; the caller frees with `auction_ffi_free`):
  ```c
  char* auction_ffi_version(const char* req);   // {"crate":"0.1.0","sdk":"…","lez":"v0.2.4"}
  char* auction_ffi_view(const char* req);      // Task 9
  char* auction_ffi_prepare(const char* req);
  char* auction_ffi_tx_begin(const char* req);
  char* auction_ffi_tx_poll(const char* req);
  char* auction_ffi_tx_end(const char* req);
  void  auction_ffi_free(char* s);
  ```
- Envelope: `{"ok":true,"value":…}` | `{"ok":false,"error":{"kind":"<snake_case SdkError variant | bad_request | panic>","message":"<Display>"}}`. A panic returns `kind:"panic"`; a null pointer or invalid UTF-8/JSON returns `bad_request`.
- Every request has `"ctx": {"deployment": "<path>", "state_dir": "<path>", "wallet_home": "<path>|null", "config": "<base58>|null"}`. `Ctx::open(&CtxJson) -> SdkResult<Ctx { chain: RpcChain, deployment: Deployment, state_dir: PathBuf }>` with the timeout/retry in Global Constraints; `Ctx::verified(&self) -> SdkResult<VerifiedConfig>`.
- Plan JSON (what the module sends): `{"program_id":"<program_id_hex>","accounts":[{"id":"<64 hex>","signer":bool}],"instruction_words":[u32,…],"signers":["<base58>"],"what":"<instruction name>","check":"<cli check command>"}`. Account ids are hex (lez_core form), signers base58 (lock names).
- `prepare` in this task supports only `{"action":"token_transfer","from":"<label|base58>","to":"…","amount":"<dec>"}` → `{"steps":[Plan],"facts":[]}` (uses `plan_host::token_transfer`; dev-network guard and `verify_chain_identity` as `dev token-transfer`).
- `tx_begin {ctx, signer}` → `Tracking` JSON; `tx_poll {ctx, tracking}` → `{"state":"pending","blocks_waited":n}` | `{"state":"included","nonce_after":"<dec>"}` | `{"state":"dropped","height":n}`; `tx_end {tracking, outcome:"included"|"dropped"|"unknown"}` → `{}`.

- [ ] **Step 1: Failing tests:** `envelope.rs`: `null_request_is_bad_request`, `invalid_json_is_bad_request`, `a_panic_is_caught_and_reported` (test-only op that panics), `sdk_errors_map_to_snake_case_kinds` (`SdkError::InFlight` → `"in_flight"`). `track.rs` (FixtureChain behind a `Chain` trait object injected through a test-only `Ctx::with_chain`): `tx_begin_twice_for_one_signer_is_in_flight`, `tx_poll_reports_included`. `prepare_transfer.rs`: `token_transfer_plan_matches_the_cli` (same accounts, words and program id as `plan_host::token_transfer` for the standard fixture; ids hex, signer flags `[true,false]`), `token_transfer_is_refused_off_dev_networks` (deployment network `"mainnet"` → kind `"mismatch"`, message names `DEV_NETWORKS`). Plus `view_against_a_closed_port_fails_within_12_s` (`ctx` deployment with `sequencer_url` `http://127.0.0.1:9` → `ok:false, kind:"chain"`, elapsed < 12 s).
- [ ] **Step 2:** Run `scripts/wsl-cargo.sh test -p auction_ffi` — expected FAIL (crate empty).
- [ ] **Step 3:** Implement. `Cargo.toml`: `crate-type = ["cdylib","rlib"]`, no `[lints] workspace = true`; `#![deny(unsafe_op_in_unsafe_fn)]`, `unsafe` only in `lib.rs`. cbindgen config as lez-programs `modules/amm/ffi/cbindgen.toml` with `include_guard = "AUCTION_FFI_H"`. `build.rs` writes `include/auction_ffi.h`.
- [ ] **Step 4:** Run the tests — PASS. Run `scripts/ci/ffi-header-drift.sh` (rebuilds, then `git diff --exit-code crates/auction_ffi/include`) — exit 0. Run `python3 scripts/check-coverage.py` after `cargo llvm-cov` as in `core.yml` — `auction_ffi` ≥ 80 %.
- [ ] **Step 5:** Commit `feat(ffi): auction_ffi crate with envelope, tracking and token transfer`.

### Task 4: `auction_engine` core module skeleton

**Files:**
- Create: `flake.nix` (root: crane package `auction_ffi`, pattern lez-programs root `mkHostFfi`, `RISC0_SKIP_BUILD=1`), `modules/auction_engine/{flake.nix,metadata.json,CMakeLists.txt}`, `src/auction_engine_impl.{h,cpp}`, `src/lez_args.{h,cpp}`, `tests/{CMakeLists.txt,main.cpp,lez_args_test.cpp,submit_test.cpp,mocks/mock_auction_ffi.cpp,logos_sdk.h}`

**Interfaces:**
- metadata: `name "auction_engine"`, `type "core"`, `interface "universal"`, `dependencies ["lez_core"]`, `external_libraries [{"name":"auction_ffi"}]`, `main "auction_engine_plugin"`.
- `struct LezArgs { std::vector<std::string> accountIds; std::vector<bool> signers; std::vector<uint8_t> instruction; std::string programIdHex; }`; `LezArgs toLezArgs(const nlohmann::json& plan)` (words → little-endian bytes, 4 per word).
- `class AuctionEngineImpl : public LogosModuleContext` (Qt-free header):
  - `LogosMap configure(const LogosMap& ctx)` — stores `ctx` (deployment, state_dir, wallet_home); returns `{network, dev, config}` from `auction_ffi_view {"view":"deployment"}` (Task 9 adds it; here returns network from the file).
  - `LogosMap openWallet()` — `lez_core.open(<home>/wallet_config.json, <home>/storage.json, <home>/statistics.json)`; non-zero but `list_accounts()` succeeds → `{opened:false, alreadyOpen:true}`; else `{error}`.
  - `LogosMap blockHeight()` — `lez_core.get_current_block_height()`.
  - `LogosMap submit(const LogosMap& req)` — `auction_ffi_prepare(req)` → for `steps[0]`: `tx_begin(signers[0])` → `toLezArgs` → `lez_core.send_generic_public_transaction(...)` → parse `{success, tx_hash, error}`; on `success:false` nothing reached the node, so release the lock with `tx_end` outcome `"dropped"` and return `{error}`; on success returns `{txId, hash, what, step:0, steps:n, tracking}`. Remaining steps kept in `m_pending[txId]`.
  - `LogosMap txStatus(const LogosMap& req)` — `tx_poll`; on `included` with steps left, submits the next step and returns `{state:"next", step}`; on final `included`/`dropped` calls `tx_end`; never sleeps.
- [ ] **Step 1: Failing tests** (logos-test CMock, pattern lez-programs `modules/stablecoin/tests`): `lez_args_test.cpp`: `words_become_little_endian_bytes` (`[1, 0x01020304]` → `01 00 00 00 04 03 02 01`), `signer_flags_follow_accounts`. `submit_test.cpp`: `submit_sends_one_step_and_returns_the_hash` (mock `auction_ffi_prepare`, `auction_ffi_tx_begin`; `mockModule("lez_core","send_generic_public_transaction").returns("{\"success\":true,\"tx_hash\":\"ab\",\"secrets\":[],\"error\":\"\"}")`; asserts `moduleCallCount == 1` and `hash == "ab"`), `a_failed_send_releases_the_lock` (`success:false` → `cFunctionCallCount("auction_ffi_tx_end") == 1`), `tx_status_submits_the_next_step_after_inclusion` (two steps; poll returns included → second send happens), `open_wallet_treats_already_open_as_success`.
- [ ] **Step 2:** Run `nix build ./modules/auction_engine#tests -L` (builder test output) — FAIL to compile.
- [ ] **Step 3:** Implement; FFI call helper as amm `call()` (dump, call, copy, `auction_ffi_free`, parse).
- [ ] **Step 4:** Run the tests — PASS; `nix build ./modules/auction_engine#install` — produces `modules/auction_engine/`.
- [ ] **Step 5:** Commit `feat(module): auction_engine core module skeleton on lez_core`.

### Task 5: `auctions` app skeleton (shell + one Dev tools action)

**Files:**
- Create: `apps/auctions/{flake.nix,metadata.json,CMakeLists.txt,icons/auctions.png}`, `src/AuctionsBackend.{rep,h,cpp}`, `src/AuctionsPlugin.{h,cpp}`, `src/AuctionsPluginInterface.h`, `qml/Main.qml`, `qml/shell/{Header,TabStrip}.qml`, `qml/dev/{DevPage,TransferDetail}.qml`, `qml/common/Format.js`

**Interfaces:**
- metadata: `name "auctions"`, `display_name "Auctions"`, `type "ui_qml"`, `view "qml/Main.qml"`, `dependencies ["lez_core","auction_engine"]`, `main "auctions_plugin"`.
- `.rep` (grows in later tasks; names fixed here): PROPs `QString network`, `bool devNetwork`, `bool walletOpen`, `QString activeAccount`, `int blockHeight`, `bool nodeReachable`, `QString lastError`; SLOTs `QVariantMap configure(QString deployment, QString stateDir, QString walletHome)`, `QVariantMap submit(QVariantMap request)`, `QVariantMap txStatus(QString txId)`, `QVariantMap view(QVariantMap request)`, `QVariantList accounts()`, `void setActiveAccount(QString label)`.
- Deployment path comes from env `AUCTIONS_DEPLOYMENT` (the run's filled deployment file, never `deployments/localnet.json`), state dir `AUCTIONS_STATE_DIR`, wallet home `LEE_WALLET_HOME_DIR`; the backend calls `configure` then `openWallet` at start (`QTimer::singleShot(0, …)`, amm pattern).
- QML: `readonly property var backend: logos.module("auctions")`; all calls through `logos.watch(promise, ok, err)`; header per boards (title 36, subtitle "Liquidation auctions · ● <network> · <account>"), five tabs in the right slot (`objectName`s `tabLive`, `tabRisk`, `tabSettled`, `tabSystem`, `tabDev`; `tabDev.visible: backend.devNetwork`). Dev tools → Token transfer form (`devTransferFrom`, `devTransferTo`, `devTransferAmount`, `devTransferSubmit`, `devTransferStatus`).
- `Format.js`: `amount(decString, decimals) -> string` with thousands separators, no `Number()` on u128.
- [ ] **Step 1:** Failing QML test `tests/qml/tst_format.qml`: `test_amount_keeps_all_digits` (`amount("2000000000000000000000000000",0)` → `"2,000,000,000,000,000,000,000,000,000"`), `test_amount_with_decimals`.
- [ ] **Step 2:** Run `nix build ./apps/auctions#tests -L` (or `qmltestrunner` per lez-programs VALIDATION.md with the 88330d04 design system on `QML2_IMPORT_PATH`) — FAIL.
- [ ] **Step 3:** Implement the skeleton; build `nix build ./apps/auctions#install`.
- [ ] **Step 4:** QML test PASS; app loads in standalone Basecamp (`scripts/basecamp/run-app.sh`, Task 6) showing the header and the five tabs (screenshot saved to `docs/architecture/img/skeleton.png`).
- [ ] **Step 5:** Commit `feat(app): Auctions app skeleton with header, tabs and a token transfer`.

### Task 6: Skeleton end-to-end on a localnet (the lez_core proof)

**Files:**
- Create: `scripts/basecamp/run-app.sh`, `apps/auctions/tests/skeleton.mjs`
- Modify: `scripts/demo.sh` (`--setup-only`: runs phases `dp_accounts … dp_positions`, writes the run deployment and keeps the sequencer running; prints `AUCTIONS_DEPLOYMENT=… LEE_WALLET_HOME_DIR=…`), `scripts/tests/demo-preflight.sh` (covers the flag), `docs/architecture/basecamp-app.md`

**Interfaces:**
- `run-app.sh [--setup] [--test FILE]`: with `--setup` runs `scripts/demo.sh --setup-only --sequencer-dev-mode`; launches `$BASECAMP_BIN --modules-dir $LEZ_CORE_ROOT --modules-dir <auction_engine install> --module <auctions install>` with the env above and `XDG_RUNTIME_DIR=/mnt/wslg/runtime-dir`; with `--test` runs `node FILE` against inspector 3768 and exits with its code.
- [ ] **Step 1:** Write `skeleton.mjs` (logos-qt-mcp `test`/`run`): open Dev tools, transfer `1` from `owner-stable` to `taker-stable`, `app.waitFor` until `devTransferStatus.text` is `"Done"` (timeout 120 s), then assert via `auction-cli dev balance --account taker-stable` (child process) that the balance rose by 1.
- [ ] **Step 2:** Run `scripts/basecamp/run-app.sh --setup --test apps/auctions/tests/skeleton.mjs` — expected PASS. This is the design's first risk (ADR-018): lez_core acf0cd50 loads in Basecamp master, accepts our plan encoding and the program id hex.
- [ ] **Step 3:** If it fails because lez_core cannot load or send in this Basecamp, stop and run **Task F** before going on; record the failure in `basecamp-app.md`. Otherwise record the passing commands.
- [ ] **Step 4:** Commit `test(app): skeleton e2e sends a token transfer through lez_core on a localnet`.

### Task F (only if Task 6 fails on lez_core): keystore signing fallback

**Files:** Create `crates/auction_ffi/src/keysend.rs`, `tests/keysend.rs`; modify `src/lib.rs`, module `submit`, app header.

**Interfaces:** `char* auction_ffi_send_with_keystore(const char* req)` — `{ctx, plan}` → signs with `Keystore::open(<wallet_home>/storage.json)` + `tx::sign`, sends with `RpcChain::send`, returns `{hash, tracking}` (lock taken before signing, as `submit`). Module `submit` uses it when `configure` returned `{signing:"keystore"}` (set when `lez_core` is absent from the loaded modules); the header shows a LogosBadge "Signing: local keystore".
- [ ] Step 1 failing test `keysend_signs_with_the_wallet_key_and_tracks` (FixtureChain send mode); Step 2 run FAIL; Step 3 implement; Step 4 PASS + skeleton e2e PASS with the fallback; Step 5 commit `feat(ffi): keystore signing fallback when lez_core is unavailable`.

---

## Phase B — one source of truth for the story's numbers

### Task 7: `auction_sdk::ops`: the CLI's orchestration moves into the SDK

**Files:**
- Create: `crates/auction_sdk/src/ops/{mod.rs,liquidate.rs,bid.rs,claim.rs,system.rs,admin.rs,dev.rs}`, `crates/auction_sdk/src/ops_tests.rs`
- Modify: `cli/src/{tx_cmds.rs,admin_cmds.rs,dev_cmds.rs,oracle_cmds.rs,tx_report.rs,env.rs}`

**Interfaces:**
- `pub struct Fact { pub key: &'static str, pub value: String }` — the CLI's `kv` lines before `plan`, in order (e.g. `quoted out`, `min out`).
- `pub enum Touch { Holding { label: String, id: AccountId, definition: AccountId }, Auction(AccountId), Accounting }` (moved from `cli/src/tx_report.rs`).
- `pub struct Prepared { pub steps: Vec<Step>, pub facts: Vec<Fact>, pub touched: Vec<Touch> }`; `pub struct Step { pub plan: Plan, pub what: &'static str, pub check: &'static str }`.
- `pub struct Accounts { pub resolve: &dyn Fn(&str) -> SdkResult<AccountId> }` — label/base58 resolution injected (CLI: keystore; FFI: keystore labels, Task 8).
- One function per command, each `(chain: &C, v: &VerifiedConfig, req: &<Cmd>Request, now_ms: u64) -> SdkResult<Prepared>`: `prepare_liquidate`, `prepare_bid` (auto `bid_with_settlement` via `needs_settlement(slippage_bps)`, `min_out` default `quote::min_out(collateral_out, bps)`, re-quote when `min_out` given), `prepare_claim_leftover`, `prepare_settle_empty`, `prepare_settle_debt`, `prepare_check_solvency`, `prepare_observe_feed`, `prepare_start_surplus` (refuses while live, same message), `prepare_take_surplus`, `prepare_close_surplus`, `prepare_start_debt` (live and ADR-039 delay refusals), `prepare_take_debt`, `prepare_close_debt`, `prepare_set_engine_params` (incl. `disable_protocol_token`), `prepare_token_transfer`, `prepare_oracle_publish` (three steps: create, rotate, follow; `free_oracle_window` + `check_new_price` first). Request structs carry exactly the CLI flags (`cli/src/args.rs`).
- [ ] **Step 1: Failing tests** in `ops_tests.rs`, one per function, asserting the `Plan` equals the plan the current CLI builds for the same fixture input and the facts list equals the golden's pre-`plan` lines (e.g. `prepare_bid_final_is_bid_with_settlement` with `quoted out 342956`, `min out 341241` from `cli/tests/golden/bid_final.txt`; `prepare_oracle_publish_has_three_steps_with_three_signers`; `prepare_start_surplus_refuses_while_live`).
- [ ] **Step 2:** Run `scripts/wsl-cargo.sh test -p auction_sdk --features testing ops` — FAIL.
- [ ] **Step 3:** Implement by moving code out of `cli/src/tx_cmds.rs` etc.; the CLI prints `facts`, calls `env.send` per step, prints `touched` with the existing formatter.
- [ ] **Step 4:** `scripts/wsl-cargo.sh test -p auction_sdk -p auction_cli --features auction_sdk/testing,auction_sdk/rpc` — every golden byte for byte unchanged (no `UPDATE_GOLDEN`); `RISC0_SKIP_BUILD=1 scripts/wsl-cargo.sh clippy --workspace --all-targets --exclude integration_tests -- -D warnings` clean.
- [ ] **Step 5:** Commit `refactor(sdk): move CLI orchestration into auction_sdk::ops`.

### Task 8: SDK views the boards need

**Files:**
- Create: `crates/auction_sdk/src/views.rs`, `crates/auction_sdk/src/views_tests.rs`
- Modify: `scan.rs` (`scan_page`, `Cursor.bids`), `keystore.rs` (`labels`), `quote.rs` (`cr_bps`, `discount_schedule`), `cli/src/read_cmds.rs` (`list-auctions`, `list-at-risk`, `config`, `host status` print from views; goldens unchanged)

**Interfaces:**
- `pub fn scan_page<C: Chain>(chain: &C, t: &ScanTargets, cursor: &Cursor, s: &ScanSettings, max_blocks: u64) -> SdkResult<ScanPage { scanned: Scanned, head: u64, done: bool }>`; `scan_report` becomes a loop over it.
- `Cursor.bids: BTreeMap<String, u32>` with `#[serde(default)]` (old cursor files still load): incremented for every included engine tx whose variant is `bid`/`bid_with_settlement` (variant indices from `encode`).
- `impl Keystore { pub fn labels(&self) -> impl Iterator<Item = (&str, &AccountId)> }`.
- `PositionHealth.cr_bps: Option<u64>` — collateral value over nominal debt, using the same price the host's min-CR check uses (`None` for zero debt). Test pins it to the host: `cr_bps < min_cr_bps` exactly when `unsafe_at_min_cr`.
- `pub fn discount_schedule(state: &AuctionState, samples: usize) -> Vec<(u64 /*t_ms from start*/, u128 /*discount_ray*/)>` from `auction_core`'s own discount function.
- Views (structs, `serde::Serialize`, u128 as strings via `#[serde(with = "dec")]`):
  - `AccountingView { buffer, unbacked_debt, debt_on_auction, freeze_threshold, surplus_live, surplus_nonce, debt_live, debt_nonce, surplus_lot_left, debt_lot_left, earliest_debt_start_ms }`
  - `HostView { frozen, admin, freeze_authority, market_oracle, min_cr_ray, max_oracle_age_ms }`
  - `OracleView { price_ray, age_ms, max_age_ms, fresh, range_lo_ray, range_hi_ray }`
  - `LiveAuctionView { id, lot, raised, target, collateral_left, discount_ray, rate_per_min_ray, t_eff_ms, ends_at_ms, max_at_ms, held_at_max, paused, bids, position, started_at_ms }`
  - `SettledAuctionView { id, lot, sold, raised, target, debt_repaid, penalty, shortfall, leftover, claimed, owner, position, ended_at_ms, bids }`
  - `AtRiskView { position, owner, collateral, nominal_debt, cr_bps, min_cr_bps, liquidatable }`, sorted lowest `cr_bps` first.
  - `AccountView { label, id, role, stable_balance, coll_balance, holding }` — roles from the label convention (`owner`, `keeper`, `caller`, `taker`, `admin`, `engine-admin`, `oracle-src`); `<role>-stable`/`<role>-coll` holdings attached to their role, not listed alone.
- [ ] **Step 1: Failing tests** (`views_tests.rs`, standard fixture): `scan_page_stops_after_max_blocks` (`done:false` with head 300, `max_blocks 100`), `scan_page_reports_a_replaced_chain` (anchor hash changed → `restarted: Some(ChainReplaced)`), `old_cursor_files_load_without_bids`, `bids_are_counted_per_auction`, `labels_lists_every_wallet_label` (14 `STANDARD_LABELS`), `cr_bps_agrees_with_the_host_check`, `discount_schedule_matches_the_engine` (2 % at 0, 10 % at 45 min, flat to 2 h for the demo schedule), `at_risk_is_sorted_lowest_cr_first`, `settled_view_splits_debt_penalty_shortfall` (sums to raised), `accounts_attach_holdings_to_roles`.
- [ ] **Step 2:** Run `scripts/wsl-cargo.sh test -p auction_sdk --features testing views` — FAIL.
- [ ] **Step 3:** Implement; the CLI read commands print from these views.
- [ ] **Step 4:** SDK + CLI tests PASS, goldens unchanged (`list_at_risk.txt` order stays cursor order: the CLI sorts its own output back, or the view exposes both orders — keep the golden).
- [ ] **Step 5:** Commit `feat(sdk): views for the Auctions app (paged scan, CR, schedule, accounting, settled)`.

### Task 9: FFI `view` and `prepare` for the whole story

**Files:**
- Create: `crates/auction_ffi/src/view.rs`, `tests/{view.rs,prepare_all.rs,parity.rs}`
- Modify: `src/prepare.rs`, `src/lib.rs`, header (regenerated)

**Interfaces:**
- `auction_ffi_view {ctx, view, …}` with `view` ∈ `deployment` (`{network, dev, config, genesis_ok}` — `dev` true only for `localnet`/`testnet-legacy` *and* `verify_chain_identity` ok), `overview` (`{block, accounting: AccountingView, host: HostView, oracle: OracleView, counts:{live, at_risk, settled}}`), `scan` (`{max_blocks}` → `{scanned_to, head, done, restarted}`; one `scan_page`, cursor saved under `state_dir`), `live`, `settled`, `at_risk`, `auction` (`{id}` → `LiveAuctionView | SettledAuctionView` + `schedule: [[t_ms, discount_ray]]` 48 samples), `position` (`{id}` → `AtRiskView` + liquidation quote), `accounts`, `quote_bid` (`{auction, coin_in_max, slippage_bps}`), `quote_liquidation`, `quote_take_surplus`, `quote_take_debt`, `quote_settle_debt` (min(buffer, unbacked) and after values), `quote_price` (`{price}` → `check_new_price` verdict + `[{position, cr_bps_now, cr_bps_after}]`).
- `auction_ffi_prepare {ctx, action, …}` with `action` ∈ every `ops::prepare_*` (snake_case names: `liquidate`, `bid`, `claim_leftover`, `observe_feed`, `settle_debt`, `check_solvency`, `start_surplus`, `take_surplus`, `start_debt`, `take_debt`, `token_transfer`, `oracle_publish`, `disable_protocol_token` = `set_engine_params` with `demo::engine_params()` and the token disabled). Returns `{steps:[Plan], facts:[{key,value}], touched:[…]}`.
- [ ] **Step 1: Failing tests:** `view.rs`: one test per view on the standard fixture, `prices_and_amounts_are_decimal_strings` (every u128 field is a JSON string), `dev_is_false_without_a_genesis_hash`. `prepare_all.rs`: one test per action that the JSON plan equals `ops::prepare_*`'s plan. `parity.rs`: `quote_bid_view_equals_the_cli_quote` (same fixture → the numbers in `cli/tests/golden/quote_bid.txt`), `overview_accounting_equals_the_cli_config_line`.
- [ ] **Step 2:** Run `scripts/wsl-cargo.sh test -p auction_ffi` — FAIL.
- [ ] **Step 3:** Implement as thin mappings; no logic beyond request parsing.
- [ ] **Step 4:** Tests PASS; header-drift script exit 0 (header changes only if a new symbol is added — none here); coverage ≥ 80 %.
- [ ] **Step 5:** Commit `feat(ffi): views and actions for the demo story`.

---

## Phase C — the screens, each extending the story e2e

Every screen task: build the board's main screen and its state strip in QML, give each interactive item an `objectName`, add QML unit tests with a fake backend (`QtObject` with the slot functions returning fixture JSON from `apps/auctions/tests/qml/fixtures/*.json`, generated in Task 10 from the FFI on the standard fixture so numbers are real), and append the screen's story segment to `apps/auctions/tests/lib/story-steps.mjs`. Visual check: screenshot via `app.screenshot()` next to the board PNG in the task's commit message body (path only).

### Task 10: Module full API, fixtures, CI for module tests

**Files:**
- Modify: `modules/auction_engine/src/auction_engine_impl.{h,cpp}`, tests
- Create: `modules/auction_engine/tests/view_test.cpp`, `scripts/basecamp/gen-qml-fixtures.sh`, `apps/auctions/tests/qml/fixtures/*.json`, `.github/workflows/basecamp-module.yml`

**Interfaces:**
- `LogosMap view(const LogosMap& req)` (pass-through to `auction_ffi_view` with stored ctx); `LogosList accounts()` (`view accounts`, each `{label, id, role, stable_balance, coll_balance, holding}`); `LogosMap rescan(const LogosMap& req)` (one page); `submit`/`txStatus` from Task 4 extended to multi-step (`oracle_publish` 3 steps, each its own tracking, one in flight per signer).
- `gen-qml-fixtures.sh` runs a test binary (`cargo test -p auction_ffi --test fixtures -- --ignored`) that writes each view's JSON for the standard fixture into `apps/auctions/tests/qml/fixtures/`.
- `basecamp-module.yml`: on push/PR touching `crates/auction_ffi/**`, `modules/**`, `apps/**`, `flake.*`: `nix build ./modules/auction_engine#tests -L` and the app QML tests; uses `cachix/install-nix-action` and the Logos binary cache.
- [ ] Step 1 failing tests: `view_passes_ctx_and_request` (`cFunctionCalledWith`), `accounts_maps_the_view`, `rescan_is_one_page`, `oracle_publish_runs_three_steps_in_order`, `a_dropped_middle_step_stops_the_sequence` (state `dropped`, remaining steps discarded, `tx_end` called once per begun step).
- [ ] Step 2 run `nix build ./modules/auction_engine#tests -L` FAIL; Step 3 implement; Step 4 PASS, fixtures generated and committed; Step 5 commit `feat(module): full auction_engine API, QML fixtures, module CI`.

### Task 11: Shell: header, account picker, confirmation dialog, lane, node notice

**Files:** Create `qml/shell/{Header,AccountPicker,TxDialog,TxLane,NodeNotice}.qml`, `qml/common/{KpiStrip,Fact,InsetCard,QuoteRows,StateNotice}.qml`, `tests/qml/tst_{header,picker,txflow}.qml`; modify `Main.qml`, backend (`activeAccount`, `accounts()`, block poll every 5 s via `QTimer` → `view overview`).

**Interfaces:**
- `AccountPicker` per v5 board (popup `LogosComboBox`-style; rows label, short id, role text, stable balance, holding; dev keys `engine-admin`/`oracle-src` omitted with the board's footnote); `objectName`s `accountPicker`, `accountOption_<label>`.
- `TxDialog { property var request; property var quote; signal confirmed() }` — v4 public dialog layout (You pay → You get, rows, "What this bid reveals" with the private column showing "Nothing. Private bids are later work (M4)."); generic for every action (title from `request.action`, rows from `facts`); Cancel focused, Esc cancels; `objectName` `txConfirm`, `txCancel`.
- `TxLane { property string txId }` — `LogosStageLane` stages Submitted → Including… → Done; failed → "Not included after N blocks" + `Re-quote` (`objectName` `txRequote`); unknown → "Outcome unknown, lock kept: check the nonce" (Review Focus 1); polls `txStatus` every 2 s; multi-step actions show one stage per step. `objectName` `txLane`, state in `txLane.state` (`waiting|done|dropped|unknown`).
- `NodeNotice` — LogosNotice Warning "Node not answering" when `nodeReachable` is false.
- [ ] Step 1 failing QML tests: `tst_header_shows_node_unreachable`, `tst_picker_lists_roles_and_hides_dev_keys`, `tst_lane_survives_account_switch` (switch account mid-lane: lane keeps its signer and state), `tst_lane_shows_unknown_not_dropped`, `tst_format_amount_keeps_all_digits` (moved here from Task 5 if already present, else extend).
- [ ] Step 2 run FAIL; Step 3 implement; Step 4 PASS + screenshot of header with the open picker matches v5 "Account picker"; Step 5 commit `feat(app): header, account picker, confirmation dialog and transaction lane`.

### Task 12: Live auctions (v3)

**Files:** Create `qml/live/{LivePage,AuctionList,AuctionDetail,DiscountChart,BidCard}.qml`, `tests/qml/tst_live.qml`; modify `tests/lib/story-steps.mjs`.

**Interfaces:**
- List "Ending soonest" (`liveList`, delegates `liveItem_<id>`), detail KPI strip (discount with rate per min, time left with end, raised/target with bar, collateral left), `DiscountChart` (QtQuick.Shapes; axes 0–10 %, 0–2 h, max reference, knee "Max at hh:mm", expiry, elapsed solid, ahead dashed, "Now x.x%" pin) from `schedule`, facts grid, `BidCard` (oracle price + FRESH/STALE badge + freshness bar; You pay field `bidAmount`, Min, "Max to finish" link `bidMax`; You get (`bidYouGet`) with slippage floor and Edit; Private bid `LogosSwitch` disabled with "Public transactions only" (`bidPrivate`); `bidSubmit`). States: oracle stale / paused (button disabled "Bidding paused"), over the max (error field + "Use N").
- Story steps added: `bidPartial(auction, 100000)`, `bidFinal(auction, 230000)` (confirmation shows `bid_with_settlement` in its facts).
- Rescan link `liveRescan` → `rescan` pages until `done`; on `restarted` the list empties (Review Focus 5).
- [ ] Step 1 failing tests: `tst_list_sorted_ending_soonest`, `tst_chart_now_pin_matches_discount` (pin y equals `discount_ray` mapping), `tst_max_to_finish_fills_left_to_raise`, `tst_over_the_max_shows_use_n`, `tst_stale_oracle_disables_bid`, `tst_rescan_after_restart_clears_list`.
- [ ] Step 2 FAIL; Step 3 implement; Step 4 PASS + screenshot vs `ui-v3.png`; Step 5 commit `feat(app): Live auctions screen and bid flow`.

### Task 13: At-risk positions (v4)

**Files:** Create `qml/risk/{RiskPage,PositionList,PositionDetail,CrGauge,LiquidationQuote}.qml`, `tests/qml/tst_risk.qml`; modify story steps.

**Interfaces:** list "Lowest CR first" with "Scanned to block N · Rescan" (`riskList`, `riskItem_<id>`, `riskRescan`); detail with CR gauge (100–250 %, threshold tick), facts, "When you liquidate" list; quote card (debt, penalty, target, seized, keeper reward with the formula note, goes to auction, network fee, caps bars, "Will end short" line when lot value < target) and `liquidateSubmit`. States: healthy (disabled + reason), oracle stale, out of range, paused auction. Story step `liquidate(position)` as keeper.
- [ ] Step 1 failing tests: `tst_sorted_lowest_cr_first`, `tst_healthy_position_cannot_be_liquidated`, `tst_out_of_range_feed_pauses`, `tst_will_end_short_shown_when_lot_below_target`, `tst_keeper_reward_from_quote`.
- [ ] Steps 2–5 as Task 12; commit `feat(app): At-risk positions screen and liquidation`.

### Task 14: Settled (v5)

**Files:** Create `qml/settled/{SettledPage,SettledList,SettledDetail,OutcomeBar,ClaimCard}.qml`, `tests/qml/tst_settled.qml`; modify story steps.

**Interfaces:** list "Most recent" with outcome text ("x to claim" accent, "n short" error, "Claimed"); detail KPIs (raised/target, collateral sold, last discount, ran for), "Where the N went" bar (debt repaid, penalty to buffer, shortfall) + legend, facts, `ClaimCard` (`claimSubmit`) enabled only when the active account is the position owner and leftover > 0. States: not the owner (reason names the owner label), claimed (block), ended short (notice + `openSystemLink` → System tab, Accounting), nothing left. Story step `claimLeftover(auction)` as owner.
- [ ] Step 1 failing tests: `tst_outcome_bar_sums_to_raised`, `tst_claim_disabled_for_non_owner`, `tst_shortfall_links_to_system`, `tst_claimed_shows_block`.
- [ ] Steps 2–5; commit `feat(app): Settled screen and claim leftover`.

### Task 15: System (v5)

**Files:** Create `qml/system/{SystemPage,SystemList,AccountingDetail,SurplusDetail,DebtDetail,HostDetail}.qml`, `tests/qml/tst_system.qml`; modify story steps, `Header.qml` (Frozen badge on every tab when `host.frozen`).

**Interfaces:** list Accounting / Surplus auction / Debt auction / Host with verdict text; Accounting: KPIs (buffer, unbacked debt, debt on auction, freeze threshold), "Can the buffer cover it?" two bars + threshold tick, Upkeep rows `observeFeed`, `checkSolvency` (secondary 32 px), Settle debt quote card `settleDebtSubmit` (quote from `quote_settle_debt`). Surplus: `startSurplus`, take card (`takeSurplusAmount`, `takeSurplusSubmit`, quote from `quote_take_surplus`). Debt: `startDebt` (disabled with the ADR-039 delay reason until `earliest_debt_start_ms`), take card (`takeDebtSubmit`). Host: status facts; frozen → LogosNotice Error "New debt is disabled". Story steps: `observeFeed`, `settleDebt`, `checkSolvency`, `startSurplus`, `takeSurplus(500)` as taker, `startDebt`, `takeDebt(5000)` as taker.
- [ ] Step 1 failing tests: `tst_settle_debt_quote_is_min_of_buffer_and_unbacked`, `tst_start_debt_waits_for_relist_delay`, `tst_frozen_badge_in_header`, `tst_take_surplus_shows_burn_note`.
- [ ] Steps 2–5; commit `feat(app): System screen (accounting, surplus and debt auctions, host)`.

### Task 16: Dev tools (v5) and the complete story e2e

**Files:** Create `qml/dev/{DevList,OraclePriceDetail,ProtocolTokenDetail}.qml`, `tests/qml/tst_dev.qml`, `apps/auctions/tests/story.mjs`; modify `DevPage.qml`, `TransferDetail.qml`, story steps.

**Interfaces:** Oracle price: KPIs (current, age, allowed range, positions), "What a new price does", "At <price>" affected list from `quote_price`, price field `devPrice` (error style out of range, "Publish anyway" secondary), `devPricePublish` (three-step lane), "Re-publish current" (`devPriceRepublish`, the keeper refresh). Token transfer (from Task 5, restyled to the v5 card). Protocol token: error notice + `devDisableProtocolToken` (red-outlined secondary; dialog repeats the warning). Tab hidden unless `deployment.dev`.
- `story.mjs` runs, in order, as UI actions with the matching account picked: liquidate P0 (keeper) → bid 100000 then 230000 (owner) → claim leftover (owner) → observe feed, settle debt, check solvency, start surplus (caller) → take surplus 500 (taker) → publish price ×1.9 (admin; signers oracle-src, admin, engine-admin) → liquidate P2 (keeper) → bid 330000 (owner) → settle debt, start debt (caller) → transfer 5000 owner-stable → taker-stable → take debt 5000 (taker) → disable protocol token (engine-admin) → check solvency (caller) → header shows Frozen. After each step it asserts the screen's figures against ADR-051's expected values (buffer 30000 after F1–F3, 29500 after the surplus take, unbacked after the shortfall in [42154, 63200], protocol tokens minted > 9800, one lot burned, frozen yes), read from the UI (`objectName`s) and cross-checked with `auction-cli config`.
- [ ] Step 1 failing tests: `tst_dev_hidden_off_dev_networks`, `tst_affected_positions_from_quote`, `tst_out_of_range_price_warns`, `tst_disable_token_needs_confirmation`; and `story.mjs` (fails at the first missing step until done).
- [ ] Step 2 FAIL; Step 3 implement; Step 4: QML tests PASS and `scripts/basecamp/run-app.sh --setup --test apps/auctions/tests/story.mjs` PASS on a fresh localnet (record wall time in `basecamp-app.md`); Step 5 commit `feat(app): Dev tools screen; the whole demo story runs in Basecamp`.

---

## Phase D — installed module, recording, evidence

### Task 17: Installed module in a clean Basecamp profile

**Files:** Create `scripts/basecamp/install-lgx.sh`; modify `docs/architecture/basecamp-app.md`, `README.md` (Basecamp section), `DEMO.md` (GUI run).

**Interfaces:** `install-lgx.sh PROFILE_DIR` builds `.#lgx` for lez_core acf0cd50, `modules/auction_engine` and `apps/auctions`, then `lgpm --modules-dir PROFILE_DIR/modules install --file …` (core first) and `lgpm --ui-plugins-dir PROFILE_DIR/plugins install --file …`; launches the full Basecamp (`.#app`) with `--user-dir PROFILE_DIR`.
- [ ] Step 1: run `install-lgx.sh /tmp/auctions-profile`, then `run-app.sh`-equivalent with the full app and `story.mjs` — PASS. A profile with a newer catalog `lez_core` is documented as unsupported (design call 3).
- [ ] Step 2: commit `feat: install the Auctions app as .lgx packages in a clean Basecamp profile`.

### Task 18: The recorded session

**Files:** Create `scripts/record/record-story.ps1`, `scripts/record/cut-idle.py`, `scripts/record/title-cards.txt`; modify `DEMO.md` (how to re-record).

**Interfaces:** `record-story.ps1 -Out C:\Users\user\Videos\rfp014-demo\basecamp.mp4` starts Windows ffmpeg `gdigrab` on the WSLg Basecamp window title, runs `run-app.sh --setup --test apps/auctions/tests/story.mjs` with `STORY_PACE_MS=1500` (story.mjs waits that long after each visible change so viewers can read), stops ffmpeg, then `cut-idle.py` removes stretches where `txLane.state == waiting` longer than 3 s (timestamps logged by story.mjs to `story-timeline.json`) and overlays the title cards (F1–F5, shortfall, freeze) at the logged step starts.
- [ ] Step 1: record; expected a 4–6 min `basecamp.mp4` with no terminal on screen; spot-check every title card against its step.
- [ ] Step 2: commit the scripts `feat: scripted recording of the Basecamp demo story` (the video itself is not committed).

### Task 19: Requirements, submission, docs

**Files:** Modify `REQUIREMENTS.md` (U2 mini-app loadable in Basecamp → PoC with `apps/auctions`, `story.mjs`; Plan 4 rows get their tests), `SUBMISSION.md` (honest notes + `scripts/submission-hashes.sh`), `docs/architecture/{basecamp-app.md,sdk.md,ci.md}`, `docs/design-log.md` (ADR for: reads in Rust / signing in lez_core; polling confirmation; design-system pin; generic `view`/`prepare` ABI).
- [ ] Step 1: `python3 scripts/check-requirements.py REQUIREMENTS.md` PASS; CI green on the branch (`core`, `basecamp-module`).
- [ ] Step 2: commit `docs: Plan 4 evidence (Basecamp app, story e2e, recording)`.
