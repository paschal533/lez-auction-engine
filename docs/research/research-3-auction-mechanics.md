# Research 3: Auction Mechanics for the RFP-014 Liquidation & Auction Engine (LEZ)

Date: 2026-10-01. Sources: GEB `master` (`src/single/*.sol`), MakerDAO `dss` `master` (`dog.sol`, `clip.sol`, `abaci.sol`), live Ethereum mainnet parameters read via `eth_call` today, RFP-014 and its appendix, LEZ `lez-programs` (stablecoin host, clock), and incident write-ups. Everything below is first-hand from source unless it is marked as inference.

---

## 0. Executive summary

1. **GEB's `IncreasingDiscountCollateralAuctionHouse` (IDCAH) is a good fit and is simpler than its reputation.** It has no bidding war, no capital lockup and no `settleAuction` (that function is a no-op). Each `buyCollateral` is a single O(1) step: oracle price × discount multiplier, then a partial fill. The parts to change for LEZ are:
   - the `rad` (1e45) accounting unit, which cannot exist in u128;
   - per-second compounding through `rpower`;
   - auctions that never expire (`totalAuctionLength = uint48(-1)`);
   - no direct keeper reward. GEB's `liquidateSAFE` pays nothing, but RFP F1 requires an incentive;
   - a governance-only `terminateAuctionPrematurely` that sends the collateral to the caller.
2. **On GEB's own production numbers, exponential and linear schedules are the same.** RAI ETH-A ran from 2% to 10% discount over 45 minutes. The compounded curve and a straight line differ by at most **0.09 percentage points**. Recommendation: a **configurable piecewise-linear schedule** of up to 8 breakpoints. It can express linear, stairstep, hold-then-ramp or approximate-exponential curves, and costs one bounded scan plus one `mul_div` per bid. Note that an exponential with rate ≤ 1 *can* be computed safely with 256-bit intermediates in O(log t) (the host stablecoin already does this for fees), so "rpower overflows" is a statement about naive u128, not a hard limit.
3. **Use Maker Liq 2.0 for its safety rails, not its price curve:**
   - keeper incentive `tip + chip·tab`, gated by `chost` so dust cannot farm it;
   - global and per-collateral caps `Hole`/`hole`;
   - the dust rule that turns a "would leave dust" partial liquidation into a full one instead of reverting;
   - the `max` price (slippage) argument on `take`;
   - restarting an auction that has gone stale.
4. **The accounting can be much simpler than GEB's.** GEB used a 28-day `debtQueue` because it could not know when an auction would end. Our auctions have terminal states, so shortfall is booked **when an auction reaches Settled or Expired**: `shortfall = max(0, principal − raised)`. Bad debt is netted against the surplus buffer. If any bad debt remains and no protocol token is configured, a permissionless `check_solvency` freezes the host through the RFP-002 freeze authority.
5. **LEZ specifics that drive the design:**
   - Time comes from the sequencer-written `CLOCK_01` account (milliseconds), never from a caller argument.
   - One program ImageID is shared by every deployment, so an "instance" is a config PDA under that program.
   - The current reference host (`lez-programs/programs/stablecoin`) has **no liquidation or seize instruction**, so the integration interface (F7) needs a host-side `seize_collateral_and_debt` gated to the engine's PDA.
   - The host uses RAY (1e27) u128 with `alloy_primitives::U256` intermediates and expresses redemption price as *collateral per stablecoin*.
6. **Debt and surplus auctions:** I recommend the instantly-settling Dutch/discount form rather than GEB's English (lock-and-refund) houses. English auctions refund the outbid bidder later. That conflicts with the RFP privacy rule that an ephemeral deshielded account is used once, then reshielded, and never reused.

---

## 1. Reflexer GEB: contract-by-contract

Units in GEB: `wad` = 1e18, `ray` = 1e27, `rad` = 1e45 (wad·ray, used for coin and debt balances).

### 1.1 SAFEEngine (core ledger)

**State:**
- `collateralTypes[ct] = {debtAmount, accumulatedRate[ray], safetyPrice[ray], debtCeiling[rad], debtFloor[rad], liquidationPrice[ray]}`
- `safes[ct][addr] = {lockedCollateral[wad], generatedDebt[wad]}`
- `tokenCollateral[ct][addr]` (free collateral, wad)
- `coinBalance[addr]` (rad), `debtBalance[addr]` (rad, i.e. unbacked debt)
- `globalDebt`, `globalUnbackedDebt`, `globalDebtCeiling`, `safeDebtCeiling`, and the `safeRights` delegation map

**Liquidation-relevant functions:**

```solidity
function confiscateSAFECollateralAndDebt(bytes32 collateralType, address safe,
    address collateralCounterparty, address debtCounterparty,
    int256 deltaCollateral, int256 deltaDebt) external isAuthorized {
    safe_.lockedCollateral = addition(safe_.lockedCollateral, deltaCollateral);
    safe_.generatedDebt    = addition(safe_.generatedDebt, deltaDebt);
    collateralType_.debtAmount = addition(collateralType_.debtAmount, deltaDebt);
    int256 deltaTotalIssuedDebt = multiply(collateralType_.accumulatedRate, deltaDebt);
    tokenCollateral[collateralType][collateralCounterparty] = subtract(..., deltaCollateral);
    debtBalance[debtCounterparty] = subtract(debtBalance[debtCounterparty], deltaTotalIssuedDebt);
    globalUnbackedDebt = subtract(globalUnbackedDebt, deltaTotalIssuedDebt);
}
function settleDebt(uint256 rad) external {          // burns equal coin + unbacked debt of msg.sender
    debtBalance[account] -= rad; coinBalance[account] -= rad;
    globalUnbackedDebt -= rad;   globalDebt -= rad;
}
function createUnbackedDebt(address debtDestination, address coinDestination, uint256 rad) external isAuthorized
```

