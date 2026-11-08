# RFP-014 competitive upgrade: design

Date: 2026-11-07. Status: approved by the author 2026-11-07 (durations option C).

## 1. Intent

A Logos reviewer should read our RFP-014 bid and conclude it is the best one for LEZ: every hard
requirement cleared with nothing a strict reviewer can mark "partial", and a clear lead on
feasibility and ecosystem impact (safer, more private, more reusable for the next CDP host).

Logos does not award first-come: it filters on mandatory requirements, then judges on merit
(RFP-020 went to the 11th proposal, RFP-003 to the 6th, RFP-004 to the 2nd). So the work is about
closing every scoreable gap and making our evidence the easiest to trust.

Research behind this design (to be committed under `docs/research/` as part of Tier 1):
`2026-11-07-rivals.md`, `2026-11-07-evm-engines.md`, `2026-11-07-non-evm-engines.md`,
`2026-11-07-synthesis.md`.

## 2. Decisions the author made

- D1. All four tiers are in scope (bid documents, engine, evidence, tooling).
- D2. The bid is posted as soon as RFP-014 reopens. Tier 1 must therefore be complete and
  stand on its own; Tiers 2–4 are listed as funded milestones and keep being built while we wait.
  A milestone item finished before posting is marked "delivered early" with its evidence link.
- D3. Price stays **$40,000 all-in, audit included**, 12 weeks. Extra scope is absorbed into the
  existing milestone structure, not priced on top.
- D4. R5: while a collateral type's feed is stale or invalid, **bids on its live auctions continue at
  the last good TWAP**, only while that TWAP is younger than a configurable maximum age and never
  below the auction's floor. Risk-reducing operations (settle, claim leftover, settle debt, owner
  actions) run under every pause level.
- D5. No port to LEZ v0.3. We deploy on public testnet 0.4 the day it exists.
- D6 (constraint carried from Plan 5). All engine changes land in `v04/` only. The v0.2.4 engine
  stays byte-identical so the `rfp014-poc` ImageID is unchanged.

### Settled after the builder check (marked as the designer's call unless stated)

- S1 (follows D2). The posted bid may still carry "PoC (partial)" rows (today R1, R2, R5, S1,
  U6, F5). Each is worded honestly as "partial now, closed in M_n" with the milestone named; the
  §1 "no partials" aim applies to the delivered result. E1 and E2 are built first so that, if
  RFP-014 stays closed long enough, R2 and R5 flip to "delivered early" before posting.
