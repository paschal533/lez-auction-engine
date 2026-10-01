# Plan 1: Foundations, Spikes and `auction_core`

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Set up the repository and its CI. Run the toolchain and platform spikes (S1–S4, S6, S7) that decide the on-chain design. Deliver the complete, property-tested, LEZ-independent `auction_core` crate, including spike S5, which is resolved inside the core.

**Architecture:**
- `auction_core` is a pure Rust library (`no_std` outside tests, with no LEZ dependency) holding all math, schedules, pricing, the pause clock, the collateral, surplus and debt auction state machines, and accounting.
- Values are immutable: every state transition returns a new struct.
- Spikes live in `spikes/` as throwaway crates. Their *results* go into `docs/spikes/Sx.md` and ADRs in `docs/design-log.md`; their code is not kept.
- Plans 2–4 (programs, SDK/CLI/CI, thin GUI) are written after this plan's spikes report.

**Tech Stack:**
- Rust 1.94.0, `proptest` and `ruint` (dev-only).
- Python 3 for the differential model.
- Kani (optional; last in the cut order).
- For spikes: LEZ v0.2.4, SPEL `1ef0500f`, risc0 3.0.5, Docker, Nix, WSL Ubuntu 20.04.

**Spec:** `docs/superpowers/specs/2026-10-01-rfp014-auction-engine-poc-design.md` (v5, approved). Read §3, §5 and §13 before starting any task.

## Global Constraints

- Rust toolchain `1.94.0` (file `rust-toolchain.toml`). `#![forbid(unsafe_code)]` in every crate. `[profile.release] overflow-checks = true`.
- `auction_core` has **no runtime dependencies**. Dev-dependencies are allowed: `proptest = "1"`, `ruint = "1"`.
- Units:
  - amounts are raw base units as `u128`;
  - prices are RAY = `1_000_000_000_000_000_000_000_000_000` (1e27), collateral per 1 stablecoin;
  - discounts are bps (`u16`, 0–10_000), with `1 bps = 1e23` in RAY;
  - time is `u64` milliseconds.
- Rounding (spec §5.1): **amounts paid out round down; amounts owed or charged round up.**
- Every product goes through `mul_div_floor` / `mul_div_ceil` with a 256-bit intermediate. Never write `a * b / c` on `u128`.
- Bounds (spec §5.12):
  - schedule: ≤ 8 points, max discount ≤ 3000 bps;
  - penalty: 0–2500 bps;
  - band: lo ≤ 10_000 ≤ hi, within [5000, 20000];
  - max oracle age: ≤ 24 h;
  - max confidence: ≤ 1000 bps.
- Pins for spikes:
  - LEZ v0.2.4 `47eba256479f6f785acbd138834340703cd03401`;
  - SPEL `1ef0500f8fc8ce3ddf95523726ae159db83f744f`;
  - risc0-zkvm `=3.0.5`;
  - lez-programs `64c01210adecd6b557dbd021a3cc94553f243fd8`;
  - lez_core `acf0cd501b262c4c15969e3735e85318297b85bf`.
- Commits use conventional-commit format and end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Nothing is pushed without the user's approval.
- Docs are updated in the same task as the code they describe (`docs/design-log.md`, `docs/architecture/math.md`, `docs/spikes/`).

## Review Focus

Inputs the spec implies but no happy-path test exercises, ranked by how likely they are to bite. Each line has a pinned test in the task that owns the code.

1. **A bid that exhausts the lot and reaches the target in the same fill.** Expected: Settled, `leftover_claimable == 0`, `shortfall == 0`, and the bidder is charged no more than `coin_in_max`. Pinned in Task 9.
2. **A bid with `coin_in_max = u128::MAX`.** Expected: no overflow; the bid is capped at the remaining target and charged exactly the capped amount. Pinned in Task 9.
3. **An oracle timestamp later than `now`** (the clock reads the previous block). Expected: age 0, the feed is valid, no panic. Pinned in Task 6.
4. **A position whose debt is below `debt_floor`.** Expected: still liquidatable, keeper reward 0, no division-by-zero. Pinned in Task 8.
5. **A single-point schedule** (constant discount), with `t_eff` before, at and long after its point. Expected: a constant discount, no out-of-bounds index. Pinned in Task 5.

---

## File structure (this plan)

```
lez-auction-engine/
  Cargo.toml                      workspace: members = ["crates/auction_core"]; spikes excluded
  rust-toolchain.toml             1.94.0
  .github/workflows/core.yml      fmt, clippy -D warnings, test, proptest, python differential
  crates/auction_core/
    Cargo.toml
    src/lib.rs                    re-exports; #![no_std] + forbid(unsafe)
    src/math.rs                   mul_wide, div_wide, mul_div_floor/ceil, MathError
    src/units.rs                  RAY, BPS, bps_to_ray, constants and bounds
    src/error.rs                  ConfigError, AuctionError, PauseReason, SystemAuctionError
    src/schedule.rs               Schedule, SchedulePoint, discount_ray
    src/price.rs                  OracleScale, OracleReading, FeedParams, to_ray, validate_feed, bid_price, effective_ref_price
    src/pause.rs                  OnsetPolicy, PauseClock (observed, effective_elapsed)
    src/liquidation.rs            CollateralParams, CapState, LiquidationInput, LiquidationQuote, quote_liquidation, chost
    src/auction.rs                Status, Auction, BidRequest, BidFill, quote_bid, apply_bid, claim_leftover, ChainEnd
    src/accounting.rs             Accounting (record_bid, record_chain_end, settle_debt)
    src/system.rs                 SurplusAuction, DebtAuction, start/take/close, check_solvency, SolvencyDecision
    tests/invariants.rs           proptest model: invariants 1–17, split-bid neutrality, two-type pause
    tests/s5_pause_onset.rs       spike S5 property tests
  tools/differential/model.py     Python big-int reference for mul_div, schedule, quote_bid
  tools/differential/check.py     reads vectors from `cargo run --example vectors`, compares
  crates/auction_core/examples/vectors.rs   emits JSON vectors
  spikes/                         throwaway crates for S1–S4, S6, S7 (excluded from workspace)
  docs/design-log.md              ADR-001..017 seeded; spike outcomes appended
  docs/spikes/S1.md … S7.md       one per spike: question, method, raw numbers, decision
  docs/architecture/math.md       how the math and rounding work
  docs/architecture/core.md       how auction_core's state machines work
```

---

### Task 1: Workspace, CI skeleton, design log

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `crates/auction_core/Cargo.toml`, `crates/auction_core/src/lib.rs`, `.github/workflows/core.yml`, `docs/design-log.md`, `docs/spikes/README.md`

**Interfaces:**
- Produces: the crate `auction_core` (empty modules), and `docs/design-log.md` with ADR-001…017.

- [ ] **Step 1: Create the workspace files**

`Cargo.toml`:
```toml
[workspace]
members = ["crates/auction_core"]
exclude = ["spikes"]
resolver = "2"

[workspace.package]
edition = "2021"
license = "MIT OR Apache-2.0"

[profile.release]
overflow-checks = true

[profile.test]
overflow-checks = true
```

`rust-toolchain.toml`:
```toml
[toolchain]
channel = "1.94.0"
profile = "default"
```

`crates/auction_core/Cargo.toml`:
```toml
[package]
name = "auction_core"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[dependencies]

[dev-dependencies]
proptest = "1"
ruint = "1"
```

`crates/auction_core/src/lib.rs`:
```rust
//! Pure, LEZ-independent logic for the RFP-014 liquidation & auction engine.
//! Every state transition returns a new value; nothing here performs I/O.
#![cfg_attr(not(test), no_std)] // unit tests (proptest) need std; the library itself is no_std
#![forbid(unsafe_code)]
#![deny(clippy::arithmetic_side_effects)]

#[cfg(test)]
mod smoke {
    #[test]
    fn crate_builds() {}
}
```

- [ ] **Step 2: Run the smoke test**

Run: `cargo test -p auction_core`
Expected: `test smoke::crate_builds ... ok`

- [ ] **Step 3: Add the CI workflow**

`.github/workflows/core.yml`:
```yaml
name: core
on: [push, pull_request]
jobs:
  core:
    runs-on: ubuntu-22.04
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.94.0
        with: { components: "rustfmt, clippy" }
      - run: cargo fmt --all -- --check
      - run: cargo clippy -p auction_core --all-targets -- -D warnings
      - run: cargo test -p auction_core
      - run: PROPTEST_CASES=2000 cargo test -p auction_core --test invariants --test s5_pause_onset
      - uses: actions/setup-python@v5
        with: { python-version: "3.12" }
      - run: cargo run -q -p auction_core --example vectors > /tmp/vectors.json && python3 tools/differential/check.py /tmp/vectors.json
```
(The last two steps fail until Tasks 11–12 exist. Commit the workflow now anyway, with these two steps commented out; Task 12 uncomments them.)

- [ ] **Step 4: Seed the design log**

`docs/design-log.md`: write one section per ADR-001…ADR-017. Each section has the headings **Context / Options / Decision / Trade-offs / Status**, with content summarised from spec §17 and the referenced spec sections. All are `Status: Accepted (spec v5)`, except ADR-007's onset policy, which is `Status: Pending spike S5`. Add a header line: "Every spike outcome is appended here as a new ADR or an amendment."

