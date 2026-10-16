# RFP-014 liquidation and auction engine: PoC submission

Repository: this repository, at the commit tagged `rfp014-poc`. The tag does not exist yet: the author creates
it at submission (`git rev-parse rfp014-poc` then gives the full hash). `scripts/submission-hashes.sh`
reproduces every hash below from that commit. Until the tag exists, the work is on the branch `feat/plan-3b`,
which has not been merged to `main`.

## What is built
- Engine, stablecoin hooks, F4 debt auction and F5 surplus auction with a mock protocol token
  (`docs/architecture/programs.md`, `docs/architecture/hooks.md`).
- `auction_sdk` and `auction-cli` with their own enum-index encoders (`docs/architecture/sdk.md`, `cli/README.md`).
- CI workflows: core, programs, IDL drift, hook patches, coverage, requirements; real-proof tiers (ADR-045);
  reproducible guests (ADR-047); fuzzing; and a localnet end-to-end workflow whose `RISC0_DEV_MODE=0` job runs
  the demo (`docs/architecture/ci.md`). Only the core workflow has run on a hosted runner so far; the others
  are pending (see Honest notes).
- Requirements map: `REQUIREMENTS.md`. Threat model and red-team pass: `THREAT_MODEL.md`,
  `docs/security/redteam-2026-10.md`. Cycles: `CYCLES.md`. Real proofs: `docs/architecture/real-proofs.md`,
  `docs/evidence/real-proofs-2026-10.md`.

## Reproduce
`DEMO.md`: `scripts/bootstrap-lez.sh` once, then `scripts/demo.sh` (about 16 minutes: 971 s measured, warm
build, dev-mode sequencer; 973 s with `--sequencer-dev-mode 0`). Deviation from spec §1.1: the prerequisites
are documented for Ubuntu (apt, rustup, rzup) instead of a Nix flake. It has been run on Ubuntu 24.04 under WSL
only; no hosted-runner run has happened yet.

## Demo recording
Not recorded yet. To be recorded by the author (not AI-narrated); the link is added by the author at
submission. No recording exists in this repository.

## Hashes
## Pins
- LEZ v0.2.4 47eba256479f6f785acbd138834340703cd03401; SPEL 1ef0500f8fc8ce3ddf95523726ae159db83f744f; risc0-zkvm 3.0.5; Rust 1.94.0; lez-programs 64c01210adecd6b557dbd021a3cc94553f243fd8

## Deployable guests (artifacts/guests/manifest.json)
- builder: `risczero/risc0-guest-builder:r0.1.88.0@sha256:3e12f71bacd27527a61dea96fa0e53e468c99aa261d3a1019b593f6dbd943eb3`
- auction_engine: ImageID `bb506524bfab7ce5d45a4f4836c00a90d7f23a769a2b6c988c66eb82d456f342`, sha256 `991779d757f089ed7acdb1f3df5698f62b27c286b86720cbc3791fc0f029a3bb`
- stablecoin: ImageID `bda698c4e929e4f63ad7b2949c8e413c26fc69df02434844067baded95c7b40d`, sha256 `9188efcce3ae239408119492d1b70a7424c938ee43892256b303bf5293062ee4`
- token: ImageID `14b3bc6cc129d0359f9595ba0f60a13e4a8df08ffe8eb1a83251c47abecc67a8`, sha256 `bc8c24bddefc00628a7f659fe2d04dd074868250167a0334b0e6ae5c13e56bc9`
- token_mint_authority: ImageID `1ea6d932d240e54e53682547e6d9f08a03a42bd9bbaa8e6b4a9974fc86d1e70e`, sha256 `8ee775b631d5243849e227a701f603a61217497b0724e42d3be135f784fddbd2`
- twap_oracle: ImageID `5681e27c27b3ece4a7c161bbf8e12e6ece6ca69925a87b77f9e193fa4a249b31`, sha256 `53c1d995d72c42226fc41a8bbd8a042667ed91a0add0db4c18a68468db809dd5`

