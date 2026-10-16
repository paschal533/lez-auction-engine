# Localnet smoke

## Plan 3a run (auction-cli, no spel)

Run of 2026-10-12 on a fresh standalone LEZ v0.2.4 sequencer (`RISC0_DEV_MODE=1`, dev-mode proofs), every program
instruction sent by `auction-cli` (`scripts/localnet-smoke.sh`, Plan 3a Task 16). The wallet binary only creates keys,
runs `auth-transfer init` and deploys programs. Scope decision: the run goes through liquidation, partial and final
bids, claim, `observe_feed`, `settle_debt`, `check_solvency` and the surplus auction. It stops before the debt
auction (F4) and the freeze path: both need a shortfall, which needs a moving oracle price, and the localnet oracle
has no publisher until Plan 3b.

**Result: complete.** Every step was `included`, `no failed execution check` in the sequencer log, and the final
balances equal the expected end state. A first run of the same script lost part of its console log (the script's
`tee /dev/stderr` truncated the log file it was redirected to; fixed in the script), so the whole run was repeated
on a fresh sequencer; the figures below are from that second run. The first run reached the same end state.

### Commands run (inside WSL, from the repo root)

```bash
scripts/localnet-smoke.sh step_build        # auction-cli; localnet_tool export-guests (engine + stablecoin only); deployment.json
scripts/localnet-smoke.sh step_sequencer    # port 3040 must be free; state under $HOME/lez-target/plan3a-localnet/
scripts/localnet-smoke.sh step_accounts     # 14 public accounts (labels in the script), wallet in plan3a-localnet/wallet-home
scripts/localnet-smoke.sh step_auth         # 5 x auth-transfer init (ADR-018)
scripts/localnet-smoke.sh step_deploy       # 5 ProgramDeploymentTransactions
scripts/localnet-smoke.sh step_host         # dev token-new COLL, dev oracle-create, host init-program
scripts/localnet-smoke.sh step_engine       # dev token-new MOCK (mint authority = the config's MINT_AUTH PDA), admin init-config
scripts/localnet-smoke.sh step_bind         # host bind: verifies the config first, then set_liquidation_engine
scripts/localnet-smoke.sh step_collateral   # admin set-collateral-config
scripts/localnet-smoke.sh step_holdings     # dev token-init-account x3
scripts/localnet-smoke.sh step_flow         # open-position x2, generate-debt x2, set-min-cr, liquidate, bid, bid, claim-leftover
scripts/localnet-smoke.sh step_system       # observe-feed, settle-debt, check-solvency, start-surplus, take-surplus
scripts/localnet-smoke.sh step_check        # admin verify and final balances
scripts/localnet-smoke.sh step_stop
```

### ImageIDs of this run (native build, see `DEPLOYMENTS.md`)

