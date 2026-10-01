# Design Log

Every spike outcome is appended here as a new ADR or an amendment.

## ADR-001: Track B Toolchain

**Context**
RFP-014 was developed against LEZ v0.2.4, SPEL `1ef0500f`, and Rust 1.94.0. The public testnet moved to v0.3.0 on 2026-10-01, but SPEL has no v0.3 support yet, and the RFP requires SPEL. Using Track B (v0.2.4) lets us deliver the PoC now.

**Options**
- (A) Track B: build against LEZ v0.2.4, SPEL, and deliver on v0.2.4 logic.
- (B) Track A: wait for SPEL v0.3 support; deliver to the public testnet.

**Decision**
Use Track B with a pure core crate (`auction_core`) that is LEZ-independent. All on-chain logic lives in `auction_core`, and the SDK keeps an encoding seam so the v0.3 port is rewritable in the program layer only.

**Trade-offs**
The public testnet deployment is blocked until SPEL supports v0.3. The v0.3 port is planned in `docs/v03-port-plan.md` and assigned to milestone M6.

**Status**
Accepted (spec v5)

---

## ADR-002: Standalone Repo with Upstream Hooks

**Context**
The engine uses three new LEZ program instructions in the stablecoin: `set_liquidation_engine`, `liquidate_position`, and `engine_freeze_new_debt`. These are offered upstream as a PR to the stablecoin (lez-programs).

**Options**
- (A) Embed a vendored copy of the stablecoin and modify it locally.
- (B) Vendor the stablecoin cleanly, offer hooks upstream as a PR, and integrate them locally after acceptance.

**Decision**
Vendor lez-programs `64c0120` as-is, append the new instructions to both the enum and guest functions (keeping IDL order intact per spel-cli fact §3.2), and offer the hooks upstream. The PoC runs with the vendored+patched version while the PR is reviewed.

**Trade-offs**
The hook placement is inflexible: new instructions must append, not insert, so the enum index equals the IDL index.

**Status**
Accepted (spec v5)

---

## ADR-003: GEB Increasing-Discount Auction with Maker Rails

**Context**
The collateral auction (F6) needs a pricing model that is both efficient (single-pass discount evaluation) and robust to parameter changes (restarting loses nothing). Reflexer's GEB uses an increasing-discount design; Maker uses caps and dust rules for safety.

**Options**
- (A) Maker-style fixed-price auction with expiry and restart.
- (B) GEB-style increasing Dutch auction with no restart and no expiry.
- (C) Hybrid: GEB pricing, Maker safety rules (caps, dust, keeper reward carved from penalty).

**Decision**
Use GEB increasing-discount design (no restart, no expiry) combined with Maker safety rails: keeper reward carved from the penalty (not extra), caps that reserve collateral (with an oversized-alone rule), dust rules per minimum bid and debt floor, and slippage bounds on every bid.

**Trade-offs**
An unsold auction keeps its cap reservation indefinitely; the lot sits at max discount. This is visible in the CLI/GUI and documented in `THREAT_MODEL.md`. Partial liquidation (M1) reduces this impact.

**Status**
Accepted (spec v5)

---

## ADR-004: LiquidationRecord Nonce Addressing

**Context**
Auction PDAs must be precomputable at signing time (§3.2). A position can be liquidated multiple times, and accounts cannot be released. A clock-based seed is unknowable at signing time.

**Options**
- (A) Use a global sequence counter; concurrent keepers would collide.
- (B) Use a per-position counter in a `LiquidationRecord` PDA; increment on each liquidation.
- (C) Use a hash of (position, timestamp); breaks at signing time.

**Decision**
Create a `LiquidationRecord` PDA on first use (Claim::Pda), keyed by position. Liquidation asserts `nonce == record.count`, increments, and uses the tuple `(position, nonce)` in the auction seed. This lets the keeper precompute the auction address before signing.

**Trade-offs**
None significant. The record is created once and persists.

**Status**
Accepted (spec v5)

---

## ADR-005: Band-Clamped Market Price with Strict Validity

**Context**
The oracle provides collateral per stablecoin. The price must be fresh, confident, and not wildly inverted. No fallback is acceptable (RFP R2).

