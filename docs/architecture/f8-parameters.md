# F8 parameter audit

RFP-014 F8 (pending text): authorities and parameters are set at instance initialisation, with no
hardcoded production values. This page lists every constant in the engine's economic code that
bounds or defaults an economic or authority value, says what kind of constant it is, and lists
every init-time parameter with the authority that sets it.

Scope: `crates/auction_core/src/**`, `crates/auction_engine_params/src/**` (which compiles
`programs/auction_engine/core/src/args.rs` and `error.rs` in through `#[path]`, so `args.rs` is
in scope) and `v04/engine_core/src/**`. Found with
`grep -rnE "const [A-Z_0-9]+: " crates/auction_core/src crates/auction_engine_params/src programs/auction_engine/core/src/args.rs v04/engine_core/src`,
then read by hand. Three inline literals that act as hardcoded economic bounds are listed too,
marked *(inline)*, because a grep for `const` does not find them.

Kinds:

- **init parameter bound**: caps or ranges a value the admin chooses at init (or later). The
  deployment picks the value; the constant only refuses an unsafe choice.
- **math constant**: a fixed-point unit. Not a policy choice.
- **protocol limit**: a fixed size of the engine's data layout.
- **production default — Tier 2**: a value the admin cannot set that a production deployment
  would want to choose. These are F8 gaps; each one is carried into the Tier 2 engine design
  (spec §5, E9).

## Constants

| Constant | Value | File:line | Kind | Why |
|---|---|---|---|---|
| `RAY` | 10^27 | `crates/auction_core/src/units.rs:3` | math constant | Fixed-point scale for prices and discounts. |
| `BPS` | 10_000 | `crates/auction_core/src/units.rs:4` | math constant | Basis-point denominator; also the ceiling of every bps argument (`args.rs` `bps()`). |
| `BPS_TO_RAY` | 10^23 | `crates/auction_core/src/units.rs:6` | math constant | 1 bps in RAY (`RAY / BPS`). |
| `MAX_PENALTY_BPS` | 2_500 | `crates/auction_core/src/liquidation.rs:9` | init parameter bound | Caps the admin-set `penalty_bps` per collateral type (`CollateralParams::validate`). |
| `MAX_ORACLE_AGE_MS` | 86_400_000 (24 h) | `crates/auction_core/src/price.rs:30` | init parameter bound | Caps the admin-set `max_oracle_age_ms` (collateral feed) and `ref_oracle_max_age_ms` (reference oracle, `args.rs:80`). |
| `MAX_CONF_BPS` (collateral feed) | 1_000 | `crates/auction_core/src/price.rs:31` | init parameter bound | Caps the admin-set `max_conf_bps` of a collateral feed (`FeedParams::validate`). |
| `MAX_CONF_BPS` (reference oracle) | 1_000 | `crates/auction_core/src/price.rs:31`, used at `programs/auction_engine/src/system.rs:47` and `v04/engine_core/src/system.rs:44` | production default — Tier 2 | The reference oracle's confidence limit is this constant itself; there is no admin parameter for it, unlike the collateral feed's `max_conf_bps`. |
| `REF_ORACLE_CLAMP_BPS` | 2_000 (±20 %) | `crates/auction_core/src/price.rs:32` | production default — Tier 2 | Clamp of a valid reference-oracle price around the admin price (`effective_ref_price`, ADR-017). Fixed; the admin cannot set it. |
| `MAX_POINTS` | 8 | `crates/auction_core/src/schedule.rs:8` | protocol limit | Fixed size of the snapshotted schedule array; the admin chooses up to 8 points. |
| `MAX_DISCOUNT_BPS` | 3_000 | `crates/auction_core/src/schedule.rs:9` | init parameter bound | Caps every point of the admin-set collateral discount schedule. |
| `MAX_MINT_HARD_CAP` | 10^27 base units | `crates/auction_core/src/system.rs:9` | init parameter bound | Caps the admin-set `max_mint_per_auction` (`SystemParams::validate`, re-checked at mint). The per-auction mint cap itself is an admin parameter. |
| `MAX_SYSTEM_DISCOUNT_BPS` | 3_000 | `crates/auction_core/src/system.rs:52` | init parameter bound | Caps the admin-set `surplus_max_discount_bps` and `debt_initial_discount_bps`. |
| `MIN_SYSTEM_TTL_MS` | 600_000 (10 min) | `programs/auction_engine/core/src/args.rs:15` | init parameter bound | Lower end of the admin-set `system_ttl_ms` (spec §5.12). |
| `MAX_SYSTEM_TTL_MS` | 604_800_000 (7 d) | `programs/auction_engine/core/src/args.rs:16` | init parameter bound | Upper end of the admin-set `system_ttl_ms` (spec §5.12). |
| `DEBT_RELIST_DELAY_MS` | 600_000 (10 min) | `programs/auction_engine/core/src/args.rs:20` | production default — Tier 2 | Minimum gap between one debt auction's end and the next one's start (ADR-039). Fixed; the admin cannot set it. |
| `MIN_SLACK_MS` (v04 only) | 1_000 | `v04/engine_core/src/check.rs:13` | init parameter bound | Lower end of the admin-set clock slack `slack_ms` (`Init`, `SetSlack`; ADR-064). |
| `MAX_SLACK_MS` (v04 only) | 300_000 | `v04/engine_core/src/check.rs:16` | init parameter bound | Upper end of the admin-set clock slack `slack_ms`. |
| feed band ranges *(inline)* | `band_lo_bps` in 5_000..=10_000, `band_hi_bps` in 10_000..=20_000 | `crates/auction_core/src/price.rs:36-37` | init parameter bound | Range of the admin-set bid-price band around the redemption price (`FeedParams::validate`). |
| feed sanity window *(inline)* | market price within [R / 2, 2 · R] | `crates/auction_core/src/price.rs:79-80` | production default — Tier 2 | `validate_feed` pauses a collateral type whose market price is outside half to twice the redemption price R, and drops a reference-oracle reading outside half to twice the admin price. The factor 2 is fixed; the admin cannot set it. |
| reference-oracle scale *(inline)* | `OracleScale::Ray` | `programs/auction_engine/src/system.rs:44-50`, `v04/engine_core/src/system.rs:41-47` | production default — Tier 2 | The reference oracle's `FeedParams.scale` is fixed to Ray; there is no admin parameter, unlike the collateral feed's `oracle_scale`. |
| reference price step *(inline)* | new price within [old / 2, 2 · old] | `programs/auction_engine/core/src/args.rs:226` | init parameter bound | Bounds each admin change of `protocol_token_ref_price` (`check_ref_price_step`, ADR-031); not applied at init, where there is no old value. |

