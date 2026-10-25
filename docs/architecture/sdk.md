# Client SDK (`crates/auction_sdk`)

The SDK is the only way the CLI and the FFI (`crates/auction_ffi`, Plan 4) talk to the engine. It is synchronous and generic over one trait.

## Chain

`chain::Chain` = `account`, `nonces`, `last_block_id`, `blocks(from..=to)`, `send`. Implementations: `testing::FixtureChain` (in memory, feature `testing`); `WorldChain` in `programs/integration_tests/tests/common/sdk.rs` (the real guests in an in-process `V03State`, Task 8); `rpc::RpcChain` (sequencer JSON-RPC, feature `rpc`, Task 12). An uninitialised account is `Account::default()`, never an error. `send` success means accepted, not executed (spec §3.2).

## Encoding

`encode::words(&ix)` is `lee::program::Program::serialize_instruction` (risc0 serde) of the program's own instruction enum, so the first word is the enum index the guest decodes. Never the IDL position: `idl/stablecoin-idl.json` differs from `stablecoin_core::Instruction` for positions 8..=18 (`STABLECOIN_IDL_POSITION_TO_ENUM`). Signed `i128` controller gains encode (spel cannot, ADR-018).

Drift tests: `encode::tests` (SDK) and `programs/integration_tests/tests/idl.rs`. Both use one shared source for the stablecoin sample set, the IDL-name extraction (`testing::support`, `#[doc(hidden)]`, feature `testing`) and the position-to-enum table (`encode::STABLECOIN_IDL_POSITION_TO_ENUM`); nothing is duplicated.

## Plans

`plan::Plan { program_id, account_ids, signing_requirements, instruction_words }`; `Plan::new` rejects duplicate accounts, unlisted signers and signer-less plans.

The `*_is_fresh` flags of the builders (`keeper_holding_is_fresh`, `bidder_collateral_is_fresh`, `receiver_is_fresh`, `owner_holding_is_fresh`) are caller-trusted: the builder cannot know whether a holding exists, so a wrong flag produces a plan the chain rejects (`RecipientMustSign` for a missing signature, or a signature on an initialised account), never a silently wrong transfer.

## RpcChain (feature `rpc`)

