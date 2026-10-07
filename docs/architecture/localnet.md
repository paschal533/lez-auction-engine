# Localnet smoke (Plan 2, Task 20)

Proof that the stablecoin (with the engine hooks) and the auction engine deploy to and run on a real standalone
LEZ v0.2.4 sequencer (`RISC0_DEV_MODE=1`, dev-mode proofs) and that the core flow works end to end:
`liquidate` -> `bid_with_settlement` -> `claim_leftover`. Clients are `spel-cli` (Plan 3 replaces it with SDK
encoders). Runbook: `scripts/localnet-smoke.sh`; helper: `programs/integration_tests/examples/localnet_tool.rs`.

**Result: complete.** Every step confirmed, no transaction of the final flow failed. Not covered here (the in-process
suite covers them): the surplus and debt system auctions, `settle_debt`, `check_solvency`, `observe_feed`,
`bid` (non-settling), the private path.

## What is deployed

Only the engine and the stablecoin are exported by `localnet_tool export-guests` (plus the upstream token,
twap_oracle and token_mint_authority guests from S1's Docker build). The test-only attacker guests and the second
stablecoin instance are never exported or deployed. ImageIDs are in `DEPLOYMENTS.md`; the stablecoin and engine ids
are from a NATIVE build and are not reproducible across machines until Plan 3's Docker builds.

| Program | ImageID |
|---|---|
| stablecoin | `91c790a23cadc2aa72be1533a4f4daa8ab6734e0dad5165fb9303cb438736953` (limbs `2727397265,2864885052,857063026,2832921764,3761530795,1595332058,3023843513,1399419704`) |
| auction_engine | `3d0d15fa3c375179e1a4e9e02e828affb1924cd8dc559653cc5fe266ec23f477` (limbs `4195683645,2035365692,3773408481,4287267374,3628896945,1402361308,1726111692,2012488684`) |

`spel program-id` on the exported `.bin` files prints the same hex.

## Commands run (inside WSL, from the repo root)

```bash
scripts/wsl-cargo.sh run -q -p engine_integration_tests --example localnet_tool -- export-guests $HOME/lez-target/plan2-localnet/guest
scripts/localnet-smoke.sh step_sequencer          # port 3040 must be free; state under $HOME/lez-target/plan2-localnet/
scripts/localnet-smoke.sh step_accounts           # 11 public accounts (labels below), wallet in plan2-localnet/wallet-home
# write plan2-localnet/localnet.env (names below), then:
scripts/localnet-smoke.sh step_auth               # auth-transfer init for ADMIN, ENGINE_ADMIN, KEEPER, OWNER (ADR-018)
scripts/localnet-smoke.sh step_deploy             # 5 ProgramDeploymentTransactions + the two spel IDL copies
# oracle PDA, upstream tool (in lez-programs): twap_oracle_pdas <TWAP id> $ORSRC 3600000   -> ORACLE
# engine and host PDAs:
scripts/wsl-cargo.sh run -q -p engine_integration_tests --example localnet_tool -- pdas $ENGINE $STABLE $ENGINE_ADMIN 1 $OWNER 0 $COLLDEF >> localnet.env
#   (again with nonce 1 into a scratch file; append only POSITION/HOST_VAULT as POSITION1/HOST_VAULT1)
scripts/localnet-smoke.sh step_host              # COLL token + oracle + stablecoin initialize_program
scripts/localnet-smoke.sh step_engine            # init_config (admin ENGINE_ADMIN, salt 1)
scripts/localnet-smoke.sh step_verify_and_bind   # decodes the stored config and aborts unless admin/host/token ids match
scripts/localnet-smoke.sh step_bind              # host set_liquidation_engine
scripts/localnet-smoke.sh step_collateral        # engine set_collateral_config
scripts/localnet-smoke.sh step_holdings          # initialise OWNER_STABLE and KEEPER_COLL holdings
scripts/localnet-smoke.sh step_flow              # flow_open, flow_raise_ratio, flow_liquidate, flow_bid, flow_claim
scripts/localnet-smoke.sh step_check
scripts/localnet-smoke.sh step_stop
```

`localnet.env` variable names (values are base58 ids / hex, no keys): `ADMIN FREEZE ORSRC COLLDEF ENGINE_ADMIN KEEPER
KEEPER_COLL OWNER OWNER_COLL OWNER_STABLE STABLE ENGINE ORACLE` (hand-written), then from `localnet_tool pdas`:
`PP SFA RPS SDEF SMASTER LES POSITION HOST_VAULT CONFIG COLL_CONFIG ACCOUNTING BUFFER ENGINE_AUTH UNUSED0 UNUSED1
LIQREC AUCTION AUCTION_VAULT ENGINE_LIMBS`, plus `POSITION1 HOST_VAULT1`, and for the oracle refresh `ORSRC2 ORACLE2`
(then `ORACLE=$ORACLE2`).

## Outcome per step

| Step | Transaction(s) | Outcome |
|---|---|---|
| sequencer | `getLastBlockId` answers; a block every ~15 s | ok |
| auth init | 4 x `auth-transfer init` (blocks 6, 8, 10, 12) | confirmed |
| deploy | token, twap_oracle, token_mint_authority, stablecoin, auction_engine (blocks 14-22) | confirmed |
| host setup | `new_fungible_definition` COLL `f66412b8...`; `create_oracle_price_account` `ab9eedce...`; `initialize_program` `8c8bed65...` | confirmed |
| `init_config` | `6d8b2361...` | confirmed, PDAs derived by spel equal `localnet_tool pdas` output |
| bind-time verification (ADR-027, I7) | read `CONFIG` with `wallet account get`, `decode-config` | admin = ENGINE_ADMIN, salt = 1, host_program_id = stablecoin ImageID, token_program_id = token ImageID, host_protocol_parameters_id = PP, stablecoin_definition_id = SDEF: matched; the script writes a `config-verified` marker and `step_bind` refuses to run without a marker for the same config id |
| `set_liquidation_engine` | `a804528f...` (host IDL index 19) | confirmed; `liquidation_engine_state` data holds the engine ImageID |
| `set_collateral_config` | `a29c7735...` | confirmed |
| holdings | 2 x token `initialize_account` | confirmed |
| positions | nonce 0: `open_position` `e7b68ed2...` (500_000 coll), `generate_debt` `65696b91...` (300_000); nonce 1: `open_position` `213c4efa...` (1_000_000), `generate_debt` `2b7ca33b...` (100_000) | confirmed |
| raise min ratio to 2.0 | first try `7953b4f3...` FAILED (see finding 1); retry `c3ec2228...` | confirmed |
| oracle refresh (ran after the failed `liquidate`, before its retry) | `create_oracle_price_account` `7d3fe69e...`, host `set_market_price_oracle` `aff4ef2f...`, engine `set_collateral_config` `a5c6780e...` | confirmed |
| `liquidate` | first try `3b541776...` FAILED `PriceFeedStale` (finding 2); after the oracle refresh `c277bd02...` | confirmed |
| `bid_with_settlement` | `48a32f98...`, coin_in_max 330_000 | confirmed |
| `claim_leftover` | `bff2fa35...` | confirmed |

Sequencer log: exactly the two `failed execution check` lines above, both before their successful retries.

### `step_check` (final balances)

| Account | Value |
|---|---|
| position 0 | `collateral_amount 0`, `normalized_debt_amount 0` |
| auction vault | 0 (collateral swept and claimed) |
| buffer (stablecoin) | 30_000 (the 10 % penalty on 300_000) |
| OWNER_COLL | 99_998_999_950 = 100_000_000_000 minted - 1_500_000 deposited + 499_950 back (bid proceeds plus claimed leftover: 500_000 - 50 keeper reward) |
| OWNER_STABLE | 70_000 = 400_000 minted - 330_000 paid in the bid |
| KEEPER_COLL | 50 (keeper reward `min(5 + 300, 50, 30_000)`) |

Matches the expected end state of the task brief.

## Findings (for Plan 3 and the upstream PR)

1. **`idl/stablecoin-idl.json` instruction order differs from `stablecoin_core::Instruction` for indices 8-18**
   (known upstream fact, spec §3.2; the full mapping is pinned by `stablecoin_idl_position_maps_to_enum_index` in
   `programs/integration_tests/tests/idl.rs`).
   spel encodes the IDL position as the instruction index; the guest decodes the enum. Indices 0-7 and the three
   appended hooks (19-21) agree (that is all `hook_instructions_share_enum_and_idl_index` checks), but e.g.
   `set_minimum_collateralization_ratio` is IDL 11 / enum 8, and IDL index 11 executed `set_timing_parameters`
   ("new_minimum_milliseconds_between_rate_updates above maximum", tx `7953b4f3...`). The runbook builds a host IDL
   copy sorted into enum order (`make_host_idl`). Plan 3's SDK encoders must use enum order, and its drift check
   should compare the whole IDL against the enum, not just the hooks. The vendored IDL is unchanged by this task.
2. **A test oracle with no publisher goes stale after 1 h** (host and engine both enforce 3_600_000 ms): the first
   `liquidate` failed `PriceFeedStale` because the slow parts of the run exceeded an hour. `step_refresh_oracle`
   creates a fresh oracle and rebinds host and engine. A real deployment needs a price publisher (Plan 3 keeper).
3. **spel (rev 1ef0500) cannot resolve a PDA seeded by another PDA of the same instruction.** `init_config`
   (`accounting` and `buffer` are seeded by `config`, itself a PDA) failed with "PDA 'accounting' requires account
   'config' - provide --config" while rejecting `--config` as "unknown argument", and `liquidate` has the same
   problem with `auction`. The runbook uses an engine IDL copy without the `pda` marking on `init_config.config` and
   `liquidate.auction`; wire format and account order are unchanged and the passed addresses come from
   `localnet_tool pdas`. This is not an encoding failure: `Vec<u32>` (`--debt-growth "0,0,1000,3000"`),
   `program_id` (`--engine-program-id` as the 8 limbs, comma-separated) and `u128` encode and execute correctly, so the
   decided fallback was not needed and no instruction was left for Plan 3.
4. `spel inspect` needs an IDL `types` entry; the engine IDL has none, so bind-time verification reads raw data with
   `wallet account get` and decodes it with `localnet_tool decode-config`. `spel inspect` works for `Position` (host
   IDL) and `TokenHolding` (token IDL).
5. `spel` prints "Transaction confirmed" when the transaction is included in a block; a transaction that fails the
   sequencer's execution check is skipped and spel keeps waiting. Always read the sequencer log
   (`failed execution check`) after each step.
6. i128 gains use the u128 IDL copy (valid for gains >= 0; both gains are 0).

## Cleanup

State lives under `$HOME/lez-target/plan2-localnet/` (sequencer home, wallet, env file, logs, exported guests);
`step_stop` stops only the sequencer that `step_sequencer` started (pid recorded from the listener on port 3040).
