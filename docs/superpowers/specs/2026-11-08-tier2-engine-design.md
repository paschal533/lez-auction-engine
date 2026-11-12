# Tier 2 engine: design

Date: 2026-11-08. Status: designer's decisions (the author handed every choice to the designer).
Parent: `docs/superpowers/specs/2026-11-07-rfp014-competitive-upgrade-design.md` (§2 D4/D6, §5 E1-E9,
§8, §10). Branch: `feat/tier2-engine`, cut from `docs/rfp014-bid-refresh` at `23a57bb`.

Notation. `P` is a price in **collateral per coin**, RAY-scaled, as the engine uses it everywhere:
a bid pays `floor(coin x P / (1 - d))` collateral (`crates/auction_core/src/auction.rs:106-110`) and
the keeper reward is `reward_coin x P` collateral (`crates/auction_core/src/liquidation.rs:93`). A
smaller `P` values the collateral higher and pays out less collateral per coin. `M` is the validated
oracle mid, `c` its confidence (same units), `T` the TWAP, `R` the host's redemption price, `d` the
discount. "Legacy" means v04 as built at `23a57bb`.

## 1. Intent and why

The bid clears every hard requirement only if R2, R5 and F8 stop being "partial". Today:

- **R2 is the weakest row.** No TWAP exists and no price is confidence-adjusted; every producer in
  the repo writes confidence 0, so the confidence gate never fires (`REQUIREMENTS.md:48`). The
  engine prices from one spot reading (`v04/engine_core/src/feed.rs:64-71`).
- **R5 is partial.** A stale or invalid feed refuses every bid (`feed.rs:56-61`), so no debt is
  recovered during an outage, and no test shows non-liquidation operations continuing under a
  pause (`REQUIREMENTS.md:51`).
- **F8 is partial.** Five values are fixed in code (`docs/architecture/f8-parameters.md`, "F8 gaps").

| Item | Scores | Reviewer doubt it answers |
|---|---|---|
| E1 TWAP + confidence edge + spot/TWAP anchor | R2 | "No TWAP, confidence always 0" |
| E2 pause levels + D4 stale-feed bids | R5, R4, F6 | "A pause stops everything; nothing shows bids continue" |
| E3 pinned reading, grace period | R2, R5 | "Bad prints and chain halts" (LUNA `minAnswer`, Aave sentinel) |
| E4 health-aware discount cap | F2, owner fairness | "Barely-unsafe owners sold at full discount" |
| E5 chunked liquidation | F1, R3, P2 | ADR-012 residual: one oversized auction holds the global cap |
| E6 stairstep preset | U6, privacy | "Quotes move while a private bid is proving" |
| E7 per-type deficit ledger, mint bucket | F4, R6 | "Debt auctions can mint without a time bound" |
| E8 delayed loosening | F8, security | "An admin key can loosen safety instantly" |
| E9 five F8 gaps become init parameters | F8 | The audit's "production default — Tier 2" rows |

## 2. Goals, non-goals, anti-goals

Goals
- R2, R5 and F8 move to "PoC" (no "partial") on the v04 engine, each with named tests.
- Every operation is tested at every pause level and under a stale feed.
- Every new value is an init parameter with a bound; no production value lives in on-chain code.
- Per-instruction cost stays bounded by a constant and is asserted in `harness/tests/cycles.rs`.

Non-goals
- Any change to the v0.2.4 engine (D6). Any RFP out-of-scope item (flash liquidations, batching,
  insurance, multi-collateral atomic liquidation).
- An engine-side "is this position unsafe" test. The unsafe decision is the host's (ADR-060,
  `HostMsg::LiquidatePosition`, `v04/programs/mock_host_core/src/lib.rs:58-65`); the engine decides
  only whether a liquidation may start and at what price.
- A guardian role, permissionless breakers that set pause levels, an End/global-settlement path.
- Multi-type configs (ruling R-F3: one collateral type per config, `REQUIREMENTS.md:50`). V5 may lift
  it; §7 says what then changes.

Anti-goals (failure even if it works)
- **Any change to the v0.2.4 guest.** See the hard rule below.
- A break of an existing v04 test or of `v04/scripts/e2e.sh` without a migration note in the same
  plan. The legacy preset (§3.0) exists so existing tests keep passing unchanged.
- Per-bid or per-liquidation cycle growth that depends on history or input size. Every new loop is
  bounded by a protocol limit (`TWAP_CAPACITY = 8`, `MAX_POINTS = 8`). As built (amended in the
  Plan 9 final review): cost is bounded by a constant but **is** price-dependent. `risk_core::math`
  takes a native division only while a product fits a u128, so above a price limit TWAP terms may
  run the frozen 128-step division (§3.1.9). That growth is bounded (at most 8 terms), but it
  depends on the input price; this goal is not met for price magnitude. Open item.
- A claim in the bid not backed by a test, run ID or record. No rival named.

### The frozen-guest rule (ADR-069)

The `rfp014-poc` guest compiles `programs/**`, `crates/auction_core/src/**` and
`crates/auction_engine_params/src/**` (which `#[path]`-includes `programs/auction_engine/core/src/args.rs`
and `error.rs`, `crates/auction_engine_params/src/lib.rs:9-12`) and embeds their paths and line
numbers in panic locations. Tier 2 therefore edits **no file under `programs/`, `crates/auction_core/`,
`crates/auction_engine_params/`, nor the root `Cargo.toml`/`Cargo.lock`** — not behind a cargo feature
either. This supersedes parent spec S2 (feature-gated growth of `auction_core`): a feature does not
move lines, but any edit to those files does, and the guarantee is easier to check as "no diff".

Consequences:
- `EngineError` (`programs/auction_engine/core/src/error.rs:11-61`) cannot gain variants. New codes
  live in a v04 enum (§3.10).
- `CollateralArgs` and `EngineParamsArgs` (`args.rs`) cannot gain fields. New parameters travel in
  v04-only structs appended to v04 messages.
- `validate_feed` (`crates/auction_core/src/price.rs:59-85`, sanity window inline at `:79-80`),
  `effective_ref_price` (`:96-107`) and the constants `MAX_CONF_BPS`/`REF_ORACLE_CLAMP_BPS`
  (`:31-32`) are re-implemented in v04 with parameters, and a differential test proves the v04
  version equals the shared one under legacy parameters.
- New pure logic goes into a new v04-only crate **`v04/risk_core`** (depends on `auction_core` for
  `math`, `units`, `schedule`; no `lee_core`; `borsh` derives behind a `borsh` feature that
  `engine_core` enables). Pure and dependency-light, so Kani harnesses run on it directly.
  Handler glue stays in `v04/engine_core` and the guest.

The check, run by every Tier 2 plan before its last commit and added as a CI step to `guests.yml`:

```bash
git diff --stat 23a57bb -- programs/ crates/auction_core/ crates/auction_engine_params/ Cargo.toml Cargo.lock   # must print nothing
scripts/check-guests.sh                                                                                       # committed guests match the manifest
```

plus the existing `guests.yml` rebuild-and-byte-compare. The builder adds the `git diff` line as
`scripts/check-frozen-v024.sh` (exit 1 on any output).

## 3. Design

### 3.0 Where things live, and the two presets

| Data | Actor | Added as |
|---|---|---|
| Per-type risk parameters, pause level, TWAP ring, feed/grace state, per-type deficit, pending change | collateral actor `collateral_actor(E, config, def)` | `CollateralState.ext: CollateralExt`, appended last |
| Engine-level parameters (E9 reference oracle, relist delay; E7 bucket; E8 delay), system pause level, pending engine change | config `(config, E)` | `ConfigState.ext: ConfigExt`, appended last |
| Mint bucket state | accounting | `AccountingState.ext: AccountingExt`, appended last |
| Stale floor (`stale_cap_ray`) | auction | `AuctionState.stale_cap_ray: u128`, appended last |

Appending keeps every existing field's offset. Borsh does not decode a pre-Tier-2 encoding into the
new layout; no 0.4 state exists outside local runs (D5: no public 0.4 network yet), so the migration
note for each plan is "recreate local state; tags unchanged".

Messages (`v04/engine_core/src/msg.rs`): existing variant indices never move (pinned by
`msg_tests.rs`). Field additions:
- `Init { .., ext: Box<EngineExtArgs> }`, `SetEngineParams { .., ext: Box<EngineExtArgs>, now_ms }`,
  `SetCollateral { .., risk: Box<RiskArgs> }` (boxed for `large_enum_variant = "deny"`).
- `BidCtx` gains `paused_since_ms: Option<u64>` and `fallback: bool`; `LiqCtx`/`Open` gain
  `min_cr_ray: u128` (E4); `Release` gains `shortfall: u128` (E7);
  `HostMsg::LiquidatePosition` gains `max_debt: u128, min_remainder: u128` (E5, mock host only).
