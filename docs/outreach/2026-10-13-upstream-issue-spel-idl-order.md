# Draft issue (for logos-co/spel; cross-link from logos-blockchain/lez-programs)

**Status:** SUPERSEDED, do not file. Upstream fixed it independently: spel #288 (closed) and commits `d58984a4`,
`adb46c66` on spel `main` order instructions by the enum. The fix is after v0.7.0 (the last release for LEZ v0.2.4),
so this PoC keeps its own enum-index encoders (ADR-040). Kept as the record of the finding.

---

**Title:** spel-cli encodes an instruction by its IDL position, which differs from the enum index when guest
functions are not in enum order (stablecoin positions 8-18)

**Body**

`spel-cli` (rev 1ef0500f, `spel-cli/src/tx.rs`) encodes an instruction as
`idl.instructions.iter().position(...)`. The guest decodes the risc0-serde enum, whose first word is the
**enum index**. The IDL lists instructions in guest-function order. In lez-programs 64c01210 the stablecoin's
guest functions are not in enum order from position 8 on, so spel-cli sends a different instruction than the
user asked for.

Observed on a v0.2.4 localnet: `set_minimum_collateralization_ratio` (enum 8, IDL position 11) sent with
spel-cli executed `set_timing_parameters` (enum 11) and failed with
"new_minimum_milliseconds_between_rate_updates above maximum" (our transaction `7953b4f3...`, recorded in
`docs/architecture/localnet.md`, finding 1).

Full mapping (IDL position -> enum index), checked by a test in our repository
(`stablecoin_idl_position_maps_to_enum_index`):

| IDL position | Instruction | Enum index |
|---|---|---|
| 0-7 | initialize_program ... close_position | 0-7 |
| 8 | withdraw_collateral | 17 |
| 9 | repay_debt | 18 |
| 10 | set_stability_fee_per_millisecond | 14 |
| 11 | set_minimum_collateralization_ratio | 8 |
| 12 | set_controller_gains | 9 |
| 13 | set_timing_parameters | 11 |
| 14 | set_admin | 12 |
| 15 | set_freeze_authority | 13 |
| 16 | set_market_price_oracle | 10 |
| 17 | freeze | 15 |
| 18 | unfreeze | 16 |

Possible fixes: (a) spel emits the enum index into the IDL (`"discriminant": n`) and spel-cli encodes that;
(b) the IDL generator orders instructions by enum index; (c) lez-programs reorders the stablecoin's guest
functions to enum order. We worked around it with our own encoders (enum index from the program's own enum).

Related, from the same work: spel-cli cannot encode `i128` arguments (the stablecoin's controller gains), and
it cannot resolve a PDA seeded by another PDA of the same instruction (`init_config`, `liquidate` in our engine;
`docs/architecture/localnet.md`, finding 3).

<author>