| Program | ImageID |
|---|---|
| stablecoin | `91c790a23cadc2aa72be1533a4f4daa8ab6734e0dad5165fb9303cb438736953` (unchanged since Plan 2: Plan 3a did not touch the guest) |
| auction_engine | `de82967436371542ea2aa862b967af1776ef3fa3f4c5ae5033ea30f27253fc68` (limbs `1956020958,1108686646,1655188202,397371321,2738876278,1353631220,4063291955,1761366898`; changed from Plan 2's `3d0d15fa...` by Tasks 1-3) |

### Step, transactions, outcome

| Step | Transaction(s) | Outcome |
|---|---|---|
| `step_build` | `STABLECOIN_ID` and `AUCTION_ENGINE_ID` as above | ok |
| `step_sequencer` | port 3040 listening, pid recorded | ok |
| `step_accounts` | 14 public accounts | created |
| `step_auth` | 5 x `auth-transfer init`: `661107ea...`, `1dd27977...`, `2d724b5e...`, `9b73c482...`, `7b2ebe8a...` (blocks 3-11) | included |
| `step_deploy` | token `43fab3ef...`, twap_oracle `32e65707...`, token_mint_authority `fb9a2082...`, stablecoin `26e36c34...`, auction_engine `21e1bd6e...` (blocks 13-20) | included |
| `step_host` | COLL `new_fungible_definition` `45f95d96...`; `create_oracle_price_account` `e98ddd7e...`; `initialize_program` `c5b9e0ba...` | included |
| `step_engine` | MOCK `new_fungible_definition` `3a1c4ae7...`; `init_config` `148c286b...` (`ref price (RAY)` printed `2000000000000000000000000000`) | included |
| `step_bind` | `host bind`: `verified config owner, address, host and token program ids, admin, host ProtocolParameters`, then `set_liquidation_engine` `bf99fccc...` | included; `bound by the host: yes` |
| `step_collateral` | `set_collateral_config` `63fe3c42...` | included (min bid 25, type cap 5000000) |
| `step_holdings` | 3 x `initialize_account`: `b6d6a508...`, `e7f21c67...`, `d91ac6b1...` | included |
| `step_flow` positions | nonce 0: `open_position` `a96c37a3...` (500_000), `generate_debt` `2721c393...` (300_000); nonce 1: `open_position` `f6de7b2a...` (1_000_000), `generate_debt` `1ce42eb5...` (100_000) | included |
| `step_flow` min CR | `set_minimum_collateralization_ratio` `c9dda4cb...` (1.5 to 2.0); `list-at-risk` shows position 0 only | included |
| `quote-liquidation` | amount to raise 330000, keeper reward 50, lot 499950 | as expected |
| `liquidate` | `cdb6611b...` | included; auction live, lot 499950, keeper balance 50 |
| `bid` (partial) | `724852bb...`, quoted out 102093, `bid (11 accounts, 1 signer(s))` | included; remaining target 230000, collateral left 397857 |
| `bid_with_settlement` (final) | `733146e2...`, quoted out 234930, `bid_with_settlement (12 accounts, 1 signer(s))` | included; auction settled, collateral left 162927 |
| `claim_leftover` | `2ae987f3...` | included; `list-auctions --all`: settled, sold 337023, raised 330000/330000, leftover 162927 (claimed 162927) |
| `observe_feed` | `c5b8164d...` | included |
| `settle_debt` | `0ddf0dde...` (nothing to burn) | included; unbacked debt 0 |
| `check_solvency` | `a34bf56a...` | included; healthy |
| `start_surplus_auction` | `eb201ebf...` | included; surplus live, next nonce 1 |
| `take_surplus` | `2e91c0cc...`, quoted tokens 200, max protocol in 201 | included; surplus no longer live |
| `step_check` | `admin verify` shows `bound by the host yes`; balances below | `no failed execution check` |

### Final balances (`step_check`)

| Account | Balance |
|---|---|
| owner-coll (COLL) | 99_998_999_950 (100_000_000_000 minted - 1_500_000 deposited + 499_950 back: bid proceeds plus claimed leftover) |
| owner-stable | 70_000 (400_000 minted - 330_000 paid in the bids) |
| keeper-coll (COLL) | 50 (keeper reward) |
| taker-stable | 500 |
| taker-protocol (MOCK) | 9_800 (10_000 minted - 200 paid for the surplus lot) |
| buffer (stablecoin) | 29_500 (30_000 penalty - 500 sold in the surplus auction) |

All program instructions are encoded by `auction_sdk` (enum index, ADR-040): the Plan 2 workarounds are gone (sorted
host IDL copy, finding 1; IDL copy without `pda` markings, finding 3; u128 copy of the i128 gains, finding 6).
Bind-time verification is `auction-cli host bind` itself (ADR-027), replacing Plan 2's `step_verify_and_bind` marker
and `decode-config` check. Not covered on localnet: F4 and the freeze path (they need a shortfall, hence a moving
price: Plan 3b oracle publisher), `settle_empty`, `close_*` (TTL 4 h), the private path.

Cleanup: state lives under `$HOME/lez-target/plan3a-localnet/` (sequencer home, wallet, CLI state, logs, exported
guests, `deployment.json` of the run); `step_stop` stops only the sequencer that `step_sequencer` started. The
committed `deployments/localnet.json` keeps `engine_admin` and `config` `null` (they are keys of one machine's wallet).

### Re-run after the final-review fixes (2026-10-13)

The full run above was repeated on a fresh sequencer at the merge candidate: 34 transactions `included`, no failed
execution check, the same final balances. `live_rpc_reads_height_clock_and_blocks` passed against it.

Restart check (ADR-043 as amended): a chain was set up and scanned to block 39, then the sequencer was restarted
(`step_stop step_sequencer` back to back) and set up again with the same wallet, so the config landed at the same
address. With the old chain's cursor restored, `list-auctions` printed `note: the node serves another chain than the
cursor was built on (block 2 differs ...)`, rescanned from block 1 to the new head (27) and later scans continued
from there. The run found two script defects, both fixed: `step_stop` did not wait for the sequencer to exit (the next
`step_sequencer` saw port 3040 still in use), and `step_accounts` kept the previous wallet's `engine_admin`/`config`
in the run's deployment file (now reset to `null`).

### Plan 3b Task 5 re-run (2026-10-14, Docker binaries)

The same 14 steps (`step_build` to `step_stop`) were repeated on a fresh sequencer, now deploying the committed
binaries in `artifacts/guests` (ADR-047) instead of the native ELFs; `step_build` reads the ImageIDs from
`artifacts/guests/manifest.json`. Result: 34 transactions `included`, `no failed execution check`, and the same final
balances (owner-coll 99_998_999_950, owner-stable 70_000, keeper-coll 50, taker-stable 500, taker-protocol 9_800,
buffer 29_500). `host bind` and `admin verify` print the Docker ImageIDs:

| Program | ImageID (Docker, deployed) | Native (in-process tests only) |
|---|---|---|
| stablecoin | `bda698c4e929e4f63ad7b2949c8e413c26fc69df02434844067baded95c7b40d` | `91c790a2...6953` |
| auction_engine | `bb506524bfab7ce5d45a4f4836c00a90d7f23a769a2b6c988c66eb82d456f342` | `de829674...fc68` |

The native ids are unchanged since the Plan 3a run above (no guest source changed in Plan 3b so far); token and
twap_oracle are the upstream ids as before.

### Plan 3b re-run (Task 12, M7, 2026-10-16)

Closes Plan 3a M7: the hardened smoke had not been re-run after its last fix round. The smoke now runs on the
shared library `scripts/lib/localnet.sh` (also used by `scripts/demo.sh` and the CI end-to-end job); the step names
and behaviour are unchanged, and it deploys the committed guests in `artifacts/guests` (ADR-047). Run as
`scripts/wsl-env.sh scripts/localnet-smoke.sh step_build ... step_stop` (the 14 steps), dev-mode sequencer on the S1
builds: exit 0, 34 transactions `included`, `no failed execution check`, about 12 minutes wall time, and the same
final balances (owner-coll 99_998_999_950, owner-stable 70_000, keeper-coll 50, taker-stable 500, taker-protocol
9_800, buffer 29_500). The sequencer was stopped by `step_stop` and port 3040 was free afterwards.

`scripts/bootstrap-lez.sh` builds the LEZ v0.2.4 sequencer and wallet a clean machine lacks. It is idempotent: it
exits at once when both binaries and both config files (`sequencer_config.json`, `wallet_config.json`) exist,
otherwise it fetches the pinned LEZ revision (a warm CI cache restores binaries only, so the checkout always runs
first) and builds what is missing. `LEZ_DEPS` defaults to the S1 builds when they exist, else `$HOME/lez-deps`; it
runs `scripts/disk-guard.sh` first, and exits 4 with the apt line when a tool is missing. Its `target/` is the one
cargo target outside the shared plan2 directory and applies to clean machines only.

Preflight (`ln_preflight`, shown by `scripts/demo.sh --preflight-only`, pinned by `scripts/tests/demo-preflight.sh`)
stops before any sequencer starts:

| Exit | Reason |
|---|---|
| 3 | disk: `scripts/disk-guard.sh` (thresholds `DEMO_MIN_C_GIB`, default 3, and `DEMO_MIN_HOME_GIB`, default 20) |
| 4 | a missing LEZ binary or config file, or a missing tool (python3, ss, awk, sha256sum) |
| 5 | port 3040 is in use |
| 6 | the guests in `GUESTS_DIR` do not match `artifacts/guests/manifest.json` |

`ln_build` refuses to build under WSL when `CARGO_TARGET_DIR` is unset: run the smoke and the demo through
`scripts/wsl-env.sh`.

## Plan 2 run (spel)

History, kept unchanged below. The Plan 2 script (spel based, with `step_verify_and_bind` and `localnet.env`) is `git show cb21843:scripts/localnet-smoke.sh`; the current `scripts/localnet-smoke.sh` is the Plan 3a one. Its engine ImageID `3d0d15fa...` is superseded by the one above.

Proof that the stablecoin (with the engine hooks) and the auction engine deploy to and run on a real standalone
LEZ v0.2.4 sequencer (`RISC0_DEV_MODE=1`, dev-mode proofs) and that the core flow works end to end:
`liquidate` -> `bid_with_settlement` -> `claim_leftover`. Clients are `spel-cli` (Plan 3 replaces it with SDK
encoders). Runbook: the Plan 2 script (`git show cb21843:scripts/localnet-smoke.sh`); helper: `programs/integration_tests/examples/localnet_tool.rs`.

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
   Resolved for localnet by the publisher (ADR-050, Plan 3b Task 10).
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
