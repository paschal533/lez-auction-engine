//! Program-id verification (I4, ADR-027): no transaction is built for a config that this module does
//! not accept. The engine chains to the program ids stored in its config with the user's signer flags,
//! so a config built on a fake host or token program must be refused client-side.

use auction_engine_core::{
    pda::{self, SLOT_PROTOCOL_TOKEN},
    state::{CollateralConfigState, EngineConfigState},
};
use lee_core::account::{Account, AccountId};
use stablecoin_core::LiquidationEngineState;

use crate::{
    addresses::{EngineAddresses, HostAddresses},
    chain::Chain,
    decode::Reader,
    deployment::{program_id_hex, Deployment},
    error::{SdkError, SdkResult},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedConfig {
    pub config_id: AccountId,
    pub state: EngineConfigState,
    pub engine: EngineAddresses,
    pub host: HostAddresses,
}

fn mismatch(what: &'static str, expected: String, found: String) -> SdkError {
    SdkError::Mismatch {
        what,
        expected,
        found,
    }
}

pub fn verify_config<C: Chain + ?Sized>(
    chain: &C,
    d: &Deployment,
    config_id: &AccountId,
) -> SdkResult<VerifiedConfig> {
    let acc = chain.account(config_id)?;
    if acc == Account::default() {
        return Err(SdkError::Missing {
            what: "engine config",
            id: config_id.to_string(),
        });
    }
    if acc.program_owner != d.engine_program {
        return Err(mismatch(
            "engine program (owner of the config account)",
            program_id_hex(&d.engine_program),
            program_id_hex(&acc.program_owner),
        ));
    }
    let state = EngineConfigState::try_from(&acc.data).map_err(|e| SdkError::Decode {
        what: "EngineConfig",
        id: config_id.to_string(),
        reason: e.to_string(),
    })?;
    let expected = pda::config_pda(&d.engine_program, &state.admin, state.salt);
    if expected != *config_id {
        return Err(mismatch(
            "config address [CONFIG, admin, salt]",
            expected.to_string(),
            config_id.to_string(),
        ));
    }
    if state.host_program_id != d.host_program {
        return Err(mismatch(
            "host program id stored in the config",
            program_id_hex(&d.host_program),
            program_id_hex(&state.host_program_id),
        ));
    }
    if state.token_program_id != d.token_program {
        return Err(mismatch(
            "token program id stored in the config",
            program_id_hex(&d.token_program),
            program_id_hex(&state.token_program_id),
        ));
    }
    if let Some(admin) = d.engine_admin {
        if state.admin != admin {
            return Err(mismatch(
                "engine admin",
                admin.to_string(),
                state.admin.to_string(),
            ));
        }
    }
    let host = HostAddresses::new(d.host_program);
    if state.host_protocol_parameters_id != host.protocol_parameters {
        return Err(mismatch(
            "host ProtocolParameters id stored in the config",
            host.protocol_parameters.to_string(),
            state.host_protocol_parameters_id.to_string(),
        ));
    }
    if state.stablecoin_definition_id != host.stablecoin_definition {
        return Err(mismatch(
            "stablecoin definition id stored in the config",
            host.stablecoin_definition.to_string(),
            state.stablecoin_definition_id.to_string(),
        ));
    }
    let pp = chain.account(&host.protocol_parameters)?;
    if pp.program_owner != d.host_program {
        return Err(mismatch(
            "owner of the host ProtocolParameters (wrong network?)",
            program_id_hex(&d.host_program),
            program_id_hex(&pp.program_owner),
        ));
    }
    Ok(VerifiedConfig {
        config_id: *config_id,
        engine: EngineAddresses::for_config(d.engine_program, *config_id),
        host,
        state,
    })
}

/// The host binds exactly this config's engine authority (checked before `liquidate`; a host bound to
/// another config would reject the hook anyway, but the user learns why before signing).
pub fn verify_binding<C: Chain + ?Sized>(chain: &C, v: &VerifiedConfig) -> SdkResult<()> {
    let les: LiquidationEngineState = Reader::new(chain, v.engine.program).host(
        &v.host.liquidation_engine_state,
        &v.host.program,
        "LiquidationEngineState",
    )?;
    if les.engine_program_id != v.engine.program {
        return Err(mismatch(
            "engine program bound by the host",
            program_id_hex(&v.engine.program),
            program_id_hex(&les.engine_program_id),
        ));
    }
    if les.engine_authority_id != v.engine.engine_authority {
        return Err(mismatch(
            "engine authority bound by the host (another config is bound)",
            v.engine.engine_authority.to_string(),
            les.engine_authority_id.to_string(),
        ));
    }
    Ok(())
}

impl VerifiedConfig {
    /// Slot account to pass (ADR-030): the configured id, or `["UNUSED", config, slot]`.
    #[must_use]
    pub fn slot(&self, slot: u64) -> AccountId {
        if slot == SLOT_PROTOCOL_TOKEN {
            self.state
                .protocol_token
                .unwrap_or(self.engine.unused_protocol_token)
        } else {
            self.state
                .protocol_token_oracle_id
                .unwrap_or(self.engine.unused_ref_oracle)
        }
    }

    #[must_use]
    pub fn coll_config_id(&self) -> AccountId {
        self.engine
            .coll_config(&self.state.collateral_definition_id)
    }

    pub fn coll<C: Chain + ?Sized>(&self, chain: &C) -> SdkResult<CollateralConfigState> {
        Reader::new(chain, self.engine.program).coll_config(&self.coll_config_id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::id;
    use crate::testing::standard::{self, k, ENGINE, HOST};

    fn what_of(e: SdkError) -> String {
        match e {
            SdkError::Mismatch { what, .. } => what.to_owned(),
            SdkError::Missing { what, .. } => format!("missing {what}"),
            other => panic!("unexpected {other}"),
        }
    }

    #[test]
    fn verify_config_accepts_the_standard_world() {
        let fx = standard::standard();
        let v = verify_config(&fx.chain, &fx.deployment, &standard::config_id()).expect("verifies");
        assert_eq!(v.engine.program, ENGINE);
        assert_eq!(v.host.program, HOST);
        verify_binding(&fx.chain, &v).expect("the host binds this config");
    }

    #[test]
    fn verify_config_rejects_every_mismatch() {
        let fx = standard::standard();
        let cfg = standard::config_id();
        let cases: Vec<(&str, Deployment)> = vec![
            (
                "engine program",
                Deployment {
                    engine_program: [0xE2; 8],
                    ..fx.deployment.clone()
                },
            ),
            (
                "host program id",
                Deployment {
                    host_program: [0xA2; 8],
                    ..fx.deployment.clone()
                },
            ),
            (
                "token program id",
                Deployment {
                    token_program: [0xB2; 8],
                    ..fx.deployment.clone()
                },
            ),
            (
                "engine admin",
                Deployment {
                    engine_admin: Some(id(k::KEEPER)),
                    ..fx.deployment.clone()
                },
            ),
        ];
        for (needle, d) in cases {
            let e = verify_config(&fx.chain, &d, &cfg).expect_err(needle);
            assert!(what_of(e).contains(needle), "{needle}");
        }
        // The config account copied to another address is not the PDA of its own admin and salt.
        let elsewhere = id(k::CALLER);
        fx.chain
            .put(elsewhere, fx.chain.account(&cfg).expect("config"));
        let e = verify_config(&fx.chain, &fx.deployment, &elsewhere).expect_err("address");
        assert!(what_of(e).contains("config address"));
        // Wrong network: the host's ProtocolParameters is owned by another program there.
        let pp = fx.chain.account(&fx.host.protocol_parameters).expect("pp");
        fx.chain.put(
            fx.host.protocol_parameters,
            Account {
                program_owner: [0xEE; 8],
                ..pp
            },
        );
        let e = verify_config(&fx.chain, &fx.deployment, &cfg).expect_err("network");
        assert!(what_of(e).contains("ProtocolParameters"));
        let e = verify_config(&fx.chain, &fx.deployment, &id(99)).expect_err("missing");
        assert!(what_of(e).contains("missing engine config"));
    }

    #[test]
    fn verify_config_rejects_foreign_ids_stored_in_the_config() {
        let fx = standard::standard();
        let cfg = standard::config_id();
        let good = fx.chain.account(&cfg).expect("config");
        let state = EngineConfigState::try_from(&good.data).expect("decodes");
        let cases = [
            (
                "ProtocolParameters id",
                EngineConfigState {
                    host_protocol_parameters_id: id(k::KEEPER),
                    ..state.clone()
                },
            ),
            (
                "stablecoin definition id",
                EngineConfigState {
                    stablecoin_definition_id: id(k::KEEPER),
                    ..state.clone()
                },
            ),
        ];
        for (needle, bad) in cases {
            fx.chain.put_state(cfg, ENGINE, &bad);
            let e = verify_config(&fx.chain, &fx.deployment, &cfg).expect_err(needle);
            assert!(what_of(e).contains(needle), "{needle}");
        }
        // Undecodable config data (wrong tag) is a decode error, not a pass.
        fx.chain.put_state(cfg, ENGINE, &state.clone());
        let acct = fx.engine.accounting;
        let pp = fx.chain.account(&acct).expect("accounting");
        fx.chain.put(
            cfg,
            Account {
                program_owner: ENGINE,
                ..pp
            },
        );
        let e = verify_config(&fx.chain, &fx.deployment, &cfg).expect_err("tag");
        assert!(matches!(e, SdkError::Decode { .. }));
    }

    #[test]
    fn verify_binding_rejects_a_host_bound_to_another_config() {
        let fx = standard::standard();
        let v = verify_config(&fx.chain, &fx.deployment, &standard::config_id()).expect("verifies");
        fx.chain.put_state(
            v.host.liquidation_engine_state,
            HOST,
            &stablecoin_core::LiquidationEngineState {
                engine_program_id: ENGINE,
                engine_authority_id: id(k::KEEPER),
            },
        );
        let e = verify_binding(&fx.chain, &v).expect_err("bound elsewhere");
        assert!(what_of(e).contains("engine authority"));
        fx.chain.put_state(
            v.host.liquidation_engine_state,
            HOST,
            &stablecoin_core::LiquidationEngineState {
                engine_program_id: [0xEE; 8],
                engine_authority_id: v.engine.engine_authority,
            },
        );
        let e = verify_binding(&fx.chain, &v).expect_err("another engine program");
        assert!(what_of(e).contains("engine program bound"));
    }

    #[test]
    fn localnet_json_matches_deployments_md() {
        let d = Deployment::from_json(
            include_str!("../../../deployments/localnet.json"),
            "deployments/localnet.json",
        )
        .expect("parses");
        let md = include_str!("../../../DEPLOYMENTS.md");
        let twap = d
            .twap_oracle_program
            .expect("localnet.json names the twap oracle");
        for (name, id) in [
            ("token", d.token_program),
            ("twap_oracle", twap),
            ("stablecoin (+ hooks)", d.host_program),
            ("auction_engine", d.engine_program),
        ] {
            let row = format!("| {name} | `{}` |", program_id_hex(&id));
            assert!(
                md.contains(&row),
                "DEPLOYMENTS.md has no row starting {row}"
            );
        }
    }
}
