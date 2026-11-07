> Research note, 2026-11-07. Sources are cited inline. Claims tagged *secondary* or *not found*
> come from search summaries or memory and must be re-checked before they are quoted in a proposal.

# Non-EVM liquidation and auction engines: ideas for the LEZ auction engine

Research date 2026-11-07. Scope: liquidation and auction designs outside Ethereum-mainnet EVM, with weight on
privacy and zk systems, read against our engine (GEB-style IDCAH collateral auctions, instant Dutch F4/F5,
surplus buffer, freeze-on-bad-debt, TWAP/confidence oracle gate with per-type pause; LEZ = RISC Zero zkVM,
Solana-like accounts/PDAs, public + private accounts, deshield -> interact -> reshield with fresh ephemeral
accounts, all auction state public). It does not repeat `docs/research/research-3-auction-mechanics.md`
(GEB, Maker Liq 2.0, Black Thursday, u128 math).

Confidence key: **[doc]** read from official docs/source; **[2nd]** secondary source or search summary only,
verify before citing in the submission; **[gap]** looked for and not found.

What we already have (so the catalogue marks it "have"): oracle age / confidence / band `[R/2, 2R]` gate with
a per-type pause clock that freezes the discount (programs.md), Maker-style caps and `chost` dust rule,
slippage args, keeper reward carved from the penalty, piecewise-linear schedule (can express stairsteps),
config-time validation (ADR-038), debt-auction mint cap, permissionless `check_solvency`.

---

## (a) Per-protocol mechanism summaries

### Solana

