# Design Log

Every spike outcome is appended here as a new ADR or an amendment.

ADRs are numbered in decision order and appear in the order they were written, so the numbering is not always monotonic in the file (ADR-028, ADR-029 and ADR-031 appear after ADR-030; ADR-035 appears after ADR-036).

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
The collateral auction (F6) needs a pricing model that is both efficient (single-pass discount evaluation) and robust to parameter changes (the price is re-read at every bid, so a restart would only reset the discount from its maximum back to the start). Reflexer's GEB uses an increasing-discount design; Maker uses caps and dust rules for safety.

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
Accepted (S5): `DerivedFromOracle`, option (b). Assumes every t_eff evaluation is preceded by a pause observation at the same instant; see docs/spikes/S5.md

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
(C): An oversized position `(amount_to_raise > cap − reserved)` is allowed only if both type and global reservations are 0 (it goes alone). When it settles, it releases its (over-size) reservation. There is no restart. The price is re-read at every bid, so a restart would only reset the discount from its maximum back to the start. There is no expiry: `redo` in Maker exists because Maker fixes the starting price; we don't. Expiry would book a shortfall while unsold collateral exists (minting against recoverable debt, dilution without loss). Instead, unsold auctions sit at max discount until settled.

Vault sweep (full seizure only): the hook requires vault balance ≥ position collateral (not equal); on full seizure, transfer the **whole** vault balance, sweeping any donations. Shortfall is booked only when the lot is exhausted.

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
Open bids are #159 Web3spell $48k and #187 Antumbra $55k. bleu won RFP-015 (L) and RFP-016 (XL) for $55k combined after negotiation.

**Options**
- (A) ~$38–42k build + separate audit line (~$10–12k, tier-1 firm that Logos contracts) — below both open bids without matching bleu's combined-RFP discount. **Chosen.**
- (B) bleu-level ~$28–32k — matches the bleu precedent; most competitive but thin margin for a solo developer over 12 weeks.
- (C) Keep the earlier $48k draft — priced at the open bids.

**Decision**
(A): Build price ~$38–42k (planned ~$40k) over 12 weeks, separate audit line ~$10–12k named to a tier-1 firm that Logos may contract directly. A variant without M5 (the GUI) comes to ~$35k.

**Trade-offs**
Lower than open bids improves competitiveness but leaves less margin for a solo developer over 12 weeks. The audit is separate so its cost does not distort the build price.

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

**Update (Plan 1, Task 12)**
The Kani proofs are written (`crates/auction_core/src/kani_proofs.rs`, behind `cfg(kani)`) but were not run in Plan 1: Kani does not support Windows, the development platform. Execution moves to M6 per this ADR; the proptest suite and the Python differential model remain the Plan 1 gates.

Kani `schedule_bounded` unwind(3) unverified; revisit at M6.

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

*Amended by ADR-026 (Task 5 review): the rounding is `floor`, not `ceil`.*

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
With a working protocol token configured, `check_solvency` never freezes, even if debt auctions keep closing untaken (accepted residual recorded in `THREAT_MODEL.md`; governance monitors). The freeze threshold departs from F4's literal "exceeds the buffer" wording but prevents one-unit rounding errors from freezing the system.
- `start_amount == 0` is reported as `MintCapConfig` and is therefore freeze-eligible (ADR-024).

**Status**
Accepted (spec v5)

---

## ADR-018: S1 Outcome: Toolchain, Multi-Instance Build, Chained-Call Budget

**Context**
Spike S1 (docs/spikes/S1.md) ran the pinned toolchain end to end: logos-execution-zone v0.2.4 (`47eba256`), lez-programs `64c01210`, risc0-zkvm 3.0.5.

**Decision**
- Confirmed commands: Docker guest build `make build-programs` (output `target/guest/*.bin`, deterministic; token and twap_oracle ImageIDs equal the live testnet ones), standalone sequencer `sequencer_service <config> --home .. -p 3040` with `RISC0_DEV_MODE=1`, wallet `deploy-program`, `spel` for instructions, offscreen Basecamp bundle with the inspector on 3768. Caveat: the inspector answered on :3768, but lez_core load into Basecamp is NOT verified (0 `lez_core` matches in logs/basecamp*.log although s7/user/modules/lez_core existed); Plan 4 must first confirm it loads. The Basecamp revision built was not recorded (floating ref; bundle store path in S1.md). Details and environment workarounds are in S1.md and `spikes/s1/run.sh`.
- Second host build: `#[used] static HOST_INSTANCE_TAG: [u8; 8] = *b"INST-B01";` in the stablecoin guest changes the ImageID (A `c655d477...`, B `6032adb2...`); other guests stay byte-identical. Two instances run independently with separate PDAs and oracles.
- Chained-call budget: `MAX_NUMBER_CHAINED_CALLS = 10`; public txs may run 11 programs, private 10. Modelled liquidate (hook, init-holding, withdraw_collateral, token transfer) = 4 executions, 865,665 cycles excluding the hook. Re-measure with the real hook in Plan 3.
- Module-builder pin for Plan 4 (from lez-programs/flake.lock; absent from the Basecamp master lock, so not proven to match the Basecamp build): `logos-module-builder` `6ef42ea8661121831ece79e6b702e27ac1cf46e7`, with lez_core `acf0cd501b262c4c15969e3735e85318297b85bf`.
- Operational constraints: signers must be `auth-transfer init`-ed before first use (a default-owned account can sign a stablecoin tx only once); spel at this rev cannot encode i128. Consequence: the stablecoin controller gains are i128; the u128 IDL copy only works for non-negative values (0 was used for both instances), so Plan 2/3 clients or tests that need signed gains need another encoder or a spel fix.

**Trade-offs**
Heavy, slow builds (about 70 min cold guests, 75 min lez_core from source); network flakiness needed retries. The hook cost is not yet measured.

**Status**
Accepted (S1)


---

## ADR-019: S2 Outcome: Host Authentication of the Engine via Caller Program ID plus Engine Authority PDA (Variant C)

**Context**
Spike S2 (docs/spikes/S2.md) compared three guards for calls from the host program into engine-only entry points (`liquidate_position`, `engine_freeze_new_debt`): A, `caller_program_id == stored engine program id`; B, the stored engine authority PDA is authorised in the call; C, both. Tested in-process on lee v0.2.4 (`47eba256`) with SPEL `1ef0500f` and risc0-zkvm 3.0.5. The brief's four tests (legitimate call, direct call, impostor program, user-signed tx listing the PDA) all pass for all three variants and originally selected B. Fix round 1 added a fifth attack, authorisation laundering: lee propagates authorisation down the call chain (`validated_state_diff/mod.rs:262-290`, monotonically growing authorised set), so if the engine chains with the `ENGINE_AUTH` seeds to any program X, X can chain to `callee.guarded_*` with the PDA authorised and B accepts it. The spike's `call_guarded` takes a caller-supplied program id, which makes this reachable. Test results: B accepts the laundered call (the bypass); A and C reject it with `guard A: caller program is not the engine program`.

**Decision**
Variant C (A and B together). The brief's decision rule ("cheapest variant passing all four tests") rested on an incomplete attack set and is superseded by the spec's requirement that only the engine can invoke the hook. A's check uses the immediate caller, which lee enforces per call (`MismatchedCallerProgramId`, `validated_state_diff/mod.rs:184-191`). Cycles of the legitimate transaction (caller + callee, 2 executions): A 367,644, B 372,606, C 372,891; C costs +285 over B (+0.08%) and +5,247 over A (+1.43%).
B's guarantee, stated correctly: the engine authority PDA is authorised somewhere in the chain that led to the call; it does not say the engine is the caller.
Invariants for Plan 2 (defence in depth): (i) the engine never authorises `ENGINE_AUTH` for any program other than the stored host program id (no caller-supplied program ids); (ii) the host never chains to an untrusted or user-chosen program while the engine authority is authorised in its call.
(iii) Confused deputy: every engine instruction that supplies the `ENGINE_AUTH` seeds derives the callee program id and the hook arguments (amounts) only from engine-validated on-chain state read in the same transaction. Any caller-supplied account id (e.g. `position`) is re-derived or checked against `EngineConfig` (host program id, `ProtocolParameters`, stablecoin definition) before it is forwarded. `ENGINE_AUTH` seeds are supplied only on calls to the stored host program. Every other program reachable later in the same transaction (token) is a pinned program id, never user-chosen, because lee keeps an authorised account authorised for later calls in the chain.

**Trade-offs**
- Zero ids: SPEL sets `ctx.caller_program_id` to zeros for top-level calls (`spel-framework-core/src/context.rs:30-33`), so A and C accept a direct call if the stored `engine_program_id` is zero or uninitialised. The config initialiser must reject zero program ids and zero authorities (Plan 2 test). The config was inserted with `force_insert_account`; creation and rotation are untested.
- Private path untested, wider scope: in the circuit `authorized_accounts` is one set for the whole transaction (`privacy_preserving_circuit/src/execution_state.rs:54,601-608`), so laundering reaches sibling calls too; forgery is still blocked (`output.rs:38`, `circuit/mod.rs:420-433`, `execution_state.rs:285-305`); a PDA first seen inside a chained call is not seed-checked in the Vacant branch, so the engine must list the authority in its own accounts. `caller_program_id` is enforced in the circuit per call (`execution_state.rs:164-170`), so C's immediate-caller check holds there, but this is from source reading only; a private-path test is an open item for Plan 2.
- Both A and B bind to the engine ImageID (PDA derived from the program id): a second or upgraded engine needs its own stored config.
- Upstream fit of B: the stablecoin uses plain `is_authorized` assertions (`lez-programs/programs/stablecoin/src/checks.rs:109`, `src/freeze.rs:58`); those have the same laundering property when the authority is a PDA. The upstream hook PR must either add an immediate-caller check to those paths or document that admin and freeze authorities must stay user keys.
- The brief's `guarded(expected_caller)` argument was replaced by a stored config (an argument chosen by the caller is not a guard). Lee rejects forged flags itself (`InvalidAccountAuthorization`), which stopped the forgery attacks on B.
- Cycles are executor cycles (dev mode, natively built guests), not proving cost. Nothing was run on a sequencer. Evidence excerpt: `spikes/s2_s3_s7/logs/final-excerpt.log`.

**Status**
Accepted (S2, revised in fix round 1: B superseded by C)

---

## ADR-020: S3 Outcome: Arg-Seeded PDAs Created on First Use

**Context**
Spike S3 (docs/spikes/S3.md) checked whether SPEL at `1ef0500f` can create `["LIQREC", config, position]` on first use with `Claim::Pda` and derive `["AUCTION", config, position, nonce]` from instruction arguments, and whether the off-chain derivation equals the on-chain one. All 7 tests pass in-process.

