=================== a38649167dacd931c.output
# Report: Logos RFP-012 and RFP-014, plus RFP program process (as of 2026-10-01)

Sources: everything was fetched from logos-co/rfp `master`: both RFPs, README, NEW_RFP.md, TERMS_AND_CONDITIONS.md (last updated 21 Jul 2026), `.github/ISSUE_TEMPLATE/proposal.yml` and `milestone.yml`, all other RFPs, git history and the issue list. Local copies are in `C:\Users\user\AppData\Local\Temp\claude\C--Users-user-Documents-basecamp-logos-basecamp\54f4bc00-cc9a-4338-a3f5-6dd50e23a06c\scratchpad\rfp\`.

## Headline facts

- **Neither RFP is open.** Both are `status: closed`, and the README's Submit column says "**Reopening soon**". **No reopening date is published anywhere**: not in the README, the commits, the issues or the RFP files. The repo has Discussions turned off, even though both RFPs say to "use Discussions" for questions.
- **When and how they closed.** Commit `ec40c6b` on 2026-08-26 ("temporarily close rfp-012 and rfp-014") flipped both statuses and removed both from the proposal-form dropdown. It gives no reason.
- **No dollar amount is published.** Each RFP has only a tier, and both are **L**. A `funding: $XXXXX` placeholder was removed from every RFP on 2026-06-18 (`3bbb0ad`), and the price column came out of the README on 2026-06-15. Applicants propose their own USD budget, split per milestone.
- **What the existing proposals ask for** (all still open and unanswered; none is labelled accepted):
  - RFP-014: #159 Web3spell Labs, $48k (the author asked "any updates?" on 08-26); #187 Antumbra, $55k, with a demo against the public LEZ testnet.
  - RFP-012: #160 Protofire, $122.1k; #163, $40k; #191 Horae, $112k.
- **A useful benchmark.** The accepted RFP-008 (XL) proposal, #141 "Argo", is $82k: a $67k fixed-price build over M0–M7 plus a $15k external audit. It was accepted on 2026-08-17 and its milestones (#170–177) are all still open.

## RFP-012: Curated Lending Vaults (tier L, about 12 weeks)

This is the Morpho Vaults V2 equivalent on LEZ, built on top of RFP-008's isolated markets. The RFP says the applying team should "ideally be the same team that delivered RFP-008".

**Functionality (F1–F10)**
1. Permissionless vault creation from a loan token, name and symbol. The creator becomes the curator.
2. Vault shares are transferable LEZ fungible tokens. The exchange rate rises monotonically, absent bad debt.
3. Deposit and withdraw, with withdrawals subject to the liquidity available across markets.
4. Allocation goes through adapters, using `allocate` and `deallocate`. There is "no ordered supply or withdraw queue".
5. ID-based absolute and relative caps per risk id (collateral, market or protocol), plus a `liquidityAdapter`.
6. **Curator:** enables and disables adapters, raises caps (timelocked), lowers caps instantly, sets the performance fee and can transfer the role. The curator "cannot allocate or deallocate capital directly."
7. **Allocator:** moves capital between idle and adapters, manages the `liquidityAdapter` and sets the maximum asset growth rate, all within caps. It cannot enable adapters, change caps or set fees.
8. **Performance fee:** paid in vault shares. Lazy accrual is fine as long as reads reflect accrued-but-unclaimed fees.
9. **Timelock:** applies to risk-increasing changes (cap increases, new adapters, fees, allocator additions, gate changes) and to sentinel appointment. Any address may execute once the delay expires. Quote: "The applicant should define a sensible minimum timelock for risk-increasing actions for LEZ; this minimum is set at vault creation and is immutable."
10. **Sentinel:** can deallocate, cut caps instantly and revoke pending actions. It "**cannot** introduce new risk".

**Usability**
- Extend RFP-008's core module, CLI and GUI. "no separate front-end is required."
- GUI per vault: APY, total deposits, per-market utilisation, caps and remaining capacity, fee.
- GUI per user: share balance, current value, accrued yield.
- The SDK does the private-account deshield (deposit token plus native gas) as one atomic action.
- The GUI shows the fee estimate and checks that the shielded balance covers everything.
- Vault shares must be usable as collateral in RFP-008 markets, if an oracle exists.

**Reliability**
1. Share value is non-decreasing, absent bad debt.
2. Caps are enforced atomically under concurrent deposits.
3. "Withdrawal always succeeds if the total available liquidity across the vault's enabled adapters covers the requested amount". `forceDeallocate` gives an in-kind exit with a curator-set penalty.
4. Bad debt is socialised and accounted "**on touch**", and tracked per market. The known Morpho asymmetry (a depositor who exits early leaves the loss to those who stay) must be documented and shown in the GUI's risk disclosures.

**Performance**
- Document the transaction size of deposit, withdraw and reallocate.
- "A vault with up to 10 allocated markets must not exceed LEZ compute limits on any single deposit or withdrawal operation."

**Supportability**
1. Deployed on LEZ devnet/testnet.
2. End-to-end tests run against a standalone sequencer in CI, and CI is green.
3. "Every hard requirement … has at least one corresponding test."
4. A README addendum.
5. Doc packets (logos-docs issue template) for each SDK feature.
6. A doc packet for the CLI additions.
7. Figma designs for the GUI additions.
8. An updated privacy-properties document covering the vault layer.
9. **Audit programme:** "must name at least one tier-1 audit firm … include the audit budget as a line item, and include the audit timeline ahead of any mainnet recommendation". Audits may be combined with RFP-008 if the same team does both.

**Privacy**
- Both public and private paths are supported. On the private path the reshield step cannot be skipped.
- A pre-confirmation summary shows what is visible on-chain and what stays private.
- The ephemeral account A is never reused.

**Soft requirements:** an APY history (on-chain or indexed), and formal verification of three invariants: share value only drops through realised bad debt, caps are never exceeded, and the timelock can't be shortened.

**Out of scope:** CDP stablecoin, governance token, bridging, leveraged looping, and anything RFP-008 already delivered.

**Dependencies**
- Hard: RFP-008 and LP-0013.
- Quote: "development is blocked until … RFP-008 is live on LEZ devnet/testnet."

## RFP-014: Liquidation & Auction Engine (tier L, 10–12 weeks)

This is a reusable RAI/Reflexer-style (GEB) program, deployed as one instance per CDP product.

**Functionality**
1. Permissionless liquidation, with an incentive for the liquidator.
2. Increasing-discount auctions with a configurable schedule. They end early once the target is raised.
3. Partial lots can be bought.
4. **Debt auction:** "must be fully implemented and tested (using a mock token), but parameterized by a configurable token address". Until a protocol token is configured, the surplus buffer absorbs losses. If bad debt exceeds the buffer, new debt is frozen through RFP-002.
5. **Surplus auction:** surplus above a threshold is sold for the protocol token, which is burned. It uses the same mock-token and configurable-address approach.
6. "strict state transitions with no reversals. All state changes are atomic and verifiable."
7. A well-defined interface to the host CDP.

**Usability**
1. Build with the **SPEL framework**, which generates the IDL and client.
2. A Logos mini-app GUI that loads in Basecamp from a git repo.
3. A CLI covering the essential operations.
4. A list of undercollateralised positions with estimated incentives, plus a trigger.
5. An auction view showing collateral, amount, current discount, raised vs target and time left, with bid previews.
6. An SDK that supports deshield-interact-reshield.

**Reliability**
- Non-reentrant.
- Uses "time-weighted or confidence-adjusted prices, not raw spot". Stale feeds pause that collateral type only.
- Auctions are isolated and can run concurrently.
- Required invariants: "seized collateral equals collateral in auction; debt shortfalls are properly accounted; surplus and debt auctions maintain system balance."

**Performance:** liquidations and bids each complete in one block, and many concurrent auctions stay within compute limits.

**Supportability**
- Deployed on devnet/testnet.
- End-to-end CI against a standalone sequencer **with `RISC0_DEV_MODE=0`**, and CI green.
- At least one test per hard requirement.
- A README covering deployment, addresses, CLI and mini-app.

**Privacy:** the same three rules as RFP-012 (no skippable reshield, the reshield target is validated as private and rejected with an explicit error if not, fresh ephemeral accounts). Every operation's deshield moves the token and the gas atomically. All auction state is public.

**Soft requirements:** multi-oracle fallback, formal verification of the invariants, and a liquidator-bot SDK.

**Out of scope:** flash-loan liquidations, aggregators and batching, insurance or socialised loss, multi-collateral atomic liquidations.

**Hard blockers**
- A price feed: RFP-020 (RFP-019 TWAP is the alternative).
- A CDP host: RFP-013 "or equivalent".
- LP-0013.
- RFP-001 and RFP-002.
- LP-0015 and LP-0012 are listed as already resolved.

## Process (README, Terms and templates)

- **How to submit:** file a GitHub issue using `proposal.yml`, and only an RFP listed in its dropdown can be chosen. Reviews happen weekly, with a reply "typically … within 14 days". The limit is one proposal per RFP per team.
- **Required fields:**
  - RFP ID, project name, team, contact
  - Team members with links, role and status
  - Summary and technical approach
  - Milestones, each with deliverables, **USD payout** and duration, plus "an **Audit milestone** if your project handles user funds"
  - Total budget, which must equal the sum of the milestones
  - Experience (URLs preferred) and a post-delivery plan
  - Checkboxes agreeing to the Terms, the MIT+Apache-2.0 licence and milestone-based delivery
- **Evaluation:** only submissions that meet every mandatory requirement are considered. Criteria are at Logos' "sole discretion" and may include "technical feasibility, ecosystem impact, budget availability, team experience, compliance considerations, reputational risk". Logos may negotiate milestones and budget, gives no reasons, and its decisions are final.
- **Award:** being selected is non-binding. Nothing is owed until a Grant Agreement is signed, and KYC or sanctions information may be requested.
- **Payment:** after acceptance, each milestone becomes a sub-issue. Payout is released after an engineering review that checks scope, the licence, green CI, docs and "approved for payout".
- **Eligibility:** you must have legal capacity, not be a sanctioned person, submit original work and comply with the law. Submissions are public and Logos receives a perpetual licence. The contracting party is Logos Collective Association in Zug.

## All RFPs

| ID | Title | Tier | Status | Who has it |
|---|---|---|---|---|
| 001 | Admin authority library | XS | closed | Delivered (milestones approved) |
| 002 | Freeze authority library | XS | closed | Delivered |
| 003 | Atomic swaps BTC/XMR/ZEC | XL | closed | Accepted, in progress |
| 004 | Privacy-preserving AMM DEX | XL | closed | Accepted |
| 005 | Multisig and coordination | L | **open only one, deadline 16/10/26** | — |
| 008 | Morpho-Blue lending | XL | closed | Argo accepted, M0–M7 open |
| 012 | Curated vaults | L | closed, reopening soon | — |
| 013 | Reflexive stablecoin (RAI-style CDP) | XL | closed | Only proposal #96 closed "completed" with no `accepted` label and no milestone issues |
| 014 | Liquidation engine | L | closed, reopening soon | — |
| 015 | Bonding-curve launchpad | L | closed | Accepted |
| 016 | LBP launchpad | XL | closed | Accepted |
| 017 | Token vesting | L | closed 09-13 | — |
| 019 | On-chain TWAP oracle | L | closed | — |
| 020 | RedStone oracle adaptor | M | closed | Kanon accepted, milestones M0–M5 |

**How they relate:**
- 012 depends on 008, which in turn needs 020 for prices.
- 014 depends on 013, 020, 001, 002 and LP-0013.
- 008 is listed as a related resource for 014, since a future lending host could plug into the liquidation engine.

## Timing signals

- Both RFPs were opened around May (RFP-014 was set open on 2026-05-14) and temporarily closed on 2026-08-26.
- The recent commit activity is almost all RFP-005. Nothing touches 012 or 014 after 08-26.
- **Likely reasons for the pause (my inference, not stated anywhere):**
  - For 012: RFP-008 is only at the milestone stage, not "live on devnet/testnet".
  - For 014: there is no clearly awarded RFP-013 CDP host.

## Ambiguities to flag

1. **"Reopening soon" has no date** and no stated reason or trigger.
2. **No funding range is published,** even though the Terms mention "indicative funding ranges". How tiers map to dollars is undocumented.
3. **RFP-014's dependency text is stale.** It says LP-0013 is "currently **open**", but lambda-prize lists LP-0013 as Closed, with a solution.
4. **RFP-012 contradicts itself and is stale:**
   - It says it is "open for proposal submission" while its status is closed.
   - It says the oracle and LP-0013 "will therefore be resolved before this RFP opens".
   - Its Resources section still has TODOs (oracle guide, SPEL docs).
   - It asks for doc packets, but RFP-005 recently replaced doc packets with "in-repo documentation". RFP-012 may be revised the same way.
5. **RFP-014 GUI wording differs from RFP-012:**
   - RFP-014 says "mini-app GUI loadable via git repo".
   - RFP-012 extends RFP-008's QML+C++ GUI and core module; a 2026-05-22 commit renamed "mini-app" to "GUI" there.
   - RFP-014 never got that update.
6. **RFP-013's real status is unclear.** Proposal #96 was closed as "completed" with no `accepted` label, yet RFP-014 relies on it "or equivalent" CDP host.
7. **RFP-012 leaves the minimum timelock to the applicant** to define.
8. **Audit requirement asymmetry.** RFP-014 has no explicit audit requirement, while RFP-012 requires a tier-1 audit firm named in the proposal. The proposal form still asks for an audit milestone whenever user funds are involved, which would cover RFP-014.
9. **Existing proposals are in limbo.** The open proposals for 012 and 014 have had no response since the closure, so it's unclear whether they carry over when the RFPs reopen.
=================== a4e1f43adc3861634.output
## Report: building and shipping a DeFi program on LEZ with a Basecamp UI (as of 2026-10-01)

### 1. Bottom line
You can build this today, and the place to start is `logos-blockchain/lez-programs`. It is Logos' own reference monorepo and already contains the full stack you'd need for a submission:
- Rust programs written with SPEL (RISC Zero guests).
- A Rust FFI crate for each program.
- A C++ "universal" core module that wraps that FFI and does chain I/O through the `lez_core` wallet module.
- A QML `ui_qml` Basecamp app.
- Nix flakes that produce `.lgx` packages.

It also already has a **stablecoin (RFP-013) CDP program**, a **TWAP oracle**, an AMM, token, ATA and faucet programs. That means an RFP-014 liquidation engine has a real host CDP and a real oracle to build against. RFP-012 vaults do not have their base yet: RFP-008 lending (team "Argo") is still in milestones.

### 2. On-chain side (LEZ)

**Repos and URLs**
- Runtime: https://github.com/logos-blockchain/logos-execution-zone (LEE and LEZ, formerly nssa/lssa). Crates are `lee_core` and `lee`.
  - Release tags: `v0.3.0` (2026-09-30), `v0.3.0-rc1`, `lez-classic`, `v0.2.4`.
  - Built-in programs live in `lez/programs/`: `token`, `amm`, `associated_token_account`, `clock`, `fee`, `bridge`, `bridge_lock`, `wrapped_token`, `cross_zone_*`, `program_loader`, `sequencer_stake`.
- Programs and apps: https://github.com/logos-blockchain/lez-programs. It has `programs/{token,amm,ata,stablecoin,twap_oracle,token_mint_authority,integration_tests,benchmark}`, `modules/{amm,stablecoin,token}`, `apps/{amm,token,shared/wallet}` and `artifacts/*-idl.json`. Two documents are worth reading first:
  - `docs/stablecoin/README.md`, a 1,200-line RAI-style design written against RFP-013.
  - `docs/testnet-runbook.md`.
- Framework: https://github.com/logos-co/spel (Smart Program Execution Layer, Anchor-like). Latest is v0.7.0 / v0.7.0-rc.2.
  - `#[lez_program]`, `#[instruction]` and `#[account(init|mut|signer|pda = literal/account/arg)]`.
  - IDL generation, an auto-generated CLI (`spel`), and `spel inspect` / `spel pda`.
  - `spel-client-gen --target logos-module` generates a typed Rust client, C FFI and a Logos module skeleton from the IDL.
  - Extension libraries: RFP-001 `admin-authority` and RFP-002 `freeze-authority` were delivered as SPEL extensions. Docs are in `logos-co/logos-docs` under `docs/lez/extensions/`.
- Scaffolding: https://github.com/logos-co/scaffold (`cargo install logos-scaffold`, then `lgs new my-app --template lez-framework`).
  - `lgs run` builds, generates the IDL, starts a localnet sequencer, funds a wallet and deploys.
  - `lgs test-node` gives throwaway sequencers for CI.
  - `lgs basecamp` launches your modules in clean Basecamp profiles.
- Docs: https://docs.logos.co (source is `logos-co/logos-docs`). See `docs/lez/programs/write-and-deploy-lez-program-with-scaffold.md` and `docs/build-an-app/tutorials/create-and-use-an-amm-liquidity-pool-...md`. AI skills are in `logos-co/logos-ai-skills` and `spel/skills`.
- Smaller SPEL examples to copy from: `logos-co/lez-multisig`, `logos-co/lez-payment-streams`, `logos-co/whisper-wall`, `logos-co/logos-lez-rln`.

**Programming model**
- Programs are stateless RISC Zero guests (`riscv32im-risc0-zkvm-elf`, risc0-zkvm `=3.0.5`). All state lives in accounts that are passed in explicitly.
- Public transactions run on the node without a proof. Private ones are run locally by the user, who generates a STARK proof. The same program binary serves both, so privacy is handled by the protocol, not by your code.
- **PDAs:** seeds always start with the program_id. Private PDAs also need npk, vpk and an identifier (a breaking change in v0.7).
- **Cross-program calls:** "chained calls", which are tail-call continuations (LP-0015, delivered). The AMM moves tokens through chained calls to the token program. Custody and authority work under the LP-0013 rules.
- **Deploy:** `cargo risczero build` (in Docker), then `wallet deploy-program <x.bin>`. The ProgramId is the ImageID, so every rebuild changes the ProgramId and every PDA derived from it.
- **Clock / timestamps: they exist.** The sequencer writes the system accounts `CLOCK_01`, `CLOCK_10` and `CLOCK_50` (see `lez/programs/clock/core/src/lib.rs`), which hold `ClockAccountData { block_id, timestamp (ms) }`. There is no time on the program context. You pass the clock in as a trailing read-only account, assert its ID, and echo it back unchanged; `twap_oracle`, `amm` and `stablecoin` all do this (stablecoin doc §5.4).
- **Events** (LP-0012) arrived in v0.3.0 with indexer RPC and FFI; liquidator bots can use them. Fees also arrived in v0.3.0 (the wallet now takes a payer and a gas limit).

**Existing DeFi pieces on testnet** (`DEPLOYMENTS.md`, sequencer https://testnet.lez.logos.co/, explorer https://explorer.testnet.lez.logos.co/)
- Deployed: token, amm (TKA/TKB pool), twap_oracle (24h window over the pool) and the faucet `token_mint_authority`.
- The stablecoin program is in the repo; its scaffolded instructions are `open_position`, `deposit/withdraw_collateral`, `generate/repay_debt`, `accrue_stability_fee`, `update_redemption_rate`, `freeze`, `close_position`. It is **not** listed as deployed, and it consumes a generic `OraclePriceAccount`.
- Explicitly out of scope for stablecoin, and handed to RFP-014: liquidation, surplus auctions and debt auctions. Doc §13 suggests either a `liquidate_position` instruction gated by a collateralisation check, or an external liquidator program.

### 3. Basecamp side (local findings)
`C:\Users\user\Documents\basecamp\` contains only `logos-basecamp`; there are no sibling checkouts. Relevant local files:
- `docs\spec.md` and `docs\project.md` (component model, LGX install flow, `ui_qml` sandbox)
- `README.md` (lines 25–35 on building `.lgx` packages; lines 111–121 on the LEZ Wallet app)
- `src\PluginLoader.*` (`loadQmlView`)
- `src\Basecamp\...\QmlSandbox`
- `tests\sandbox\evil_app\metadata.json`, a minimal `ui_qml` manifest
- `src\AppsModel.*` (catalog by `repositoryUrl`)
- `flake.nix` (inputs: logos-cpp-sdk, logos-module, logos-view-module-runtime, nix-bundle-lgx, …)

Basecamp has **no DeFi or LEZ code of its own**. LEZ apps are external `.lgx` packages.

**Two kinds of component**
1. **Core module** (`type: "core"`, `interface: "universal"`). It runs in its own `logos_host` process. You write a Qt-free C++ class deriving from `LogosModuleContext`; its public methods are the API, and the `.lidl` contract and Qt glue are generated by `logos-co/logos-module-builder` (`mkLogosModule`). It reaches other modules through `modules().<name>`.
2. **UI app** (`type: "ui_qml"`, `view: qml/Main.qml`, optional C++ backend via `main`). It is built with `mkLogosQmlModule`. The backend runs in a separate ui-host process connected by Qt Remote Objects (`.rep` file), and QML reaches modules through `logos.module(...)`.
   - Pure-QML apps are sandboxed: no network, no access outside their own directory, and no native plugins.
   - Packaging: `nix build .#lgx` / `.#lgx-portable` / `.#install`, and `nix run .` opens the app in logos-standalone-app.
   - Distribution is through `logos-modules` / `logos-modules-release-action` catalogs, or a local "Install LGX Package".

**The LEZ wallet module you call**
`logos-blockchain/logos-execution-zone-module` provides the `lez_core` module (v0.5.0, header `src/lez_core_module.h`). Its API includes:
- `list_accounts`, `create_account_public/private`, `get_account_public/private`, `get_balance`
- `transfer_public/shielded/deshielded/private`
- `send_generic_public_transaction(account_ids, signing_reqs, instruction_bytes, program_id_hex, payer, shard_ids)`
- `send_generic_private_transaction(..., program_elf, deps, ...)`
- `send_program_deployment_transaction`, `poll_transaction_status`, `sync_to_block`, `get_current_block_height`

The wallet UI is `logos-blockchain/logos-execution-zone-wallet-ui`. Other related repos: `lez-explorer-ui`, `lez-indexer-module`, `logos-amm-module`, `logos-amm-ui-module`, `logos-token-module`, `logos-token-ui-module`.

**Data flow to copy (from lez-programs)**
- `programs/amm` (SPEL guest) feeds `modules/amm/ffi` (a Rust `amm_ffi` crate with a JSON-in/JSON-out C ABI that does PDA derivation, account decoding, quotes and instruction encoding).
- That feeds `modules/amm/src/amm_module_impl.h` (a universal core module `amm_module`, depending on `lez_core`, which submits through `send_generic_public_transaction`).
- That feeds `apps/amm` (`ui_qml`, `metadata.json` dependencies `["lez_core","amm_module"]`).
- The app reuses the shared `apps/shared/wallet` QML and C++ (`WalletControl`, `TransactionConfirmationDialog`, `LogosWalletProvider`). Its tests are QML `tst_*.qml` files plus `.mjs` UI tests driven through logos-qt-mcp.
- `modules/stablecoin` follows the same pattern and has C++ unit tests with an FFI mock, so it is the closest template for a liquidation module.

### 4. Recommended stack and repo layout
Use one repo that mirrors lez-programs:
```
my-defi/
  programs/<engine>/{core,methods/guest/src/bin/<engine>.rs,src/*.rs,examples/*_pdas.rs}
  programs/integration_tests/   # RISC0_DEV_MODE=1 plus a real-proof CI job (RFP-014 requires RISC0_DEV_MODE=0)
  modules/<engine>/{ffi (Rust cdylib + cbindgen), src/<engine>_module_impl.{h,cpp}, metadata.json, flake.nix, tests/}
  apps/<engine>/{qml/..., src/*Backend.{h,cpp,rep}, metadata.json, flake.nix, tests/}
  apps/shared/wallet  (vendor or path-depend from lez-programs)
  artifacts/*-idl.json, DEPLOYMENTS.md, flake.nix (root exports the ffi crates + apps)
```
- Toolchain: Rust (pinned), cargo-risczero/rzup, SPEL (`spel-framework` tag), logos-scaffold for the local loop, and Nix with logos-module-builder for modules and the UI.
- Compose with the existing programs through chained calls: token and ATA for custody, `twap_oracle` (or later the RedStone adaptor) for prices, the clock account for time, and admin-authority / freeze-authority SPEL extensions for the RFP-001 and RFP-002 requirements.
- Add the deshield→interact→reshield flow with a fresh ephemeral account in the module/SDK layer. Both RFPs make this mandatory.

### 5. Gaps and blockers
- **Version skew is the biggest practical risk.** LEZ v0.3.0 (tagged 2026-09-30) is heavily breaking: borsh I/O codec, sharded program state, fees, `program_owner` changed to AccountId, built-ins addressed by name, and events. Meanwhile:
  - SPEL v0.7.0 targets LEZ **v0.2.4**.
  - lez-programs pins `lee_core` at **v0.2.4**.
  - `DEPLOYMENTS.md` still says "Testnet (v0.2.4)".
  - The AMM flakes pin `lez_core` at an "0.4.1-interim" rev built on the v0.2.4 wallet FFI.
  - The runbook needs a forked SPEL branch (`0x-r4bbit/spel@refactor/lez-v020-compat`).

  Before writing code, confirm which version the public testnet actually runs and whether a SPEL release for v0.3 exists.
- **Oracle:** the TWAP oracle exists and is deployed but is fed only by the AMM's test pool. The RedStone external-price adaptor (RFP-020) is in progress: M0 approved, M1–M5 open. RFP-014's hard requirements (F1, R2: TWAP or confidence-adjusted prices, pause when stale) can be met with TWAP. The doc says TWAP is an acceptable alternative.
- **Timestamps:** solved by the clock accounts; this is not a blocker.
- **Compute and throughput:** per-transaction compute is thin and there is roughly one private transaction per block. Concurrent auctions and "liquidation in one block" need benchmarking (there is a `programs/benchmark` harness to start from).
- **Token authorities (LP-0013):** the RFP-014 text says "open", but the lambda-prize file is marked CLOSED and v0.3.0 added "authorized balance decrease". This needs confirming.
- **Host protocols:** the stablecoin program is not deployed. RFP-008 lending is only at milestone stage (Argo, rfp issues #141 and #170–177). Vaults (RFP-012) are formally blocked until RFP-008 is live on testnet.
- `logos-module-builder` requires `interface: "universal"` (or `cdylib`) for core modules; hand-written plugins are refused.

### 6. Feasibility by RFP
Both RFPs show "status: closed / Reopening soon" in the README of https://github.com/logos-co/rfp.

**RFP-014 Liquidation & Auction Engine (L, 10–12 weeks): high feasibility now.**
- Host CDP: the lez-programs stablecoin, whose design doc already defines the liquidation hook.
- Oracle: twap_oracle. Time: clock. Admin/freeze: SPEL extensions. Bots: events.
- The work is the program (increasing-discount collateral auctions, partial lots, surplus and debt auctions with a mock token), a module, a UI and an SDK.
- Competing proposals already filed: Antumbra (#187) and Web3spell Labs (#159). Differentiate on shipped LEZ code and real-proof CI.

**RFP-012 Curated Lending Vaults (L, 12 weeks after RFP-008 is live): low feasibility today.**
- It must extend RFP-008's core module and GUI. The RFP prefers the RFP-008 team, and Argo's proposal explicitly claims the vault follow-on.
- Competing proposals: Horae (#191), Protofire (#160), Yearn-model (#163).
- You could only build a standalone ERC-4626-style vault over the AMM or a mock market as a demo.

**Recommendation:** go for RFP-014. Fork the lez-programs layout, put the engine next to the stablecoin program, and copy `modules/stablecoin`, `apps/amm` and `apps/shared/wallet` for the module and UI.

=================== a5dc0296719ab68ad.output
# Logos RFP program: past submissions, winners, and what wins (research for RFP-012 / RFP-014)

The data was pulled on 2026-10-01 from `gh` (all 117 issues and their comments in logos-co/rfp, plus PRs), from logos-co/ecosystem, logos-blockchain/lez-programs, the Logos forum API and the grantee repos. Raw dumps are in `C:/Users/user/AppData/Local/Temp/claude/C--Users-user-Documents-basecamp-logos-basecamp/54f4bc00-cc9a-4338-a3f5-6dd50e23a06c/scratchpad/rfp/` (`issues/*.json`, `comments.txt`, `body_141.md`, `body_117.md`, `body_112.md`, `body_118.md`, `body_46.md`, plus the RFP-012, RFP-014, RFP-008 and template specs).

## 0. Facts that should shape the strategy

- **RFP-012 and RFP-014 are both closed right now.** PR #193 (2026-08-26) "temporarily closed" them, and the README says "Reopening soon". The proposal form no longer lists them. Proposals already filed stay open with no decision.
- **RFP-012 effectively has an incumbent.**
  - The spec says "Ideally the same team that delivered RFP-008".
  - RFP-012 cannot start development until RFP-008 is live on testnet.
  - RFP-008 went to **Kolkheti Labs, "Argo"** (#141, $82k). Their proposal explicitly pitches the RFP-012 vault layer as their follow-on: their architect "built a vault on Morpho Blue with direct guidance from the Morpho team", and their advisor brings a vault primitive.
  - A non-Kolkheti RFP-012 bid has to beat that, or partner with them.
- **RFP-014 depends on things that are not ready:**
  - RFP-013 (the stablecoin/CDP host) is closed. The ecosystem tracking issue (logos-co/ecosystem #210) calls it "Externalized — built by an external team". A Logos-written draft design of that host already exists as `stablecoin` in logos-blockchain/lez-programs (`docs/stablecoin/README.md`, RAI-style PI controller).
  - RFP-020 (oracle) is in delivery with Equilibrium, "Kanon".
  - LP-0013 (token authorities) is still open.
  - Ecosystem #210 also says Logos wants **one studio to build both the Stablecoin UI RFP and the RFP-014 UI RFP**. That points to a later UI RFP worth bidding on too.
- **Current competition, all open with no decision:**
  - RFP-012:
    - Protofire, $122,100 (#160)
    - Humea "Horae", $112,000 (#191), a 50 KB proposal with a pinned-commit primitive inventory
    - 0xConsole "Yearn-model", $40,000 (#163), independent, no repos
  - RFP-014:
    - Web3spell Labs, $48,000 (#159). Their repo `Web3Spell/Liquidation-Auction-Engine` contains only a README.
    - **Antumbra (edenbd1), $55,000 (#187).** This is the strong one: the auction program is already deployed on the public LEZ testnet, with a narrated demo video, a `verify-onchain.sh` reproducer, differential tests, a committed IDL and zkVM cycle counts (https://github.com/edenbd1/antumbra-auction).
- **Your own history in the repo:** the user (paschal533) filed #30, an RFP-004 DEX proposal. It was labelled under-review and closed on 2026-05-31 with no comment. RFP-004 then went to the internal IFT team.

## 1. Submissions and their status

| RFP | Tier | Accepted (winner) | Rejected or not selected | Still open or pending |
|---|---|---|---|---|
| 001 Admin authority lib | XS | #46 mmlado, $3,500 (3 milestones of $875 / $1,750 / $875); delivered, M3 closed 2026-09-22 | #26, #29, #50, #53, #55, #58/#59, #72; #16 and #44 were off-topic ideas | – |
| 002 Freeze authority lib | XS | #47 mmlado, delivered | #27 (declined), #31, #56, #60, #73 | – |
| 003 Atomic swaps | XL | #112 Gateway.fm, $296k all-in ($225k build + $71k itemised audit); 7 milestones; M1 accepted | #51, #69, #76, #83 ("insufficient details compared to others"); #61 superseded by #112 | – |
| 004 Privacy DEX | XL | #40 internal IFT smart-contracts team, no budget figure | #43, #63, #80; #30 (yours) closed | – |
| 008 Lending (Morpho Blue) | XL | #141 Kolkheti Labs "Argo", $82k ($67k build + $15k audit); 8 milestones over 16 weeks; M0 submitted | #68, #71, #139, #142 (Ankr), #143 (Infinity Base), #144 (Protofire), #145, #152, #162, #166: ten losers | – |
| 012 Curated vaults | L | none yet | – | #160 Protofire $122k, #163 $40k, #191 Humea $112k |
| 013 Reflexive stablecoin | XL | "externalized" | #96 closed | – |
| 014 Liquidation engine | L | none yet | – | #159 Web3spell $48k, #187 Antumbra $55k |
| 015 / 016 Launchpads | L / XL | #118 and #119 bleu, combined $55k after negotiation, 5 milestones over about 19 weeks | #179, #180, #183 (duplicates) | #79, #85, #88, #164, #167, #181, #184, #185, #190, #192, #196 |
| 017 Vesting | L | pending | #178 duplicate | about 12 open (#84, #161, #182, #186, #188, #189, #195, #197, #198, #201, …) |
| 019 TWAP oracle | L | RFP closed with no award visible | #70, #86 closed | – |
| 020 RedStone oracle | M | #117 Equilibrium "Kanon"; budget "to be agreed", operates feeds under an SLA to March 2028 | #75, #87, #99, #100, #105, #110, #111, #114, #115, #116 | – |

Funnel: RFP-008 drew 11 bids for 1 win and RFP-020 drew 11 for 1. Winners are a mix of established firms (Equilibrium, Gateway.fm, bleu) and small teams with proven delivery on LEZ (Kolkheti, mmlado).

## 2. Accepted proposals in detail

**RFP-008 Argo (#141), the most relevant precedent at about 57 KB.** The proposal follows the issue form exactly:

- **Team table:** role, full-time or advisor status, links, and a "why this team fits" paragraph that maps each person to RFP wording. Their wedge was "the only builder in this RFP with Logos-selected work shipped on live LEZ": Lambda Prize LP-0016 was merged as the accepted reference, and LP-0002 is a RISC0 multisig with real (non-dev-mode) STARK proofs on testnet. The point is that every claim is a clickable, public artifact.
- **Technical Approach, 18 numbered sections:**
  - stack (Rust + SPEL + RISC0 guest `riscv32im-risc0-zkvm-elf`) and a repo layout tree
  - singleton/state sharding
  - virtual-share inflation resistance with exact constants
  - LP-0013 custody under the tail-call/ChainedCall model
  - in-frame IRM, liquidation compute budget, flash loans via continuation
  - bounded admin reusing the RFP-001 library; oracle-agnostic adapter
  - privacy (deshield → interact → reshield: single-use ephemeral accounts, a reshield step that cannot be skipped, a check that the reshield target is shielded, fees funded inside the deshield)
  - a two-module app: a Rust core module with no GUI, plus a QML/C++ Basecamp GUI, a CLI, a reference liquidator and a risk monitor
  - Kani + proptest invariants
  - an audit programme naming Zellic or Trail of Bits, with OpenZeppelin or Cantina as alternates, plus a second audit
  - a challenges/mitigations table
  - a requirement traceability table mapping every F/U/R/P/S item to a section, a milestone/test and a status
- **Milestones:** M0 "foundations and de-risking spikes" ($5k, 2 weeks) with explicit go/no-go spikes (ChainedCall atomicity, LP-0013 custody, LP-0012 events, the per-transaction compute budget, fee funding inside the deshield). Then M1–M7 at $7k–$12k each. It states the sequential fallback timeline (18.5 weeks) against the committed 16.
- **Post-delivery:** 6 months of support included, no revenue share.
- **Repo:** https://github.com/Kolkheti-Labs/argo. Main is only a skeleton; the M0 work sits in PR #1 (tag `m0-submit`) with `argo_core`, `irm_core`, `programs/argo_lending`, `idl/*.json`, `harness/` (localnet, a clean-host reproduction script, IDL generation), `spikes/S-A…S-E`, `evidence/{localnet,testnet}` and `docs/m0/{acceptance,verdicts,verification-log}.md`.
- **CI:** fmt, clippy, `--locked`, both riscv32 guests, an IDL drift check, and e2e against a standalone sequencer built from LEZ v0.2.4 source.
- **Pins:** LEZ v0.2.4, a SPEL rev, risc0 3.0.5, Rust 1.94.

**RFP-020 Kanon (Equilibrium, #117, about 51 KB).**
- Includes a "dependencies that moved since the RFP was written" table. An example: the RFP-001 admin library was still contested, so they put the admin check behind their own shim.
- Treats cost measurement as the primary deliverable and opens with an M0 cost-only spike (#146, already approved).
- Has a requirement traceability table and a full Servicing/SLA section (99.97% measured uptime elsewhere; acknowledge within 1 hour, mitigate within 4).
- A reviewer praised its "attention-to-detail".
- Repo: https://github.com/equilibriumco/logos-kanon, with `m0/cost-baseline`, `m0/lez-probe` and `M0-report.pdf`.

**RFP-003 Gateway (#112).**
- Engineer-week allocation per person per milestone and a blended-rate pricing method.
- The auditor fee is itemised separately ($71k), with an offer for Logos to contract the auditor directly. Lists the out-of-scope items they noticed and a dependency graph showing which milestones overlap.
- Repo: https://github.com/gateway-fm/lez-atomic-swaps. Rust workspace (`crates/*` SDKs, nodes, adapters), `apps/basecamp/{maker,taker}` built with Nix flakes, and `compat/` reproducers per LEZ version.
- CI workflows: ci, node-e2e, public-readiness, release-images, upstream-compatibility.
- Milestone submissions read like evidence packs: a MILESTONES.md mapping, EVIDENCE.md, REPRODUCE.md, a release tag per submission, mp4 walkthroughs inside Basecamp, and an HTML/PDF deck.
- Modules are published to a catalog repo (`mandrigin/logos-modules-release-base`) that installs through Basecamp's package manager.

**RFP-015/016 bleu (#118, #119).**
- About 19 KB each, with a requirement-coverage section.
- Built a PoC repo during review (https://github.com/bleu/lez-bonding-curve-launchpad: `crates/curve-math`, `pool`, `private-flow-core`, `methods/guest`, `idl/`, `cli/`, `verify/e2e.sh`, `docs/adr`). They updated it when the spec changed (PR #204).
- Accepted after negotiating the price down to $55k combined.
- Milestones: M0 product/technical design (PRDs, acceptance criteria) → M1 programs → M2 SDK/CLI/IDL → M3 Basecamp QML mini-apps → M4 hardening, CI, CU measurement.

**RFP-001/002 mmlado (#46, #47).** Small XS bids of $3,500 each in three milestones: API design plus a draft PR to logos-co/spel, then implementation, then tests, docs and CI. The deliverable was upstream SPEL macros (`#[require_admin]`).

## 3. Reviewer feedback patterns

The reviewers are hackyguru (program lead and contracting), jzaki (RFP-003/020), fryorcraken (spec author) and danisharora099 (milestone reviewer).

**What they asked for or criticised:**
- **Full requirement coverage.** fryorcraken's review of #40 is the clearest rubric. Add a statement that each hard requirement is in scope or descoped with a reason. Items flagged as missing:
  - IDL via SPEL
  - SDK doc packet
  - Figma designs
  - CLI with README usage steps
  - per-operation transaction-size documentation
  - single-transaction operations
  - CI green with e2e against a standalone sequencer
  - consistency under concurrent operations
  - actionable error messages
  - an atomic token + gas deshield
  - a private path for every operation
  - a pre-confirmation public-vs-private summary
  - "account A never reused" stated explicitly
- **Scope discipline.** Drop features outside the RFP ("Remove RecoverSurplus"). Do not upsell above-spec extras.
- **UI must be a QML UI module with a C++ backend module**, not a generic Qt plugin. Use logos-design-system rather than paying for new branding.
- **Fresh codebase**, not an extension of a Logos sample app (#51).
- **Realistic budget.** "$10k for 3 engineers plus an audit is not a usable number." Say clearly whether the audit is included (#116). Divide work into milestones (#115).
- **Define your terms** (jzaki asking "what is CU?").
- **Verifiable track record.** #114 was challenged for a site with dead links, a GitHub with no history, and an empty X account.
- Use `F1`, `U2`-style IDs, not `#1` (GitHub turns those into issue links).
- Absorb spec updates as they happen (the RFP-020 SLA, RFP-015/016 PR #204).

**Rejection wording, mostly boilerplate:** "went ahead with another proposal", "insufficient details in comparison to other applications", "address all criteria", "fit our goals/timelines/budget better", "technical merit, commercials, time fit and previous experiences". Detailed feedback is given privately on Discord (@hackyguru).

**Milestone review is strict:**
- Every deliverable is checked against a pinned commit.
- Reviewers re-run the release bundle themselves.
- Process slips are recorded (for example, "SDK trait surface not shared before first implementation commit").
- Acceptance of a design milestone explicitly does not certify its security.

**Upcoming spec changes to anticipate** (template PRs #208 and #209):
- docs kept in your own repo rather than Logos doc packets
- modules published to your own catalog built from `logos-co/logos-modules-release-base`, released with `logos-modules-release-action`
- a **design decision log** in the repo, updated at each milestone

The RFP-005 forum post also adds: give estimates "assuming a team already productive on LEZ; new to the platform? account for ramp-up separately", and phasing is encouraged.

## 4. Winning-pattern checklist

1. Use the exact issue-form fields: RFP ID, project name, team or org, contact, team members, summary, technical approach, milestones/payout/timeline, total budget equal to the sum of milestones, experience, post-delivery plan.
2. Lead with **verifiable LEZ delivery**: merged Lambda Prize or RFP work, deployed testnet programs with transaction hashes and ImageIDs, public repos. Strongest of all is a working PoC or deployed program plus a demo video before the decision (as Antumbra, bleu and Gateway did).
3. Name the stack precisely: Rust, SPEL (IDL generated and drift-checked in CI), RISC0 guest, `lee_core`, pinned LEZ, SPEL and risc0 versions, and the `ring`/riscv32 build issue handled.
4. Include a **requirement traceability table** covering every F/U/R/P/S/Privacy item: approach section, milestone, test, status (Covered / gated / descoped plus reason).
5. Include a "dependencies that moved" table (LP-0013, LP-0015, LP-0012, RFP-001/002, oracle) with a fallback shim for each blocker.
6. Make **M0 a cheap de-risking or cost spike** with go/no-go gates: compute budget per operation, ChainedCall atomicity, custody rule, fee funding inside the deshield. Give a design-decision-log entry for each.
7. Spell out the privacy flow: an atomic deshield of token plus gas, single-use ephemeral accounts, a reshield step that cannot be skipped, a check that the reshield target is shielded, a pre-confirmation visibility summary, and a privacy-properties document. For RFP-012, also document the bad-debt first-toucher asymmetry and show it in the GUI.
8. Use the app architecture Logos expects: a Rust core module with no GUI, a QML + C++ Basecamp module loadable from git and published to your own catalog, a CLI, and an SDK. Include Figma designs (or equivalent) and use logos-design-system.
9. Testing: at least one test per hard requirement; e2e against a standalone sequencer with `RISC0_DEV_MODE=0` in green CI; property tests and Kani for invariants; differential test vectors; measured per-operation transaction size and cycle counts (RFP-012 requires up to 10 markets within compute limits).
10. Audit: name a tier-1 firm, give an itemised audit line and timeline, and publish reports before any mainnet recommendation. RFP-012 makes this a hard requirement.
11. Budget: milestones of roughly $5k–$18k each, with engineer-weeks shown where possible. L-tier precedent is $48k–$122k bid and $55k awarded; XL is $82k–$296k. Expect to negotiate down.
12. Post-delivery: a support window of about 6 months, maintenance against SPEL/LEZ version bumps, audit remediation, no revenue share.
13. Plan milestone submissions as evidence packs: a release tag, a MILESTONES mapping, REPRODUCE steps, an evidence index, a Basecamp walkthrough video.

## 5. Relevant existing repos

- **Platform:**
  - https://github.com/logos-blockchain/logos-execution-zone
  - https://github.com/logos-blockchain/lez-programs: `token`, `amm`, `ata`, `stablecoin` (Logos's RFP-013 CDP draft, the natural host for RFP-014), `twap_oracle` (canonical price accounts), `token_mint_authority`, plus a QML `apps/amm` built with Nix
  - https://github.com/logos-co/spel
  - https://github.com/logos-co/logos-modules-release-base
  - https://github.com/logos-blockchain/logos-execution-zone-module
  - https://github.com/logos-blockchain/logos-execution-zone-wallet-ui
- **Lending (the RFP-012 base):** https://github.com/Kolkheti-Labs/argo (M0 in PR #1). Also https://github.com/vacuumlabs/Logos-RFP-008-PROPOSAL, a losing bid.
- **Liquidation/auction (RFP-014 competitors):**
  - https://github.com/edenbd1/antumbra-auction: deployed, integer increasing-discount auction, IDL, zkVM cycle counts
  - https://github.com/edenbd1/antumbra-lez: curve, LBP and vesting programs, with a Basecamp `.lgx` app, `module.json` and executor tests (a good packaging reference)
  - https://github.com/Web3Spell/Liquidation-Auction-Engine: README only
- **Oracle:** https://github.com/equilibriumco/logos-kanon (RFP-020, in delivery) and https://github.com/fryorcraken/lez-signature-bench.
- **Other reference implementations:**
  - https://github.com/gateway-fm/lez-atomic-swaps (best example of milestone evidence and Basecamp packaging)
  - https://github.com/bleu/lez-bonding-curve-launchpad (the PoC that helped win)
  - https://github.com/paradoxcomputer/lpad and https://github.com/paradoxcomputer/ldex-mono
  - https://github.com/retraca/logos-authority-libs and https://github.com/retraca/logos-mint-authority (LP-0013)
  - https://github.com/fryorcraken/lez-proof-vault
  - https://github.com/logos-co/whisper-wall (SPEL demo)
  - https://github.com/logos-kit/logos-kit (wallet/provider SDK)
  - https://github.com/paradoxcomputer/zonescan (explorer)
- **No existing LEZ implementation found** of a Morpho-style vault (RFP-012), a RAI-style surplus/debt auction house, or a standalone lending oracle adapter, beyond the repos above.

## 6. Other evidence

- The forum (forum.logos.co) posts RFP announcements as "New RFP: RFP-0NN …", and hackyguru posted RFP-005 on 2026-09-30. Contracting and milestone updates are posted as replies in the same thread and in Discord `#builder-hub`.
- The process is documented in logos-co/ecosystem `content/rfp_lifecycle.md`, with tracking on Project 18. Proposals now get added to it automatically (PR #207).
- No public scoring rubric exists beyond the spec's FURPS hard requirements and the stated factors: technical merit, commercials, time fit, prior experience.
- Ecosystem issue #201 ("[LP] lending & borrowing app") plans a Lambda Prize for the Basecamp app over RFP-008 and RFP-012, which is another possible entry point.
