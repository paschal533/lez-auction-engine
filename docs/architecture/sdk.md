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
