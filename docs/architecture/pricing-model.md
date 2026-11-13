# Pricing model

How the engine prices a collateral auction, what happens when the price feed goes bad, where
oracle-extractable value can leak, and what a liquidation costs on LEZ 0.4. Decision record:
ADR-005 (band-clamped price), ADR-012 (no restart, no expiry), ADR-067 (live pricing, bounded
stale-feed bids), and for the v04 engine ADR-070 (TWAP), ADR-071 (confidence edge), ADR-072
(bid anchor and deviation breaker), ADR-073 (pause levels) and ADR-074 (D4 as built) in
`docs/design-log.md`.

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
(`v04/engine_core/src/gate.rs`, `bid_pricing`; design note `docs/v04-port-design.md` §4.1).

**The v04 TWAP (ADR-070).** Each collateral actor keeps a ring of at most 8 validated mids
(`TWAP_CAPACITY`), written by the Observe block that `SetCollateral`, `ObserveFeed`, `Liquidate`
and `Bid` already run and that `SetPauseLevel` ends with, and only while `twap_window_ms > 0`. A valid reading is recorded at
`min(reading time, now)` unless it is not newer than the newest sample or less than
`twap_min_spacing_ms` after it; an invalid reading is never recorded. The TWAP
(`risk_core::twap::twap`) anchors at `min(now, newest + max_oracle_age_ms)`, so it is constant for
the whole of an outage. It weights each sample by how long it was the current publication,
clipped to `[anchor − window, anchor]`, stops at the first older sample that was current for
longer than `max_oracle_age_ms` (a stale period breaks it), and is unavailable with fewer than
`twap_min_obs` weighted samples. `T = Σ floor(p_i · w_i / Σw)`, so `T` lies in
`[min p_i − 7, max p_i]` of the used samples (`twap_within_sample_range`). The ring holds 8
samples, so `RiskArgs::validate` refuses `twap_window_ms > 8 · twap_min_spacing_ms`: a longer
window could be cut to the ring's span by one `ObserveFeed` per spacing interval. At the bound a
full ring spans at least 7 spacings, 7/8 of the window. The recommended preset has 6 spacings per
window.

**The price per operation (ADR-071, ADR-072).** Every engine price is a payout (collateral per
coin), so each one takes the smaller `P`. With `conf_bps = ceil(c · 10_000 / M)` and
`edge(x) = floor(x · (10_000 − conf_bps) / 10_000)` (`risk_core::price`):

| Operation | Price | Refused when |
|---|---|---|
| `Liquidate` (keeper reward, stale floor) | `band(edge(min(M, T)))` | pause level ≥ 1 (6073; 6020 under `admin_paused`); invalid feed (its own code); no TWAP (`TwapUnavailable`, 6070); breaker on (`dev > 0`) and `abs(M − T) · 10_000 > dev · T` (`SpotTwapDeviation`, 6071) |
| `Bid`, live feed | `band(min(edge(M), floor(T · (10_000 + dev) / 10_000)))` | level 3 (6075; 6020 under `admin_paused`). The anchor applies only with `dev > 0` and the TWAP available: a bid during warm-up is `band(edge(M))`, never refused (ruling R9-5) |
| `Bid`, stale feed (D4, §2) | `band(edge(T_lg))` at the ring's last confidence, then the floor | §2 |
| TWAP off (`twap_window_ms = 0`, the legacy preset) | `band(edge(M))` for both | as v0.2.4, plus the levels |

`band` is v0.2.4's `bid_price` clamp to `[R·lo, R·hi]`. The bid is anchored from above only: a
manipulated spot cannot pay out more than `dev` beyond the TWAP, while in a fast fall (TWAP
lagging below spot) bids still follow the market. At confidence 0, as the legacy producers write,
`edge` is the identity, so the legacy preset prices exactly as before. Tests:
`liquidate_refused_until_twap_has_min_obs`, `keeper_reward_uses_the_smaller_of_spot_and_twap`,
`a_spot_spike_is_refused_by_the_deviation_breaker`, `bid_price_takes_the_confidence_edge`
(nonzero confidence), `bid_payout_capped_by_the_twap_anchor`, `bid_during_twap_warm_up_is_not_refused`
(`v04/harness/tests/twap.rs`); `liq_gate_order_admin_feed_twap_deviation`,
`bid_pricing_anchors_once_warm_and_skips_the_breaker` (`v04/engine_core/src/gate.rs`);
`deviation_matches_the_exact_rule`, `live_bid_is_capped_at_the_floored_anchor`
(`v04/risk_core/src/price.rs`).

