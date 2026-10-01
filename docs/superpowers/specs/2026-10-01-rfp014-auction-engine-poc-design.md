# RFP-014 Liquidation & Auction Engine: Proof of Concept (design spec)

| | |
|---|---|
| **Status** | Draft v2 for review. It includes the internal spec review and the external review of 2026-10-01 (see §19). |
| **Date** | 2026-10-01 |
| **Author** | paschal533, with Claude |
| **Scope of this spec** | Sub-project **A**: a 3-week, engine-first PoC. Sub-project **B** (follow-ups) is listed in §2.3 and gets its own spec. |
| **Target RFP** | [logos-co/rfp RFP-014](https://github.com/logos-co/rfp/blob/master/RFPs/RFP-014-liquidation-auction-engine.md). Status `closed`, "Reopening soon", no date given. |
| **Evidence base** | `docs/research/research-0..6-*.md`, `docs/design/ui/*`; code at lez-programs `64c0120`, LEZ v0.2.4, SPEL `1ef0500f` |

---

## 1. Goal, strategy and success criteria

**Goal.** Build the parts of RFP-014 that no competitor has built and that are least likely to change when the RFP reopens:
- the engine;
- real hooks into the Logos stablecoin;
- the **F4 debt auction and F5 surplus auction**, using a mock protocol token;
- a CLI;
- **real-proof CI**.

A **thin** Basecamp demo is added on top. The full approved UI design is kept in `docs/design/ui/` for the separate RFP-014 UI RFP that ecosystem#210 announces.

**Why engine-first (ADR-013).**
- **Precedent.** Proposals that filed early and followed spec changes won: bleu took RFP-015/016 at about $27.5k per L-tier RFP. Antumbra, with finished PoCs for both, got no award.
- **The UI is likely to move.** ecosystem#210 says the liquidation and auction UI is "a separate RFP".
- **The gap competitors left open:**
  - Antumbra "does not move tokens yet", has no host hook and no real-proof sequencer CI.
  - Web3spell has only a README.
  - Neither has F4/F5.

**Success criteria.**
1. **One-command reproduction.** On a clean machine with Nix, one script does the whole flow:
   1. starts a standalone LEZ v0.2.4 sequencer;
   2. deploys the stablecoin (with our hooks), the engine and a mock protocol token;
   3. opens a position and makes it liquidatable;
   4. liquidates it, places partial bids and settles;
   5. drives a shortfall into the **debt auction** and a surplus into the **surplus auction**;
   6. shows the "no protocol token configured, so the host freezes new debt" path.
2. A demo recorded by the author (not AI-narrated), plus `SUBMISSION.md` with full linked hashes and reproduction steps.
3. CI is green on `main`. It runs:
   - unit and property tests;
   - dev-mode sequencer end-to-end tests;
   - **one real-proof (`RISC0_DEV_MODE=0`) end-to-end job**, on every push to `main`, scoped to fit CI time;
   - the full real-proof suite nightly and on tags;
   - drift checks for IDL order, headers and encoders;
   - the cycle-count regression check;
   - the `REQUIREMENTS.md` coverage check.
4. Every RFP-014 hard requirement is mapped to a test (PoC), a milestone (promise), or an honest descope with a reason.
5. The stablecoin's own test suite passes unchanged with our hooks appended. The hooks are offered upstream as a PR. Two upstream issues are filed: the SPEL CLI's IDL-order encoding, and the oracle scale question.
6. **Early engagement:** a short note on ecosystem#205 or Discord `#builder-hub`, drafted in `docs/outreach/`. The user posts it.

## 2. Scope

### 2.1 In the PoC (sub-project A)
- **`auction_core`**, a pure Rust crate: math, schedule, collateral, debt and surplus auction state machines, accounting, invariants.
- **`auction_engine`**, a SPEL program:
  - `init_config`, `set_collateral_config`
  - `liquidate`, `bid`, `expire`, `observe_feed`, `claim_leftover`, `settle_debt`
  - `start_surplus_auction`, `take_surplus`, `start_debt_auction`, `take_debt`, `close_system_auction`
  - `check_solvency`
- **Stablecoin hooks**, appended at the end of **both** the instruction enum and the guest function list: `set_liquidation_engine`, `liquidate_position`, `engine_freeze_new_debt`.
- **`mock_protocol_token`**: an ordinary token definition from the token program, whose mint authority is the engine's PDA. It is used only for F4/F5 tests and the demo.
- **`auction_sdk`** (Rust lib with its **own instruction encoders**), `auction_ffi`, and **`auction_cli`**, which uses the SDK encoders and not spel-cli (§6.6).
- **Thin GUI**: `auction_engine_module` (C++ universal core module) plus `apps/auctions`, limited to **the Live auctions view and the bid flow**. It uses the approved styling, budgeted at about 2 days. The At-risk positions view and the full confirmation dialog stay designed but unbuilt (§7).
- Tests, security review, benchmarks and docs (§9–§12).

### 2.2 Not in the PoC (promised in proposal milestones)
- The private-path SDK and UI: deshield → interact → reshield for liquidate, bid and settle (U6 private part, PRIV1–3, PA1–4).
- Public testnet deployment (S1). It is blocked by v0.3 (§3.1).
- Full GUI (U4 view, full confirmation dialog), the module catalog and release action (PEND5/6), Figma or equivalent (PEND3; approved mockups exist), the full `PRIVACY.md` (PEND4), and the audit (PEND8).
- SOFT1 multi-oracle and SOFT3 bot SDK as a product. Kani beyond `mul_div` and the schedule.
- A sale path for `ProtocolCollateral`. Its holdings are covered by the debt auction meanwhile (§5.8).

### 2.3 Sub-project B (next spec)
- The private path (SDK and UI).
- The v0.3 port (once SPEL supports v0.3) and testnet deployment.
- The full GUI per `docs/design/ui/`.
- The catalog, `PRIVACY.md`, threat-model completion and audit preparation.
- Multi-oracle support.
- A `ProtocolCollateral` sale path.
- Partial liquidation, if reviewers want it.

### 2.4 Out of scope (RFP OOS1–4)
- flash-loan liquidations;
- aggregators and batching;
- insurance or socialised loss;
- multi-collateral atomic liquidations.

## 3. Context and constraints

### 3.1 Toolchain pins (ADR-001, Track B)

| Component | Pin |
|---|---|
| LEZ (`lee`, `lee_core`) | v0.2.4 `47eba256479f6f785acbd138834340703cd03401` |
| SPEL | `1ef0500f8fc8ce3ddf95523726ae159db83f744f` (matches lez-programs) |
| risc0-zkvm | `=3.0.5` |
| Rust | 1.94.0 |
| lez-programs (vendored stablecoin) | `64c01210adecd6b557dbd021a3cc94553f243fd8` (current HEAD as of 2026-10-01) |
| `lez_core` module | `acf0cd501b262c4c15969e3735e85318297b85bf` (metadata 0.4.1) |
| logos-module-builder | the revision matching the Basecamp release used in end-to-end tests (spike S1) |
| logos-design-system | whatever Basecamp bundles. It is not shipped in our package. |

**Why not v0.3.0.** The public testnet moved to v0.3.0 on 2026-10-01. SPEL has no v0.3 support, and RFP U1 requires SPEL. The stablecoin is on v0.2.4. Porting is a rewrite of the program layer.

**Mitigation:** all logic lives in `auction_core`, and the SDK keeps an encoding seam. The port is planned in `docs/v03-port-plan.md`.

### 3.2 Platform facts that shape the design
- **Chained calls:** at most 10 per transaction (11 executions with the top level). They are depth-first and **return no data**. Any panic reverts the transaction. If one transaction touches the same account in two chained calls, the later call carries the updated state.
- **Limits:** **32 Mi (33,554,432) cycles** per execution. Account data is limited to 100 KiB.
- **The callee sees `ctx.caller_program_id`**, which the runtime enforces. A top-level call reports all zeros.
- **PDA authorisation is inherited down the call chain.** A PDA we authorise when calling the stablecoin can be re-authorised by the stablecoin to any program it calls. This is trusted-upstream code, recorded in `THREAT_MODEL.md`.
- **No reentrancy guard exists anywhere in LEZ v0.2.4.** R1 rests entirely on our programs never calling back into the engine (§10).
- **The sequencer executes transactions one at a time.** There are no write conflicts. A transaction whose precomputed account addresses no longer match simply fails.
- **`Token::Burn`** needs only the holder's authorisation. **`Token::Transfer`** needs no permission from the recipient, so **anyone can raise any holding's balance** (§10, "at least, sweep").
- **No clock in the SPEL context.** Time comes from `CLOCK_01` in ms, as a read-only account with its id asserted. Within a block it reads the previous block's timestamp.
- **No account-listing RPC and no events on v0.2.4.**
- **lez_core 0.4.1 `success` means accepted, not executed.** Failed transactions are silently dropped at block build, and the signer nonce advances only on apply. `poll_transaction_status` arrives only with the v0.3 FFI (lez_core ≥ 0.4.3 built on v0.3).
- **Accounts can never be released.**
- **spel-cli encodes an instruction as its position in the IDL** (`spel-cli/src/tx.rs`: `idl.instructions.iter().position(...)`). The IDL follows the order of the guest functions, not the enum. On the stablecoin these differ from index 8 onwards (e.g. Withdraw is 17 in the enum and 8 in the IDL), so spel-cli mis-encodes stablecoin instructions.
- **The host has exactly one collateral type per instance.** The collateral definition is fixed at `initialize_program`, and the position seed has no type. It has no separate liquidation ratio: health is checked at `minimum_collateralization_ratio` against the redemption price. The oracle account struct (`OraclePriceAccount`) has a `confidence_interval` field, but every current producer writes 0.

### 3.3 Dev environment
Windows 11, WSL Ubuntu 20.04, Nix, about 7 GB RAM. Raise the WSL memory limit and add swap before the first Basecamp build. Guest builds run in Docker.

## 4. Architecture

### 4.1 Layers

```
apps/auctions (thin QML app) ──► C++ backend (.rep) ──► auction_engine_module (C++ universal) ──► lez_core
                                                              │ char* op(const char* json)
                                                        auction_ffi (Rust cdylib)
                                                              │
auction_cli ─────────────────────────────────────────►  auction_sdk  (PDAs, decoders, quotes,
                                                              │       OWN instruction encoders,
                                                              │       block scanner, tx confirm)
auction_engine (thin SPEL layer) ─────────────────────► auction_core (pure logic)
On-chain: auction_engine ─chained─► stablecoin.{liquidate_position, engine_freeze_new_debt}
          auction_engine ─chained─► token.{Transfer, Burn, Mint(mock protocol token)}
          reads: CLOCK_01, OraclePriceAccount(s), host state
```

### 4.2 Repository layout

```
lez-auction-engine/
  crates/auction_core/   crates/auction_sdk/
  programs/auction_engine/   programs/stablecoin/ (vendored @64c0120 + hook patch series)
  programs/integration_tests/
  modules/auction_engine/{ffi/, src/, tests/, metadata.json, flake.nix, CMakeLists.txt}
  apps/auctions/{qml/, src/, tests/, metadata.json, flake.nix}   apps/shared/wallet/ (vendored)
  cli/   scripts/{localnet.sh, demo.sh, verify.sh, run-basecamp-e2e.sh}
  idl/  artifacts/  DEPLOYMENTS.md  CYCLES.md  REQUIREMENTS.md  SUBMISSION.md  THREAT_MODEL.md
  docs/{design-log.md, architecture/, spikes/, outreach/, v03-port-plan.md, PRIVACY.md, research/, design/ui/, superpowers/}
  flake.nix  rust-toolchain.toml  .github/workflows/
```

## 5. On-chain design

### 5.1 Units and math
- Amounts are raw base units as `u128`. Prices are RAY = 1e27. Discounts are in bps, converted to RAY by multiplying by 1e23. Time is in ms as `u64`.
- Every product goes through `mul_div_floor` / `mul_div_ceil` with a 256-bit intermediate.
- **Rounding:** amounts paid out round down; amounts owed or charged round up.
- `overflow-checks = true` in release, and `#![forbid(unsafe_code)]`.

### 5.2 Price and feed validity (ADR-005)
The host's unit of account is the collateral. The redemption price R and the oracle price M are both **collateral per 1 stablecoin**.

There is a single oracle struct, `OraclePriceAccount`. Its value's scale is a configuration matter: `CollateralConfig.oracle_scale ∈ {Ray, Q64x64}`, converted to RAY in `auction_core`. Upstream has not yet reconciled the TWAP producer's Q64.64 with the host's 1e27 (lez-programs PR #401). The PoC uses `Ray`, and the `Q64x64` conversion is unit-tested.

```
valid(M) := oracle id == CollateralConfig.oracle_id
         && now − M.timestamp ≤ max_oracle_age_ms
         && M.price > 0
         && M.confidence_interval · 1e4 ≤ max_conf_bps · M.price      (confidence-adjusted half of R2)
         && R/2 ≤ M.price ≤ 2R                                         (catches mis-scaled/inverted feeds)
P := clamp(M.price, R·lo_bps/1e4, R·hi_bps/1e4)          default lo = 9000, hi = 11000
collateral_out(coin, t) := floor(coin · P / (RAY − d(t)))
```

- R comes from the host's `compute_current_redemption_price_wide` at clock time. The engine asserts it fits in `u128`.
- If `valid(M)` is false, the engine instance is **paused** (§5.7). Operations revert with `PriceFeedStale`, `PriceFeedLowConfidence` or `PriceFeedOutOfRange`. There is **no fallback price**: RFP R2 requires a pause. This deliberately overrides research-6 Q1's fallback to R.
- **Honesty note for R2.** "Time-weighted" depends on the feed. The twap_oracle-produced account is time-weighted, but it cannot be used live until the scale question is settled. The confidence check satisfies the "confidence-adjusted" wording, but it never triggers with today's producers, which write 0. Both points are stated in `REQUIREMENTS.md`.
- **When the lot binds**, the bidder receives the remaining lot and is charged `ceil(lot · (RAY − d)/P)`. They are not charged their full input.

### 5.3 Discount schedule (ADR-006)
- A piecewise-linear table of up to 8 `(elapsed_ms, bps)` points. Time is strictly increasing and the discount non-decreasing. A hard maximum of 3000 bps is checked in code.
- The schedule is **snapshotted into each auction**.
- Default (RAI): 2% → 10% over 45 min, then held.
- `d` is interpolated in RAY and rounded down. It is evaluated at the auction's **effective elapsed time** (§5.7).

### 5.4 Instances and accounts
**One engine instance (`EngineConfig`) per host stablecoin instance (F7).** The host has one collateral type, so each instance has exactly one `CollateralConfig`. The struct is kept separate so that a multi-collateral host can add more later. **R4/R5 ("only that type pauses") are demonstrated with two host instances using different collateral, each with its own engine instance** (§9). `REQUIREMENTS.md` states this honestly.

| Account | Seeds | Holds |
|---|---|---|
| `EngineConfig` | `["CONFIG", salt]` | admin; host program id + `ProtocolParameters` id; stablecoin def; `protocol_token: Option<def>`; global cap and `global_reserved`; surplus/debt auction params; `freeze_on_bad_debt` |
| `CollateralConfig` | `["COLL", config, collateral_def]` | oracle id, `oracle_scale`, max age, `max_conf_bps`, band; penalty; keeper tip/chip/cap; type cap and `type_reserved`; schedule; ttl; min bid; `debt_floor`, `chost`; `admin_paused`; **pause clock** `paused_since_ms: Option`, `paused_total_ms`, `last_seen_oracle_ts` |
| `Accounting` | `["ACCT", config]` | `unbacked_debt`, `total_shortfall_booked`, `total_penalty_collected`, `total_debt_burned`, `total_debt_settled`, `surplus_nonce`, `debt_nonce`, `host_frozen_by_engine` |
| `SurplusBuffer` holding | `["BUFFER", config]` (stablecoin) | penalty coins |
| `ProtocolCollateral` holding | `["PCOLL", config, collateral_def]` | unsold collateral from final expiries |
| `LiquidationRecord` | `["LIQREC", config, position]` | `count: u64` |
| `Auction` | `["AUCTION", config, position, nonce]` | status; position; owner; keeper; `collateral_seized/sold/left`, `keeper_reward`, `leftover_claimable/claimed`; `debt_principal`, `amount_to_raise`, `reserved_amount`, `debt_raised`, `penalty_raised`; `start_ms`, `ttl_ms`; schedule snapshot; `type_paused_total_at_start` |
| `AuctionVault` holding | `["VAULT", auction]` (collateral) | lot |
| `SurplusAuction` / `DebtAuction` | `["SURP"/"DEBT", config, nonce]` | §5.11 |

**Auction addressing (ADR-004).** Account addresses must be known when the transaction is signed. A liquidated position can be refilled and liquidated again, and accounts can never be released. So `liquidate(position, nonce)` asserts `nonce == LiquidationRecord.count` and then increments the count. The record is created on first use. A clock-based seed is unknowable at signing time. A global sequence would make concurrent keepers' precomputed addresses collide, so one transaction would fail; the sequencer does not "race", it just rejects the stale address.

**No `live_auctions` list (ADR-008).** Clients find auctions by block scan (§6.4). An on-chain list could be griefed into capacity exhaustion. It would also cost cycles to serialise and hash.

**Isolation (R3).**
- A non-final `bid` writes only its `Auction`, `AuctionVault`, the `SurplusBuffer`, the bidder's holdings, and `Accounting` if spike S6 picks per-bid updates.
- A chain-ending transition also releases the cap reservation and writes `Accounting`.
- Caps change only at `liquidate` (reserve) and at chain end (release).

### 5.5 Collateral auction state machine (F6)

```
liquidate ──► Auctioning ──bid (partial)──┐
                 │  ▲─────────────────────┘
                 ├─ raised ≥ amount_to_raise, or collateral_left = 0 ──► Settled (terminal)
                 └─ t_eff ≥ ttl, permissionless expire ──────────────► Expired (terminal)
```

- **No restarts (ADR-012).** The price is re-read at every bid, so a restart would only reset the discount from its maximum back to the start, on a lot nobody bought at the maximum. Maker's `redo` exists because Maker fixes the auction's starting price, which we don't.
- Both terminal states end the auction and apply the **chain-end effects** (§5.8) exactly once.
- Every handler asserts the status first. Terminal accounts are immutable except for `leftover_claimed`.

### 5.6 Collateral instructions

| Instruction | Signer | Effect (compute from pre-state; checks before effects; chained calls last) |
|---|---|---|
| `init_config` / `set_collateral_config` | admin | Bounds-checked. `admin_paused` opens or closes the pause clock (§5.7). Schedule changes affect only new auctions. **No admin path moves auction collateral or buffer funds.** |
| `liquidate(position, nonce)` | anyone | 1. Pause observation; the feed must be valid. 2. Read the position's collateral `pc` and nominal debt `D` (rounded up), and the **host vault's actual balance `c ≥ pc`**. 3. `amount_to_raise = ceil(D·(1e4+penalty)/1e4)`. **Cap rule:** allowed if `amount_to_raise ≤ cap − reserved` for both type and global, **or** if the type's and global reservations are both 0 (an oversized position can always be liquidated alone). Otherwise revert `CapacityExhausted`. 4. Record nonce check and increment. 5. Keeper reward (§5.9). 6. Write the `Auction` (lot `c − reward`) and reserve. 7. Chained calls: initialise the `AuctionVault` → `stablecoin.liquidate_position(pc, normalized_debt)`, which **sweeps the whole vault balance `c`** into `AuctionVault` → `Token::Transfer(reward)` to the keeper. |
| `bid(auction, coin_in_max, min_collateral_out)` | bidder | Pause observation; the feed must be valid; status must be `Auctioning`. Compute `d(t_eff)`, `P`, the fill and the charge. Reject if below `min_collateral_out`; if the charge is below `min_bid` unless it finishes the auction; or if it leaves `0 < remaining < chost`. **Debt first:** `Burn` the debt part from the bidder → `Transfer` the penalty part to `SurplusBuffer` → `Transfer` the collateral to the bidder. If the auction ends, chain-end effects apply in the same transaction. |
| `expire(auction)` | anyone | Pause observation. Requires `t_eff ≥ ttl`, which cannot happen while paused. Sets `Expired` and applies chain-end effects. |
| `observe_feed()` | anyone | Pause observation only (§5.7). |
| `claim_leftover(auction)` | position owner | Pays `leftover_claimable − leftover_claimed` (pull model). |
| `settle_debt(amount)` | anyone | Burns `min(amount, buffer_balance, unbacked_debt)` from the buffer. **Uses the actual buffer balance, which may exceed the book value because of donations.** |

### 5.7 Pause and frozen discount (ADR-007)
- **Paused** means `admin_paused` or `!valid(M)`. While paused, `liquidate`, `bid`, `expire` and collateral-dependent system operations revert for this instance only. Other instances and all non-liquidation host operations continue.
- **The pause clock lives in `CollateralConfig`** (it is per type, and there is one type per instance). `t_eff = (now − start) − (paused_total − auction.type_paused_total_at_start) − (paused_since ? now − max(paused_since, start) : 0)`, floored at 0. `ttl` uses `t_eff` too.
- **Admin pause** is exact: the admin transaction carries the clock.
- **Observation:** `liquidate`, `bid`, `expire` and `observe_feed` open the clock when the feed is invalid. They close it when the feed is valid and there is no admin pause.
- **When a stale period starts is decided in spike S5:**
  - (a) at observation time;
  - (b) at `last_seen_oracle_ts + max_age`;
  - (c) (b) bounded by the oracle's history.

  Acceptance: a property test shows that an adversary controlling oracle-update **or** observation timing gets no more discount than the schedule gives over unpaused time, within one block. If only (a) passes, the app and keepers call `observe_feed` on every refresh, and the residual risk is documented.

### 5.8 Accounting and chain end (R6)
**Chain-end effects** (Settled or Expired, applied once):
1. `shortfall = max(0, debt_principal − debt_raised)`. Then `unbacked_debt += shortfall` and `total_shortfall_booked += shortfall`.
2. Release `reserved_amount`.
3. Collateral:
   - target reached: `leftover_claimable = collateral_left`, owed to the owner;
   - lot exhausted: nothing remains;
   - Expired: the remaining lot is transferred to `ProtocolCollateral`. It **does not go to the defaulted owner.** It stays protocol-owned. Its unbacked debt is covered by F4 debt auctions, and a direct sale path is in sub-project B.

**Other accounting:**
- The buffer receives only penalty coins. `settle_debt` nets the buffer against `unbacked_debt`.
- **Supply invariant:** `stablecoin_supply ≤ Σ position nominal debt + Σ live (debt_principal − debt_raised) + unbacked_debt`.
- Stability fees are not routed to the buffer in the PoC (research-6 Q2).

### 5.9 Keeper reward
- `reward_coin = min(tip + chip_bps·D/1e4, reward_cap, penalty_bps·D/1e4)`, paid only if `D ≥ debt_floor`.
- `reward_coll = min(floor(reward_coin · P / RAY), c)`. **The cap at `c` prevents underflow on deeply underwater positions.** A property test covers it.
- The reward is carved from the penalty, so self-liquidation never pays when the lot covers the penalty. With a shortfall, the reward comes out of collateral that would otherwise reduce bad debt. `reward_cap` bounds this, and it is noted in `THREAT_MODEL.md`.
- `chost = ceil(debt_floor·(1e4+penalty)/1e4)`. `debt_floor` is an engine parameter, because the host has none.

### 5.10 Stablecoin hooks (vendored `64c0120`, offered upstream)
**Placement.** New variants are appended at the end of `stablecoin_core::Instruction` **and** the guest functions are appended in the same order, so the enum index equals the IDL index for the new instructions. A CI test checks the order (§3.2 spel-cli fact).

**Engine state.** A new PDA, `LiquidationEngineState { engine_authority_id }`. `ProtocolParameters` is unchanged.

**`set_liquidation_engine(engine_authority_id)`**
- The admin signs. The all-zero id is rejected.

**`liquidate_position(expected_collateral_amount, expected_normalized_debt_amount)`**
- The engine authority signs. How it is authenticated is decided in spike S2: `caller_program_id`, an engine PDA signer, or both.
- The position PDA is re-derived. Debt is greater than 0, `expected_* == Position.*`.
- Vault PDA and definition match. **The vault balance must be ≥ `Position.collateral_amount` (not equal).**
- The custody holding is already initialised with the same definition.
- The oracle is fresh.
- **Health is evaluated with four read-only accounts** (`stability_fee_accumulator`, `redemption_price_state`, `protocol_parameters`, `CLOCK_01`). It reuses the U512 projection from `withdraw_collateral.rs` (lines 179–194) as a boolean, and requires the position to be **not** collateralised at `minimum_collateralization_ratio`.
- Effects: zero the position, then **transfer the entire vault balance** (sweeping donations) to custody.
- Allowed while the host is frozen (it reduces risk). This needs maintainer confirmation.
- Upstream discussion points: there is no separate liquidation ratio (it uses min_CR); liquidation while frozen.

**`engine_freeze_new_debt()`**
- The engine authority signs.
- It sets the host's **existing** `ProtocolParameters.is_frozen` flag. The existing instructions already check that flag, so **no existing instruction's account list changes**. Adding a new account to `generate_debt` would break upstream decoders and fixtures.
- The hook can only freeze. The host's single `freeze_authority_account_id` slot is untouched and stays the only way to unfreeze, through the existing `unfreeze`.
- This is the F4 "pause new debt generation" path.
- **Caveat for the upstream discussion:** the host's freeze also blocks operations other than new debt (per its §7). Whether that is acceptable for F4, or whether upstream prefers a narrower flag, is raised in the PR.
- The test asserts exactly the frozen behaviour the host already defines.

### 5.11 F4 debt auction and F5 surplus auction (instant Dutch, mock token)
Both are instant-settling Dutch auctions. Nothing is escrowed and nothing is refunded later, so they stay compatible with the single-use-account privacy rule. Each is parameterised by `EngineConfig.protocol_token: Option<TokenDefinitionId>`, as F4/F5 require. **At most one surplus and one debt auction are live at a time**, addressed by `Accounting.surplus_nonce` / `debt_nonce`.

**Reference price for the protocol token.** `EngineConfig.protocol_token_oracle_id` points to an `OraclePriceAccount` quoting stablecoin per protocol token. It goes through the same validity checks. A stale feed pauses system auctions only.

**Surplus auction (F5)**
- **Start:** `start_surplus_auction(nonce)`, permissionless. Requires:
  - `protocol_token.is_some()`;
  - `unbacked_debt == 0` after an implicit `settle_debt`;
  - `buffer_balance ≥ surplus_buffer_target + surplus_lot`;
  - `now ≥ last_surplus_end + surplus_delay`.

  It moves `surplus_lot` stablecoin from the buffer into the auction's holding.
- **Pricing:** the protocol tokens required per lot start at `ref_price × (1 + surplus_premium)` and decay by the schedule to a floor of `ref_price × (1 − surplus_max_discount)`.
- **`take_surplus(amount_coin, max_protocol_in)`:** the taker pays protocol tokens, **burned in the same transaction**, and receives stablecoin. Partial takes follow the same dust rule.
- **End:** when the lot is empty → Settled; at ttl → `close_system_auction` returns the remainder to the buffer.
- Without a protocol token, surplus just accumulates in the buffer.

**Debt auction (F4)**
- **Start:** `start_debt_auction(nonce)`, permissionless. Requires:
  - `protocol_token.is_some()`;
  - `unbacked_debt − buffer_balance ≥ debt_lot`, after `settle_debt`.
- **Pricing:** the protocol tokens offered for `debt_lot` stablecoin start at `ref_price⁻¹ × debt_lot × (1 − debt_initial_discount)` and **grow** by the schedule up to `max_mint_per_auction`.
- **`take_debt(amount_coin, min_protocol_out)`:** the taker pays stablecoin, which is **burned** and reduces `unbacked_debt`. In return the engine **mints** mock protocol tokens; its PDA is the mint authority.
- **End:** at ttl → `close_system_auction` closes it, and a new one can be started.

**`check_solvency()`** (permissionless)
- After `settle_debt`, if `unbacked_debt > 0` **and** (`protocol_token.is_none()` **or** `freeze_on_bad_debt` with the debt auction unable to start), it calls `stablecoin.engine_freeze_new_debt()` and sets `host_frozen_by_engine`.
- This implements "until a protocol token is configured, the surplus buffer absorbs shortfalls up to its capacity; if bad debt exceeds the buffer, the system pauses new debt generation".

**System invariants**
- The protocol tokens burned equal the sum of surplus take payments.
- The protocol tokens minted in a debt auction never exceed `max_mint_per_auction`, and each debt auction reduces `unbacked_debt` by exactly the coins burned.
- The buffer balance never goes below 0. `min(buffer, unbacked)` is 0 after `settle_debt(MAX)`.

### 5.12 Defaults (bounded in code)

| Parameter | Default | Bound |
|---|---|---|
| penalty | 1000 bps | 0–2500 |
| schedule | 2% → 10% / 45 min | ≤ 8 pts, max ≤ 3000 bps |
| ttl | 4 h effective | 10 min – 7 d |
| min bid / debt floor / chost | 25 / 100 / 110 LSC | chost ≥ min bid |
| keeper tip / chip / cap | 5 LSC / 10 bps / 50 LSC | ≤ penalty |
| max oracle age / max conf / band | 60 min / 100 bps / 9000–11000 | ≤ 24 h / ≤ 1000 / [5000, 20000] |
| type cap / global cap | 60,000 / 250,000 LSC | type ≤ global |
| surplus target / lot / delay / premium / max discount | 1,000 / 500 LSC / 2 h / 500 bps / 2000 bps | — |
| debt lot / initial discount / max mint per auction | 500 LSC / 0 / 10× lot at ref price | hard cap in code |

### 5.13 Invariants (`auction_core` proptest; asserted on-chain where cheap)
1. `collateral_seized == keeper_reward + collateral_sold + collateral_left`. Sold is tracked independently of left, and claims never reduce `left`.
2. The vault balance is ≥ `collateral_left − leftover_claimed` (≥, because donations are possible). Terminal sweeps take the **whole balance**.
3. Each fill: `charged ≤ coin_in_max`, `out ≥ min_out`, and `raised ≤ amount_to_raise`.
4. `d` is monotone in `t_eff` and bounded. `P` stays within the band.
5. Status only moves forward; terminal states are absorbing.
6. After each bid, the remaining amount is 0 or ≥ `chost`.
7. `Σ live reserved == type_reserved` and `Σ type == global_reserved`. A reservation is made once and released once. Reservations exceed the caps only through the oversized-alone rule, so at most one oversized auction exists per instance.
8. At chain end, `debt_raised + shortfall == debt_principal`, and `total_shortfall_booked == Σ shortfall`.
9. Buffer conservation: book buffer = penalties − settled burns − surplus lots moved out + surplus remainders returned. The actual balance is ≥ the book value.
10. Host and engine conservation: the change in host debt equals Σ principal seized; the change in host collateral equals Σ vault balances swept.
11. The supply invariant (§5.8), including debt-auction burns.
12. Liveness (fuzz): an unsafe position with a valid feed is always liquidatable, either within the caps or alone.
13. Split-bid neutrality.
14. Frozen clock: `t_eff` never advances while paused, whether by admin or by the feed.
15. Collateral destinations are only: a bidder, the keeper, the owner (leftover after the target is reached), or `ProtocolCollateral`.
16. Keeper reward ≤ min(penalty value, `c`).
17. F4/F5 system invariants (§5.11).

## 6. Off-chain layers

### 6.1 `auction_sdk`
- PDA derivation and decoders for every account.
- Quotes that call the same `auction_core` functions the program uses (liquidation, bid, surplus take, debt take).
- **Its own instruction encoders**, with enum-index encoding verified against the program's enum and IDL in CI. It does not depend on spel-cli.
- Plan builders that output `{program_id, account_ids, signing_requirements, instruction_words}`.
- The block scanner (§6.4) and transaction confirmation (§6.5).

### 6.2 `auction_ffi`
- One `char* op(const char* json)` per SDK function, plus `auction_ffi_free`.
- Results come back in the envelope `{"ok":…,"value"|"error"}`, wrapped in `catch_unwind`.
- Big integers travel as decimal strings.
- The cbindgen header is committed, and CI fails on drift.

### 6.3 `auction_engine_module` (thin)
- Methods: `getConfig`, `listAuctions`, `getAuction`, `quoteBid`, `bid`, `liquidate`, `quoteLiquidation`, `rescan`, `awaitTx`.
- Every declaration stays on one line, and a test calls every method.
- Each method returns `{status, error, …}`.
- Submission uses the 4-argument `send_generic_public_transaction` from lez_core 0.4.1.

### 6.4 Discovery (ADR-008)
- **Positions:** a `getBlockRange` scan keeps transactions that list the host's `ProtocolParameters`.
- **Auctions:** the same scan keeps transactions that list our `EngineConfig`, which every `liquidate` includes. Auction addresses are then derived from `(position, nonce)` and their status is read.
- A persisted cursor tracks the last scanned block. The UI shows "Scanned to block N · Rescan".
- Production uses the indexer's `getTransactionsByAccount`, then v0.3 events.

### 6.5 Transaction confirmation (ADR-009)
1. Record the signer nonce `n0` and the block height `h0`.
2. Submit, then poll the nonce about every 2 s, checking that the target account changed.
3. `nonce > n0` → **Included**. Still at `n0` at `h0 + 5` → **Dropped**: nothing was applied, so re-quote.
4. Allow one in-flight transaction per signer.

This switches to `poll_transaction_status` after the v0.3 port.

### 6.6 `auction_cli` (U3)
- Built on `auction_sdk` encoders, not spel-cli, which mis-encodes stablecoin instructions (§3.2).
- Commands:
  - `quote-liquidation`, `liquidate`;
  - `quote-bid`, `bid`, `expire`;
  - `list-auctions`, `list-at-risk`, `rescan`;
  - `settle-debt`, `check-solvency`;
  - `start-surplus`, `take-surplus`;
  - `start-debt`, `take-debt`;
  - `observe-feed`;
  - `watch`.
- Signing goes through the LEZ wallet keystore.
- Usage is documented in `cli/README.md`.

## 7. Basecamp app (thin, ADR-013)
- **Built in the PoC:**
  - the **Live auctions** master–detail view (KPI strip, discount chart, bid card with live preview);
  - a compact public-bid confirmation;
  - the `awaitTx` states (Included / Dropped → Re-quote);
  - a "Liquidate" action that takes a position id (no At-risk browsing view).

  It is a `ui_qml` app with a C++ `.rep` backend and the `Logos.Theme`/`Logos.Controls` tokens per `docs/design/ui/ui-review.md`. The primary button has a black label.
- **Designed, not built:** the At-risk positions view, the full confirmation dialog with the private variant, and all state variants (`ui-v4.png`). They are milestone M5, or the separate UI RFP.
- Budget: about 2 days. If time runs short, the CLI demo carries U2–U5, and the GUI shows only the Live view and the bid flow.

## 8. Privacy (PoC scope)
- The PoC submits public transactions only. The UI says so.
- The design keeps the private path possible without rework: every user action settles instantly, and F4/F5 are instant Dutch auctions with no escrow or refunds. The only pull claim is the position owner's leftover.
- The private SDK and UI are sub-project B (designs already exist).

## 9. Testing

| Layer | Scope | Tooling |
|---|---|---|
| `auction_core` | math, schedule, collateral, surplus and debt state machines, accounting; invariants 1–17 | TDD; `proptest`; differential model (Python big-int); Kani for `mul_div` and `d(t)` |
| Stablecoin hooks | non-engine caller, zero binding, healthy position, mismatched amounts, stale oracle all rejected; **donation into the vault still liquidates and sweeps**; `engine_freeze_new_debt` sets `is_frozen` with exactly the host's existing frozen semantics, and only the freeze authority can unfreeze; enum/IDL order test; **upstream suite unchanged** | SPEL tests + lez-programs suite |
| `auction_engine` | full path on the real stablecoin; partial bids; lot binds; dust; min bid; expire → `ProtocolCollateral`; lot exhausted with shortfall; re-liquidation; caps incl. oversized-alone; reward ≤ c on an underwater position; admin and feed pause with frozen discount; low-confidence and out-of-range pause; **R4/R5 with two host+engine instances on different collateral**; R3 concurrent auctions; `settle_debt`; **surplus auction with mock token (burn)**; **debt auction (mint, cap)**; **`check_solvency` → host freezes new debt when no token is set**; R6 conservation | `integration_tests`, standalone v0.2.4 sequencer, dev mode |
| Real proofs | one full end-to-end path per push to `main`; full suite nightly and on tags | `RISC0_DEV_MODE=0` |
| SDK / FFI / CLI | quote equals the on-chain result; encoder index equals program enum and IDL; JSON fixtures; panic safety; header drift; CLI golden tests | Rust tests |
| C++ module | every method; error mapping; `awaitTx` | `LOGOS_CMOCK` + `mockModule("lez_core")` |
| QML / Basecamp | Live view and bid flow; Basecamp offscreen end-to-end | `tst_*.qml`; `run-basecamp-e2e.sh` + logos-qt-mcp |
| Requirements | each hard requirement has ≥ 1 named test, or an explicit descope | `REQUIREMENTS.md` checker |

TDD throughout; ≥ 80% coverage on the Rust crates.

## 10. Security
`THREAT_MODEL.md` covers:
- the 11 ranked attacks from research-3, plus the template's C-class (griefing price discovery) and F-class (sniping);
- **donation griefing**;
- **list-capacity griefing**, avoided by having no list;
- **the absence of a reentrancy guard in LEZ v0.2.4**: R1 rests on our programs making no callbacks, and on the host never calling the engine;
- **the trust that PDA authorisation inherits** through the stablecoin;
- **the oversized-alone cap rule** (one such auction at a time);
- **keeper reward in a shortfall**;
- **residual pause-onset risk** (S5);
- **mock-token mint authority**.

**Per-instruction checklist** (`docs/architecture/security-checklist.md`):
- time only from `CLOCK_01`;
- oracle id pinned and fully validated;
- every PDA re-derived;
- owner and definition checks;
- zero caller id rejected;
- **"at least X, sweep everything" for any holding a third party can raise. Never an equality check on a balance**;
- checks → effects → chained calls;
- caps;
- no dust;
- rounding directions;
- frozen-discount property;
- no admin path to auction, buffer or `ProtocolCollateral` funds.

**Process:**
- a security-reviewer subagent on every on-chain change;
- a red-team pass (`security-redteam`) before submission;
- `cargo-fuzz` on decoders and quote paths;
- `cargo-audit`/`deny`;
- `clippy -D warnings`.

## 11. Benchmarks and interoperability
- `CYCLES.md` records cycles per instruction against **32 Mi**, plus transaction sizes (P1–P3). CI fails on a regression greater than 10%. The harness is committed (PEND10).
- Load test: N concurrent auctions with interleaved bids.
- Interop: the vendored stablecoin suite; the real `lez_core` 0.4.1 in end-to-end tests; IDL and encoder drift checks.

## 12. Documentation (kept current at every task)
- `docs/design-log.md` holds ADR-001…013 and every spike outcome.
- `docs/architecture/*.md` explain how each layer works.
- Also maintained: `README.md` (S5), `REQUIREMENTS.md`, `SUBMISSION.md`, `DEMO.md`, `CYCLES.md`, `THREAT_MODEL.md`, `docs/v03-port-plan.md`, `docs/PRIVACY.md` (stub) and `cli/README.md`.
- `docs/outreach/` holds drafts of public messages, which the user posts.

## 13. Spikes (week 1)

| Spike | Question | Decided by |
|---|---|---|
| S1 | Does the toolchain build and run end to end (guest, sequencer, stablecoin init, Basecamp harness with lez_core 0.4.1)? How many chained calls does `liquidate` really take? | Running it |
| S2 | How does the host authenticate the engine: `caller_program_id`, PDA signer, or both? | Unforgeability tests + cycles + upstream fit |
| S3 | Can SPEL create `LiquidationRecord` on first use (`Claim::Pda`) and derive arg-seeded auction PDAs? | A prototype |
| S4 | Discount curve: piecewise-linear vs exponential vs stairstep | Cycles + accuracy |
| S5 | When does a stale period start? | The §5.7 property test |
| S6 | `Accounting` updated per bid, or only at chain end? | Concurrency + R6 proof |
| S7 | Mock protocol token mint authority as an engine PDA; cost of mint and burn in chained calls | A prototype + cycles |

## 14. Schedule (≈ 3 weeks, solo)
- **Week 1:** spikes S1–S7; `auction_core` (collateral, surplus and debt logic plus property tests); stablecoin hooks with the upstream suite green; design log; outreach note drafted.
- **Week 2:** `auction_engine` with all instructions and integration tests (including F4/F5 and two-instance R4); SDK encoders and CLI; CI including the per-push real-proof job and drift checks.
- **Week 3:** benchmarks; security and red-team pass; thin module and app (about 2 days) with the Basecamp end-to-end test; `SUBMISSION.md`, demo, draft proposal; upstream PR and the two upstream issues.

Execution goes through `writing-plans`, then `subagent-driven-development`: implementer (TDD) → spec reviewer → code-quality reviewer (+ security reviewer for on-chain code) → verification.

## 15. Proposal mapping

**Pricing (ADR-014).** About **$40k** for the build, plus a **separate audit line of about $10–12k** naming a tier-1 firm, which Logos may contract directly. This is below both open bids ($48k, $55k) without matching bleu's discount for two RFPs combined. A variant without M5 comes to about **$35k**.

| Milestone | Duration | Payout |
|---|---|---|
| M0 Spec, spikes, upstream hook PR + issues | 1 wk | $3,000 |
| M1 Collateral auctions with real token movement on the stablecoin (PoC hardened) | 2 wk | $8,000 |
| M2 Prices: live TWAP once scales are reconciled, multi-oracle (SOFT1), pause hardening | 1.5 wk | $5,000 |
| M3 F4/F5 hardening, `ProtocolCollateral` sale path, solvency/freeze (PoC hardened) | 1 wk | $4,000 |
| M4 Private-path SDK (PRIV1–3, PA1–4), bot SDK (SOFT3) | 2 wk | $7,000 |
| M5 Full Basecamp GUI per approved designs, catalog (**separable**) | 1.5 wk | $5,000 |
| M6 v0.3 port + testnet (S1), full real-proof suite, `CYCLES.md`, `PRIVACY.md`, Kani (SOFT2) | 2 wk | $5,500 |
| M7 Audit remediation | 1 wk | $2,500 |
| **Build total** | **12 wk** | **$40,000** |

**Traceability** (PoC = shown in the PoC; M# = promised; D = descoped):

| ID | Status | Where |
|---|---|---|
| F1 | PoC | `liquidate`, keeper reward |
| F2 | PoC | §5.3, early settle |
| F3 | PoC | partial fills, lot-binds charge |
| F4 | **PoC** (buffer, debt auction with mock token, freeze new debt) · M3 hardening | §5.8, §5.11 |
| F5 | **PoC** (surplus auction with mock token, burn) · M3 hardening | §5.11 |
| F6 | PoC | §5.5 refusal tests |
| F7 | PoC | hooks; one engine instance per host |
| U1 | PoC | SPEL program + IDL; order check |
| U2 | PoC (thin) · M5 full | `apps/auctions` |
| U3 | PoC | `auction_cli` (own encoders) |
| U4 | PoC (CLI `list-at-risk`, GUI liquidate-by-id) · M5 GUI view | block scanner |
| U5 | PoC | Live view, `quote-bid` |
| U6 | PoC (public SDK) · M4 private | `auction_sdk` |
| R1 | PoC | no callbacks; documented, since there is no platform guard |
| R2 | PoC (validity incl. confidence, band, pause) · M2 live TWAP | §5.2 honesty note |
| R3 | PoC | per-auction PDAs, concurrency test |
| R4/R5 | PoC (two host+engine instances; one type per host stated honestly) | §5.4 |
| R6 | PoC | §5.13 |
| P1/P3 | PoC | single-transaction instructions, `CYCLES.md` |
| P2 | PoC | load test |
| S1 | M6 (blocked by v0.3) | localnet reproduction in the PoC |
| S2 | PoC | real-proof CI job |
| S3/S4/S5 | PoC | CI, checker, README |
| PRIV1–3, PA1–4 | M4 | designs exist |
| SOFT1 / SOFT2 / SOFT3 | M2 / PoC partial + M6 / M4 | — |
| PEND1–12 | §12 / M5 / M6 | research-5 §1 |

## 16. Risks

| Risk | Mitigation |
|---|---|
| RFP reopens with a changed scope (UI split, new items) | Engine-first; GUI separable; engage early; follow spec PRs quickly |
| Upstream stablecoin churn | Pin `64c0120`; clean patch series; rebase weekly |
| A finished PoC doesn't win alone | Engage early (§1.6); price below the open bids; scope discipline |
| v0.3 port | `auction_core` isolation, SDK encoder seam, port plan |
| Oracle scale, weak freshness, zero confidence | Validity checks + band + `[R/2, 2R]`; honest R2 note; TWAP live in M2 |
| F4/F5 add week-2 load | Instant-Dutch design reuses the schedule and dust code; GUI shrunk to about 2 days |
| Pause-onset precision | S5 property test; documented residual |
| Solo throughput, WSL memory | Strict scope, subagent-driven execution; `.wslconfig` + swap |
| lez_core skew vs the catalog wallet | Clean Basecamp profile; fixed by the v0.3 port |

## 17. Decisions (seed for `docs/design-log.md`)
- **ADR-001** Track B toolchain, with a pure core crate.
- **ADR-002** Standalone repo; reusable engine plus appended host hooks offered upstream.
- **ADR-003** GEB increasing-discount auction with Maker rails: reward carved from the penalty, caps, dust rule, slippage bounds.
- **ADR-004** `LiquidationRecord` nonce addressing.
- **ADR-005** Band-clamped market price, collateral per coin. The feed must be valid (age, confidence, band, `[R/2, 2R]`). No fallback.
- **ADR-006** Piecewise-linear schedule, snapshotted per auction.
- **ADR-007** Type-level pause clock; frozen discount and ttl.
- **ADR-008** No on-chain auction list; block-scan discovery.
- **ADR-009** Nonce-based transaction confirmation.
- **ADR-010** Debt-first payment; penalty to the buffer; engine-only accounting.
- **ADR-011** UI on Basecamp's colour grid and the AppManagerView layout.
- **ADR-012** Full seizure with the oversized-alone cap rule; vault sweep (≥, not =); no restarts; expiry sends the lot to `ProtocolCollateral`; shortfall at every end.
- **ADR-013** Engine-first PoC with a thin GUI; F4/F5 in the PoC.
- **ADR-014** Pricing at about $40k plus a separate audit line; a variant without the GUI.

## 18. References
- RFP-014 and its appendix.
- ecosystem #185, #200, #205, #210, #224.
- Template PRs #208, #209, #45.
- Proposals: #187 (Antumbra), #159 (Web3spell), #118/#119 (bleu), #141 (Argo), #117 (Kanon), #112 (Gateway).
- Code: LEZ v0.2.4, lez-programs `64c0120`, spel `1ef0500f`, logos-execution-zone-module `acf0cd50`, logos-module-builder, logos-design-system, reflexer-labs/geb, makerdao/dss.
- `docs/research/`, `docs/design/ui/`.

## 19. Review history
- **v1 → internal spec review (2026-10-01):**
  - partial vs full seizure;
  - shortfall at every end;
  - stranded expiry;
  - leftover to the owner;
  - cap vs bid write set;
  - admin pause clock;
  - `chost` definition.
- **External review (2026-10-01).** Claims re-checked against the code: the donation absorption is in the stablecoin `Deposit` docs; spel-cli encodes by IDL position (`tx.rs`); every `confidence_interval` producer writes 0. Changes made in v2:
  - strategy reshaped to engine-first with F4/F5 in the PoC and a thin GUI;
  - vault-donation griefing fixed (≥ and sweep);
  - oversized positions liquidatable alone;
  - `live_auctions` dropped;
  - restarts removed;
  - reward capped at `c`;
  - confidence check added;
  - one collateral per host stated, with R4 tested across two instances;
  - one oracle struct with a configurable scale;
  - own CLI encoders and an enum/IDL order check;
  - 32 Mi;
  - `poll_transaction_status` only after the v0.3 port;
  - health computed with four accounts using the U512 projection;
  - threat-model platform facts (no reentrancy guard, inherited PDA authorisation, sequential sequencer).
