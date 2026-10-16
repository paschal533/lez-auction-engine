# Red-team review, October 2026

- **Commit:** `5227ba2` (branch `feat/plan-3b`)
- **Date:** 2026-10-17
- **Reviewer:** independent red-team pass (the `security-redteam` pass that spec §10 calls for). The reviewer did not implement any of the reviewed code.
- **Result:** no Critical, High or Medium finding. There are 6 Low and 6 Info findings, RT-01 to RT-12.

## Scope and method

Scope, in priority order:

1. `programs/auction_engine`:
   - all 17 handlers, the guest dispatch and `auction_engine_core` (instruction enum, args, state, PDAs);
   - the `auction_core` functions the handlers call: `liquidation`, `auction`, `accounting`, `pause`, `price`, `system`.
2. The stablecoin hooks: `programs/stablecoin/src/liquidation_hooks.rs`, the host helpers they call (`checks.rs`, `update_redemption_rate::decode_oracle`) and `patches/stablecoin`.
3. `crates/auction_sdk`: encoders, `verify_*`, confirmation and the lock file, the scanner cursor, the keystore, and quotes.
4. `cli/`: what is signed and what is shown, exit codes, the dev-command guard, and the oracle publisher.
5. `scripts/*.sh`, `scripts/lib/localnet.sh`, `.github/workflows` and `.github/actions`.

Known and accepted items were excluded unless a new consequence was found:

- the residuals and review log in `THREAT_MODEL.md`;
- the ADR rulings in `docs/design-log.md`;
- the "Declined to judge" lists in `.superpowers/sdd/*/final-review.md`.

### Method

White-box source review with an adversarial disprove step. Each candidate was traced from the input to the effect, checked for mitigations elsewhere (lee pre-state and authorisation rules, PDA pinning, client checks), and then confirmed or dropped. Areas 3 to 5 were first swept by two parallel read-only passes. The reviewer then re-verified every reported item against the source before writing it down.

### Test runs

All runs were in WSL, in dev mode, one cargo command at a time. There was no real proving and no localnet.

| Command (via `scripts/wsl-cargo.sh`) | Result |
|---|---|
| `test -p auction_core` | all pass |
| `test -p auction_engine_program --lib` | 157 passed |
| `RISC0_SKIP_BUILD=1 test -p stablecoin_program --lib liquidation_hooks` | 51 passed |
| `test -p engine_integration_tests --test security` | 15 passed; the impostor, laundering and private-path rejections panic as designed |

No finding below needed a run to establish. Each one is confirmed by reading the code at the cited lines. RT-01 rests on one unconfirmed premise, stated in its section.

### What was tried and found sound

The engine and hooks:

- **Authorisation on every handler.**
  - Every handler requires a signer.
  - Each admin path uses `require_admin` against the PDA-bound `cfg.admin`.
  - `claim_leftover` takes the owner from the auction record.
  - Bids and takes require the signed holding to be the one that is debited.
- **Account confusion.**
  - Every engine account is checked for owner, tag, PDA and back-reference (`config`).
  - Every holding is checked for its token program and definition.
  - Optional slots are pinned (ADR-030).
  - Host accounts are re-derived under `cfg.host_program_id`.
  - Aliasing a recipient with an engine vault is refused explicitly. Any other alias fails lee's pre-state check.
  - Duplicate-account and swapped-definition combinations were tried for `bid`, `take_surplus` and `take_debt`; each fails a definition check or the signer requirement.
- **Fund movement and rounding.**
  - Paid amounts are floored and owed amounts ceiled.
  - The lot-binds re-strike uses `ceil(L·(1−d)/P)`.
  - The liveness exception is bounded to one base unit per auction (accepted).
  - Sweeps use the actual vault balance.
  - `settle_debt` excludes `debt_on_auction`.
  - The debt-take proration cannot exceed `max_mint_per_auction`, because the system parameters are locked while `debt_live`.
- **Caps.**
  - The reservation is added at liquidation and released exactly once, at chain end (`bid_with_settlement` or `settle_empty`).
  - `ChainEndAccountsMissing` stops a plain `bid` from ending an auction without `Accounting`.
  - The type and global reservations stay in step.
- **Pause clock.**
  - Every `t_eff` use follows an observation at the same `now`, and `Auction::open` snapshots `paused_total_ms` after that observation.
  - No auction can start during an open pause or an unclosed gap.
  - Future oracle timestamps are clamped.
  - No new consequence was found beyond the accepted S5 residual.