**Why there is no `redo`, `cusp` or `tail`.** Maker's Clipper sets a starting price
`top = feed x buf` at `kick` and lets it decay along an Abacus curve. The decay knows nothing
about the market after `kick`. So Clipper needs a reset: `redo` is allowed once
`now - tic > tail` (too old) or `price / top < cusp` (fallen too far), re-reads the feed and sets a
new `top`, and pays a reward for doing so (`docs/research/2026-11-07-evm-engines.md` §A1, from Maker's
`dss` repository, `src/clip.sol`). Here the market term is current at every bid, and only the discount moves
with time. A reset would only move the discount from its maximum back to its start. ADR-012 records the
decision; the research survey lists it as row 7 of its feature table, in scope
for debt and surplus auctions only because collateral auctions price live.

**Inside the band, it does not get stuck when the market falls past the maximum discount.** With a
fixed `top`, a market that falls faster than the curve leaves the auction price above the market:
nobody bids until `redo`. Here the bid price follows the market down, so while `M` is inside the
band a bidder gets at least the market rate plus the current discount. An unsold auction stays open at its maximum discount until
someone bids; there is no expiry
(`unsold_auction_holds_max_discount_and_keeps_its_reservation`,
`programs/integration_tests/tests/bid.rs`).

**Inside the band, it does not sell far below market when the market rises.** With a fixed `top`,
a market that rises after `kick` leaves the auction price under the market, and the curve keeps
lowering it. Here a rise in the collateral's value lowers `P` (fewer collateral units per coin) at
the next bid, so the sale stays within the scheduled discount of the current feed price.

**Where the band limits this.** The clamp is a guard against a mis-scaled or manipulated feed
(ADR-005), and it is the one place the price is not the market's:

- If `M` is above the band (collateral cheap in coin terms), `P` stops at `R x hi`. A bidder gets
  less collateral than the market rate. If the market moves far enough past the edge that even the
  maximum discount does not cover the gap, bidding stops being worth it until the market comes
  back or the feed leaves `[R/2, 2R]` and the type pauses. The condition is
  `M > R x hi / (1 - d_max)`. With `hi = 11000` and the 10 % maximum of the `rai_default` test
  schedule (`crates/auction_core/src/schedule.rs` lines 81-93) that is
  `M > 1.1R / 0.9`; even at the 30% cap it is `M > 1.1R / 0.7`. Both are inside the `[R/2, 2R]`
  validity range, so the type does not pause there. `R` moves at the host's redemption rate, not
  with the collateral market (`programs/auction_engine/src/feed.rs` lines 25-44), so a fall in
  the collateral of about the band width is enough to reach the edge.
- If `M` is below the band (collateral dear), `P` stops at `R x lo`. A bidder gets more collateral
  than the market rate, so the sale is below market by the gap between `M` and the band edge. With
  the default `lo = 9000` and a valid feed as low as `R/2`, `P` is `0.9R`, so the sale can be well
  below market before any discount.
- The band is a per-type parameter (`band_lo_bps` 5000-10000, `band_hi_bps` 10000-20000,
  `FeedParams::validate` in `price.rs`). A wider band makes the price follow the market further.

## 2. Stale feeds and R5

**With D4 off** (v0.2.4, and the v04 legacy preset). A bid needs a valid feed. `validate_feed`
(`price.rs` lines 59-85; on v04 `validate_feed_ext`, equal to it under the legacy policy) checks
age, confidence and the `[R/2, 2R]` range. If any check fails, the bid reverts with no state change
and no tokens moved, and the type counts as paused from the feed's derived onset (policy (b),
ADR-007, ADR-032; `require_live`, `programs/auction_engine/src/feed.rs` lines 84-92):

- v0.2.4: `bid_rejects_invalid_feed` (`programs/integration_tests/tests/bid.rs`) covers stale,
  low-confidence and out-of-range feeds.
- 0.4: `a_stale_feed_stops_the_bid` (`v04/harness/tests/bid.rs`).

