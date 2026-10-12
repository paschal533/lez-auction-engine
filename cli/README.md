# auction-cli

Command-line client of the RFP-014 auction engine (spec §6.6), built on `auction_sdk` (own encoders; not spel-cli, which mis-encodes stablecoin instructions, spec §3.2). LEZ v0.2.4, public transactions only.

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

`list-auctions`, `list-at-risk`, `rescan` and `watch` take `--batch N` (blocks per `getBlockRange` request, default 100): lower it when 100 blocks exceed the node's 16 MiB response cap. Lists read each discovered account from the chain and drop ids that no longer decode.

`rescan --from <block>` repositions the cursor and keeps what was already discovered; `rescan --from 1` rebuilds everything and is also the repair for a corrupt cursor file. Blocks before `<block>` are not scanned unless the cursor already covers them.

When the deployment file has `"engine_admin": null`, the admin shown by `config` comes from the chain alone: check that it is yours before signing anything.

Quotes are valid only at their clock; the transaction commands derive their slippage bounds from a fresh quote.

## Tests

Golden tests (`cli/tests/golden.rs`) run every command against the SDK's standard fixture; no network. Regenerate with `UPDATE_GOLDEN=1` and review the diff.
