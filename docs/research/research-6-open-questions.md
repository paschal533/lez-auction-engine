# Research 6: open questions for the RFP-014 increasing-discount auction engine

Date: 2026-10-01. Read-only research. No code was changed in any repo.

## Sources and pins

| Repo | SHA read | Local path (scratchpad) |
|---|---|---|
| logos-blockchain/lez-programs | `64c01210adecd6b557dbd021a3cc94553f243fd8` | `r2/lez-programs` |
| logos-blockchain/logos-execution-zone, tag v0.2.4 | `47eba256479f6f785acbd138834340703cd03401` | `r2/lez` |
| logos-co/spel, pinned rev | `1ef0500f8fc8ce3ddf95523726ae159db83f744f` | `spel-1ef` (a new worktree of `r2/spel`, which sits at HEAD `415b7bcb`) |
| reflexer-labs/geb master | `1a59f16a377386c49f520006ed0f7fd9d128cb09`. I diffed `r3/CollateralAuctionHouse.sol` against raw master and it is identical. | `r3/` |
| logos-execution-zone-module (lez_core) 0.4.1 | `acf0cd501b262c4c15969e3735e85318297b85bf`. The header and impl were captured earlier as `repos/lez_acf.{h,cpp}`. | `repos/` |
| logos-blockchain/lez-indexer-module | HEAD `b27f3ee7`. History was checked. `a1c10fe8` pins LEZ v0.2.2 and `d101d7f3` pins v0.2.5-rc2; no commit pins v0.2.4. | `r6-indexer-module` (new) |
| RFP-014 text | from earlier research | `r5/rfp014.md` |

I also probed the live testnet sequencer at `https://testnet.lez.logos.co/` on 2026-10-01:
- `getLastBlockId` returned `872`.
- `getBlockRange` returned base64 Borsh blocks.
- `getProgramIds` worked.

---

## Summary of recommendations

| Q | Recommendation |
|---|---|
| Q1 | **The oracle quotes the stablecoin's market price in collateral units** (collateral per 1 stablecoin, raw base units, read at 1e27). This is the same unit and direction as `redemption_price`. **Value a bid with a bounded price** `P = clamp(M, R·lo, R·hi)`, where M is the fresh market price and R is the redemption price. Fall back to `P = R` when the feed is stale or fails a sanity band. Setting `lo = hi = 1` gives RAI's production behaviour (pure redemption). **Formula:** `collateral_out = floor(coin_in · P / (ONE − d(t)))`. P and d are RAY (1e27), the schedule is configured in bps and ms, and the coins charged on lot exhaustion are `ceil(lot · (ONE − d) / P)`. |
| Q2 | **Per bid, apply a waterfall.** Coins first pay the auction's remaining *debt part*, which is burned straight from the bidder's holding with `Token::Burn`. The excess is the *penalty part* and goes by `Token::Transfer` to an engine-PDA **surplus-buffer** holding. **On a terminal state**, book `shortfall = remaining_debt` into an engine `unbacked_debt` counter. A permissionless **`settle_debt`** burns `min(buffer, unbacked_debt)` from the buffer (GEB `settleDebt`). F4 (freeze) triggers on `unbacked_debt > 0` after settling with no protocol token configured; F5 (surplus auction) triggers on `buffer − unbacked_debt > threshold`. **Keep all auction accounting in the engine.** The host needs only the seize hook and a debt-generation-pause hook. Do **not** materialize stability fees in the PoC. |
| Q3 | **Positions are reusable, so `(config, position)` alone collides.** LEZ PDAs can never be released. Use (a): an engine-owned **`LiquidationRecord` PDA** `[LIQ_RECORD, config, position]` holding `count`, with auction PDA `[AUCTION, config, position, arg(liq_nonce)]` and `liq_nonce == record.count` asserted. SPEL 1ef0500 supports `arg()` seeds (`u64 → 32 bytes`) but cannot read seeds from account data or the clock. Option (c) (clock block_id) is unworkable: the PDA must be listed in the tx before the execution block is known. |
| Q4 | **v0.2.4 has no "accounts by program owner" RPC anywhere.** The sequencer has `getBlockRange`, `getTransaction` and `getAccount`. The indexer has `getTransactionsByAccount`, but it needs a Bedrock L1 node and there is no v0.2.4-pinned module build. lez-programs enumerates only with static registries plus wallet `list_accounts`. **PoC:** positions = local list ∪ an FFI block scanner over sequencer `getBlockRange` that filters txs touching the stablecoin `ProtocolParameters` PDA. Auctions are derived from `LiquidationRecord` counters, plus a bounded on-chain live-auction list in engine state. **Production:** indexer `getTransactionsByAccount(protocol_parameters_pda)`, then LP-0012 events from v0.3. |
| Q5 | **lez_core 0.4.1 "success" only means the sequencer passed stateless checks.** Failed txs are silently dropped at block build. **Pattern:** record the signer nonce and block height, submit, then poll `get_account_public(signer).nonce` (it increments only when the tx is applied) and an effect predicate on the target PDA. Give up after `h0 + 5` blocks. Optionally, the FFI calls sequencer `getTransaction(hash)` directly; this is what the LEZ wallet's `TxPoller` does. `sync_to_block` only scans *private* txs, so use it only after the reshield step. |

---

## Q1. Price convention for bids

### 1.1 What the oracle quotes

`programs/twap_oracle/core/src/lib.rs` (lez-programs `64c0120`, l.336–353):

```rust
pub struct OraclePriceAccount {
    /// Canonical identifier for the priced asset.
    pub base_asset: AccountId,
    /// Canonical identifier for the quote asset that denominates `price`.
    pub quote_asset: AccountId,
    /// Amount of `quote_asset` one unit of `base_asset` is worth.
    pub price: u128,
    pub timestamp: u64,
    ...
}
```

