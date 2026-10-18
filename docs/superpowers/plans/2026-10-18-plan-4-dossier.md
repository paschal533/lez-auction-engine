# Plan 4 dossier (research for the implementation plan)

Repo: `C:\Users\user\Documents\basecamp\lez-auction-engine` (= `/mnt/c/Users/user/Documents/basecamp/lez-auction-engine` in WSL), HEAD `dc47f75` (docs: Plan 4 design). Paths below are relative to the repo root unless absolute. Design doc: `docs/superpowers/specs/2026-10-17-plan-4-basecamp-app-design.md` (164 lines). PoC spec: `docs/superpowers/specs/2026-10-01-rfp014-auction-engine-poc-design.md` (§4.1 layers :125, §6.2 FFI :407, §6.3 module :413, §9 testing :463).

## 0. Contradictions / surprises (read first)

1. **`scan::scan_report` is not paged.** `crates/auction_sdk/src/scan.rs:280-330` loops `while from <= head` in `batch`-sized `chain.blocks` calls until the head. One call = whole backlog. Design (call 1, "one page per call") needs a new SDK fn (e.g. `scan_page(chain, t, cursor, s, max_blocks) -> Scanned + done:bool`) or a `ScanSettings.max_blocks` cap. CLI `scan_now` (cli/src/read_cmds.rs:137) must keep current behaviour (golden `rescan.txt`, `list_auctions.txt`).
2. **No label listing in the keystore.** `Keystore` (crates/auction_sdk/src/keystore.rs:48-53) has private `labels: BTreeMap<String, AccountId>`; public API is only `default_path`, `path_from`, `open`, `parse`, `resolve(label_or_id)`, `key(id)` (:81-224). The account picker needs a new `pub fn labels(&self) -> impl Iterator<Item=(&str, &AccountId)>` (or ask lez_core for labels; see §5).
3. **Signing via lez_core vs SDK confirm.** `confirm::submit` (confirm.rs:435) = lock + `tx::sign` with a `KeySource` + `chain.send` + blocking `confirm` loop (default 2 s x 900 polls). If lez_core signs and sends, the module can still reuse the SDK pieces that are not signing: `InFlightLock::acquire` (confirm.rs:255), `lock_with_current_baseline` (private, :400) and `confirm()` (:79, pure over `Chain` + injected sleeper). These must become pub/non-blocking: split into `begin_submit(chain, signers, lock_dir, s) -> (InFlightLock, nonces, h0)` and `poll_once(chain, signer, n0, h0, s) -> Option<Outcome>` so the FFI never blocks for a confirmation (20 s IPC timeout). The lock must be held across FFI calls (lock file on disk + `keep_on_drop`), so the FFI needs a handle table or "re-open lock by path" API.
4. **"freshness" in the design's sequence** (quote → min-out → needs-settlement → freshness → plan) is, in the CLI, `env.is_fresh(holding)` = recipient holding is `Account::default()` (env.rs:84). It is not oracle freshness. Oracle freshness for the bid card is not computed by any CLI command; `quote::auction_view` gives `paused: Option<String>` (quote.rs:362) and `dev keeper` computes oracle age (cli/src/oracle_cmds.rs:77-110: `age = chain_now - oracle.timestamp` vs `coll.max_oracle_age_ms`). A view op must expose `oracle_age_ms` / `max_oracle_age_ms`.
5. **Workspace lint `unsafe_code = "deny"`** (Cargo.toml `[workspace.lints.rust]`), and `auction_sdk`/`auction_cli` have `#![forbid(unsafe_code)]`. `auction_ffi` needs `extern "C"` + raw pointers: do not inherit `[lints] workspace = true`, or `#![allow(unsafe_code)]` scoped to the ABI module. Also `fuzz/` notes `#[no_mangle] extern "C"` cannot carry forbid (docs/architecture/sdk.md Fuzzing).
6. **`RpcChain` must not run inside a tokio runtime** (sdk.md RpcChain; rpc.rs refuses with `Chain` error). The FFI is called from the C++ module thread: fine, but any async Rust layer in the FFI is forbidden. `RpcChain` retries reads for up to 30 s (`ReadRetry::DEFAULT`, 5 tries, 2/4/8/16 s) and has a 30 s overall request timeout (`DEFAULT_TIMEOUT`, rpc.rs:28): **a single FFI read can exceed the 20 s IPC timeout** when the node is down. FFI should construct `RpcChain::connect_with_timeout(url, ~5 s)` and `.with_read_retry(<short>)` (rpc.rs:127, :151).
7. **Freeze trigger is an admin command with a params file**, not a dev command: `admin set-engine-params --admin engine-admin --params cli/params/demo-engine.json --disable-protocol-token` then `check-solvency --caller caller` (scripts/lib/demo-scenario.sh:131-132). SDK already has `demo::engine_params()` (crates/auction_sdk/src/demo.rs:20) and `demo::collateral_args()` (:30), so the FFI need not read files.
8. **Oracle publish = 3 transactions, 3 signers**: `create_oracle_price_account` (signer price-source `oracle-src`), `set_market_price_oracle` (host admin `admin`), `set_collateral_config` (engine admin `engine-admin`) (cli/src/oracle_cmds.rs:22-75). The UI lane needs a multi-step lane, each step its own confirmation.
9. **Coverage gate** `scripts/check-coverage.py:12-18` `CRATES` dict lists 5 crates; `auction_ffi` must be added (80 % threshold, :19) and to `cargo llvm-cov -p ...` in core.yml `coverage`, `cargo fmt -p ...` in `core`, and `cargo test -p ...` in `programs`.
10. **RAM**: Windows host 15.8 GB physical; WSL sees 7.8 GiB + 16 GiB swap (WSL default 50 %). Design says "8 GB RAM" (= WSL view). A `.wslconfig` `memory=12GB` would help Qt/Basecamp Nix builds.
11. `deployments/localnet.json` has `"config": null`, `"engine_admin": null`, `"chain_genesis_hash": null`; demo runs write a filled run deployment. Dev tools (ADR-046) refuse sends without `chain_genesis_hash` (`verify::verify_chain_identity`, verify.rs:188). The app must load the run's deployment file (path from module config), not the committed one.