## Stablecoin hook patch series (patches/stablecoin)
- 0001-feat-stablecoin-engine-binding-and-Variant-C-freeze-.patch: sha256 `854a192b453990937d5522077e9e69108aeabaae2eaecd083f7372b78bc6229f`
- 0002-fix-stablecoin-test-setter-PDA-owner-guards-IDL-path.patch: sha256 `b601d0cd99caeb0c406e480ab62492661a013560f5339030e4b45319c63f7e96`
- 0003-feat-stablecoin-liquidate_position-hook-with-proport.patch: sha256 `57a6a96113e735e0a93fdb41c140c3b1fcf0bac81c1c678d171b6f57563f2665`
- 0004-fix-stablecoin-round-the-proportional-seize-bound-do.patch: sha256 `a65c6e22d9b73b2126f33ab0f01ffcbaa3ddb1ce6af161081b5bb31852c6d41e`

## Evidence files
- REQUIREMENTS.md: sha256 `ba671ade9aacad1a7f9d97f5c2b9914cb74d09b7394b5c3dc19e150f6d1531ca`
- CYCLES.md: sha256 `66845470e5156378cfda0fda7e200d630b01c98a662a35b9bf27af17e8bbcbfc`
- cycles/baseline.json: sha256 `2ba4bdff260fa849bb132a6a38835727fe5c2ba583d40540eeadc7940d15fdc8`
- DEMO.md: sha256 `9a9831293c5d7cd668cee29675f8353356eb1f9e8adbead122462dcf4a6c821b`
- THREAT_MODEL.md: sha256 `eb924d92172c179a8b72feaca4a32a27269fcde9939f198112c8e17856641095`
- docs/security/redteam-2026-10.md: sha256 `e7bdc5c9ed2243e53e29b8daae91b91a9862f7cbc8f868d9ae524dad4450bbdf`
- docs/architecture/real-proofs.md: sha256 `8a3c8c5cf52779efc8f32916e8f7895293d912dbd303b306c19cae54684d718b`
- docs/evidence/real-proofs-2026-10.md: sha256 `db1dbaaa0b7d51c2958ad3a94927d301328adea5ef6f3523ca2222ec0d7c535e`

## Honest notes
- LEZ v0.2.4 executes public transactions without proving them. A `RISC0_DEV_MODE=0` sequencer therefore does
  not prove them either; real receipts are produced separately for every execution of each instruction
  (ADR-045). The `RISC0_DEV_MODE=0` demo ran locally, not on a hosted runner.
- Real proofs: the full ADR-036 suite was proved locally (42 executions, all receipts verified;
  `docs/evidence/real-proofs-2026-10.md`). The privacy circuit (`prove_ppe_observe_feed`) has never been run.
  The hosted tiers (push tier B, nightly tier C) have not run: the workflows are not on `main`, and tier C
  needs a self-hosted runner that is off by default.
- The localnet end-to-end workflow (`e2e.yml`) has never run on a hosted runner.
- Branch `feat/plan-3b` is not merged to `main`, and the `rfp014-poc` tag does not exist.
- Rows marked `PoC (partial)` in `REQUIREMENTS.md` say in their notes what is missing.
- Red-team pass: no Critical, High or Medium findings; six Low and six Info, all off-chain, with residuals in
  `THREAT_MODEL.md`. `cargo audit` reports advisories in the dependency tree; each is traced in the appendix of
  `docs/security/redteam-2026-10.md`.
- The F4 freeze triggers above an absolute threshold (spec §5.11, `REQUIREMENTS.md` F4).
- R2: no live TWAP until the oracle scale is settled (`docs/outreach/2026-10-13-upstream-issue-oracle-scale.md`).
- Privacy: only a stub exists (`docs/PRIVACY.md`); the full document is milestone M6.
- Testnet deployment (S1) is blocked by v0.3 (`docs/v03-port-plan.md`).
- Upstream drafts, none filed, offered or posted: `docs/outreach/2026-10-13-*.md` (two issues, the hook pull
  request, a status note). The author decides what to send.
- The hashes above are of the working tree when `scripts/submission-hashes.sh` was run; rerun the script after
  any later change to a hashed file.
