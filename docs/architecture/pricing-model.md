# Pricing model

How the engine prices a collateral auction, what happens when the price feed goes bad, where
oracle-extractable value can leak, and what a liquidation costs on LEZ 0.4. Decision record:
ADR-005 (band-clamped price), ADR-012 (no restart, no expiry), ADR-067 (live pricing, bounded
stale-feed bids) in `docs/design-log.md`.

## 1. Live price on every bid

Each bid reads the oracle again. The price is never fixed when the auction opens.

**The formula** (`quote_bid`, `crates/auction_core/src/auction.rs` lines 91-150):

- `P` is the bid price in collateral per coin: the validated market price `M`, clamped to a band
  around the redemption price `R` (`bid_price`, `crates/auction_core/src/price.rs` lines 89-93;
  ADR-005, default band 90%-110% of `R`).
- `d` is the discount at the auction's effective elapsed time, read from its piecewise-linear
  schedule (`Schedule::discount_ray`, `crates/auction_core/src/schedule.rs`). The schedule only
  rises and is capped at `MAX_DISCOUNT_BPS = 3000` (30%).
- A bid of `coin` coin receives `floor(coin x P / (1 - d))` collateral (`auction.rs` lines
  106-110), before the lot and final-fill rules.

On 0.4 the same core runs behind the Observe block: every `Bid` reads the host and the oracle at
the collateral actor, runs `require_live` and the band clamp, and only then reaches the auction
(`live_price`, `v04/engine_core/src/feed.rs`; design note `docs/v04-port-design.md` §4.1).

**Why there is no `redo`, `cusp` or `tail`.** Maker's Clipper sets a starting price
`top = feed x buf` at `kick` and lets it decay along an Abacus curve. The decay knows nothing
about the market after `kick`. So Clipper needs a reset: `redo` is allowed once
`now - tic > tail` (too old) or `price / top < cusp` (fallen too far), re-reads the feed and sets a
new `top`, and pays a reward for doing so (`docs/research/2026-11-07-evm-engines.md` §A1, from Maker's
`dss` repository, `src/clip.sol`). Here the market term is current at every bid, and only the discount moves
with time. A reset would only move the discount from its maximum back to its start. ADR-012 records the
decision; the research survey lists it as row 7 of its feature table ("IN for debt and surplus only: our collateral auctions price live").

**It does not get stuck when the market falls past the maximum discount.** With a fixed `top`, a
market that falls faster than the curve leaves the auction price above the market: nobody bids
until `redo`. Here the bid price follows the market down, so at any time a bidder gets at least the
market rate plus the current discount. An unsold auction stays open at its maximum discount until
someone bids; there is no expiry
(`unsold_auction_holds_max_discount_and_keeps_its_reservation`,
`programs/integration_tests/tests/bid.rs`).

**It does not sell far below market when the market rises.** With a fixed `top`, a market that
rises after `kick` leaves the auction price under the market, and the curve keeps lowering it. Here
a rise in the collateral's value lowers `P` (fewer collateral units per coin) at the next bid, so
the sale stays within the scheduled discount of the current price.

**Where the band limits this.** The clamp is a guard against a mis-scaled or manipulated feed
(ADR-005), and it is the one place the price is not the market's:

- If `M` is above the band (collateral cheap in coin terms), `P` stops at `R x hi`. A bidder gets
  less collateral than the market rate. If the market moves far enough past the edge that even the
  maximum discount does not cover the gap, bidding stops being worth it until the market comes
  back or the feed leaves `[R/2, 2R]` and the type pauses.
- If `M` is below the band (collateral dear), `P` stops at `R x lo`. A bidder gets more collateral
  than the market rate, so the sale is below market by the gap between `M` and the band edge.
- The band is a per-type parameter (`band_lo_bps` 5000-10000, `band_hi_bps` 10000-20000,
  `FeedParams::validate` in `price.rs`). A wider band makes the price follow the market further.

## 2. Stale feeds and R5

**Today.** A bid needs a valid feed. `validate_feed` (`price.rs` lines 59-85) checks age,
confidence and the `[R/2, 2R]` range. If any check fails, the type pauses and the bid is refused
with no state change and no tokens moved:

- v0.2.4: `bid_rejects_invalid_feed` (`programs/integration_tests/tests/bid.rs`) covers stale,
  low-confidence and out-of-range feeds.
- 0.4: `a_stale_feed_stops_the_bid` (`v04/harness/tests/bid.rs`).

While the type is paused, the discount clock stops: the paused time is taken out of the effective
elapsed time (`crates/auction_core/src/pause.rs`), so an outage does not hand the next bidder a
larger discount (`discount_frozen_while_paused`, `programs/integration_tests/tests/bid.rs`).

This is safe but not live. During an outage no debt is recovered, the lot carries the market risk,
and its cap reservation stays held.

**Decided rule (D4), planned for M1/M2, not built.** While a type's feed is stale or invalid, bids
on its live auctions continue at the last good TWAP, only while that TWAP is younger than a
configurable maximum age, and never below the auction's floor. Risk-reducing operations (settle,
claim leftover, settle debt, owner actions) run under every pause level. The TWAP is milestone M2
(E1); the stepped pause levels that apply D4 are milestone M1 (E2). Their exact parameters and the
floor's definition belong to the Tier 2 engine design.

