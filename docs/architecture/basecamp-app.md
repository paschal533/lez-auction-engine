# Basecamp app

The Auctions app puts the engine in Basecamp. It is built from three parts, all in this repository, and each has its own `flake.nix` on `logos-module-builder` `de169fd` (the pin Basecamp master uses).

| Part | What it is | Talks to |
|---|---|---|
| `crates/auction_ffi` | Rust cdylib over `auction_sdk`, with a committed cbindgen header and seven symbols: `version`, `view`, `prepare`, `tx_begin`, `tx_poll`, `tx_end`, `free`. `view` and `prepare` dispatch on a JSON name (ADR-055). Every reply is an `{ok, value \| error{kind, message}}` envelope, and u128 values are decimal strings. | The sequencer RPC, for reads and scans only (ADR-052) |
| `modules/auction_engine` | Basecamp core module (C++, universal interface) that links `auction_ffi` and depends on `lez_core`. Methods: `configure`, `openWallet`, `blockHeight`, `submit`, `txStatus`, `view`, `accounts`, `rescan`. | `auction_ffi`, and `lez_core.send_generic_public_transaction` to sign and send |
| `apps/auctions` | `ui_qml` app: a C++ `.rep` backend that forwards to the core module, and QML on Logos.Theme and Logos.Controls (design system `88330d04`, ADR-054). Screens: Live, At-risk, Settled, System, Dev tools. Every interactive item has an `objectName`. | `auction_engine` through QtRO |

Rules that hold across the three parts:
- **No call blocks.** IPC calls time out at 20 s. Each FFI call has a 12 s deadline, each module call a 15 s budget, and scans are paged (ADR-052).
- **Confirmation is polled.** `submit` returns a `txId` at once, and the app polls `txStatus` every 2 s. ADR-042's nonce rule and per-signer lock files hold (ADR-053).
- **Same numbers as the CLI.** Quotes, plans and facts come from `auction_sdk::ops` and `auction_sdk::views`, the functions the CLI prints from. The FFI parity tests and the story's UI = CLI checks pin this.

Decisions: ADR-052 to ADR-055 in [design-log.md](../design-log.md). CI: the `basecamp-module` workflow ([ci.md](ci.md)).

## Host

The Auctions app is developed and run in Basecamp's standalone mode, inside WSL. `scripts/basecamp/host.sh` builds the two things it needs and guards the C: disk.

Pinned inputs:

| Input | Pin | Output used |
|---|---|---|
| Basecamp | `github:logos-co/logos-basecamp/e09962f5998792afd3bfed8c69c3b8f07f5825a5` (2026-10-15, version `0.0.0-dev`) | `#app-standalone` |
| lez_core wallet module | `github:logos-blockchain/logos-execution-zone-module?rev=acf0cd501b262c4c15969e3735e85318297b85bf` (`lez_core` 0.4.1) | `#install` |

`#app-standalone` already has the QML inspector on (port 3768). `#install` is an install root with `modules/lez_core/` (plugin, `libwallet_ffi.so`, `manifest.json`) and an empty `plugins/`. The dev build variant matches the `#install` output (the portable variant would pair with `#install-portable`). Nix is 2.34.8.

Commands (inside WSL, or from Git Bash, where the script re-runs itself through `wsl`):

```bash
scripts/basecamp/host.sh check   # "C: <N> GB free (min 10): ok"; exit 2 and "refusing: ..." below the minimum
scripts/basecamp/host.sh build   # check, nix build Basecamp, check, nix build lez_core, check
scripts/basecamp/host.sh paths   # BASECAMP_BIN=... and LEZ_CORE_ROOT=... lines
```

`MIN_C_GB` overrides the 10 GB minimum (`MIN_C_GB=1000 scripts/basecamp/host.sh check` exits 2). Result links are under `~/lez-target/basecamp/` (`app`, `lez_core`). The WSL disk is a file on C:, so C: free space is what runs out; the first build took C: from 31 GB to 18 GB free.

Launch smoke (empty install root with `modules/` and `plugins/`):

```bash
EMPTY=$(mktemp -d); mkdir -p $EMPTY/modules $EMPTY/plugins
XDG_RUNTIME_DIR=/mnt/wslg/runtime-dir $BASECAMP_BIN --user-dir "$(mktemp -d)" \
  --modules-dir $LEZ_CORE_ROOT --module $EMPTY
```

