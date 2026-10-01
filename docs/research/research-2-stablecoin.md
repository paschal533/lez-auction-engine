# RFP-014 research brief: the stablecoin CDP, the oracle, token/ATA, chained calls and hook design

**Sources read (shallow clones, read-only):**

| Repo | Ref read | SHA |
|---|---|---|
| logos-blockchain/lez-programs | `main` HEAD | `64c01210adecd6b557dbd021a3cc94553f243fd8` (last stablecoin commit: "feat(stablecoin): expose stability fee accumulator") |
| logos-blockchain/logos-execution-zone (LEE/LEZ runtime) | tag `v0.2.4` (the version lez-programs pins) | `47eba256479f6f785acbd138834340703cd03401` |
| logos-co/spel | rev pinned by lez-programs | `1ef0500f8fc8ce3ddf95523726ae159db83f744f`. HEAD `415b7bcb…` was also checked for the docs. |
| logos-co/rfp | master | `96c0244df538788466553a0ea78153c8e812a415` |
| logos-co/lambda-prize | master, LP-0013.md and LP-0015.md | (raw fetch) |

All `lez-programs` paths below are relative to the repo root.

---

## 0. Findings that change the design

1. **A callee can tell which program called it, and the runtime enforces this.** `ProgramInput.caller_program_id: Option<ProgramId>` is set by the state machine. The machine also rejects any output whose `caller_program_id` differs from the real caller (`lee/state_machine/src/validated_state_diff/mod.rs`: `ensure!(program_output.caller_program_id == caller_data.program_id, …MismatchedCallerProgramId…)`). SPEL exposes it as `ctx.caller_program_id`, which is **`DEFAULT_PROGRAM_ID` (all zeros) for a top-level tx** (`spel-framework-macros/src/lib.rs:892-897`). A second, unforgeable gate is a **PDA signer**: an account counts as `is_authorized` in a chained call only if it is `for_public_pda(caller_program_id, seed)` for a seed the caller listed in `pda_seeds`, or if it was already authorized up-chain.
2. **Chained calls return no data.** They run **after** the caller's output is final, in depth-first order. Any panic anywhere reverts the whole transaction. The engine must therefore compute the seize amounts itself, pass them as instruction arguments, and let the stablecoin assert that they match.
3. **`Token::Burn` needs only the holder's authorization. It never consults the mint authority.** The engine can burn stablecoin it raised straight from its own PDA holding, so the stablecoin program does not need to be involved in burns.
4. **The collateralization check uses the internal *redemption price*, not the oracle.** The oracle only feeds the PI controller and acts as a liveness/staleness gate on `generate_debt`. The collateral itself is the unit of account.
5. **The price scales do not match: TWAP writes Q64.64 (`1.0 = 2^64`), but the stablecoin reads `OraclePriceAccount.price` as 1e27 fixed point.** This is acknowledged upstream: open PR #401 says "Nonzero controller gains are rejected until the TWAP Q64.64 and protocol 10^27 price scales are reconciled". PR #397's deploy script uses a *static*, wallet-owned oracle at 1e27 scale.
6. **The stablecoin is not deployed anywhere.** `DEPLOYMENTS.md` (testnet v0.2.4) lists only token, amm, twap_oracle and token_mint_authority. Deploy tooling is still in open PRs (#397, #401).

---

## 1. The stablecoin program (`programs/stablecoin`)

**Layout:**

- Host logic is in `programs/stablecoin/src/*.rs`.
- Data types and the instruction enum are in `programs/stablecoin/core/src/{lib.rs,protocol_parameters.rs,stability_fee_accumulator.rs,redemption_price_state.rs,math.rs,controller.rs}`.
- The SPEL guest is `programs/stablecoin/methods/guest/src/bin/stablecoin.rs`: `#[lez_program(instruction = "stablecoin_core::Instruction")]`.

**Dependencies:** `lee_core` and `clock_core` at LEZ tag `v0.2.4`; `spel-framework` at rev `1ef0500…`; `token_core` and `twap_oracle_core` as path deps.

**ABI note:** the wire format is the **variant index** of `stablecoin_core::Instruction` under risc0 serde (spel `docs/reference/macros.md` §Chained calls: "serialization is structural — variant index, then fields"). New variants must be **appended** after `RepayDebt`.

**Every instruction:**

- Each handler returns `(Vec<AccountPostState>, Vec<ChainedCall>)`.
- Every handler gets `ctx.self_program_id`. None use `caller_program_id` today.
- Account order is shown below. `S` = `#[account(signer)]`, `M` = `mut`, `I` = `init`.
- In the variant list (`core/src/lib.rs:42-372`) the enum order is: InitializeProgram, AccrueStabilityFee, UpdateRedemptionRate, RefreshGlobals, OpenPosition, DepositCollateral, GenerateDebt, ClosePosition, SetMinimumCollateralizationRatio, SetControllerGains, SetMarketPriceOracle, SetTimingParameters, SetAdmin, SetFreezeAuthority, SetStabilityFeePerMillisecond, Freeze, Unfreeze, WithdrawCollateral, RepayDebt.

| Instruction (args) | Accounts in order | Chained calls |
|---|---|---|
| `initialize_program(freeze_authority_account_id: AccountId, initial_stability_fee_per_millisecond: u128, initial_controller_proportional_gain: i128, initial_controller_integral_gain: i128, initial_minimum_collateralization_ratio: u128, minimum_milliseconds_between_rate_updates: u64, maximum_oracle_price_age_milliseconds: u64, initial_redemption_price: u128, stablecoin_name: String)` | admin S; protocol_parameters I; stability_fee_accumulator I; redemption_price_state I; stablecoin_definition I; stablecoin_master_holding I; collateral_definition; market_price_oracle; clock | `Token::NewFungibleDefinition{name, total_supply:0, mint_authority: Some(stablecoin_definition_id)}` with seeds `[def_seed, master_seed]`. The three globals are claimed via `Claim::Pda`. |
| `accrue_stability_fee()` | caller S; protocol_parameters; stability_fee_accumulator M; clock | none |
| `update_redemption_rate()` (strict) | caller S; protocol_parameters; redemption_price_state M; market_price_oracle; clock | none |
| `refresh_globals()` (best-effort, both halves) | caller S; protocol_parameters; stability_fee_accumulator M; redemption_price_state M; market_price_oracle; clock | none |
| `open_position(position_nonce: u64, initial_collateral_amount: u128)` | owner S; position I; vault I; user_collateral_holding M S; collateral_definition; protocol_parameters; clock | `Token::InitializeAccount` [collateral_def, vault (authorized via `vault_seed`)], then `Token::Transfer` [user_holding, post-init vault] |
| `deposit_collateral(amount)` (allowed while frozen; sets collateral = vault balance + amount) | owner S; position M; vault M; user_collateral_holding M S; protocol_parameters | `Token::Transfer` user→vault |
| `generate_debt(amount)` (blocked while frozen) | owner S; position M; stablecoin_definition M; user_stablecoin_holding M; stability_fee_accumulator; redemption_price_state; market_price_oracle; protocol_parameters; clock | `Token::Mint` [definition authorized via `compute_stablecoin_definition_pda_seed()`, user_holding] |
| `close_position()` | owner S; position M; vault; protocol_parameters | none. Position is cleared to `Account::default()` data; the PDA lingers. |
| `withdraw_collateral(amount)` (blocked while frozen) | owner S; position M; vault M; user_collateral_holding M; stability_fee_accumulator; redemption_price_state; protocol_parameters; clock | `Token::Transfer` [vault authorized via `vault_seed`, user_holding] |
| `repay_debt(amount)` (allowed while frozen) | owner S; position M; stablecoin_definition M; user_stablecoin_holding M S; stability_fee_accumulator; protocol_parameters; clock | `Token::Burn` [definition, user_holding] (no PDA seed) |
| `set_stability_fee_per_millisecond(new_rate)` | admin S; protocol_parameters M; stability_fee_accumulator M; clock | none (accrues at the old rate first) |
| `set_minimum_collateralization_ratio(new_ratio)` | admin S; protocol_parameters M | none |
| `set_controller_gains(kp: i128, ki: i128)` | admin S; protocol_parameters M | none |
| `set_timing_parameters(min_ms_between_updates: u64, max_oracle_age_ms: u64)` | admin S; protocol_parameters M | none |
| `set_admin(new_admin_account_id)` | admin S; protocol_parameters M | none |
| `set_freeze_authority(new_freeze_authority_account_id)` | admin S; protocol_parameters M | none |
| `set_market_price_oracle()` | admin S; protocol_parameters M; new_oracle | none. Validates shape and base/quote; does **not** pin `program_owner`. |
| `freeze()` / `unfreeze()` | freeze_authority S; protocol_parameters M | none |

### PDA seeds (all `AccountId::for_public_pda(&stablecoin_program_id, &seed)`)

| PDA | How the seed is built | Constant / source |
|---|---|---|
| ProtocolParameters | `sha256(b"STABLECOIN__PROTOCOL_PARAMS_____")` | `core/src/protocol_parameters.rs`. The design doc says `"PROTOCOL_PARAMETERS"`; **the code wins**. |
| StabilityFeeAccumulator | `sha256(b"STABILITY_FEE_ACCUMULATOR")` | `core/src/stability_fee_accumulator.rs` |
| RedemptionPriceState | `sha256(b"REDEMPTION_PRICE_STATE")` | `core/src/redemption_price_state.rs` |
| Stablecoin definition | `sha256(b"STABLECOIN__DEFINITION__________")` | `core/src/lib.rs:37` |
| Stablecoin master holding | `sha256(b"STABLECOIN__MASTER_HOLDING______")` | `core/src/lib.rs:38` |
| Position | `sha256(owner_id.to_bytes() ‖ position_nonce.to_le_bytes() ‖ b"POSITION")` | `compute_position_pda_seed`, `core/src/lib.rs:431` |
| Position vault | `sha256(position_id.to_bytes() ‖ b"POSITION_VAULT")` | `compute_position_vault_pda_seed`, `core/src/lib.rs:462` |

The runtime address formula (`lee/state_machine/core/src/program/mod.rs:127`) uses the prefix `b"/LEE/v0.2/AccountId/PDA/\0…"` hashed with the program id and the seed.

### Account data structs

All are Borsh-encoded with `#[account_type]`. `FIXED_POINT_ONE = 10^27` (`core/src/math.rs:12`).

```rust
// core/src/protocol_parameters.rs
pub struct ProtocolParameters {
    pub admin_account_id: AccountId,
    pub freeze_authority_account_id: AccountId,
    pub stablecoin_definition_id: AccountId,      // immutable
    pub collateral_definition_id: AccountId,      // immutable, single collateral per instance
    pub market_price_oracle_id: AccountId,
    pub stability_fee_per_millisecond: u128,      // (1+r)*1e27, band [1e27, 2e27]
    pub controller_proportional_gain: i128,
    pub controller_integral_gain: i128,
    pub minimum_collateralization_ratio: u128,    // band [1.1e27, 10e27]
    pub minimum_milliseconds_between_rate_updates: u64, // 1..=86_400_000
    pub maximum_oracle_price_age_milliseconds: u64,     // 1..=86_400_000
    pub is_frozen: bool,
}
// core/src/stability_fee_accumulator.rs
pub struct StabilityFeeAccumulator { pub accumulated_rate_at_last_accrual: u128, pub last_accrued_at: u64 }
// core/src/redemption_price_state.rs
pub struct RedemptionPriceState {
    pub redemption_price_at_last_update: u128,   // collateral-per-stablecoin, 1e27
    pub redemption_rate_per_millisecond: u128,
    pub controller_integral_term: i128,
    pub last_updated_at: u64,
}
// core/src/lib.rs:385
pub struct Position {
    pub owner_account_id: AccountId, pub position_nonce: u64, pub vault_account_id: AccountId,
    pub collateral_amount: u128, pub normalized_debt_amount: u128, pub opened_at: u64,
}
```

### How the vault PDA authorizes outgoing transfers

From `src/withdraw_collateral.rs`. The same pattern would serve liquidation.

```rust
let vault_seed = verify_position_vault_and_get_seed(&vault, position.account_id, stablecoin_program_id);
...
let mut vault_authorized = vault.clone();
vault_authorized.is_authorized = true;
let transfer_call = ChainedCall::new(
    token_program_id,                      // = vault.account.program_owner
    vec![vault_authorized, user_collateral_holding],
    &token_core::Instruction::Transfer { amount_to_transfer: amount },
).with_pda_seeds(vec![vault_seed]);
```

The runtime accepts `is_authorized = true` on the vault only because `for_public_pda(stablecoin_id, vault_seed) == vault.account_id`. This is checked through `compute_public_authorized_pdas` in `validated_state_diff/mod.rs:133-170`.

### The collateralization check

From `src/checks.rs:assert_position_is_collateralized`. The comparison is exact, in U512, with no division:

```
collateral * ONE^3  >=  normalized_debt * accumulator(now) * redemption_price(now) * min_ratio
```

- Zero debt always passes. The right-hand side saturates.
- On failure it panics with `"Position is undercollateralized"`.
- **There is no non-panicking boolean variant.** `liquidate_position` needs one, for example `is_position_collateralized(...) -> bool` with `assert_` delegating to it.
- Callers project both values to `now` first, using `math::compute_current_accumulated_rate_wide` and `compute_current_redemption_price_wide`. The compounding window is clamped to `MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS = 604_800_000`.
- Callers also assert the projected redemption price is non-zero.

### Debt normalization and accrual

This follows RAI's lazy accumulator. `nominal = normalized * accumulator / ONE`.

- `generate_debt`: `delta = mul_div_ceil(amount, ONE, acc)` (rounded up).
- `repay_debt`: `delta = mul_div(amount, ONE, acc)` (rounded down).
- Position operations only **read** the globals. `accrue_stability_fee` / `refresh_globals` are permissionless pokes.
- No surplus is ever minted. Design invariant 7 (`docs/stablecoin/README.md` §7) is `total_supply ≤ Σ nominal_debt`, and the gap is "fee credit, not materialized; RFP-014 materializes it".

### Freeze

`src/freeze.rs`: the signer must equal `ProtocolParameters.freeze_authority_account_id`, and the instruction sets `is_frozen`.

- **Blocked when frozen:** `open_position`, `generate_debt`, `withdraw_collateral`.
- **Still allowed:** deposit, repay, close, the pokes and the admin setters.
- There is a **single freeze-authority slot**.

### Admin

`checks::assert_admin_authorized` requires `admin.is_authorized && admin.account_id == params.admin_account_id`. The admin is a plain `AccountId`; the RFP-001/002 libraries are not used yet. Because the admin is a plain AccountId, **a PDA of another program can be admin or freeze authority**: that program authorizes it via `pda_seeds` in a chained call.

---

## 2. How the stablecoin reads price and time

### Price

`OraclePriceAccount` is defined in `programs/twap_oracle/core/src/lib.rs:345`:

```rust
pub struct OraclePriceAccount {
    pub base_asset: AccountId,   // must = stablecoin_definition_id
    pub quote_asset: AccountId,  // must = collateral_definition_id
    pub price: u128,             // stablecoin quoted in collateral units
    pub timestamp: u64,          // ms; consumer freshness signal
    pub source_id: AccountId,
    pub confidence_interval: u128, // always 0 today (never written by twap)
}
```

The reader is `src/update_redemption_rate.rs:decode_oracle`:

- It asserts the account is initialized and that `account_id == params.market_price_oracle_id`, then Borsh-decodes it.
- **`program_owner` is deliberately not pinned.**

Staleness is checked in two places:

- `generate_debt`: `now.checked_sub(oracle.timestamp).expect("…dated in the future")`, then `<= maximum_oracle_price_age_milliseconds`.
- `update_redemption_rate`: a saturating version of the same check, plus `price > 0`. `refresh_globals` skips softly instead.

### Time

`src/accrue_stability_fee.rs:123`:

```rust
pub(crate) fn read_clock(clock: &AccountWithMetadata) -> u64 {
    assert_eq!(clock.account_id, CLOCK_01_PROGRAM_ACCOUNT_ID, "Clock account must be the system CLOCK_01 account");
    assert_ne!(clock.account, Account::default(), "Clock account must be initialized");
    ClockAccountData::from_bytes(clock.account.data.as_ref()).timestamp
}
```

- The clock is always the **last** account and is echoed unchanged in post-states.
- From LEZ `lez/programs/clock/core/src/lib.rs`, the system accounts are:
  - `CLOCK_01 = AccountId::new(*b"/LEZ/ClockProgramAccount/0000001")`
  - `CLOCK_10 = …/0000010`
  - `CLOCK_50 = …/0000050`
- Their data is `#[derive(Borsh)] struct ClockAccountData { block_id: u64, timestamp: Timestamp /* u64 ms */ }`.
- The clock program is invoked "exclusively by the sequencer as the last transaction in every block" (`lez/programs/clock/src/main.rs`). Within a block, `now` is therefore the previous block's timestamp.
- CLOCK_10 and CLOCK_50 update every 10 and 50 blocks respectively. The stablecoin uses only CLOCK_01.

---

## 3. The twap_oracle program (`programs/twap_oracle`)

### Accounts

From `core/src/lib.rs`. PDAs live under the twap program id.

| Account | Seed | Contents |
|---|---|---|
| `CurrentTickAccount` | `sha256(price_source_id ‖ b"CURRENT_TICK_ACCOUNT")` | `{ tick: i32, last_updated: u64 }` |
| `PriceObservations` | `sha256(price_source_id ‖ window_duration.le ‖ b"PRICE_OBSERVATIONS")` | `{ price_source_id, write_index: u32, total_entries: u64, last_recorded_tick: i32, entries: Vec<ObservationEntry{timestamp:u64, tick_cumulative:i64}> }`. Ring buffer of `OBSERVATIONS_CAPACITY = 2048`. |
| `OraclePriceAccount` | `sha256(price_source_id ‖ window_duration.le ‖ b"ORACLE_PRICE_ACCOUNT")` | as shown in §2 |

### Instructions

`CreatePriceObservations{initial_tick, window_duration}`, `CreateOraclePriceAccount{base_asset, quote_asset, initial_price, window_duration}`, `CreateCurrentTickAccount{initial_price}`, `UpdateCurrentTick{price}`, `PublishPrice{price_source_id, window_duration}`, `RecordTick{price_source_id, window_duration}`.

### How price is written

1. **`UpdateCurrentTick`** requires `price_source.is_authorized`. The AMM does this from `swap` via a chained call, authorizing the pool PDA with the pool seed (`programs/amm/src/swap.rs:126-146`). The function converts the price to a tick with `price_to_tick` and stamps `last_updated = clock`.
2. **`RecordTick`** is permissionless and sampled no more often than `window/2048`. It clamps the tick delta to `MAX_TICK_DELTA = 9_116`.
3. **`PublishPrice`** is permissionless and writes the TWAP over `[oldest obs, now]`, extrapolating the tail from the current tick. It sets `price = tick_to_oracle_price(twap_tick)`, which is **Q64.64** (`PRICE_FRACTIONAL_BITS = 64`), and **`timestamp = now` (CLOCK_01) on every publish**. With fewer than two observations it is a no-op.

### Staleness and window semantics

- `timestamp` is the *publication* time, not the time of the latest data point. Any account can call `PublishPrice` and refresh `timestamp` even when no new ticks were recorded. The doc says this is intended, since an unchanged price is "fresh".
- **As a stale-feed signal, `OraclePriceAccount.timestamp` is weak.** For R2 ("pause when the feed is stale"), the engine should also read `CurrentTickAccount.last_updated` and/or the newest `PriceObservations` entry timestamp, plus `total_entries ≥ 2`, when the oracle is twap-owned. It should also apply its own max-age.
- **`confidence_interval` is always 0.** Nothing writes it (`create_oracle_price_account.rs:101`), so it cannot be used.
- `CreateOraclePriceAccount` only requires the `price_source` to sign, so **any keypair can create an oracle with arbitrary base/quote labels and drive its price** through `UpdateCurrentTick`. Trust rests on which oracle id the admin binds.

### Testnet

`DEPLOYMENTS.md` lists twap `oracle_price_account` `6jppmhj32NF8HeuJc12duLxARRDq1rk32WJxUFbGGhVw`, using the TKA/TKB pool as source with a 24h window. That is not a stablecoin pair.

---

## 4. The token and ATA programs

### Token (`programs/token`)

`token_core::Instruction` variants (risc0-serde variant order): `Transfer{amount_to_transfer}`, `NewFungibleDefinition{name, total_supply, mint_authority: Option<AccountId>}`, `NewDefinitionWithMetadata{…}`, `InitializeAccount`, `Burn{amount_to_burn}`, `Mint{amount_to_mint}`, `MintWithAuthority{amount_to_mint}`, `PrintNft`, `SetAuthority{new_authority: Option<AccountId>}`, `SetAuthorityWithAuthority{new_authority}`.

Data layouts:

- `TokenDefinition::Fungible{name, total_supply, metadata_id: Option<AccountId>, authority: Option<AccountId>}`
- `TokenHolding::Fungible{definition_id, balance}`

| Instruction | Accounts (guest `methods/guest/src/bin/token.rs`) | Authorization rule (src) |
|---|---|---|
| Transfer | sender M S, recipient M | `sender.is_authorized`. If the recipient is default it is claimed with `Claim::Authorized`, so **a fresh recipient must itself be authorized**; an initialized recipient need not be (`src/transfer.rs`). |
| InitializeAccount | definition, account_to_initialize I S | target authorized (signer or PDA seed) |
| Burn | definition M, holding M S | **only `holding.is_authorized`**; decrements balance and `total_supply`. The mint authority is not checked (`src/burn.rs`). |
| Mint | definition M S, holding M | the definition itself must be authorized **and** equal the stored `authority` (self/PDA authority). `authority = None` means fixed supply (`src/mint.rs`). |
| MintWithAuthority | definition M, holding M, authority S | the external authority signs and must equal the stored `authority` |
| SetAuthority / SetAuthorityWithAuthority | definition M S / definition M, authority S | rotate or revoke |

**Consequences for RFP-014:**

- **Burning raised stablecoin:** the engine holds coin in its own PDA holding and emits `Token::Burn{[stablecoin_definition, engine_coin_holding(authorized via engine seed)]}`. No stablecoin-program hook is needed.
- **Surplus auction burning the protocol token:** same pattern. The winning bid lands in an engine PDA holding and the engine burns it.
- **Debt auction minting the protocol token:** set the token definition's `authority` to an engine PDA, then emit `MintWithAuthority` with the authority account authorized via the engine seed. Alternatively, make the definition itself an engine PDA and use self-authority `Mint`.
- The **stablecoin's** mint authority is the stablecoin definition PDA (self-authority). Only the stablecoin program can mint it, and no instruction exists to rotate it.

**LP-0013** ("Token program improvements: authorities") is CLOSED and is exactly this mint-authority model. **There is no delegate/approve primitive**, meaning no third-party, allowance-style authorized balance decrease. A holding can be debited only when it is authorized: signed by its key, PDA-authorized by the program that derives it, or already authorized up-chain. Native `Account.balance` decreases are allowed only for the owning program (`validate_execution` rule 5).

### ATA (`programs/ata`)

- `Create{token_program_id}`, `Transfer{token_program_id, amount}`, `Burn{token_program_id, amount}`.
- Seed: `sha256(token_program_id(le words) ‖ owner_id ‖ definition_id)` under the ATA program (`core/src/lib.rs`).
- Transfer and Burn need the owner as signer; the ATA program then authorizes its own PDA in the chained Token call.
- An engine PDA can be an ATA owner if the engine authorizes that owner PDA when calling the ATA program. Plain engine-PDA token holdings (via `InitializeAccount`) are simpler.

---

## 5. Chained calls (LP-0015 as delivered)

### API

From `lee_core::program`, re-exported by SPEL:

```rust
ChainedCall::new(program_id, pre_states: Vec<AccountWithMetadata>, &instruction).with_pda_seeds(vec![seed, …])
// return from a SPEL handler:
Ok(spel_framework::SpelOutput::execute(post_states, chained_calls))
```

`instruction` only needs to match the callee enum's **shape**: variant index, then fields. The practice is to depend on the callee's `*_core` crate.

### Execution model

From `lee/state_machine/src/validated_state_diff/mod.rs:104-285` at LEZ v0.2.4:

- **Ordering:** the transaction starts as one `VecDeque` call. Each program's emitted calls are `push_front`ed in reverse, giving **depth-first, in-order** execution: A's call #1 and everything it chains run before A's call #2.
- **Limit:** `MAX_NUMBER_CHAINED_CALLS = 10` (`state/mod.rs:21`). `chain_calls_counter <= 10` is checked before each execution, and the top-level call counts, so at most 11 program executions per transaction.
- **Compute:** `MAX_NUM_CYCLES_PUBLIC_EXECUTION = 32M` cycles **per program execution** (`src/program/mod.rs:15,63`). Account data is capped at `DATA_MAX_LENGTH = 100 KiB`.
- **Return data:** none. The callee runs after the caller's output is written, so the caller cannot see the callee's results.
  - The workaround is a self-call continuation, the "flash swap" pattern in `lee/state_machine/test_methods/guest/src/bin/flash_swap_initiator.rs`: A emits `[token transfer, callback program, A::InvariantCheck]`.
  - `InvariantCheck` gates itself with `assert_eq!(caller_program_id, Some(self_program_id))`. This is the LP-0015 "internal continuation" pattern.
- **Atomicity:** every `?`/`ensure!` returns `Err`, and the whole transaction is rejected. There is no partial commit and no try/catch.
- **Pre-state consistency:** each callee's `pre_states` must equal the current in-tx state (`state_diff` or chain state). If an account is touched twice, the second chained call must carry its post-first-touch value. Examples:
  - `amm/src/swap.rs:412-432` hand-builds `vault_deposit_source` with the updated balance.
  - `stablecoin/src/open_position.rs:145-160` builds `post_init_vault`.
- **Authorization:** marking `is_authorized = true` on a callee pre-state is valid only if the account was authorized by the caller's own inputs (`authorized_accounts`, which grows monotonically down the chain) or is `for_public_pda(caller_id, s)` for some `s` in `pda_seeds`. Over-marking and under-marking are both errors (`InvalidAccountAuthorization` / `AuthorizedAccountMarkedAsNotAuthorized`).
- **Caller identity:** `program_output.caller_program_id` must equal the real caller (`None` at top level). SPEL maps `None` to `DEFAULT_PROGRAM_ID` in `ctx.caller_program_id`.

### Concrete examples in lez-programs

- `amm/src/swap.rs:385-447`: user→vault Transfer; vault→user Transfer with `vault_seed`; fee Transfer with two seeds; plus twap `UpdateCurrentTick`, with the pool PDA authorized via the pool seed.
- `stablecoin/src/{withdraw_collateral,generate_debt,repay_debt,open_position,initialize_program}.rs`.

### Gating "only program X may call me"

1. `assert_eq!(ctx.caller_program_id, X)`. Never accept `DEFAULT_PROGRAM_ID` as X: an unset or zero binding would admit every top-level user.
2. Require a signer account equal to a stored `AccountId` that is X's PDA. This is the twap price-source pattern and the stablecoin admin/freeze `AccountId` model. No lez-program currently checks `caller_program_id`; only the LEE test programs do.

---

## 6. Minimum design for the two hooks

### Engine authorization (recommended)

Use a **PDA-signer handle**:

```
engine_authority_id = AccountId::for_public_pda(engine_program_id, sha256(b"AUCTION_ENGINE__STABLECOIN_AUTH" ‖ stablecoin_program_id))
```

- Store it in the stablecoin and check `authority.is_authorized && authority.account_id == stored`. This matches the existing admin/freeze `AccountId` style.
- It is unforgeable: only the engine can produce that authorization.
- It works with a plain keypair in unit tests.
- It needs no `ctx` plumbing in host functions.
- Optional belt-and-braces: also pass `ctx.caller_program_id` to the host function and compare it to a stored engine program id. This requires rejecting `DEFAULT_PROGRAM_ID`.

**Storage: use a new PDA rather than growing `ProtocolParameters`.** Appending a Borsh field breaks `ProtocolParameters::try_from_slice` for existing data, `modules/stablecoin/ffi` `decode_protocol_parameters`, and every test fixture. The new PDA is `LiquidationEngineState`, seed `sha256(b"STABLECOIN__LIQUIDATION_ENGINE__")`:

```rust
pub struct LiquidationEngineState {
    pub engine_authority_id: AccountId,     // the PDA handle above; set by admin
    pub outstanding_liquidated_debt: u128,  // nominal debt seized from positions, not yet settled
    pub accumulated_bad_debt: u128,         // shortfalls recorded by settle_auction
}
```

### New instructions, appended to `stablecoin_core::Instruction`

1. **`SetLiquidationEngine { engine_authority_id: AccountId }`**
   - Accounts: admin S, protocol_parameters, liquidation_engine_state (I on first call via `Claim::Pda`, then M).
   - Admin-gated via `assert_admin_authorized`.

2. **`LiquidatePosition { expected_collateral_amount: u128, expected_normalized_debt_amount: u128 }`** (full-position seizure, called by the engine through a chained call)

   | # | Account | Flags | Checks |
   |---|---|---|---|
   | 1 | engine_authority | S (engine PDA, authorized via the engine's `pda_seeds`) | `is_authorized`; id == `liquidation_engine_state.engine_authority_id` |
   | 2 | liquidation_engine_state | M | canonical PDA, owned by the stablecoin, initialized |
   | 3 | position | M | initialized; owned by the stablecoin; `position.account_id == compute_position_pda(self, data.owner_account_id, data.position_nonce)` (no owner account is passed); `normalized_debt_amount > 0`; equals the `expected_*` args |
   | 4 | vault | M | `verify_position_vault_and_get_seed`; `== data.vault_account_id`; holding definition == `params.collateral_definition_id`; `vault_balance == data.collateral_amount` (or seize the full vault balance) |
   | 5 | collateral_recipient (engine's per-auction custody holding) | M | **already initialized** (see note); same token program as the vault; definition == collateral; no authorization needed |
   | 6 | stability_fee_accumulator | – | `decode_global` canonical |
   | 7 | redemption_price_state | – | `decode_global` canonical; projected price non-zero |
   | 8 | market_price_oracle | – | `decode_oracle`; `price > 0`; `now - timestamp <= max_age` (the R2 pause; reject future-dated) |
   | 9 | protocol_parameters | – | `decode_global` canonical (deliberately *not* blocked by `is_frozen`, since liquidation reduces risk; maintainers should confirm) |
   | 10 | clock | – | `read_clock` (CLOCK_01) |

   Logic:
   - Assert `!is_position_collateralized(position, acc(now), rp(now), params.minimum_collateralization_ratio)`.
   - Compute `nominal = mul_div_ceil(normalized, acc, ONE)` (round in the protocol's favour).
   - Effects: write `Position{collateral_amount: 0, normalized_debt_amount: 0, ..}`, and `outstanding_liquidated_debt += nominal`.
   - Chained call: `Token::Transfer{amount: vault_balance}` `[vault(is_authorized, seed=vault_seed), collateral_recipient]`.
   - Invariant 1 (vault == collateral) still holds, and the owner can `close_position` afterwards.
   - Partial liquidation (a RAI `liquidationQuantity` cap) could add `collateral_to_seize` / `normalized_debt_to_cover` arguments with bounds checks. RFP F3 only asks for partial *lots* in the auction, which the engine can do on its own.

3. **`SettleAuction { debt_settled: u128, shortfall: u128, freeze_new_debt: bool }`** (optional, but useful for accounting and for F4)

   | # | Account | Flags | Checks |
   |---|---|---|---|
   | 1 | engine_authority | S | as above |
   | 2 | liquidation_engine_state | M | canonical |
   | 3 | protocol_parameters | M | canonical; written only if `freeze_new_debt` |

   Effects: `outstanding_liquidated_debt -= debt_settled + shortfall` (checked); `accumulated_bad_debt += shortfall`; if `freeze_new_debt`, set `is_frozen = true`. That last part delivers F4 without the engine taking the single `freeze_authority` slot, which the alternative would require (admin sets `freeze_authority = engine PDA`, and humans lose freeze).
   - **Proceeds are not routed through the stablecoin.** The engine burns the raised coin itself with `Token::Burn` from its PDA holding, keeps the penalty part as its surplus buffer, and returns leftover collateral to the owner from its own custody.

### Engine-side `liquidate` transaction (one tx, about 4 to 5 executions, within the limit of 10)

Keeper → `auction_engine::liquidate(...)`:

- **Engine post-states:** claim `Auction` PDA (seed `hash(stablecoin_id, position_id, nonce)`); update engine state.
- **Engine chained calls, in order:**
  1. `token::InitializeAccount [collateral_def, auction_custody(PDA, authorized via engine seed)]`
  2. `stablecoin::LiquidatePosition [engine_authority(authorized via engine seed), …, auction_custody(as post-InitializeAccount state)]` → nested `token::Transfer vault→auction_custody`
  3. (optional keeper incentive) `token::Transfer [auction_custody(authorized; pre-state = post-seize balance), keeper_holding]`
- **Pre-computation:** the engine reads the same `Position` and globals and computes `(collateral, normalized_debt, nominal_debt)` with vendored `stablecoin_core::math`. Since there is no return data, the engine writes the auction target into its own post-state and the stablecoin asserts the `expected_*` arguments, so any disagreement reverts the transaction.
- **Why the custody holding must be pre-initialized:** a default recipient in `Token::Transfer` is claimed with `Claim::Authorized` and must be authorized. The stablecoin is the token's caller there and cannot authorize an *engine* PDA.

---

## 7. `modules/stablecoin` (pattern to mirror)

The module is a headless "universal" Logos **core** module: `metadata.json` has `"type":"core","interface":"universal","dependencies":["lez_core"]`, with `external_libraries: [{name: "stablecoin_ffi"}]`.

- **`ffi/` (Rust, `cdylib + rlib`, cbindgen)** owns PDA derivation, exact decoding, validation and instruction serialization.
  - Exports `stablecoin_program_info`, `stablecoin_decode_protocol_parameters`, `stablecoin_decode_stability_fee_accumulator` and `stablecoin_initialize_program_plan`. Each is `char* fn(const char* request_json)`, with `stablecoin_free(char*)` to release.
  - Every response is an envelope `{"ok":bool,"value"?:{…},"error"?:"stable_code"}`, wrapped in `catch_unwind` (panics become `backend_error`); see `ffi/src/ffi.rs`.
  - Requests are camelCase serde structs (`ffi/src/api/request.rs`). Accounts arrive as `AccountRead{id,status,account:{program_owner,balance,nonce,data}}` (`ffi/src/account.rs`).
  - A "plan" returns `{programId, accountIds[], signingRequirements[], instruction (risc0 words)}` (`ffi/src/api/plan.rs`).
- **`src/stablecoin_module_impl.{h,cpp}` (Qt-free C++)** is the `class StablecoinModuleImpl : public LogosModuleContext`.
  - Methods return `LogosMap` envelopes `{status:"ok"|"error", error}`.
  - It reads accounts with `modules().lez_core.get_account_public(id)` and submits with `modules().lez_core.send_generic_public_transaction(account_ids, signing_requirements, instruction_bytes, program_id, &err)`.
  - It hard-checks the plan shape: for example, `initializeProgram` requires 9 accounts with signers `{true,false×8}`.
  - The program id comes from env `STABLECOIN_PROGRAM_ID` / `STABLECOIN_PROGRAM_BIN`.
  - `src/stablecoin_module_support.{h,cpp}` holds pure helpers: id normalization, `jsonInstructionLeBytes`, `stableFfiError`, `transactionId`.
- **Tests (`tests/`)** use the `LogosTest` CMake target:
  - `MOCK_C_SOURCES mocks/mock_stablecoin_ffi.cpp` re-implements every FFI symbol via `LOGOS_CMOCK_RECORD` / `LOGOS_CMOCK_RETURN_STRING`.
  - Tests do `context.mockCFunction("stablecoin_program_info").returns(json)` and `context.mockModule("lez_core","get_account_public").returns(...)`, then assert `cFunctionCallCount` / `moduleCalledWith`.
  - `tests/logos_sdk.h` adapts the Qt `LezCore` client to the universal std API.
- **Build:** `flake.nix` uses `logos-module-builder.lib.mkLogosModule`, with input `lez_core = github:logos-blockchain/logos-execution-zone-module?ref=byte-string-fix`. Commands:
  - `nix build .#stablecoin_ffi`
  - `.#stablecoin-module`
  - `.#stablecoin-module-tests`
- Open PRs #333/#334/#338/#339/#340/#399/#400 are extending this module (position, pokes, open/deposit APIs), so the shape is still moving.

---

## 8. Deployment status and running our own instance

**Status:**

- Not deployed. Testnet (`https://testnet.lez.logos.co/`, v0.2.4) has token `2Pp7aXgG…`, amm, twap_oracle `6pgw6c78…` and token_mint_authority (faucet PDA `GaEqKnux…`).
- Open PRs:
  - #397 `scripts/deploy-stablecoin.sh`
  - #401 `apps/stablecoin/tests/testnet/setup-stablecoin-testnet.sh` + `scripts/deploy_stablecoin.py`, network-agnostic, with a resumable manifest

**Bootstrap steps:**

1. **Build and deploy:** `wallet deploy-program programs/stablecoin/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/stablecoin.bin` (`docs/testnet-runbook.md`). The ProgramId is the ImageID, so every PDA depends on the exact binary.
2. **Collateral:** any `TokenDefinition::Fungible`. On testnet the cleanest is a faucet-mintable token (`mint_authority` = the faucet PDA), or the existing Token A def `9zHnyUgG…`. The definition is bound immutably.
3. **Oracle:** an `OraclePriceAccount` with `base_asset = compute_stablecoin_definition_pda(program_id)` (predicted *before* init) and `quote_asset = collateral_def`, price > 0, at **1e27 scale**.
   - Either use twap `CreateOraclePriceAccount` signed by a wallet "price source" (window ≥ 2048 ms), as #397 does, or write our own tiny admin-settable oracle program emitting the same struct. The stablecoin does not pin the owner.
   - **#397's static oracle goes stale after `maximum_oracle_price_age_milliseconds` (max 1 day).** After that, `generate_debt` and our liquidation gate fail. For a long-lived testnet we need a refreshable 1e27 oracle. Do *not* refresh via twap `PublishPrice`, which writes Q64.64.
4. **`initialize_program`:** 9 accounts, only the admin signs. Example parameters from #397/#401:
   - fee `1e27` (0%) or slightly above
   - gains `0` (#401 rejects nonzero until the scale fix)
   - ratio `1.5e27`
   - `min_ms_between_rate_updates` e.g. `60_000`
   - `max_oracle_age` ≤ `86_400_000`
   - `initial_redemption_price` e.g. `1e27`
   - a name
5. **Making positions liquidatable on testnet:** the admin raises `set_minimum_collateralization_ratio`, which leaves existing positions retroactively undercollateralized. The other routes are fee accrual over time and redemption drift once gains are nonzero.

---

## 9. Open risks

1. **Price scale (Q64.64 vs 1e27).** The design is blocked upstream for live twap feeds. The engine must use the same scale as the stablecoin for auction pricing (collateral per coin = `1/oracle.price`), and must handle both or pin one.
2. **Weak twap freshness.** `PublishPrice` re-stamps `timestamp = now` without new data, and `confidence_interval` is always 0. R2 needs extra checks (`CurrentTickAccount.last_updated`, newest observation, `total_entries`), plus manipulation limits (`MAX_TICK_DELTA` only bounds per-sample moves).
3. **Liquidation trigger semantics.** The stablecoin values collateral at the *redemption* price (internal, drifting at the last controller rate when the oracle is stale), not at a market collateral price. The "TWAP / confidence-adjusted price" of R2 applies to auction pricing and to the staleness pause. Reviewers may expect a market-price trigger.
4. **No return data from chained calls.** The engine must pre-compute and the stablecoin must assert. Multi-touch accounts need hand-built pre-states, which is error-prone; mirror the amm/open_position patterns and cover them with integration tests.
5. **Execution limits.** At most 10 chained calls (11 executions), 32M cycles per execution, and 100 KiB per account. Keep auction state one PDA per auction (also needed for R3 isolation). Lot-buying plus keeper incentive plus burn in one bid tx must stay under 10 calls.
6. **`caller_program_id` pitfall.** A top-level call shows up as `DEFAULT_PROGRAM_ID` in SPEL; an unset engine binding of all zeros would let anyone liquidate. Reject it explicitly, or prefer the PDA-signer handle.
7. **Upstream ABI churn.** `Instruction` uses variant-index encoding, so new variants must be appended. Changing `ProtocolParameters` breaks the FFI decoder and fixtures, so a separate PDA is safer. Many stablecoin PRs are open (#333–#401), so pin `64c0120` and expect rebases.
8. **Single freeze-authority slot (F4).** Making the engine the freeze authority removes human emergency control; the `SettleAuction{freeze_new_debt}` hook avoids that but needs maintainer buy-in. RFP-001/002 libraries are not integrated (admin and freeze are plain `AccountId`s).
9. **Supply/debt accounting.** Seized nominal debt includes never-minted fees (supply ≤ Σ principal). Burning `D` coins from auctions is correct in aggregate, but surplus/shortfall bookkeeping (`outstanding_liquidated_debt`) must be kept in one place, either the engine or the hook. Using both creates a dual source of truth.
10. **Owner refunds.** `Position.owner_account_id` may be a private or group-owned account, not a token holding. Return leftover collateral through a **pull-claim** from the engine (the owner proves ownership), not a push transfer.
11. **Custody bootstrapping.** A fresh recipient holding must be authorized to be claimed. The engine must `InitializeAccount` its custody PDA before the stablecoin's nested Transfer, in the same transaction.
12. **Clock granularity.** `now` is the previous block's CLOCK_01 timestamp (updated as the last tx of each block), and blocks are about 15 s apart on a local node. Discount schedules and auction expiries move in block-sized steps.
13. **Single collateral per stablecoin instance.** R4/R5 ("pause per collateral type") map to per-instance config in the engine, keyed by stablecoin program id / collateral def.
14. **Oracle trust.** The stablecoin does not pin the oracle's `program_owner`, and anyone can create an `OraclePriceAccount` with arbitrary base/quote labels. The engine should pin its feed per instance (admin-set), and optionally check `program_owner`.
15. **Deployment not yet upstream.** We would deploy our own stablecoin instance and our own oracle; testnet ProgramIds are binary-specific.