Counts per Kind: math constant 3 · init parameter bound 12 · protocol limit 1 · production
default — Tier 2 5 (21 rows; `MAX_CONF_BPS` has one row per role).

### Not listed

- Test-only constants (`#[cfg(test)]` modules): `liquidation.rs:110` `P`, `price.rs:114` `R`,
  `system.rs:394` and `:554` `REF` in `auction_core`; `args.rs:242` `CAP`; `v04/engine_core`
  `assertions.rs:189` `ENGINE`, `bid.rs:132` `START`, `solvency.rs:84-87` and
  `system.rs:320-321` `RAY` / `NOW` / `THRESHOLD`.
- Non-economic constants: `math.rs:10` `LO64` (64-bit mask), `v04/engine_core/src/msg.rs:16`
  `SEIZED_INDEX` (wire variant index), the account tags `TAG_*` in `v04/engine_core/src/state.rs`
  and `system_state.rs`, and the PDA seed bytes in `crates/auction_engine_params/src/seeds.rs`.
- `schedule::rai_default()` (2 % to 10 % over 45 min) is a function, not a production default:
  it is used only by tests, the `vectors` example and the Kani proofs. A deployment always passes
  its own schedule.
- `EngineParamsArgs::defaults_for_tests()` and `CollateralArgs::defaults_for_tests()` in
  `args.rs` are test and localnet values, not defaults applied on chain: every on-chain config
  takes all its parameters from its init message.
