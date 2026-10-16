# RFP-014 requirements: evidence, milestones and descopes

Every hard requirement of RFP-014 maps to evidence in this repository (PoC), a proposal milestone (M1–M7,
spec §15), the thin GUI of Plan 4, or an honest descope. `scripts/check-requirements.py` (CI job
`requirements`) fails if a named test, golden file, script, CI job or document does not exist, if an
`#[ignore]`d test is not marked `[ignored]`, or if a required id is missing. Status meanings: **PoC** shown
here; **PoC (partial)** shown here in part, the rest promised; **Plan 4** in this PoC's next plan (FFI,
module, thin Basecamp app); **M#** a proposal milestone; **Descoped** with the reason.

The checker proves that evidence exists, not that a CI job has run on a hosted runner. Where a hosted run
has not happened yet, the row says so in its notes and is at most **PoC (partial)**.

| ID | Requirement | Status | Evidence | Notes |
|---|---|---|---|---|
| F1 | Permissionless liquidation with a liquidator incentive | PoC | `liquidate_happy_path`, `keeper_reward_to_fresh_holding`, `underwater_reward_is_capped_at_the_seizure`, golden:liquidate | Reward carved from the penalty, capped (spec §5.9). |
| F2 | Increasing discount over time, configurable schedule, early termination at target | PoC | `final_bid_reaches_target_and_settles_once`, `schedule_change_affects_only_new_auctions`, `unsold_auction_holds_max_discount_and_keeps_its_reservation`, `discount_frozen_while_paused` | No expiry: an unsold auction holds the maximum discount (ADR-012). |
| F3 | Partial lots | PoC | `partial_bid_burns_debt_first_and_leaves_accounting_alone`, `lot_binds_charges_only_for_the_lot_and_books_the_shortfall`, `split_bid_neutrality` | |
| F4 | Debt auction for shortfalls (mock token, configurable token); buffer absorbs shortfalls; freeze new debt above the buffer without a token | PoC | `debt_auction_mints_against_burned_coin`, `mint_never_exceeds_the_absolute_cap`, `no_protocol_token_freezes_above_threshold_and_only_the_freeze_authority_unfreezes`, `shortfall_at_threshold_does_not_freeze`, `demo_scenario_runs_f1_to_f5_and_the_freeze_path`, script:scripts/demo.sh | Interpretation: the freeze triggers above an absolute admin-set threshold (default 1,000 LSC, at least one debt lot), not at the first unit above the buffer, so one unit of rounding cannot freeze the stablecoin (spec §5.11, ADR-017). Hardening M3. |
| F5 | Surplus auction for a protocol token, burned; buffer accumulates without a token | PoC | `surplus_auction_burns_protocol_tokens`, `take_surplus_max_amount_fills_lot`, `close_after_ttl_returns_the_remainder`, golden:take_surplus | Hardening M3. |
| F6 | Strict, forward-only, atomic state transitions | PoC | `shortfall_is_booked_once_and_a_settled_lot_binding_auction_rejects_bids`, `settle_empty_applies_once_and_blocks_further_bids`, `auction_invariants` | |
| F7 | Well-defined host interface; one configured instance per product | PoC | `host_admin_binds_engine_through_the_guest`, `instances_have_distinct_programs_and_state`, `rogue_config_with_the_same_salt_cannot_liquidate_the_bound_host`, doc:docs/architecture/hooks.md | Hooks offered upstream as a patch series (`patches/stablecoin`, PR draft in docs/outreach). |
| U1 | Built with SPEL; IDL generated | PoC | `every_engine_variant_has_a_sample_and_an_idl_entry_at_its_enum_index`, `stablecoin_idl_position_maps_to_enum_index`, ci:core.yml/idl-drift | spel-cli mis-encodes stablecoin positions 8–18; our SDK encodes enum indices (ADR-040); upstream issue draft in docs/outreach. |
| U2 | Logos mini-app GUI loadable in Basecamp | Plan 4 | | The thin app (Live auctions view, bid flow) is Plan 4; the full GUI per docs/design/ui is M5 (spec §7). |
| U3 | CLI covering the essential operations | PoC | golden:liquidate, golden:bid_partial, golden:bid_final, golden:take_debt, golden:list_auctions, doc:cli/README.md | |
| U4 | Undercollateralised positions with liquidator incentive; trigger liquidation | PoC (partial) | golden:list_at_risk, golden:quote_liquidation, `liquidation_quote_equals_the_auction_the_engine_opens` | CLI shows it; the GUI At-risk view is M5. |
| U5 | Active auctions with discount, raised vs target, time; bid previews | PoC (partial) | golden:quote_bid, golden:list_auctions_held_at_max, `bid_quote_at_inclusion_time_equals_the_on_chain_fill`, `stale_quote_is_protected_by_min_collateral_out` | CLI now; GUI Live view in Plan 4. |
| U6 | SDK for modules, with the deshield-interact-reshield private path | PoC (partial) | `liquidate_bid_settle_and_claim_via_sdk_plans`, `verify_config_rejects_every_mismatch`, doc:docs/architecture/sdk.md | Public SDK here; the private path is M4 (spec §8). |
| R1 | Non-reentrant operations, ordered transitions | PoC | `laundering_freeze_through_an_intermediate_program_is_rejected`, `engine_takes_no_program_id_argument`, `set_liquidation_engine_through_a_chained_call_is_rejected` | LEZ v0.2.4 has no reentrancy guard; R1 rests on our programs never calling back (THREAT_MODEL). |
| R2 | Time-weighted or confidence-adjusted prices; stale feeds pause that type | PoC | `liquidate_rejects_stale_feed_and_admin_pause`, `low_confidence_and_out_of_range_open_the_clock`, `bid_rejects_invalid_feed`, `derived_policy_bounded_when_publishes_observed` | Honesty note: the twap producer's Q64.64 scale is not yet reconciled with the host's 1e27 (lez-programs #401), so the PoC uses a RAY price account; every current producer writes confidence 0, so the confidence check never triggers today. Live TWAP is M2. |
| R3 | Concurrent, isolated auctions | PoC | `r3_a_bid_touches_only_its_own_auction`, `load_concurrent_auctions_with_interleaved_bids_stay_isolated` | |
| R4 | A failed feed pauses only that type | PoC | `stale_feed_on_a_does_not_pause_b`, `stale_feed_on_b_does_not_pause_a`, `two_type_pause_isolation` | The host has one collateral type per instance; shown with two host+engine instances (spec §5.4, ADR-035). |
| R5 | Other types and non-liquidation operations continue | PoC | `admin_pause_on_a_does_not_pause_b`, `admin_pause_on_b_does_not_pause_a`, `pause_isolation_offsets_open_pause_and_discount` | As R4. |
| R6 | Seized = in auction; shortfalls accounted; system balance | PoC | `collateral_invariants_hold_over_random_bids`, `system_invariants_burn_and_mint_totals`, `inv10_debt_clear_matches_seizure_over_full_chain` | |
| P1 | Liquidation in one block | PoC | `every_instruction_fits_the_chain_budget`, script:scripts/check-cycles.py, doc:CYCLES.md | One transaction, at most 5 executions, within 32 Mi cycles each. |
| P2 | Many concurrent auctions within compute limits | PoC | `load_concurrent_auctions_with_interleaved_bids_stay_isolated`, ci:core.yml/programs, doc:CYCLES.md | |
| P3 | Bid in one block | PoC | `every_instruction_fits_the_chain_budget`, `debt_and_solvency_chains_fit_the_budget`, doc:CYCLES.md, doc:docs/evidence/real-proofs-2026-10.md | One transaction per bid: bid is 3 executions and bid_final 4, each far below 32 Mi cycles. Real receipts for every bid execution come from the local full-suite run in the evidence document; hosted CI proves only `settle_debt` on pushes to main (ADR-045), so the `prove_bid_with_settlement` test is not cited as CI evidence. |
| S1 | Deployed and tested on LEZ devnet/testnet | M6 | | Blocked: the testnet is v0.3 and SPEL has no v0.3 support (spec §3.1); the PoC reproduces on a standalone localnet (DEMO.md). |
| S2 | End-to-end tests against a standalone sequencer with `RISC0_DEV_MODE=0`, in CI | PoC (partial) | ci:e2e.yml/localnet-e2e, script:scripts/demo.sh, doc:DEMO.md, doc:docs/architecture/real-proofs.md | Pending: the workflow is on `feat/plan-3b` only and has not yet run on a hosted runner; the `scripts/demo.sh --sequencer-dev-mode 0` flow was run locally (973 s, 37 transactions, every check ok, DEMO.md). LEZ v0.2.4 executes public transactions without proving them; real receipts per execution are produced separately (ADR-045, `docs/evidence/real-proofs-2026-10.md`). The first hosted run is recorded after the merge. |
| S3 | CI green on the default branch | PoC (partial) | ci:core.yml/core, ci:core.yml/programs, doc:docs/architecture/ci.md | `core.yml` is green on hosted runs of `feat/plan-3b` (recorded in docs/architecture/ci.md); green on `main` is pending the merge. |
| S4 | Every hard requirement has a test | PoC | script:scripts/check-requirements.py, ci:core.yml/requirements | This file. |
| S5 | README with deployment, addresses and CLI/mini-app walkthrough | PoC (partial) | doc:README.md, doc:DEMO.md, doc:DEPLOYMENTS.md, doc:cli/README.md | Mini-app walkthrough in Plan 4. |
| PRIV1 | Public and deshield-interact-reshield paths; reshield not skippable | M4 | | Spec §8: public transactions only in the PoC; designs exist (docs/design/ui). |
| PRIV2 | Reshield target must be private; explicit error otherwise | M4 | | As PRIV1. |
| PRIV3 | Fresh ephemeral account per operation | M4 | | As PRIV1. |
| PA1 | Private flow for liquidate, bid, settle | M4 | `private_path_liquidate_is_not_expressible_on_v0_2_4` | `liquidate` is not private-capable on v0.2.4 with this chain shape (ADR-034); bid and settle in M4. |
| PA2 | Atomic deshield of token and gas | M4 | | As PRIV1. |
| PA3 | Outputs reshielded; account never reused | M4 | | As PRIV1. |
| PA4 | What is public and what is private | M4 | | `docs/PRIVACY.md` does not exist yet: a stub is planned in Plan 3b Task 17; the full document is M6. |
| SOFT1 | Multi-oracle redundancy | M2 | | Single validated feed with pause in the PoC (spec §2.2). |
| SOFT2 | Formal verification of auction invariants | PoC (partial) | `collateral_invariants_hold_over_random_bids`, `mul_div_floor_never_exceeds_ceil`, `schedule_bounded` | Property tests run in CI. The two Kani harnesses (`crates/auction_core/src/kani_proofs.rs`, `mul_div` and the schedule) exist but no CI job runs Kani; more in M6. |
| SOFT3 | Liquidator bot SDK | PoC (partial) | golden:watch_observe, doc:docs/architecture/sdk.md | `watch` and the SDK scanner; a bot SDK product is M4. |
| PEND2 | CLI documentation | PoC | doc:cli/README.md | |
| PEND4 | Privacy and anonymisation document | M6 | | Not written yet: a stub is planned in Plan 3b Task 17 (`docs/PRIVACY.md`), the full document is M6. |
| PEND7 | Design decision log | PoC | doc:docs/design-log.md | |
| PEND8 | Audit programme | M7 | | Audit line in the proposal (spec §15). |
| PEND10 | Committed, reproducible benchmark harness | PoC | script:scripts/cycles.sh, doc:CYCLES.md | |
| PEND11 | Threat model (adversary classes, C- and F-class) | PoC | doc:THREAT_MODEL.md | |