**Why bounded last-good TWAP, and not the two simpler rules:**

| Rule | What goes wrong |
|---|---|
| Refuse every bid (today) | No recovery for the length of the outage. Black Thursday showed that the hours a feed or the chain is degraded are the hours liquidations matter most. |
| Freeze the price seen at auction open | The fixed-`top` problem of §1, plus no `redo` to escape it: the price drifts from the market for as long as the outage lasts. |
| Last good TWAP, bounded by age and floor | Bids keep clearing on the most recent price the engine trusted. The age bound stops it once that price is too old to trust; the floor bounds the worst sale. A TWAP is harder to move with one bad print than a spot reading. |

## 3. Oracle-extractable value

Oracle-extractable value comes from a fixed prize paid to whoever acts first after a price update
(`docs/research/2026-11-07-evm-engines.md` §A15). The engine has two places a prize could sit.

**The bid.** No fixed bonus exists. The discount starts low and rises with time, so bidders compete
it down: the first bidder to whom the current discount is worth it takes the lot, and the protocol
keeps the gap between that discount and the maximum. The rising discount auctions the value back.
Because the price is re-read at each bid, an oracle update does not open a gap between the auction
price and the market either. The residual is oracle lag: a bidder who sees the market move before
the feed does can trade on the difference, within the feed's maximum age and the band.

**The trigger.** The keeper reward is the only fixed prize: `min(tip + chip x debt, reward_cap,
penalty)` in coin, paid in collateral at the bid price (`crates/auction_core/src/liquidation.rs`
lines 86-93). It is capped by `reward_cap` and by the penalty, so the value a searcher can extract
by triggering first is bounded by those parameters, and they can be set near the cost of the
transaction.

Oracle-level recapture (Chainlink SVR, UMA Oval, API3 OEV Network) needs the oracle or the
sequencer to run an auction for the update. That is outside the engine; the research survey marks
it ADJ (row 23 of its feature table).

## 4. What a liquidation costs

**Measured** (`v04/CYCLES.md`, LEZ `v0.4.0-rc1`, `v04/harness/tests/cycles.rs`): `Liquidate` is 29
executions and 839,357 cycles, under 9% of the 10,000,000-cycle gas cap. A partial bid is 15
executions and 407,700 cycles; a final bid 21 and 576,148.

These figures include the whole conversation, not only the engine's counters: real token transfers
through the token program, custody of the seized collateral in a per-auction vault, the host's own
seizure, and the cap reservations.

**Where the 29 executions go.** Counted from the message order in the guest sources
(`v04/programs/methods/guest/src/bin/engine/proxy.rs`, `observe.rs`, `liquidate.rs`, `auction.rs`,
`v04/programs/methods/guest/src/bin/mock_host.rs`) and the token flows in `docs/v04-port-design.md`
§6 (a token debit is three executions):

| Part | Executions | Messages |
|---|---|---|
| Entry | 2 | keeper's proxy; seeded `Probe { Collateral }` |
| Feed observation | 7 | Observe; `ReadState` to the host and its reply; `ReadState` to the oracle and its reply; `AssertTimestamp`; `Proceed` |
| Engine state | 4 | `LiqCtx` at the config; record `Bump`; auction `Open`; auction `Finish` |
| Host seizure | 6 | `LiquidatePosition`; the host's oracle `ReadState` and reply; `Seize`; `Seized` report; host `Finish` |
| Token transfers | 6 | vault sweep to the auction vault (3); keeper reward from the auction vault (3) |
| Custody check | 2 | `ReadState` to the auction vault's holding and its reply |
| Cap reservations | 2 | `Reserve` at the collateral actor and at the config |
| **Total** | **29** | matches `LIQUIDATE_EXECUTIONS` in `v04/harness/tests/cycles.rs` |

`Liquidate` does not touch the system buffer. Buffer accounting (shortfall booking, settle, burn)
shows up in the final bid and in the system instructions (`v04/CYCLES.md`, system table).

**Counters-only share: an estimate, not a measurement.** The simulator reports cycles per
transaction, not per execution, so the share cannot be measured from what is in the repo. The
estimate takes the 10 executions a counters-only implementation would not need (token transfers 6,
custody check 2, cap reservations as separate actors 2) and prices them three ways, using
cycles-per-execution figures from `v04/CYCLES.md`:

| Cycles per execution | Source | 10 executions | Rest (19 executions) |
|---|---|---|---|
| 22,740 | `ClaimLeftover`: 136,440 / 6 | 227,400 | 611,957 |
| 28,943 | `Liquidate`: 839,357 / 29 | 289,433 | 549,924 |
| 34,715 | `SettleEmpty`: 208,291 / 6 | 347,152 | 492,205 |

So, under the stated assumption that those executions cost about as much as an average one,
transfers, custody and separate reservations account for roughly 230,000 to 350,000 of the 839,357
cycles (about 27% to 41%), and a counters-only liquidation with the same feed checks and host
seizure would cost roughly 490,000 to 610,000. A design that also drops the host conversation
would cost less again, but would no longer move or check any collateral.
