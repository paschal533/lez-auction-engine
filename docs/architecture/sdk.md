# Client SDK (`crates/auction_sdk`)

The SDK is the only way the CLI (and, in Plan 4, the FFI) talks to the engine. It is synchronous and generic over one trait.

## Chain

`chain::Chain` = `account`, `nonces`, `last_block_id`, `blocks(from..=to)`, `send`. Implementations: `testing::FixtureChain` (in memory, feature `testing`); planned: `WorldChain` in `programs/integration_tests/tests/common/sdk.rs` (the real guests in an in-process `V03State`, Task 8) and `rpc::RpcChain` (sequencer JSON-RPC, feature `rpc`, Task 12). An uninitialised account is `Account::default()`, never an error. `send` success means accepted, not executed (spec §3.2).

## Encoding

`encode::words(&ix)` is `lee::program::Program::serialize_instruction` (risc0 serde) of the program's own instruction enum, so the first word is the enum index the guest decodes. Never the IDL position: `idl/stablecoin-idl.json` differs from `stablecoin_core::Instruction` for positions 8..=18 (`STABLECOIN_IDL_POSITION_TO_ENUM`). Signed `i128` controller gains encode (spel cannot, ADR-018).

Drift tests: `encode::tests` (SDK) and `programs/integration_tests/tests/idl.rs`. Both use one shared source for the stablecoin sample set, the IDL-name extraction (`testing::support`, `#[doc(hidden)]`, feature `testing`) and the position-to-enum table (`encode::STABLECOIN_IDL_POSITION_TO_ENUM`); nothing is duplicated.

## Plans

`plan::Plan { program_id, account_ids, signing_requirements, instruction_words }`; `Plan::new` rejects duplicate accounts, unlisted signers and signer-less plans.

The `*_is_fresh` flags of the builders (`keeper_holding_is_fresh`, `bidder_collateral_is_fresh`, `receiver_is_fresh`, `owner_holding_is_fresh`) are caller-trusted: the builder cannot know whether a holding exists, so a wrong flag produces a plan the chain rejects (`RecipientMustSign` for a missing signature, or a signature on an initialised account), never a silently wrong transfer.

## Module map

| Module | Task | Purpose |
|---|---|---|
| `chain`, `encode`, `error`, `plan`, `testing` | 4 | trait, encoders, errors, plan type, fixtures |
| `addresses`, `decode` | 5 | PDAs from the engine's and host's own functions; decoders that check initialisation, owner and tag |
| `deployment`, `verify` | 5 | `deployments/<network>.json` (must match `DEPLOYMENTS.md`); `verify_config` refuses a config not owned by the engine ImageID, not at `["CONFIG", admin, salt]`, or storing another host/token program, admin, `ProtocolParameters` or stablecoin definition, or whose host `ProtocolParameters` has another owner (I4, ADR-027); `verify_binding` checks the host binds this config |
| `testing::standard` | 5 | the "standard world" fixture (`standard()`, `deployment()`, `config_id()` and the shared constants) used by SDK, CLI and golden tests; built from direct account inserts, no program runs |
| `quote` | 6 | liquidation, bid, surplus-take and debt-take quotes on the handler's own path; auction view (held at max discount, paused reason); slippage helpers; next debt start (ADR-039) |
| `plan` (builders) | 7 | one builder per engine instruction (Shared Reference S1 lists, checked against `idl/auction_engine-idl.json`); fresh recipient holdings added as signers; slots from the config (ADR-030); `bid` vs `bid_with_settlement` chosen by the caller from `BidQuote::needs_settlement` (ADR-029) |
| `plan_host` | 7 | stablecoin (incl. signed `i128` controller gains), token and twap_oracle builders used by the CLI admin/host/dev commands; encoded by enum index, never the IDL position |
| `keystore` | 9 | the wallet's `storage.json` read directly; labels or base58 ids; keys verified against their ids; private accounts refused |
| `tx` | 9 | `KeySource`, `sign(plan, nonces, keys)` |
| `confirm` | 10 | nonce-polling confirmation (Included / Dropped after 5 blocks), one in-flight transaction per signer via a lock file, `submit` = lock + sign + send + confirm |
| `scan` | 11 | block scan with a persisted, atomically written cursor; rewinds when the cursor's block hash changed; fails (cursor unchanged) on a missing or non-extending block |

## Program-id verification (I4)

