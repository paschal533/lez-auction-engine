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

**Decided by**
Running it

---

## S2: Host Authentication for the Engine

**Question**
How does the host authenticate the engine: `caller_program_id`, PDA signer, or both?

**Decided by**
Unforgeability tests + cycles + upstream fit

---

## S3: LiquidationRecord Creation and Auction PDA Derivation

**Question**
Can SPEL create `LiquidationRecord` on first use (`Claim::Pda`) and derive arg-seeded auction PDAs?

**Decided by**
A prototype

---

## S4: Discount Curve Shape

**Question**
Discount curve: piecewise-linear vs exponential vs stairstep?

**Decided by**
Cycles + accuracy

---

## S5: Pause Onset Timing

**Question**
When does a stale period start?
- (a) at observation time;
- (b) at `last_seen_oracle_ts + max_age`;
- (c) (b) bounded by the oracle's history.

**Decided by**
The §5.7 property test

---

## S6: Accounting Update Timing

**Question**
`Accounting` updated per bid, or only at chain end?

**Decided by**
Concurrency + R6 proof

---

## S7: Mock Protocol Token Mint Authority

**Question**
Mock protocol token mint authority as an engine PDA; cost of mint and burn in chained calls.

**Decided by**
A prototype + cycles
