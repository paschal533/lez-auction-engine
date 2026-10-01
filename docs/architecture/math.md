# 256-bit Math: Multiply-Divide with Explicit Rounding

## Overview

The `auction_core::math` module provides safe, explicit arithmetic for the RFP-014 liquidation and auction engine. Specifically, it implements 256-bit intermediate multiply-divide (`x * y / d`) with two explicit rounding modes: floor and ceiling.

This module exists because:
1. **No u256 dependency**: Adding a u256 crate introduces significant code size overhead and is incompatible with zkVM cost targets (recursion, proving, verification time).
2. **Explicit rounding**: Floor and ceiling rounding are explicit in the API, preventing silent truncation bugs when proportions and interest rates are computed.
3. **Safety**: The functions return errors on division by zero and overflow rather than silently wrapping.

## Architecture

### Limb Decomposition (`mul_wide`)

The product of two 128-bit numbers can be up to 256 bits. We decompose each operand into high and low 64-bit limbs:

```
a = a1 * 2^64 + a0
b = b1 * 2^64 + b0
```

The product is computed via four partial products:

```
p00 = a0 * b0            (fits in u128)
p01 = a0 * b1            (fits in u128)
p10 = a1 * b0            (fits in u128)
p11 = a1 * b1            (fits in u128)
```

These are then reassembled into `(hi, lo)` where the full 256-bit result equals `hi * 2^128 + lo`.

**Key insight**: Each partial product fits in u128, so no intermediate overflow occurs. The only subtlety is the mid-stream carry handling when adding `p01` and `p10` contributions; the mid sum is bounded by `3·(2^64−1) < 2^66`, which fits in u128.

### Restoring Division (`div_wide`)

Given a 256-bit numerator as `(hi, lo)` and a 128-bit denominator `d`, we perform long division bit-by-bit:

```
N = hi * 2^128 + lo
q, r = divmod(N, d)
```

The algorithm processes each of the 128 bits of the lower limb from most to least significant. At each step:
1. We capture the carry bit (top bit of the current remainder).
2. We shift the remainder left by 1 bit and bring down the next bit from `lo`.
3. We test if the carry bit was set or the updated remainder is ≥ `d`.
4. If so, we subtract `d` and set the corresponding bit of `q`.

**Invariant**: `rem < d` is maintained at the start of each iteration.

**Quotient bounds**: The `hi >= d` guard ensures the final quotient fits in u128 by checking that the initial remainder (which is `hi`) is less than the divisor.

**Wrapping arithmetic safety**: Before the subtraction, the true 129-bit value is `carry·2^128 + rem`, which is `< 2d` (because `rem < d` before the shift, and `carry ∈ {0, 1}`). If `carry == 1` or `rem >= d`, then `carry·2^128 + rem >= d`, so subtracting `d` yields a value `< d` that fits in u128. The `wrapping_sub` computes this result modulo 2^128, which is exact since the true value already fits in u128.

## Public API

### Types

```rust
pub enum MathError {
    DivByZero,    // Denominator is zero
    Overflow,     // Quotient does not fit in u128
}
```

### Functions

#### `mul_wide(a: u128, b: u128) -> (u128, u128)`
Computes the full 256-bit product of two u128s. Returns `(hi, lo)` where the product equals `hi * 2^128 + lo`.

**Safe**: Operates only on addition, shift, and 64×64→128 multiplication; no overflow.

#### `div_wide(hi: u128, lo: u128, d: u128) -> Result<(u128, u128), MathError>`
Divides `hi * 2^128 + lo` by `d`, returning `(quotient, remainder)`.

**Errors**:
- `DivByZero` if `d == 0`.
- `Overflow` if `hi >= d` (quotient would not fit in u128).

#### `mul_div_floor(x: u128, y: u128, d: u128) -> Result<u128, MathError>`
Computes `floor(x * y / d)`.

