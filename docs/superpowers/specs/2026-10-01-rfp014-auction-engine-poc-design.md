# RFP-014 Liquidation & Auction Engine: Proof of Concept (design spec)

| | |
|---|---|
| **Status** | Draft for review (brainstorming output) |
| **Date** | 2026-10-01 |
| **Author** | paschal533, with Claude |
| **Scope of this spec** | Sub-project **A**: the 3-week demo-ready PoC. Sub-project **B** (extensions) is listed in §2.3 and gets its own spec. |
| **Target RFP** | [logos-co/rfp RFP-014](https://github.com/logos-co/rfp/blob/master/RFPs/RFP-014-liquidation-auction-engine.md), status `closed`, "Reopening soon" (no date) |
| **Evidence base** | `docs/research/research-0..6-*.md`, `docs/design/ui/*` |

---

## 1. Goal and success criteria

**Goal.** Build a working, reproducible RFP-014 engine on the Logos Execution Zone (LEZ): on-chain program, SDK, CLI, Basecamp core module and Basecamp app. The engine liquidates real positions of the Logos-written stablecoin CDP. When RFP-014 reopens, we submit a proposal backed by this PoC that beats the open competing bids, Antumbra (#187, $55k) and Web3spell (#159, $48k).

**Success criteria for the PoC.**
1. A reviewer can run one script on a clean machine with Nix. It starts a standalone LEZ v0.2.4 sequencer, deploys the stablecoin (with our hooks) and the engine, opens a position, makes it liquidatable, liquidates it, places partial bids, settles, and shows the result. The same flow can be driven in Basecamp through our app.
2. A human-narrated demo video, recorded by the author and not AI-narrated, plus `SUBMISSION.md` with full, linked transaction hashes and reproduction steps.
3. CI is green on `main` and runs: unit and property tests, sequencer end-to-end tests in dev mode, one real-proof (`RISC0_DEV_MODE=0`) end-to-end job (nightly on `main` and on every tag; S2), C++ module tests, QML tests, a Basecamp offscreen end-to-end test, IDL and header drift checks, the cycle-count regression check, and the `REQUIREMENTS.md` coverage check.
4. Every RFP-014 hard requirement is mapped in `REQUIREMENTS.md` to a test (PoC), a milestone (promise), or a descoping with a reason.
5. The stablecoin's own upstream test suite passes unchanged with our hooks appended, and the hooks are offered upstream as a PR to `logos-blockchain/lez-programs`.

**What "better than the competition" means concretely** (from `research-5`):
- A real host: the Logos stablecoin, not a stand-in.
- Real token movement: vault custody, burn and transfer.
- Time comes only from `CLOCK_01`, never from the caller. The price is re-read at every bid.
- Sequencer end-to-end tests plus a real-proof job in CI.
- A real `ui_qml` Basecamp app using logos-design-system.
- Correct transaction confirmation (lez_core "success" is not inclusion; see §6.5).

## 2. Scope

### 2.1 In the PoC (sub-project A)
- `auction_core` pure Rust crate: math, discount schedule, state machine, accounting, invariants.
- `auction_engine` SPEL program: `init_config`, `set_collateral_config`, `liquidate`, `bid`, `expire` (which also restarts), `observe_feed`, `claim_leftover`, `settle_debt`.
- Stablecoin hooks, appended at the end of the instruction enum: `set_liquidation_engine` and `liquidate_position`.
- `auction_sdk` (Rust lib), `auction_ffi` (C ABI), `auction_cli`.
- `auction_engine_module`, a C++ universal core module.
- `apps/auctions`, a `ui_qml` app with three views: Live auctions, At-risk positions, and the Bid confirmation dialog. **Public path only.** The private toggle is rendered disabled with "Private bids arrive in M4".
- Tests, security review, benchmarks and docs as described in §9–§12.

### 2.2 Explicitly not in the PoC (promised in proposal milestones)
- F4 debt auction, F5 surplus auction, and the host pause/freeze hook. The PoC does implement the surplus **buffer** and unbacked-debt accounting, because R6 needs them.
- The private-path SDK (deshield → interact → reshield) for liquidate, bid and settle (U6, PRIV1–3, PA1–4).
- Public testnet deployment (S1). It is blocked by v0.3 (§3.2).
- Our own module catalog and release action (PEND5/6), Figma files (PEND3), the full `PRIVACY.md` (PEND4), and the audit (PEND8).
- SOFT1 multi-oracle, SOFT3 bot SDK as a product, and full Kani coverage. The PoC includes Kani for `mul_div` and the schedule only.

### 2.3 Sub-project B (next spec, after the PoC)
Debt and surplus auctions using instant Dutch auctions and a mock protocol token. Host pause hook plus `check_solvency` (F4). The private-path SDK and UI. The v0.3 port (once SPEL supports v0.3) and testnet deployment. The catalog. Full `PRIVACY.md` and threat-model coverage. Audit preparation.

### 2.4 Out of scope (RFP OOS1–4)
Flash-loan liquidations, liquidation aggregators and batching, insurance or socialised loss, and multi-collateral atomic liquidations.

## 3. Context and constraints

### 3.1 Toolchain pins (decision ADR-001, Track B)

| Component | Pin | Source |
|---|---|---|
| LEZ (`lee`, `lee_core`) | v0.2.4 `47eba256479f6f785acbd138834340703cd03401` | research-1 §3 |
| SPEL | `1ef0500f8fc8ce3ddf95523726ae159db83f744f` (matches lez-programs) | research-1 §2 |
| risc0-zkvm | `=3.0.5` | research-1 |
| Rust | 1.94.0 | research-1 |
| lez-programs (vendored stablecoin) | `64c01210adecd6b557dbd021a3cc94553f243fd8` | research-2 |
| `lez_core` module | `acf0cd501b262c4c15969e3735e85318297b85bf` (metadata 0.4.1) | research-4 decision 1 |
| logos-module-builder | the rev matching the Basecamp release used in end-to-end tests (resolved in spike S1) | research-4 |
| logos-design-system | the version Basecamp bundles (do not ship it in our package) | research-4 |

**Why not v0.3.0.** The public testnet moved to LEZ v0.3.0 on 2026-10-01. SPEL has no v0.3 support, and RFP U1 requires SPEL. The stablecoin host is on v0.2.4. Moving from v0.2.4 to v0.3.0 is a rewrite of the program layer (plan/apply entry, sharded accounts, Borsh, fees, every PDA). Mitigation: all business logic lives in `auction_core`, which has no LEZ dependency, so the port rewrites only the thin program layer, the SDK encoding and the `lez_core` adapter. This is documented in `docs/v03-port-plan.md`.

### 3.2 Environment facts that shape the design
- Chained calls: at most 10 per transaction (11 executions including the top level). 32M cycles per execution. Account data is limited to 100 KiB. **Chained calls return no data.** Any panic reverts the whole transaction (research-2).
- The callee sees `ctx.caller_program_id`, and the runtime enforces it. A top-level call has caller `DEFAULT_PROGRAM_ID` (all zeros) (research-2).
- `Token::Burn` needs only the holder's authorisation, not the mint authority (research-2).
- There is no clock in the SPEL context. Time comes from the `CLOCK_01` account in ms, passed as a read-only account with its id asserted. Within a block, the clock reads the previous block's timestamp (research-2/3).
- v0.2.4 has no RPC to list accounts by owner and no events (research-6 Q4).
- In lez_core 0.4.1, `success` means only that the transaction was accepted. Transactions that fail execution are silently dropped when the block is built. The signer nonce advances only when the transaction is applied (research-6 Q5).
- LEZ accounts can never be released: once claimed, an owner is permanent (research-6 Q3).
- Dev box: Windows 11 + WSL Ubuntu 20.04, Nix, about 7 GB RAM. Raise the WSL memory limit and add swap before the first Basecamp build. Guest builds use Docker (research-4).

## 4. Architecture

### 4.1 Layers (each depends only on the one below)

```
apps/auctions (QML, Logos.Theme/Controls)  ──►  C++ backend (.rep, ui-host)
                                                     │ logos.module("auction_engine_module")
auction_engine_module (C++ universal core module) ──► modules().lez_core (wallet: sign/submit/read)
        │ char* op(const char* json)
auction_ffi (Rust cdylib, JSON envelopes)
        │
auction_sdk (Rust: PDAs, decoders, quotes, plan builders, scanner, tx confirm)  ◄── auction_cli
        │
auction_core (pure Rust: math, schedule, state machine, accounting, invariants)  ◄── auction_engine program
                                                                                    (thin SPEL layer)
On-chain:  auction_engine ──chained──► stablecoin.liquidate_position ──chained──► token.Transfer
           auction_engine ──chained──► token.Burn / token.Transfer ; reads CLOCK_01, oracle, host state
```

### 4.2 Repository layout

```
lez-auction-engine/
  crates/auction_core/          pure, no LEZ deps; proptest + Kani
  crates/auction_sdk/           LEZ types: PDAs, decode, quotes, plan builders, block scanner, tx confirm
  programs/auction_engine/      SPEL v0.7 thin layer over auction_core
  programs/stablecoin/          vendored lez-programs@64c0120 + appended hooks (kept as a clean patch series)
  programs/integration_tests/   standalone v0.2.4 sequencer, dev mode + one real-proof test
  modules/auction_engine/{ffi/, src/, tests/, metadata.json, flake.nix, CMakeLists.txt}
  apps/auctions/{qml/, src/, tests/, metadata.json, flake.nix}
  apps/shared/wallet/           vendored from lez-programs (Logos.Wallet), patched only as needed
  cli/                          auction_cli (+ SPEL-generated CLI usage docs)
  scripts/                      localnet.sh, demo.sh, verify.sh, run-basecamp-e2e.sh
  idl/  artifacts/  DEPLOYMENTS.md  CYCLES.md  REQUIREMENTS.md  SUBMISSION.md  THREAT_MODEL.md
  docs/{design-log.md, architecture/*.md, v03-port-plan.md, PRIVACY.md, research/, design/ui/, superpowers/}
  flake.nix  rust-toolchain.toml  .github/workflows/
```

## 5. On-chain design

### 5.1 Units and math (`auction_core::math`)
- Amounts are raw token base units as `u128`. Prices are RAY = 1e27 fixed point. Discounts are in bps (0–10_000), converted to RAY by multiplying by 1e23. Time is in ms (`u64`).
- Every product goes through `mul_div_floor` / `mul_div_ceil` with a 256-bit intermediate. There is no `u256` type in storage and no external dependency.
- **Rounding rule:** anything paid out (collateral to bidders, keeper reward, leftovers) rounds **down**. Anything owed or charged (target, coins charged when the lot binds) rounds **up**.
- `overflow-checks = true` in release builds and `#![forbid(unsafe_code)]`.

### 5.2 Price used for bids (ADR-005)
The host's unit of account is the collateral. Both `redemption_price` R and the oracle market price M are **collateral per 1 stablecoin** at 1e27 (research-6 Q1).

```
valid(M)  := oracle id == CollateralConfig.oracle_id
             && now − M.timestamp ≤ max_oracle_age_ms
             && M.price > 0
             && R/2 ≤ M.price ≤ 2R
P         := clamp(M.price, R·lo_bps/1e4, R·hi_bps/1e4)        default lo=9000, hi=11000
collateral_out(coin, t) := floor( coin · P / (RAY − d(t)) )     (256-bit intermediate)
```

- R comes from the host's `compute_current_redemption_price_wide` evaluated at the clock time. The engine must assert it fits in `u128`.
- **If `valid(M)` is false, the collateral type is paused.** `liquidate` and `bid` revert with `PriceFeedStale` or `PriceFeedOutOfRange`. There is no fallback price; the requirement is RFP R2/R4. *This deliberately overrides research-6 Q1's "fall back to P = R when stale" (see ADR-005). Do not "fix" it back.*
- When the lot binds (the remaining collateral is less than `collateral_out`), the bidder receives the remaining lot and is charged `ceil(lot · (RAY − d) / P)`, not their full input. GEB charges the full bid here; we deliberately do not.
- The `TwapQ64` adapter (Q64.64 → RAY, plus checks on the underlying observations) is implemented and unit-tested in `auction_core`. It is **not** used live until upstream reconciles the oracle scales (lez-programs PR #401).

### 5.3 Discount schedule (ADR-006, confirmed by spike S4)
- A piecewise-linear table of up to 8 `(elapsed_ms, discount_bps)` points. It is strictly increasing in time, non-decreasing in discount, and capped by a hard maximum `MAX_DISCOUNT_BPS = 3000` checked in code.
- It is **snapshotted into each auction** when the auction starts, so parameter changes never affect a live auction.
- The default is RAI production: `[(0, 200), (2_700_000, 1000)]`, i.e. 2% → 10% over 45 minutes, then held.
- `d(t)` is interpolated in RAY and rounded down (favouring the protocol).
- `t` is the auction's **effective elapsed time**: wall time minus frozen time (§5.7).

### 5.4 Accounts (all PDAs under the engine program and all seeded by `config`)

| Account | Seeds | Holds | Written by |
|---|---|---|---|
| `EngineConfig` | `["CONFIG", salt]` | admin; host stablecoin program id; stablecoin token def; `global_cap`; `global_reserved`; `live_auctions: [AuctionId; 512]` + len | admin; `liquidate`; chain-ending transitions |
| `CollateralConfig` | `["COLL", config, collateral_def]` | oracle account id; max oracle age; price band lo/hi; penalty bps; keeper tip/chip/cap; per-type cap and `type_reserved`; schedule; ttl; min bid; `debt_floor`; `chost`; max restarts; `admin_paused`; **type-level pause clock:** `paused_since_ms: Option<u64>`, `paused_total_ms`, `last_seen_oracle_ts` | admin; `liquidate`; chain-ending transitions; pause observation (§5.7) |
| `Accounting` | `["ACCT", config]` | `unbacked_debt`, `total_shortfall_booked`, `total_penalty_collected`, `total_debt_burned`, `total_debt_settled` | bid (per spike S6, or only at chain end), chain-ending transitions, `settle_debt` |
| `SurplusBuffer` holding | `["BUFFER", config]` token holding (stablecoin) | penalty coins | bid (transfer in), `settle_debt` (burn) |
| `ProtocolCollateral` holding | `["PCOLL", config, collateral_def]` token holding (collateral) | unsold collateral after a final expiry; reserved for sub-project B (debt auction or admin-gated sale) | final expiry (transfer in) |
| `LiquidationRecord` | `["LIQREC", config, position]` | `count: u64` | `liquidate`, `expire` (when it restarts) |
| `Auction` | `["AUCTION", config, position, nonce: u64]` | status; position; owner; keeper; collateral def; `collateral_seized`, `collateral_sold`, `collateral_left`, `keeper_reward`, `leftover_claimable`, `leftover_claimed`; `debt_principal`, `amount_to_raise`, `reserved_amount`, `debt_raised`, `penalty_raised`; `start_ms`, `ttl_ms`; schedule snapshot; `type_paused_total_at_start`; `restart_index`; predecessor and successor | its own instructions |
| `AuctionVault` holding | `["VAULT", auction]` token holding (collateral) | seized collateral | token program via chained calls |

**Capacity of `live_auctions`.** When the list is full, `liquidate` reverts with `LiveAuctionsFull`. The capacity of 512 is fixed in the PoC; invariant 12 holds only below capacity. This is noted in `THREAT_MODEL.md`.

**Auction uniqueness (ADR-004, research-6 Q3).** A liquidated position can be refilled and liquidated again. `LiquidationRecord.count` gives each liquidation a new nonce. `liquidate` takes `nonce` as an argument, asserts `nonce == record.count`, and increments the count. Every address is derivable at signing time. Alternatives rejected:
- a clock- or block-id seed: unknown when the transaction is signed;
- a global sequence number: two keepers in the same block race and one fails, violating R3;
- a counter map on the host: an unbounded hot global account.

**Isolation (R3).** A **non-final** `bid` writes only its `Auction`, its `AuctionVault`, the `SurplusBuffer`, and the bidder's holdings, plus the `Accounting` totals if spike S6 chooses per-bid updates. A bid that ends the chain is itself a chain-ending transition and also writes the cap counters, `live_auctions` and `Accounting`. `EngineConfig.global_reserved` and `CollateralConfig.type_reserved` change only at `liquidate` (reserve) and at chain end (release). Caps therefore count the reservation made at liquidation, not the amount still to raise (Maker's `digs`-per-take is not used).

### 5.5 State machine (F6)

```
liquidate ──► Auctioning ──bid (partial)──┐
                 │  ▲─────────────────────┘
                 ├─ raised ≥ amount_to_raise, or collateral_left = 0 ─────────► Settled   (terminal, ENDS CHAIN)
                 └─ t_eff ≥ ttl → expire:
                       ├─ restart_index < max_restarts ──► Expired (terminal) + NEW successor Auction
                       │                                    in the same tx (nonce = record.count++),
                       │                                    remaining lot/target and reservation carried over
                       └─ restart_index = max_restarts ───► Expired (terminal, ENDS CHAIN, "final")
```

- **Chain-ending transitions** are Settled and final Expired. They book the shortfall, release the cap reservation, remove the auction from `live_auctions`, and apply the leftover rules (§5.8). A restart ends no chain: it carries the remainder over.
- Every handler asserts the expected status first. Terminal accounts are immutable except for `leftover_claimed` increasing through `claim_leftover`.
- "Liquidated" is not a stored state. `liquidate` atomically seizes the collateral and creates the auction directly in `Auctioning` (P1).
- An auction can never stay Expired-but-restartable. `expire` restarts it in the same transaction, so nobody has to remember to call a separate restart.

### 5.6 Engine instructions

| Instruction | Signer | Effect (all checks before effects; chained token calls last) |
|---|---|---|
| `init_config(salt, params)` | admin | Creates `EngineConfig`, `Accounting` and `SurplusBuffer`. Every parameter is bounds-checked. |
| `set_collateral_config(params)` | admin | Creates or updates `CollateralConfig`. Bounds-checked. It can set `admin_paused`, which also opens or closes the type-level pause clock (§5.7). Schedule changes apply only to auctions started afterwards, because each auction snapshots its schedule. It can **never** move auction collateral. |
| `liquidate(position, nonce)` | anyone (keeper) | **Full-position seizure only** (ADR-012). All computation happens first, from pre-state. 1. The price feed must be valid (§5.2) and the type must not be paused; this also runs the pause observation (§5.7). 2. Read the position's collateral `c` and nominal debt `D` (the stablecoin's accrued debt, rounded up). Compute `amount_to_raise = ceil(D × (1e4 + penalty_bps) / 1e4)`. Revert with `CapacityExhausted` unless `amount_to_raise` fits both `cap − reserved` budgets, and with `LiveAuctionsFull` if the list is full. 3. Create the `LiquidationRecord` on first use, assert `nonce == count`, and increment the count. 4. Compute the keeper reward (§5.9) in collateral from the **predicted post-seize vault balance `c`**. 5. Write the `Auction` (lot `c − reward`), reserve the capacity, append to `live_auctions`. 6. Chained calls run after the engine's output is final, in this order: initialise the `AuctionVault` holding → `stablecoin.liquidate_position(c, normalized_debt)`, which transfers all `c` from the host vault into `AuctionVault` → `Token::Transfer(reward)` from `AuctionVault` to the keeper. The host re-checks undercollateralisation and the expected amounts, so a stale quote reverts the whole transaction. |
| `bid(auction, coin_in_max, min_collateral_out)` | bidder | Requires a valid feed (pauses otherwise, §5.7) and status `Auctioning`. Computes `d(t_eff)`, `P`, the fill and the charged amount. Rejects if `collateral_out < min_collateral_out`, if the charge is below `min_bid` (unless it finishes the auction), or if the remainder `0 < amount_to_raise − raised < chost` (no dust). Payment order (debt first): `Token::Burn` the debt part from the bidder, then `Token::Transfer` the penalty part to `SurplusBuffer`, then `Token::Transfer` the collateral from the vault to the bidder. Settles in the same transaction when the target is reached or the lot is empty. |
| `expire(auction, next_nonce)` | anyone | Runs the pause observation (§5.7) first. Requires `t_eff ≥ ttl`, so a paused type cannot expire because its clock is frozen. Sets `Expired`. **If `restart_index < max_restarts`:** in the same transaction, creates the successor auction (`next_nonce == record.count`, then increments) with `collateral_seized = predecessor.collateral_left`, `debt_principal = predecessor.debt_principal − debt_raised`, `amount_to_raise = predecessor.amount_to_raise − raised`, the same `reserved_amount`, `keeper_reward = 0`, `restart_index + 1`, and a fresh schedule snapshot. It moves the remaining collateral from the old vault to the new vault with a chained transfer, links predecessor and successor, and replaces the entry in `live_auctions`. **Otherwise (final):** runs the chain-ending effects of §5.8. `next_nonce` is ignored when final. |
| `claim_leftover(auction)` | position owner | Pays out `leftover_claimable − leftover_claimed` collateral (pull, because the owner may be a private or group account). Only settled auctions that raised their target have any leftover. |
| `settle_debt(amount)` | anyone | Burns `min(amount, buffer balance, unbacked_debt)` from `SurplusBuffer` and reduces `unbacked_debt` (GEB `settleDebt`). |

### 5.7 Pause and frozen discount (ADR-007)
- **Pause conditions** apply per collateral type: an invalid feed (§5.2) or `CollateralConfig.admin_paused`. While paused, `liquidate`, `bid` and the restart branch of `expire` revert for that type only. Other types and all non-liquidation host operations continue (R4, R5).
- **The pause clock is type-level.** All auctions of a type share one oracle and one admin switch, so pausing is a type event. `CollateralConfig` holds `paused_since_ms: Option<u64>` and `paused_total_ms`. Each auction snapshots `type_paused_total_at_start`.
- **Frozen discount.** `t_eff = (now − start_ms) − (type.paused_total_ms − auction.type_paused_total_at_start) − (type.paused_since_ms.map(|s| now − max(s, start_ms)).unwrap_or(0))`, floored at 0. `ttl` is measured in the same effective time. Stalling the oracle or an admin pause therefore cannot push the discount up or expire an auction.
- **Admin pause.** `set_collateral_config(admin_paused = true)` sets `paused_since_ms = now` if it is not already set. Unpausing adds the interval to `paused_total_ms` and clears it. This is exact, because the admin transaction carries the clock.
- **Pause observation.** `liquidate`, `bid`, `expire` and a permissionless `observe_feed(collateral)` perform it. If the feed is invalid and `paused_since_ms` is unset, set it. If the feed is valid, there is no admin pause, and `paused_since_ms` is set, close the interval. **How precisely the onset of a stale period is timed is decided in spike S5.** The candidates:
  - (a) the onset is the observation time (simple; the discount can run until someone observes);
  - (b) the onset is derived as `last_seen_oracle_ts + max_age`, which is exact when the oracle could not have updated unseen;
  - (c) (b) bounded by the oracle's observation history.

  Acceptance criterion for S5: a property test shows an adversary who controls only the oracle's update timing **or** observation timing cannot obtain more discount than the schedule gives over the type's unpaused time, within one block. Admin pauses are covered by the same test. If only (a) is feasible, keepers and the app call `observe_feed` on every refresh, and the residual risk goes in `THREAT_MODEL.md`.

### 5.8 Accounting, settlement and leftovers (R6)
**Chain-ending effects** apply on Settled and on final Expired, and happen exactly once per chain:
1. `shortfall = max(0, debt_principal − debt_raised)` (penalty excluded). `Accounting.unbacked_debt += shortfall`, and `total_shortfall_booked += shortfall`. A lot that runs out before the debt is covered books a shortfall too.
2. Release `reserved_amount` from `type_reserved` and `global_reserved`, and remove the auction from `live_auctions`.
3. Collateral:
   - **Settled because the target was raised:** `leftover_claimable = collateral_left`, owed to the position owner (GEB behaviour).
   - **Settled because the lot ran out:** nothing remains.
   - **Final Expired:** the remaining `collateral_left` moves by chained transfer to the engine's `ProtocolCollateral` holding. **It does not go to the defaulted owner** (no moral hazard, and value is preserved). Sub-project B sells it through debt auctions or an admin-gated sale, and its proceeds then reduce `unbacked_debt`.
- The surplus buffer receives only penalty coins. `settle_debt` nets the buffer against `unbacked_debt`.
- **Supply invariant**, extending the host's invariant 7: `stablecoin_supply ≤ Σ position nominal debt + Σ live (debt_principal − debt_raised) + unbacked_debt`.
- Stability fees are **not** routed into the buffer in the PoC. Doing so would require a host-wide debt counter on every position operation, breaking the host's design (research-6 Q2).

### 5.9 Keeper reward and caps
- `reward_coin = min(tip + chip_bps·D/1e4, reward_cap)`, paid only if `D ≥ debt_floor`, where `D` is the position's full nominal debt.
- It is converted to collateral at `P` and rounded down, then paid from the seized lot when the auction is created.
- Code enforces `reward_coin ≤ penalty_bps·D/1e4`. The reward is carved out of the penalty, so self-liquidation never pays **when the lot covers the penalty**. If there is a shortfall, the reward is paid from collateral that would otherwise reduce bad debt. That residual is documented in `THREAT_MODEL.md`, and the reward cap bounds it.
- Caps: `liquidate` reserves `amount_to_raise` against the per-type and global capacity (Maker `hole`/`Hole`) and reverts with `CapacityExhausted` if the whole position does not fit. There is no partial seizure (ADR-012). Partial liquidation is listed as a possible M2 extension; it needs extra hook arguments.
- **`debt_floor` and `chost`.** `debt_floor` is an engine `CollateralConfig` parameter, because the host has no dust or floor parameter. `chost = ceil(debt_floor × (1e4 + penalty_bps) / 1e4)`, which is Maker's `dust·chop`. It is used by the bid dust rule (§5.6) and the final-bid rules.

### 5.10 Stablecoin hooks (vendored at `64c0120`, offered upstream)
- **Appended** at the end of `stablecoin_core::Instruction`; the wire format is the variant index. Engine state lives in a **new** PDA, `LiquidationEngineState { engine_authority_id }`. `ProtocolParameters` is untouched, so existing decoders, the module FFI and fixtures keep working (research-2).
- `set_liquidation_engine(engine_authority_id)`: signed by the admin. It rejects the all-zero id.
- `liquidate_position(expected_collateral_amount, expected_normalized_debt_amount)`, account list per research-2 §"Proposed hooks":
  - The engine authority must sign and must equal the stored id. The authorisation mechanism, `caller_program_id` vs an engine PDA signer, is decided in spike S2; research-2 suggests both are unforgeable.
  - The position PDA is re-derived. Debt must be greater than zero, and the expected amounts must equal the stored ones.
  - The vault's PDA and definition must match, with balance equal to the position's collateral.
  - The engine's custody holding must already be initialised with the same collateral definition.
  - The oracle must be fresh. The position must be **not** collateralised, using a boolean variant of `assert_position_is_collateralized` (U512, exact).
  - Effects: zero the position's collateral and debt, then one chained `Token::Transfer` from the vault to the custody holding using the vault seed.
- **Chained-call budget for `liquidate`:** about 4–5 of the 10 allowed (initialise the vault holding → host `liquidate_position`, which nests the vault-to-vault transfer → keeper reward transfer). When one transaction touches the same account in two chained calls, the later call carries the updated state (research-2). Spike S1 measures the real count.
- Liquidation while the host is frozen: **allowed** (it reduces risk). This needs maintainer confirmation in the upstream PR discussion.
- The pause/freeze hook for F4 is sub-project B. The PoC does not touch the host's single freeze-authority slot.

### 5.11 Default parameters (bounded in code)

| Parameter | Default | Bound |
|---|---|---|
| penalty | 1000 bps | 0–2500 |
| schedule | 2% → 10% over 45 min | ≤ 8 points, max ≤ 3000 bps |
| ttl | 2 h effective | 10 min – 7 d |
| max restarts | 3 | ≤ 10 |
| min bid | 25 LSC (demo) | > 0 |
| debt_floor | 100 LSC (demo) | > 0 |
| chost | derived: `ceil(debt_floor × (1e4 + penalty_bps)/1e4)` = 110 LSC | ≥ min bid |
| keeper tip / chip / cap | 5 LSC / 10 bps / 50 LSC (demo) | reward ≤ penalty |
| max oracle age | 60 min | ≤ 24 h |
| price band | 9000–11000 bps of R | lo ≤ 10000 ≤ hi, within [5000, 20000] |
| per-type cap / global cap (coin on auction, Maker `hole`/`Hole`) | 60,000 LSC / 250,000 LSC (demo) | per-type ≤ global |
| `live_auctions` capacity | 512 | fits within 100 KiB |

### 5.12 Invariants (tested in `auction_core` with proptest; asserted on-chain where cheap)
1. `collateral_seized == keeper_reward + collateral_sold + collateral_left`. `collateral_sold` is tracked independently of `collateral_left`, and `collateral_left` is never reduced by claims. After a restart, the successor's `collateral_seized` equals its predecessor's `collateral_left`.
2. The vault balance equals `collateral_left − leftover_claimed` while the vault holds the lot. It is 0 after a restart transfer, a final-expiry transfer to `ProtocolCollateral`, or a full claim.
3. Each fill satisfies `charged ≤ coin_in_max` and `collateral_out ≥ min_collateral_out`. `debt_raised + penalty_raised ≤ amount_to_raise`.
4. `d(t)` is monotone non-decreasing in `t_eff`, bounded by the schedule's maximum. `P` stays within the band.
5. Status is monotone, and terminal states are absorbing.
6. After every bid, the remaining amount to raise is either 0 or ≥ `chost`.
7. `Σ live chains reserved_amount == type_reserved ≤ type_cap`, and `Σ types == global_reserved ≤ global_cap`. A reservation is made at `liquidate`, carried across a restart, and released exactly once at chain end.
8. At each chain end, `shortfall == max(0, debt_principal − debt_raised)` computed on the **final** auction of the chain, which carries the chain's remaining principal. The sum of `debt_raised` over the chain plus the shortfall equals the original principal, and `total_shortfall_booked == Σ shortfall`. No shortfall is booked on a restart.
9. Buffer balance = `total_penalty_collected − Σ settle_debt burns`. After `settle_debt(u128::MAX)`, `min(buffer, unbacked_debt) == 0`.
10. Host and engine conservation: the change in host debt equals `Σ debt_principal seized`, and the change in host collateral equals `Σ collateral_seized`.
11. Supply invariant (§5.8).
12. Liveness (fuzz): an unsafe position with a valid feed, remaining capacity for its whole `amount_to_raise`, and space in `live_auctions` can always be liquidated.
13. Split-bid neutrality: n bids summing to X at the same `t_eff` never yield more collateral than one bid of X.
14. The frozen clock: `t_eff` never advances while the type is paused, whether by admin or by the feed (S5 property).
15. Collateral destination: auction collateral only ever leaves to a bidder, the keeper (at `liquidate`), a successor vault, the owner (leftover only after the target is raised), or `ProtocolCollateral` (final expiry).

## 6. Off-chain layers

### 6.1 `auction_sdk` (Rust library; later the bot SDK)
- PDA derivation for every account in §5.4, and decoders for all engine and host account layouts.
- Quotes that call the **same** `auction_core` functions the program uses: `quote_liquidation(position, host_state, oracle, clock)` and `quote_bid(auction, coin_in, oracle, clock)`. The UI preview therefore equals the on-chain result for the same inputs.
- Plan builders: `{program_id, account_ids, signing_requirements, instruction_words}` for each instruction. Only Rust knows the instruction encoding.
- Discovery (§6.4) and transaction confirmation (§6.5).

### 6.2 `auction_ffi` (Rust cdylib)
- One `char* op(const char* json)` per SDK function, plus `auction_ffi_free`.
- Envelope `{"ok":true,"value":…}` or `{"ok":false,"error":"CODE","detail":…}`, wrapped in `catch_unwind`.
- Big integers travel as decimal strings.
- The cbindgen header is committed, and CI fails on drift. This follows `modules/stablecoin` in lez-programs.

### 6.3 `auction_engine_module` (C++ universal core module)
- `AuctionEngineImpl : LogosModuleContext`, `metadata.json` with `type: core`, `interface: universal`, `dependencies: ["lez_core"]`, `external_libraries: [{name: "auction_ffi"}]`.
- Methods: `getConfig`, `listAuctions`, `getAuction`, `listAtRiskPositions`, `rescan`, `quoteLiquidation`, `quoteBid`, `liquidate`, `bid`, `expire`, `observeFeed`, `claimLeftover`, `settleDebt`, `awaitTx`.
- **Each declaration stays on one line**; the generator silently drops multi-line declarations. A test calls every method.
- Every method returns `{status, error, …}` with stable error codes.
- Submission uses `lez_core.send_generic_public_transaction`, the 4-argument 0.4.1 form, with instruction bytes as risc0 u32 words in little-endian.

### 6.4 Discovery of positions and auctions (ADR-008, PoC-grade)
- **Positions.** `auction_sdk` scans blocks with the sequencer's `getBlockRange` over HTTP. It keeps transactions that list the host's `ProtocolParameters` PDA, which every position operation includes, and merges them with a local list of known positions. A persisted cursor stores the last scanned block. The UI shows "Scanned to block N · Rescan".
- **Auctions.** Read `EngineConfig.live_auctions`. History comes from `LiquidationRecord`s.
- **Production path,** in the port plan: the indexer's `getTransactionsByAccount(protocol_parameters_pda)`, then LP-0012 events on v0.3.

### 6.5 Transaction confirmation (ADR-009)
`awaitTx` hides the following mechanism:
1. Before submitting, record the signer nonce `n0` and the block height `h0`.
2. Submit, then poll `get_account_public(signer).nonce` about every 2 s, together with a check that the expected target account changed.
3. `nonce > n0` means **Included**. Still at `n0` by `h0 + 5` means **Dropped**: nothing was applied, so re-quote and retry.
4. Submit one in-flight transaction per signer.

On lez_core ≥ 0.4.3 the mechanism switches to `poll_transaction_status`.

### 6.6 `auction_cli`
- Commands: `quote-liquidation`, `quote-bid`, `list-auctions`, `list-at-risk`, `rescan`, `watch`. Transactions go through the SPEL-generated CLI (U3) or an `auction_cli submit` wrapper using the same plan builders.
- Usage is documented in `cli/README.md` (PEND2).

## 7. Basecamp app (`apps/auctions`)
- **Design source of truth:** `docs/design/ui/ui-review.md` (tokens with file:line sources), `ui-v3.png` (main screen) and `ui-v4.png` (at-risk positions, confirmation, states). It follows Basecamp's own colour grid and layout (AppManagerView grid: 40 px margins, 200 px list column, radius-16 panels, `#ED7B58` accent, dark Logos.Theme).
- **Views:**
  1. Live auctions: master–detail. KPI strip, discount chart (QtQuick.Shapes), spec facts, and a bid card (oracle freshness, amount with min and max-to-finish, live preview with the guaranteed minimum, Place bid).
  2. At-risk positions: positions sorted by CR, a CR gauge, the liquidation quote with keeper reward and cap bars, and Liquidate & start auction.
  3. Bid confirmation dialog: `LogosDialog`, the visibility summary, then the progress strip (Submitted → Included → Done, or Dropped → Re-quote).
- **States:** healthy (disabled with a reason), feed stale, feed out of range, paused auction with a frozen discount, over the max, dropped transaction.
- **Decisions carried from the UI review:**
  - the main example uses a CR of about 140% (normal path), and "will end short" is a separate state example;
  - the network-fee row is hidden when the fee is 0 (v0.2.4);
  - the private toggle is disabled in the PoC;
  - the primary button has a black label (contrast), via a small `LogosButton` override, offered upstream as a design-system fix.
- **Architecture:** a `ui_qml` app with a C++ backend over Qt Remote Objects (`.rep`), forwarding to `auction_engine_module`. QML uses `logos.module` / `logos.watch`. Wallet controls come from the vendored `Logos.Wallet`, patched for any 0.4.1 API differences.
- **Sandbox:** no network from QML. All chain I/O goes through the module.

## 8. Privacy (PoC scope)
- The PoC submits public transactions only. The UI states this and shows what is visible.
- The design keeps the private path possible without rework, because every user action settles instantly. No later claim or refund ever needs the same ephemeral account, except the owner's `claim_leftover`, which belongs to the position owner and not to a bidder or keeper.
- Sub-project B delivers: an atomic deshield of token plus gas into a fresh account A, the operation, then a reshield that cannot be skipped (PRIV1–3, PA1–4). The UI flow is already designed (`ui-v4.png`, private variant).

## 9. Testing

| Layer | Scope | Tooling |
|---|---|---|
| `auction_core` | math, schedule, state machine, accounting; invariants 1–14 | TDD unit tests; `proptest`; a differential model in Python big-int; Kani for `mul_div` and `d(t)` |
| Stablecoin hooks | non-engine caller rejected; zero binding rejected; healthy position rejected; mismatched amounts rejected; stale oracle rejected; happy path. **Upstream suite runs unchanged.** | SPEL unit tests + the existing lez-programs tests |
| `auction_engine` | full path on the real stablecoin; partial bids; lot binds; dust rule; min bid; `expire` with restart and with final expiry (collateral to `ProtocolCollateral`); lot exhausted with a shortfall booked; re-liquidation of a position; `CapacityExhausted` and `LiveAuctionsFull`; admin pause freezes the discount; every illegal transition refused (F6); 2+ concurrent auctions (R3); stale and out-of-range pause per type (R2/R4); frozen discount; caps; `settle_debt`; R6 conservation | `integration_tests` on a standalone v0.2.4 sequencer, dev mode |
| Real proofs | the full happy path end to end | `RISC0_DEV_MODE=0`, nightly and on tags (S2) |
| SDK / FFI | quote equals the on-chain result; JSON fixtures per op; panic safety; header drift | Rust tests, CI diff |
| C++ module | every method; error mapping; `awaitTx` states (Included, Dropped) | `LOGOS_CMOCK` FFI mocks + `mockModule("lez_core")` |
| QML | components with a fake backend | `tst_*.qml` via qmltestrunner |
| Basecamp end to end | install our `.lgx` in a clean profile; liquidate; bid; verify | `run-basecamp-e2e.sh`: offscreen `bin-bundle-dir-inspector` + logos-qt-mcp `.mjs` |
| Requirements | each hard requirement maps to ≥ 1 named test (S4) | `REQUIREMENTS.md` checker in CI |

Development follows TDD: a failing test first, then the implementation. Coverage target is ≥ 80% on Rust crates.

## 10. Security
- `THREAT_MODEL.md` covers the 11 ranked attacks from research-3 §7, plus the draft template's C-class (griefing price discovery) and F-class (sniping) scenarios. Each threat links to its mitigating tests.
- **Per-instruction checklist** (in `docs/architecture/security-checklist.md`, enforced in review):
  - time only from `CLOCK_01`;
  - oracle id pinned;
  - every PDA re-derived;
  - owner and definition checks;
  - zero caller id rejected;
  - checks before effects, token calls last;
  - caps respected;
  - no dust;
  - rounding direction as in §5.1;
  - frozen-discount property holds;
  - no admin path to auction collateral.
- **Process:**
  - a security-reviewer subagent on every on-chain change;
  - a full red-team pass (`security-redteam` skill) before submission;
  - `cargo-fuzz` on decoders and quote paths;
  - `cargo-audit` / `cargo-deny`, `clippy -D warnings`.

## 11. Benchmarks and interoperability
- `CYCLES.md` records zkVM executor cycles for each instruction against the 32M budget, plus transaction size for each instruction (P1–P3). CI fails on a regression greater than 10%. The harness is committed and reproducible (PEND10).
- Load test: N concurrent auctions with interleaved bids. Report the throughput and the conflict rate.
- Interop: the vendored stablecoin passes its own suite; the real `lez_core` 0.4.1 is used in end-to-end tests; IDL drift is checked against the SPEL-generated IDL.

## 12. Documentation (kept current at every task)
- `docs/design-log.md`: one ADR per decision, covering context, options, choice and trade-offs (PEND7). It is seeded with ADR-001…009 from this spec, and every spike outcome is added.
- `docs/architecture/`: `overview.md`, `onchain.md`, `math.md`, `sdk-ffi.md`, `module.md`, `app.md`, `security-checklist.md`. Each explains how its layer works, with diagrams.
- `README.md`: end-to-end usage covering deployment, addresses, CLI and app (S5).
- `REQUIREMENTS.md`, `SUBMISSION.md`, `DEMO.md`, `CYCLES.md`, `THREAT_MODEL.md`, `docs/v03-port-plan.md`, and `docs/PRIVACY.md` (a stub in the PoC).

## 13. Spikes in week 1 (M0): try alternatives, measure, decide

| Spike | Question | Options compared | Decided by |
|---|---|---|---|
| S1 | Does the pinned toolchain build and run end to end locally? | — | The guest builds; standalone sequencer deploy; stablecoin `initialize_program`; the Basecamp offscreen harness boots with lez_core 0.4.1 |
| S2 | How should the host authenticate the engine? | `caller_program_id` check vs engine PDA signer vs both | Unforgeability tests + cycles + upstream acceptability |
| S3 | Auction addressing | `LiquidationRecord` nonce (chosen) vs alternatives | Confirm SPEL can create the record on first use (`Claim::Pda`) and the arg-seeded auction PDA |
| S4 | Discount curve | piecewise-linear vs exponential (256-bit) vs stairstep | Cycles + accuracy vs RAI; keep piecewise unless exponential is within 10% of its cycles |
| S5 | Frozen-discount bookkeeping | (a) poke, (b) oracle-timestamp derivation, (c) both | The §5.7 property test |
| S6 | Bid write set | update `Accounting` per bid vs only at terminal transitions | Concurrency test + R6 provability |
| S7 | One-step bid vs quote-then-bid | — | UX + cycles |

Each spike produces a short write-up under `docs/spikes/` and an ADR.

## 14. PoC schedule (≈ 3 weeks, solo, near full-time)
- **Week 1:**
  - Spikes S1–S7.
  - `auction_core` complete with property tests and Kani for `mul_div`.
  - Stablecoin hooks with tests; the upstream suite green.
  - Design log seeded.
- **Week 2:**
  - `auction_engine` program and integration tests (dev mode).
  - SDK, FFI and CLI.
  - CI: unit, end to end, the real-proof job, drift checks, `CYCLES.md`.
- **Week 3:**
  - C++ module and QML app; the Basecamp end-to-end test.
  - Benchmarks; security review and red-team pass.
  - `SUBMISSION.md` and the demo video.
  - Draft proposal; open the upstream PR to lez-programs.

Execution: `superpowers:writing-plans` produces the task plan. Then `superpowers:subagent-driven-development` runs each task: an implementer subagent working with TDD, then a spec-compliance reviewer, then a code-quality reviewer (plus a security reviewer for on-chain code), then verification before the task is marked complete.

## 15. Proposal mapping

**Milestones (L tier).** Drafted from research-5 §6c. Final numbers are set when the RFP reopens.

| Milestone | Duration | Payout |
|---|---|---|
| M0 Spec, spikes, upstream host-hook PR | 1 wk | $4,000 |
| M1 Collateral auction house with real token movement on the stablecoin | 2 wk | $9,000 |
| M2 Prices, isolation, pause/frozen discount, SOFT1 | 1.5 wk | $6,000 |
| M3 Buffer, debt and surplus auctions (mock token), freeze | 1.5 wk | $6,500 |
| M4 SDK/FFI, C++ module, private path, CLI, bot SDK | 2 wk | $8,000 |
| M5 Basecamp `ui_qml` app, Figma/equivalent, catalog (separable) | 1.5 wk | $6,000 |
| M6 v0.3 port and testnet, real-proof CI, full requirement matrix, `CYCLES.md`, `PRIVACY.md`, Kani | 1.5 wk | $5,500 |
| M7 Audit remediation | 1 wk | $3,000 |
| **Build total** | **12 wk** | **$48,000** |

- The audit is listed separately, about $10–12k with a named tier-1 firm, and Logos may contract the firm directly.
- Variant without M5, about $42k, if the GUI moves to the separate RFP-014 UI RFP (ecosystem #210). We would then bid that RFP together with the Stablecoin UI RFP.

**Traceability** (PoC = shown in the PoC; M# = promised; D = descoped with a reason):

| ID | Status | Evidence / where |
|---|---|---|
| F1 permissionless liquidation + incentive | PoC | `liquidate`, keeper reward, tests |
| F2 increasing discount, configurable, early end | PoC | §5.3, `bid` settles on target |
| F3 partial lots | PoC | `bid` partial fills, lot-binds charge |
| F4 shortfall → debt auction / buffer / freeze | PoC partial (buffer + unbacked accounting) · M3 | §5.8 |
| F5 surplus auction | M3 | — |
| F6 strict transitions | PoC | §5.5 refusal tests |
| F7 host interface, per-product instance | PoC | §5.10 hooks, `EngineConfig` per instance |
| U1 SPEL + IDL | PoC | IDL drift check |
| U2 Basecamp mini-app | PoC | `apps/auctions`, Basecamp end to end |
| U3 CLI | PoC | `auction_cli`, SPEL CLI |
| U4 at-risk positions + incentives + trigger | PoC | At-risk view, block scanner |
| U5 auction view + bid preview | PoC | Live view, `quote_bid` |
| U6 SDK incl. private pattern | PoC (public SDK) · M4 (private) | `auction_sdk` |
| R1 non-reentrant, ordered | PoC | checks-effects-calls, no callbacks |
| R2 TWAP/confidence, stale ⇒ pause | PoC (Oracle1e27 + band; TWAP adapter unit-tested) · M2 live TWAP | §5.2, §5.7 |
| R3 concurrent, isolated | PoC | per-auction PDAs, concurrency test |
| R4/R5 per-type pause only | PoC | pause tests |
| R6 invariants | PoC | §5.12, proptest + on-chain checks |
| P1/P3 one block | PoC | single-transaction instructions, `CYCLES.md` |
| P2 many concurrent auctions | PoC (load test) | `CYCLES.md` |
| S1 devnet/testnet deployment | M6 (blocked by v0.3, §3.1) | localnet + reproducible scripts in the PoC |
| S2 sequencer end to end with `RISC0_DEV_MODE=0` in CI | PoC | CI job |
| S3 CI green | PoC | — |
| S4 ≥ 1 test per hard requirement | PoC (checker) | `REQUIREMENTS.md` |
| S5 README | PoC | `README.md` |
| PRIV1–3, PA1–4 | M4 | UI designed (`ui-v4.png`) |
| SOFT1 multi-oracle | M2 | — |
| SOFT2 formal verification | PoC partial (Kani: math) · M6 | — |
| SOFT3 liquidator bot SDK | M4 | `auction_sdk` is its base |
| PEND1–12 (pending template changes) | addressed per §12 / M5 / M6 | research-5 §1 |

## 16. Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| RFP-014 reopens with changed scope (UI split, new supportability items) | high | Separable GUI milestone; PEND items already planned; respond fast to spec PRs |
| Upstream stablecoin churn (PRs #333–#401 open) | high | Vendor at `64c0120`; hooks kept as a clean patch series; rebase weekly |
| v0.3 port effort | certain, later | `auction_core` isolation; port plan; plan-builder seam |
| Oracle scale mismatch and weak freshness upstream | high | `Oracle1e27` + price band + `[R/2, 2R]` sanity check; TWAP adapter behind a flag |
| Frozen-discount bookkeeping is not exact | medium | Spike S5 property test; documented residual risk |
| Single developer throughput | medium | 3-week PoC as proof; strict scope; subagent-driven execution |
| WSL memory and Nix build times | medium | `.wslconfig` + swap; binary caches; build guests in Docker |
| `lez_core` version skew with the catalog wallet (0.4.3) | medium | Clean Basecamp profile in the demo; resolved by the v0.3 port |
| Redemption-price trigger surprises reviewers | low | ADR explaining RAI semantics |

## 17. Decisions log (seed for `docs/design-log.md`)
- ADR-001: toolchain Track B (v0.2.4 + SPEL) with a pure core crate. The v0.3 port is a milestone.
- ADR-002: standalone repo; a reusable engine plus appended host hooks offered upstream (approach 1).
- ADR-003: GEB increasing-discount auction with Maker safety rails (tip+chip reward carved from the penalty, caps, dust rule, slippage bounds, restart).
- ADR-004: auction addressing via the `LiquidationRecord` nonce.
- ADR-005: bid price = band-clamped market price around the redemption price, collateral per coin; no fallback price. This overrides research-6 Q1's fallback to R, because RFP R2 requires a pause.
- ADR-006: piecewise-linear schedule of ≤ 8 points, snapshotted per auction.
- ADR-007: stale or out-of-range feed, or an admin pause, pauses the type. A type-level pause clock freezes the discount and ttl of all its auctions.
- ADR-012: full-position seizure only, with whole-position cap reservation. `expire` restarts in the same transaction. Final-expiry collateral goes to `ProtocolCollateral`, not to the owner. Shortfall is booked at every chain end. (These follow from the spec review on 2026-10-01.)
- ADR-008: PoC discovery by block scan plus `live_auctions`; indexer and events later.
- ADR-009: transaction confirmation by signer-nonce polling; dropped after 5 blocks.
- ADR-010: debt-first payment order; penalty into the surplus buffer; accounting only in the engine; no stability-fee routing in the PoC.
- ADR-011: UI follows Basecamp's colour grid and AppManagerView layout; master–detail; black label on the orange primary button.

## 18. References
- RFP-014 and its appendix; ecosystem issues #185, #200, #205, #210, #224; template PRs #208, #209, #45.
- Competitors: #187 (Antumbra; edenbd1/antumbra-auction, antumbra-lez), #159 (Web3spell).
- Precedents: #141 Argo, #117 Kanon, #112 Gateway, #118/#119 bleu.
- Code: logos-execution-zone v0.2.4, lez-programs `64c0120`, spel `1ef0500f`, logos-execution-zone-module `acf0cd50`, logos-module-builder, logos-design-system, reflexer-labs/geb, makerdao/dss.
- Research reports: `docs/research/`. UI: `docs/design/ui/`.
