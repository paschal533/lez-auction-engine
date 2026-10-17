# Legacy testnet run (LEZ v0.2.4)

`scripts/testnet-legacy.sh` runs the demo scenario of [DEMO.md](../../DEMO.md) on the public LEZ v0.2.4 legacy
testnet, `https://testnet-legacy.lez.logos.co/` (JSON-RPC over HTTPS, `POST /`), instead of a standalone
sequencer. No testnet run is recorded in this document yet; the rehearsals below ran on a local sequencer.

## Why (interim)

The public testnet (`testnet.lez.logos.co`) runs LEZ v0.3.1, which this PoC does not target, and RFP-014 is moving
to testnet 0.4 ([v03-port-plan.md](../v03-port-plan.md)). Until 0.4 is released, the interim public-testnet evidence
(RFP S1) is a deployment on the legacy testnet, which runs LEZ build `v0.2.4-lez-legacy`, wire and RPC compatible
with the v0.2.4 this PoC is built and tested against. The legacy testnet is likely temporary.

## What it does

1. Preflight, read-only (`--preflight-only` stops here): the node answers `getLastBlockId`; its block 1 hash is
   `c8e3bb9ec8f246e5c2759e0e8152840aeb7df29948a9ad12c9c302a641230011` (ADR-046 chain identity; `TESTNET_GENESIS`
   overrides it after a reset, with a warning); the committed guests match `artifacts/guests/manifest.json`; for
   each guest, whether the node has its deployment transaction; and, once `auction-cli` is built, that the
   stablecoin host is still uninitialised.
2. A new run directory `run-<UTC time>` under the state directory (default `$HOME/lez-target/testnet-legacy`,
   `--state-dir` or `TESTNET_STATE_DIR`). The script runs with `umask 077` (directories 700, files 600), refuses a
   state directory under `/mnt` (a Windows drive under WSL has no Unix permissions, so the keys would be readable
   by anyone), refuses a non-empty state directory without its marker `.testnet-legacy-state`, and never deletes
   anything: each run's directory keeps its wallet (the keys of its testnet accounts), CLI state, logs and summary.
3. Builds `auction-cli`, writes `deployment.json` with `"network": "testnet-legacy"`, the URL, the manifest
   ImageIDs and the expected block 1 hash, and checks block 1 again through `auction-cli dev chain-id`.
4. Fresh keys for every label (a new wallet per run, so the config `["CONFIG", admin, 1]`, the positions
   `[owner, nonce]`, the oracle `[price source, window]` and the collateral and protocol token definitions are
   accounts nobody else controls), `auth-transfer init`, then deploys `stablecoin.bin` and `auction_engine.bin`,
   skipping one the node already has. `token`, `twap_oracle` and `token_mint_authority` are already on the testnet
   with the manifest ImageIDs and are not deployed. Program deployment is an unsigned, free
   `ProgramDeploymentTransaction`; no funding is needed (the scenario mints its own tokens with `dev token-new`).
5. The scenario of `scripts/demo.sh`, from the same file (`scripts/lib/demo-scenario.sh`): positions, F1-F3,
   F5, price move to 1.9 R, shortfall, F4 and the freeze path, with the same checks. Before the first position
   the block scanner is seeded with `rescan --from <block after the deployments>`, so it does not scan the public
   chain from block 1.
6. `testnet-summary.txt` in the run directory: the phases and checks, then, on every exit (success, failure or
   interruption), an evidence block: the deployment file, every label's account id, the scenario's account ids
   and phase markers, the wallet's deployment and `auth-transfer init` transaction hashes with their blocks, and
   every `auction-cli` transaction (`command | instruction | hash`, also in `transactions.txt`). The full console
   log is `logs/run.log`. The wallet's key setup output, which includes the recovery phrase, goes to
   `logs/wallet-accounts.log` and not to the console; if `wallet account new` fails, its error lines are printed
   without the phrase.

## How to run

