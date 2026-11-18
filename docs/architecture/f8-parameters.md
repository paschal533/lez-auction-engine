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
- **init parameter**: the value of a parameter the admin sets in a message. On v04 the five
  values that were F8 gaps are init parameters since Plan 10 (spec §3.9, E9); the constant is
  their legacy value. The frozen v0.2.4 engine (ADR-069) still uses these constants as fixed
  values, so on v0.2.4 they stay fixed.
- **production default — Tier 2**: a value the admin cannot set that a production deployment
  would want to choose (an F8 gap). None is left: Plan 10 made the last five init parameters on
  v04, and `scripts/check-requirements.py` fails if a table row of this page has this kind.

## Constants

| Constant | Value | File:line | Kind | Why |
|---|---|---|---|---|
| `RAY` | 10^27 | `crates/auction_core/src/units.rs:3` | math constant | Fixed-point scale for prices and discounts. |
| `BPS` | 10_000 | `crates/auction_core/src/units.rs:4` | math constant | Basis-point denominator; also the ceiling of every bps argument (`args.rs` `bps()`). |
| `BPS_TO_RAY` | 10^23 | `crates/auction_core/src/units.rs:6` | math constant | 1 bps in RAY (`RAY / BPS`). |
| `MAX_PENALTY_BPS` | 2_500 | `crates/auction_core/src/liquidation.rs:9` | init parameter bound | Caps the admin-set `penalty_bps` per collateral type (`CollateralParams::validate`). |
| `MAX_ORACLE_AGE_MS` | 86_400_000 (24 h) | `crates/auction_core/src/price.rs:30` | init parameter bound | Caps the admin-set `max_oracle_age_ms` (collateral feed) and `ref_oracle_max_age_ms` (reference oracle, `args.rs:80`). |
| `MAX_CONF_BPS` (collateral feed) | 1_000 | `crates/auction_core/src/price.rs:31` | init parameter bound | Caps the admin-set `max_conf_bps` of a collateral feed (`FeedParams::validate`). |
| `MAX_CONF_BPS` (reference oracle) | 1_000 | `crates/auction_core/src/price.rs:31`, used at `programs/auction_engine/src/system.rs:47` | init parameter | v0.2.4 uses this constant as the reference oracle's confidence limit. v04 reads the admin-set `ref_conf_bps` of `EngineExtArgs` instead (`v04/engine_core/src/system.rs:50`), legacy 1_000, bounded by `REF_CONF_MAX_BPS`, which equals this constant. |
| `REF_ORACLE_CLAMP_BPS` | 2_000 (±20 %) | `crates/auction_core/src/price.rs:32` | init parameter | Clamp of a valid reference-oracle price around the admin price (`effective_ref_price`, ADR-017), fixed on v0.2.4. v04 calls `effective_ref_price_ext` with the admin-set `ref_clamp_bps` (`v04/engine_core/src/system.rs:65`), legacy 2_000, bounded by `REF_CLAMP_MAX_BPS`. |
| `MAX_POINTS` | 8 | `crates/auction_core/src/schedule.rs:8` | protocol limit | Fixed size of the snapshotted schedule array; the admin chooses up to 8 points. |
| `MAX_DISCOUNT_BPS` | 3_000 | `crates/auction_core/src/schedule.rs:9` | init parameter bound | Caps every point of the admin-set collateral discount schedule. |
| `MAX_MINT_HARD_CAP` | 10^27 base units | `crates/auction_core/src/system.rs:9` | init parameter bound | Caps the admin-set `max_mint_per_auction` (`SystemParams::validate`, re-checked at mint). The per-auction mint cap itself is an admin parameter. |
| `MAX_SYSTEM_DISCOUNT_BPS` | 3_000 | `crates/auction_core/src/system.rs:52` | init parameter bound | Caps the admin-set `surplus_max_discount_bps` and `debt_initial_discount_bps`. |
| `MIN_SYSTEM_TTL_MS` | 600_000 (10 min) | `programs/auction_engine/core/src/args.rs:15` | init parameter bound | Lower end of the admin-set `system_ttl_ms` (spec §5.12). |
| `MAX_SYSTEM_TTL_MS` | 604_800_000 (7 d) | `programs/auction_engine/core/src/args.rs:16` | init parameter bound | Upper end of the admin-set `system_ttl_ms` (spec §5.12). |
| `DEBT_RELIST_DELAY_MS` | 600_000 (10 min) | `programs/auction_engine/core/src/args.rs:20`, used at `programs/auction_engine/src/debt.rs:67` | init parameter | Minimum gap between one debt auction's end and the next one's start (ADR-039), fixed on v0.2.4. v04's `debt_precheck` takes the admin-set `debt_relist_delay_ms` (`v04/engine_core/src/system.rs:179`, from `SystemCtx.ext`), legacy 600_000, bounded by `RELIST_DELAY_MIN_MS..=RELIST_DELAY_MAX_MS`. |
| `MIN_SLACK_MS` (v04 only) | 1_000 | `v04/engine_core/src/check.rs:14` | init parameter bound | Lower end of the admin-set clock slack `slack_ms` (`Init`, `SetSlack`; ADR-064). |
| `MAX_SLACK_MS` (v04 only) | 300_000 | `v04/engine_core/src/check.rs:17` | init parameter bound | Upper end of the admin-set clock slack `slack_ms`. |
| feed band ranges *(inline)* | `band_lo_bps` in 5_000..=10_000, `band_hi_bps` in 10_000..=20_000 | `crates/auction_core/src/price.rs:36-37` | init parameter bound | Range of the admin-set bid-price band around the redemption price (`FeedParams::validate`). |
| feed sanity window *(inline)* | market price within [R / 2, 2 · R] | `crates/auction_core/src/price.rs:79-80` | init parameter | v0.2.4's `validate_feed` pauses a collateral type whose market price is outside half to twice the redemption price R, and drops a reference-oracle reading outside half to twice the admin price; the factor 2 is fixed there. v04's `validate_feed_ext` takes the window from the admin: per type `sanity_lo_bps` / `sanity_hi_bps` (`RiskArgs`, `v04/engine_core/src/feed.rs:43-44`) and, for the reference oracle, `ref_sanity_lo_bps` / `ref_sanity_hi_bps` (`EngineExtArgs`, `v04/engine_core/src/system.rs:51-52`); legacy 5_000 / 20_000. |
| reference-oracle scale *(inline)* | `OracleScale::Ray` | `programs/auction_engine/src/system.rs:44-50` | init parameter | v0.2.4 fixes the reference oracle's `FeedParams.scale` to Ray. v04 reads the admin-set `ref_scale` (0 Ray, 1 Q64x64; `v04/engine_core/src/system.rs:42-46`), legacy 0, so a Q64x64 reference oracle can be configured. |
| reference price step *(inline)* | new price within [old / 2, 2 · old] | `programs/auction_engine/core/src/args.rs:226` | init parameter bound | Bounds each admin change of `protocol_token_ref_price` (`check_ref_price_step`, ADR-031); not applied at init, where there is no old value. |