While the type is paused, the discount clock stops: the paused time is taken out of the effective
elapsed time (`crates/auction_core/src/pause.rs`), so an outage does not hand the next bidder a
larger discount. Tests: `discount_frozen_while_paused` (`programs/integration_tests/tests/bid.rs`,
an admin pause); for a stale feed, `derived_onset_closes_unobserved_gap`
(`crates/auction_core/src/pause.rs`) and `observe_feed_persists_stale_pause`
(`programs/integration_tests/tests/observe.rs`).

This is safe but not live. During an outage no debt is recovered, the lot carries the market risk,
and its cap reservation stays held.

**D4 as built (ADR-074), v04 only.** While a type's feed fails `validate_feed_ext`, a bid on one
of its auctions proceeds only if all four hold (`engine_core::gate::bid_pricing`,
`risk_core::stale::fallback_twap`):

1. the pause gate lets it through: effective level <= 2 (at level 3 the gate answers first, with
   `BidsPaused`, 6075, or `Paused`, 6020, under `admin_paused`);
2. D4 is on: `stale_bid_max_age_ms > 0` and `twap_window_ms > 0` (`RiskArgs::d4_on`);
3. the last-good TWAP `T_lg` is available with `twap_min_obs` samples (else `TwapUnavailable`,
   6070);
4. `now − newest.ts <= stale_bid_max_age_ms` (else `LastGoodTwapTooOld`, 6077).

With D4 off the bid fails with the feed's own code, as above. `now` is the bid's claimed time, so
condition 4 can pass up to `slack_ms` after the newest sample's age bound would (the feed-age check
itself uses `now + slack_ms`); the floor bounds what that slack can pay out.

The price is `P_fb = band(edge(T_lg))` with the ring's `last_conf_bps`, and the redemption price
read in the same observation. At `Seized` the auction stores, only while D4 is on,

```
stale_cap_ray = floor(P_liq * RAY / (RAY - d_max_eff))
```

with `P_liq` the liquidation price and `d_max_eff` the last point of the auction's snapshotted
schedule (`Schedule::max_discount_ray`; the health cap of E4 that would lower it is not built). A
fallback bid pays at

```
P_used = min(P_fb, floor(stale_cap_ray * (RAY - d) / RAY))
```

where `d` is the bid's discount, **frozen**: the bid carries the open pause (`BidCtx`
`paused_since_ms`, `fallback: true`), so `bid::quote` stops the discount clock at the outage's
onset and an outage never hands a larger discount. In words, during a stale feed the auction never
sells collateral for less coin per unit than `(1 − d_max_eff) / P_liq`, the worst price its own
schedule reaches at its opening price. The bid is clamped to the floor, not refused;
`min_collateral_out` protects the bidder. Live bids ignore the floor. An auction opened while D4
was off stored `stale_cap_ray = 0`, so a fallback bid on it is refused with `PriceFeedStale`
(6021) rather than sold unfloored.

Risk-reducing operations (settle, claim leftover, settle debt, closes, `CheckSolvency`) run at
every pause level (ADR-073).

Tests: `stale_feed_bid_uses_the_last_good_twap`, `stale_feed_bid_refused_past_max_age`,
`stale_feed_bid_refused_without_min_obs`, `stale_feed_bid_refused_on_an_auction_opened_without_d4`,
`stale_feed_bid_held_at_the_floor`, `discount_stays_frozen_across_fallback_bids`,
`fallback_final_bid_settles_and_releases` (`v04/harness/tests/stale_bids.rs`);
`stale_bid_prices_at_the_last_good_twap`, `stale_bid_refused_past_the_max_age`,
`stale_bid_with_d4_off_is_the_feed_code`, `stale_bid_level_3_and_admin_refuse_first`
(`v04/engine_core/src/gate.rs`); `fallback_quote_freezes_the_discount`,
`fallback_quote_is_held_at_the_floor_and_live_ignores_it`,
`floorless_fallback_quote_is_refused_as_stale` (`v04/engine_core/src/bid.rs`);
`quote_seized_sets_the_stale_cap_only_with_d4` (`v04/engine_core/src/liquidation.rs`);
`floor_uses_the_capped_max_discount`, `fallback_price_clamped_to_the_floor`,
`fallback_twap_follows_the_d4_conditions` (`v04/risk_core/src/stale.rs`). The Kani harness
`fallback_price_never_breaches_the_floor` is written, not run (ADR-015). The `tier2` stage of
`v04/scripts/e2e.sh` settles an auction with a fallback bid on a standalone sequencer; it passed
locally with a 10 s spacing and again on 2026-11-13 with the 40 s spacing the ring bound needs
(§1); it has not run in CI yet. It exercises the flow, not TWAP manipulation
resistance.

