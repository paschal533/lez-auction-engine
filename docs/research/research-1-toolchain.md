# RFP-014 PoC: which LEZ / SPEL / risc0 versions to pin (researched 2026-10-01)

**Bottom line:** the working plan (LEZ v0.2.4, SPEL v0.7.0, risc0 3.0.5) **can no longer deploy to the public testnet.** The testnet was reset and upgraded to **LEZ v0.3.0** today (2026-10-01, genesis about 08:43 UTC). Nothing in the SPEL, lez-programs or scaffold ecosystem supports v0.3.0 yet. So a public-testnet deployment means writing the guest directly against `lee_core` v0.3.0 (the Plan/Apply model) without SPEL, and funding a fee payer with LGO bridged in from Bedrock (there is no LEZ faucet any more).

Labels: **[V]** = verified (live RPC call or file at a pinned SHA). **[I]** = inferred.

---

## 1. What the public testnet runs now: LEZ v0.3.0 (reset 2026-10-01)

**Evidence from the live RPC at `https://testnet.lez.logos.co/`** (read-only JSON-RPC POSTs, 2026-10-01 ~15:35 UTC):

| Probe | Result | What it shows |
|---|---|---|
| `getLastBlockId` | `711` | Very young chain **[V]** |
| `getBlock [2]` / `[3]` / `[100]` | timestamps 2026-10-01 08:43:24, 08:43:54, 09:55:11 UTC (u64 ms at offset 72) | Chain genesis was about 08:43 UTC today, with a ~30 s block time **[V]** |
| `getBlock [1]` | contains the string `/LEZ/v0.3/GenesisDeposit` | This domain separator appears in `lez/sequencer/core/src/lib.rs` at both `v0.3.0` and `lez-classic`, but not at `v0.2.4` **[V]** |
| `getFeeState` | `{"base_fee_exec":8,"base_fee_stor":8,"max_gas_exec":10000000,...}` | Fee market exists, so the node is newer than v0.2.4 (the v0.2.4 RPC has no `getFeeState`) **[V]** |
| `getAccountView []` | `-32602 Invalid params` (not `-32601 Method not found`) | `getAccountView` exists **only** in the v0.3.0 RPC trait. It is absent from v0.2.5-rc1/rc3 and `lez-classic` **[V]** |
| `getProgramIds` | `amm`, `privacy_preserving_circuit`, `token` only | No `authenticated_transfer` or `pinata`. That matches the v0.3.0 `artifacts/lez/programs/` set; `lez-classic` and v0.2.4 both ship `authenticated_transfer.bin` **[V]** |
| `getChannelId` | `0303…03` | Matches `lez_sequencer_channel_id` in the infra repo (below) **[V]** |
| `getAccount` on lez-programs' published token-a-def `9zHn…` and AMM config `FQeK…` | `{"nonce":0,"data":{"shards":{}}}` | The old deployments are gone, and accounts now use the v0.3 sharded format **[V]** |

RPC method lists come from `lez/sequencer/service/rpc/src/lib.rs`:
- v0.2.4 `47eba256` has: sendTransaction, checkHealth, getBlock, getBlockRange, getLastBlockId, getAccountBalance, getTransaction, getAccountsNonces, getProofsAndRoot, getAccount, getProgramIds, getChannelId.
- v0.3.0 `db66590a` adds `getFeeState`, `getAccountView`, `getCrossZoneDeadLetters` and `requeueCrossZoneDeadLetter`.