Inside WSL on the author's machine (shared target directory):

    scripts/wsl-env.sh scripts/testnet-legacy.sh --preflight-only
    scripts/wsl-env.sh scripts/testnet-legacy.sh --send
    scripts/wsl-env.sh scripts/testnet-legacy.sh --resume $HOME/lez-target/testnet-legacy/run-<UTC time>

`--send` is required for anything to be sent. Needs the LEZ v0.2.4 wallet (`scripts/bootstrap-lez.sh`; the
sequencer binary is not used), python3 and outbound HTTPS. Exit codes: 2 usage or state directory, 3 disk,
4 wallet or tools missing, 6 guests do not match the manifest, 7 node unreachable, 8 another chain, 9 the
stablecoin host is already initialised, 10 a resume would risk sending a step twice.
`scripts/tests/testnet-legacy-preflight.sh` pins 2, 7 and 8, the `/mnt` refusal and the environment warning
against a local stub node.

**Back up the run directory as soon as the host phase is done** (`PHASE_host_done=1` in its `run.env`): its
`wallet-home` holds the only copy of the host and engine admin keys, and the host can be initialised only once
per chain. The script prints the reminder at the end of a run.

## Resume

A run is a list of named phases (`accounts`, `auth`, `deploy`, `host`, `engine`, `bind`, `collateral`,
`holdings`, `positions`, `f1f3`, `f5`, `price_move`, `shortfall`, `f4`, `freeze`, `final`) plus a keeper round
before six of them (`tick_<phase>`). Before a phase the script records every label's nonce
(`PHASE_<name>_nonces`) and `PHASE_<name>_started=1` in `run.env`, and `PHASE_<name>_done=1` after it. A failed or
interrupted run prints, on the console and in the summary: "host may be claimed; admin keys are in
`<run>/wallet-home`; resume with: `scripts/wsl-env.sh scripts/testnet-legacy.sh --resume <run>`; check nonces
before acting".

`--resume RUN_DIR` reuses the run's wallet, `deployment.json` (its URL and genesis; another `--url` is refused),
`run.env` and `cli-state`, rebuilds `auction-cli` without rewriting the deployment file, and skips every done
phase (so the host check and the fresh-wallet refusal do not apply to that run). A phase that started but did not
finish is re-run only when nothing of it was applied: no in-flight lock in `cli-state/locks` (an unknown outcome)
and every nonce equal to the recorded ones. Otherwise the script stops with exit 10, lists the nonces that moved
and says to finish that phase by hand (check each nonce before acting), add `PHASE_<name>_done=1` and resume
again. `accounts` sends nothing, so an interrupted one starts again (the unused keys are moved aside, not deleted);
`deploy` is idempotent (deployed guests are detected and skipped).

## Mock oracle keeper

The market price is our own twap_oracle price account, signed by the run's `oracle-src` key (ADR-050), not a
real feed. The collateral config allows an oracle age of 1 h (`max_oracle_age_ms`, `cli/params/demo-collateral.json`),
and a run on 60 s blocks takes longer than that. Before every scenario phase from `positions` on (except the price
move, which publishes itself) the script runs one round of `auction-cli dev keeper` with the current price (R, then
1.9 R after the move): it re-publishes when the oracle is within `KEEPER_REFRESH_BEFORE_MS` (default 30 min) of
its maximum age. The keeper runs in line, as its own guarded step, not as a background process: it signs with the
same host and engine admin keys as the scenario, and a concurrent signer would race the scenario's nonces and
in-flight locks.

## Guard

`auction-cli`'s sending dev commands accept exactly two network names, `localnet` and `testnet-legacy`
(`DEV_NETWORKS` in `cli/src/dev_cmds.rs`), and on both the node's block 1 must equal the deployment file's
`chain_genesis_hash` (ADR-046; a name alone is never enough, Plan 3a M6). Any other name, or a file without the
hash, is refused before signing.

## Rehearsals (local sequencer, not the testnet)

All on 2026-10-17, against a standalone LEZ v0.2.4 sequencer that already had the three upstream guests, with
`--url http://127.0.0.1:3040/` and `TESTNET_GENESIS` set to that sequencer's block 1.