**Why bounded last-good TWAP, and not the two simpler rules:**

| Rule | What goes wrong |
|---|---|
| Refuse every bid (D4 off) | No recovery for the length of the outage. Black Thursday (Maker, March 2020: congestion, failing keepers, zero-bid auctions; `docs/research/research-3-auction-mechanics.md` §3, incident 1) showed that the hours the chain or a feed is degraded are the hours liquidations matter most. |
| Freeze the price seen at auction open | The fixed-`top` problem of §1, plus no `redo` to escape it: the price drifts from the market for as long as the outage lasts. |
| Last good TWAP, bounded by age and floor | Bids keep clearing on the most recent price the engine trusted. The age bound stops it once that price is too old to trust; the floor bounds the worst sale. A TWAP is harder to move with one bad print than a spot reading. |

## 3. Oracle-extractable value

Oracle-extractable value comes from a fixed prize paid to whoever acts first after a price update
(`docs/research/2026-11-07-evm-engines.md` §A15). The engine has two places a prize could sit.

**The bid.** Beyond the schedule's starting discount (a per-type parameter; 2% in `rai_default`),
no bonus is fixed. The discount never goes below its start, so the first bidder after `Liquidate`
always gets at least that much off the feed price; competition does not remove it. Above it, the
discount starts low and rises with time, so bidders compete
it down: the first bidder to whom the current discount is worth it takes the lot, and the protocol
keeps the gap between that discount and the maximum. The rising discount auctions the value back.
Because the price is re-read at each bid, an oracle update does not open a gap between the auction
price and the market either. The residual is oracle lag: a bidder who sees the market move before
the feed does can trade on the difference, within the feed's maximum age and the band.

**The trigger.** Apart from that starting discount, the keeper reward is the only fixed prize: zero
below `debt_floor`, otherwise `min(tip + chip x debt, reward_cap, penalty)` in coin, paid in collateral at the bid price (`crates/auction_core/src/liquidation.rs`
lines 86-93). It is capped by `reward_cap` and by the penalty, so the value a searcher can extract
by triggering first is bounded by those parameters, and they can be set near the cost of the
transaction.

Oracle-level recapture (Chainlink SVR, UMA Oval, API3 OEV Network) needs the oracle or the
sequencer to run an auction for the update. That is outside the engine; the research survey marks
it ADJ (row 23 of its feature table).

## 4. What a liquidation costs

**Measured** (`v04/CYCLES.md`, LEZ `v0.4.0-rc1`, `v04/harness/tests/cycles.rs`): `Liquidate` is 29
executions and 839,357 cycles, under 9% of the 10,000,000-cycle gas cap. A partial bid is 15
executions and 407,700 cycles; a final bid 21 and 576,148. These are the 2026-11-03 figures, which
the breakdown below uses. Re-measured 2026-11-11 after Plan 9, the execution counts are unchanged:
under the legacy preset `Liquidate` is 845,645 cycles. Re-measured 2026-11-12 under the recommended
preset with a full TWAP ring it is 884,567 (partial bid 431,804, final bid 606,596;
`v04/CYCLES.md`, Tier 2 table). Price does not move the TWAP's cost onto the frozen division:
`risk_core::math` has two native routes: `x · y / d` when the product fits a u128, and otherwise `(x / d) · y + (x mod d) · y / d` when that second product fits, and a TWAP term always fits the second. At a price of 2,000,000 `Liquidate` is 888,662
and a partial bid 434,981, inside the same budgets. The final bid is not measured at that price,
and one call (`conf_bps`) can still take one frozen division at a confidence of about 34,000,000 or
more (ADR-070).

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
| Feed observation | 7 | `Liquidate` at the collateral actor (starts the Observe block); `ReadState` to the host and its reply; `ReadState` to the oracle and its reply; `AssertTimestamp`; `Proceed` |
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
custody check 2, cap reservations as separate actors 2) and prices them at the lowest and
highest per-execution averages in the `v04/CYCLES.md` liquidation-slice table and at `Liquidate`'s
own average:

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