**Options**
- (A) Use the raw oracle price; accept staleness.
- (B) Fall back to the redemption price R if the oracle is stale.
- (C) Validate freshness, confidence, and a band `[R/2, 2R]`; pause if invalid.

**Decision**
Validate:
- Age: `now − M.timestamp ≤ max_oracle_age_ms`
- Confidence: `M.confidence_interval · 1e4 ≤ max_conf_bps · M.price`
- Band: `R/2 ≤ M.price ≤ 2R` (detects mis-scaled or inverted feeds)

Clamp to the band: `P = clamp(M.price, R·lo_bps/1e4, R·hi_bps/1e4)` with defaults `lo = 9000, hi = 11000`.

If any check fails, pause the instance. No fallback.

**Trade-offs**
The confidence check never triggers with producers that write 0 (documented in `REQUIREMENTS.md`). TWAP live in M2 once the scale question is settled upstream.

**Status**
Accepted (spec v5)

---

## ADR-006: Piecewise-Linear Discount Schedule

**Context**
Discount rates evolve over auction time. The schedule must be efficient (computed on every bid) and safe (bounded, no sudden changes).

**Options**
- (A) Exponential decay: fast convergence, hard to reason about.
- (B) Stairstep: discrete jumps, griefable at transitions.
- (C) Piecewise-linear: simple, predictable, snapshots well.

**Decision**
Use a piecewise-linear table of up to 8 `(elapsed_ms, bps)` points. Time is strictly increasing, discount non-decreasing. Hard max is 3000 bps. The schedule is snapshotted into each auction. Discount `d` is interpolated in RAY and rounded down. Default (RAI): 2% → 10% over 45 min, then held.

**Trade-offs**
Eight points is a conservative limit to avoid griefing via complexity.

**Status**
Accepted (spec v5)

---

## ADR-007: Type-Level Pause Clock with Frozen Discount

**Context**
An invalid oracle feed should pause liquidations without pausing the whole stablecoin. Pauses may end and resume. The discount should not advance while paused.

**Options**
- (A) Global pause; affects all collateral types.
- (B) Per-type pause with a clock; lets other types continue.

**Decision**
Pause clock lives in `CollateralConfig` (per type, and there is one type per host). Effective elapsed time is:
`t_eff = (now − start) − (paused_total − auction.type_paused_total_at_start) − (paused_since ? now − max(paused_since, start) : 0)`, floored at 0.

When the feed becomes invalid, start the pause clock. When it becomes valid again and there is no admin pause, close the clock.

When a stale period starts (onset, decision in spike S5):
- (a) at observation time; or
- (b) at `last_seen_oracle_ts + max_age`; or
- (c) (b) bounded by the oracle's history.

Acceptance: a property test shows that adversaries controlling oracle-update or observation timing get no more discount than the schedule gives over unpaused time, within one block.

**Trade-offs**
Onset precision is a residual risk documented in `THREAT_MODEL.md`.

**Status**
Pending spike S5

---

## ADR-008: No On-Chain Auction List; Block-Scan Discovery

**Context**
Clients need to find live auctions. An on-chain list could be griefed into exhaustion and costs cycles to maintain.

**Options**
- (A) Keep a `live_auctions` list on-chain; expensive and griefable.
- (B) Clients scan blocks for transactions listing `EngineConfig`; O(blocks) off-chain.

**Decision**
No on-chain list. Every `liquidate` includes `EngineConfig` in its accounts, so the block scanner keeps transactions that mention it and derives auction PDAs from `(position, nonce)`, reading status on-demand.

**Trade-offs**
Off-chain work; no real-time push. Mitigated by a UI/CLI "Rescan" button and a persisted cursor tracking the last scanned block.

**Status**
Accepted (spec v5)

---

## ADR-009: Nonce-Based Transaction Confirmation

**Context**
LEZ v0.2.4 has no `poll_transaction_status` (that arrives in v0.3). We need to know if a submission succeeded or was dropped.

**Options**
- (A) Sleep and retry; unreliable.
- (B) Poll the signer nonce; indirect but available in v0.2.4.
- (C) Use an indexer; not available to the PoC.

