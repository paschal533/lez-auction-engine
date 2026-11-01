use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{
    account::Actor,
    program::{ReceiveInput, Response},
};

// S9 spike (lez-auction-engine): a minimal CDP host on LEZ 0.4.
//
// `Init` binds the engine actor (account and program) and sets the collateral.
// `Seize` is accepted only from exactly that engine actor; it lowers the collateral and replies
// `Seized { amount }` to the sender, the 0.4 stand-in for a return value.

#[derive(BorshSerialize, BorshDeserialize)]
pub enum HostMsg {
    Init { engine: Actor, collateral: u64 },
    Seize { amount: u64 },
}

#[derive(BorshSerialize, BorshDeserialize)]
struct Host {
    engine: Actor,
    collateral: u64,
}

// Engine message `Seized` (s9_engine.rs `EngineMsg` variant 2).
#[derive(BorshSerialize)]
enum EngineMsg {
    _Init,
    _Liquidate,
    Seized { amount: u64 },
}

lee_core::actor_entrypoint!(handle_message);

fn handle_message(input: &ReceiveInput, message: HostMsg) -> Response {
    match message {
        HostMsg::Init { engine, collateral } => {
            assert!(input.is_authorized, "init: the host account must sign");
            assert!(input.pre_state.as_ref().is_empty(), "init: already initialised");
            Response::set_state(borsh::to_vec(&Host { engine, collateral }).expect("encodes"))
        }
        HostMsg::Seize { amount } => {
            let mut host = Host::try_from_slice(input.pre_state.as_ref()).expect("host state");
            assert_eq!(input.from, Some(host.engine), "seize: not from the bound engine actor");
            host.collateral = host.collateral.checked_sub(amount).expect("seize: not enough collateral");
            Response::set_state(borsh::to_vec(&host).expect("encodes"))
                .send_sync(host.engine, &EngineMsg::Seized { amount })
        }
    }
}