**The infra repo confirms it.** status-im/infra-logos branch `lez-v3-testnet` (PR #71, "LEZ v3 Testnet upgrade", commits 2026-10-01 07:52–08:57 UTC), file `ansible/vars/lez/testnet.yml`:

```
lez_cont_tag: 'v0.3.0'
lez_blockchain_cont_tag: '0.3.0'
lez_sequencer_channel_id:  '0303030303030303030303030303030303030303030303030303030303030303'
```

- `master` still says `lez_cont_tag: 'v0.2.4'`. The PR is open but already deployed, and the live RPC matches the PR. **[V]**
- The same PR moves the old chain to `testnet-classic.lez.logos.co` (`lez_cont_tag: 'lez-classic'`). **That is not v0.2.4.**
  - `lez-classic` is tag SHA `74780044`, which is `v0.2.5-rc3` plus one commit ("bump logos-blockchain to 0.3.0-rc.5").
  - The endpoint does not serve yet: TLS name mismatch, empty body. **[V]**
- Devnet (`devnet.lez.logos.co`) runs `v0.3.0-rc1`, block ~3362. **[V]**

**LEZ tags** (`gh release list`):

| Tag | SHA | Released |
|---|---|---|
| `v0.2.4` | `47eba256479f6f785acbd138834340703cd03401` | 2026-08-07 |
| `v0.2.5-rc1` … `rc3` | rc3 = `a03b54aa` | 2026-09-09 to 09-14 |
| `v0.3.0-rc1` | | 2026-09-26 |
| `lez-classic` | `74780044` | 2026-09-29 |
| `v0.3.0` | `db66590ab821a4e142c211017a3866d007f6fa77` | 2026-09-30 |

`main` is now `411adc8fdb` (2026-10-01 08:04). It is v0.3.0 plus 5 commits; the only files changed are `.config/nextest.toml` and `lez/indexer/ffi/Cargo.toml` (the r0vm in-process link fix and CI). Program artifacts and ImageIDs are unchanged. **[V]** The testnet was most likely built from the `v0.3.0` container tag. **[I]**

**Docs lag behind.** logos-co/logos-docs `docs/lez/get-started/run-lez-wallet-via-cli.md` still says "check out … `git checkout v0.2.4`". **[V, now stale]**

**Reset history:**
- 2026-08-05/06: v0.2.0 → v0.2.2 (logos-co/lez-faucet `docs/testnet.md`).
- 2026-09-08: reset (edenbd1/antumbra-auction commit `8ab3bdca`).
- 2026-10-01: reset and upgrade to v0.3.0.

Expect more resets.

**Not reached:** forum.logos.co and testnet.blockchain.logos.co did not resolve from this machine (DNS). The explorer HTML shows no version string.

---

## 2. SPEL targets LEZ v0.2.4 only; no v0.3.0 support exists

`spel-framework-core/Cargo.toml` and `spel-framework/Cargo.toml` at all three SPEL tags **[V]**:

| SPEL ref | SHA | Pins |
|---|---|---|
| `v0.7.0` | `4107dcb14ec0eb64e9e80a9691a3df0b9acd3c2b` (2026-09-17) | `lee_core` / `lee` as `tag = "v0.2.4"`; spel-cli also pins `common`, `sequencer_service_rpc`, `wallet` at `v0.2.4`; `risc0-zkvm = "3.0.3"` (spel-cli); toolchain `1.94.0` |
| `v0.7.0-rc.1` | `512e9591` (dated *after* v0.7.0) | identical: v0.2.4 |
| `v0.7.0-rc.2` | `6c0598a4` (2026-09-22) | identical: v0.2.4 |
| `main` | `415b7bcb` (2026-09-30) | identical: v0.2.4 |

- CHANGELOG v0.7.0: "**Upgrade to LEZ v0.2.4.** The Logos Execution Zone dependency moves from `v0.2.0` to `v0.2.4` across every crate."
- **No v0.3.0 branch or release.**
  - The only newer work is open PR #290 "Mke SPEL work with LEZ 0.2.5-rc2" (0x-r4bbit fork, branch `chore/lez-0.2.5-rc2-compat` at `dfa5c3ce`, pins `tag = "v0.2.5-rc2"`). That is still not v0.3.
  - The `release/v0.3.0` branch is an old SPEL v0.3.0 release branch, not LEZ v0.3.
- edenbd1/antumbra-lez `programs/vesting/Cargo.toml` says it outright: "SPEL has no v0.3 support." **[V]**

---

## 3. lez-programs pins (main `64c01210ad`, 2026-10-01)

| Item | Pin | Evidence |
|---|---|---|
| lee_core / lee | `tag = "v0.2.4"` (lock: `47eba256…`) | `Cargo.toml` L42-43, `Cargo.lock` **[V]** |
| SPEL | `rev = "1ef0500f8fc8ce3ddf95523726ae159db83f744f"` (crate version 0.6.0, commit 2026-08-25 "witness exchange…(#246)"; already pins LEZ v0.2.4) | `programs/*/methods/guest/Cargo.toml`, `tools/idl-gen/Cargo.toml` **[V]** |
| risc0 | `risc0-zkvm = "=3.0.5"`; `risc0-binfmt = "=3.0.4"` in the FFI module | **[V]** |
| Rust toolchain | `1.94.0` | `rust-toolchain.toml` **[V]** |
| lez_core module (Nix) | `logos-execution-zone-module?rev=acf0cd501b262c4c15969e3735e85318297b85bf` (module v0.4.1). It wraps LEZ `9edf4a62`, which is v0.2.4 plus 1 commit (nix risc0 fetch fix) | `modules/amm/flake.nix`, `flake.lock` **[V]** |

**`docs/testnet-runbook.md`:**
- It requires `spel` / `wallet` built from **`0x-r4bbit/spel` branch `refactor/lez-v020-compat`** (head `df17acd9`, 2026-07-03). That branch pins **LEZ v0.2.0**, so the runbook is stale even for v0.2.4. **[V]**
- It also uses `wallet config set sequencer_addr …`, which SPEL v0.7.0 notes is silently ignored on v0.2.4 (the config needs the `sequencers: [{ sequencer_addr }]` array).

**`DEPLOYMENTS.md`:**
- Still headed "Testnet (v0.2.4)" (last change 2026-09-10).
- All of those accounts are now empty on the v0.3 chain. **[V]**

**CI:**
- `gh run list -R logos-blockchain/lez-programs`: every recent `main` CI run is **success**, including 2026-10-01 09:35. **[V]**
- Upgrade work in progress: open PR #385 "Upgrade programs to LEZ v0.2.5-rc2" (all checks pass). There is no v0.3.0 port yet.

**Relevant to RFP-014:** lez-programs ships `programs/stablecoin` (the RFP-013 RAI-style CDP design, with positions, vaults and an `OraclePriceAccount` struct) plus `twap_oracle`. Its design doc says: "liquidation lives in RFP-014". It is on v0.2.4.

---

## 4. Third-party projects

**Kolkheti-Labs/argo** (RFP-008 lending)
- PR #1 head = tag `m0-submit` = `566fb611724c94501b6d917e5d511d0eaad22879` (PR open, last update 2026-09-14).
- Pins: LEZ `tag = "v0.2.4"` ("pinned to the tag the public testnet runs"), SPEL `rev = 1ef0500f…` ("the revision lez-programs builds against"), `risc0-zkvm`/`risc0-build = "=3.0.5"`, toolchain `1.94.0`. **[V]**
- **No testnet deployment.** README: "Nothing here is deployed and nothing custodies value… The harness never touches the public testnet." **[V]**

**edenbd1/antumbra-auction** (collateral auction maths; the closest prior art to RFP-014)
- HEAD `8ab3bdca` (2026-09-27).
- Vendors SPEL with `lee_core`/`lee` at `rev = "47eba256…"` (= v0.2.4); `risc0-zkvm 3.0.5`. **[V]**
- `DEPLOYMENTS.md` claims a v0.2.4 deploy: ImageID `38ab5886…`, deploy tx `ac43ac1a…` at block 20377. The commits suggest it ran around 2026-08-24. **[I]**
- Its own latest commit says those transactions were wiped by the **2026-09-08 reset**. They do not exist on the current chain either.

**edenbd1/antumbra-lez** (sister repo, already ported to v0.3)
- Pins every LEZ crate to `rev = "db66590ab821a4e142c211017a3866d007f6fa77"` (v0.3.0): `wallet`, `lee`, `lee_core`, `common`, `programs`, `clock_core`, `token_core`, `program_loader_core`. Also `risc0-zkvm =3.0.5`, `borsh 1.5`, and the RISC Zero docker builder `r0.1.91.1`. **[V]**
- `DEPLOYMENTS.md`: "testnet v0.3 deployment: **pending funding**". The full lifecycle was run on a **local** v0.3.0 sequencer only. **[V]**
- The best available v0.3 port guide is `docs/v03-port-plan.md` and `docs/deploy-v03.md` in this repo.

**Conclusion:** no known third-party program is deployed on the current v0.3 public testnet. Every v0.2.4 deployment has been erased.

---

## 5. logos-scaffold (`lgs`)

- Latest is **v0.4.0**, `f63d70f75bd0809c340db535a189173ab32ba506` (2026-09-25): "support only Basecamp 0.3.0". **[V]**
- `src/constants.rs` at v0.4.0 **[V]**:
  - `DEFAULT_LEZ = { sha: "cf3639d8…", tag: "v0.1.2" }`
  - `DEFAULT_SPEL = { sha: "73fc462e…", tag: "v0.5.0" }`
  - `DEFAULT_CIRCUITS_VERSION = "0.4.1"`
  - `DEFAULT_BASECAMP_PIN = "bbe5da0e…"` (Basecamp 0.3.0)
- The `lez-framework` template substitutes `{{lez_tag}}` and `{{spel_pin}}` and pins `risc0-zkvm 3.0.5`.
- So `lgs new --template lez-framework` produces a **LEZ v0.1.2 / SPEL v0.5.0** project by default. That is three LEZ minor versions behind the testnet.
- Pins can be overridden through `[repos.lez]` / `[repos.spel]` in `scaffold.toml`, or `--lez-ref`. `doctor` enforces that SPEL and LEZ agree.
- With v0.2.4 plus SPEL v0.7.0 overrides, `lgs run` and `lgs test-node` should work for a **localnet**. **[I; not executed, this was a read-only task]**
- **v0.3.0 cannot work** through the lez-framework template, because SPEL has no v0.3. **[I, strong]**
- Open PR #199 renames the template to `spel` and delegates scaffolding to `spel init`.
- `lgs basecamp` targets Basecamp 0.3.0, and upstream Basecamp is now **0.3.1** (released 2026-10-01).

---

## 6. logos-execution-zone-module (`lez_core`)

The repo has no tags or releases. Versions come from `metadata.json` "version", and the wrapped LEZ comes from the `flake.lock` node `logos-execution-zone` **[V]**:

| Module commit | Module version | LEZ wrapped |
|---|---|---|
| `b22014427d` (2026-08-27) | 0.4.0 | `87fca2a1` (rev pin) |
| `acf0cd501b` (2026-09-14) | 0.4.1 | `9edf4a62` (v0.2.4 + 1 nix commit). **This is the v0.2.4 line**, used by lez-programs |
| `0ea57f8a1c` / `825d2a4126` | 0.4.1 / 0.4.2 | `v0.2.5-rc2` (`d8596eb7`); adds the `payer` FFI argument |
| `61cee4d898` / `f3a08c659c` | 0.4.2 / 0.4.3 | `v0.3.0-rc1` (`f7fda38a`) |
| `bbe150ace6` (2026-09-30) | 0.5.0 | `v0.3.0` (`db66590a`) |
| **`b89e5d24df` (main, 2026-10-01)** | **0.5.0** | **`411adc8f`** (LEZ main = v0.3.0 + r0 link fix) |

**Basecamp itself does not pin `lez_core`.** I searched logos-co/logos-basecamp code and the 0.3.0 and 0.3.1 `flake.lock` files for `execution-zone`/`lez`: no hits. The module is installed separately as an LGX (it is listed in logos-co/logos-modules). Which build a Basecamp user gets therefore depends on the module package they install. **[V for the absence; I for the install path]**

The local checkout `C:\Users\user\Documents\basecamp\logos-basecamp` is at `01d3f59` (2026-07-20), which is older than 0.3.x.

---

## 7. Breaking changes from v0.2.4 to v0.3.0 and the migration cost

Source: v0.3.0 release notes (`gh release view v0.3.0`), `examples/program_deployment/methods/guest/src/bin/hello_world.rs` at three tags, `lez/wallet/src/cli/programs/program_loader.rs`, and antumbra-lez `docs/v03-port-plan.md`.

| Area | v0.2.4 | v0.3.0 | Impact on a liquidation/auction program |
|---|---|---|---|
| Guest entry API | `read_lee_inputs` → `ProgramInput{pre_states}` → `ProgramOutput::new(...).write()`, whole-account post states | `run_program(plan, apply)`. The `Plan` sees account ids, signer flags and shard selectors but **no data**; `apply(effect, &ShardData) -> Option<Vec<u8>>` runs per shard **[V, hello_world.rs]**. (v0.2.5 was an intermediate third API: `read_lee_call`, `AccountStateDiff`.) | **Full guest rewrite.** Every state-dependent check (health factor, auction price, bid validity) moves into `apply`. A panic there rolls back the whole transaction. |
| State model | One `program_owner` and one `data` blob per account | `shards` map keyed by program account id; native balance is a shard (#835 sharded state, #886 native shard balance). Live accounts return `{"data":{"shards":{}}}` **[V]** | No program-owned escrow. Vaults become PDA holdings of shards. Every reader (FFI, C++ module, QML) must decode shards. |
| `program_owner` and program identity | `ProgramId` (ImageID) | `program_owner: AccountId` (#722). Programs are addressed by a **header account** chosen at deploy, with bytecode in linked segment accounts (`program_loader`: `WriteSegment`, `CreateHeader`, `UpdateHeader`, `Deploy`, `Update`) **[V]** | All PDAs derive from the header id, so every derivation changes. Upgrades become possible in place (header rewrite). |
| Built-ins | Referenced by ImageID | Addressed by stable **name** (#908); `authenticated_transfer`, `pinata` and `faucet` removed (#817, #875) **[V via getProgramIds]** | Token and transfer calls change shape (antumbra: `Transfer { amount_to_transfer, descriptor: { definition_id, kind } }` **[I from antumbra]**). Native transfers go through `native_token::custody_transfer`. |
| Chained calls | `ChainedCall.pre_states`, by ProgramId | `pre_state_refs`, no ProgramId (#790, #735, #929) | The liquidation → token-transfer → auction-settlement CPI chain must be re-plumbed. |
| Codec | risc0 words / serde | Borsh codec, I/O changes (#770) | Instruction encoding, IDL and the FFI serializer all change. |
| Fees and payer | No fees | Fee market (#801): `getFeeState` reports base fee 8, `max_gas_exec` 10,000,000. Public transactions name a **payer** (#841) and are charged even on failure; private transactions are exempt **[I from antumbra]** | Keepers and liquidators must hold LGO. The 10M gas cap per transaction bounds batch liquidations. |
| Clock and time | Plans could read the clock account | Plans cannot read the clock; they set timestamp validity windows instead **[I from antumbra]** | Dutch-auction price decay and liquidation deadlines must use validity windows plus timestamps passed as arguments. |
| Events | None | `Plan::event` (public transactions), indexer event RPC and filters (#705, #707, #713, #785) | A plus: the UI can subscribe to auction and liquidation events. |
| Other | | Private-account Identifier u128 → 256 bit (#919); ciphertext padding (#630); private authorization key (#669); incremental updates with a restricted program surface (#912) | Matters only if private bids are in scope. |
| Toolchain | Rust 1.94.0, risc0 3.0.5 | Rust **1.98.1** (`rust-toolchain.toml` at v0.3.0), risc0-zkvm **3.0.5**, risc0-build 3.0.5, risc0-binfmt 3.0.2 **[V]** | Host crates need Rust 1.98.1. Guests still use risc0 3.0.5. |

**Migration cost:**
- For anything written on v0.2.4 plus SPEL, this is a **rewrite**, not a port: new guest programming model, new state layout, new addressing and PDAs, a new codec, fees, and hand-written IDL/CLI/FFI, because SPEL is absent.
- antumbra-lez's single vesting program needed a new core crate, a new guest, a new Rust CLI on the v0.3 `wallet` crate, new executor tests, and C++ shard decoding. That took roughly a day of commits (2026-09-30 to 10-01). **[I]**
- For our four layers (SPEL program, Rust FFI, C++ module, QML app), the cheapest path is to **design for v0.3.0 from day one** rather than build on v0.2.4 and port later.

---

## Recommendation

### Track A: deploy to the public testnet now (requires v0.3.0)

| Component | Pin |
|---|---|
| LEZ crates (`lee_core`, `lee`, `wallet`, `common`, `programs`, `clock_core`, `token_core`, `program_loader_core`) | `rev = "db66590ab821a4e142c211017a3866d007f6fa77"` (tag v0.3.0). Use one form (rev *or* tag) everywhere so Cargo resolves a single source. `411adc8f` (main) is equivalent for programs; prefer it only if you link the indexer FFI. |
| risc0 | `risc0-zkvm = "=3.0.5"`, `risc0-build = "3.0.5"`; docker guest builder `RISC0_DOCKER_CONTAINER_TAG=r0.1.91.1` **[I, from antumbra]** |
| Rust toolchain (host) | `1.98.1` |
| SPEL | **none**. Write the guest against `lee_core` `run_program(plan, apply)` and hand-maintain the IDL. |
| lez-programs | **no compatible rev.** Main `64c01210` is v0.2.4. Use it only as design reference (stablecoin, twap_oracle). |
| lez_core module | `b89e5d24df` (main, v0.5.0, wraps LEZ `411adc8f`) |
| Basecamp | 0.3.1 (upstream latest; scaffold supports 0.3.0) |

**Confidence:**
- That this is the right version set: **high**.
- That you can actually *land* a deploy today: **medium-low**. Risks:
  1. **Funding.** There is no faucet on v0.3; the fee payer needs LGO via a Bedrock testnet node, the Bedrock faucet and a `ChannelDeposit`, and antumbra-lez is stuck at "pending funding". I could not reach testnet.blockchain.logos.co from here.
  2. **No SPEL**, so no generated IDL, CLI or FFI.
  3. **Instability.** The infra PR is still open, the chain is 8 hours old, and resets have happened three times since August.
  4. **Docs lag.** logos-docs still says v0.2.4.

### Track B: fastest working PoC with existing tooling (localnet only)

| Component | Pin |
|---|---|
| LEZ | `tag = "v0.2.4"` (`47eba256479f6f785acbd138834340703cd03401`) |
| SPEL | v0.7.0 (`4107dcb1`), or `rev 1ef0500f` to match lez-programs and argo exactly |
| risc0 | `=3.0.5` |
| Rust toolchain | `1.94.0` |
| lez-programs | main `64c01210ad` (CI green; provides the stablecoin to liquidate against) |
| lez_core module | `acf0cd501b262c4c15969e3735e85318297b85bf` (v0.4.1, LEZ v0.2.4) |

- Confidence that it builds and runs on a local sequencer: **high**.
- Confidence that it reaches a public network: **zero**. No public network runs v0.2.4 any more, and `testnet-classic` runs `lez-classic` (the v0.2.5-rc3 line) and is not serving yet.
- Everything built on this track must be rewritten for v0.3.

**Suggested approach:** take Track A for the on-chain program and use Track B's lez-programs stablecoin only as a specification reference. Ask Logos (forum or Discord) for testnet LGO funding or a fee-payer grant up front, because funding is the gating item. Also ask whether a SPEL release for v0.3 is planned.

### Not verified (read-only constraints)

- I did not run `lgs` or any build.
- I did not compute ImageIDs from the v0.3.0 ELFs. The v0.3.0 identification rests on the RPC method set, the builtin set, the genesis domain string and the infra vars.
- I did not test fee enforcement on a write.
- forum.logos.co was unreachable.