| Blocks | What | Result |
|---|---|---|
| 15 s | `--send` (first version, `KEEPER_REFRESH_BEFORE_MS=3500000` so most keeper rounds publish) | `demo complete` in 17 min, every check `ok` (unbacked debt 42_276), 4 keeper rounds published, 49 `auction-cli` and 8 wallet transactions in the summary; the wallet's two deployment hashes equal `lez_rpc.py deploy-hash` |
| 15 s | `--send` killed (SIGTERM to the process group) right after `PHASE_host_done`, then `--resume` | resume skipped accounts, auth, deploy and host and completed: every check `ok` (unbacked debt 42_283, `frozen yes`); 847 s for both. Removing `PHASE_freeze_done` and resuming again stopped with exit 10 (`engine-admin nonce 4 -> 5`, `caller nonce 7 -> 8`): nothing re-sent |
| 15 s | `--send` killed after `PHASE_auth_done` | the failure banner reached the console and the summary (exit 143) |
| 60 s | `--send`, uninterrupted (a copied sequencer config with `block_create_timeout` `60s`, via `SEQ_CONFIG`) | `demo complete`, every check `ok` (unbacked debt 42_652, inside the R4 range; `frozen yes`); 37 `auction-cli` and 8 wallet transactions; every keeper round found the price fresh (oldest 21 min; the price move re-publishes); 2850 s wall time including the sequencer start and the three upstream deployments |

## Caveats

- **One run per chain.** The stablecoin host is a single instance per program: its `ProtocolParameters`,
  stablecoin definition and engine binding are PDAs of the stablecoin program alone. The first `--send` run
  initialises them with its own admin key; a later run stops at preflight with exit 9. A second run needs a reset
  testnet (a new block 1, so a new `TESTNET_GENESIS`) or a different stablecoin build, which would change the
  ImageID in the manifest. An interrupted run is continued with `--resume`, not started again.
- **The legacy testnet may be reset or retired.** A reset changes block 1: the preflight then exits 8, and the
  deployment files of earlier runs no longer match (the dev commands refuse them, exit 2).
- **60 s blocks.** Every transaction waits for its block, so a run takes about 50 minutes (2850 s rehearsed: 37
  `auction-cli` and 8 wallet transactions, more when keeper rounds publish). The script passes `--drop-after-blocks 10` to `auction-cli` (a transaction counts as
  dropped after 10 blocks without inclusion; the CLI default is 5). The wallet's inclusion poller sleeps
  `seq_poll_timeout` `seq_tx_poll_max_blocks - 1` times, so the script's 30 s and 40 give 39 sleeps, 19.5 min per
  wallet transaction. The shortfall figure depends on how long the bid waits after the liquidation; the check is
  the same R4 range as the localnet demo.
- **A flaky endpoint.** The public endpoint answered 502 and failed DNS resolution a few times on 2026-10-17. Reads
  are retried (5 tries over about 30 s, in `auction_sdk::rpc` and `scripts/lib/lez_rpc.py`), confirmation polling
  waits through up to 5 failed polls in a row, and `sendTransaction` is never retried: an unknown send outcome
  keeps the in-flight lock and says to check the nonce before acting.
- **Already deployed programs** are detected by the hash of their deployment transaction, which depends on the
  bytes alone (`scripts/lib/lez_rpc.py deployed`): `getTransaction` finds it only if exactly these bytes were
  deployed. LEZ v0.2.4 has no RPC that lists deployed programs (`getProgramIds` returns the built-ins), so a
  program deployed from other bytes, or present in the genesis state, is not seen. On 2026-10-17 the node had the
  deployments of the committed `token`, `twap_oracle` and `token_mint_authority` (blocks 2301, 2308 and 2298) and
  neither of ours. Re-deploying an existing program is rejected by the node (`ProgramAlreadyExists`) and the
  wallet times out waiting for it.
- **Block size.** `auction_engine.bin` is 731 116 bytes and `stablecoin.bin` 521 704; the LEZ debug sequencer
  allows 1 MiB blocks. The legacy testnet's limit is not published; a larger guest than the limit would be refused
  at `sendTransaction` ("Transaction too large").