**Decision**
Use these forms (exact syntax in S3.md): record `#[account(mut, pda = [literal("LIQREC"), account("config"), arg("position")])]` with an explicit `AccountPostState::new_claimed_if_default(.., Claim::Pda(seed))` (the `init` attribute fails with `AccountAlreadyInitialized` on a non-default account, so it cannot express create-if-absent); auction `#[account(init, pda = [literal("AUCTION"), account("config"), arg("position"), arg("nonce")])]`, claimed either by the macro (two-segment `SpelOutput::execute(vec![idents], ..)`) or explicitly (three-segment path). Seed encoding: literal zero-padded to 32 bytes, `account` is the 32-byte id, `[u8;32]` raw, `u64` little-endian zero-padded, several seeds combined as SHA-256 of the concatenation. Off-chain code must use the same rule; `spike_core::combine_seeds` is verified equal to `spel_framework::pda::compute_pda` and to the addresses actually claimed. A replayed nonce is rejected (by `AccountAlreadyInitialized` first, by the `nonce == count` assertion second); a skipped nonce is rejected by the assertion. Cycles (`caller` only): `open_record` about 158k create / 165k update, `open_auction` about 224k to 231k, macro-claim variant 244k.

**Trade-offs**
- Tested in-process only: no sequencer submit, no `spel` CLI call with arg-seeded PDAs, no IDL generation check for the array/u64 args.
- Counter owner (controller ruling): `count` is incremented only by auction creation. This is the spike's `open_auction` reading (record created if absent inside `open_auction`, `nonce == count`, `count += 1`). The standalone `open_record` that also bumps `count` is the rejected shared-counter design: a permissionless `open_record` lets anyone bump `count` and front-run or grief auction creation. It stays in the spike only as a create-if-absent demonstration; Plan 2 removes the standalone bump.
- `config` was a plain default account and must be validated in Plan 2; the instructions are permissionless for any signer, so access control is not part of this result.
- The macro rewrite to `execute_with_claims` only happens for the exact two-segment path `SpelOutput::execute` with a `vec![idents]` argument.
- Cycles are executor cycles from natively built guests.

**Status**
Accepted (S3)

---

## ADR-021: S7 Outcome: Mock Protocol Token Minted from an Engine PDA

**Context**
Spike S7 (docs/spikes/S7.md) checked the mint-by-PDA flow of `lez-programs/programs/token_mint_authority` for the engine: a token definition whose `mint_authority` is a PDA of the engine program, minted through a chained `Token::MintWithAuthority` with PDA seeds, then burned through a chained `Token::Burn`. All 4 tests pass in-process against the token guest at lez-programs `64c01210`.

**Decision**
The engine can be the sole minter of the mock protocol token: set the definition's authority to the engine's `MINT_AUTH` PDA and chain `MintWithAuthority` with `.with_pda_seeds`. A user key cannot mint (wrong authority, or PDA listed but unauthorised, or self-authority `Mint` not signed). A holder-signed chained `Burn` works. Cycles (2 executions each): mint 465,840 (caller 285,927 + token 179,913), burn 330,939 (caller 189,074 + token 141,865).

**Trade-offs**
- The definition and the holder's holding were inserted into the test state directly; the S1 spike created a PDA-authority token on a real sequencer for the faucet PDA, but this harness did not repeat that for the engine PDA. Minting to a fresh (uninitialised) holding and burning tokens held by an engine PDA were not tested.
- Deviation from the brief, which names `Token::Mint`: the working path is `Token::MintWithAuthority` (external authority account); `Token::Mint` needs the definition account itself to sign.
- Untested paths that are prerequisites for later features, each needing a Plan 2 test: minting into a taker's possibly-fresh holding (F4 `take_debt`) and burning from an engine-PDA-held holding with seeds (`settle_debt`, F5).
- `mint_to` is open to any signer in the spike; the PDA only guarantees that the engine program is the sole minting path. Gating who may trigger a mint is engine logic and is untested here.
- Cycles are executor cycles from natively built guests, not proving cost.

**Status**
Accepted (S7)

---

## ADR-022: S4 Outcome: Keep the Piecewise-Linear Discount Schedule

**Context**
Spike S4 (docs/spikes/S4.md) measured the zkVM cost of the `auction_core` operations in a risc0 3.0.5 guest (executor cycles, dev mode, natively built guest; `auction_core` copied at `627bba4`) and compared the piecewise-linear schedule with an exponential variant `RAY * (1 - r)^t` (binary exponentiation with `mul_div_floor`, `t` up to 7 200 seconds). Marginal cycles per operation: `mul_div_floor` about 10.3k; `discount_ray` 8,889 (2 points) and 8,936 (8 points); `quote_liquidation` 38,537; `quote_bid` + `apply_bid` 19,802; `take_debt` 29,032; exponential 165,779 at `t = 7 200` and 248,565 in the worst case (`t = 6 143`).

**Options**
- (A) Piecewise-linear schedule of up to 8 points (current).
- (B) Exponential decay by binary exponentiation.
- (C) Stairstep.

**Decision**
Keep (A). The brief's rule allowed (B) only if it was within 10% of (A)'s cycles (at most about 9.8k); it costs 18.7x to 28x as much (+1,765% to +2,696%), so the "reviewers would care" condition was not needed. Inside a bid, (B) would turn 19.8k cycles into 177k to 259k (8.9x to 13.1x). (C) was not run; it needs no `mul_div`, and a piecewise schedule with closely spaced points approximates it.

**Trade-offs**
- Cycles are executor cycles of a natively built guest, not proving cost and not the Docker-built image. The ratios are the useful result; the absolute values are not a budget.
- The exponential variant's accuracy was argued, not measured: its floored multiplications slightly overstate the discount (by at most about 24 RAY units), the wrong direction for a protocol-favouring rule.
- `mul_div_floor` has no cheap path for small operands (10,365 cycles vs 10,260 with wide operands), so every `mul_div` in a hot path costs about 10k cycles; the 8-point schedule costs only 47 cycles more than the 2-point one.
- Evidence excerpt: `spikes/s4_s6/logs/cycles-excerpt.log`.

**Status**
Accepted (S4)

---

## ADR-023: S6 Outcome: `Accounting` Is Updated at Chain End, Not on Every Bid

**Context**
Spike S6 (docs/spikes/S6.md) priced updating `Accounting` per bid. Measured with the current `Accounting` struct (10 fields, 129 bytes in borsh): the borsh read-modify-write (decode, `record_bid`, encode) is 1,530 cycles, but adding the account to the instruction costs 93,374 cycles more (read as a pre-state, echoed in the program output, written as a post-state; real `lee_core` v0.2.4 types), so a per-bid update adds 94,904 cycles. The real bid chain (spec section 5.6) is the engine instruction (pause observation, oracle read, clock) chaining Burn, Transfer (penalty to the buffer) and Transfer (collateral out): three token executions. Modelled as S1's `withdraw_collateral` (626,902) plus two transfers (2 x 133,827) = 894,556 cycles, the extra cost is 10.6%; that model under-counts the bid (one token execution fewer, no oracle or pause work), so 10.6% is an upper estimate. With a third token execution (1,028,383 cycles) it is 9.2%; an account-count estimate (about 1.07 M) gives 8.9%; against the core-only bid (19,802) it is 479%. The 94,904 slightly over-attributes (full `Account` clone as post-state) and is single-program executor user cycles.

**Options**
- (A) Per-bid `Accounting` update (simpler R6 statement).
- (B) Update only at chain end, folding in the auction's cumulative `debt_raised` and `penalty_raised`.

**Decision**
(B). The brief's rule prefers (A) unless it adds more than 5% of a bid's cycles; it adds 8.9% to 10.6% under every full-bid estimate, so (B). `record_bid` is additive and the `Auction` already holds the running sums, so one application at chain end gives the same totals; nothing in the core reads `total_debt_burned` or `total_penalty_collected` to decide anything, and the supply invariant of spec section 5.8 does not use them. The final bid writes `Accounting` anyway (`record_chain_end`). Non-final bids no longer share an account across auctions, which also tightens R3 isolation. The 5% threshold would only be met by a full bid above about 1.9 M cycles.

**Trade-offs**
- The denominator is modelled (no engine program exists yet): re-measure in Plan 3 (`CYCLES.md`). If a real bid exceeds about 1.9 M cycles the decision flips cheaply.
- The counters lag for in-flight auctions: `total_debt_burned + total_penalty_collected` equals the sum of `raised()` only after each chain end. The property test `tests/invariants.rs` and any dashboard must be restated accordingly (Plan 2).
- Every chain-end path must apply the cumulative update, or counters are dropped: the settling `bid` (`apply_bid` returning a `ChainEnd`) and `settle_empty` (the raised amounts from earlier bids still need folding in, and `record_chain_end` books the shortfall). Plan 2 should add a small `Accounting::record_settled(&Auction)` helper rather than build a synthetic `BidFill`; no change was made to the reviewed core in this task.
- Whether a bid ends the auction is decided at execution time (price and `t_eff` move between signing and inclusion), while LEZ needs the account list at signing. A bid signed without the chain-end accounts (`Accounting`, `CollateralConfig`/`EngineConfig` for the reservation release) that turns out to end the auction must revert with a distinct error so the client re-quotes, or Plan 2 adds a permissionless finalize path. Clients list the accounts whenever `coin_in_max >= remaining` or the quoted fill is near the lot. If most bids end up listing `Accounting`, re-evaluate S6: its 94,904-cycle saving assumes non-final bids omit the account.
- Per-account guest I/O is expensive because `Account.data` serialises one word per byte and `AccountId` through a string; this also matters for every other account the engine instructions take.
- Executor cycles of a natively built guest; not proving cost.

**Status**
Accepted (S6)

**Amended (Plan 3b, Task 7)**
The cost argument above counted executor cycles. With real proofs (po2 19) a non-final `bid` takes 1,638,400 cycles in 4 segments; adding the 94,904-cycle per-bid update gives 1,733,304, which fits the 2,097,152 cycles of those segments, so the update would add no segment and no proving time. The decision stays for the reason ADR-036 records (the final bid already writes `Accounting`), and the per-bid update is an M1 review item. See the ADR-036 amendment for the measurement.

---

## ADR-024: Plan 1 Core Amendments to the Spec

**Context**
Building and reviewing `auction_core` in Plan 1 (Tasks 9 to 12 and the whole-branch review) required deviations from, and additions to, the v5 spec. Each is recorded here with the spec section it changes, the reason, and its bound or cost, so Plan 2 and 3 implement the amended behaviour rather than the original text. This ADR also lists what the program layer must do because the core deliberately does not.