- **System auctions and `check_solvency`.**
  - Takes and closes require `live`.
  - At most one auction of each kind is live.
  - The relist delay is applied before any effect.
  - The freeze decision matches spec §5.11.
  - The `ENGINE_AUTH` seed goes only to `cfg.host_program_id`, and every forwarded account is pinned.
- **Hooks.**
  - Variant C is asserted before any effect, with a non-zero bound program id.
  - `set_liquidation_engine` is top-level only.
  - The proportional bound rounds down in U512.
  - Full seizure sweeps the whole vault.
  - The health gate uses the host's own accumulator and redemption projection.
  - Freeze is idempotent.
- **Encoding.** Bps arguments are range-checked before the `u16` conversion (`args::bps`). Schedules are validated, and there is no narrowing `as` cast on argument paths.

The SDK and CLI:

- The encoders use the same enum the guest dispatches.
- `verify_config` runs before every signing command that uses a config, and `verify_binding` runs before `liquidate`.
- `verify_chain_identity` fails closed while `chain_genesis_hash` is null.
- `--at-ms` is refused for signing.
- Quote-derived bounds round in the user's favour.
- The keystore never formats key material (custom `Debug`; errors give a position, never content).
- The lock file is created atomically and only its owner removes it.
- Cursor tampering can only add or hide candidates, and every candidate is re-verified on chain.

Scripts and CI:

- There is no `pull_request_target` and no `secrets.*`.
- No `github.event.*` value is interpolated into `run:`.
- The LEZ revision is checked against `^[0-9a-f]{40}$`.
- Recursive deletes are `mktemp -d` under an EXIT trap or `${LN:?}`-guarded.
- `ln_stop_sequencer` checks the pid and its cmdline before signalling.
- The Docker builder is pinned by digest.
- No key or wallet is uploaded as an artifact.

## Findings

| ID | Severity | Component | Finding | Reproduction | Ruling | Resolution |
|---|---|---|---|---|---|---|
| RT-01 | Low | auction_sdk confirm, cli error | A `Dropped` verdict followed by the advised resend can apply twice if the original lands late; ADR-042's "at most one applies" holds only if the resend is signed first | confirm_tests sketch (RT-01) | park to M1: CLI wording and a confirm.rs test; cost if wrong: a resend after a `Dropped` verdict whose original lands late pays twice, on any network | parked to M1: reword the Dropped message and add a confirm.rs test; residual in THREAT_MODEL.md |
| RT-02 | Low | CI `install-risc0` | `curl \| bash` of an unpinned installer on every cold cache, including the guest-repro and self-hosted jobs | `grep -n curl .github/actions/install-risc0/action.yml` | accept as a residual; pin the installer by hash in M1; cost if wrong: a compromised installer runs on a CI runner | accepted: residual in THREAT_MODEL.md; M1: pin the installer by hash |
| RT-03 | Low | CI workflows | Third-party actions pinned by movable tags; `install-action` and the fuzz toolchain float | `grep -nE 'uses: (dtolnay\|Swatinem\|taiki-e)' .github/workflows/*.yml` | park to M1: SHA-pin the actions (already a Task 5 deferred minor); cost if wrong: a moved tag runs new code in CI | parked to M1: SHA-pin third-party actions and the floating tool versions |
| RT-04 | Low | CI `core.yml`, `fuzz.yml` | No `permissions:` block, and checkout persists the token | `grep -L permissions .github/workflows/*.yml` | fix now, as its own commit: two `permissions: contents: read` blocks; cost if wrong: none | fixed: d7a0b2b (core.yml and fuzz.yml `permissions: contents: read`) and a follow-up setting `persist-credentials: false` on every `actions/checkout` in all five workflows, so no token stays in `.git/config`; re-reviewed by the red-team seat, no regressions |
| RT-05 | Low | CI `real-proofs.yml` | The self-hosted prover runs any dispatched branch or pushed tag, and its persistent `~/.risc0` carries into later main runs | read `real-proofs.yml:3-8,42-48` | accept; add a hardening note to real-proofs.md; the job is off by default; cost if wrong: only once the runner is enabled | accepted: residual in THREAT_MODEL.md; hardening note in docs/architecture/real-proofs.md (default branch only, ephemeral runner, no persisted ~/.risc0) |
| RT-06 | Low | `scripts/export-hook-patches.sh`, `check-hook-patches.sh` | A `[local]` commit can change deployed stablecoin source without it reaching the upstream patch series, and the check still passes | throwaway-branch commands (RT-06) | accept, documented; M1: compare the deployed tree against the patch series; cost if wrong: an upstream PR could miss a deployed change | accepted: residual in THREAT_MODEL.md; M1: compare the deployed tree |
| RT-07 | Info | `auction_core::price::bid_price`, ADR-005 | Band clamp: bidders are paid above market when `M < R·lo`; auctions are unbiddable when `M > R·hi/(1−d_max)` | core test sketch (RT-07) | accept (Info); recorded as a THREAT_MODEL residual | accepted: residual in THREAT_MODEL.md |
| RT-08 | Info | `cli/src/oracle_cmds.rs` | `dev oracle-publish` is three non-atomic transactions; `dev keeper` stops on its first error | tx_golden sketch (RT-08) | accept (Info) | accepted (Info) |
| RT-09 | Info | `cli/src/admin_cmds.rs` | `admin set-engine-params` shows only the slot and ref-price changes, not the freeze levers it signs | read `admin_cmds.rs:287-300` | accept (Info) | accepted (Info) |
| RT-10 | Info | `auction_sdk/src/rpc.rs` | The RPC URL, including any `user:pass@`, is copied into error text | read `rpc.rs:155-167` | accept (Info) | accepted (Info) |
| RT-11 | Info | `scripts/demo.sh`, `prove-suite.sh`, `build-guests.Dockerfile` | Script hygiene: the delete guard is a file name, `pkill -x r0vm`, a shared cache-mount id | read the cited lines | accept (Info) | accepted (Info) |
| RT-12 | Info | `cli/src/env.rs` | `Env::account` parses a base58 id before the keystore, skipping its private-account refusal (no impact found) | read `env.rs:74-79` | accept (Info); no impact found | accepted (Info) |