- New variants, appended from 47: `SetPauseLevel { salt, collateral_def, level, now_ms }` (47),
  `PauseLevelUpdate { level }` (48), `ApplyPending { config, now_ms }` (49), `CancelPending { salt }` (50).

**Presets (ADR-082).** F8 forbids production values in on-chain code, so the engine has no defaults:
every value arrives in a message and is validated against a bound. Two named presets live in
`v04/sdk` (`RiskArgs::legacy()`, `RiskArgs::recommended(..)`, same for `EngineExtArgs`) and are
re-exported for the harness:
- **legacy** reproduces v04 as built: TWAP off, D4 off, breakers off, today's five constants. The
  harness `World` uses it by default, so every existing test runs unchanged. One caveat: the
  confidence edge (§3.1.4) is always on; with confidence 0, as every existing producer writes, it is
  the identity.
- **recommended** is the production proposal documented in `f8-parameters.md`. New tests and the
  new e2e stage use it (or a time-compressed copy).

As built (Plan 9, ruling R9-1; amended in Plan 9 Task 14): the presets are inherent methods in
`risk_core::presets` (`v04/risk_core/src/presets.rs`) behind the cargo feature `presets`, which
`sdk` and `harness` (and `engine_core`'s dev-dependency) enable and the guest does not; the SDK
re-exports `RiskArgs`. `recommended()` takes no argument in Plan 9. There is no `EngineExtArgs`
yet: it arrives with E9 in Plan 10.

### 3.1 E1 — TWAP and confidence-adjusted pricing (R2)

#### 3.1.1 Data

```rust
pub const TWAP_CAPACITY: usize = 8;               // protocol limit, like MAX_POINTS
pub struct Sample { pub ts_ms: u64, pub price_ray: u128 }   // validated mid M (RAY), oracle publish time
pub struct TwapRing { pub samples: Vec<Sample>, pub last_conf_bps: u16 }  // oldest first, len <= 8
```

`TwapRing` lives in `CollateralExt`. It is written by the Observe block's `proceed`
(`v04/programs/methods/guest/src/bin/engine/observe.rs:168-220`), which every `SetCollateral`,
`ObserveFeed`, `Liquidate` and `Bid` already runs; no new message or execution. As built (amended
in Plan 9 Task 14), `SetPauseLevel`'s trailing `Observe` runs the same block, so it writes the ring
too (`pause.rs:68`, `observe.rs:171-191`). Size: 4 + 8x24 + 2 =
198 bytes. The copy of the collateral state that rides in `LiqCtx`/`Open` and the auction's
`QuoteCtx` scratch (`v04/engine_core/src/scratch.rs:86-92`) is stripped of the ring
(`CollateralState::for_quote()`); nothing downstream reads it.

#### 3.1.2 Recording

On each observation whose reading passes `validate_feed_ext` (§3.1.5):

```
ts = min(reading.timestamp_ms, now_ms)                 // same clamp as pause.rs:316
if newest exists and ts <= newest.ts            -> no change (same publish seen again)
if newest exists and ts - newest.ts < spacing   -> no change
push Sample{ts, M}; if len > 8 drop oldest; last_conf_bps = ceil(c * 10_000 / M)
```

An invalid reading is never recorded. A reverted transaction drops its sample, as it drops its pause
observation (ADR-032). Anyone can observe, but only the oracle publisher sets the values. The
spacing rule caps recording at one sample per `twap_min_spacing_ms`, but it does not make extra
observations harmless. As built (amended in the Plan 9 final review): the ring holds 8 samples, so
when `twap_window_ms > 8 · twap_min_spacing_ms`, one permissionless `ObserveFeed` per spacing
interval evicts older samples, and the TWAP spans only about the last 7-8 spacing intervals, not
its window. `RiskArgs::validate` does not refuse such settings yet (ADR-070). The sample time is
the caller's claimed `now`, so it can sit up to `slack_ms` before the oracle's publish time; with
`spacing <= slack_ms` one publication can be recorded twice.

#### 3.1.3 TWAP

`twap(ring, now, window_ms, min_obs, max_age_ms) -> Option<(T, newest_ts)>`:

```
n      = newest sample
anchor = min(now, n.ts + max_age_ms)          // live: now; stale: frozen at the moment n went stale
lo     = anchor - window_ms (saturating)
end = anchor; Σw = 0; terms = []
for s in samples, newest first:
    if s is not n and end - s.ts > max_age_ms: break      // a gap longer than the feed's max age breaks the TWAP
    start = max(s.ts, lo)
    if end > start: terms.push((s.price, end - start)); Σw += end - start
    if s.ts <= lo: break
    end = s.ts
if terms.len() < min_obs or Σw == 0: None
T = Σ mul_div_floor(p_i, w_i, Σw)             // each term <= p_i; floor error < 8 units of 1e-27
```

Each price is weighted by how long it was the current publication, clipped to the window. Gaps:
a stretch between two publications longer than `max_oracle_age_ms` is a stale period; samples
before it are excluded, so the TWAP restarts after an outage and needs `min_obs` fresh samples. No
new gap parameter: the feed's own maximum age is the definition of "too long". The anchor makes the
last-good TWAP constant for the whole of an outage (it does not slide the window over nothing).

Bounds: `T` lies within `[min p_i − 7, max p_i]` of the used samples, the lower end saturating at
0 (proptest `twap_within_sample_range`; the Kani harness `twap_bounded_by_samples` is written, not
run; amended in Plan 9 Task 14). The upper bound is exact. The lower one is not `min p_i`: each of the at most 8 terms
floors away less than one unit, so `T` can sit up to 7 below the smallest price (prices 1 and 1
with weights 1 and 2 give `T = 0`). Cost: at most 8 `mul_div_floor` calls. As built (Plan 9 Task 11,
ADR-070) that is `risk_core::math::mul_div_floor`, not the frozen `auction_core::math` one: it gives
exactly `auction_core`'s results but divides natively when `x · y` fits a u128, and calls the frozen
128-step bit-serial division only otherwise, because that division costs about 10,000 guest cycles
a call. The same wrapper serves `validate_feed_ext`'s confidence limit and sanity bounds,
`conf_bps`, `edge`, the TWAP anchor, `within_deviation`, the stale cap and the floored price. Two
steps still call the frozen `auction_core::math::mul_div_floor`: `band`, which is
`auction_core::price::bid_price` (two calls, for `R·lo` and `R·hi`) and ends every price in
§3.1.4, and `auction_core::price::to_ray` for a Q64x64 reading (a Ray reading needs no division).

#### 3.1.4 The confidence edge, and the price per operation (ADR-071)

The engine never decides whether a position is unsafe; every price it uses decides **how much
collateral leaves per coin** (bid payout, keeper reward, floor, discount cap). So one rule covers
every operation: **value the collateral at the high edge**, i.e. use the smaller `P`.

Let `edge(x) = x * (10_000 - conf_bps) / 10_000` (floor), `conf_bps = ceil(c * 10_000 / M)`
(already `<= max_conf_bps <= 1_000` or the feed is invalid), `band(x) = clamp(x, R*lo, R*hi)`
(`bid_price`, `price.rs:89-93`), `dev = max_spot_twap_dev_bps`.

| Operation | Price | Why |
|---|---|---|
| Liquidation (`P_liq`, used for the keeper reward, E4 and the floor) | `band(edge(min(M, T)))` | Smallest `P` of spot and TWAP: a one-print spike in `P` cannot inflate the reward or the floor |
| Bid, live feed | `band(min(edge(M), T * (10_000 + dev) / 10_000))` | Follows the live market (ADR-067) but is anchored from above: a manipulated spot cannot pay out more than `dev` beyond the TWAP. Not anchored from below, which would pay out more |
| Bid, stale feed (D4) | `band(edge_lg(T_lg))`, then the floor (§3.2.4) | Last trusted data; `edge_lg` uses `last_conf_bps` |
| TWAP disabled (`window = 0`) | as legacy, with `edge` applied | Legacy behaviour when `c = 0` |

Why the bid is not priced at `min(M, T)`: in a fast fall of the collateral, `T` lags below `M` and
bids would get less than the market until the TWAP catches up — the Black Thursday "no bids"
failure. The one-sided anchor caps manipulation profit at `dev` and leaves liveness to the rising
discount.

Liquidation gate (in this order, at the collateral actor's `proceed`, `observe.rs:202-218`):
level (§3.2) → feed valid → grace (§3.3) → `T` available (else `TwapUnavailable`) →
`|M - T| * 10_000 <= dev * T` (exact, inclusive; else `SpotTwapDeviation`) → `P_liq`. With the TWAP off
(`twap_window_ms = 0`) the two TWAP steps are skipped and `P_liq = band(edge(M))`. The deviation refusal is the
ClipperMom-style breaker (`docs/research/2026-11-07-evm-engines.md` row 1); it is built in Plan 9
with E1 because the bid anchor needs the same parameter (dependency, §5).

#### 3.1.5 `validate_feed_ext`

`risk_core::feed::validate_feed_ext(reading, policy, R, age_now) -> Result<u128, FeedFault>` repeats
`validate_feed`'s order (age, scale, zero, confidence, range) with the sanity window as parameters
(E9) and adds the pinned check (E3). `FeedFault` maps one-to-one to `PauseReason` plus `Pinned`.
Differential test: for every input and the legacy policy (`sanity 5_000/20_000`, pins off) it returns
exactly what `auction_core::price::validate_feed` returns (proptest, plus a Kani harness on bounded
inputs). `v04/engine_core/src/feed.rs::observe` switches to it.

#### 3.1.6 Parameters (per type, `RiskArgs`)

| Field | Legacy | Recommended | Bound (else `InvalidConfig`) |
|---|---|---|---|
| `twap_window_ms` | 0 (off) | 1_800_000 (30 min) | 0 or 60_000..=86_400_000 |
| `twap_min_spacing_ms` | 0 | 300_000 (5 min) | if window > 0: 1_000..=window |
| `twap_min_obs` | 0 | 3 | if window > 0: 1..=8 |
| `max_spot_twap_dev_bps` | 0 (off) | 2_000 (Compound's ±20 % anchor) | 0 or 100..=5_000; needs window > 0 |

With 5-minute spacing a 30-minute window holds up to 6 samples, inside the 8-slot ring.

#### 3.1.7 Errors

`TwapUnavailable` (6070), `SpotTwapDeviation` (6071). Feed faults keep their v0.2.4 codes
(6021-6023).

#### 3.1.8 Tests

`risk_core`: `twap_single_sample_is_its_price`, `twap_weights_by_time_current`,
`twap_ignores_samples_before_the_window`, `twap_partly_weights_the_sample_crossing_the_window`,
`twap_gap_longer_than_max_age_drops_older_samples`, `twap_below_min_obs_is_unavailable`,
`twap_is_constant_during_an_outage`, `record_skips_same_publish_and_close_spacing`,
`record_clamps_a_future_timestamp_to_now`, `ring_drops_the_oldest_past_capacity`,
`edge_lowers_p_by_the_confidence`, `liq_price_is_the_smaller_of_spot_and_twap_at_the_edge`,
`bid_price_is_anchored_from_above_only`, proptests `twap_within_sample_range`,
`validate_feed_ext_matches_v024_under_legacy_policy`; Kani `twap_bounded_by_samples`,
`validate_feed_ext_equals_validate_feed_legacy`.
Harness (`v04/harness/tests/twap.rs`, new): `liquidate_refused_until_twap_has_min_obs`,
`keeper_reward_uses_the_smaller_of_spot_and_twap`, `a_spot_spike_is_refused_by_the_deviation_breaker`,
`bid_price_takes_the_confidence_edge` (oracle publishes nonzero confidence),
`bid_payout_capped_by_the_twap_anchor`, `invalid_reading_is_not_recorded`,
`observe_feed_fills_the_ring`. Existing `feed_age_counts_the_slack`, `live_price_clamps_to_the_band`,
`out_of_band_and_unknown_scale_fail_as_in_v024` (`feed.rs:96-166`) keep passing under legacy.

#### 3.1.9 Cycles

No new executions. Added work per observing instruction: ring decode/encode at the collateral actor
on 4 messages (root, two replies, `Proceed`) and 2 more on a final bid's `Release`, ~200 bytes each;
TWAP at most 8 `mul_div_floor`; edge and anchor 3 more. Estimate for E1 alone against `v04/CYCLES.md`:
`Bid (partial)` 407,700 → ~460,000-490,000; `Bid (final)` 576,148 → ~640,000-680,000;
`Liquidate` 839,357 → ~900,000-960,000 (ring stripped from `QuoteCtx`); `ObserveFeed` 176,218 →
~200,000. Every row stays under 10 % of the 10,000,000 cap. Budget assertions added to
`cycles.rs`: partial bid <= 520,000, final bid <= 720,000, `Liquidate` <= 1,000,000; and
`bid_cost_is_independent_of_ring_fill` (1 sample vs 8 samples differ by <= 40,000). The estimates are
replaced by measurements in `v04/CYCLES.md` at the end of Plan 9.

Measured (Plan 9, 2026-11-11, `RiskArgs::recommended()` with a full ring, at the price 0.5):
`Bid (partial)` 431,669, `Bid (final)` 606,461, `Liquidate` 883,636, `ObserveFeed` 181,947,
ring-fill spread 16,312; every budget holds at that price. These are fast-path figures (amended in
the Plan 9 final review). A TWAP term `p · w` (RAY price times a weight of at most
`min(twap_window_ms, max_oracle_age_ms)` ms) is guaranteed to fit a u128 while
`p < 2^128 / min(window, max_age)`: the price below which every term takes the fast path. In
collateral base units per coin base unit that is about 226,855 for the recommended preset at its
largest accepted max age (1,499,999 ms). It is about 189,045 for a 1,800,000 ms window when the max
age is at least the window, and about 3,938 at the 24 h maximum window and max age. Above the
limit, only a term whose own `p · w` reaches 2^128 may take the frozen division: at most 8 per
`Liquidate` or `Bid`. Every other multiply-divide stays native up to far higher prices (the sanity
bound to a redemption price of about 17,000,000, the anchor to about 23,000,000, `edge` to about
34,000,000). Slow-path rows have **not been measured**. Whether the rows stay inside their
assertions just above the limit has not been established by measurement. The pre-change
measurement with every multiply-divide on the frozen path (`Liquidate` 1,012,946, `Bid (partial)`
561,253, `Bid (final)` 736,045, `SetPauseLevel` 407,582; ADR-070) is a loose upper bound,
approached only at prices far above the limit where the remaining multiply-divides also switch.
`SetPauseLevel` computes no TWAP. The budgets and "under 10 % of the cap" are established on the
fast path only. Open item, not decided: a cheaper exact wide division in `risk_core::math`, or slow-path budgets. The estimate above priced each `mul_div_floor` as cheap; the frozen
`auction_core::math` version is not (about 10,000 cycles a call), and with it the budgets failed,
so the TWAP terms, the confidence edge, the anchor, the deviation test and the stale cap go through
`risk_core::math`; the band clamp (`bid_price`) and the Q64x64 `to_ray` still call the frozen
function (§3.1.3, ADR-070).

### 3.2 E2 — Pause levels and D4 (R5)

#### 3.2.1 Levels

| Level | Meaning |
|---|---|
| 0 | Normal |
| 1 | No new liquidations of this type |
| 2 | Level 1, and no new system auctions (`StartSurplus`, `StartDebt`) |
| 3 | Level 2, and no bids on this type's auctions and no system takes |

No level moves collateral or cancels an auction (no `yank`); every level is reversible. Data:
`CollateralExt.pause_level: u8`; `ConfigExt.system_level: u8`, which the config writes from the same
`SetPauseLevel` (with one type per config it equals that type's level; system auctions are
engine-wide, so a type at level 2 holds the engine's recovery path).

The legacy flag `admin_paused` (`CollateralArgs`, locked in `args.rs`) stays and means level 3 **for
that type's collateral operations only**, with its v0.2.4 code `Paused` (6020), so the existing
tests (`admin_pause_toggle_runs_observe`, `stale_feed_pauses_and_reverts_liquidate` in
`v04/harness/tests/liquidate.rs`, and `admin_pause_starts_the_clock_at_now_and_refuses_liquidation`
in `v04/engine_core/src/feed.rs`) keep their codes. It does not raise `system_level`. Effective
collateral level: `max(pause_level, if admin_paused {3} else {0})`.

Discount clock: only an effective level 3 or an invalid feed stops it. `feed::observe`
(`feed.rs:40-49`) passes `admin_paused: effective_level == 3` into the pause clock; levels 1 and 2
leave live auctions and their discount running.

`SetPauseLevel` (47): root at the admin's proxy; seeded `Probe { Config }`; the config checks the
admin, writes `system_level`, sends `Probe { Collateral }` + `PauseLevelUpdate` (48) to its recorded
type, which writes `pause_level`, then `Observe { now_ms }` so the pause clock records the change
(as `SetCollateral` does, `admin.rs:488-525`). Raising is instant; lowering is a loosening and goes
through E8 once Plan 12 lands (until then, instant).

#### 3.2.2 Operation x pause-level matrix

`✓` runs; `✗ n` refused with wire code `n`. "Stale-ok" is a stale or invalid feed where D4's
conditions hold; "stale-no" where they do not. Grace is E3. Risk-reducing operations always run.

| Operation | Class | L0 | L1 | L2 | L3 | Stale-ok | Stale-no | Grace |
|---|---|---|---|---|---|---|---|---|
| `Liquidate` | opens risk | ✓ | ✗ 6073 | ✗ 6073 | ✗ 6073 (6020 if `admin_paused`) | ✗ feed code | ✗ feed code | ✗ 6072 |
| `Bid` | fill | ✓ | ✓ | ✓ | ✗ 6075 (6020 if `admin_paused`) | ✓ at last-good TWAP, floored | ✗ 6070 / 6077 / feed code | ✓ |
| `StartSurplus`, `StartDebt` | opens system auction | ✓ | ✓ | ✗ 6074 | ✗ 6074 | ✓ | ✓ | ✓ |
| `TakeSurplus`, `TakeDebt` | system fill | ✓ | ✓ | ✓ | ✗ 6075 | ✓ | ✓ | ✓ |
| `SettleEmpty` | risk-reducing | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `ClaimLeftover` | risk-reducing (owner) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `SettleDebt` | risk-reducing | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `CheckSolvency` (may freeze) | risk-reducing | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `CloseSurplus`, `CloseDebt` | risk-reducing (releases) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `ObserveFeed` | observation | ✓ | ✓ | ✓ | ✓ | ✓ records | ✓ records | ✓ |
| `Init`, `SetCollateral`, `SetEngineParams`, `SetSlack`, `SetPauseLevel`, `CancelPending` | admin | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `ApplyPending` | admin effect, permissionless | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Host owner actions (open, add collateral, repay) | owner | not gated by the engine; `GenerateDebt` only by the F4 freeze |

System columns "Stale-ok/no" refer to the collateral feed, which system operations do not read; the
reference oracle keeps its fall-back-to-admin-price rule (`v04/engine_core/src/system.rs:36-58`).
Where checked: `Liquidate` and `Bid` at the collateral actor's `proceed`; system starts and takes at
the config `K` before it relays to the accounting or the auction. Rule table lives in
`risk_core::pause::allowed(op, level, feed) -> Result<Gate, RiskError>` (as built, Plan 9 rulings
R9-7 and R9-8; `grace` and 6072 arrive with E3 in Plan 10). `Gate` is `Run`, `FeedRefusal` (a
liquidation on a stale feed: the feed's own code) or `Fallback` (a bid on a stale feed: D4
decides); `engine_core::gate` applies the feed codes and `Paused`, which `risk_core` cannot name.
The gated handlers call it: `Liquidate` and `Bid` at the collateral actor, and at the config the
system starts and takes, `CloseSurplus`, `CloseDebt`, `SettleDebt` and `CheckSolvency` (which it
always lets run). The other operations always run and do not call it. The matrix test derives its
expected outcomes from the same table. The pause gate comes before any feed code and, at the
config, before `TokenNotConfigured`.

#### 3.2.3 D4: bids on a stale feed (ADR-074)

A bid on a type whose reading fails `validate_feed_ext` proceeds iff all hold:
1. effective level <= 2. This condition never decides a refusal on its own: the level is checked
   first (§3.2.2), so at level 3 the bid is refused `BidsPaused` (6075), or `Paused` (6020) under
   `admin_paused`, before D4 is reached (amended in Plan 9 Task 14);
2. `stale_bid_max_age_ms > 0` and `twap_window_ms > 0`;
3. `twap(ring, now, ..)` is available with `min_obs` samples (else `TwapUnavailable`, 6070);
4. `now - newest.ts <= stale_bid_max_age_ms` (else `LastGoodTwapTooOld`, 6077).
Otherwise it fails with the feed's own code (6021-6023, 6076), as today.
As built (amended in Plan 9 Task 14): an auction opened while D4 was off stored `stale_cap_ray = 0`
(§3.2.4), so a fallback bid on it is refused with `PriceFeedStale` (6021) in `engine_core::bid`
(`bid.rs:107-113`) instead of being sold unfloored; `BidCtx` does not carry the exact fault.

Price: `P_fb = band(edge_lg(T_lg))` (R is the host's current redemption price, read in the same
observation). The collateral actor sends `BidCtx { fallback: true, paused_since_ms, .. }`. The pause
clock is open (the feed is invalid), so the discount stays frozen: `bid::quote`
(`v04/engine_core/src/bid.rs:53-83`) builds its `PauseClock` from `paused_since_ms` instead of the
fixed `None` it uses today (`bid.rs:55-59`); live bids pass `None`, so their result is unchanged.
Freezing is the conservative choice: an outage never hands a larger discount, and the floor bounds
the worst sale.

#### 3.2.4 The floor, precisely

At `Seized` the auction stores
`stale_cap_ray = floor(P_liq * RAY / (RAY - d_max_eff))`, where `d_max_eff` is the last point of the
auction's snapshotted (E4-capped) schedule. As built (amended in Plan 9 Task 14): E4 is not built,
so `d_max_eff` is the snapshotted schedule's `Schedule::max_discount_ray` (its last point), and the
cap is computed only while D4 is on (else 0). A fallback bid uses
`P_used = min(P_fb, floor(stale_cap_ray * (RAY - d) / RAY))` with `d` the bid's (frozen) discount.

In words: **during a stale feed the auction never sells collateral for less coin per unit than
`(1 - d_max_eff) / P_liq` — the worst price its own schedule would have reached at its opening
price.** The bid is clamped to the floor, not refused, so a bidder still gets a fill at exactly the
floor price; `min_collateral_out` protects the bidder. Live bids ignore the floor (ADR-067: live
pricing follows the market).

Parameter (`RiskArgs`): `stale_bid_max_age_ms` — legacy 0 (D4 off); recommended 3_600_000 (1 h);
bound `0..=MAX_ORACLE_AGE_MS` (24 h), and `> 0` only if `twap_window_ms > 0` and
`twap_window_ms > max_oracle_age_ms + (twap_min_obs − 2) · twap_min_spacing_ms` (saturating,
`twap_min_obs − 2` floors at 0; refused as `InvalidConfig` at `SetCollateral`). Reason: on a stale
feed the TWAP anchor freezes at `newest.ts + max_oracle_age_ms` (§3.1.3), and the `k`-th newest
sample is at most `newest.ts − (k − 1) · spacing`. A shorter window can never hold `min_obs`
samples, so condition 3 of §3.2.3 would always fail and D4 could never fire. (Added by the Plan 9
pre-flight, with the plan's ruling R9-3.)

#### 3.2.5 Errors

`LiquidationsPaused` (6073), `SystemAuctionsPaused` (6074), `BidsPaused` (6075),
`LastGoodTwapTooOld` (6077). Out-of-range level: `InvalidConfig` (6005).

#### 3.2.6 Tests

`risk_core`: `pause_table_is_exhaustive` (every `Op` x level x feed has an entry, compile-time
match; the grace column arrives with E3 in Plan 10, as-built amendment, Plan 9 Task 14),
`risk_reducing_ops_allowed_everywhere`, `fallback_price_clamped_to_the_floor`,
`floor_uses_the_capped_max_discount`; Kani `fallback_price_never_breaches_the_floor` (written, not
run).
Harness (`v04/harness/tests/pause_levels.rs`, new). As built (ruling R9-12; amended in Plan 9 Task
14): the two matrix tests are in `v04/harness/tests/pause_matrix.rs` and the D4 tests
(`stale_feed_bid_*`, `discount_stays_frozen_across_fallback_bids`,
`fallback_final_bid_settles_and_releases`) in `v04/harness/tests/stale_bids.rs`; the level tests
stay in `pause_levels.rs`:
- `every_operation_at_every_pause_level` — table-driven over the 17 operations of §3.2.2 x levels
  0-3; asserts the wire code and, for every refusal, that no actor state and no holding changed;
- `every_operation_with_a_stale_feed` — the same over Stale-ok and Stale-no;
- `stale_feed_bid_uses_the_last_good_twap`, `stale_feed_bid_refused_past_max_age`,
  `stale_feed_bid_refused_without_min_obs`, `stale_feed_bid_held_at_the_floor`,
  `discount_stays_frozen_across_fallback_bids`, `fallback_final_bid_settles_and_releases`;
- `admin_paused_keeps_the_v024_code`, `admin_paused_does_not_stop_system_auctions`,
  `level_2_blocks_starts_not_takes_or_closes`, `lowering_the_level_resumes_liquidation`,
  `set_pause_level_only_through_the_admin_proxy`, `level_change_records_the_pause_clock`.
Existing `a_stale_feed_stops_the_bid` (`v04/harness/tests/bid.rs:331`) keeps passing under legacy
(D4 off); `stale_feed_pauses_and_reverts_liquidate` (`liquidate.rs:177`) unchanged.

#### 3.2.7 Cycles

Level checks are a byte compare. A fallback bid skips nothing and adds one `mul_div_floor` at the
auction: within the E1 bid estimate. New row `SetPauseLevel`: ~12 executions (proxy, 2 probes,
config, `PauseLevelUpdate`, the 7-execution Observe block), ~330,000 cycles (comparable to
`SetCollateral`, 11 / 325,765).

Measured (Plan 9, recommended preset, full ring): `SetPauseLevel` is 12 executions (hand-counted)
and 360,079 cycles, 3.6 % of the cap, asserted `<= 400,000`. This is a recorded deviation from the
estimate and from §4's "< 3 % of cap" (Plan 9 ruling R9-10, ADR-070): the row carries the whole
7-execution Observe block (`ObserveFeed` alone is 181,947), and `SetCollateral`, at 11 executions,
measures 359,048, so 300,000 is not reachable without restructuring the message.

### 3.3 E3 — Pinned readings and the grace period (R2, R5)

The deviation breaker is in §3.1.4.

**Pinned reading.** `RiskArgs.feed_min_raw`, `feed_max_raw` (raw oracle units, the feed's own
answer bounds, LUNA `minAnswer`). A reading with `feed_min_raw > 0 && price_raw <= feed_min_raw` or
`feed_max_raw < u128::MAX && price_raw >= feed_max_raw` is invalid with fault `Pinned`: it pauses
the type like a stale feed, is not recorded, and lets D4 apply. Legacy and recommended: `0` /
`u128::MAX` (off) — the bounds are the oracle's, so the deployment sets them; the SDK refuses to
build a recommended config for an oracle that declares bounds without them. Bound:
`feed_min_raw < feed_max_raw`.

**Grace period (Aave PriceOracleSentinel; Ajna lesson).** State: `CollateralExt.feed_invalid_since_ms:
Option<u64>`, `grace_until_ms: u64`. On a valid observation at `now`:

```
gap = match feed_invalid_since { Some(s) => now - s,                                   // observed outage
                                 None    => ts - (last_good_ts + max_oracle_age_ms) }  // unobserved, saturating
if gap >= grace_min_gap_ms and grace_ms > 0: grace_until = max(grace_until, now + grace_ms)
feed_invalid_since = None
```

and on an invalid one `feed_invalid_since.get_or_insert(min(last_good_ts + max_age, now))` (the
derived onset of `pause.rs:335-339`). The oracle publishes by transaction, so a sequencer halt shows
up as a publish gap; one rule covers feed and chain outages. Grace refuses **`Liquidate` only**
(`GracePeriod`, 6072); bids, settles and system operations run. A gap that nobody observed but the
oracle filled can start a spurious grace (the engine sees only the latest publication); the cost is
liquidation latency only, the same over-pause policy (b) already accepts (ADR-032), and the K1 keeper
observes every `twap_min_spacing_ms`.

| Field | Legacy | Recommended | Bound |
|---|---|---|---|
| `feed_min_raw`, `feed_max_raw` | 0, `u128::MAX` | oracle's bounds | min < max |
| `grace_ms` | 0 (off) | 1_800_000 (30 min) | `0..=86_400_000` |
| `grace_min_gap_ms` | 0 | 600_000 (10 min) | `0..=86_400_000` |

Errors: `GracePeriod` (6072), `PriceFeedPinned` (6076).
Tests: `pinned_low_and_high_readings_pause_the_type`, `pinned_reading_is_not_recorded`,
`grace_blocks_liquidate_not_bid` (harness: a live auction is bid during grace),
`grace_needs_a_gap_of_at_least_min`, `unobserved_gap_starts_grace`, `grace_expires_at_its_end`,
`grace_refusal_moves_nothing`; incident replays for V4 reuse these (LUNA pinned floor, Compound
single +30 % print → `SpotTwapDeviation`).
Cycles: a few compares and two `u64` fields; within the E1 estimate.

### 3.4 E4 — Health-aware discount cap (F2)

At `Seized` (`quote_seized`, `v04/engine_core/src/liquidation.rs:76-110`) the engine knows the
reported collateral `C` and debt `D`; the observation adds the host's `min_cr_ray` (already in the
host `Params` the Observe block reads, `mock_host_core/src/lib.rs:122`; carried in
`PendingObserve` → `LiqCtx` → `Open` → `QuoteCtx`). Health at `P_liq`:

```
value = C * RAY / P_liq                      // coin
h     = value * RAY / ceil(D * min_cr / RAY) // RAY; h < RAY: under-collateralised at the engine price
under = if h < RAY { (RAY - h) * 10_000 / RAY } else { 0 }       // bps
d_cap = min(sched_max, max(health_floor_bps, sched_0, under * health_slope_bps / 10_000))
```

The snapshotted schedule becomes `points.map(|p| min(p.discount_bps, d_cap))`, still a valid
non-decreasing schedule whose first point is unchanged (`d_cap >= sched_0`). The cap is written once;
`quote_bid` and every bid are untouched, so the per-bid cost is zero. A barely-unsafe owner
(`h` near 1) is sold at no more than `health_floor_bps`; a deeply under-water position keeps the
full schedule and clears (Euler reverse Dutch, Kamino solvency cap).

The floor matters for liveness: there is no expiry (ADR-012), so a cap too small could leave a lot
unsold forever while it holds its reservation. Live pricing makes `health_floor_bps` attractive
whatever the market does inside the band; the recommended 500 bps exceeds the 2 % starting
discount of `rai_default` (`crates/auction_core/src/schedule.rs:81-93`).

| Field | Legacy | Recommended | Bound |
|---|---|---|---|
| `health_slope_bps` | 0 (off: no cap) | 10_000 (1:1) | `0..=50_000` |
| `health_floor_bps` | 0 | 500 | `<= MAX_DISCOUNT_BPS` (3_000) |

**Finding (blocks E4 tests, fix first in Plan 11).** The mock host judges `C * price < min_cr * D`
(`v04/programs/mock_host_core/src/lib.rs:212-219`), i.e. it reads the oracle as coin per collateral,
while the engine reads the same number as collateral per coin (see Notation) and the v0.2.4 host
judges `C >= D * R * ratio` with `R` in collateral per coin (`programs/stablecoin/src/checks.rs:15-29`).
In the harness scenario (price 0.5, `MIN_CR_RAY = 1.5`, `v04/harness/src/world.rs:27`) the mock says
unsafe while the engine's convention gives a collateral ratio of 1.67. E4 uses the engine's
convention; Plan 11 first changes the mock host's predicate to collateral per coin, unsafe iff
`C * RAY^2 < D * min_cr_ray * P` (compared as exact 256-bit products), adjusts the harness
scenario's price so its positions stay unsafe, and records it in the host-interface note (ADR-078).

Errors: none new. Tests: `barely_unsafe_owner_capped_at_the_floor`,
`deep_underwater_keeps_the_full_schedule`, `cap_never_below_the_start_discount`,
`capped_schedule_validates`, `safe_at_engine_price_gets_the_floor`; Kani
`health_cap_within_floor_and_schedule`; harness `health_cap_is_snapshotted_at_seize`,
`capped_auction_still_clears_at_the_floor`. Cycles: ~4 `mul_div` on `Liquidate` (~10,000).

### 3.5 E5 — Chunked partial liquidation (F1, R3)

Today one auction books the whole position, and the "alone" rule lets an oversized one take the
whole cap (`crates/auction_core/src/liquidation.rs:78-83`, ADR-012 residual). E5 is Maker `Dog.bark`
room fitting.

- `RiskArgs.max_liquidation_debt: u128` (0 = unlimited).
- At `LiqCtx` the config (`v04/programs/methods/guest/src/bin/engine/liquidate.rs:42-93`) computes
  `room = min(type_cap - type_reserved, global_cap - global_reserved)`, or `min(type_cap, global_cap)`
  when nothing is reserved; `room_debt = floor(room * 10_000 / (10_000 + penalty_bps))`;
  `max_debt = min(max_liquidation_debt or ∞, room_debt)`. If `max_debt < debt_floor` and something
  is reserved: `CapacityExhausted` (6033) at the config, before the host is asked.
- `LiquidatePosition { .., max_debt, min_remainder: debt_floor }`. The host: if
  `D <= max_debt` or `D - max_debt < min_remainder` → seize everything (today); else
  `debt_part = max_debt`, `coll_part = floor(C * max_debt / D)` (rounds toward the owner), the
  position keeps the rest, and `Seized` reports `(coll_part, debt_part)`. The engine's quote already
  books the reported amounts (ruling R-5e), so `quote_seized` is unchanged.
- The rest of the position keeps its collateral ratio, stays unsafe and is liquidated again under
  the next nonce (the per-position record already counts auctions, ADR-004). Each chunk is at least
  `debt_floor`, so it is a real auction and earns at most one capped reward.
- The full-on-dust case may exceed the room; the quote's cap check then refuses it unless alone,
  as today.

Host interface: this is v3 of ADR-060's interface (`max_debt`, `min_remainder` on
`LiquidatePosition`). The mock host in `v04/programs` implements it; for the upstream host it goes in
the dependency table and the upstream proposal. `max_debt = u128::MAX` reproduces v2.

| Field | Legacy | Recommended | Bound |
|---|---|---|---|
| `max_liquidation_debt` | 0 | SDK preset: `floor(type_cap * 10_000 / (10_000 + penalty) / 2)` | 0 or `>= debt_floor` |

Errors: none new. Tests: host `partial_seize_is_proportional`, `dust_remainder_seizes_fully`,
`max_debt_max_reproduces_full_seizure`; `risk_core` proptest
`any_unsafe_position_partly_liquidatable_when_room_at_least_chost`; harness
`oversized_position_is_liquidated_in_chunks`, `chunk_fits_the_free_room`,
`chunks_use_increasing_nonces`, `split_liquidation_neutrality` (sum of chunk outcomes vs one full
liquidation at a constant price, equal up to per-chunk rounding), `capacity_refusal_moves_nothing`;
the existing `liquidate_opens_an_auction_and_pays_the_keeper` keeps passing (legacy).
Cycles: two `mul_div` at the config, one at the host; no new executions (~8,000 on `Liquidate`).

### 3.6 E6 — Stairstep preset for private bidding (U6)

The shared schedule is piecewise-linear (`Schedule::discount_ray`, `schedule.rs:56-75`) and holds its
last point (`:58-61`); it cannot change. A stair is built inside it with 1 ms risers:

```
stairstep(start_bps, rise_bps, levels <= 4, step_ms) =
  [(0, s0), (step-1, s0), (step, s1), (2*step-1, s1), (2*step, s2), (3*step-1, s2), (3*step, s3)]
  with s_k = start_bps + k * rise_bps, at most 7 points (MAX_POINTS = 8)
```

Within a step the discount, and so a bid's quote at a fixed `M`, is constant: a private bidder
whose proof takes longer than a ramp would move the quote gets the same discount wherever its
`now_ms` lands in the step. Measured proxy for private proving: a non-final bid's three executions
proved in 938.2 s (po2 19, `docs/design-log.md:811`); the private path (ADR-034) proves the same
executions. Preset `stairstep_private()` = `stairstep(200, 250, 4, 1_800_000)` (2 % → 9.5 %, 30-min
steps). The preset lives in `risk_core::presets` and the SDK; nothing on chain changes.

Tests: `stairstep_is_a_valid_schedule`, `stairstep_quote_constant_within_a_step`,
`stairstep_step_exceeds_measured_private_proving_time` (asserts
`step_ms > 1.5 * PRIVATE_BID_PROVE_MS + MAX_SLACK_MS` = 1,407,300 + 300,000; the constant cites
design-log:811 and is re-measured in M4); harness `stairstep_auction_quote_survives_a_proving_window`.
Documented in `pricing-model.md` as the recommended schedule for privacy-heavy deployments.
Cycles: none.

### 3.7 E7 — Per-type deficit ledger and the mint bucket (F4, R6)

**The global ledger already exists.** `record_chain_end` books every shortfall into
`unbacked_debt` and `total_shortfall_booked` (`crates/auction_core/src/accounting.rs:29-32`); buffer
cover goes to `total_debt_settled` (`:88-94`) and mint cover to `total_debt_auctioned` (`:69-73`), so
`booked = settled + auctioned + unbacked`. E7 adds:
- per type: `CollateralExt.deficit_booked: u128`, incremented by the new `Release.shortfall`
  (`Release` already reaches the collateral actor at every chain end, `bid.rs` guest `:177-188`);
- the identity and `Σ per-type deficit == total_shortfall_booked` as checks in
  `v04/harness/src/invariants.rs::check`, after every harness step;
- events `DeficitBooked`, `DebtCovered` (selectors as `docs/v04-port-design.md` §7), state being the
  primary record.

**No separate minimum write-off (ADR-080).** Dust shortfalls cannot start recovery already: a debt
auction needs `unbacked - buffer >= debt_lot` (`InsufficientBadDebt`), and the freeze needs a
threshold `>= debt_lot` (`ThresholdBelowDebtLot`). A third knob would change nothing observable, so
E7 does not add one; the ledger shows dust as it is.

**Mint bucket (rolling cap on debt auctions).** `AccountingExt { bucket: u128, bucket_ts_ms: u64 }`;
`EngineExtArgs { mint_bucket_cap, mint_bucket_period_ms }`. At every debt take
(`book_debt_take`, `v04/engine_core/src/system.rs`):

```
refill = mint_bucket_cap * (now - bucket_ts) / period            // floor, saturating
bucket = min(cap, bucket + refill); bucket_ts = now
if minted > bucket: EpochMintCapReached (6078)  else bucket -= minted
```

A token bucket bounds minting in any window of length `period` by `2 * cap` and smooths it; it
needs two fields and no history. It sits on top of `max_mint_per_auction`, so restarts cannot mint
faster than the bucket. The take reverts rather than shrinks; the SDK quotes the room. Legacy
`cap = 0` means off; recommended `cap = 4 * max_mint_per_auction`, `period = 86_400_000`; bounds
`cap <= MAX_MINT_HARD_CAP`, `period 3_600_000..=604_800_000` when `cap > 0`.

Tests: `mint_bucket_refills_linearly`, `mint_bucket_caps_at_cap`, Kani
`bucket_minting_within_any_period_at_most_twice_cap`; harness
`take_debt_refused_beyond_the_bucket`, `bucket_refill_allows_a_later_take`,
`restarted_debt_auctions_share_the_bucket`, `per_type_deficit_sums_to_booked` (invariant),
`legacy_bucket_off_is_unchanged` (existing `mint_never_exceeds_the_absolute_cap`,
`v04/harness/tests/debt.rs:245`, keeps passing). Cycles: ~2 `mul_div` on `TakeDebt` (~6,000) and a
`u128` on `Release`.

### 3.8 E8 — Tighten now, loosen after a delay (F8, security)

`EngineExtArgs.loosen_delay_ms` (legacy 0 = everything instant; recommended 86_400_000; bound
`0..=MAX_SYSTEM_TTL_MS`). Each admin change is compared field by field with the current values:

| Tightening (applies at once) | Always delayed when changed |
|---|---|
| lower: `max_oracle_age_ms`, `max_conf_bps`, `band_hi_bps`, `tip`, `chip_bps`, `reward_cap`, `type_cap`, `global_cap`, `bad_debt_freeze_threshold`, `ref_oracle_max_age_ms`, `max_mint_per_auction`, `slack_ms`, `max_spot_twap_dev_bps` (0 counts as ∞), `stale_bid_max_age_ms`, `grace_min_gap_ms`, `sanity_hi_bps`, `max_liquidation_debt` (0 = ∞), `ref_conf_bps`, `ref_clamp_bps`, `ref_sanity_hi_bps`, `mint_bucket_cap` (0 = ∞) | `oracle_scale`, `penalty_bps`, `debt_floor`, `min_bid`, the protocol token, the reference oracle, `ref_scale`, every `surplus_*`/`debt_*` field and system schedule, `min_take`, `system_ttl_ms`, `twap_window_ms`, `twap_min_spacing_ms`, `feed_min_raw`/`feed_max_raw`, `health_slope_bps`, `health_floor_bps` |
| raise: `band_lo_bps`, `twap_min_obs`, `grace_ms`, `sanity_lo_bps`, `ref_sanity_lo_bps`, `debt_relist_delay_ms`, `mint_bucket_period_ms`, `loosen_delay_ms`, pause level | |
| `admin_paused` false→true, `freeze_on_bad_debt` false→true | the reverse of each |
| collateral schedule: same point times, each discount <= old | any other schedule change |

Exempt both ways: `protocol_token_ref_price` (its safety is the ADR-031 step bound; a delay would
stop it tracking the market). The table is one `match` destructuring every field of every args
struct, so adding a field without classifying it fails to compile.

Mechanism: a change that only tightens applies as today. Any other change is stored whole as
`pending: Option<PendingChange { target: Engine | Collateral, args, eta_ms }>` (one slot in
`ConfigExt`, one in `CollateralExt`) with `eta = now + loosen_delay_ms`, an event
`ParamChangeQueued`, and no effect. A new change to the same target replaces the pending one (a
tightening-only change also clears it; the admin re-queues if still wanted). `ApplyPending` (49) is
root at `K`, permissionless, asserts the clock, refuses before `eta` (`ChangeNotDue`, 6079) or with
nothing pending (`NoPendingChange`, 6080), re-validates, re-checks the system-auction lock
(`ProtocolTokenLocked`, `v04/engine_core/src/assertions.rs:68-76`) and applies through the same path
as the original message. `CancelPending` (50) is admin-only. `SetEngineParams` gains `now_ms` for
`eta`.

Tests: `classification_covers_every_field` (compile-time), `tightening_applies_at_once`,
`loosening_waits_the_delay`, `apply_before_eta_is_refused`, `apply_is_permissionless_after_eta`,
`cancel_is_admin_only`, `a_new_change_replaces_the_pending_one`, `lowering_the_pause_level_is_delayed`,
`ref_price_step_is_exempt`, `apply_respects_the_system_auction_lock`, `legacy_delay_zero_is_instant`.
Cycles: admin rows only (field compares, ~10,000-20,000); new row `ApplyPending` ~6-12 executions
depending on target.

### 3.9 E9 — The five F8 gaps (F8)

| Gap (`f8-parameters.md`) | Parameter | Where | Legacy = today | Bound |
|---|---|---|---|---|
| Ref-oracle confidence `MAX_CONF_BPS` used as value (`price.rs:31`; `system.rs:44`) | `ref_conf_bps` | `EngineExtArgs` | 1_000 | `<= MAX_CONF_BPS` |
| `REF_ORACLE_CLAMP_BPS` (`price.rs:32`) | `ref_clamp_bps` | `EngineExtArgs` | 2_000 | `0..=5_000` |
| `DEBT_RELIST_DELAY_MS` (`args.rs:20`; used `system.rs:165`) | `debt_relist_delay_ms` | `EngineExtArgs` | 600_000 | `60_000..=MAX_SYSTEM_TTL_MS` |
| Sanity window `[R/2, 2R]` (`price.rs:79-80`) | `sanity_lo_bps`, `sanity_hi_bps` (collateral, per type); `ref_sanity_lo_bps`, `ref_sanity_hi_bps` (reference) | `RiskArgs`; `EngineExtArgs` | 5_000, 20_000 | `1_000 <= lo <= band_lo_bps`, `band_hi_bps <= hi <= 100_000` (reference: `lo <= 10_000 <= hi`) |
| Ref-oracle scale `OracleScale::Ray` (`v04/engine_core/src/system.rs:41-47`) | `ref_scale` | `EngineExtArgs` | 0 (Ray) | 0 or 1 |

`ref_price` (`system.rs:36-58`) switches to `validate_feed_ext` and
`risk_core::feed::effective_ref_price_ext(admin, oracle, clamp_bps)`; `debt_precheck`
(`system.rs:161-170`) reads `SystemCtx.debt_relist_delay_ms`; `SystemCtx::from_config` copies the
new fields. The recommended preset keeps today's values (reasoned in ADR-017 and ADR-039): E9 is
about who chooses, not about new numbers.

Tests: `ref_conf_limit_is_a_parameter`, `ref_clamp_is_a_parameter`
(`effective_ref_price_ext_matches_v024_at_2000`, proptest), `q64_reference_oracle_reads`,
`debt_relist_delay_is_a_parameter` (extends `relist_delay_boundary_uses_the_claimed_time`,
`debt.rs:294`), `sanity_window_is_a_parameter`, `legacy_preset_equals_the_v024_constants` (asserts
each legacy value equals the shared constant it replaces), and a check in `scripts/check-requirements.py`
or a new script that `f8-parameters.md` has no "production default — Tier 2" row left.
Cycles: none beyond decoding ~30 more bytes in `SystemCtx`.

### 3.10 Refusal codes (ADR-076)

`risk_core::RiskError` with codes 70-80 (wire 6070-6080), chosen above the shared enum's highest
non-`Math` code (61) and below `Math` (99):

| Wire | Variant | Raised by |
|---:|---|---|
| 6070 | `TwapUnavailable` | `Liquidate` (TWAP on), D4 bid |
| 6071 | `SpotTwapDeviation` | `Liquidate` |
| 6072 | `GracePeriod` | `Liquidate` |
| 6073 | `LiquidationsPaused` | `Liquidate` at level >= 1 |
| 6074 | `SystemAuctionsPaused` | `StartSurplus`, `StartDebt` at system level >= 2 |
| 6075 | `BidsPaused` | `Bid`, `TakeSurplus`, `TakeDebt` at level 3 |
| 6076 | `PriceFeedPinned` | `Liquidate`, `Bid` without D4 |
| 6077 | `LastGoodTwapTooOld` | D4 bid |
| 6078 | `EpochMintCapReached` | `TakeDebt` |
| 6079 | `ChangeNotDue` | `ApplyPending` |
| 6080 | `NoPendingChange` | `ApplyPending` |

`engine_core::check` gains `enum Refusal { Engine(EngineError), Risk(RiskError) }` with `From` for
both; `fail` and `fail_text` (`check.rs:51-64`) take `impl Into<Refusal>` and print the same
`Program error [6000+code]: Program error {code}: {Name}` text, so every existing call site compiles
unchanged. `scripts/gen-refusal-codes.py` learns to parse `risk_core/src/error.rs`; the catalogue
marks these rows "(0.4 only)".

## 4. Cycle budget summary

| Row (`v04/CYCLES.md`) | Today | Estimate after Tier 2 | Assertion added |
|---|---|---|---|
| `Bid (partial)` | 15 / 407,700 | 15 / ~470,000-490,000 | <= 520,000 |
| `Bid (final)` | 21 / 576,148 | 21 / ~650,000-690,000 | <= 720,000 |
| `Liquidate` | 29 / 839,357 | 29 / ~920,000-980,000 (E1+E4+E5) | <= 1,000,000 |
| `ObserveFeed` | 7 / 176,218 | 7 / ~200,000 | — |
| `SetCollateral` | 11 / 325,765 | 11 / ~350,000 | — |
| `TakeDebt` | 11 / 271,617 | 11 / ~280,000 | — |
| `SetPauseLevel` | — | 12 / 360,079 (measured, Plan 9) | <= 400,000 (4 % of cap; recorded deviation, below) |
| `ApplyPending` | — | ~6-12 | < 3 % of cap |

All rows stay under 10 % of the 10,000,000-cycle cap. Every added loop is bounded by 8. Estimates
are replaced by measurements at the end of each plan.

As-built note (Plan 9 final review): the Plan 9 measurements, and the assertions they satisfy, are
fast-path figures at the price 0.5 (§3.1.9). Above the TWAP limit of §3.1.9 the `Liquidate` and
`Bid` rows have not been measured. Just above it only the TWAP terms can switch (at most 8 frozen
divisions per `Liquidate` or `Bid`), and whether the rows then stay inside their assertions has
not been established by measurement. The pre-change figures (`Liquidate` 1,012,946, `Bid
(partial)` 561,253, `Bid (final)` 736,045) are a loose upper bound, approached only at prices far
above that (redemption price from about 17,000,000), where the remaining multiply-divides also
switch. So "under 10 % of the cap" and the assertions are established for the fast path only. On
the fast path the measured E1 row (883,636) leaves 116,364 under the 1,000,000 assertion for E4
and E5 (estimated at ~10,000 and ~8,000, §3.4-§3.5). That headroom for later plans is likewise
established only on the fast path. Open item: a
cheaper exact wide division in `risk_core::math`, or slow-path budgets.

Recorded deviation (Plan 9 ruling R9-10): this row first read "`SetPauseLevel` ~12 / ~330,000,
< 3 % of cap"; the two figures contradict (3 % is 300,000). `SetPauseLevel` measured 360,079 cycles
(3.6 %) and is asserted `<= 400,000`. Reason: it is 12 executions including the 7-execution Observe
block; `ObserveFeed` alone is 181,947 and `SetCollateral` at 11 executions is 359,048 (all measured
under `RiskArgs::recommended()` with a full ring, `v04/CYCLES.md`). Plan 9's measured rows:
`Bid (partial)` 15 / 431,669, `Bid (final)` 21 / 606,461, `Liquidate` 29 / 883,636 (E1 only),
`ObserveFeed` 7 / 181,947, `SetCollateral` 11 / 359,048.

## 5. Order of work and plan split

Dependencies found:
- D4 (E2) needs the TWAP (E1); the matrix needs the levels and the D4 price.
- The bid anchor (E1) needs `max_spot_twap_dev_bps`, so the **deviation breaker moves from E3 into
  Plan 9**.
- `validate_feed_ext` is written once in Plan 9 with the full `FeedPolicy` (sanity window and pin
  bounds as fields, filled with legacy values), so Plan 10 only adds the parameters and the pinned
  rule, not a second rewrite.
- E4 needs `P_liq` (E1) and the corrected host price convention; E5 needs the host interface change.
- E8 classifies every parameter, so it comes last.

| Plan | Items | Milestone | Order inside the plan |
|---|---|---|---|
| **9** | E1 (incl. deviation breaker) + E2 (incl. D4) | M2 (E1), M1 (E2) | frozen-guest check script → `risk_core` scaffold, `RiskError`, `Refusal`, refusal generator → `validate_feed_ext` + differential → TWAP ring + Kani → state ext, message fields, legacy/recommended presets in SDK and harness → price per operation → levels + `SetPauseLevel` → D4 + floor → matrix tests → `tier2` e2e stage → cycles re-measure → docs |
| **10** | E3 (pinned, grace) + E9 | M2 | E9 engine fields and `SystemCtx` → ref-oracle parameters → per-type sanity window → pinned rule → grace → matrix gains the Grace column → docs |
| **11** | E4 + E5 + E6 | M1 | mock-host price convention fix → host interface v3 (`max_debt`) → chunking → health cap → stairstep preset → docs |
| **12** | E7 + E8 | M3 | per-type deficit + invariants → mint bucket → classification table → pending/apply/cancel → lowering levels delayed → docs |

Plans 9 and 10 together deliver R2 and R5; if RFP-014 stays closed long enough they are marked
"delivered early" (parent S1). Plan 11 can start in parallel with Plan 10 once Plan 9's state and
message plumbing is merged; Plan 12 waits for both.

e2e: the existing `slice` and `system` stages keep the legacy preset. Plan 9 adds a `tier2` stage
to `v04/scripts/e2e.sh` with a time-compressed recommended preset (window 300 s, first written as
60 s: see the deviation below; spacing 10 s, `min_obs` 3, grace 30 s) that shows: liquidation
refused before warm-up, nonzero confidence, a level-1 pause with a bid still filling, a stale feed
with a fallback bid, and `SettleEmpty`/claim at level 3. Later plans extend that stage.

Deviation (Plan 9 pre-flight): the `tier2` stage uses a **300 s** window, not 60 s. Its feed max
age is 180 s, and by the §3.2.4 bound a window must exceed `180 s + (3 − 2) · 10 s = 190 s` for the
fallback bid to be reachable: with 60 s the stale-feed TWAP holds only the newest sample and D4
always refuses `TwapUnavailable`. The 300 s window also keeps every live sample in the window across
the e2e's 15 s blocks. The other values stand (Plan 9 ruling R9-13). Note (Plan 9 final review):
300 s / 10 s is 30 spacings per window, past the 8-sample ring (§3.1.2), and spacing 10 s is below
the stage's 60 s slack. So the stage exercises the flow, not TWAP manipulation resistance.

## 6. New ADRs

| ADR | Decision |
|---|---|
| ADR-069 | Tier 2 edits no file under `programs/`, `crates/auction_core/`, `crates/auction_engine_params/` or the root manifest (supersedes spec S2); new pure logic lives in `v04/risk_core`; checked by `git diff` + `check-guests.sh` + `guests.yml`. |
| ADR-070 | On-chain TWAP: an 8-slot ring of validated mids at the collateral actor, time-weighted over a window anchored at `min(now, newest + max_age)`, broken by any gap longer than the feed's max age, valid only with `min_obs` samples. `T` lies in `[min p_i − 7, max p_i]` (lower end saturating at 0): the floor rounding of up to 8 terms can put it up to 7 units below the smallest sample, never above the largest. |
| ADR-071 | One confidence rule: every engine price is a payout, so it values the collateral at the high edge (`edge(min(M, T))` for liquidation, `edge(M)` for bids); the unsafe test stays the host's. |
| ADR-072 | Bids are anchored to the TWAP from above only (`P <= T(1 + dev)`); a liquidation is refused while spot deviates from the TWAP by more than `dev`, i.e. unless `|M - T| * 10_000 <= dev * T` (exact). |
| ADR-073 | Pause levels 0-3 per type with a config-level system level; `admin_paused` = level 3 for collateral operations with code 6020; risk-reducing operations are never gated; one rule table in `risk_core::pause` drives code and tests. |
| ADR-074 | D4 as built: on an invalid feed, bids at levels <= 2 use the last-good TWAP while it is younger than `stale_bid_max_age_ms`, with the discount clock frozen and the price clamped to the floor `P_liq / (1 - d_max_eff)`. |
| ADR-075 | A reading at the feed's configured bound is invalid; a feed or chain gap of at least `grace_min_gap_ms` starts a grace period that refuses `Liquidate` only. |
| ADR-076 | v04-only refusal codes 6070-6080 in `risk_core::RiskError`, wrapped with `EngineError` in `Refusal`; same failure text format. |
| ADR-077 | The health-aware discount cap is computed once at `Seized` and written into the auction's schedule snapshot, with a floor for liveness; no per-bid cost. |
| ADR-078 | Chunked liquidation through host interface v3 (`max_debt`, `min_remainder` on `LiquidatePosition`), room-fitted at the config; the mock host's price convention is corrected to collateral per coin. |
| ADR-079 | Stairstep schedules are an SDK preset built from 1 ms risers inside the shared linear schedule; step length is tested against the measured proving time. |
| ADR-080 | A per-type deficit ledger and a token-bucket mint cap on debt takes; no separate minimum write-off, because `debt_lot` and the freeze threshold already ignore dust. |
| ADR-081 | Tightening applies at once; any other change waits `loosen_delay_ms` in one pending slot per actor, applied permissionlessly; the classification is an exhaustive `match`. |
| ADR-082 | F8 closure: the five fixed values become init parameters; the engine holds no defaults; `legacy` and `recommended` presets live in the SDK and harness. |

Each ADR is written in `docs/design-log.md` in the existing format (Context, Options, Decision,
Trade-offs, Evidence, Status) by the plan that builds it.

## 7. Left to the builder

- Module layout inside `risk_core` and `engine_core`, names of private helpers, the exact field
  order inside each `*Ext` struct (append-only once a plan merges).
- Whether `RiskArgs` grows per plan (default) or is defined whole in Plan 9 with not-yet-active
  fields required to equal legacy values; either is fine if each plan has a migration note.
- Proptest strategies and Kani bounds (unwind <= 9 for ring loops).
- The exact time-compressed values of the `tier2` e2e stage, and whether events are emitted in the
  same plan as their state (state is required; events are best-effort, design note §7).
- SDK views for the new state (ring, level, grace, pending change, bucket); the K1 keeper's observe
  cadence.
- If V5 lifts R-F3 (two types per config): `system_level` becomes the count of types at level >= 2,
  and `SetPauseLevel` updates it by delta; the pending slot in `ConfigExt` stays engine-level.
- Wording of the REQUIREMENTS and proposal changes.

## 8. Docs to update after each plan

| After | Update |
|---|---|
| Plan 9 | `REQUIREMENTS.md` R2 (TWAP, confidence edge, anchor, deviation tests; drop "confidence always 0" once a test publishes nonzero confidence) and R5 (matrix and D4 tests) → "PoC" if all pass, R4 note; `refusal-codes.md` regenerated (6070, 6071, 6073-6075, 6077); `f8-parameters.md` new `RiskArgs` table with bounds and both presets; `pricing-model.md` §2 "Decided rule (D4), planned" → as built, with the floor formula and test names, and the TWAP in §1; ADR-067 Status → built; `v04/CYCLES.md` re-measured with the new rows; `docs/v04-port-design.md` §3 state layout; proposal: R2/R5 rows, M1/M2 "delivered early" with evidence links |
| Plan 10 | REQUIREMENTS F8 → "PoC" (five gaps closed), R2 note (pinned, grace); `refusal-codes.md` (6072, 6076); `f8-parameters.md` gap rows → init parameters, "F8 gaps" section emptied; `pricing-model.md` §2 grace and pinned; proposal F8 row |
| Plan 11 | REQUIREMENTS F1 (chunks), F2 (health cap; the missing v04 schedule-change test), R3/P2 notes; `f8-parameters.md` E4/E5 fields; `pricing-model.md` §1 health cap and the stairstep recommendation; `docs/architecture/hooks.md` and the dependency table: host interface v3 and the price convention; proposal F1/F2 and the host dependency row |
| Plan 12 | REQUIREMENTS F4 (bucket, per-type ledger), F8 (delayed loosening), R6 (ledger invariant); `refusal-codes.md` (6078-6080); `f8-parameters.md` classification table and `loosen_delay_ms`; `security-checklist.md` admin-key section; proposal M3 |

Every plan also: runs the frozen-guest check (§2), re-measures the touched `CYCLES.md` rows, and
updates README test counts through the existing generator.