Every transaction for an engine config starts with `verify_config(chain, &deployment, &config)`; `admin init-config` and the host/dev commands, which have no config to verify yet, do not. It reads the config account through `Chain` and compares it with the deployment file, so the check does not rest on caller-supplied ids alone. A mismatch is `SdkError::Mismatch { what, expected, found }` (CLI exit 2).

Trust boundary: the RPC endpoint is trusted. There are no state proofs, so a malicious sequencer URL can serve a consistent set of fake accounts that passes every check; the deployment file and the endpoint come from the operator.

`engine_admin: null` in a deployment disables the admin check (configs are permissionless, ADR-027), so operators should set it. The CLI (Tasks 13 and 15) prints the config's admin before signing.

`verify_config` deliberately does not check the collateral definition, the `CollateralConfig` oracle id, or the protocol token and its oracle. Those are checked at use time: `Reader::oracle` (owner = twap oracle program when the deployment names it), `Reader::token_definition` (owner = token program), `Reader::balance`, and on-chain by the engine's own account pinning.

The ImageIDs in `deployments/localnet.json` are native-build values until Plan 3b's reproducible Docker builds (THREAT_MODEL I4 residual); they equal `DEPLOYMENTS.md` today and Task 16 refreshes both after the engine guest changes of Tasks 2 and 3.

## Quotes

