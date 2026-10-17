# Deployments

## Localnet (Plan 3a CLI smoke, standalone LEZ v0.2.4 sequencer, dev-mode proofs)

| Program | ImageID | Source |
|---|---|---|
| token | `14b3bc6cc129d0359f9595ba0f60a13e4a8df08ffe8eb1a83251c47abecc67a8` | lez-programs 64c01210 Docker build (S1) |
| twap_oracle | `5681e27c27b3ece4a7c161bbf8e12e6ece6ca69925a87b77f9e193fa4a249b31` | same |
| stablecoin (+ hooks) | `bda698c4e929e4f63ad7b2949c8e413c26fc69df02434844067baded95c7b40d` | `artifacts/guests` (Docker, ADR-047) |
| auction_engine | `bb506524bfab7ce5d45a4f4836c00a90d7f23a769a2b6c988c66eb82d456f342` | `artifacts/guests` (Docker, ADR-047) |

All five ImageIDs come from reproducible Docker builds (ADR-047): the binaries are committed in
`artifacts/guests/` with their sha256 and ImageIDs in `artifacts/guests/manifest.json`, and CI job
`guest-repro` rebuilds our two guests from source on every push and pull request touching their inputs and requires identical bytes (the three upstream guests are rebuilt from the pinned 64c01210 source only on the scheduled (weekly) and manual runs; on push and pull request they are only hash-checked against the manifest), and checks every committed binary against the manifest. Anyone can rebuild with
`scripts/build-guests.sh` (our two guests) and lez-programs `make build-programs` at 64c01210 (token,
twap_oracle, token_mint_authority) and compare with `scripts/check-guests.sh`. The in-process test suites
run natively built ELFs of the same source, whose ImageIDs differ; they are never deployed.

Clients must compare the `host_program_id` and `token_program_id` stored in an engine config against this table
before binding or bidding (threat model item I4); `auction-cli admin verify` prints them, and `auction-cli host bind` checks them before binding.

## Legacy testnet (public, LEZ v0.2.4-lez-legacy, 2026-10-17)

Node `https://testnet-legacy.lez.logos.co/`, block 1 `c8e3bb9ec8f246e5c2759e0e8152840aeb7df29948a9ad12c9c302a641230011`.
Deployed by `scripts/testnet-legacy.sh` (run `run-20261017T171726Z`, `docs/architecture/testnet-legacy.md`;
evidence in `docs/evidence/testnet-legacy-2026-10-17/`). Same ImageIDs as the table above.

| Program | ImageID | Block | Deployed by |
|---|---|---|---|
| token | `14b3bc6cc129d0359f9595ba0f60a13e4a8df08ffe8eb1a83251c47abecc67a8` | 2301 | already on the testnet |
| twap_oracle | `5681e27c27b3ece4a7c161bbf8e12e6ece6ca69925a87b77f9e193fa4a249b31` | 2308 | already on the testnet |
| token_mint_authority | `1ea6d932d240e54e53682547e6d9f08a03a42bd9bbaa8e6b4a9974fc86d1e70e` | 2298 | already on the testnet |
| stablecoin (+ hooks) | `bda698c4e929e4f63ad7b2949c8e413c26fc69df02434844067baded95c7b40d` | 5280 | this PoC, transaction `aa1ad271654084f2a98eaba5fcae71e26c12f8a807657e92dd367758c6eb387f` |
| auction_engine | `bb506524bfab7ce5d45a4f4836c00a90d7f23a769a2b6c988c66eb82d456f342` | 5281 | this PoC, transaction `9f5bd3455ac87cc1c399ef3d3d31dc421b5135308e72d03dbd4e178d033dc2e7` |

| Account | Id |
|---|---|
| Engine config `["CONFIG", admin, 1]` | `Bg3nHnQjPtzngmXBxQ7Zby3sSXqj5Jk9yn8Edmnh7mNS` |
| Engine admin | `7JyKg2s8d7KupBSqSn3kMjmsdTK8w1GhEvgTLVvWH14L` |
| Host `ProtocolParameters` | `H7XSZV2ksnQW3sHwMqCUBg5nQfgPntd6K8ijQnXStKNW` (host admin `HeWbN5chqf4dkQT6raegpCTAVvSqXA9rzEpYVKgthsCd`) |

The host is a single instance per chain and is now claimed by this run's admin key. The config's protocol token
is a mock (`dev token-new`, mint authority the engine PDA) and the market oracle is the run's own price account
(ADR-050); after the freeze path the protocol token is disabled (`protocol token none`). The legacy testnet may
be reset or retired.

Testnet 0.4, the target of rfp #214: milestone M6, after the port (`docs/v03-port-plan.md`).

## Reference price changes (ADR-017)

Every change of `protocol_token_ref_price` is recorded here: date, config, old → new, reason.

| Date | Config | Old → New | Reason |
|---|---|---|---|
| 2026-10-07 | localnet smoke `init_config` | none → `2_000_000_000_000_000_000_000_000_000` (2e27 RAY) | initial value passed to `init_config` by `scripts/localnet-smoke.sh` (`step_engine`, `--protocol-token-ref-price`) |
| 2026-10-12 | localnet smoke (Plan 3a) init_config | none → `2_000_000_000_000_000_000_000_000_000` (2e27 RAY) | initial value from `cli/params/localnet-engine.json` (step_engine) |
| 2026-10-17 | legacy testnet `Bg3nHnQjPtzngmXBxQ7Zby3sSXqj5Jk9yn8Edmnh7mNS` init_config | none → `2_000_000_000_000_000_000_000_000_000` (2e27 RAY) | initial value from `cli/params/demo-engine.json` (`scripts/testnet-legacy.sh`, transaction `89551d16f507a291a8de2204f3bba4957f9612f56ffac0a1c5b99b7a64a10ed6`); the freeze path's `set_engine_params` re-sent the same file and kept 2e27 |
