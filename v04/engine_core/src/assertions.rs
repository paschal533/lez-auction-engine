//! The config's leaf assertions of `SetEngineParams` (design note §4.2 row 1; v0.2.4
//! `set_engine_params`), as pure checks over the delivery so the sender branches are unit-tested:
//!
//! - `AssertCapAtMost` at a collateral actor: the type cap must not exceed the new global cap.
//! - `AssertSystemIdle` at an accounting actor: no surplus or debt auction may be live.
//!
//! Each accepts only its own config: the sender must be an actor of the receiver's program whose
//! account re-derives the receiver (ADR-057). Neither writes nor sends anything.

use auction_engine_params::EngineError;
use lee_core::{
    account::{AccountId, Actor},
    program::ReceiveInput,
};

use crate::{
    actors::{accounting_actor, collateral_actor},
    state::{AccountingState, CollateralState},
};

/// The sending config's account: the sender must be an actor of the receiver's program, else
/// `NotAdmin`.
fn engine_config_sender(input: &ReceiveInput) -> Result<AccountId, EngineError> {
    let engine = input.receiver.program_account_id;
    match input.from {
        Some(from) if from == Actor::new(from.account_id, engine) => Ok(from.account_id),
        _ => Err(EngineError::NotAdmin),
    }
}

/// `AssertCapAtMost { global_cap }` at a collateral actor, from exactly its config.
pub fn cap_at_most(input: &ReceiveInput, global_cap: u128) -> Result<(), EngineError> {
    let config = engine_config_sender(input)?;
    let coll = CollateralState::decode(input.pre_state.as_ref())
        .ok_or(EngineError::WrongAccount("coll_config"))?;
    let engine = input.receiver.program_account_id;
    if coll.config != config
        || input.receiver != collateral_actor(engine, config, coll.collateral_definition_id)
    {
        return Err(EngineError::NotAdmin);
    }
    if coll.type_cap > global_cap {
        return Err(EngineError::TypeCapAboveGlobal);
    }
    Ok(())
}

/// `AssertSystemIdle` at an accounting actor, from exactly its config.
pub fn system_idle(input: &ReceiveInput) -> Result<(), EngineError> {
    let config = engine_config_sender(input)?;
    if input.receiver != accounting_actor(input.receiver.program_account_id, config) {
        return Err(EngineError::NotAdmin);
    }
    let accounting = AccountingState::decode(input.pre_state.as_ref())
        .ok_or(EngineError::WrongAccount("accounting"))?;
    if accounting.surplus_live || accounting.debt_live {
        return Err(EngineError::ProtocolTokenLocked);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        actors::config_actor,
        state::fixtures::{coll, id},
    };

    const ENGINE: u8 = 9;

    fn engine() -> AccountId {
        id(ENGINE)
    }

    /// The admin, its config `K`, and another admin's config `K2`.
    fn actors() -> (Actor, Actor, Actor) {
        let admin = Actor::new(id(1), engine());
        (
            admin,
            config_actor(engine(), id(1), 1),
            config_actor(engine(), id(2), 1),
        )
    }

    fn delivery(receiver: Actor, from: Option<Actor>, state: Vec<u8>) -> ReceiveInput {
        ReceiveInput {
            receiver,
            from,
            is_authorized: false,
            pre_state: state.into(),
            message: Vec::new(),
        }
    }

    /// `K`'s collateral actor for definition 4, cap 5.
    fn collateral_at(k: Actor) -> (Actor, Vec<u8>) {
        let state = CollateralState {
            config: k.account_id,
            ..coll()
        };
        let at = collateral_actor(engine(), k.account_id, state.collateral_definition_id);
        (at, state.encode())
    }

    #[test]
    fn cap_assertion_accepts_only_its_config() {
        let (admin, k, k2) = actors();
        let (c, state) = collateral_at(k);
        let impostor = Actor::new(k.account_id, id(77));
        for from in [None, Some(admin), Some(k2), Some(impostor)] {
            assert_eq!(
                cap_at_most(&delivery(c, from, state.clone()), 5),
                Err(EngineError::NotAdmin),
                "{from:?}"
            );
        }
        assert_eq!(cap_at_most(&delivery(c, Some(k), state.clone()), 5), Ok(()));
        assert_eq!(
            cap_at_most(&delivery(c, Some(k), state), 4),
            Err(EngineError::TypeCapAboveGlobal)
        );
        assert_eq!(
            cap_at_most(&delivery(c, Some(k), Vec::new()), 5),
            Err(EngineError::WrongAccount("coll_config"))
        );
    }

    #[test]
    fn idle_assertion_accepts_only_its_config() {
        let (admin, k, k2) = actors();
        let acc = accounting_actor(engine(), k.account_id);
        let idle = AccountingState::new().encode();
        let impostor = Actor::new(k.account_id, id(77));
        for from in [None, Some(admin), Some(k2), Some(impostor)] {
            assert_eq!(
                system_idle(&delivery(acc, from, idle.clone())),
                Err(EngineError::NotAdmin),
                "{from:?}"
            );
        }
        assert_eq!(system_idle(&delivery(acc, Some(k), idle)), Ok(()));
        for (surplus_live, debt_live) in [(true, false), (false, true)] {
            let live = AccountingState {
                surplus_live,
                debt_live,
                ..AccountingState::new()
            };
            assert_eq!(
                system_idle(&delivery(acc, Some(k), live.encode())),
                Err(EngineError::ProtocolTokenLocked)
            );
        }
        assert_eq!(
            system_idle(&delivery(acc, Some(k), Vec::new())),
            Err(EngineError::WrongAccount("accounting"))
        );
    }
}
