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

- No restart: the starting price is fixed by the schedule, not by an oracle snapshot at open, so a restart would only reset the discount to its starting value, which achieves nothing.
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

When the bid would take at least all remaining collateral, the bidder receives exactly `collateral_left` and is charged only for it: `charged = min(ceil(collateral_left x denom / price_ray), coin)`. The ceil rounds the coin owed up (favours the protocol), and the `min` guarantees the bidder is never charged more than the capped `coin`. The bidder is not billed for collateral that does not exist.

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

## Rounding

See the `auction` rows in the Call Sites table of [math.md](math.md).