**Algorithm**:
1. Compute `(hi, lo) = mul_wide(x, y)`.
2. Divide to get `(q, r) = div_wide(hi, lo, d)`.
3. Return `q` (the quotient is the floor).

**Errors**: `DivByZero` or `Overflow` from `div_wide`.

#### `mul_div_ceil(x: u128, y: u128, d: u128) -> Result<u128, MathError>`
Computes `ceil(x * y / d)`.

**Algorithm**:
1. Compute `(hi, lo) = mul_wide(x, y)`.
2. Divide to get `(q, r) = div_wide(hi, lo, d)`.
3. If remainder `r == 0`, return `q`. Otherwise, return `q + 1`.

**Errors**: `DivByZero` or `Overflow` from `div_wide`, or `Overflow` if `q + 1` overflows.

## Rounding Semantics

**General rule**: Amounts paid out round down (floor); amounts owed or charged round up (ceil).

### Floor

When exact division is impossible, we round down:

```
floor(7 * 3 / 2) = floor(21 / 2) = floor(10.5) = 10
```

Use this for distributing a pool or calculating interest that accrues to the protocol (never overestimate outflows).

### Ceiling

When exact division is impossible, we round up:

```
ceil(7 * 3 / 2) = ceil(21 / 2) = ceil(10.5) = 11
```

Use this for interest that accrues to borrowers or when you owe someone (always ensure full repayment).

### Call Sites

| Module | Operation | x | y | d | Mode | Rationale |
|--------|-----------|---|---|---|------|-----------|
| *(future)* | *(TBD)* | ... | ... | ... | floor/ceil | *(added as tasks progress)* |

## Overflow Handling

### When `hi >= d` in `div_wide`

The quotient would exceed u128::MAX. We return `MathError::Overflow` rather than:
- Silently truncating to fit (loses correctness).
- Panicking (loses graceful error handling).

**Example**: `mul_div_floor(u128::MAX, u128::MAX, 1)` returns `Err(MathError::Overflow)` because the true result is `u128::MAX * u128::MAX > u128::MAX`.

### When `q + 1` overflows in `mul_div_ceil`

If `q == u128::MAX` and we need to add 1 for ceiling, `checked_add` returns `None`. We convert this to `Err(MathError::Overflow)`.

**Example of no overflow**: `mul_div_ceil(u128::MAX, 2, 2)` returns `Ok(u128::MAX)` because `(u128::MAX * 2) / 2 = u128::MAX` with no remainder, so no rounding up is needed.

**Ceiling overflow with remainder**: If the quotient is `u128::MAX` and the remainder is non-zero, ceiling must add 1 to the quotient, causing overflow. Such cases return `Err(MathError::Overflow)`.

**Concrete example** (see test `ceil_overflow_with_remainder`): Let `x = 7`, `y = 2·(u128::MAX / 7) + 1`, `d = 2`. Since `2^128 ≡ 4 (mod 7)`, we have `2^129 - 1 ≡ 0 (mod 7)`, so `y = (2^129 - 1) / 7` exactly. The product `x·y = 2^129 - 1` divides by 2 to give `q = u128::MAX, r = 1`. Thus `mul_div_floor(x, y, d) = Ok(u128::MAX)` and `mul_div_ceil(x, y, d) = Err(MathError::Overflow)`.

## Test Coverage

The test suite includes:
1. **Small values**: Verify correctness on simple cases (e.g., `7 * 3 / 2`).
2. **Large products**: Confirm handling of products that exceed u128 but fit in 256 bits (e.g., RAY * RAY).
3. **Error cases**: Division by zero, overflow on both floor and ceil.
4. **Proptest reference**: 256 random property tests comparing against a `ruint::U256` reference implementation to ensure correctness across the full input space.

## Safety

The crate denies `clippy::arithmetic_side_effects` globally. The `math.rs` and `units.rs` modules opt out with `#![allow(clippy::arithmetic_side_effects)]` because every operation in these modules is provably bounded as documented inline. All functions use only safe Rust with `#[forbid(unsafe_code)]`.