**Kamino Lend (klend)** [doc] https://kamino.com/docs/curators/markets/liquidations ,
https://kamino.com/docs/curators/markets/elevation-groups , https://kamino.com/docs/risk/safeguards/auto-deleverage
- Close factor 25 % by default, jumping to 100 % past an insolvency-risk LTV (~95 %).
  `min_full_liquidation_value_threshold` lets small positions be fully liquidated, skipping the close factor
  (the same idea as Maker's dust rule).
- `max_liquidatable_debt_market_value_at_once` caps the value liquidated per call (a throttle).
- **Bonus scales with how deep the breach is**: `bonus = max(min_bonus, LTV - liq_LTV)`, then capped at
  `max_bonus`, then capped at **`100 % - LTV`** (the *solvency cap*: the bonus may never itself create bad
  debt). At LTV >= 99 % it switches to a small fixed `badDebtLiquidationBonusBps` (~1 %) so that someone
  still cleans up the position.
- Reference bonus ranges: stables 1-2 %, majors 3-10 %, long tail 5-25 %. `protocolLiquidationFeePct`
  routes a share of the bonus to the protocol.
- **Elevation groups**: isolated risk tiers, each with its own LTV and a tighter bonus cap for correlated
  assets; `allow_new_loans` puts a group into reduce-only mode.
- `price_triggered_liquidation_disabled` is an incident switch that stops oracle-triggered liquidations.
- **Auto-deleverage** is a margin call with a grace window before liquidation, triggered by a Risk-Council
  multisig and never used to date. The mintlify mirror of klend describes it differently, as bypassing LTV
  checks [2nd]: https://www.mintlify.com/kamino-finance/klend/advanced/autodeleverage
- `socialize-loss` is irreversible and admin-only (out of our scope).

**marginfi (now 0.xyz)** [doc/2nd] https://docs.0.xyz/protocol-overview/risk-engine
- **Confidence-band pricing**: assets are valued at the oracle's lower 95 % band and liabilities at the upper
  band. If the confidence interval exceeds 5 % it is clamped or the transaction aborts.
- Oracle-dependent actions are blocked when the price is stale.
- The liquidator can only bring health back to 0, never above, so liquidations are minimal and partial.
- The 5 % fee is split between the liquidator and a per-bank insurance fund (2.5 / 2.5 in the risk-engine
  example).

**Solend / Save** [2nd] https://www.theblock.co/post/182055/solend-exploited-oracle-attack
- The Nov 2022 USDH exploit cost $1.26 M of bad debt in isolated pools. The oracle read a single thin Saber
  pool.
- The attacker **write-locked the Saber account in the same slot** to stop arbitrage and timed the moment
  the oracle updated.
- Lessons: account-lock griefing is a real attack in a Solana-style account model, and isolated pools
  contained the damage.

**Drift v2 (docs now redirect to "Velocity")** [doc] https://docs.velocity.exchange/liquidations/liquidation-engine ,
https://docs.drift.trade/protocol/insurance-fund , keeper bot: https://www.mintlify.com/drift-labs/keep-rs/bots/liquidator
- Open orders are cancelled first, which may already restore health.
- Partial liquidation targets maintenance margin plus a 2 % buffer, so the account is not re-liquidated
  immediately.
- The fee is split between the liquidator (0.75 %) and the insurance fund (0.75 %); the insurance fund
  absorbs losses before any socialisation.
- **Liquidator-fee aging**: after a 600 s grace period the liquidator fee rises by 0.01 bps per 400 ms, up to
  the lesser of 3x base and the maintenance ratio. Neglected accounts become steadily more attractive to
  liquidate.
- **Throttle ramp**: the first fill may take only an initial percentage, rising to 100 % over a set duration.
  Positions under $50 bypass it.
- **Oracle band**: liquidation is blocked (`PriceBandsBreached`) if the oracle deviates >= 50 % from the
  5-minute TWAP. **Admins can tighten this band but not widen it.**
- Keeper: `liquidate_perp_with_fill` matches against the DLOB's resting makers in the same transaction, and
  the bot waits at least 5 slots between attempts on the same account.
- The insurance fund is per asset pool, with per-market limits; isolated markets cannot draw on the main
  fund. Insurance is out of our scope.

**Jupiter Perps** [2nd] https://docs.jup.ag/user-docs/trade/perps , https://static-r2.jup.ag/lend/risk-research/jlp-risk-report.pdf
- A Jupiter-run keeper executes requests, so liquidation is not permissionless in practice.
- Liquidation fees go to the JLP pool (75 % of fees) and Jupiter (25 %). The pool is the counterparty.
- Lesson by contrast: a single operator keeper centralises liveness. RFP F1's permissionless trigger with a
  reward is the right call.

**Mango v4 (post Oct-2022 $114 M oracle-manipulation exploit)** [doc]
https://blockworksfoundation.gitbook.io/mango-markets/stable-price , changelog:
https://cdn.jsdelivr.net/npm/mango-v4-test-pack@0.21.27/CHANGELOG.md
- **Stable price**: a rate-limited price that follows the oracle. Its growth limits (defaults 290 %/h overall,
  6 %/h versus a 24 h-delayed price) are tightest when the two prices diverge. Assets are valued at
  `min(oracle, stable)` and liabilities at `max`.
- A permissionless keeper updates the stable price at most once every 10 s.
- **Risk-reducing actions are allowed with a stale oracle**: full repay and full withdraw (v0.19.1).
- Hard deposit limits per token cap collateral weight. An oracle price band rejects order-book orders outside
  it.
- A security-council "post-only / reduce-only" mode exists.
- A bug: the stable price initialised to 0 when a token was listed before its oracle had a price. The fix
  initialises it only on the first valid read. This is a config-time validation lesson.

**Solana MEV / keepers** [2nd] https://helius.dev/blog/solana-mev-an-introduction
- Jito sells atomic bundles in an out-of-protocol, tip-based partial-block auction. The Jito mempool is
  deprecated.
- Liquidations are **back-runs**: the bot sees the state change in block N and competes in the next auction.
  Tips are 10-50 k lamports at peak [2nd].
- Lesson: trigger races are won on latency plus tip. On LEZ, private actions add proving latency (see (c)).

**Compute budgeting pattern (general Solana)**
- Per-auction PDAs, no global vectors, bounded loops.
- Write-lock contention on hot accounts serialises transactions: the hot-account problem. It matters for us
  because every `bid` writes `BUFFER` (penalty) and the collateral config (cap release) (security-checklist
  row `bid`).

### Cosmos / app-chains

**Kujira ORCA** [doc/2nd] https://docs.rs/kujira-orca/latest/src/kujira_orca/lib.rs.html ,
https://hexdocs.pm/kujira/Kujira.Orca.Queue.html , https://docs.kujira.app/dapps-and-infrastructure/orca/basics.md
- A **standing liquidation bid queue**. Bidders park stablecoin in one of `max_slot` premium pools (e.g. 15
  slots x `premium_rate_per_slot` 1 % = 0-15 %+ discount; the UI says up to 30 %).
- Liquidations fill **from the smallest premium upward**, giving "near-instant settlement" with no auction
  duration.
- New bids wait `waiting_period` (e.g. 600 s) before activation, which stops just-in-time sniping by bids
  placed after the liquidation is visible. A `bid_threshold` exists (exact semantics [gap]; the Elixir client
  calls it `activation_threshold`).
- Bidders may name a **delegate that activates bids** for them. Bots and the third-party "Sea Shanty" do
  this, and Sea Shanty also sends Telegram alerts for filled bids and collateral at risk.
- Example fees: `liquidation_fee` 1 % and `withdrawal_fee` 0.5 %. Closable pools can guarantee a minimum
  discount while liquidity is bootstrapped. Bids can wait in a yield-bearing asset.
- **The UX is the lesson**: users pick a discount and walk away.

**Injective burn auction** [2nd] https://injective.com/blog/the-all-new-inj-burn-standard-launch , https://injective.com/getinj
- Weekly: a basket of protocol fees (60 % at launch, any share since INJ 2.0, any asset from anyone since
  the "Burn Standard") is auctioned for INJ, and the INJ is burned.
- The direct analogue of our F5 surplus auction, with a **fixed cadence and a basket**.

**dYdX v4** [doc] https://docs.dydx.xyz/concepts/trading/liquidations
- Liquidation orders may only fill within the **fillable price**
  `P x (1 - (SMMR x MMF) x (BA x (1 - Q)))`, where `Q = account value / maintenance requirement`. The
  permitted price concession grows as the account approaches bankruptcy and is small for barely-unhealthy
  accounts.
- The penalty (max 1.5 %) goes to the insurance fund. ADL against offsetting positions is the last resort.
- Lesson: **health-dependent price limits** protect barely-unhealthy owners from deep-discount fills.

**Mars Protocol v2** [2nd] https://docs-mgzh62pi2-mars-team-labs-1.vercel.app/docs/learn/mars-v2/liquidations
- **Dynamic liquidation bonus** `min(intercept + slope*(1-HF), maxLB*)`, with
  `maxLB* = max(min(CR-1, maxLB), minLB)`. That is a solvency cap like Kamino's, plus a dynamic close factor.

**Skip POB (Cosmos MEV)** [doc] https://github.com/skip-mev/pob
- In-protocol top-of-block auction lane: `MsgAuctionBid` carries signed back-run bundles, and the revenue is
  redistributed on-chain.
- Lesson: the liquidation trigger race can be internalised and its MEV returned to the protocol. That is
  not available at the LEZ sequencer level today.

**Osmosis, Umee/UX, Neutron** [gap]: not researched to source depth in this pass; no claims made.

### Privacy / zk chains

**Penumbra** [doc] https://protocol.penumbra.zone/main/dex/swap.html , blog: https://penumbra.exchange/blog/dutch-auctions
- **Gradual Dutch auctions (GDA)**: an auction is a schedule of Dutch sub-auctions, each implemented as a
  concentrated-liquidity position on the DEX at the current step price. Arbitrage clears it automatically.
  Dutch auctions are shill-proof.
- **Auction ownership is a shielded NFT**: the schedule and reserves are public, the auctioneer is not.
  `ActionDutchAuctionEnd` and `ActionDutchAuctionWithdraw` return the reserves to the shielded pool.
- **Swap -> SwapClaim two-phase design**: inputs are burned publicly and outputs minted privately. The claim
  needs no spend authority (only the viewing key), **prepays its own fee out of the swap**, and can go only
  to the address fixed in the original swap.
- The docs say to **claim immediately**: a claim reveals its block and pair, so claiming later only narrows
  the anonymity set further.
- Sealed-bid batch swaps (validator-threshold-encrypted amounts that reveal only the net flow) are still
  *planned*. The docs warn that batches of one or a few swaps give little privacy until there are many users.

**Aztec** [2nd] https://forum.aztec.network/t/questions-about-private-defi-applications/6255 ,
https://forum.anoma.net/t/shielded-borrowing-lending/2645
- There is no permissionless private *shared* state, so lending and liquidation state is public while
  identities are private. That is the LEZ model too.
- One forum proposal: a pooled proxy socialises liquidation (one user underwater liquidates everyone), so use
  **one proxy per user** and let notes commit only to the proxy.
- Lesson: per-position isolation is mandatory when identities are hidden.

**Secret Network** [doc] https://docs.scrt.network/secret-network-documentation/confidential-computing-layer/ethereum-evm-developer-toolkit/usecases/sealed-bid-auction
- TEE-sealed-bid auctions over SNIP-20 tokens, a factory, and cross-chain bids through SecretPath.
- Privacy rests on SGX trust, which is not available on LEZ.

**Zk sealed-bid research** [doc]
- Cryptobazaar (scalable private sealed-bid with ZK well-formedness; only the winner and price are revealed):
  https://eprint.iacr.org/2024/1410.pdf
- Censorship-resistant sealed-bid with anonymous-deposit-membership ZK, which fixes commit-reveal's
  "refuse to open" problem: https://arxiv.org/pdf/2606.14939
- PACCs (anonymous collateralised commitments against MEV; the paper says extraction moves to censorship):
  https://arxiv.org/abs/2301.12818
- Every one of these needs **escrow plus a second phase**, which conflicts with RFP public state, with
  instant settlement and with ephemeral single-use accounts.

**Renegade** [doc] https://docs.renegade.fi
- A dark pool: MPC matching between relayers, a collaborative SNARK for settlement, and crosses at the
  **oracle midpoint**. It shows that oracle-anchored pricing plus hidden identity is a workable private
  market. That is our model minus hidden amounts.

**Railgun (measurement)** [2nd] https://arxiv.org/pdf/2606.25926 , https://arxiv.org/pdf/2608.22987
- Deposit-to-withdraw delays split into fast cyclers and long holders. Fast cyclers are exposed to timing
  analysis whatever the pool size.
- Reported real-world demixing used **unique denominations plus short dwell time**.
- Railgun's own docs flag **RPC providers** as a leak.

**Namada MASP** [2nd] https://docs.namada.net/users/shielded-accounts
- A unified multi-asset shielded set: one anonymity set for all assets.
- Shielded-set rewards (the Convert circuit) subsidise early depositors to grow the set.

**Aleo / Zcash shielded assets** [gap]: not researched to source depth.
- Our understanding is that Aleo splits private records from public `finalize` mappings, which is again
  "identity private, shared state public". Treat that as unverified.

**LEZ itself** [2nd] https://github.com/logos-blockchain/logos-execution-zone
- Private accounts are visible only to viewing-key holders and spent through a nullifier set; private
  execution is proven in RISC Zero.
- A Feb-2026 brief says private accounts **cannot receive funds directly**: a new account must be claimed.
  This matters for leftover payouts.

### Other ecosystems

**Hyperliquid** [doc] https://hyperliquid.gitbook.io/hyperliquid-docs/trading/liquidations
- Liquidations go to the order book first at mark price. Positions > 100 k USDC liquidate **20 % per order,
  then a 30 s cooldown**.
- Below 2/3 of maintenance margin the **backstop liquidator vault** (part of HLP) takes the position, and
  the trader loses the maintenance margin. The profit goes to HLP depositors. ADL is the last resort.
- Backstop vaults and insurance are out of our scope. The partial-plus-cooldown throttle is relevant.

**Sui (NAVI)** [doc] https://docs.naviprotocol.io/technical/technical-docs/technical-reference/liquidation-mechanics-and-walkthrough
- Health factor < 1, close factor ~35 %. The docs give the keeper profit as `bonus - gas` explicitly, and
  `debt_to_cover = u256::MAX` means "as much as allowed" (the same ergonomics as our `max_amount`).
- Scallop, Suilend, DeepBook [gap]: parameters not found in official docs in this pass.

**Aptos (Thala MOD)** [2nd] https://chaincatcher.com/en/article/2086924
- A stability pool absorbs liquidations pro-rata. **If the pool is short, the collateral falls back to a
  Dutch auction**, with partial liquidation to a safe level. Aries [gap].

**Cardano** [doc/2nd]
- Indigo: a per-iAsset stability pool burns iAssets to clear the debt and gives the collateral to stakers
  pro-rata. Fees are 2 % processing and 0.5 % withdrawal (V2). Indigo's MCP tool **builds unsigned liquidation
  transactions for client-side signing**.
  https://docs.indigoprotocol.io/readme/stability-pools-and-liquidations ,
  https://glama.ai/mcp/servers/IndigoProtocol/indigo-protocolindigo-mcp/tools/liquidate_cdp
- Liqwid: the bot takes the highest reward first and **skips to the next position on UTxO contention**.
  There is one bot per market, and rewards are redeemed manually. A batching system (LBS) was pending at
  launch. https://liqwidfinance.substack.com/p/liqwid-mainnet-launch-checklist

**Polkadot / Acala aUSD (Aug 2022)** [2nd] https://medium.com/acalanetwork/acala-incident-report-14-08-2022-392089588642 ,
https://medium.com/acalanetwork/acalas-path-to-resuming-operations-bf8ffdff5bbf
- A **misconfigured iBTC/aUSD pool** let ~3.02 B aUSD be minted in error.
- Governance halted the chain, burned ~1.29 B by referendum, and re-collateralised from the foundation's
  own CDP.
- The staged reopening **extended liquidation-auction durations**.
- Lessons: parameter misconfiguration is as dangerous as code bugs; a mint cap per period limits the blast
  radius; recovery needs "pause new, let existing finish".

**Bitcoin L2 (Stacks / Arkadiko)** [doc/2nd] https://docs.arkadiko.finance/protocol/vaults
- A protocol liquidation pool (USDA) performs most liquidations, at a 10 % example penalty.
- An audit notes that a **zero oracle price made vaults unliquidatable**:
  https://cocoa007.github.io/clarity-audit/arkadiko-vaults-manager.html
- A reported Oct-2026 **governance-takeover via just-in-time DIKO voting power** authorised a malicious vault
  contract [2nd, single source]. Lesson: a minimal, time-locked admin surface.
- Zest [gap].

---

## (b) Feature catalogue

Scope: **In** = within RFP-014 as written; **Out** = explicitly excluded (flash loans, aggregators/batching,
insurance/socialised loss, multi-collateral atomic); **Ext** = permitted but beyond the minimum (SDK/GUI/ops).
LEZ fit considers the account model (declared accounts, write locks, PDAs), zkVM cycle budget, and the
privacy model (ephemeral accounts, public state, no later pull from a reused account).

| # | Feature | Protocols | Problem solved | Fit for LEZ | Scope | Effort |
|---|---|---|---|---|---|---|
| 1 | Discount steps sized to private proving latency (stairstep schedule, step >= measured proof time) | Penumbra GDA (discrete steps); ours supports breakpoints | Private bidders need minutes to prove, so they lose every continuous-price race | Excellent: config only, schedule already piecewise-linear | In (F2 config) | S |
| 2 | Reduce-only / wind-down mode: risk-reducing actions never blocked by pause or staleness | Mango (full repay/withdraw on stale oracle; post-only), Kamino `allow_new_loans`, Acala staged reopen | Pauses that trap users or stop auctions finishing make incidents worse | Good: classify each instruction; leftover claim and `settle_*` need no price | In (F6 safety) | S |
| 3 | One-way-ratchet safety parameters (admin may tighten, not loosen, without a delay) | Velocity/Drift price band | Compromised or rushed admin weakening the safety rails | Good: compare old vs new in `set_*_config`, plus a delayed-activation slot for loosening | In (F7 host interface hygiene) | S-M |
| 4 | Spot-vs-TWAP deviation circuit breaker (separate from the `[R/2,2R]` band) | Velocity 50 % vs 5-min TWAP; Mango oracle band | Manipulated spot in thin markets (Mango, Solend USDH) | Good if the feed carries TWAP plus spot; one comparison, negligible cycles | In (oracle gate) | S |
| 5 | Confidence-band-adjusted pricing (collateral at lower band for trigger health, upper band for bid pricing) with a hard clamp/abort | marginfi, Mango stable price (min/max valuation) | Noise near the threshold; underpriced fills during volatility | Good; we already reject low confidence, this makes it graded | In | S |
| 6 | Health-aware discount cap: max discount <= collateral surplus at liquidation start | dYdX fillable price; Kamino/Mars solvency cap | Barely-unhealthy owners lose far more than the penalty if an auction runs to max discount | Good: snapshot one extra field per auction; one `min` per bid | In (F2) | M |
| 7 | Trigger-reward aging for neglected positions | Drift/Velocity fee aging; Kamino bad-debt bonus | Small or awkward positions never get liquidated (unprofitable trigger) | Medium: needs an "unsafe since" timestamp; a permissionless `flag_unsafe` costs a tx; or age from the oracle crossing | In (F1) | M |
| 8 | Liquidation throttle per time window (shrink, don't revert) | Hyperliquid 20 %/30 s, Velocity ramp, Kamino per-call cap | Dumping huge lots at once crashes the market and the oracle | Have Maker caps; a rolling-window variant needs one per-type window counter | In | S-M |
| 9 | Hot-account removal (bids on different auctions don't serialise on `BUFFER`/config) | Solana hot-account practice; Liqwid UTxO contention | Throughput under stress when many auctions run at once | Medium: per-auction penalty escrow swept lazily to the buffer; affects the accounting invariants | In (robustness) | M-L |
| 10 | Claim-to-commitment for the owner's leftover (bearer claim, fee prepaid) | Penumbra SwapClaim and auction NFT | Private owners collecting leftovers link the old position to new accounts; LEZ private accounts can't receive directly | Medium: leftover is claimable by proof of a secret bound at liquidation (host passes owner commitment) | In (privacy path, PRIV) | M |
| 11 | Client-side conditional bids ("ORCA without escrow"): user sets a target discount, own client fires | Kujira ORCA + Sea Shanty auto-activation | UX: bid and walk away; keeps privacy (no escrow, no on-chain intent) | Excellent: SDK/GUI only; needs the user's machine online | Ext | M |
| 12 | On-chain standing bid queue with premium slots and activation delay | Kujira ORCA; Indigo/Thala stability pools | Instant clearing, deep standing liquidity | Poor for privacy (escrow plus later pull from the same account); write-heavy | Out-ish (pool/aggregator; stretch for public users only) | L |
| 13 | Local-only alerts (position at risk, auction opened, discount reached, bid filled) | Sea Shanty, Kamino UI | Users miss liquidations and opportunities | Excellent: scanner already exists; must not call third-party servers | Ext (U2) | S-M |
| 14 | Keeper profitability estimator incl. fees, proving cost/time, slippage | NAVI `bonus - gas`, Drift keep-rs | Unprofitable or failing keeper txs; private keepers underestimate latency | Excellent: SDK quote path exists | Ext | S |
| 15 | Contention-aware retry and attempt spacing | Liqwid skip-on-contention, keep-rs 5-slot spacing | Bot storms, wasted proofs | Excellent | Ext | S |
| 16 | Unsigned-tx builder / tool interface for external signers or agents | Indigo MCP | Wallet-agnostic keepers; hardware wallets | Good (SDK already builds plans) | Ext | S |
| 17 | Fixed-cadence surplus auction (lots released on a schedule) | Injective weekly burn | Predictable participation; fewer tiny auctions | Good; partly done (relist delay ADR-039) | In (F5 params) | S |
| 18 | Stability-pool fallback before the auction | Thala, Indigo, Arkadiko | Instant clearing | Socialises pro-rata; looks like an aggregator | Out | L |
| 19 | Backstop vault / insurance fund / ADL | Hyperliquid HLP, Drift IF, dYdX | Absorbs losses beyond the auction | n/a | Out | - |
| 20 | Sealed-bid zk auctions (commit-reveal, range proofs) | Secret, Cryptobazaar, arXiv 2606.14939 | Bid privacy, anti-sniping | Poor: two phases, escrow, lockups; RFP wants public state; zk range proofs in guest = high cycles | Out (future research) | L |
| 21 | In-protocol MEV lane for triggers | Skip POB, Jito | Returns trigger MEV to the protocol | Needs sequencer support | Out (platform) | L |
| 22 | Account-lock griefing analysis (oracle and auction accounts) | Solend USDH | Attacker stalls oracle or bids by locking accounts | Doc plus tests; staleness pause is the safe failure | In (threat model) | S |
| 23 | Price rate-limit ("stable price") as second trigger condition | Mango | Spike-driven liquidations | Medium: per-type state updated by a keeper; overlaps TWAP | In | M |
| 24 | Public auction explorer and health metrics (time-to-clear, clearing discount, keeper concentration, shortfall) | Kujira ORCA UI, Drift/Kamino dashboards | Reviewer evidence; ops monitoring | Good: scanner plus GUI | Ext | M |
| 25 | Per-collateral risk tiers (elevation groups / isolated pools) | Kamino, Solend, Drift isolated IF | Contagion between collaterals | Have: per-collateral config and caps | In | - |

---

## (c) Privacy-specific ideas

### What a private bidder / liquidator on LEZ should and should not get

Should get:
1. **Unlinkable identity per action.**
   - A trigger keeper, a bidder or an F4/F5 taker appears as a fresh ephemeral account funded by deshield
     and drained by reshield.
   - No on-chain record ties two actions to the same person unless the user's own behaviour does
     (amounts, timing, RPC).
2. **Equal economic terms.**
   - The same price, reward and slippage protection as public users.
   - A schedule that does not structurally exclude them because they must prove locally (see item 1 of (d)).
3. **No later obligations.**
   - Every private action settles in one transaction (already the design for bids and F4/F5): no refunds,
     no claims, no escrow.
   - The only pull is the position owner's leftover, which needs its own unlinkable path (catalogue item 10).
4. **Plain-language disclosure** of what remains public.
   - Public: amounts, prices, the auction ID, timing, the ephemeral account IDs, and that a deshield and a
     reshield happened around the action.

Should not get:
- **Hidden amounts or prices.** RFP: auction state is public, and keepers and auditors must verify
  conservation.
- **Anything that relaxes a check.** Private actions run the same guest code. They do not bypass min bid,
  `chost`, caps or the oracle gate.
- **Standing on-chain intents or escrow** (ORCA-style queues, sealed bids). These require an account to
  persist and to be pulled from later, which breaks the single-use model and creates a fingerprint (a
  long-lived bid at a characteristic premium).
- **Privileged or delayed reveals.** No "private priority" and no commit-reveal that lets a private bidder
  cancel after seeing others. Censorship-resistant sealed-bid research shows how quickly that becomes an
  option for the bidder.
- **Liquidation of their own position from a private account for profit.** Already unprofitable (reward <
  penalty), but the GUI should say so, because private self-liquidation looks like a way to recover the
  penalty.

### Linkability risks in deshield -> act -> reshield, specific to auctions

| Risk | Why auctions make it worse | Mitigation |
|---|---|---|
| **Amount fingerprint** | `coin_in` equals the deshielded amount, and the reshielded collateral equals `collateral_out` exactly; both are public. Railgun demixing used unique denominations. | Deshield **rounded denominations** (e.g. 1-2-5 steps) at or above the need; the bid uses `max_coin_in`; reshield collateral **and change** together; never deshield an odd exact amount. |
| **Timing chain** | Deshield in block N, bid in N+1, reshield in N+2 is a tight chain, and few users interact with a given auction. | Prefer one transaction for deshield + bid + reshield if LEZ allows it (an atomic chain has no timing gap). Otherwise send back to back. Per Penumbra, finish immediately: delay helps nothing once the act is public. Randomise *when you start* (pre-deshield into a standing pool of fresh accounts at random times) rather than the gaps. |
| **Tiny anonymity set** | Penumbra warns that one- or few-participant batches give little privacy. A single auction may have one private bidder; if only one private user is active in that hour, the action is attributable. | GUI **privacy meter**: count of shield/deshield events of that token in the last N blocks. Warn below a threshold ("you would be the only private participant in this window"). |
| **Fee payer / gas account** | A shared fee payer links everything. | The fee comes from the same ephemeral account (PA2: atomic token-and-gas deshield); the SDK refuses a shared payer in private mode. |
| **Reuse** | Reusing an ephemeral account across auctions links them all. | Keystore marks an ephemeral account burnt after first use; the SDK hard-errors on reuse (PRIV3/PA3). |
| **RPC / scanner queries** | Querying a specific auction, position or quote over a third-party RPC reveals interest before acting (Railgun flags RPC leakage). | Scanner downloads **all** live auctions, not targeted ones; quotes are computed locally with `auction_core`; recommend own node or a local proxy; never send position IDs to remote services. |
| **Behavioural fingerprint** | Bidding at the same discount step every time, the same lot fraction, or the same reaction delay. | SDK jitters the step chosen within the user's tolerance and the fill size within bounds (opt-in); document the residual risk. |
| **Leftover claim** | The owner claiming the leftover links the liquidated position (public) to the receiving account. LEZ private accounts can't receive directly. | Claim-to-commitment (item 10): a fresh account proves the secret and receives the leftover; prepaid fee; then reshield. |
| **Keeper reward** | The reward goes to the trigger's holding, and its amount is unique. | Reward to a fresh ephemeral holding; reshield together with other funds; private triggering is not possible today (ADR-034), so say so plainly. |
| **Cross-correlation with the host** | A private owner's position on the host stablecoin is a public account; liquidation reveals its history. | Out of the engine's control; document that position privacy is the host's concern. |
| **Proving-time correlation** | A private action shows up exactly "proof time" after a price step is reached, which looks different from public keepers. | Stairstep schedule (item 1) makes the step the timing anchor for everyone. |

### Concrete SDK / GUI safeguards (proposed)

1. **Private mode is a mode, not a flag per call.**
   - It covers the keystore, scanner and signer.
   - It forbids mixing public and private accounts in one session plan.
   - It refuses targeted RPC lookups.
2. **Plan preview, "what becomes public".**
   - Before signing, list every public field the transaction writes: amounts, auction ID, ephemeral account
     IDs, and the deshield and reshield events.
   - Show a linkability score built from amount roundness, anonymity-set size and reuse checks.
3. **Denomination planner**: choose the deshield amount from the user's slippage bound and round it upward;
   change is always reshielded in the same transaction.
4. **Non-skippable reshield** (PRIV1).
   - If reshield fails, the SDK retries reshield first, before anything else, and persists a recovery
     journal locally.
   - The GUI shows "funds exposed in ephemeral account X" until reshield succeeds.
5. **Burn-after-use keystore.** Ephemeral keys are tombstoned after their first transaction; any reuse is a
   hard error with no override in the GUI.
6. **Anonymity-set warning.** A threshold is configurable, the default is conservative, and it never blocks
   (the user decides).
7. **Local-only alerts and conditional bids** (items 11 and 13). No Telegram or cloud bot ever learns a
   position or target discount; alerts run in Basecamp or the CLI daemon.
8. **Honest status.**
   - Private `liquidate` is unsupported on LEZ v0.2.4 (ADR-034); the GUI greys it out with the reason.
   - Bids, settlement and system auctions are untested on the private path; label them experimental until
     M4 evidence exists.

---

## (d) Top 15 recommended improvements

Ranked by combined value to an RFP-014 reviewer (requirement coverage, evidence, distinctiveness) and
real-world safety. Effort: S <= 2 days, M <= 1 week, L > 1 week.

1. **Proving-latency-aware stairstep schedule (S).**
   - Measure private-path proof time and publish it in CYCLES.md (one data point already: 543 s for
     `settle_debt` on the hosted runner).
   - Set the default collateral schedule's step length well above it; document why.
   - Add a test that a private-latency bidder gets the same price as a fast bidder within a step.
   - Unique to a zk chain; directly supports the RFP's privacy goals. Sources: Penumbra GDA, Jito latency
     races.
2. **Private-mode SDK safeguards bundle (M).** Denomination planner, burn-after-use keystore, non-skippable
   reshield with a recovery journal, refusal of shared fee payers, whole-set scanning. Turns PRIV1-3 and PA2-3
   from promises into code plus tests, and is the clearest privacy evidence for M6.
3. **Reduce-only / wind-down semantics (S).**
   - Classify every instruction as risk-increasing or risk-reducing.
   - Guarantee that `claim_leftover`, `settle_empty`, `settle_debt` and owner-side actions on the host run
     under any pause or stale oracle; test each.
   - Mango and Acala show that stuck pauses compound incidents.
4. **Health-aware discount cap (M).**
   - Snapshot `max_discount_eff = min(schedule_max, 1 - debt_with_penalty / collateral_value_at_start)`, with
     a floor so auctions still clear.
   - Stops barely-unhealthy owners losing more than the penalty.
   - Kamino and Mars solvency caps, dYdX fillable price. Strong fairness story.
5. **One-way-ratchet parameters with delayed loosening (S-M).**
   - Tightening (lower max discount, lower caps, shorter staleness, smaller band) applies at once.
   - Loosening waits for a delay and is announced in an event.
   - Velocity price band; Arkadiko governance takeover and Acala misconfiguration as motivation. Cheap, and
     governance-minimising, as in GEB.
6. **Spot-vs-TWAP deviation breaker plus graded confidence (S).**
   - Add a per-type `max_spot_twap_dev_bps` gate in addition to `[R/2, 2R]`.
   - Use the confidence-adjusted price (protocol-favourable side) in `collateral_out`.
   - Mango, Solend USDH, marginfi.
7. **Account-lock and griefing section in the threat model, with tests (S).**
   - Oracle-account and auction-account write-lock stalls; whether a staleness pause is the only effect.
   - Whether a griefer can keep a type paused indefinitely, and what that costs them.
   - Solend USDH precedent; reviewers look for platform-specific threats.
8. **Hot-account audit and concurrent-auction benchmark (M; L if refactored).**
   - Show N simultaneous auctions with interleaved bids: cycles and transaction size per bid, and which
     accounts every bid writes (today `BUFFER` and the collateral config).
   - Decide whether to escrow the penalty per auction and sweep it lazily.
   - Covers the "many concurrent auctions" robustness question.
9. **Local alerting and watchlists in Basecamp and the CLI (S-M).** Position at risk, auction opened,
   discount reached, bid filled, type paused or unpaused. Strictly local (Sea Shanty UX without the
   third-party bot). High GUI value.
10. **Client-side conditional bids, "ORCA without escrow" (M).**
    - The user sets a collateral type, a target discount, a max coin and an expiry.
    - The local daemon executes when the condition is met, through the private or public path.
    - Gives ORCA's set-and-forget UX with no on-chain intent, so privacy is kept.
11. **Keeper profitability and latency estimator (S).**
    - `quote_* -> {gross, fees, proving_time_est, slippage_est, net, ok}`.
    - Contention-aware retry with attempt spacing; highest-reward-first ordering.
    - From NAVI, Drift keep-rs and Liqwid. Clear SDK-quality evidence.
12. **Claim-to-commitment for the owner's leftover (M).**
    - The host passes an owner commitment at liquidation; any fresh account presenting the secret (proof in
      the guest) claims to itself with a prepaid fee.
    - From Penumbra SwapClaim and auction NFT. Fixes the one remaining pull path for private owners.
    - Needs a host hook change, so mark it as a proposal for upstream.
13. **Trigger-reward aging for neglected positions (M).**
    - A small time-based increase of `tip` after a grace period, from an oracle-derived "unsafe since"
      timestamp, still capped at the penalty.
    - From Drift/Velocity fee aging and the Kamino bad-debt bonus. Addresses dust positions that nobody
      liquidates.
14. **Public auction explorer and health metrics (M).**
    - Time-to-clear, discount at clearing, shortfall per auction.
    - Keeper concentration (share of the top-3 bidders, which shows whether Reflexer-style keeper
      centralisation is recurring).
    - Paused time per type. Gives reviewers live evidence; fed by the existing scanner.
15. **Rolling-window liquidation throttle (S-M).**
    - Complement the static caps with a per-type "max collateral entering auction per window" that shrinks
      lots rather than reverting.
    - From the Hyperliquid 20 %/30 s throttle and the Velocity ramp. Limits self-reinforcing oracle crashes on
      thin LEZ markets.

Deliberately not recommended (scope or fit): stability pools and on-chain ORCA queues (pooling and escrow
break single-use privacy and look like aggregation), insurance/backstop vaults/ADL (excluded), sealed-bid or
commit-reveal zk auctions (two phases, escrow, high guest cycles, conflicts with RFP public state; worth a
future-work paragraph citing Cryptobazaar and arXiv 2606.14939), in-protocol MEV lanes (needs sequencer
support).

### Gaps to close before citing in the submission
- Kujira ORCA `bid_threshold` semantics and current live parameters (only Terra-era examples were found).
- Exact Hyperliquid ADL rules, dYdX SMMR/BA defaults, Suilend/Scallop/Aries parameters, Osmosis/Umee/Neutron,
  Aleo/Zcash ZSA.
- Penumbra's auction spec page (protocol.penumbra.zone/main/dex/auction.html) returned 404; the blog and the
  swap spec were used instead.
- Drift's docs now redirect to Velocity Exchange; the cited mechanics come from the Velocity page and may
  differ from historical Drift v2.
