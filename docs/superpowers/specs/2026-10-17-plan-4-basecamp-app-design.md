# Plan 4: the Auctions app in Basecamp (design)

Date: 2026-10-17. Status: approved by the author 2026-10-18.

## Intent

A Logos reviewer should watch one recorded session in Basecamp in which every feature of the engine works end
to end, with no terminal on screen. The author asked for "everything, all the features from beginning to end".
This replaces the thin-GUI scope of the PoC spec (§7: Live view, bid and liquidate-by-id only; ADR-013 "the GUI
is not a showcase") for Plan 4: the GUI is now a showcase. The reason is evidence from the RFP process: a
reviewer criticised an earlier bid because "the demo video doesn't cover Basecamp app but only CLI"
(docs/research/research-5-traceability.md:49), and the RFP asks for a narrated demo showing the Basecamp app
(:350, :374) and a mini-app loadable in Basecamp (U2).

## Success

1. The Auctions app loads in a current Basecamp (standalone `--module` mode and as an installed module) and runs
   the whole demo story on a localnet: positions at risk, liquidation with keeper reward, the falling price, two
   partial bids (the second finishes the auction), claim leftover, a surplus auction, a price move that leaves an
   auction short, the shortfall settled through a debt auction, and the host freezing new debt.
2. A recorded video of that session (target 4 to 6 minutes after idle cuts), made by a scripted run so it can be
   re-recorded after changes.
3. Tests at every layer (FFI, core module, QML/e2e) and the existing gates stay green.

## Non-goals

- Deployment and setup (deploying guests, creating mock tokens, opening the three positions, binding the host)
  stay scripted (`scripts/lib/demo-scenario.sh` phases before the scenario). The video starts on a ready chain.
- The private path (PRIV1, M4): the UI shows "public transactions only" (spec §8) and the private-bid switch is
  disabled with that note.
- Catalog publishing and the release action (rfp #208): later.
- LEZ v0.3 / testnet 0.4: not this plan (docs/v03-port-plan.md).

## Anti-goals (a failure even if it works)

- Placeholder or legacy styling. The app must look Basecamp-native: Logos.Theme tokens and the palette and
  layout rules in docs/design/ui/ui-review.md (AppManagerView layout, orange means "now / act" only, black label on
  the primary button, Public Sans with tabular figures).
- A UI that freezes while a transaction confirms (15 s blocks on localnet).
- Logic duplicated in QML or C++: quotes, plans and encodings come from `auction_sdk` (ADR-040: one place).
- Numbers in the UI that differ from what the CLI would print for the same state.

## Constraints

- LEZ v0.2.4, SPEL 1ef0500f, the committed guests (unchanged ImageIDs; the `rfp014-poc` evidence stays valid).
- Wallet and signing through Basecamp's `lez_core` module pinned at `acf0cd50` (v0.4.1, built on LEZ v0.2.4,
  wallet home `LEE_WALLET_HOME_DIR`, the same `storage.json` our SDK reads). Not the v0.3 wallet module.
- Basecamp at upstream master `e09962f` or later (needs the standalone `--module` mode, `cf16f9d`, and #506).
  The local builds (01d3f59) are too old and must be rebuilt.
- Build inside WSL with Nix (single-user Nix 2.34.8, flakes, Logos cache configured); 8 GB RAM; C: has about
  30 GB free and holds the WSL disk.
- IPC calls between modules time out after 20 s by default, so no call may block for a confirmation.

## How it is built (decided)

Mirrors lez-programs `apps/amm` + `modules/amm` (main 64c0120), which is the proven pattern on this stack.

1. **`crates/auction_ffi`** (Rust, cdylib + rlib, cbindgen header committed, CI fails on header drift; spec
   §6.2). One `char* op(const char* json)` per operation plus `auction_ffi_free`; every call in `catch_unwind`
   returning `{"ok":true,"value":…}` or `{"ok":false,"error":{kind,message}}`; u128 as decimal strings. "Brain
   only": decode, quote, plan (accounts, signers, instruction words, program id) and the views the UI needs. No
   chain I/O and no signing in the FFI.
2. **Orchestration moves into the SDK.** The quote → min-out → needs-settlement → freshness → plan sequence and
   the touched-account report now live in `cli/src/tx_cmds.rs` and `Env`. They move into `auction_sdk` functions
   that both the CLI and the FFI call, so the GUI and the CLI cannot diverge. The CLI's output must not change
   (golden tests).
3. **`modules/auction_engine`** (core module, universal interface, C++ impl on `LogosModuleContext`, depends on
   `lez_core`, links `auction_ffi`; spec §6.3). Reads chain state through `lez_core`, asks the FFI for plans,
   submits with `lez_core.send_generic_public_transaction(accounts, signers, instruction, program_id)` and returns
   the transaction hash at once. Confirmation runs off the call path (a worker thread that emits a
   `txStatus(hash, Included|Dropped)` event, or a cheap `txStatus(hash)` the backend polls), keeping ADR-042's
   nonce rule and one in-flight transaction per signer.
4. **`apps/auctions`** (`ui_qml` app with a C++ `.rep` backend forwarding to the core module; QML uses
   `logos.module`, `logos.watch`, Logos.Theme and Logos.Controls). Every interactive item has an `objectName` for
   the e2e and the recording script.
5. All three live in this repository (`crates/`, `modules/`, `apps/`) with their own `flake.nix` files using
   `logos-module-builder` (`mkLogosModule`, `mkLogosQmlModule`), as lez-programs does.

## What the app shows (screens)

The approved designs are the base: docs/design/ui/auctions-main-v3.html (Live auctions),
auctions-positions-and-confirm-v4.html (At-risk, confirmation, post-submit lane) and
auctions-settled-system-dev-v5.html (Settled, System, Dev tools, account picker; approved 2026-10-18). The app
builds these boards faithfully, including their state strips and build maps; ui-review.md (v5) lists the
placeholders the real data replaces.

- **Header**: "Auctions", network and active account; an account picker (wallet labels: keeper, bidder, owner,
  caller, admin) because one person drives every role in the video; "Scanned to block N · Rescan".
- **At-risk positions** (v4): rows lowest CR first; detail with CR gauge, facts and the liquidation quote;
  "Liquidate & start auction" (keeper reward shown).
- **Live auctions** (v3): list ending soonest; KPI strip, discount-schedule chart with the now-pin, facts; bid
  card with oracle freshness, "You pay", min / max-to-finish, "You get", slippage; public-bid confirmation dialog;
  post-submit lane (Submitted → Including → Done, or Dropped → Re-quote). A bid that needs settlement is sent as
  `bid_with_settlement` automatically and the confirmation says so.
- **Settled**: finished auctions with outcome (raised vs target, shortfall); "Claim leftover" for the position
  owner where collateral is left.
- **System** (new tab, same layout): the accounting (buffer, unbacked debt, debt on auction), host status (frozen
  or not, oracle, min CR), surplus auction (start, take), debt auction (settle debt, start, take), check solvency.
  Each action shows its quote and goes through the same confirmation and lane.
- **Dev tools** (new tab, visible only when the deployment names a dev network, `localnet` or
  `testnet-legacy`, ADR-046): oracle price publish (the price move) and the admin's
  "disable protocol token" (the freeze trigger), clearly labelled as dev/admin actions.

## The recorded session

A script (logos-qt-mcp test framework, like amm's tests/swap.mjs) drives the app through the story while
Windows ffmpeg (gdigrab, the WSLg window) records. Idle waits for blocks are cut afterwards (as for the terminal
video). Title cards name each feature (F1 to F5, freeze). No voice-over is produced; the author may narrate.

## Testing

- FFI: unit tests per op, panic safety, envelope shape, header drift (spec §6.2, Plan 3a carry-over).
- Core module: LOGOS_CMOCK with `mockModule("lez_core")` (spec §9), every method called once.
- App: QML tests where cheap; an e2e (`nix run .#ui-tests` / logos-qt-mcp) that runs the story on a localnet.
- The SDK refactor keeps every CLI golden test byte for byte.

## Calls made for the builder (the designer's calls, not the author's)

1. **Chain reads stay in Rust.** `lez_core` 0.4.1 has no block-range call, and discovery is a block scan
   (ADR-043). So the FFI keeps the SDK's synchronous `Chain` over the sequencer RPC for every read and scan, with
   the sequencer address taken from `lez_core.get_sequencer_addr` (or the deployment file). Only signing and
   submission go through `lez_core`. Scans are paged (one page per call) so no call nears the 20 s IPC timeout.
   This narrows "no chain I/O in the FFI" above to "no signing or submission in the FFI".
2. **Fallback if `lez_core` `acf0cd50` will not load in the target Basecamp**: the core module signs and sends
   with the SDK's own keystore and `confirm::submit` (already proven on LEZ v0.2.4), and the app says so in its
   header. Basecamp stays at upstream master; the LEZ v0.2.4 pin and the evidence are never given up for this.
3. **"Installed module"** means a locally built `.lgx` installed through Basecamp's package manager into a clean
   profile, together with the `lez_core` 0.4.1 build it depends on. A profile that already has a newer catalog
   `lez_core` is not supported in the PoC. Our core module opens the wallet at the deployment's wallet home if
   `lez_core` has none open.
4. **Accounts**: the picker lists the wallet's labels; the scenario's roles are kept (the owner bids, as in
   demo-scenario.sh, so ADR-051's expected values hold). Holding accounts follow the label convention
   `<role>-stable` / `<role>-coll` and are shown, not hidden.
5. **Every step of the story is a UI action**, none off screen: `observe-feed` and check-solvency on System,
   the token transfer before take-debt and the oracle publish on Dev tools. A keeper price refresh, if the
   localnet oracle nears its age limit, is a Dev tools action too.
6. **The story e2e runs locally**, not as a hosted CI job (Actions minutes, Nix and Qt build cost). CI runs the
   FFI and core-module tests and the header-drift check.
7. **Networks**: the story and the video run on a localnet. The app works on any deployment file the CLI
   accepts (Dev tools only on `localnet` / `testnet-legacy`, ADR-046), but no story run on the legacy testnet is
   promised.

## The author's answers (2026-10-17)

- **Disk**: the author frees space on C: before the Basecamp rebuild; the build starts only after that, with the
  Logos binary cache, and stops if C: drops below about 10 GB free.
- **Time**: no fixed date. Build it in full; order the tasks so a recordable end-to-end story exists as early
  as possible, then fill in the rest.

## Left to the builder

- Event vs polling for confirmation, after checking the module host's threading (unverified).
- The exact op list of the FFI (driven by the screens above).
- Component choices within Logos.Controls; the System and Dev tools layouts within the master-detail pattern.
- How the recording script waits for blocks (poll the lane state).

## Risks

- `lez_core` loading inside Basecamp has never been verified (ADR-018): the first task proves it with a trivial
  read and a send on the localnet before anything else is built.
- Design-system version skew fails silently (spinner, then nothing): pin the design system the target Basecamp
  ships, or override the input.
- Disk on C: and 8 GB RAM for Nix builds of Basecamp and Qt.
- The 20 s IPC timeout for reads that scan many blocks (rescan): page the scan.
