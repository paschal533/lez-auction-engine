# Design note: the auction engine on the LEZ 0.4 actor model

## 1. Status and scope

Written 2026-10-31 from code reading only. Nothing was built. Baseline: this repo on LEZ v0.2.4 (ADR-001).
Target: LEZ **v0.4.0-rc1** (`1806e873`). Every LEZ claim below cites a file at that tag; paths are relative
to `logos-blockchain/logos-execution-zone`. Claims I could not check in source are marked **[unverified]**.
Background and upstream status: `docs/research/2026-10-31-lez-0.4-port-research.md` (the "research").

Scope: how each engine instruction becomes an actor conversation, where each piece of state lives, how
authorization and the RFP invariants survive, and what to build first. Out of scope: the private path
(PRIV/PA rows stay M4), the Basecamp app, and the FFI ABI (ADR-055 survives; only `lez_core` moves).

Blocked upstream (research §1, §3-§5):

| Item | State on 2026-10-31 | Blocks |
|---|---|---|
| SPEL for 0.4 | none; the only port (fork `0x-r4bbit/spel`) targets v0.3 plan/apply | U1 (SPEL-built, IDL generated) |
| Public testnet 0.4 | none; `testnet.lez.logos.co` runs 0.3.x; reported for late November [3P] | S1 |
| lez-programs 0.4 (stablecoin, token, oracle) | no branch; `main` is on v0.2.5-rc2 | real host hooks, real token |
| `lez_core` 0.6.0 | open PR #72 (`9e3d774c`), changes `send_generic_public_transaction` again | FFI, module, CLI signing path |
| PDA authorization | open issue #974 ("consider PDA authorization scheme") | may change §2 points 5-6 |

Not blocked: the engine guest itself, a local token and oracle, a mock CDP host, the SDK transaction
builder, and an in-process test harness on `lee` v0.4.0-rc1 (§8, first milestone).

## 2. The 0.4 model, as far as this design relies on it

Abbreviations: `PM` = `lee/state_machine/core/src/program/mod.rs`, `ACC` = `lee/state_machine/core/src/account.rs`,
`EX` = `lee/state_machine/core/src/execution_state.rs`, `NT` = `lee/state_machine/core/src/native_token.rs`.

1. **One handler, one actor state.** An `Actor` is `(account_id, program_account_id)` (ACC:194-212). An
   account holds a map `program -> ActorState` (ACC:131-133); there is no owner field and no balance field.
   A guest is `actor_entrypoint!(handler)` with `fn(&ReceiveInput, M) -> Response` (PM:13-25, PM:734-743). The
   handler sees only `pre_state` of its own actor (PM:371-385). Any other state must arrive in a message.
2. **Who is calling.** `ReceiveInput.from` is the sending actor, or `None` for the transaction root
   (PM:376-378). `from_own_program()` compares only the program half (PM:389-393), so it does not pin *which*
   actor of the program sent the message.
3. **Effects are messages.** `Response::set_state` / `keep_state`, then `send_sync`, `send_async`, `event`,
   `block_window`, `timestamp_window` (PM:424-500). A panic fails the transaction (PM:734).
4. **Sync messages: depth-first, after the handler, atomic, no return value.** The handler's post-state is
   staged before its messages run (EX:1334); sent messages are pushed in reverse onto a LIFO work stack
   (EX:1402, EX:1151), so the first message and its whole subtree run before the second. A message cannot
   return a value. A reply is a new message back to `from`; `StateReply { state }` is the convention for
   reads (PM:396-407). The native token answers `ReadState` with `send_sync(reader, &StateReply)`
   (NT:80-85).
5. **Authorization.** `is_authorized` = the account signed the transaction (it is in the public context's
   authorized accounts, EX:1028) **or** a PDA seed in the incoming message derives that account from the
   *sending* program's address **or** an inherited grant (EX:1431-1469). Grants are passed down to every
   message the receiver sends (EX:1365). A signature authorizes the account for *every* program that
   messages it in that transaction.
6. **PDAs.** `AccountId::for_public_pda(&program_account_id, &seed)` with the **deployed program address**
   (PM:149-167; example `examples/program_deployment/methods/guest/src/bin/tail_call_with_pda.rs:8-29`).
   `SyncMessage::with_pda_seeds` grants the PDA to the receiving program (PM:317-321). Issue #974 is open.
7. **Async messages.** Delivered in the same transaction only if the destination is a declared public
   actor; otherwise queued for a later transaction (EX:1378-1398). They run on whatever program is deployed
   at the destination then (PM:326-327).
8. **Native token.** Program `[0; 32]` (NT:9). `Transfer` debits (needs `is_authorized`) and credits the
   recipient by an **async** `Credit` (NT:68-70); `Credit` is accepted only from the native program (NT:71-74).
9. **Time.** The clock program is ticked as the last transaction of each block
   (`lez/programs/clock/src/main.rs:7`). It does not reply with its state; it answers
   `AssertTimestamp { at_least, at_most }` by asserting its stored timestamp is in range (`main.rs:67-74`).
10. **The "read, act, read" pattern.** `lee/state_machine/test_methods/guest/src/bin/flash_swap_initiator.rs`
    keeps a stack of pending phases in its own actor state (lines 81-95), sends `ReadState`, and checks in the
    reply that `from` is the expected actor before trusting the state (lines 97-105). Its last message is a
    second read that asserts the end state (lines 123-134). This note reuses that pattern throughout.