- S2 (designer's call). Shared `crates/auction_core` may grow, but every new item goes behind a
  cargo feature (e.g. `v04-ext`) that the v0.2.4 program never enables, so v0.2.4 guest bytes
  cannot change; `guests.yml` byte-compare stays the proof. Existing functions keep their
  behaviour. This keeps the "one reusable pure core" claim true.
- S3 (designer's call). The 0.4 engine (`v04/`) becomes the primary engine in README and
  REQUIREMENTS. REQUIREMENTS gets separate evidence columns for v04 and v0.2.4. The v0.2.4
  engine is kept as the public-chain evidence (legacy testnet run) and as history.
- S4 (designer's call, follows the anti-goal "no rival named"). The rival analysis and the
  synthesis stay **out of the public repo** (kept in the author's private notes). Only the
  ecosystem research (`evm-engines`, `non-evm-engines`) is committed. The pricing note
  cites protocols, never rivals.
- S5 (designer's call). Commits are GPG-signed by the author. The plan batches commits so the
  author can sign them in a few sittings (a `make-commits.sh`-style script, as in Plan 6/7).

## 3. Goals, non-goals, anti-goals

Goals
- Pass the hard-requirement filter of the current RFP text **and** the pending PR #229 text
  (new F8, tightened F3, dependency-naming clause).
- Close the three places Antumbra leads on substance: R2 (TWAP / confidence-adjusted price),
  R5 (operations continue under a pause), R4 (two types isolated inside one instance, on chain).
- Match or beat their evidence presentation: one-command verifier, refusal-code catalogue,
  honest-docs CI gates, reproducible guest build.
- Add the best-in-class safety mechanisms the ecosystem research found, where in RFP scope.

Non-goals
- A v0.3 deployment (D5). Changing v0.2.4 code (D6).
- Anything the RFP lists out of scope: flash-loan liquidations, aggregators/batching,
  insurance/socialised loss, multi-collateral atomic liquidations.
- Mechanisms the research rejected for LEZ: sealed-bid / commit-reveal zk auctions, stability
  pools, on-chain standing-bid queues, backstop vaults, LLAMMA soft liquidation, kicker bonds.

Anti-goals (failure even if it "works")
- Any claim in the bid that the repo cannot prove with a test, run ID or on-chain record.
- Attacking the rival by name in the proposal. Contrast is made by stating our design choices
  and their reasons; the reviewer draws the comparison.
- Any Claude / AI attribution in commits, PRs or docs (standing rule for this user).
- A price adjustment or new milestone that breaks the $40k / 12-week envelope.

## 4. Tier 1 — bid documents (do first; no engine code)

Lands in the repo on one branch (`docs/rfp014-bid-refresh`), plus edits to the untracked proposal
draft `../lez-auction-engine/docs/outreach/rfp014-proposal-draft.md`.

T1.1 Ecosystem research committed: `evm-engines` and `non-evm-engines` under `docs/research/`,
with a header saying which claims are secondary-sourced (both reports flag these). The rival
analysis and the synthesis are not committed (S4).

T1.2 Dependency / capability table (PR #229 clause), in the proposal and in `README.md`:
one row per capability — price feed, host CDP, collateral/stable token authorities,
admin authority (RFP-001), freeze authority (RFP-002), runtime (LEZ version), framework (SPEL) —
with: implementation we build against today, version/commit, what we do if it is late,
and how an interface change is absorbed (the adapter/hook boundary that isolates it).

T1.3 `REQUIREMENTS.md` gains rows for F8 and for F3 as reworded in PR #229. F8: list every
constant in the 0.4 engine and show it is either an init parameter or a hard *safety bound*
on an init parameter (e.g. max bps), with the reason. Any value that is really a production
default hidden in code becomes a Tier 2 item. F3: a written justification for `BelowMinBid`
and `DustRemainder` (dust lots cost more to settle than they raise; Maker `chost` precedent), and
show that a bid for the **whole remaining lot** is always accepted regardless of size.

T1.4 `README.md` refresh: current status (0.4 port, Plans 5–7 merged, hosted CI run IDs),
correct test counts generated by a script rather than typed (≈1,569 `#[test]` today; split
v0.2.4 / v04 / integration / QML), deployments, how to verify.

T1.5 `docs/architecture/refusal-codes.md`: every engine error variant (v0.2.4 and v04) with its
numeric code, the instruction(s) that raise it, the condition, and the test that proves it.
Generated or checked by a script so it cannot drift.

T1.6 Design note `docs/architecture/pricing-model.md`:
- live oracle pricing on every bid versus a price fixed at auction open — why ours needs no
  restart/expiry machinery (Maker `redo`/`cusp`/`tail`) and cannot get stuck when the market
  moves past the maximum discount, and cannot sell far below market when it rises;
- the R5 rule (D4) and why bounded last-good TWAP is safer than either refusing bids or
  freezing the open price;
- oracle-extractable value: the rising discount auctions it back; the capped trigger reward
  is the only fixed prize;
- the cycle-cost note: our ~840k-cycle liquidation on 0.4 includes real token transfers,
  custody and buffer accounting; what a counters-only implementation would cost by comparison.

T1.7 Proposal draft rewrite:
- correct numbers (tests, cycles, run IDs), the dependency table, the "what we do that others
  don't" section stated as design choices (no rival named);
- milestones re-mapped per §8 with "delivered early" markers;
- an explicit item list for the author: audit quotes and named firm, video upload, every
  `[FILL]`.

## 5. Tier 2 — engine (v04 only). Outline; gets its own design doc.

Fixed by this design:
- E1 R2 price: on-chain TWAP (observation ring buffer, configurable window and minimum
  observations) and confidence-adjusted pricing (collateral valued at the protocol-favourable
  edge of the confidence band; reject above max confidence). Liquidation triggers and bids use it.
- E2 R5 / pause levels: stepped pause per collateral type — level 1 no new liquidations,
  level 2 also no new system auctions, level 3 also no bids. A stale/invalid feed behaves per D4.
  Risk-reducing operations always run. Every operation × every level gets a test.
- E3 Oracle safety: spot-vs-TWAP deviation breaker; a reading pinned at the feed's min/max
  counts as invalid; a grace period after a sequencer or feed gap blocks **new liquidations
  only** (never bids on live auctions).
- E4 Health-aware discount: the starting (and maximum) discount is bounded by how far underwater
  the position was, so a barely-unsafe position is not sold at full discount.
- E5 Chunked partial liquidation (per-liquidation quantity limit) — closes the ADR-012 gap where
  one oversized auction holds the global cap.
- E6 A stairstep schedule preset whose step length exceeds the measured private proving time,
  with a test; documented as the recommended setting for privacy-heavy deployments.
- E7 Deficit ledger with a minimum write-off amount; rolling-window mint cap on debt auctions.
- E8 Parameter changes that loosen safety wait a delay and emit an event; tightening is instant.
- E9 Any F8 findings from T1.3. The audit (`docs/architecture/f8-parameters.md`) found five
  values fixed in code that the admin cannot set (`production default — Tier 2`); each becomes
  an init parameter with today's value as its default:
  - `MAX_CONF_BPS` (1_000 bps) as the reference oracle's confidence limit (`price.rs:31`);
  - `REF_ORACLE_CLAMP_BPS` (±2_000 bps) reference-oracle clamp around the admin price
    (`price.rs:32`);
  - `DEBT_RELIST_DELAY_MS` (600_000 ms) debt-auction relist delay (`args.rs:20`);
  - the feed sanity window, market price within [R / 2, 2 · R] (inline, `price.rs:79-80`);
  - the reference-oracle scale, hardcoded to `OracleScale::Ray` (`programs/auction_engine/src/system.rs:44-50`), default Ray.

Left to that design: data layouts, exact parameters and defaults, error codes, ADR numbers
(continue from ADR-067), cycle budgets.

## 6. Tier 3 — evidence. Outline; own design doc.

- V1 Manifest-based verifier (`scripts/verify-onchain.*`, curl + python only): per-run manifest
  of applied and refused transactions and state checks; checks refused transactions changed no
  state (account hash before/after), and that an unknown random hash does not resolve. Runs
  against the legacy v0.2.4 testnet run, the local 0.4 sequencer, and in `v04-e2e` CI.
- V2 Reproducible v04 guest build (Docker), byte-compared in CI like `guests.yml` does for v0.2.4.
- V3 Honest-docs CI gates: README test counts vs cargo, every repo path and link in docs exists,
  cycle tables diffed against measurement, refusal-code catalogue in sync.
- V4 Incident-replay tests: Black Thursday zero bids, feed stuck at floor (LUNA), single bad
  print, self-liquidation, debt-auction restart loop, dust farm; plus an invariants document in
  Certora-rule style linked to the Kani/proptest harnesses, and a sequence fuzz.
- V5 R4 on chain: two collateral types in one instance in the e2e, one paused, the other
  liquidating and bidding.
- V6 Concurrent-auction benchmark and a shared-account contention audit (every bid writes the
  buffer and collateral config today).

## 7. Tier 4 — tooling. Outline; own design doc.

- K1 Reference keeper bot on the SDK with a break-even / profitability estimator (fees, proving
  time, discount path).
- K2 Private-mode SDK safeguards: rounded deshield amounts, one-use key tombstones, reshield
  recovery journal, a "what becomes public" preview / privacy meter in CLI and Basecamp.
- K3 Client-side conditional bids ("bid when discount ≥ X") and local-only alerts in CLI and
  Basecamp (no third-party service learns positions).
- K4 (stretch) leftover claim by proof of a secret bound at liquidation — needs a host-hook
  change, so it is written as an upstream proposal, not built unilaterally.

## 8. Milestone mapping (inside $40k / 12 weeks; payouts unchanged)

| Milestone | Payout | Adds from this design |
|---|---|---|
| M0 alignment | $2,000 | T1.2–T1.5 already delivered (marked early); F8 audit result |
| M1 collateral auctions | $6,000 | E2, E4, E5, E6, V4, V5 |
| M2 price feeds | $4,000 | E1, E3, E9 |
| M3 debt & surplus | $3,000 | E7, E8 |
| M4 private path + bot SDK | $6,000 | K1, K2, K3 |
| M5 Basecamp mini-app | $4,000 | K2/K3 surfaces in the GUI |
| M6 testnet 0.4 | $5,000 | V1 on public 0.4, V2, V6 |
| M7 audit | $10,000 | unchanged |

V1 (on legacy testnet + local 0.4) and V3 are done before posting where possible, marked early.

**Durations (author's decision C, 2026-11-07): rebalance weeks inside 12 and name pre-built work.**
Weeks move from milestones whose work is largely already built to the ones this design loads up.
Payouts stay as in the table. Proposed split (the designer's call; T1.7 may adjust by ±0.5 wk while
the total stays 12):

| M0 | M1 | M2 | M3 | M4 | M5 | M6 | M7 | Total |
|---|---|---|---|---|---|---|---|---|
| 0.5 | 2.5 | 1.5 | 1 | 2.5 | 0.5 | 1.5 | 2 | 12 |

Rationale: M5's Basecamp app (162 QML tests, 172-step story) and M6's actor-model port are already
built, so they shrink. M1 and M4 carry most of the new scope, so they grow. Every milestone in the
bid has a "**Built before the grant**" line listing the items already delivered, each with its
evidence link, so the remaining weeks visibly cover only what's left.

## 9. Order of work

1. Tier 1 (this design → plan → build). Target: postable bid.
2. Tier 2 design → plan → build, E1 and E2 first (they decide R2/R5 scoring).
3. Tier 3 (V1 and V3 can start in parallel with Tier 2; V4/V5 follow the engine changes).
4. Tier 4.
After each tier: update REQUIREMENTS, proposal draft and "delivered early" markers.

## 10. Constraints and working rules

- Signed commits (GPG, user passphrase); never bypass signing. Fast-forward or merge commits,
  never squash/rebase merges. No push or post without the author's go-ahead.
- No Claude / AI attribution anywhere.
- Subagent-driven development with tests, review and security review per task (as Plans 1–7).
- Heavy builds run in WSL (memory limits; see the project's host notes).

## 11. Left to the builder

File layout of new docs, script language for generators/checkers (prefer what `scripts/`
already uses), exact wording of the proposal sections, how test counts are split, the order of
tasks inside Tier 1.
