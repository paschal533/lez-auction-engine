# Research 5: RFP-014 requirement traceability, the competitors, and how to win (as of 2026-10-01)

This report builds on `scratchpad/reports.md` and does not repeat what that covers: the program process, the platform stack, or the general winning pattern. Raw material is in `scratchpad/r5/`:
- `rfp014.md` and `appendix.md`
- `body187.md` and `body159.md`, with `comments*.md`
- `aa/` (antumbra-auction files) and `al/` (antumbra-lez files)
- `eco.txt`, `reviewer_comments.txt`, `accepted_comments.txt`
- `pr208.diff`, `pr209.diff`, `pr45.diff`

---

## 0. The facts that change the picture

1. **Antumbra's "deployed, not proposed" auction no longer exists on chain.**
   - The public LEZ testnet was reset on **2026-09-08** and is now v0.3.
   - All 11 transaction hashes in proposal #187 are listed by Antumbra themselves in `antumbra-auction/evidence/pre-reset-hashes.txt`, under the comment "each must NOT resolve". That commit is `8ab3bdc`, dated 2026-09-27.
   - The auction has not been redeployed on v0.3.
   - Their only v0.3 work is the *vesting* program, run on a **local** sequencer. Their `docs/deploy-v03.md` says "Testnet v0.3 deployment: pending funding (LGO arrives only through a Bedrock deposit … There is no LEZ faucet)."
   - So today, **nobody has any RFP-014 artifact on a live chain.**
2. **Antumbra's on-chain auction is an accounting simulator, and the clock is caller-controlled.** In `programs/auction/src/bin/antumbra_auction.rs`:
   - `start_auction` is **admin-signed**, not permissionless. It takes `collateral`, `to_raise` and **`price`** as plain arguments. The README itself says "`collateral` is what the caller asserts was taken".
   - `buy_collateral` takes **`now: u64` from the caller** and only rejects `now < last_seen`.
   - **Any bidder can therefore pass a far-future `now` and buy at the maximum discount immediately.** The guard stops rewinding the clock, not fast-forwarding it.
   - The price is frozen at auction start rather than re-read from an oracle at each bid (RAI re-reads it).
   - No tokens move: there is no payment debit, no collateral credit, and no host seizure. The README admits this under "What this is not, yet".
