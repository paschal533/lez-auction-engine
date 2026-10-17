# auction-cli

Command-line client of the RFP-014 auction engine (spec §6.6), built on `auction_sdk` (own encoders; not spel-cli, which at our pin `1ef0500f` mis-encodes stablecoin instructions, spec §3.2; fixed upstream after SPEL v0.7.0). LEZ v0.2.4, public transactions only.

## Build (WSL)

    scripts/wsl-cargo.sh build -p auction_cli        # binary: $HOME/lez-target/plan2/debug/auction-cli

## Global options

| Option | Default | Meaning |
|---|---|---|
| `--deployment PATH` | `deployments/localnet.json` | trusted ImageIDs, sequencer URL, optional config and admin (I4) |
| `--config ID` | the file's `config` | engine config account |
| `--rpc URL` | the file's `sequencer_url` | sequencer JSON-RPC |
| `--wallet-home DIR` | `$LEE_WALLET_HOME_DIR` | LEZ wallet home (`storage.json`) |
| `--state-dir DIR` | `.auction-cli` | scan cursor (`<config>.cursor.json`) and in-flight locks |
| `--at-ms MS` | chain clock | evaluate quotes at this time |
| `--drop-after-blocks N` | `5` | blocks without inclusion after which a sent transaction counts as dropped (`scripts/testnet-legacy.sh` uses 10 on 60 s blocks) |

Account arguments accept a base58 id or a wallet label. Every command that touches a config first verifies it against the deployment file (owner = engine ImageID, `["CONFIG", admin, salt]`, stored host/token program ids, admin, host `ProtocolParameters` owner) and refuses on any mismatch.

## Exit codes

`0` ok · `1` usage or other error (including an unreadable deployment file) · `2` verification failed (wrong network / ImageID) · `3` transaction dropped: nothing applied, re-quote · `4` keystore error.

## Read-only commands

    auction-cli config                         # verified config; the admin and whether the deployment file vouches for it
    auction-cli quote-liquidation --position <id>
    auction-cli quote-bid --auction <id> --coin-in-max <coin> [--slippage-bps 50]
    auction-cli list-auctions [--all]          # scans new blocks first; flags auctions held at max discount
    auction-cli list-at-risk                   # positions unsafe at the host's minimum ratio
    auction-cli rescan [--from <block>]
    auction-cli watch [--interval-ms 15000] [--iterations N]

`list-auctions`, `list-at-risk`, `rescan` and `watch` take `--batch N` (blocks per `getBlockRange` request, default 100): lower it when 100 blocks exceed the node's 16 MiB response cap. Lists read each discovered account from the chain and drop ids that no longer decode. Ids that decode but are the wrong account, or that the engine rejects, are skipped with `warning: skipped <id>: ...` on stderr; stdout stays script-safe.

`rescan --from <block>` repositions the cursor and keeps what was already discovered; `rescan --from 1` rebuilds everything and is also the repair for a corrupt cursor file. Blocks before `<block>` are not scanned unless the cursor already covers them. Such a gap is reported on stderr (`note: blocks X..N-1 skipped`).

When the deployment file has `"engine_admin": null`, the admin shown by `config` comes from the chain alone: check that it is yours before signing anything.

Quotes are valid only at their clock; the transaction commands derive their slippage bounds from a fresh quote.

## Tests

Golden tests (`cli/tests/golden.rs`) run every command against the SDK's standard fixture; no network. Regenerate with `UPDATE_GOLDEN=1` and review the diff.

## Transaction commands