**Decision**
Amendments to the spec:
- Final-fill exception (Task 9), spec 5.1 "paid out rounds down". A bid that reaches the target and whose plain floor is 0 receives exactly 1 collateral base unit. Reason: with no expiry (ADR-012) such an auction would be stuck forever and keep its cap reservations pinned. Bound: at most 1 base unit per auction (see core.md Liveness).
- `settle_empty` (Task 9), spec 5.6 instruction table. New permissionless instruction that settles an auction with lot 0: the shortfall is booked and the reservation is released. Reason: a lot-0 auction cannot be settled by a bid (every bid is `ZeroFill`). Cost: one more instruction to expose.
- Price-0 guard in `quote_bid` (defensive). A zero price returns `Math` instead of dividing by zero. Cost: none in practice; it should be unreachable.
- `Accounting` gains `debt_on_auction` and `total_debt_auctioned` (spec 5.4 table). `settle_debt` burns `min(amount, buffer_balance, unbacked_debt - debt_on_auction)` (spec 5.6 formula) and is paired with `record_debt_start`, `record_debt_take` and `record_debt_close`. Reason: the debt a live debt auction is selling must not also be burned from the buffer; covering it twice would mint tokens without a matching loss, which spec 5.5 and ADR-012 forbid. This is the Maker `Ash` pattern. Bound: `debt_on_auction <= unbacked_debt`.
- `close_surplus` and `close_debt` (with `record_debt_close`) implement spec 5.11 `close_system_auction` in the core, using `system_ttl_ms`. Cost: system auctions have a time-based close; collateral auctions still have none.
- `SystemParams::validate`, spec 5.11 parameters. Bounds: discounts at most 3000 bps, lots greater than 0, `min_take <= lots`, `0 < mint cap <= hard cap`, `ttl > 0`.
- `take_debt` that would mint 0 returns `ZeroFill`; `start_debt` with `start_amount == 0` returns `MintCapConfig`, which is therefore freeze-eligible. This widens the meaning of the closed freeze list in spec 5.11 (see ADR-017). A distinct `DebtLotTooSmall` variant mapped to Freeze is a cleaner option for Plan 2.
- S7 uses `Token::MintWithAuthority`, not `Token::Mint` (ADR-021).