3. **Two of Antumbra's dependency claims are now out of date.**
   - "LP-0012 events: `Method not found`" was true on v0.2.4. LEZ **v0.3.0 (2026-09-30)** shipped events, events RPC, events FFI and event filters (LEZ PRs #705, #707, #709, #713, #714, #785, #802).
   - The same release added "authorized balance decrease / permissionless claims" (#817), which bears on LP-0013 custody.
4. **RFP-013, the host CDP, is being built inside Logos.**
   - The ecosystem #185 body was edited on 2026-09-02 to read "Grantee: **Logos R&D Program Team**".
   - The code is `logos-blockchain/lez-programs` `programs/stablecoin`, actively developed by Andrea Franz, Ricardo Schmidt (3esmit) and 0x-r4bbit. Recent work:
     - freeze/unfreeze and admin setters (09-21)
     - a U512 redemption-price fix (09-24)
     - open PRs #333–#340 and #399–#400, which add a stablecoin **module API**
     - #401, a network-agnostic stablecoin **testnet deployment script**
     - #385, an upgrade to LEZ v0.2.5-rc2
   - #401 also warns: "Nonzero controller gains are rejected until the TWAP Q64.64 and protocol 10^27 price scales are reconciled." That is a concrete integration hazard our engine must handle.
   - The design doc (§12, line 1220) names the RFP-014 hook it expects: "a new instruction `liquidate_position` callable by the liquidator program (gated by checking the position's collateralization)".
   - Ecosystem #210 contradicts #185 by calling RFP-013 "Externalized". The #185 body edit is the more recent and more specific statement.
5. **The RFP-014 UI is being split into its own RFP.** Ecosystem #210 (Stablecoin UI RFP draft, edited 2026-09-07/08) says:

   > "The liquidation/auction surface over the Liquidation & Auction Engine (RFP-014) is a **separate** RFP … we want a **single studio to pick up both** — this UI RFP and the RFP-014 UI RFP"

   and "RFP-014 … Its UI is a separate RFP". So when RFP-014 reopens, U2, U4, U5 and the mini-app parts of Privacy 1 may be removed or reduced, and a UI-only RFP will exist that pairs with the stablecoin UI.
6. **Antumbra already lost a head-to-head where they also had deployed programs.**
   - They filed for RFP-015 (#184), RFP-016 (#185) and RFP-017 (#186) with live v0.2 programs.
   - RFP-015/016 went to **bleu** (#118/#119, accepted, renegotiated to **$55k combined for two L RFPs**). Antumbra's bids remain open with no decision.
   - Three of the λPrize submissions they cite as "currently open" (LP-0002, LP-0003, LP-0008) were **closed without merge** on 2026-09-07 by reviewer weboko. His reasons:
     - "demo video doesn't cover Basecamp app but only CLI"
     - "demo video is narrated using AI which is not fair"
     - "CI doesn't seem like doing any full e2e with `RISC0_DEV_MODE=0`"
     - "ensure all information is in submission file … transaction hashes are trimmed and do not have links"
   - Only LP-0005 is merged (PR #64, 2026-08-02).
7. **Web3spell (#159) has no code.** `Web3Spell/Liquidation-Auction-Engine` is a single 384-line README, last pushed 2026-08-06. Its proposal marks all 30 requirements "Fully Met". Its discount formula, `d_min·(1+r)^(t−t0)`, is exactly the compounding that Antumbra showed overflows `u128`.

---

## 1. Requirement checklist (current RFP-014 text, status `closed`, last content change `8a76efb`, 2026-07-21)

The IDs follow the RFP's own numbering. Each one is quoted exactly.

### Functionality (hard)
| ID | Exact wording |
|---|---|
| F1 | "Anyone can trigger liquidation on any undercollateralized position. No whitelist or special permissions required. The liquidator receives an incentive (e.g., a percentage of seized collateral)." |
| F2 | "Seized collateral is sold via auctions where the discount to market price increases over time. The discount schedule must be configurable. Auctions terminate early when the target amount is reached." |
| F3 | "Auctions support buying partial collateral lots, preventing large positions from becoming stuck." |
| F4 | "When collateral auctions don't raise enough to cover debt plus penalty, the shortfall is handled via a debt auction mechanism. The mechanism must be fully implemented and tested (using a mock token), but parameterized by a configurable token address so the deployer can point it at whatever protocol token they choose. The protocol token itself is out of scope … Until a protocol token is configured, the surplus buffer absorbs shortfalls up to its capacity; if bad debt exceeds the buffer, the system pauses new debt generation via freeze authority (RFP-002)." |
| F5 | "When the protocol accumulates excess stablecoin above a threshold, surplus auctions sell it for a protocol token (which is burned). As with F4, the surplus auction mechanism must be fully implemented and tested with a mock token, parameterized by a configurable token address. Until a protocol token is configured, surplus accumulates in the buffer without being auctioned." |
| F6 | "Auctions follow strict state transitions with no reversals. All state changes are atomic and verifiable." |
| F7 | "The system integrates with its host CDP protocol via a well-defined interface. Each product deploys its own configured instance of the program." |

### Usability (hard)
| ID | Exact wording |
|---|---|
| U1 | "Build the program using the SPEL framework, which generates the IDL and client code from the program definition." |
| U2 | "Provide a Logos mini-app GUI with local build instructions, downloadable assets, and loadable in Logos app (Basecamp) via git repo." |
| U3 | "Provide a CLI that covers core functionality of the program. The CLI may have fewer features than the GUI mini-app but must support all essential operations." |
| U4 | "Tools display undercollateralized positions with estimated liquidator incentives and allow users to trigger liquidation, enabling efficient bot and manual operation." |
| U5 | "Active auctions display collateral type, amount, current discount, raised amount vs target, and time remaining. Users can bid with clear previews of received collateral and costs." |
| U6 | "Provide an SDK that can be used to build Logos modules for interacting with the program. The SDK must support private account operations via the deshield-interact-reshield pattern." |

### Reliability (hard)
| ID | Exact wording |
|---|---|
| R1 | "Auction operations are non-reentrant. State transitions are ordered to prevent manipulation." |
| R2 | "Liquidation uses time-weighted or confidence-adjusted prices, not raw spot. If price feeds are stale, liquidations pause for that collateral type." |
| R3 | "Multiple liquidations and auctions can occur simultaneously without interference. Each auction maintains isolated state." |
| R4 | "If a price feed fails for one collateral type, only that type's liquidations pause." |
| R5 | "Other collateral types and non-liquidation operations continue unaffected." |
| R6 | "Critical invariants: seized collateral equals collateral in auction; debt shortfalls are properly accounted; surplus and debt auctions maintain system balance." |

### Performance (hard)
| ID | Exact wording |
|---|---|
| P1 | "Liquidation transactions complete within one block." |
| P2 | "The system supports many concurrent auctions without exceeding compute limits." |
| P3 | "Bid transactions complete within one block." |

### Supportability (hard)
| ID | Exact wording |
|---|---|
| S1 | "The program is deployed and tested on LEZ devnet/testnet." |
| S2 | "End-to-end integration tests run against a LEZ sequencer (standalone mode) with `RISC0_DEV_MODE=0` and are included in CI." |
| S3 | "CI must be green on the default branch." |
| S4 | "Every hard requirement in Functionality, Usability, Reliability, and Performance has at least one corresponding test." |
| S5 | "A README documents end-to-end usage: deployment steps, program addresses, and step-by-step instructions for interacting with the program via CLI and mini-app." |

### Privacy (hard)
| ID | Exact wording |
|---|---|
| PRIV1 | "The mini-app and SDK must support both direct public account interaction and the deshield-interact-reshield pattern for private account interaction. When a user chooses the private account path, the SDK must enforce the complete pattern — the reshield step must not be skippable." |
| PRIV2 | "When using the private account path, the SDK must validate that the target account for reshielding is a private (shielded) account before submitting the transaction, and reject the operation with an explicit error if it is not." |
| PRIV3 | "The ephemeral public account created during the deshield step must never be reused across operations. Each protocol interaction from a private account must use a freshly generated account with no prior on-chain history." |

**Derived from the "Privacy Architecture" section.** These are not numbered in the RFP but are binding in practice; RFP-004 review #40 treated the equivalent as required.
| ID | Exact wording |
|---|---|
| PA1 | "For every protocol operation (liquidate, bid, settle)" the private flow applies: so it covers **liquidate, bid and settle**, not bid alone. |
| PA2 | "The deshield atomically transfers both the operation token and enough native token for gas in a single indivisible action." |
| PA3 | "The ephemeral account shields any outputs back to the user's private account. The account is never reused." |
| PA4 | Public: "All auction state … All liquidation events and their amounts. Price feeds and position health data." Private: "Which private account initiated any liquidation or bid. Where auction proceeds go after reshielding. Any link between multiple protocol interactions by the same user." |

### Soft requirements
| ID | Exact wording |
|---|---|
| SOFT1 (Reliability 1) | "Multi-oracle redundancy for price feeds, with fallback when primary is stale." |
| SOFT2 (Reliability 2) | "Formal verification of auction invariants: collateral seized equals collateral in auction; debt shortfalls are correctly propagated to debt auction house." |
| SOFT3 (Usability 1) | "Liquidator bot SDK with position monitoring and automated liquidation triggers." |

### Out of scope (bidding on these reads as scope creep)
- OOS1: "Flash loan liquidations"
- OOS2: "Liquidation aggregators (batching across multiple positions)"
- OOS3: "Insurance funds or socialized loss mechanisms"
- OOS4: "Multi-collateral atomic liquidations"

### Dependencies (frontmatter and body), with their real status today
| ID | RFP says | Reality (2026-10-01) |
|---|---|---|
| DEP1 RFP-013 | "Reflexive Stablecoin Protocol (or equivalent CDP host)" | Built in-house by Logos R&D in lez-programs. It has no liquidation hook yet, is pinned to LEZ v0.2.4 (upgrade PR #385 targets v0.2.5-rc2), and has a testnet deploy script in PR #401. |
| DEP2 RFP-020 | external price feeds | Kanon (Equilibrium) has M0 approved and later milestones open. lez-programs `twap_oracle` exists, and the RFP itself names RFP-019 TWAP as the alternative. |
| DEP3 LP-0013 | "currently **open**" | Lambda-prize marks it closed. LEZ v0.3.0 has "authorized balance decrease" (#817). PDA-owned vault plus chained `Token::Transfer` already works (the stablecoin vault does exactly this, design doc line 207). |
| DEP4/5 RFP-001/002 | "libraries are in development" | Delivered by mmlado as SPEL extensions; RFP-001 M3 closed 2026-09-22. The stablecoin added its own `freeze`/`unfreeze` instructions on 09-21. |
| DEP6 LP-0015 | resolved | Chained calls are in use. |
| DEP7 LP-0012 (body) | "closed" (resolved) | Delivered in **v0.3.0**; not available on v0.2.4. |

### Internal inconsistency to note in the proposal
The Overview and ecosystem #205 say "a **fixed-discount** auction", while F2 and the appendix specify an **increasing-discount** auction. Follow F2. Asking for this in a clarification shows a careful reading.

### Changes likely to arrive when RFP-014 reopens (pending, not yet merged)
These come from the template PRs #208 and #209, and from RFP-005, the most recently opened RFP.

| ID | Source | Wording |
|---|---|---|
| PEND1 | PR #208 (template S6) | "Provide documentation in repo for the Logos module/library, covering the developer integration journey." This replaces doc packets. |
| PEND2 | PR #208 (S7) | "Provide documentation in repo for the CLI, covering the core operator/user journey." |
| PEND3 | template S8 (RFP-014 currently lacks it) | "Provide Figma designs or equivalent for all GUI artifacts." |
| PEND4 | template S9 (RFP-014 lacks it) | Privacy and anonymisation properties document: what is visible, what is protected, which trust assumptions are "enforced by the on-chain program and which depend on correct client behaviour", and "what happens if a user bypasses the expected interaction path". |
| PEND5 | PR #208 (S10) | "Publish the resulting modules in a module catalog of the team's own, built from the Logos module catalog template (logos-modules-release-base)". |
| PEND6 | PR #208 (S11) | "The modules must be built and released to that catalog using logos-modules-release-action." |
| PEND7 | PR #209 | "Provide a design decision log in repo recording each major design decision and choice: the context, the options considered, the option chosen, and the rationale and trade-offs … current at each milestone delivery." |
| PEND8 | RFP-005 S13 pattern | An audit programme that names a tier-1 firm, with an audit budget line item and an audit timeline before any mainnet recommendation. RFP-014 has no such requirement, but the proposal form asks for "an Audit milestone if your project handles user funds", and this engine custodies seized collateral. |
| PEND9 | RFP-005 S3 pattern | "compatible with Logos testnet 0.3 and 0.4". |
| PEND10 | RFP-005 S7 pattern | "Performance requirements are satisfied by reported measurements … but the benchmark harness must be committed and reproducible." |
| PEND11 | Draft PR #45 (threat model) | Adversary classes O/A/C/S/F/X. Under it, "Launchpad and auction protocols: C-class scenarios for griefing the price discovery process; F-class scenarios for sniping". If merged, the privacy doc must address every scenario. |
| PEND12 | ecosystem #210 | The GUI (U2/U4/U5) may move to a separate "RFP-014 UI RFP". |

---

## 2. The competitor proposals in detail

### #187 Antumbra (edenbd1 + nadaamd)
- **Filed** 2026-08-23. Status: open, no reviewer comment.
- **Budget:** $55,000 all-in = $43,000 in milestones + $12,000 auditor fee. Commits to 12 weeks, with a 9.5-week critical path.
- **Milestones:**

  | Milestone | Payout | Duration |
  |---|---|---|
  | M0 cost baseline + 2 spikes + audit booked | $3.5k | 1 wk |
  | M1 hardened auction house | $7k | 2 wk |
  | M2 liquidation, prices, isolation | $7k | 2 wk |
  | M3 debt/surplus + buffer | $5k | 1.5 wk |
  | M4 host interface + reference host | $3.5k | 1 wk |
  | M5 SDK, CLI, mini-app, privacy path | $5.5k | 1.5 wk |
  | M6 CI, tests, docs, testnet bundle | $3.5k | 1 wk |
  | M7 audit coordination and remediation | $8k | 2 wk |

- **Strengths:**
  - A complete 30-row traceability table.
  - Strong numerics:
    - `u128` overflow of GEB's `rdivide`/`rpower`
    - floor-rounding and split-bid extraction
    - a minimum-improvement `beg` rounding bug in surplus/debt bids
    - 256-bit compare for health
    - staleness modelled as a state (`Paused`)
  - Differential vectors against Python, and zkVM cycle counts re-measured in CI (27,753 cycles for one bid).
  - A self-found defect story, a verify script, and reproducible builds.
  - Six months of post-delivery support.
  - All three soft requirements in scope.
- **Weaknesses we can exploit:** see §0 items 1–3 and 6, plus:
  - The "host" is an in-repo stand-in (`CollateralSource`). They explicitly defer conformance to a real CDP to post-delivery.
  - The Basecamp work is a legacy `type: "ui"` C++ widget (read-only, vesting). It is not a `ui_qml` app with a C++ backend, which reviewers asked for in #40.
  - The team is bidding on four RFPs at once (014/015/016/017) with two people.
  - It uses AI narration, which a λPrize reviewer explicitly rejected.
  - No CI e2e against a sequencer.
  - The F7 reading ("one program, many config accounts") is defensible, and we should adopt it too (see §6).

### #159 Web3spell Labs (rythmern02 + 2, plus contractors)
- **Filed** 2026-08-07. Status: open; the only comment is the author's own "any updates?" on 08-26.
- **Budget:** $48,000 over 12 weeks.

  | Milestone | Payout | Duration |
  |---|---|---|
  | M1 scaffold, math, host interface | $11k | 3 wk |
  | M2 auction houses, oracle breakers | $14k | 4 wk |
  | M3 CLI, mini-app, privacy SDK, bot | $12k | 3 wk |
  | M4 audit prep, CI e2e, deploy, README | $11k | 2 wk |

- **No code, no LEZ track record,** and no audit firm or audit fee.
- Claims a "TS/Rust SDK" and consumes "LP-0012 events".
- Uses compounding discount math (the `u128` trap).
- Puts testnet deployment and CI last (M4).
- Marks all 30 requirements "Fully Met" at proposal stage, which reads as unverifiable.
- Its experience section is mostly events, community and non-LEZ repos, the pattern reviewers challenged in #114.
- It is not a serious threat on merit; it mainly sets a price anchor at $48k.

---

## 3. Traceability matrix (strict: "done" only where code or a deployment exists today)

Legend:
- **Done** = verifiable now.
- **Done (gone)** = the code exists, but its only on-chain evidence was wiped by the 2026-09-08 reset.
- **Lib-only** = an off-chain Rust function plus tests, not a program instruction.
- **—** = nothing exists.

Antumbra repos:
- AA = https://github.com/edenbd1/antumbra-auction (21 commits, last 2026-09-27)
- AL = https://github.com/edenbd1/antumbra-lez (87 commits, last 2026-10-01)

| ID | Antumbra DONE (verified artifact) | Antumbra PROMISE | Web3spell | Gap / our opening |
|---|---|---|---|---|
| F1 | Lib-only: `health`, `liquidator_reward` in AA `src/liquidation.rs`. No `liquidate` instruction. `start_auction` is admin-only. | M2: permissionless `liquidate` plus incentive; "three unrelated signers" on testnet | Promise M2 | **Nobody liquidates a real position.** We liquidate a real lez-programs stablecoin position through an upstreamed `liquidate_position` hook. |
| F2 | Done (gone): linear increasing multiplier in AA `src/lib.rs` + `buy_collateral`, with early termination on target or empty lot. **Flaw:** caller-supplied `now` (fast-forward to max discount) and a price fixed at start. | M1: hardening (reserve price, min bid, deadline) | Promise; compounding formula | Read the clock from `CLOCK_01` and the price from twap_oracle at bid time. Make the schedule a configurable curve. |
| F3 | Done (gone): partial fills with floor rounding and re-strike; 600-fill test | M1 | Promise | Parity, plus real token transfers on each fill |
| F4 | Lib-only: `settlement.rs` (`Settlement::accepts/improve`, `beg` ≥ 1 unit). No buffer, no freeze, no instruction. | M3 | Promise M2 | Shortfall flows into an on-chain buffer, and the freeze calls the host's real `freeze` instruction (stablecoin `494dba8`) |
| F5 | Lib-only (same) | M3 | Promise M2 | Surplus from the host's fee accumulator (stablecoin `materialize_surplus` idea, doc §12 line 1232); mock-token auction plus burn |
| F6 | Done: `settled` flag terminal; double and early settle refused. Only two states. | M1: "transition pairs enumerated" | Promise | Explicit state enum (Open, Settled, ShortfallBooked …) with a transition table test |
| F7 | Done: `init_config` gives one config PDA per product under one program. No host interface. | M4: `CollateralSource` trait + reference host | Promise M1 (mock host) | A real host (stablecoin) plus a documented interface matching the appendix (`seize_collateral`, `cancel_debt`, `get_system_coin_redemption_price`, `redeem_against_position` stub) |
| U1 | **Done:** SPEL program, `idl/antumbra_auction.idl.json` generated by `spel generate-idl` | — | Promise | Parity; add an IDL drift check in CI |
| U2 | — for auction. AL has a read-only legacy `ui` C++ widget (vesting), `.lgx` built with logos-module-builder, a plugin-load test in CI. | M5 | Promise M3 (QML) | `ui_qml` app + C++ core module, built on lez-programs `apps/shared/wallet` and logos-design-system |
| U3 | — for auction (AL has a vesting CLI) | M5 | Promise M3 | Rust CLI using the generated client |
| U4 | — | M5: polling-based list | Promise M3 (event bot) | Position scanner over the stablecoin's `position` accounts (module API PR #334), plus v0.3 events when available |
| U5 | — | M5: "preview and fill diffed equal" | Promise M3 | Auction view and bid preview, using the same Rust function compiled into the FFI |
| U6 | — (manual deshield→buy run twice on pre-reset v0.2.4 for *launchpad*, AL `DEPLOYMENTS.md` §"The private path") | M5 | Promise M3 (TS/Rust) | An SDK in the FFI and module layer with a non-skippable reshield type |
| R1 | Single-instruction design; no reentrancy test exists | M1 | Promise | A test where a chained-call re-entry is refused |
| R2 | Lib-only: `Price` with observation time; stale returns `Paused` | M2: in-repo TWAP reference | Promise M2 | Consume lez-programs `twap_oracle` on chain; handle the **Q64.64 vs 1e27 price scale** (lez-programs #401) |
| R3 | Done (gone): two configs and two auctions driven concurrently | M1 | Promise | Parity plus a P2 stress test |
| R4 | Lib-only | M2 | Promise M2 | Per-collateral-type pause, tested on chain |
| R5 | Lib-only | M2 | Promise | Bids and settles continue while liquidations of that type are paused |
| R6 | Partial: on-chain `sold + left == seized`, `raised ≤ to_raise`. No system-balance invariant. | M1/M3 | Promise ("property tests") | Add a system-balance invariant across buffer, debt and surplus; proptest now, Kani later |
| P1 | — | M2: "inclusion block measured" | "stress-tested" (no evidence) | Measure in the standalone-sequencer e2e |
| P2 | Done (measured): executor cycles for arithmetic only (`zkvm/CYCLES.md`); not whole-instruction | M4: whole-instruction table | Promise | Whole-instruction cycle and transaction-size table vs N live auctions, committed and re-run in CI |
| P3 | Done (gone): 62 s proof, landed next block (v0.2) | M1 | Promise | Re-measure on the current LEZ |
| S1 | **Not currently met.** It was deployed on the v0.2 testnet; the hashes are wiped and self-declared pre-reset. No v0.3 deploy: needs LGO via Bedrock. | M1/M6 | Promise M4 | If Logos can fund LGO, a live v0.3 deployment would be the only one in the field |
| S2 | **—** (CI runs unit, differential, cycles and docs; no sequencer e2e, no `RISC0_DEV_MODE=0`) | M6 | Promise M4 | Ship it in the PoC: dev-mode per PR plus a real-proof (`RISC0_DEV_MODE=0`) job |
| S3 | Done (green on main for what exists) | M6 | Promise | Parity |
| S4 | — (test names exist, no requirement matrix) | M6 | Promise | A `REQUIREMENTS.md` mapping every ID to a named test, checked by CI |
| S5 | Partial: README and DEPLOYMENTS (pre-reset addresses) | M6 | Promise M4 | README with addresses, CLI and app walkthrough |
| PRIV1 | — | M5: "operation handle can only be consumed by reshielding" | Promise M3 | Typestate in the SDK: `Deshielded → Executed → Reshielded`, where only `Reshielded` yields the result |
| PRIV2 | — | M5 | Promise (`PublicReshieldTargetProhibited`) | Typed error plus a negative test |
| PRIV3 | Done (gone), for launchpad only: two distinct ephemerals on v0.2.4 | M5 | Promise | A fresh keypair per operation, with an assertion of no prior history |
| PA1–PA4 | — | M5 (bid only explicit) | Promise | Cover **liquidate, bid and settle**, plus atomic token+gas deshield (PA2) |
| SOFT1 | — | M2 | Promise M2 | twap_oracle as primary and a second feed (mock or RedStone shim) as fallback |
| SOFT2 | — for auction (AL has **Kani** proofs for vesting in CI, which shows the capability) | M6 | "property-based fuzz" M4 | proptest in the PoC; Kani harness as a milestone |
| SOFT3 | — | M4/M5: reference liquidator plus monitor | M3: Rust bot daemon | Bot built on the FFI plus events |
| PEND1–2 | Partial (rich READMEs) | M6 | Partial | In-repo `docs/` from day one |
| PEND3 | — | — | Designer on team (claims Framer) | Figma or equivalent screens for the auction view |
| PEND4 | — | Not explicit | Not explicit | A `PRIVACY.md` covering the bypass path |
| PEND5–6 | — (AL `.lgx` is self-packaged; no catalog/release action) | — | — | Own catalog from logos-modules-release-base plus release action |
| PEND7 | Partial (findings narrated in README, not a DDL) | — | — | `docs/decisions/` ADR log |
| PEND8 | Audit: firm "Trail of Bits, Zellic, NCC or similar", $12k fee line, booked in M0 | M7 | None named | Name a firm and itemise the fee (see §6c) |

**Score, strictly by artifacts that exist and are verifiable today:**
- **Antumbra:**
  - Fully done: U1 and S3.
  - Done (gone): F2, F3, F6, F7 (config only), R3 and P3, all of which existed only on a chain that has been reset.
  - Lib-only: F1, F4, F5, R2, R4, R5.
  - Partial: R6, P2, S5.
  - Nothing exists: U2–U6, S1, S2, S4 and all PRIV items.
- **Web3spell:** zero.

---

## 4. What Logos says about RFP-014's future (ecosystem issues)

- **#205 `[RFP] Liquidation & Auction Engine`** (fryorcraken; opened 08-10, updated 09-22; label `write-rfp`)
  - It is a tracking issue that restates the RFP and its dependency list: RFP-013, RFP-020, LP-0013, RFP-001, RFP-002, LP-0015.
  - It has no comments, no reopen date and no scope change yet.
- **#210 `[RFP] Stablecoin UI`** (fryorcraken; opened 08-19, edited 09-07/09-08)
  - The original draft covered both the stablecoin UI and the RFP-014 liquidator/bidder UI.
  - The edits split them: the stablecoin UI goes in this RFP, and "Out of scope: the liquidator/bidder auction surface over RFP-014, covered by its own RFP."
  - It says "we want a **single studio** to pick up both".
  - **Implication:** RFP-014 is likely to be rewritten as engine plus SDK plus CLI, possibly keeping a minimal GUI. A separate RFP-014 UI RFP will be bundled in spirit with the stablecoin UI.
- **#200 `[LP] stablecoin app`** (08-10, label `write-lambda-prize`)
  - A planned Lambda Prize for the Basecamp stablecoin app "over RFP-013 … and RFP-014". It mirrors the lending-vault-app LP pattern (#201).
  - This is a third, cheaper entry point that our QML app could target.
- **#185 `[RFP] Reflexive Stablecoin Protocol`**
  - Edited 2026-09-02 to read "Grantee: **Logos R&D Program Team**".
  - RFP-013 is internal, and the code is the lez-programs stablecoin (very active as of 2026-10-01).
- **#224 `[RFP] CDP-Backed Synthetic Assets (sASSET)`** (08-31, draft rfp PR #57)
  - A future second CDP host that "requires a Liquity-style stability-pool liquidation mechanism". That is a different resolution path, but it would use the same trigger layer.
  - It supports the "reusable engine with a pluggable resolution path" story.
- **Timing (inference):** no reopen date exists anywhere.
  - RFP-014 depends on RFP-013, which is internal and still adding instructions; liquidation is listed as RFP-014's job.
  - The likely trigger for reopening is the stablecoin reaching testnet (lez-programs #401 is the deploy script, opened 09-30) and its LEZ upgrade landing (#385 targets v0.2.5-rc2).
  - Plausible window: weeks, not months. **A 3-week PoC lands at about the right time.**

---

## 5. The review rubric (distilled from evidence)

Sources:
- fryorcraken's #40 review
- jzaki and hackyguru comments on about 45 proposals
- acceptance comments on #117, #112, #118 and #119
- weboko's λPrize closures
- the Argo M0 submission #170
- the outcome of Antumbra versus bleu

**Gate (pass or fail)**
1. **Every hard requirement is addressed**, each "in scope, or explicitly descoped with a reason" (#40). Rejections cite "address all criteria" (#105).
2. A milestone breakdown with a USD payout per milestone that sums to the total ("Can you please divide things up into requested milestones", #115).
3. A budget that is plausible and says clearly what it includes: "Are you including the audit cost?" (#116); "$10,000 … plus a third-party audit is not a usable number" (#40).
4. A verifiable team: "Do you have some existing work history somewhere? … github has no history" (#114).

**Comparative scoring.** Stated factors: "technical merit, commercials, time fit and previous experiences" (#53); "fit our goals/timelines/budget better" (#27).

5. **Detail relative to the field:** "insufficient details in comparison to other applications" (#83). Positive signals: "We appreciated the detail in your submission" (#69, #76) and "attention-to-detail" (#117, the winner).
6. **Precision.** Define terms ("What is CU here?", #111). Measurements should include verification cost.
7. **Scope discipline.** No extras outside the RFP ("Remove RecoverSurplus", #40); out-of-scope items not billed.
8. **The expected architecture:**
   - "QML UI module with a C++ backend module, not a general Qt/C++ plugin" (#40)
   - a fresh codebase, not an extension of a sample app (#51)
   - logos-design-system rather than paid branding (#51)
   - an SDK clearly defined as a core module with an FFI API, a Rust library, or both (#40)
9. **Absorbing spec changes quickly.** Proposals and PoCs updated after spec PRs (jzaki on RFP-020's SLA; bleu after PR #204).
10. **Commercials are negotiated down.** bleu took two L RFPs for $55k combined.
11. **Evidence quality** (λPrize reviewer weboko, the same organisation):
    - the demo must show the **Basecamp app, the CLI and the design choices**
    - **human narration** (an AI voice was rejected)
    - all information lives in the submission document, with **full, linked tx hashes**
    - **CI must run full e2e with `RISC0_DEV_MODE=0`**
12. **Milestone acceptance is strict:**
    - a pinned commit and tag
    - reproduction from empty toolchain homes (Argo `harness/clean-host.sh`)
    - spike verdicts (GO/NO-GO)
    - a verification log
    - reviewers re-run the bundle
13. A prototype is **necessary but not sufficient.** Antumbra had live programs for RFP-015/016 and lost to bleu, an established firm with product-design M0 (PRDs and acceptance criteria), QML mini-apps and price flexibility.

---

## 6. Recommendations

### (a) Differentiators over Antumbra, ranked by impact per unit of effort for a 3-week PoC

| Rank | Differentiator | Why it beats Antumbra | Effort |
|---|---|---|---|
| 1 | **Liquidate a real lez-programs stablecoin position end to end** with a `liquidate_position` hook in the stablecoin, plus a draft upstream PR to logos-blockchain/lez-programs. | They only have a stand-in host and defer conformance to post-delivery. The reviewers wrote the stablecoin. It matches the design doc's own suggested hook (§12). It also covers F1 and F7 and turns DEP1 from a blocker into a demo. | Med-High (about 5–6 days) |
| 2 | **Trustless time and price:** read `CLOCK_01` and the twap_oracle account at **each bid**; no caller-supplied `now` and no admin-supplied price. Record it as a decision-log entry. | It removes the fast-forward-to-max-discount flaw and makes R2 and F2 real. Cite it neutrally as "why time is never an instruction argument" rather than naming the competitor. | Low (1 day) |
| 3 | **Real value movement:** a PDA collateral vault; chained `Token::Transfer` collateral to the bidder and stablecoin from the bidder; debt cancelled on the host. | Antumbra moves no tokens. This makes R6 a token-balance invariant, not just a counter. | Medium (2–3 days; copy the stablecoin vault pattern) |
| 4 | **S2 in the PoC:** standalone sequencer e2e in GitHub Actions. Dev mode on every push; a real-proof job (`RISC0_DEV_MODE=0`) on a small suite, nightly or on demand, and green. | Nobody has it, and Logos reviewers flagged its absence on Antumbra's λPrize submissions. | Medium (2 days; proofs take about 60 s per tx, so keep the real-proof suite to 3–5 transactions) |
| 5 | **Human-narrated demo** (about 4 minutes): Basecamp app + CLI + design choices, with full, linked tx hashes in a `SUBMISSION.md`. | Directly meets weboko's criteria, which Antumbra failed. | Low (half a day) |
| 6 | **A fresh "dependencies that moved" table** (October 2026): v0.3 events delivered; #817 authorized balance decrease; RFP-013 internal; testnet reset 09-08; faucet removed (LGO via Bedrock); RFP-001/002 delivered; the TWAP Q64.64 vs 1e27 scale issue. | It corrects the stale claims in #187 and shows we are current. | Low (half a day) |
| 7 | **QML `ui_qml` app + C++ universal core module + Rust FFI** reusing `apps/shared/wallet`, with an auction list, a bid preview built on the same Rust quote function, and a liquidation trigger. Packaged `.lgx`, own catalog via release action (PEND5/6). | This is the architecture Logos asked for in #40; Antumbra has a legacy read-only widget. It also positions us for the separate RFP-014 UI RFP and the stablecoin UI RFP. | Medium-High (4–5 days by copying `modules/stablecoin` and `apps/amm`) |
| 8 | **Requirement matrix enforced by CI:** `REQUIREMENTS.md` maps each of the 30 IDs plus PA1–PA4 to a named test or "milestone Mx", and a script fails if a test name is missing. | Turns S4 from a promise into an artifact. | Low (half a day) |
| 9 | **Decision log** (`docs/decisions/`, PEND7) and in-repo docs (PEND1/2) from day one. | Matches the template changes before they are merged. | Low |
| 10 | **Numerics parity:** `mul_div` in 256-bit; floor toward the auction; `beg` of at least 1 base unit; staleness modelled as a state; proptest for R6. | Neutralises Antumbra's main strength. Credit the overflow finding as known GEB-on-u128 behaviour; do not compete on it. | Low-Med (1–2 days) |
| 11 | Debt/surplus auctions on chain with a mock token, the buffer and a freeze through the host's real `freeze` instruction. | F4/F5 "fully implemented and tested". Antumbra has lib-only. | Medium (3 days); stretch for the PoC |
| 12 | Private path for bid, with typestate reshield, PRIV2 typed error and a fresh ephemeral (PRIV3). | Antumbra has done it only manually, for another program. | Med-High; stretch or M-promise |
| 13 | Public testnet v0.3 deployment of engine + stablecoin + oracle. | It would be the only live RFP-014 deployment. It needs LGO (ask in Discord #builder-hub) and the stablecoin on v0.3. | Uncertain; attempt in week 3 only if both are available |

**The version decision (critical):**
- lez-programs (the host) is pinned to LEZ **v0.2.4**, with an upgrade PR to **v0.2.5-rc2**. The public testnet is **v0.3**. SPEL v0.7.0 targets v0.2.x.
- For the PoC, **pin to whatever lez-programs `main` pins**, and run the standalone sequencer from that tag in CI.
- Write a v0.3 port plan as a decision-log entry. Antumbra-lez `docs/v03-port-plan.md` shows what breaks: borsh I/O, sharded state, fees and `program_owner: AccountId`.
- Track lez-programs #385 and #401 and rebase when they merge.

### (b) Must be demonstrated in the PoC vs fine as milestone promises

**Must be in the PoC** (the minimum that beats "deployed, not proposed"):
- **F1** permissionless liquidation of a real stablecoin position, with an incentive paid.
- **F2/F3** increasing discount from `CLOCK_01`, partial fills and early termination.
- **F6** state machine with refusal tests.
- **F7** documented host interface plus the real stablecoin as host.
- **R2/R4/R5** twap_oracle read, with a stale feed pausing liquidation of one collateral type while bids continue.
- **R3** two concurrent auctions.
- **R6** on-chain conservation check plus proptest.
- **U1** SPEL + IDL (drift-checked).
- **U3** CLI covering liquidate, bid, settle and inspect.
- **U5 (minimal) + U2 (minimal)** QML app showing live auctions with discount, raised vs target, time left and a bid preview, plus a liquidate button. Build it in its own module and milestone so it can be stripped if the UI moves to the separate RFP.
- **S2/S3** green CI, with standalone sequencer e2e and one real-proof job.
- **S5** README.
- **P1/P3** one block each, measured in the e2e.
- A narrated demo plus `SUBMISSION.md`.
- `REQUIREMENTS.md` (S4 skeleton).
- Decision log.

**Fine as milestone promises** (state the plan, the test and the milestone for each):
- F4/F5 full on chain. If time allows, show the buffer accounting plus a mock-token skeleton in the PoC.
- U4 full scanner.
- U6 + PRIV1–3 + PA1–PA4: the SDK private path.
- S1 public testnet v0.3 deployment, gated on LGO funding and the host port.
- S4 complete.
- P2 cycle table vs N auctions.
- SOFT1 (oracle fallback), SOFT2 (Kani), SOFT3 (bot SDK on v0.3 events).
- PEND3 (Figma), PEND4 (privacy doc), PEND5/6 (catalog and release action, if not already in the PoC).
- The audit.

### (c) Suggested L-tier milestone and budget structure

**Calibration points:**
- bleu negotiated to about $27.5k per L RFP.
- Antumbra asks $43k for the build plus $12k for the auditor; Web3spell asks $48k.
- Argo (XL) asks $67k for the build plus $15k for the audit.

A $45k–$60k L-tier bid is defensible only if the PoC already retires the engine risk. Lead with **$46k–$52k total**, itemise the audit, and make the GUI milestone separable.

| Milestone | Deliverables (each with a pinned tag, a REPRODUCE script and evidence) | Weeks | Payout |
|---|---|---|---|
| M0 Spec, spikes and host-hook upstreaming | Design decision log. Interface spec with the stablecoin team (liquidate_position, cancel_debt, redemption-price normalisation, the Q64.64 → 1e27 scale). PR to lez-programs. v0.3 port plan. Per-op cost baseline. Audit firm booked. | 1 | $4,000 |
| M1 Collateral auction house, production-grade | F1–F3, F6, F7, R1, R3, R6 on chain with real token movement against the real stablecoin. Proptest invariants. IDL drift CI. | 2 | $9,000 |
| M2 Prices and failure isolation | twap_oracle integration, per-type pause (R2/R4/R5), SOFT1 fallback feed, P1/P3 measurements | 1.5 | $6,000 |
| M3 System balance: buffer, debt and surplus auctions | F4/F5 with a mock token and configurable address; freeze via the host; system-balance invariant (R6 part 3) | 1.5 | $6,500 |
| M4 SDK, CLI and private path | Rust SDK/FFI + C++ core module; PRIV1–3, PA1–PA4 (liquidate/bid/settle); CLI; module docs in repo; bot SDK (SOFT3) on v0.3 events | 2 | $8,000 |
| M5 Basecamp mini-app *(separable if the UI moves to its own RFP)* | `ui_qml` + C++ backend: U2, U4, U5; Figma-equivalent screens; own catalog + release action | 1.5 | $6,000 |
| M6 Hardening, deployment and docs | S1 public testnet (v0.3), S2 real-proof e2e green, S4 full matrix, P2 cycle table vs N auctions, PRIVACY.md, README, Kani harness for the SOFT2 invariants | 1.5 | $5,500 |
| M7 Audit remediation | Coordinate the firm, fix findings, re-verify, publish the report | 1 (+ the auditor's lead time) | $3,000 |
| | **Build subtotal** | **12** | **$48,000** |
| Audit line (paid to the firm; offer that Logos contracts the firm directly, as Gateway did) | Named firm (e.g. Zellic / OtterSec / Trail of Bits with Rust/zkVM practice); scope is the engine guest + vault + host hook | — | **~$10–12k, itemised separately or optional** |

**Packaging advice:**
- **Headline $48k, with the audit as a separate optional line.** This lands between Web3spell ($48k, no code) and Antumbra ($55k).
- Offer a reduced variant of **about $42k without M5** in case the UI is split into the RFP-014 UI RFP. In the same breath, say we would bid on that UI RFP together with the Stablecoin UI RFP (#210 wants one studio for both).
- Commit to 12 weeks, with a sequential-fallback note.
- Offer 6 months of support, including SPEL/LEZ version bumps and host-API changes.
- No upfront payment; pay on acceptance per milestone.

**One-developer risk to address explicitly:**
- Antumbra fields two people. We should give each milestone an engineer-week figure.
- Name a backup reviewer or advisor if possible.
- Point to the PoC as proof of throughput.
- Avoid bidding on parallel RFPs; Antumbra's four simultaneous bids are a capacity weakness we should not copy.

---

## 7. Key links
- RFP-014: https://github.com/logos-co/rfp/blob/master/RFPs/RFP-014-liquidation-auction-engine.md. Appendix: https://github.com/logos-co/rfp/blob/master/appendix/appendix-liquidation-auction-ecosystem.md
- Pending template PRs: https://github.com/logos-co/rfp/pull/208, https://github.com/logos-co/rfp/pull/209, https://github.com/logos-co/rfp/pull/45 (draft)
- Proposals: https://github.com/logos-co/rfp/issues/187, https://github.com/logos-co/rfp/issues/159
- Antumbra: https://github.com/edenbd1/antumbra-auction (reset declaration commit `8ab3bdc`; `evidence/pre-reset-hashes.txt`; caller `now` in `programs/auction/src/bin/antumbra_auction.rs`), https://github.com/edenbd1/antumbra-lez (`docs/deploy-v03.md`, `app/README.md`)
- λPrize closures: https://github.com/logos-co/lambda-prize/pull/125, /126, /129
- Web3spell: https://github.com/Web3Spell/Liquidation-Auction-Engine (README only)
- Ecosystem: https://github.com/logos-co/ecosystem/issues/205, /210, /200, /185, /224
- Host: https://github.com/logos-blockchain/lez-programs (`docs/stablecoin/README.md` §12; PRs #385, #401, #333–#340, #399–#400)
- LEZ v0.3.0 release notes (events, fees, #817): https://github.com/logos-blockchain/logos-execution-zone/releases/tag/v0.3.0