Counts per Kind: math constant 3 · init parameter bound 12 · protocol limit 1 · init parameter
5 (21 rows; `MAX_CONF_BPS` has one row per role).

### Not listed

- Test-only constants (`#[cfg(test)]` modules): `liquidation.rs:110` `P`, `price.rs:114` `R`,
  `system.rs:394` and `:554` `REF` in `auction_core`; `args.rs:242` `CAP`; `v04/engine_core`
  `assertions.rs:189` `ENGINE`, `bid.rs:132` `START`, `solvency.rs:84-87` and
  `system.rs:320-321` `RAY` / `NOW` / `THRESHOLD`.
- Non-economic constants: `math.rs:10` `LO64` (64-bit mask), `v04/engine_core/src/msg.rs:16`
  `SEIZED_INDEX` (wire variant index), `v04/engine_core/src/feed.rs:26` `CONF_SATURATED_BPS`
  (the confidence recorded for a valid reading whose confidence does not convert, a case
  `validate_feed_ext` already refuses; its own comment calls it the safe side), the account tags `TAG_*` in `v04/engine_core/src/state.rs`
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

Lock note: while a system auction is live (surplus or debt), `set_engine_params` refuses any change to the protocol token, the reference oracle, or any system-auction parameter (`surplus_*`, `debt_*`, both schedules, `max_mint_per_auction`, `min_take`, `system_ttl_ms`): the check is `system_rec != cfg.system` (`programs/auction_engine/src/admin.rs:201-206`, `ProtocolTokenLocked`; v04: `v04/engine_core/src/assertions.rs:73-84`, `system_idle` → `ProtocolTokenLocked`; the SDK adds the accounting actor when locked fields change, `v04/sdk/src/system.rs:118-124`). `global_cap`, `freeze_on_bad_debt`, `bad_debt_freeze_threshold` and `ref_oracle_max_age_ms` stay changeable. On v04 a change of `EngineExtArgs.ref_scale` is a locked-field change too (Plan 10 ruling R10-5; `changing_ref_scale_needs_an_idle_system`); the other five `EngineExtArgs` fields stay changeable.

