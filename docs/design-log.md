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
(C): An oversized position `(amount_to_raise > cap − reserved)` is allowed only if both type and global reservations are 0 (it goes alone). When it settles, it releases its (over-size) reservation. There is no restart: a fixed starting price would mean restarting just resets the discount to the start, pointless. There is no expiry: `redo` in Maker exists because Maker fixes the starting price; we don't. Expiry would book a shortfall while unsold collateral exists (minting against recoverable debt, dilution without loss). Instead, unsold auctions sit at max discount until settled.

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
With a working protocol token configured, `check_solvency` never freezes, even if debt auctions keep closing untaken (accepted residual recorded in `THREAT_MODEL.md`; governance monitors). The freeze threshold departs from F4's literal "exceeds the buffer" wording but prevents one-unit rounding errors from freezing the system.

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
Variant C (A and B together). The brief's decision rule ("cheapest variant passing all four tests") rested on an incomplete attack set and is superseded by the spec's requirement that only the engine can invoke the hook. A's check uses the immediate caller, which lee enforces per call (`MismatchedCallerProgramId`, `validated_state_diff/mod.rs:184-191`). Cycles of the legitimate transaction (caller + callee, 2 executions): A 367,354, B 372,316, C 372,601; C costs +285 over B (+0.08%) and +5,247 over A (+1.43%).
B's guarantee, stated correctly: the engine authority PDA is authorised somewhere in the chain that led to the call; it does not say the engine is the caller.
Invariants for Plan 2 (defence in depth): (i) the engine never authorises `ENGINE_AUTH` for any program other than the stored host program id (no caller-supplied program ids); (ii) the host never chains to an untrusted or user-chosen program while the engine authority is authorised in its call.

**Trade-offs**
- Zero ids: SPEL sets `ctx.caller_program_id` to zeros for top-level calls (`spel-framework-core/src/context.rs:30-33`), so A and C accept a direct call if the stored `engine_program_id` is zero or uninitialised. The config initialiser must reject zero program ids and zero authorities (Plan 2 test). The config was inserted with `force_insert_account`; creation and rotation are untested.
- Private path untested, wider scope: in the circuit `authorized_accounts` is one set for the whole transaction (`privacy_preserving_circuit/src/execution_state.rs:54,601-608`), so laundering reaches sibling calls too; forgery is still blocked (`output.rs:38`, `circuit/mod.rs:420-433`, `execution_state.rs:285-305`); a PDA first seen inside a chained call is not seed-checked in the Vacant branch, so the engine must list the authority in its own accounts. `caller_program_id` is enforced in the circuit per call (`execution_state.rs:164-170`), so C's immediate-caller check holds there, but this is from source reading only; a private-path test is an open item for Plan 2.
- Both A and B bind to the engine ImageID (PDA derived from the program id): a second or upgraded engine needs its own stored config.
- Upstream fit of B: the stablecoin uses plain `is_authorized` assertions (`lez-programs/programs/stablecoin/src/checks.rs:109`, `src/freeze.rs:58`); those have the same laundering property when the authority is a PDA.
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