**Decision**
Record signer nonce `n0` and block height `h0`. Submit, then poll the nonce every 2 s.
- `nonce > n0` → **Included**.
- Still at `n0` at `h0 + 5` → **Dropped** (re-quote and resubmit).

**Trade-offs**
Switches to `poll_transaction_status` after v0.3 port.

**Status**
Accepted (spec v5)

---

## ADR-010: Debt-First Payment; Penalty to Buffer

**Context**
When a bidder fills an auction, they pay with stablecoin (burn debt) and collateral goes out. Surplus goes to the protocol. How is the payment split?

**Options**
- (A) Penalty first, then debt: surplus accrues early.
- (B) Debt first, then penalty: simpler accounting, penalty feeds the buffer.

**Decision**
Debt-first:
1. Burn the debt part from the bidder.
2. Transfer the penalty part to `SurplusBuffer`.
3. Transfer collateral to the bidder.

Accounting is engine-only; the buffer is a single stablecoin holding outside the host's accounting.

**Trade-offs**
None significant.

**Status**
Accepted (spec v5)

---

## ADR-011: UI on Basecamp Color Grid and AppManagerView Layout

**Context**
The GUI is built on Basecamp's design system. It must respect the theme and fit the AppManager layout.

**Options**
- (A) Custom colors; won't integrate.
- (B) Use Basecamp's `Logos.Theme` and `Logos.Controls` tokens; integrates cleanly.

**Decision**
Build on Basecamp's color grid and AppManagerView layout. The primary button has a black label (audit outcome).

**Trade-offs**
Constrained by existing Basecamp design; no custom brand colors.

**Status**
Accepted (spec v5)

---

## ADR-012: Full Seizure with Oversized-Alone Rule; No Restart or Expiry

**Context**
An auction can be larger than the type cap. Should it be blocked? Should unsold lots expire?

**Options**
- (A) Reject oversized positions; limits usability.
- (B) Allow oversized but hold both caps; caps themselves become restrictive.
- (C) Allow oversized alone (if all existing caps are free, liquidate alone).

**Decision**
(C): An oversized position `(amount_to_raise > cap − reserved)` is allowed only if both type and global reservations are 0 (it goes alone). When it settles, it releases its (over-size) reservation. There is no restart: a fixed starting price would mean restarting just resets the discount to the start, pointless. There is no expiry: `redo` in Maker exists because Maker fixes the starting price; we don't. Expiry would book a shortfall while unsold collateral exists (minting against recoverable debt, dilution without loss). Instead, unsold auctions sit at max discount until settled.

Vault sweep (full seizure only): on full seizure, transfer the **whole** vault balance (not just the lot), sweeping any donations. Shortfall is booked only when the lot is exhausted.

**Trade-offs**
One oversized auction at a time can hold the global cap. This is documented in `THREAT_MODEL.md` as an accepted residual. Partial liquidation (M1) reduces the impact.

**Status**
Accepted (spec v5)

---

## ADR-013: Engine-First PoC with Thin GUI; F4/F5 in the PoC

