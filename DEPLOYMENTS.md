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
`guest-repro` rebuilds them from source and requires identical bytes. Anyone can rebuild with
`scripts/build-guests.sh` (our two guests) and lez-programs `make build-programs` at 64c01210 (token,
twap_oracle, token_mint_authority) and compare with `scripts/check-guests.sh`. The in-process test suites
run natively built ELFs of the same source, whose ImageIDs differ; they are never deployed.

Clients must compare the `host_program_id` and `token_program_id` stored in an engine config against this table
before binding or bidding (threat model item I4); `auction-cli admin verify` prints them, and `auction-cli host bind` checks them before binding.

Testnet deployment: Plan 3b / milestone M6.

## Reference price changes (ADR-017)

Every change of `protocol_token_ref_price` is recorded here: date, config, old → new, reason.

| Date | Config | Old → New | Reason |
|---|---|---|---|
| 2026-10-07 | localnet smoke `init_config` | none → `2_000_000_000_000_000_000_000_000_000` (2e27 RAY) | initial value passed to `init_config` by `scripts/localnet-smoke.sh` (`step_engine`, `--protocol-token-ref-price`) |
| 2026-10-12 | localnet smoke (Plan 3a) init_config | none → `2_000_000_000_000_000_000_000_000_000` (2e27 RAY) | initial value from `cli/params/localnet-engine.json` (step_engine) |
