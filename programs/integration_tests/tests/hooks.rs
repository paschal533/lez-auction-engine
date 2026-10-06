mod common;

use common::*;
use lee_core::{account::AccountId, program::ProgramId};
use stablecoin_core::{LiquidationEngineState, ProtocolParameters};

const TEST_ENGINE: ProgramId = [0xE1; 8];

fn test_authority() -> AccountId {
    AccountId::new([0xEA; 32])
}

#[test]
fn host_admin_binds_engine_through_the_guest() {
    let mut w = World::new();
    w.bind_engine_to(TEST_ENGINE, test_authority())
        .expect("set_liquidation_engine");
    let s: LiquidationEngineState = w.read(les());
    assert_eq!(
        (s.engine_program_id, s.engine_authority_id),
        (TEST_ENGINE, test_authority())
    );
}

#[test]
fn non_admin_bind_is_rejected_through_the_guest() {
    let mut w = World::new();
    let r = w.host(
        vec![id(k::ATTACKER), protocol_parameters(), les()],
        &[k::ATTACKER],
        stablecoin_core::Instruction::SetLiquidationEngine {
            engine_program_id: TEST_ENGINE,
            engine_authority_id: id(k::ATTACKER),
        },
    );
    assert_err(r, "Signer is not the protocol's admin");
}

#[test]
fn direct_engine_freeze_is_rejected_through_the_guest() {
    let mut w = World::new();
    w.bind_engine_to(TEST_ENGINE, test_authority()).unwrap();
    let r = w.host(
        vec![id(k::ATTACKER), les(), protocol_parameters()],
        &[k::ATTACKER],
        stablecoin_core::Instruction::EngineFreezeNewDebt,
    );
    assert_err(r, "Caller program is not the liquidation engine");
    let p: ProtocolParameters = w.read(protocol_parameters());
    assert!(!p.is_frozen);
}