**Context**
The RFP and the timeline push for early engagement. The UI is "a separate RFP" (ecosystem#210). The engine and F4/F5 are the differentiators competitors have not built.

**Options**
- (A) Full GUI in the PoC; 2–3 weeks for UI polish.
- (B) CLI only; no visual proof.
- (C) Engine + F4/F5 + thin GUI; 2 days for the Live view and bid flow.

**Decision**
(C): Build the engine first, then F4/F5 (debt and surplus auctions with mock token), then a thin GUI showing the Live auctions view and bid flow. The At-risk positions view, full confirmation dialog, and state variants are designed (in `docs/design/ui/`) but unbuilt; they move to M5 or the separate UI RFP. The CLI carries the full PoC demo.

**Trade-offs**
The GUI is not a showcase; the CLI demo is. If schedule slips, the GUI is cut first (cut order ADR-015).

**Status**
Accepted (spec v5)

---

## ADR-014: Pricing at ~$40k; Separate Audit Line

**Context**
Two open proposals bid $48k and $55k. bleu won RFP-015 (L) and RFP-016 (XL) for $55k combined after negotiation, by filing early. A lower price and early filing may improve odds.

**Options**
- (A) Price at $40k, no audit; accept audit risk.
- (B) Price at $45k, include light audit; mid-range.
- (C) Price at ~$40k + separate audit line ($10–12k) named to a tier-1 firm that Logos may contract directly.

**Decision**
(C): Build price ~$40k over 12 weeks (from §15 milestone table), separate audit line ~$10–12k naming a tier-1 firm that Logos may contract directly. A variant without M5 (the GUI) comes to ~$35k. This price is below both open bids ($48k, $55k).

**Trade-offs**
None.

**Status**
Accepted (spec v5)

---

## ADR-015: Cut Order: GUI, Then Two-Instance Test, Then Kani

**Context**
If the schedule slips, some scope must be descoped. What order?

**Options**
- (A) Cut core first; lose the point of the PoC.
- (B) Cut systematically: non-critical UI, then tests, then tooling.

**Decision**
Never cut: engine, hooks, F4/F5, CLI, real-proof CI.
Cut in order:
1. GUI: the CLI demo carries U2–U5, and full GUI moves to M5.
2. Two-instance R4/R5 test: single-instance pause and per-type tests stay; two-instance moves to M2.
3. Kani: proptest and differential model remain; Kani moves to M6.

**Trade-offs**
The R4/R5 test and Kani are not on the critical path.

**Status**
Accepted (spec v5)

---

## ADR-016: Final Partial-Capable Hook Shape

**Context**
Partial liquidation (M1) requires the hook to accept amounts smaller than the full position. The final shape must be future-proof.

**Options**
- (A) `liquidate_position(pc)`: full only. Breaks at M1.
- (B) `liquidate_position(collateral_to_seize, normalized_debt_to_cover)`: supports any amounts now.

**Decision**
(B): The PoC passes full amounts `(collateral_amount, normalized_debt)`, so M1 is an engine-only change with no second upstream PR. The host enforces a proportionality bound (owner protection independent of engine correctness):
`collateral_to_seize ≤ min(Position.collateral_amount, ceil(Position.collateral_amount · normalized_debt_to_cover / Position.normalized_debt · (1e4 + MAX_PENALTY_BPS) / 1e4))`

Vault sweep applies on full seizure only: transfer the entire vault balance (sweeping donations).

**Trade-offs**
None; this is the final shape.

**Status**
Accepted (spec v5)

---

## ADR-017: F4/F5 Safety: Absolute Mint Cap, Oracle Clamp, Logged Prices

**Context**
F4 (debt auction) and F5 (surplus auction) need to start protocol-token minting and burning safely. The mint cap and oracle behavior are critical.

**Options**
- (A) Derive mint cap from price; oracle moves break the cap.
- (B) Absolute mint cap, optional oracle with fallback, logged admin price changes.

**Decision**
(B): An absolute mint cap (demo: 5,000 mock tokens) set at init and never price-derived. The `protocol_token_ref_price` is an admin parameter (RAY). Optionally, if `protocol_token_oracle_id` is set and valid, override it, clamped to ±20% of the admin price (a manipulated oracle cannot move it further). An invalid optional oracle falls back to the admin price; it does not pause.

The `bad_debt_freeze_threshold` is an absolute value set by the admin, default 1,000 LSC, bounded to ≥ `debt_lot`. This is an interpretation of F4.

`check_solvency` freezes only when `unbacked_debt > bad_debt_freeze_threshold` AND one of:
- (a) `protocol_token.is_none()` (no protocol token configured); or
- (b) `freeze_on_bad_debt` is true, no debt auction is live, and `start_debt_auction` would revert with exactly one of: `MintCapConfig`, `MintAuthorityMissing`, or `ProtocolTokenMismatch`.

If a debt auction is live, `check_solvency` reverts with `RecoveryInProgress` and freezes nothing. With a protocol token configured and working, `check_solvency` never freezes, even if debt auctions keep closing untaken.

**Trade-offs**
None significant. This matches the RFP and is documented in `THREAT_MODEL.md` as an accepted residual.

**Status**
Accepted (spec v5)