## RT-01 (Low): a dropped transaction that lands late plus the advised resend can apply twice

**Component:** `crates/auction_sdk/src/confirm.rs:94-96` (Dropped), `:425-450` (`submit` reads the nonce again on every call); `cli/src/error.rs:12` (message, exit 3); ADR-042 "Trade-offs" and `docs/architecture/sdk.md:94`.

**Failure:**

1. A bid (or a take) is accepted by the sequencer but not applied by `h0 + 5`. `confirm` returns `Dropped`, `submit` releases the lock, and the CLI prints "nothing was applied; re-quote and resend" and exits 3.
2. The original transaction is applied later, and the signer nonce becomes `n0 + 1`.
3. The user or a script reacts to exit 3 and runs the same command again. `lock_with_current_baseline` reads the new nonce `n0 + 1` and signs the resend at that nonce. It is applied as well.
4. Two fills execute. The user spends up to `coin_in_max` twice and receives two lots.

ADR-042 argues that "a re-quoted transaction reuses nonce n0, so at most one of the two applies". That holds only when the resend is signed before the late original lands. If the original lands first, the resend reads `n0 + 1` and nothing stops it.

**Impact:** no protocol funds are at risk; each fill is individually correct. The user overspends against their own intent after being told nothing happened. Unattended scripts that retry on exit 3 are exposed most.

**Not confirmed:** whether the LEZ v0.2.4 sequencer can apply a transaction more than 5 blocks after accepting it. The ADR itself assumes it can ("a backlogged transaction can still land later").

**Reproduction (test sketch):** add to `crates/auction_sdk/src/confirm_tests.rs`, using its fixture:

1. `fx.chain.set_send_mode(SendMode::Drop); fx.chain.set_height_step(1);` then `run_observe(&fx, dir, &s)` (the file's `submit` wrapper). Assert `Outcome::Dropped`.
2. `fx.chain.set_nonce(id(k::CALLER), 1);` simulates the late landing.
3. `fx.chain.set_send_mode(SendMode::Include);` then `run_observe(&fx, dir, &s)` again. Assert `Outcome::Included`, and assert that the second sent transaction carries nonce 1. Both transactions are now applied.
4. Run `scripts/wsl-cargo.sh test -p auction_sdk --lib confirm`.

**Suggested fix:**

- On `Dropped`, write a tombstone `{n0, h0, hash}` next to the lock.
- On the next `submit` for that signer, if the nonce is now above the tombstone's `n0`, refuse with "a transaction reported dropped has since landed; re-read before resending", or require `--resend`.
- At minimum, change the exit-3 message to "not applied by block N; it may still land, so check the nonce and state before resending", and correct the ADR-042 sentence.

## RT-02 (Low): unpinned installer piped to bash

**Component:** `.github/actions/install-risc0/action.yml:15` (and the cache key at `:10`).

**Attack:** on a cache miss, `curl -L https://risczero.com/install | bash` runs. The script is not pinned and has no checksum, and it fetches `rzup`. The cache key is a fixed string that does not cover the installer. A compromise of risczero.com, or of its CDN or TLS path, then runs code in:

- `core.yml` (`programs`, `coverage`, both also on `pull_request`);
- `guests.yml` (`guest-repro`, the job that checks that the committed guest binaries are reproducible);
- `e2e.yml`;
- both `real-proofs.yml` jobs, including the self-hosted one.

A tampered toolchain in `guest-repro` undermines the I4 reproducibility claim (ADR-047) at its root. The component versions are pinned (`:16-19`). Whether `rzup` verifies component checksums was not checked.

**Reproduction:** `grep -n 'curl' .github/actions/install-risc0/action.yml`.

**Suggested fix:**

- Download a fixed `rzup` release asset, check its sha256 against a value committed in the action, and then run it. Alternatively, run the jobs in the upstream `ci.Dockerfile` image pinned by digest.
- Put the `rzup` version or hash in the cache key.

## RT-03 (Low): third-party actions on movable refs; tool versions float

**Component:**

- `dtolnay/rust-toolchain@1.94.0` (a branch name in that repository): `core.yml:8,29,52,78`, `e2e.yml:35`, `guests.yml:46`, `real-proofs.yml:25,47`.
- `dtolnay/rust-toolchain@nightly`: `fuzz.yml:16`.
- `Swatinem/rust-cache@v2`: `core.yml:32,53,83`, `e2e.yml:37`, `guests.yml:48`, `real-proofs.yml:27,49`.
- `taiki-e/install-action@v2` with no tool version: `core.yml:81`, `fuzz.yml:17`.

**Attack:** whoever controls one of those upstream refs moves it and gets code execution in every job that uses it. That includes the self-hosted prover (RT-05) and, together with RT-04, a token that may have write scope.

**Reproduction:** `grep -nE 'uses: (dtolnay|Swatinem|taiki-e)' .github/workflows/*.yml`.

**Suggested fix:**

- Pin each third-party action to a full commit SHA, with a `# vX` comment.
- Give `install-action` explicit `tool@version` pairs.
- Pin the nightly toolchain to a dated nightly.

## RT-04 (Low): default token permissions and persisted credentials in two workflows

**Component:** `.github/workflows/core.yml` and `fuzz.yml`, which have no `permissions:` block. `e2e.yml`, `guests.yml` and `real-proofs.yml` set `contents: read`.

**Attack:**

- On `push` and `schedule` runs, `GITHUB_TOKEN` gets the repository default. That is write-all on repositories created before 2023 or where an admin chose it; the repository setting could not be checked offline.
- `actions/checkout` stores the token in `.git/config` by default, where any later step can read it, including code arriving through RT-02 or RT-03.
- Chained with either of those, the token could push commits or move tags.

**Reproduction:** `grep -L permissions .github/workflows/*.yml` prints `core.yml` and `fuzz.yml`.

**Suggested fix:** add a top-level `permissions: { contents: read }` to both files, and `with: { persist-credentials: false }` on every checkout that does not push.

## RT-05 (Low): the self-hosted prover runs unreviewed refs, and its state persists

**Component:** `.github/workflows/real-proofs.yml:3-8,42-48`, plus `install-risc0/action.yml:15,20`.

**Attack:**

1. With `vars.REAL_PROOF_SELF_HOSTED == 'true'`, the job runs for any `workflow_dispatch` (from any branch) and any pushed `v*` or `rfp014-*` tag. A tag can point at an unmerged, unreviewed commit.
2. A self-hosted runner is not ephemeral. `~/.risc0`, `~/.cargo` and the rust-cache survive between jobs. `install-risc0` skips installation when `~/.risc0/bin/rzup` exists and puts `~/.risc0/bin` first on `PATH`.
3. A collaborator with write access tags or dispatches a commit whose `build.rs` or test replaces `~/.risc0/bin/r0vm`, for example with a shim that reports success.
4. Every later nightly run on `main` uses the shim, and `check-proof-report.py` accepts its output as real-proof evidence.

Outsiders cannot do this, because there is no `pull_request` trigger. The attacker needs write access, but this gets around review on `main`.

**Reproduction:** read the `if:` at `real-proofs.yml:42`: `(github.event_name != 'push' || startsWith(github.ref, 'refs/tags/')) && vars.REAL_PROOF_SELF_HOSTED == 'true'`.

**Suggested fix:**

- Use ephemeral or just-in-time runners, or wipe `~/.risc0`, `~/.cargo` and `target/` at job start.
- Restrict the job to `refs/heads/main` and protected tags.
- Have the report checker also record `sha256(r0vm)` and compare it with the pinned release.

## RT-06 (Low): `[local]` commits bypass the upstream patch series, and the check still passes

**Component:** `scripts/export-hook-patches.sh:14` skips commits whose subject contains `[local]`. `scripts/check-hook-patches.sh:23-28` replays those same commits onto the pristine tree before its `diff -r` (`:37-38`).

**Failure:** a change to `programs/stablecoin/src/**` (for example `liquidation_hooks.rs`) made in a commit whose subject contains `[local]`:

- reaches the deployed stablecoin guest (guest-repro rebuilds from the tree, so it matches);
- never appears in `patches/stablecoin`, which is what the upstream PR and its reviewers see;
- leaves the hook-patches job green.

**Existing case:** today the only such commit is `0aecfc9` (ADR-035). It touches `programs/stablecoin/methods/guest/src/bin/stablecoin.rs`, the deployed guest's entry point, with a change gated by `#[cfg(feature = "instance-b")]`. That change is benign, but nothing checks that a `[local]` commit stays benign.

**Reproduction (throwaway branch; needs the pristine checkout at `$HOME/lez-spikes/s1/lez-programs`):**

```
git switch -c tmp-r06
echo '// r06' >> programs/stablecoin/src/liquidation_hooks.rs
git commit -qam 'tweak [local]'
scripts/export-hook-patches.sh && scripts/check-hook-patches.sh   # passes; no new patch
git diff --stat HEAD~1 -- patches/                                 # empty
```

**Suggested fix:** let a `[local]` commit touch only an allowlist of paths (`programs/stablecoin/methods_b/**`, the guest `Cargo.toml` `[features]` table, `#[cfg(feature = "instance-b")]` items). The check should fail if one touches `programs/stablecoin/src/**` or `programs/stablecoin/core/src/**`. Alternatively, list the allowed `[local]` SHAs explicitly in the script.

## RT-07 (Info): what the band clamp does at the band edges (ADR-005)

**Component:** `crates/auction_core/src/price.rs:93-97` (`bid_price`), used by `bid.rs:53` and `liquidate.rs:184`; validity at `price.rs:83-89`.

**Observation:**

- `P = clamp(M, R·lo, R·hi)`, while the feed is valid for `M ∈ [R/2, 2R]` and `lo` may be configured as low as 5000 bps (`FeedParams::validate`). Here M and R are collateral per coin. A bid pays `coin·P/(1−d)` collateral.
- **`M < R·lo` (coin below target):**
  - The engine pays more collateral per coin than the market rate.
  - Anyone can buy coin at M, bid it, and receive `R·lo/(1−d)`.
  - Example with lo = 9000, M = 0.6R and d = 2%: 0.918R collateral per coin bought for 0.6R, a gain of about 53% at the opening discount.
  - The extra collateral comes out of the owner's leftover, or becomes a booked shortfall when the lot runs out.
- **`M > R·hi/(1−d_max)` (coin above target):**
  - At hi = 11000 and d_max = 30% that is M > 1.57R, still a valid feed.
  - At every discount, a bid buys less collateral than its coin is worth.
  - The auctions hold their reservations (no expiry, ADR-012) until the market returns, and later liquidations may hit `CapacityExhausted`.

Neither edge is a code defect. Both follow from ADR-005 together with ADR-012, but `THREAT_MODEL.md` states neither.

**Reproduction (core test sketch):** in `crates/auction_core/src/price.rs` tests, use `params()` (lo 9000, hi 11000):

- `bid_price(6 * R / 10, R, &params())` returns `9 * R / 10`, which is above M.
- `bid_price(16 * R / 10, R, &params())` returns `11 * R / 10`. With `RAY - 3000 * BPS_TO_RAY` as the denominator, `11/10 / 0.7 < 16/10`.

**Suggested fix:**

- State both edges in `THREAT_MODEL.md` under research attack 1 or 4.
- Keep `lo` close to 10000 in deployments.
- Have the CLI/GUI show "auction unbiddable at the market price" when `M·(1−d_max) > P`.

## RT-08 (Info): `dev oracle-publish` is three non-atomic transactions; `dev keeper` stops on its first error

**Component:** `cli/src/oracle_cmds.rs:59-66` (`create_oracle_price_account`, then `set_market_price_oracle`, then `set_collateral_config`) and `:97` (`publish(env, a, out)?` inside the keeper loop).

**Failure:**

1. The host rotation lands, but the engine follow-up is dropped or rejected.
2. Every `liquidate`, `bid` and `observe_feed` then reverts with `OracleMismatch` until someone re-runs the command.
3. The keeper exits on that error instead of retrying, so the feed then also goes stale.

The failure is safe (reverts, no funds move) and limited to localnet by the dev guard (ADR-046).

**Reproduction (sketch):** in `cli/tests/tx_golden.rs`, make `FixtureChain` drop the third send, run `dev oracle-publish`, then assert that the host's `market_price_oracle_id` differs from the `CollateralConfig.oracle_id`. Run `scripts/wsl-cargo.sh test -p auction_cli --test tx_golden`.

**Suggested fix:**

- Have `publish` detect a host/engine oracle mismatch first and send only the missing engine update.
- Have the keeper report a failed round and continue, as `watch` does.

## RT-09 (Info): `admin set-engine-params` shows only part of what it signs

**Component:** `cli/src/admin_cmds.rs:287-300`.

**Observation:**

- Before signing, the command prints the protocol-token slot change, the ref-oracle slot change and the ref-price change.
- It does not print changes to:
  - `freeze_on_bad_debt` and `bad_debt_freeze_threshold`, which `THREAT_MODEL.md` lists as host-freeze levers;
  - `global_cap`, `max_mint_per_auction` and the system schedules.
- A wrong or stale params file can lower the freeze threshold without any line saying so.

**Suggested fix:** compute the full `EngineParamsArgs` diff against the verified state and print `field: old -> new` for every field that differs.

## RT-10 (Info): the RPC URL is copied into error messages

**Component:** `crates/auction_sdk/src/rpc.rs:155-167` (`classify_send_error`) and the `call` error paths.

**Observation:** a URL with embedded credentials (`https://user:token@host`) is printed on stderr and in CI logs.

**Suggested fix:** strip the userinfo part (and query parameters) before formatting.

## RT-11 (Info): script hygiene

- **`scripts/demo.sh:48-52`.** `rm -rf "${LN:?}"` runs when the directory is empty, or holds `.demo-run` or `demo-summary.txt` at its top level. A directory that happens to contain a copied `demo-summary.txt` would be wiped. Fix: accept only the `.demo-run` marker, write the repository path into it, and refuse `$HOME`, `/` and the repository root.
- **`scripts/prove-suite.sh:38`.** `pkill -x r0vm` kills every `r0vm` the user owns. That contradicts `localnet.sh:211` ("do not pkill by name"). Fix: kill a recorded pid or the process group.
- **`scripts/build-guests.Dockerfile:14-15`.** The cache-mount ids are shared with the lez-programs build. On a developer machine, a tampered shared registry cache would reach the "reproducible" local build; CI starts with fresh Docker and is unaffected. Fix: use distinct mount ids, or prune before a release build.

## RT-12 (Info): ids skip the keystore's private-account refusal

**Component:** `cli/src/env.rs:74-79`.

**Observation:** `Env::account` parses a base58 id before trying a keystore label, so `Keystore::resolve`'s refusal of private accounts never runs for an id argument. No impact was found: signing still refuses private accounts, and a fresh recipient must sign. It is a consistency gap only.

**Suggested fix:** when the id is in the keystore, apply the same refusal.

## Appendix: cargo audit (2026-10-17)

Tool: cargo-audit in WSL, against the advisory database as fetched on 2026-10-17. Inputs: `Cargo.lock` (832 crate dependencies) and `fuzz/Cargo.lock`. Paths come from `cargo tree --locked -i <crate>@<version>`, which resolves but does not build.

- **Root lock file:** 13 vulnerabilities and 8 allowed warnings (unmaintained, unsound, yanked).
- **Fuzz lock file:** 4 vulnerabilities (`rsa`, `ruint` x2, `tracing-subscriber`) and 4 warnings (`bincode`, `derivative`, `paste`, yanked `chacha20`). All of them are also in the root list.
- **New since the 2026-10-07 list in `THREAT_MODEL.md`** (seven advisories: `ruint` x2, `hickory-proto` x2, `rsa`, `rustls` RUSTSEC-2026-0285, `tracing-subscriber`): six vulnerabilities, namely `quinn-proto` x3 and `rustls` x3 (RUSTSEC-2026-0349, -0348, -0346). Among the warnings, `unsafe-libyaml` is new. `ruint` RUSTSEC-2026-0220 is **not** new: the 2026-10-07 entry already lists it.

### Vulnerabilities

| ID | Crate and version | Dependency path (root workspace) | New? | On chain / CLI signing path? |
|---|---|---|---|---|
| RUSTSEC-2026-0118, -0119 | `hickory-proto` 0.25.0-alpha.5 | `hickory-resolver` > `libp2p-dns` > `libp2p` 0.55 > `logos-blockchain-cryptarchia-sync` / `-libp2p` > `-chain-service` > `-common-http-client` > LEZ `common` > `auction_sdk` > `auction_cli` | no | Not on chain (absent from every guest lock file). Linked into the SDK and CLI through LEZ `common`; the CLI never starts libp2p or DNS, and the signing code does not call it. |
| RUSTSEC-2026-0340, -0338, -0343 | `quinn-proto` 0.11.17 | `quinn` > `libp2p-quic` > `libp2p` (same chain as above) | yes | Same as `hickory-proto`: not on chain; linked via `common`; QUIC is never used by the CLI. |
| RUSTSEC-2023-0071 | `rsa` 0.9.10 (no fix exists) | `rzup` 0.5.2 > `risc0-build` > `risc0-zkvm` > the `*-methods` crates (build time) | no | Not on chain: a build-script dependency of the guest crates, absent from every guest lock file. Not in the CLI signing path (the keystore uses no RSA). |
| RUSTSEC-2026-0349, -0348, -0285, -0346 | `rustls` 0.23.43 | (a) `futures-rustls` > `libp2p-tls` > `libp2p-quic` > `libp2p`; (b) `hyper-rustls` > `jsonrpsee-http-client` > `jsonrpsee` > `sequencer_service_rpc` > `auction_sdk`; (c) `hyper-rustls` > `reqwest` > `bonsai-sdk` > `risc0-zkvm` | -0349, -0348, -0346 yes; -0285 no | Not on chain. Path (b) is the SDK's RPC transport to the sequencer: it matters only for an `https://` endpoint, carries no key material (signing is local), and the endpoint is already a trusted input (THREAT_MODEL, I4). |
| RUSTSEC-2025-0055 | `tracing-subscriber` 0.2.25 | `ark-relations` > `ark-crypto-primitives` > `ark-groth16` > `risc0-groth16` > `risc0-zkvm`; also `logos-blockchain-groth16` > ... > `common` > `auction_sdk` | no | Appears in each guest's `cargo tree` through `risc0-zkvm` > `risc0-groth16`, but no guest initialises a subscriber, and the advisory concerns log output (ANSI escape poisoning). No effect on execution results or signing. |
| RUSTSEC-2025-0137 | `ruint` 1.17.0 (`reciprocal_mg10` unsound) | see the ruint section below | no | Compiled into the guests (see below); `reciprocal_mg10` is not called by any crate of ours. |
| RUSTSEC-2026-0220 | `ruint` 1.17.0 (shift flags) | see the ruint section below | no | Compiled into the guests; no affected call can produce a wrong result (see below). |

### Warnings

| ID | Crate and version | Kind | Dependency path | New? | On chain / signing path? |
|---|---|---|---|---|---|
| RUSTSEC-2025-0141 | `bincode` 1.3.3 | unmaintained | `logos-blockchain-core` > `-blend-message` > `-ledger` > `-chain-service` > `-common-http-client` > `common` | no | Not in a guest. Host side, via `common`. |
| RUSTSEC-2024-0388 | `derivative` 2.2.0 | unmaintained (proc-macro) | `ark-crypto-primitives` > `ark-groth16` > `risc0-groth16` > `risc0-zkvm`; and `logos-blockchain-groth16` | no | Proc-macro only; no runtime code. |
| RUSTSEC-2024-0436 | `paste` 1.0.15 | unmaintained (proc-macro) | `alloy-primitives` > `stablecoin_core`, `twap_oracle_core`, `auction_engine_program`, `auction_sdk`; also `ark-ff` | no | Proc-macro only; no runtime code. |
| RUSTSEC-2024-0370 | `proc-macro-error` 1.0.4 | unmaintained (proc-macro) | `utoipa-gen` > `utoipa` > `logos-blockchain-http-api-common` > `-common-http-client` > `common` | no | Host side only. |
| RUSTSEC-2024-0449 | `unsafe-libyaml` 0.2.11 | unmaintained | `serde_yaml` > `logos-blockchain-utils` > `-blend-crypto` > ... > `common` | yes | Host side only, via `common`; not in a guest. |
| RUSTSEC-2026-0002, -0253 | `lru` 0.12.5 | unsound | `libp2p-swarm` > `libp2p` > `logos-blockchain-cryptarchia-sync` > ... > `common` | the crate was on the 10-07 list; the IDs it carried were not recorded | Not in a guest; the CLI does not use libp2p. |
| (yanked) | `chacha20` 0.10.1 | yanked | `lee_core` > `auction_cli`, `auction_engine_core` (root workspace) | no | The guest lock files pin `chacha20` 0.10.0, which is not yanked; 0.10.1 is in the host-side resolution only. |

### `ruint` RUSTSEC-2026-0220: does it reach on-chain arithmetic?

Short answer: `ruint` 1.17.0 **is compiled into the guests**, but none of the affected shift functions is called in a way the bug can change, and our own on-chain arithmetic does not use `ruint` at all.

1. **Where `ruint` sits.** `cargo tree -i ruint@1.17.0` on `programs/auction_engine/methods/guest` shows two routes into the auction-engine guest: `alloy-primitives` (through `auction_engine_program`, `stablecoin_core` and `twap_oracle_core`, plus `uniswap_v3_math`) and `risc0-binfmt` (through `risc0-zkvm`, `lee_core`, `clock_core`, `spel-framework`). The stablecoin, token and test-program guests resolve `ruint` 1.17.0 too (their lock files pin `=1.17.0` for the guest toolchain's MSRV). `auction_core` lists `ruint` only under `[dev-dependencies]`, used inside `#[cfg(test)]` as the reference oracle of the property tests. Its production arithmetic is the crate's own `mul_div_*` and `div_wide` on `u128`.
2. **Affected API.** The advisory lists the `overflowing_`, `checked_`, `saturating_` and `wrapping_` forms of `shl` and `shr` below 1.20.0. The defects are false-negative overflow flags (the shifted value is right, only the flag is wrong) and a shift amount truncated modulo 2^32 for `wrapping_*` on 64, 128 and 256-bit types.
3. **Our uses.** A search for `checked_sh*`, `wrapping_sh*`, `saturating_sh*`, `overflowing_sh*`, `strict_sh*` and shift operators on `U256`, `U512` and `I256` across `programs/auction_engine`, `programs/stablecoin`, `crates/auction_sdk` and `cli` finds none. The engine, `stablecoin_core` and the hooks use `alloy_primitives::U256` and `U512` only for `checked_mul`, `checked_div`, `saturating_mul`, comparison and `from`; the shifts in `compound_rate` are on `u64`. The only callers are in `programs/twap_oracle/core/src/lib.rs`, in `price_to_tick`. The engine does not call it (it imports only the `OraclePriceAccount` type); it runs in the upstream twap_oracle program:
   - `U256::from(price).checked_shl(128)` with `price < 2^128`: no bit is lost, so the `Some` result is correct;
   - `U256::ONE.checked_shl(start)` with `start < 256`: a single set bit, no loss;
   - `wrapping_shr(1)` and `wrapping_shr(2)`: tiny constant amounts, and the logical drop of low bits is intended.
4. **Dependencies' uses.** `uniswap_v3_math` 0.6.2 (`get_tick_at_sqrt_ratio`, reached only from `price_to_tick`) uses the `>>` and `<<` operators with constant amounts of at most 128 in the paths `price_to_tick` reaches. The operators do not read the overflow flag, and the amounts are far below 2^32, so neither defect applies. The `risc0-binfmt` use is internal to risc0 and not reachable with attacker-chosen shift amounts from our code; it was not audited beyond the dependency graph.
5. **Impact class if it were reached.** The advisory's category is denial of service (a `to_base_be` loop on no-alloc builds, wrong checked results). A misbehaving guest run would fail to prove or revert; it cannot move funds by itself. The CLI signing path calls none of these functions.

Conclusion: no advisory reaches the guests or the CLI's signing path in a way that can change a result. The `ruint` pair stays on the existing watch list. The fix needs `ruint` >= 1.20.0, which the guest pins (`=1.17.0`, set for the guest toolchain's MSRV) block until the LEZ and risc0 pins move.