`docs/spikes/README.md`: list S1–S7 with their questions, copied from spec §13, and the rule "each spike writes `docs/spikes/Sx.md` with: question, method, commands run, raw numbers, decision, ADR link; spike code in `spikes/` is throwaway and excluded from the workspace."

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml rust-toolchain.toml crates .github docs/design-log.md docs/spikes/README.md
git commit -m "chore: scaffold workspace, auction_core crate, CI and design log"
```

---

### Task 2: Spike S1, toolchain, standalone sequencer, second host build, chained-call count

**Files:**
- Create: `spikes/s1/README.md`, `spikes/s1/run.sh`, `docs/spikes/S1.md`
- Modify: `docs/design-log.md` (append ADR-018 "S1 outcome")

**Interfaces:**
- Produces, for Plans 2–3: the exact working commands to build guests, start a standalone v0.2.4 sequencer, deploy and initialise the stablecoin, build a **second** stablecoin binary with a different image id, and boot the offscreen Basecamp harness with lez_core 0.4.1. Also the measured chained-call depth of a `liquidate`-shaped transaction.

Run everything inside WSL Ubuntu 20.04 (`wsl -d Ubuntu-20.04`). Before starting, set `memory=12GB` and `swap=16GB` in `%UserProfile%\.wslconfig`, then run `wsl --shutdown`.

- [ ] **Step 1: Clone the pinned sources into `spikes/s1/src`**

```bash
mkdir -p spikes/s1/src && cd spikes/s1/src
git clone https://github.com/logos-blockchain/logos-execution-zone lez && git -C lez checkout 47eba256479f6f785acbd138834340703cd03401
git clone https://github.com/logos-blockchain/lez-programs && git -C lez-programs checkout 64c01210adecd6b557dbd021a3cc94553f243fd8
```
Expected: both checkouts are at detached HEAD with the given SHAs.

- [ ] **Step 2: Build the lez-programs guests and run their tests (baseline)**

```bash
cd lez-programs && make build-programs && make test
```
Expected: guest `.bin` files under `programs/*/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/`, and `cargo test` green. Record the build wall time and any failure verbatim in `docs/spikes/S1.md`.

- [ ] **Step 3: Start the standalone sequencer**

```bash
cd ../lez && RUST_LOG=info cargo run --features standalone -p sequencer_service lez/sequencer/service/configs/debug
```
Expected: it logs that the RPC is listening, and the block height increases. In a second shell, run `curl -s -X POST localhost:<port> -H 'content-type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"getLastBlockId","params":{}}'`, taking the port from `configs/debug`. Expected: a number that grows over time.

- [ ] **Step 4: Deploy the stablecoin and initialise it through the wallet CLI**

Follow `lez-programs/docs/testnet-runbook.md`, pointed at the local sequencer: create a wallet, then `wallet deploy-program programs/stablecoin/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/stablecoin.bin`. Then initialise it with `initialize_program`, using a faucet-mintable collateral token and an oracle at 1e27 scale (spec §5.2). Record every command and the resulting ImageID in `docs/spikes/S1.md`.

- [ ] **Step 5: Build a second stablecoin binary with a different image id**

In a scratch copy of the stablecoin guest (`spikes/s1/src/lez-programs-b`), add the following to `programs/stablecoin/methods/guest/src/bin/stablecoin.rs`:
```rust
#[used]
static HOST_INSTANCE_TAG: [u8; 8] = *b"INST-B01";
```
Rebuild, then deploy and initialise next to the first. Expected: a different ImageID, two independent `ProtocolParameters` PDAs, and both instances usable. If `#[used]` does not change the image id, try an env-driven `const` read by an instruction path. Record which variant works.

- [ ] **Step 6: Count chained calls for a liquidate-shaped transaction**

Read `lee/state_machine/src/validated_state_diff/mod.rs` for `MAX_NUMBER_CHAINED_CALLS`. Then write `spikes/s1/chain_count.rs`: an in-process `V03State` test, modelled on `lez-programs/programs/integration_tests/tests/stablecoin.rs`, that runs one transaction doing init-holding → stablecoin `withdraw_collateral` (as a stand-in for the hook's vault transfer) → token transfer. Run it with `RISC0_DEV_MODE=1 cargo test`. Record the number of program executions and the executor cycles for each.

- [ ] **Step 7: Boot the offscreen Basecamp harness with lez_core 0.4.1**

Follow `lez-programs/apps/token/tests/run-basecamp-e2e.sh`: build `logos-basecamp#bin-bundle-dir-inspector` and the lez_core `acf0cd50` `install-portable` output, then launch offscreen with `QML_INSPECTOR_PORT=3768`. Expected: the inspector answers on `localhost:3768`. Record the logos-module-builder revision that matches this Basecamp build. Plan 4 pins it.

- [ ] **Step 8: Write the outcome and commit**

`docs/spikes/S1.md` sections: Question, Environment, Commands (verbatim), Results (times, ImageIDs, chained-call count, cycles), Problems hit, Decision. Append to `docs/design-log.md`: **ADR-018 S1 outcome**, covering the confirmed commands, the second-build method, the measured chained-call budget, and the module-builder pin.
```bash
git add spikes/s1/README.md spikes/s1/run.sh docs/spikes/S1.md docs/design-log.md
git commit -m "docs: record spike S1 toolchain and second-host-build results"
```
If any step is impossible, stop, record exactly what failed in S1.md, and report to the user. Do not work around a failed pin silently.

---

### Task 3: Spikes S2, S3 and S7, host authentication of the engine, SPEL PDA creation, mock-token minting

**Files:**
- Create: `spikes/s2_s3_s7/` (a SPEL workspace with two tiny programs), `docs/spikes/S2.md`, `docs/spikes/S3.md`, `docs/spikes/S7.md`
- Modify: `docs/design-log.md` (ADR-019 S2, ADR-020 S3, ADR-021 S7)

**Interfaces:**
- Produces: the decided authentication mechanism for `liquidate_position` / `engine_freeze_new_debt`; confirmed SPEL syntax for creating an arg-seeded PDA on first use; confirmed mint-by-PDA flow and its cycle cost.

- [ ] **Step 1: Scaffold two SPEL programs, `callee` and `caller`, pinned to SPEL `1ef0500f` and LEZ v0.2.4**

Copy the layout of `lez-programs/programs/token` (core, program and methods/guest). In `callee`, add the instruction `guarded(expected_caller: [u8;32])`, which stores nothing and panics unless the check under test passes. In `caller`, add `call_guarded()`, which makes a chained call to `callee.guarded`.

- [ ] **Step 2: S2, implement three variants of the guard in `callee` and test each**

- Variant A: `assert_eq!(ctx.caller_program_id, stored_engine_program_id)`.
- Variant B: require that an account equal to `stored_engine_authority_pda` is authorised in the call. The caller authorises it with `.with_pda_seeds(vec![seed])`.
- Variant C: both A and B.

For each variant, write in-process `V03State` tests:
- (1) the legitimate chained call from `caller` succeeds;
- (2) a top-level transaction calling `callee.guarded` directly fails;
- (3) a third program `impostor` that chains to `callee` fails;
- (4) a transaction listing the PDA account but signed by a user key fails.

Record cycles per variant. **Decision rule:** choose the cheapest variant that passes all four tests. If both pass, prefer B (PDA signer), because that mirrors the stablecoin's existing admin and freeze checks (upstream fit), unless A is more than 20% cheaper. Write `docs/spikes/S2.md` and ADR-019.

- [ ] **Step 3: S3, create a PDA on first use with an argument-derived seed**

In `caller`, add `open_record(position: [u8;32])`, which creates `["LIQREC", config, position]` if it is uninitialised (`Claim::Pda`) and increments a `count: u64`. Also add `open_auction(position: [u8;32], nonce: u64)`, which asserts `nonce == record.count`, creates `["AUCTION", config, position, nonce]`, and increments the count. Tests:
- the first call creates the record;
- the second call with the same nonce fails;
- the call with the next nonce succeeds;
- the off-chain PDA derivation equals the on-chain one.

Write `docs/spikes/S3.md` with the exact SPEL attribute syntax that worked, and ADR-020.

- [ ] **Step 4: S7, mint from an engine PDA, then burn**

Create a token definition whose mint authority is a `caller` PDA, following the token program's mint-authority model (see `lez-programs/programs/token_mint_authority`). In `caller`, add `mint_to(holder, amount)` (a chained `Token::Mint` with PDA seeds) and `burn_from(holder, amount)` (a chained `Token::Burn`, authorised by the holder). Tests:
- mint by the PDA succeeds;
- mint by a user key fails;
- burn by the holder succeeds.

Record the cycles. Write `docs/spikes/S7.md` and ADR-021.

- [ ] **Step 5: Commit**

```bash
git add spikes/s2_s3_s7 docs/spikes/S2.md docs/spikes/S3.md docs/spikes/S7.md docs/design-log.md
git commit -m "docs: record spikes S2, S3 and S7 (engine auth, PDA records, PDA mint)"
```

---

### Task 4: Math, 256-bit `mul_div` with explicit rounding

**Files:**
- Create: `crates/auction_core/src/math.rs`, `crates/auction_core/src/units.rs`, `docs/architecture/math.md`
- Modify: `crates/auction_core/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub enum MathError { DivByZero, Overflow }`
  - `pub fn mul_wide(a: u128, b: u128) -> (u128, u128)` returning `(hi, lo)`
  - `pub fn div_wide(hi: u128, lo: u128, d: u128) -> Result<(u128, u128), MathError>` returning `(q, r)`
  - `pub fn mul_div_floor(x: u128, y: u128, d: u128) -> Result<u128, MathError>`
  - `pub fn mul_div_ceil(x: u128, y: u128, d: u128) -> Result<u128, MathError>`
  - `units`: `pub const RAY: u128 = 1_000_000_000_000_000_000_000_000_000; pub const BPS: u128 = 10_000; pub const BPS_TO_RAY: u128 = 100_000_000_000_000_000_000_000;` and `pub fn bps_to_ray(bps: u16) -> u128`

- [ ] **Step 1: Write the failing tests** (at the end of `math.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use ruint::aliases::U256;

    #[test]
    fn small_values() {
        assert_eq!(mul_div_floor(7, 3, 2), Ok(10));
        assert_eq!(mul_div_ceil(7, 3, 2), Ok(11));
        assert_eq!(mul_div_ceil(6, 3, 2), Ok(9));
    }

    #[test]
    fn product_beyond_u128_but_quotient_fits() {
        // 1e27 * 2000e18 / 1e27 = 2000e18; the product is 2e48 > u128::MAX
        let ray = crate::units::RAY;
        let x = 2_000_000_000_000_000_000_000u128;
        assert_eq!(mul_div_floor(x, ray, ray), Ok(x));
    }

    #[test]
    fn errors() {
        assert_eq!(mul_div_floor(1, 1, 0), Err(MathError::DivByZero));
        assert_eq!(mul_div_floor(u128::MAX, u128::MAX, 1), Err(MathError::Overflow));
        assert_eq!(mul_div_ceil(u128::MAX, 1, 1), Ok(u128::MAX));
        assert_eq!(mul_div_ceil(u128::MAX, 2, 2), Ok(u128::MAX));
    }

    fn reference(x: u128, y: u128, d: u128) -> Option<(u128, u128)> {
        let p = U256::from(x) * U256::from(y);
        let (q, r) = p.div_rem(U256::from(d));
        if q > U256::from(u128::MAX) { return None; }
        Some((q.to::<u128>(), r.to::<u128>()))
    }

    proptest! {
        #[test]
        fn matches_u256_reference(x: u128, y: u128, d in 1u128..) {
            match reference(x, y, d) {
                None => prop_assert_eq!(mul_div_floor(x, y, d), Err(MathError::Overflow)),
                Some((q, r)) => {
                    prop_assert_eq!(mul_div_floor(x, y, d), Ok(q));
                    let c = if r == 0 { Some(q) } else { q.checked_add(1) };
                    match c {
                        Some(c) => prop_assert_eq!(mul_div_ceil(x, y, d), Ok(c)),
                        None => prop_assert_eq!(mul_div_ceil(x, y, d), Err(MathError::Overflow)),
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p auction_core math`
Expected: FAIL to compile, with "cannot find function `mul_div_floor`".

- [ ] **Step 3: Implement `units.rs` and `math.rs`**

`units.rs`:
```rust
//! Fixed-point units shared by every module.
#![allow(clippy::arithmetic_side_effects)] // bps ≤ 10_000 times a constant: cannot overflow
pub const RAY: u128 = 1_000_000_000_000_000_000_000_000_000;
pub const BPS: u128 = 10_000;
/// 1 bps expressed in RAY (1e27 / 1e4).
pub const BPS_TO_RAY: u128 = 100_000_000_000_000_000_000_000;

/// Converts basis points to RAY. `bps` ≤ 10_000, so this never overflows.
pub const fn bps_to_ray(bps: u16) -> u128 {
    (bps as u128) * BPS_TO_RAY
}
```

`math.rs`:
```rust
//! 256-bit-intermediate multiply-divide. Never write `a * b / c` on u128 elsewhere.
#![allow(clippy::arithmetic_side_effects)] // every operation below is bounded; see comments

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathError {
    DivByZero,
    Overflow,
}

const LO64: u128 = u64::MAX as u128;

/// Full 256-bit product of two u128s as (hi, lo).
pub fn mul_wide(a: u128, b: u128) -> (u128, u128) {
    let (a1, a0) = (a >> 64, a & LO64);
    let (b1, b0) = (b >> 64, b & LO64);
    // Each partial product is a u64*u64 and fits in u128.
    let p00 = a0 * b0;
    let p01 = a0 * b1;
    let p10 = a1 * b0;
    let p11 = a1 * b1;
    // mid ≤ 3·(2^64−1) < 2^66, so it fits.
    let mid = (p00 >> 64) + (p01 & LO64) + (p10 & LO64);
    let lo = (p00 & LO64) | (mid << 64);
    let hi = p11 + (p01 >> 64) + (p10 >> 64) + (mid >> 64);
    (hi, lo)
}

/// (hi·2^128 + lo) / d as (quotient, remainder). Requires hi < d so the quotient fits in u128.
pub fn div_wide(hi: u128, lo: u128, d: u128) -> Result<(u128, u128), MathError> {
    if d == 0 {
        return Err(MathError::DivByZero);
    }
    if hi >= d {
        return Err(MathError::Overflow);
    }
    let mut rem = hi; // invariant: rem < d at the top of each iteration
    let mut q: u128 = 0;
    for i in (0..128).rev() {
        let carry = rem >> 127;
        rem = (rem << 1) | ((lo >> i) & 1);
        q <<= 1;
        // The true value is carry·2^128 + rem < 2d; subtracting d once restores rem < d.
        if carry == 1 || rem >= d {
            rem = rem.wrapping_sub(d);
            q |= 1;
        }
    }
    Ok((q, rem))
}

/// floor(x·y / d)
pub fn mul_div_floor(x: u128, y: u128, d: u128) -> Result<u128, MathError> {
    let (hi, lo) = mul_wide(x, y);
    div_wide(hi, lo, d).map(|(q, _)| q)
}

/// ceil(x·y / d)
pub fn mul_div_ceil(x: u128, y: u128, d: u128) -> Result<u128, MathError> {
    let (hi, lo) = mul_wide(x, y);
    let (q, r) = div_wide(hi, lo, d)?;
    if r == 0 { Ok(q) } else { q.checked_add(1).ok_or(MathError::Overflow) }
}
```

In `lib.rs`, add `pub mod math; pub mod units;` and remove the `smoke` module.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p auction_core math`
Expected: PASS (4 tests; the proptest runs 256 cases by default).

- [ ] **Step 5: Document and commit**

`docs/architecture/math.md` should explain: why there is no `u256` type (zkVM cost, no dependency); the limb decomposition in `mul_wide`; the restoring division and its `rem < d` invariant; the rounding rule (out = floor, owed = ceil) with a table of every call site, filled in as later tasks add them; and the overflow semantics.
```bash
git add crates/auction_core/src docs/architecture/math.md
git commit -m "feat(core): add 256-bit mul_div with floor/ceil rounding"
```

---

### Task 5: Discount schedule

**Files:**
- Create: `crates/auction_core/src/error.rs`, `crates/auction_core/src/schedule.rs`
- Modify: `crates/auction_core/src/lib.rs`

**Interfaces:**
- Consumes: `mul_div_floor`, `bps_to_ray`, `RAY`.
- Produces:
  - `pub enum ConfigError { EmptySchedule, TooManyPoints, FirstPointNotZero, TimeNotIncreasing, DiscountDecreasing, DiscountAboveMax, PenaltyAboveMax, BandInvalid, OracleAgeAboveMax, ConfidenceAboveMax, ChostBelowMinBid, MintCapAboveHardCap }`
  - `pub const MAX_POINTS: usize = 8; pub const MAX_DISCOUNT_BPS: u16 = 3000;`
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct SchedulePoint { pub elapsed_ms: u64, pub discount_bps: u16 }`
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct Schedule { points: [SchedulePoint; MAX_POINTS], len: u8 }`
  - `impl Schedule { pub fn new(points: &[SchedulePoint]) -> Result<Self, ConfigError>; pub fn points(&self) -> &[SchedulePoint]; pub fn discount_ray(&self, t_eff_ms: u64) -> u128; pub fn max_discount_ray(&self) -> u128 }`
  - `pub fn rai_default() -> Schedule` (2% → 10% over 2_700_000 ms)

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{bps_to_ray, BPS_TO_RAY};
    use proptest::prelude::*;

    fn p(e: u64, d: u16) -> SchedulePoint { SchedulePoint { elapsed_ms: e, discount_bps: d } }

    #[test]
    fn rai_default_interpolates_and_holds() {
        let s = rai_default();
        assert_eq!(s.discount_ray(0), bps_to_ray(200));
        assert_eq!(s.discount_ray(1_350_000), bps_to_ray(600)); // halfway: 6%
        assert_eq!(s.discount_ray(2_700_000), bps_to_ray(1000));
        assert_eq!(s.discount_ray(u64::MAX), bps_to_ray(1000)); // held forever (no expiry)
    }

    #[test]
    fn interpolation_rounds_down() {
        let s = Schedule::new(&[p(0, 0), p(3, 1)]).unwrap();
        // 1 bps over 3 ms → after 1 ms the exact value is BPS_TO_RAY/3, rounded down
        assert_eq!(s.discount_ray(1), BPS_TO_RAY / 3);
    }

    #[test]
    fn single_point_schedule_is_constant() { // Review Focus #5
        let s = Schedule::new(&[p(0, 500)]).unwrap();
        for t in [0u64, 1, 2_700_000, u64::MAX] {
            assert_eq!(s.discount_ray(t), bps_to_ray(500));
        }
    }

    #[test]
    fn validation() {
        assert_eq!(Schedule::new(&[]), Err(ConfigError::EmptySchedule));
        assert_eq!(Schedule::new(&[p(5, 0)]), Err(ConfigError::FirstPointNotZero));
        assert_eq!(Schedule::new(&[p(0, 0), p(0, 1)]), Err(ConfigError::TimeNotIncreasing));
        assert_eq!(Schedule::new(&[p(0, 5), p(1, 4)]), Err(ConfigError::DiscountDecreasing));
        assert_eq!(Schedule::new(&[p(0, 3001)]), Err(ConfigError::DiscountAboveMax));
        let nine: [SchedulePoint; 9] = core::array::from_fn(|i| p(i as u64, 0));
        assert_eq!(Schedule::new(&nine), Err(ConfigError::TooManyPoints));
    }

    proptest! {
        #[test]
        fn monotone_and_bounded(t1: u64, t2: u64) {
            let s = rai_default();
            let (a, b) = if t1 <= t2 { (t1, t2) } else { (t2, t1) };
            prop_assert!(s.discount_ray(a) <= s.discount_ray(b));
            prop_assert!(s.discount_ray(b) <= s.max_discount_ray());
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p auction_core schedule`
Expected: FAIL to compile (`Schedule` not found).

- [ ] **Step 3: Implement**

`error.rs`:
```rust
//! Error types shared across auction_core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    EmptySchedule, TooManyPoints, FirstPointNotZero, TimeNotIncreasing, DiscountDecreasing,
    DiscountAboveMax, PenaltyAboveMax, BandInvalid, OracleAgeAboveMax, ConfidenceAboveMax,
    ChostBelowMinBid, MintCapAboveHardCap,
}
```

`schedule.rs`:
```rust
//! Piecewise-linear discount schedule (spec §5.3, ADR-006). Snapshotted per auction.
use crate::error::ConfigError;
use crate::math::mul_div_floor;
use crate::units::bps_to_ray;

pub const MAX_POINTS: usize = 8;
pub const MAX_DISCOUNT_BPS: u16 = 3000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SchedulePoint { pub elapsed_ms: u64, pub discount_bps: u16 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Schedule { points: [SchedulePoint; MAX_POINTS], len: u8 }

impl Schedule {
    pub fn new(points: &[SchedulePoint]) -> Result<Self, ConfigError> {
        let first = points.first().ok_or(ConfigError::EmptySchedule)?;
        if points.len() > MAX_POINTS { return Err(ConfigError::TooManyPoints); }
        if first.elapsed_ms != 0 { return Err(ConfigError::FirstPointNotZero); }
        for w in points.windows(2) {
            if w[1].elapsed_ms <= w[0].elapsed_ms { return Err(ConfigError::TimeNotIncreasing); }
            if w[1].discount_bps < w[0].discount_bps { return Err(ConfigError::DiscountDecreasing); }
        }
        if points.iter().any(|p| p.discount_bps > MAX_DISCOUNT_BPS) {
            return Err(ConfigError::DiscountAboveMax);
        }
        let mut buf = [SchedulePoint::default(); MAX_POINTS];
        buf[..points.len()].copy_from_slice(points);
        Ok(Self { points: buf, len: points.len() as u8 })
    }

    pub fn points(&self) -> &[SchedulePoint] { &self.points[..self.len as usize] }

    /// Discount in RAY at effective elapsed time `t`, rounded down (favours the protocol).
    pub fn discount_ray(&self, t: u64) -> u128 {
        let pts = self.points();
        let last = pts[pts.len() - 1];
        if t >= last.elapsed_ms { return bps_to_ray(last.discount_bps); }
        // pts[0].elapsed_ms == 0 ≤ t < last.elapsed_ms, so a segment exists.
        let i = pts.windows(2).position(|w| t < w[1].elapsed_ms).unwrap_or(0);
        let (a, b) = (pts[i], pts[i + 1]);
        let base = bps_to_ray(a.discount_bps);
        let rise = bps_to_ray(b.discount_bps) - base;          // non-negative by validation
        let span = (b.elapsed_ms - a.elapsed_ms) as u128;        // > 0 by validation
        let into = (t - a.elapsed_ms) as u128;                  // < span
        // rise ≤ 3000 bps in RAY and into < span, so the quotient is < rise; it cannot fail.
        base + mul_div_floor(rise, into, span).unwrap_or(0)
    }

    pub fn max_discount_ray(&self) -> u128 { bps_to_ray(self.points()[self.len as usize - 1].discount_bps) }
}

pub fn rai_default() -> Schedule {
    Schedule::new(&[
        SchedulePoint { elapsed_ms: 0, discount_bps: 200 },
        SchedulePoint { elapsed_ms: 2_700_000, discount_bps: 1000 },
    ])
    .expect("constant schedule is valid")
}
```
Add `#![allow(clippy::arithmetic_side_effects)]` at the top of `schedule.rs`, with the comment `// subtractions are ordered by validation`. In `lib.rs`, add `pub mod error; pub mod schedule;`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p auction_core schedule`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/auction_core/src
git commit -m "feat(core): add piecewise-linear discount schedule"
```

---

### Task 6: Price, feed validity, band clamp, oracle scales, protocol-token reference price

**Files:**
- Create: `crates/auction_core/src/price.rs`
- Modify: `crates/auction_core/src/error.rs`, `crates/auction_core/src/lib.rs`

**Interfaces:**
- Consumes: `mul_div_floor`, `mul_div_ceil`, `RAY`, `BPS`.
- Produces:
  - `pub enum PauseReason { AdminPaused, Stale, ZeroPrice, LowConfidence, OutOfRange }` (in `error.rs`)
  - `pub enum OracleScale { Ray, Q64x64 }`
  - `pub struct OracleReading { pub price_raw: u128, pub confidence_raw: u128, pub timestamp_ms: u64 }`
  - `pub struct FeedParams { pub scale: OracleScale, pub max_age_ms: u64, pub max_conf_bps: u16, pub band_lo_bps: u16, pub band_hi_bps: u16 }`
  - `impl FeedParams { pub fn validate(&self) -> Result<(), ConfigError> }`
  - `pub fn to_ray(raw: u128, scale: OracleScale) -> Result<u128, MathError>`
  - `pub fn validate_feed(r: &OracleReading, p: &FeedParams, redemption_ray: u128, now_ms: u64) -> Result<u128, PauseReason>`, returning M in RAY
  - `pub fn bid_price(m_ray: u128, redemption_ray: u128, p: &FeedParams) -> u128`, returning P
  - `pub const REF_ORACLE_CLAMP_BPS: u16 = 2000;`
  - `pub fn effective_ref_price(admin_ray: u128, oracle: Option<Result<u128, PauseReason>>) -> u128`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::RAY;

    const R: u128 = 752 * RAY / 1_000_000; // 0.000752 collateral per coin
    fn params() -> FeedParams {
        FeedParams { scale: OracleScale::Ray, max_age_ms: 3_600_000, max_conf_bps: 100, band_lo_bps: 9000, band_hi_bps: 11000 }
    }
    fn reading(price: u128, conf: u128, ts: u64) -> OracleReading { OracleReading { price_raw: price, confidence_raw: conf, timestamp_ms: ts } }

    #[test]
    fn valid_feed_returns_price() {
        assert_eq!(validate_feed(&reading(R, 0, 1_000), &params(), R, 2_000), Ok(R));
    }
    #[test]
    fn future_timestamp_is_age_zero() { // Review Focus #3
        assert_eq!(validate_feed(&reading(R, 0, 5_000), &params(), R, 2_000), Ok(R));
    }
    #[test]
    fn stale_zero_lowconf_outofrange() {
        let p = params();
        assert_eq!(validate_feed(&reading(R, 0, 0), &p, R, 3_600_001), Err(PauseReason::Stale));
        assert_eq!(validate_feed(&reading(0, 0, 0), &p, R, 0), Err(PauseReason::ZeroPrice));
        assert_eq!(validate_feed(&reading(R, R / 50, 0), &p, R, 0), Err(PauseReason::LowConfidence)); // 2% > 1%
        assert_eq!(validate_feed(&reading(R * 2 + 1, 0, 0), &p, R, 0), Err(PauseReason::OutOfRange));
        assert_eq!(validate_feed(&reading(R / 2 - 1, 0, 0), &p, R, 0), Err(PauseReason::OutOfRange));
    }
    #[test]
    fn band_clamp() {
        let p = params();
        assert_eq!(bid_price(R, R, &p), R);
        assert_eq!(bid_price(R * 3 / 2, R, &p), R * 11 / 10);
        assert_eq!(bid_price(R * 6 / 10, R, &p), R * 9 / 10);
    }
    #[test]
    fn q64_conversion() {
        assert_eq!(to_ray(1u128 << 64, OracleScale::Q64x64), Ok(RAY));
        assert_eq!(to_ray(1u128 << 63, OracleScale::Q64x64), Ok(RAY / 2));
        assert_eq!(to_ray(RAY, OracleScale::Ray), Ok(RAY));
    }
    #[test]
    fn ref_price_clamped_to_20_percent() {
        let a = 1_000 * RAY;
        assert_eq!(effective_ref_price(a, None), a);
        assert_eq!(effective_ref_price(a, Some(Err(PauseReason::Stale))), a);
        assert_eq!(effective_ref_price(a, Some(Ok(1_100 * RAY))), 1_100 * RAY);
        assert_eq!(effective_ref_price(a, Some(Ok(5_000 * RAY))), 1_200 * RAY);
        assert_eq!(effective_ref_price(a, Some(Ok(1))), 800 * RAY);
    }
    #[test]
    fn params_validation() {
        let mut p = params(); p.band_lo_bps = 4999;
        assert_eq!(p.validate(), Err(ConfigError::BandInvalid));
        let mut p = params(); p.max_age_ms = 86_400_001;
        assert_eq!(p.validate(), Err(ConfigError::OracleAgeAboveMax));
        let mut p = params(); p.max_conf_bps = 1001;
        assert_eq!(p.validate(), Err(ConfigError::ConfidenceAboveMax));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p auction_core price`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

Add to `error.rs`:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseReason { AdminPaused, Stale, ZeroPrice, LowConfidence, OutOfRange }
```

`price.rs`:
```rust
//! Feed validity and the price used for bids (spec §5.2, ADR-005). No fallback price.
use crate::error::{ConfigError, PauseReason};
use crate::math::{mul_div_floor, MathError};
use crate::units::{BPS, RAY};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OracleScale { Ray, Q64x64 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OracleReading { pub price_raw: u128, pub confidence_raw: u128, pub timestamp_ms: u64 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeedParams {
    pub scale: OracleScale, pub max_age_ms: u64, pub max_conf_bps: u16,
    pub band_lo_bps: u16, pub band_hi_bps: u16,
}

pub const MAX_ORACLE_AGE_MS: u64 = 86_400_000;
pub const MAX_CONF_BPS: u16 = 1000;
pub const REF_ORACLE_CLAMP_BPS: u16 = 2000;

impl FeedParams {
    pub fn validate(&self) -> Result<(), ConfigError> {
        let ok_band = (5000..=10_000).contains(&self.band_lo_bps) && (10_000..=20_000).contains(&self.band_hi_bps);
        if !ok_band { return Err(ConfigError::BandInvalid); }
        if self.max_age_ms > MAX_ORACLE_AGE_MS { return Err(ConfigError::OracleAgeAboveMax); }
        if self.max_conf_bps > MAX_CONF_BPS { return Err(ConfigError::ConfidenceAboveMax); }
        Ok(())
    }
}

pub fn to_ray(raw: u128, scale: OracleScale) -> Result<u128, MathError> {
    match scale {
        OracleScale::Ray => Ok(raw),
        OracleScale::Q64x64 => mul_div_floor(raw, RAY, 1u128 << 64),
    }
}

/// Returns the market price M in RAY when the feed is usable; otherwise why the type is paused.
pub fn validate_feed(r: &OracleReading, p: &FeedParams, redemption_ray: u128, now_ms: u64) -> Result<u128, PauseReason> {
    if now_ms.saturating_sub(r.timestamp_ms) > p.max_age_ms { return Err(PauseReason::Stale); }
    let m = to_ray(r.price_raw, p.scale).map_err(|_| PauseReason::OutOfRange)?;
    if m == 0 { return Err(PauseReason::ZeroPrice); }
    let conf = to_ray(r.confidence_raw, p.scale).map_err(|_| PauseReason::LowConfidence)?;
    // conf / m ≤ max_conf_bps / 1e4  ⇔  conf·1e4 ≤ max_conf_bps·m   (compared via mul_div to avoid overflow)
    let conf_limit = mul_div_floor(m, p.max_conf_bps as u128, BPS).map_err(|_| PauseReason::LowConfidence)?;
    if conf > conf_limit { return Err(PauseReason::LowConfidence); }
    let lo = redemption_ray / 2;
    let hi = redemption_ray.saturating_mul(2);
    if m < lo || m > hi { return Err(PauseReason::OutOfRange); }
    Ok(m)
}

/// P = clamp(M, R·lo, R·hi). The bounds are rounded toward R.
pub fn bid_price(m_ray: u128, redemption_ray: u128, p: &FeedParams) -> u128 {
    let lo = mul_div_floor(redemption_ray, p.band_lo_bps as u128, BPS).unwrap_or(0);
    let hi = mul_div_floor(redemption_ray, p.band_hi_bps as u128, BPS).unwrap_or(u128::MAX);
    m_ray.clamp(lo, hi)
}

/// Admin price, optionally overridden by a valid oracle clamped to ±20% of it (spec §5.11, ADR-017).
pub fn effective_ref_price(admin_ray: u128, oracle: Option<Result<u128, PauseReason>>) -> u128 {
    match oracle {
        Some(Ok(o)) => {
            let lo = mul_div_floor(admin_ray, (BPS as u16 - REF_ORACLE_CLAMP_BPS) as u128, BPS).unwrap_or(0);
            let hi = mul_div_floor(admin_ray, (BPS as u16 + REF_ORACLE_CLAMP_BPS) as u128, BPS).unwrap_or(u128::MAX);
            o.clamp(lo, hi)
        }
        _ => admin_ray,
    }
}
```
Add `#![allow(clippy::arithmetic_side_effects)]` with the comment `// constant bps arithmetic below 2^16`. Add `pub mod price;` to `lib.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p auction_core price`
Expected: PASS (7 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/auction_core/src
git commit -m "feat(core): add feed validation, band clamp, oracle scales and ref price"
```

---

### Task 7: Pause clock and spike S5 (stale-onset policy)

**Files:**
- Create: `crates/auction_core/src/pause.rs`, `crates/auction_core/tests/s5_pause_onset.rs`, `docs/spikes/S5.md`
- Modify: `crates/auction_core/src/lib.rs`, `docs/design-log.md` (resolve the ADR-007 onset policy)

**Interfaces:**
- Produces:
  - `pub enum OnsetPolicy { AtObservation, DerivedFromOracle }`
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)] pub struct PauseClock { pub paused_since_ms: Option<u64>, pub paused_total_ms: u64, pub last_seen_oracle_ts: Option<u64> }`
  - `pub struct Observation { pub now_ms: u64, pub feed_valid: bool, pub admin_paused: bool, pub oracle_ts: u64, pub max_age_ms: u64 }`
  - `impl PauseClock { pub fn observed(self, o: Observation, policy: OnsetPolicy) -> Self; pub fn is_paused(&self) -> bool; pub fn effective_elapsed(&self, start_ms: u64, total_at_start: u64, now_ms: u64) -> u64 }`

- [ ] **Step 1: Write the failing unit tests** (in `pause.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    fn obs(now: u64, valid: bool, admin: bool, ts: u64) -> Observation {
        Observation { now_ms: now, feed_valid: valid, admin_paused: admin, oracle_ts: ts, max_age_ms: 100 }
    }
    #[test]
    fn admin_pause_freezes_elapsed() {
        let c0 = PauseClock::default().observed(obs(0, true, false, 0), OnsetPolicy::AtObservation);
        let c1 = c0.observed(obs(50, true, true, 50), OnsetPolicy::AtObservation);   // admin pauses at 50
        assert_eq!(c1.effective_elapsed(0, 0, 80), 50);                               // frozen while open
        let c2 = c1.observed(obs(90, true, false, 90), OnsetPolicy::AtObservation);   // resumes at 90
        assert_eq!(c2.effective_elapsed(0, 0, 100), 60);                              // 100 − 40 paused
    }
    #[test]
    fn auction_started_after_old_pause_ignores_it() {
        let c = PauseClock { paused_since_ms: None, paused_total_ms: 500, last_seen_oracle_ts: Some(0) };
        assert_eq!(c.effective_elapsed(1_000, 500, 1_200), 200);
    }
    #[test]
    fn open_pause_that_began_before_start_counts_from_start() {
        let c = PauseClock { paused_since_ms: Some(10), paused_total_ms: 0, last_seen_oracle_ts: Some(0) };
        assert_eq!(c.effective_elapsed(20, 0, 50), 0);
    }
    #[test]
    fn derived_onset_closes_unobserved_gap() {
        // valid at 0 (ts 0); nobody observes; the oracle republishes at ts 400; observed valid at 400.
        let c0 = PauseClock::default().observed(obs(0, true, false, 0), OnsetPolicy::DerivedFromOracle);
        let c1 = c0.observed(obs(400, true, false, 400), OnsetPolicy::DerivedFromOracle);
        // stale from 0+100 to 400 → 300 ms paused
        assert_eq!(c1.paused_total_ms, 300);
        assert_eq!(c1.effective_elapsed(0, 0, 400), 100);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p auction_core pause`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `pause.rs`**

```rust
//! Type-level pause clock (spec §5.7, ADR-007). Freezes the discount clock of every auction of the type.
#![allow(clippy::arithmetic_side_effects)] // saturating/ordered arithmetic on u64 ms

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnsetPolicy {
    /// (a) The pause starts when someone observes the invalid feed.
    AtObservation,
    /// (b) The pause starts at last valid oracle timestamp + max_age; unobserved gaps are closed on recovery.
    DerivedFromOracle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PauseClock {
    pub paused_since_ms: Option<u64>,
    pub paused_total_ms: u64,
    pub last_seen_oracle_ts: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observation { pub now_ms: u64, pub feed_valid: bool, pub admin_paused: bool, pub oracle_ts: u64, pub max_age_ms: u64 }

impl PauseClock {
    pub fn is_paused(&self) -> bool { self.paused_since_ms.is_some() }

    pub fn observed(self, o: Observation, policy: OnsetPolicy) -> Self {
        let paused_now = o.admin_paused || !o.feed_valid;
        let derived_onset = |last: Option<u64>| -> u64 {
            match (policy, last) {
                (OnsetPolicy::DerivedFromOracle, Some(ts)) if !o.admin_paused => ts.saturating_add(o.max_age_ms).min(o.now_ms),
                _ => o.now_ms,
            }
        };
        let mut next = self;
        match (paused_now, self.paused_since_ms) {
            (true, None) => next.paused_since_ms = Some(derived_onset(self.last_seen_oracle_ts)),
            (false, Some(since)) => {
                next.paused_total_ms = self.paused_total_ms.saturating_add(o.now_ms.saturating_sub(since));
                next.paused_since_ms = None;
            }
            (false, None) => {
                // Policy (b): close a gap nobody observed (the feed went stale, then recovered unseen).
                if let (OnsetPolicy::DerivedFromOracle, Some(last)) = (policy, self.last_seen_oracle_ts) {
                    let stale_from = last.saturating_add(o.max_age_ms);
                    if o.oracle_ts > stale_from {
                        next.paused_total_ms = self.paused_total_ms.saturating_add(o.oracle_ts - stale_from);
                    }
                }
            }
            (true, Some(_)) => {}
        }
        if o.feed_valid {
            next.last_seen_oracle_ts = Some(self.last_seen_oracle_ts.map_or(o.oracle_ts, |l| l.max(o.oracle_ts)));
        }
        next
    }

    /// t_eff = (now − start) − (paused_total − total_at_start) − open_pause_since_max(since, start); floored at 0.
    pub fn effective_elapsed(&self, start_ms: u64, total_at_start: u64, now_ms: u64) -> u64 {
        let wall = now_ms.saturating_sub(start_ms);
        let closed = self.paused_total_ms.saturating_sub(total_at_start);
        let open = self.paused_since_ms.map_or(0, |s| now_ms.saturating_sub(s.max(start_ms)));
        wall.saturating_sub(closed).saturating_sub(open)
    }
}
```
Add `pub mod pause;` to `lib.rs`.

- [ ] **Step 4: Run the unit tests to verify they pass**

Run: `cargo test -p auction_core pause`
Expected: PASS (4 tests).

- [ ] **Step 5: Write the S5 property tests**

`crates/auction_core/tests/s5_pause_onset.rs`:
```rust
//! Spike S5: an adversary controls oracle publish times and observation times.
//! Ground truth: an auction's discount should advance only while the feed is fresh (age ≤ max_age).
use auction_core::pause::{Observation, OnsetPolicy, PauseClock};
use proptest::prelude::*;

const MAX_AGE: u64 = 100;
const BLOCK: u64 = 10; // one block of tolerance

/// Fresh time in [start, end) given publish timestamps (sorted, first ≤ start).
fn fresh_time(publishes: &[u64], start: u64, end: u64) -> u64 {
    (start..end).filter(|t| publishes.iter().rev().find(|p| **p <= *t).map_or(false, |p| t - p <= MAX_AGE)).count() as u64
}

fn run(policy: OnsetPolicy, publishes: &[u64], observes: &[u64], end: u64) -> (u64, u64) {
    let start = publishes[0];
    let mut c = PauseClock::default().observed(
        Observation { now_ms: start, feed_valid: true, admin_paused: false, oracle_ts: start, max_age_ms: MAX_AGE }, policy);
    let total_at_start = c.paused_total_ms;
    for &t in observes.iter().filter(|t| **t > start && **t <= end) {
        let last = *publishes.iter().rev().find(|p| **p <= t).unwrap();
        c = c.observed(Observation { now_ms: t, feed_valid: t - last <= MAX_AGE, admin_paused: false, oracle_ts: last, max_age_ms: MAX_AGE }, policy);
    }
    (c.effective_elapsed(start, total_at_start, end), fresh_time(publishes, start, end))
}

fn scenario() -> impl Strategy<Value = (Vec<u64>, Vec<u64>, u64)> {
    (prop::collection::vec(1u64..400, 1..12), prop::collection::vec(1u64..2_000, 0..40), 100u64..2_000)
        .prop_map(|(gaps, mut obs, end)| {
            let mut t = 0; let mut pubs = vec![0];
            for g in gaps { t += g; pubs.push(t); }
            obs.sort_unstable();
            (pubs, obs, end)
        })
}

proptest! {
    /// Policy (b): the adversary never gains more than one block of discount time over the fresh time,
    /// provided observations happen at every publish (the engine observes on liquidate/bid; keepers on refresh).
    #[test]
    fn derived_policy_bounded_when_publishes_observed((pubs, mut obs, end) in scenario()) {
        obs.extend(pubs.iter().copied()); obs.sort_unstable();
        let (t_eff, fresh) = run(OnsetPolicy::DerivedFromOracle, &pubs, &obs, end);
        prop_assert!(t_eff <= fresh + BLOCK, "t_eff={t_eff} fresh={fresh}");
    }
}

/// Policy (a) documents its residual: without observations, the stale time counts as elapsed.
#[test]
fn at_observation_policy_overcounts_unobserved_staleness() {
    let pubs = [0u64, 1_000];             // stale from 100 to 1000
    let (t_eff, fresh) = run(OnsetPolicy::AtObservation, &pubs, &[1_000], 1_100);
    assert!(t_eff > fresh + BLOCK, "policy (a) residual: t_eff={t_eff} fresh={fresh}");
}
```

- [ ] **Step 6: Run the S5 tests**

Run: `PROPTEST_CASES=5000 cargo test -p auction_core --test s5_pause_onset`
Expected: PASS for both tests. If `derived_policy_bounded_when_publishes_observed` fails, read the shrunk counterexample, fix `observed` (not the test), and re-run. The test encodes the spec's S5 acceptance criterion.

- [ ] **Step 7: Record the decision and commit**

`docs/spikes/S5.md` covers:
- the question;
- both policies;
- the property and how many cases ran;
- the residual of policy (b): it is exact only if every oracle publish is observed (the engine observes on `liquidate`/`bid`, the app and keepers call `observe_feed` on refresh), and a re-stamping TWAP producer cannot be detected;
- the decision: **adopt `DerivedFromOracle`**.

Amend ADR-007 in `docs/design-log.md` to `Status: Accepted (S5)`.
```bash
git add crates/auction_core docs/spikes/S5.md docs/design-log.md
git commit -m "feat(core): add type-level pause clock; resolve spike S5 onset policy"
```

---

### Task 8: Liquidation quote, caps and keeper reward

**Files:**
- Create: `crates/auction_core/src/liquidation.rs`
- Modify: `crates/auction_core/src/error.rs`, `crates/auction_core/src/lib.rs`

**Interfaces:**
- Consumes: `mul_div_floor`, `mul_div_ceil`, `RAY`, `BPS`, `Schedule`, `ConfigError`.
- Produces:
  - `pub enum AuctionError { ZeroDebt, VaultBelowCollateral, CapacityExhausted, NotAuctioning, ZeroFill, Slippage, BelowMinBid, DustRemainder, NotOwner, NothingToClaim, Math }` (in `error.rs`, with `impl From<MathError> for AuctionError`)
  - `pub const MAX_PENALTY_BPS: u16 = 2500;`
  - `pub struct CollateralParams { pub penalty_bps: u16, pub tip: u128, pub chip_bps: u16, pub reward_cap: u128, pub debt_floor: u128, pub min_bid: u128, pub schedule: Schedule }`
  - `impl CollateralParams { pub fn validate(&self) -> Result<(), ConfigError>; pub fn chost(&self) -> Result<u128, MathError> }`
  - `pub struct CapState { pub type_cap: u128, pub type_reserved: u128, pub global_cap: u128, pub global_reserved: u128 }`
  - `pub struct LiquidationInput { pub position_collateral: u128, pub vault_balance: u128, pub nominal_debt: u128, pub price_ray: u128 }`
  - `pub struct LiquidationQuote { pub debt_principal: u128, pub amount_to_raise: u128, pub reserve: u128, pub keeper_reward: u128, pub lot: u128, pub seized: u128 }`
  - `pub fn quote_liquidation(i: &LiquidationInput, p: &CollateralParams, caps: &CapState) -> Result<LiquidationQuote, AuctionError>`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::rai_default;
    use crate::units::RAY;

    const P: u128 = 752 * RAY / 1_000_000; // 1 LSC = 0.000752 wTKA; amounts in whole units for readability
    fn params() -> CollateralParams {
        CollateralParams { penalty_bps: 1000, tip: 5, chip_bps: 10, reward_cap: 50, debt_floor: 100, min_bid: 25, schedule: rai_default() }
    }
    fn caps() -> CapState { CapState { type_cap: 60_000, type_reserved: 0, global_cap: 250_000, global_reserved: 0 } }

    #[test]
    fn nominal_quote() {
        // c = 125_000 base units, D = 15_900
        let i = LiquidationInput { position_collateral: 125_000, vault_balance: 125_000, nominal_debt: 15_900, price_ray: P };
        let q = quote_liquidation(&i, &params(), &caps()).unwrap();
        assert_eq!(q.amount_to_raise, 17_490);                   // ceil(15_900 × 1.1)
        assert_eq!(q.reserve, 17_490);
        // reward_coin = min(5 + 15.9→15, 50, 1590) = 20 ; reward_coll = floor(20 × 0.000752) = 0
        assert_eq!(q.keeper_reward, 0);
        assert_eq!(q.lot, 125_000);
        assert_eq!(params().chost().unwrap(), 110);
    }
    #[test]
    fn donated_vault_is_swept() {
        let i = LiquidationInput { position_collateral: 1_000, vault_balance: 1_001, nominal_debt: 900, price_ray: P };
        assert_eq!(quote_liquidation(&i, &params(), &caps()).unwrap().seized, 1_001);
        let bad = LiquidationInput { vault_balance: 999, ..i };
        assert_eq!(quote_liquidation(&bad, &params(), &caps()), Err(AuctionError::VaultBelowCollateral));
    }
    #[test]
    fn debt_below_floor_pays_no_reward_but_liquidates() { // Review Focus #4
        let i = LiquidationInput { position_collateral: 10, vault_balance: 10, nominal_debt: 99, price_ray: RAY };
        let q = quote_liquidation(&i, &params(), &caps()).unwrap();
        assert_eq!(q.keeper_reward, 0);
        assert_eq!(q.amount_to_raise, 109);
    }
    #[test]
    fn reward_capped_at_lot() {
        // price 1 collateral per coin, tiny collateral: the reward in collateral would exceed c
        let i = LiquidationInput { position_collateral: 3, vault_balance: 3, nominal_debt: 10_000, price_ray: RAY };
        let q = quote_liquidation(&i, &params(), &caps()).unwrap();
        assert_eq!(q.keeper_reward, 3);
        assert_eq!(q.lot, 0);
    }
    #[test]
    fn caps_and_oversized_alone() {
        let i = LiquidationInput { position_collateral: 1, vault_balance: 1, nominal_debt: 100_000, price_ray: P };
        assert!(quote_liquidation(&i, &params(), &caps()).is_ok()); // oversized but alone
        let busy = CapState { type_reserved: 1, ..caps() };
        assert_eq!(quote_liquidation(&i, &params(), &busy), Err(AuctionError::CapacityExhausted));
        let small = LiquidationInput { nominal_debt: 1_000, ..i };
        let nearly_full = CapState { type_reserved: 59_000, ..caps() };
        assert_eq!(quote_liquidation(&small, &params(), &nearly_full), Err(AuctionError::CapacityExhausted)); // 1_100 > 1_000 room
    }
    #[test]
    fn zero_debt_rejected() {
        let i = LiquidationInput { position_collateral: 1, vault_balance: 1, nominal_debt: 0, price_ray: P };
        assert_eq!(quote_liquidation(&i, &params(), &caps()), Err(AuctionError::ZeroDebt));
    }
    #[test]
    fn params_validation() {
        assert_eq!(CollateralParams { penalty_bps: 2501, ..params() }.validate(), Err(ConfigError::PenaltyAboveMax));
        assert_eq!(CollateralParams { min_bid: 200, ..params() }.validate(), Err(ConfigError::ChostBelowMinBid));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p auction_core liquidation`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

Add to `error.rs`:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuctionError {
    ZeroDebt, VaultBelowCollateral, CapacityExhausted, NotAuctioning, ZeroFill, Slippage,
    BelowMinBid, DustRemainder, NotOwner, NothingToClaim, Math,
}
impl From<crate::math::MathError> for AuctionError { fn from(_: crate::math::MathError) -> Self { AuctionError::Math } }
```

`liquidation.rs`:
```rust
//! Full-position liquidation quote (spec §5.6, §5.9, ADR-012). Reads only pre-state.
use crate::error::{AuctionError, ConfigError};
use crate::math::{mul_div_ceil, mul_div_floor, MathError};
use crate::schedule::Schedule;
use crate::units::{BPS, RAY};

pub const MAX_PENALTY_BPS: u16 = 2500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollateralParams {
    pub penalty_bps: u16, pub tip: u128, pub chip_bps: u16, pub reward_cap: u128,
    pub debt_floor: u128, pub min_bid: u128, pub schedule: Schedule,
}

impl CollateralParams {
    /// chost = ceil(debt_floor · (1e4 + penalty) / 1e4)
    pub fn chost(&self) -> Result<u128, MathError> {
        mul_div_ceil(self.debt_floor, BPS + self.penalty_bps as u128, BPS)
    }
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.penalty_bps > MAX_PENALTY_BPS { return Err(ConfigError::PenaltyAboveMax); }
        let chost = self.chost().map_err(|_| ConfigError::ChostBelowMinBid)?;
        if chost < self.min_bid { return Err(ConfigError::ChostBelowMinBid); }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapState { pub type_cap: u128, pub type_reserved: u128, pub global_cap: u128, pub global_reserved: u128 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiquidationInput { pub position_collateral: u128, pub vault_balance: u128, pub nominal_debt: u128, pub price_ray: u128 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiquidationQuote {
    pub debt_principal: u128, pub amount_to_raise: u128, pub reserve: u128,
    pub keeper_reward: u128, pub lot: u128, pub seized: u128,
}

pub fn quote_liquidation(i: &LiquidationInput, p: &CollateralParams, caps: &CapState) -> Result<LiquidationQuote, AuctionError> {
    if i.nominal_debt == 0 { return Err(AuctionError::ZeroDebt); }
    if i.vault_balance < i.position_collateral { return Err(AuctionError::VaultBelowCollateral); }
    let d = i.nominal_debt;
    let amount_to_raise = mul_div_ceil(d, BPS + p.penalty_bps as u128, BPS)?;
    let fits = amount_to_raise <= caps.type_cap.saturating_sub(caps.type_reserved)
        && amount_to_raise <= caps.global_cap.saturating_sub(caps.global_reserved);
    let alone = caps.type_reserved == 0 && caps.global_reserved == 0;
    if !(fits || alone) { return Err(AuctionError::CapacityExhausted); }

    let seized = i.vault_balance; // sweep everything (≥, not =)
    let reward_coin = if d < p.debt_floor { 0 } else {
        let chip = mul_div_floor(d, p.chip_bps as u128, BPS)?;
        let penalty = mul_div_floor(d, p.penalty_bps as u128, BPS)?;
        p.tip.saturating_add(chip).min(p.reward_cap).min(penalty)
    };
    let reward_coll = mul_div_floor(reward_coin, i.price_ray, RAY)?.min(seized);
    Ok(LiquidationQuote {
        debt_principal: d, amount_to_raise, reserve: amount_to_raise,
        keeper_reward: reward_coll, lot: seized - reward_coll, seized,
    })
}
```
Add `#![allow(clippy::arithmetic_side_effects)]` with the comment `// seized - reward_coll: reward_coll ≤ seized by min()`, and `pub mod liquidation;` to `lib.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p auction_core liquidation`
Expected: PASS (7 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/auction_core/src
git commit -m "feat(core): add liquidation quote with caps, sweep and capped keeper reward"
```

---

### Task 9: Collateral auction, bids, settlement and leftovers

**Files:**
- Create: `crates/auction_core/src/auction.rs`, `docs/architecture/core.md`
- Modify: `crates/auction_core/src/lib.rs`

**Interfaces:**
- Consumes: `LiquidationQuote`, `Schedule`, `mul_div_floor/ceil`, `RAY`, `AuctionError`.
- Produces:
  - `pub enum Status { Auctioning, Settled }`
  - `pub struct Auction { pub status: Status, pub collateral_seized: u128, pub collateral_sold: u128, pub collateral_left: u128, pub keeper_reward: u128, pub leftover_claimable: u128, pub leftover_claimed: u128, pub debt_principal: u128, pub amount_to_raise: u128, pub reserved_amount: u128, pub debt_raised: u128, pub penalty_raised: u128, pub start_ms: u64, pub schedule: Schedule, pub type_paused_total_at_start: u64 }`
  - `impl Auction { pub fn open(q: &LiquidationQuote, start_ms: u64, schedule: Schedule, type_paused_total_at_start: u64) -> Self; pub fn raised(&self) -> u128; pub fn remaining_target(&self) -> u128 }`
  - `pub struct BidRequest { pub coin_in_max: u128, pub min_collateral_out: u128 }`
  - `pub struct BidFill { pub charged: u128, pub collateral_out: u128, pub debt_part: u128, pub penalty_part: u128, pub discount_ray: u128, pub ends_auction: bool }`
  - `pub struct ChainEnd { pub shortfall: u128, pub release: u128, pub leftover: u128 }`
  - `pub fn quote_bid(a: &Auction, req: &BidRequest, price_ray: u128, t_eff_ms: u64, min_bid: u128, chost: u128) -> Result<BidFill, AuctionError>`
  - `pub fn apply_bid(a: &Auction, f: &BidFill) -> (Auction, Option<ChainEnd>)`
  - `pub fn claim_leftover(a: &Auction) -> Result<(Auction, u128), AuctionError>`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::liquidation::LiquidationQuote;
    use crate::schedule::{rai_default, Schedule, SchedulePoint};
    use crate::units::RAY;

    fn quote(lot: u128, debt: u128, target: u128) -> LiquidationQuote {
        LiquidationQuote { debt_principal: debt, amount_to_raise: target, reserve: target, keeper_reward: 0, lot, seized: lot }
    }
    fn flat(bps: u16) -> Schedule { Schedule::new(&[SchedulePoint { elapsed_ms: 0, discount_bps: bps }]).unwrap() }

    #[test]
    fn partial_bid_debt_first() {
        // P = 1 collateral per coin, discount 0: 400 coin → 400 collateral
        let a = Auction::open(&quote(1_000, 900, 990), 0, flat(0), 0);
        let f = quote_bid(&a, &BidRequest { coin_in_max: 400, min_collateral_out: 400 }, RAY, 0, 25, 110).unwrap();
        assert_eq!((f.charged, f.collateral_out, f.debt_part, f.penalty_part, f.ends_auction), (400, 400, 400, 0, false));
        let (a2, end) = apply_bid(&a, &f);
        assert!(end.is_none());
        assert_eq!((a2.collateral_left, a2.collateral_sold, a2.debt_raised), (600, 400, 400));
    }

    #[test]
    fn discount_increases_collateral_out_rounded_down() {
        let a = Auction::open(&quote(10_000, 900, 990), 0, flat(1000), 0); // 10% off
        let f = quote_bid(&a, &BidRequest { coin_in_max: 100, min_collateral_out: 0 }, RAY, 0, 25, 110).unwrap();
        assert_eq!(f.collateral_out, 111); // floor(100 / 0.9)
    }

    #[test]
    fn lot_binds_charges_only_for_lot() {
        let a = Auction::open(&quote(50, 900, 990), 0, flat(0), 0);
        let f = quote_bid(&a, &BidRequest { coin_in_max: 400, min_collateral_out: 0 }, RAY, 0, 25, 110).unwrap();
        assert_eq!((f.collateral_out, f.charged, f.ends_auction), (50, 50, true));
        let (a2, end) = apply_bid(&a, &f);
        assert_eq!(a2.status, Status::Settled);
        assert_eq!(end.unwrap(), ChainEnd { shortfall: 850, release: 990, leftover: 0 });
    }

    #[test]
    fn target_and_lot_end_together() { // Review Focus #1
        let a = Auction::open(&quote(990, 900, 990), 0, flat(0), 0);
        let f = quote_bid(&a, &BidRequest { coin_in_max: 990, min_collateral_out: 990 }, RAY, 0, 25, 110).unwrap();
        assert!(f.charged <= 990 && f.ends_auction);
        let (_, end) = apply_bid(&a, &f);
        assert_eq!(end.unwrap(), ChainEnd { shortfall: 0, release: 990, leftover: 0 });
    }

    #[test]
    fn huge_coin_in_is_capped_at_target() { // Review Focus #2
        let a = Auction::open(&quote(1_000_000, 900, 990), 0, flat(0), 0);
        let f = quote_bid(&a, &BidRequest { coin_in_max: u128::MAX, min_collateral_out: 0 }, RAY, 0, 25, 110).unwrap();
        assert_eq!((f.charged, f.collateral_out, f.penalty_part), (990, 990, 90));
        let (a2, end) = apply_bid(&a, &f);
        assert_eq!(end.unwrap(), ChainEnd { shortfall: 0, release: 990, leftover: 1_000_000 - 990 });
        let (a3, paid) = claim_leftover(&a2).unwrap();
        assert_eq!(paid, 999_010);
        assert_eq!(claim_leftover(&a3), Err(AuctionError::NothingToClaim));
    }

    #[test]
    fn rejections() {
        let a = Auction::open(&quote(1_000, 900, 990), 0, rai_default(), 0);
        let r = |coin, min| quote_bid(&a, &BidRequest { coin_in_max: coin, min_collateral_out: min }, RAY, 0, 25, 110);
        assert_eq!(r(10, 0), Err(AuctionError::BelowMinBid));
        assert_eq!(r(100, 1_000), Err(AuctionError::Slippage));
        assert_eq!(r(900, 0), Err(AuctionError::DustRemainder)); // leaves 90 < chost 110
        let settled = Auction { status: Status::Settled, ..a };
        assert_eq!(quote_bid(&settled, &BidRequest { coin_in_max: 100, min_collateral_out: 0 }, RAY, 0, 25, 110), Err(AuctionError::NotAuctioning));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p auction_core auction::`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `auction.rs`**

```rust
//! Collateral auction state machine (spec §5.5, §5.6, §5.8). No expiry; Settled is the only terminal state.
#![allow(clippy::arithmetic_side_effects)] // subtractions are guarded by the min()/checks noted inline
use crate::error::AuctionError;
use crate::liquidation::LiquidationQuote;
use crate::math::{mul_div_ceil, mul_div_floor};
use crate::schedule::Schedule;
use crate::units::RAY;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status { Auctioning, Settled }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Auction {
    pub status: Status,
    pub collateral_seized: u128, pub collateral_sold: u128, pub collateral_left: u128, pub keeper_reward: u128,
    pub leftover_claimable: u128, pub leftover_claimed: u128,
    pub debt_principal: u128, pub amount_to_raise: u128, pub reserved_amount: u128,
    pub debt_raised: u128, pub penalty_raised: u128,
    pub start_ms: u64, pub schedule: Schedule, pub type_paused_total_at_start: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BidRequest { pub coin_in_max: u128, pub min_collateral_out: u128 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BidFill {
    pub charged: u128, pub collateral_out: u128, pub debt_part: u128, pub penalty_part: u128,
    pub discount_ray: u128, pub ends_auction: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainEnd { pub shortfall: u128, pub release: u128, pub leftover: u128 }

impl Auction {
    pub fn open(q: &LiquidationQuote, start_ms: u64, schedule: Schedule, type_paused_total_at_start: u64) -> Self {
        Self {
            status: Status::Auctioning,
            collateral_seized: q.seized, collateral_sold: 0, collateral_left: q.lot, keeper_reward: q.keeper_reward,
            leftover_claimable: 0, leftover_claimed: 0,
            debt_principal: q.debt_principal, amount_to_raise: q.amount_to_raise, reserved_amount: q.reserve,
            debt_raised: 0, penalty_raised: 0, start_ms, schedule, type_paused_total_at_start,
        }
    }
    pub fn raised(&self) -> u128 { self.debt_raised + self.penalty_raised }
    pub fn remaining_target(&self) -> u128 { self.amount_to_raise - self.raised() } // raised ≤ target (invariant 3)
}

pub fn quote_bid(a: &Auction, req: &BidRequest, price_ray: u128, t_eff_ms: u64, min_bid: u128, chost: u128) -> Result<BidFill, AuctionError> {
    if a.status != Status::Auctioning { return Err(AuctionError::NotAuctioning); }
    let remaining = a.remaining_target();
    let coin = req.coin_in_max.min(remaining);
    let d = a.schedule.discount_ray(t_eff_ms);
    let denom = RAY - d; // d ≤ 30% of RAY
    let full_out = mul_div_floor(coin, price_ray, denom)?;
    let (collateral_out, charged) = if full_out >= a.collateral_left {
        // The lot binds: pay only for what is left, rounded up (owed), never above `coin`.
        let c = mul_div_ceil(a.collateral_left, denom, price_ray)?.min(coin);
        (a.collateral_left, c)
    } else {
        (full_out, coin)
    };
    if collateral_out == 0 || charged == 0 { return Err(AuctionError::ZeroFill); }
    if collateral_out < req.min_collateral_out { return Err(AuctionError::Slippage); }
    let remaining_after = remaining - charged;
    let lot_empties = collateral_out == a.collateral_left;
    let ends_auction = remaining_after == 0 || lot_empties;
    if !ends_auction && charged < min_bid { return Err(AuctionError::BelowMinBid); }
    if !ends_auction && remaining_after < chost { return Err(AuctionError::DustRemainder); }
    let debt_left = a.debt_principal - a.debt_raised.min(a.debt_principal);
    let debt_part = charged.min(debt_left);
    Ok(BidFill { charged, collateral_out, debt_part, penalty_part: charged - debt_part, discount_ray: d, ends_auction })
}

pub fn apply_bid(a: &Auction, f: &BidFill) -> (Auction, Option<ChainEnd>) {
    let mut n = *a;
    n.collateral_left -= f.collateral_out;
    n.collateral_sold += f.collateral_out;
    n.debt_raised += f.debt_part;
    n.penalty_raised += f.penalty_part;
    if !f.ends_auction { return (n, None); }
    n.status = Status::Settled;
    let target_reached = n.raised() == n.amount_to_raise;
    n.leftover_claimable = if target_reached { n.collateral_left } else { 0 };
    let end = ChainEnd {
        shortfall: n.debt_principal.saturating_sub(n.debt_raised),
        release: n.reserved_amount,
        leftover: n.leftover_claimable,
    };
    (n, Some(end))
}

pub fn claim_leftover(a: &Auction) -> Result<(Auction, u128), AuctionError> {
    let amount = a.leftover_claimable - a.leftover_claimed;
    if a.status != Status::Settled || amount == 0 { return Err(AuctionError::NothingToClaim); }
    let mut n = *a;
    n.leftover_claimed = n.leftover_claimable;
    Ok((n, amount))
}
```
The owner check for `claim_leftover` (`NotOwner`) lives in the program layer (Plan 2). Add `pub mod auction;` to `lib.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p auction_core auction::`
Expected: PASS (6 tests).

- [ ] **Step 5: Document and commit**

`docs/architecture/core.md` covers: the auction state diagram (Auctioning → Settled; no expiry, with the reason per ADR-012), the bid algorithm step by step, debt-first splitting, the lot-binds charge, the dust rules, the chain-end effects and the leftover rule. Add a "rounding call sites" row for `quote_bid` to `docs/architecture/math.md`.
```bash
git add crates/auction_core/src docs/architecture
git commit -m "feat(core): add collateral auction bids, settlement and leftovers"
```

---

### Task 10: Accounting, settle_debt, surplus and debt auctions, check_solvency

**Files:**
- Create: `crates/auction_core/src/accounting.rs`, `crates/auction_core/src/system.rs`
- Modify: `crates/auction_core/src/error.rs`, `crates/auction_core/src/lib.rs`, `docs/architecture/core.md`

**Interfaces:**
- Consumes: `BidFill`, `ChainEnd`, `Schedule`, `mul_div_*`, `RAY`, `BPS`, `bps_to_ray`.
- Produces:
  - `pub struct Accounting { pub unbacked_debt: u128, pub total_shortfall_booked: u128, pub total_penalty_collected: u128, pub total_debt_burned: u128, pub total_debt_settled: u128, pub surplus_nonce: u64, pub debt_nonce: u64, pub host_frozen_by_engine: bool }`
  - `impl Accounting { pub fn record_bid(self, f: &BidFill) -> Self; pub fn record_chain_end(self, e: &ChainEnd) -> Self; pub fn settle_debt(self, amount: u128, buffer_balance: u128) -> (Self, u128) }`
  - `pub enum SystemAuctionError { TokenNotConfigured, UnbackedDebtPresent, BufferBelowTarget, DelayNotElapsed, InsufficientBadDebt, MintCapConfig, MintAuthorityMissing, ProtocolTokenMismatch, DebtAuctionAlreadyLive, SurplusAuctionAlreadyLive, NotLive, Slippage, DustRemainder, ZeroFill, Math }`
  - `pub struct SystemParams { pub surplus_buffer_target: u128, pub surplus_lot: u128, pub surplus_delay_ms: u64, pub surplus_premium_bps: u16, pub surplus_max_discount_bps: u16, pub debt_lot: u128, pub debt_initial_discount_bps: u16, pub debt_growth: Schedule, pub surplus_decay: Schedule, pub max_mint_per_auction: u128, pub min_take: u128, pub system_ttl_ms: u64 }`
  - `pub const MAX_MINT_HARD_CAP: u128 = 1_000_000_000_000_000_000_000_000_000;`
  - `pub struct SurplusAuction { pub lot_left: u128, pub start_ms: u64, pub live: bool }`, `pub struct DebtAuction { pub lot_left: u128, pub start_amount: u128, pub start_ms: u64, pub live: bool }`
  - `pub fn surplus_required_tokens(lot: u128, ref_ray: u128, s: &SystemParams, t_ms: u64) -> Result<u128, SystemAuctionError>`
  - `pub fn debt_start_amount(ref_ray: u128, s: &SystemParams) -> Result<u128, SystemAuctionError>`
  - `pub fn debt_offered_tokens(start_amount: u128, s: &SystemParams, t_ms: u64) -> Result<u128, SystemAuctionError>`
  - `pub fn start_surplus(acc: &Accounting, buffer: u128, token_configured: bool, last_end_ms: u64, now_ms: u64, s: &SystemParams, live: bool) -> Result<SurplusAuction, SystemAuctionError>`
  - `pub fn take_surplus(a: &SurplusAuction, coin: u128, max_tokens_in: u128, ref_ray: u128, s: &SystemParams, now_ms: u64) -> Result<(SurplusAuction, u128 /*tokens to burn*/), SystemAuctionError>`
  - `pub fn start_debt(acc: &Accounting, buffer: u128, token_configured: bool, mint_authority_ok: bool, token_ok: bool, ref_ray: u128, s: &SystemParams, now_ms: u64, live: bool) -> Result<DebtAuction, SystemAuctionError>`
  - `pub fn take_debt(a: &DebtAuction, coin: u128, min_tokens_out: u128, s: &SystemParams, now_ms: u64) -> Result<(DebtAuction, u128 /*tokens to mint*/), SystemAuctionError>`
  - `pub enum SolvencyDecision { Healthy, Freeze, RecoveryInProgress, NoAction }`
  - `pub fn check_solvency(unbacked_after_settle: u128, threshold: u128, token_configured: bool, freeze_on_bad_debt: bool, debt_auction_live: bool, start_check: Result<(), SystemAuctionError>) -> SolvencyDecision`

- [ ] **Step 1: Write the failing tests** (`accounting.rs` and `system.rs` test modules)

```rust
// accounting.rs tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::auction::{BidFill, ChainEnd};
    #[test]
    fn records_and_settles() {
        let a = Accounting::default()
            .record_bid(&BidFill { charged: 990, collateral_out: 0, debt_part: 900, penalty_part: 90, discount_ray: 0, ends_auction: true })
            .record_chain_end(&ChainEnd { shortfall: 40, release: 990, leftover: 0 });
        assert_eq!((a.total_debt_burned, a.total_penalty_collected, a.unbacked_debt, a.total_shortfall_booked), (900, 90, 40, 40));
        let (a2, burned) = a.settle_debt(u128::MAX, 25);
        assert_eq!((burned, a2.unbacked_debt, a2.total_debt_settled), (25, 15, 25));
        let (a3, burned) = a2.settle_debt(u128::MAX, 1_000);
        assert_eq!((burned, a3.unbacked_debt), (15, 0));
    }
}