- The host stablecoin program (`programs/stablecoin/**`, upstream-derived) has its own constants,
  e.g. `MAX_PENALTY_BPS` at `programs/stablecoin/src/liquidation_hooks.rs:25` (sizes the seizure
  and must equal the engine's) and the bounds in `programs/stablecoin/src/checks.rs`. It is out of
  this audit's scope, which is the auction engine.

## Init-time parameters and their authority

**Who is the admin.** v0.2.4 `init_config` (`programs/auction_engine/src/admin.rs:30`) is
permissionless: the signer becomes the config's admin, and the config PDA includes the admin and
the salt, so no one can create a config at another admin's address (ADR-027). v04 `Init`
(`v04/programs/methods/guest/src/bin/engine/admin.rs:77`, `on_init`) does the same through the
admin's proxy `(admin, E)`: the config accepts `Init` only from that proxy and re-derives its own
address from `(admin, salt)`. After init, every parameter change requires that admin
(`require_admin` in v0.2.4; `expect_admin_proxy` plus `config.admin == admin` in v04).

### Fixed at init (no setter)

| Parameter | v0.2.4 (`init_config`) | v04 (`Init`) |
|---|---|---|
| admin | the init signer | the init signer, through its proxy `(admin, E)` |
| salt | admin's argument | admin's argument |
| host | admin chooses the host by passing its `ProtocolParameters` account (ADR-030) | admin's `Init.host`, checked against the host's own params (ADR-065) |
| stablecoin definition, collateral definition, token program | read from the chosen host's `ProtocolParameters` | admin's `Init.stable_def` / `Init.token`; the collateral type comes with the first `SetCollateral` |
| collateral oracle | must equal the host's `market_price_oracle_id` (ADR-028), passed at `set_collateral_config` | admin's `Init.oracle` |
| clock | the system clock account | admin's `Init.clock` |
| protocol-token genesis | — | admin's `Init.protocol_genesis` |

The other side of the binding is the host's authority: the host stablecoin program accepts a
liquidation engine only through its own admin-only `set_liquidation_engine`
(`programs/stablecoin/src/liquidation_hooks.rs:90`), so an engine config is trusted by a host only
after the host's admin binds it (ADR-027).

### `EngineParamsArgs` (`args.rs:45`): set by the admin at init, changeable by the admin through `set_engine_params` / `SetEngineParams`

| Parameter | Bound in code |
|---|---|
| `global_cap` | > 0; not below the collateral type cap |
| `freeze_on_bad_debt` | — |
| `bad_debt_freeze_threshold` | ≥ `debt_lot` |
| `protocol_token_enabled` | locked while a system auction is live (see the lock note below) |
| `protocol_token_ref_price` | > 0; each change within [old / 2, 2 · old] |
| `ref_oracle_enabled` (and the reference-oracle actor / account) | locked while a system auction is live |
| `ref_oracle_max_age_ms` | ≤ `MAX_ORACLE_AGE_MS`; > 0 when the reference oracle is enabled |
| `surplus_buffer_target` | — |
| `surplus_lot` | > 0 |
| `surplus_delay_ms` | — |
| `surplus_premium_bps` | ≤ 10_000 |
| `surplus_max_discount_bps` | ≤ `MAX_SYSTEM_DISCOUNT_BPS` |
| `debt_lot` | > 0 |
| `debt_initial_discount_bps` | ≤ `MAX_SYSTEM_DISCOUNT_BPS` |
| `debt_growth` (schedule) | ≤ `MAX_POINTS` points, each ≤ `MAX_DISCOUNT_BPS` |
| `surplus_decay` (schedule) | ≤ `MAX_POINTS` points, each ≤ `MAX_DISCOUNT_BPS` |
| `max_mint_per_auction` | > 0; ≤ `MAX_MINT_HARD_CAP` |
| `min_take` | ≤ `surplus_lot` and ≤ `debt_lot` |
| `system_ttl_ms` | `MIN_SYSTEM_TTL_MS..=MAX_SYSTEM_TTL_MS` |

Lock note: while a system auction is live (surplus or debt), `set_engine_params` refuses any change to the protocol token, the reference oracle, or any system-auction parameter (`surplus_*`, `debt_*`, both schedules, `max_mint_per_auction`, `min_take`, `system_ttl_ms`): the check is `system_rec != cfg.system` (`programs/auction_engine/src/admin.rs:201-206`, `ProtocolTokenLocked`; v04: `v04/sdk/src/system.rs:115-118`). `global_cap`, `freeze_on_bad_debt`, `bad_debt_freeze_threshold` and `ref_oracle_max_age_ms` stay changeable.

v04 only: `slack_ms`, set by the admin at `Init`, changeable by the admin through `SetSlack`,
bounded by `MIN_SLACK_MS..=MAX_SLACK_MS`.

### `CollateralArgs` (`args.rs:134`): set by the admin per collateral type through `set_collateral_config` / `SetCollateral`, changeable the same way (a live auction keeps its snapshotted schedule)

| Parameter | Bound in code |
|---|---|
| `oracle_scale` | 0 (Ray) or 1 (Q64x64) |
| `max_oracle_age_ms` | > 0; ≤ `MAX_ORACLE_AGE_MS` |
| `max_conf_bps` | ≤ `MAX_CONF_BPS` |
| `band_lo_bps` | 5_000..=10_000 |
| `band_hi_bps` | 10_000..=20_000 |
| `penalty_bps` | ≤ `MAX_PENALTY_BPS` |
| `tip` | — |
| `chip_bps` | ≤ `penalty_bps` |
| `reward_cap` | — |
| `debt_floor` | `chost` = ceil(`debt_floor` · (10_000 + `penalty_bps`) / 10_000) must be ≥ `min_bid` (`ChostBelowMinBid`, `CollateralParams::validate`, `liquidation.rs:31-33`) |
| `min_bid` | ≤ `chost` (`ChostBelowMinBid`, `liquidation.rs:31-33`) |
| `type_cap` | > 0; ≤ `global_cap` |
| `schedule` | ≤ `MAX_POINTS` points, first at 0 ms, time increasing, discount non-decreasing, each ≤ `MAX_DISCOUNT_BPS` |
| `admin_paused` | — |

## F8 gaps (production default — Tier 2)

Five values are fixed in code and cannot be set by the admin. Each is carried into spec §5 E9:

1. Reference-oracle confidence limit: `MAX_CONF_BPS` (1_000 bps), used as the value rather than
   as a cap.
2. Reference-oracle clamp: `REF_ORACLE_CLAMP_BPS` (±2_000 bps around the admin price).
3. Debt-auction relist delay: `DEBT_RELIST_DELAY_MS` (600_000 ms).
4. Feed sanity window: market price within [R / 2, 2 · R] (inline in `validate_feed`).
5. Reference-oracle scale: hardcoded to `OracleScale::Ray` (`programs/auction_engine/src/system.rs:44-50`, `v04/engine_core/src/system.rs:41-47`), so a Q64x64 reference oracle cannot be configured, unlike a collateral feed's `oracle_scale`. It becomes an init parameter with Ray as the default.
