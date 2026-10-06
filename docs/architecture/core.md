# Auction Core: Collateral Auction State Machine

Module: `crates/auction_core/src/auction.rs` (spec sections 5.5, 5.6, 5.8). Pure and `no_std`: every transition takes an `Auction` by reference and returns a new value; nothing mutates in place and nothing performs I/O. The program layer (Plan 2) owns accounts, the clock, the oracle price and the owner check for `claim_leftover` (`NotOwner`).

## State Diagram

```
          open(quote, start_ms, schedule, paused_at_start)
                         |
                         v
                  +-------------+   apply_bid, f.ends_auction == false
                  | Auctioning  |------------------------------+
                  +-------------+<-----------------------------+
                         |
                         | apply_bid, f.ends_auction == true
                         v
                  +-------------+   claim_leftover (repeatable until
                  |   Settled   |   claimable == claimed; then NothingToClaim)
                  +-------------+
```

`Auctioning -> Settled` is the only transition. Settled is the only terminal state. There is no restart and no expiry (ADR-012):

- No restart: The price is re-read at every bid, so a restart would only reset the discount from its maximum back to the start.
- No expiry: Maker's `redo` exists because Maker fixes its starting price; this engine does not. Expiring an auction would book a shortfall while unsold collateral still exists, minting against debt that is still recoverable and diluting holders without a real loss. Instead an unsold auction stays open at its maximum discount until it settles.

## Opening

`Auction::open` copies the `LiquidationQuote` into the auction: `collateral_seized = q.seized`, `collateral_left = q.lot`, plus `debt_principal`, `amount_to_raise`, `reserved_amount = q.reserve` and `keeper_reward`. The raised counters, `collateral_sold` and the leftover counters start at 0. `start_ms`, the `Schedule` and `type_paused_total_at_start` are stored for the program layer's effective-time computation.

Invariant: `raised() = debt_raised + penalty_raised <= amount_to_raise`, so `remaining_target() = amount_to_raise - raised()` cannot underflow.

## Bid Algorithm (`quote_bid`)

Prices are RAY, collateral per 1 stablecoin. Collateral paid out = coin x P / (RAY - discount); a lower P pays out less collateral.

1. Reject with `NotAuctioning` unless `status == Auctioning`.
2. `remaining = remaining_target()`; `coin = min(coin_in_max, remaining)`. A huge `coin_in_max` is therefore capped at what the auction still needs to raise.
3. `d = schedule.discount_ray(t_eff_ms)`; `denom = RAY - d` (safe: validation caps the discount at 30% of RAY).
4. `full_out = floor(coin x price_ray / denom)` (collateral to the bidder, rounded down).
5. If `full_out >= collateral_left`, the lot binds (see below). Otherwise `collateral_out = full_out` and `charged = coin`.
6. Reject with `ZeroFill` if `collateral_out == 0` or `charged == 0`.
7. Reject with `Slippage` if `collateral_out < min_collateral_out`.
8. `remaining_after = remaining - charged`; the bid ends the auction if `remaining_after == 0` or the lot empties (`collateral_out == collateral_left`).
9. Dust rules (non-ending bids only; see below).
10. Split `charged` debt-first (below). Return a `BidFill`.

`apply_bid` then updates `collateral_left`, `collateral_sold`, `debt_raised` and `penalty_raised`; if `ends_auction` it also settles the auction and returns a `ChainEnd`.

## Debt-First Splitting

`debt_left = debt_principal - min(debt_raised, debt_principal)`; `debt_part = min(charged, debt_left)`; `penalty_part = charged - debt_part`. Proceeds repay principal first, and only once the principal is fully raised does the remainder count as penalty. Any shortfall therefore lands on the penalty portion before it touches the debt.

## Lot-Binds Charge

When the bid would take at least all remaining collateral, the bidder receives exactly `collateral_left` and is charged only for it: `charged = min(ceil(collateral_left x denom / price_ray), coin)`. The ceil rounds the coin owed up (favours the protocol). The `.min(coin)` is provably a no-op (`full_out >= L` with integer `coin` implies `coin >= ceil(L x denom / P)`); it is kept only as a defensive bound, not a real cap. The bidder is not billed for collateral that does not exist.

## Dust Rules

Both apply only when the bid does not end the auction, so a final bid is always allowed regardless of size:

- `BelowMinBid`: `charged < min_bid`.
- `DustRemainder`: `remaining_after < chost`, i.e. the bid would leave an uncollectable remainder below the debt-floor-derived `chost`.

## Chain-End Effects

When a bid ends the auction, `apply_bid` sets `status = Settled` and returns `ChainEnd { shortfall, release, leftover }`:

- `shortfall = debt_principal - debt_raised` (saturating at 0): bad debt to book. It is nonzero only when the lot is exhausted before the principal is covered, which is the only case in which a shortfall may be booked.
- `release = reserved_amount`: the whole cap reservation held by this auction is released, including the over-size reservation of an oversized-alone auction.
- `leftover`: see below.

## Leftover Rule

`leftover_claimable = collateral_left` only if the target was reached (`raised() == amount_to_raise`); otherwise 0. If the lot ran out before the target was met, nothing is left over by definition, and the shortfall is booked instead. After settlement, `claim_leftover` returns `Err(NothingToClaim)` unless the auction is `Settled` and `leftover_claimable - leftover_claimed > 0`; on success it returns the new auction with `leftover_claimed = leftover_claimable` and the amount paid. A second claim therefore fails with `NothingToClaim`.

## Liveness

ADR-012 has no expiry, so an auction that cannot settle would hold its cap reservation forever and permanently disable the oversized-alone rule. Two situations could cause this; both are handled.

- (a) Empty lot (`collateral_left == 0`, e.g. the keeper reward clamps to the whole seizure, or the vault balance is 0). Every bid takes the lot-binds branch with `charged = 0` and fails with `ZeroFill`. `settle_empty` is a permissionless transition, valid only while `Auctioning` with `collateral_left == 0` (otherwise `NotAuctioning`). It sets `Settled` and returns `ChainEnd { shortfall: debt_principal - debt_raised (saturating), release: reserved_amount, leftover: 0 }`, the same chain-end semantics as `apply_bid`'s settlement.
- (b) Remaining target worth less than 1 collateral unit (low price, or a partial bid leaving a remainder below one unit, since `chost` may be worth < 1 unit). Every bid floors `full_out` to 0 and the lot cannot bind. `quote_bid` therefore has a final-fill exception: in the non-binding branch, if `coin == remaining`, `full_out == 0` and `collateral_left > 0`, it pays out `collateral_out = 1` for `charged = coin`. This is the only place a payout rounds up; it applies only to the target-reaching fill and is bounded by 1 collateral base unit per auction. A bid with `coin < remaining` that floors to 0 is still `ZeroFill`.

## Rounding

See the `auction` rows in the Call Sites table of [math.md](math.md).

## Accounting

`Accounting` (engine-only, immutable updates) tracks bad debt and totals. Flows:

- `record_bid(BidFill)`: `total_debt_burned += debt_part`, `total_penalty_collected += penalty_part`. `unbacked_debt` is untouched by bids.
- `record_chain_end(ChainEnd)`: `unbacked_debt += shortfall` and `total_shortfall_booked += shortfall`. Only a chain end (from `apply_bid` or `settle_empty`) can book a shortfall.
- All counters use saturating addition.

## settle_debt

`settle_debt(amount, buffer_balance)` burns `min(amount, buffer_balance, unbacked_debt - debt_on_auction)` (see Debt Reservation) and returns the new `Accounting` and the burned amount. `buffer_balance` is the ACTUAL balance of the surplus holding account, never a ledger-tracked figure, so debt can only be settled with coin that really exists. It decreases `unbacked_debt` and increases `total_debt_settled` by the burned amount; it cannot underflow because `burn <= unbacked_debt - debt_on_auction <= unbacked_debt`.

## Surplus Auction (F5)

An instant Dutch auction that sells stablecoin surplus for protocol tokens, which are burned. `ref_ray` is RAY **stablecoin per 1 protocol token**.

- Start (`start_surplus`) requires, in order: no live surplus auction, token configured, `unbacked_debt == 0`, `buffer >= surplus_buffer_target + surplus_lot`, and `now >= last_end + surplus_delay`.
- Multiplier (bps, dimensionless): `mult(t) = max(BPS + surplus_premium_bps - decay_bps(t), BPS - surplus_max_discount_bps)`, where `decay_bps(t) = surplus_decay.discount_ray(t) / BPS_TO_RAY`.
- Price: `tokens = ceil( ceil(coin * mult / BPS) * RAY / ref_ray )`. Units: coin (stablecoin) x dimensionless / (RAY coin per token) = protocol tokens. Tokens are owed by the taker, so both steps round up (favours the protocol).
- `take_surplus` caps the take at `lot_left`, rejects zero (`ZeroFill`), a nonzero remainder below `min_take` (`DustRemainder`), and `tokens > max_tokens_in` (`Slippage`). It returns the tokens to burn.

## Debt Auction (F4)

An instant Dutch auction that mints protocol tokens in exchange for stablecoin, which is burned against `unbacked_debt`.