// system.rs tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::{Schedule, SchedulePoint};
    use crate::units::RAY;
    fn sched(end_bps: u16) -> Schedule {
        Schedule::new(&[SchedulePoint { elapsed_ms: 0, discount_bps: 0 }, SchedulePoint { elapsed_ms: 1_000, discount_bps: end_bps }]).unwrap()
    }
    fn sp() -> SystemParams {
        SystemParams { surplus_buffer_target: 1_000, surplus_lot: 500, surplus_delay_ms: 0, surplus_premium_bps: 500,
            surplus_max_discount_bps: 2000, debt_lot: 500, debt_initial_discount_bps: 0, debt_growth: sched(3000),
            surplus_decay: sched(2500), max_mint_per_auction: 5_000, min_take: 10, system_ttl_ms: 14_400_000 }
    }
    const REF: u128 = 2 * RAY; // 2 coin per protocol token

    #[test]
    fn surplus_units() { // corrected units: lot / ref × (1 + premium − decay)
        assert_eq!(surplus_required_tokens(500, REF, &sp(), 0), Ok(263));      // ceil(250 × 1.05)
        assert_eq!(surplus_required_tokens(500, REF, &sp(), 1_000), Ok(200));  // floor of 0.80
    }
    #[test]
    fn debt_amounts_round_down_and_cap() {
        assert_eq!(debt_start_amount(REF, &sp()), Ok(250));                    // floor(500 / 2)
        assert_eq!(debt_offered_tokens(250, &sp(), 1_000), Ok(325));           // floor(250 × 1.30)
        let tiny_ref = sp();
        assert_eq!(debt_start_amount(RAY / 100, &tiny_ref), Err(SystemAuctionError::MintCapConfig)); // 50_000 > 5_000
    }
    #[test]
    fn mint_cap_holds_with_low_price() {
        let s = SystemParams { debt_growth: sched(3000), ..sp() };
        assert!(debt_offered_tokens(4_900, &s, 1_000).unwrap() <= s.max_mint_per_auction);
    }
    #[test]
    fn solvency_decisions() {
        use SolvencyDecision::*;
        assert_eq!(check_solvency(999, 1_000, false, true, false, Ok(())), Healthy);           // 1-unit style shortfall
        assert_eq!(check_solvency(1_001, 1_000, false, true, false, Ok(())), Freeze);           // no token
        assert_eq!(check_solvency(1_001, 1_000, true, true, true, Err(SystemAuctionError::DebtAuctionAlreadyLive)), RecoveryInProgress);
        assert_eq!(check_solvency(1_001, 1_000, true, true, false, Err(SystemAuctionError::MintCapConfig)), Freeze);
        assert_eq!(check_solvency(1_001, 1_000, true, true, false, Err(SystemAuctionError::MintAuthorityMissing)), Freeze);
        assert_eq!(check_solvency(1_001, 1_000, true, true, false, Err(SystemAuctionError::ProtocolTokenMismatch)), Freeze);
        assert_eq!(check_solvency(1_001, 1_000, true, true, false, Ok(())), NoAction);           // a keeper should start one
        assert_eq!(check_solvency(1_001, 1_000, true, false, false, Err(SystemAuctionError::MintCapConfig)), NoAction);
    }
    #[test]
    fn start_and_take_flows() {
        let acc = Accounting { unbacked_debt: 2_000, ..Accounting::default() };
        let d = start_debt(&acc, 0, true, true, true, REF, &sp(), 0, false).unwrap();
        let (d2, minted) = take_debt(&d, 250, 0, &sp(), 0).unwrap();          // half the lot at t=0 → 125 tokens
        assert_eq!((minted, d2.lot_left), (125, 250));
        assert_eq!(start_debt(&acc, 0, true, true, true, REF, &sp(), 0, true), Err(SystemAuctionError::DebtAuctionAlreadyLive));
        let clean = Accounting::default();
        let s = start_surplus(&clean, 1_500, true, 0, 0, &sp(), false).unwrap();
        let (_, burn) = take_surplus(&s, 500, u128::MAX, REF, &sp(), 0).unwrap();
        assert_eq!(burn, 263);
        assert_eq!(start_surplus(&clean, 1_499, true, 0, 0, &sp(), false), Err(SystemAuctionError::BufferBelowTarget));
        assert_eq!(start_surplus(&acc, 9_999, true, 0, 0, &sp(), false), Err(SystemAuctionError::UnbackedDebtPresent));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p auction_core accounting system`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `accounting.rs`**

```rust
//! Engine-only accounting (spec §5.8, ADR-010). Immutable updates.
use crate::auction::{BidFill, ChainEnd};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Accounting {
    pub unbacked_debt: u128, pub total_shortfall_booked: u128, pub total_penalty_collected: u128,
    pub total_debt_burned: u128, pub total_debt_settled: u128,
    pub surplus_nonce: u64, pub debt_nonce: u64, pub host_frozen_by_engine: bool,
}

impl Accounting {
    pub fn record_bid(self, f: &BidFill) -> Self {
        Self { total_debt_burned: self.total_debt_burned.saturating_add(f.debt_part),
               total_penalty_collected: self.total_penalty_collected.saturating_add(f.penalty_part), ..self }
    }
    pub fn record_chain_end(self, e: &ChainEnd) -> Self {
        Self { unbacked_debt: self.unbacked_debt.saturating_add(e.shortfall),
               total_shortfall_booked: self.total_shortfall_booked.saturating_add(e.shortfall), ..self }
    }
    /// Burns min(amount, buffer_balance, unbacked_debt). `buffer_balance` is the ACTUAL holding balance.
    pub fn settle_debt(self, amount: u128, buffer_balance: u128) -> (Self, u128) {
        let burn = amount.min(buffer_balance).min(self.unbacked_debt);
        (Self { unbacked_debt: self.unbacked_debt.saturating_sub(burn), // burn ≤ unbacked
                total_debt_settled: self.total_debt_settled.saturating_add(burn), ..self }, burn)
    }
}
```

- [ ] **Step 4: Implement `system.rs`**

```rust
//! F4 debt auction and F5 surplus auction: instant Dutch auctions, plus the solvency decision (spec §5.11, ADR-017).
#![allow(clippy::arithmetic_side_effects)] // ordered subtractions noted inline
use crate::accounting::Accounting;
use crate::math::{mul_div_ceil, mul_div_floor};
use crate::schedule::Schedule;
use crate::units::{BPS, BPS_TO_RAY, RAY};

pub const MAX_MINT_HARD_CAP: u128 = 1_000_000_000_000_000_000_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemAuctionError {
    TokenNotConfigured, UnbackedDebtPresent, BufferBelowTarget, DelayNotElapsed, InsufficientBadDebt,
    MintCapConfig, MintAuthorityMissing, ProtocolTokenMismatch, DebtAuctionAlreadyLive,
    SurplusAuctionAlreadyLive, NotLive, Slippage, DustRemainder, ZeroFill, Math,
}
impl From<crate::math::MathError> for SystemAuctionError { fn from(_: crate::math::MathError) -> Self { Self::Math } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemParams {
    pub surplus_buffer_target: u128, pub surplus_lot: u128, pub surplus_delay_ms: u64,
    pub surplus_premium_bps: u16, pub surplus_max_discount_bps: u16,
    pub debt_lot: u128, pub debt_initial_discount_bps: u16,
    pub debt_growth: Schedule, pub surplus_decay: Schedule,
    pub max_mint_per_auction: u128, pub min_take: u128, pub system_ttl_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurplusAuction { pub lot_left: u128, pub start_ms: u64, pub live: bool }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DebtAuction { pub lot_left: u128, pub start_amount: u128, pub start_ms: u64, pub live: bool }

/// Multiplier in bps at time t: (1e4 + premium − decay(t)), floored at (1e4 − max_discount).
fn surplus_multiplier_bps(s: &SystemParams, t_ms: u64) -> u128 {
    let decay_bps = s.surplus_decay.discount_ray(t_ms) / BPS_TO_RAY;
    let start = BPS + s.surplus_premium_bps as u128;
    let floor = BPS - s.surplus_max_discount_bps as u128;
    start.saturating_sub(decay_bps).max(floor)
}

/// Protocol tokens a taker pays for `coin` stablecoin: ceil(coin × mult / 1e4 × RAY / ref). Owed → round up.
pub fn surplus_required_tokens(coin: u128, ref_ray: u128, s: &SystemParams, t_ms: u64) -> Result<u128, SystemAuctionError> {
    let scaled = mul_div_ceil(coin, surplus_multiplier_bps(s, t_ms), BPS)?;
    Ok(mul_div_ceil(scaled, RAY, ref_ray)?)
}

/// floor(debt_lot × RAY / ref × (1e4 − init_discount) / 1e4); errors with MintCapConfig if above the cap.
pub fn debt_start_amount(ref_ray: u128, s: &SystemParams) -> Result<u128, SystemAuctionError> {
    let tokens = mul_div_floor(s.debt_lot, RAY, ref_ray)?;
    let amount = mul_div_floor(tokens, BPS - s.debt_initial_discount_bps as u128, BPS)?;
    if amount > s.max_mint_per_auction || s.max_mint_per_auction > MAX_MINT_HARD_CAP { return Err(SystemAuctionError::MintCapConfig); }
    Ok(amount)
}

/// Offered tokens for the whole lot at t: floor(start × (1e4 + growth(t)) / 1e4), capped. Paid out → round down.
pub fn debt_offered_tokens(start_amount: u128, s: &SystemParams, t_ms: u64) -> Result<u128, SystemAuctionError> {
    let growth_bps = s.debt_growth.discount_ray(t_ms) / BPS_TO_RAY;
    Ok(mul_div_floor(start_amount, BPS + growth_bps, BPS)?.min(s.max_mint_per_auction))
}

pub fn start_surplus(acc: &Accounting, buffer: u128, token_configured: bool, last_end_ms: u64, now_ms: u64,
                     s: &SystemParams, live: bool) -> Result<SurplusAuction, SystemAuctionError> {
    if live { return Err(SystemAuctionError::SurplusAuctionAlreadyLive); }
    if !token_configured { return Err(SystemAuctionError::TokenNotConfigured); }
    if acc.unbacked_debt > 0 { return Err(SystemAuctionError::UnbackedDebtPresent); }
    if buffer < s.surplus_buffer_target.saturating_add(s.surplus_lot) { return Err(SystemAuctionError::BufferBelowTarget); }
    if now_ms < last_end_ms.saturating_add(s.surplus_delay_ms) { return Err(SystemAuctionError::DelayNotElapsed); }
    Ok(SurplusAuction { lot_left: s.surplus_lot, start_ms: now_ms, live: true })
}

pub fn take_surplus(a: &SurplusAuction, coin: u128, max_tokens_in: u128, ref_ray: u128, s: &SystemParams,
                    now_ms: u64) -> Result<(SurplusAuction, u128), SystemAuctionError> {
    if !a.live { return Err(SystemAuctionError::NotLive); }
    let take = coin.min(a.lot_left);
    if take == 0 { return Err(SystemAuctionError::ZeroFill); }
    let left = a.lot_left - take;
    if left != 0 && left < s.min_take { return Err(SystemAuctionError::DustRemainder); }
    let tokens = surplus_required_tokens(take, ref_ray, s, now_ms.saturating_sub(a.start_ms))?;
    if tokens > max_tokens_in { return Err(SystemAuctionError::Slippage); }
    Ok((SurplusAuction { lot_left: left, live: left != 0, ..*a }, tokens))
}

#[allow(clippy::too_many_arguments)]
pub fn start_debt(acc: &Accounting, buffer: u128, token_configured: bool, mint_authority_ok: bool, token_ok: bool,
                  ref_ray: u128, s: &SystemParams, now_ms: u64, live: bool) -> Result<DebtAuction, SystemAuctionError> {
    if live { return Err(SystemAuctionError::DebtAuctionAlreadyLive); }
    if !token_configured { return Err(SystemAuctionError::TokenNotConfigured); }
    if !token_ok { return Err(SystemAuctionError::ProtocolTokenMismatch); }
    if !mint_authority_ok { return Err(SystemAuctionError::MintAuthorityMissing); }
    if acc.unbacked_debt.saturating_sub(buffer) < s.debt_lot { return Err(SystemAuctionError::InsufficientBadDebt); }
    let start_amount = debt_start_amount(ref_ray, s)?;
    Ok(DebtAuction { lot_left: s.debt_lot, start_amount, start_ms: now_ms, live: true })
}

/// Taker pays `coin` stablecoin (burned, reduces unbacked debt); receives minted tokens pro rata, rounded down.
pub fn take_debt(a: &DebtAuction, coin: u128, min_tokens_out: u128, s: &SystemParams, now_ms: u64)
                 -> Result<(DebtAuction, u128), SystemAuctionError> {
    if !a.live { return Err(SystemAuctionError::NotLive); }
    let take = coin.min(a.lot_left);
    if take == 0 { return Err(SystemAuctionError::ZeroFill); }
    let left = a.lot_left - take;
    if left != 0 && left < s.min_take { return Err(SystemAuctionError::DustRemainder); }
    let offered = debt_offered_tokens(a.start_amount, s, now_ms.saturating_sub(a.start_ms))?;
    let minted = mul_div_floor(offered, take, s.debt_lot)?;
    if minted < min_tokens_out { return Err(SystemAuctionError::Slippage); }
    Ok((DebtAuction { lot_left: left, live: left != 0, ..*a }, minted))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolvencyDecision { Healthy, Freeze, RecoveryInProgress, NoAction }

/// spec §5.11 check_solvency. Callers pass `unbacked` AFTER settle_debt.
pub fn check_solvency(unbacked: u128, threshold: u128, token_configured: bool, freeze_on_bad_debt: bool,
                      debt_auction_live: bool, start_check: Result<(), SystemAuctionError>) -> SolvencyDecision {
    use SystemAuctionError as E;
    if unbacked <= threshold { return SolvencyDecision::Healthy; }
    if !token_configured { return SolvencyDecision::Freeze; }
    if debt_auction_live { return SolvencyDecision::RecoveryInProgress; }
    if !freeze_on_bad_debt { return SolvencyDecision::NoAction; }
    match start_check {
        Err(E::MintCapConfig) | Err(E::MintAuthorityMissing) | Err(E::ProtocolTokenMismatch) => SolvencyDecision::Freeze,
        Err(E::DebtAuctionAlreadyLive) => SolvencyDecision::RecoveryInProgress,
        _ => SolvencyDecision::NoAction,
    }
}
```
Add `pub mod accounting; pub mod system;` to `lib.rs`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p auction_core accounting system`
Expected: PASS (6 tests). If `surplus_units` fails on the 200 value, recompute by hand: `mult = max(10500 − 2500, 8000) = 8000`, so `ceil(500 × 0.8) = 400` coin-equivalent, and `ceil(400 / 2) = 200`. Fix the code, not the test.

- [ ] **Step 6: Document and commit**

Extend `docs/architecture/core.md` with: the accounting flows, `settle_debt` using the actual balance, the surplus and debt auction pricing formulas with units, and the `check_solvency` decision table (one row per case in `solvency_decisions`).
```bash
git add crates/auction_core/src docs/architecture/core.md
git commit -m "feat(core): add accounting, F4 debt and F5 surplus auctions, solvency decision"
```

---

### Task 11: Invariant property tests (model-based), split-bid neutrality, two-type pause

**Files:**
- Create: `crates/auction_core/tests/invariants.rs`

**Interfaces:**
- Consumes: every public item from Tasks 4–10.

- [ ] **Step 1: Write the property tests**

```rust
//! Invariants 1–17 (spec §5.13) over random action sequences, plus split-bid neutrality and two-type pause.
use auction_core::{accounting::Accounting, auction::*, liquidation::*, pause::*, schedule::*, units::RAY};
use proptest::prelude::*;

fn params() -> CollateralParams {
    CollateralParams { penalty_bps: 1000, tip: 5, chip_bps: 10, reward_cap: 50, debt_floor: 100, min_bid: 25, schedule: rai_default() }
}

#[derive(Debug, Clone)]
enum Act { Bid { coin: u128, dt: u64 } }

fn acts() -> impl Strategy<Value = Vec<Act>> {
    prop::collection::vec((1u128..50_000, 0u64..600_000).prop_map(|(coin, dt)| Act::Bid { coin, dt }), 0..30)
}

proptest! {
    #[test]
    fn auction_invariants(collateral in 1u128..1_000_000, vault_extra in 0u128..1_000, debt in 1u128..200_000,
                          price_milli in 100u128..5_000, seq in acts()) {
        let price = RAY * price_milli / 1_000_000; // 0.0001 .. 0.005 collateral per coin
        let caps = CapState { type_cap: u128::MAX, type_reserved: 0, global_cap: u128::MAX, global_reserved: 0 };
        let i = LiquidationInput { position_collateral: collateral, vault_balance: collateral + vault_extra, nominal_debt: debt, price_ray: price };
        let q = quote_liquidation(&i, &params(), &caps).unwrap();
        // Invariant 16: reward ≤ min(penalty value, c)
        prop_assert!(q.keeper_reward <= q.seized);
        let mut a = Auction::open(&q, 0, params().schedule, 0);
        let mut acc = Accounting::default();
        let mut t = 0u64; let mut last_d = 0u128; let mut ends = 0u32;
        let chost = params().chost().unwrap();
        for Act::Bid { coin, dt } in seq {
            t = t.saturating_add(dt);
            let req = BidRequest { coin_in_max: coin, min_collateral_out: 0 };
            let Ok(f) = quote_bid(&a, &req, price, t, 25, chost) else { continue };
            // Invariant 3 and 4
            prop_assert!(f.charged <= coin);
            prop_assert!(f.discount_ray >= last_d && f.discount_ray <= a.schedule.max_discount_ray());
            last_d = f.discount_ray;
            let (n, end) = apply_bid(&a, &f);
            acc = acc.record_bid(&f);
            // Invariant 1: seized = reward + sold + left
            prop_assert_eq!(n.collateral_seized, n.keeper_reward + n.collateral_sold + n.collateral_left);
            // Invariant 3: raised ≤ target
            prop_assert!(n.raised() <= n.amount_to_raise);
            // Invariant 6: no dust remainder unless the auction ended
            prop_assert!(n.status == Status::Settled || n.remaining_target() == 0 || n.remaining_target() >= chost);
            if let Some(e) = end {
                ends += 1;
                acc = acc.record_chain_end(&e);
                // Invariant 8: raised debt + shortfall = principal
                prop_assert_eq!(n.debt_raised + e.shortfall, n.debt_principal);
                prop_assert_eq!(e.release, n.reserved_amount);
                // Invariant 15: leftover only when the target is reached
                if n.raised() < n.amount_to_raise { prop_assert_eq!(e.leftover, 0); }
            }
            // Invariant 5: terminal is absorbing
            if a.status == Status::Settled { prop_assert_eq!(n, a); }
            a = n;
        }
        prop_assert!(ends <= 1); // chain end applied once
        prop_assert_eq!(acc.total_debt_burned + acc.total_penalty_collected, a.raised());
    }

    /// Invariant 13: n bids summing to X at the same t never yield more collateral than one bid of X.
    #[test]
    fn split_bid_neutrality(parts in prop::collection::vec(25u128..5_000, 1..8), t in 0u64..3_000_000) {
        let q = LiquidationQuote { debt_principal: 1_000_000, amount_to_raise: 1_100_000, reserve: 1_100_000, keeper_reward: 0, lot: 10_000_000_000, seized: 10_000_000_000 };
        let a0 = Auction::open(&q, 0, rai_default(), 0);
        let p = RAY; // 1 collateral per coin, so every part (≥ 25) fills a non-zero amount
        let total: u128 = parts.iter().sum();
        let one = quote_bid(&a0, &BidRequest { coin_in_max: total, min_collateral_out: 0 }, p, t, 1, 1).unwrap().collateral_out;
        let mut a = a0; let mut sum = 0u128;
        for c in parts {
            let f = quote_bid(&a, &BidRequest { coin_in_max: c, min_collateral_out: 0 }, p, t, 1, 1).unwrap();
            sum += f.collateral_out; a = apply_bid(&a, &f).0;
        }
        prop_assert!(sum <= one);
    }

    /// Invariant 14 + per-type isolation: two collateral types with their own clocks.
    #[test]
    fn two_type_pause_isolation(pause_at in 1u64..1_000, resume_after in 1u64..1_000, now_extra in 0u64..1_000) {
        let o = |now, valid| Observation { now_ms: now, feed_valid: valid, admin_paused: false, oracle_ts: now, max_age_ms: 100 };
        let a0 = PauseClock::default().observed(o(0, true), OnsetPolicy::AtObservation);
        let b0 = a0;
        let a1 = a0.observed(o(pause_at, false), OnsetPolicy::AtObservation);   // type A's feed fails
        let b1 = b0.observed(o(pause_at, true), OnsetPolicy::AtObservation);    // type B is fine
        let resume = pause_at + resume_after;
        let a2 = a1.observed(o(resume, true), OnsetPolicy::AtObservation);
        let now = resume + now_extra;
        prop_assert_eq!(a2.effective_elapsed(0, 0, now), now - resume_after);   // A frozen for the pause
        prop_assert_eq!(b1.effective_elapsed(0, 0, now), now);                  // B unaffected
    }
}
```

**Coverage split (stated so nothing is silently dropped).** This task covers the core-level invariants 1, 3, 4, 5, 6, 8, 13, 14, 15, 16 and the F4/F5 math (17, partly, via Task 10). Account-level invariants depend on real holdings and the host, so they are tested in Plan 2's sequencer integration tests: 2 (vault ≥ book), 7 (cap reservation sums), 9 (buffer conservation), 10 (host/engine conservation), 11 (supply), 12 (liveness) and 17 (burn/mint totals).

- [ ] **Step 2: Run them**

Run: `PROPTEST_CASES=5000 cargo test -p auction_core --test invariants`
Expected: PASS. **If a property fails, the shrunk counterexample shows a real bug.** Fix the implementation in Tasks 8–10, add the counterexample as a named unit test in the owning module, and re-run. Never weaken the property without the user's approval.

- [ ] **Step 3: Commit**

```bash
git add crates/auction_core/tests/invariants.rs crates/auction_core/src
git commit -m "test(core): add model-based invariant, split-bid and two-type pause properties"
```

---

### Task 12: Differential model, vectors, CI gates and optional Kani

**Files:**
- Create: `tools/differential/model.py`, `tools/differential/check.py`, `crates/auction_core/examples/vectors.rs`, `crates/auction_core/src/kani_proofs.rs` (behind `#[cfg(kani)]`)
- Modify: `.github/workflows/core.yml` (uncomment the last two steps), `crates/auction_core/src/lib.rs`

**Interfaces:**
- Consumes: `mul_div_floor/ceil`, `Schedule::discount_ray`, `quote_bid`, `Auction::open`.

- [ ] **Step 1: Write the vector emitter**

`crates/auction_core/examples/vectors.rs`:
```rust
//! Emits deterministic JSON vectors for the Python differential model.
use auction_core::{auction::*, liquidation::LiquidationQuote, math::*, schedule::rai_default, units::RAY};

fn lcg(s: &mut u128) -> u128 { *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); *s }

fn main() {
    let mut s = 42u128; let mut out = Vec::new();
    for _ in 0..3_000 {
        let (x, y, d) = (lcg(&mut s) >> (lcg(&mut s) % 128), lcg(&mut s) >> 40, (lcg(&mut s) >> 60).max(1));
        out.push(format!(r#"{{"k":"md","x":"{x}","y":"{y}","d":"{d}","f":{},"c":{}}}"#,
            mul_div_floor(x, y, d).map_or("null".into(), |v| format!("\"{v}\"")),
            mul_div_ceil(x, y, d).map_or("null".into(), |v| format!("\"{v}\""))));
    }
    let sch = rai_default();
    for t in (0..3_000_000u64).step_by(7_919) {
        out.push(format!(r#"{{"k":"sch","t":{t},"d":"{}"}}"#, sch.discount_ray(t)));
    }
    let q = LiquidationQuote { debt_principal: 900_000, amount_to_raise: 990_000, reserve: 990_000, keeper_reward: 0, lot: 1_000_000_000, seized: 1_000_000_000 };
    let a = Auction::open(&q, 0, rai_default(), 0);
    let p = RAY * 752 / 1_000_000;
    for i in 0..500u128 {
        let coin = 25 + (lcg(&mut s) % 900_000); let t = (i as u64) * 6_000;
        if let Ok(f) = quote_bid(&a, &BidRequest { coin_in_max: coin, min_collateral_out: 0 }, p, t, 1, 1) {
            out.push(format!(r#"{{"k":"bid","coin":"{coin}","t":{t},"out":"{}","charged":"{}"}}"#, f.collateral_out, f.charged));
        }
    }
    println!("[{}]", out.join(","));
}
```

- [ ] **Step 2: Write the Python model and checker**

`tools/differential/model.py`:
```python
"""Independent big-integer reference for auction_core (no shared code with the Rust crate)."""
RAY = 10**27
BPS_TO_RAY = 10**23
U128 = 2**128 - 1

def mul_div_floor(x, y, d):
    q = (x * y) // d
    return q if q <= U128 else None

def mul_div_ceil(x, y, d):
    q = -((-(x * y)) // d)
    return q if q <= U128 else None

def rai_discount(t):
    if t >= 2_700_000:
        return 1000 * BPS_TO_RAY
    return 200 * BPS_TO_RAY + (800 * BPS_TO_RAY * t) // 2_700_000

def bid(coin, t, price, lot=1_000_000_000, remaining=990_000):
    coin = min(coin, remaining)
    denom = RAY - rai_discount(t)
    out = (coin * price) // denom
    if out >= lot:
        return lot, min(-((-(lot * denom)) // price), coin)
    return out, coin
```

`tools/differential/check.py`:
```python
import json, sys
from model import mul_div_floor, mul_div_ceil, rai_discount, bid, RAY

def s(v):
    return None if v is None else int(v)

def main(path):
    vec = json.load(open(path)); bad = 0
    price = RAY * 752 // 1_000_000
    for v in vec:
        if v["k"] == "md":
            x, y, d = int(v["x"]), int(v["y"]), int(v["d"])
            ok = s(v["f"]) == mul_div_floor(x, y, d) and s(v["c"]) == mul_div_ceil(x, y, d)
        elif v["k"] == "sch":
            ok = int(v["d"]) == rai_discount(v["t"])
        else:
            out, charged = bid(int(v["coin"]), v["t"], price)
            ok = (int(v["out"]), int(v["charged"])) == (out, charged)
        if not ok:
            bad += 1; print("MISMATCH", v)
    print(f"{len(vec)} vectors, {bad} mismatches"); sys.exit(1 if bad else 0)

if __name__ == "__main__":
    main(sys.argv[1])
```

- [ ] **Step 3: Run the differential check**

Run: `cargo run -q -p auction_core --example vectors > /tmp/vectors.json && python3 tools/differential/check.py /tmp/vectors.json`
Expected: `… vectors, 0 mismatches`, exit code 0. A mismatch is a bug in one of the two implementations. Work out which side is wrong from first principles, fix it, and record the case as a unit test.

- [ ] **Step 4: Optional Kani proofs (last in the cut order, ADR-015)**

`crates/auction_core/src/kani_proofs.rs`:
```rust
#![allow(clippy::arithmetic_side_effects)]
#[cfg(kani)]
mod proofs {
    use crate::math::*;
    #[kani::proof]
    fn mul_div_floor_never_exceeds_ceil() {
        let (x, y, d): (u128, u128, u128) = (kani::any(), kani::any(), kani::any());
        kani::assume(d > 0 && x < (1 << 64) && y < (1 << 64));
        if let (Ok(f), Ok(c)) = (mul_div_floor(x, y, d), mul_div_ceil(x, y, d)) {
            assert!(f <= c && c - f <= 1);
        }
    }
    #[kani::proof]
    #[kani::unwind(3)]
    fn schedule_bounded() {
        let t: u64 = kani::any();
        let s = crate::schedule::rai_default();
        assert!(s.discount_ray(t) <= s.max_discount_ray());
    }
}
```
Add `mod kani_proofs;` to `lib.rs`. Run `cargo kani -p auction_core` (install with `cargo install --locked kani-verifier && cargo kani setup`). If the proofs time out or the tool is unavailable in WSL, record that in `docs/design-log.md` (moved to M6 per ADR-015) and continue.

- [ ] **Step 5: Enable the CI gates and commit**

Uncomment the proptest and differential steps in `.github/workflows/core.yml`.
```bash
git add tools crates/auction_core .github/workflows/core.yml docs/design-log.md
git commit -m "test(core): add Python differential model, vectors and Kani proofs; enable CI gates"
```

---

### Task 13: Spikes S4 and S6 (zkVM cost of the core), then the Plan 1 close-out

**Files:**
- Create: `spikes/s4_s6/` (a risc0 guest that links `auction_core`), `docs/spikes/S4.md`, `docs/spikes/S6.md`
- Modify: `docs/design-log.md` (ADR-022 S4, ADR-023 S6), `README.md` (create: a project status section)

**Interfaces:**
- Consumes: `auction_core` (Tasks 4–10) and the S1 toolchain (Task 2).
- Produces: cycle counts per core operation, the schedule decision, and the `Accounting` write-set decision for Plan 2.

- [ ] **Step 1: Build a risc0 guest that runs the core operations, and measure cycles**

Scaffold `spikes/s4_s6` from the S1 toolchain (a risc0 3.0.5 host plus guest; `auction_core = { path = "../../crates/auction_core" }` in the guest). The guest reads an operation id and executes one of:
- `mul_div_floor` on worst-case operands;
- `Schedule::discount_ray` for 2 and for 8 points;
- `quote_liquidation`;
- `quote_bid` + `apply_bid`;
- `take_debt`;
- an exponential-discount variant: `RAY · (1 − r)^t` via binary exponentiation using `mul_div_floor`, with `t` in seconds up to 7_200.

The host runs each under the executor (`RISC0_DEV_MODE=1`) and prints `session.cycles()`. Record a table in `docs/spikes/S4.md`.

- [ ] **Step 2: S4 decision**

Keep the piecewise-linear schedule unless the exponential variant is within 10% of its cycles **and** reviewers would care (spec §13). Record ADR-022.

- [ ] **Step 3: S6 decision**

Using the measured cycles, decide whether `bid` updates `Accounting` on every bid or only at chain end. Measure the cost of the extra account write by serialising the `Accounting` struct (borsh-like) inside the guest. **Rule:** prefer per-bid updates (simpler R6 proof) unless they add more than 5% of a bid's cycles. Note that the sequencer executes transactions one at a time, so there is no write-conflict cost. Record ADR-023 in `docs/spikes/S6.md`.

- [ ] **Step 4: Close out**

Create `README.md` with: what the project is (one paragraph), its status ("Plan 1 complete: core + spikes"), how to run the tests, links to the spec, design log, spikes and architecture docs. Then run the full suite:
```bash
cargo fmt --all -- --check && cargo clippy -p auction_core --all-targets -- -D warnings && \
PROPTEST_CASES=5000 cargo test -p auction_core && \
cargo run -q -p auction_core --example vectors > /tmp/vectors.json && python3 tools/differential/check.py /tmp/vectors.json
```
Expected: everything is green.

- [ ] **Step 5: Commit**

```bash
git add spikes/s4_s6 docs README.md
git commit -m "docs: record spikes S4 and S6; close out Plan 1"
```

**Handoff:** once Task 13 is done, report the ADR-018…023 outcomes to the user. Plans 2 (programs), 3 (SDK, CLI, CI and real proofs) and 4 (thin GUI) are then written from those outcomes.