The stablecoin binds the labels in two places.

`programs/stablecoin/core/src/lib.rs:62-63`:

```text
/// 8. `market_price_oracle` — initialized; validated as an `OraclePriceAccount` with
///    `base_asset` = stablecoin definition, `quote_asset` = collateral definition.
```

`programs/stablecoin/src/admin.rs:292-297` (`set_market_price_oracle`):

```text
"New oracle base_asset must equal the stablecoin definition's account_id"
"New oracle quote_asset must equal the collateral definition's account_id"
```

So **`price` = collateral per 1 stablecoin.** It is in raw base units: `TokenDefinition::Fungible` has no decimals field. The stablecoin reads it at **1e27** fixed point, because the controller subtracts it straight from the redemption price.

`programs/stablecoin/core/src/controller.rs:69-72`:

```rust
// error = redemption − market (signed).
let error = signed(current_redemption_price)
    .checked_sub(signed(market_price))
```

The redemption price is "collateral per stablecoin, fixed-point" (`docs/stablecoin/README.md:292`). The design table (README §2) says: "Single oracle quoting stablecoin in collateral units; redemption price in collateral-per-stablecoin … Closed-loop unit system; no external reference asset."

**Consequence: there is no collateral price feed at all.** The collateral is the numeraire, so its price is identically 1. The only market input is M, the stablecoin's price in collateral, which has the same direction and scale as R.

Two pitfalls carry over from research-2 and are confirmed here:
- **Scale.** The TWAP producer writes **Q64.64**, but the host reads 1e27. Open PR #401 rejects nonzero gains until this is reconciled.
- **Direction.** The AMM feeds `spot_price_q64_64(reserve_a, reserve_b)` = `reserve_quote / reserve_base` with token A as base (`programs/amm/core/src/lib.rs:352-363`, `programs/amm/src/swap.rs:126-129`). A TWAP feed for a stablecoin/collateral pool therefore only has the right direction if the stablecoin is the pool's token A. The oracle's base/quote labels are set freely at `CreateOraclePriceAccount`, so the labels prove nothing about direction. The engine must pin its feed per instance, and should sanity-check M against R (§1.5).

### 1.2 What GEB does (IDCAH, geb `1a59f16a`, `src/single/CollateralAuctionHouse.sol`)

```solidity
// l.1396  getDiscountedCollateralPrice
return wmultiply(
  rdivide(getFinalBaseCollateralPrice(collateralFsmPriceFeedValue, collateralMedianPriceFeedValue), systemCoinPriceFeedValue),
  customDiscount);
// l.1260  getBoughtCollateral
uint256 boughtCollateral = wdivide(adjustedBid, discountedCollateralPrice);
boughtCollateral = (boughtCollateral > bids[id].amountToSell) ? bids[id].amountToSell : boughtCollateral;
```

**Collateral side.** `getFinalBaseCollateralPrice` (l.1373) clamps the fresh median to `[fsm·lowerCollateralMedianDeviation, fsm·(2−upperCollateralMedianDeviation)]`. **In this host that whole mechanism collapses**, because collateral is the unit of account and its price is identically 1. There is nothing to clamp on the collateral side.

**System-coin side** (`getCollateralFSMAndFinalSystemCoinPrices`, l.1344–1371). It starts from `redemptionPrice`. If a market price exists, it moves toward the market within `lower/upperSystemCoinMedianDeviation` around redemption:

```solidity
if (uint(systemCoinPriceFeedValue) < systemCoinAdjustedPrice) {
  systemCoinAdjustedPrice = maximum(uint256(systemCoinPriceFeedValue), floorPrice);
} else {
  systemCoinAdjustedPrice = minimum(uint256(systemCoinPriceFeedValue), ceilingPrice);
}
```

If the market feed is invalid, it stays at redemption: `getSystemCoinMarketPrice` returns 0 and the `if` is skipped.

`getSystemCoinFloorDeviatedPrice` (l.1325) returns R itself when the band is narrower than `minSystemCoinMedianDeviation`. With the defaults (`lower = upper = WAD`, `min = 0.999E18`, l.1053–1057), and with RAI's production values (1.0/1.0/0.96, research-3 §1.4), **P = R always. RAI valued bids at the redemption price.**

There is also a quirk in GEB `buyCollateral` (l.1587–1640). When the lot is exhausted, the bidder still pays the whole `adjustedBid` (`transferInternalCoins(..., adjustedBid*RAY)`) even though `boughtCollateral` was capped. **Do not copy this.** Charge only the coins needed for the remaining lot.

### 1.3 Translating to this host's convention

GEB's coins-per-collateral price is `collateralPrice / coinPrice × discount`, so `bought = bid / that`. With collateral price = 1 and coin price = P (collateral per coin):

```
collateral_out = coin_in × P / (1 − d)
```

Here `(1 − d)` is GEB's discount multiplier (e.g. 0.95). This matches research-3 §(iii): "`collateral_out = coin_in × redemption_or_market_price / discount_multiplier`".

### 1.4 Redemption vs. market vs. bounded: what happens to outcomes

Notation:
- π = M/R is the coin's market premium over redemption.
- A bidder pays coins worth M collateral each and receives P/(1−d) collateral per coin.
- Bidder edge = P/(1−d) − M, in collateral per coin.
- The host liquidates when `C < N·R·mcr` (`src/checks.rs`, research-2 §1), so a lot holds about `C ≈ N·R·mcr` collateral.
- Full recovery of `A = N·penalty` needs `A·P/(1−d) ≤ C`.

Worked example: N = 1000, R = 1, mcr = 1.5, liquidated at C = 1450, penalty 10% (A = 1100), discount 5% rising to 25%.