A quote is the handler's pricing path run on fetched accounts at a chosen `now` (default: the chain clock, `chain::clock_ms`). It calls `auction_engine_program::feed::{redemption_price, observe, require_live}` and the `auction_core` functions in the handler's order, so `t_eff` always comes from the clock *after* simulating `observed()` (ADR-024): an unobserved stale gap freezes the discount in the quote exactly as it will on-chain. A quote is valid only at its `now`; the on-chain result at inclusion time can differ (discount moved, oracle moved). Pass the `CLOCK_01` value the transaction will execute under (`chain::clock_ms`, the previous block's timestamp), not wall-clock time; `clock_ms` is that value only if the transaction lands in the very next block, otherwise the clock has moved, which the slippage bound covers. Quotes predict pricing and engine/host state checks, including the host hook's "position is collateralized" refusal (`HostReject`), but not the caller's balances or recipient shape: takes can still revert with `InsufficientBalance`, `VaultBelowBook`, `RecipientMustSign`; bids with `VaultBelowBook`, `InsufficientBalance`, `RecipientMustSign`, and `ChainEndAccountsMissing` (avoided by `needs_settlement`). This is why every transaction command derives `min_collateral_out` / `max_protocol_in` / `min_protocol_out` from the quote with a slippage margin (Review Focus 1, `sdk_quotes.rs`).

## Proof on the real guests

`programs/integration_tests/tests/sdk_plans.rs` runs every engine, host and token builder through `World::run_plan` against the real token, stablecoin and engine guests (liquidate, partial and final bid, claim, observe, settle_debt, check_solvency, surplus and debt auctions, init/bind/collateral/params, signed controller gains). `sdk_quotes.rs` pins quote == on-chain result: the liquidation quote equals the opened auction, a bid quote taken at the inclusion clock equals the fill, and a quote taken earlier protects the bidder through `min_collateral_out` when the oracle moves (Review Focus 1). The harness uses the SDK's `encode::{init_config_ix, set_engine_params_ix, set_collateral_config_ix}` instead of its own copies, and `WorldChain` reads the in-process `V03State`; the clock a quote must use is the one the transaction executes under (in the World `World::now`, seeded into `CLOCK_01` by `submit`/`run_plan`, advanced by `World::advance`).

## Keystore

The SDK reads the LEZ wallet's `storage.json` itself (ADR-041); the `wallet` crate is not a dependency. The path is `--wallet-home` or `$LEE_WALLET_HOME_DIR`, then `/storage.json`. Format (LEZ v0.2.4 `lez/wallet/src/storage/persistent.rs`): `key_chain.accounts[]` entries `{"Public": {account_id, chain_index, data: {sk, ssk, pk, cc, cci}}}`, `{"ImportedPublic": {account_id, pub_sign_key}}`, `{"Private": {account_id, ...}}` and `{"ImportedPrivate": {...}}`, plus `labels: {label: {"Public"|"Private": account_id}}`. Public accounts sign with `ssk` (the wallet signs with `&ChildKeysPublic.ssk`; the account id derives from `PublicKey::new_from_private_key(&ssk)`), imported public accounts with `pub_sign_key`; every key is checked to derive its stored account id (`KeyMismatch` otherwise). Signatures are LEZ's own `WitnessSet::for_message` over `Message::hash`, verified by `WitnessSet::is_valid_for`.

Private accounts are recorded only to be refused (`PrivateAccount`). Upstream `ImportedPrivate` entries carry no `account_id`, so the parser skips them: an unlabelled imported private account fails as `NoKey`, a labelled one as `PrivateAccount`. A base58 account id resolves without a key (recipients); signing needs one.

The v0.2.4 wallet keeps keys unencrypted on disk, so protect the wallet home like a key file. Errors are typed (`KeystoreError`: `HomeNotSet`, `NotFound`, `Unreadable`, `Malformed`, `UnknownLabel`, `NoKey`, `PrivateAccount`, `KeyMismatch`; the CLI exits 4) and "Debug never prints key material": `Keystore`'s `Debug` lists the path, public account ids and labels only, and no error, `Display`, `Debug` or log contains a private key, seed, mnemonic or password (pinned by `keystore_formats_never_contain_key_bytes_in_any_error_or_debug`). `lee::PrivateKey` has no `Zeroize` upstream, so keys cannot be wiped from memory by this crate; nothing is written to disk.

## Confirmation

`sendTransaction` success means accepted, not executed (spec §3.2). `submit` records the primary signer's nonce `n0` and the height `h0`, takes the signer's lock, signs, sends, then polls the nonce through `Chain` (the sleeper is injected, so tests do no real sleeping): a nonce above `n0` is `Included { nonce_after }`; still `n0` when the height reaches `h0 + 5` is `Dropped { height }`; no verdict after `max_polls` (default 900 at 2 s) is an error that says to check the nonce before sending again. Each poll reads the height first and the nonce second (the sequencer moves the nonce before the tip), and an empty nonce reply is an error. `Dropped` means not applied by `h0 + 5`; a backlogged transaction could still land later, but a re-quote reuses nonce `n0`, so at most one of the two applies. `Included` means some transaction of this signer at nonce `n0` applied, not necessarily this hash (a double-proceed race on the lock, two CLIs with different state directories, or a transaction sent outside the lock such as by the wallet CLI), so re-read the accounts you touched; `poll_transaction_status` after the v0.3 port is the hash-level fix. A `Dropped` transaction is never resent by the SDK: re-quote first (the CLI prints "nothing was applied; re-quote and resend" and exits 3). The spec's "target account changed" check is not implemented (ADR-042): on v0.2.4 the signer nonce advances only in `State::apply_state_diff`, after the transaction validated and executed, so a higher nonce already proves application.

Lock: `<state dir>/locks/<signer>.lock` holds `{nonce, height, owner}`. It is created atomically across processes on Linux and Windows (written to a temp file, hard-linked into place; `create_new` where hard links are unavailable) and released on drop by the process that still owns it, on a definite verdict only: Included, Dropped, a definite send rejection, or a failure before the send. A timeout, an unknown send result (`SdkError::SendUncertain`, to be returned by chain implementations for transport failures after the request may have been delivered) or a panic keeps the lock, which then clears by the staleness rule within 5 blocks. A lock is stale when the signer's nonce moved past its nonce or the height reached its `height + 5`, or it does not parse; a live, in-window lock is never replaced and the second submission fails with `InFlight` before anything is signed or sent. Staleness uses chain facts, not process ids or file ages, because pids do not transfer between WSL, Windows, containers and machines and are reused.

## Discovery

`scan::scan(chain, targets, cursor, settings)` returns a new cursor and never mutates its input, so a failed scan leaves the caller's cursor as it was.

- An engine transaction whose first word is `Liquidate`'s enum index and whose account 2 is our config yields an auction (account 6) and a position (account 4).
- A host transaction listing our `ProtocolParameters` whose first word is `OpenPosition`'s enum index yields a position (account 1).
- If the block at `cursor.last_block` no longer has `cursor.last_hash`, the scan rewinds to `last_block - reorg_depth` (floor 0, default 10) and rescans. A cursor above the node's head rewinds to `min(last_block - reorg_depth, head)`. A cursor block within the head that the node cannot return fails the scan (cursor unchanged) with an error that names `rescan`. Discoveries are sets, so a rescan never duplicates.
- Within a scan every block must extend the previous one (`prev_hash`) and the node must return every requested block; otherwise the scan fails and the cursor is not advanced. Work is bounded per request by `batch` (default 100 blocks).
- `save_cursor` writes a uniquely named temp file next to the cursor (fsynced) and renames it over the cursor; a failed write removes the temporary file. `load_cursor` returns the genesis cursor for a missing file and `SdkError::Io` for an unparsable one.
