# Client SDK (`crates/auction_sdk`)

The SDK is the only way the CLI (and, in Plan 4, the FFI) talks to the engine. It is synchronous and generic over one trait.

## Chain

`chain::Chain` = `account`, `nonces`, `last_block_id`, `blocks(from..=to)`, `send`. Implementations: `testing::FixtureChain` (in memory, feature `testing`); planned: `WorldChain` in `programs/integration_tests/tests/common/sdk.rs` (the real guests in an in-process `V03State`, Task 8) and `rpc::RpcChain` (sequencer JSON-RPC, feature `rpc`, Task 12). An uninitialised account is `Account::default()`, never an error. `send` success means accepted, not executed (spec §3.2).

## Encoding

`encode::words(&ix)` is `lee::program::Program::serialize_instruction` (risc0 serde) of the program's own instruction enum, so the first word is the enum index the guest decodes. Never the IDL position: `idl/stablecoin-idl.json` differs from `stablecoin_core::Instruction` for positions 8..=18 (`STABLECOIN_IDL_POSITION_TO_ENUM`). Signed `i128` controller gains encode (spel cannot, ADR-018).

Drift tests: `encode::tests` (SDK) and `programs/integration_tests/tests/idl.rs`. Both use one shared source for the stablecoin sample set, the IDL-name extraction (`testing::support`, `#[doc(hidden)]`, feature `testing`) and the position-to-enum table (`encode::STABLECOIN_IDL_POSITION_TO_ENUM`); nothing is duplicated.

## Plans

`plan::Plan { program_id, account_ids, signing_requirements, instruction_words }`; `Plan::new` rejects duplicate accounts, unlisted signers and signer-less plans.

## Module map

| Module | Task | Purpose |
|---|---|---|
| `chain`, `encode`, `error`, `plan`, `testing` | 4 | trait, encoders, errors, plan type, fixtures |
| `addresses`, `decode` | 5 | PDAs from the engine's and host's own functions; decoders that check initialisation, owner and tag |
| `deployment`, `verify` | 5 | `deployments/<network>.json` (must match `DEPLOYMENTS.md`); `verify_config` refuses a config not owned by the engine ImageID, not at `["CONFIG", admin, salt]`, or storing another host/token program, admin, `ProtocolParameters` or stablecoin definition, or whose host `ProtocolParameters` has another owner (I4, ADR-027); `verify_binding` checks the host binds this config |
| `testing::standard` | 5 | the "standard world" fixture (`standard()`, `deployment()`, `config_id()` and the shared constants) used by SDK, CLI and golden tests; built from direct account inserts, no program runs |

## Program-id verification (I4)

Every transaction for an engine config starts with `verify_config(chain, &deployment, &config)`; `admin init-config` and the host/dev commands, which have no config to verify yet, do not. It reads the config account through `Chain` and compares it with the deployment file, so the check does not rest on caller-supplied ids alone. A mismatch is `SdkError::Mismatch { what, expected, found }` (CLI exit 2).

Trust boundary: the RPC endpoint is trusted. There are no state proofs, so a malicious sequencer URL can serve a consistent set of fake accounts that passes every check; the deployment file and the endpoint come from the operator.

`engine_admin: null` in a deployment disables the admin check (configs are permissionless, ADR-027), so operators should set it. The CLI (Tasks 13 and 15) prints the config's admin before signing.

`verify_config` deliberately does not check the collateral definition, the `CollateralConfig` oracle id, or the protocol token and its oracle. Those are checked at use time: `Reader::oracle` (owner = twap oracle program when the deployment names it), `Reader::token_definition` (owner = token program), `Reader::balance`, and on-chain by the engine's own account pinning.

The ImageIDs in `deployments/localnet.json` are native-build values until Plan 3b's reproducible Docker builds (THREAT_MODEL I4 residual); they equal `DEPLOYMENTS.md` today and Task 16 refreshes both after the engine guest changes of Tasks 2 and 3.