v04 only: `slack_ms`, set by the admin at `Init`, changeable by the admin through `SetSlack`,
bounded by `MIN_SLACK_MS..=MAX_SLACK_MS`.

### `EngineExtArgs` (v04, `Init` and `SetEngineParams`)

v04 only (Plan 10, E9). `Init` and `SetEngineParams` carry an `ext: Box<EngineExtArgs>` as their
last field. The config validates it with `EngineExtArgs::validate` (`v04/risk_core/src/engine_ext.rs`;
any failure is `InvalidConfig`, 6005, `engine_core::ext::validate_engine_ext`) and stores it in
`ConfigExt.engine`; `SystemCtx.ext` carries a copy to the accounting. The engine has no defaults:
every value arrives in the message. `SetPauseLevel` keeps it (`set_pause_level_keeps_the_engine_ext`).
The recommended preset equals the legacy one: E9 is about who chooses, not about new numbers
(spec §3.9).

| Field | legacy | recommended | Bound in code (else `InvalidConfig`) | ADR |
|---|---|---|---|---|
| `ref_conf_bps` | 1_000 | 1_000 | `<= REF_CONF_MAX_BPS` (1_000) | ADR-082 |
| `ref_clamp_bps` | 2_000 (±20 %) | 2_000 | `<= REF_CLAMP_MAX_BPS` (5_000) | ADR-017, ADR-082 |
| `debt_relist_delay_ms` | 600_000 (10 min) | 600_000 | `RELIST_DELAY_MIN_MS..=RELIST_DELAY_MAX_MS` (60_000..=604_800_000) | ADR-039, ADR-082 |
| `ref_sanity_lo_bps` | 5_000 | 5_000 | `SANITY_LO_MIN_BPS..=10_000` (1_000..=10_000) | ADR-082 |
| `ref_sanity_hi_bps` | 20_000 | 20_000 | `10_000..=SANITY_HI_MAX_BPS` (10_000..=100_000) | ADR-082 |
| `ref_scale` | 0 (Ray) | 0 | 0 (Ray) or 1 (Q64x64); a change needs an idle system (R10-5) | ADR-082 |

The fields are checked in this order; the first failure is returned. The 60_000 floor of the relist
delay is below `MIN_SYSTEM_TTL_MS` (600_000): ADR-039's reason is anti-spam, and the admin now owns
the choice (ruling R10-2). `risk_core` cannot name `auction_engine_params`, so it defines
`REF_CONF_MAX_BPS` and `RELIST_DELAY_MAX_MS` itself; `legacy_preset_equals_the_v024_constants`
(`v04/engine_core/src/system_ref_tests.rs`) asserts they equal `MAX_CONF_BPS` and
`MAX_SYSTEM_TTL_MS`, and that each legacy value equals the v0.2.4 constant it replaces.

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

### `RiskArgs` (v04, per type, `SetCollateral`)

v04 only (Plan 9; six fields appended in Plan 10). `SetCollateral` carries a `risk: Box<RiskArgs>`
as its last field, and the config relays it to the collateral actor in `Configure`.
`RiskArgs::validate(max_oracle_age_ms, band_lo_bps, band_hi_bps)` (`v04/risk_core/src/args.rs`)
checks it against the same message's `max_oracle_age_ms` and band; any failure is `InvalidConfig`
(6005, `engine_core::ext::validate_risk`). The engine has no defaults: every value arrives in the
message. Changeable the same way as `CollateralArgs`; a new `SetCollateral` keeps the type's TWAP
ring, pause level and grace state (`last_good_ts_ms`, `feed_invalid_since_ms`, `grace_until_ms`;
`set_collateral_keeps_the_grace_state`).

