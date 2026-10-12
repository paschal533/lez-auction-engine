# Deployments

## Localnet (Plan 3a CLI smoke, standalone LEZ v0.2.4 sequencer, dev-mode proofs)

| Program | ImageID | Source |
|---|---|---|
| token | `14b3bc6cc129d0359f9595ba0f60a13e4a8df08ffe8eb1a83251c47abecc67a8` | lez-programs 64c01210 Docker build (S1) |
| twap_oracle | `5681e27c27b3ece4a7c161bbf8e12e6ece6ca69925a87b77f9e193fa4a249b31` | same |
| stablecoin (+ hooks) | `91c790a23cadc2aa72be1533a4f4daa8ab6734e0dad5165fb9303cb438736953` | this repo, `stablecoin_methods::STABLECOIN_ELF` (native risc0 build) |
| auction_engine | `de82967436371542ea2aa862b967af1776ef3fa3f4c5ae5033ea30f27253fc68` | this repo, `auction_engine_methods::AUCTION_ENGINE_ELF` (native risc0 build) |

**The stablecoin and auction_engine ImageIDs come from a NATIVE build (local risc0 toolchain in WSL), not from the
reproducible Docker build. They are not reproducible across machines**: the same source can give a different ImageID
on another machine or toolchain state. Until Plan 3b produces reproducible Docker builds, treat these two values as
the record of what ran in the Plan 3a smoke only, not as ids a client can compare a rebuild against. The token and
twap_oracle ids are the upstream Docker-built ones.

Clients must compare the `host_program_id` and `token_program_id` stored in an engine config against this table
before binding or bidding (threat model item I4); `auction-cli admin verify` prints them, and `auction-cli host bind` checks them before binding.

Reproducible (Docker) builds and testnet deployment: Plan 3b / milestone M6.

## Reference price changes (ADR-017)

Every change of `protocol_token_ref_price` is recorded here: date, config, old → new, reason.

| Date | Config | Old → New | Reason |
|---|---|---|---|
| 2026-10-07 | localnet smoke `init_config` | none → `2_000_000_000_000_000_000_000_000_000` (2e27 RAY) | initial value passed to `init_config` by `scripts/localnet-smoke.sh` (`step_engine`, `--protocol-token-ref-price`) |
| 2026-10-12 | localnet smoke (Plan 3a) init_config | none → `2_000_000_000_000_000_000_000_000_000` (2e27 RAY) | initial value from `cli/params/localnet-engine.json` (step_engine) |