11. **Transactions.** A public message is `RootCall { to: Actor, message }`, the declared actors, the
    signers' nonces and an optional `FeeDeclaration { payer, gas_limit, tip, max_fee }`
    (`lee/state_machine/src/public_transaction/message.rs:18-46`, `lee/state_machine/src/fees.rs:13-30`).
12. **Fees.** The reserve is `gas_limit * base_fee_exec + ...` (`lez/programs/fee/core/src/assess.rs:61-70`);
    the actual fee is the executed cycles clamped to `gas_limit` (`assess.rs:84-86`). That a **reverted**
    transaction is charged its whole `gas_limit` was reported by a third party (vpavlin's LEZ-V04-NOTES) and is
    now **confirmed on chain** by spike S8 (`docs/spikes/S8.md`): a revert at gas 200k and 5M was charged exactly
    `gas_limit × 8` plus storage; a successful call was charged its metered 7,555 cycles.
13. **Events.** `ProgramEvent { selector: [u8; 8], data }`, selector by convention the first 8 bytes of
    `sha256("<program>::<EventName>")` (PM:701-710). That the sequencer stores events only for final blocks
    is **[unverified]** (research, [3P]).

## 3. Account and actor layout

Notation: `E` = the engine's deployed program address, `H` = host stablecoin, `T` = token program,
`O` = oracle program. `pda(X, seeds)` = `AccountId::for_public_pda(&X, &PdaSeed::new(combined))` with
today's seed encoding (ADR-020, `auction_engine_core::pda`), only with `E` in place of the ImageID.

| Today (v0.2.4) | 0.4 actor | Who writes it | Change |
|---|---|---|---|
| `EngineConfig` `["CONFIG", admin, salt]` | `(pda(E, ..), E)` | engine | program ids become program *addresses* (`AccountId`); optional slots become `Option<Actor>` |
| `CollateralConfig` `["COLL", config, def]` | `(pda(E, ..), E)` | engine | none |
| `Accounting` `["ACCT", config]` | `(pda(E, ..), E)` | engine | none |
| `LiquidationRecord` `["LIQREC", config, position]` | `(pda(E, ..), E)` | engine | none |
| `Auction` `["AUCTION", config, position, nonce]` | `(pda(E, ..), E)` | engine | gains a `phase` field (§4.1) |
| `SurplusAuction`, `DebtAuction` | `(pda(E, ..), E)` | engine | gain a `phase` field |
| `SurplusBuffer` `["BUFFER", config]` | `(pda(E, ..), T)` | token, on engine seed | the PDA account now holds a *token* actor state |
| `AuctionVault` `["VAULT", auction]`, `SurplusVault` | `(pda(E, ..), T)` | token, on engine seed | same; no `InitializeAccount` call needed (§6) |
| `EngineAuthority` `["ENGINE_AUTH", config]` | retired | — | the host checks `from == (config, E)` instead (§5) |
| `MintAuthority` `["MINT_AUTH", config]` | retired | — | the protocol-token definition names `(config, E)` as its minting actor (§6) |
| `UnusedSlot(k)` (ADR-030) | retired | — | no account lists exist; `None` in the config is enough |
| Oracle account | `(oracle, O)` | oracle | read by `ReadState` |
| Host `ProtocolParameters`, position, accumulator, redemption-price state, `LiquidationEngineState` | `(.., H)` each | host | read by the host itself, not the engine (§5); redemption-price state read by `ReadState` for bids |
| Token holdings, definitions | `(holding, T)`, `(definition, T)` | token | one holding per account, as today (§6) |
| `CLOCK_01` | `(CLOCK_01, clock)` | system | asserted, not read (§4.1 time) |

Consequences:

- **No account lists.** A message carries arguments only. Every actor the engine messages is derived from
  its own stored config or re-derived as a PDA from fields it holds. The caller cannot substitute an account,
  so ADR-019 invariant (iii) and the ADR-030 slot pins hold by construction. The SDK still must *declare*
  every touched actor in the public context (§2 point 11), or the transaction fails or an async credit is
  deferred (§2 point 7).
- **Compare whole actors, never bare account ids.** Any program may hold an actor state at any account. A
  check like `from.account_id == config` accepts `(config, AttackerProgram)`. Every check in this note
  compares a full `Actor`. This replaces the "owner = engine" half of `accounts::load_*`.
- **"Uninitialised" = empty actor state.** `expect_default` becomes `pre_state.is_empty()`; creation is the
  first `set_state` (helper `write_once`, PM:754-761). Empty data deletes an actor state (ACC:142-148), so
  every engine state must encode to non-empty bytes; the 8-byte `TAG_*` prefix already guarantees that.

**Per-instance model (F7, ADR-026/027).** `init_config` stays permissionless; the signer becomes the
admin and the admin's id stays in the config seed, so a squatter cannot take the admin's address. A config
actor cannot see the admin's signature (it is a PDA; §2 point 5), so identity uses a **signer proxy**: the
admin sends the root message to its own engine actor `(admin, E)`, where `is_authorized` is true only if
the admin signed; that stateless actor forwards to `(pda(E, ["CONFIG", admin, salt]), E)`, which checks
`from == Some((admin, E))` and re-derives its own address from `from.account_id` and `salt`. This is the
`forward_as_authority` shape of `lez/programs/wrapped_token/src/main.rs:193-197`. The same proxy carries
every admin setter and `claim_leftover` (owner). All engine and collateral parameters are still set in the
init message and validated by `args::*::validate`, unchanged.

**I4 on 0.4.** The config stores program addresses. A deployed program can be updated in place unless its
`ProgramHeader.immutable` is set (PM:510-524). The client check therefore becomes: for each stored address,
read the `ProgramHeader` at the loader actor, compare `image_id` with the reproducible ImageID in
`DEPLOYMENTS.md` (ADR-047), and require `immutable == true` (or accept a named upgrade authority as a new,
documented trust assumption).

## 4. Instruction-by-instruction mapping

### 4.1 Five building blocks

Every instruction below is built from these. Names are ours, not LEZ's.

| Block | What it does | Based on |
|---|---|---|
| **Proxy** | root message to `(signer, E)`; forwards only if `is_authorized`; the receiver checks `from == (signer, E)` | §3; wrapped_token `forward_as_authority` |
| **Relay** | actor A writes its own state, then sends a context message to actor B; B checks `from == A`, with A's address re-derived from B's own stored fields or from the message's fields | hello_world_with_move_function.rs:42-47 |
| **Read** | A sends `ReadState` to a foreign actor X; X replies `StateReply` to `from`; A's reply handler checks `from == X` and stores the reading in a scratch field of its own state | NT:80-85; flash_swap_initiator.rs:97-105 |
| **Sentinel** | the *last* message of a response is A → A `Proceed` / `Finish`. Depth-first order runs it after every earlier subtree (§2 point 4). It consumes the scratch readings, asserts the conversation reached its terminal phase, and clears the scratch. A missing reply makes it panic, so a half-done conversation can never commit | flash_swap_initiator.rs:123-142 |
| **Assert** | A acts, then sends B a message that only checks B's state and panics if a precondition fails. Cheapest cross-actor check: no reply, no scratch | clock `AssertTimestamp` (`main.rs:67-74`) |

**Time.** The caller puts `now` in the root message. The engine sends
`AssertTimestamp { at_least: now, at_most: now + slack }` to `(CLOCK_01, clock)`, so the claim can only be
*earlier* than the clock. `slack` is a new config parameter (proposal: one block interval). Conservative
use: discounts, `t_eff` and delay checks (`DelayNotElapsed`, relist, TTL) use `now`, which favours the
protocol; feed-age checks use `now + slack`, so an understated `now` cannot make a stale feed look fresh.
Today `CLOCK_01` also holds the previous block's timestamp, so the semantics are close to ADR-024's.

**Observe** (shared by `liquidate`, `bid`, `observe_feed`, `set_collateral_config`) runs at the collateral
actor `C = (pda(E, ["COLL", config, def]), E)`, which owns the pause clock: Read `(redemption_state, H)`,
Read `(oracle, O)`, Assert clock, then Sentinel `Proceed`, which runs `feed::observe` and
`PauseClock::observed` unchanged and writes the pause clock.

**Errors.** Handlers panic with today's text `Program error [6000+code]: Program error {code}: <Name>`, so
the SDK's error mapping survives. How 0.4 receipts expose panic text is **[unverified]**.

### 4.2 The mapping

`K` = config actor, `C` = collateral actor, `A` = auction, `Acc` = Accounting, `S`/`D` = surplus/debt
auction, `V` = vault holding `(pda, T)`, `B` = buffer holding, `→` = sync message, `R(x)` = Read of x.
"Signs" = the account is a transaction signer.

| # | Instruction | Root actor | Message sequence | State written | Authorization |
|---|---|---|---|---|---|
| 0 | `init_config` | proxy `(admin, E)` | → K `Init{salt, params, actors}`; K: R(`(protocol_parameters, H)`) to check the stablecoin definition and oracle; R(protocol definition) for ADR-038; Proceed; → Acc `Init` | K, Acc (created) | K: `from == (admin, E)`, own address == `pda(E, CONFIG, admin, salt)`; Acc: `from == K` |
| 1 | `set_engine_params` | proxy | → K; K writes; → Acc *Assert* "no live system auction" if the change touches the protocol token or system params (`ProtocolTokenLocked`); → C *Assert* caps if needed | K | K: `from == (cfg.admin, E)` |
| 2 | `set_collateral_config` | proxy | → K → C; C runs Observe with the new admin flag, writes | C | C: `from == K` |
| 3 | `observe_feed` | C | Observe only; an invalid reading is recorded, never a panic (ADR-032) | C (pause clock) | none (permissionless) |
| 4 | `liquidate` | proxy `(keeper, E)` | → C (Observe, `require_live`) → K `LiqCtx`. K sends, in order: record `Bump{nonce}`; A `Open` (phase `AwaitingSeize`); host `LiquidatePosition{custody: V, oracle, reply_to: A}` [host checks, moves collateral to V, → A `Seized{..}`; A quotes, pays the keeper reward from V, sends `Reserve`]; A `Finish` (Sentinel) | record, A, C/K reservations (sent by A as `Reserve` after the quote), V | host: `from == (K, E)` (§5); A on `Seized`: `from ==` the bound host actor stored in `Open`; C/K on `Reserve`: `from ==` A re-derived from `(config, position, nonce)` |
| 5-6 | `bid` / `bid_with_settlement` → one `Bid` | C | Observe → A `BidCtx{price, r, now, paused_total}`; A quotes, writes; → `(bidder_stable, T)` Burn(debt part) → Transfer(penalty part → B) → V `Transfer` or `Sweep{at_least}` → bidder collateral (VAULT seed); at chain end → Acc `Settled`, → C `Release`, → K `Release` | A, C, Acc, K at chain end; holdings | bidder holding: signs; V: seed `["VAULT", A]`; A: `from == C`; Acc/C/K: `from == A` re-derived |
| 7 | `settle_empty` | A | A asserts `Auctioning` and `collateral_left == 0`; → Acc `Settled`, → C, K `Release` | A, Acc, C, K | permissionless (today's "caller signs" requirement disappears; the fee payer is the caller) |
| 8 | `claim_leftover` | proxy `(owner, E)` | → A; → V `Sweep{to, at_least: claimable - claimed}` (VAULT seed) | A | A: `from == (A.owner, E)` |
| 9 | `settle_debt` | Acc | R(B); Proceed: burn `min(amount, balance, unbacked - on_auction)`; → B `Burn` (BUFFER seed) | Acc, B | permissionless |
| 10 | `start_surplus_auction` | K | K: R(ref oracle) if enabled, Assert clock; Proceed → Acc `SurplusCtx`; Acc: R(B), Proceed: implicit settle + `start_surplus`; → B Burn, → B Transfer(lot → surplus vault), → S `Open` | Acc, S, B, surplus vault | Acc: `from == K`; S: `from == Acc` |
| 11 | `take_surplus` | S | Assert clock; S quotes from its snapshot; → taker protocol holding Burn (signs); → surplus vault Transfer(filled → taker) (SURPVAULT seed); → Acc `SurplusTaken` | S, Acc | taker holding signs; Acc: `from == S` re-derived from `(config, nonce)` |
| 12 | `close_surplus_auction` | S | Assert clock (TTL); R(surplus vault); Proceed: → vault `Sweep` to B; → Acc `SurplusReturned{amount}` | S, Acc | permissionless |
| 13 | `start_debt_auction` | K | K: R(protocol definition) (mint authority must be K), R(ref oracle), Assert clock; Proceed → Acc; Acc: R(B), Proceed: relist delay, implicit settle, `start_debt`, `record_debt_start`; → B Burn; → D `Open` | Acc, D | as row 10 |
| 14 | `take_debt` | D | Assert clock; D quotes; → taker stable holding Burn(filled) (signs); → Acc `DebtTaken{filled}`; → K `Mint{to, minted}`; K → `(protocol_def, T)` `MintTo` | D, Acc, holdings | K: `from == D` re-derived; token: `from == def.minter == (K, E)` |
| 15 | `close_debt_auction` | D | Assert clock (TTL); → Acc `DebtClosed{lot_left}` | D, Acc | permissionless |
| 16 | `check_solvency` | K | K: R(protocol definition), R(ref oracle); Proceed → Acc; Acc: R(B), Proceed: implicit settle + decision; `Freeze` → K `Freeze` → host `EngineFreezeNewDebt`; `RecoveryInProgress` panics | Acc, host flag | host: `from == (K, E)` |

Notes on the table:

- **One bid message.** ADR-029 split `bid` from `bid_with_settlement` only because `Accounting` had to be in
  the account list. On 0.4 the engine names `Acc` itself; the SDK simply always declares it. The
  `ChainEndAccountsMissing` revert disappears. Keep variant 6 as an alias so encoder indices stay stable.
- **Multi-account atomic writes.** Today one handler returns every post-state. On 0.4 each actor writes its
  own state in its own handler; the whole conversation is one transaction and any panic reverts all of it
  (§2 point 3). The Relay checks guarantee each write was caused by a genuine engine step, and the Sentinel
  guarantees the conversation finished. Atomicity therefore holds; it is enforced by the transaction, not
  by one handler.
- **Executions grow.** `liquidate` goes from 5 executions (ADR-036) to roughly 15-20; `bid` from 3-4 to
  about 10-12. P1/P3 ("in one block") still hold because it is one transaction, but gas and fees must be
  re-measured (CYCLES.md, ADR-048) before the design is frozen.

### 4.3 Three hard questions

**(a) No return values.** Today's chained calls do not return values either; the engine *precomputes* from
account states it was given (`token_calls::with_balance`). On 0.4 the problem is that those states are no
longer given. Each former read takes one of three forms:

| Value | Today | 0.4 choice | Why |
|---|---|---|---|
| Seized collateral, debt, host vault balance (`liquidate`) | read position and host vault, then hook with full amounts | **host report**: the host moves the collateral and replies `Seized{collateral, normalized_debt, nominal_debt, owner}` to A; A books exactly that | R6 (seized == in auction) by construction: A books what was moved, in the same transaction. Alternative: A reads the position via `ReadState`, sends exact amounts, and the host asserts they equal the full position. Both keep R6; the report needs fewer executions and keeps the host the only reader of its own state |
| Vault balance before a sweep (`bid` final fill, `claim_leftover`) | read vault, check `>=` book | **token `Sweep{to, at_least}`**: move the whole balance, panic if below `at_least` | ADR-033 and `VaultBelowBook` move into the token message; no reply |
| Buffer balance (`settle_debt`, implicit settles) | read buffer | **Read**, then burn an exact amount | the burned amount must be booked in `Acc`, so it must be known first |
| Surplus vault balance (`close_surplus_auction`) | read vault | **Read** | same reason (`total_surplus_returned`) |
| Oracle, redemption price, protocol definition | read accounts | **Read** | foreign state |
| Engine state on another engine actor | read accounts | **Relay** or **Assert** | e.g. `ProtocolTokenLocked` as an Assert to `Acc` |

Optional on-chain R6 check: `A.Finish` reads V and asserts `balance == collateral_seized - keeper_reward`.
Two extra executions; recommended for the PoC because R6 is a headline invariant, removable later.

F6 (forward-only): auction and system-auction phases are `AwaitingSeize -> Auctioning -> Settled` (and
`Live -> Ended`). Each handler asserts the phase it expects and only moves forward; the Sentinel asserts the
terminal phase of its conversation. A pending phase therefore never survives a transaction.

**(b) A revert is charged its whole gas limit** (confirmed by spike S8, §2 point 12). So:

- Racing keepers and bidders pay for losing. The second `liquidate` of a position, or a bid after the final
  fill (`NotAuctioning`), costs `gas_limit * base_fee_exec` instead of a few executions.
- The SDK must simulate locally (`lee::simulate_public_transaction`, research §2) right before sending,
  and set `gas_limit` from the simulated cycles plus a margin (proposal: 20 %), never a flat limit. (SPEL's
  v0.3 fork hard-coded 10 M gas and was rejected with `PayerCannotFund`, research §1.)
- Pre-checks the SDK already does (feed validity, `remaining_target`, nonce) become money-saving, not just UX.
  `observe_feed` should stay non-reverting.
- Option to evaluate after measuring: turn pure race losses into successful no-ops that emit an event
  (cheap), keeping reverts for safety failures. This changes error semantics, so it needs an ADR.
- Open: whether a failed validity window rejects the transaction (no fee) or reverts it (fee) — §9.

**(c) R1, non-reentrancy.** Facts: a handler cannot be re-entered mid-execution, because it returns before
any of its messages run (EX:1334, EX:1402). A *later* message in the same transaction can reach an engine
actor again and sees the already-updated state. LEZ 0.4 has no reentrancy guard, as on v0.2.4
(THREAT_MODEL). Guards in this design:

1. During a conversation, control passes only to actors the engine names from its config: H, T, O, clock,
   native token. None of them forwards to caller-chosen code (a requirement on the host and token ports, §5,
   §6). So no attacker code runs mid-conversation.
2. An actor in a pending phase accepts only the expected reply from the expected actor; any other message,
   including a root message, panics.
3. Every internal message checks the exact `from` actor (§3, "compare whole actors").
4. The Sentinel rejects a conversation that did not reach its terminal phase.
5. Grants are inherited down the call tree (EX:1365). The engine attaches a seed only on messages to `T` for
   its own holdings, and the token port must send nothing onward except its own `Credit` and `StateReply`.
   Retiring `ENGINE_AUTH` (§5) removes today's inherited-grant surface on the host.

Tests: port `security.rs` to actor form (impostor program sending `Seized`, `Reserve`, `Release`, `Mint`
from `(K, Impostor)` and from `(attacker, E)`; a reply arriving outside a pending phase; a conversation
whose host never replies).

## 5. Host (stablecoin) hooks on 0.4

**Variant C becomes one comparison.** Today a hook requires `caller_program_id == engine_program_id` *and*
an authorised `["ENGINE_AUTH", config]` (ADR-019). On 0.4, `from` is the whole sending actor (§2 point 2).
The binding stores one `Actor`, and every engine-only hook checks
`input.from == Some(LiquidationEngineState.engine_actor)`, where `engine_actor = (config_pda, E)`. That
pins both the program and the configuration (ADR-027) with no PDA grant. Consequences:

- `ENGINE_AUTH` retires, and so does the inherited-grant risk noted in THREAT_MODEL ("an account the engine
  authorises for the host stays authorised for every program the host calls").
- The "top-level only" rule for `set_liquidation_engine` becomes `from == None` plus the host admin's
  signature on the actor that receives it. Whether the host keeps admin checks as plain `is_authorized` is
  upstream's choice; the laundering note in THREAT_MODEL still applies to a PDA admin.
- The binding must be at a host PDA derived from the host's *deployed address*, as all host PDAs now are.

**The hook messages** (the interface we need from the host port; F7):

| Message | Sent by | Host does | Host sends |
|---|---|---|---|
| `SetLiquidationEngine { engine_actor }` | host admin, root | stores the binding | nothing |
| `LiquidatePosition { position, custody: Actor, oracle: Actor, reply_to: Actor }` | `(K, E)` | Variant C; checks the oracle equals its market oracle (ADR-028 moves here); runs today's checks (unsafe at the minimum ratio, fresh oracle); seizes the **whole** position (ADR-012) | token Transfer of the whole vault balance to `custody` (vault PDA seed), then `Seized { position, owner, collateral, normalized_debt, nominal_debt }` → `reply_to` |
| `EngineFreezeNewDebt` | `(K, E)` | Variant C; sets `is_frozen` (idempotent) | nothing |

The engine's `A` accepts `Seized` only from the host actor it recorded in `Open`, and only in phase
`AwaitingSeize`. The proportional-seize bound (ADR-026 amendment) is kept for partial seizure (M1); with full
seizure the host simply zeroes the position. The host needs its own reads (accumulator, redemption price,
parameters, oracle) to judge health: that is a host-internal conversation and upstream's design. Order in
the host's response: the Transfer first, then `Seized`, so the collateral is in `V` before `A` books it
(§2 point 4).

**What we need from upstream.** (1) A 0.4 port of the stablecoin with a stable instruction and actor layout.
(2) Agreement on the three messages above, including the `Seized` report and `reply_to`. (3) A `ReadState`
answer on the redemption-price and protocol-parameters actors (the engine reads them in Observe and at
init). (4) A host that never forwards to caller-chosen code from these hooks (§4.3 (c) point 1). Note the
v0.2.5 changes already on lez-programs `main` (new enum variant 21, `generate_debt` without oracle,
SC-NORM #445; research §4): our seams move anyway.

**If upstream is late.**

| Option | Effort | For | Against |
|---|---|---|---|
| A. Port the vendored host ourselves | 3-4 weeks (about 12k lines incl. tests, research §6) | real host in tests and on testnet 0.4 | duplicate of upstream's work; diverges; hard to upstream |
| B. **Minimal mock CDP host** implementing only the F7 interface | 3-5 days | unblocks the engine, SDK and e2e now; small, test-only, never deployed as "the stablecoin" | does not show the real host; S1 evidence is weaker |
| C. Wait | 0 | no waste | engine work stalls |

Recommendation: **B now**, then rebase onto upstream's port (or A if upstream has not moved by the time
testnet 0.4 is up). The mock holds positions (`collateral`, `normalized_debt`, owner, a vault holding),
an accumulator constant, a redemption price, `ProtocolParameters` with the market oracle, and implements
`OpenPosition`, `GenerateDebt` (mints stablecoin through T), `ReadState`, and the three hook messages with
the same checks as today's `liquidation_hooks.rs`. The engine and SDK tests then run against the same
interface the real host must provide, which doubles as the spec for the upstream PR.

## 6. Token flows

**Native token or a token program?** The native token knows only `Transfer`, `Credit` and `ReadState`
(NT:12-16): it cannot mint or burn. Its credit is async, so it lands in the same transaction only when the
recipient is declared (§2 points 7-8). The `wrapped_token` program is bridge-only: only the cross-zone inbox
may mint (`lez/programs/wrapped_token/core/src/lib.rs:1-4`).

| Asset | Needs | 0.4 choice |
|---|---|---|
| Stablecoin | minted by the host, burned by the engine (debt first, ADR-010; settle; debt takes) | token program `T` |
| Protocol token | minted by the engine (F4), burned by takers (F5) | `T` |
| Collateral | transfers only | `T` for the PoC (one code path, `Sweep` available); native collateral is a later option: custody works with a seeded native `Transfer`, but there is no sweep, so sweeps become Read + exact Transfer |

**What the vendored token program becomes.** A port of today's `programs/token` to actors, keeping its
model with one change: a holding is `T`'s actor state on an account, a map `{ balances: definition -> balance }`
(an entry at zero is removed), so one account can hold any number of tokens; definition = `T`'s actor state on the definition account, `{ supply, minter: Option<Actor>, .. }`.

| Message (at) | Check | Effect |
|---|---|---|
| `NewDefinition { minter, initial_to, initial_supply }` (definition) | `is_authorized`; empty state | creates the definition |
| `Transfer { definition, to, amount }` (holding) | `is_authorized` (signature or PDA seed) | debit that entry, then sync `Credit` → `(to, T)` |
| `Sweep { definition, to, at_least }` (holding) | `is_authorized`; entry `>= at_least` | moves that entry's whole balance (ADR-033) |
| `Burn { definition, amount }` (holding) | `is_authorized` | debit that entry, then sync `SupplyDown` → `(definition, T)` |
| `MintTo { to, amount }` (definition) | `from == minter` (whole actor) | supply up, then `Credit` → `(to, T)` |
| `Credit { definition, amount }` (holding) | `from_own_program()` | adds to that definition's entry, creating it |
| `ReadState` (any) | `from` is set | `StateReply` → `from` |

Two deliberate differences from v0.2.4: credits are **sync**, so a whole engine conversation settles in one
transaction without relying on async promotion; and a fresh holding needs no signature from its owner. Only
`T` can write `T`'s actor states, and `RecipientMustSign` (spec, S7) is dropped [I]. Squatting is closed two
ways: a holding keeps one entry per definition, so a junk token credited to an account (or to a predictable
vault PDA) cannot lock out the real one, and `NewDefinition` needs the definition account's authorisation, so
nobody can claim someone else's account as a definition. The token must send nothing but `Credit`,
`SupplyDown` and `StateReply` (§4.3 (c) point 5). Whether an undeclared fresh account is admitted is decided by
the sequencer (`admit`, EX:1190-1210): a declared, unsigned, nonexistent key account needs
`PublicAccountEvidence` (found in the Task 3 tests), so the SDK declares recipients and attaches it.

**Flows** (seed = the PDA seed the engine attaches so `T` sees `is_authorized` on an engine holding):

| Flow | Messages |
|---|---|
| Bid payment | `(bidder_stable, T)` Burn(debt part) [bidder signs]; Transfer(penalty part) → B |
| Collateral payout | V Transfer(`collateral_out`) or Sweep(`at_least`) → bidder collateral [seed `["VAULT", A]`] |
| Keeper reward | V Transfer → keeper holding [VAULT seed] |
| Seizure | host vault Transfer/Sweep → V [host's own vault seed] |
| Settle debt, implicit settles | B Burn [seed `["BUFFER", config]`] |
| Surplus start / take / close | B Transfer(lot) → surplus vault [BUFFER]; taker protocol Burn [taker signs] + surplus vault Transfer → taker [`["SURPVAULT", S]`]; surplus vault Sweep → B |
| Debt take | taker stable Burn [taker signs]; K → `(protocol_def, T)` MintTo → taker protocol |

The mint authority (`["MINT_AUTH", config]`, ADR-021) becomes `minter = (K, E)` in the protocol-token
definition; `start_debt_auction` reads the definition and checks it (`MintAuthorityMissing`). The vendored
`twap_oracle` gets the same treatment: an actor port with a `ReadState` answer, or, for the first milestone,
a minimal oracle actor whose publisher sets `(price, timestamp, confidence)` (ADR-050's role).

## 7. Discovery and events

Events exist (`ProgramEvent`, PM:701-710), but the sequencer reportedly stores them only for final blocks
and a standalone sequencer never finalizes ([unverified], research §2). S2 requires e2e against a
standalone sequencer, so events cannot be the only discovery path. Decision: **keep the block scan (ADR-008,
ADR-043) as the primary path; emit events as well**, so a later indexer can use them on a finalizing
network.

What changes in `crates/auction_sdk/src/scan.rs`:

- Today `collect` matches `tx.program_id == engine_program`, decodes the risc0-serde variant index, and
  takes ids by account-list position (config at 2, auction at 6, position at 4; `scan.rs:178-209`).
- On 0.4 there is no account list and no program id per transaction. A public transaction carries
  `RootCall { to: Actor, message }` and the declared actors (§2 point 11). The scanner matches
  `root.to.program_account_id == E` (or the keeper/owner proxy `(x, E)`), decodes the **borsh** root message,
  and reads ids from the message fields (`config`, `position`, `nonce`), deriving the auction PDA with
  `auction_engine_core::pda`. It no longer depends on the order of declared actors.
- A root message does not prove the conversation succeeded. As today, a discovery is only a candidate: the
  views read the auction actor (`getAccount`, then `actor_states[E]`, or `getAccountView(Actor)`; research
  §2) and drop ids whose state is empty.
- The cursor (anchor hash, rewind) is unchanged; the block type and its decoder change.

Events to emit (selector = first 8 bytes of `sha256("auction_engine::<Name>")`): `AuctionOpened`,
`BidFilled`, `AuctionSettled`, `LeftoverClaimed`, `SurplusStarted/Taken/Closed`, `DebtStarted/Taken/Closed`,
`EngineFroze`, `FeedPaused`. Payloads are the borsh of the same structs the views already print. Confirmation
moves to transaction-status polling (ADR-053 already polls `txStatus` through `lez_core`).

## 8. What carries over, what is rewritten, task list

**Carries over unchanged**
- `crates/auction_core` (no_std, no LEZ dependency) with its proptests and Kani harnesses.
- The decision logic inside the handlers: quotes, caps, reward, `Accounting::record_*`, system-auction
  rules, solvency decision, feed validation and pause clock. These are calls into `auction_core` today.
- `auction_engine_core`: `args` validation, error codes, PDA seed functions (the program-id argument becomes
  an `AccountId`), state structs (plus `phase`, `Actor` fields, `slack`).
- SDK quote and plan math (ADR-040) and the encoder seam: `encode.rs` stays the one place that builds
  message bytes; only risc0 serde becomes borsh.

**Rewritten**
- The guest and the handler shell: `accounts.rs` (owner/tag/PDA checks become actor checks),
  `token_calls.rs` (gone; message builders instead), every handler's I/O.
- Handler unit tests built on `AccountWithMetadata`; the integration harness (`V03State`) and the
  security, invariant and chain-budget tests move to `lee` v0.4.
- Host hooks (§5), token and oracle programs (§6).
- SDK transaction builder (root actor, declared actors, nonces, `FeeDeclaration`, payer signature),
  actor-state decoders, scanner (§7), fee-aware confirmation; CLI fee and payer flags; FFI and module on
  `lez_core` 0.6.0; scripts and CI; IDL (hand-kept until SPEL supports 0.4).

**Ordered tasks** (one engineer; assumes little churn after rc1; refines research §6)

| # | Task | Effort | Status |
|---|---|---|---|
| 0 | Spike: standalone sequencer at rc1, deploy an example with `wallet deploy-program`, one fee-paying tx from Rust; record genesis keys, fees, and whether reverts charge the full gas limit | 1-2 d | doable now |
| 1 | Review this note; record ADRs: actor layout and retired PDAs, the five building blocks, claimed time, one bid message, host interface v2 | 1-2 d | doable now |
| 2 | Refactor on v0.2.4 first: split each handler into a pure step function (state in, state out, effects list) and a thin I/O shell; existing tests stay green | 3-4 d | doable now, de-risks 6 |
| 3 | Rust 1.98.1, edition 2024 where needed; pin `lee`/`lee_core` to v0.4.0-rc1 in a new workspace member | 1 d | doable now |
| 4 | Token actor port (§6) and a minimal oracle actor | 4-6 d | doable now |
| 5 | Mock CDP host implementing the F7 interface (§5 option B) | 3-5 d | doable now |
| 6 | Engine guest on `actor_entrypoint!`: messages, building blocks, all 17 instructions; hand-kept SPEL-schema IDL with a drift test | 2-3 wk | doable now; U1 not met |
| 7 | In-process harness on `lee` v0.4; port security (§4.3 (c)), invariant (R6, inv 2/7/9-11/17) and two-instance tests | 1-1.5 wk | after 4-6 |
| 8 | Measure cycles and gas per instruction; new CYCLES.md baseline (ADR-048) | 2-3 d | after 7 |
| 9 | SDK: tx builder, decoders, PDA math, simulate-then-send with fitted `gas_limit`, scanner, confirmation | 1.5-2 wk | after 6 |
| 10 | Scripts and CI: bootstrap from the v0.4 tag, e2e on a standalone sequencer with `RISC0_DEV_MODE=0`, no reliance on events | 3-5 d | after 9 |
| 11 | CLI, `auction_ffi`, Basecamp module on `lez_core` 0.6.0 | 1 wk | blocked: lez_core #72 |
| 12 | Replace the mock host with upstream's port (or our own, §5 option A) | 1 wk after upstream; 3-4 wk alone | blocked upstream |
| 13 | Reproducible guests, ImageIDs, immutable deploys, DEPLOYMENTS.md (ADR-047), real proofs (ADR-045) | 3-5 d | after 12 |
| 14 | Deploy and test on public testnet 0.4 (S1) | 2-3 d | blocked: no testnet |
| 15 | Move to SPEL entrypoint, IDL and client (U1) | 1-2 wk | blocked: no SPEL for 0.4 |

Rough total: 9-13 weeks; tasks 0-10 (about 7-9 weeks) need no upstream release.

**First milestone (doable before anything unblocks):** tasks 0-8. Exit criteria: on `lee` v0.4.0-rc1
in-process, `init_config → liquidate → bid (partial, final) → claim_leftover`, plus `settle_debt` and one
debt and one surplus auction, run against our token actor, the minimal oracle and the mock CDP host; R6 and
the reservation invariants hold after every step; the ported security tests pass (impostor `from`, replies
outside a pending phase, a host that never replies); and a cycle/gas table exists. This proves the
conversation design and the invariants, and turns §5's interface into an executable spec for upstream.

## 9. Open questions for Logos

1. **U1.** Until SPEL supports 0.4, is a hand-written `actor_entrypoint!` guest with a hand-maintained IDL
   in SPEL's JSON schema (plus a drift test) acceptable for RFP-014's "built with SPEL; IDL generated"?
   rfp #237 asks the same.
2. **PDA authorization (#974).** Will the seed grant (EX:1431-1469) and grant inheritance (EX:1365) stay?
   This design relies on seeds only for engine-held token holdings, and on whole-actor `from` checks
   everywhere else, to limit exposure to a change.
3. **Reads.** Is `ReadState` → `StateReply` (NT:80-85, PM:396-407) meant to be a convention every program
   implements? Will the lez-programs ports (stablecoin, token, oracle) answer it? Is a protocol-level read of
   another actor's state planned, which would remove most of our Read round trips?
4. **Stablecoin 0.4 port.** Timing, instruction and actor layout, and whether upstream will take the hook
   interface of §5 (Variant C as `from == engine_actor`, `LiquidatePosition` with a `Seized` report).
5. **Fees on failure.** Is a reverted public transaction charged its whole `gas_limit`? Is a transaction
   whose validity window fails rejected without a fee, or included and charged?
6. **Events in standalone mode.** Confirm events are stored only for final blocks, and whether a standalone
   sequencer can be made to finalize for CI.
7. **Time.** Is `AssertTimestamp` on `CLOCK_01` the intended way for programs to bind to time, or should
   programs use `Response::timestamp_window` (PM:489-492)? What is the block interval on testnet 0.4 (it sets
   our `slack`)?
8. **Program immutability.** Will testnet 0.4 programs be expected to deploy with `immutable = true`? Our I4
   client check (§3) assumes it for the host and token.
9. **Execution limits.** Is there a cap on messages or depth per transaction? `liquidate` needs about 15-20
   deliveries in this design.
10. **`lez_core` 0.6.0 (PR #72).** Expected merge and release date; until then we would link
    `lez/core_ffi` directly, as one third-party playground does.