(The deltas are negative on liquidation, so `subtract(x, negative)` *increases* the counterparty's `debtBalance` and `globalUnbackedDebt`.)

**Invariants (the "fundamental equation"):**
- `Σ coinBalance == globalDebt`
- `globalDebt == Σ_ct debtAmount·accumulatedRate + globalUnbackedDebt`
- `Σ debtBalance == globalUnbackedDebt`

Liquidation moves debt from a SAFE into the AccountingEngine's `debtBalance` (it becomes unbacked) and moves collateral into the auction house. Coins raised flow to the AccountingEngine, and `settleDebt` annihilates coin against unbacked debt.

### 1.2 OracleRelayer (price → liquidation price)

```solidity
uint256 safetyPrice_      = hasValidValue ? rdivide(rdivide(priceFeedValue*1e9, redemptionPrice_), safetyCRatio)      : 0;
uint256 liquidationPrice_ = hasValidValue ? rdivide(rdivide(priceFeedValue*1e9, redemptionPrice_), liquidationCRatio) : 0;
```

`redemptionPrice()` compounds `redemptionRate` per second with `rpower`.

**The key safety property:** an invalid or stale feed writes `liquidationPrice = 0`, and `liquidateSAFE` requires `liquidationPrice > 0`. So **a failed feed pauses liquidation for that collateral type only**, which is exactly RFP R2 and R4. The `collateralFSM` is an OSM-style one-hour delayed feed. RAI ETH-A had `safetyCRatio = liquidationCRatio = 1.35`.

### 1.3 LiquidationEngine

**State:**
- `collateralTypes[ct] = {collateralAuctionHouse, liquidationPenalty[wad], liquidationQuantity[rad]}`
- `onAuctionSystemCoinLimit`, `currentOnAuctionSystemCoins` (rad)
- `safeSaviours`, `chosenSAFESaviour`, a per-SAFE `mutex`
- `contractEnabled`, `accountingEngine`

**External functions:**
- `modifyParameters` (×4), `disableContract`
- `connectSAFESaviour` / `disconnectSAFESaviour`, `protectSAFE`
- `liquidateSAFE(ct, safe)`
- `removeCoinsFromAuction(rad)` (auth: the auction house)
- `getLimitAdjustedDebtToCover`

Core logic:

```solidity
require(mutex[collateralType][safe] == 0); mutex[...] = 1;
require(both(liquidationPrice > 0, safeCollateral*liquidationPrice < safeDebt*accumulatedRate), "safe-not-unsafe");
require(currentOnAuctionSystemCoins < onAuctionSystemCoinLimit &&
        onAuctionSystemCoinLimit - currentOnAuctionSystemCoins >= debtFloor, "liquidation-limit-hit");
// optional saviour try/catch, then re-check unsafe
uint256 limitAdjustedDebt = min(safeDebt,
    min(liquidationQuantity, onAuctionSystemCoinLimit - currentOnAuctionSystemCoins) * WAD / accumulatedRate / liquidationPenalty);
require(limitAdjustedDebt == safeDebt || (safeDebt - limitAdjustedDebt)*accumulatedRate >= debtFloor, "dusty-safe");
uint256 collateralToSell = min(safeCollateral, safeCollateral * limitAdjustedDebt / safeDebt);
safeEngine.confiscateSAFECollateralAndDebt(ct, safe, address(this), address(accountingEngine),
                                           -int(collateralToSell), -int(limitAdjustedDebt));
accountingEngine.pushDebtToQueue(limitAdjustedDebt * accumulatedRate);
uint256 amountToRaise_ = limitAdjustedDebt * accumulatedRate * liquidationPenalty / WAD;
currentOnAuctionSystemCoins += amountToRaise_;
auctionId = auctionHouse.startAuction(safe /*forgone collateral receiver*/, accountingEngine, amountToRaise_, collateralToSell, 0);
```

Observations for us:
- **No keeper reward** on `liquidateSAFE`. Keepers earn only by buying at a discount. RFP F1 needs an explicit incentive.
- **"dusty-safe" reverts.** When the cap allows only a partial liquidation that would leave dust, the call reverts and the position *cannot be liquidated at all* until the cap frees up. Maker's `bark` fixes this by liquidating the whole position instead (§2). This is a known liquidation-DoS class.
- **External saviour call inside liquidation.** That is a griefing surface: gas and DoS (the Dec-2023 GEB "liquidation DoS" zero-day was disclosed by DeFi Wonderland/HAI, but the root-cause write-up was not retrievable). **Do not put arbitrary external callbacks in the liquidation path.**

### 1.4 IncreasingDiscountCollateralAuctionHouse (the pattern named by the RFP)

**Per-auction struct `Bid`:**
- `amountToSell` (wad collateral), `amountToRaise` (rad)
- `currentDiscount`, `maxDiscount` (wad multipliers: 0.98 means a 2% discount)
- `perSecondDiscountUpdateRate` (ray), `latestDiscountUpdateTime`, `discountIncreaseDeadline`
- `forgoneCollateralReceiver` (the SAFE), `auctionIncomeRecipient` (the AccountingEngine)

**Global parameters:**
- `minimumBid`, `minDiscount`, `maxDiscount`, `perSecondDiscountUpdateRate`, `maxDiscountUpdateRateTimeline`
- `lower/upperCollateralMedianDeviation`, `lower/upperSystemCoinMedianDeviation`, `minSystemCoinMedianDeviation`
- `totalAuctionLength = uint48(-1)` (that is, never), `lastReadRedemptionPrice`

**External functions:**
- `startAuction` (auth)
- `buyCollateral(id, wad)`
- `getCollateralBought` / `getApproximateCollateralBought` (quotes)
- `getNextCurrentDiscount`, `getAdjustedBid`
- the price helpers `getCollateralMedianPrice`, `getSystemCoinMarketPrice`, `getFinalBaseCollateralPrice`, `getDiscountedCollateralPrice`, and the deviated-price helpers
- `settleAuction` (**no-op**), `terminateAuctionPrematurely` (auth)
- getters and `modifyParameters`

**Discount evolution:**

```solidity
if (now < discountIncreaseDeadline && currentDiscount > maxDiscount) {
    nextDiscount = rmultiply(rpower(perSecondDiscountUpdateRate, now - latestDiscountUpdateTime, RAY), currentDiscount);
    if (nextDiscount <= maxDiscount) nextDiscount = maxDiscount;
} else if ((currentDiscount == 0 && maxDiscount > 0) || (now >= deadline && currentDiscount != maxDiscount)) {
    nextDiscount = maxDiscount;
}
```

**Price selection.** This is the `lowerCollateralMedianDeviation` bound the brief asked about:

```solidity
function getFinalBaseCollateralPrice(uint256 fsm, uint256 median) public view returns (uint256) {
    uint256 floorPrice   = wmultiply(fsm, lowerCollateralMedianDeviation);          // e.g. fsm*0.80
    uint256 ceilingPrice = wmultiply(fsm, 2*WAD - upperCollateralMedianDeviation);  // e.g. fsm*1.20
    uint256 adjustedMedianPrice = (median == 0) ? fsm : median;
    if (adjustedMedianPrice < fsm) return max(adjustedMedianPrice, floorPrice);
    else                           return min(adjustedMedianPrice, ceilingPrice);
}
discountedPrice = wmultiply(rdivide(finalBaseCollateralPrice, systemCoinPrice /*redemption, optionally market-bounded*/), discount);
boughtCollateral = min(wdivide(adjustedBid, discountedPrice), amountToSell);
```

Purpose: the auction uses the *fresh* median when it differs from the one-hour-delayed FSM, but only within ±(1−deviation). In a crash, the delayed price is stale and too high, so nobody would bid. Using the lower spot price, floored at FSM×0.8, keeps auctions clearing without letting a manipulated spot drop the price arbitrarily. The system-coin side does the same: market price bounded around the redemption price, with RAI using the redemption price (deviations = 1.0).

**`buyCollateral` (partial fill, auto-close):**

```solidity
uint256 adjustedBid = wad;
if (wad*RAY > amountToRaise) adjustedBid = amountToRaise/RAY + 1;     // cap at what is left (rounds up 1 wei)
require(collateralFsmPriceFeedValue > 0, "collateral-fsm-invalid-value");   // stale/invalid feed -> no bids
boughtCollateral = getBoughtCollateral(..., updateCurrentDiscount(id));
require(boughtCollateral > 0);
amountToSell -= boughtCollateral;
amountToRaise = (adjustedBid*RAY > amountToRaise) ? 0 : amountToRaise - adjustedBid*RAY;
require(amountToRaise == 0 || amountToRaise >= RAY, "invalid-left-to-raise");   // dust guard
safeEngine.transferInternalCoins(msg.sender, auctionIncomeRecipient, adjustedBid*RAY);
safeEngine.transferCollateral(ct, address(this), msg.sender, boughtCollateral);
if (amountToRaise == 0 || amountToSell == 0) {        // sold all -> return leftover collateral to SAFE owner
    liquidationEngine.removeCoinsFromAuction(remainingToRaise);
    safeEngine.transferCollateral(ct, address(this), forgoneCollateralReceiver, amountToSell);
    delete bids[id];
} else liquidationEngine.removeCoinsFromAuction(adjustedBid*RAY);
```

**State machine (implicit):**
- `Nonexistent → Active` (startAuction)
- `Active → Active` (partial buy)
- `Active → Deleted`, via one of: target raised; lot exhausted (the shortfall stays as unbacked debt in AccountingEngine); or governance `terminateAuctionPrematurely`, which sends the lot to `msg.sender`, i.e. governance.

There is no expiry. After `discountIncreaseDeadline` the discount stays at max forever.

**Quirks not to copy:**
- If the lot runs out first, the bidder is still charged the full `adjustedBid` even though fewer coins were needed (the "re-strike" problem).
- `emit SettleAuction(id, bids[id].amountToSell)` runs after `delete`, so it always logs 0.
- `remainingToRaise` is computed from `wad` rather than `adjustedBid`.
- There is no slippage protection on the bid.

**Invariants:**
- collateral held by the house = Σ `amountToSell`
- `currentOnAuctionSystemCoins` = Σ `amountToRaise` of live auctions (this drifts slightly in GEB because of the `+1` rounding)
- the discount is monotone, with `maxDiscount ≤ currentDiscount ≤ minDiscount < 1`

### 1.5 AccountingEngine

**State:**
- `debtQueue[timestamp]`, `totalQueuedDebt`, `totalOnAuctionDebt`
- `surplusBuffer`, `surplusAuctionAmountToSell`, `surplusAuctionDelay`, `lastSurplusAuctionTime`
- `popDebtDelay`, `debtAuctionBidSize`, `initialDebtAuctionMintedTokens`
- `extraSurplusIsTransferred`, `surplusTransferAmount`, `extraSurplusReceiver`
- `disableCooldown`, `contractEnabled`, `systemStakingPool`, `protocolTokenAuthority`

**External functions:**
- `pushDebtToQueue` (auth), `popDebtFromQueue(ts)`
- `settleDebt(rad)`, `cancelAuctionedDebtWithSurplus(rad)`
- `auctionDebt()`, `auctionSurplus()`, `transferExtraSurplus()`
- `disableContract()`, `transferPostSettlementSurplus()`
- `unqueuedUnauctionedDebt() = debtBalance(this) − totalQueuedDebt − totalOnAuctionDebt`

**Triggers (the thresholds to port):**

```solidity
// Debt auction: only when no surplus is left and enough bad debt exists
require(debtAuctionBidSize <= unqueuedUnauctionedDebt());
settleDebt(coinBalance(this)); require(coinBalance(this) == 0, "surplus-not-zero");
totalOnAuctionDebt += debtAuctionBidSize;
debtAuctionHouse.startAuction(address(this), initialDebtAuctionMintedTokens, debtAuctionBidSize);

// Surplus auction: only when coin > debt + lot + buffer, no unbacked debt, and cooldown passed
settleDebt(unqueuedUnauctionedDebt());
require(now >= lastSurplusAuctionTime + surplusAuctionDelay);
require(coinBalance(this) >= debtBalance(this) + surplusAuctionAmountToSell + surplusBuffer);
require(unqueuedUnauctionedDebt() == 0);
```

**State machine:**
1. Debt is pushed into the time-bucketed queue.
2. After `popDebtDelay` (RAI: 28 days) it is popped. The delay gives collateral auctions time to raise coins.
3. It is then settled against surplus or sent to a debt auction.

The queue only exists because GEB auctions have no terminal "shortfall known" event. **We do not need it.**

### 1.6 DebtAuctionHouse (reverse English: mint protocol token for coins)

**Per-auction struct `Bid{bidAmount[rad], amountToSell[wad prot], highBidder, bidExpiry, auctionDeadline}`.**

**Parameters:** `bidDecrease`, `amountSoldIncrease`, `bidDuration`, `totalAuctionLength`, `protocolToken`, `accountingEngine`, `activeDebtAuctions`.

**External functions:** `startAuction` (auth), `restartAuction` (lot × `amountSoldIncrease` if no bids), `decreaseSoldAmount(id, amountToBuy, bid)`, `settleAuction` (mints protocol token to the winner), `terminateAuctionPrematurely` (only after disable), `disableContract`.

```solidity
require(bid == bids[id].bidAmount);                                  // fixed coin amount
require(bidDecrease * amountToBuy <= amountToSell * ONE);            // must accept >= 3% fewer tokens
safeEngine.transferInternalCoins(msg.sender, bids[id].highBidder, bid); // refunds previous bidder in full
if (bids[id].bidExpiry == 0) AccountingEngine(highBidder).cancelAuctionedDebtWithSurplus(min(bid, totalOnAuctionDebt));
```

**State machine:**
- `Started` → (no bid by deadline) → `restart` (lot grows ×1.2)
- `Started` → `Bidding` (bid expiry refreshed on every bid)
- `Bidding` → `Settled` (mint), once `bidExpiry` or `auctionDeadline` has passed

### 1.7 BurningSurplusAuctionHouse (English: protocol token for surplus coins, then burn)

**Parameters:** `bidIncrease` (1.05), `bidDuration`, `totalAuctionLength`, `protocolToken`.

**External functions:** `startAuction` (auth; pulls the coin lot), `restartAuction`, `increaseBidSize(id, amountToBuy==lot, bid)` (refunds the previous bidder's tokens and pulls only the increment), `settleAuction` (pays coins to the winner and burns the tokens), `disableContract`, `terminateAuctionPrematurely` (post-disable refund).

**Invariant:** tokens held by the house = Σ `bidAmount` of live auctions, and all of it is burned at settlement.

### 1.8 RAI mainnet production values (read today via `eth_call`)

Note: ETH-A `debtAmount` reads 0 today, so these are the last-configured values of a system that no longer carries ETH-A debt.

| Contract | Parameter | Value |
|---|---|---|
| IDCAH ETH-A `0x7fFd…9aDE` | minDiscount / maxDiscount | 0.98 (2%) / 0.90 (10%) |
| | perSecondDiscountUpdateRate | 0.999969118399… ray (−3.088e-5/s, reaches 10% in about 46 minutes) |
| | maxDiscountUpdateRateTimeline | 2700 s (45 min) |
| | minimumBid | 25 RAI |
| | lower/upperCollateralMedianDeviation | 0.80 / 0.80 (median allowed ±20% from the FSM) |
| | lower/upperSystemCoinMedianDeviation, minSystemCoinMedianDeviation | 1.0, 1.0, 0.96 (market price effectively unused) |
| | auctionsStarted | 83 (lifetime) |
| LiquidationEngine | liquidationPenalty ETH-A | 1.10 (10%) |
| | liquidationQuantity ETH-A | 90,000 RAI |
| | onAuctionSystemCoinLimit | 500,000 RAI |
| OracleRelayer | safety/liquidation CRatio ETH-A | 1.35 / 1.35 |
| SAFEEngine | debtFloor ETH-A | ~2,843 RAI |
| AccountingEngine | surplusBuffer | 500,000 RAI |
| | surplusAuctionAmountToSell / surplusAuctionDelay | ~28,576 RAI / 2 h |
| | popDebtDelay | 28 days |
| | debtAuctionBidSize / initialDebtAuctionMintedTokens | ~27,913 RAI / ~226.8 FLX |
| | disableCooldown | 21 days |
| BurningSurplusAuctionHouse | bidIncrease / bidDuration / totalAuctionLength | 1.05 / 1.5 h / 3 days |
| DebtAuctionHouse | bidDecrease / amountSoldIncrease / bidDuration / total | 1.03 / 1.20 / 30 min / 3 days |

GEB also had a governance-minimisation overlay (`MinimalDiscountCollateralAuctionHouseOverlay`) that restricted governance to `minDiscount`, `maxDiscount` and `perSecondDiscountUpdateRate`, with a hard `discountLimit` floor on `maxDiscount`. **We should copy the idea: put hard bounds on discount parameters in the program, not only in governance.**

---

## 2. MakerDAO Liquidations 2.0 (Dog + Clipper + Abacus)

### Dog (trigger)

**State:**
- global `Hole` (max DAI on auction), `Dirt` (current)
- per ilk `{clip, chop (penalty), hole, dirt}`

`bark(ilk, urn, kpr)`:

```solidity
require(spot > 0 && ink*spot < art*rate, "Dog/not-unsafe");
require(Hole > Dirt && milk.hole > milk.dirt, "Dog/liquidation-limit-hit");
uint256 room = min(Hole - Dirt, milk.hole - milk.dirt);
dart = min(art, room*WAD/rate/milk.chop);
if (art > dart) {
    if ((art - dart)*rate < dust) dart = art;         // take EVERYTHING rather than leave dust (no DoS)
    else require(dart*rate >= dust, "Dog/dusty-auction-from-partial-liquidation");
}
dink = ink*dart/art;  vat.grab(...);  vow.fess(dart*rate);
tab = dart*rate*chop/WAD;  Dirt += tab; ilks[ilk].dirt += tab;
id = Clipper.kick(tab, dink, urn, kpr);
```

`digs(ilk, rad)` decrements `Dirt` and `dirt` as auctions raise DAI or close.

### Clipper (auction)

**Parameters:** `buf` (start-price multiplier), `tail` (max age before reset), `cusp` (max price drop before reset), `chip` (% of tab paid to the keeper), `tip` (flat DAI to the keeper), `chost = dust·chop` (cached), `calc` (Abacus), `stopped` (breaker levels 0–3).

**State:** `sales[id] = {pos, tab, lot, usr, tic, top}`, `active[]`, `kicks`.

**External functions:** `kick` (auth), `redo(id, kpr)` (permissionless reset when `done`), `take(id, amt, max, who, data)`, `getStatus`, `list`, `count`, `upchost`, `yank` (auth), `file`.

```solidity
// kick: keeper incentive paid by minting unbacked DAI against the vow
top = rmul(getFeedPrice(), buf);           // getFeedPrice = OSM.peek / par, requires has
coin = tip + wmul(tab, chip);  vat.suck(vow, kpr, coin);
// redo: only if (now - tic > tail) || (price/top < cusp); incentive only if tab >= chost && lot*feed >= chost
// take:
require(!done, "Clipper/needs-reset"); require(max >= price, "Clipper/too-expensive");
slice = min(lot, amt); owe = slice*price;
if (owe > tab) { owe = tab; slice = owe/price; }                 // never overpay
else if (owe < tab && slice < lot && tab - owe < chost) {         // never leave dust tab
    require(tab > chost, "Clipper/no-partial-purchase"); owe = tab - chost; slice = owe/price; }
vat.flux(collateral -> who); optional clipperCall(who) flash callback; vat.move(DAI msg.sender -> vow, owe);
dog.digs(ilk, lot == 0 ? tab + owe : owe);
if (lot == 0) remove; else if (tab == 0) { return lot to usr; remove; }
```

### Abacus price curves

```solidity
LinearDecrease:                price = top * (tau - dur) / tau   (0 at tau)
StairstepExponentialDecrease:  price = top * cut^(dur/step)
ExponentialDecrease:           price = top * cut^dur
```

### Mainnet ETH-A values (read today)

| Parameter | Value |
|---|---|
| Hole / hole | 150M DAI / 40M DAI |
| chop | 1.13 |
| buf | 1.10 |
| tail | 7200 s |
| cusp | 0.45 |
| chip | 0.1% |
| tip | 250 DAI |
| dust | 7,500 DAI |
| chost | 8,475 DAI |
| calc | StairstepExponentialDecrease, `cut` = 0.99, `step` = 90 s |

### GEB vs Maker for LEZ

| Aspect | GEB IDCAH | Maker Clipper | Recommendation for LEZ |
|---|---|---|---|
| Price anchor | Oracle price × discount that grows from 2% to 10% | Starts at oracle×1.10 and decays, can go far below market | **GEB.** The price stays tied to the oracle, so there is no "fell to near zero" tail. Required by the RFP. |
| Termination | Never expires | `tail`/`cusp` then `redo` with a fresh price | **Maker-like expiry.** Expired is terminal, and a new auction is spawned for the remainder (no reversal). |
| Keeper incentive | None at trigger | `tip + chip·tab` minted from the vow | Explicit incentive (§6), gated like `chost`. |
| Caps | One global cap | Global `Hole` plus per-ilk `hole` | Both. The per-collateral cap also isolates risk (R3, R4). |
| Dust | `minimumBid`; reverts on "dusty-safe" | Full liquidation instead of revert; `chost` on take | **Maker's rules.** |
| Slippage | None | `max` price | Add `min_collateral_out` / `max_price`. |
| Flash callback | No | `clipperCall` | **No.** Flash liquidations are out of scope, and a callback is a reentrancy surface. |
| Active list | — | `active[]` array on chain | **No on-chain list.** Use events and an indexer, and keep O(1) per instruction. |
| Price staleness | FSM `valid` flag; `liquidationPrice=0` pauses | `require(has)` | Explicit max-age plus confidence check per collateral type. |

---

## 3. Failure modes and incidents, with the mitigation to build

1. **Black Thursday (Maker, 12–13 March 2020).** ETH fell about 50%. Gas spiked and some keepers broke or ran out of DAI. In 1,461 English auctions, 62,843 ETH was won for ~0 DAI, which left the system 6.65M DAI under-collateralised and forced an MKR debt auction. Root causes:
   - English auctions accept *any* first bid;
   - a short `ttl` and `tau`;
   - no keeper diversity;
   - a lagging oracle (OSM) combined with congestion.

   *Mitigation:*
   - Price every fill from the oracle with a **bounded maximum discount**, so there is no zero bid.
   - Allow no bids on a stale feed.
   - Enforce caps per collateral type and globally.
   - Give auctions an expiry and restart them at a fresh price.
   - Give keepers an explicit trigger incentive.
   - Ship an open-source keeper bot.
2. **Oracle staleness or lag in a crash** (GEB's FSM, Maker's OSM). A delayed price above market means nobody buys. *Mitigation:* GEB-style dual price. Use the TWAP or delayed price as anchor and the fresh spot when it is lower, floored at `anchor × (1 − max_deviation)`. Require `age ≤ max_age` and `confidence ≤ max_conf`, and pause per collateral type when either check fails.
3. **Oracle manipulation** (for example Compound, Nov 2020: a Coinbase DAI print caused ~$89M of liquidations; Mango and Inverse manipulated thin markets). *Mitigation:*
   - Trigger on TWAP or median (RFP R2).
   - Bound spot against TWAP.
   - Never liquidate on a single raw print.
   - Treat the deviation bound as a circuit breaker: if spot and TWAP diverge by more than X, pause liquidations of that type.
4. **Dust and rounding attacks.** Splitting bids so that rounding favours the bidder; an infinitesimal remaining `tab` that nobody will clear; dust positions that farm a flat tip. *Mitigation:*
   - Round every quantity *paid out* down and every quantity *owed* up.
   - Set a minimum bid.
   - Remaining raise must be 0 or ≥ `min_remaining` (the `chost` rule).
   - Pay the flat tip only if the debt is at least `chost`.
   - Require the host to have a debt floor.
5. **Partial-fill dust and stuck lots.** The lot is exhausted before the target is raised, or the target is raised while a sliver of lot remains. *Mitigation:*
   - Auto-close on either condition.
   - Return leftover lot to the position owner.
   - Book the shortfall at that moment.
   - Re-strike (charge only for what was received when the lot is the binding side).
6. **Liquidation DoS:**
   - the "dusty-safe" revert under a cap;
   - an external saviour callback;
   - a global counter at its cap;
   - the Dec-2023 GEB zero-day. Root cause not public in sources I could access; the class is "liquidation path revertable by attacker".

   *Mitigation:*
   - Under a cap, liquidate fully instead of leaving dust.
   - No third-party callbacks in `liquidate`.
   - Caps reduce the size of a liquidation rather than reverting it when there is any room.
   - Fuzz "is there any state in which an unsafe position cannot be liquidated?"
7. **Keeper griefing and front-running.** Discount auctions are first-come-first-served. The sequencer or other searchers can order ahead, and a price can change between quote and inclusion. *Mitigation:*
   - Slippage arguments: `max_coin_in`, `min_collateral_out`, and optionally `expected_discount_floor`.
   - Each fill is independent, so being beaten never causes a loss, only a revert.
   - Small discount steps so racing has low value.
   - The trigger reward is paid to whoever lands first, which is acceptable.
8. **Self-liquidation and incentive farming.** An owner liquidates their own position to collect the reward. *Mitigation:* the reward must always be less than the penalty (`tip + chip·tab ≤ (penalty − 1)·debt`), so self-liquidation is never profitable versus repaying. Pay the reward only above `chost`.
9. **Stale debt (fees not accrued).** Liquidating with the debt figure from the last poke under-seizes. *Mitigation:* the host's seize call must accrue or project fees first. On the LEZ host, project the `StabilityFeeAccumulator` to the `CLOCK_01` time.
10. **Caller-supplied time.** If `now` is an instruction argument, a caller can re-price at a smaller discount. *Mitigation:*
    - Read `CLOCK_01` (sequencer-written; `ClockAccountData{block_id, timestamp_ms}`).
    - Assert the account id equals `CLOCK_01_PROGRAM_ACCOUNT_ID`.
    - Keep a monotonic `last_seen_ms` per auction as a belt-and-braces check.
11. **Debt auctions of a protocol token** (GEB had a reported high-severity over-inflation bug in RAI debt auctions in Oct 2023, details sparse). *Mitigation:*
    - Hard cap on tokens minted per auction and per epoch.
    - Monotone lot growth with a ceiling.
    - Mock token in tests.
    - Invariant: `minted ≤ max_mint`.
12. **Governance termination as a drain** (GEB `terminateAuctionPrematurely` sends the lot to the authorised caller). *Mitigation:* no admin path that moves auction collateral anywhere except the bidder or the original owner. The emergency path is `pause` (stop new auctions), not `yank`.

---

## 4. u128 fixed-point design

### Which GEB math does not fit in u128 (max ≈ 3.4e38)

- `rad = wad·ray` (1e45 scale). Not representable. **Remove the unit entirely:** store debt and coins in token base units, with an index (accumulated rate) in RAY.
- `rdivide(x, y) = x·RAY / y`. For any x above 3.4e11 the product overflows. Every realistic price fails.
- `rpower` / `rmultiply`. The first squaring of a RAY value is 1e54.
- `wmultiply(price, discount)` with WAD inputs, and `amount·price`: fine only for small amounts. An 18-decimal token at 1e9 units × a 1e27 price is 1e54.

### Safe pattern

- One primitive: `mul_div(a, b, c, Rounding::{Down, Up})`, computed as a 256-bit product followed by a 256→128 divide that checks the result fits. The reference host already does this with `alloy_primitives::U256` (`mul_div`, `mul_div_ceil` in `programs/stablecoin/core/src/math.rs`). **Reuse that crate or the same approach** for one consistent rounding convention across host and engine.
- Never chain two multiplications before a division. Compose `mul_div` calls instead.
- Checked arithmetic everywhere: `overflow-checks = true` in the release profile, plus `checked_*` on every add and subtract.

### Scale choice

- **Prices and ratios: RAY (1e27).** This matches the host (`FIXED_POINT_ONE = 1e27`) and the redemption price. Precision is ample, and with `mul_div` there is no overflow concern.
- **Discount multiplier and schedule: basis points (u32), or WAD.** Basis points are enough for a discount schedule, make parameters human-auditable, and keep breakpoint tables small. Convert to a RAY multiplier in one `mul_div`.
- **Amounts: native token base units (u128).**
- **Time: milliseconds (u64)** from `CLOCK_01`.

### Rounding direction (always favour the protocol and the auction)

| Quantity | Direction |
|---|---|
| `collateral_out = coin_in · RAY / discounted_price` | **down** |
| Re-strike when the lot is binding: `coin_charged = collateral_out · discounted_price / RAY` | **up**, capped at `coin_in` |
| `amount_to_raise = debt · penalty` | **up** |
| `collateral_to_seize` on partial liquidation (`ink·dart/art`) | Maker rounds down and favours the user. Use **up** with `min(…, ink)`, or simply favour full liquidation. |
| Keeper reward | **down** |
| Shortfall (bad debt) | computed exactly from integer balances, no rounding |
| Surplus and debt auction lots | lot paid out **down**, bid required **up** |

### Discount schedule: linear vs exponential vs stairstep vs table

Measured on RAI's parameters (0.98 → 0.90 multiplier, rate 0.99996912/s):

| t (s) | exponential | linear over 2758 s | difference |
|---|---|---|---|
| 600 | 0.96201 | 0.96260 | −0.06 pp |
| 1380 | 0.93911 | 0.93997 | −0.09 pp |
| 2000 | 0.92130 | 0.92199 | −0.07 pp |
| 2700 | 0.90160 | 0.90168 | −0.01 pp |

So compounding buys nothing at production ranges.

Options:
- **(a) Linear**, `d(t) = d0 + (dmax − d0)·min(t, T)/T`. One `mul_div`. Simple, but only one shape.
- **(b) Exponential via `rpow`.** Feasible: with a rate below 1, each squaring is < 1e54 in U256 and the result is < RAY in u128. Costs O(log t) 256-bit multiplications (about 20–40 for t in ms up to hours). Fine, but it is extra cycles and extra audit surface for no economic benefit.
- **(c) Stairstep** (Maker's `cut^(t/step)`). Discrete steps reduce the value of being first within a step and make previews exact for the step's lifetime. A table can express it.
- **(d) Recommended: a piecewise-linear table `[(t_i_ms, discount_bps_i)]` with up to 8 points**, validated at configuration time:
  - `t_0 = 0`, strictly increasing `t_i`;
  - non-decreasing discount;
  - `discount_max ≤ HARD_MAX_DISCOUNT_BPS` (for example 3000);
  - the last point is the plateau.

  Evaluation is a scan of at most 8 entries plus one `mul_div` interpolation. It expresses linear (2 points), hold-then-ramp, stairstep (pairs of equal-discount points with an instantaneous jump), or a tabulated exponential. This satisfies F2 "configurable schedule" without a loop whose length depends on auction age. The schedule is **snapshotted into the auction account at start**, so governance changes never alter a live auction, as in GEB.

---

## 5. Mapping RFP-014 requirements to engine design

### 5.1 Minimum state machine (per auction PDA, monotone `status: u8`)

```
                 bid (partial)
                  ┌──────┐
                  ▼      │
[none] ──start──► Auctioning ──(raised ≥ target) or (lot == 0)──► Settled   (terminal)
   (only via liquidate)  │
                         └──(now ≥ start + ttl) && poke_expire──► Expired   (terminal)
                                                                     │
                                       restart(expired_id) ──────────┘ creates NEW auction id
                                                                       with remaining lot/target,
                                                                       fresh price & schedule
```

- "Liquidated" is not a stored auction state. It is the atomic `liquidate` instruction, which seizes from the host and creates the auction PDA directly in `Auctioning` within one transaction (P1).
- Transitions are only ever `Auctioning → {Settled, Expired}` (F6, no reversals). Every handler first asserts the expected status. Terminal accounts are immutable except for an optional `close` that reclaims rent once all balances are zero.
- The `Expired → new auction` restart preserves both "no reversal" and Maker's `redo` liveness. The old account records `successor_id`, the new one records `predecessor_id`, and the shortfall is not booked until the final link terminates. Alternatively, cap restarts at `max_restarts` and then book the shortfall.

### 5.2 Account model

| Account | Seeds (PDA under the single engine program) | Contents | Written by |
|---|---|---|---|
| `EngineConfig` | `("CONFIG", instance_salt)` | Host program id, stablecoin token def, admin (RFP-001), freeze-authority link (RFP-002), protocol-token def (Option), global `hole`, global `on_auction`, buffer target, auction parameters for surplus and debt | admin; `liquidate`, `settle` (global counters) |
| `CollateralConfig` | `("COLL", config, collateral_def)` | Oracle account ids (primary, fallback), max age, max confidence, max deviation, penalty, `chip`, `tip`, `chost`, per-type `hole`, `on_auction`, discount schedule, ttl, min bid, `paused` | admin; `liquidate`, `settle` |
| `Auction` | `("AUCTION", config, position_id, position_liq_nonce)` | status, collateral_def, owner/position, keeper, `collateral_seized`, `collateral_sold`, `collateral_left`, `debt_principal`, `amount_to_raise`, `raised`, `start_ms`, `ttl_ms`, schedule snapshot, `last_seen_ms`, predecessor/successor | `liquidate`, `bid`, `settle`/`expire` |
| `AuctionCollateralVault` | token holding owned by an `Auction`-derived PDA | Seized collateral (per-auction isolation, R3) | token program via chained calls |
| `AuctionCoinVault` (optional) | per auction | Raised coins until settle. **Bids then never write global state.** | bid, settle |
| `Accounting` | `("ACCT", config)` | `bad_debt`, `buffer_balance` (or the token holding), `total_shortfall_booked`, `total_surplus_booked`, `on_debt_auction`, `last_surplus_auction_ms`, `frozen_by_engine` | settle, expire, `check_solvency`, surplus and debt auctions |
| `SurplusAuction` / `DebtAuction` | `("SURP"/"DEBT", config, nonce)` | Dutch-style state (§5.5) | their instructions |

Notes:
- **Seeding by `(position_id, position_liq_nonce)`** avoids a global auction counter, which would be a write hotspot. Each position can only be in one live auction at a time: add a host-side `in_liquidation` flag or nonce, which replaces GEB's `mutex`.
- **Instances.** On LEZ the program id is the guest ImageID, so every CDP product shares the program and differs by `EngineConfig` (F7). All PDAs are seeded by `config`, which prevents cross-instance mixing.
- **Account checks in every instruction:** owner == engine program, PDA re-derivation, the expected token definition, the `CLOCK_01` id and the oracle ids from `CollateralConfig`.

### 5.3 Instruction flow per requirement

| Req | Design decision |
|---|---|
| **F1** permissionless trigger with incentive | `liquidate(position)`, callable by anyone. It: checks the oracle (age, confidence, deviation) for that collateral; computes health against the **TWAP or anchored** price; computes `debt_to_cover` under `min(global room, type room)` with Maker's dust rule; makes a chained call `host.seize_collateral_and_debt(position, dink, dart)` (host-side, gated to the engine PDA as caller; **missing today in `lez-programs/stablecoin`**); creates the Auction PDA and vault; and pays the keeper reward (§6). Emits `Liquidated`. |
| **F2** increasing discount, configurable, early termination | Piecewise-linear schedule snapshotted per auction. A `bid` that reaches `raised ≥ amount_to_raise` closes in the same instruction. |
| **F3** partial lots | `bid(auction, coin_in_max, min_collateral_out)`. Partial fills, Maker's `chost` rule for the remainder, re-strike when the lot binds, `min_bid`. |
| **F4** shortfall → debt auction, freeze fallback | At a terminal state: `shortfall = principal − min(raised, principal)`. Add it to `Accounting.bad_debt` and immediately net it against `buffer_balance`. If `bad_debt > 0` after netting **and** `protocol_token == None` (or the debt auction is disabled), `check_solvency` (permissionless, also chained from settle) issues the RFP-002 freeze on the host (the host's `ProtocolParameters.freeze_authority_account_id` must be the engine's freeze PDA, or the RFP-002 lib must allow it as a delegate). If a token is configured and `bad_debt ≥ debt_lot`, `start_debt_auction`. |
| **F5** surplus auction | `start_surplus_auction` allowed iff `protocol_token.is_some()`, `bad_debt == 0`, `buffer_balance ≥ buffer_target + surplus_lot`, and `now ≥ last + surplus_delay`. Otherwise surplus simply accrues. |
| **F6** strict transitions, atomic | Status byte, checks before effects, chained token transfers last. Each instruction is one LEZ transaction, so it is atomic. No transition out of a terminal state. |
| **F7** host interface | Engine to host: `seize_collateral_and_debt`, `get_position(debt projected to now, collateral)`, `get_redemption_price`, `freeze`/`unfreeze`, `burn` or `credit` coins (repay system debt). Host to engine: nothing mandatory; liquidations are pulled by keepers. Keep `redeem_against_position` reserved, as the appendix suggests. |
| **U4/U5** tooling | View functions (no state change), runnable off-chain against the same Rust crate: `quote_liquidation(position) → (debt_to_cover, collateral, keeper_reward)` and `quote_bid(auction, coin_in, now) → (collateral_out, coin_charged, current_discount, time_left)`. The GUI and CLI share the crate, so the preview equals the on-chain result. |
| **U6/Privacy** | Every user-facing instruction takes the bidder or keeper as a plain signer plus payout accounts, with no stored per-user state that later needs the same account (no pull-refunds, no claim later). **This is why debt and surplus auctions should settle instantly (§5.5).** |
| **R1** non-reentrant | No external callbacks. State is written before chained calls. Status checks reject re-entry; optionally add an `in_progress` flag cleared at the end. The host seize call is invoked by the engine; the host must not call back into the engine. |
| **R2** TWAP or confidence price, stale ⇒ pause per type | Liquidation uses `anchor = TWAP` (RFP-019) or the RFP-020 price, with `age ≤ max_age_ms` and `conf/price ≤ max_conf_bps`. Bids use `min(spot, anchor)` floored at `anchor × (1 − max_dev)`. Any failure means `liquidate` and `bid` for **that** collateral revert with `OracleStale(collateral)`. |
| **R3** isolation | One Auction PDA and vault per auction. Bids touch only that auction's accounts, the bidder's holdings, the clock and the oracle. Global counters are touched only at `liquidate` and terminal settle. |
| **R4** per-type pause | `CollateralConfig.paused` plus oracle gating. Other types and non-liquidation host operations are unaffected. |
| **R5/R6** invariants | §5.4 |
| **P1–P3** performance | O(1) per instruction: no loops over auctions or positions, a schedule scan of at most 8, roughly 6–10 `mul_div` calls. No on-chain list of active auctions; use LP-0012 events (`Liquidated`, `Bid`, `Settled`, `Expired`, `ShortfallBooked`, `Frozen`) plus an indexer. |

### 5.4 Invariants to test (unit, property-based and on-chain assertions)

Per auction:
1. `collateral_seized == collateral_sold + collateral_left + collateral_returned_to_owner + keeper_collateral_reward` (if the reward is in collateral). Track `collateral_sold` **independently** of `collateral_left`; asserting `left + (seized − left) == seized` is a tautology.
2. The vault token balance equals `collateral_left` while `Auctioning`, and is 0 once terminal.
3. `raised ≤ amount_to_raise + 1 unit` (no overcharge), and each fill satisfies `coin_charged ≤ coin_in_max` and `collateral_out ≥ min_collateral_out`.
4. The discount is monotone non-decreasing in time and bounded by `[d0, d_max ≤ HARD_MAX]`. The price used is never above the anchor.
5. The status is monotone, and terminal is absorbing.
6. After every bid, `amount_to_raise − raised` is `0` or `≥ chost` (no dust).

System-wide:
7. `Σ live auctions (amount_to_raise − raised) == type.on_auction` and `Σ type.on_auction == global.on_auction ≤ global.hole`.
8. Shortfall accounting: for each terminal auction, `shortfall == max(0, principal − raised)` and `surplus_contribution == max(0, raised − principal)`. Then `Accounting.total_shortfall_booked == Σ shortfall`.
9. Buffer conservation: `buffer_balance == Σ surplus_contribution + fees_in − bad_debt_netted − surplus_auctioned`. `bad_debt` and `buffer_balance` are never both > 0 after `net()`.
10. Host and engine conservation: `host_total_debt_before == host_total_debt_after + Σ principal_seized` and `host_collateral_before == host_collateral_after + Σ collateral_seized`.
11. Freeze: `bad_debt > 0 && protocol_token.is_none()` implies the host is frozen after `check_solvency`.
12. Surplus and debt auctions: protocol tokens burned == Σ winning payments; protocol tokens minted ≤ `max_mint_per_auction`; and `on_debt_auction ≤ bad_debt`.
13. Liveness (fuzz): for every reachable state with an unsafe position, a fresh oracle and room > 0, `liquidate` succeeds.
14. Split-bid neutrality: `n` bids summing to `X` never yield more collateral than one bid of `X` at the same timestamp.

### 5.5 Surplus and debt auction mechanism (recommended)

GEB's English houses need bidder escrow and a refund to the outbid bidder later. With deshield → interact → reshield and single-use ephemeral accounts, a refund arriving after reshield either strands the funds or forces reuse of the ephemeral account. Recommended alternative, with the same economic role:

- **Surplus auction (instant Dutch).** Lot = `surplus_lot` stablecoin. The protocol-token price required starts at `oracle_or_reference × (1 + premium)` and decays per a schedule (same table code) to a floor. The first taker pays the protocol token, which is **burned in the same transaction**, and receives the stablecoin. Partial takes are allowed with the same dust rules. Expiry restarts with a new lot. *If no protocol-token price exists*, fall back to the GEB English variant with bidder-specified refund recipients, and document the privacy caveat.
- **Debt auction (instant reverse Dutch).** Fixed `debt_lot` stablecoin wanted. Protocol tokens offered per lot start low and grow per the schedule (GEB `amountSoldIncrease` ×1.2 per restart is the discrete analogue), capped at `max_mint`. The first taker pays the stablecoin, which repays bad debt (burn) immediately, and receives minted mock or protocol tokens.

Both are parameterised by `protocol_token: Option<TokenDefinitionId>` and tested against a mock token (F4, F5).

### 5.6 Parameter list with defaults

| Parameter | Scope | Default | Hard bound in code |
|---|---|---|---|
| `liquidation_penalty_bps` | type | 1000 (10%, RAI 1.10; Maker 1.13) | 0–2500 |
| `discount_schedule` | type | `[(0, 200), (2_700_000ms, 1000)]`, i.e. RAI's 2%→10% over 45 min, linear | ≤ 8 points; max ≤ 3000 bps; monotone |
| `auction_ttl_ms` | type | 2 h (Maker `tail` 7200 s) | 10 min – 7 d |
| `max_restarts` | type | 3, then book shortfall | ≤ 10 |
| `min_bid` | type | about 25 coin units (RAI) or the equivalent of ~$5 on LEZ (no gas war); tune | > 0 |
| `chost` (min remaining raise) | type | `debt_floor × penalty` | ≥ `min_bid` |
| `keeper_tip` | type | small flat amount (Maker 250 DAI scaled to the LEZ fee level) | `tip + chip·tab < (penalty−1)·tab` |
| `keeper_chip_bps` | type | 10 (0.1%, Maker) up to 50 | ≤ penalty − 1 |
| `hole` | type | ~1/3 of the type's debt ceiling (Maker 40M/150M) | — |
| `global_hole` | engine | Σ type holes or less (RAI 500k) | — |
| `oracle_max_age_ms` | type | 1 h for TWAP; feed heartbeat ×2 for a push feed | ≤ 24 h |
| `oracle_max_conf_bps` | type | 100 | — |
| `spot_vs_anchor_max_dev_bps` | type | 2000 (RAI 0.80) | ≤ 5000 |
| `surplus_buffer_target` | engine | about 2–5% of debt (RAI 500k on ~$30–60M) | — |
| `surplus_lot`, `surplus_delay_ms` | engine | buffer/20; 2 h | — |
| `debt_lot`, `debt_initial_mint`, `debt_mint_growth`, `max_mint` | engine | ≈ surplus_lot; set from the mock token price; 1.2× per restart; hard cap | — |
| `protocol_token` | engine | None (surplus accrues; bad debt beyond buffer ⇒ freeze) | — |
| `freeze_on_bad_debt` | engine | true | — |

---

## 6. Liquidator-bot and incentive design

- **Two keeper roles:** a *trigger* keeper, who calls `liquidate`, and a *bidder*, who calls `bid`. GEB rewarded only bidders, which centralised liquidations on RAI (Reflexer ran keepers itself). RFP F1 explicitly wants the trigger rewarded.
- **Recommended incentive:** `reward = min(tip + chip·tab, reward_cap)`, paid only if `tab ≥ chost`. Pay it **in seized collateral at the trigger**, valued at the anchored oracle price and rounded down. That makes the payout self-funding and avoids Maker's `vat.suck`, which creates unbacked debt. The invariant then includes `keeper_collateral_reward`.
  - Constraint: `reward_value ≤ (penalty − 1)·principal`. The reward is carved out of the penalty, never out of principal coverage, so self-liquidation is unprofitable and the system stays whole.
  - Alternative: a stablecoin tip from the surplus buffer, only when `buffer_balance > buffer_target`. This is cleaner for the auction lot but unavailable at launch when the buffer is empty.
- **Bidder economics:** profit = `discount × value − DEX slippage − fees`. Because flash loans are out of scope, bidders need inventory. Low `min_bid` and partial fills let small keepers participate. The discount reaching ~10% within 45 minutes guarantees a clear even in thin markets.
- **Bot SDK (soft requirement):**
  1. Subscribe to host position events and oracle updates, and recompute health locally with the same crate.
  2. Call `quote_liquidation` and submit when `reward > tx_cost`.
  3. Watch `Liquidated` and `Bid` events and keep an off-chain set of live auctions.
  4. Every block, `quote_bid` against external price and liquidity; bid when `collateral_out × ext_price × (1 − slippage) > coin_in`.
  5. Always pass `min_collateral_out` and `max_coin_in`.
  6. Call `expire` and `restart` on stale auctions, and `check_solvency` when bad debt appears.
  7. Private mode: a fresh deshielded account per action, then reshield the outputs. No later claims are needed because every action settles instantly.

---

## 7. Design brief (summary)

**State machine.** `liquidate` atomically creates `Auctioning`, which goes to `Settled` (target raised or lot empty) or `Expired` (ttl). `restart` spawns a new auction from an expired one. Terminal states are absorbing, and shortfall and surplus are booked at the terminal state.

**Accounts.**
- `EngineConfig` per CDP instance
- `CollateralConfig` per collateral
- one `Auction` PDA plus its collateral vault (and optionally a coin vault) per auction, seeded by `(config, position, liq_nonce)`
- one `Accounting` PDA
- one PDA per surplus or debt auction
- no on-chain lists

**Math.**
- u128 storage; RAY prices; basis-point discounts; millisecond time from `CLOCK_01`
- every product goes through `mul_div` with a 256-bit intermediate and explicit rounding
- piecewise-linear discount table snapshotted per auction; no `rpow` needed
- no `rad` unit

**Top attacks and mitigations, ranked:**
1. **Oracle stale or manipulated, leading to under-priced sales or bad liquidations.** Anchor on TWAP, bound spot by deviation, gate on age and confidence, pause per type, cap the maximum discount.
2. **Rounding and split-bid extraction.** Floor what is paid out, ceil what is owed, re-strike when the lot binds, set a minimum bid, enforce the `chost` remainder rule, and fuzz split-bid neutrality.
3. **Liquidation DoS** (dust revert, cap exhaustion, callbacks). Use Maker's full-liquidation dust rule, no callbacks, caps that shrink rather than revert, and a liveness fuzz.
4. **Zero-bid or near-zero clearing (Black Thursday).** Oracle-anchored price with a hard maximum discount, expiry plus restart at a fresh price, and caps.
5. **Unaccounted shortfall** (the lot runs out and the target is reported as raised). Track sold and left independently, book shortfall at terminal states, and enforce the conservation invariants.
6. **Caller-controlled time.** Use `CLOCK_01` only, with a monotonic per-auction guard.
7. **Incentive farming or self-liquidation.** Reward below the penalty, reward only above `chost`, and a host debt floor.
8. **Front-running between quote and fill.** Slippage parameters; fills are independent.
9. **Bad debt beyond the buffer with no backstop.** Permissionless `check_solvency` triggers the RFP-002 freeze, and debt auctions use a hard mint cap.
10. **Privileged drain.** No admin path that moves auction collateral; admin can only pause new liquidations and tune parameters within code-level bounds.
11. **Cross-instance and account confusion.** Seed every PDA by config, verify owner and definition ids, and enforce that one live auction per position is guarded on the host.

**Integration gaps to raise with the RFP and host owners:**
- (i) The host stablecoin has no seize, liquidation or `in_liquidation` hooks.
- (ii) The host's single `freeze_authority_account_id` must be the engine PDA, or a delegate via RFP-002.
- (iii) The host's redemption price is *collateral per stablecoin*, so the engine pricing formula must use that convention: `collateral_out = coin_in × redemption_or_market_price / discount_multiplier`.
- (iv) Confirm whether the spel-framework version exposes a clock in the program context. The host comments say it does not, so read the `CLOCK_01` account.

---

## Sources

- RFP-014: https://github.com/logos-co/rfp/blob/master/RFPs/RFP-014-liquidation-auction-engine.md ; appendix: https://github.com/logos-co/rfp/blob/master/appendix/appendix-liquidation-auction-ecosystem.md
- GEB source: https://github.com/reflexer-labs/geb (`src/single/{CollateralAuctionHouse,LiquidationEngine,AccountingEngine,DebtAuctionHouse,SurplusAuctionHouse,SAFEEngine,OracleRelayer}.sol`)
- GEB docs: https://github.com/reflexer-labs/geb-docs (auction module, keeper docs); mainnet addresses: https://github.com/reflexer-labs/geb-changelog (releases/mainnet/2.0.0); governance overlay: https://github.com/reflexer-labs/geb-gov-minimization-overlay
- Maker dss: https://github.com/makerdao/dss (`dog.sol`, `clip.sol`, `abaci.sol`); MIP45: https://forum.makerdao.com/t/mip45-liquidations-2-0-liq-2-0-liquidation-system-redesign/6352
- Mainnet parameters: read via public Ethereum RPC `eth_call` on 2026-10-01 (RAI ETH-A IDCAH `0x7fFdF1Dfef2bfeE32054C8E922959fB235679aDE`, Maker Dog `0x135954d155898D42C90D2a57824C690e0c7BEf1B`, ETH-A Clipper `0xc67963a226eddd77b91ad8c421630a1b0adff270`)
- Black Thursday: https://insights.glassnode.com/what-really-happened-to-makerdao ; https://cointelegraph.com/news/maker-debt-crisis-post-mortem-recommends-new-safeguards
- GEB 2023 liquidation-DoS zero-day (HAI/Wonderland; root-cause details not retrievable): https://defi-wonderland.notion.site/SOLVED-a-zero-day-exploit-in-GEB-codebase-9aec5f799ca24688b524e439da60b987
- LEZ clock: https://github.com/logos-blockchain/logos-execution-zone (`lez/programs/clock/core/src/lib.rs`); host stablecoin: https://github.com/logos-blockchain/lez-programs (`programs/stablecoin/core/src/{math,protocol_parameters,lib}.rs`, `src/freeze.rs`)
- Prior art on u128 overflow (read for findings only, no code reused): https://github.com/edenbd1/antumbra-auction (README)
