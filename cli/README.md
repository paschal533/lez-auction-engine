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

`list-auctions`, `list-at-risk`, `rescan` and `watch` take `--batch N` (blocks per `getBlockRange` request, default 100): lower it when 100 blocks exceed the node's 16 MiB response cap. Lists read each discovered account from the chain and drop ids that no longer decode. Ids that decode but are the wrong account, or that the engine rejects, are skipped with `warning: skipped <id>: ...` on stderr; stdout stays script-safe.

`rescan --from <block>` repositions the cursor and keeps what was already discovered; `rescan --from 1` rebuilds everything and is also the repair for a corrupt cursor file. Blocks before `<block>` are not scanned unless the cursor already covers them. Such a gap is reported on stderr (`note: blocks X..N-1 skipped`).

When the deployment file has `"engine_admin": null`, the admin shown by `config` comes from the chain alone: check that it is yours before signing anything.

Quotes are valid only at their clock; the transaction commands derive their slippage bounds from a fresh quote.

## Tests

Golden tests (`cli/tests/golden.rs`) run every command against the SDK's standard fixture; no network. Regenerate with `UPDATE_GOLDEN=1` and review the diff.

## Transaction commands

Every one verifies the config (exit 2 on mismatch), prints the config's admin and where that admin comes from (before anything is signed), signs with the wallet (exit 4 on a keystore problem), sends, and confirms by nonce polling: `included` when the signer's nonce moves (the touched accounts are then re-read and printed as `after ...` lines, read from the chain, not computed), exit 3 ("dropped: nothing was applied; re-quote") when it has not moved 5 blocks later. One transaction per signer is in flight at a time (lock in `<state-dir>/locks`). A send whose result is unknown, or a confirmation that times out, prints "outcome unknown, lock kept", how to check, and exits 1; nothing is ever resent automatically.

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
