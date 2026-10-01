# Spikes (Week 1)

Each spike writes `docs/spikes/Sx.md` with:
- **Question:** the decision point
- **Method:** how to answer it
- **Commands run:** exact invocations
- **Raw numbers:** results
- **Decision:** outcome and implications
- **ADR link:** the design decision it informs

Spike code in `spikes/` is throwaway and excluded from the workspace.

---

## S1: Toolchain and Multi-Instance Setup

**Question**
Does the toolchain build and run end to end (guest, sequencer, stablecoin init, Basecamp harness with lez_core 0.4.1)? How many chained calls does `liquidate` really take? Can a second stablecoin build (instance tag constant → different image id) be deployed and initialised next to the first?

**Rationale**
SPEL's build, sequencer startup, and chained-call depth are unknowns at the spec stage. The second stablecoin instance is required by R4/R5 (the pause test across two collateral types, each with its own engine instance). Each host instance needs a separate binary with a distinguishing compile-time constant.

---

## S2: Host Authentication for the Engine

**Question**
How does the host authenticate the engine: `caller_program_id`, PDA signer, or both?

**Rationale**
The hook `liquidate_position` must verify the caller is the engine, not a random program. Three patterns exist; cycles and unforgeability differ.

---

## S3: LiquidationRecord Creation and Auction PDA Derivation

**Question**
Can SPEL create `LiquidationRecord` on first use (`Claim::Pda`) and derive arg-seeded auction PDAs?

**Rationale**
ADR-004 rests on PDA derivation from `(position, nonce)`. SPEL's `Claim::Pda` must work on first creation.

---

## S4: Discount Curve Shape

**Question**
Discount curve: piecewise-linear vs exponential vs stairstep?

**Rationale**
Efficiency, griefability, and predictability differ. Piecewise-linear is favored; S4 measures cycles and accuracy.

---

## S5: Pause Onset Timing

**Question**
When does a stale period start?
- (a) at observation time;
- (b) at `last_seen_oracle_ts + max_age`;
- (c) (b) bounded by the oracle's history.

**Rationale**
ADR-007's frozen-discount property test must pass. The choice affects when discounts stop advancing.

---

## S6: Accounting Update Timing

**Question**
`Accounting` updated per bid, or only at chain end?

**Rationale**
Concurrency and R6 (conservation) depend on when `unbacked_debt` and other counters are updated.

---

## S7: Mock Protocol Token Mint Authority

**Question**
Mock protocol token mint authority as an engine PDA; cost of mint and burn in chained calls.

**Rationale**
F4 (debt auction) needs to mint; F5 (surplus auction) needs to burn. Costs are measured in cycles against the 32 Mi budget.