Every one verifies the config (exit 2 on mismatch), prints the config's admin and where that admin comes from (before anything is signed), signs with the wallet (exit 4 on a keystore problem), sends, and confirms by nonce polling: `included` when the signer's nonce moves (the touched accounts are then re-read and printed as `after ...` lines, read from the chain, not computed), exit 3 ("dropped": check the signer's nonce before acting; if it did not move, nothing was applied, re-quote) when it has not moved `--drop-after-blocks` (default 5) blocks later. Polling waits through up to 5 consecutive failed reads; reads are retried with backoff (5 tries over about 30 s), `sendTransaction` never. One transaction per signer is in flight at a time (lock in `<state-dir>/locks`). A send whose result is unknown, or a confirmation that times out, prints "outcome unknown, lock kept", how to check, and exits 1; nothing is ever resent automatically.

    auction-cli liquidate --position <id> --keeper <signer> --keeper-holding <collateral holding>
    auction-cli bid --auction <id> --coin-in-max <coin> --bidder-stable <holding> --bidder-collateral <holding> [--slippage-bps 50 | --min-out N]
    auction-cli claim-leftover --owner <signer> --owner-holding <holding> --auction <id>
    auction-cli settle-empty --caller <signer> --auction <id>
    auction-cli settle-debt --caller <signer> [--amount N]
    auction-cli check-solvency --caller <signer>
    auction-cli observe-feed --caller <signer>
    auction-cli start-surplus --caller <signer>
    auction-cli take-surplus --payer <protocol-token holding> --receiver <stablecoin holding> --amount <coin> [--slippage-bps 50]
    auction-cli close-surplus --caller <signer>
    auction-cli start-debt --caller <signer>
    auction-cli take-debt --payer <stablecoin holding> --receiver <protocol-token holding> --amount <coin> [--slippage-bps 50]
    auction-cli close-debt --caller <signer>
    auction-cli watch --observe-with <signer>  # refresh the pause clock every round

`bid` quotes first and sets `min_collateral_out` from the quote and the slippage (so a stale quote reverts with `Slippage` instead of filling worse); it sends `bid_with_settlement` when the fill may end the auction (ADR-029). `liquidate` refuses, before signing, a position the host would not liquidate. `start-surplus` / `start-debt` refuse while one is live, and `start-debt` refuses inside the relist delay (ADR-039). A fresh (never initialised) recipient holding must also be in the wallet: it signs (`RecipientMustSign`). Signers that sign more than one stablecoin-program transaction must be `wallet auth-transfer init`-ed first (ADR-018). Never unfreeze the host, or advise it, while settled bad debt is above the freeze threshold (THREAT_MODEL); `host_frozen_by_engine` means "has ever frozen".

`watch --observe-with` stops on the first observe that fails or is dropped (exit code as mapped above); a keeper should run it under a supervisor or restart loop. `--at-ms` is for read commands only: a transaction command refuses it (exit 1, nothing sent), because its slippage bounds must come from the chain clock. `--slippage-bps` must be below 10000. With an explicit `--min-out`, `--slippage-bps` still sets the near-empty margin that chooses `bid_with_settlement`. `config` shows the engine accounting and the live system auctions, which is what to check after an unknown outcome of a take, start or close.

## Admin, host and dev commands (localnet setup without spel)

    auction-cli admin addresses --admin <label> [--salt 1]
    auction-cli admin init-config --admin <label> [--salt 1] --params cli/params/localnet-engine.json [--protocol-token <id|label>] [--ref-oracle <id>]
    auction-cli admin set-engine-params --admin <label> --params <json> [--protocol-token ...] [--ref-oracle ...] [--disable-protocol-token] [--disable-ref-oracle]
    auction-cli admin set-collateral-config --admin <label> --params cli/params/localnet-collateral.json
    auction-cli admin verify
    auction-cli host addresses
    auction-cli host status                         # read-only: freeze flag, admins, market oracle, min ratio, max oracle age
    auction-cli host bind --admin <host admin>      # refuses unless the config verifies (ADR-027)
    auction-cli host init-program --admin <l> --freeze <l> --collateral-def <l> --oracle <id> [--kp <i128>] [--ki <i128>] ...
    auction-cli host open-position --owner <l> --owner-collateral <l> [--nonce 0] --amount <n>
    auction-cli host generate-debt --owner <l> --position <id> --owner-stable <l> --amount <n>
    auction-cli host set-min-cr --admin <l> --ratio <RAY>
    auction-cli host set-controller-gains --admin <l> --kp <i128> --ki <i128>   # negative values allowed
    auction-cli dev resolve --label <l>
    auction-cli dev token-new --definition <l> --holding <l> --name <s> --supply <n> [--mint-authority <id>]
    auction-cli dev token-init-account --definition <id|l> --account <l>
    auction-cli dev token-transfer --from <l> --to <id|l> --amount <n>
    auction-cli dev oracle-create --price-source <l> --base <id> --quote <id|l> --price <n> [--window-ms 3600000]
    auction-cli dev oracle-publish --price-source <l> --price <RAY> --host-admin <l> --engine-admin <l> [--base-window-ms 3600000]
    auction-cli dev keeper <the same flags> [--interval-ms 60000] [--iterations N] [--refresh-before-ms 600000]
    auction-cli dev balance --account <id|l>
    auction-cli dev chain-id

`dev resolve`, `dev balance`, `dev chain-id`, `host addresses` and `host status` are read-only and work on any network (they print no deployment note). The other dev commands sign, so they need a named dev network in the deployment file (`"localnet"` or `"testnet-legacy"`) and the chain it was recorded on: the file's `chain_genesis_hash` (64 lowercase hex characters, the hash of block 1) must equal the node's, otherwise the command exits 2 before signing (ADR-046). Record it with `auction-cli dev chain-id` against the intended node (the output line `genesis <hex>` gives the value); the smoke script does this right after it starts the sequencer. The committed `deployments/localnet.json` has `"chain_genesis_hash": null`, so the state-changing dev commands refuse with it until a run records the value.

Parameter files hold `u128` values as decimal strings; the protocol-token and reference-oracle flags come from `--protocol-token` / `--ref-oracle`. An enabled protocol token must already exist as a fungible token definition (ADR-038): create it first with `dev token-new ... --mint-authority <mint authority from admin addresses>`. Every reference-price change printed by `set-engine-params` goes into `DEPLOYMENTS.md` (ADR-017).

The parameter files in `cli/params/`: `localnet-engine.json` and `localnet-collateral.json` are the localnet defaults. `demo-engine.json` and `demo-collateral.json` are the one-command demo's (ADR-051): they differ from the localnet files only in `debt_lot` (`5000`, was `500`), `bad_debt_freeze_threshold` (`5000`, was `1000`) and `band_hi_bps` (`20000`, was `11000`), so that a price move to 1.9 R leaves a shortfall large enough for a debt auction and the freeze path. `auction_sdk::demo` holds the same values; a test pins that the files match.

`set-engine-params` rewrites the protocol-token and reference-oracle slots from its flags, so an omitted flag would disable a slot the config has: the command refuses unless `--disable-protocol-token` / `--disable-ref-oracle` is passed, and prints `old -> new` for every slot that changes. `--protocol-token` is checked client-side (ADR-038) to be a fungible definition of the token program. `admin init-config` refuses a signer other than the deployment file `engine_admin`. Signing `dev` commands only run when the deployment file names a dev network (`localnet` or `testnet-legacy`) and its recorded genesis matches the node. `host bind` prints the engine the host is bound to now before it re-binds; `host set-min-cr` / `set-controller-gains` print `old -> new`.

### Dev-network price publisher and keeper (ADR-050)

`dev oracle-publish` moves the localnet market price by rotation, in three transactions: a new twap_oracle price account for the same `--price-source` at the next free window (starting at `--base-window-ms`), the host's `SetMarketPriceOracle`, then the engine's `set_collateral_config` with unchanged parameters and the new oracle (ADR-028: the engine must follow the host). It runs on the dev networks only (localnet and the legacy testnet; the dev-command guard, ADR-046). On the legacy testnet the price source is our own mock oracle, not a market feed. Before anything is signed it verifies the config, checks that `--host-admin` and `--engine-admin` are the recorded admins (exit 2) and runs the engine's own feed validation on the new price: a price outside `[R/2, 2R]` (R the redemption price) or zero is refused, naming the engine error (`PriceFeedOutOfRange`, ...), with exit 1 and nothing sent.

`dev keeper` repeats it: every `--interval-ms` it reads the oracle's age and publishes `--price` again when age plus `--refresh-before-ms` reaches the configured maximum age; otherwise it prints `fresh`. `--iterations N` stops after N rounds (default: run until interrupted).

The price source signs the new price account on every publish, so on a real localnet that account must be initialised with `wallet auth-transfer init` first (ADR-018).

Partial failure: between the host rotation and the engine follow-up the host and the engine point at different oracles, and `liquidate` fails with `OracleMismatch`. If the command stops after the second transaction, run `dev oracle-publish` again: it creates the next window's account and rotates both. `host status` shows the host's current market oracle and the freeze flag.