Program-layer obligations (Plan 2/3):
- `observed()` trusts the caller's `feed_valid`: the engine must derive it with `validate_feed` from the same `(oracle_ts, now, max_age)`.
- Every `t_eff` evaluation, including SDK and CLI previews, first simulates `observed()` at `now` (Plan 3).
- Call `CollateralParams::validate`, `FeedParams::validate` and `SystemParams::validate` at config time; core functions do not call them. Unvalidated `SystemParams` panic on `BPS - x`.
- Enforce `bad_debt_freeze_threshold >= debt_lot` (ADR-017).
- Pair `start_debt` with `record_debt_start(debt_lot)`, `take_debt` with `record_debt_take(old.lot_left - new.lot_left)`, and `close_debt` with `record_debt_close`. Transfer and burn the FILLED amount (the `lot_left` delta), never the requested amount.
- Expose the `settle_empty` and `close_system_auction` instructions.
- Reject zero program ids and zero authorities at config (SPEL's top-level `caller_program_id` is zero; ADR-019).
- Reject a zero `protocol_token_ref_price` at config. With 0, `start_debt` returns `Math`, so `check_solvency` gives `NoAction` instead of `Freeze`.
- Create `THREAT_MODEL.md` containing: the S5 residual (a re-stamping producer; unobserved publishes), the 1-unit final-fill bound, the oversized-alone limitation (both directions), and the keeper reward in a shortfall.
- Plan 4's first task confirms that `lez_core` loads in Basecamp and pins its own Basecamp revision (ADR-018).
- Plan 2 restates the `tests/invariants.rs` per-bid `Accounting` check for chain-end accounting (a `record_settled` helper; ADR-023).

**Trade-offs**
- The final-fill exception departs from "paid out rounds down" by at most 1 base unit per auction in exchange for liveness.
- Reusing `MintCapConfig` for a zero start amount keeps the core small but blurs the closed freeze list; this is the price of not adding a variant in Plan 1.
- The obligations above are not enforced by the core; each needs a Plan 2 or 3 test.

**Status**
Accepted (Plan 1)

---

## ADR-025: Vendored Stablecoin as a Committed Tree; Hooks Exported as a Patch Series

**Context**
Spec §4.2 asks for `programs/stablecoin/` "vendored @64c0120 + hook patch series" and §1 for the hooks to be offered upstream with the upstream suite unchanged. The stablecoin path-depends on `programs/token/core` and `programs/twap_oracle/core`, and its tests need the token guest.

**Options**
- (A) Pristine sources plus `.patch` files applied at build time (quilt style).
- (B) A pristine vendor commit, hooks as ordinary commits on top, and the patch series exported from history.

**Decision**
(B). The vendor commit (`vendor: lez-programs 64c01210 …`) is a `git archive` of upstream with no edits; token, twap_oracle core and idl-gen are vendored at the same paths so the stablecoin's relative path dependencies resolve unchanged. Each hook commit touches only what upstream needs; `scripts/export-hook-patches.sh` writes one patch per commit into `patches/stablecoin/` at the repo root (outside `programs/stablecoin/`, so no patch ever carries an earlier exported patch; commits tagged `[local]`, such as the instance-B build, are skipped). Patch paths equal upstream paths, so `git am` applies them at the lez-programs root; `scripts/check-hook-patches.sh` checks exactly that against a pristine `git archive` after every export.

**Trade-offs**
Builds, IDE and tests see one tree, and the series is reproducible from history. A rebase onto a newer upstream is a new pristine vendor commit followed by `git am` of the exported series. The upstream integration tests live at `programs/upstream_integration_tests/` (upstream `programs/integration_tests/`) because our engine tests own the spec's `programs/integration_tests/` path; its manifest is trimmed of crates we do not vendor (`programs/UPSTREAM.md`). Formatting of vendored code follows upstream (nightly rustfmt, not available in WSL here; stable rustfmt with the vendored config is the local check), so CI formats only our crates. Upstream's `clippy.toml`/`rustfmt.toml` live next to the vendored crates, not at the repo root. Changes under `programs/upstream_integration_tests/` are not exported (its path differs from upstream's `programs/integration_tests/`): upstream-bound hook tests live in `programs/stablecoin` as unit tests or are ported by hand in the upstream PR.

**Status**
Accepted (Plan 2, Task 3)

---

## ADR-026: Hook Binding and Authentication

**Context**
ADR-019 chose Variant C and left open how the host stores the engine and how the upstream laundering note (admin/freeze checks are plain `is_authorized`) is handled. Spec §5.10 names `LiquidationEngineState { engine_authority_id }`.

**Options**
- (A) Store only the authority id; derive nothing else.
- (B) Store `engine_program_id` and `engine_authority_id` explicitly; the admin binds both.
- (C) Store the program id and derive the authority from the engine's seed convention.

**Decision**
(B). Variant C needs the program id (immediate caller) and the authority (authorised PDA); storing both keeps the host ignorant of the engine's seed scheme. `set_liquidation_engine` rejects zero ids (SPEL reports a zero caller for top-level calls, ADR-019) and requires `caller_program_id == 0`, so the binding cannot be changed through a program in which an admin PDA was authorised. The existing admin and freeze instructions are left unchanged (upstream suite unchanged); the upstream PR documents that their authorities must remain user keys.

**Trade-offs**
Rotation is a single admin step (like `set_admin`). A wrong pair simply makes the hooks unusable until rebound. The binding names one engine configuration per host instance (ADR-027).

**Status**
Accepted (Plan 2, Task 4)

**Amendment (Task 5 review).** Spec §5.10 and ADR-016 round the proportional seize bound `min(C, C·cover/N·(1e4 + MAX_PENALTY_BPS)/1e4)` up. That lets every call take at least one collateral unit regardless of `cover` (C = 1000, N = 2,000,000, cover = 1 gives a bound of 1 against a fair share of 0.000625), so repeated tiny partial liquidations could drain a position while clearing about 0.05% of its debt. This contradicts the spec's own "never more than its share plus the maximum penalty" and the global rule that paid-out amounts round down (the owner's collateral is paid out). The bound now rounds down. Full seizure is unaffected: `floor(1.25·C) >= C`, capped at `C`. A full-collateral, partial-debt seizure can leave dust debt on a zero-collateral position; that is the engine's concern.

---

## ADR-027: Engine PDAs Are Seeded by the Engine Configuration

**Context**
One engine program (one ImageID) serves several `EngineConfig`s (spec §5.4 seeds them `["CONFIG", salt]`), one per host instance. `init_config` is permissionless by design (whoever signs becomes that config's admin). If the engine authority were a single program-wide PDA (`["ENGINE_AUTH"]`, as in spike S2), a rogue config created against host A's `ProtocolParameters` could call `liquidate_position` on A's positions with its own penalty and keeper reward, and A's Variant C guard would accept it (right caller program, authorised PDA). The same holds for the protocol-token mint authority.

**Decision**
Engine-authority and mint-authority PDAs include the config: `["ENGINE_AUTH", config]`, `["MINT_AUTH", config]`. A host binds exactly one engine config by storing that config's authority id (ADR-026). This is how the engine "identifies the host instance": the host names the config, and the config stores the host program id and `ProtocolParameters` id, re-checked on every hook call path.

The config PDA itself includes its admin: `["CONFIG", admin, salt]` (amends spec §5.4 `["CONFIG", salt]`). With `["CONFIG", salt]` anyone could front-run `init_config` at the address an operator announced and own that config; with the admin in the seed, a config initialised by a different admin with the same salt is a different account (pinned by tests added in Task 8 (`nobody_can_squat_the_admins_config_address`) and Task 16 (`rogue_config_with_the_same_salt_cannot_liquidate_the_bound_host`)). The admin is therefore immutable.

**Bind-time verification.** Before `set_liquidation_engine`, the host admin reads the `EngineConfig` it is about to bind and checks: `admin` is the expected operator, `host_program_id` and `host_protocol_parameters_id` are this host, `token_program_id` is the expected token ImageID, and the parameters (caps, penalty bounds via its `CollateralConfig`, protocol token) are the agreed ones. The runbook (Task 20) and `docs/architecture/hooks.md` carry the step; the SDK (Plan 3) automates it.

**Trade-offs**
Rotating a host to a new engine config is a host-admin `set_liquidation_engine`. Auctions opened under the old config stay settleable under it (their accounts are seeded by it). To be pinned by `nobody_can_squat_the_admins_config_address` (Task 8) and `rogue_config_with_the_same_salt_cannot_liquidate_the_bound_host` (Task 16).

**Status**
Accepted (Plan 2, Task 6)

---

## ADR-030: Spel-Encodable Arguments and Optional-Account Slots

**Context**
spel-cli encodes only `bool`, `u8`, `u32`, `u64`, `u128`, `string`, `account_id`, `program_id`, byte arrays, some `Vec`s and `Option` (`spel-cli/src/serialize.rs:122-139`); no `u16`, no `i128` (ADR-018), no nested structs. LEZ needs every account at signing time and rejects duplicate account ids (`validated_state_diff/mod.rs:72-75`), so an "optional" account cannot be omitted or repeated.

**Decision**
Instruction arguments use only `bool`, `u8`, `u32`, `u64`, `u128`, `Vec<u32>`; bps travel as `u32` (range-checked to ≤ 10_000, band hi to ≤ 20_000); schedules as interleaved `(elapsed_ms, bps)` `u32` pairs, so a schedule point's elapsed time is at most `u32::MAX` ms (about 49.7 days), far beyond any schedule. The host program id is read from the `ProtocolParameters` account owner, not passed. Optional accounts (protocol-token reference oracle, protocol-token definition) occupy fixed slots; when not configured the caller passes the never-initialised PDA `["UNUSED", config, slot]`, and the engine asserts that exact id, so a caller can never substitute an account that changes a decision (e.g. force a freeze in `check_solvency`).

**Trade-offs**
Instruction structs are wide and flat. The slot convention must be followed by the SDK (Plan 3).

**Status**
Accepted (Plan 2, Task 6)

---

## ADR-028: The Engine Prices from the Host's Market Oracle

**Context**
Spec §5.4 stores an `oracle_id` in `CollateralConfig`; spec §5.10 makes the hook check the host's market oracle for freshness. LEZ rejects duplicate account ids in a transaction, so listing "engine oracle" and "host oracle" separately when they are the same account is impossible, and listing two different oracles would let the host's health check and the engine's price disagree within one liquidation.

**Decision**
`set_collateral_config` requires `oracle_id == ProtocolParameters.market_price_oracle_id`, and `liquidate` re-checks it against the live `ProtocolParameters` (`OracleMismatch` if the host admin rotated its oracle; the engine admin then re-runs `set_collateral_config`). One `oracle` account serves both the engine's validity/price and the hook's freshness gate. Bids keep using the stored id.

**Trade-offs**
A host oracle rotation pauses new liquidations until the engine admin follows; bids on live auctions are unaffected.

**Status**
Accepted (Plan 2, Task 9)

---

## ADR-032: `observe_feed` Is Permissionless; Reverted Observations Are Dropped

**Context**
Spec §5.7: `liquidate`, `bid` and `observe_feed` observe the feed; any panic reverts the whole transaction (spec §3.2), so a `liquidate` or `bid` that finds the feed invalid cannot persist the pause it observed.

**Decision**
`observe_feed` is permissionless and never reverts on a bad feed; it is the one way to persist an observation of an invalid feed. `liquidate`/`bid` observe in memory, revert when paused, and their observation is lost. Two failure classes are distinct: an *invalid reading* (stale, low confidence, out of range, zero price) is a valid observation that is **recorded as a pause and never reverts** in `observe_feed` (and reverts, dropping the observation, in `liquidate`/`bid`); an *undecodable or wrong oracle account* (`WrongAccount`, `OracleMismatch`) **reverts everywhere**, including `observe_feed`, and policy (b) recovers the unobserved gap at the next successful observation. Policy (b) (S5, ADR-007) derives the stale onset from `last_seen_oracle_ts + max_age`, so a lost observation does not move the onset; the S5 property test bounds an adversary's extra discount to one block. Keepers and the app call `observe_feed` on refresh (Plan 3).

**Trade-offs**
An observation costs a transaction; the residual (unobserved publishes, a re-stamping producer) is the S5 residual recorded in `THREAT_MODEL.md`.

**Status**
Accepted (Plan 2, Task 9)

---

## ADR-029: Two Bid Instructions; a Bid That Turns Final Without `Accounting` Reverts

**Context**
ADR-023 (S6) updates `Accounting` only at chain end, and its consequence 6 asks Plan 2 to decide what happens when a bid signed without the chain-end accounts turns out to end the auction (price and `t_eff` move between quote and inclusion). In this design the reservation release writes `CollateralConfig` and `EngineConfig`, which every bid already lists (pause observation and validation), so `Accounting` is the only chain-end-only account.

**Options**
- (A) Revert with a distinct error so the client re-quotes with the account.
- (B) A permissionless `finalize` that applies chain-end effects later (an extra "settled, not finalized" state).
- (C) Always list `Accounting` (gives up the S6 saving).

**Decision**
(A), as two instructions sharing one handler: `bid` (no `Accounting`; reverts `ChainEndAccountsMissing` if the fill ends the auction) and `bid_with_settlement` (lists `Accounting`; any fill). Spec §5.5/§5.6 require chain-end effects "in the same transaction" and "exactly once"; (B) would let `unbacked_debt` lag a settled shortfall, which `check_solvency` and `start_debt_auction` read, and adds a state. Rest-account (`Vec<AccountWithMetadata>`) optional lists exist in the SPEL macro but were never exercised in the spikes; two fixed lists use only confirmed features and keep the IDL simple. Clients (Plan 3) use `bid_with_settlement` whenever `coin_in_max >= remaining_target` or the quoted fill is within the slippage margin of emptying the lot. If most bids end up listing `Accounting`, revisit S6 (Plan 3 `CYCLES.md`).

**Trade-offs**
A racing bid can revert and need a re-quote; nothing is applied when it does (test `bid_that_turns_final_without_accounting_reverts`).

**R3 deviation (M8).** Spec §5.4 lists a non-final bid's writes as its auction, vault, buffer and the bidder's holdings. Here every bid also writes `CollateralConfig` (the type-level pause clock observed before `t_eff`, spec §5.7) and lists `EngineConfig` (echoed unchanged unless the bid settles). Both are per-instance shared state by design; no other auction's data changes (test `r3_a_bid_touches_only_its_own_auction`).

**Status**
Accepted (Plan 2, Task 11)

---

## ADR-033: Sweep Rules for Engine-Held Holdings

**Context**
Anyone can raise any holding's balance (spec §3.2); spec §5.13 invariant 2 says terminal sweeps take the whole balance.

**Decision**
A fill that empties the lot transfers the whole auction-vault balance to the bidder; `claim_leftover` transfers the whole vault balance to the owner; `settle_debt` burns from the buffer's actual balance; `close_surplus_auction` returns the whole surplus-vault balance to the buffer. `settle_empty` (lot 0) moves nothing: any donation into such a vault stays there (the donor's loss, no protocol effect). Balances are only ever compared with `>=`.

**Trade-offs**
Donations go to whoever the sweep pays; they never change a booked number.

**Status**
Accepted (Plan 2, Task 11)

---

## ADR-031: System-Auction Accounts and Lifecycle

**Context**
Spec §5.11 describes F4/F5 as instant Dutch auctions addressed by `Accounting.surplus_nonce` / `debt_nonce`, at most one of each live, closed by `close_system_auction` after `system_ttl_ms`. The core (`auction_core::system`) is stateless about liveness and the last surplus end; the protocol-token reference price may come from an optional oracle.

**Decision**
- Program-layer `Accounting` fields: `surplus_live`, `debt_live`, `last_surplus_end_ms`, and counters `total_protocol_burned`, `total_protocol_minted`, `total_surplus_lots_out`, `total_surplus_returned` (invariants 9 and 17). `start_*(nonce)` requires `nonce == *_nonce`, then increments it, so a nonce can never be replayed after its auction ended.
- The surplus lot moves into its own holding `["SURPVAULT", surplus_auction]` at start (spec: "into the auction's holding"), so `settle_debt` cannot spend coin a live surplus auction is selling. `close_surplus_auction` returns the whole vault balance (ADR-033); a fully taken auction ends at its last take.
- `ref_ray` is snapshotted into `SurplusAuction` at start (the debt auction snapshots `start_amount` in the core), so takes need no oracle account and never read `ref_oracle_max_age_ms`. The oracle, its max age and the admin price may change while an auction is live without altering that auction's price (Task 8 review carry-forward; tests `live_take_ignores_later_ref_price_and_oracle_max_age_changes`).
- Use-time protocol-token checks (Task 8 review): every instruction that spends or mints the protocol token verifies, in the same transaction, that the definition slot is the configured id (or the placeholder), owned by `cfg.token_program_id` and a fungible definition (`ProtocolTokenMismatch`), and, for minting (Task 14), that its mint authority is `["MINT_AUTH", config]` (`MintAuthorityMissing`).
- `close_system_auction` is two instructions, `close_surplus_auction` and `close_debt_auction`, because their account lists differ (the surplus close returns the vault balance to the buffer; the debt close only releases `debt_on_auction`).
- The reference oracle is pinned by id only: the engine does not check its owner. lee lets only an account's owner program write its data, so an id the admin configured can only carry data its owning program wrote; the admin must configure an initialised oracle id (an uninitialised one fails to decode and the reading counts as invalid, falling back to the admin price). The price is clamped to +/-20% of the admin price and must lie within `[admin/2, 2 x admin]` (Task 13 review fold).
- F4 (Task 14): `start_debt_auction` reserves the lot with `record_debt_start(debt_lot)` (`debt_on_auction`, `debt_live`, `debt_nonce`), every `take_debt` calls `record_debt_take(filled)` with the filled amount and burns exactly that amount, `close_debt_auction` calls `record_debt_close(lot_left)`. The mint is `Token::MintWithAuthority` from the engine PDA `["MINT_AUTH", config]`, authorised only in the call that attaches its seed; a fresh taker holding must sign (S7, `RecipientMustSign`).
- `set_engine_params` refuses to change the protocol token, its oracle or the system-auction parameters while a system auction is live (`ProtocolTokenLocked`), since takes read them.
- Reference-price step rule (plan review ruling I5): each admin change of `protocol_token_ref_price` stays within `[old/2, 2*old]` (`args::check_ref_price_step`, `RefPriceStepTooLarge`; tests `ref_price_steps_are_bounded_to_half_and_double`).
- No admin path moves surplus funds: the buffer, the surplus vault and the lot move only through the permissionless start/take/close paths.

**Trade-offs**
One more holding per surplus auction (accounts are never released). The reference price of a running auction does not follow the oracle (it is a Dutch schedule seed, ADR-017). Governance-trust residual (I5): the step bound limits one change, not repeated changes (a series of changes of at most 2x each can still move the price far over time); switching or disabling the protocol token (allowed whenever no system auction is live) remains an admin power, and disabling it lets anyone's `check_solvency` freeze the host when `unbacked_debt > bad_debt_freeze_threshold` (recorded in `THREAT_MODEL.md`).

**Status**
Accepted (Plan 2, Task 13; F4 pairing added in Task 14)

---

## ADR-034: Private Path: Variant C Holds; `liquidate` Is Public-Only on v0.2.4; Attack-Test Surface

**Context**
ADR-019 left a private-path test of Variant C open (the circuit keeps one authorised set per transaction). Spec §8: the PoC submits public transactions only; the private SDK is sub-project B (milestone M4). Task 7's review also required end-to-end attack tests through the real guests for the hook guards and for laundering.

**Decision**
- Private path, two in-process `execute_and_prove` tests (all-public identities, `security.rs`): (1) an impostor chaining to `engine_freeze_new_debt` with its own legitimately authorised PDA is rejected by the hook's immediate-caller check (circuit error `ProgramProveFailed("Guest panicked: assertion `left == right` failed: Caller program is not the liquidation engine ...")`), so Variant C holds on the private path; (2) `liquidate` is rejected by the circuit with `CircuitProvingError("Guest panicked: assertion `left == right` failed: Inconsistent authorization for account ... left: false right: true")`: the auction vault is authorised by its seed in `InitializeAccount` and passed unauthorised to the hook, which the transaction-wide set forbids, the same limitation the upstream `open_position` has. Making `liquidate` private-capable needs a different vault-initialisation shape (e.g. the hook initialising custody) and is recorded for sub-project B; nothing in the PoC depends on it.
- Public path, real lee + real guests, attacker programs in `programs/test_programs` (`impostor`, `flawed_engine`): forged `is_authorized` flag, the impostor's own PDA, an impostor chaining `liquidate_position`, one transaction chaining both hooks, and `set_liquidation_engine` through a chained call (rejected `set_liquidation_engine must be a top-level call`, binding intact) are all rejected. The cases where the authority id matches and is genuinely authorised (the host rebound to the real engine program id with the impostor's PDA as authority) leave only the immediate-caller check standing. The red check confirms this directly only for the liquidate rebound test (the attack succeeds without the check). The both-hooks rebound test still goes red, but with a different rejection (below). `impostor_own_pda_is_rejected_by_the_immediate_caller_check` uses the real binding, so without the caller check it is still rejected by the bound-authority check (`Engine authority account is not the bound engine authority`).
- Laundering: the real engine has no path that authorises `ENGINE_AUTH` for a program other than `cfg.host_program_id`, and the host's hooks chain to nothing (invariants (i), (ii)), so the attack cannot be mounted against the real engine. The test binds the host to `flawed_engine`, a test-only program that authorises its PDA for a caller-chosen program (S2's `call_untrusted`); `flawed_engine` -> `impostor` (forwards the flags it received, no seeds, nothing forged) -> `engine_freeze_new_debt` / `liquidate_position`. lee propagates the authorisation (the authority is authorised and its id matches the binding), and the hook still rejects with `Caller program is not the liquidation engine` because the immediate caller is the impostor. Observed with the immediate-caller assert removed from `liquidation_hooks.rs` (red check, Task 16 fix round 1): the laundered `engine_freeze_new_debt` and `liquidate_position` both succeeded, so the laundering tests fail with "transaction was expected to fail"; likewise `impostor_chaining_liquidate_position_with_a_rebound_own_pda_is_rejected` (host bound to the real engine id and the impostor's own PDA) fails because the transaction succeeds, and the chained `set_liquidation_engine` rebind is stopped only by its own top-level check. The forged-flag shapes are rejected by lee (`InvalidAccountAuthorization`) even without the check, so they pin the message only in the green build; the both-hooks rebound shape fails red with `InconsistentAccountPreState` (the first hook ran, the second saw a stale pre-state), not with a success.
- Documentation-only tests: the engine IDL carries no program-id argument (M5) and a config built on a fake host is accepted (I4), so clients must verify `host_program_id` / `token_program_id` against DEPLOYMENTS.md.

**Trade-offs**
Private liquidation is not available on v0.2.4 with this chain shape. Bids, claims and system auctions were not exercised on the private path in Plan 2. The laundering test needs a deliberately flawed engine, so it proves the host guard, not the real engine's invariant (i); that invariant is enforced in code and reviewed in Task 19.

**Status**
Accepted (Plan 2, Task 16)

---

## ADR-036: Chained-Call Budget per Instruction (measured)

**Context**
ADR-018: at most 11 executions per public transaction, 10 per private one. Spec section 11 asks for cycles per instruction (`CYCLES.md` is Plan 3).

**Decision**
Every engine instruction stays within 10 executions; `programs/integration_tests/tests/chains.rs` asserts it (and the exact counts of `liquidate` = 5, final `bid_with_settlement` = 4, `take_surplus` = 3, `take_debt` = 3 and the burn variants). Measured in Plan 2 by replaying each transaction's whole chain with the risc0 executor (`tests/common/measure.rs`, the S1 step 6 method): executor cycles of natively built guests in dev mode, not proving cost. The scenario is the harness default (one position, defaults from `defaults_for_tests`).

| Instruction | Executions | Total cycles |
|---|---|---|
| observe_feed | 1 | 689,818 |
| set_collateral_config | 1 | 926,597 |
| liquidate (init vault, host hook, keeper reward) | 5 | 3,151,610 |
| bid (non-final: burn, collateral out) | 3 | 1,626,443 |
| bid_with_settlement (final: burn, penalty, collateral out) | 4 | 1,960,962 |
| claim_leftover | 2 | 839,296 |
| start_surplus_auction (no implicit burn) | 3 | 1,157,463 |
| start_surplus_auction (implicit burn) | 4 | 1,352,191 |
| take_surplus | 3 | 1,216,593 |
| close_surplus_auction | 2 | 914,363 |
| settle_debt (empty buffer, nothing burned) | 1 | 591,029 |
| settle_debt (burn) | 2 | 785,555 |
| start_debt_auction (no implicit burn) | 1 | 835,255 |
| start_debt_auction (implicit burn) | 2 | 1,029,778 |
| take_debt | 3 | 1,317,796 |
| check_solvency (freeze, empty buffer) | 2 | 1,335,557 |
| check_solvency (implicit burn + freeze) | 3 | 1,530,390 |

`settle_empty`, `close_debt_auction` and `set_engine_params` are single engine executions, `init_config` is 2 (Shared Reference R3; not measured here). The maximum is `liquidate` with 5 executions, half the private-path limit. The engine execution is the largest part of every chain (0.59 to 1.96 M cycles); each token call costs 0.11 to 0.18 M and the host hook 0.27 to 0.81 M.

**Trade-offs**
Cycle numbers are indicative; Plan 3 re-measures with the real-proof pipeline and owns the regression check. The final bid (`bid_with_settlement`, 1,960,962 cycles) is above the roughly 1.9 M cycle line at which the S6 spike said to revisit ADR-023 (accounting written only at chain end); it exceeds it by about 3 percent, in executor cycles of one scenario. Revisited: ADR-023 stands. Its per-bid `Accounting` update would be added only to non-final bids, because the final bid already writes `Accounting` at chain end. Against the measured non-final `bid` (1,626,443 cycles) the S6 cost of 94,904 cycles is 5.8 percent, still above the 5 percent rule. The 1.9 M line applied to the bid that would carry the update, and the final bid does not need it.

**Status**
Accepted (Plan 2, Task 17)

**Amended (Plan 3b, Task 7)**
Real-proof re-measure (ADR-045, po2 19): non-final `bid` engine execution (`scripts/measure-proof.sh prove_bid 19`) 1,638,400 total cycles in 4 segments, 938.2 s for the three executions (712.4 s engine, 126.3 s and 99.5 s for the two token calls); the `bid` of the full-suite run (a different scenario, 1,703,936 total cycles, also 4 segments) gives the same result; the S6 per-bid `Accounting` update (+94,904 cycles, 1,733,304 in all) fits the 4 x 2^19 = 2,097,152 cycles of the segments already paid for: marginal proving cost 0. Outcome: ADR-023 stands for the reason above (the final bid already writes `Accounting`); under proving the per-bid update would cost nothing, so it is an M1 review item. No engine change in Plan 3b.

---

## ADR-035: Second Host Instance via a Guest Feature

**Context**
Spec §5.4 and S1: one stablecoin binary supports one host instance; a second needs a distinct ImageID. S1 used a patched clone and a Docker build.

**Decision**
The vendored guest gains feature `instance-b`, which adds `#[used] static HOST_INSTANCE_TAG: [u8; 8] = *b"INST-B01"` (the S1 mechanism). `programs/stablecoin/methods_b` embeds the same guest package with that feature (`embed_methods_with_options`). The commit is tagged `[local]` and excluded from the upstream patch series. `scripts/check-hook-patches.sh` applies the `programs/stablecoin` diffs of `[local]` commits after the series, so its byte-for-byte comparison still catches any un-exported, non-`[local]` change. The engine needs nothing: its seeds include the config, and each config binds one host.

**Trade-offs**
A second guest build in the test workspace (one more long first build).

**Status**
Accepted (Plan 2, Task 18)

---

## ADR-037: Plan 3 Is Split into 3a (Engine Hardening, SDK, CLI) and 3b (CI, Real Proofs, Reproducible Builds, Evidence)

**Context**
Plan 3 (ADR-018 split: SDK / CLI / CI / real proofs) gathered the spec §6.1, §6.4–§6.6, §9, §11 and §12 obligations plus eleven carry-overs from Plan 2 (`docs/architecture/programs.md`). Planned as one document it is about 30 tasks in two independent deliverables.

**Options**
- (A) One Plan 3.
- (B) 3a = engine hardening (THREAT_MODEL findings 4 and 8), test hygiene, `auction_sdk`, `auction_cli`, CLI-driven localnet smoke; 3b = program CI with risc0, real-proof CI, Docker reproducible builds and the ImageID refresh, `CYCLES.md` and the ADR-023 re-measure, `REQUIREMENTS.md` and its checker, `cargo-fuzz`, coverage gate, red-team pass, oracle publisher/keeper, one-command demo, upstream issue drafts, `SUBMISSION.md`.
- (C) Split by layer (programs vs clients).

**Decision**
(B). 3a ships working software on its own (a user drives the engine from the CLI on localnet without spel). 3b depends on 3a (its real-proof end-to-end job and the demo drive the CLI); 3a does not depend on 3b. Both on-chain changes land in 3a, so 3b's reproducible builds refresh the ImageIDs once, after the last guest change.

**Trade-offs**
Until 3b, CI still runs only the `auction_core` job, and the ImageIDs in `DEPLOYMENTS.md` remain native-build values (THREAT_MODEL I4 residual). The 3a localnet run cannot show F4 or the freeze path: both need a shortfall, which needs a moving oracle price (3b publisher).

**Status**
Accepted (Plan 3a, Task 1)

---

## ADR-038: The Protocol-Token Slot Is Validated at Config Time

**Context**
THREAT_MODEL finding 4 (MEDIUM, accepted in Plan 2): `init_config` / `set_engine_params` recorded an enabled protocol-token slot by id only. A non-token account there let `start_surplus_auction` lock the surplus lot until `close_surplus_auction` (every take fails `ProtocolTokenMismatch`), and made `start_debt` fail `ProtocolTokenMismatch`, which `check_solvency` maps to Freeze above the threshold when `freeze_on_bad_debt` is set.

**Options**
- (A) Require a fungible token definition owned by the config's token program.
- (B) (A) plus `["MINT_AUTH", config]` as its mint authority.
- (C) Keep use-time checks only.

**Decision**
(A), checked in `init_config` after the slot checks and in `set_engine_params` after the live-auction lock (so a locked change still reports `ProtocolTokenLocked`). Not (B): the token program can rotate a mint authority (`SetAuthority`), so a config-time authority check would not hold later, and a surplus-only token without a mint authority is legitimate; `MintAuthorityMissing` remains a spec §5.11 freeze reason at use time.

**Trade-offs**
The mock token must exist before a config names it (fixtures and the localnet runbook create it first). The use-time `ProtocolTokenMismatch` checks remain as defence in depth; tests reach them only through force-inserted accounts.

**Status**
Accepted (Plan 3a, Task 2)

---

## ADR-039: Fixed Relist Delay Between Debt Auctions

**Context**
THREAT_MODEL finding 8 (MEDIUM, accepted in Plan 2): `start_debt_auction` and `close_debt_auction` are permissionless and the debt side had no delay, so a griefer could keep `debt_live` true almost all the time; `set_engine_params` then refuses protocol-token, oracle and system-parameter changes (`ProtocolTokenLocked`), and the admin wins only by ordering two transactions (close, change) between the griefer's close and next start.

**Options**
- (A) A `debt_delay_ms` engine parameter: new instruction argument, `SystemParams` field in `auction_core`, `start_debt` signature change (about 60 edit sites, IDL change).
- (B) `set_engine_params` closes an expired debt auction atomically: the admin still loses when the griefer's close and start are ordered first at expiry.
- (C) A fixed program-layer relist delay after any debt-auction end, recorded in `Accounting`.

**Decision**
(C), `DEBT_RELIST_DELAY_MS = 600_000` (the spec's minimum system TTL). After any end the admin has a guaranteed 10-minute window with no live debt auction. Engine-only: no argument, account list or `auction_core` change.

**Trade-offs**
Debt auctions are at most one per `system_ttl_ms + 10 min`, so a bad-debt backlog that needs many lots clears more slowly; the delay is fixed (a griefer cannot extend it) but the admin cannot shorten it in an emergency either. The delay is not configurable (a parameter can be added in M3 if governance wants one). `AccountingState` grows by one `u64`, so existing localnet state needs a fresh deployment (Task 16 redeploys anyway).

**Status**
Accepted (Plan 3a, Task 3)

---

## ADR-040: SDK Architecture: Synchronous `Chain` Trait, Own Enum-Index Encoders, Handler-Mirroring Quotes

**Context**
Spec §6.1 asks for an SDK with its own encoders (spel-cli encodes IDL positions and mis-encodes the stablecoin, spec §3.2), PDAs, decoders, quotes that call the same `auction_core` functions as the program, plan builders, discovery and confirmation. The CLI is synchronous and the Plan 4 FFI is a synchronous `char* op(const char* json)`.

**Options**
- (A) Async SDK over the jsonrpsee client.
- (B) Synchronous SDK over a `Chain` trait; the RPC implementation owns a current-thread tokio runtime.
- (C) Wrap spel-cli.

**Decision**
(B). Encoders are `lee::program::Program::serialize_instruction` of each program's own enum (the path `Message::try_new` uses), so the first word is the enum index by construction. Quotes call `auction_engine_program::feed::{redemption_price, observe, require_live}` and the `auction_core` functions in the handler's order (ADR-024 `observed()` simulation included) instead of re-implementing them; `programs/integration_tests` proves quote == on-chain result on the real guests (Task 8). Every transaction for an engine config starts with `verify::verify_config` against a deployment file (I4); `admin init-config` and host/dev commands, which have no config to verify yet, do not.

**Trade-offs**
The SDK depends on the pure handler crate `auction_engine_program` (no zkVM), which ties it to the program's source revision; that is the point (same code). A blocking runtime per `RpcChain` is fine for a CLI, not for a server.

**Status**
Accepted (Plan 3a, Task 4)

---

## ADR-041: The SDK Reads the Wallet Keystore File Directly

**Context**
Spec §6.6: "Signing goes through the LEZ wallet keystore". The LEZ v0.2.4 `wallet` crate exposes `Storage::from_path` and `pub_account_signing_key`, but depending on it pulls `keycard_wallet` → `pcsc` (a native smart-card library) and the wallet's sync client into the SDK.

**Options**
- (A) Depend on the `wallet` crate.
- (B) Shell out to the `wallet` binary for every signature.
- (C) Read `storage.json` (the wallet's persisted format) and sign with `lee::PrivateKey`.

**Decision**
(C). The SDK parses `key_chain.accounts` (`Public` → `data.ssk`, `ImportedPublic` → `pub_sign_key`, private entries recorded to be refused) and `labels`, and checks that every key derives its stored account id. Upstream `ImportedPrivate` entries have no `account_id` and are skipped, so an unlabelled imported private account fails as `NoKey`. Accounts are created, `auth-transfer init`-ed and funded with the real `wallet` binary. Debug never prints key material: paths and labels may appear in messages and `Debug`; private keys, seeds, mnemonics and passwords never appear in `Debug`, `Display`, errors or logs.

**Trade-offs**
A future wallet storage format change breaks the reader (pinned to LEZ v0.2.4; the format is tested by `keystore_json` fixtures). Keys are unencrypted on disk in v0.2.4 (upstream has not implemented storage encryption yet); the SDK never prints them and cannot zeroize `lee::PrivateKey` (no `Zeroize` upstream). Hardware (keycard) accounts are not supported by the CLI. Unknown account-entry kinds fail the whole keystore (fail-closed) while the reader is pinned to v0.2.4.

**Status**
Accepted (Plan 3a, Task 9)

---

## ADR-042: Confirmation Without a Target-Account Check; Lock File for One In-Flight Transaction per Signer

**Context**
Spec §6.5: record the signer nonce `n0` and height `h0`; poll the nonce about every 2 s "checking that the target account changed"; `nonce > n0` is Included, still `n0` at `h0 + 5` is Dropped; one in-flight transaction per signer.

**Options**
- (A) Also compare a target account before and after.
- (B) Nonce only; per-signer lock file in the CLI state directory.
- (C) Nonce only; in-process lock only.

**Decision**
(B). On v0.2.4 the signer nonce advances only when the transaction is applied, so a nonce above `n0` already proves application; a target-account comparison adds a read and can be confused by unrelated writes to the same account. Verified in the v0.2.4 source (`lee/state_machine/src/state/mod.rs`, `V03State::apply_state_diff`): `public_account_nonce_increment()` is called there for every signer, and `apply_state_diff` is reached only with a `ValidatedStateDiff`, which `ValidatedStateDiff::from_public_transaction` (`validated_state_diff/mod.rs`) returns only after signatures, nonces, program execution and every post-state check passed; the sequencer (`lez/sequencer/core/src/lib.rs`, transaction validation in block build) logs an `Err` from `validate_on_state` and skips the transaction without touching state. So the nonce cannot advance without the transaction's effects.

The lock is a file `<state>/locks/<signer>.lock` holding `{nonce, height, owner}`, created with `create_new`, so of two racing processes exactly one succeeds. A lock is stale, and replaced, when the signer's nonce moved past its nonce, or the height reached `height + 5`, or it does not parse (torn write after a crash), so a crashed CLI never blocks a signer for more than 5 blocks. Staleness is decided on chain facts only, not on process liveness or file age: pids are not comparable across WSL, Windows, containers and machines, are reused, and a liveness probe needs platform-specific code (and `unsafe` or an extra crate); a file age is a wall-clock guess against block time. The cost is that a lock outlives a crashed holder by up to 5 blocks, and that a live holder that stalls past its window loses its lock, which is correct because its transaction is by then either applied or dropped. Replacement re-reads the file and removes it only if unchanged, and a holder removes its lock on drop only if the file still carries its `owner`, so a takeover is never undone by the old holder.

**Trade-offs**
Included means "some transaction of this signer at nonce n0 was applied", not necessarily this hash. That can differ when two processes both hold a lock (replacing a stale lock is remove-then-create, so two processes judging the same stale lock at the same instant can both proceed), when two CLIs use different state directories, or when a transaction is sent outside the lock (for example by the wallet CLI). Both transactions carry nonce n0, so at most one applies and the other is dropped by the sequencer; the loser can therefore be told Included. Callers must re-read the accounts they touched after Included. Hash-level confirmation arrives with `poll_transaction_status` after the v0.3 port, which also replaces the lock.

Dropped means "not applied by height h0 + 5". A backlogged transaction can still land later, but a re-quoted transaction reuses nonce n0, so at most one of the two applies. Polling reads the height before the nonce, because the sequencer moves the nonce before the tip; the reverse order could pair a stale nonce with a fresh height and report a false Dropped for a transaction that just landed.

After a send is attempted the lock is released only on a definite verdict (Included, Dropped, or a send error that is a definite rejection). A timeout, an unknown send result (`SdkError::SendUncertain`, which chain implementations return for a transport failure after the request may have been delivered) or a panic keep the lock file; the chain-fact staleness rule clears it within 5 blocks. The lock is written to a temp file and hard-linked into place, so a live slow writer never leaves an empty or torn lock; on filesystems without hard links it falls back to `create_new` + write, where an empty file is re-read briefly and torn files from crashes are still recovered. An empty nonce reply is an error, never nonce 0.

**Amended (Plan 3a final review, M1)**
`submit` used to read `n0`/`h0` before taking the lock. If another transaction of the same signer landed and released its lock in between, the lock was taken on a stale `n0`, and the first poll reported that other transaction as Included. `submit` now re-reads the nonces and height under the lock; a moved nonce releases the lock and repeats the read (3 attempts, then `InFlight`). The height read under the lock is written into the lock record (temp file renamed over the lock, only while this process owns it) and used for confirmation, so the lock and the confirmation share one baseline and the drop window never starts before the signing. A height that moved a whole drop window between the two reads retries as well, since other processes would already judge that record resolved.

**Amended (Plan 3b, Task 2)**
- Rebase margin (Plan 3a N1): the baseline re-read under the lock must leave at least one block of the drop window (`h1 + 1 < h0 + drop`), otherwise the lock is re-taken from a fresh baseline; a takeover between the re-read and the rename would need two blocks within microseconds. Test `a_re_read_one_block_short_of_the_window_end_retakes_the_lock`.
- Pinned behaviour: three stale attempts end in `InFlight` with no lock file left (`a_nonce_that_moves_on_every_read_exhausts_the_retries_and_leaves_no_lock`, N4); a panic after the send keeps the lock (`a_panic_after_the_send_keeps_the_lock`).

**Status**
Accepted (Plan 3a, Task 10); amended (Plan 3a final review, Plan 3b Task 2)

---

## ADR-043: Scanner Cursor Stores the Block Hash and the Discoveries

**Context**
Spec §6.4: positions and auctions are found by a `getBlockRange` scan with a persisted cursor ("Scanned to block N · Rescan"). A localnet can be restarted (block N then has another hash), the node can fail to return a block, and the process can die mid-write.

**Options**
- (A) Cursor = last block number only; discoveries recomputed each run.
- (B) Cursor = `{last_block, last_hash, auctions, positions}`; rewind `reorg_depth` blocks when the stored hash no longer matches; atomic tmp+rename writes.

**Decision**
(B). Discoveries are sets of ids, so a rescan never duplicates; list commands read each discovered account and drop ids that no longer decode. A missing or non-extending block is an error and the cursor is not advanced.

**Trade-offs**
A rewind deeper than `reorg_depth` (10) needs `rescan --from 1`. Ids discovered in rebuilt blocks that no longer exist stay in the set until they fail to decode (harmless). Production uses the indexer's `getTransactionsByAccount`, then v0.3 events (spec §6.4).

**Amended (Plan 3a final review, I1)**
The Task 11 ruling that a fork deeper than `reorg_depth` is "impossible by design (the rewind point's hash is re-read from the node)" does not hold for a replaced chain. The config address is a PDA of admin and salt, so a cursor file survives a localnet restart and a redeploy; re-reading the rewind point's hash from the new node keeps the scan self-consistent but not complete: a cursor above the new head scanned nothing, and a new chain longer than `last_block - 10` was scanned only above the rewind point, both reported as success. The cursor now also stores:
- `anchor`: the hex hash of block 2. Not block 1: the v0.2.4 sequencer builds its genesis block from its config with `timestamp: 0` (`lez/sequencer/core/src/lib.rs`), so a restarted localnet reproduces the block 1 hash; block 2 is the first block stamped with the wall clock.
- `recent`: the hashes of the last `reorg_depth + 1` scanned blocks.

Every scan reads the node's block 2 first. A different anchor (or a chain now shorter than block 2 under an anchored cursor) discards the cursor, its discoveries included, rescans from block 1 and reports `Restart::ChainReplaced`; the CLI prints one `note:` line on stderr. With a matching anchor, a changed cursor block or a cursor above the head rewinds to `min(last_block - reorg_depth, head)` as before, but the rewind is accepted only if it reaches genesis or the node's hash of the rewind block equals the one in `recent`; otherwise the scan fails, cursor unchanged, with "run `rescan --from 1`". A hash chain makes an equal hash at the rewind block prove that every block below it is the one scanned, so a successful scan is now complete. `rescan --from N` keeps working: it positions the cursor at the node's block `N-1` and records the node's anchor (the user chose the start), and still discards a cursor of another chain first.

Old cursor files still load (`anchor` and `recent` default to empty). An empty anchor means "unknown": a cursor that recorded nothing past block 1 is kept; any other is rebuilt by a full rescan from block 1 (`Restart::NoAnchor`, one stderr note), which records the anchor. A full rescan was chosen over adopting the node's current anchor because the old cursor may already belong to a replaced chain, and adopting the anchor would make that permanent and silent; the cost is one full scan per old cursor file.

Ruling (supersedes the Task 11 "deeper-than-depth fork is impossible" ruling): the scanner proves completeness from the cursor's own records (anchor, recent hashes) and never from hashes re-read from the node; when it cannot prove it, it fails and names `rescan --from 1`.

**Amended (Plan 3b, Task 2)**
`rescan --from N` applies the same rule as `scan` to an anchorless cursor past block 2: it is discarded (`NoAnchor`), so its ids are dropped and the CLI notes the skipped blocks 1..N-1 (Plan 3a N2; `start_at_resets_an_anchorless_cursor_past_the_anchor_block`, golden test `rescan_from_on_a_cursor_without_anchor_notes_the_reset_and_the_gap`).

**Status**
Accepted (Plan 3a, Task 11); amended (Plan 3a final review, Plan 3b Task 2)

---

## ADR-044: CLI Conventions: Labels or Ids, Plain Key-Value Output, Exit Codes, Golden Tests

**Context**
Spec §6.6 lists the commands; spec §9 asks for CLI golden tests. The CLI must be scriptable (the localnet runbook drives it) and its failures must be distinguishable. Two platform facts shape it: clap exits 2 on a usage error (which would collide with the verification exit code), and a scanner cursor can be corrupt or too wide for the node's 16 MiB response cap, which the scanner's own messages tell users to fix with `rescan`.

**Options**
- (A) JSON output (`--json`) and one exit code for every failure.
- (B) Plain `key value` lines, a stable exit-code table, and golden files that pin the output.
- (C) Build the CLI on spel-cli.

**Decision**
(B). Account arguments take a base58 id or a wallet label. Output is `key value` lines (key padded to 17 columns, then a space, so the last field is always the value) and one line per listed item, with no colour or tables, so scripts can `awk` it and golden files stay stable. Exit codes: 1 usage/other (including an unreadable deployment file and clap usage errors, which `main` remaps from clap's default 2), 2 verification (I4), 3 dropped (re-quote), 4 keystore. The mapping is one function, `CliError::exit_code`, fed by `From<SdkError>` (a keystore error is 4, a `Mismatch` is 2, everything else 1). `run` is generic over `Chain`; golden tests run every command against `FixtureChain` with a temporary wallet, replacing temp paths and transaction hashes. Lists read the discovered ids on chain and drop those that no longer decode (ADR-043). `rescan --from N` repositions the cursor at block N-1 (its hash read from the node) and keeps the discovered sets; `--from 1` also starts from an empty cursor when the file is corrupt. `--batch N` on every scanning command sets the `getBlockRange` size. When the deployment file names no operator, `Env::announce_admin` prints the admin read from the chain, labelled as chain-only, and every signing command must call it before signing; `config` shows it.

**Trade-offs**
No JSON output in the PoC (the Plan 4 FFI is the machine interface). Golden files must be reviewed when output changes (`UPDATE_GOLDEN=1`). spel-cli is not an option (it mis-encodes stablecoin instructions, spec §3.2). `rescan --from N` with N beyond the cursor leaves the blocks in between unscanned by design; use `--from 1` to rebuild. The CLI says so on stderr (`note: blocks X..N-1 skipped`), and warns on stderr (`warning: skipped <id>: ...`) about list candidates that decode but are the wrong account or are rejected by the engine, instead of hiding them; only gone or undecodable ids are dropped silently. A wrong-account `Mismatch` on a user-supplied id (for example `quote-bid` on a non-auction) exits 2 like a program-id or deployment mismatch; that is intended.

**Amended (Plan 3a final review, M2-M5)**
- Exit 2 also covers a config account that is not on the node (`verify_config` returns a `Mismatch`, "wrong network, or wrong --config?") and a host with no `LiquidationEngineState` in `verify_binding`, and a signer that is not the config admin (admin commands) or the host admin (`host bind`, `set-min-cr`, `set-controller-gains`); these used to exit 1. The host-admin check now runs before signing for all three host admin commands.
- `watch` reports a failed step (`observe-feed` or `list-auctions`) on stderr (`watch: round N: ... failed (continuing): ...`) and goes on; a failed `observe-feed` does not skip the listing. Setup errors (verification 2, keystore 4, usage, closed output) stop it at once. A run bounded by `--iterations` exits with the last step error if any step failed, so exit 0 still means every round succeeded.
- Signing commands (tx, `admin`, `host`, `dev`, and `watch --observe-with`) print `deployment: <absolute path> (network <name>)` on stderr once, because the default `deployments/localnet.json` (and the default state directory `.auction-cli`) resolve against the current directory. Run the CLI from the repository root, or pass `--deployment` and `--state-dir`, so that every invocation trusts the same file and shares the same locks.
- Scanning commands print one `note:` line on stderr when the scanner discarded a cursor of another chain or an old cursor without an anchor (ADR-043, amended).

**Amended (Plan 3b, Task 3)**
A bounded watch exits with the most severe step error (3 before 1), the first of equals (Plan 3a N3, keep_worst). State-changing dev commands also require the deployment file's chain_genesis_hash to match the node (ADR-046, exit 2); dev resolve, dev balance and the new dev chain-id work on any network, and print no deployment note (they sign nothing).

**Status**
Accepted (Plan 3a, Task 13); amended (Plan 3a final review)

---

## ADR-045: Real Proofs on LEZ v0.2.4 and Their CI Tiers

**Context**
Spec §1.3 and §9 ask for one real-proof (`RISC0_DEV_MODE=0`) end-to-end job per push to `main` and the full real-proof suite nightly and on tags; RFP S2 asks for end-to-end tests against a standalone sequencer with `RISC0_DEV_MODE=0`. LEZ v0.2.4 never proves public transactions (the sequencer uses the executor), and hosted runners for this private repository have 2 vCPUs, 7 GiB RAM and a 360-minute job limit.

**Options**
- (A) Treat a `RISC0_DEV_MODE=0` sequencer run as the whole requirement.
- (B) Prove every execution of each instruction's chain with the risc0 prover, in CI where it fits, plus (A).
- (C) Prove only locally and commit the logs.

**Decision**
(B), tiered by the Plan 3b Task 1 measurement (`docs/architecture/real-proofs.md`):
- RFP S2: the `localnet-e2e` job (added in Task 14) runs `scripts/demo.sh` (added in Task 14) against a sequencer with `RISC0_DEV_MODE=0` on every push to `main` (R1). The local smoke run at `RISC0_DEV_MODE=0` is deferred to Task 14 (disk).
- Segment size `AUCTION_PROVE_PO2 = 19` (R2). `prove_liquidate` passed at po2 18 (2167 s, 2368 MiB) and po2 19 (2729 s, 4719 MiB); po2 20 was not run; expected about 9.4 GiB from the po2 18→19 doubling (one measured step), above the 5632 MiB ceiling; PO2 = 19 by controller ruling.
- Per push to `main`: Tier B: `real-proof-push` proves `settle_debt` on a hosted runner; the liquidate-bid path is proved by the Tier C self-hosted job when it is enabled, plus the committed local full run (R3).
- Nightly and on tags: Tier C: a self-hosted job (disabled unless `vars.REAL_PROOF_SELF_HOSTED == 'true'`) plus a committed local full run in `docs/evidence/` (R4).
- Privacy circuit (`observe_feed`): deferred to Task 7 (disk); not scheduled until measured (R5).

**Trade-offs**
The proved guests are native builds of the same source (the deployed Docker builds have different ImageIDs, ADR-047): a receipt shows the source is provable within the limits, not that a particular deployed binary was proved. Hosted timing scales by an assumed factor of 4 from an 8-core machine; Task 7 records the first CI timings and moves a tier if they exceed the budgets. Tier B was established at po2 18 (4 × 367.6 s = 1470 s ≤ 2700 s); `settle_debt` and `bid_with_settlement` at po2 19 are deferred to Task 7's local full run (disk), which confirms it. On the Windows dev machine, proving memory grows the pagefile on C:, which limits how many local proofs can run.

**Status**
Accepted (Plan 3b, Task 1); finalised in Task 7 (CI tiers as built: PUSH B, NIGHTLY C, first hosted timings pending in docs/architecture/real-proofs.md; the workflow is not on `main` yet).

---

## ADR-046: Chain Identity for State-Changing Dev Commands

**Context**
Plan 3a M6: the dev commands (mock tokens, mock oracles) were guarded by the deployment file's `network` label only, so `--rpc` could aim them at any node with a file that says `localnet`. The read-only `dev resolve` and `dev balance` were blocked off localnet for no reason.

**Options**
- (A) Refuse `--rpc` for dev commands.
- (B) Refuse non-loopback RPC URLs.
- (C) Record the chain in the deployment file and compare it with the node before signing.

**Decision**
(C). The deployment file gains `chain_genesis_hash`, the hash of block 1. LEZ v0.2.4 builds block 1 from the sequencer config with timestamp 0, so it identifies a chain's configuration: a restarted localnet keeps it, a testnet or another configuration does not. `verify_chain_identity` compares it before any state-changing dev command signs (`Mismatch`, exit 2). The scripts record it right after starting the sequencer (`auction-cli dev chain-id`). The read-only `resolve`, `balance` and `chain-id` run on any network. The committed `deployments/localnet.json` keeps `null` (the value is recorded per run), so state-changing dev commands refuse with the committed file until a run records it.

**Trade-offs**
Two localnets started from the same config share block 1; both are throwaway, so this is accepted. The scanner keeps its own anchor at block 2 (ADR-043), which also changes across restarts.

**Status**
Accepted (Plan 3b, Task 3)

---

## ADR-047: Reproducible Docker Guest Builds; Committed Binaries; Native ELFs Stay in Tests

**Context**
I4: clients compare a config's program ids with `DEPLOYMENTS.md`, which until now held native-build ImageIDs that nobody else could reproduce (THREAT_MODEL I4 residual). The upstream guests are Docker-built and their ImageIDs match lez-programs' own DEPLOYMENTS.md (S1).

**Options**
- (A) Keep native builds.
- (B) `RISC0_USE_DOCKER` for every test build.
- (C) Build the deployable guests with the upstream Docker recipe, commit the binaries with a manifest, check reproduction in CI; tests keep native ELFs.

**Decision**
(C). `scripts/build-guests.sh` runs lez-programs' recipe (builder image pinned by digest, same rustflags, the vendored `risc0-packager`) for stablecoin (with hooks) and auction_engine. The two binaries and the three upstream ones (token, twap_oracle, token_mint_authority from lez-programs `make build-programs`) are committed in `artifacts/guests/` with `manifest.json` (sha256, ImageID, source). `deployments/localnet.json` and `DEPLOYMENTS.md` carry these ImageIDs (`guest_manifest_matches_the_localnet_deployment`). CI job `guest-repro` rebuilds our two guests on every push and pull request touching their inputs and requires identical bytes; it also checks every committed binary against the manifest (sha256 and ImageID). The three upstream guests are rebuilt from the pinned lez-programs 64c01210 source only on the nightly and `workflow_dispatch` runs; on push and pull request they are only hash-checked against the pinned manifest. The localnet scripts deploy the committed binaries, so a demo needs no Docker.

**Trade-offs**
About 2.3 MB of binaries in git. Two ImageID sets exist: the deployed Docker one and the native one of the in-process tests and of the Task 7 receipts (ADR-045). Every later guest change must rebuild and refresh the manifest in the same commit (Plan 3b Global Constraints).

**Status**
Accepted (Plan 3b, Task 5)

---

## ADR-048: Cycle Baseline, Transaction Sizes and the 10 % Regression Rule

**Context**
Spec §11: `CYCLES.md` records cycles per instruction against 32 Mi and transaction sizes; CI fails on a regression above 10 %. ADR-036 measured the chains once.

**Options**
- (A) Assert hard-coded numbers in tests.
- (B) A committed baseline compared by a script, with the table generated from the same data.
- (C) Real-proof cycles as the baseline.

**Decision**
(B). `chains.rs` records every measured instruction (`CYCLES_OUT`, executor cycles and borsh size of the signed transaction, built by the same helper that submits it); `scripts/check-cycles.py` fails on growth above 10 % (exactly 10 % passes), on a label added or removed without a baseline change, on more than 10 executions and on any execution over 32 Mi; `CYCLES.md` is generated from the same file. Under `AUCTION_PROVE=1` nothing is recorded and the summary line is `PROOF[..] TOTAL`, so prover and executor cycles are never mixed. The baseline is the local native build; CI measures its own native build. Executor cycles are deterministic for one ELF and input, and toolchain differences between native builds stay well inside 10 %. Measured on the first hosted run: the CI cycles differ from the local baseline on 16 of the 17 labels, the largest relative difference being 0.082 % (`start_surplus(burn)`); the gate passed and the baseline was kept. The 17 measured totals are within 0.63 % of the ADR-036 table (Plan 3a and 3b code changes since). The P2 load test asserts two things: a first-round bid with 6 live auctions stays within 5 % of the same bid with exactly 1 live auction (1,611,475 to 1,612,210 cycles against a reference of 1,612,060), and the 18 interleaved bids (6 auctions, 3 rounds) are uniform within 5 % (measured spread 0.05 %).

**Trade-offs**
Executor cycles, not proving time: proving is tiered separately (ADR-045). Refreshing the baseline is a reviewed change in the same commit as its cause.

**Status**
Accepted (Plan 3b, Task 6)

---

## ADR-049: Coverage Gate: 80 % Lines per Crate, Our Five Crates, Tests Excluded

**Context**
Spec §9: TDD throughout and at least 80 % coverage on the Rust crates. A workspace total would let a well-tested crate hide a weak one, and counting test files inflates the figure.

**Options**
- (A) `--fail-under-lines 80` on the workspace total.
- (B) Per-crate threshold over production files only.
- (C) No gate, a report only.

**Decision**
(B). `cargo llvm-cov` over `auction_core`, `auction_engine_core`, `auction_engine_program`, `auction_sdk` (features `testing`, `rpc`) and `auction_cli`; files matching `(/tests/|_tests\.rs$|/testing/|/methods/|/examples/)` are not counted (test code, test fixtures, the guest wrappers that run only inside the zkVM, the localnet tool). `scripts/check-coverage.py` requires 80 % of lines per crate. Measured at Plan 3b Task 8: `auction_core` 99.4 %, `auction_engine_core` 95.6 %, `auction_engine_program` 80.3 %, `auction_sdk` 93.7 %, `auction_cli` 88.5 %. Excluded production files: None.

**Trade-offs**
Line coverage, not branch coverage. Guest code (`methods/guest`) runs in the zkVM and is covered by the real-guest integration suites, not by this gate.

**Status**
Accepted (Plan 3b, Task 8)

---

## ADR-050: Localnet Price Publisher by Oracle Rotation

**Context**
Localnet finding 2: the localnet oracle has no publisher, so it goes stale after an hour, and without a price move there is no shortfall, hence no F4 debt auction and no freeze path on localnet. The oracle is a twap_oracle `OraclePriceAccount` created at RAY scale; twap's `PublishPrice` writes a Q64.64 TWAP of ticks (lez-programs #401), and the engine and host read one scale per instance (spec §5.2, `Ray` in the PoC).

**Options**
- (A) Use twap `PublishPrice` and switch the instance to `Q64x64` (the host reads 1e27; mixing scales is the open upstream question).
- (B) A test-only oracle program.
- (C) Rotation: a new RAY price account for the same source at the next free window, the host's `SetMarketPriceOracle`, then the engine's `set_collateral_config` with unchanged parameters and the new oracle.

**Decision**
(C), as `auction-cli dev oracle-publish` and `dev keeper` (localnet only, ADR-046). The price is checked on the engine's own feed path (`oracle::check_new_price`: `feed::observe` + `feed::require_live`) before anything is signed; both admins are checked before signing. The keeper re-publishes the current price when the oracle's age plus `--refresh-before-ms` reaches the configured maximum age. Proved on the real guests mid-auction (`rotating_the_oracle_mid_auction_keeps_bids_on_the_new_feed`).

**Trade-offs**
Three transactions per move. Between the host rotation and the engine follow-up, `liquidate` fails with `OracleMismatch` (ADR-028); a failure there leaves the two pointing at different oracles, and re-running `oracle-publish` fixes it. Each move uses a new account (accounts are never released; localnet only). Production needs a real publisher once the oracle scale is settled (M2).

**Status**
Accepted (Plan 3b, Task 10)

---

## ADR-051: Demo Scenario Parameters

**Context**
Spec §1.1: the one-command reproduction must drive a shortfall into the debt auction, a surplus into the surplus auction, and show the "no protocol token, so the host freezes new debt" path. With the default band (9000–11000 bps) the clamped price cannot leave the lot short of the debt, and a localnet run is too slow to tune by trial.

**Options**
- (A) Make positions underwater through the stability fee (slow, host-parameter dependent).
- (B) Move the market price inside the validity range with a wider band.
- (C) Inject state (not possible on a real node).

**Decision**
(B), pinned on the real guests by `demo_scenario_runs_f1_to_f5_and_the_freeze_path` with `auction_sdk::demo` (the same values as `cli/params/demo-*.json`, pinned by `the_demo_files_carry_the_pinned_scenario`). The defaults change in three places: `band_hi_bps = 20000` (the bound in spec §5.12), `debt_lot = 5000` and `bad_debt_freeze_threshold = 5000`. The price moves to 1.9 R (valid: R/2 ≤ 1.9 R ≤ 2R). A 500_000 / 300_000 position (R = 1) then raises at most about 257_850 coin from its lot, leaving a shortfall of at least about 42_150 whatever the discount at inclusion (2 % to 10 %). After the buffer (29_500) and one debt lot (5_000), at least about 7_650 of bad debt remains, above the threshold, so disabling the protocol token lets `check_solvency` freeze the host.

**Trade-offs**
The demo's band is wider than the PoC default; that is a parameter choice within spec bounds, stated in DEMO.md. The in-process scenario runs at R = 0.5 with twice the debt (the harness's standard position): same proportions, different absolute numbers.

**Status**
Accepted (Plan 3b, Task 11)

---