- Start (`start_debt`) requires, in order: no live debt auction, token configured, protocol token matches (`ProtocolTokenMismatch`), mint authority held (`MintAuthorityMissing`), and `unbacked_debt - buffer >= debt_lot` (`InsufficientBadDebt`); then `debt_start_amount` must fit the mint cap (`MintCapConfig`). A `start_amount` of 0 (ref price so high that the lot mints nothing) is also `MintCapConfig`: the auction cannot mint usefully, so `check_solvency` treats it as a freeze-worthy config fault.
- Start amount (protocol tokens): `floor( floor(debt_lot * RAY / ref_ray) * (BPS - debt_initial_discount_bps) / BPS )`. Units: coin / (RAY coin per token) = tokens. Capped at `max_mint_per_auction`, itself at most `MAX_MINT_HARD_CAP`; exceeding either is `MintCapConfig`.
- Offered for the whole lot at `t`: `min( floor(start_amount * (BPS + growth_bps(t)) / BPS), max_mint_per_auction )`, with `growth_bps(t) = debt_growth.discount_ray(t) / BPS_TO_RAY`.
- `take_debt(coin)` mints `floor(offered * take / debt_lot)` tokens (minted tokens are paid out, so round down; favours the protocol), subject to `min_tokens_out` (`Slippage`) and the same zero/dust rules as the surplus auction. The mint cap therefore holds at every `t`.

## check_solvency

`check_solvency(unbacked_after_settle, threshold, token_configured, freeze_on_bad_debt, debt_auction_live, start_check)` is evaluated top to bottom, first match wins. Callers pass `unbacked` after `settle_debt`.

| Case (from `solvency_decisions`) | Condition | Decision |
|---|---|---|
| Within threshold | `unbacked <= threshold` | `Healthy` |
| No protocol token | `unbacked > threshold`, token not configured | `Freeze` |
| Recovery already running | token configured, debt auction live | `RecoveryInProgress` |
| Freeze disabled | token configured, none live, `freeze_on_bad_debt == false` | `NoAction` |
| Unstartable: `MintCapConfig` | none live, freeze enabled, `start_check = Err(MintCapConfig)` | `Freeze` |
| Unstartable: `MintAuthorityMissing` | as above, `Err(MintAuthorityMissing)` | `Freeze` |
| Unstartable: `ProtocolTokenMismatch` | as above, `Err(ProtocolTokenMismatch)` | `Freeze` |
| Start already live | as above, `Err(DebtAuctionAlreadyLive)` | `RecoveryInProgress` |
| Startable (or transient error) | as above, `Ok(())` or a transient start error (`InsufficientBadDebt`, `Math`) | `NoAction` (a keeper should start an auction) |

## Debt Reservation (no double coverage)

A live debt auction sells the same bad debt that `settle_debt` could burn from the buffer; covering it twice would mint tokens for debt that no longer exists. `Accounting.debt_on_auction` reserves it (invariant: `debt_on_auction <= unbacked_debt`):

- `record_debt_start(lot)`: `debt_on_auction += lot`.
- `record_debt_take(coin_taken)`: `unbacked_debt -= coin_taken`, `debt_on_auction -= coin_taken`, `total_debt_auctioned += coin_taken`. `total_debt_burned` stays reserved for collateral-auction debt.
- `record_debt_close(lot_left)`: `debt_on_auction -= lot_left`; the untaken remainder is settleable again.
- `settle_debt` burns `min(amount, buffer_balance, unbacked_debt - debt_on_auction)`.

`take_surplus` and `take_debt` fill at most `lot_left`: the coin actually filled is `old.lot_left - new.lot_left` (<= the requested coin), and callers must transfer/burn that amount, not the requested one. A `take_debt` that would mint 0 tokens is `ZeroFill`.

## Closing System Auctions and Params Validation

- `close_surplus` / `close_debt` (spec §5.11): permitted once `now >= start_ms + system_ttl_ms`; a non-live auction is `NotLive`, an early close is `DelayNotElapsed`. The result is `live: false, lot_left: 0` plus the released amount: surplus coin returns to the buffer; for debt nothing is minted for the untaken part, and the caller pairs it with `Accounting::record_debt_close`.
- `SystemParams::validate` (program layer must call it): `surplus_max_discount_bps <= 3000`, `debt_initial_discount_bps <= 3000`, nonzero lots, `min_take <=` both lots, `0 < max_mint_per_auction <= MAX_MINT_HARD_CAP`, `system_ttl_ms > 0`. The schedules are validated `Schedule`s.
- `bad_debt_freeze_threshold >= debt_lot` (ADR-017) is a config precondition enforced at the program layer (Plan 2), since the threshold is not a `SystemParams` field.
- `surplus_nonce`, `debt_nonce` and `host_frozen_by_engine` in `Accounting` are written by the program layer (Plan 2), not by these pure functions.
