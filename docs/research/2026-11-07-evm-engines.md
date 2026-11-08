> Research note, 2026-11-07. Sources are cited inline. Claims tagged *secondary* or *not found*
> come from search summaries or memory and must be re-checked before they are quoted in a proposal.

# Research: production liquidation and auction engines on EVM, and what to adopt

Scope: extends `docs/research/research-3-auction-mechanics.md` (R3). R3 already covers GEB IDCAH maths, Maker Dog/Clipper
basics (tip/chip/chost/Hole/hole), Black Thursday, u128 fixed point and the schedule choice. This note goes wider (more
protocols, newer designs, incidents after 2020, verification practice) and ends with ranked recommendations.

Baseline of our engine, from `crates/auction_core` and `docs/design-log.md`:
- `CollateralParams`: penalty, `tip`, `chip_bps`, `reward_cap`, `debt_floor`, `min_bid`, a piecewise-linear `Schedule`
  and a derived `chost`.
- Type and global caps (`CapState`). An oversized position may go alone (ADR-012). Full seizure; partial liquidation is
  milestone M1.
- Live oracle pricing on every bid, with a band clamp, age and confidence gates, and a per-type pause clock that freezes
  the discount (ADR-005, ADR-007). No restart or expiry: an unsold auction holds the maximum discount (ADR-012).
- Slippage arguments `coin_in_max` and `min_collateral_out`; debt-first payment; the penalty goes to the buffer (ADR-010).
- Surplus and debt auctions: Dutch, instantly settling, with `max_mint_per_auction`, a relist delay (ADR-039) and
  `check_solvency`, which freezes the host above a threshold.
- Caller-claimed time asserted against `CLOCK_01` within `slack_ms` (ADR-058).
- Evidence: proptest invariants, a Python differential model, Kani harnesses.

Effort key: S = under 3 days, M = 1 to 2 weeks, L = more than 2 weeks. Scope: **IN** = within RFP-014;
**OUT** = excluded by the RFP (flash liquidations, aggregators or batching, insurance or socialised loss, multi-collateral
atomic liquidation); **ADJ** = adjacent (the host or the oracle owns it, but the engine should expose a hook).

---

## (a) Per-protocol mechanism summaries (auction and liquidation parameters)