`rpc::RpcChain` is the LEZ v0.2.4 `sequencer_service_rpc` HTTP client (jsonrpsee 0.26 `HttpClient`) behind `Chain`, with its own current-thread tokio runtime; every call blocks, so it must not run inside another tokio runtime. The default SDK build pulls no HTTP stack: `rpc` enables the optional `sequencer_service_rpc`, `common` and `tokio` dependencies (all already in `Cargo.lock` through LEZ, except the jsonrpsee tree that LEZ's `sequencer_service_rpc` `client` feature (`jsonrpsee/client`) brings, including target-gated crates such as gloo and jni that do not compile on Linux or Windows hosts; same git tag, nothing re-pinned).

Methods used (verified in `lez/sequencer/service/rpc/src/lib.rs`): `getAccount`, `getAccountsNonces`, `getLastBlockId`, `getBlockRange`, `sendTransaction`. Hardening: one overall request timeout (default 30 s; the HTTP client has no separate connect timeout, so it bounds the connect too), response limit 16 MiB, request limit 2 MiB, `blocks` ranges of at most 256 blocks, replies checked against the request (nonce count, block count and ids), server text copied into errors truncated to 200 characters. Scan batch x block size must stay under the 16 MiB response limit: the default batch of 100 blocks times the LEZ `max_block_size` of 1 MiB can exceed it on a busy chain, and the failure is a `Chain` error; lower the scan batch (`--batch` in the CLI). Calling `RpcChain` from inside a tokio runtime is refused with a `Chain` error (before any send) instead of panicking. Idempotent reads (`getAccount`, `getAccountsNonces`, `getLastBlockId`, `getBlockRange`) are retried on a transient failure (transport error, HTTP 5xx, timeout, unparsable reply) by `ReadRetry::DEFAULT`: 5 tries, waiting 2, 4, 8 and 16 s (30 s in all); a JSON-RPC error object is definite and not retried, and the error names the number of attempts. `send` is never retried: a resend could act twice.

`send` error mapping. `SendUncertain` keeps the in-flight lock and says to check the nonce before resending; a plain `Chain` error means the transaction was definitely not accepted.

| Outcome of `sendTransaction` | `SdkError` |
|---|---|
| JSON-RPC success | `Ok(hash)` |
| JSON-RPC error code -32602 (`InvalidParams`: stateless check failed, transaction too large, sequencer-only program), answered before the mempool | `Chain` (definite rejection) |
| TCP connect refused, or host name did not resolve (found in the transport error's source chain) | `Chain` (provably never sent) |
| Transaction over 1 MiB borsh, refused locally | `Chain` (nothing written) |
| Any other JSON-RPC error (e.g. -32603 internal) | `SendUncertain` |
| HTTP 4xx/5xx status | `SendUncertain` |
| Garbled or unparsable response | `SendUncertain` |
| Connection closed or reset after the request was written | `SendUncertain` |
| Timeout (cannot tell connect from after-write) | `SendUncertain` |
| Any transport error not proven pre-send | `SendUncertain` |

Each row has a test against a local mock HTTP server (`rpc_tests.rs`, no external network; the DNS case uses the reserved `.invalid` name). The live test `live_rpc_reads_height_clock_and_blocks` is `#[ignore]`d; against the localnet of `scripts/localnet-smoke.sh` (sequencer on 127.0.0.1:3040, started by `step_sequencer`), run inside WSL: `LEZ_RPC_URL=http://127.0.0.1:3040 scripts/wsl-cargo.sh test -p auction_sdk --features rpc,testing -- --ignored live_rpc`.

## Module map

| Module | Task | Purpose |
|---|---|---|
| `chain`, `encode`, `error`, `plan`, `testing` | 4 | trait, encoders, errors, plan type, fixtures |
| `addresses`, `decode` | 5 | PDAs from the engine's and host's own functions; decoders that check initialisation, owner and tag |
| `deployment`, `verify` | 5 | `deployments/<network>.json` (must match `DEPLOYMENTS.md`, which is checked by hand); `verify_config` refuses a config not owned by the engine ImageID, not at `["CONFIG", admin, salt]`, or storing another host/token program, admin, `ProtocolParameters` or stablecoin definition, or whose host `ProtocolParameters` has another owner (I4, ADR-027); `verify_binding` checks the host binds this config |
| `testing::standard` | 5 | the "standard world" fixture (`standard()`, `deployment()`, `config_id()` and the shared constants) used by SDK, CLI and golden tests; built from direct account inserts, no program runs |
| `quote` | 6 | liquidation, bid, surplus-take and debt-take quotes on the handler's own path; auction view (held at max discount, paused reason); slippage helpers; next debt start (ADR-039) |
| `plan` (builders) | 7 | one builder per engine instruction (Shared Reference S1 lists, checked against `idl/auction_engine-idl.json`); fresh recipient holdings added as signers; slots from the config (ADR-030); `bid` vs `bid_with_settlement` chosen by the caller from `BidQuote::needs_settlement` (ADR-029) |
| `plan_host` | 7 | stablecoin (incl. signed `i128` controller gains and `set_market_price_oracle`, enum index 10 / IDL position 16, accounts `[admin, protocol_parameters, new_oracle]`), token and twap_oracle builders used by the CLI admin/host/dev commands; encoded by enum index, never the IDL position |
| `oracle` | 3b-10 | localnet price publisher support (ADR-050): `collateral_args_from_state` (the stored config as `set_collateral_config` arguments), `check_new_price` (the engine's own `feed::observe` + `feed::require_live` on a hypothetical oracle, so a price outside `[R/2, 2R]` is refused before signing) and `free_oracle_window` (next unused price-account window, at most `MAX_WINDOW_PROBES` reads) |
| `keystore` | 9 | the wallet's `storage.json` read directly; labels or base58 ids; keys verified against their ids; private accounts refused |
| `tx` | 9 | `KeySource`, `sign(plan, nonces, keys)` |
| `confirm` | 10 | nonce-polling confirmation (Included / Dropped after 5 blocks), one in-flight transaction per signer via a lock file, `submit` = lock + sign + send + confirm |
| `scan` | 11 | block scan with a persisted, atomically written cursor anchored to its chain (block 2's hash); a replaced chain is rescanned from block 1; rewinds when the cursor's block hash changed, only as far as it can prove; fails (cursor unchanged) on a missing or non-extending block or an unprovable rewind |
| `rpc` | 12 | RpcChain over sequencer_service_rpc (getAccount, getAccountsNonces, getLastBlockId, getBlockRange, sendTransaction); blocks summarised to their public transactions |
| `confirm` (`track_begin`, `poll_once`, `track_end`) | P4-2 | non-blocking confirmation for callers that must not block (ADR-053): take the lock, one poll step, release on a definite outcome; shares `verdict()` and the lock removal with `confirm` |
| `ops` | P4-7 | the orchestration the CLI used to hold: one `prepare_*` per action returning `Prepared { steps, facts, touched }` (quote → min-out → needs-settlement → freshness → plan); the CLI and the FFI both call it |
| `views`, `dec` | P4-8 | the read models the screens and the CLI print from: `accounting`, `host`, `oracle`, `auctions`/`auction_entry`, `positions`/`at_risk`, `accounts`; u128 serialised as decimal strings (`dec`); `scan::scan_page` for paged scans |

## Program-id verification (I4)

Every transaction for an engine config starts with `verify_config(chain, &deployment, &config)`; `admin init-config` and the host/dev commands, which have no config to verify yet, do not. It reads the config account through `Chain` and compares it with the deployment file, so the check does not rest on caller-supplied ids alone. A mismatch is `SdkError::Mismatch { what, expected, found }` (CLI exit 2).

Chain identity (ADR-046): `verify_chain_identity(chain, &deployment)` compares the hash of the node's block 1 with the deployment file's `chain_genesis_hash` (`Deployment.chain_genesis`, 64 lowercase hex characters, validated when the file is parsed). It returns `Mismatch` (CLI exit 2) when the file records none or the hashes differ. LEZ v0.2.4 builds block 1 from the sequencer config, so it identifies the chain's configuration, not one run. The CLI calls it before every state-changing dev command; it is separate from `verify_config`.

Trust boundary: the RPC endpoint is trusted. There are no state proofs, so a malicious sequencer URL can serve a consistent set of fake accounts that passes every check; the deployment file and the endpoint come from the operator.

`engine_admin: null` in a deployment disables the admin check (configs are permissionless, ADR-027), so operators should set it. The CLI (Tasks 13 and 15) prints the config's admin before signing.

`verify_config` deliberately does not check the collateral definition, the `CollateralConfig` oracle id, or the protocol token and its oracle. Those are checked at use time: `Reader::oracle` (owner = twap oracle program when the deployment names it), `Reader::token_definition` (owner = token program), `Reader::balance`, and on-chain by the engine's own account pinning.

The ImageIDs in `deployments/localnet.json` are the reproducible Docker-build values of `artifacts/guests/manifest.json` (ADR-047, Plan 3b Task 5); `deployments/localnet.json` is pinned to the manifest by the test `guest_manifest_matches_the_localnet_deployment`; `DEPLOYMENTS.md` carries the same values and is kept equal by hand (no test reads it).

## Quotes

A quote is the handler's pricing path run on fetched accounts at a chosen `now` (default: the chain clock, `chain::clock_ms`). It calls `auction_engine_program::feed::{redemption_price, observe, require_live}` and the `auction_core` functions in the handler's order, so `t_eff` always comes from the clock *after* simulating `observed()` (ADR-024): an unobserved stale gap freezes the discount in the quote exactly as it will on-chain. A quote is valid only at its `now`; the on-chain result at inclusion time can differ (discount moved, oracle moved). Pass the `CLOCK_01` value the transaction will execute under (`chain::clock_ms`, the previous block's timestamp), not wall-clock time; `clock_ms` is that value only if the transaction lands in the very next block, otherwise the clock has moved, which the slippage bound covers. Quotes predict pricing and engine/host state checks, including the host hook's "position is collateralized" refusal (`HostReject`), but not the caller's balances or recipient shape: takes can still revert with `InsufficientBalance`, `VaultBelowBook`, `RecipientMustSign`; bids with `VaultBelowBook`, `InsufficientBalance`, `RecipientMustSign`, and `ChainEndAccountsMissing` (avoided by `needs_settlement`). This is why every transaction command derives `min_collateral_out` / `max_protocol_in` / `min_protocol_out` from the quote with a slippage margin (Review Focus 1, `sdk_quotes.rs`).

## Proof on the real guests

`programs/integration_tests/tests/sdk_plans.rs` runs every engine, host and token builder through `World::run_plan` against the real token, stablecoin and engine guests (liquidate, partial and final bid, claim, observe, settle_debt, check_solvency, surplus and debt auctions, init/bind/collateral/params, signed controller gains). `sdk_quotes.rs` pins quote == on-chain result: the liquidation quote equals the opened auction, a bid quote taken at the inclusion clock equals the fill, and a quote taken earlier protects the bidder through `min_collateral_out` when the oracle moves (Review Focus 1). The harness uses the SDK's `encode::{init_config_ix, set_engine_params_ix, set_collateral_config_ix}` instead of its own copies, and `WorldChain` reads the in-process `V03State`; the clock a quote must use is the one the transaction executes under (in the World `World::now`, seeded into `CLOCK_01` by `submit`/`run_plan`, advanced by `World::advance`).

## Keystore

The SDK reads the LEZ wallet's `storage.json` itself (ADR-041); the `wallet` crate is not a dependency. The path is `--wallet-home` or `$LEE_WALLET_HOME_DIR`, then `/storage.json`. Format (LEZ v0.2.4 `lez/wallet/src/storage/persistent.rs`): `key_chain.accounts[]` entries `{"Public": {account_id, chain_index, data: {sk, ssk, pk, cc, cci}}}`, `{"ImportedPublic": {account_id, pub_sign_key}}`, `{"Private": {account_id, ...}}` and `{"ImportedPrivate": {...}}`, plus `labels: {label: {"Public"|"Private": account_id}}`. Public accounts sign with `ssk` (the wallet signs with `&ChildKeysPublic.ssk`; the account id derives from `PublicKey::new_from_private_key(&ssk)`), imported public accounts with `pub_sign_key`; every key is checked to derive its stored account id (`KeyMismatch` otherwise). Signatures are LEZ's own `WitnessSet::for_message` over `Message::hash`, verified by `WitnessSet::is_valid_for`.

Private accounts are recorded only to be refused (`PrivateAccount`). Upstream `ImportedPrivate` entries carry no `account_id`, so the parser skips them: an unlabelled imported private account fails as `NoKey`, a labelled one as `PrivateAccount`. A base58 account id resolves without a key (recipients); signing needs one.

The v0.2.4 wallet keeps keys unencrypted on disk, so protect the wallet home like a key file. Errors are typed (`KeystoreError`: `HomeNotSet`, `NotFound`, `Unreadable`, `Malformed`, `UnknownLabel`, `NoKey`, `PrivateAccount`, `KeyMismatch`; the CLI exits 4) and "Debug never prints key material": `Keystore`'s `Debug` lists the path, public account ids and labels only, and no error, `Display`, `Debug` or log contains a private key, seed, mnemonic or password (pinned by `keystore_formats_never_contain_key_bytes_in_any_error_or_debug`). `lee::PrivateKey` has no `Zeroize` upstream, so keys cannot be wiped from memory by this crate; nothing is written to disk.

## Confirmation

`sendTransaction` success means accepted, not executed (spec §3.2). `submit` reads the signers' nonces and the height, takes the primary signer's lock, then reads them again under the lock: if a nonce moved in between (another transaction of the signer landed), the lock is released and the read repeated (at most 3 attempts, then `InFlight`), so the baseline is never older than the lock. A re-read within one block of the drop window's end (`REBASE_MARGIN_BLOCKS`) also counts as stale and retries, because until the rebase renames the record the lock still says `h0` and another process could judge it stale (ADR-042, amended). It records the primary signer's nonce `n0` and the height `h0` from the read under the lock, and rewrites the lock record with that height (temp file renamed over the lock, only while this process owns it), so the lock and the confirmation share one baseline; it then signs, sends, then polls the nonce through `Chain` (the sleeper is injected, so tests do no real sleeping): a nonce above `n0` is `Included { nonce_after }`; still `n0` when the height reaches `h0 + 5` is `Dropped { height }`; no verdict after `max_polls` (default 900 at 2 s) is an error that says to check the nonce before sending again. Each poll reads the height first and the nonce second (the sequencer moves the nonce before the tip). A failed poll read (each already retried by `RpcChain`) does not end the wait: up to `MAX_CONSECUTIVE_POLL_ERRORS` (5) failures in a row are waited through, then the outcome is `Unconfirmed` (lock kept, "check the nonce before acting"); an empty nonce reply counts as such a failure and is never read as nonce 0. The drop window is `ConfirmSettings::drop_after_blocks` (default 5; the CLI's `--drop-after-blocks`). `Dropped` means not applied by `h0 + 5`; a backlogged transaction could still land later, but a re-quote reuses nonce `n0`, so at most one of the two applies. `Included` means some transaction of this signer at nonce `n0` applied, not necessarily this hash (a double-proceed race on the lock, two CLIs with different state directories, or a transaction sent outside the lock such as by the wallet CLI), so re-read the accounts you touched; `poll_transaction_status` after the v0.3 port is the hash-level fix. A `Dropped` transaction is never resent by the SDK: re-quote first (the CLI prints "check the signer's nonce before acting; if it did not move, nothing was applied: re-quote and resend" and exits 3). The spec's "target account changed" check is not implemented (ADR-042): on v0.2.4 the signer nonce advances only in `State::apply_state_diff`, after the transaction validated and executed, so a higher nonce already proves application.

Lock: `<state dir>/locks/<signer>.lock` holds `{nonce, height, owner}`. It is created atomically across processes on Linux and Windows (written to a temp file, hard-linked into place; `create_new` where hard links are unavailable) and released on drop by the process that still owns it, on a definite verdict only: Included, Dropped, a definite send rejection, or a failure before the send. A timeout, an unknown send result (`SdkError::SendUncertain`, to be returned by chain implementations for transport failures after the request may have been delivered) or a panic keeps the lock, which then clears by the staleness rule within 5 blocks. A lock is stale when the signer's nonce moved past its nonce or the height reached its `height + 5`, or it does not parse; a live, in-window lock is never replaced and the second submission fails with `InFlight` before anything is signed or sent. Staleness uses chain facts, not process ids or file ages, because pids do not transfer between WSL, Windows, containers and machines and are reused.

## Discovery

`scan::scan(chain, targets, cursor, settings)` returns a new cursor and never mutates its input, so a failed scan leaves the caller's cursor as it was. `scan::scan_report` returns the same cursor plus `restarted: Option<Restart>`, set when the stored cursor was discarded (the CLI prints it as one `note:` line on stderr). `scan::start_at(chain, cursor, from)` positions a cursor for `rescan --from`; like `scan`, it resets an anchorless cursor that got past block 1 (`Restart::NoAnchor`) and one of another chain (`Restart::ChainReplaced`).

- An engine transaction whose first word is `Liquidate`'s enum index and whose account 2 is our config yields an auction (account 6) and a position (account 4).
- A host transaction listing our `ProtocolParameters` whose first word is `OpenPosition`'s enum index yields a position (account 1).
- Chain identity (ADR-043, amended): the cursor stores `anchor`, the hash of block 2 (block 1, the genesis block, is built with timestamp 0 and repeats across localnet restarts). Each scan reads the node's block 2 first; a different anchor discards the cursor and its discoveries and rescans from block 1 (`Restart::ChainReplaced`). A cursor without an anchor (written before anchors existed) that got past block 1 is rebuilt the same way once (`Restart::NoAnchor`); old cursor files still load.
- If the block at `cursor.last_block` no longer has `cursor.last_hash`, or the cursor is above the node's head, the scan rewinds to `min(last_block - reorg_depth, head)` (floor 0, default depth 10). The rewind is accepted only if it reaches genesis or the node's hash of the rewind block equals the one recorded in `cursor.recent` (the last `reorg_depth + 1` scanned hashes); otherwise the blocks below it may not be the ones scanned and the scan fails (cursor unchanged) with "run `rescan --from 1`". A successful scan has therefore seen every block of the node's chain up to its head. A cursor block within the head that the node cannot return fails the scan with an error that names `rescan`. Discoveries are sets, so a rescan never duplicates.
- Within a scan every block must extend the previous one (`prev_hash`) and the node must return every requested block; otherwise the scan fails and the cursor is not advanced. Work is bounded per request by `batch` (default 100 blocks).
- `save_cursor` writes a uniquely named temp file next to the cursor (fsynced) and renames it over the cursor; a failed write removes the temporary file. `load_cursor` returns the genesis cursor for a missing file and `SdkError::Io` for an unparsable one.

## Fuzzing

The SDK reads untrusted input: the node's account bytes, the wallet storage file, the scanner cursor file and the deployment JSON. `fuzz/` is a separate crate (its own workspace, excluded from the root one, nightly toolchain, `libfuzzer-sys`) with five `cargo-fuzz` targets. Each must never panic; `quote_bid` also checks invariant 3.

| Target | Input | Property |
|--------|-------|----------|
| `keystore_parse` | any bytes read as wallet storage text | `Keystore::parse` returns a keystore or a typed error (its message is formatted, key material is never printed). |
| `deployment_json` | any bytes read as a deployment file | `Deployment::from_json` returns a deployment or a typed error. |
| `cursor_json` | any bytes read as a scanner cursor file | Deserialising a `Cursor` and reading `auction_ids()` and `position_ids()` never panics. |
| `decode_accounts` | any bytes as the data of an account owned by the engine | `Reader::{config, coll_config, accounting, auction, surplus_auction, debt_auction, record_count}` each return a state or a typed error. |
| `quote_bid` | `coin_in_max` (0 to about 2 targets) and `min_out` (0 to about 2 lots) as 16-bit fractions of the standard live auction's target and lot, a clock offset, and an oracle price from 0.5 R to 1.5 R (the feed band is 0.9 R to 1.1 R); one input in 256 uses raw `u128` amounts and a price from 0 to about 6.5 R. The fixture is built once per process; each input resets only the clock and the oracle. | `quote_bid` returns an error or a fill with `charged <= coin_in_max`, `collateral_out >= min_out` and `collateral_out <= collateral_left` (invariant 3, spec 5.13). About one input in five reaches the assertions (a quote, not an error). |

The targets cannot carry `#![forbid(unsafe_code)]`: `fuzz_target!` expands to a `#[no_mangle] extern "C"` entry point.

Run them inside WSL, from the repository root. `scripts/wsl-env.sh` puts the build in the shared target directory (`$HOME/lez-target/plan2/fuzz`), which is deleted after use. One-time setup: `rustup toolchain install nightly --profile minimal` and `scripts/wsl-env.sh cargo install --locked cargo-fuzz`.

```bash
scripts/disk-guard.sh && scripts/wsl-env.sh scripts/fuzz-smoke.sh 60; scripts/disk-guard.sh   # every target, 60 s each
scripts/wsl-env.sh cargo +nightly fuzz run --target-dir "$HOME/lez-target/plan2/fuzz" quote_bid -- -max_total_time=60   # one target
rm -rf "$HOME/lez-target/plan2/fuzz"
```

Crash procedure (libFuzzer writes `fuzz/artifacts/<target>/crash-<hash>`):

1. Reproduce: `scripts/wsl-env.sh cargo +nightly fuzz run --target-dir "$HOME/lez-target/plan2/fuzz" <target> fuzz/artifacts/<target>/crash-<hash>`.
2. Minimise: `scripts/wsl-env.sh cargo +nightly fuzz tmin --target-dir "$HOME/lez-target/plan2/fuzz" <target> fuzz/artifacts/<target>/crash-<hash>`.
3. If the panic is in our crates, add a unit test with the minimised input to the owning crate (red), fix it (green), and run the target for 60 s again.
4. If the panic is inside LEZ, borsh or another dependency, stop and record the input as an upstream finding; the target then needs a ruling.

The `fuzz-nightly` job (`.github/workflows/fuzz.yml`) runs `scripts/fuzz-smoke.sh 300` every week (Sunday) with `RISC0_SKIP_BUILD=1` (the fuzzers never prove, so the risc0 toolchain and guest ELFs are not installed) and uploads `fuzz/artifacts` as `fuzz-crashes` when a target crashes. See [ci.md](ci.md).

## Localnet

The CLI-driven run on a real standalone sequencer (every program instruction sent by `auction-cli`) is recorded in the Plan 3a section of [localnet.md](localnet.md); the runbook is `scripts/localnet-smoke.sh`.

## Plan 3b changes

- Confirmation and scanner (Task 2): a rebase margin in `confirm.rs`, anchorless cursors reset on rescan in `scan.rs`, lock edge cases pinned in `confirm_tests.rs` (ADR-042, ADR-043 amendments).
- Dev commands and watch (Task 3): state-changing dev commands require the recorded chain identity (`verify.rs`, `deployment.rs`, ADR-046); `watch` exits with the worst step error (ADR-044 amendment).
- Reproducible guests (Task 5): `verify.rs` pins `deployments/localnet.json` to `artifacts/guests/manifest.json` (ADR-047); `DEPLOYMENTS.md` is kept equal by hand.
- Fuzz targets (Task 9): see Fuzzing above.
- Oracle publisher and keeper (Task 10): `oracle.rs`, `plan_host.rs`, `auction-cli dev oracle-publish` and `dev keeper`, `host status` (ADR-050).
- Demo scenario (Task 11): `demo.rs` and `cli/params/demo-*.json` (ADR-051).

## Plan 4 changes

- **Non-blocking confirmation (Task 2).** `track_begin`, `poll_once` and `track_end` in `confirm.rs` (ADR-053). `Tracking` serialises with the signer in base58 and the nonce as a decimal string. `submit` keeps `lock_with_current_baseline` + `confirm` (R6), so the CLI is unchanged. Tests: `poll_once_is_pending_until_the_nonce_moves`, `track_end_with_unknown_outcome_keeps_the_lock`.
- **`ops` (Task 7).** The CLI's transaction commands are now printers over `ops::prepare_*`. Client-side refusals are `SdkError::Refused`. `params.rs` moved from the CLI into the SDK. Every golden test is unchanged. Tests: `ops_tests.rs`, one or more per `prepare_*`.
- **`views` (Task 8).** `config`, `list-auctions`, `list-at-risk` and `host status` print from the views. The stale-candidate rule moved from the CLI into `views` (`Listed { items, skipped }`). `quote::position_health` gained `cr_bps`: the host's min-CR inequality at the redemption price R, solved for the ratio (R17). `quote::discount_schedule` samples the schedule for the chart. The scan cursor counts included bids per auction (`Cursor.bids`, `bid_txs`). Tests: `views_tests.rs`.

## FFI (`crates/auction_ffi`)

The FFI is a thin JSON layer over the SDK for the Basecamp core module ([basecamp-app.md](basecamp-app.md)). It has no logic of its own.
- **`view`.** The views and quotes call `views::*` and `quote::*`.
- **`prepare`.** It maps each action to `ops::prepare_*` (ADR-055).
- **`tx_begin`, `tx_poll`, `tx_end`.** These are `track_begin`, `poll_once` and `track_end` (ADR-053).
- **`ctx`.** It loads the deployment file and verifies the config (I4), as the CLI does. It reads through `RpcChain::connect_with_timeout(url, 5 s)` under a 12 s per-call deadline (ADR-052).
- **Refusals.** Dev-only actions are refused off `localnet` and `testnet-legacy` (`ctx.require_dev`, kind `mismatch`). A lock path outside `<state_dir>/locks` is refused (R7).
- **Safety.** `unsafe` appears only in the extern shims of `lib.rs`. Every call runs in `catch_unwind`.

Tests:
- one file per area: `envelope.rs`, `view.rs`, `prepare_all.rs`, `prepare_transfer.rs`, `track.rs`;
- `parity.rs` checks the views against the CLI goldens (`quote_bid.txt`, `config.txt`).

`scripts/ci/ffi-header-drift.sh` fails when the committed `include/auction_ffi.h` differs from a fresh cbindgen run. `scripts/basecamp/gen-qml-fixtures.sh` regenerates the app's QML fixtures from the FFI (`tests/fixtures.rs`, `#[ignore]`d) on the standard fixture world.