Result: the app starts, the log shows `Inspector server listening on port 3768`, and a TCP connection to `localhost:3768` is accepted. Without `XDG_RUNTIME_DIR` Basecamp aborts in WSL. `--user-dir` keeps the run out of `./.logos-basecamp-dev`.

## Skeleton end-to-end (lez_core proof)

`scripts/basecamp/run-app.sh` runs the Auctions app in standalone Basecamp against a localnet:

```bash
scripts/basecamp/run-app.sh [--setup] [--test FILE]
```

- `--setup` first runs `scripts/demo.sh --setup-only --sequencer-dev-mode 1`: the demo phases `accounts` to `positions`, then it keeps the sequencer running and prints `AUCTIONS_DEPLOYMENT=… LEE_WALLET_HOME_DIR=…` (the run's filled deployment file and wallet, never `deployments/localnet.json`).
- It builds `modules/auction_engine#install` and `apps/auctions#install` (result links under `~/lez-target/basecamp/`) and launches `$BASECAMP_BIN --modules-dir $LEZ_CORE_ROOT --modules-dir <auction_engine install> --module <auctions install>` with that environment, `AUCTIONS_STATE_DIR=<run dir>/cli-state` (the CLI's locks) and `XDG_RUNTIME_DIR=/mnt/wslg/runtime-dir`. The Basecamp log is `<run dir>/logs/basecamp.log`.
- `--test FILE` runs `node FILE` against the inspector on port 3768 (logos-qt-mcp from Basecamp `e09962f5`), exits with its code, and stops Basecamp and, with `--setup`, the sequencer. Without `--test` both keep running.

`apps/auctions/tests/skeleton.mjs` opens Dev tools, transfers `1` from `owner-stable` to `taker-stable`, waits up to 120 s for `devTransferStatus` to read `Done`, and checks with `auction-cli dev balance --account taker-stable` that the balance rose by 1. `SKELETON_SCREENSHOT=<png>` also saves a screenshot of the header and the five tabs.

Result (2026-10-25, inside WSL, Basecamp `e09962f5`, lez_core `acf0cd50`, sequencer in dev mode):

```text
$ scripts/basecamp/run-app.sh --setup --test apps/auctions/tests/skeleton.mjs
...
Running 1 test(s)...

    taker-stable balance: 0 -> 1
  ✓ skeleton: Dev tools token transfer goes through lez_core and lands on chain

1 passed, 0 failed
```

So `lez_core` loads in Basecamp master, accepts the plan's account ids, signer flags, instruction words as little-endian bytes and the program id hex, and sends a transaction that the sequencer includes (ADR-018). The keystore signing fallback is not needed.

![Auctions app skeleton in Basecamp](img/skeleton.png)

## The whole demo story in Basecamp (Plan 4 Task 16)

`apps/auctions/tests/story.mjs` drives every step of `scripts/lib/demo-scenario.sh` after the setup phases as UI actions, in its order (R39), with the matching account picked in the header: liquidate P0 (keeper) → bid 100000, then 230000 (owner) → claim the leftover (owner) → observe feed, settle debt, check solvency, start surplus (caller) → take surplus 500 (taker) → publish price 1.9 (admin; signed by oracle-src, admin and engine-admin, three transactions) → liquidate P2 (keeper) → bid 330000 (owner) → settle debt, start debt (caller) → transfer 5000 owner-stable → taker-stable (Dev tools) → take debt 5000 (taker) → disable protocol token (engine-admin) → check solvency (caller) → the header shows Frozen. The steps are data (`apps/auctions/tests/lib/story-steps.mjs`, `FULL_STORY`); the runner refuses a story with a repeated step id.

After the steps that move the books it reads the figure from the screen by `objectName` (a System `Fact`'s raw `value`, the debt take's quote, the header badge) and checks it against ADR-051 and against `auction-cli` on the same state (`config` for unbacked debt, `dev balance` of the buffer and of `taker-protocol`, `host status` for frozen). Six of the seven checks read the figure from the screen. The protocol-token check is the exception: it compares the CLI balance of `taker-protocol` before the take plus the on-screen quote with the CLI balance after. It writes `story-timeline.json` (step id, caption, start/end ms, lane waits) next to the deployment (`STORY_TIMELINE` overrides) and pauses `STORY_PACE_MS` after each step (default 0). `STORY_SHOTS_DIR` keeps screenshots of the key steps; `STORY_FROM=<step id>` resumes on the same localnet.

One step differs from the CLI script: after F1–F3 nothing is unbacked, so Settle debt quotes 0 and its button stays disabled; the story checks the quote reads 0 instead of sending the CLI's no-op `settle-debt`.

Result (2026-10-25, inside WSL, Basecamp `e09962f5`, sequencer in dev mode, fresh localnet):

```text
$ scripts/basecamp/run-app.sh --setup --test apps/auctions/tests/story.mjs
...
    check buffer holds the 10 % penalty (F1–F3): ui 30000, cli 30000, expected 30000: ok
    check buffer after the surplus lot: ui 29500, cli 29500, expected 29500: ok
    check unbacked debt after the shortfall: ui 42277, cli 42277, expected [42154, 63200]: ok
    check the buffer settled 29,500 of the bad debt: ui 12777, cli 12777, expected 12777: ok
    check protocol tokens minted to the taker (balance before -> after): ui 13050, cli 13050, expected > 9800 (9800): ok
    check one lot of bad debt burned: ui 7777, cli 7777, expected 7777: ok
    check host frozen: ui yes, cli yes, expected yes: ok
  ✓ story: the whole demo runs in Basecamp, figures match ADR-051 and auction-cli
```

Wall time: 992 s for the whole command, of which about 660 s is `demo.sh --setup-only` (build check, sequencer, deploy, positions) and 329 s the 172 story steps; 283 s of those are lane waits (transactions being included).

## Installed as .lgx packages in the full Basecamp (Plan 4 Task 17)

`scripts/basecamp/install-lgx.sh` installs the app the way a user gets it: as `.lgx` packages in a Basecamp profile, run by the full Basecamp rather than standalone mode.

```bash
scripts/basecamp/install-lgx.sh PROFILE_DIR [--install-only] [run-app.sh options]
```

It runs these steps in order:

1. Builds five outputs, with result links under `~/lez-target/basecamp/`:
   - `#lgx` of lez_core `acf0cd50` (0.4.1), `modules/auction_engine` and `apps/auctions`;
   - the full Basecamp `#app` at `e09962f5` (the dev build, inspector on);
   - `lgpm`, from `logos-package-manager` `30b3a3f8`, the revision that Basecamp `e09962f5` locks.
2. Installs the core modules first: `lgpm --modules-dir PROFILE_DIR/modules install --file …` for lez_core, then auction_engine.
3. Installs the UI plugin second: `lgpm --ui-plugins-dir PROFILE_DIR/plugins install --file …` for auctions.
4. Hands over to `run-app.sh --profile PROFILE_DIR …`. This starts the full Basecamp with `--user-dir PROFILE_DIR` against the localnet. It uses the same `--setup`, `--test` and teardown as standalone runs.

Notes:

- With `--profile`, `story.mjs` first clicks `sidebar.app.auctions` (`STORY_OPEN_APP`). The full Basecamp starts on its welcome page.
- `--install-only` stops after the install.
- Use a Linux path for `PROFILE_DIR` (for example `/tmp/auctions-profile`), not one under `/mnt/c`.
- The packages are unsigned dev builds. lgpm warns `Package is unsigned` and installs them.
- With `--profile`, `run-app.sh` keeps its default path otherwise unchanged. Without it, it still runs standalone mode with a fresh temporary user dir.

**Unsupported: a profile with a newer lez_core (design call 3).** The app is built against lez_core 0.4.1 (LEZ v0.2.4, 4-argument send, enum-index instructions). Basecamp's catalogue can offer a newer `lez_core` (0.5.x, LEZ v0.3.1). A profile that already has one, or that is upgraded to one from the Package Manager, is unsupported. `install-lgx.sh` refuses such a profile with exit 4, before and after installing. It also refuses a profile whose lez_core manifest has no readable version (it fails closed) (`install-lgx: PROFILE has lez_core 0.5.x; the Auctions app needs lez_core 0.4.1 …`). Use a fresh `PROFILE_DIR`, and do not accept a lez_core upgrade in a profile that runs the Auctions app. Nothing in the app works around a newer lez_core.

`scripts/tests/install-lgx.sh` covers the script with stubbed `nix`, `lgpm` and `df`: the pinned refs, core-before-UI order, `--install-only`, the hand-over to `run-app.sh --profile`, the newer-lez_core refusals and the low-disk refusal.

Result (2026-10-25, inside WSL, fresh `/tmp/auctions-profile`, fresh localnet, sequencer in dev mode):

```text
$ scripts/basecamp/install-lgx.sh /tmp/auctions-profile --setup --test apps/auctions/tests/story.mjs
...
install-lgx: installed into /tmp/auctions-profile (lez_core 0.4.1, auction_engine, auctions)
...
    check buffer holds the 10 % penalty (F1–F3): ui 30000, cli 30000, expected 30000: ok
    check buffer after the surplus lot: ui 29500, cli 29500, expected 29500: ok
    check unbacked debt after the shortfall: ui 42302, cli 42302, expected [42154, 63200]: ok
    check the buffer settled 29,500 of the bad debt: ui 12802, cli 12802, expected 12802: ok
    check protocol tokens minted to the taker (balance before -> after): ui 13050, cli 13050, expected > 9800 (9800): ok
    check one lot of bad debt burned: ui 7802, cli 7802, expected 7802: ok
    check host frozen: ui yes, cli yes, expected yes: ok
  ✓ story: the whole demo runs in Basecamp, figures match ADR-051 and auction-cli
1 passed, 0 failed
```

Basecamp's log reads `Base data directory: /tmp/auctions-profile` and `Loading core dependencies for "auctions" : QList("lez_core", "auction_engine")`, with no `Standalone mode` line. Every module came from the profile.

Wall time: 1261 s for the whole command. The story itself took 329 s for 172 steps, of which 284 s were lane waits, the same as standalone. The rest went to evaluating the flakes, installing, and `demo.sh --setup-only`. The full `#app` and lgpm came from the Logos cache (the app was a 14.5 MiB download). The only local derivations were the `.lgx` packaging steps; neither Qt nor Basecamp was compiled.

## Known limits

- **"Publish anyway" never sends (R46).** In Dev tools, a price outside `[R/2, 2R]` turns the button into "Publish anyway". The engine's own check (`prepare_oracle_publish`) refuses that price before anything is signed, so the button always ends in a `refused` notice. The board implies the collateral would pause instead.
- **Settle debt after F1–F3 (R44).** Nothing is unbacked at that point, so the quote is 0 and the button is disabled. The CLI script sends a no-op `settle-debt` there; the story checks the 0 instead.
- **"> 9800" (R45).** The story's mint check compares the taker's protocol balance before the debt take (9800) with the balance after (13050). The amount minted is 3250.
- **The lez_core guard runs at install time only (design call 3).** `install-lgx.sh` refuses a profile without lez_core 0.4.1. A newer lez_core accepted later from Basecamp's Package Manager is not caught; that profile is unsupported.
- **Array-like lists (R43).** Lists cross the QtRO replica as array-likes, not JS arrays. `Format.list` converts every list the screens read: accounts, `items`, the discount schedule, and the positions in `quote_price`. Any new list field must go through it.
- **Doubles at the QtRO boundary.** An integer read from a reply arrives as a double. A request field that echoes one must coerce it (`Number(x) | 0`, as for `slippage_bps`).
- **The chain records no end times (R18, R30, R35, R36).** Live auctions sort by deepest discount and show when the maximum discount is reached. Settled auctions keep scan order.

## Recording

A scripted screen recording of the story (`STORY_PACE_MS=1500`, idle lane waits cut, title cards for F1–F5, the shortfall and the freeze) is at `C:\Users\user\Videos\rfp014-demo\basecamp.mp4`: 333.0 s, 9,021,687 bytes, SHA-256 `2b686a8621ba691df02aa1e24f0fb67094b0691c4ecf6191c362a649cba2389f`. It was made with `scripts/record/record-story.ps1` at aeb1320 in one unattended run (172/172 steps, every checked figure equal to `auction-cli`). The video is not committed.

Two limits of this take: the last seconds were trimmed by hand to drop a frame of the desktop behind the closing window, and the title cards and idle cuts run about 2 s late because that build of `cut-idle.py` treated the capture as starting with the story. Both are fixed in the committed scripts (59e7cfa: `--window-gone` and the real capture offset); a re-record gives exact timings.