### A1. MakerDAO / Sky: Liquidations 2.0 (Dog + Clipper + Abacus), detail beyond R3
Source: `dss/src/clip.sol` (https://raw.githubusercontent.com/sky-ecosystem/dss/master/src/clip.sol), `dog.sol`,
`abaci.sol`; MIP45 (https://forum.makerdao.com/t/mip45-liquations-2-0-liq-2-0-liquidation-system-redesign/6352).
- **`take(id, amt, max, who, data)`**
  - `max` is a price ceiling (slippage).
  - The buyer takes `min(lot, amt)`, and the charge is capped at `tab`.
  - Dust rule: if a partial take would leave `0 < tab' < chost`, then `require(tab > chost)`, and the buyer must either
    leave at least `chost` or take everything.
- **Order of operations in `take`**
  1. `vat.flux` collateral to `who`.
  2. Optional `ClipperCallee(who).clipperCall(...)`. This is the flash path; it is OUT of scope for us.
  3. `vat.move` DAI from the buyer.
  4. `dog.digs` releases `Dirt`/`dirt`.
  - A global `lock` makes `kick`/`redo`/`take`/`yank`/`file` mutually non-reentrant during the callback.
- **`redo` (reset)**
  - Allowed only when `status().done`, that is `now - tic > tail || price/top < cusp`. `tail` is a time limit; `cusp` is
    a price-drop ratio.
  - Re-reads the feed and sets `top = feed·buf`. `buf` is the starting premium (e.g. 1.2 to 1.3).
  - Pays `tip + chip·tab` only if `tab ≥ chost && lot·feedPrice ≥ chost`, which blocks farming redo rewards on dust.
- **Circuit breaker `stopped`** (driven by `ClipperMom`)
  - Levels: 0 = none; 1 blocks `kick`; 2 blocks `kick` and `redo`; 3 blocks `kick`, `redo` and `take`.
  - `ClipperMom.tripBreaker(clip)` is permissionless. It trips level 2 when the OSM's next price is below
    `tolerance × current`, a "price will drop more than X% next hour" detector. The OSM's one-hour delay makes the next
    price known in advance.
  - `yank` (governance only) cancels an auction and returns the lot to the caller. End/ES uses it.
- **Abacus curves**
  - `LinearDecrease(tau)`.
  - `StairstepExponentialDecrease(step, cut)`, the one used in production for ETH-A: `cut` ≈ 0.99 every `step` = 90 s.
  - `ExponentialDecrease(cut)`.
  - The curve is a separate contract per ilk, so it can be swapped without migrating the Clipper.
- **Incentives**
  - Paid by `vat.suck`, which creates unbacked DAI from the Vow. R3 chose to carve ours from the penalty instead, which
    is better.
  - `Dog.bark` also enforces `room = min(Hole-Dirt, hole-dirt)`. It liquidates **partially** to fit the room, and fully
    if the remainder would be dust.
- **Emergency Shutdown (End)**
  - Steps: `cage` → `cage(ilk)` fixes the final price → `skip` yanks live auctions → `skim` vaults → `thaw` →
    `flow` → `pack` → `cash`. Holders redeem pro rata.
  - Docs: https://docs.makerdao.com/smart-contract-modules/shutdown/end-detailed-documentation

### A2. Sky surplus and debt: Flapper → Splitter + FlapperUniV2 + Kicker; Flopper
Source: https://github.com/sky-ecosystem/dss-flappers (README).
- The English Flapper was replaced by a **Splitter**:
  - `kick` pulls `vow.bump` USDS every `hop` seconds.
  - It sends `burn` (WAD fraction, e.g. 0.70) to the flapper and the rest to a staking farm.
- **FlapperUniV2 / SwapOnly**
  - Buys `gem` (SKY) on a Uniswap v2 pool with an **oracle bound**: it insists on at least `want × pip` (e.g.
    `want` = 0.98, 2% worse than the reference) or reverts.
  - SwapOnly sends the tokens to a receiver. The other variant adds liquidity and sends the LP tokens.
- **Kicker** replaces `Vow.flap`. It has a signed threshold `khump` and lot `kbump`, and may run even when
  `dai(Vow) < sin(Vow)`.
- Lesson: surplus sales moved from auctions to a rate-limited, oracle-bounded market buy. The equivalent on a chain
  without an AMM is our Dutch surplus auction with a premium cap (ADR-017).
- **Flopper** (debt):
  - Reverse English auction: bidders bid DAI and accept fewer MKR (`beg`).
  - `tick` restarts an unbid auction with `lot × pad` (e.g. 1.2), so the protocol offers more tokens over time.
  - Parameters on the Vow: `sump` (DAI per lot), `dump` (initial MKR), `wait` (delay before flop).

### A3. Reflexer GEB / RAI and HAI (Optimism fork)
Sources: https://docs.reflexer.finance/system-contracts/auction-module ;
https://docs.reflexer.finance/system-contracts/auction-module/increasing-discount-collateral-auction-house ;
HAI CAH docs https://hai-amber.vercel.app/detailed/auctions/cah.html ; HAI governance (tally.xyz/gov/hai).
- **LiquidationEngine**
  - `liquidationQuantity` per collateral: the most debt sent to one auction, so large SAFEs are liquidated in
    **chunks**.
  - `onAuctionSystemCoinLimit` / `currentOnAuctionSystemCoins`: a global in-auction cap, like Maker's `Hole`.
  - The `SAFESaviour` hook lets a third-party contract top the SAFE up before liquidation. It was later a liveness and
    attack surface.
- **IncreasingDiscountCollateralAuctionHouse**
  - Parameters: `minDiscount`, `maxDiscount`, `perSecondDiscountUpdateRate`, `minimumBid`,
    `lowerCollateralMedianDeviation`, `upperCollateralMedianDeviation`, `lowerSystemCoinMedianDeviation`,
    `minSystemCoinMedianDeviation`.
  - The deviation bounds clamp the FSM price against the median: the dual-price rule from R3.
  - HAI production example (msETH): start 0%, max 20%, reached over 2 h, `minimumBid` 100 HAI, penalty 20%,
    `liquidationQuantity` 50,000 HAI. (HAI msETH proposal on tally.xyz)
- **AccountingEngine**
  - `surplusBuffer`, `surplusAmount`, `surplusDelay`, `debtAuctionBidSize`, `initialDebtAuctionMintedTokens`,
    `popDebtDelay` (debt queue), `disableCooldown`.
  - `extraSurplusIsTransferred` with `extraSurplusReceiver` sends surplus to a receiver instead of auctioning it.
- **SurplusAuctionHouse variants**
  - `BurningSurplusAuctionHouse` (burn FLX).
  - `RecyclingSurplusAuctionHouse` (send FLX to a receiver such as staking).
  - `MixedStratSurplusAuctionHouse` (split).
  - `PostSettlementSurplusAuctionHouse` (after global settlement).
  - Same idea as Sky's Splitter.
- **DebtAuctionHouse**: `bidDecrease`, `amountSoldIncrease` (Flopper `pad`, applied by `restartAuction`), `bidDuration`,
  `totalAuctionLength`.
- **StabilityFeeTreasury + `IncreasingTreasuryReimbursement`**
  - Permissionless upkeep calls (oracle `updateResult`, rate setter, debt pop) pay a caller reward.
  - The reward starts at `baseUpdateCallerReward` and **grows** by `perSecondCallerRewardIncrease` up to
    `maxUpdateCallerReward` the longer the call is overdue.
  - The treasury is guarded by `treasuryCapacity`, `minimumFundsRequired` and `expensesMultiplier`.
  - This is the canonical gas-aware keeper reward on EVM. (geb-treasury-reimbursement on github.com/reflexer-labs)
- **Incident**
  - A debt-auction over-mint bug was introduced in RAI in Dec 2020, found in a 2023 HAI audit and fixed through
    Immunefi.
  - Write-up: https://mirror.xyz/vnmrtz.eth/WXm4QJFInoB992czPniFbQyAkGUkdoaSd5zEjK5uRIo
  - The class: restart and lot-growth arithmetic in the debt auction lets the minted amount exceed what the bid
    justifies. It supports R3's absolute mint cap; see recommendation 10.

### A4. Liquity v1 and v2 (BOLD)
Sources: https://github.com/liquity/bold/ (README) ; https://docs.liquity.org/v2-documentation/risk-disclosure ;
v1 https://github.com/liquity/dev
- **No auctions.** A Trove below MCR is **offset** against the Stability Pool (SP): SP BOLD is burned and the SP
  receives the collateral at a discount equal to the penalty. Any remainder is **redistributed** to all active Troves.
- **v2 penalties**: `LIQUIDATION_PENALTY_SP ≤ LIQUIDATION_PENALTY_REDISTRIBUTION ≤ 10% ≤ MCR`, set per branch. Collateral
  beyond `1 + penalty` goes to `CollSurplusPool`, where the borrower can claim it (our `claim_leftover`).
- **Gas compensation**
  - A flat 0.0375 WETH is escrowed at open and always paid to the liquidator.
  - Plus `min(0.5% of coll, 2 units)`, only for the SP-offset portion.
  - The flat part is pre-funded by the borrower, so dust can never make liquidation unprofitable. v1 used 200 LUSD
    plus 0.5%.
- **Branch shutdown (v2)**
  - Triggered permissionlessly when TCR < SCR, or **automatically on oracle failure** in `fetchPrice` (revert, zero or
    stale).
  - Freezes: open, adjust, borrow and normal redemptions.
  - Keeps: close, liquidate and SP.
  - Adds **urgent redemptions** with a 2% collateral bonus at the frozen `lastGoodPrice`.
  - There is no admin pause at all.
- v1 Recovery Mode (TCR < 150% widened liquidations) was removed in v2 as a source of griefing and complexity.
- Relevance: the SP is pooled pre-committed liquidity, a backstop like insurance, so it is **OUT**. Redistribution is
  socialised loss, so it is **OUT**. The *shutdown-on-oracle-failure per collateral branch* and the
  *borrower-prefunded flat keeper reward* are **IN**/**ADJ** ideas.

### A5. crvUSD / LlamaLend: LLAMMA soft liquidation
Sources: https://docs.curve.finance/developer/crvusd/llamma-explainer ; https://resources.curve.finance/crvusd/loan-concepts
- Collateral is deposited across N bands (4 to 50) of a two-token AMM. When the price enters the range, arbitrageurs
  progressively convert collateral to crvUSD (soft liquidation) and back on recovery.
- Each conversion costs a little health. Hard liquidation (anyone repays and takes everything) happens only at health
  ≤ 0. Parameters: `A` (band width), `loan_discount`, `liquidation_discount`, and the AMM fee.
- Lessons: continuous de-risking avoids a cliff, but losses are path-dependent and hard to quantify. CRV-collateral
  LlamaLend markets took bad debt in June 2024 when soft liquidation could not keep up with a thin-market crash.
- Relevance: **OUT** as a mechanism (it is an AMM, not an auction). The idea "begin selling *before* insolvency, in small
  pieces" maps to recommendation 2 (chunked lots).

### A6. Aave v3 → v3.3 (+ Umbrella, SVR)
Sources: https://governance.aave.com/t/bgd-aave-v3-3-feat-umbrella/20129 ; https://aave.com/docs/primitives/umbrella
- **Liquidation incentive**
  - `liquidationBonus` per reserve (e.g. 5%). `liquidationProtocolFee` takes a share of the bonus for the treasury.
  - Close factor: 50%, or 100% when HF < 0.95.
  - v3.3 applies the close factor to the whole position, not per reserve.
- **v3.3 dust rules**
  - 100% close factor when the debt or collateral is below `MIN_BASE_MAX_CLOSE_FACTOR_THRESHOLD`.
  - A liquidation reverts if it would leave debt or collateral in `(0, MIN_LEFTOVER_BASE)`, where MIN_LEFTOVER_BASE is
    half the threshold.
  - This is Maker's `chost` rule applied to lending.
- **Deficit tracking (v3.3)**
  - When a liquidation leaves an account with zero collateral and nonzero debt, the remaining debt is burned and
    recorded in `reserve.deficit`.
  - It does not touch the aToken exchange rate.
  - Only the `UMBRELLA` role can call `eliminateReserveDeficit()`.
  - The first month showed about $400 of total deficit against about $9.5B borrowed.
- **Umbrella**: per-asset staking that is slashed automatically when the deficit exceeds a DAO first-loss `offset`.
  Insurance or socialised loss, so **OUT**. The **deficit ledger** itself is **IN**.
- **PriceOracleSentinel (L2)**: after a sequencer outage, a **grace period** blocks liquidations, so borrowers can react
  to prices that moved while the chain was down. **IN**: LEZ has a single sequencer.
- **OEV**: Chainlink SVR. About $887M of liquidations and about $22M recaptured as of mid-2026, with ~65% of proceeds to
  Aave. (https://governance.aave.com/t/arfc-svr-expansion-next-phase-of-multi-network-expansion/25332/2)

### A7. Compound III (Comet)
Sources: https://docs.compound.finance/liquidation/ ; https://rareskills.io/post/compound-finance-liquidation
- **Two-step liquidation**
  - `absorb(absorber, accounts[])`: anyone moves an underwater account's collateral and debt into the protocol. Reserves
    take the debt at once and the borrower is left with nothing. The absorber gets points, not a direct reward.
  - `buyCollateral(asset, minAmount, baseAmount, recipient)`: anyone buys protocol-owned collateral at a fixed discount,
    but **only while reserves < `targetReserves`**.
- **Discount**: `storeFrontPriceFactor × (1 − liquidationFactor)`. Part of the liquidation penalty is passed to buyers;
  the protocol keeps the rest.
- Lesson: separating *trigger* (absorb) from *sale* (buyCollateral) is GEB/Maker's structure. The `targetReserves` gate
  (stop selling collateral once reserves are healthy) is the counterpart of our surplus-buffer target.
- The absorb-into-reserves step is a form of protocol self-insurance, close to the **OUT** line. Our buffer netting is
  the in-scope version.

### A8. Euler v2 (EVK): health-proportional ("reverse Dutch") discount
Source: `euler-vault-kit/src/EVault/modules/Liquidation.sol`
(https://raw.githubusercontent.com/euler-xyz/euler-vault-kit/master/src/EVault/modules/Liquidation.sol) ;
https://docs.euler.finance/Dutch_Liquidation_Analysis.pdf
- **Discount**
  - `discountFactor = collateralAdjustedValue / liabilityValue`, which equals the health score, so the discount is
    `1 − HS`.
  - It is floored at `1 − maxLiquidationDiscount` (v1 default 20%).
  - The deeper the violation, the larger the discount. There is no timer: the price is a function of state.
- `liquidationCoolOffTime`: no liquidation until this long after the violator's last successful status check. It defends
  against self-liquidation set-ups in the same block or transaction.
- `minYieldBalance` slippage, and an explicit `E_SelfLiquidation` revert.
- Debt socialisation happens only if the liability is at least `MIN_SOCIALIZATION_LIABILITY_VALUE` (1e6) and no
  collateral is left. Below that it is skipped, because oracle rounding dominates at that size. Socialisation is
  **OUT**; the "minimum size before writing off" rule is **IN** for our deficit ledger.
- **Euler v1 hack (March 2023, ~$197M)**
  - `donateToReserves` let an attacker make their own account deeply insolvent. They then self-liquidated at the
    maximum discount.
  - Lesson: any user action that can push one's *own* health far below 1 must not be followed by a liquidation that
    pays more than the penalty.
  - Our `reward ≤ penalty` rule and the host's donation-proof vault sweep (ADR-012) are the defence. Keep them under
    test.

### A9. Morpho Blue and pre-liquidations
Sources: https://docs.morpho.org/overview/concepts/liquidation/ ; https://github.com/morpho-org/pre-liquidation ;
https://docs.morpho.org/build/borrow/concepts/preliquidation
- **LIF**: `LIF = min(1.15, 1 / (β·LLTV + (1 − β)))`, with β = 0.3. Riskier (lower-LLTV) markets give a larger bonus. No
  close factor.
- **Bad debt** is realised immediately (socialised to suppliers) when collateral reaches 0. **OUT** for us.
- **Pre-liquidation**: an opt-in per-borrower contract.
  - Between `preLLTV` and `LLTV`, anyone may partially liquidate with `preLCF` and `preLIF`, both linear in LTV.
  - Constraints: `preLLTV < LLTV`; `preLCF1 ≤ preLCF2`; `1 ≤ preLIF1 ≤ preLIF2 ≤ 1/LLTV`.
  - The README warns that splitting into several pre-liquidations can exceed `preLCF` in total.
- **Bot**: `morpho-org/morpho-blue-liquidation-bot`, MIT, v4 adds pre-liquidations.
- Relevance: "softer, smaller, earlier" liquidation as a **host-opt-in** layer. In our engine it is a second, gentler
  schedule for positions between two thresholds (ADJ, M).

### A10. Ajna (oracle-less): kicker bonds and Dutch decay
Sources: Ajna whitepaper §7 (https://www.ajna.finance/pdf/Ajna_Protocol_Whitepaper_03-24-2023.pdf) ;
analysis https://blockanalitica.substack.com/p/ajna-liquidation-analysis ; audits https://github.com/ajna-finance/audits
- **Kick**: the kicker posts a **bond** proportional to the debt. The bond is rewarded if the auction clears below the
  loan's Neutral Price (NP) and forfeited if it clears above, so the kicker bets that the loan really was unhealthy.
- **Price**: starts at `256 × max(HTP, NP)` after the 2023 fix, then halves on a schedule: fast in the first hours,
  slower later. The auction runs at most 72 h. After that, `settle` uses reserves, then writes off against depositors.
- **Sept 2023 incident**: a mechanism-design (not code) flaw in the reference price and grace period was exploitable
  with flash loans. Fixes: reference price `max(HTP, NP)`, no grace period, a take penalty above NP, and a lower start
  multiple. Redeployed Jan 2024.
- Relevance: bonds exist because Ajna has **no oracle**. We have one, so kicker bonds are unnecessary. But "make the
  trigger party lose money if they trigger wrongly" is a pattern to keep in mind if a host ever allows discretionary
  triggers.

### A11. Fluid (Instadapp) vaults
Source: https://docs.fluid.instadapp.io/contracts/core-contracts/protocols/vault/vaultt1
- Positions sit in **ticks** (ratio buckets). A liquidation processes whole ticks and branches in O(1), so many
  positions at once. That is batching, so **OUT**.
- Liquidation penalty up to about 10.23%. It applies between the liquidation threshold and the `liquidationMaxLimit`.
- **`absorb`**: debt above the max limit (no one will liquidate it, because it is beyond 100% LTV or too small) is
  absorbed into the protocol so the tick book stays clean. Analogue: our shortfall booking plus the deficit ledger.

### A12. Reserve Protocol: Dutch with a defensive phase, plus batch fallback
Source: https://github.com/reserve-protocol/protocol/blob/master/docs/system-design.md ; `docs/mev.md`
- **DutchTrade**: the first 40% of the auction is a geometric decay from ~1000× the best plausible price (defensive);
  the last 60% is linear from the best to the worst plausible price. The prices come from `oracleError` and
  `maxTradeSlippage`.
- **Circuit breaker**: if a trade clears **during the geometric phase**, Dutch auctions for that asset are **disabled**
  until governance re-enables them. A fill that early means the oracle price, or the bid asset's price, was wrong or
  manipulated.
- **GnosisTrade**: a batch auction on Gnosis EasyAuction with `minBuyAmount`. Atomic settlement is disabled. It is the
  fallback when Dutch auctions are disabled.

### A13. Batch auctions for system auctions (Gnosis EasyAuction / CoW)
- EasyAuction: a sealed-ish uniform-price batch with min-buy limits, settled once. CoW: batch matching by solvers. Both
  remove intra-auction ordering MEV. They need escrow and a later settlement, which conflicts with our "use once,
  instant settle" privacy constraint (R3 §0.6).
- Relevance: an alternative for **surplus or debt** auctions if privacy allows escrow. Today: **ADJ, L**.

### A14. Olympus and Frax AMOs (surplus management)
- Olympus Range-Bound Stability: treasury market operations inside price "walls". Frax AMOs: algorithmic market
  operations that deploy surplus into strategies.
- Both replace auctions with **rule-bound market operations** that need an on-chain AMM or market. LEZ has none for
  our tokens yet. **OUT** for now; Sky's Splitter (A2) is the realistic analogue.

### A15. OEV recapture and keeper incentive design
- **Chainlink SVR**:
  - A dual-aggregator feed. The price update that enables a liquidation goes through a private auction: searchers bid
    to backrun it, and the proceeds go to the protocol (Aave: ~65%).
  - Docs: https://docs.chain.link/data-feeds/svr-feeds ; analysis https://blog.chain.link/chainlink-svr-analysis
- **UMA Oval**: a wrapper that delays a Chainlink update until an MEV-Share auction for the right to liquidate on it has
  run. Proceeds go to the protocol.
- **API3 OEV Network**: an auction for the right to update the dAPI price. Proceeds go to the dApp.
  (https://docs.api3.org/reference/oev-network)
- **Lesson for us**
  - OEV exists because a **fixed** liquidation bonus is paid to whoever lands first after a price update.
  - A **time-increasing discount auction is itself an OEV-recapture mechanism**: bidders compete the discount down, and
    the protocol keeps the gap between the clearing price and the maximum discount.
  - The only fixed-prize part of our engine is the **trigger reward** (`tip + chip·tab`). Keep it small and capped, so
    the extractable value at trigger time stays close to gas cost.
  - An oracle-level OEV auction is **ADJ**: the oracle and the sequencer would have to provide it.
- **Gas-aware rewards**
  - GEB's increasing caller reward (A3).
  - Liquity's borrower-prefunded flat gas compensation (A4).
  - Maker's `tip` covers gas and `chip` scales with size (R3).

---

## (b) Feature catalogue

| # | Feature | Protocols | Problem it solves | LEZ applicability | Effort |
|---|---|---|---|---|---|
| 1 | Price-drop circuit breaker (next-price vs current, or spot-vs-anchor jump) that stops new liquidations | Maker ClipperMom `tolerance`; Liquity v2 oracle-fail shutdown | Liquidating into a crash, or on a bad print | IN (we have staleness and confidence gates, no jump detector) | S |
| 2 | "Cleared too early" detector disables auctions per asset | Reserve DutchTrade geometric phase | A wrong or manipulated oracle sells at the wrong price | IN: flag when a fill happens at a discount below an `implausible_discount` threshold, or at max discount within X s | S |
| 3 | Graduated stop levels (no new kicks / no resets / no takes) | Maker Clipper `stopped` 1/2/3 | A blunt pause stops healthy liquidations too | IN | S |
| 4 | Partial liquidation in chunks of at most `liquidationQuantity`; `room` fitting under caps | GEB/HAI `liquidationQuantity`; Maker Dog `room`; Aave close factor | Market impact, cap exhaustion by one whale (ADR-012 residual) | IN (already milestone M1) | M |
| 5 | Dust and leftover rules (no residual in (0, min)) | Maker `chost`; Aave v3.3 `MIN_LEFTOVER_BASE`; Liquity MIN_DEBT | Positions too small to liquidate profitably accumulate as bad debt | IN (have `chost` and `min_bid`; add a property test that no leftover is in (0, min)) | S |
| 6 | Explicit deficit ledger with minimum write-off size | Aave v3.3 `deficit`; Euler `MIN_SOCIALIZATION_LIABILITY_VALUE`; Fluid `absorb` | Hidden or unrecognised bad debt; dust write-offs | IN (shortfall booked; expose a per-type deficit and events) | S |
| 7 | Timer-based reset with a reward (`redo`, `tail`, `cusp`) | Maker; GEB `restartAuction` (debt) | A start price fixed at kick goes stale | IN for debt and surplus only: our collateral auctions price live (ADR-012 is correct); document why | S (doc) |
| 8 | Lot growth on unbid debt auctions, with a ceiling | Maker Flopper `pad`; GEB `amountSoldIncrease` | No bidder at the initial mint ratio | IN (have `debt_growth` schedule + `max_mint_per_auction`; add a rolling-epoch cap) | S |
| 9 | Oracle-bounded surplus sale (`want × pip`) + split (burn vs recipient) | Sky Splitter/FlapperUniV2; GEB MixedStrat/Recycling | Selling surplus into a manipulated price; governance needs a flexible use of surplus | IN (premium cap exists; the split is new) | S |
| 10 | Rate-limited system auctions (`hop`, relist delay) | Sky Splitter `hop`; GEB `surplusDelay`; ours ADR-039 | Draining the buffer or flooding token supply | IN (have) | done |
| 11 | Gas-aware, time-increasing caller reward for upkeep | GEB IncreasingTreasuryReimbursement | Nobody calls `observe_feed`, `settle_empty`, `check_solvency` or system kicks when it is not profitable | IN (paid from buffer above target) | M |
| 12 | Borrower-prefunded flat liquidation reward | Liquity gas compensation | Dust positions never worth liquidating | ADJ (the host escrows at open; the engine pays out) | M |
| 13 | Sequencer and clock-gap grace period | Aave PriceOracleSentinel | Liquidating users who could not react while the chain was down | IN (detect a `CLOCK_01` or feed gap > G; no new liquidations for `grace_ms`) | S |
| 14 | Self-liquidation guard and cool-off | Euler v2 `E_SelfLiquidation`, `liquidationCoolOffTime`; Euler v1 hack | Profitable self-liquidation after a self-inflicted insolvency | IN (reward < penalty already; add an invariant test and optionally a same-block cool-off from the host's last update) | S |
| 15 | Health-proportional start discount | Euler reverse Dutch; Morpho LIF by LLTV | A fixed start discount is too low for deeply underwater positions | IN (start at `max(schedule(0), k·(1 − HS))`, capped) | M |
| 16 | Pre-liquidation zone (soft, opt-in, small bonus) | Morpho pre-liquidation; crvUSD LLAMMA (OUT) | Cliff liquidations; user experience | ADJ (host opt-in; the engine supplies a second schedule) | M |
| 17 | Per-collateral wind-down or shutdown on oracle death or TCR < SCR | Liquity v2 branch shutdown | A dead oracle leaves the system unpriced indefinitely | IN/ADJ (freeze new debt for one type via the host hook; keep liquidations live at a frozen `lastGoodPrice` with a wider discount) | M |
| 18 | Global settlement (End / ESM) | Maker End; GEB GlobalSettlement | Orderly unwind if the system is broken | ADJ (mostly the host; engine hooks: stop new auctions, settle live ones) | L |
| 19 | Flash `take` / callee | Maker ClipperCallee; Reserve `bidWithCallback` | Capital-free bidding | OUT (RFP) | n/a |
| 20 | Stability pool / Umbrella / reserves absorb | Liquity SP; Aave Umbrella; Compound absorb | Instant liquidity and loss-absorption | OUT (insurance or socialised loss) | n/a |
| 21 | Batch or tick liquidation | Fluid ticks; Liquity `batchLiquidateTroves` | Gas per position | OUT (batching) | n/a |
| 22 | Batch or uniform-price auctions for surplus and debt | Gnosis EasyAuction; Reserve GnosisTrade; CoW | Ordering MEV in system auctions | ADJ (needs escrow, which conflicts with ephemeral-account privacy) | L |
| 23 | OEV-recapture of the trigger | Chainlink SVR; UMA Oval; API3 OEV | Fixed bonus captured by searchers | ADJ (oracle or sequencer level); IN: keep the trigger reward small and capped | S (doc) |
| 24 | Kicker bonds | Ajna | Discretionary triggers without an oracle | OUT (not needed with an oracle) | n/a |
| 25 | Formal specs (Certora/K) + stateful fuzz (Echidna/Foundry invariants) | Maker k-dss; Aave and Morpho Certora; Liquity Echidna | Proof the conservation and liveness properties hold | IN (we have Kani + proptest; extend) | M |
| 26 | Reference keeper bot and auction UI | Maker auction-keeper (archived 2023); Morpho liquidation bot; Comet example liquidator | Keeper centralisation | IN (soft requirement; we have SDK + CLI) | M |
| 27 | Agent-based stress simulation of parameters | Gauntlet / Chaos Labs reports for Aave and Compound; Maker risk models | Parameters picked by intuition | IN (off-chain; visible, testable evidence) | M |

---

## (c) Incidents → safeguards

| Incident | What went wrong | Adopted mitigation | Concrete safeguard for our engine | Status |
|---|---|---|---|---|
| Maker Black Thursday, Mar 2020 (6.65M DAI deficit) | English auctions accepted zero bids; congestion; few keepers; OSM lag | Liq 2.0 Dutch with `buf`, `cusp`, `tail` and caps; `chost`; `tip` and `chip`; ClipperMom | Oracle-anchored price, max-discount cap, caps, and a trigger reward. **Add** a jump breaker (#1) and a chain-gap grace period (#13) | mostly done |
| Compound DAI, Nov 2020 (~$89M liquidated) | A single-venue (Coinbase) print moved the oracle | Anchored view: the reporter price must sit within ±20% of the Uniswap TWAP | Band clamp and confidence gate (ADR-005). **Add** a max per-observation jump and a "triggered at band edge" flag | partly |
| Venus/Blizz, LUNA May 2022 | The Chainlink feed was clamped at `minAnswer`, so the protocol saw a price above market | Protocols check `answer` against `minAnswer`/`maxAnswer` | **Add**: treat a feed value at its configured floor or ceiling as invalid (pause) | new |
| Inverse Finance, Apr 2022 (~$14.5M) | A Keep3r TWAP on a thin Sushi pool was moved within a few blocks | Chainlink feeds; later FiRM's "pessimistic price" (lowest of the last 2 days for collateral) | Trigger on TWAP (have). **Consider** a pessimistic min(TWAP, spot) for the trigger only, never for the bid price | partly |
| Mango, Oct 2022 (~$114M) | A thin-market token pumped; the oracle reported faithfully | Position and open-interest limits; illiquid collateral delisted | Per-type caps (have). **Add** `debt_ceiling_hint` docs; the host must cap debt by collateral liquidity. The liquidation quantity (#4) limits sale size | ADJ |
| Cream, Oct 2021 (~$130M) | A vault share price (yUSD) inflated by donation | Do not price collateral by a donation-manipulable share price | Oracle onboarding checklist: no share-price feeds without a rate cap. The host vault sweep handles donations (ADR-012) | doc |
| Aave CRV, Nov 2022 (~$1.6M bad debt) | Large short plus thin liquidity; liquidations could not clear | v3 supply and borrow caps; v2 markets frozen | Caps plus liquidation quantity plus a breaker; the deficit ledger makes losses visible | partly |
| Euler v1, Mar 2023 (~$197M) | `donateToReserves` made the attacker's account deeply insolvent; self-liquidation at max discount | v2: health-check every action, `E_SelfLiquidation`, cool-off | Invariant: **total paid to the liquidator and bidders ≤ debt × (1 + penalty)** for any self-inflicted path. Test self-liquidation from the owner key | add test |
| Ajna, Sept 2023 | Reference price and grace-period design were exploitable with flash loans | Reference = `max(HTP, NP)`, no grace, take penalty above NP | Our grace (#13) must block **liquidation triggers only**, never bids on live auctions. A test that bids still work during the grace period | design note |
| RAI debt-auction over-mint (2020–2023) | Restart and lot-growth arithmetic minted more than intended | Patched via Immunefi | Absolute `max_mint_per_auction` (have). **Add** a rolling-epoch mint cap and a Kani proof `minted ≤ cap` across restarts | partly |
| LlamaLend CRV, June 2024 | Soft liquidation outpaced by a crash in a thin market | Parameter changes; market caps | Chunked lots and a max discount reached quickly. A stress test with an exogenous liquidity curve | sim |
| Keeper centralisation (RAI run by its team; Maker 2020) | Few bidders; capital-heavy bidding | Open-source keepers; Dutch auctions with no lock-up; `tip` | SDK + CLI (have). **Ship** a reference keeper loop, low `min_bid`, and docs of break-even maths | partly |
| Dust that never clears (Aave pre-3.3, Maker dust farms) | Gas > bonus for small positions | `chost`/`MIN_LEFTOVER`; 100% close factor below a threshold; flat gas comp | `chost`, `min_bid`, full seizure (have). Property test: no reachable state leaves an auction with `0 < remaining_target < chost` | partly |
| Liquidation DoS via callbacks (GEB SAFESaviour; Maker callee reentrancy class) | Third-party code on the liquidation path | Maker global `lock`; Liquity has no hooks | No callbacks in `liquidate` (have, R3). Keep it as a stated invariant in the threat model | done |
| Auction griefing (tiny bids, cap hogging) | Many small bids, or one oversized auction holding the cap | `min_bid`, `chost`, `room` partial fit | `min_bid` (have). ADR-012 residual, fixed by #4 | M1 |

---

## (d) Top 15 recommended improvements, ranked

Ranking weighs (i) visible, testable evidence, (ii) real safety value, (iii) effort. All are IN scope
unless marked.

1. **Chunked partial liquidation (`liquidation_quantity` + Maker `room` fitting)** — M.
   Ends the ADR-012 "one oversized auction holds the global cap" residual and limits market impact (HAI 50k HAI per
   auction; Maker `room`). It is already milestone M1. Ship it with the dust rule so a chunk never leaves
   `0 < debt < debt_floor`. Tests: cap-fit liveness ("any unsafe position can always be at least partly liquidated when
   room > chost"), and split-liquidation neutrality.
2. **Oracle jump circuit breaker + "implausible fill" detector** — S.
   - Two triggers, both permissionless:
     - (a) Pause liquidations of a type when one observation moves more than `max_jump_bps` from the last good
       observation, or when spot deviates from TWAP by more than X (ClipperMom-style).
     - (b) The Reserve rule: if a fill clears within the first `suspicious_ms` of an auction, or at a discount below
       `min_plausible_discount`, set a per-type flag that blocks new liquidations until the admin clears it.
   - This is cheap, visible and directly answers Black Thursday, Compound DAI and Mango-style prints.
3. **Graduated stop levels** — S.
   - Replace the binary pause with Maker-style levels: 1 = no new liquidations; 2 = no new system auctions; 3 = no bids,
     reserved for a proven exploit.
   - Each level is reversible. **No level can move collateral** (no `yank`), keeping R3's "no privileged drain" rule.
   - Document the transitions as a state machine for integrators and auditors.
4. **Chain-gap and oracle-resume grace period** — S.
   - LEZ has a single sequencer. If consecutive `CLOCK_01` readings, or the feed's resume after a pause, show a gap
     above `max_gap_ms`, block new *liquidation triggers* for `grace_ms` (Aave PriceOracleSentinel).
   - Bids on live auctions continue: this is the Ajna lesson, a grace must never protect a bad auction.
   - It fits the existing pause clock (ADR-007).
5. **Oracle sanity hardening** — S.
   - (a) Treat a reading equal to the feed's configured min or max bound as invalid (LUNA `minAnswer`).
   - (b) Optional pessimistic trigger price `min(TWAP, spot)` for collateral when *triggering only*; bids keep the
     clamped anchor.
   - (c) An oracle-onboarding checklist in the docs: no donation-manipulable share prices (Cream), and a liquidity
     floor (Mango, Inverse).
6. **Permissionless upkeep rewards, gas-aware and time-increasing** — M.
   - A GEB IncreasingTreasuryReimbursement analogue for `observe_feed`, `settle_empty`, `check_solvency`, system
     auction start and `close_*`.
   - `reward = min(base + rate·overdue_ms, max)`, paid only from buffer above `surplus_buffer_target`, with a per-epoch
     treasury cap.
   - This removes the "someone must run cron" liveness assumption that integrators and auditors check.
7. **Deficit ledger and bad-debt transparency** — S.
   - Per-type and global `deficit_booked`, `deficit_covered_by_buffer`, `deficit_covered_by_mint`, plus events (Aave
     v3.3).
   - A `min_writeoff` so dust is not written off below oracle-rounding scale (Euler).
   - A dashboard in the Basecamp System screen. It makes F4 visibly correct.
8. **Invariant specification + formal proofs, mapped to industry rules** — M.
   - Publish `docs/security/invariants.md` that names each invariant in the style of Certora rules (Morpho Blue
     `certora/`, Aave `aave-v3-origin/certora`, Maker `k-dss`), with links to its Kani harness, proptest or
     differential test. Key invariants:
     - conservation;
     - `reward ≤ penalty·debt`;
     - discount monotone and bounded;
     - no zero price;
     - `minted ≤ cap` per auction and per epoch;
     - terminal states absorbing;
     - liquidation liveness.
   - Add a stateful (Echidna-like) sequence fuzz across liquidate, bid, pause, observe and system auctions.
9. **Incident-replay test suite** — M.
   - Golden scenarios as integration tests: Black Thursday (a 50% drop plus a 2 h feed gap), LUNA (a feed stuck at its
     floor), a Compound-DAI print (a single +30% spike), Euler v1 (owner self-liquidation after self-insolvency), a RAI
     debt-auction restart loop, and a dust-farm attempt.
   - Each asserts the safeguard fires; it also turns section (c) into evidence.
10. **Debt-auction mint hardening** — S.
    - Add a rolling-window mint cap (`max_mint_per_epoch`) on top of `max_mint_per_auction`.
    - A Kani proof that repeated restarts and lot growth never exceed it.
    - The initial mint ratio comes from the anchored protocol-token price with a floor, so a token price crash cannot
      cause unbounded dilution. The freeze path takes over instead.
11. **Per-collateral wind-down mode (Liquity v2 branch shutdown)** — M (ADJ).
    - When a type's feed has been invalid for more than `wind_down_after_ms`, or its collateral ratio is below a
      shutdown ratio:
      - freeze new debt for that type only, via the host hook (finer than the global F4 freeze);
      - keep liquidations live at the frozen last good price with a wider max discount.
    - Document an End/GlobalSettlement design for the host as a future milestone (L, ADJ).
12. **Reference keeper bot with profitability model** — M.
    - A runnable `auction-keeper` on the SDK. It covers trigger, bid and upkeep, with break-even
      `bid if collateral_out·ext_price·(1 − slippage) − coin_in > fee`, private-mode account rotation, and a dry-run
      mode.
    - Maker's keeper is archived (2023); Morpho's is current. It is the main defence against keeper centralisation.
13. **Health-proportional starting discount** — M.
    - `start_discount = min(max_discount, max(schedule(0), k·(1 − CR/LR)))` (Euler reverse Dutch, Morpho LIF by risk).
    - Deeply underwater positions (gap risk after a chain halt) clear quickly. Healthy-edge positions start near 0 and
      let competition set the price.
    - It needs a differential-model update.
14. **Surplus strategy split** — S.
    - `surplus_burn_bps`: burn through the auction, with the remainder to a configurable `surplus_receiver` (staking or
      treasury), as Sky's Splitter and GEB's MixedStrat/Recycling do.
    - Keep the auction's oracle-bounded premium (`want × pip` analogue, ADR-017) and the relist delay (`hop`).
    - It shows integrators the engine is reusable across tokenomics.
15. **Trigger-reward OEV analysis + self-liquidation cool-off** — S.
    - A short ADR showing that the time-increasing discount recaptures bidder-side OEV, that the fixed trigger reward is
      the only OEV-exposed prize (capped, at most gas plus margin), and that oracle-level OEV auctions (SVR, Oval, API3)
      are ADJ for LEZ.
    - Optionally reject liquidation in the same block as the host's last position update for that position (Euler
      cool-off), and add the Euler-v1 owner self-liquidation test from item 9.

Explicitly not recommended (OUT per RFP or a poor fit): flash `take`/callee, stability pool or Umbrella-style
insurance, redistribution or debt socialisation, tick or batch liquidation, kicker bonds, LLAMMA soft liquidation.

---

## Sources
- Maker/Sky: https://raw.githubusercontent.com/sky-ecosystem/dss/master/src/clip.sol ;
  https://github.com/sky-ecosystem/dss/blob/master/src/dog.sol ; https://github.com/sky-ecosystem/dss/blob/master/src/abaci.sol ;
  https://github.com/sky-ecosystem/dss-flappers ; https://docs.makerdao.com/keepers/the-auctions-of-the-maker-protocol ;
  https://docs.makerdao.com/smart-contract-modules/shutdown/end-detailed-documentation ;
  https://github.com/makerdao/auction-keeper (archived 2023-04-18) ; https://github.com/makerdao/k-dss ;
  https://forum.makerdao.com/t/black-thursday-response-thread/1433
- Reflexer/HAI: https://docs.reflexer.finance/system-contracts/auction-module ;
  https://docs.reflexer.finance/system-contracts/auction-module/increasing-discount-collateral-auction-house ;
  https://hai-amber.vercel.app/detailed/auctions/cah.html ; https://www.tally.xyz/gov/hai ;
  https://mirror.xyz/vnmrtz.eth/WXm4QJFInoB992czPniFbQyAkGUkdoaSd5zEjK5uRIo ; https://docs.reflexer.finance/risk/geb-risks
- Liquity: https://github.com/liquity/bold/ ; https://docs.liquity.org/v2-documentation/risk-disclosure ;
  https://github.com/liquity/dev ; https://dedaub.com/audits/liquity/liquity-v2-aug-28-2024/
- Curve: https://docs.curve.finance/developer/crvusd/llamma-explainer ; https://resources.curve.finance/crvusd/loan-concepts
- Aave: https://governance.aave.com/t/bgd-aave-v3-3-feat-umbrella/20129 ; https://aave.com/docs/primitives/umbrella ;
  https://governance.aave.com/t/arfc-svr-expansion-next-phase-of-multi-network-expansion/25332/2 ;
  https://github.com/aave-dao/aave-v3-origin (certora/)
- Compound: https://docs.compound.finance/liquidation/ ; https://rareskills.io/post/compound-finance-liquidation ;
  https://www.openzeppelin.com/news/compound-iii-audit ; https://github.com/ajb413/comet-liquidator
- Euler: https://raw.githubusercontent.com/euler-xyz/euler-vault-kit/master/src/EVault/modules/Liquidation.sol ;
  https://docs.euler.finance/Dutch_Liquidation_Analysis.pdf ; https://docs.euler.finance/user-guide/liquidation
- Morpho: https://docs.morpho.org/overview/concepts/liquidation/ ; https://github.com/morpho-org/pre-liquidation ;
  https://github.com/morpho-org/morpho-blue (certora/) ; https://github.com/morpho-org/morpho-blue-liquidation-bot
- Ajna: https://blockanalitica.substack.com/p/ajna-liquidation-analysis ; https://github.com/ajna-finance/audits ;
  https://audits.sherlock.xyz/contests/75/report
- Fluid: https://docs.fluid.instadapp.io/contracts/core-contracts/protocols/vault/vaultt1
- Reserve: https://github.com/reserve-protocol/protocol/blob/master/docs/system-design.md ;
  https://github.com/reserve-protocol/protocol/blob/master/docs/mev.md
- OEV: https://docs.chain.link/data-feeds/svr-feeds ; https://blog.chain.link/chainlink-svr-analysis ;
  https://docs.api3.org/reference/oev-network ; https://community.venus.io/t/vrc-venus-dual-vendor-oev-trial-chainlink-svr-performance-report/5776
- Incidents: https://blockworks.co/news/mango-markets-mangled-by-oracle-manipulation-for-112m ;
  https://halborn.com/blog/post/top-oracle-manipuation-hacks-defi ; https://www.certik.com/blog/2022-year-in-review-lending-protocols ;
  https://insights.glassnode.com/what-really-happened-to-makerdao

Verification caveats:
- Several numeric values come from search snippets or secondary sources, not from contract reads:
  - HAI parameters;
  - the SVR totals;
  - Aave v3.3 threshold values (the BGD post uses them as examples, not as final values);
  - the Ajna start multiple.
- Recheck them on-chain before quoting them in the proposal. The Ajna post-mortem and the RAI write-up returned HTTP 403,
  so their details are taken from search summaries.
- The Compound DAI, Venus/LUNA, Aave CRV, Euler v1 and Inverse "pessimistic price" details are from memory and the
  secondary sources above.