| π = M/R | P = R (RAI) | P = M (unbounded) | P = clamp(M, 0.9R, 1.1R) |
|---|---|---|---|
| 0.90 (coin cheap) | Clears at once at d = 5%. Uses 1158 collateral. Bidder edge ≈ 17% of value (168 collateral) **taken from the owner's leftover**. No bad debt. | Uses 1042. Edge 5.3%. Owner keeps 408. | P = 0.9R. Uses 1042 at d = 5%. Same as market. |
| 1.00 | 1158 used, edge 5% | same | same |
| 1.10 | No bid until d ≥ 9.1%. ~1210 used. | 1274 used at 5%. Clears immediately. | P = 1.1R. Same as market. |
| 1.30 | No bid until d ≥ 23.1%. ~1430 used; recovers just in time. | The lot (1450) runs out after raising 1060. **Debt fully covered**, penalty partly. | No bid until d ≥ 15.4%. ~1430 used. |
| 1.40 | Needs d ≥ 28.6% > d_max = 25%: **the auction stalls and expires, so the whole N = 1000 becomes bad debt**, even though collateral was there. | Raises 984. Shortfall 16. | Needs d ≥ 21.4%. Clears. |
| manipulated M ↑ (attacker pumps the coin's price) | Not affected | **Drains the lot.** The attacker gets P/(1−d) per coin, the owner loses collateral, and bad debt appears if the lot runs out. | Bounded at +10%. |
| manipulated M ↓ | Not affected | **Stalls auctions** (DoS), which leads to expiry and bad debt. | Bounded at −10%. |

Reading the table:
- **Coin below redemption (π < 1).** Valuing at R overpays bidders. The cost falls on the position owner's leftover collateral, not on protocol solvency, because R-valued debt is still recovered.
- **Coin above redemption (π > 1).** Valuing at R makes the auction wait until `d > 1 − 1/π`. If `π > 1/(1−d_max)` the auction never clears and the whole debt becomes bad debt. This is the dangerous case.
- **Pure market (P = M).** Clears at the intended discount to market, which is the RFP's wording ("discount to market price increases over time"). But it hands a feed manipulator a direct way to drain collateral.

The host trigger itself uses R, so when π > 1 positions are liquidated later than market risk implies. That is a host property and the engine cannot fix it. A bounded P limits how much of that risk spills into the auction.

**Recommendation: use the bounded combination, as GEB does, applied to the system-coin side only.**
- Per-collateral config: `lower_price_band_bps` (default 9000), `upper_price_band_bps` (default 11000), and `price_sanity_factor` (default 2×).
- `lo = hi = 10000` reproduces RAI exactly.
- Choose `max_discount_bps` so that `1 − d_max ≤ hi/π_stress`. This keeps auctions clearable up to the premium you design for.
- Treat M as **invalid** when any of these holds:
  - `now − ts > max_age`;
  - `M == 0`;
  - M lies outside `[R/2, 2R]`. This also catches a mis-scaled Q64.64 feed or one with inverted direction.
- **When M is invalid, use `P = R`.** This is GEB's fallback, and R cannot be manipulated.
- Bids on collateral that has already been seized may continue at `P = R`. *New seizures* still pause on a stale feed (R2/R4); that gate lives in `liquidate` and the host hook.
- An optional `pause_bids_on_stale_feed` flag gives the strictest reading of R2.

### 1.5 Final formula (scales and rounding)

Scales:
- `ONE = 1e27` (the host's `FIXED_POINT_ONE`, `core/src/math.rs:12`).
- Config fields are in **bps** (`BPS = 10_000`).
- Time is in **ms** from `CLOCK_01` (`ClockAccountData.timestamp`). Within a block it is the previous block's timestamp.

**R, the redemption price.** Read-only projection with the host's own math:

```
R = compute_current_redemption_price_wide(rp_at_last_update, rate_per_ms, last_updated_at, now)
```

- Source: `stablecoin_core::math`.
- Returns `U512`; the engine must reject it if it does not fit `u128`.
- Assert `R > 0`.
- The account is checked as the canonical PDA owned by the stablecoin, as `decode_redemption_inputs` does.

**M, the market price.**
- `oracle.price` from the oracle id pinned in the engine config, at 1e27.
- Valid iff `M > 0`, `now ≥ ts` and `now − ts ≤ max_age_ms`, and M is inside the sanity band `[R·BPS/(f·BPS), R·f]`.

**P, the bid price:**

```
P = if valid(M) { clamp(M, mul_div(R, lo_bps, BPS), mul_div(R, hi_bps, BPS)) } else { R }
```

Both bounds use floor rounding. A smaller P favours the protocol (less collateral per coin).

**d(t), the discount.**
- The schedule is up to 8 points `(t_i ms, d_i bps)`, non-decreasing, with `d_last < BPS`.
- `t = now − auction.start_ms`.
- On `[t_k, t_{k+1})`:

```
d(t) = d_k·1e23 + floor((d_{k+1} − d_k)·1e23·(t − t_k) / (t_{k+1} − t_k))
```

- Past the last point, `d(t) = d_last·1e23`.
- The scaling is exact: `1e23 = ONE/BPS`. RAY interpolation avoids bps step artefacts. Floor rounding gives a smaller discount, which favours the protocol.

**m, the multiplier:** `m = ONE − d(t)`. It is exact and `> 0`.

**Inputs from the bidder:**
- `coin_in` (max coins), `min_collateral_out` (slippage guard), and the recipient holding.
- `coin_eff = min(coin_in, remaining_to_raise)`.

**Collateral out:**
- `raw = floor(coin_eff · P / m)`. Compute this in `U256` and cap it at `lot_remaining` *before* narrowing to u128; `coin·P < 2^256` always fits.
- If `raw ≥ lot_remaining`:
  - `out = lot_remaining`;
  - `charged = ceil(lot_remaining · m / P)`, using `mul_div_ceil`. This is always ≤ `coin_eff`.
- Otherwise:
  - `out = raw`;
  - `charged = coin_eff`.

**Guards:**
- `out > 0` and `out ≥ min_collateral_out`.
- Dust rule: `remaining_to_raise − charged` is either 0 or ≥ `min_raise_dust`, and `lot_remaining − out` is either 0 or ≥ `min_lot_dust`. Otherwise revert.

**Setting up the auction at liquidation:**
- `N = mul_div_ceil(normalized_debt, acc(now), ONE)`, rounded up as in the research-2 host hook.
- `amount_to_raise = mul_div_ceil(N, penalty_bps, BPS)`, with `penalty_bps ≥ BPS`.
- Keeper incentive, if paid in collateral: `floor(C · incentive_bps / BPS)`.

**Unit check:** coin_raw × (collateral_raw/coin_raw · 1e27) / 1e27 = collateral_raw. ✓

Worked example:
- R = 2.0e27, M = 2.1e27, band ±5%, so P = min(2.1e27, 2.1e27) = 2.1e27.
- d = 10% gives m = 0.9e27.
- coin_in = 1000 gives out = floor(1000 · 2.1 / 0.9) = 2333 collateral units.

---

## Q2. Surplus handling

### 2.1 Host facts (lez-programs `64c0120`)

- **Fees are virtual and no surplus is minted.** README §2: "Fee accrual | Pure RAI virtual accrual — no surplus mint."
- **The README's version of invariant 7.** §7 states it as *"Supply ≤ total open principal"*. The supporting text says total supply "equals the mint-side debt … NOT the nominal debt … The gap `Σ(nominal_debt) − total_supply` is the protocol's accumulated fee credit (… not materialized on-chain in this design)." §14 adds: "When RFP-014 lands, the implicit fee credit … becomes extractable. May require a one-shot `materialize_surplus` instruction."
- **The host keeps no global debt counter.** Position ops only *read* globals (README §2, "Rate plumbing"), so `Σ nominal_debt` cannot be computed on-chain today.
- **There is no per-position split** of principal versus accrued fee: `Position` has only `normalized_debt_amount`.
- **`Token::Burn` needs only the holding to be authorized** (`programs/token/src/burn.rs`: `assert!(user_holding_account.is_authorized …)`). It decrements `balance` and `total_supply`. An authorized bidder holding can therefore be burned from directly in the engine's chained call: authorization flows down the chain (research-2 §5).

### 2.2 Recommended flow

**At `liquidate`.** Seize the full position through the host hook, which yields N (nominal, including accrued fees).
- Create the auction with `remaining_debt = N` and `remaining_to_raise = A = N·penalty`.
- Engine aggregate: `debt_on_auction += N`, which also serves as the Maker-style `hole` cap.
- **Nothing is burned or minted yet.**

**At `buy_collateral` (per-bid waterfall, debt first).** Compute `charged` and `out` (§1.5), then:
- `debt_part = min(charged, remaining_debt)`
- `penalty_part = charged − debt_part`

Chained calls, all in one tx:
1. `Token::Burn{debt_part}` on `[stablecoin_definition (M), bidder_coin_holding (authorized: signer)]`.
2. `Token::Transfer{penalty_part}` on `[bidder_coin_holding (authorized; pre-state = post-burn balance, hand-built as amm/swap.rs does), surplus_buffer_holding]`. Skip it if the amount is 0.
3. `Token::Transfer{out}` on `[auction_custody (authorized via engine seed), bidder_collateral_recipient]`.

Engine state updates:
- `remaining_debt -= debt_part`, `remaining_to_raise -= charged`, `lot -= out`.
- `debt_on_auction -= debt_part`.
- If `remaining_to_raise == 0`: mark the auction **Settled** and make the leftover lot claimable by the owner (pull-claim, research-2 risk 10).

Why debt first rather than a proportional split? If the lot runs out short, debt-first minimises booked bad debt, and the penalty only funds the buffer once the debt is whole. Burning straight from the bidder's holding avoids leaving coins in an intermediate per-auction holding, which keeps invariant R6 ("seized collateral equals collateral in auction") simple.

**At `settle_auction` (permissionless).** It runs when the lot is exhausted with `remaining_to_raise > 0`, or when the auction expires.
- `shortfall = remaining_debt`. The unpaid penalty is foregone revenue, not debt.
- `unbacked_debt += shortfall`, `debt_on_auction -= remaining_debt`.
- The auction becomes Settled/Expired, and any remaining lot is returned or claimable.
- If the auction stalled while collateral remains, a Maker-style `redo` (restart at the start discount) is preferable to expiry. That is a design option from research-3.

**`settle_debt()` (permissionless, GEB `settleDebt`).**
- `y = min(buffer_balance, unbacked_debt)`.
- `Token::Burn{y}` on `[stablecoin_definition, surplus_buffer_holding (authorized via engine seed)]`.
- `unbacked_debt -= y`.

**`check_solvency()` (permissionless, F4).**
- Condition: after `settle_debt`, `unbacked_debt > 0` and no protocol token is configured.
- Action: a chained call to the host's engine-gated freeze/pause-debt hook. This is research-2's `SettleAuction{freeze_new_debt}`, simplified to a pure pause hook.
- If a protocol token is configured, start a debt auction instead: mint the protocol token, receive coins, burn the coins, and reduce `unbacked_debt`.

**Surplus auction (F5).** Allowed only when a token is configured and `buffer − unbacked_debt > surplus_threshold`. Otherwise the buffer just accumulates.

### 2.3 How this interacts with the stablecoin's accounting

Let:
- `D_pos` = Σ nominal debt of open positions (host, virtual);
- `D_auc` = Σ `remaining_debt` of live auctions (engine);
- `U` = `unbacked_debt` (engine);
- `S` = `total_supply`;
- `B` = buffer balance (part of S).

Extended invariant 7′: **`S ≤ D_pos + D_auc + U`**. Each step preserves it:

| Step | Effect |
|---|---|
| liquidate | N moves from `D_pos` to `D_auc`; S is unchanged. |
| bid, debt part | S and `D_auc` both drop by `debt_part`. |
| bid, penalty part | Coins move to the buffer; S is unchanged. |
| shortfall | `D_auc` drops by `shortfall`, `U` rises by `shortfall`. |
| `settle_debt` | S and U both drop by y. |

The protocol's equity (realised plus unrealised) is `E = D_pos + D_auc + U − S + (B − U)`. Penalties raise realised surplus B. Fees on liquidated positions are realised automatically, because N includes accrued fees and is burned. This mirrors how `repay_debt` burns nominal debt including fees.

**Single source of truth.** Keep `D_auc`, `U`, the buffer and the penalties collected in the **engine** (per-collateral config/state). Drop research-2's host-side `outstanding_liquidated_debt` and `accumulated_bad_debt`; keeping both creates the dual-bookkeeping risk research-2 flagged (§9 risk 9). The host keeps only:
- `LiquidationEngineState { engine_authority_id }`, plus optionally a monotonic `total_seized_nominal_debt` and `total_seized_collateral` for audit;
- `LiquidatePosition` (the seize hook);
- a pause-debt hook gated to the engine authority.

**Stability-fee surplus (fee credit `D_pos − S_circulating`).** **Do not materialize it in the PoC.** Doing so needs a host global `total_normalized_debt`, written by every `generate_debt` and `repay_debt`. That contradicts the host's "position ops read but don't write globals" rule and changes the stablecoin's account layout. It also creates coins backed only by uncollected debt.

Document fee revenue as deflation instead: it is burned on repay and burned on liquidation. If materialization is wanted later, it would be a host `materialize_surplus(amount)` that mints at most `Σnom − S − already_materialized` into the engine buffer, gated to the engine authority.

One systemic note to put in the docs. Since `Σ nominal > S` whenever fees > 0, liquidating every position at once could not raise enough coins in aggregate. This is inherent to virtual accrual (README §2: "users acquire stablecoin from market to repay"), and in a mass-liquidation event it shows up as shortfall booked to U.

### 2.4 Accounts touched

| Instruction | Engine accounts | Host and Token accounts |
|---|---|---|
| `init_collateral_config` (admin) | config `I`, surplus-buffer holding `I` (via chained `Token::InitializeAccount`, engine-seed authorized) | stablecoin definition (read) |
| `liquidate` | config `M` (`debt_on_auction`, caps), LiquidationRecord `M/I` (Q3), Auction `I`, custody holding `I` | host `LiquidatePosition` (position M, vault M, globals, oracle, clock), then nested `Token::Transfer` vault→custody; optional keeper Transfer from custody |
| `buy_collateral` | Auction `M`, config `M` | stablecoin definition `M`, bidder coin holding `M S`, buffer `M`, custody `M`, bidder collateral recipient `M`, oracle, redemption state, clock (read); chained Burn, Transfer, Transfer (3 calls) |
| `settle_auction` | Auction `M`, config `M` (`U`, `D_auc`) | custody → owner claim holding (or record claimable) |
| `settle_debt` | config `M` | stablecoin definition `M`, buffer `M` (engine-seed authorized); `Token::Burn` |
| `check_solvency` | config (read) | host pause hook (engine authority PDA signer), `ProtocolParameters` `M` |

---

## Q3. Uniqueness of auction accounts

### 3.1 Can a liquidated position be liquidated again?

**Yes.** Research-2's hook writes `Position{collateral_amount: 0, normalized_debt_amount: 0, ..}` and keeps the data. After that:
- `deposit_collateral` only requires an initialized, owner-matching, stablecoin-owned position and vault (`src/deposit_collateral.rs:48-147`).
- `generate_debt` adds the frozen check and the collateralization check (`src/generate_debt.rs:62-187`).

Neither checks liquidation history, so the owner can refill the position, borrow again, and be liquidated again.

**Accounts can never be released, either.** LEZ v0.2.4 `lee/state_machine/core/src/program/mod.rs:698-703`:

```rust
// 4. Program ownership changes are not allowed
if pre.account.program_owner != post.account.program_owner {
    return Err(ExecutionValidationError::ModifiedProgramOwner { .. });
```

The host notes the same thing itself in `programs/stablecoin/src/close_position.rs:12-17`: "LEE's `validate_execution` forbids a program from changing an account's `program_owner` or `nonce`, so the PDA cannot be released". A claim also requires a default owner, from `lee/state_machine/src/validated_state_diff/mod.rs:217-222`:

```rust
// The invoked program can only claim accounts with default program id.
ensure!(post.account().program_owner == DEFAULT_PROGRAM_ID, …ClaimedNonDefaultAccount…);
```

So a second `liquidate` that tries to `init` the same auction PDA always fails. With seeds `(config, position)` alone, that is a **permanent liquidation DoS on any position that has been liquidated before**, which leads directly to bad debt. Partial liquidation (Maker-style caps) would cause the same collision even without reuse.

### 3.2 What SPEL 1ef0500 can express

From `spel-framework-macros/src/lib.rs`:
- `PdaSeedDef` (l.195–205) is exactly `Const(String) | Account(String) | Arg(String)`.
- The parser (l.766–800) accepts `const`/`literal`, `account` and `arg`.
- Validation (l.1534–1620) computes `spel_framework::pda::compute_pda(self_program_id, &[seeds…])`.

From `spel-framework-core/src/pda.rs`:
- An `account("x")` seed is the 32-byte account id.
- `arg("x")` uses `ToSeed`, implemented for `u64` and `u32` (LE, zero-padded to 32 bytes), `[u8;32]`, and `String`/`&str`.
- Multiple seeds are combined as `sha256(seed1‖…‖seedN)`.

`init` (l.1460–1476) only checks `account == Account::default()`, and auto-claims `Claim::Pda(seed)`. There is **no `init_if_needed`**, **no seed from another account's data**, and **no clock in `ProgramContext`** (`context.rs:27-34`: only `self_program_id` and `caller_program_id`).

A handler *can* still compute any seed in its body and return `AccountPostState::new_claimed(acc, Claim::Pda(seed))`, or use `AutoClaim::pda_from_seeds` (`spel_output.rs:64-84`). The runtime only checks `account_id == for_public_pda(program_id, seed)`. So a value read from another account or from the clock *can* be used as a seed, either:
- manually; or
- declaratively, as an `arg(...)` that the handler then asserts equals the stored value.

The hard constraint is on the client side. A public tx lists its account ids up front (`lee/state_machine/src/public_transaction/message.rs:14-19`: `Message { program_id, account_ids, nonces, instruction_data }`). Every seed must therefore be **predictable when the tx is signed**.

### 3.3 Options compared

| Option | Uniqueness | Predictable at signing time | Cross-position interference | Discoverability (Q4) | Cost and risk |
|---|---|---|---|---|---|
| **(a) Engine `LiquidationRecord` PDA** `[const("LIQ_RECORD"), account("config"), account("position")]` holding `{count: u64, live_auction: Option<AccountId>}`. Auction: `[const("AUCTION"), account("config"), account("position"), arg("liq_nonce")]` with `init`; handler asserts `liq_nonce == record.count` and writes `count + 1`. | Yes, forever | Yes: read the record (default means 0) | None. Only a same-position race fails, and that is mutually exclusive anyway. | Excellent: every auction of a known position is `k ∈ [0, count)` | One extra small account per position. Needs a manual init-if-needed claim. |
| (b) Counter map in host `LiquidationEngineState` | Yes | Yes | Global hot account. The map grows without bound under the 100 KiB `DATA_MAX_LENGTH`, and is re-serialized on every liquidation. | Good | Couples engine logic to a host redeploy; needs a host layout change. **Reject.** |
| (c) `(position, CLOCK_01.block_id)` | Practically yes | **No.** The execution block is unknown at signing. Blocks are 10–15 s (`sequencer_config.json`: `block_create_timeout` 10s/15s), so a tx landing one block later fails with `PdaMismatch`. | None | Poor: block ids must be scanned | Not expressible declaratively (needs arg + assert). **Reject.** |
| (d1) Global `auction_seq` in config, auction `[config, seq]` (GEB `auctionId`) | Yes | Only optimistically | **Yes.** Two keepers liquidating *different* positions in the same block race on the same `seq`, and the loser's tx fails. This violates R3. | Excellent (0..seq) | **Reject** for the PDA address. Fine as a *field* inside the auction. |
| (d2) Keeper-chosen random salt `arg("salt")` | Yes (init check) | Yes | None | **Poor**: needs an index | Simple. Fallback only. |
| (d3) Host tombstones the position (clears data, as `close_position` does), auction `[config, position]` | Yes: the position can never be reused, because `open_position` requires an uninitialized account | Yes | None | Good | Forbids partial liquidation forever. Breaks host module decoding of liquidated positions. Changes host semantics. Workable fallback only. |

**Recommendation: (a).** It gives derivable addresses (which also answers Q4), has no cross-position races, does not change the host, and allows future partial liquidation.

```rust
#[instruction]
pub fn liquidate(
    ctx: ProgramContext,
    #[account(signer)] keeper: AccountWithMetadata,
    #[account(mut, pda = [const("CONFIG"), account("stablecoin_program_marker")])] config: AccountWithMetadata, // shape illustrative
    #[account(mut, pda = [const("LIQ_RECORD"), account("config"), account("position")])] record: AccountWithMetadata,
    #[account(init, pda = [const("AUCTION"), account("config"), account("position"), arg("liq_nonce")])] auction: AccountWithMetadata,
    #[account(init, pda = [const("CUSTODY"), account("auction")])] custody: AccountWithMetadata,
    position: AccountWithMetadata, /* … host accounts, clock last */
    liq_nonce: u64,
) -> SpelResult
// body: let rec = if record.account == Account::default() { Record::default() } else { decode };
//       require(liq_nonce == rec.count); rec.count += 1; rec.live_auction = Some(auction.account_id);
//       record post-state: if default -> AccountPostState::new_claimed(acc, Claim::Pda(sha256(seed_from_str("LIQ_RECORD")‖config‖position)))
//                          else      -> AccountPostState::new(acc)
```

The manual claim seed must equal SPEL's `compute_pda` combination: SHA-256 of the three 32-byte seeds, in order. `AutoClaim::pda_from_seeds(&[&seed_from_str("LIQ_RECORD"), config_id, position_id])` produces exactly that.

The custody holding can be seeded by `account("auction")`, so it is unique automatically. As research-2 notes, it is created through `Token::InitializeAccount` with the engine seed in `pda_seeds`, not through a SPEL `init` claim. The handler must also make sure the SPEL "rule 7" filter (`spel-framework-macros/src/lib.rs:305-329`) does not drop any account it intends to return.

---

## Q4. Discovering positions and auctions off-chain on v0.2.4

### 4.1 What the RPCs expose

**Sequencer v0.2.4** (`lez/sequencer/service/rpc/src/lib.rs:39-92`):
- `sendTransaction`, `checkHealth`
- `getBlock(id)`, `getBlockRange(start, end)`, `getLastBlockId`
- `getAccountBalance`, `getTransaction(hash) -> Option<(tx, BlockId)>`
- `getAccountsNonces`, `getProofsAndRoot`, `getAccount`, `getProgramIds`, `getChannelId`

The block, transaction and account getters carry this comment: "These functions should be removed after wallet starts using indexer". **There is no list-by-owner or list-by-program method.**

**Indexer v0.2.4** (`lez/indexer/service/rpc/src/lib.rs:15-79`):
- `subscribeToFinalizedBlocks`, `getLastFinalizedBlockId`
- `getBlockById`, `getBlockByHash`, `getBlocks(before, limit)`
- `getAccount`, `getAccountAtBlock`, `getTransaction`
- **`getTransactionsByAccount(account_id, offset, limit)`**
- `getStatus`, `checkHealth`

The by-account index (`storage/src/indexer/write_atomic.rs:149-200`) keys each tx by `affected_public_account_ids()`, which is the signers plus `message.account_ids` (`lee/.../public_transaction/transaction.rs:43-50`). These are top-level account lists, not accounts touched only by chained calls. **There is still no by-program-owner query.** The indexer reads from a Bedrock L1 node via the zone channel (`IndexerConfig.bedrock_config.addr`, `channel_id`).

**`lez-indexer-module`** exposes the same set over the Logos protocol: `getTransactionsByAccount(account_id, offset, limit)`, `getAccount`, `getBlocks`, `getTransaction` and so on (README; header at `a1c10fe8` l.56–82). Its LEZ pins are v0.2.2 (`a1c10fe8`), v0.2.5-rc1/rc2 (`aeb96a11`, `d101d7f3`), then v0.3.0. **No release pins v0.2.4.** The 0.2.2 and 0.2.5 indexers would have to be checked for wire compatibility with a v0.2.4 sequencer's L1 inscriptions. It also has to be started explicitly (`start_indexer(config_path)`) and needs a reachable Bedrock node. A testnet explorer exists (`https://explorer.testnet.lez.logos.co/`, HTTP 200), so an indexer is running, but no public indexer RPC URL is documented in `DEPLOYMENTS.md`.

**lez_core 0.4.1** (`repos/lez_acf.h`) can:
- read single accounts: `get_account_public` is a live sequencer `getAccount` (`lez/wallet/src/lib.rs:611-615`);
- report block height: `get_current_block_height`;
- `sync_to_block`, which scans *private* txs only (`lez/wallet/src/lib.rs:905-945`: `let LeeTransaction::PrivacyPreserving(pp_tx) = &tx else { continue; }`);
- list wallet-owned accounts with `list_accounts`;
- return the sequencer URL with `get_sequencer_addr`.

It has **no block, tx or indexer queries**.

**lez-programs at `64c0120` never uses the indexer.**
- The AMM app enumerates tokens and pools from a static registry, either local JSON files or a remote document (`apps/amm/src/RegistryLoader.h`: "Loads the AMM app's 'known tokens' and 'known pools' … TOKENS_CONFIG / AMM_POOLS_CONFIG … AMM_REGISTRY_URL").
- Holdings come from wallet `list_accounts()` plus a read of each (`modules/amm/src/amm_module_impl.cpp:337`, `modules/token/src/token_module_impl.cpp:324`).
- The stablecoin module's open PR #334 ("expose position account", head `16e25331`) does "exact owner/nonce lookup", i.e. it derives a PDA from a known `(owner, nonce)`.

### 4.2 Approaches

| Approach | U4: under-collateralised positions | U5: live auctions | PoC fit | Production fit |
|---|---|---|---|---|
| **Known PDAs from a local list or registry** (AMM pattern), plus the wallet's own `(owner, nonce 0..k)` | Only positions you already know about | Derivable from the Q3 records of known positions | Trivial, and enough for a demo where we create the positions | Insufficient: misses other people's positions |
| **Block scan in the module FFI** over sequencer `getBlockRange` (HTTP to `get_sequencer_addr()`), Borsh-decoding `common::block::Block { header, body: { transactions: Vec<LeeTransaction> } }` (`lez/common/src/block.rs:40-65`) | Complete. Keep any tx (Public or PrivacyPreserving) whose public account ids include the host **`ProtocolParameters` PDA**. Every position op (open, deposit, generate, withdraw, repay, close) lists it, so this yields every position and a "dirty" set for incremental refresh. Then read each candidate and keep those that decode as a `Position` owned by the stablecoin. | Same scan with the engine config id (`liquidate`/`buy`/`settle` all list it) | Good. Persist a cursor; chunk by 100 blocks (the wallet's `seq_block_poll_max_amount`). Verified working on testnet. | Weak: a full scan on cold start (~7k blocks/day), and these sequencer methods are slated for removal |
| **Indexer** `getTransactionsByAccount(protocol_parameters_pda)` and `(engine_config)`, paged by offset | Complete, incremental, cheap | Complete | Heavy for a PoC: needs Bedrock plus an indexer at the right LEZ version | **Best for v0.2.x**, then LP-0012 events from v0.3 |
| **On-chain registry written by the engine** | Not possible for positions: the engine never sees a position until it is liquidated, and a host-side registry would add global writes to every position op | **Good**: a bounded `live_auctions: Vec<AccountId>` (e.g. ≤ 1024 × 32 B = 32 KiB < 100 KiB) in the per-collateral engine state, appended in `liquidate` and removed in `settle`. That account is already written for the caps, its address is fixed (no race), and the bound doubles as an R/P2 compute rail. | Good | Good (keep it even with an indexer, for trustless O(1) reads) |

### 4.3 Recommendation

**PoC (U4 and U5 in the GUI and CLI):**
1. **Positions:** the union of (a) local known ids (created through our CLI/GUI, pasted, or a seed JSON like the AMM registry) and (b) the FFI block scanner over `getBlockRange`, filtered on the `ProtocolParameters` PDA, with the cursor persisted in the module's data dir.
2. Health is computed off-chain with the vendored `stablecoin_core::math`, using the same U512 check as `assert_position_is_collateralized`. The incentive estimate is `incentive_bps · collateral`.
3. **Auctions:** read the engine state's `live_auctions` list. Historical auctions come from `LiquidationRecord.count` per known position.

**Production:** swap (b) for the indexer's `getTransactionsByAccount` (or `lez_indexer_module`), and move to LP-0012 events (`Liquidated`, `AuctionStarted`, `Bid`, `Settled`) when the stack reaches v0.3. Keep the derivable PDAs and the on-chain live list, so any client can check the indexer's answers against state.

---

## Q5. Transaction status with lez_core 0.4.1

### 5.1 Evidence

**What "success" means.** A successful `send_generic_public_transaction` only means `sendTransaction` passed stateless checks. In `lez/sequencer/service/src/service.rs:46-80` the handler checks size and `transaction_stateless_check()`, then returns the hash.

**Execution failures are silently dropped at block build.** `lez/sequencer/core/src/lib.rs:666-688`:

```rust
Err(err) => { error!("Transaction with hash {tx_hash} failed execution check with error: {err:#?}, skipping it"); return false; }
```

So **any tx included in a block executed successfully**, and a tx that is never included most likely failed: stale price, already liquidated, slippage, or a stale nonce.

**Signer nonces increment only when a tx is applied.** `lee/state_machine/src/state/mod.rs:217-221`:

```rust
for account_id in signer_account_ids { self.get_account_by_id_mut(account_id).nonce.public_account_nonce_increment(); }
```

Stale nonces are rejected (`validated_state_diff/mod.rs:384-390`). `get_account_public` returns `nonce` as 16-byte hex (`repos/lez_acf.cpp:154`) and reads live from the sequencer.

**How the LEZ wallet CLI confirms a tx.** It polls sequencer `getTransaction(hash)` (`lez/wallet/src/poller.rs:35-66`, `TxPoller::poll_tx`). Defaults are `seq_poll_timeout` 12 s and `seq_tx_poll_max_blocks` 5 (`wallet/src/config.rs:83-86`). wallet-ffi v0.2.4 does **not** export this; its exported functions are listed in `lez/wallet-ffi/src/*.rs`.

**lez-programs apps do not confirm at all.**
- The AMM shows "Swap submitted" and re-resolves the pool (`apps/amm/qml/pages/SwapPage.qml:215-218`, `SwapCard.qml:488-498`).
- The modules return `transactionId` only (`modules/stablecoin/src/stablecoin_module_impl.cpp:348-351`, `modules/amm/src/amm_module_impl.cpp:507`).
- `LogosWalletProvider::loadSnapshot` calls `sync_to_block(get_current_block_height())` (`apps/shared/wallet/src/LogosWalletProvider.cpp:345-355`). That only refreshes *private* accounts.

### 5.2 Recommended pattern (engine module, Qt-free C++)

```
confirm(tx_kind, signer_id, effect_predicate):
  n0 = nonce(get_account_public(signer_id))       // before submit
  h0 = get_current_block_height()
  r  = send_generic_public_transaction(...)        // {success, tx_hash}
  if !r.success: return Rejected(r.error)          // stateless rejection
  loop every ~2 s (block time 10–15 s):
     n = nonce(get_account_public(signer_id))
     if n > n0:                                     // applied ⇒ executed successfully
         return effect_predicate() ? Confirmed : ConfirmedButSuperseded
     if get_current_block_height() >= h0 + K (K = 5, the wallet default):
         return Dropped("not included within K blocks; likely failed execution — re-quote and retry")
```

Effect predicates, read through `get_account_public`:
- **liquidate:** the `LiquidationRecord.count` went up and `Auction(nonce)` exists.
- **bid:** the auction's `remaining_to_raise` or bid counter changed, and the recipient's collateral balance rose.
- **settle:** the auction is in a terminal state.

Notes:
- **Serialize submissions per signer.** The wallet stamps the current nonce at submit time, so a second tx from the same signer in the same block is dropped. The privacy path (deshield → op → reshield) must run step by step, waiting on `confirm` between steps. For a single-use ephemeral account the nonce check is exact (0 → 1).
- **Private accounts.** After the reshield step is confirmed, call `sync_to_block(get_current_block_height())` so the wallet decrypts the new private note. This is the only correct use of `sync_to_block` here.
- **Optional precise path.** The FFI (Rust) may call sequencer JSON-RPC `getTransaction(tx_hash)` over HTTP at `get_sequencer_addr()`, exactly as `TxPoller` does. `Some((tx, block_id))` gives a definitive inclusion block. Keep the nonce and effect check as the portable fallback, since this method is marked for removal from the sequencer.
- **Forward compatibility.** lez_core ≥ 0.4.3 (LEZ 0.3) adds `bool poll_transaction_status(tx_hash)` (research-4 §3). Wrap confirmation behind one module method, `transactionStatus(tx_hash, signer, n0, h0)`, so the backend can be swapped when the pin moves.

---

## Open risks found during this research

1. **Oracle scale and direction.** The host reads 1e27; TWAP writes Q64.64. AMM price direction depends on which pool token is A. The `[R/2, 2R]` sanity band in §1.5 turns a mis-scaled or inverted feed into the `P = R` fallback instead of a drain.
2. **Redemption price overflow.** `compute_current_redemption_price_wide` can exceed u128 at extreme rates (math.rs docs: "overflows the u128 projection about 45 minutes out" at the maximum rate). The engine must reject or pause, not panic in an unexpected place.
3. **Stall when π > 1/(1−d_max).** With P anchored near R, auctions stall in a large premium regime. Pick `d_max` and `hi` together, and prefer restart over expiry.
4. **Indexer version gap.** No `lez-indexer-module` build pins v0.2.4, and the sequencer `getBlock*` methods are marked for removal. The block scanner is a PoC-only crutch.
5. **Live testnet state.** The testnet sequencer reported block 872 on 2026-10-01, which suggests a recent reset. Re-check `DEPLOYMENTS.md` and `getProgramIds` before relying on recorded ids.