| Field | legacy | recommended | Bound in code (else `InvalidConfig`) | ADR |
|---|---|---|---|---|
| `twap_window_ms` | 0 (TWAP off) | 1_800_000 (30 min) | 0 or `TWAP_WINDOW_MIN_MS..=TWAP_WINDOW_MAX_MS` (60_000..=86_400_000); with D4 on (`stale_bid_max_age_ms > 0`), `> max_oracle_age_ms + (twap_min_obs − 2) · twap_min_spacing_ms` (saturating; `twap_min_obs − 2` floors at 0) (R9-3); TWAP on: `<= 8 · twap_min_spacing_ms` (the ring's capacity) | ADR-070, ADR-074 |
| `twap_min_spacing_ms` | 0 | 300_000 (5 min) | TWAP on: `TWAP_SPACING_MIN_MS..=twap_window_ms` (1_000..); `<= max_oracle_age_ms` (R9-3); TWAP off: 0 | ADR-070 |
| `twap_min_obs` | 0 | 3 | TWAP on: `1..=TWAP_MIN_OBS_MAX` (8); `<= twap_window_ms / twap_min_spacing_ms` (R9-3); TWAP off: 0 | ADR-070 |
| `max_spot_twap_dev_bps` | 0 (breaker and bid anchor off) | 2_000 (±20 %) | 0 or `DEV_MIN_BPS..=DEV_MAX_BPS` (100..=5_000); nonzero only with the TWAP on | ADR-072 |
| `stale_bid_max_age_ms` | 0 (D4 off) | 3_600_000 (1 h) | `<= MAX_ORACLE_AGE_MS` (86_400_000); nonzero only with the TWAP on; when nonzero, `> max_oracle_age_ms` (R9-3) | ADR-074 |
| `sanity_lo_bps` | 5_000 | 5_000 | `SANITY_LO_MIN_BPS..=band_lo_bps` (1_000..=the type's `band_lo_bps`) | ADR-082 |
| `sanity_hi_bps` | 20_000 | 20_000 | `band_hi_bps..=SANITY_HI_MAX_BPS` (the type's `band_hi_bps`..=100_000) | ADR-082 |
| `feed_min_raw` | 0 (off) | 0 (off); the SDK's `recommended_risk(OracleBounds::Declared { .. })` sets the oracle's bound | `< feed_max_raw` | ADR-075 |
| `feed_max_raw` | `u128::MAX` (off) | `u128::MAX` (off); set the same way | `> feed_min_raw` | ADR-075 |
| `grace_ms` | 0 (grace off) | 1_800_000 (30 min) | `<= GRACE_MAX_MS` (86_400_000) | ADR-075 |
| `grace_min_gap_ms` | 0 | 600_000 (10 min) | `<= GRACE_MAX_MS`; nonzero when `grace_ms > 0` (else every reading would open a grace) | ADR-075 |

Order of the checks: the ranges above field by field, then the R9-3 bounds, then "TWAP off means
every other field is 0", then the six Plan 10 fields in table order (ruling R10-7); the first failure is returned (`RiskArgsError.field`, not on the wire).
With the TWAP off, a nonzero `max_spot_twap_dev_bps` or `stale_bid_max_age_ms` is already refused
by its own range row, so the "TWAP off" rule decides only `twap_min_spacing_ms` and
`twap_min_obs`; the wire code is `InvalidConfig` either way.

**The recommended preset needs a short feed max age.** By R9-3 it validates only when the type's
`max_oracle_age_ms` lies in 300_000..=1_499_999 (spacing `<= max_age`; stale 3_600_000 `> max_age`;
window 1_800_000 `> max_age + 300_000`). The harness's Tier 2 world uses 600_000
(`TIER2_MAX_AGE_MS`, `v04/harness/src/tier2.rs`); `CollateralArgs::defaults_for_tests()`
(3_600_000) refuses it. The six Plan 10 fields never refuse the recommended preset: its sanity
window [5_000, 20_000] contains every band `validate` allows (5_000..=10_000 and 10_000..=20_000).
The e2e `tier2` stage uses a time-compressed copy (window 300_000, spacing
40_000, `min_obs` 3, dev 2_000, stale 600_000, grace 30_000, grace gap 30_000, with
`max_oracle_age_ms` 180_000). Its window is 7.5
spacings, inside the ring bound below. Its spacing is still below the stage's 60 s slack, so the
stage exercises the flow, not TWAP manipulation resistance.

**Window vs ring capacity (enforced).** The ring holds `TWAP_CAPACITY` = 8 samples, and the
oldest has no weight before its own time. A window longer than 8 spacings could be cut short by
anyone: one permissionless `ObserveFeed` per spacing interval evicts older samples. So
`RiskArgs::validate` refuses `twap_window_ms > 8 · twap_min_spacing_ms` (field `twap_window_ms`,
wire `InvalidConfig`; `risk_args_refuse_a_window_longer_than_the_ring`). At the bound, a full ring
spans at least 7 spacings, so the TWAP always covers at least 7/8 of its window. The recommended
preset has 6 spacings per window. Two consequences: the spacing may not exceed
`max_oracle_age_ms` (R9-3), so the window is at most 8 feed max ages; and the longest window
(24 h) needs a spacing of at least 3 h. A related unenforced case: with
`twap_min_spacing_ms <= slack_ms` one oracle publication can be recorded twice, because the sample
time is the claimed `now` (ADR-070).

**Legacy is v04 as built, at confidence 0.** Legacy turns the TWAP, the breaker and D4 off, so
every price is `band(edge(M))`. The confidence edge (ADR-071) is always on: with confidence 0, as
the existing producers write, it is the identity; with a nonzero confidence the legacy preset
pays out less collateral than v0.2.4 would.

**Where the presets live.** `RiskArgs::legacy()`, `RiskArgs::recommended()`,
`EngineExtArgs::legacy()` and `EngineExtArgs::recommended()` are in `risk_core::presets`
(`v04/risk_core/src/presets.rs`), behind the cargo feature `presets`. `sdk` and `harness` enable it
(and `engine_core`'s dev-dependency, for its unit tests). `engine_core`'s normal dependency and the
guest do not, so no production value is compiled into on-chain code (ADR-082). The SDK re-exports
`RiskArgs` (not `EngineExtArgs`, which callers name as `risk_core::EngineExtArgs`).

**Oracle bounds (ruling R10-8).** The pin bounds belong to the oracle, so `recommended()` keeps them
off. The SDK adds `OracleBounds { Undeclared, Declared { min_raw, max_raw } }` and
`recommended_risk(bounds)` (`v04/sdk/src/builder.rs`), which returns the recommended preset with
`feed_min_raw` / `feed_max_raw` set from a `Declared` oracle and off for `Undeclared`
(`recommended_risk_takes_the_oracles_bounds`). The type makes the caller state the bounds when it
uses `recommended_risk`; `RiskArgs::recommended()` itself stays public, so it does not force them,
and the SDK does not validate `Declared` bounds (`SetCollateral` refuses `min_raw >= max_raw`).

New v04 constants (`v04/risk_core/src`), outside the table above:

| Constant | Value | File | Kind |
|---|---|---|---|
| `TWAP_CAPACITY` | 8 | `twap.rs` | protocol limit (ring size, like `MAX_POINTS`) |
| `TWAP_WINDOW_MIN_MS`, `TWAP_WINDOW_MAX_MS` | 60_000, 86_400_000 | `args.rs` | init parameter bound |
| `TWAP_SPACING_MIN_MS` | 1_000 | `args.rs` | init parameter bound |
| `TWAP_MIN_OBS_MAX` | 8 | `args.rs` | init parameter bound |
| `DEV_MIN_BPS`, `DEV_MAX_BPS` | 100, 5_000 | `args.rs` | init parameter bound |
| `GRACE_MAX_MS` | 86_400_000 | `args.rs` | init parameter bound (`grace_ms`, `grace_min_gap_ms`) |
| `REF_CONF_MAX_BPS` | 1_000 | `engine_ext.rs` | init parameter bound (`ref_conf_bps`; equals `MAX_CONF_BPS`) |
| `REF_CLAMP_MAX_BPS` | 5_000 | `engine_ext.rs` | init parameter bound (`ref_clamp_bps`) |
| `RELIST_DELAY_MIN_MS`, `RELIST_DELAY_MAX_MS` | 60_000, 604_800_000 | `engine_ext.rs` | init parameter bound (`debt_relist_delay_ms`; the maximum equals `MAX_SYSTEM_TTL_MS`) |
| `SANITY_LO_MIN_BPS`, `SANITY_HI_MAX_BPS` | 1_000, 100_000 | `engine_ext.rs` | init parameter bound (`sanity_lo_bps` / `sanity_hi_bps` and `ref_sanity_lo_bps` / `ref_sanity_hi_bps`) |
| `SANITY_PAR_BPS`, `REF_SCALE_MAX` (private) | 10_000, 1 | `engine_ext.rs` | init parameter bound (the reference window must contain par; `ref_scale` 0 or 1) |
| `LEGACY_SANITY_LO_BPS`, `LEGACY_SANITY_HI_BPS` | 5_000, 20_000 | `feed.rs` | init parameter (the legacy sanity window. `FeedPolicy::legacy` starts from them, and `engine_core::feed::feed_policy` replaces both with the type's `sanity_lo_bps` / `sanity_hi_bps`, so on chain they are never the window used) |

## F8 gaps

None is left on v04. The audit found five values fixed in code that the admin could not set; Plan
10 (spec §3.9, E9) made each an init parameter of the v04 engine, with its old value as the legacy
(and recommended) value. `scripts/check-requirements.py` fails if a table row above has the kind
"production default — Tier 2" again. The frozen v0.2.4 engine still fixes all five (ADR-069).

| Former gap | v04 parameter | Closing tests |
|---|---|---|
| 1. Reference-oracle confidence limit: `MAX_CONF_BPS` (1_000 bps), used as the value rather than as a cap | `EngineExtArgs.ref_conf_bps` | `ref_conf_limit_is_a_parameter` (`v04/engine_core/src/system_ref_tests.rs`) |
| 2. Reference-oracle clamp: `REF_ORACLE_CLAMP_BPS` (±2_000 bps around the admin price) | `EngineExtArgs.ref_clamp_bps` | `ref_clamp_is_a_parameter` (`system_ref_tests.rs`) |
| 3. Debt-auction relist delay: `DEBT_RELIST_DELAY_MS` (600_000 ms) | `EngineExtArgs.debt_relist_delay_ms` | `debt_relist_delay_is_a_parameter` (`system_ref_tests.rs`), `relist_delay_follows_the_engine_ext` (`v04/harness/tests/debt.rs`) |
| 4. Feed sanity window: market price within [R / 2, 2 · R] (inline in `validate_feed`) | `RiskArgs.sanity_lo_bps` / `sanity_hi_bps` (collateral feed, per type); `EngineExtArgs.ref_sanity_lo_bps` / `ref_sanity_hi_bps` (reference oracle) | `sanity_window_is_a_parameter` (`v04/engine_core/src/feed.rs`), `sanity_window_bounds_are_inclusive` (`v04/risk_core/src/args.rs`), `ref_sanity_window_is_a_parameter` (`system_ref_tests.rs`) |
| 5. Reference-oracle scale: hardcoded to `OracleScale::Ray` | `EngineExtArgs.ref_scale` (0 Ray, 1 Q64x64) | `q64_reference_oracle_reads` (`system_ref_tests.rs`), `changing_ref_scale_needs_an_idle_system` (`v04/harness/tests/engine_ext.rs`) |

The wire and state: `init_writes_the_engine_ext`, `init_rejects_an_out_of_bound_engine_ext`,
`set_engine_params_replaces_the_engine_ext`, `set_engine_params_rejects_an_out_of_bound_engine_ext`,
`set_pause_level_keeps_the_engine_ext` (`v04/harness/tests/engine_ext.rs`); the bounds:
`engine_ext_bounds_are_inclusive`, `engine_ext_refuses_the_first_bad_field`,
`engine_ext_presets_validate` (`v04/risk_core/src/engine_ext.rs`); the legacy values:
`legacy_preset_equals_the_v024_constants` (`system_ref_tests.rs`); the per-type fields:
`set_collateral_rejects_out_of_bound_risk_args`, `set_collateral_writes_the_risk_args`
(`v04/harness/tests/risk_args.rs`).