### Cross-cutting flags from §4-§7 (external repos)
12. **lez_core = `logos-blockchain/logos-execution-zone-module` @ `acf0cd501b262c4c15969e3735e85318297b85bf`** (not logos-co). metadata 0.4.1 but a "0.4.1-interim" build (lez-programs flake.nix:36-58), not an ancestor of `main`, no tags; `version()` returns `"0.3.0"`. Built on LEZ `9edf4a62` = v0.2.4-1 (only a nix risc0 fetch fix). Design's "v0.4.1 on LEZ v0.2.4": OK with this qualification. lez-programs `modules/stablecoin/flake.nix:6` floats on `?ref=byte-string-fix` (b60be46): do not copy that.
13. **`send_generic_public_transaction(vector<string> account_ids /*hex32*/, vector<bool> signing_requirements /*parallel per account*/, vector<uint8_t> instruction /*LE bytes of u32 words*/, string program_id_hex)`** returns a JSON **string** `{"success":bool,"tx_hash":str,"secrets":[...],"error":str}` (lez_core_module.cpp:973-1047, :134-148), not a bare hash. Our `Plan` must be mapped: account ids → **hex** (not base58; `account_id_from_base58` exists), `signing_requirements` → bool per account (`plan.account_ids[i] ∈ plan.signing_requirements`), words → LE bytes, `program_id_hex(&ProgramId)` (deployment.rs:47, 64 hex LE limbs) — check it equals lez_core's expected program-id hex form (amm passes the same kind of string). The wallet chooses nonces and signs; **our SDK `tx::sign` and keystore are bypassed**, so the `submit` lock baseline must be read before the send (nonce via `RpcChain::nonces` or `get_account_public(..).nonce` hex LE16).
14. **No events, no threads anywhere** in lez_core or amm_module: every method is synchronous; QML gets async only from `logos.watch(promise, ok, err)`. So "event vs polling" → polling: `submit` returns the hash at once, the backend polls a cheap `txStatus(hash)` op (one `confirm` step per call) from a QML Timer.
15. **No block-range / tx-status in lez_core acf0cd50** (confirmed); block-ish: `sync_to_block`, `get_last_synced_block`, `get_current_block_height`. `poll_transaction_status` exists only on lez_core main 0.5.1 (LEZ v0.3.1, 6-arg send, Borsh instructions: incompatible). Design call 1 (reads in Rust via `RpcChain`) stands.
16. **LEE_WALLET_HOME_DIR is not read by lez_core.** `open(config_path, storage_path, statistics_path)` / `create_new(..., password)` take explicit paths; LEZ wallet's `get_home()` = `$LEE_WALLET_HOME_DIR` else `~/.lee/wallet`, files `wallet_config.json`, `storage.json`, `statistics.json`. Our module computes them (design call 3 "open the wallet at the deployment's wallet home if none is open": `open` fails if already open → treat that as success; `list_accounts` works when open). lez-programs `LogosWalletProvider` does exactly this (apps/shared/wallet/src/LogosWalletProvider.cpp).
17. **Labels**: lez_core has `resolve_label(label)` → `"Public/<hex>"`, `get_all_labels_for_account(hex,is_private)`, `check_label_available`, `add_label` (returns SUCCESS even on FFI failure, cpp:1262-1266), but **no "list all labels"**. Picker: `list_accounts()` + `get_all_labels_for_account` per account, or read `storage.json` labels via SDK keystore (needs the new accessor, §0.2).
18. **amm FFI is pure ("brain only"), chain reads go through lez_core in C++** (`get_account_public` JSON passed into FFI requests). Our design deliberately differs (FFI owns `RpcChain` reads, call 1). The amm FFI's envelope `error` is a **string**; ours is `{kind,message}` (design). amm has **no header-drift CI** and **no C++ tests**; copy the CMock pattern from `modules/stablecoin/tests` instead.
19. **Design system skew**: lez-programs' own runners resolve `logos-design-system` transitively at f6e95ae (2026-05-05) / 6176f0d7 / 3312444e, none with `LogosStageLane`; f6e95ae lacks `LogosNotice`, `LogosLink`, `LogosIcon`. Basecamp master `e09962f` pins `88330d04` (2026-09-22), which has all 14 board components. Inside Basecamp the host's design system is on `QML2_IMPORT_PATH` (basecamp nix/app.nix:310), so build/test our app against Basecamp master (or override our builder's `logos-design-system` input to 88330d04); a lez-programs-style `nix run .#<app>` with an unpinned old builder would fail to load (silent spinner).
20. Basecamp master head confirmed `e09962f5998792afd3bfed8c69c3b8f07f5825a5` (2026-10-15), contains `cf16f9d` (`--module`, 2026-10-06) and PR #506 merge `63c1643f` (2026-10-13: `nix run .#dev`, `--app/--dep/--dep-package`). Standalone mode loads only package_manager/capability_module/modules_state and does **not** install deps: `lez_core` must be passed via `--modules-dir` (or `--dep`). `--module` wants an install root (`modules/` + `plugins/`), a dir with `manifest.json` directly is refused. Dev build ↔ `#install`, portable ↔ `#install-portable`.
21. No `nix run .#ui-tests` in lez-programs (design's Testing line says `nix run .#ui-tests`); that output comes from logos-module-builder's ui module outputs (Basecamp README: `nix run .#ui-tests` runs tests/*.mjs headless via `LOGOS_BASECAMP`). Verify it exists in the builder rev we pin.

## 1. auction_sdk public API (`crates/auction_sdk`)

Cargo (`crates/auction_sdk/Cargo.toml`): features `testing` (FixtureChain + support), `rpc` (= `sequencer_service_rpc` client + `common` + current-thread `tokio`; LEZ git tag `v0.2.4`). Deps: `lee`, `lee_core` (v0.2.4), `clock_core`, `auction_core`, `auction_engine_core`, `auction_engine_program`, `stablecoin_core/program`, `token_core`, `twap_oracle_core`, `borsh`, `serde(_json)`, `hex`, `thiserror 2`.

`src/lib.rs:5-29`: `#![forbid(unsafe_code)]`; pub mods `addresses chain confirm decode demo deployment encode error keystore oracle plan plan_host quote rpc(feature rpc) scan testing(cfg test|testing) tx verify`; re-exports `BlockSummary, Chain, TxSummary, SdkError, SdkResult, Plan`.

### chain.rs
- `TxSummary { hash:[u8;32], program_id: ProgramId, account_ids: Vec<AccountId>, instruction_words: Vec<u32> }` :16
- `BlockSummary { id:u64, hash, prev_hash, timestamp_ms:u64, txs: Vec<TxSummary> }` :24
- `trait Chain` :32 — `account(&self,&AccountId)->SdkResult<Account>` (uninit = `Account::default()`), `nonces(&self,&[AccountId])->SdkResult<Vec<Nonce>>`, `last_block_id()->SdkResult<u64>`, `blocks(from,to)->SdkResult<Vec<BlockSummary>>`, `send(&PublicTransaction)->SdkResult<[u8;32]>` (accepted, not executed). Blanket `impl Chain for &T` :45.
- `fn clock_ms<C>(chain)->SdkResult<u64>` :64 — `CLOCK_01` timestamp (previous block); quotes default to it.

### error.rs
`enum SdkError` :6 — `Encode(String)`, `Missing{what,id}`, `Decode{what,id,reason}`, `Mismatch{what,expected,found}` (I4), `Engine{name,code:u32}` (from `EngineError` :57), `HostReject(String)`, `Deployment{path,reason}`, `Chain(String)`, `Keystore(KeystoreError)`, `InFlight{signer,lock,blocks}`, `SendUncertain(String)` (lock kept), `Unconfirmed(String)` (lock kept), `Scan(String)`, `Io{path,reason}`. `type SdkResult<T>` :66. → FFI `error.kind` = variant name (snake/camel), `message` = Display.

### deployment.rs
`DeploymentFile` (serde, strings) :13; `Deployment { network, sequencer_url, engine_program, host_program, token_program: ProgramId, twap_oracle_program: Option<ProgramId>, engine_admin: Option<AccountId>, config: Option<AccountId>, chain_genesis: Option<String> }` :30; `Deployment::from_json(text,path)` :67, `Deployment::load(&Path)` :109; `program_id_hex` :47, `program_id_from_hex` :54.

### verify.rs
- `VerifiedConfig { config_id, state: EngineConfigState, engine: EngineAddresses, host: HostAddresses }` :21; `.slot(u64)` :161, `.coll_config_id()` :174, `.coll(chain)->CollateralConfigState` :179.
- `verify_config(chain, &Deployment, &config_id)->SdkResult<VerifiedConfig>` :36 (I4; every engine tx starts here).
- `verify_binding(chain, &VerifiedConfig)->SdkResult<()>` :126 (liquidate calls it).
- `verify_chain_identity(chain, &Deployment)->SdkResult<()>` :188 (dev sends).

### addresses.rs
`HostAddresses { program, protocol_parameters, stability_fee_accumulator, redemption_price_state, stablecoin_definition, stablecoin_master_holding, liquidation_engine_state }` :14, `new(program)` :26, `position(owner,nonce)` :39, `position_vault(pos)` :44. `EngineAddresses { program, config, accounting, buffer, engine_authority, mint_authority, unused_ref_oracle, unused_protocol_token }` :50, `new(program,admin,salt)` :64, `for_config(program,config)` :69, `coll_config(def)` :83, `record(pos)` :88, `auction(pos,nonce)` :93, `auction_vault(a)` :98, `surplus_auction(n)` :103, `surplus_vault(n)` :108, `debt_auction(n)` :113.

### decode.rs — `Reader<'a, C>` :21
`new(chain, engine)` :37, `for_deployment(chain,&Deployment)` :46; `config` :80 `coll_config` :84 `accounting` :88 (→`AccountingState`) `auction` :92 (→`AuctionState`) `surplus_auction` :96 `debt_auction` :100 `record_count` :105 `host<T>(id,&host,what)` :115 (e.g. ProtocolParameters) `oracle` :125 (→`OraclePriceAccount`, `.price`, `.timestamp`) `token_definition` :141 `balance(id,&token_program,&definition)->SdkResult<Option<u128>>` :158 `meta` :195.

### quote.rs (all take `now_ms`; pass `clock_ms`, never wall time)
- `PositionHealth { position, owner, position_nonce:u64, collateral, normalized_debt, nominal_debt: u128, unsafe_at_min_cr: bool }` :112; `position_health(chain,&v,&pos,now)` :163.
- `LiquidationQuoteView { health, nonce:u64, auction, now_ms, vault_balance, redemption_ray, market_ray, price_ray, quote: LiquidationQuote }` :176 (`quote.keeper_reward`, `.amount_to_raise`, `.lot`); `quote_liquidation(chain,&v,&pos,now)` :190 (may return `HostReject` for a safe position).
- `BidQuote { auction, now_ms, t_eff_ms, redemption_ray, price_ray, remaining_target, collateral_left, coin_in_max, fill: BidFill }` :260 (`fill.{charged, collateral_out, discount_ray, debt_part, penalty_part, ends_auction}`); `BidQuote::needs_settlement(slippage_bps)->bool` :277 (ADR-029); `quote_bid(chain,&v,&auction,coin_in_max,min_collateral_out,now)` :304.
- `AuctionView { id, state: AuctionState, t_eff_ms, discount_ray, held_at_max: bool, paused: Option<String> }` :353; `auction_view(chain,&v,&id,now)` :366 (paused feed reported, not error).
- `SystemTakeQuote { nonce, now_ms, filled, tokens, lot_left_after }` :396; `quote_take_surplus(chain,&v,nonce,amount_coin,now)` :430; `quote_take_debt(...)` :468.
- `min_out(amount,bps)` :504 (floor), `max_in(amount,bps)` :511 (ceil), `earliest_debt_start(&AccountingState)->Option<u64>` :517 (ADR-039).
- No curve/schedule sampler for the discount chart: chart needs `auction_core` discount fn exposed (new view op: sample `d(t)` over [start, start+ttl]).

### plan.rs — `Plan { program_id: ProgramId, account_ids: Vec<AccountId>, signing_requirements: Vec<AccountId>, instruction_words: Vec<u32> }` :24
`Plan::new(program_id, accounts, signers, &ix)` :35 (rejects dup accounts, unlisted signers, no signer); `variant_index()` :70. Builders (`v: &VerifiedConfig`):
`init_config` :102, `set_engine_params` :131, `set_collateral_config(v, admin, oracle, &CollateralArgs)` :154, `observe_feed(v,caller,oracle)` :177, `LiquidateArgs{keeper, keeper_collateral_holding, keeper_holding_is_fresh, position, nonce, oracle}` :194 / `liquidate(v,&a)` :206 (17 accounts, 1-2 signers), `BidArgs{bidder_stable, bidder_collateral, bidder_collateral_is_fresh, auction, coin_in_max, min_collateral_out, with_settlement, oracle}` :239 / `bid(v,&a)` :251, `settle_empty(v,caller,auction)` :289, `claim_leftover(v,owner,owner_holding,owner_holding_is_fresh,auction)` :304, `settle_debt(v,caller,amount)` :325, `start_surplus(v,caller,nonce)` :340, `TakeArgs{payer, receiver, receiver_is_fresh, nonce, amount_coin, limit}` :360 / `take_surplus` :372 / `take_debt` :429, `close_surplus` :393, `start_debt` :410, `close_debt` :451, `check_solvency(v,caller)` :466.
plan_host.rs: `initialize_program` :25, `set_liquidation_engine` :54, `open_position` :75, `generate_debt` :103, `set_minimum_collateralization_ratio(&host,signer,ratio)` :129, `set_controller_gains` :143, `set_market_price_oracle(&host,admin,new_oracle)` :162, `token_new_fungible` :175, `token_initialize_account` :195, `token_transfer(token_program,from,to,amount)` :208, `create_oracle_price_account(twap,source,base,quote,price,window)->(Plan,AccountId)` :225.
→ The FFI plan JSON for lez_core: `account_ids` (base58), `signing_requirements` (base58), `instruction_words` (u32 list), `program_id` (`[u32;8]` → hex via `program_id_hex`). Check §5 for the exact argument format lez_core wants.

### encode.rs
`words(&ix)` :21 (risc0 serde = enum index first), `variant_index(words)` :26, `STABLECOIN_IDL_POSITION_TO_ENUM` :16, `init_config_ix` :62, `set_engine_params_ix` :88, `set_collateral_config_ix` :113.

### oracle.rs (ADR-050)
`collateral_args_from_state(&CollateralConfigState)->CollateralArgs` :27; `check_new_price(chain,&v,new_oracle,price_source,price,now)` :60; `free_oracle_window(chain,twap,source,base)->(u64,AccountId)` :97 (≤ `MAX_WINDOW_PROBES`=10_000 reads :24 — potentially slow over RPC; page or cap for FFI).

### demo.rs
`PRICE_MOVE_NUM/DEN` 19/10, `BAND_HI_BPS` 20_000, `DEBT_LOT` 5_000, `FREEZE_THRESHOLD` 5_000 (:9-16), `engine_params()` :20, `collateral_args()` :30.

### keystore.rs (ADR-041)
`HOME_ENV = "LEE_WALLET_HOME_DIR"` :26; `KeystoreError` :29 {`HomeNotSet, NotFound, Unreadable, Malformed, UnknownLabel, NoKey, PrivateAccount, KeyMismatch`}; `Keystore` :48 (private fields path, keys, private, labels); `default_path(Option<&Path>)` :81 (`--wallet-home` or env, then `/storage.json`), `path_from` :85, `open(&Path)` :98, `parse(text,&Path)` :113, `resolve(label_or_id)->AccountId` :211, `key(&id)->&PrivateKey` :224. No labels accessor (see §0.2).

### tx.rs
`trait KeySource { fn signing_key(&self,&AccountId)->SdkResult<&PrivateKey>; }` :18 (impl for Keystore :22); `sign(&Plan, &[Nonce], &dyn KeySource)->SdkResult<PublicTransaction>` :29.

### confirm.rs (ADR-042)
`MAX_CONSECUTIVE_POLL_ERRORS=5` :34; `ConfirmSettings{poll_interval:Duration(2 s), drop_after_blocks:u64(5), max_polls:u32(900)}` :37/:43; `enum Outcome { Included{nonce_after:u128}, Dropped{height:u64} }` :54; `Submitted{hash,outcome}` :65; `confirm(chain,&signer,n0,h0,&s,sleep:&dyn Fn(Duration))->SdkResult<Outcome>` :79 (height first, then nonce); `InFlightLock` :242 — `acquire(dir,&signer,n0,h0,&s)` :255, `set_keep_on_drop(bool)` :306, `path()` :310, `rebase(nonce,height)` :318, `owner()` :351, Drop :356 releases unless kept; lock file `<state>/locks/<signer>.lock` `{nonce,height,owner}`. `lock_with_current_baseline` :400 (private, ≤ `LOCK_ATTEMPTS` re-reads, `REBASE_MARGIN_BLOCKS`). `submit(chain, keys, plan, lock_dir, &s, sleep)->SdkResult<Submitted>` :435. Tests: `confirm_tests.rs` (717 lines).

### scan.rs (ADR-043)
`ANCHOR_BLOCK=2` :31; `Cursor{last_block, last_hash, anchor, recent: BTreeMap<u64,String>, auctions: BTreeSet<String>, positions: BTreeSet<String>}` :34 (+`auction_ids()` :60, `position_ids()` :64); `enum Restart{ChainReplaced, NoAnchor}` :71 (Display :78); `Scanned{cursor, restarted}` :95; `ScanTargets{engine_program, config, host_program, protocol_parameters}` :101, `ScanTargets::of(&v)` :110; `ScanSettings{batch:100, reorg_depth:10}` :121; `scan(...)` :247; `scan_report(chain,&t,&cursor,&s)->Scanned` :280 (**loops to head**, :301-322); `start_at(chain,&cursor,from)` :336; `load_cursor(&Path)` :359 (missing → genesis cursor); `save_cursor(&Path,&Cursor)` :387 (atomic).

### rpc.rs (feature rpc)
`DEFAULT_TIMEOUT=30 s` :28, `MAX_BLOCK_RANGE=256` :30, `ReadRetry` :48 (`delay_before` :68), `retry_read` :78, `RpcChain` :108 — `connect(url)` :123, `connect_with_timeout(url,timeout)` :127, `with_read_retry(ReadRetry)` :151; `summarize(&Block)->BlockSummary` :198; `impl Chain for RpcChain` :255. Methods used: `getAccount`, `getAccountsNonces`, `getLastBlockId`, `getBlockRange`, `sendTransaction`.

### testing (feature `testing`)
`testing/mod.rs`: `SendMode` :26, `key(n)` :37, `id(n)` :42, `FixtureChain` :54 (`put_holding(...)` etc.), re-exports `keystore_json, write_keystore, TestKeys, STANDARD_LABELS` (:9; 14 labels, wallet.rs:18). `testing/standard.rs`: `ENGINE/HOST/TOKEN/TWAP` ids :26-29, `T0`, `R`, `SALT`, `k::*` keys :40, `Fixture` :57, `config_id()` :68, `deployment()` :73, `oracle_state` :88, `standard()` :286. → FFI tests can reuse `standard()` + `write_keystore` exactly like the CLI goldens.

## 2. auction_cli orchestration (`cli/`)

Binary `auction-cli` (`cli/Cargo.toml`, deps `auction_sdk` features `rpc`; dev-dep features `rpc,testing`). `src/lib.rs:27` `run(cmd, env, out)` / `:32 run_with(cmd, env, out, err)` dispatch (lib.rs:38-85). `main.rs` builds `Env` with `RpcChain::connect(--rpc or deployment.sequencer_url)`, `sleep: std::thread::sleep`. Global args (args.rs:15-35): `--deployment` (default `deployments/localnet.json`), `--config`, `--rpc`, `--wallet-home`, `--state-dir` (default `.auction-cli`), `--at-ms`, `--drop-after-blocks` (default 5, ≥1). Bid/take `--slippage-bps` default 50 (args.rs:69, :198, :214; must be < 10000). Exit codes (error.rs:28): 1 usage/sdk/unknown, 2 verify, 3 dropped, 4 keystore.

### Env (`cli/src/env.rs`)
`struct Env<C: Chain> { chain, deployment, config: Option<AccountId>, wallet_home: Option<PathBuf>, state_dir: PathBuf, at_ms: Option<u64>, confirm: ConfirmSettings, sleep: fn(Duration) }` :24. Methods: `config_id()` :37 (`--config` or deployment.config), `verified()` :46 (`verify_config`), `now()` :55 (at_ms or clock), `chain_now()` :64 (always clock; tx use this), `keystore()` :68, `account(arg)` :74 (base58 else wallet label), `is_fresh(&id)` :84 (`account == default`), `cursor_path(config)` :89 (`<state>/<config>.cursor.json`), `lock_dir()` :94 (`<state>/locks`), `announce_admin(&v,out)` :102 (prints `admin`, `admin source`), `refuse_at_ms()` :114, `send(v,plan,meta,out)` :126 = `submit_and_report` + `tx_report::print_applied`, `send_plain(plan,what,check,out)` :138, `submit_and_report` :155 (prints `plan  <what> (<n> accounts, <k> signer(s))`, calls `confirm::submit`, prints `submitted <hex>`, `included signer nonce N` or `Dropped` → exit 3), `with_check_hint` :186 (Unknown → "outcome unknown, lock kept ... `auction-cli <check>` ...").

### tx_report.rs
`enum Touch { Holding{label,id,definition}, Auction(AccountId), Accounting }` :19; `TxMeta{what,check,touched}` :34; `print_applied(env,v,touched,out)` :44 (prints `note  the figures below are read from the chain after inclusion (ADR-042)` then `after <label>` lines: holding `balance N`/`uninitialised`; auction `settled yes|no, collateral left X, remaining target Y`; accounting line). `accounting(&r,&v)->String` :~100 ("unbacked debt …, debt on auction …, surplus live … (next nonce …), debt live … (next nonce …)"); `system_lots` :~117 (surplus/debt lot left). These string builders are pure given a `Reader` → movable to SDK as structured `TouchedReport` + CLI formatter.

### Transaction commands (`cli/src/tx_cmds.rs`); common prefix `begin()` :33 = `refuse_at_ms` → `verified()` (I4) → `announce_admin`
| CLI command | fn | Sequence (pure parts in bold-free list; I/O = chain reads, keystore, submit) | golden |
|---|---|---|---|
| `liquidate --position --keeper --keeper-holding` | `liquidate` :73 | begin; `verify_binding` :79; resolve 3 accounts; `quote_liquidation(chain,&v,&pos,chain_now)` (HostReject → Usage "refusing before signing…") :83; print `nonce`, `auction`, `keeper reward`; `plan::liquidate{keeper_holding_is_fresh: is_fresh(holding), nonce: q.nonce, oracle: v.coll().oracle_id}`; touched `[Auction(q.auction), Holding keeper/collateral]`; check `list-auctions` | `liquidate.txt`, `liquidate_safe_position.txt` |
| `bid --auction --coin-in-max --bidder-stable --bidder-collateral [--min-out] [--slippage-bps 50]` | `bid` :117 | begin; resolve; `now=chain_now`; `q=quote_bid(…, coin_in_max, 0, now)`; `min_out = --min-out or quote::min_out(q.fill.collateral_out, bps)`; if `--min-out` given re-run `quote_bid(…, min_out, now)` (engine's own Slippage check) :127; `with_settlement = q.needs_settlement(bps)` :131 (auto `bid_with_settlement`); print `quoted out`, `min out`; `plan::bid{bidder_collateral_is_fresh: is_fresh(coll), …}`; `what = "bid_with_settlement" | "bid"`; touched `[Auction, Holding "bidder coll", Holding "bidder stable"]` | `bid_partial.txt`, `bid_final.txt`, `bid_dropped.txt`, `bid_at_ms_refused.txt` |
| `claim-leftover --owner --owner-holding --auction` | `claim_leftover` :168 | begin; `plan::claim_leftover(v, owner, holding, is_fresh(holding), auction)`; touched `[Auction, Holding owner]` | `claim_leftover.txt` |
| `settle-empty --caller --auction` | `settle_empty` :203 | begin; plan; touched `[Auction, Accounting]` | `settle_empty.txt` |
| `settle-debt --caller [--amount]` | `settle_debt` :234 | begin; `plan::settle_debt(v, caller, amount.unwrap_or(u128::MAX))`; `accounting_tx` :217 (touched `[Accounting]`, check `config`) | `settle_debt.txt` |
| `check-solvency --caller` | `check_solvency` :245 | begin; `plan::check_solvency`; accounting_tx | `check_solvency.txt` |
| `observe-feed --caller` | `observe_feed` :255 | begin; `plan::observe_feed(v, caller, v.coll().oracle_id)`; no touched | `observe_feed.txt` |
| `start-surplus --caller` | `start_surplus` :265 | begin; `accounting()` :40; refuse if `acc.surplus_live` (Usage "a surplus auction is live (nonce N)…"); `plan::start_surplus(v, caller, acc.surplus_nonce)`; accounting_tx `start_surplus_auction` | `start_surplus_while_live.txt` |
| `take-surplus --payer --receiver --amount [--slippage-bps]` | `take_surplus` :282 | begin; `nonce = latest(acc.surplus_nonce)` :45 (next-1); `q = quote_take_surplus(…, nonce, amount, chain_now)`; `limit = max_in(q.tokens, bps)`; print `quoted tokens`, `max protocol in`; `plan::take_surplus{receiver_is_fresh,…}`; touched `take_touched(…, surplus=true)` :380 (`[Accounting, Holding payer/protocol, Holding receiver/stable]`; protocol def via `protocol_definition` :51) | `take_surplus.txt` |
| `close-surplus --caller` | `close_surplus` :309 | latest nonce; plan; accounting_tx | `close_surplus.txt` |
| `start-debt --caller` | `start_debt` :320 | refuse if `acc.debt_live`; refuse if `chain_now < earliest_debt_start(acc)` (ADR-039 relist delay); `plan::start_debt(v,caller,acc.debt_nonce)` | (in `system_and_maintenance_commands` test) |
| `take-debt --payer --receiver --amount [--slippage-bps]` | `take_debt` :344 | as take-surplus with `quote_take_debt`, `limit = min_out(q.tokens,bps)`, print `min protocol out`; touched surplus=false | `take_debt.txt` |
| `close-debt --caller` | `close_debt` :367 | | `close_debt.txt` |

### Read commands (`cli/src/read_cmds.rs`)
- `config` :26 — verified; prints `config`, admin(+source), `engine program`, `host program`, `token program`, `stablecoin`, `collateral`, `protocol token` (or none), `global cap`, `global reserved`, `bad-debt freeze`; then `system_state` :47 (`accounting` line + `surplus lot left`/`debt lot left`). Goldens `config.txt`, `config_no_operator.txt`, `config_not_found.txt`, `verify_wrong_engine.txt`.
- `quote-liquidation --position` :60 (prints position, owner, unsafe (min CR), nonce, auction, collateral, vault balance, nominal debt, amount to raise, keeper reward, lot, price (RAY), clock (ms)) — `quote_liquidation.txt`.
- `quote-bid --auction --coin-in-max [--slippage-bps]` :84 (auction, clock, t_eff, discount (`output::bps`), price, remaining target, collateral left, charged, collateral out, `min out (N bps)`, debt part, penalty part, ends auction, instruction) — `quote_bid.txt`, `quote_bid_not_an_auction.txt`.
- `scan_now(env,&v,&ScanArgs,err)` :137 — load cursor → `scan_report` → `save_cursor` → `note: <Restart>` on stderr. `scan_settings` :120 (`--batch`).
- `auction_line(&AuctionView)` :156 (settled: `<id>  settled  sold X  raised a/b  leftover L (claimed C)`; live: `<id>  live  lot X  raised a/b  discount D  t_eff T ms` + `held at max discount (keeps its cap reservation of R)` + `bids revert now: <p> (run observe-feed)`).
- `skip_stale(id,e,err)` :195 (Missing/Decode silent; Mismatch/Engine warn; else fail) — pure policy, move to SDK as `classify_stale`.
- `list-auctions [--all]` :206 (`scanned to block N`, lines, `no auctions`) — `list_auctions.txt`, `list_auctions_held_at_max.txt`.
- `list-at-risk` :238 (filter `unsafe_at_min_cr && normalized_debt > 0`; line `<pos>  owner O  collateral C  debt D  unsafe at min CR`; `no position is unsafe`) — `list_at_risk.txt`. **Sort order: cursor order (BTreeSet of base58), not CR**; v4 board wants lowest CR first → SDK needs a CR value (PositionHealth has no CR field).
- `rescan [--from]` :289 (`cursor_for_rescan` :274) — `rescan.txt`, `rescan_unknown_block.txt`, `cursor_corrupt.txt`.
- `watch` :371 (`keep_worst` :338) — `watch.txt`, `watch_observe.txt`.

### Host commands (`cli/src/host_cmds.rs`, dispatch `host` :25)
`host status` → `status` :125 (reads ProtocolParameters via `admin_cmds::protocol_parameters` :35; prints `frozen`, `admin`, `freeze authority`, `market oracle`, `min collateral ratio`, `max oracle age ms`) — `host_status.txt`. Others: `addresses` (`host_addresses.txt`), `bind`, `init-program`, `open-position`, `generate-debt`, `set-min-cr` (uses `require_host_admin` :~100), `set-controller-gains`.

### Admin commands (`cli/src/admin_cmds.rs`, `admin` :107)
`addresses` :151, `init-config` :167, `set-engine-params` :260 (`--params <json>`, `--disable-protocol-token` / `--disable-ref-oracle` guarded by `guard_slot` :214: omitting a configured slot needs the flag; `check_protocol_token` :244; `announce_slot` :230), `set-collateral-config` :311, `verify` :336. Helpers `host_addresses` :31, `protocol_parameters` :35, `optional` :46, `print_after` :54, `check_network` :64, `begin` :86. Params JSON parsing in `cli/src/params.rs` (159 lines; `cli/params/demo-engine.json`, `demo-collateral.json`). Goldens `admin_*.txt`.

### Dev commands (`cli/src/dev_cmds.rs`, `dev` :34)
Non-read-only: `refuse_at_ms`; `DEV_NETWORKS` (`localnet`, `testnet-legacy`) check on `deployment.network`; `verify_chain_identity`. Commands: `chain-id` (block 1 hash; `dev_chain_id.txt`), `resolve --label` (`dev_resolve.txt`), `token-new` (`dev_token_new.txt`), `token-init-account`, `token-transfer --from --to --amount` (`plan_host::token_transfer(deployment.token_program, from, to, amount)`; `send_plain`; prints `after from`/`after to` via `describe` :~185), `oracle-create` (`dev_oracle_create.txt`), `balance --account` :~170 (`account`, `definition`, `balance` or `state uninitialised|not a fungible token holding`; `dev_balance.txt`), `oracle-publish` → `oracle_cmds::publish` :22 (`dev_oracle_publish.txt`), `keeper` → `oracle_cmds::keeper` :77.
`oracle-publish --price-source --price --host-admin --engine-admin [--base-window-ms]`: `begin` (engine admin) → twap from deployment → `protocol_parameters` → `require_host_admin` → old oracle via `Reader::oracle(coll.oracle_id)` → `free_oracle_window(chain,twap,source,base)` → `check_new_price` (Usage "refusing before signing: price … invalid") → print `price old -> new`, `new oracle`, `window ms` → 3 `send_plain`s (create / rotate / follow) → `print_after "engine oracle"`.

### Pure vs I/O (what moves to SDK)
Pure given a `&C: Chain` (movable unchanged, both CLI and FFI call them): every quote, `min_out`/`max_in`, `needs_settlement`, `latest(next_nonce)`, live/relist refusals (start-surplus/start-debt), `protocol_definition`, `take_touched`, the `Touch` lists, `accounting`/`system_lots`/`auction` report builders (return structs, CLI formats), `skip_stale` classification, list-at-risk filter, `auction_line` data, host status read, oracle-publish pre-checks (`free_oracle_window`+`check_new_price`) and its 3 plans, `begin` checks (verify + admin source). Proposed SDK module e.g. `auction_sdk::ops` with `prepare_bid(chain,&v,&BidRequest{auction, bidder_stable, bidder_collateral, coin_in_max, min_out:Option, slippage_bps}, now)->SdkResult<Prepared{plan, what, quote fields, touched: Vec<Touch>}>` per command; CLI `tx_cmds` becomes print + `env.send`.
I/O that stays at the edges: `Env::keystore`/label resolution (CLI) vs lez_core (module); `confirm::submit` (CLI) vs lez_core send + non-blocking poll (module); cursor file load/save path (both, but FFI passes state dir); stdout formatting (CLI only).

## 3. Tests, harnesses, CI

- Goldens: `cli/tests/golden.rs` (27 tests, read cmds), `cli/tests/tx_golden.rs` (15: `liquidate` :31, `liquidate_refuses_a_position_that_is_safe` :54, `bid_partial` :106, `bid_final` :116, `bid_with_a_min_out_above_the_quote_is_refused_before_signing` :128, `bid_dropped` :139, `bid_without_a_verdict_keeps_the_lock_and_says_how_to_check` :159, `a_tx_command_without_a_keystore_is_exit_4_and_sends_nothing` :183, `system_and_maintenance_commands` :199, `every_tx_command_announces_the_admin_before_it_signs` :295, `take_quotes_set_the_slippage_bounds` :305, `watch_observe` :333, `a_tx_command_refuses_at_ms` :349, `watch_reports_a_failed_round_and_goes_on` :368, `watch_stops_at_once_on_a_setup_error` :399), `cli/tests/admin_golden.rs` (30). Shared `cli/tests/common/mod.rs`: `Case { fx: Fixture, dir: TempDir, with_wallet, deployment }` (:18), `Case::new()` (standard fixture + temp dir), `cursor_path`, `write_cursor`, `run(args)->String`, `run_split(args)->(stdout,stderr)` normalised (hash → `<hash>`), output ends with `exit: N`. Golden files `cli/tests/golden/*.txt` (51). Regenerate with `UPDATE_GOLDEN=1` (golden.rs:2) and review diff; never hand-edit.
- Sample `bid_final.txt`: `admin …`, `admin source deployment file (checked)`, `quoted out 342956`, `min out 341241`, `plan bid_with_settlement (12 accounts, 1 signer(s))`, `submitted <hash>`, `included signer nonce 1`, `note …`, `after auction settled no, collateral left 475000, remaining target 660000`, `after bidder coll balance 0`, `after bidder stable balance 1000000`, `exit: 0`. (Keys padded to 17 cols by `output::kv` :11.)
- SDK unit tests: `plan_tests.rs`, `confirm_tests.rs`, `rpc_tests.rs` (mock HTTP), `encode::tests`, quote tests; live ignored test `live_rpc_reads_height_clock_and_blocks`.
- Real-guest proofs: `programs/integration_tests/tests/sdk_plans.rs`, `sdk_quotes.rs`, `chains`, `idl.rs` (`WorldChain` in `programs/integration_tests/tests/common/sdk.rs`).
- Run (WSL only, from Git Bash): `MSYS_NO_PATHCONV=1 wsl -d Ubuntu-20.04 -e bash -lc 'cd /mnt/c/Users/user/Documents/basecamp/lez-auction-engine && scripts/wsl-cargo.sh test -p auction_sdk -p auction_cli --features auction_sdk/testing,auction_sdk/rpc'`. `scripts/wsl-env.sh` sources `${LEZ_ENV_FILE:-$HOME/lez-spikes/s1/env.sh}` (risc0 toolchain, libclang, sysroot), sets `CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$HOME/lez-target/plan2}` ("never point CARGO_TARGET_DIR anywhere else"), `RISC0_DEV_MODE=1`, cd repo root, `exec "$@"`. `scripts/wsl-cargo.sh` = `wsl-env.sh cargo "$@"`. `RISC0_SKIP_BUILD=1` for builds that need no guest ELFs (clippy, fuzz). `scripts/disk-guard.sh` before/after heavy builds. Distro name is `Ubuntu-20.04` but OS is Ubuntu 24.04.4.
- Other scripts: `scripts/demo.sh` (localnet run, `--sequencer-dev-mode`), `scripts/lib/demo-scenario.sh` (phases `dp_accounts dp_auth dp_deploy dp_host dp_engine dp_bind dp_collateral dp_holdings dp_positions dp_f1f3 dp_f5 dp_price_move dp_shortfall dp_f4 dp_freeze dp_final`; hooks `demo_run_phase`, `demo_after_deploy`, `demo_before_positions`, `demo_tick`; `remember`/`recall` via run.env; expected values ADR-051: buffer 30000 after F1-F3, 29500 after surplus take, unbacked U1 in [42154, 63200], etc.), `scripts/lib/localnet.sh` (`ln_*`), `scripts/localnet-smoke.sh` (sequencer 127.0.0.1:3040), `scripts/testnet-legacy.sh`, `scripts/fuzz-smoke.sh`, `scripts/ci/free-disk.sh`, `scripts/ci/idl-drift.sh`, `scripts/check-coverage.py`, `scripts/check-requirements.py` (REQUIREMENTS.md: every hard requirement needs a named test), `scripts/tests/demo-preflight.sh`, `testnet-legacy-preflight.sh`.
- Demo scenario as UI actions (labels): liquidate P0 keeper/keeper-coll; bid A0 100000 then 230000 owner-stable/owner-coll (2nd finishes → bid_with_settlement); claim-leftover owner/owner-coll; observe-feed, settle-debt, check-solvency, start-surplus (caller); take-surplus taker-protocol→taker-stable 500; dev oracle-publish oracle-src price 1.9e27 admin/engine-admin; host status; liquidate P2; bid A2 330000; list-auctions --all; settle-debt; start-debt; dev token-transfer owner-stable→taker-stable 5000; take-debt taker-stable→taker-protocol 5000; admin set-engine-params --disable-protocol-token (engine-admin); check-solvency; host status frozen=yes; admin verify. Signing labels needed: keeper, owner, caller, admin, engine-admin, oracle-src, taker-* (design's picker lists "keeper, bidder, owner, caller, admin" — also needs `engine-admin`, `oracle-src`, `taker-stable`/`taker-protocol`; the scenario has no `bidder` label, the owner bids).
- CI `.github/workflows/core.yml` jobs: `core` (fmt `-p auction_core -p auction_engine_core -p auction_engine_program -p engine_integration_tests -p auction_sdk -p auction_cli`; clippy auction_core; tests; PROPTEST 2000; differential vectors; demo/testnet preflights), `programs` (RISC0_DEV_MODE=1; install-risc0; `RISC0_SKIP_BUILD=1 cargo clippy --locked --workspace --all-targets --exclude integration_tests -- -D warnings`; `cargo test --locked -p auction_engine_core -p auction_engine_program -p engine_integration_tests -p auction_sdk -p auction_cli --features auction_sdk/testing,auction_sdk/rpc`; vendored suites; cycles; checker unit tests), `idl-drift` (`scripts/ci/idl-drift.sh`), `hook-patches` (clones lez-programs 64c01210adecd6b557dbd021a3cc94553f243fd8), `coverage` (cargo-llvm-cov on 5 crates, `check-coverage.py`), `requirements`. Other workflows: `e2e.yml` (push/schedule; bootstrap-lez + `scripts/demo.sh --sequencer-dev-mode` matrix), `fuzz.yml` (weekly), `guests.yml`, `real-proofs.yml`. Add for Plan 4: `auction_ffi` to fmt/clippy(workspace already)/test/coverage, a `ffi-header-drift` step (`cbindgen` regenerate + `git diff --exit-code`), and a core-module test job (Nix or CMake + Qt; design call 6 says CI runs FFI + core-module tests + header drift, not the story e2e).


<!-- §4 from the lez-programs survey -->
## 4. lez-programs @ 64c0121: lez-programs AMM stack (apps/amm, apps/shared/wallet, modules/amm, amm_ffi)

Repo: `~/lez-spikes/s1/lez-programs` @ `64c0121` (logos-blockchain/lez-programs). Read-only survey.

### 4.1 Layout

```
apps/amm/                         ui_qml module "amm_ui"
  flake.nix flake.lock metadata.json CMakeLists.txt README.md VALIDATION.md
  amm-pools.json.example amm-tokens.json.example icons/amm.png
  src/  AmmUiBackend.{h,cpp,rep}  AmmUiPlugin.{h,cpp}  AmmUiPluginInterface.h  RegistryLoader.{h,cpp}
  qml/  Main.qml
        chrome/{NavBar,SettingsModal}.qml
        pages/{SwapPage,PoolsPage,PoolDetailPage,PositionsPage,LiquidityPage}.qml
        state/{NewPositionFlow,DummySwapState}.qml
        components/swap/{SwapCard,SwapSummary,TokenInput,SwapConfirmationSummary}.qml
        components/shared/{SuccessToast,SlippageToleranceControl,AmmTokenSelectButton,TokenSelectorModal}.qml TokenList.js TokenVisuals.js
        components/liquidity/{AmmTokenAmountSurface,AmmDetailCard,RemoveLiquidityDialog,AmmPairSeparator,
          LiquidityConfirmationSummary,AmmTheme,AmmTokenAccessory,SummaryRow,AmmSecondaryButton,AmmActionCard,
          AmmPrimaryButton,TokenAmountInput,EstimateInfoButton,NewPositionForm}.qml AmountMath.js
  tests/ swap.mjs create-pool.mjs add-liquidity.mjs remove-liquidity.mjs custom-token.mjs faucet-swap.mjs README.md
         qml/tst_{ResponsivePopups,PoolsPage,LiquidityPage,NewPositionForm,LiquidityConfirmationDialog,TokenAmountInput}.qml
         testnet/setup-amm-testnet.sh
apps/shared/wallet/               static lib logos_wallet_access + QML module Logos.Wallet 1.0
  CMakeLists.txt
  src/ WalletProvider.{h,cpp} LogosWalletProvider.{h,cpp} WalletAccountModel.{h,cpp} WalletController.{h,cpp}
  qml/ WalletControl.qml ProgramAccountSelector.qml TransactionConfirmationDialog.qml SubmittedTransaction.qml
       internal/{AccountDelegate,WalletIconButton,CopyButton,CreateAccountDialog,CreateWalletDialog,WalletMessageDialog}.qml
       internal/icons/{copy,power,account,back,checkmark}.svg
  tests/cpp/LogosWalletProviderTest.cpp  tests/cpp/fixtures/logos_sdk.h (FakeExecutionZone)
  tests/support/FakeWalletProvider.h     tests/qml/main.cpp + tst_{TransactionConfirmationDialog,SubmittedTransaction,ProgramAccountSelector,WalletControl}.qml
modules/amm/                      core module "amm_module" (universal interface)
  flake.nix flake.lock metadata.json CMakeLists.txt README.md
  src/amm_module_impl.{h,cpp}     (h 318 lines, cpp 1578 lines) -- NO tests/ dir
  ffi/ Cargo.toml build.rs cbindgen.toml include/amm_ffi.h tests/public_api.rs
       src/{lib,ffi,account}.rs  src/api/{admin,token_holdings,quote,holding,swap,fee,config,pair,context,mod,liquidity,request,oracle,tests}.rs
```
Other FFI crates (same pattern): `modules/token/ffi`, `modules/stablecoin/ffi` (stablecoin has C++ CMock tests, see 9).

### 4.2 Flakes

### apps/amm/flake.nix
- inputs: `logos-module-builder.url = "github:logos-co/logos-module-builder"` (:5);
  `shared_wallet = { url = "path:../shared/wallet"; flake = false; }` (:8-11);
  `lez_core.url = "github:logos-blockchain/logos-execution-zone-module?rev=acf0cd501b262c4c15969e3735e85318297b85bf"` (:18);
  `amm_module = { url = "path:../../modules/amm"; inputs.lez_core.follows = "lez_core"; }` (:24-27).
- outputs (:36-67) verbatim args:
```nix
logos-module-builder.lib.mkLogosQmlModule {
  src = ./.; configFile = ./metadata.json; flakeInputs = inputs; externalLibInputs = { };
  preConfigure = ''
    cmakeFlagsArray+=("-DLOGOS_WALLET_SOURCE_DIR=${shared_wallet}")
    cmakeFlagsArray+=("-DLOGOS_WALLET_GENERATED_DIR=$PWD/generated_code/include")
  '';
  postInstall = '' # find */shared-wallet/qml/Logos/Wallet/qmldir, copy to $out/lib/Logos/Wallet,
    # strip qmldir lines matching ^(linktarget|optional plugin|plugin|classname|typeinfo|prefer)
    # -> pure-QML module at plugin root (Basecamp rejects `prefer :/qt/qml/...` "Invalid null URL")
  '';
}
```
### modules/amm/flake.nix
- inputs: `logos-module-builder` (unpinned url), `lez_core` same rev acf0cd50 (:12), `lez_programs.url = "path:../.."` (:20; root flake supplies amm_ffi).
- outputs (:23-31):
```nix
logos-module-builder.lib.mkLogosModule {
  src = ./.; configFile = ./metadata.json; flakeInputs = inputs;
  externalLibInputs = { amm_ffi = { input = lez_programs; packages.default = "amm_ffi"; }; };
}
```
### root flake.nix (464 lines)
- inputs: nixpkgs (nixos-unstable), flake-utils, crane, rust-overlay, `logos-module-builder` (:32), `lez_core` rev acf0cd50 (:59; "0.4.1-interim": byte-string `send_generic_public_transaction` fix on v0.2.4 wallet-ffi, comments :36-58).
- `mkHostFfi { package, sourceDir, header }` (:84-120): crane `buildPackage` `-p <pkg>`, `doCheck=false`, postInstall copies `${sourceDir}/include/${header}` to `$out/include`; Darwin `install_name_tool -id` absolute.
  `packages.{default,amm_ffi}=ammFfi`, `stablecoin_ffi`, `token_ffi` (:122-144).
- `appOutputs = mkLogosQmlModule { src=./apps/amm; ...; flakeInputs = inputs // { amm_module = ammModuleOutputs; }; externalLibInputs = {}; preConfigure/postInstall same as apps flake but LOGOS_WALLET_SOURCE_DIR=${./apps/shared/wallet} }` (:153-202).
- `ammModuleOutputs = mkLogosModule { src=./modules/amm; ...; externalLibInputs.amm_ffi = { input = self; packages.default = "amm_ffi"; }; }` (:286-293). (No `tests=` for amm; token/stablecoin have `tests = { dir = ./modules/<x>/tests; mockCLibs = [ "<x>_ffi" ]; }` :318-321, :349-352.)
- Package/app renames: UI `default` -> `amm-ui` (package + app, app wrapped by `wrapWithDyld` setting `DYLD_FALLBACK_LIBRARY_PATH=${amm_ffi}/lib` and `QML_IMPORT_PATH=${amm-ui}/lib`, :366-393); aliases `amm-ui-lgx`, `amm-ui-lgx-portable`, `amm-ui-install`, `amm-ui-install-portable` (:272-278); core `amm-module`, `amm-module-lgx{,-portable}`, `amm-module-install{,-portable}` (:300-306); same for token-ui/token-module/stablecoin-module. Other builder outputs (e.g. `lgx`, `test-framework`, `integration-test`, `smoke-test`) pass through un-renamed via `removeAttrs ... ["default"]`; final `(removeAttrs appOutputs ["apps" "packages"]) // { apps = mergedApps; packages = mergedPackages; }` (:459-463).

### flake.lock revs (direct inputs)
| lock | logos-module-builder | lez_core | other |
|---|---|---|---|
| root | 6ef42ea8661121831ece79e6b702e27ac1cf46e7 | acf0cd501b262c4c15969e3735e85318297b85bf | nixpkgs e7a3ca80…, crane 80db5bdc…, rust-overlay e013376c… |
| apps/amm | f4942648c28127d4ef4d4b467b22bd33873ab4fa | acf0cd50… (node lez_core_2; amm_module's nested lez_core b60be464… is overridden by follows) | amm_module/shared_wallet = path |
| modules/amm | ab800a61b37363e843aabf4c79516c04a81a292f | acf0cd50… | lez_programs = path ../.. |

Module-builder sub-inputs: root -> logos-cpp-sdk d12a7bbb…, logos-qt-sdk 6c9a0de1…, logos-standalone-app c87bbf4c…, logos-test-framework eb1600cc…; apps/amm -> cpp-sdk aea29d37…, qt-sdk cdf077cd…, standalone-app f812b43f…, test-framework 5b8fcc9f…; modules/amm -> cpp-sdk 937f17ed…, **logos-design-system 3312444e96076b8786e3e21a000676fc4e851350**, qt-sdk 5c094543…, test-framework 31008b6a….
Transitive (any depth): logos-design-system revs f6e95ae2…, 6176f0d7…, 4a37b344… (+3312444e in apps lock); logos-qt-mcp c5223b4b…, c87f6bd9… (+1fea5094… in apps lock). No logos-basecamp node. VALIDATION.md pins design system `github:logos-co/logos-design-system/6176f0d7a5dfeb64a7f0f98e7ca2bf71a4804772` for qmltestrunner.

### 4.3 metadata.json (verbatim essentials)
apps/amm/metadata.json:
```json
{ "name":"amm_ui","display_name":"Logos DEX App","version":"0.3.0","type":"ui_qml","category":"amm",
  "description":"Trade tokens and provide liquidity on the Logos DEX.","author":"Logos Core Team",
  "main":"amm_ui_plugin","view":"qml/Main.qml","icon":"icons/amm.png","dependencies":["lez_core","amm_module"],
  "nix":{"packages":{"build":["pkg-config"],"runtime":["qt6.qtdeclarative","zstd","krb5","abseil-cpp","libbase58"]},
         "external_libraries":[],"cmake":{"find_packages":[],"extra_sources":[],"extra_include_dirs":[],"extra_link_libraries":[]}}}
```
modules/amm/metadata.json:
```json
{ "name":"amm_module","display_name":"Logos DEX Core","version":"0.3.0","type":"core","interface":"universal","category":"amm",
  "description":"Core module for the Logos DEX (AMM) — on-chain pool resolution, swaps, and liquidity.","author":"Logos Core Team",
  "main":"amm_module_plugin","dependencies":["lez_core"],
  "nix":{"packages":{"build":[],"runtime":[]},"external_libraries":[{"name":"amm_ffi"}],
         "cmake":{"find_packages":[],"extra_sources":[],"extra_include_dirs":[],"extra_link_libraries":[]}}}
```
Hand-staged manifest for logoscore (modules/amm/README.md:198-204): `{"name":"amm_module","type":"core","version":"0.1.0","manifestVersion":"0.2.0","dependencies":["lez_core"],"main":{"darwin-arm64-dev":"amm_module_plugin.dylib"}}` + `variant` file.

### 4.4 CMake
- modules/amm/CMakeLists.txt: include `$ENV{LOGOS_MODULE_BUILDER_ROOT}/cmake/LogosModule.cmake` (:5-11); `string(JSON MODULE_NAME GET ${METADATA_JSON} name)` (:14-15);
  `logos_module(NAME ${MODULE_NAME} SOURCES src/amm_module_impl.h src/amm_module_impl.cpp EXTERNAL_LIBS amm_ffi)` (:24-31). No explicit include dirs / target_link_libraries: builder resolves `amm_ffi` from `externalLibInputs` (lib + `$out/include/amm_ffi.h`). lez_core NOT linked; reached via generated `logos_sdk.h`.
- apps/amm/CMakeLists.txt: `find_package(Qt6 6.8 ... Core Gui Network Qml Quick QuickControls2)`, `qt_standard_project_setup(REQUIRES 6.8)`, `include(CTest)`; `add_subdirectory("${LOGOS_WALLET_SOURCE_DIR}" .../shared-wallet)` (:23);
  `logos_module(NAME amm_ui REP_FILE src/AmmUiBackend.rep SOURCES <7 src files> FIND_PACKAGES Qt6Gui LINK_LIBRARIES Qt6::Gui Qt6::Network LINK_TARGETS logos_wallet_access)` (:30-48). `.rep` compiled by `logos_module(REP_FILE ...)` -> `rep_AmmUiBackend_source.h`, `AmmUiBackendSimpleSource`, `AmmUiBackendViewPluginBase`.
- apps/shared/wallet/CMakeLists.txt: `logos_wallet_access` STATIC (PIC, cxx_std_17; PRIVATE include `${LOGOS_WALLET_GENERATED_DIR}{,/include}` for `logos_sdk.h`; FATAL if missing :12-18); `qt_add_qml_module(logos_wallet_qml URI Logos.Wallet VERSION 1.0 RESOURCE_PREFIX /qt/qml ...)` (:91-103), internal files `QT_QML_INTERNAL_TYPE`, rpath `$ORIGIN`/`@loader_path`, installed to `lib/qml/Logos/Wallet`. BUILD_TESTING: `logos_wallet_access_test` (QtTest, fixtures dir shadows `logos_sdk.h`) and `logos_wallet_qml_test` (QUICK_TEST_MAIN, offscreen env) (:137-176).

### 4.5 Core module C++ (modules/amm/src/amm_module_impl.h)
- `class AmmModuleImpl : public LogosModuleContext` (:28); includes `<logos_json.h>` (LogosMap/LogosList = nlohmann::json aliases), `<logos_module_context.h>`. Header must stay Qt-free (glue generated from header because `"interface":"universal"`). No Q_INVOKABLE; public methods = API.
- Public API (all sync, return std types):
  `LogosMap resolvePoolAccount(const std::string& def_a_hex, const std::string& def_b_hex)` :41;
  `LogosMap configAccount()` :48; `LogosMap setAmmProgramId(const LogosMap&)` :55; `LogosMap setConfigId(const LogosMap&)` :61;
  `LogosMap transferOwnership(const LogosMap&)` :68; `withdrawProtocolFees` :76; `createPriceObservations` :86; `createOraclePriceAccount` :87;
  `LogosMap swapExactInQuote(const std::string& token_in_hex, const std::string& token_out_hex, const nlohmann::json& amount_in, int64_t slippage_bps)` :99; `swapExactOutQuote(..., const nlohmann::json& amount_out, int64_t)` :113;
  `std::string swapExactInput(def_a_hex, def_b_hex, user_input_holding_hex, user_output_holding_hex, const nlohmann::json& amount_in, const nlohmann::json& min_out, const nlohmann::json& deadline)` :128 (returns tx hash or "");
  `std::string swapExactOutput(..., amount_out, max_in, deadline)` :142;
  `LogosMap createPoolQuote/createPool/addLiquidityQuote/addLiquidity/removeLiquidityQuote/removeLiquidity/syncReserves(const LogosMap& request)` :166-242;
  `LogosList tokenHoldings(bool wallet_open)` :252; `LogosList feeTiers()` :259; `LogosList resolveTokens(const LogosMap& request, bool wallet_open)` :273 (list wrapped in map: "universal-module glue only supports map/scalar inputs").
  `nlohmann::json` params: generated dispatch passes raw value; accepts JSON int or decimal string, rejects floats (logoscore promotes big ints to double).
- Private: `ammProgramId()`, `ammConfigId()`, `loadAmmElf()` (reads env `AMM_PROGRAM_BIN`), `normalizeAccountId()`, `readConfig()`, `readPublicAccount()`, `walletAccountReads()`, `oracleSetupSubmit()`; state `m_activeProgramId`, `m_activeConfigId` (env fallback `AMM_CONFIG_ID`).
- cpp: `#include "logos_sdk.h"` (generated, `LogosModules` with std-typed `lez_core` accessor) :19; `extern "C" { #include "amm_ffi.h" }` :21-23; tracing `AMM_TRACE` gated on env `AMM_DEBUG` -> stderr (:28-37).
- lez_core calls (std types): `modules().lez_core.account_id_from_base58(std::string)` :295; `.get_account_public(account_id)` -> JSON string `{program_owner,balance,nonce,data}` hex :308; `.list_accounts()` -> json array (`is_public`) :337; `.send_generic_public_transaction(std::vector<std::string> accounts, std::vector<bool> signers, std::vector<uint8_t> instruction /*LE bytes of u32 words*/, std::string program_id)` -> JSON `{success, tx_hash}` :499,554,639,865,956,1084,1247,1405,1471.
- FFI call helper (:188-216): `FfiResult call(char* (*op)(const char*), const json& request)` -> `request.dump()`, `op(payload.c_str())`, copy to std::string, `amm_free(raw)`, parse; returns `{ok, value(object), error}`.
- Response shape to callers: `{status:"ok", error:"", ...}` / `{status:"error", error:<code>}`; new-position path also `publicError` envelope `{status,canSubmit,code,errors[{code,recoverable,blockingFields,details}],warnings,accountPreview}` (:218-243).
- Threads/async/events: none. Every method is a blocking sequence (FFI + synchronous lez_core IPC). No events emitted.

### 4.6 FFI (modules/amm/ffi)
- Cargo: `crate-type = ["cdylib","rlib"]`; deps amm_core, token_core, twap_oracle_core, lee_core, clock_core (git LEZ tag v0.2.4), risc0-zkvm, risc0-binfmt =3.0.4, borsh, serde(_json); build-dep `cbindgen = "0.28"`.
- cbindgen.toml verbatim: `language = "C"`, `include_guard = "AMM_FFI_H"`, `pragma_once = true`, `cpp_compat = true`, `autogen_warning = "/* Generated by cbindgen. Do not edit. */"`, `[export] prefix = ""`.
- build.rs: `cbindgen::generate(CARGO_MANIFEST_DIR).write_to_file("{crate_dir}/include/amm_ffi.h")`; `rerun-if-changed=src`, `cbindgen.toml`. Header is committed and regenerated in-tree on every cargo build; **no CI drift check** (ci.yml has no git-diff/header step).
- Style: `char* amm_<op>(const char* request_json)`, 25 ops (amm_ffi.h:17-65): config_id, config_account, pair_ids, resolve_tokens, swap_pair, resolve_pool, pool_id, swap_exact_{in,out}_{quote,plan}, create_pool_{quote,plan}, add_liquidity_{quote,plan}, remove_liquidity_{quote,plan}, sync_reserves_plan, transfer_ownership_plan, withdraw_protocol_fees_plan, create_price_observations_plan, create_oracle_price_account_plan, token_holdings, program_id, fee_tiers; plus `void amm_free(char* value)`.
- ffi.rs: `#[unsafe(no_mangle)] pub extern "C" fn amm_x(request_json: *const c_char) -> *mut c_char { call::<XRequest>(request_json, api::x) }`; `call` = `catch_unwind` + serde parse + op; envelope `{"ok":bool, "value"?:<json>, "error"?:<string>}` (`skip_serializing_if none`); panic -> `"internal panic"`; returns `CString::into_raw`. Request structs `#[serde(rename_all = "camelCase")]` (api/request.rs:122-135), e.g. `SwapExactInQuoteRequest { tokenInId, tokenOutId, amountIn: String, slippageBps: u32, config: AccountRead, poolData: hex }` — the FFI is pure: chain reads are passed in by the C++ module.
- Plans return `{programId, accountIds[], signingRequirements[], instruction:[u32 words]}` consumed by module.

### 4.7 UI app C++ (apps/amm/src)
- `AmmUiBackend.rep` (235 lines): PROPs `isWalletOpen, walletStateReady, walletExists, configPath, storagePath, walletHome, lastSyncedBlock, currentBlockHeight, sequencerAddr, sequencerReachable, registryRevision, registryUrl, activeNetwork, networks(QVariantList)`; SLOTs (sample): `QString createAccountPublic()`, `QString createNewDefault(QString password)`, `bool openExisting()`, `void disconnectWallet()`, `QVariantMap resolvePoolAccount(QString defAHex, QString defBHex)`, `QVariantMap swapExactInQuote(QString tokenInHex, QString tokenOutHex, QString amountInDecimal, int slippageBps)`, `QString swapExactInput(QString defAHex, QString defBHex, QString userInputHoldingHex, QString userOutputHoldingHex, QString amountInDecimal, QString minOutDecimal, QString deadlineDecimal)`, `QVariantMap createPool/addLiquidity/removeLiquidity(QVariantMap request)` (+ *Quote), `QVariantList tokenList()/poolList()/feeTiers()/resolveTokens()/tokenHoldings()`, `QVariantMap addCustomToken(QString)`, `void refreshRegistry()/saveRegistryUrl(QString)/selectNetwork(QString)`.
- `AmmUiPluginInterface : public PluginInterface`, IID `"org.logos.AmmUiPluginInterface"`. `AmmUiPlugin : QObject, AmmUiPluginInterface, AmmUiBackendViewPluginBase`, `Q_PLUGIN_METADATA(IID ... FILE "../metadata.json")`, `Q_INVOKABLE void initLogos(LogosAPI* api)` -> `new AmmUiBackend(api,this); setBackend(m_backend);` (AmmUiPlugin.cpp).
- `AmmUiBackend : public AmmUiBackendSimpleSource` with `Q_PROPERTY(WalletAccountModel* accountModel READ accountModel CONSTANT)` (h:33-35). Members: `LogosAPI* m_logosAPI` (fallback `new LogosAPI("amm_ui", this)`), `std::unique_ptr<LogosModules> m_logos`, `LogosWalletProvider`, `WalletController(*m_wallet, "AmmUI")`, `RegistryLoader`.
- Forwarding is a synchronous one-liner with Qt types: `return m_logos->amm_module.resolvePoolAccount(defAHex, defBHex);` (cpp:164); submit slots guard `if (!isWalletOpen()) return {};` then `refreshBalances()` on success (cpp:210-225). On `RegistryLoader::changed`: `m_logos->amm_module.setAmmProgramId(QVariantMap{{"ammProgramId", ...}})`, `.setConfigId(...)`, `setNetworks/setActiveNetwork/setRegistryRevision` (cpp:47-57). Startup: `QTimer::singleShot(0, ...)` -> `setWalletStateReady(true); m_registry->refresh();` (cpp:71-79). Settings `QSettings("Logos","AmmUI")` key `registryUrl`.
- Shared wallet: `WalletProvider` interface (WalletProvider.h:97-111: `connect(WalletPaths)`, `createWallet`, `snapshot`, `createAccount(bool)`, `readPublicAccount`, `submitPublicTransaction(WalletTransaction)`, `disconnect`); `LogosWalletProvider` calls Qt-typed `logos->lez_core.open(config, storage, statistics)`, `create_new`, `create_account_public/private`, `get_account_public`, `send_generic_public_transaction(QStringList, signingRequirements, QVariant::fromValue(QByteArray LE bytes), QString programId)` (LogosWalletProvider.cpp:283-298), `get_sequencer_addr`, `list_accounts` (QVariantList), `get_current_block_height`, `sync_to_block`, `get_last_synced_block`, `get_balance(addr,bool)`, `save()` == `WALLET_FFI_SUCCESS`.

### 4.8 QML
- Main.qml imports: `QtQuick 2.15`, `QtQuick.Controls 2.15`, `QtQuick.Layouts 1.15`, `Logos.Theme`, `"chrome"`, `"pages"`.
  `readonly property var backend: logos.module("amm_ui")`, `readonly property var accountModel: logos.model("amm_ui", "accountModel")`; `Connections { target: logos; function onViewModuleReadyChanged(moduleName, isReady) {...} }`; `Component.onCompleted: root.ready = root.backend !== null && logos.isViewModuleReady("amm_ui")`. Tabs via `NavBar { objectName: "navBar" }` + `navbar.select(tab, sub)`; pages SwapPage(objectName "swapPage"), PoolsPage, PoolDetailPage, PositionsPage, LiquidityPage; `SettingsModal`; button `appSettingsButton`. No StackView.
- Async pattern: `logos.watch(root.backend.swapExactInQuote(a,b,amt,bps), function(quote){ if (isStale()) return; ... }, function(err){...})` (SwapCard.qml:131,214,308,488; SwapPage.qml:90,104; NavBar.qml:288). Generation counters guard stale replies.
- Usage counts across apps/amm/qml + apps/shared/wallet/qml: `logos.module` 7 (1 real, rest comments), `logos.watch` 7, `logos.model` 1, `logos.isViewModuleReady` 1. objectName: 87 occurrences in apps/amm/qml.
- Imports counts: `import Logos.Wallet` 7; `Logos.Theme` 2 (Main.qml, NavBar.qml...), `Logos.Controls` 2, `Logos.Icons` 2 (files: Main.qml, chrome/NavBar.qml, pages/LiquidityPage.qml, components/liquidity/NewPositionForm.qml). Mixed versioned (`QtQuick 2.15` x11) and unversioned (`QtQuick` x29) imports; `QtQuick.Controls.Basic` x9.
- Design-system identifiers: `LogosIconButton` 2 (NewPositionForm.qml:236, LiquidityPage.qml:222, `iconSource: LogosIcons.refresh`), `LogosIcons` 2, `LogosSpinBox` 1 (NewPositionForm.qml:398). Theme tokens: `Theme.palette.*` 18 (text 4, borderSecondary 4, textSecondary 3, backgroundSecondary 3, background 3, warning 1), `Theme.colors.*` 6 (textSecondary 2, textPrimary, panelBg, border, background), `Theme.isDark` 3. Most styling is local (`AmmTheme.qml`, `Amm*Button.qml`).
- Logos.Wallet used: `ProgramAccountSelector` x4, `TransactionConfirmationDialog` x2, `WalletControl` x1, `SubmittedTransaction` x1.

### 4.9 Tests
- UI e2e (`apps/amm/tests/*.mjs`, 2057 lines): framework from logos-qt-mcp:
  `const fwRoot = process.env.LOGOS_QT_MCP || new URL("../result-mcp", import.meta.url).pathname; const { test, run } = await import(resolve(fwRoot, "test-framework/framework.mjs"));`
  `test("amm swap: sell token #1 for token #2", async (app) => {...}); run();` (swap.mjs:181,293).
  app API used (counts): `app.waitFor(fn, {timeout, interval, description})` 76, `app.inspector.send("click"|"evaluate"|"setProperty", {objectId, expression|property,value})` 45 (22/16/7), `app.findByProperty("objectName", name)` -> `{matches:[{id}]}` 12, `app.getProperties(id)` -> `{properties:[{name,value}]}` 11, `app.expectTexts([...])` 9, `app.screenshot(...)` 6, `app.click("Trade")` 5.
  Run: `nix build .#test-framework -o apps/amm/result-mcp`; app `LEE_WALLET_HOME_DIR=... AMM_PROGRAM_BIN=... TOKENS_CONFIG=... CUSTOM_TOKEN_CONFIG=... nix run .#amm-ui` (inspector localhost:3768); `node apps/amm/tests/swap.mjs`. Headless: `nix build .#integration-test -L`. Needs live sequencer + `tests/testnet/setup-amm-testnet.sh` (fixed mnemonic wallet, TKA/TKB/TKC/D, seeded A/B pool). No `nix run .#ui-tests` app.
- QML unit: `tests/qml/tst_*.qml` (QtTest `TestCase`, fake backend `QtObject` with `function poolList()` etc., `import "../../qml/pages" as Pages`), run manually via `qmltestrunner` per VALIDATION.md (not wired into CMake/CI for apps/amm; shared wallet's are via CTest `logos_wallet_qml`).
- C++: shared wallet `LogosWalletProviderTest` (QTEST_GUILESS_MAIN, fixture `logos_sdk.h` with `FakeExecutionZone` recording calls). **amm_module has no C++ tests.** Reference pattern in stablecoin:
  - tests/CMakeLists.txt: `include(LogosTest)`; `logos_test(NAME stablecoin_module_tests MODULE_SOURCES ../src/*.cpp TEST_SOURCES main.cpp *_test.cpp MOCK_C_SOURCES mocks/mock_stablecoin_ffi.cpp GENERATED_SOURCES ../generated_code/lez_core_api.cpp EXTRA_INCLUDES ../src ../ffi/include ../generated_code)`; main.cpp `#include <logos_test.h>` + `LOGOS_TEST_MAIN()`; flake `tests = { dir; mockCLibs = ["stablecoin_ffi"]; }`.
  - mock: `#include <logos_clib_mock.h>`; each ffi fn `LOGOS_CMOCK_RECORD(name); const char* r = LOGOS_CMOCK_RETURN_STRING(name);` malloc copy; `stablecoin_free` records + `std::free`.
  - test: `LOGOS_TEST(name) { LogosTestContext context("stablecoin_module"); LogosModules modules(context.api()); Impl module; module._logosCoreSetLogosModulesPtr_(&modules); context.mockCFunction("fn").returns(json_str); context.mockModule("lez_core","get_account_public").returns(str); ... LOGOS_ASSERT_EQ/LOGOS_ASSERT_TRUE; context.moduleCallCount("lez_core","get_account_public"); context.moduleCalledWith(...); context.cFunctionCallCount("fn"); }`. tests/logos_sdk.h adapts Qt client to std (`UniversalLezCore`).
- Rust: `modules/amm/ffi/tests/public_api.rs`, `src/api/tests.rs`; run by CI `cargo test --workspace`.

### 4.10 CI (.github/workflows/ci.yml, only workflow)
Triggers push main / PR. Jobs: Rust Format (`cargo +nightly fmt --all -- --check`), TOML Format (`taplo fmt --check .`), Lint (`make clippy`, `make clippy-guest`), Unit Tests (`cargo test --workspace --exclude integration_tests`), Integration Tests (`cargo test -p integration_tests`), Build Guest Programs (`make build-programs` + check `target/guest/<p>.bin`), Check IDL (`cargo build -p idl-gen --release`, diff IDLs). **No Nix, Qt, module, UI, or header-drift jobs.** README mentions a release CI building `module_path=apps/amm` / `modules/amm` with `nix build .#lgx-portable` (not in this repo).

### 4.11 Build/run/install docs
- apps/amm/README.md: standalone `nix run .#amm-ui` (from repo root, absolute paths); `nix build .#amm-module`, `nix build .#amm_ffi`; Basecamp: `nix build '.#lgx'` / `'.#lgx-portable'`, wallet core `nix build 'github:logos-blockchain/logos-execution-zone-module?rev=d70225ce…#lgx'`, `lgpm --modules-dir "$BASECAMP_DIR/modules" install --file result-core/*.lgx` then UI lgx (`BASECAMP_DIR=$HOME/.local/share/Logos/LogosBasecampDev` on Linux); Basecamp via `nix build 'github:logos-co/logos-basecamp' -o ~/.basecamp-result`. Env: `AMM_PROGRAM_BIN`, `TOKENS_CONFIG`, `AMM_POOLS_CONFIG`, `CUSTOM_TOKEN_CONFIG`, `AMM_REGISTRY_URL`, `LEE_WALLET_HOME_DIR` (default `~/.lee/wallet`). Inside Basecamp the backend adopts the shared open wallet.
- modules/amm/README.md: env on host process (`AMM_PROGRAM_BIN` required, `AMM_DEBUG`); headless `logoscore -D -m ./modules --persistence-path ./data`, `logoscore load-module lez_core`, `load-module amm_module`, `logoscore call lez_core open <cfg> <storage>`, `logoscore call amm_module resolvePoolAccount <a> <b>`; big amounts quoted `'"1000…"'`. "Core modules get no .lgx from the builder" -> hand-staged dir (note: root flake does alias `amm-module-lgx`). Full runbook `docs/module/amm.md`; byte-string note `docs/amm-swap-qtro-serialization-bug.md`.

<!-- §5-7 from the lez_core / Basecamp / design-system survey -->
## 5. lez_core module @ acf0cd50

Repo: `github:logos-blockchain/logos-execution-zone-module`, commit `acf0cd5` "chore: bump version to 0.4.1"
(parents: `c9d3e6d` bump LEZ to r0 fix + 0.2.4, `a29c878` bump lez to v0.2.4, `b60be46` byte-string fix).
Files: `src/lez_core_module.h`, `src/lez_core_module.cpp`, `metadata.json`, `flake.nix`, `tests/`, `doctests/`, `config/testnet.config.yaml`.

### metadata.json
`name: lez_core`, `version: 0.4.1`, `type: core`, `interface: universal`, `codegen.impl_header: src/lez_core_module.h`,
`impl_class: LEZCoreModule`, `main: lez_core_plugin`, `dependencies: []`, external lib `wallet_ffi`, runtime `nlohmann_json`.

### LEZ dependency
`flake.nix:7`: `logos-execution-zone.url = "github:logos-blockchain/logos-execution-zone?ref=erhant/0.2.4-with-r0-fix"`;
flake.lock rev `9edf4a622fe966199beed71685c6f0b855db0784` = `git describe` **v0.2.4-1-g9edf4a62** (v0.2.4 + "fix(nix): fetch risc0 crate from static.crates.io"). Confirmed LEZ v0.2.4. (wallet-ffi crate `lez/wallet-ffi/Cargo.toml` version 0.1.0.)

### Interface (`src/lez_core_module.h`, universal; Qt glue generated by logos-cpp-generator:
std::string->QString, int64_t->int, LogosList->QVariantList; header comment l.13-20)
No Q_INVOKABLE/.rep in repo; **no events/signals emitted** (no emit/eventResponse anywhere in src).

| line | signature | returns |
|---|---|---|
| 29 | `std::string name() const` | "lez_core" |
| 30 | `std::string version() const` | "0.3.0" (!) |
| 33 | `std::string create_new(const std::string& config_path, const std::string& storage_path, const std::string& statistics_path, const std::string& password)` | mnemonic, "" on fail/already open |
| 34 | `int64_t open(const std::string& config_path, const std::string& storage_path, const std::string& statistics_path)` | SUCCESS(0)/INTERNAL_ERROR; fails if already open |
| 35 | `int64_t save()` | WalletFfiError |
| 37 | `int64_t restore_storage(const std::string& mnemonic, const std::string password, uint32_t depth)` | WalletFfiError |
| 40 | `std::string create_account_public()` | account id hex (32B), "" on fail |
| 41 | `std::string create_account_private()` | same |
| 42 | `LogosList list_accounts()` | JSON array of `{"account_id":hex,"is_public":bool}` |
| 45 | `std::string get_balance(const std::string& account_id_hex, bool is_public)` | **decimal string** (u128 LE16 → decimal), "" on fail |
| 46 | `std::string get_account_public(const std::string& account_id_hex)` | JSON `{"program_owner":hex32,"balance":hexLE16,"nonce":hexLE16,"data":hex}` |
| 47 | `std::string get_account_private(const std::string& account_id_hex)` | same shape |
| 48 | `std::string get_public_account_key(const std::string& account_id_hex)` | |
| 49 | `std::string get_private_account_keys(const std::string& account_id_hex)` | JSON `{nullifier_public_key, viewing_public_key}` |
| 52 | `std::string account_id_to_base58(const std::string& account_id_hex)` | |
| 53 | `std::string account_id_from_base58(const std::string& base58_str)` | |
| 56 | `int64_t sync_to_block(int64_t block_id)` | WalletFfiError |
| 57 | `int64_t get_last_synced_block()` | 0 on error |
| 58 | `int64_t get_current_block_height()` | 0 on error |
| 61-63 | `claim_pinata(...)`, `claim_pinata_private_owned_already_initialized(..., int64_t winner_proof_index, const std::string& winner_proof_siblings_json)`, `claim_pinata_private_owned_not_initialized(...)` | transfer JSON |
| 66-71 | `transfer_public(from_hex, to_hex, amount_le16_hex)`, `transfer_shielded(from_hex, to_keys_json, amount_le16_hex)`, `transfer_deshielded(from_hex, to_hex, amount_le16_hex)`, `transfer_private(from_hex, to_keys_json, amount_le16_hex)`, `transfer_shielded_owned(from_hex,to_hex,amt)`, `transfer_private_owned(from_hex,to_hex,amt)` (all `const std::string&`) | transfer JSON |
| 72-73 | `std::string register_public_account(const std::string& account_id_hex)`, `register_private_account(...)` | transfer JSON |
| 75-78 | `std::vector<uint8_t> authenticated_transfer_elf()`, `token_elf()`, `amm_elf()`, `ata_elf()` | ELF bytes |
| **85** | **`std::string send_generic_public_transaction(const std::vector<std::string>& account_ids, const std::vector<bool>& signing_requirements, const std::vector<uint8_t>& instruction, const std::string& program_id_hex)`** | generic tx JSON |
| 86 | `std::string send_generic_private_transaction(const std::vector<std::string>& account_ids, const std::vector<uint32_t>& instruction, const std::vector<uint8_t>& program_elf, const std::vector<std::vector<uint8_t>>& program_dependencies)` | generic tx JSON (uint32 vector -> opaque `any` over QtRO, see l.80-84) |
| 87 | `std::string send_program_deployment_transaction(const std::vector<uint8_t>& program_elf)` | |
| 90 | `std::string bridge_withdraw(const std::string& from_hex, const std::string& bedrock_account_pk_hex, uint64_t amount)` | |
| 93-95 | `get_vault_balance(owner_hex)`, `vault_claim(owner_hex, amount_le16_hex)`, `vault_claim_private(owner_hex, amount_le16_hex)` | |
| 98 | `std::string get_sequencer_addr()` | address string from wallet config, "" if null |
| 101 | `bool check_label_available(const std::string& label)` | false on error |
| 102 | `int64_t add_label(const std::string& label, const std::string& account_id_hex, bool is_private)` | always SUCCESS unless bad hex (!) |
| 103 | `std::string resolve_label(const std::string& label)` | `"Public/<hex>"` or `"Private/<hex>"`, "" on fail |
| 104 | `std::vector<std::string> get_all_labels_for_account(const std::string& account_id_hex, bool is_private)` | |

### send_generic_public_transaction detail (`cpp:973-1047`)
- Arg order: `account_ids` (hex32 each), `signing_requirements` (parallel bools, per account → `wallet_ffi_resolve_public_account(id, signing_requirements[i], &identity)`), `instruction` (bytes; regrouped as LE u32 words, `size/4`, trailing bytes dropped), `program_id_hex` (32-byte hex).
- Calls `wallet_ffi_send_generic_public_transaction(walletHandle, identities, n, words, nwords, program_id, &result)`.
- Returns `genericTransactionResultToJson` (`cpp:134-148`): `{"success":bool,"tx_hash":string,"secrets":[hex32],"error":string}`.
  Errors return the same shape with `success:false, tx_hash:""` and `error` text (e.g. `"send_generic_public_transaction: wallet FFI error N"`).
- Transfer JSON (`transferResultToJson`, `cpp:124-132`): `{"success","tx_hash","error"}`.
- No fee payer arg, no shard arg (those arrive in master, see below).

### Tx status / blocks / nonce
- wallet-ffi @9edf4a62 exports (all `wallet_ffi_*`): create_account_public/private, create_private_accounts_key, list_accounts, get_balance, get_account_public/private, import_public/private_account, bridge_withdraw, serialization_helper, send_generic_public/private_transaction, get_public_account_key, get_private_account_keys, account_id_to/from_base58, resolve_public/private_account, check_label_available, add_label, resolve_label, get_all_labels_for_account, account_id_for_public_pda / _private_pda, claim_pinata*, program_deployment, *_elf, sync_to_block, get_last_synced_block, get_current_block_height, transfer_*, register_*, get_vault_balance, vault_claim*, create_new, open, destroy, save, restore_data, get_sequencer_addr, free_*.
- **No tx-status/receipt, no get-block, no block-range** in FFI or module. Nonce only via `get_account_public(...).nonce` (hex LE16).
- Unexposed in module but present in FFI: `import_*_account`, `account_id_for_public_pda`/`_private_pda`, `serialization_helper`.

### LEE_WALLET_HOME_DIR (LEZ repo @9edf4a62)
- `lez/wallet/src/lib.rs:56` `pub const HOME_DIR_ENV_VAR: &str = "LEE_WALLET_HOME_DIR";`
- `lez/wallet/src/helperfunctions.rs:36-51` `get_home()` = `$LEE_WALLET_HOME_DIR` else `$HOME/.lee/wallet`.
- `fetch_config_path()` → `<home>/wallet_config.json`; `fetch_persistent_storage_path()` → `<home>/storage.json`; `fetch_statistics_path()` → `<home>/statistics.json` (l.53-76).
- `lez/wallet/src/lib.rs:109` wallet constructor "using HOME_DIR_ENV_VAR ... or user home"; `lez/wallet/src/cli/mod.rs:106`; `README.md:137`.
- `wallet-ffi/src` does not reference it; only the CLI path does. Module never reads env.

### Newer main (`51a6fee`, "release as 0.5.1", metadata 0.5.1, LEZ `?ref=v0.3.1`) — significant breaking diffs
- `open(config_path, storage_path)` / `create_new(config_path, storage_path, password)` (statistics_path dropped); `restore_storage(..., uint64_t depth)`; new `std::string wallet_dir()`.
- `send_generic_public_transaction(account_ids, signing_requirements, instruction /*Borsh bytes, passed untouched*/, program_id_hex, const std::string& payer_account_id_hex, const std::vector<std::string>& shard_program_account_ids_hex)` (6 args).
- New `bool poll_transaction_status(const std::string& tx_hash_hex)` (tx-status exists only on main).
- `send_generic_private_transaction` gains `program_kind_json`, `dependency_kinds_json`, shard args; `send_program_deployment_transaction(header_account_id_hex, segment_account_ids_hex, program_elf, bool immutable, payer_account_id_hex)`.
- Removed: claim_pinata*, register_public/private_account, *_elf getters, vault_* calls.
- Instruction encoding changes from risc0 u32-words-as-LE-bytes to Borsh → incompatible with currently deployed guests (lez-programs issue #351 per flake comment).

---

## 6. Basecamp standalone mode, packaging, test tooling

### Commits
- master head: `e09962f5998792afd3bfed8c69c3b8f07f5825a5` 2026-10-15 "test(intents): stop the breaker tests racing the wall clock (#505)".
- `cf16f9dae427f041d69a85389aee1c9b3fdda8db` 2026-10-06 "feat: standalone mode (--module) for developing an app in Basecamp" — ancestor of master. Adds `app/utils/StandaloneMode.{h,cpp}`, `app/utils/HostProfile.{h,cpp}`, `app/QmlHotReload.{h,cpp}`, `nix/mkPluginTest.nix`, `.#app-standalone`.
- **PR #506** "feat: allow adding more apps when running basecamp standalone for dev purposes", by Khushboo-dev-cpp, MERGED 2026-10-13, merge commit `63c1643f`, branch `feat/basecampStandaloneLoadOtherApps`. Adds `nix/dev.nix` (`nix run .#dev`), `nix/devDeps.nix`, `--app`/`--dep`/`--dep-package` launcher flags, `DependencyBlocker.h` tweak, tests in `tests/dependency_gate_test.cpp`, `tests/standalone_mode_test.cpp`. (Local WSL clones ~/logos-basecamp 1f56c29 and ~/basecamp-build 01d3f59 predate both.)

### CLI (`app/main.cpp:166-230`, QCommandLineParser.parse(); unknown flags tolerated)
- `-u/--user-dir <path>` (= env `LOGOS_USER_DIR`, used verbatim, no "Dev" suffix)
- `--access-policy enforce|path|json` (env `LOGOS_ACCESS_POLICY`)
- `--uri <basecamp://...>`, `--new-instance`
- `--module <install root>` (repeatable): dir with `modules/` and/or `plugins/` (output of `nix build .#install`). A dir containing `manifest.json` directly is **refused** (`StandaloneMode.cpp:89-95`). Every UI module (manifest `type` starts with "ui") under `plugins/` opens as a tab.
- `--modules-dir <dir>` (repeatable): deps available but not opened; if no modules/plugins subdirs, dir used as both roots.
- `--qml-source <app>=<dir>`: load view from source dir + hot-reload on .qml/.js/.mjs save; `LOGOS_QML_HOT_RELOAD=0` disables watching.
- `--width`, `--height`. `--modules-dir`/`--qml-source` without `--module` → exit 1 (`main.cpp:283-287`).
- Order: modules-dir first, `--module` last (later root wins name clash).
- `applyStandaloneCapabilities` (`StandaloneMode.cpp:119-131`): launcherLimitedToOpenedApps, useUserModules=false, packageCatalog=false, repositoryManagement=false, registerUrlScheme=false, singleInstance=false (→ implies --new-instance), defaultUserDir=`./.logos-basecamp-dev`, applicationName=`LogosBasecampStandalone` (QSettings), devHost, enforceDependencyVersions=false (out-of-range dep loads with warning; missing dep still blocks).
- Only `package_manager` (+capability_module, modules_state) loaded; no downloader/App Manager/catalogue. Bundled copies lose to --module/--modules-dir copies.
- Basecamp installs nothing: every `metadata.json` dependency must come via `--modules-dir` (or `nix run .#dev`).

### Launch recipes (README.md:132-272)
```
nix run .#app-standalone -- --module ../my-app/result-install --modules-dir ../dep/result-install --qml-source my_app=../my-app/src/qml
nix run .#dev -- --module <root> [--dep-package <name>=<dir with .lgx>] [--dep <name>=<flake ref>] [--app <name>=<flake ref>] [--test <file.mjs>] [Basecamp flags]
```
- Builds: `.#app` (dev, full), `.#bin-bundle-dir` (portable), `.#app-standalone` (dev; only package_manager, capability_module, modules_state), `.#app-standalone-portable` (portable, inspector on). App must match build variant: dev ↔ module `#install`, portable ↔ `#install-portable`.
- From a module repo (logos-module-builder): `nix run .` / `nix build .#ui-dev && ./result/bin/run-logos-standalone-ui` / `nix run .#ui-tests` (tests/*.mjs headless). Env `LOGOS_BASECAMP=<flake>` selects Basecamp flake (default `github:logos-co/logos-basecamp#dev`); `LOGOS_BASECAMP_DEV` = prebuilt. `--test` uses `LOGOS_TEST_TIMEOUT` (s/file, default 120).
- `lib.<system>.mkPluginTest` (`nix/mkPluginTest.nix`): runs app-standalone offscreen with `--module ${installPkg} --modules-dir ${depsRoot}`, then `node <tf> --ci <launchScript> --verbose`, with `QT_QPA_PLATFORM=offscreen`, `LOGOS_QT_MCP=<pkg>`, fresh user dir per file.

### .lgx build + install
- Modules (logos-module-builder, head a0de2d34): `nix build .#lgx`, `.#lgx-portable`, `.#install` / `.#install-portable` (install roots), `.#lib`, `.#include`, `nix run .` (docs/quick-reference.md:347-367). Basecamp bundles modules via `nix-bundle-logos-module-install` (`flake.nix:181-182` bundlers `.dev` / `.portable`); lez module uses `nix-bundle-lgx`.
- lez_core lgx: `nix build 'github:logos-blockchain/logos-execution-zone-module?rev=acf0cd50...#lgx'`.
- CLI `lgpm`: `nix profile install 'github:logos-co/logos-package-manager#cli'`; then
  `lgpm --modules-dir "$DIR/modules" install --file core.lgx` and `lgpm --ui-plugins-dir "$DIR/plugins" install --file ui.lgx` (lez-programs `apps/amm/README.md:45-110`).
- Data dirs: dev builds `~/.local/share/Logos/LogosBasecampDev` (Linux) / `~/Library/Application Support/Logos/LogosBasecampDev` (macOS); portable builds `.../LogosBasecamp`; standalone `./.logos-basecamp-dev`. Subdirs `modules/`, `plugins/`, `module_data/`, `logs/`.
- GUI: Package Manager → "Install from file" (core .lgx first, then UI .lgx). Not available in standalone mode.

### logos-qt-mcp (pinned by basecamp master: `89d1d498d37700ccdc864fe93cd98ec840f95ffb`, 2026-10-10)
- Port: default **3768**; env **`QML_INSPECTOR_PORT`** (qt-plugin `inspectorserver.cpp:87`, `inspectorserver.h:32`; basecamp `main.cpp:582-584` `InspectorServer::attach(mainWindow)` under `ENABLE_QML_INSPECTOR`). Client env: `QML_INSPECTOR_HOST` (localhost), `QML_INSPECTOR_PORT`, `QML_INSPECTOR_TIMEOUT`.
- `test-framework/framework.mjs` exports: `class Inspector` (`connect()`, `send(command, params)`), `class App` (`click(text, opts)`, `screenshot()`, `getTree(opts)`, `listInteractive()`, `listFileDialogs()`, `fileDialogAction(objectId, action, path)`, `findByProperty(property, value)`, `getProperties(objectId)`, `expectTexts(texts)`, `expectStatus(expected)`, `getPropertyByType(typeName, propName)`, `expectProperty(typeName, propName, expected)`, `waitFor(fn, {timeout=5000, interval=300, description})`), `test(name, fn, {skip})` (l.286), `async run()` (l.348).
- CLI: `node <file> [filter] [--verbose|-v]` (app already running) or `node <file> --ci <app-binary> [filter] [--verbose] [--xvfb]`; mode from `QT_QPA_PLATFORM`.
- MCP tools: qml_call_method, qml_click, qml_evaluate, qml_file_dialog_action, qml_find_and_click, qml_find_by_property, qml_find_by_type, qml_get_properties, qml_get_tree, qml_list_file_dialogs, qml_list_interactive, qml_screenshot, qml_send_keys, qml_set_property.
- Basecamp `tests/ui-tests.mjs` (master: 57 tests; WSL ~/logos-basecamp: 9): loads framework from `process.env.LOGOS_QT_MCP || <repo>/result-mcp` + `test-framework/framework.mjs`; master wraps with `makeTest` from `tests/fixtures/harness.mjs` (exports `assertResponsive, findByObjectName, makeTest, pollProperty, sleep`; adds G-ERR/G-ALIVE epilogue + `{xfail}`), fixture `tests/fixtures/lgx.mjs` `FIXTURE_A`.

---

## 7. Logos design system QML

Repo `github:logos-co/logos-design-system`. QML under `src/qml/Logos/{Controls,Theme,Icons}/`.
Modules (qmldir): **`Logos.Controls 1.0`**, **`Logos.Theme 1.0`** (`singleton Theme`, `ColorPalette`, `Spacing`, `Typography`, `DarkTheme`), **`Logos.Icons 1.0`** (`singleton LogosIcons`). Import e.g. `import Logos.Controls`, `import Logos.Theme`.

### Revs
| who | rev | date | Notice | StageLane | Link | Icon |
|---|---|---|---|---|---|---|
| lez-programs (transitive, most nodes) | f6e95ae24ede3871c380f8250feffd17d173f5a4 | 2026-05-05 | n | n | n | n |
| apps/amm root path (module-builder_32 → standalone-app_30) | 6176f0d7 | 2026-05-08 | n | n | - | - |
| modules/amm (module-builder_28) | 3312444e | 2026-08-20 | y | n | y | y |
| basecamp master root input (`logos-design-system_2`) | 88330d04968954be956311b0be429048a41da2ea | 2026-09-22 | y | y | y | y |
| basecamp's module-builder | e43a4817 | 2026-09-17 | y | y | y | y |
| design-system HEAD | a4f3014 | 2026-10-12 | y | y | y | y |

### Requested components @ f6e95ae (lez-programs pin)
Existing (all `src/qml/Logos/Controls/<Name>.qml`, `Logos.Controls 1.0`):
- `LogosTabBar` (TabBar): indicatorColor, indicatorHeight=3, animationDuration=200, alias indicatorItem.
- `LogosTabButton` (TabButton): activeColor, inactiveColor, iconSource:url, aliases iconItem/labelItem.
- `LogosItemDelegate` (ItemDelegate): hoverColor, pressedColor, highlightColor, textColor, radius.
- `LogosBadge` (Control): text, iconSource, color=accentOrange, backgroundColor, borderColor, radius, borderWidth.
- `LogosDialog` (Dialog): borderColor, backgroundColor, `list<Item> leftActions/rightActions`, aliases header/footer.
- `LogosTextField` (**Control**, not TextField): `alias text`, placeholderText, placeholderTextColor, echoMode; `textInput` alias for signals/validator.
- `LogosSwitch` (Switch): trackColorOn/Off, handleColor.
- `LogosButton` (**Control**): `alias text`, radius, `isActive`, `signal clicked()`; uses `enabled`.
- `LogosComboBox` (ComboBox): placeholderText, textColor, indicatorColor, popup aliases.
- `LogosProgressBar` (ProgressBar): trackColor, fillColor.
Other @f6e95ae: LogosIconButton, LogosText, LogosCheckbox, LogosSearchBar, LogosPaginator, LogosTable, LogosTableColumn, LogosSpinner, LogosScrollBar, LogosTextArea, LogosRadioButton, LogosSlider, LogosMenu, LogosMenuItem, LogosToolBar, LogosToolButton, LogosFrame, LogosGroupBox, LogosSpinBox, LogosScrollView, LogosToolTip, LogosDrawer, LogosStackView, LogosMenuSeparator, LogosToolSeparator.

**Missing @f6e95ae**: LogosNotice, LogosStageLane, LogosLink, LogosIcon. Nearest alternatives: Notice → LogosBadge / LogosFrame + LogosText (or LogosDialog for blocking); StageLane → LogosProgressBar or LogosTabBar (read-only) / Row of LogosBadge; Link → LogosText + MouseArea (or LogosToolButton); Icon → `Image` + `LogosIcons.<name>` or LogosIconButton.

Added later (HEAD signatures; present in basecamp's 88330d04):
- `LogosNotice` (Control; d59e9b4 2026-08-19): `enum Severity {Info, Success, Warning, Error}`, severity, title, message, closable, `list<Item> actions`, shown, `signal dismissed()`. `LogosToast` extends it (duration=4000).
- `LogosStageLane` (Item; ee39228 2026-09-16): `list<LogosStage> stages`, currentIndex=-1, busy, failed, interactive, pulseDuration=2800, `count`, `signal stageClicked(int index)`; `LogosStage` (QtObject): label, busyLabel, enabled.
- `LogosLink` (Control; 5c30e6e 2026-07-28): text, href:url, linkColor, hoverColor, underline, elide, `signal activated(url href)`.
- `LogosIcon` (Item; 4a37b34 2026-06-08): source:url, color, brightness.
- Also new by HEAD: LogosAbstractButton, LogosArtwork, LogosTile, LogosSelectableText, LogosCopyButton, LogosCopyableText, LogosWarningDialog, LogosStatCard, LogosInfoButton, LogosListView, LogosDotMatrix.

### Theme tokens (@f6e95ae, `Logos.Theme`)
- `Theme` singleton: `Theme.palette.*` (semantic, from DarkTheme), `Theme.colors` (ColorPalette, `getColor(baseColor, alpha)`), `Theme.spacing`, `Theme.typography`, `isDark`, `currentTheme`, `themeNames`, `settings.theme` ("dark").
- `Theme.palette`: background, backgroundSecondary, backgroundTertiary, backgroundElevated, backgroundMuted, backgroundBlack, backgroundInset, backgroundButton, surface, surfaceRaised, surfaceRecessed, surfaceContrast, text, textSecondary, textTertiary, textPlaceholder, textMuted, border, borderSecondary, borderTertiary, borderTertiaryMuted, borderSubtle, borderHairline, borderInteractive, borderDark, borderStrong, primary, primaryHover, primaryPressed, primarySoft, success, successHover, successPressed, error, errorHover, errorPressed, warning, warningHover, info, notification, accentOrange, accentOrangeMid, accentOrangeDeep, accentBurntOrange, accentYellowSoft, hover, pressed, disabled, focus, glassOverlay, glassStrong, overlayDark, overlayLight, overlayOrange.
  HEAD adds: surfaceInteractiveHover, textSubtle, borderGlyph, successBorder, errorBorder, warningBorder, primaryBorder, scrim; primaryPressed changed orange600→orange800.
- `Theme.colors` raw: white, whiteOpacity06/63, black, blackOpacity10, gray50…gray975 (+gray550Opacity07/20/30), orange300-700 (+orange400Opacity30), peach100, red400-700, yellow300-500, green400-800, blue400-700, cyan400/500.
- `Theme.spacing`: tiny 4, small 8, medium 12, large 16, xlarge 20, xxlarge 40; radiusSmall 4, radiusMedium 6, radiusLarge 8, radiusXlarge 16, radiusPill 999 (HEAD adds radiusTiny 2).
- `Theme.typography`: publicSans (font family), weightRegular 400 / weightMedium 500 / weightBold 700; sizes mainTitleText 256, pageTitleText 36, titleText 30, panelTitleText 24, subtitleText 16, primaryText 14, secondaryText 12 (HEAD adds `mono: "monospace"`, badgeText 8).
- `LogosIcons` (Logos.Icons): arrowLeft, arrowLeftDouble, arrowRight, arrowRightDouble, triangleUp, triangleDown, refresh, install, trash, more, search.

### Skew implications
- Code using LogosNotice/StageLane/Link/Icon compiles inside Basecamp master (88330d04 on host QML2_IMPORT_PATH) but **fails** under lez-programs' standalone runner (`nix run .#amm-ui`, f6e95ae/6176f0d7). Conversely an older Basecamp release with an older design system will fail to compile such a view silently (spinning icon; see memory note on ui_qml load failures).
- lez-programs apps already import `Logos.Controls`, `Logos.Theme`, `Logos.Icons`, plus its own `Logos.Wallet`.

## 8. Disk / RAM (measured 2026-10-18)
- C: free **28.6 GB** (Get-PSDrive: Used 482.1 GB, Free 28.60 GB). Design's stop line: < ~10 GB.
- WSL `Ubuntu-20.04` (WSL2, OS Ubuntu 24.04.4) disk `C:\Users\user\AppData\Local\Packages\CanonicalGroupLimited.Ubuntu20.04LTS_79rhkp1fndgsc\LocalState\ext4.vhdx` = **136.5 GB** on C:; inside: `/` 251 G size, 131 G used, **108 G avail** (vhdx grows into C:'s 28 GB, so the real limit is C:). `docker-desktop` distro stopped.
- `~/lez-target` = **73 GB** (shared CARGO_TARGET_DIR plan2 + others): the main reclaimable space (deleting inside WSL does not shrink the vhdx; needs `wsl --shutdown` + `Optimize-VHD`/diskpart compact to return space to C:).
- `/nix/store` 6.2 GB; Nix 2.34.8. 8 vCPUs. RAM: host 15.8 GB; WSL 7.8 GiB + 16 GiB swap.
- WSL home dirs: `~/logos-basecamp` (1f56c29), `~/basecamp-build/logos-basecamp` (01d3f59), `~/lez-spikes/s1/lez-programs` (64c0121 = 64c01210), `~/lez-spikes/s1/{lez,spel,wallet-home,seqhome,env.sh}`, `~/apps/` (Basecamp AppImages v0.2.3/v0.3.0/v0.3.2 + logs). Windows: `C:\Users\user\Documents\basecamp\logos-basecamp` (01d3f59).
